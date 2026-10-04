mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, Value},
    save::{self, Captured, Recovery, Repository, SaveWorker, Slot, Stage, format},
    snapshot::Snapshot,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn seed<'a>(catalogue: &'a fallout_data::loaded_scripts::Catalogue) -> World<'a> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x29; 16]).unwrap(),
    )
    .unwrap();
    let handle = world
        .create_instance(
            &definition(catalogue),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 1 })])
        .unwrap();
    world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    world
}
fn set_number(world: &mut World<'_>, bits: u64) {
    let id = world.snapshot().instances[0].id;
    let handle = world.handle(id).unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits })])
        .unwrap();
}
fn number(world: &World<'_>) -> u64 {
    let snapshot = world.snapshot();
    let handle = world.handle(snapshot.instances[0].id).unwrap();
    let Value::Number { bits } = world.instance(handle).unwrap().local(42).unwrap() else {
        panic!("numeric fixture");
    };
    *bits
}
fn repo(path: &Path, world: &World<'_>) -> Repository {
    Repository::create(path, &[], world.campaign()).unwrap()
}

#[test]
fn capture_is_owned_and_serializes_on_a_worker_without_reading_later_mutations() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository = repo(&directory.path().join("native"), &world);
    let capture = Captured::at_boundary(&world);
    let expected = capture.snapshot().clone();
    let worker_repository = repository.clone();
    let worker = std::thread::spawn(move || worker_repository.commit(&capture).unwrap());
    set_number(&mut world, 0x7ff8123456789abc);
    assert_eq!(worker.join().unwrap().metadata.generation, 1);
    let (restored, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.slot, Slot::Current);
    assert_eq!(restored.snapshot(), expected);
    assert_eq!(number(&restored), 1);
    assert_eq!(number(&world), 0x7ff8123456789abc);
}

#[test]
fn repeated_commits_keep_a_verified_previous_slot_and_report_recovery_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository = repo(&directory.path().join("native"), &world);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    set_number(&mut world, 2);
    let old = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    set_number(&mut world, 3);
    let receipt = repository.commit(&Captured::at_boundary(&world)).unwrap();
    assert_eq!(receipt.metadata.generation, 3);
    assert_eq!(receipt.previous_generation, Some(2));
    fs::write(repository.path().join("current.frsv"), b"truncated").unwrap();
    assert!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .is_err()
    );
    let (restored, receipt) = repository
        .load(
            &catalogue,
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(restored.snapshot(), old);
    assert_eq!(receipt.slot, Slot::Previous);
    assert!(receipt.current_failure.is_some());
    assert!(!receipt.current_repaired);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        b"truncated"
    );
    assert!(repository.commit(&Captured::at_boundary(&world)).is_err());
    let (mut restored, receipt) = repository
        .recover_previous(&catalogue, Limits::default())
        .unwrap();
    assert!(receipt.current_repaired);
    assert_eq!(number(&restored), 2);
    assert!(
        repository
            .recover_previous(&catalogue, Limits::default())
            .is_err()
    );
    set_number(&mut restored, 4);
    assert_eq!(
        repository
            .commit(&Captured::at_boundary(&restored))
            .unwrap()
            .metadata
            .generation,
        3
    );
    assert_eq!(
        number(
            &repository
                .load(&catalogue, Limits::default(), Recovery::Strict)
                .unwrap()
                .0
        ),
        4
    );
}

