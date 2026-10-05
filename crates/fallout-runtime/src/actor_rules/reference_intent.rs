//! Explicit engineering mutations on an admitted, privately restored snapshot.
//! Source placement validates identity; it supplies no enable, pose or AI defaults.
use super::{context, equipment};
use crate::{
    World,
    foreign::Content,
    identity::ReferenceId,
    inventory::ItemId,
    reference_state::{Pose, State},
    snapshot::Snapshot,
};
use fallout_data::{
    actors::{self, placements},
    identity::FormKey,
    loaded_scripts,
    world::Transform,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Clone, Copy)]
pub struct Sources<'a, 'source> {
    pub placements: &'a placements::Catalogue,
    pub actors: &'a actors::Catalogue<'source>,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub context: context::Limits,
    pub equipment: equipment::Limits,
    pub max_request_bytes: usize,
    pub max_snapshot_bytes: usize,
    pub max_projection_bytes: usize,
    pub max_fact_links: usize,
    pub max_extra_bytes: usize,
    pub max_slot_links: usize,
    pub max_visits: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            context: Default::default(),
            equipment: Default::default(),
            max_request_bytes: 1024 * 1024,
            max_snapshot_bytes: 32 * 1024 * 1024,
            max_projection_bytes: 64 * 1024 * 1024,
            max_fact_links: 100_000,
            max_extra_bytes: 16 * 1024 * 1024,
            max_slot_links: 65_536,
            max_visits: 2_100_000,
        }
    }
}
/// Empty struct variants make the wire reject unrecognized intent properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    Engineering {},
    Faithful {},
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    /// SHA256 of compact canonical serde_json Snapshot serialization, not file whitespace.
    pub expected_snapshot_sha256: String,
    pub reference: ReferenceId,
    pub actor: FormKey,
    pub intent: Intent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub claim: Claim,
    pub cell: FormKey,
    pub position_bits: [u32; 3],
    pub rotation_bits: [u32; 3],
    // Explicit null preserves missing scale; an omitted field is not an intent.
    #[serde(deserialize_with = "required_optional")]
    pub scale_bits: Option<u32>,
    pub enabled: bool,
}
pub(super) fn required_optional<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d)
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor engineering intent input snapshot or selected source changed")]
    ContextChanged,
    #[error("faithful actor initialization, transfer and equipment rules are unmeasured")]
    FaithfulUnsupported,
    #[error("actor engineering intent unavailable: {0}")]
    Unavailable(&'static str),
    #[error("actor engineering intent {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Context(#[from] context::Error),
    #[error(transparent)]
    Equipment(#[from] equipment::Error),
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Operation {
    Reference {
        state: State,
    },
    Transfer {
        item: ItemId,
        destination: ReferenceId,
        changed: bool,
    },
    Equipment {
        item: ItemId,
        supplied_slots: Option<Vec<u16>>,
    },
}
/// This output has no deserialization or live-world apply authority. The caller
/// may explicitly save its snapshot and make a new claim against that snapshot.
#[derive(Debug, Serialize)]
pub struct CandidateSnapshot<'a> {
    claim: Claim,
    actor_before: context::Observation<'a>,
    selected_before: Option<equipment::Selection>,
    operation: Operation,
    input_snapshot_sha256: String,
    candidate_snapshot_sha256: String,
    candidate_snapshot: Snapshot,
    visits: usize,
    faithful_rules_supported: bool,
    scope: &'static str,
}
impl CandidateSnapshot<'_> {
    pub fn snapshot(&self) -> &Snapshot {
        &self.candidate_snapshot
    }
    pub fn input_sha256(&self) -> &str {
        &self.input_snapshot_sha256
    }
    pub fn candidate_sha256(&self) -> &str {
        &self.candidate_snapshot_sha256
    }
    pub fn actor_before(&self) -> &context::Observation<'_> {
        &self.actor_before
    }
    pub fn selected_before(&self) -> Option<&equipment::Selection> {
        self.selected_before.as_ref()
    }
    pub fn visits(&self) -> usize {
        self.visits
    }
}
struct Admission {
    bytes: usize,
    maximum: usize,
    hash: Sha256,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= self.maximum)
            .ok_or_else(|| std::io::Error::other("actor intent byte budget"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn digest(
    value: &impl Serialize,
    maximum: usize,
    label: &'static str,
) -> Result<String, Error> {
    let mut sink = Admission {
        bytes: 0,
        maximum,
        hash: Sha256::new(),
    };
    serde_json::to_writer(&mut sink, value).map_err(|_| Error::Capacity(label))?;
    Ok(format!("{:x}", sink.hash.finalize()))
}
fn bytes(value: &impl Serialize, maximum: usize, label: &'static str) -> Result<usize, Error> {
    let mut sink = Admission {
        bytes: 0,
        maximum,
        hash: Sha256::new(),
    };
    serde_json::to_writer(&mut sink, value).map_err(|_| Error::Capacity(label))?;
    Ok(sink.bytes)
}
fn replace_bytes(total: usize, old: usize, new: usize) -> Result<usize, Error> {
    total
        .checked_sub(old)
        .and_then(|n| n.checked_add(new))
        .ok_or(Error::Capacity("candidate snapshot byte"))
}
/// Exact serialized length delta for these three narrowly declared mutations.
/// This is an admission calculation, not a save encoder or another snapshot DTO.
fn admit_candidate_clone(
    input: &Snapshot,
    world: &World<'_>,
    claim: &Claim,
    selected: Option<&equipment::Selection>,
    operation: &Operation,
    world_limits: crate::Limits,
    limits: Limits,
) -> Result<(), Error> {
    let maximum = limits
        .max_snapshot_bytes
        .min(world_limits.max_snapshot_bytes);
    let mut total = bytes(input, maximum, "input snapshot byte")?;
    total = replace_bytes(
        total,
        bytes(&input.state_revision, 20, "candidate snapshot byte")?,
        bytes(&world.revision(), 20, "candidate snapshot byte")?,
    )?;
    match operation {
        Operation::Reference { state } => {
            #[derive(Serialize)]
            struct Entry<'a> {
                id: ReferenceId,
                state: &'a State,
            }
            let new = Entry {
                id: claim.reference,
                state,
            };
            let new_bytes = bytes(&new, limits.max_request_bytes, "candidate snapshot byte")?;
            if let Some(old) = input
                .reference_states
                .iter()
                .find(|s| s.id == claim.reference)
            {
                total = replace_bytes(
                    total,
                    bytes(old, maximum, "candidate snapshot byte")?,
                    new_bytes,
                )?;
            } else {
                total = total
                    .checked_add(new_bytes)
                    .and_then(|n| n.checked_add(usize::from(!input.reference_states.is_empty())))
                    .ok_or(Error::Capacity("candidate snapshot byte"))?;
            }
        }
        Operation::Transfer {
            destination,
            changed,
            ..
        } => {
            if *changed {
                let selection = selected.expect("private transfer selection");
                let owner = selection.selected_lot().owner();
                total = replace_bytes(
                    total,
                    bytes(&owner, 20, "candidate snapshot byte")?,
                    bytes(destination, 20, "candidate snapshot byte")?,
                )?;
                let source = input
                    .inventory_banks
                    .iter()
                    .find(|b| b.owner == owner)
                    .expect("admitted source bank");
                let target = input
                    .inventory_banks
                    .iter()
                    .find(|b| b.owner == *destination)
                    .expect("admitted destination bank");
                total = replace_bytes(
                    total,
                    usize::from(source.items.len() > 1),
                    usize::from(!target.items.is_empty()),
                )?;
            }
        }
        Operation::Equipment { supplied_slots, .. } => {
            let old = &selected
                .expect("private equipment selection")
                .selected_lot()
                .facts()
                .equipped_slots;
            total = replace_bytes(
                total,
                bytes(old, maximum, "candidate snapshot byte")?,
                bytes(
                    supplied_slots,
                    limits.max_request_bytes,
                    "candidate snapshot byte",
                )?,
            )?;
        }
    }
    admit(total, maximum, "candidate snapshot byte")
}
/// Bounded compact identity calculation for an explicit current snapshot claim.
pub fn snapshot_sha256(
    input: &Snapshot,
    world_limits: crate::Limits,
    limits: Limits,
) -> Result<String, Error> {
    // Bound borrowed bytes before source-name validation can normalize strings.
    let hash = digest(
        input,
        limits
            .max_snapshot_bytes
            .min(world_limits.max_snapshot_bytes),
        "input snapshot byte",
    )?;
    input.validate_intrinsic(world_limits)?;
    Ok(hash)
}
pub(super) fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
pub(super) fn visits(values: &[usize], limits: Limits) -> Result<usize, Error> {
    let total = values
        .iter()
        .try_fold(0usize, |n, v| n.checked_add(*v))
        .ok_or(Error::Capacity("visit"))?;
    admit(total, limits.max_visits, "visit")?;
    Ok(total)
}
/// Shared admission stays in this actor-owned module. No saved view/report is accepted.
#[allow(clippy::too_many_arguments)]
pub(super) fn restore<'a, 'source, 'scripts>(
    input: &Snapshot,
    scripts: &'scripts loaded_scripts::Catalogue,
    content: &Content,
    sources: Sources<'a, 'source>,
    claim: &Claim,
    request: &impl Serialize,
    world_limits: crate::Limits,
    limits: Limits,
) -> Result<(World<'scripts>, context::Observation<'a>, String), Error> {
    if claim.intent != (Intent::Engineering {}) {
        return Err(Error::FaithfulUnsupported);
    }
    // Count the complete request before retaining any of its strings/slot vectors.
    digest(request, limits.max_request_bytes, "request byte")?;
    for count in [
        sources.placements.sources().len(),
        sources.actors.sources().len(),
    ] {
        admit(count, limits.context.max_sources, "source")?;
    }
    let input_hash = snapshot_sha256(input, world_limits, limits)?;
    if input_hash != claim.expected_snapshot_sha256 {
        return Err(Error::ContextChanged);
    }
    let world = World::restore(scripts, input.clone(), world_limits)?;
    let actor = context::observe(
        &world,
        content,
        sources.placements,
        sources.actors,
        claim.reference,
        limits.context,
    )?;
    if actor.actor.key != &claim.actor {
        return Err(Error::ContextChanged);
    }
    visits(&[actor.visits], limits)?;
    Ok((world, actor, input_hash))
}
#[allow(clippy::too_many_arguments)]
pub(super) fn finish<'a>(
    input: &Snapshot,
    world: &World<'_>,
    claim: &Claim,
    actor: context::Observation<'a>,
    selected: Option<equipment::Selection>,
    operation: Operation,
    input_hash: String,
    work: usize,
    world_limits: crate::Limits,
    limits: Limits,
) -> Result<CandidateSnapshot<'a>, Error> {
    admit_candidate_clone(
        input,
        world,
        claim,
        selected.as_ref(),
        &operation,
        world_limits,
        limits,
    )?;
    let snapshot = world.snapshot();
    snapshot.validate_intrinsic(world_limits)?;
    let candidate_hash = digest(
        &snapshot,
        limits
            .max_snapshot_bytes
            .min(world_limits.max_snapshot_bytes),
        "candidate snapshot byte",
    )?;
    let candidate = CandidateSnapshot {
        claim: claim.clone(),
        actor_before: actor,
        selected_before: selected,
        operation,
        input_snapshot_sha256: input_hash,
        candidate_snapshot_sha256: candidate_hash,
        candidate_snapshot: snapshot,
        visits: work,
        faithful_rules_supported: false,
        scope: "Explicit engineering intent applied once through canonical APIs to a privately restored candidate; source identity qualified, input unchanged, no retail initialization, AI, pickup, equip or slot conflict rules",
    };
    digest(&candidate, limits.max_projection_bytes, "projection byte")?;
    Ok(candidate)
}

