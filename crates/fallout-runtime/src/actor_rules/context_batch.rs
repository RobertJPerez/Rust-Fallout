//! Ordered current actor contexts with one admitted source cohort and base table.
use super::context::{self, ActorSource};
use crate::{World, foreign::Content, identity::ReferenceId, reference_state};
use fallout_data::actors::{self, placements};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub single: context::Limits,
    pub max_references: usize,
    pub max_unique_bases: usize,
    pub max_total_fields: usize,
    pub max_visits: usize,
    pub max_current_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            single: Default::default(),
            max_references: 4096,
            max_unique_bases: 4096,
            max_total_fields: 2_000_000,
            max_visits: 4_000_000,
            max_current_bytes: 16 * 1024 * 1024,
            max_projection_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor context batch {0} budget exceeded")]
    Capacity(&'static str),
    #[error("actor context batch contains a repeated reference id")]
    DuplicateReference,
    #[error(transparent)]
    Context(#[from] context::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
}
#[derive(Debug, Serialize)]
pub struct ReferenceContext<'a> {
    reference: reference_state::View,
    placement: &'a placements::Definition,
    base_source_index: usize,
    base_field_index: usize,
    transform_field_index: usize,
    fields: usize,
}
impl ReferenceContext<'_> {
    pub fn reference(&self) -> &reference_state::View {
        &self.reference
    }
    pub fn placement(&self) -> &placements::Definition {
        self.placement
    }
    pub fn base_source_index(&self) -> usize {
        self.base_source_index
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub source_admissions: usize,
    pub source_receipt_visits: usize,
    pub selected_references: usize,
    pub unique_bases: usize,
    pub placement_fields: usize,
    pub actor_fields: usize,
    pub total_fields: usize,
    pub current_view_bytes: usize,
    pub visits: usize,
}
/// All joins are admitted before canonical views are cloned or output published.
/// This is immutable observation, with no deserialization or mutation method.
#[derive(Debug, Serialize)]
pub struct ContextBatch<'a> {
    base_sources: Vec<ActorSource<'a>>,
    references: Vec<ReferenceContext<'a>>,
    usage: Usage,
    actor_initialization_supported: bool,
    authored_enable_evaluated: bool,
    scope: &'static str,
}
impl ContextBatch<'_> {
    pub fn base_sources(&self) -> &[ActorSource<'_>] {
        &self.base_sources
    }
    pub fn references(&self) -> &[ReferenceContext<'_>] {
        &self.references
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}
const SCOPE: &str = "Ordered explicit canonical actor references with one admitted whole source cohort and deterministic shared winning actor-base table; each authored placement and optional current state remain separate, no discovery, defaults, registration, retail initialization or mutation authority";
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