#[test]
fn stale_requests_and_other_campaigns_cannot_replace_current_state() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository = repo(&directory.path().join("native"), &world);
    let old = Captured::at_boundary(&world);
    repository.commit(&old).unwrap();
    set_number(&mut world, 2);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let before = fs::read(repository.path().join("current.frsv")).unwrap();
    assert!(repository.commit(&old).is_err());
    // Two branches can reach the same revision with different values. Revision
    // equality alone must not let the losing branch replace the current save.
    let mut branch = World::restore(&catalogue, old.snapshot().clone(), Limits::default()).unwrap();
    set_number(&mut branch, 3);
    assert_eq!(branch.revision(), world.revision());
    assert!(repository.commit(&Captured::at_boundary(&branch)).is_err());
    let other = World::new(&catalogue, Limits::default()).unwrap();
    assert_ne!(other.campaign(), world.campaign());
    assert!(repository.commit(&Captured::at_boundary(&other)).is_err());
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        before
    );
    let foreign = repo(&directory.path().join("foreign"), &other);
    foreign.commit(&Captured::at_boundary(&other)).unwrap();
    fs::copy(
        foreign.path().join("current.frsv"),
        repository.path().join("current.frsv"),
    )
    .unwrap();
    assert!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .is_err()
    );
}

#[test]
fn original_directories_and_unknown_existing_folders_are_not_adopted() {
    let directory = tempfile::tempdir().unwrap();
    let inputs = directory.path().join("original");
    fs::create_dir(&inputs).unwrap();
    let campaign = CampaignId::from_bytes([1; 16]).unwrap();
    assert!(
        Repository::create(
            &inputs.join("native"),
            std::slice::from_ref(&inputs),
            campaign
        )
        .is_err()
    );
    assert!(!inputs.join("native").exists());
    fs::write(inputs.join("original.fos"), b"untouched").unwrap();
    assert!(Repository::open(&inputs, &[]).is_err());
    assert!(Repository::create(&inputs, &[], campaign).is_err());
    assert_eq!(fs::read(inputs.join("original.fos")).unwrap(), b"untouched");
    let native = Repository::create(&directory.path().join("native"), &[inputs], campaign).unwrap();
    fs::write(native.path().join(".rust-fallout-saves"), b"wrong marker").unwrap();
    assert!(Repository::open(native.path(), &[]).is_err());
}

#[test]
fn unsupported_or_corrupt_previous_state_cannot_be_silently_restored() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository = repo(&directory.path().join("native"), &world);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    set_number(&mut world, 2);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    fs::write(repository.path().join("current.frsv"), b"broken current").unwrap();
    fs::write(repository.path().join("previous.frsv"), b"broken previous").unwrap();
    let before = world.snapshot();
    assert!(
        repository
            .load(
                &catalogue,
                Limits::default(),
                Recovery::PreviousIfCurrentInvalid
            )
            .is_err()
    );
    assert!(
        repository
            .recover_previous(&catalogue, Limits::default())
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        b"broken current"
    );
}

#[test]
fn blocked_backup_publication_preserves_current_and_cleans_owned_temporaries() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository = repo(&directory.path().join("native"), &world);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let before = fs::read(repository.path().join("current.frsv")).unwrap();
    set_number(&mut world, 2);
    fs::create_dir(repository.path().join("previous.frsv")).unwrap();
    assert!(repository.commit(&Captured::at_boundary(&world)).is_err());
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        before
    );
    assert_eq!(
        number(
            &repository
                .load(&catalogue, Limits::default(), Recovery::Strict)
                .unwrap()
                .0
        ),
        1
    );
    assert!(!fs::read_dir(repository.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pending-")
    }));
}

