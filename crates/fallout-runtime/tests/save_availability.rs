mod common;
use common::*;
use fallout_data::loaded_scripts::Catalogue;
use fallout_runtime::{
    Error, Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    save::{
        self, Captured, Recovery, Repository, Slot, SlotAvailability as Availability,
        SlotAvailabilityReport as Report, SlotRejectionCode as Code, format,
    },
    snapshot::Snapshot,
    state::initialization,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, process::Command, sync::Arc};

fn availability_request(
    repository: &Repository,
    catalogue: &Catalogue,
    id: u64,
) -> save::AvailabilityRequest {
    save::AvailabilityRequest::new(
        id.try_into().unwrap(),
        repository.campaign(),
        &fallout_runtime::snapshot::cohort(catalogue).unwrap(),
    )
    .unwrap()
}
fn async_availability(
    repository: &Repository,
    catalogue: Arc<Catalogue>,
    limits: Limits,
) -> Report {
    let request = availability_request(repository, &catalogue, 1);
    let mut task =
        save::AvailabilityTask::start(repository.clone(), catalogue, limits, request.clone())
            .unwrap();
    task.finish().unwrap();
    let save::AvailabilityPoll::Ready(candidate) = task.try_poll() else {
        panic!("availability required")
    };
    let report = candidate.take_for(&request).unwrap();
    assert!(matches!(task.try_poll(), save::AvailabilityPoll::Delivered));
    assert!(!task.cancel());
    report
}
fn availability_entries(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                if kind.is_file() {
                    Some(fs::read(entry.path()).unwrap())
                } else {
                    None
                },
            )
        })
        .collect()
}
#[test]
fn availability_task_gate_defers_actual_slot_read_and_returns_one_source_bound_observation() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, [first, _]) = setup(root.path());
    let catalogue = Arc::new(catalogue);
    let request = availability_request(&repository, &catalogue, 17);
    let (mut task, gate) = save::AvailabilityTask::start_gated(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        request.clone(),
    )
    .unwrap();
    assert_eq!(task.request(), &request);
    assert!(matches!(task.try_poll(), save::AvailabilityPoll::Pending));
    assert!(matches!(task.try_poll(), save::AvailabilityPoll::Pending));
    // The read observes a change made after spawn, never a pair frozen there.
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    fs::write(
        repository.path().join("current.frsv"),
        wrong_local(&current),
    )
    .unwrap();
    let before = directory_bytes(repository.path());
    gate.release().unwrap();
    task.finish().unwrap();
    assert_eq!(Arc::strong_count(&catalogue), 1);
    let save::AvailabilityPoll::Ready(candidate) = task.try_poll() else {
        panic!("report required")
    };
    assert_eq!(candidate.request(), &request);
    let report = candidate.take_for(&request).unwrap();
    assert_eq!(
        rejected(report.current(), Code::IncompatibleLocal).local_index(),
        Some(42)
    );
    assert_eq!(
        loadable(report.previous()).state_revision,
        first.state_revision
    );
    assert_eq!(
        report,
        repository
            .inspect_availability(Arc::clone(&catalogue), Limits::default())
            .unwrap()
    );
    assert!(matches!(task.try_poll(), save::AvailabilityPoll::Delivered));
    assert!(!task.cancel());
    assert_eq!(directory_bytes(repository.path()), before);
}
#[test]
fn availability_task_preserves_actual_missing_invalid_io_and_byte_budget_slot_reasons() {
    for case in 0..10 {
        if case == 9 && !cfg!(windows) {
            continue;
        }
        let root = tempfile::tempdir().unwrap();
        let (catalogue, repository, _) = setup(root.path());
        let catalogue = Arc::new(catalogue);
        let current = repository.path().join("current.frsv");
        let previous = repository.path().join("previous.frsv");
        let wire = fs::read(&current).unwrap();
        let mut limits = Limits::default();
        match case {
            1 => fs::write(&current, wrong_local(&wire)).unwrap(),
            2 => fs::write(&current, b"broken native container").unwrap(),
            3 => fs::remove_file(&current).unwrap(),
            4 => fs::remove_file(&previous).unwrap(),
            5 => limits.max_locals = 2,
            6 => {
                limits.max_snapshot_bytes = format::decode(&wire, limits)
                    .unwrap()
                    .metadata
                    .snapshot_bytes
                    - 1
            }
            7 => {
                fs::remove_file(&current).unwrap();
                fs::create_dir(&current).unwrap();
            }
            8 => limits.max_snapshot_bytes = usize::MAX,
            _ => {}
        }
        let before = availability_entries(repository.path());
        let held_slot = if case == 9 {
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&current)
                .unwrap();
            file.lock().unwrap();
            Some(file)
        } else {
            None
        };
        let report = async_availability(&repository, Arc::clone(&catalogue), limits);
        assert_eq!(
            report,
            repository
                .inspect_availability(Arc::clone(&catalogue), limits)
                .unwrap()
        );
        match case {
            0 => {
                assert_eq!(loadable(report.current()).generation, 2);
                assert_eq!(loadable(report.previous()).generation, 1);
            }
            1 => {
                let reason = rejected(report.current(), Code::IncompatibleLocal);
                assert_eq!(reason.local_index(), Some(42));
                assert!(!reason.truncated());
                loadable(report.previous());
            }
            2 => {
                rejected(report.current(), Code::NativeFormat);
                loadable(report.previous());
            }
            3 => {
                assert!(matches!(report.current(), Availability::Missing));
                loadable(report.previous());
            }
            4 => {
                loadable(report.current());
                assert!(matches!(report.previous(), Availability::Missing));
            }
            5 => {
                assert_eq!(
                    rejected(report.current(), Code::RuntimeCapacity).budget(),
                    Some("saved locals")
                );
                rejected(report.previous(), Code::RuntimeCapacity);
            }
            6 => {
                assert_eq!(
                    rejected(report.current(), Code::NativeFormat).message(),
                    "native save format: save file exceeds byte budget"
                );
                loadable(report.previous());
            }
            7 => {
                rejected(report.current(), Code::NativeFormat);
                loadable(report.previous());
            }
            9 => {
                let reason = rejected(report.current(), Code::Io);
                assert!(reason.io_kind().is_some());
                assert!(reason.raw_os_error().is_some());
                loadable(report.previous());
            }
            _ => {
                assert_eq!(
                    rejected(report.current(), Code::NativeFormat).message(),
                    "native save format: save file budget overflow"
                );
                rejected(report.previous(), Code::NativeFormat);
            }
        }
        assert_eq!(Arc::strong_count(&catalogue), 1);
        drop(held_slot);
        assert_eq!(availability_entries(repository.path()), before);
    }
}
#[test]
fn availability_task_revalidates_marker_without_writer_lock_and_rejects_wrong_current_identity() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, _) = setup(root.path());
    let catalogue = Arc::new(catalogue);
    let before = directory_bytes(repository.path());
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.lock().unwrap();
    let report = async_availability(&repository, Arc::clone(&catalogue), Limits::default());
    loadable(report.current());
    loadable(report.previous());
    drop(lock);
    assert_eq!(directory_bytes(repository.path()), before);
    for current in [
        save::AvailabilityRequest::new(
            2.try_into().unwrap(),
            repository.campaign(),
            report.catalogue_fingerprint(),
        )
        .unwrap(),
        save::AvailabilityRequest::new(
            1.try_into().unwrap(),
            CampaignId::from_bytes([0x33; 16]).unwrap(),
            report.catalogue_fingerprint(),
        )
        .unwrap(),
        save::AvailabilityRequest::new(
            1.try_into().unwrap(),
            repository.campaign(),
            &"f".repeat(64),
        )
        .unwrap(),
    ] {
        let request = availability_request(&repository, &catalogue, 1);
        let mut task = save::AvailabilityTask::start(
            repository.clone(),
            Arc::clone(&catalogue),
            Limits::default(),
            request,
        )
        .unwrap();
        task.finish().unwrap();
        let save::AvailabilityPoll::Ready(candidate) = task.try_poll() else {
            panic!("report required")
        };
        assert!(matches!(
            candidate.take_for(&current),
            Err(save::AvailabilityError::Superseded)
        ));
        assert_eq!(directory_bytes(repository.path()), before);
    }
    fs::write(
        repository.path().join(".rust-fallout-saves"),
        b"invalid marker",
    )
    .unwrap();
    let before = directory_bytes(repository.path());
    let request = availability_request(&repository, &catalogue, 1);
    let mut task =
        save::AvailabilityTask::start(repository.clone(), catalogue, Limits::default(), request)
            .unwrap();
    task.finish().unwrap();
    assert!(
        matches!(task.try_poll(),save::AvailabilityPoll::Failed(error) if matches!(&*error,save::AvailabilityError::Save(_)))
    );
    assert_eq!(directory_bytes(repository.path()), before);
}
#[test]
fn availability_task_native_source_invalid_report_matches_fresh_worker_without_slot_writes() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_AVAILABILITY_TASK_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fs::create_dir_all(root).unwrap();
    let (catalogue, repository, [first, second]) = setup(root);
    let catalogue = Arc::new(catalogue);
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    let valid = async_availability(&repository, Arc::clone(&catalogue), Limits::default());
    let wire = fs::read(repository.path().join("current.frsv")).unwrap();
    fs::write(root.join("valid.current.frsv"), &wire).unwrap();
    fs::write(
        root.join("expected.previous.json"),
        first.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.current.json"),
        second.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("valid.report.json"),
        serde_json::to_vec_pretty(&valid).unwrap(),
    )
    .unwrap();
    let invalid = wrong_local(&wire);
    format::decode(&invalid, Limits::default()).unwrap();
    fs::write(repository.path().join("current.frsv"), invalid).unwrap();
    let report = async_availability(&repository, Arc::clone(&catalogue), Limits::default());
    assert_eq!(
        rejected(report.current(), Code::IncompatibleLocal).local_index(),
        Some(42)
    );
    loadable(report.previous());
    fs::write(
        root.join("invalid.report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let before = directory_bytes(repository.path());
    let weak = Arc::downgrade(&catalogue);
    drop(catalogue);
    assert!(weak.upgrade().is_none());
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_availability_task_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_AVAILABILITY_TASK_COLD_ROOT", root)
        .output()
        .unwrap();
    fs::write(root.join("cold.stdout.txt"), &child.stdout).unwrap();
    fs::write(root.join("cold.stderr.txt"), &child.stderr).unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(directory_bytes(repository.path()), before);
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}
#[test]
#[ignore = "fresh read-only source-bound asynchronous observer invoked by parent"]
fn cold_availability_task_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_AVAILABILITY_TASK_COLD_ROOT").unwrap());
    let catalogue = Arc::new(load(&root, &["FalloutNV.esm"]));
    let weak = Arc::downgrade(&catalogue);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let before = directory_bytes(repository.path());
    let report = async_availability(&repository, catalogue, Limits::default());
    assert!(weak.upgrade().is_none());
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(root.join("invalid.report.json")).unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        rejected(report.current(), Code::IncompatibleLocal).local_index(),
        Some(42)
    );
    loadable(report.previous());
    fs::write(
        root.join("cold.task.report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert_eq!(directory_bytes(repository.path()), before);
}

fn seed(catalogue: &Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x20; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x100))).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Null],
    };
    let stage = world
        .stage_instance_initialization(
            &definition(catalogue),
            &Owner::Placed { reference },
            &context,
            &[
                (
                    2,
                    Value::Number {
                        bits: 0x8000_0000_0000_0000,
                    },
                ),
                (42, Value::Number { bits: 1 }),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    },
                ),
            ],
            initialization::Limits::default(),
        )
        .unwrap();
    let (_, handle) = world.commit_instance_initialization(stage).unwrap();
    world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    world
}
fn setup(root: &Path) -> (Catalogue, Repository, [Snapshot; 2]) {
    write_fixture(root, false);
    let catalogue = load(root, &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let first = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    world
        .assign(
            world.handle(first.instances[0].id).unwrap(),
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
        )
        .unwrap();
    let second = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    (catalogue, repository, [first, second])
}
fn directory_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            (
                entry.file_name().to_str().unwrap().into(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
fn loadable(slot: &Availability) -> &format::Metadata {
    match slot {
        Availability::Loadable { metadata } => metadata,
        _ => panic!("expected loadable: {slot:?}"),
    }
}
fn rejected(slot: &Availability, code: Code) -> &save::SlotRejection {
    match slot {
        Availability::Rejected { reason } => {
            assert_eq!(reason.code(), code);
            reason
        }
        _ => panic!("expected rejected: {slot:?}"),
    }
}
// Author a modified native wire independently of Captured; update every extent,
// metadata binding and checksum. Production format/restore remain the readers.
fn repack(bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let decoded = format::decode(bytes, Limits::default()).unwrap();
    let mut snapshot = serde_json::to_value(decoded.snapshot).unwrap();
    edit(&mut snapshot);
    let body = serde_json::to_vec(&snapshot).unwrap();
    let mut out = bytes[..200].to_vec();
    out[68..72]
        .copy_from_slice(&(snapshot["schema_version"].as_u64().unwrap() as u32).to_le_bytes());
    out[80..88].copy_from_slice(&snapshot["clocks"]["tick"].as_u64().unwrap().to_le_bytes());
    out[120..128].copy_from_slice(&(body.len() as u64).to_le_bytes());
    for (index, byte) in snapshot["campaign"].as_array().unwrap().iter().enumerate() {
        out[128 + index] = byte.as_u64().unwrap().try_into().unwrap();
    }
    out[144..152].copy_from_slice(&snapshot["state_revision"].as_u64().unwrap().to_le_bytes());
    out[160..168].copy_from_slice(&(body.len() as u64).to_le_bytes());
    let meta_sha = Sha256::digest(&out[64..152]);
    out[32..64].copy_from_slice(&meta_sha);
    out[168..200].copy_from_slice(&Sha256::digest(&body));
    out.extend(body);
    let container_sha = Sha256::digest(&out);
    out.extend(container_sha);
    out
}
fn wrong_local(bytes: &[u8]) -> Vec<u8> {
    repack(bytes, |snapshot| {
        let local = snapshot["instances"][0]["locals"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|local| local["index"] == 42)
            .unwrap();
        local["value"] = json!({"kind":"reference","value":{"kind":"null"}});
    })
}

#[test]
fn two_valid_slots_are_distinct_source_bound_observations_with_no_retained_catalogue_or_writes() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, [first, second]) = setup(root.path());
    let catalogue = Arc::new(catalogue);
    let weak = Arc::downgrade(&catalogue);
    let before = directory_bytes(repository.path());
    let report = repository
        .inspect_availability(Arc::clone(&catalogue), Limits::default())
        .unwrap();
    assert_eq!(report.campaign(), first.campaign);
    assert_eq!(report.catalogue_fingerprint(), first.catalogue_sha256);
    assert_eq!(loadable(report.current()).generation, 2);
    assert_eq!(
        loadable(report.current()).state_revision,
        second.state_revision
    );
    assert_eq!(loadable(report.previous()).generation, 1);
    assert_eq!(
        loadable(report.previous()).state_revision,
        first.state_revision
    );
    assert_eq!(directory_bytes(repository.path()), before);
    assert_eq!(weak.strong_count(), 1);
    drop(catalogue);
    assert!(weak.upgrade().is_none());
    assert_eq!(loadable(report.current()).generation, 2); // owned metadata only
    assert!(serde_json::to_vec(&report).unwrap().len() < 16 * 1024);
}

#[test]
fn rehashed_source_schema_invalid_current_is_rejected_while_valid_previous_stays_loadable() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, [first, _]) = setup(root.path());
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let bad = wrong_local(&current);
    let decoded = format::decode(&bad, Limits::default()).unwrap();
    assert!(matches!(
        World::restore(&catalogue, decoded.snapshot, Limits::default()),
        Err(Error::IncompatibleLocal(42))
    ));
    fs::write(repository.path().join("current.frsv"), &bad).unwrap();
    let before = directory_bytes(repository.path());
    let report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    let reason = rejected(report.current(), Code::IncompatibleLocal);
    assert_eq!(reason.local_index(), Some(42));
    assert!(!reason.truncated());
    assert_eq!(
        loadable(report.previous()).state_revision,
        first.state_revision
    );
    assert_eq!(directory_bytes(repository.path()), before);
    assert!(matches!(
        repository.load(&catalogue, Limits::default(), Recovery::Strict),
        Err(save::Error::State(Error::IncompatibleLocal(42)))
    ));
    let (world, receipt) = repository
        .load(
            &catalogue,
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(world.snapshot(), first);
    assert_eq!(receipt.slot, Slot::Previous);
    assert!(!receipt.current_repaired);
    assert_eq!(directory_bytes(repository.path()), before);
}

