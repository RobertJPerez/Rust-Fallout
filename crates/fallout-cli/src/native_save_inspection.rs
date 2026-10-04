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
        ForkLimits, ForkReceipt, ForkRequest, ForkSelection, ForkStage, LoadReceipt, Recovery,
        Rejection, Repository, RequestIdentity, RestoreError, RestorePoll, RestoreTask, SaveState,
        SaveStatus, SaveWorker, Slot, SlotRejectionCode, Stage, format,
    },
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    num::NonZeroU64,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

fn sibling_path(path: &Path, suffix: &str) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or("Native save probe repository needs a leaf name")?;
    let mut sibling = name.to_os_string();
    sibling.push(format!("-{suffix}"));
    Ok(path.with_file_name(sibling))
}

fn cold_fork(
    source: &Repository,
    catalogue: &loaded_scripts::Catalogue,
    install: &Path,
    destination: &Path,
    recovery: Recovery,
    expected: &ForkSelection,
) -> Result<(
    ForkReceipt,
    fallout_runtime::snapshot::Snapshot,
    LoadReceipt,
)> {
    let protected = [install.to_path_buf()];
    let receipt = source.fork_boundary(
        catalogue,
        ForkRequest {
            destination,
            protected: &protected,
            recovery,
            expected,
            world_limits: Limits::default(),
            limits: ForkLimits::default(),
        },
    )?;
    let forked = Repository::open(destination, &protected)?;
    let (world, cold_receipt) = forked.load(catalogue, Limits::default(), Recovery::Strict)?;
    Ok((receipt, world.snapshot(), cold_receipt))
}

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
    let (entered, entries) = mpsc::sync_channel(1);
    let (release, releases) = mpsc::sync_channel(1);
    let mut worker = SaveWorker::start_observing(
        repository.clone(),
        2,
        SaveWorker::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES,
        move |stage| {
            if stage == Stage::CurrentTempWritten {
                entered.send(()).expect("shutdown observer has its host");
                releases
                    .recv_timeout(Duration::from_secs(30))
                    .expect("host releases gated publication");
            }
        },
    )?;
    let mut first_status = SaveStatus::new(worker.try_submit(capture)?);
    entries.recv_timeout(Duration::from_secs(30))?;
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
    let second = engineering.world.snapshot();
    let mut second_status =
        SaveStatus::new(worker.try_submit(Captured::at_boundary(&engineering.world))?);
    if !worker.close_admission() || worker.close_admission() {
        return Err("Save admission close was not idempotent".into());
    }
    let refused = worker
        .try_submit(Captured::at_boundary(&engineering.world))
        .unwrap_err();
    if refused.reason != Rejection::WorkerStopped || refused.capture.snapshot() != &second {
        return Err("Closed save admission changed the rejected capture".into());
    }
    drop(refused);
    for _ in 0..2 {
        if worker.try_shutdown()?
            || !matches!(first_status.poll(), SaveState::Pending)
            || !matches!(second_status.poll(), SaveState::Pending)
        {
            return Err("Held first publication did not preserve Pending shutdown/tickets".into());
        }
    }
    release.send(())?;
    entries.recv_timeout(Duration::from_secs(30))?;
    if !matches!(first_status.poll(), SaveState::Published(_)) {
        return Err("Published first ticket was not observable while second write was held".into());
    }
    let first_receipt = first_status.wait()?;
    // The second writer owns its lock while held. Inspect the complete first
    // native boundary and use the same canonical source restore without a
    // competing writer lock; this read neither selects recovery nor repairs.
    let decoded = format::decode(
        &bounded_file(&repository.path().join("current.frsv"))?,
        Limits::default(),
    )?;
    let restored =
        fallout_runtime::World::restore(&catalogue, decoded.snapshot, Limits::default())?;
    if restored.snapshot() != first {
        return Err("Worker capture included later state mutations".into());
    }
    for _ in 0..2 {
        if worker.try_shutdown()? || !matches!(second_status.poll(), SaveState::Pending) {
            return Err("Held second publication did not preserve Pending shutdown/ticket".into());
        }
    }
    release.send(())?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !worker.try_shutdown()? {
        second_status.poll();
        if Instant::now() >= deadline {
            return Err("Save shutdown observation timed out".into());
        }
        std::thread::yield_now();
    }
    if !worker.try_shutdown()?
        || worker.close_admission()
        || !matches!(second_status.poll(), SaveState::Published(_))
    {
        return Err("Joined shutdown or individual published ticket was not stable".into());
    }
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
    let final_current = fs::read(&current_path)?;
    let final_previous = fs::read(&previous_path)?;
    let final_current_metadata = format::decode(&final_current, Limits::default())?.metadata;
    let final_previous_metadata = format::decode(&final_previous, Limits::default())?.metadata;
    let current_selection = ForkSelection::new(
        final_current_metadata.campaign,
        &final_current_metadata.catalogue_sha256,
        &final_current_metadata.snapshot_sha256,
    )?;
    let current_fork_path = sibling_path(root, "fork-current")?;
    let (current_fork, current_fork_snapshot, current_cold_receipt) = cold_fork(
        &repository,
        &catalogue,
        install,
        &current_fork_path,
        Recovery::Strict,
        &current_selection,
    )?;
    if current_fork.source().slot != Slot::Current
        || current_fork.source().metadata != final_current_metadata
        || current_fork.destination().metadata.generation != 1
        || current_fork.destination().metadata.snapshot_sha256
            != final_current_metadata.snapshot_sha256
        || current_fork_snapshot != second
        || current_cold_receipt.metadata != current_fork.destination().metadata
    {
        return Err(
            "Current-slot fork did not preserve and cold-restore its selected boundary".into(),
        );
    }

    // This explicit recovery fork must select the older valid slot, then leave
    // the source's damaged current bytes exactly as they were.
    let previous_selection = ForkSelection::new(
        final_previous_metadata.campaign,
        &final_previous_metadata.catalogue_sha256,
        &final_previous_metadata.snapshot_sha256,
    )?;
    let previous_fork_path = sibling_path(root, "fork-previous")?;
    fs::write(&current_path, b"deliberately truncated native fork source")?;
    let previous_fork_result = cold_fork(
        &repository,
        &catalogue,
        install,
        &previous_fork_path,
        Recovery::PreviousIfCurrentInvalid,
        &previous_selection,
    );
    let restore_current_result = fs::write(&current_path, &final_current);
    restore_current_result?;
    let (previous_fork, previous_fork_snapshot, previous_cold_receipt) = previous_fork_result?;
    if previous_fork.source().slot != Slot::Previous
        || previous_fork.source().metadata != final_previous_metadata
        || previous_fork.source().current_failure.is_none()
        || previous_fork.source().current_repaired
        || previous_fork.destination().metadata.generation != 1
        || previous_fork_snapshot != first
        || previous_cold_receipt.metadata != previous_fork.destination().metadata
    {
        return Err(
            "Previous-slot fork did not preserve and cold-restore its selected ancestor".into(),
        );
    }
    if fs::read(&current_path)? != final_current || fs::read(&previous_path)? != final_previous {
        return Err("Successful fork changed the source repository bytes".into());
    }

    // Block only the fork destination after it has been created and its first
    // temporary file written. Publication must report failure without rotating
    // either source slot; its own diagnostic obstruction stays in the child.
    let failed_fork_path = sibling_path(root, "fork-publication-failure")?;
    let mut obstruction_created = false;
    let mut obstruction_error = None;
    let failed_fork_result = repository.fork_boundary_observing(
        &catalogue,
        ForkRequest {
            destination: &failed_fork_path,
            protected: &[install.to_path_buf()],
            recovery: Recovery::Strict,
            expected: &current_selection,
            world_limits: Limits::default(),
            limits: ForkLimits::default(),
        },
        |stage| {
            if stage == Stage::CurrentTempWritten {
                match fs::create_dir(failed_fork_path.join("current.frsv")) {
                    Ok(()) => obstruction_created = true,
                    Err(error) => obstruction_error = Some(error.to_string()),
                }
            }
        },
    );
    if let Some(error) = obstruction_error {
        return Err(
            format!("Could not install isolated fork publication obstruction: {error}").into(),
        );
    }
    let failed_fork = match failed_fork_result {
        Err(failure)
            if failure.stage() == ForkStage::Publication
                && failure.reason().code() == SlotRejectionCode::NativeFormat =>
        {
            failure
        }
        Err(failure) => {
            return Err(format!("Fork publication failed at the wrong boundary: {failure}").into());
        }
        Ok(_) => return Err("Injected fork publication failure was accepted".into()),
    };
    if !obstruction_created
        || fs::read(&current_path)? != final_current
        || fs::read(&previous_path)? != final_previous
        || !failed_fork_path.join(".rust-fallout-saves").is_file()
        || !failed_fork_path.join("writer.lock").is_file()
        || !failed_fork_path.join("current.frsv").is_dir()
        || fs::read_dir(&failed_fork_path)?.any(|entry| {
            entry
                .map(|entry| entry.file_name().to_string_lossy().starts_with(".pending-"))
                .unwrap_or(true)
        })
    {
        return Err(
            "Failed fork publication changed the source or left a temporary child file".into(),
        );
    }
    let current_fork_report = serde_json::to_value(&current_fork)?;
    let previous_fork_report = serde_json::to_value(&previous_fork)?;
    let failed_fork_report = serde_json::to_value(&failed_fork)?;
    let fork_probe = json!({
        "current_fork": current_fork_report,
        "current_cold_restore_equal": true,
        "previous_recovery_fork": previous_fork_report,
        "previous_cold_restore_equal": true,
        "source_bytes_unchanged_after_successful_forks": true,
        "failed_publication": failed_fork_report,
        "failed_publication_stage": "publication",
        "source_bytes_unchanged_after_failed_publication": true,
        "destination_artifacts_retained": true,
        "pending_temporary_files_removed": true,
    });
    let mut report = json!({"schema_version":1,"profile":"nv-original","scope":"Filesystem engineering probe on explicit values using original compiled schemas; no original live-state capture",
        "sources":catalogue.sources,"instances":engineering.world.instance_count(),"pending_events":engineering.world.pending_events().len(),
        "current":current_metadata,"previous":previous_metadata,"first_write":first_receipt,"second_write":second_receipt,"final_write":final_receipt,
        "worker_capture_isolated":true,"current_round_trip_equal":true,"strict_truncation_rejected":true,"recovery":recovery,"repair":repair,
        "worker_shutdown_probe":{"held_first_pending_polls":2,"held_second_pending_polls":2,
            "close_admission_idempotent":true,"third_capture_refused_intact":true,
            "first_published_while_second_pending":true,"first_complete_source_restore_equal":true,
            "joined_clean_stable":true,"individual_tickets_published":true,"blocking_finish_compatible":true},
        "previous_round_trip_equal":true,"canonical_snapshot_sha256":format!("{:x}",Sha256::digest(second.encode(Limits::default().max_snapshot_bytes)?)),
        "original_live_state_captured":false,"retail_save_compatibility":false,"retail_parity_accepted":false});
    report["fork_probe"] = fork_probe;
    if let Some(mut event_commit) = event_commit {
        event_commit["strict_current_failure"] = json!(current_failure);
        event_commit["worker_pre_post_boundaries_equal"] = json!(true);
        report["engineering_event_commit"] = event_commit;
    }
    Ok(report)
}
