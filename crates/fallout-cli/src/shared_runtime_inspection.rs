//! Move explicit engineering state across a worker boundary with owned sources.
//! This exercises source lifetime and persistence, not original script execution.
use super::{Result, inspection_input::Order, script_state_inspection};
use fallout_data::loaded_scripts::{self, Catalogue};
use fallout_runtime::{Limits, World, snapshot::Snapshot};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

pub(super) fn handles_sha256(catalogue: &Catalogue) -> Result<String> {
    let mut hash = Sha256::new();
    for (_, script) in catalogue.iter() {
        let bytes = serde_json::to_vec(script.handle())?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn inspect(install: &Path, order_path: &Path, cache: Option<&Path>) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let catalogue = Arc::new(Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )?);
    let sources = serde_json::to_value(&catalogue.sources)?;
    let source_count = catalogue.counts.scripts;
    let expected_handles = handles_sha256(&catalogue)?;
    let cache_report = serde_json::to_value(store.index_cache_report())?;
    drop(store);

    let borrowed = script_state_inspection::engineering_world(&catalogue)?;
    let expected = borrowed.world.snapshot();
    let expected_bytes = expected.encode(Limits::default().max_snapshot_bytes)?;
    let old_handles = borrowed.handles;
    drop(borrowed.world);
    let world: World<'static> =
        World::restore(Arc::clone(&catalogue), expected.clone(), Limits::default())?;
    let weak = Arc::downgrade(&catalogue);
    drop(catalogue);
    if weak.strong_count() != 1 {
        return Err("Owned world did not retain exactly one shared source owner".into());
    }
    let worker_expected = expected.clone();
    let worker_handles = expected_handles.clone();
    let worker = std::thread::Builder::new()
        .name("fallout-shared-world-probe".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || -> std::result::Result<World<'static>, String> {
            if world.snapshot() != worker_expected
                || handles_sha256(world.catalogue()).map_err(|e| e.to_string())? != worker_handles
            {
                return Err("Worker source handles or canonical state changed".into());
            }
            for (id, old) in old_handles {
                if world.instance(old).is_ok()
                    || world
                        .instance(world.handle(id).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?
                        .id()
                        != id
                {
                    return Err(
                        "Worker accepted an old world handle or lost a persistent ID".into(),
                    );
                }
            }
            Ok(world)
        })?;
    let mut world = worker
        .join()
        .map_err(|_| "Shared runtime worker panicked")??;
    if world
        .snapshot()
        .encode(Limits::default().max_snapshot_bytes)?
        != expected_bytes
    {
        return Err("Worker changed canonical state bytes".into());
    }
    let before = world.snapshot();
    let mut invalid = before.clone();
    invalid.catalogue_sha256 = "0".repeat(64);
    if world.replace_from_snapshot(invalid).is_ok() || world.snapshot() != before {
        return Err("Owned world replacement failed its atomic source check".into());
    }
    let decoded = Snapshot::decode(&expected_bytes, Limits::default())?;
    world.replace_from_snapshot(decoded)?;
    if world.snapshot() != expected || weak.strong_count() != 1 {
        return Err(
            "Owned replacement lost canonical state or retained another source owner".into(),
        );
    }
    let cohort = world.catalogue_fingerprint().to_string();
    drop(world);
    if weak.upgrade().is_some() {
        return Err("Source catalogue was not released with its last world".into());
    }
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"Owned immutable source lifetime, worker transfer and restoration over explicit engineering inputs; no game session or bytecode execution",
        "sources":sources,"source_definitions_checked":source_count,"winning_handles_sha256":expected_handles,
        "catalogue_sha256":cohort,"instances":expected.instances.len(),"pending_events":expected.pending_events.len(),
        "snapshot_bytes":expected_bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&expected_bytes)),
        "worker_state_and_source_handles_equal":true,"old_handles_rejected":true,
        "failed_replacement_atomic":true,"owned_replacement_equal":true,"last_source_owner_released":true,
        "explicit_load_order":order.names,"load_order_sha256":order.sha256,"index_cache":cache_report,
        "original_state_captured":false,"bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[]}))
}