#[test]
fn checksummed_chunks_reject_corruption_unknown_versions_and_forged_extents() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = seed(&catalogue);
    let bytes = format::encode(&Captured::at_boundary(&world), 7).unwrap();
    let decoded = format::decode(&bytes, Limits::default()).unwrap();
    assert_eq!(decoded.snapshot, world.snapshot());
    assert_eq!(decoded.metadata.generation, 7);
    assert_eq!(
        decoded.metadata.container_bytes,
        decoded.metadata.snapshot_bytes + format::OVERHEAD
    );
    for position in [
        0,
        8,
        12,
        16,
        20,
        24,
        32,
        64,
        80,
        100,
        128,
        152,
        160,
        180,
        bytes.len() - 33,
        bytes.len() - 1,
    ] {
        let mut bad = bytes.clone();
        bad[position] ^= 1;
        assert!(format::decode(&bad, Limits::default()).is_err());
    }
    for length in [0, 7, 15, 63, 151, 199, 231, bytes.len() - 1] {
        assert!(format::decode(&bytes[..length], Limits::default()).is_err());
    }
    let whole = |bad: &mut Vec<u8>| {
        let end = bad.len() - 32;
        let digest = Sha256::digest(&bad[..end]);
        bad[end..].copy_from_slice(&digest);
    };
    let meta = |bad: &mut Vec<u8>| {
        let digest = Sha256::digest(&bad[64..152]);
        bad[32..64].copy_from_slice(&digest);
        whole(bad);
    };
    for (offset, data) in [
        (8, 2_u16.to_le_bytes().to_vec()),
        (10, 1_u16.to_le_bytes().to_vec()),
        (12, 3_u32.to_le_bytes().to_vec()),
        (16, b"UNKN".to_vec()),
        (20, 2_u32.to_le_bytes().to_vec()),
        (24, u64::MAX.to_le_bytes().to_vec()),
        (152, b"UNKN".to_vec()),
        (156, 2_u32.to_le_bytes().to_vec()),
        (160, u64::MAX.to_le_bytes().to_vec()),
    ] {
        let mut bad = bytes.clone();
        bad[offset..offset + data.len()].copy_from_slice(&data);
        whole(&mut bad);
        assert!(
            format::decode(&bad, Limits::default()).is_err(),
            "header field {offset}"
        );
    }
    for offset in [64, 68, 72, 80, 88, 120, 128, 144] {
        let mut bad = bytes.clone();
        if offset == 72 {
            bad[72..80].fill(0);
        } else {
            bad[offset] ^= 1;
        }
        meta(&mut bad);
        assert!(
            format::decode(&bad, Limits::default()).is_err(),
            "metadata field {offset}"
        );
    }
    let mut trailing = bytes[..bytes.len() - 32].to_vec();
    trailing.extend(b"extra");
    trailing.extend([0; 32]);
    whole(&mut trailing);
    assert!(format::decode(&trailing, Limits::default()).is_err());
    assert!(
        format::decode(
            &bytes,
            Limits {
                max_snapshot_bytes: 10,
                ..Limits::default()
            }
        )
        .is_err()
    );
}

#[test]
fn legacy_snapshot_migration_preserves_state_and_requires_explicit_campaign() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = seed(&catalogue);
    let mut old = serde_json::to_value(world.snapshot()).unwrap();
    let fields = old.as_object_mut().unwrap();
    fields.remove("campaign");
    fields.remove("state_revision");
    fields.remove("next_item");
    fields.remove("inventory_banks");
    fields.insert("schema_version".into(), 1.into());
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(Snapshot::decode(&bytes, Limits::default()).is_err());
    let migrated = Snapshot::migrate_v1(&bytes, Limits::default(), world.campaign()).unwrap();
    let mut expected = world.snapshot();
    expected.state_revision = 0;
    assert_eq!(migrated, expected);
    assert_eq!(
        World::restore(&catalogue, migrated, Limits::default())
            .unwrap()
            .snapshot(),
        expected
    );
    old["unknown"] = true.into();
    assert!(
        Snapshot::migrate_v1(
            &serde_json::to_vec(&old).unwrap(),
            Limits::default(),
            world.campaign()
        )
        .is_err()
    );
    old.as_object_mut().unwrap().remove("unknown");
    old["schema_version"] = 3.into();
    assert!(
        Snapshot::migrate_v1(
            &serde_json::to_vec(&old).unwrap(),
            Limits::default(),
            world.campaign()
        )
        .is_err()
    );
}