#[test]
fn missing_corrupt_foreign_campaign_changed_source_and_explicit_migration_need_distinct_results() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, _) = setup(root.path());
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let empty = Repository::create(
        &root.path().join("empty"),
        &[],
        CampaignId::from_bytes([0x20; 16]).unwrap(),
    )
    .unwrap();
    let empty_bytes = directory_bytes(empty.path());
    let report = empty
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    assert_eq!(report.current(), &Availability::Missing);
    assert_eq!(report.previous(), &Availability::Missing);
    assert_eq!(directory_bytes(empty.path()), empty_bytes);
    fs::write(repository.path().join("current.frsv"), b"truncated").unwrap();
    let before = directory_bytes(repository.path());
    let report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    rejected(report.current(), Code::NativeFormat);
    loadable(report.previous());
    assert_eq!(directory_bytes(repository.path()), before);
    let foreign = repack(&current, |snapshot| {
        snapshot["campaign"] = json!(vec![0x21_u8; 16])
    });
    format::decode(&foreign, Limits::default()).unwrap();
    fs::write(repository.path().join("current.frsv"), &foreign).unwrap();
    let before = directory_bytes(repository.path());
    let report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    assert_eq!(
        rejected(report.current(), Code::NativeFormat).message(),
        "native save format: save campaign differs from repository identity"
    );
    loadable(report.previous());
    assert_eq!(directory_bytes(repository.path()), before);
    fs::write(repository.path().join("current.frsv"), &current).unwrap();
    let before = directory_bytes(repository.path());
    let changed = load(root.path(), &["FalloutNV.esm", "Other.esm"]);
    let report = repository
        .inspect_availability(&changed, Limits::default())
        .unwrap();
    rejected(report.current(), Code::DefinitionChanged);
    rejected(report.previous(), Code::DefinitionChanged);
    assert_eq!(directory_bytes(repository.path()), before);
    let legacy = repack(&current, |snapshot| {
        snapshot["schema_version"] = json!(3);
        snapshot.as_object_mut().unwrap().remove("reference_states");
    });
    assert!(format::decode(&legacy, Limits::default()).is_err());
    let migrated = format::migrate_v3(&legacy, Limits::default()).unwrap();
    World::restore(&catalogue, migrated.snapshot, Limits::default()).unwrap();
    fs::write(repository.path().join("current.frsv"), &legacy).unwrap();
    let before = directory_bytes(repository.path());
    let report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    rejected(report.current(), Code::NativeFormat);
    loadable(report.previous());
    assert_eq!(directory_bytes(repository.path()), before);
}

