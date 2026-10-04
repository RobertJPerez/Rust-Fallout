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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Eligibility,
    Selection,
    Scheduling,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedDependency {
    OriginalConditionTruth,
    ConditionGrouping,
    EffectivePackageSelection,
    ScheduleWordInterpretation,
    PackageExecution,
}
#[derive(Debug, Clone, Copy, Serialize, thiserror::Error)]
#[error("actor package {operation:?} execution unsupported: {dependencies:?}")]
pub struct Refusal {
    pub operation: Operation,
    pub dependencies: &'static [UnsupportedDependency],
}
fn refusal(operation: Operation) -> Refusal {
    use UnsupportedDependency::*;
    Refusal {
        operation,
        dependencies: match operation {
            Operation::Eligibility => &[OriginalConditionTruth, ConditionGrouping],
            Operation::Selection => &[
                OriginalConditionTruth,
                ConditionGrouping,
                EffectivePackageSelection,
            ],
            Operation::Scheduling => &[
                OriginalConditionTruth,
                ConditionGrouping,
                EffectivePackageSelection,
                ScheduleWordInterpretation,
                PackageExecution,
            ],
        },
    }
}
#[derive(Debug, Clone, Copy)]
pub struct CapabilityLimits {
    pub requests: Limits,
    pub max_fields: usize,
    pub max_event_fields: usize,
    pub max_scripts: usize,
    pub max_script_entries: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for CapabilityLimits {
    fn default() -> Self {
        Self {
            requests: Limits::default(),
            max_fields: 65_536,
            max_event_fields: 16_384,
            max_scripts: 4096,
            max_script_entries: 65_536,
            max_visits: 2_000_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default, Serialize)]
pub struct CapabilityCounts {
    pub fields: usize,
    pub event_fields: usize,
    pub scripts: usize,
    pub script_entries: usize,
    pub visits: usize,
}
#[derive(Serialize)]
pub struct PackageInputs<'a> {
    pub occurrence_index: usize,
    /// All existing physical scalar fields, including opaque fields and findings.
    pub source: Option<&'a actors::packages::Definition>,
    pub dependency_findings: &'a [actors::fields::Finding],
    pub event_fields: &'a [package_dependencies::EventField],
    pub scripts: &'a [package_dependencies::Embedded<'a>],
    pub physical_source_complete: bool,
}
#[derive(Serialize)]
pub struct Capability<'a> {
    pub observation: Observation<'a>,
    pub actor_fields: &'a [actors::fields::Field],
    pub actor_findings: &'a [actors::fields::Finding],
    pub package_inputs: Vec<PackageInputs<'a>>,
    /// Complete retained physical source is separate from usable semantics.
    /// This may be true for zero CTDA, repeated or unknown physical fields.
    pub physical_source_complete: bool,
    pub counts: CapabilityCounts,
    pub refusal: Refusal,
    pub execution_admitted: bool,
    pub state_changed: bool,
    pub scope: &'static str,
}
impl Capability<'_> {
    /// No executor exists behind this capability. Empty or source-complete
    /// declarations never grant an operation or silently complete a no-op AI.
    pub fn require_execution(&self) -> Result<(), Refusal> {
        Err(self.refusal)
    }
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
    pub fn capability<'b>(
        &'b self,
        world: &'b World<'_>,
        content: &Content,
        explicit_subject: Option<ReferenceId>,
        operation: Operation,
        limits: CapabilityLimits,
    ) -> Result<Capability<'b>, Error> {
        // The shared adapter supplies exact faithful refusals, including the
        // source site and descriptor/subject/operand inputs. No query truth is
        // copied from an engineering observation or privately interpreted.
        let observation = self.observe(
            world,
            content,
            explicit_subject,
            condition::Intent::Faithful,
            limits.requests,
        )?;
        let mut counts = CapabilityCounts {
            fields: self.actor.fields.len(),
            visits: self.visits,
            ..Default::default()
        };
        admit(counts.fields, limits.max_fields, "capability field")?;
        counts.visits = counts
            .visits
            .checked_add(self.actor.fields.len())
            .and_then(|n| n.checked_add(self.actor.findings.len()))
            .and_then(|n| n.checked_add(self.issues.len()))
            .ok_or(Error::Capacity("capability visit"))?;
        admit(counts.visits, limits.max_visits, "capability visit")?;
        let mut inputs = Vec::new();
        for (index, occurrence) in self.occurrences.iter().enumerate() {
            let mut input = PackageInputs {
                occurrence_index: index,
                source: None,
                dependency_findings: &[],
                event_fields: &[],
                scripts: &[],
                physical_source_complete: false,
            };
            if let Some(package) = occurrence.package {
                input.source = Some(package.source_definition());
                input.dependency_findings = &package.findings;
                input.event_fields = &package.event_fields;
                input.scripts = &package.scripts;
                input.physical_source_complete = package.record().is_some()
                    && package.conditions.is_some()
                    && package.source.decoded_record_sha256.is_some();
                counts.fields = counts
                    .fields
                    .checked_add(package.source_definition().fields.len())
                    .ok_or(Error::Capacity("capability field"))?;
                counts.event_fields = counts
                    .event_fields
                    .checked_add(package.event_fields.len())
                    .ok_or(Error::Capacity("capability event field"))?;
                counts.scripts = counts
                    .scripts
                    .checked_add(package.scripts.len())
                    .ok_or(Error::Capacity("capability script"))?;
                admit(counts.fields, limits.max_fields, "capability field")?;
                admit(
                    counts.event_fields,
                    limits.max_event_fields,
                    "capability event field",
                )?;
                admit(counts.scripts, limits.max_scripts, "capability script")?;
                let mut entries = 0usize;
                for script in &package.scripts {
                    entries = entries
                        .checked_add(script.declarations.len())
                        .and_then(|n| n.checked_add(script.references.len()))
                        .and_then(|n| n.checked_add(script.issues.len()))
                        .ok_or(Error::Capacity("capability script entry"))?;
                }
                counts.script_entries = counts
                    .script_entries
                    .checked_add(entries)
                    .ok_or(Error::Capacity("capability script entry"))?;
                admit(
                    counts.script_entries,
                    limits.max_script_entries,
                    "capability script entry",
                )?;
                counts.visits = counts
                    .visits
                    .checked_add(package.source_definition().fields.len())
                    .and_then(|n| n.checked_add(package.source_definition().findings.len()))
                    .and_then(|n| n.checked_add(package.findings.len()))
                    .and_then(|n| n.checked_add(package.event_fields.len()))
                    .and_then(|n| n.checked_add(package.scripts.len()))
                    .and_then(|n| n.checked_add(entries))
                    .ok_or(Error::Capacity("capability visit"))?;
                admit(counts.visits, limits.max_visits, "capability visit")?;
            }
            inputs.push(input);
        }
        let result = Capability {
            observation,
            actor_fields: &self.actor.fields,
            actor_findings: &self.actor.findings,
            physical_source_complete: inputs.iter().all(|input| input.physical_source_complete),
            package_inputs: inputs,
            counts,
            refusal: refusal(operation),
            execution_admitted: false,
            state_changed: false,
            scope: "Complete retained authored PACK fields and existing faithful condition/event/embedded-unit inputs; physical source completeness never admits eligibility, effective selection, schedule interpretation or AI execution",
        };
        serde_json::to_writer(
            &mut ProjectionBudget {
                bytes: 0,
                maximum: limits.max_projection_bytes,
            },
            &result,
        )?;
        Ok(result)
    }
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
