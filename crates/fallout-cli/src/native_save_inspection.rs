use super::{
    Result,
    inspection_input::Order,
    script_state_inspection::{EngineeringCommit, engineering_world, event_commit_probe},
};
use fallout_data::{baseline, loaded_scripts};
use fallout_runtime::{
    Limits,
    identity::Value as LocalValue,
    save::{
        AvailabilityError, AvailabilityPoll, AvailabilityRequest, AvailabilityTask, Captured,
        Recovery, Repository, RequestIdentity, RestoreError, RestorePoll, RestoreTask, SaveStatus,
        SaveWorker, format,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    num::NonZeroU64,
    path::Path,
    sync::Arc,
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
pub(super) fn availability(install: &Path, order_path: &Path, root: &Path) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )?);
    let weak = Arc::downgrade(&catalogue);
    let repository = Repository::open(root, &[install.into()])?;
    let cohort = fallout_runtime::snapshot::cohort(&catalogue)?;
    let request =
        AvailabilityRequest::new(NonZeroU64::new(1).unwrap(), repository.campaign(), &cohort)?;
    let current =
        AvailabilityRequest::new(NonZeroU64::new(2).unwrap(), repository.campaign(), &cohort)?;
    let (mut task, gate) = AvailabilityTask::start_gated(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        request.clone(),
    )?;
    if !matches!(task.try_poll(), AvailabilityPoll::Pending)
        || !matches!(task.try_poll(), AvailabilityPoll::Pending)
    {
        return Err("Held availability read did not remain pending".into());
    }
    gate.release()?;
    // This CLI may join off frame; a menu host continues nonblocking polling.
    task.finish()?;
    let report = match task.try_poll() {
        AvailabilityPoll::Ready(candidate) => candidate.take_for(&request)?,
        AvailabilityPoll::Failed(error) => return Err(error.into()),
        state => return Err(format!("Availability did not complete: {state:?}").into()),
    };
    if !matches!(task.try_poll(), AvailabilityPoll::Delivered) || task.cancel() {
        return Err("Accepted availability was delivered again or revoked".into());
    }
    let (mut cancelled, held) = AvailabilityTask::start_gated(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        request.clone(),
    )?;
    if !matches!(cancelled.try_poll(), AvailabilityPoll::Pending) || !cancelled.cancel() {
        return Err("Held availability cancellation failed".into());
    }
    cancelled.finish()?;
    if !matches!(cancelled.try_poll(), AvailabilityPoll::Cancelled)
        || !matches!(held.release(), Err(AvailabilityError::Cancelled))
    {
        return Err("Cancelled availability gate revived the job".into());
    }
    let mut superseded = AvailabilityTask::start(
        repository,
        Arc::clone(&catalogue),
        Limits::default(),
        request.clone(),
    )?;
    superseded.finish()?;
    let AvailabilityPoll::Ready(candidate) = superseded.try_poll() else {
        return Err("Supersession check needs a completed availability report".into());
    };
    if !matches!(
        candidate.take_for(&current),
        Err(AvailabilityError::Superseded)
    ) {
        return Err("Superseded availability report was admitted".into());
    }
    drop(catalogue);
    if weak.upgrade().is_some() {
        return Err("Availability result retained source storage".into());
    }
    Ok(json!({
        "schema_version":1,"profile":"nv-original","availability":report,
        "scope":"Separate read-only source-bound slot observations; choose an explicit Recovery policy for a later load",
        "pair_is_atomic":false,"slot_selected":false,"current_repaired":false,
        "availability_task_probe":{"request":request,"superseding_request":current,
            "held_read_pending_polls":2,"single_candidate_delivered":true,"candidate_identity_checked":true,
            "accepted_report_survives_later_cancel":true,"held_cancel_revokes_gate":true,
            "superseded_report_refused":true,"worker_source_released":true,
            "host_world_replaced":false,"callbacks_dispatched":false},
        "original_live_state_captured":false,"retail_parity_accepted":false
    }))
}
pub(super) fn restore_probe(
    install: &Path,
    order_path: &Path,
    root: &Path,
    request_id: NonZeroU64,
    recover_previous: bool,
) -> Result<Value> {
    let superseding_id = request_id
        .get()
        .checked_add(1)
        .and_then(NonZeroU64::new)
        .ok_or("Restore probe needs a request ID below u64::MAX")?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue = Arc::new(loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )?);
    let weak = Arc::downgrade(&catalogue);
    let repository = Repository::open(root, &[install.into()])?;
    let cohort = fallout_runtime::snapshot::cohort(&catalogue)?;
    let identity = RequestIdentity::new(request_id, repository.campaign(), &cohort)?;
    let current = RequestIdentity::new(superseding_id, repository.campaign(), &cohort)?;
    let recovery = if recover_previous {
        Recovery::PreviousIfCurrentInvalid
    } else {
        Recovery::Strict
    };
    let (mut task, admission) = RestoreTask::start_gated(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        recovery,
        identity.clone(),
    )?;
    if !matches!(task.try_poll(), RestorePoll::Pending)
        || !matches!(task.try_poll(), RestorePoll::Pending)
    {
        return Err("Held restore read did not remain pending".into());
    }
    admission.release()?;
    // A command-line proof may join off frame. A live host keeps polling.
    task.finish()?;
    let candidate = match task.try_poll() {
        RestorePoll::Ready(candidate) => candidate,
        RestorePoll::Failed(error) => return Err(error.into()),
        state => return Err(format!("Restore did not complete: {state:?}").into()),
    };
    if !matches!(task.try_poll(), RestorePoll::Delivered) {
        return Err("Restore candidate was delivered more than once".into());
    }
    let (mut restored, receipt) = candidate.take_for(&identity)?;
    if task.cancel() {
        return Err("Accepted world was revoked by later cancellation".into());
    }
    let snapshot = restored.snapshot();
    let (ordinary, ordinary_receipt) =
        repository.load(Arc::clone(&catalogue), Limits::default(), recovery)?;
    if snapshot != ordinary.snapshot()
        || serde_json::to_value(&receipt)? != serde_json::to_value(&ordinary_receipt)?
        || receipt.current_repaired
    {
        return Err("Asynchronous restore differs from ordinary explicit-policy load".into());
    }
    let old_stage_rejected = if let Some(reference) = snapshot.reference_states.first() {
        let stage = ordinary.stage_reference_state(
            &ordinary.reference_view(reference.id)?,
            reference.state.clone(),
        )?;
        if !matches!(
            restored.commit_reference_state(stage),
            Err(fallout_runtime::Error::StaleHandle)
        ) || restored.snapshot() != snapshot
        {
            return Err("Old canonical stage acted on accepted restored epoch".into());
        }
        Some(true)
    } else {
        None
    };
    let (mut cancelled, held) = RestoreTask::start_gated(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        recovery,
        identity.clone(),
    )?;
    if !matches!(cancelled.try_poll(), RestorePoll::Pending)
        || !cancelled.cancel()
        || !matches!(cancelled.try_poll(), RestorePoll::Cancelled)
    {
        return Err("Held restore cancellation failed".into());
    }
    cancelled.finish()?;
    if !matches!(held.release(), Err(RestoreError::Cancelled)) {
        return Err("A retained admission revived a cancelled restore".into());
    }
    let mut superseded = RestoreTask::start(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        recovery,
        identity.clone(),
    )?;
    superseded.finish()?;
    let RestorePoll::Ready(candidate) = superseded.try_poll() else {
        return Err("Supersession proof needs a complete candidate".into());
    };
    if !matches!(candidate.take_for(&current), Err(RestoreError::Superseded))
        || restored.snapshot() != snapshot
    {
        return Err("Superseded candidate changed selected host state".into());
    }
    for explicit in [true, false] {
        let mut revoked = RestoreTask::start(
            repository.clone(),
            Arc::clone(&catalogue),
            Limits::default(),
            recovery,
            identity.clone(),
        )?;
        revoked.finish()?;
        let RestorePoll::Ready(candidate) = revoked.try_poll() else {
            return Err("Revocation proof needs a delivered candidate".into());
        };
        if explicit && !revoked.cancel() {
            return Err("Delivered restore cancellation failed".into());
        }
        drop(revoked);
        if !matches!(candidate.take_for(&identity), Err(RestoreError::Cancelled)) {
            return Err("Delivered candidate survived cancellation/drop".into());
        }
    }
    let bytes = snapshot.encode(Limits::default().max_snapshot_bytes)?;
    let report = json!({
        "schema_version":1,"profile":"nv-original","request":identity,"superseding_request":current,
        "receipt":receipt,"canonical_snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),
        "canonical_snapshot_bytes":bytes.len(),"state_schema":snapshot.schema_version,
        "instances":snapshot.instances.len(),"references":snapshot.references.len(),
        "reference_states":snapshot.reference_states.len(),"inventory_banks":snapshot.inventory_banks.len(),
        "pending_events":snapshot.pending_events.len(),"recovery_policy":if recover_previous {"previous_if_current_invalid"} else {"strict"},
        "held_read_pending_polls":2,"single_candidate_delivered":true,"candidate_explicitly_accepted":true,
        "complete_ordinary_restore_equal":true,"retained_gate_cancel_refused":true,
        "superseded_candidate_refused":true,"delivered_candidate_cancel_and_drop_refused":true,
        "accepted_world_survives_later_cancel":true,"old_reference_stage_rejected":old_stage_rejected,
        "host_world_replaced":false,"callbacks_dispatched":false,"current_repaired":false,
        "source_bound_restore":true,"original_live_state_captured":false,"retail_parity_accepted":false
    });
    drop(ordinary);
    drop(restored);
    drop(catalogue);
    if weak.upgrade().is_some() {
        return Err("Completed restore retained source catalogue storage".into());
    }
    Ok(report)
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
    let first_status = SaveStatus::new(worker.try_submit(capture)?);
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
    let first_receipt = first_status.wait()?;
    let (restored, _) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    if restored.snapshot() != first {
        return Err("Worker capture included later state mutations".into());
    }
    let second = engineering.world.snapshot();
    let second_status =
        SaveStatus::new(worker.try_submit(Captured::at_boundary(&engineering.world))?);
    worker.finish()?;
    let second_receipt = second_status.wait()?;
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
