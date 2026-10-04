//! Physical SNAM -> winning FACT source requests. No live faction state.
use crate::{
    World,
    foreign::Content,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::{
    actors::{self, associations, factions},
    identity::FormKey,
    inventory,
    store::SourceReceipt,
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_occurrences: usize,
    /// Count each repeated FACT's physical fields again.
    pub max_faction_fields: usize,
    pub max_relations: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_occurrences: 4096,
            max_faction_fields: 131_072,
            max_relations: 16_384,
            max_visits: 2_000_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor faction request source cohort or campaign changed")]
    ContextChanged,
    #[error("actor faction source is unavailable or internally inconsistent: {0}")]
    Source(&'static str),
    #[error("actor faction request {0} budget exceeded")]
    Capacity(&'static str),
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
    faction: Option<&'a factions::Definition>,
    relation_indices: Vec<usize>,
}

/// Joins can only be constructed from source-coherent existing catalogues.
/// Authored memberships are requests, including repeated declarations.
pub struct Requests<'a> {
    campaign: CampaignId,
    cohort: String,
    actor: &'a actors::Definition<'a>,
    links: &'a associations::Definition<'a>,
    configuration: Option<(usize, &'a inventory::Field)>,
    authored_configuration: Option<(usize, &'a actors::fields::Field)>,
    issues: Vec<Issue>,
    occurrences: Vec<Occurrence<'a>>,
    faction_fields: usize,
    relations: usize,
    visits: usize,
}

#[derive(Serialize)]
pub struct RelationRequest<'a> {
    pub faction_field_index: usize,
    /// XNAM's original binding, signed modifier and raw reaction are unchanged.
    pub field: &'a factions::Field,
    pub evaluated_modifier: Option<i32>,
    pub evaluated_reaction: Option<u32>,
}
#[derive(Serialize)]
pub struct FactionSource<'a> {
    /// All physical fields retain rank declarations, legacy optional flags,
    /// opaque field hashes and findings. No effective flag defaults are added.
    pub definition: &'a factions::Definition,
    pub relations: Vec<RelationRequest<'a>>,
}
#[derive(Serialize)]
pub struct FactionObservation<'a> {
    pub association: &'a associations::Association,
    pub field: &'a actors::fields::Field,
    pub faction: Option<FactionSource<'a>>,
    pub live_membership: Option<bool>,
    pub effective_rank: Option<i8>,
}
#[derive(Serialize)]
pub struct Observation<'a> {
    pub campaign: CampaignId,
    pub source_cohort_sha256: &'a str,
    pub state_revision: u64,
    pub actor_key: &'a FormKey,
    pub actor_source: &'a inventory::Source,
    pub configuration: Option<(usize, &'a inventory::Field)>,
    /// Unique physical actor ACBS, including authored disposition base.
    pub authored_configuration: Option<(usize, &'a actors::fields::Field)>,
    pub issues: &'a [Issue],
    pub association_findings: &'a [actors::fields::Finding],
    pub explicit_subject: Option<ReferenceId>,
    /// Existence and origin only; this does not identify the actor's base form.
    pub explicit_subject_origin: Option<&'a FormKey>,
    pub factions: Vec<FactionObservation<'a>>,
    pub faction_fields: usize,
    pub relationship_requests: usize,
    pub preparation_visits: usize,
    pub template_selection_supported: bool,
    pub membership_initialization_supported: bool,
    pub relationship_evaluation_supported: bool,
    pub condition_truth_supported: bool,
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
                "actor faction projection byte budget",
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
        factions: &'a factions::Catalogue,
        actor_key: &FormKey,
        limits: Limits,
    ) -> Result<Self, Error> {
        for (sources, digest) in [
            (actors.sources(), actors.winning_content_sha256()),
            (
                associations.sources(),
                associations.winning_content_sha256(),
            ),
            (factions.sources(), factions.winning_content_sha256()),
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
            .ok_or(Error::Source("chosen actor winner missing or deleted"))?;
        let links = associations
            .get(actor_key)
            .ok_or(Error::Source("chosen actor associations missing"))?;
        let fields = &actor.inventory_definition().fields;
        let visits = fields
            .len()
            .checked_add(links.associations.len())
            .and_then(|visits| visits.checked_add(actor.fields.len()))
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;
        let mut configuration = None;
        let mut configuration_count = 0;
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
            links,
            configuration: None,
            authored_configuration: None,
            issues: Vec::new(),
            occurrences: Vec::new(),
            faction_fields: 0,
            relations: 0,
            visits,
        };
        match configuration_count {
            1 => {
                let (index, field) = configuration.expect("one configuration");
                result.configuration = configuration;
                result.authored_configuration = Some(
                    actor
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, scalar)| {
                            scalar.kind == *b"ACBS"
                                && scalar.decoded_offset == field.decoded_offset
                                && scalar.sha256 == field.sha256
                                && scalar.bytes == field.bytes
                        })
                        .ok_or(Error::Source("ACBS scalar field origin differs"))?,
                );
                if let inventory::Value::ActorBase { template_flags, .. } = field.value
                    && template_flags & 0x04 != 0
                {
                    result.issues.push(Issue {
                        code: "faction_template_selection_unsupported",
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
            if association.role != associations::Role::Faction {
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
                .filter(|field| field.kind == *b"SNAM")
                .ok_or(Error::Source("SNAM physical field join differs"))?;
            let mut occurrence = Occurrence {
                association,
                field,
                faction: None,
                relation_indices: Vec::new(),
            };
            if association.binding.status == inventory::Status::Defined
                && association.schema_kind_allowed == Some(true)
            {
                let key = association
                    .binding
                    .key
                    .as_ref()
                    .ok_or(Error::Source("defined FACT link has no key"))?;
                let target = association
                    .binding
                    .target
                    .as_ref()
                    .ok_or(Error::Source("defined FACT link has no target"))?;
                let faction = factions
                    .get(key)
                    .filter(|faction| !faction.deleted)
                    .ok_or(Error::Source("defined FACT winner unavailable"))?;
                if target.kind != faction.header.kind
                    || target.source_plugin != faction.source.plugin
                    || target.record_file_offset != faction.source.record_file_offset
                    || target.record_flags != faction.source.record_flags
                {
                    return Err(Error::Source("FACT target provenance differs"));
                }
                result.faction_fields = result
                    .faction_fields
                    .checked_add(faction.fields.len())
                    .ok_or(Error::Capacity("faction field"))?;
                result.visits = result
                    .visits
                    .checked_add(faction.fields.len())
                    .ok_or(Error::Capacity("visit"))?;
                admit(
                    result.faction_fields,
                    limits.max_faction_fields,
                    "faction field",
                )?;
                admit(result.visits, limits.max_visits, "visit")?;
                for (index, field) in faction.fields.iter().enumerate() {
                    if matches!(field.value, factions::Value::Relation { .. }) {
                        result.relations = result
                            .relations
                            .checked_add(1)
                            .ok_or(Error::Capacity("relation"))?;
                        admit(result.relations, limits.max_relations, "relation")?;
                        occurrence.relation_indices.push(index);
                    }
                }
                occurrence.faction = Some(faction);
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
        admit(
            self.faction_fields,
            limits.max_faction_fields,
            "faction field",
        )?;
        admit(self.relations, limits.max_relations, "relation")?;
        admit(self.visits, limits.max_visits, "visit")?;
        let factions = self
            .occurrences
            .iter()
            .map(|occurrence| FactionObservation {
                association: occurrence.association,
                field: occurrence.field,
                faction: occurrence.faction.map(|definition| FactionSource {
                    definition,
                    relations: occurrence
                        .relation_indices
                        .iter()
                        .map(|&index| RelationRequest {
                            faction_field_index: index,
                            field: &definition.fields[index],
                            evaluated_modifier: None,
                            evaluated_reaction: None,
                        })
                        .collect(),
                }),
                live_membership: None,
                effective_rank: None,
            })
            .collect();
        let result = Observation {
            campaign: self.campaign,
            source_cohort_sha256: &self.cohort,
            state_revision: world.revision(),
            actor_key: self.actor.key,
            actor_source: self.actor.source,
            configuration: self.configuration,
            authored_configuration: self.authored_configuration,
            issues: &self.issues,
            association_findings: &self.links.findings,
            explicit_subject,
            explicit_subject_origin: origin,
            factions,
            faction_fields: self.faction_fields,
            relationship_requests: self.relations,
            preparation_visits: self.visits,
            template_selection_supported: false,
            membership_initialization_supported: false,
            relationship_evaluation_supported: false,
            condition_truth_supported: false,
            original_behavior_verified: false,
            scope: "Physical authored SNAM order and winning FACT/XNAM requests; explicit caller context, no actor-base association inference, effective membership/rank, template inheritance, reaction aggregation, condition truth or gameplay",
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
}
