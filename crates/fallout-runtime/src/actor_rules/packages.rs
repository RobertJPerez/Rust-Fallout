//! Physical PKID occurrences and PACK CTDA requests, without eligibility rules.
use crate::{
    World,
    execution::condition,
    foreign::Content,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::{
    actors::{self, associations, package_dependencies},
    condition_operands::RecordIdentity,
    identity::FormKey,
    inventory, plugin,
    store::SourceReceipt,
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_occurrences: usize,
    /// Repeated references to one PACK each consume their physical site count.
    pub max_conditions: usize,
    pub max_visits: usize,
    /// Aggregate GetItemCount contribution admission across the whole report.
    pub max_query_contributions: usize,
    /// Compact serialized observation, not a process heap bound.
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_occurrences: 4096,
            max_conditions: 16_384,
            max_visits: 2_000_000,
            max_query_contributions: 100_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor package request source cohort or campaign changed")]
    ContextChanged,
    #[error("actor package source is unavailable or internally inconsistent: {0}")]
    Source(String),
    #[error("actor package request {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Condition(#[from] condition::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Projection(#[from] serde_json::Error),
}

#[derive(Debug, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub inventory_field_index: Option<usize>,
}

struct Occurrence<'a> {
    association: &'a associations::Association,
    field: &'a actors::fields::Field,
    package: Option<&'a package_dependencies::Definition<'a>>,
    conditions: Vec<condition::Request<'a>>,
}

/// Privately constructed joins over the existing source catalogues. Contains no
/// mutable actor state and does not deduplicate repeated package declarations.
pub struct Requests<'a> {
    campaign: CampaignId,
    cohort: String,
    actor: &'a actors::Definition<'a>,
    configuration: Option<(usize, &'a inventory::Field)>,
    issues: Vec<Issue>,
    occurrences: Vec<Occurrence<'a>>,
    conditions: usize,
    visits: usize,
}

#[derive(Debug, Serialize)]
pub struct PackageSource<'a> {
    pub key: &'a FormKey,
    pub source: &'a inventory::Source,
    pub header: &'a plugin::RecordHeader,
    pub condition_record: Option<&'a RecordIdentity>,
}

#[derive(Serialize)]
pub struct PackageObservation<'a> {
    /// Physical PKID association, including raw FormID and unavailable target.
    pub association: &'a associations::Association,
    pub field: &'a actors::fields::Field,
    pub package: Option<PackageSource<'a>>,
    pub conditions: Vec<condition::Observation<'a>>,
    pub eligible: Option<bool>,
}

#[derive(Serialize)]
pub struct Observation<'a> {
    pub campaign: CampaignId,
    pub source_cohort_sha256: &'a str,
    pub state_revision: u64,
    pub actor_key: &'a FormKey,
    pub actor_source: &'a inventory::Source,
    pub configuration: Option<(usize, &'a inventory::Field)>,
    pub issues: &'a [Issue],
    /// Caller-provided context; its canonical origin is disclosed. No actor-base
    /// association or placed-reference selection is inferred from this input.
    pub explicit_subject: Option<ReferenceId>,
    pub explicit_subject_origin: Option<&'a FormKey>,
    pub intent: condition::Intent,
    pub packages: Vec<PackageObservation<'a>>,
    pub condition_requests: usize,
    pub preparation_visits: usize,
    pub query_contributions: usize,
    pub effective_package_selection_supported: bool,
    pub scheduling_supported: bool,
    pub original_behavior_verified: bool,
    pub scope: &'static str,
}

