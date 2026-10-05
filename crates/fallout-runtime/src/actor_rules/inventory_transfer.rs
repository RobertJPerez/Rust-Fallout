//! Exact whole-lot engineering transfer on a privately restored actor candidate.
use super::{
    equipment,
    reference_intent::{self, CandidateSnapshot, Claim, Error, Limits, Sources},
};
use crate::{foreign::Content, identity::ReferenceId, inventory::ItemId, snapshot::Snapshot};
use fallout_data::loaded_scripts;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub claim: Claim,
    pub item: ItemId,
    pub destination: ReferenceId,
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
    let (mut world, actor, hash) = reference_intent::restore(
        input,
        scripts,
        content,
        sources,
        &choice.claim,
        choice,
        world_limits,
        limits,
    )?;
    let selected = equipment::observe(
        &world,
        content,
        choice.claim.reference,
        choice.item,
        limits.equipment,
    )?;
    // The existing iterator checks registration and initialized bank without a
    // destination observation clone or any bank creation.
    drop(world.inventory_items(choice.destination)?);
    let work = reference_intent::visits(&[actor.visits, selected.visits(), 2], limits)?;
    let changed = selected.selected_lot().owner() != choice.destination;
    world.transfer_item(choice.item, choice.destination)?;
    reference_intent::finish(
        input,
        &world,
        &choice.claim,
        actor,
        Some(selected),
        reference_intent::Operation::Transfer {
            item: choice.item,
            destination: choice.destination,
            changed,
        },
        hash,
        work,
        world_limits,
        limits,
    )
}
