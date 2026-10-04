use super::{
    Result,
    inspection_input::Order,
    script_state_inspection::{EngineeringCommit, engineering_world, event_commit_probe},
};
use fallout_data::{baseline, loaded_scripts};
use fallout_runtime::{
    Limits,
    identity::Value as LocalValue,
    save::{Captured, Recovery, Repository, SaveWorker, format},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

fn save_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn bounded_file(path: &Path) -> Result<Vec<u8>> {
    let maximum = Limits::default().max_snapshot_bytes + format::OVERHEAD;
    let file = baseline::open_source(path)?;
    if file.metadata()?.len() > maximum as u64 {
        return Err("Native save file exceeds byte budget".into());
    }
    let mut bytes = Vec::new();
    file.take((maximum + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err("Native save file exceeds byte budget".into());
    }
    Ok(bytes)
}
pub(super) fn file(path: &Path) -> Result<Value> {
    let decoded = format::decode(&bounded_file(path)?, Limits::default())?;
    Ok(
        json!({"schema_version":1,"metadata":decoded.metadata,"scope":"Native container and canonical JSON shape/integrity; source-bound restoration is separate","retail_save_compatibility":false}),
    )
}
pub(super) fn load(install: &Path, order_path: &Path, root: &Path) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let repository = Repository::open(root, &[install.into()])?;
    let (world, receipt) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    let snapshot = world.snapshot();
    let bytes = snapshot.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","receipt":receipt,"instances":world.instance_count(),
        "references":world.reference_count(),"pending_events":world.pending_events().len(),"canonical_snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),
        "canonical_snapshot_bytes":bytes.len(),"source_bound_restore":true,"original_live_state_captured":false,"retail_parity_accepted":false}),
    )
}
pub(super) fn probe(
    install: &Path,
    order_path: &Path,
    root: &Path,
    engineering_event_commit: Option<&Path>,
) -> Result<Value> {
    let request = engineering_event_commit
        .map(EngineeringCommit::read)
        .transpose()?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let mut engineering = engineering_world(&catalogue)?;
    let capture = Captured::at_boundary(&engineering.world);
    let first = capture.snapshot().clone();
    // Complete the opt-in transaction before creating the new repository or
    // starting a writer. Invalid input cannot publish even a precommit save.
    let event_commit = request
        .map(|request| event_commit_probe(&mut engineering.world, request))
        .transpose()?;
    let repository = Repository::create(root, &[install.into()], engineering.world.campaign())?;
    let mut worker = SaveWorker::start(repository.clone(), 2)?;
    let ticket = worker.try_submit(capture)?;
    if event_commit.is_none() {
        let instance = first
            .instances
            .iter()
            .find(|instance| {
                instance
                    .locals
                    .iter()
                    .any(|local| matches!(local.value, LocalValue::Number { .. }))
            })
            .ok_or("Probe needs a numeric local")?;
        let local = instance
            .locals
            .iter()
            .find(|local| matches!(local.value, LocalValue::Number { .. }))
            .ok_or("Probe numeric local")?;
        engineering.world.assign(
            engineering.world.handle(instance.id)?,
            &[(
                local.index,
                LocalValue::Number {
                    bits: 0x7ff8123456789abc,
                },
            )],
        )?;
    }
    let first_receipt = ticket.wait()?;
    let (restored, _) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    if restored.snapshot() != first {
        return Err("Worker capture included later state mutations".into());
    }
    let second = engineering.world.snapshot();
    let second_receipt = worker
        .try_submit(Captured::at_boundary(&engineering.world))?
        .wait()?;
    worker.finish()?;
    let (restored, _) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    if restored.snapshot() != second {
        return Err("Native current-slot round trip differs".into());
    }
    let current_path = repository.path().join("current.frsv");
    let previous_path = repository.path().join("previous.frsv");
    let current = bounded_file(&current_path)?;
    let previous = bounded_file(&previous_path)?;
    let current_metadata = format::decode(&current, Limits::default())?.metadata;
    let previous_metadata = format::decode(&previous, Limits::default())?.metadata;
    save_new(&repository.path().join("golden-current.frsv"), &current)?;
    save_new(&repository.path().join("golden-previous.frsv"), &previous)?;
    // Fault injection is confined to this newly created native repository.
    fs::write(&current_path, b"deliberately truncated native probe")?;
    let current_failure = match repository.load(&catalogue, Limits::default(), Recovery::Strict) {
        Ok(_) => return Err("Truncated native current slot was accepted".into()),
        Err(error) => error.to_string(),
    };
    let (recovered, recovery) = repository.load(
        &catalogue,
        Limits::default(),
        Recovery::PreviousIfCurrentInvalid,
    )?;
    if recovered.snapshot() != first
        || recovery.current_failure.as_deref() != Some(current_failure.as_str())
        || recovery.current_repaired
    {
        return Err("Explicit previous-slot recovery differs".into());
    }
    let (_, repair) = repository.recover_previous(&catalogue, Limits::default())?;
    let (repaired, _) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    if repaired.snapshot() != first || !repair.current_repaired {
        return Err("Native previous-slot repair differs".into());
    }
    // Publish the later explicit host snapshot again for the cold-process proof.
    let final_receipt = repository.commit(&Captured::at_boundary(&engineering.world))?;
    let mut report = json!({"schema_version":1,"profile":"nv-original","scope":"Filesystem engineering probe on explicit values using original compiled schemas; no original live-state capture",
        "sources":catalogue.sources,"instances":engineering.world.instance_count(),"pending_events":engineering.world.pending_events().len(),
        "current":current_metadata,"previous":previous_metadata,"first_write":first_receipt,"second_write":second_receipt,"final_write":final_receipt,
        "worker_capture_isolated":true,"current_round_trip_equal":true,"strict_truncation_rejected":true,"recovery":recovery,"repair":repair,
        "previous_round_trip_equal":true,"canonical_snapshot_sha256":format!("{:x}",Sha256::digest(second.encode(Limits::default().max_snapshot_bytes)?)),
        "original_live_state_captured":false,"retail_save_compatibility":false,"retail_parity_accepted":false});
    if let Some(mut event_commit) = event_commit {
        event_commit["strict_current_failure"] = json!(current_failure);
        event_commit["worker_pre_post_boundaries_equal"] = json!(true);
        report["engineering_event_commit"] = event_commit;
    }
    Ok(report)
}