#[test]
fn revision_exhaustion_rejects_mutations_without_partial_state() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = seed(&catalogue);
    let mut snapshot = world.snapshot();
    snapshot.state_revision = u64::MAX;
    let mut world = World::restore(&catalogue, snapshot.clone(), Limits::default()).unwrap();
    let handle = world.handle(snapshot.instances[0].id).unwrap();
    assert!(
        world
            .assign(handle, &[(42, Value::Number { bits: 2 })])
            .is_err()
    );
    assert!(world.register_reference(None).is_err());
    assert!(
        world
            .enqueue(handle, Trigger::ObjectEvent { mask: 1 }, Context::default())
            .is_err()
    );
    assert!(
        world
            .acknowledge(snapshot.pending_events[0].sequence)
            .is_err()
    );
    assert_eq!(world.snapshot(), snapshot);
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn child(root: &Path, mode: &str, ready: &Path) -> ChildGuard {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "native_save_child", "--nocapture"])
        .env("FALLOUT_SAVE_TEST_ROOT", root)
        .env("FALLOUT_SAVE_TEST_MODE", mode)
        .env("FALLOUT_SAVE_TEST_READY", ready)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    ChildGuard(command.spawn().unwrap())
}
fn await_ready(ready: &Path, child: &mut ChildGuard) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "child did not reach save stage");
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "save child exited early"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn killed_writers_release_locks_and_leave_complete_old_or_new_slots() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    for (index, stage) in [
        Stage::CurrentTempWritten,
        Stage::CurrentTempSynced,
        Stage::PreviousTempSynced,
        Stage::PreviousPublished,
        Stage::CurrentPublished,
    ]
    .into_iter()
    .enumerate()
    {
        let world = seed(&catalogue);
        let root = directory.path().join(format!("native-{index}"));
        let repository = repo(&root, &world);
        repository.commit(&Captured::at_boundary(&world)).unwrap();
        let ready = directory.path().join(format!("ready-{index}"));
        let mut writer = child(directory.path(), &format!("kill:{index}"), &ready);
        await_ready(&ready, &mut writer);
        assert!(matches!(
            repository.commit(&Captured::at_boundary(&world)),
            Err(save::Error::Busy)
        ));
        writer.0.kill().unwrap();
        writer.0.wait().unwrap();
        let (restored, receipt) = repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap();
        assert_eq!(
            number(&restored),
            if stage == Stage::CurrentPublished {
                2
            } else {
                1
            }
        );
        assert_eq!(
            receipt.metadata.generation,
            if stage == Stage::CurrentPublished {
                2
            } else {
                1
            }
        );
        let next = Captured::at_boundary(&restored);
        assert!(repository.commit(&next).is_ok());
    }
}

#[test]
fn cold_process_reopens_snapshot_with_context_and_exact_values() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = seed(&catalogue);
    let repository = repo(&directory.path().join("native-cold"), &world);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let ready = directory.path().join("cold-success");
    let mut reader = child(directory.path(), "cold", &ready);
    await_ready(&ready, &mut reader);
    assert!(reader.0.wait().unwrap().success());
}

fn staged_cold_seed(catalogue: &fallout_data::loaded_scripts::Catalogue) -> World<'_> {
    let mut world = seed(catalogue);
    let reference = world.register_reference(Some(form(0x100))).unwrap();
    let handle = world.handle(world.snapshot().instances[0].id).unwrap();
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: fallout_runtime::identity::ReferenceValue::Live { id: reference },
                },
            )],
        )
        .unwrap();
    world.acknowledge(1).unwrap();
    world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context {
                calling_reference: Some(reference),
                target: Some(fallout_runtime::identity::ReferenceValue::Live { id: reference }),
                arguments: vec![fallout_runtime::identity::ReferenceValue::Live { id: reference }],
                ..Context::default()
            },
        )
        .unwrap();
    world
}

