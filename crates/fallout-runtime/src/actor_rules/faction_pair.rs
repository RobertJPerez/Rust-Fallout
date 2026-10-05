//! Fresh current actor endpoints joined to directed physical faction inputs.
use super::{context, factions as requests};
use crate::{World, foreign::Content, identity::ReferenceId};
use fallout_data::{
    actors::{self, associations, factions, placements},
    identity::FormKey,
    inventory,
};
use serde::{Serialize, ser::SerializeSeq};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub context: context::Limits,
    pub faction: requests::Limits,
    pub max_occurrences: usize,
    pub max_faction_fields: usize,
    pub max_relation_pairs: usize,
    pub max_unavailable_relations: usize,
    pub max_visits: usize,
    pub max_current_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            context: Default::default(),
            faction: Default::default(),
            max_occurrences: 8192,
            max_faction_fields: 262144,
            max_relation_pairs: 65536,
            max_unavailable_relations: 32768,
            max_visits: 4_000_000,
            max_current_bytes: 16 * 1024 * 1024,
            max_projection_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor faction pair {0} budget exceeded")]
    Capacity(&'static str),
    #[error("actor faction pair source projection differs from admitted requests")]
    Source,
    #[error(transparent)]
    Context(#[from] context::Error),
    #[error(transparent)]
    Factions(#[from] requests::Error),
}
fn admit(n: usize, max: usize, label: &'static str) -> Result<(), Error> {
    if n > max {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
fn add(n: usize, m: usize, label: &'static str) -> Result<usize, Error> {
    n.checked_add(m).ok_or(Error::Capacity(label))
}
#[derive(Debug, Serialize)]
pub struct Unavailable {
    pub code: &'static str,
    pub inventory_field_index: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct Membership<'a> {
    pub occurrence_index: usize,
    pub association: &'a associations::Association,
    pub field: &'a actors::fields::Field,
    pub faction: Option<&'a factions::Definition>,
    pub unavailable: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Endpoint<'a> {
    pub context: context::Observation<'a>,
    pub memberships: Vec<Membership<'a>>,
    pub issues: Vec<Unavailable>,
    pub association_findings: &'a [actors::fields::Finding],
}
#[derive(Debug, Serialize)]
pub struct DirectedRelation<'a> {
    pub source_occurrence_index: usize,
    pub source_field_index: usize,
    pub source_faction: &'a FormKey,
    pub relationship_field_index: usize,
    pub field: &'a factions::Field,
    pub target_occurrence_index: usize,
    pub target_field_index: usize,
    pub target_faction: &'a FormKey,
    pub modifier: i32,
    pub group_combat_reaction: u32,
}
#[derive(Debug, Serialize)]
pub struct UnavailableRelation<'a> {
    pub source_occurrence_index: usize,
    pub relationship_field_index: usize,
    pub field: &'a factions::Field,
    pub code: &'static str,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Usage {
    pub occurrences: usize,
    pub faction_fields: usize,
    pub source_relationships: usize,
    pub target_index_entries: usize,
    pub relation_pairs: usize,
    pub unavailable_relations: usize,
    pub current_view_bytes: usize,
    pub visits: usize,
}
/// Serialize-only read-only source inputs. No retained Requests or handle is
/// accepted by this consumer and no reaction/membership authority is produced.
#[derive(Debug, Serialize)]
pub struct DirectedPairInputs<'a> {
    pub from: Endpoint<'a>,
    pub to: Endpoint<'a>,
    pub relations: Vec<DirectedRelation<'a>>,
    pub unavailable_relations: Vec<UnavailableRelation<'a>>,
    pub usage: Usage,
    pub effective_membership_supported: bool,
    pub reaction_evaluation_supported: bool,
    pub scope: &'static str,
}
const SCOPE: &str = "Fresh explicit canonical actor endpoints and directed physical SNAM/FACT XNAM matching occurrences; repeated source ranks, signed modifiers and raw reaction words preserved, no effective membership, symmetry, same-faction defaults, reaction aggregation, disposition, reputation or gameplay truth";

fn each_relation<'a, E>(
    from: &Endpoint<'a>,
    to: &Endpoint<'a>,
    targets: &BTreeMap<&FormKey, Vec<usize>>,
    mut matched: impl FnMut(DirectedRelation<'a>) -> Result<(), E>,
    mut unavailable: impl FnMut(UnavailableRelation<'a>) -> Result<(), E>,
) -> Result<(), E> {
    for m in &from.memberships {
        if let Some(faction) = m.faction {
            for (relationship_field_index, field) in faction.fields.iter().enumerate() {
                if let factions::Value::Relation {
                    faction: binding,
                    modifier,
                    group_combat_reaction,
                    schema_kind_allowed,
                } = &field.value
                {
                    if binding.status != inventory::Status::Defined
                        || *schema_kind_allowed != Some(true)
                    {
                        unavailable(UnavailableRelation {
                            source_occurrence_index: m.occurrence_index,
                            relationship_field_index,
                            field,
                            code: "authored_relation_target_unavailable",
                        })?;
                    } else if let Some(bucket) =
                        binding.key.as_ref().and_then(|key| targets.get(key))
                    {
                        for &target_index in bucket {
                            let target = &to.memberships[target_index];
                            let target_faction = target.faction.expect("admitted target index");
                            matched(DirectedRelation {
                                source_occurrence_index: m.occurrence_index,
                                source_field_index: m.association.field_index,
                                source_faction: &faction.key,
                                relationship_field_index,
                                field,
                                target_occurrence_index: target.occurrence_index,
                                target_field_index: target.association.field_index,
                                target_faction: &target_faction.key,
                                modifier: *modifier,
                                group_combat_reaction: *group_combat_reaction,
                            })?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
struct RelationRows<'p, 'a> {
    from: &'p Endpoint<'a>,
    to: &'p Endpoint<'a>,
    targets: &'p BTreeMap<&'a FormKey, Vec<usize>>,
    count: usize,
}
impl Serialize for RelationRows<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.count))?;
        each_relation(
            self.from,
            self.to,
            self.targets,
            |row| seq.serialize_element(&row),
            |_| Ok(()),
        )?;
        seq.end()
    }
}
struct UnavailableRows<'p, 'a> {
    from: &'p Endpoint<'a>,
    to: &'p Endpoint<'a>,
    targets: &'p BTreeMap<&'a FormKey, Vec<usize>>,
    count: usize,
}
impl Serialize for UnavailableRows<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.count))?;
        each_relation(
            self.from,
            self.to,
            self.targets,
            |_| Ok(()),
            |row| seq.serialize_element(&row),
        )?;
        seq.end()
    }
}

#[derive(Clone, Copy)]
struct Sources<'a> {
    placements: &'a placements::Catalogue,
    actors: &'a actors::Catalogue<'a>,
    associations: &'a associations::Catalogue<'a>,
    factions: &'a factions::Catalogue,
}
fn endpoint<'a>(
    world: &World<'_>,
    content: &Content,
    sources: Sources<'a>,
    id: ReferenceId,
    limits: Limits,
) -> Result<(Endpoint<'a>, usize, usize, usize), Error> {
    let Sources {
        placements,
        actors,
        associations,
        factions,
    } = sources;
    let joined = context::observe(world, content, placements, actors, id, limits.context)?;
    // Derive the actor from this exact placement; never accept a caller's
    // retained ACT07 Requests or an unrelated explicit-subject origin.
    let prepared = requests::Requests::prepare(
        world,
        actors,
        associations,
        factions,
        joined.actor.key,
        limits.faction,
    )?;
    let observed = prepared.observe(world, content, Some(id), limits.faction)?;
    let links = associations.get(joined.actor.key).ok_or(Error::Source)?;
    let mut memberships = Vec::with_capacity(observed.factions.len());
    // ACT07 owns source validation. Reborrow original catalogue fields by its
    // admitted physical indices instead of copying or retaining its projection.
    for (occurrence_index, association) in links
        .associations
        .iter()
        .filter(|a| a.role == associations::Role::Faction)
        .enumerate()
    {
        let prior = observed
            .factions
            .get(occurrence_index)
            .ok_or(Error::Source)?;
        if prior.association.field_index != association.field_index {
            return Err(Error::Source);
        }
        let faction = prior.faction.as_ref().map(|f| &f.definition.key);
        let faction = faction
            .map(|key| factions.get(key).ok_or(Error::Source))
            .transpose()?;
        memberships.push(Membership {
            occurrence_index,
            association,
            field: joined
                .actor
                .fields
                .get(association.field_index)
                .ok_or(Error::Source)?,
            faction,
            unavailable: if faction.is_some() {
                "current_membership_unsupported"
            } else {
                "authored_faction_unavailable"
            },
        });
    }
    if memberships.len() != observed.factions.len() {
        return Err(Error::Source);
    }
    let mut issues = observed
        .issues
        .iter()
        .map(|i| Unavailable {
            code: i.code,
            inventory_field_index: i.inventory_field_index,
        })
        .collect::<Vec<_>>();
    issues.push(Unavailable {
        code: "current_membership_unsupported",
        inventory_field_index: None,
    });
    let fields = observed.faction_fields;
    let relations = observed.relationship_requests;
    let visits = observed.preparation_visits;
    Ok((
        Endpoint {
            context: joined,
            memberships,
            issues,
            association_findings: &links.findings,
        },
        fields,
        relations,
        visits,
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "The assigned actor-pair API explicitly names its four existing catalogue inputs and two selected endpoints"
)]
pub fn observe_pair<'a>(
    world: &World<'_>,
    content: &Content,
    placements: &'a placements::Catalogue,
    actors: &'a actors::Catalogue<'a>,
    associations: &'a associations::Catalogue<'a>,
    factions: &'a factions::Catalogue,
    from_reference: ReferenceId,
    to_reference: ReferenceId,
    limits: Limits,
) -> Result<DirectedPairInputs<'a>, Error> {
    let admission = context::admit_sources(world, content, placements, actors, limits.context)?;
    let mut current_view_bytes = 0;
    for id in [from_reference, to_reference] {
        let borrowed = admission.borrowed_reference(id)?;
        let bytes = context::projection_bytes(&borrowed, limits.max_current_bytes)
            .map_err(|_| Error::Capacity("current byte"))?;
        current_view_bytes = add(current_view_bytes, bytes, "current byte")?;
        admit(current_view_bytes, limits.max_current_bytes, "current byte")?;
    }
    // Precharge both endpoint memberships/FACT extents before Requests can
    // allocate occurrence/relation vectors. These are borrowed count checks;
    // ACT07 still owns all source validation and the resulting declarations.
    for source_count in [associations.sources().len(), factions.sources().len()] {
        admit(source_count, limits.context.max_sources, "source")?;
    }
    let mut total_occurrences = 0;
    let mut total_fields = 0;
    let mut preflight_visits = admission.source_visits;
    for id in [from_reference, to_reference] {
        let origin = world
            .reference_origin(id)
            .map_err(context::Error::from)?
            .ok_or(context::Error::Unavailable(
                "reference_has_no_authored_origin",
            ))?;
        let joined = admission.join(origin, limits.context)?;
        // joined.visits contains the already shared source receipt charge.
        preflight_visits = add(
            preflight_visits,
            joined.visits - admission.source_visits,
            "visit",
        )?;
        let links = associations.get(joined.actor.key).ok_or(Error::Source)?;
        admit(links.associations.len(), limits.max_visits, "visit")?;
        preflight_visits = add(preflight_visits, links.associations.len(), "visit")?;
        admit(preflight_visits, limits.max_visits, "visit")?;
        for association in &links.associations {
            if association.role == associations::Role::Faction {
                total_occurrences = add(total_occurrences, 1, "occurrence")?;
                admit(total_occurrences, limits.max_occurrences, "occurrence")?;
                if association.binding.status == inventory::Status::Defined
                    && association.schema_kind_allowed == Some(true)
                    && let Some(faction) = association
                        .binding
                        .key
                        .as_ref()
                        .and_then(|k| factions.get(k))
                {
                    total_fields = add(total_fields, faction.fields.len(), "faction field")?;
                    admit(total_fields, limits.max_faction_fields, "faction field")?;
                }
            }
        }
    }
    let sources = Sources {
        placements,
        actors,
        associations,
        factions,
    };
    let (from, from_fields, from_relations, from_visits) =
        endpoint(world, content, sources, from_reference, limits)?;
    let (to, to_fields, _, to_visits) = endpoint(world, content, sources, to_reference, limits)?;
    let occurrences = add(from.memberships.len(), to.memberships.len(), "occurrence")?;
    admit(occurrences, limits.max_occurrences, "occurrence")?;
    let faction_fields = add(from_fields, to_fields, "faction field")?;
    admit(faction_fields, limits.max_faction_fields, "faction field")?;
    let visits = add(
        add(
            add(
                add(from_visits, to_visits, "visit")?,
                from.context.visits,
                "visit",
            )?,
            to.context.visits,
            "visit",
        )?,
        preflight_visits,
        "visit",
    )?;
    let mut usage = Usage {
        occurrences,
        faction_fields,
        source_relationships: from_relations,
        target_index_entries: 0,
        relation_pairs: 0,
        unavailable_relations: 0,
        current_view_bytes,
        visits,
    };
    admit(usage.visits, limits.max_visits, "visit")?;
    let mut targets: BTreeMap<&FormKey, Vec<usize>> = BTreeMap::new();
    for m in &to.memberships {
        usage.visits = add(usage.visits, 1, "visit")?;
        admit(usage.visits, limits.max_visits, "visit")?;
        if let Some(faction) = m.faction {
            targets
                .entry(&faction.key)
                .or_default()
                .push(m.occurrence_index);
            usage.target_index_entries = add(usage.target_index_entries, 1, "occurrence")?;
        }
    }
    // First count complete multiplicity and unavailable inputs. No Cartesian
    // output allocation occurs until every bucket has passed aggregate caps.
    for m in &from.memberships {
        if let Some(faction) = m.faction {
            for field in &faction.fields {
                usage.visits = add(usage.visits, 1, "visit")?;
                admit(usage.visits, limits.max_visits, "visit")?;
                if let factions::Value::Relation {
                    faction: binding,
                    schema_kind_allowed,
                    ..
                } = &field.value
                {
                    if binding.status != inventory::Status::Defined
                        || *schema_kind_allowed != Some(true)
                    {
                        usage.unavailable_relations =
                            add(usage.unavailable_relations, 1, "unavailable relation")?;
                        admit(
                            usage.unavailable_relations,
                            limits.max_unavailable_relations,
                            "unavailable relation",
                        )?;
                    } else if let Some(bucket) =
                        binding.key.as_ref().and_then(|key| targets.get(key))
                    {
                        usage.relation_pairs =
                            add(usage.relation_pairs, bucket.len(), "relation pair")?;
                        admit(
                            usage.relation_pairs,
                            limits.max_relation_pairs,
                            "relation pair",
                        )?;
                        usage.visits = add(usage.visits, bucket.len(), "visit")?;
                        admit(usage.visits, limits.max_visits, "visit")?;
                    }
                }
            }
        }
    }
    let traversal = add(
        add(from_fields, usage.relation_pairs, "visit")?,
        usage.unavailable_relations,
        "visit",
    )?;
    usage.visits = add(
        usage.visits,
        traversal.checked_mul(3).ok_or(Error::Capacity("visit"))?,
        "visit",
    )?;
    admit(usage.visits, limits.max_visits, "visit")?;
    #[derive(Serialize)]
    struct Probe<'p, 'a> {
        from: &'p Endpoint<'a>,
        to: &'p Endpoint<'a>,
        relations: RelationRows<'p, 'a>,
        unavailable_relations: UnavailableRows<'p, 'a>,
        usage: Usage,
        effective_membership_supported: bool,
        reaction_evaluation_supported: bool,
        scope: &'static str,
    }
    // Streaming stack rows establish the exact full projection size before
    // allocating either multiplicity-expanded relation output vector.
    context::projection_bytes(
        &Probe {
            from: &from,
            to: &to,
            relations: RelationRows {
                from: &from,
                to: &to,
                targets: &targets,
                count: usage.relation_pairs,
            },
            unavailable_relations: UnavailableRows {
                from: &from,
                to: &to,
                targets: &targets,
                count: usage.unavailable_relations,
            },
            usage,
            effective_membership_supported: false,
            reaction_evaluation_supported: false,
            scope: SCOPE,
        },
        limits.max_projection_bytes,
    )
    .map_err(|_| Error::Capacity("projection byte"))?;
    let mut relations = Vec::with_capacity(usage.relation_pairs);
    let mut unavailable_relations = Vec::with_capacity(usage.unavailable_relations);
    each_relation(
        &from,
        &to,
        &targets,
        |row| {
            relations.push(row);
            Ok::<_, Error>(())
        },
        |row| {
            unavailable_relations.push(row);
            Ok(())
        },
    )?;
    let result = DirectedPairInputs {
        from,
        to,
        relations,
        unavailable_relations,
        usage,
        effective_membership_supported: false,
        reaction_evaluation_supported: false,
        scope: SCOPE,
    };
    Ok(result)
}