pub fn apply_private<'a, 'source>(
    input: &Snapshot,
    scripts: &loaded_scripts::Catalogue,
    content: &Content,
    sources: Sources<'a, 'source>,
    choice: &Choice,
    world_limits: crate::Limits,
    limits: Limits,
) -> Result<CandidateSnapshot<'a>, Error> {
    let (mut world, actor, hash) = restore(
        input,
        scripts,
        content,
        sources,
        &choice.claim,
        choice,
        world_limits,
        limits,
    )?;
    if content.source_form(&world, &choice.cell)?.kind != *b"CELL" {
        return Err(Error::Unavailable("supplied_cell_is_not_a_live_cell"));
    }
    let pose = Pose::from_source(
        &Transform {
            position: choice.position_bits.map(f32::from_bits),
            rotation: choice.rotation_bits.map(f32::from_bits),
        },
        choice.scale_bits.map(f32::from_bits),
    )?;
    let state = State::new(choice.cell.clone(), pose, choice.enabled)?;
    let work = visits(&[actor.visits, 4], limits)?;
    let view = world.reference_view(choice.claim.reference)?;
    let stage = world.stage_reference_state(&view, state.clone())?;
    world.commit_reference_state(stage)?;
    finish(
        input,
        &world,
        &choice.claim,
        actor,
        None,
        Operation::Reference { state },
        hash,
        work,
        world_limits,
        limits,
    )
}