#[test]
fn staged_commit_cold_boundaries_preserve_pending_work_and_reference_links() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = staged_cold_seed(&catalogue);
    let before = world.snapshot();
    let stage = world
        .stage_event_changes(
            2,
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
            true,
        )
        .unwrap();
    let before_repository = repo(&directory.path().join("native-stage-before"), &world);
    before_repository
        .commit(&Captured::at_boundary(&world))
        .unwrap();
    assert_eq!(world.snapshot(), before);
    let receipt = world.commit_event_changes(stage).unwrap();
    assert_eq!(receipt.after_revision, before.state_revision + 1);
    let after_repository = repo(&directory.path().join("native-stage-after"), &world);
    after_repository
        .commit(&Captured::at_boundary(&world))
        .unwrap();
    for mode in ["before", "after"] {
        let ready = directory.path().join(format!("stage-{mode}-success"));
        let mut reader = child(directory.path(), &format!("cold-stage:{mode}"), &ready);
        await_ready(&ready, &mut reader);
        assert!(reader.0.wait().unwrap().success());
    }
}

const STAGED_SAVE_STAGES: [Stage; 5] = [
    Stage::CurrentTempWritten,
    Stage::CurrentTempSynced,
    Stage::PreviousTempSynced,
    Stage::PreviousPublished,
    Stage::CurrentPublished,
];

fn expected_staged_after(mut before: Snapshot) -> Snapshot {
    before.state_revision += 1;
    before.pending_events.remove(0);
    before.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = Value::Number {
        bits: 0x7ff8_1234_5678_9abc,
    };
    before
}

fn staged_publication_fixture() -> (Option<tempfile::TempDir>, PathBuf) {
    match std::env::var_os("FALLOUT_STAGED_INTERRUPTION_EVIDENCE") {
        Some(root) => {
            let root = PathBuf::from(root).join("authored");
            fs::create_dir(&root).unwrap();
            (None, root)
        }
        None => {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_path_buf();
            (Some(directory), root)
        }
    }
}

fn copy_stage_repository(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
    }
}

