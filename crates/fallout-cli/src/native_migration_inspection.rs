//! Explicit schema-2 native import. Original files remain read-only inputs.
use super::{Result, inspection_input::Order};
use fallout_data::{baseline, loaded_scripts};
use fallout_runtime::{
    Limits, World,
    save::{Captured, Recovery, Repository, format},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(super) fn import(
    install: &Path,
    order_path: &Path,
    input: &Path,
    destination: &Path,
) -> Result<Value> {
    eprintln!("Native migration: validating schema-2 container and exact source cohort...");
    let limits = Limits::default();
    // Retain the write-denying input handle through restoration and publication.
    let mut source = baseline::open_source(input)?;
    let maximum = limits
        .max_snapshot_bytes
        .checked_add(format::OVERHEAD)
        .ok_or("Save budget overflow")?;
    if source.metadata()?.len() > maximum as u64 {
        return Err("Legacy native file exceeds byte budget".into());
    }
    let mut bytes = Vec::new();
    (&mut source)
        .take((maximum + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("Legacy native file exceeds byte budget".into());
    }
    let migration = format::migrate_v2(&bytes, limits)?;
    let source_metadata = migration.source_metadata;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let expected = migration.snapshot;
    let world = World::restore(&catalogue, expected.clone(), limits)?;
    if world.snapshot() != expected {
        return Err("Migrated canonical state differs after restore".into());
    }
    // Only validated, source-bound state can create the new repository.
    let repository = Repository::create(
        destination,
        &[install.into(), input.into()],
        world.campaign(),
    )?;
    let receipt = repository.commit(&Captured::at_boundary(&world))?;
    let (restored, _) = repository.load(&catalogue, limits, Recovery::Strict)?;
    if restored.snapshot() != expected {
        return Err("Migrated native round trip differs".into());
    }
    let encoded = expected.encode(limits.max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","source_state_schema":2,"target_state_schema":expected.schema_version,
        "source_metadata":source_metadata,"new_write":receipt,"snapshot_bytes":encoded.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&encoded)),
        "instances":world.instance_count(),"references":world.reference_count(),"pending_events":world.pending_events().len(),
        "inventory_banks":expected.inventory_banks.len(),"next_item":expected.next_item,"source_bound_restore":true,"canonical_state_round_trip_equal":true,
        "source_opened_read_only":true,"scope":"Explicit import of our schema-2 native container into a new repository; prior state preserved, inventories uninitialized",
        "original_live_values_captured":false,"retail_save_compatibility":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
