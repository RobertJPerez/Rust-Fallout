//! Intrinsic identities and links, independent of loaded source declarations.
use super::{SCHEMA_VERSION, Snapshot};
use crate::{
    Error, Limits, Result,
    events::Clocks,
    identity::{CampaignId, Value, valid_form},
};
use fallout_data::identity::ProfileId;
use std::collections::BTreeSet;

pub(super) fn check(snapshot: &Snapshot, limits: Limits) -> Result<()> {
    if snapshot.schema_version != SCHEMA_VERSION || snapshot.profile != ProfileId::NvOriginal {
        return Err(Error::Invalid(
            "snapshot schema or profile is unsupported".into(),
        ));
    }
    CampaignId::from_bytes(snapshot.campaign.bytes())?;
    if [
        snapshot.next_item,
        snapshot.next_instance,
        snapshot.next_reference,
        snapshot.next_event_sequence,
    ]
    .contains(&0)
    {
        return Err(Error::Invalid("snapshot allocator cannot be zero".into()));
    }
    let mut references = BTreeSet::new();
    let mut authored = BTreeSet::new();
    for reference in &snapshot.references {
        if reference.id.0.get() >= snapshot.next_reference || !references.insert(reference.id) {
            return Err(Error::Invalid(
                "duplicate reference or allocator would reuse an identity".into(),
            ));
        }
        if let Some(key) = &reference.authored {
            valid_form(key)?;
            if !authored.insert(key) {
                return Err(Error::Invalid(
                    "duplicate authored reference identity".into(),
                ));
            }
        }
    }
    let reference_exists = |id| {
        if references.contains(&id) {
            Ok(())
        } else {
            Err(Error::MissingReference)
        }
    };
    let mut reference_states = BTreeSet::new();
    for saved in &snapshot.reference_states {
        reference_exists(saved.id)?;
        if !reference_states.insert(saved.id) {
            return Err(Error::Invalid("duplicate saved reference state".into()));
        }
        saved.state.validate()?;
    }
    let mut instances = BTreeSet::new();
    let mut owners = BTreeSet::new();
    for instance in &snapshot.instances {
        if instance.id.0.get() >= snapshot.next_instance || !instances.insert(instance.id) {
            return Err(Error::Invalid(
                "duplicate script instance or allocator would reuse an identity".into(),
            ));
        }
        crate::identity::check_owner(&instance.owner, &reference_exists)?;
        crate::identity::check_context(
            &instance.context,
            limits.max_event_arguments,
            &reference_exists,
        )?;
        if !owners.insert(&instance.owner) {
            return Err(Error::Invalid("duplicate saved script owner".into()));
        }
        let mut locals = BTreeSet::new();
        for local in &instance.locals {
            if !locals.insert(local.index) {
                return Err(Error::Invalid("duplicate saved local".into()));
            }
            if let Value::Reference { value } = &local.value {
                crate::identity::check_reference(value, &reference_exists)?;
            }
        }
    }
    let instance_exists = |id| {
        if instances.contains(&id) {
            Ok(())
        } else {
            Err(Error::MissingInstance)
        }
    };
    let mut banks = BTreeSet::new();
    let mut items = BTreeSet::new();
    for bank in &snapshot.inventory_banks {
        reference_exists(bank.owner)?;
        if !banks.insert(bank.owner) {
            return Err(Error::Invalid("duplicate saved inventory bank".into()));
        }
        for item in &bank.items {
            if item.owner() != bank.owner
                || item.id().0.get() >= snapshot.next_item
                || !items.insert(item.id())
            {
                return Err(Error::Invalid(
                    "saved item owner, duplicate identity or allocator is invalid".into(),
                ));
            }
            crate::inventory::check_facts(
                item.facts(),
                limits,
                &reference_exists,
                &instance_exists,
            )?;
        }
    }
    let mut prior_sequence = 0;
    let mut prior_clocks = Clocks::default();
    for event in &snapshot.pending_events {
        if event.sequence <= prior_sequence
            || event.sequence >= snapshot.next_event_sequence
            || !event.arrived.no_later_than(snapshot.clocks)
            || !prior_clocks.no_later_than(event.arrived)
        {
            return Err(Error::Invalid(
                "saved event order, clocks or allocator is invalid".into(),
            ));
        }
        instance_exists(event.instance)?;
        crate::identity::check_context(
            &event.context,
            limits.max_event_arguments,
            &reference_exists,
        )?;
        prior_sequence = event.sequence;
        prior_clocks = event.arrived;
    }
    // Source definition versions, declaration kinds and compiled event sites
    // are checked separately against immutable source schemas.
    Ok(())
}