pub fn observe_batch<'a>(
    world: &World<'_>,
    content: &Content,
    placements: &'a placements::Catalogue,
    actors: &'a actors::Catalogue<'_>,
    selected: &[ReferenceId],
    limits: Limits,
) -> Result<ContextBatch<'a>, Error> {
    admit(selected.len(), limits.max_references, "reference")?;
    // Both the selected list and its key index are bounded before allocation.
    let mut ids = BTreeSet::new();
    for &id in selected {
        if !ids.insert(id) {
            return Err(Error::DuplicateReference);
        }
    }
    let admitted = context::admit_sources(world, content, placements, actors, limits.single)?;
    let mut usage = Usage {
        source_admissions: 1,
        source_receipt_visits: admitted.source_visits,
        selected_references: selected.len(),
        unique_bases: 0,
        placement_fields: 0,
        actor_fields: 0,
        total_fields: 0,
        current_view_bytes: 0,
        visits: add(admitted.source_visits, selected.len(), "visit")?,
    };
    admit(usage.visits, limits.max_visits, "visit")?;
    // Admit every borrowed current row before source joins can normalize names.
    // A malformed, oversized final origin must refuse before any View clone.
    for &id in selected {
        let borrowed = admitted.borrowed_reference(id)?;
        let bytes = context::projection_bytes(&borrowed, limits.max_current_bytes)
            .map_err(|_| Error::Capacity("current byte"))?;
        usage.current_view_bytes = add(usage.current_view_bytes, bytes, "current byte")?;
        admit(
            usage.current_view_bytes,
            limits.max_current_bytes,
            "current byte",
        )?;
    }
    let mut joined = Vec::with_capacity(selected.len());
    let mut bases = BTreeMap::new();
    // Source fields remain borrowed throughout admission and output.
    for &id in selected {
        let origin = world
            .reference_origin(id)?
            .ok_or(context::Error::Unavailable(
                "reference_has_no_authored_origin",
            ))?;
        let row = admitted.join(origin, limits.single)?;
        usage.placement_fields = add(usage.placement_fields, row.placement.fields.len(), "field")?;
        if !bases.contains_key(row.actor.key) {
            admit(
                bases.len().checked_add(1).ok_or(Error::Capacity("base"))?,
                limits.max_unique_bases,
                "base",
            )?;
            usage.actor_fields = add(usage.actor_fields, row.actor.fields.len(), "field")?;
            bases.insert(row.actor.key, row.actor);
        }
        usage.total_fields = add(usage.placement_fields, usage.actor_fields, "field")?;
        admit(usage.total_fields, limits.max_total_fields, "field")?;
        let joins = selected
            .len()
            .checked_mul(7)
            .ok_or(Error::Capacity("visit"))?;
        usage.visits = add(
            add(
                add(admitted.source_visits, selected.len(), "visit")?,
                usage.total_fields,
                "visit",
            )?,
            joins,
            "visit",
        )?;
        admit(usage.visits, limits.max_visits, "visit")?;
        joined.push(row);
    }
    usage.unique_bases = bases.len();
    let indices: BTreeMap<_, _> = bases
        .keys()
        .enumerate()
        .map(|(index, &key)| (key, index))
        .collect();
    let base_sources: Vec<_> = bases.into_values().collect();
    #[derive(Serialize)]
    struct BorrowedRow<'s, 'a> {
        reference: context::BorrowedReference<'s>,
        placement: &'a placements::Definition,
        base_source_index: usize,
        base_field_index: usize,
        transform_field_index: usize,
        fields: usize,
    }
    let mut probe_rows = Vec::with_capacity(selected.len());
    for (&id, row) in selected.iter().zip(&joined) {
        probe_rows.push(BorrowedRow {
            reference: admitted.borrowed_reference(id)?,
            placement: row.placement,
            base_source_index: indices[row.actor.key],
            base_field_index: row.base_field_index,
            transform_field_index: row.transform_field_index,
            fields: row.fields,
        });
    }
    #[derive(Serialize)]
    struct Probe<'s, 'a, 'p> {
        base_sources: &'p [ActorSource<'a>],
        references: &'p [BorrowedRow<'s, 'a>],
        usage: Usage,
        actor_initialization_supported: bool,
        authored_enable_evaluated: bool,
        scope: &'static str,
    }
    context::projection_bytes(
        &Probe {
            base_sources: &base_sources,
            references: &probe_rows,
            usage,
            actor_initialization_supported: false,
            authored_enable_evaluated: false,
            scope: SCOPE,
        },
        limits.max_projection_bytes,
    )
    .map_err(|_| Error::Capacity("projection byte"))?;
    let mut references = Vec::with_capacity(selected.len());
    for (&id, row) in selected.iter().zip(joined) {
        references.push(ReferenceContext {
            reference: world.reference_view(id)?,
            placement: row.placement,
            base_source_index: indices[row.actor.key],
            base_field_index: row.base_field_index,
            transform_field_index: row.transform_field_index,
            fields: row.fields,
        });
    }
    Ok(ContextBatch {
        base_sources,
        references,
        usage,
        actor_initialization_supported: false,
        authored_enable_evaluated: false,
        scope: SCOPE,
    })
}