#[test]
fn interrupted_staged_publication_cold_restores_a_whole_boundary_at_every_stage() {
    let (_temporary, root) = staged_publication_fixture();
    write_fixture(&root, false);
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let mut rows = Vec::new();
    for (index, stage) in STAGED_SAVE_STAGES.into_iter().enumerate() {
        let world = staged_cold_seed(&catalogue);
        let before = world.snapshot();
        let repository = repo(&root.join(format!("native-stage-kill-{index}")), &world);
        let before_bytes = format::encode(&Captured::at_boundary(&world), 1).unwrap();
        let after_world = World::restore(
            &catalogue,
            expected_staged_after(before.clone()),
            Limits::default(),
        )
        .unwrap();
        let after_bytes = format::encode(&Captured::at_boundary(&after_world), 2).unwrap();
        let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
        let saved = worker.try_submit(Captured::at_boundary(&world)).unwrap();
        worker.finish().unwrap();
        assert_eq!(saved.wait().unwrap().metadata.generation, 1);
        assert_eq!(
            fs::read(repository.path().join("current.frsv")).unwrap(),
            before_bytes
        );
        let ready = root.join(format!("stage-kill-ready-{index}"));
        let mut writer = child(&root, &format!("kill-stage:{index}"), &ready);
        await_ready(&ready, &mut writer);
        assert!(matches!(
            repository.load(&catalogue, Limits::default(), Recovery::Strict),
            Err(save::Error::Busy)
        ));
        writer.0.kill().unwrap();
        writer.0.wait().unwrap();
        let (restored, receipt) = repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap();
        let published = stage == Stage::CurrentPublished;
        let expected = if published {
            expected_staged_after(before.clone())
        } else {
            before.clone()
        };
        assert_eq!(restored.snapshot(), expected);
        assert_eq!(receipt.metadata.generation, if published { 2 } else { 1 });
        assert_eq!(
            fs::read(repository.path().join("current.frsv")).unwrap(),
            if published {
                &after_bytes
            } else {
                &before_bytes
            }
            .as_slice()
        );
        assert_eq!(repository.path().join("previous.frsv").exists(), index >= 3);
        if index >= 3 {
            assert_eq!(
                fs::read(repository.path().join("previous.frsv")).unwrap(),
                before_bytes
            );
            assert_eq!(
                format::decode(
                    &fs::read(repository.path().join("previous.frsv")).unwrap(),
                    Limits::default()
                )
                .unwrap()
                .snapshot,
                before
            );
        }
        let observed = root.join(format!("observed-stage-{index}"));
        copy_stage_repository(repository.path(), &observed);
        let cold_ready = root.join(format!("stage-kill-cold-{index}"));
        let mut cold = child(
            &root,
            &format!(
                "cold-killed-stage:{index}:{}",
                if published { "after" } else { "before" }
            ),
            &cold_ready,
        );
        await_ready(&cold_ready, &mut cold);
        assert!(cold.0.wait().unwrap().success());
        if index >= 3 {
            let fallback = root.join(format!("fallback-stage-{index}"));
            copy_stage_repository(repository.path(), &fallback);
            fs::write(fallback.join("current.frsv"), b"truncated").unwrap();
            let cold_ready = root.join(format!("stage-fallback-cold-{index}"));
            let mut cold = child(
                &root,
                &format!("cold-killed-fallback:{index}:before"),
                &cold_ready,
            );
            await_ready(&cold_ready, &mut cold);
            assert!(cold.0.wait().unwrap().success());
            assert_eq!(
                fs::read(fallback.join("current.frsv")).unwrap(),
                b"truncated"
            );
            assert_eq!(
                fs::read(fallback.join("previous.frsv")).unwrap(),
                before_bytes
            );
        }
        let mut resumed = SaveWorker::start(repository.clone(), 1).unwrap();
        let saved = resumed
            .try_submit(Captured::at_boundary(&restored))
            .unwrap();
        resumed.finish().unwrap();
        assert_eq!(
            saved.wait().unwrap().metadata.generation,
            if published { 3 } else { 2 }
        );
        assert_eq!(
            repository
                .load(&catalogue, Limits::default(), Recovery::Strict)
                .unwrap()
                .0
                .snapshot(),
            expected
        );
        let resumed = format::decode(
            &fs::read(repository.path().join("current.frsv")).unwrap(),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(
            fs::read(repository.path().join("previous.frsv")).unwrap(),
            if published {
                &after_bytes
            } else {
                &before_bytes
            }
            .as_slice()
        );
        rows.push(serde_json::json!({
            "stage":format!("{stage:?}"), "index":index,
            "before":format::decode(&before_bytes, Limits::default()).unwrap().metadata,
            "after":format::decode(&after_bytes, Limits::default()).unwrap().metadata,
            "interrupted_current":receipt.metadata, "previous_published":index >= 3,
            "resumed_current":resumed.metadata,
            "cold_full_state_equal":true, "cold_fallback_verified":index >= 3
        }));
    }
    fs::write(
        root.join("interruption-receipt.json"),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "helper launched only by native save process tests"]
fn native_save_child() {
    let root = PathBuf::from(std::env::var_os("FALLOUT_SAVE_TEST_ROOT").expect("child root"));
    let mode = std::env::var("FALLOUT_SAVE_TEST_MODE").unwrap();
    let ready = PathBuf::from(std::env::var_os("FALLOUT_SAVE_TEST_READY").unwrap());
    let catalogue = load(&root, &["FalloutNV.esm"]);
    if mode == "cold" {
        let repository = Repository::open(&root.join("native-cold"), &[]).unwrap();
        let (world, _) = repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap();
        assert_eq!(world.snapshot(), seed(&catalogue).snapshot());
        assert_eq!(world.pending_events().len(), 1);
        fs::write(ready, b"verified").unwrap();
        return;
    }
    if let Some(boundary) = mode.strip_prefix("cold-stage:") {
        let repository =
            Repository::open(&root.join(format!("native-stage-{boundary}")), &[]).unwrap();
        let (world, _) = repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap();
        let mut expected = staged_cold_seed(&catalogue).snapshot();
        if boundary == "after" {
            expected.state_revision += 1;
            expected.pending_events.remove(0);
            expected.instances[0]
                .locals
                .iter_mut()
                .find(|local| local.index == 42)
                .unwrap()
                .value = Value::Number {
                bits: 0x7ff8_1234_5678_9abc,
            };
        } else {
            assert_eq!(boundary, "before");
        }
        assert_eq!(world.snapshot(), expected);
        let handle = world.handle(expected.instances[0].id).unwrap();
        assert_eq!(
            world.instance(handle).unwrap().local(90).unwrap(),
            &Value::Reference {
                value: fallout_runtime::identity::ReferenceValue::Live {
                    id: expected.references[0].id
                }
            }
        );
        fs::write(ready, b"verified staged boundary").unwrap();
        return;
    }
    let cold_boundary = mode
        .strip_prefix("cold-killed-stage:")
        .map(|parameters| (parameters, false))
        .or_else(|| {
            mode.strip_prefix("cold-killed-fallback:")
                .map(|parameters| (parameters, true))
        });
    if let Some((parameters, fallback)) = cold_boundary {
        let (index, boundary) = parameters.split_once(':').unwrap();
        let index: usize = index.parse().unwrap();
        let name = if fallback {
            "fallback-stage"
        } else {
            "native-stage-kill"
        };
        let repository = Repository::open(&root.join(format!("{name}-{index}")), &[]).unwrap();
        if fallback {
            assert!(
                repository
                    .load(&catalogue, Limits::default(), Recovery::Strict)
                    .is_err()
            );
        }
        let (world, receipt) = repository
            .load(
                &catalogue,
                Limits::default(),
                if fallback {
                    Recovery::PreviousIfCurrentInvalid
                } else {
                    Recovery::Strict
                },
            )
            .unwrap();
        assert_eq!(
            receipt.slot,
            if fallback {
                Slot::Previous
            } else {
                Slot::Current
            }
        );
        assert_eq!(receipt.current_failure.is_some(), fallback);
        assert!(!receipt.current_repaired);
        let before = staged_cold_seed(&catalogue).snapshot();
        let expected = match boundary {
            "before" => before,
            "after" => expected_staged_after(before),
            _ => panic!("invalid staged boundary"),
        };
        assert_eq!(world.snapshot(), expected);
        fs::write(ready, b"verified complete interrupted staged boundary").unwrap();
        return;
    }
    if let Some(index) = mode.strip_prefix("kill-stage:") {
        let index: usize = index.parse().unwrap();
        let stage = STAGED_SAVE_STAGES[index];
        let repository =
            Repository::open(&root.join(format!("native-stage-kill-{index}")), &[]).unwrap();
        let mut world = staged_cold_seed(&catalogue);
        let staged = world
            .stage_event_changes(
                2,
                &[(
                    42,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9abc,
                    },
                )],
                true,
            )
            .unwrap();
        world.commit_event_changes(staged).unwrap();
        repository
            .commit_observing(&Captured::at_boundary(&world), |observed| {
                if observed == stage {
                    fs::write(&ready, b"staged publication ready").unwrap();
                    loop {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            })
            .unwrap();
        return;
    }
    let index = mode
        .strip_prefix("kill:")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let stage = [
        Stage::CurrentTempWritten,
        Stage::CurrentTempSynced,
        Stage::PreviousTempSynced,
        Stage::PreviousPublished,
        Stage::CurrentPublished,
    ][index];
    let repository = Repository::open(&root.join(format!("native-{index}")), &[]).unwrap();
    let mut world = seed(&catalogue);
    set_number(&mut world, 2);
    repository
        .commit_observing(&Captured::at_boundary(&world), |observed| {
            if observed == stage {
                fs::write(&ready, b"ready").unwrap();
                loop {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        })
        .unwrap();
}
