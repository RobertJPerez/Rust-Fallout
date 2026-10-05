//! Caller-supplied slot metadata, with no retail equipment selection or conflicts.
use super::{
    equipment,
    reference_intent::{self, CandidateSnapshot, Claim, Error, Limits, Sources},
};
use crate::{foreign::Content, inventory::ItemId, snapshot::Snapshot};
use fallout_data::loaded_scripts;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub claim: Claim,
    pub item: ItemId,
    /// Required field: null means unknown, [] means known empty. Order is retained.
    #[serde(deserialize_with = "reference_intent::required_optional")]
    pub supplied_slots: Option<Vec<u16>>,
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
    let slots = choice.supplied_slots.as_ref().map_or(0, Vec::len);
    reference_intent::admit(slots, limits.max_slot_links, "slot link")?;
    let selected = equipment::observe(
        &world,
        content,
        choice.claim.reference,
        choice.item,
        limits.equipment,
    )?;
    let old = selected.selected_lot().facts();
    let old_slots = old.equipped_slots.as_ref().map_or(0, Vec::len);
    let links = old
        .links()
        .checked_sub(old_slots)
        .and_then(|n| n.checked_add(slots))
        .ok_or(Error::Capacity("fact link"))?;
    reference_intent::admit(links, limits.max_fact_links, "fact link")?;
    reference_intent::admit(old.extra_bytes()?, limits.max_extra_bytes, "extra byte")?;
    let work = reference_intent::visits(&[actor.visits, selected.visits(), links, 2], limits)?;
    // Precharged admitted facts are cloned once; only the chosen optional slots change.
    let mut facts = old.clone();
    facts.equipped_slots = choice.supplied_slots.clone();
    world.replace_item_facts(choice.item, facts)?;
    reference_intent::finish(
        input,
        &world,
        &choice.claim,
        actor,
        Some(selected),
        reference_intent::Operation::Equipment {
            item: choice.item,
            supplied_slots: choice.supplied_slots.clone(),
        },
        hash,
        work,
        world_limits,
        limits,
    )
}