fn same_sources(left: &[SourceReceipt], right: &[SourceReceipt]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor package projection byte budget",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> Requests<'a> {
    pub fn prepare(
        world: &World<'_>,
        actors: &'a actors::Catalogue<'a>,
        associations: &'a associations::Catalogue<'a>,
        packages: &'a package_dependencies::Catalogue<'a>,
        actor_key: &FormKey,
        limits: Limits,
    ) -> Result<Self, Error> {
        for (sources, digest) in [
            (actors.sources(), actors.winning_content_sha256()),
            (
                associations.sources(),
                associations.winning_content_sha256(),
            ),
            (packages.sources(), packages.winning_content_sha256()),
        ] {
            if !same_sources(sources, &world.catalogue().sources)
                || digest != world.catalogue().winning_content_sha256()
            {
                return Err(Error::ContextChanged);
            }
        }
        let actor = actors
            .get(actor_key)
            .filter(|actor| !actor.deleted)
            .ok_or_else(|| Error::Source("chosen actor winner missing or deleted".into()))?;
        let links = associations
            .get(actor_key)
            .ok_or_else(|| Error::Source("chosen actor associations missing".into()))?;
        let fields = &actor.inventory_definition().fields;
        let visits = fields
            .len()
            .checked_add(links.associations.len())
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;
        let mut configuration = None;
        let mut configuration_count = 0usize;
        for (index, field) in fields.iter().enumerate() {
            if matches!(field.value, inventory::Value::ActorBase { .. }) {
                configuration_count += 1;
                configuration = Some((index, field));
            }
        }
        let mut result = Self {
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
            actor,
            configuration: None,
            issues: Vec::new(),
            occurrences: Vec::new(),
            conditions: 0,
            visits,
        };
        match configuration_count {
            1 => {
                let (index, field) = configuration.expect("one configuration");
                result.configuration = configuration;
                if let inventory::Value::ActorBase { template_flags, .. } = field.value
                    && template_flags & 0x20 != 0
                {
                    result.issues.push(Issue {
                        code: "ai_package_template_selection_unsupported",
                        inventory_field_index: Some(index),
                    });
                }
            }
            0 => result.issues.push(Issue {
                code: "missing_actor_configuration",
                inventory_field_index: None,
            }),
            _ => result.issues.push(Issue {
                code: "ambiguous_actor_configuration",
                inventory_field_index: None,
            }),
        }
        for association in &links.associations {
            if association.role != associations::Role::Package {
                continue;
            }
            admit(
                result.occurrences.len() + 1,
                limits.max_occurrences,
                "occurrence",
            )?;
            let field = actor
                .fields
                .get(association.field_index)
                .filter(|field| field.kind == *b"PKID")
                .ok_or_else(|| Error::Source("PKID physical field join differs".into()))?;
            let mut occurrence = Occurrence {
                association,
                field,
                package: None,
                conditions: Vec::new(),
            };
            if association.binding.status == inventory::Status::Defined
                && association.schema_kind_allowed == Some(true)
            {
                let key = association
                    .binding
                    .key
                    .as_ref()
                    .ok_or_else(|| Error::Source("defined PACK link has no key".into()))?;
                let target = association
                    .binding
                    .target
                    .as_ref()
                    .ok_or_else(|| Error::Source("defined PACK link has no target".into()))?;
                let package = packages
                    .get(key)
                    .filter(|package| !package.deleted)
                    .ok_or_else(|| Error::Source("defined PACK winner unavailable".into()))?;
                if target.kind != package.header.kind
                    || target.source_plugin != package.source.plugin
                    || target.record_file_offset != package.source.record_file_offset
                    || target.record_flags != package.source.record_flags
                {
                    return Err(Error::Source("PACK target provenance differs".into()));
                }
                let record = package.conditions.as_ref().ok_or_else(|| {
                    Error::Source("nondeleted PACK conditions not prepared".into())
                })?;
                result.conditions = result
                    .conditions
                    .checked_add(record.sites().len())
                    .ok_or(Error::Capacity("condition"))?;
                // The shared adapter searches physical sites by offset. Charge
                // the full bounded search work before preparing any request.
                let sites = record.sites().len();
                let search = sites
                    .checked_mul(sites.checked_add(1).ok_or(Error::Capacity("visit"))?)
                    .ok_or(Error::Capacity("visit"))?
                    / 2;
                result.visits = result
                    .visits
                    .checked_add(sites)
                    .and_then(|visits| visits.checked_add(search))
                    .ok_or(Error::Capacity("visit"))?;
                admit(result.conditions, limits.max_conditions, "condition")?;
                admit(result.visits, limits.max_visits, "visit")?;
                for site in record.sites() {
                    occurrence.conditions.push(condition::Request::prepare(
                        world,
                        record,
                        site.field_decoded_offset(),
                    )?);
                }
                occurrence.package = Some(package);
            }
            result.occurrences.push(occurrence);
        }
        Ok(result)
    }

    pub fn observe<'b>(
        &'b self,
        world: &'b World<'_>,
        content: &Content,
        explicit_subject: Option<ReferenceId>,
        intent: condition::Intent,
        limits: Limits,
    ) -> Result<Observation<'b>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        content.validate_world(world)?;
        let origin = explicit_subject
            .map(|id| world.reference_origin(id))
            .transpose()?
            .flatten();
        admit(self.occurrences.len(), limits.max_occurrences, "occurrence")?;
        admit(self.conditions, limits.max_conditions, "condition")?;
        admit(self.visits, limits.max_visits, "visit")?;
        let mut packages = Vec::new();
        let mut contributions = 0usize;
        for occurrence in &self.occurrences {
            let mut conditions = Vec::new();
            for request in &occurrence.conditions {
                let observation = request.observe(
                    world,
                    content,
                    explicit_subject,
                    intent,
                    limits.max_query_contributions - contributions,
                )?;
                if let condition::Outcome::EngineeringObservation { trace } = &observation.outcome {
                    contributions += trace.query.contributions.len();
                }
                conditions.push(observation);
            }
            packages.push(PackageObservation {
                association: occurrence.association,
                field: occurrence.field,
                package: occurrence.package.map(|package| PackageSource {
                    key: package.key,
                    source: package.source,
                    header: package.header,
                    condition_record: package.conditions.as_ref().map(|record| record.identity()),
                }),
                conditions,
                eligible: None,
            });
        }
        let result = Observation {
            campaign: self.campaign,
            source_cohort_sha256: &self.cohort,
            state_revision: world.revision(),
            actor_key: self.actor.key,
            actor_source: self.actor.source,
            configuration: self.configuration,
            issues: &self.issues,
            explicit_subject,
            explicit_subject_origin: origin,
            intent,
            packages,
            condition_requests: self.conditions,
            preparation_visits: self.visits,
            query_contributions: contributions,
            effective_package_selection_supported: false,
            scheduling_supported: false,
            original_behavior_verified: false,
            scope: "Physical authored PKID order and PACK CTDA requests through the shared canonical condition adapter; explicit caller context, no actor-base association inference, effective package selection, condition truth/grouping, schedule, embedded-script execution or AI",
        };
        let mut budget = ProjectionBudget {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        };
        serde_json::to_writer(&mut budget, &result)?;
        Ok(result)
    }
}