#[test]
fn exact_slot_bytes_and_restore_budgets_reject_without_changing_available_peer_or_repository() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, _) = setup(root.path());
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let length = format::decode(&current, Limits::default())
        .unwrap()
        .metadata
        .snapshot_bytes;
    let before = directory_bytes(repository.path());
    let exact = Limits {
        max_snapshot_bytes: length,
        max_instances: 1,
        max_references: 1,
        max_locals: 3,
        max_pending_events: 1,
        max_event_blocks: 1,
        ..Limits::default()
    };
    let report = repository.inspect_availability(&catalogue, exact).unwrap();
    loadable(report.current());
    loadable(report.previous());
    let report = repository
        .inspect_availability(
            &catalogue,
            Limits {
                max_snapshot_bytes: length - 1,
                ..exact
            },
        )
        .unwrap();
    assert_eq!(
        rejected(report.current(), Code::NativeFormat).message(),
        "native save format: save file exceeds byte budget"
    );
    loadable(report.previous());
    let report = repository
        .inspect_availability(
            &catalogue,
            Limits {
                max_locals: 2,
                ..exact
            },
        )
        .unwrap();
    assert_eq!(
        rejected(report.current(), Code::RuntimeCapacity).budget(),
        Some("saved locals")
    );
    rejected(report.previous(), Code::RuntimeCapacity);
    let report = repository
        .inspect_availability(
            &catalogue,
            Limits {
                max_snapshot_bytes: usize::MAX,
                ..exact
            },
        )
        .unwrap();
    assert_eq!(
        rejected(report.current(), Code::NativeFormat).message(),
        "native save format: save file budget overflow"
    );
    rejected(report.previous(), Code::NativeFormat);
    assert_eq!(directory_bytes(repository.path()), before);
}

#[test]
fn inspection_observes_slots_while_writer_is_locked_and_revalidates_marker_without_touching_lock() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, _) = setup(root.path());
    let before = directory_bytes(repository.path());
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.lock().unwrap();
    let report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    loadable(report.current());
    loadable(report.previous());
    // Windows forbids reading the locked bytes through another handle. Observe
    // all other files while held, then verify the complete lock bytes on release.
    for name in ["current.frsv", "previous.frsv", ".rust-fallout-saves"] {
        assert_eq!(
            fs::read(repository.path().join(name)).unwrap(),
            before[name]
        );
    }
    assert_eq!(
        fs::read_dir(repository.path()).unwrap().count(),
        before.len()
    );
    assert!(matches!(
        repository.load(&catalogue, Limits::default(), Recovery::Strict),
        Err(save::Error::Busy)
    ));
    drop(lock);
    assert_eq!(directory_bytes(repository.path()), before);
    let marker = repository.path().join(".rust-fallout-saves");
    fs::write(&marker, b"changed marker").unwrap();
    let before = directory_bytes(repository.path());
    assert!(
        repository
            .inspect_availability(&catalogue, Limits::default())
            .is_err()
    );
    assert_eq!(directory_bytes(repository.path()), before);
}

#[test]
fn source_bound_availability_is_equal_in_fresh_consumer_and_never_repairs_invalid_current() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_SLOT_AVAILABILITY_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fs::create_dir_all(root).unwrap();
    let (catalogue, repository, [first, second]) = setup(root);
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let valid = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    fs::write(
        root.join("valid.report.json"),
        serde_json::to_vec_pretty(&valid).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.previous.json"),
        first.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.current.json"),
        second.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(root.join("valid.current.frsv"), &current).unwrap();
    let invalid = wrong_local(&current);
    format::decode(&invalid, Limits::default()).unwrap();
    fs::write(repository.path().join("current.frsv"), &invalid).unwrap();
    let report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    rejected(report.current(), Code::IncompatibleLocal);
    loadable(report.previous());
    fs::write(
        root.join("invalid.report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let before = directory_bytes(repository.path());
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(catalogue);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_slot_availability_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_SLOT_AVAILABILITY_COLD_ROOT", root)
        .output()
        .unwrap();
    fs::write(root.join("cold.stdout.txt"), &child.stdout).unwrap();
    fs::write(root.join("cold.stderr.txt"), &child.stderr).unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(directory_bytes(repository.path()), before);
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}

#[test]
#[ignore = "fresh source-bound slot observer invoked by its parent"]
fn cold_slot_availability_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_SLOT_AVAILABILITY_COLD_ROOT").unwrap());
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let before = directory_bytes(repository.path());
    let report: Report = repository
        .inspect_availability(&catalogue, Limits::default())
        .unwrap();
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(root.join("invalid.report.json")).unwrap()
        )
        .unwrap()
    );
    rejected(report.current(), Code::IncompatibleLocal);
    loadable(report.previous());
    fs::write(
        root.join("cold.report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join("expected.previous.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let (world, receipt) = repository
        .load(
            &catalogue,
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(world.snapshot(), expected);
    assert_eq!(receipt.slot, Slot::Previous);
    assert!(!receipt.current_repaired);
    assert_eq!(directory_bytes(repository.path()), before);
}
