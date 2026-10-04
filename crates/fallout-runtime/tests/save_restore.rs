mod common;
use common::*;
use fallout_data::{loaded_scripts::Catalogue, world::Transform};
use fallout_runtime::{
    Error, Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    reference_state::{Pose, State},
    save::{self, Captured, Recovery, Repository, RequestIdentity, RestorePoll, RestoreTask, Slot},
    snapshot::Snapshot,
    state::initialization,
};
use std::{
    collections::BTreeMap, fs, io::Write, num::NonZeroU64, path::Path, process::Command, sync::Arc,
};

fn setup(root: &Path) -> (Arc<Catalogue>, Repository, [Snapshot; 2]) {
    write_fixture(root, false);
    fs::OpenOptions::new()
        .append(true)
        .open(root.join("FalloutNV.esm"))
        .unwrap()
        .write_all(&record(b"CELL", 0x400, 0, &[]))
        .unwrap();
    let catalogue = Arc::new(load(root, &["FalloutNV.esm"]));
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes([0x21; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x100))).unwrap();
    let unknown = world.register_reference(None).unwrap();
    let pose = Pose::from_source(
        &Transform {
            position: [-0.0, f32::from_bits(1), 40.0],
            rotation: [0.25, -0.0, -0.5],
        },
        None,
    )
    .unwrap();
    let state = State::new(form(0x400), pose, false).unwrap();
    let stage = world
        .stage_reference_state(&world.reference_view(reference).unwrap(), state)
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    world.initialize_inventory(reference).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: unknown }),
        arguments: vec![
            ReferenceValue::Null,
            ReferenceValue::Content { key: form(0x100) },
        ],
    };
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &Owner::Placed { reference },
            &context,
            &[
                (2, Value::Number { bits: 1 << 63 }),
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
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &Owner::Fragment {
                activation: NonZeroU64::new(7).unwrap(),
            },
            &Context::default(),
            &[],
            initialization::Limits::default(),
        )
        .unwrap();
    let (_, other) = world.commit_instance_initialization(stage).unwrap();
    world
        .enqueue(
            other,
            Trigger::ObjectEvent { mask: 0x8000_0001 },
            Context::default(),
        )
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 3,
            menu_nanoseconds: 5,
            real_nanoseconds: 7,
        })
        .unwrap();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let first = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    world
        .assign(
            handle,
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
fn request(snapshot: &Snapshot, id: u64) -> RequestIdentity {
    RequestIdentity::new(
        NonZeroU64::new(id).unwrap(),
        snapshot.campaign,
        &snapshot.catalogue_sha256,
    )
    .unwrap()
}
fn take(task: &mut RestoreTask, identity: &RequestIdentity) -> (World<'static>, save::LoadReceipt) {
    task.finish().unwrap();
    let RestorePoll::Ready(candidate) = task.try_poll() else {
        panic!("expected restored candidate")
    };
    candidate.take_for(identity).unwrap()
}
fn bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().into(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn exact_and_one_under_restore_limits_keep_the_existing_source_admission_and_no_slot_writes() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, [_, expected]) = setup(temp.path());
    let before = bytes(repository.path());
    let maximum = expected.encode(1 << 20).unwrap().len();
    let identity = request(&expected, 1);
    let limits = Limits {
        max_snapshot_bytes: maximum,
        max_instances: 2,
        max_locals: 6,
        max_pending_events: 2,
        ..Limits::default()
    };
    let mut task = RestoreTask::start(
        repository.clone(),
        Arc::clone(&catalogue),
        limits,
        Recovery::Strict,
        identity.clone(),
    )
    .unwrap();
    let (world, receipt) = take(&mut task, &identity);
    assert_eq!(world.snapshot(), expected);
    assert_eq!(receipt.metadata.generation, 2);
    drop(world);
    for limits in [
        Limits {
            max_snapshot_bytes: maximum - 1,
            ..limits
        },
        Limits {
            max_instances: 1,
            ..limits
        },
        Limits {
            max_locals: 5,
            ..limits
        },
        Limits {
            max_pending_events: 1,
            ..limits
        },
        Limits {
            max_snapshot_bytes: usize::MAX,
            ..limits
        },
    ] {
        let ordinary = repository
            .load(Arc::clone(&catalogue), limits, Recovery::Strict)
            .err()
            .unwrap();
        let mut task = RestoreTask::start(
            repository.clone(),
            Arc::clone(&catalogue),
            limits,
            Recovery::Strict,
            identity.clone(),
        )
        .unwrap();
        task.finish().unwrap();
        let RestorePoll::Failed(error) = task.try_poll() else {
            panic!("budget must refuse")
        };
        assert!(
            matches!(&*error, save::RestoreError::Save(actual) if actual.to_string() == ordinary.to_string())
        );
        assert_eq!(Arc::strong_count(&catalogue), 1);
    }
    assert_eq!(bytes(repository.path()), before);
}

#[test]
fn explicit_previous_recovery_and_changed_source_keep_ordinary_load_failure_and_policy() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, [first, current]) = setup(temp.path());
    fs::write(
        repository.path().join("current.frsv"),
        b"truncated current for explicit recovery",
    )
    .unwrap();
    let before = bytes(repository.path());
    let ordinary = repository
        .load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict)
        .err()
        .unwrap()
        .to_string();
    let identity = request(&current, 1);
    let mut strict = RestoreTask::start(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        Recovery::Strict,
        identity.clone(),
    )
    .unwrap();
    strict.finish().unwrap();
    assert!(
        matches!(strict.try_poll(), RestorePoll::Failed(error) if matches!(&*error, save::RestoreError::Save(actual) if actual.to_string() == ordinary))
    );
    let mut recovery = RestoreTask::start(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        Recovery::PreviousIfCurrentInvalid,
        identity.clone(),
    )
    .unwrap();
    let (world, receipt) = take(&mut recovery, &identity);
    assert_eq!(world.snapshot(), first);
    assert_eq!(receipt.slot, Slot::Previous);
    assert_eq!(receipt.current_failure.as_deref(), Some(ordinary.as_str()));
    assert!(!receipt.current_repaired);
    drop(world);
    let changed = Arc::new(load(temp.path(), &["FalloutNV.esm", "Other.esm"]));
    let weak = Arc::downgrade(&changed);
    let mut wrong = RestoreTask::start(
        repository.clone(),
        changed,
        Limits::default(),
        Recovery::PreviousIfCurrentInvalid,
        identity,
    )
    .unwrap();
    wrong.finish().unwrap();
    assert!(
        matches!(wrong.try_poll(), RestorePoll::Failed(error) if matches!(&*error, save::RestoreError::Save(save::Error::State(Error::DefinitionChanged))))
    );
    assert!(weak.upgrade().is_none());
    assert_eq!(bytes(repository.path()), before);
}

#[test]
fn fresh_async_consumer_observes_every_canonical_field_without_dispatch_or_repository_repair() {
    let temp = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("FALLOUT_RESTORE_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fs::create_dir_all(root).unwrap();
    let (catalogue, repository, [first, current]) = setup(root);
    fs::write(
        root.join("expected.previous.json"),
        first.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.current.json"),
        current.encode(1 << 20).unwrap(),
    )
    .unwrap();
    let before = bytes(repository.path());
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(catalogue);
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_restore_helper", "--ignored", "--nocapture"])
        .env("FALLOUT_RESTORE_COLD_ROOT", root)
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
    assert_eq!(bytes(repository.path()), before);
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}

#[test]
#[ignore = "fresh source-bound asynchronous consumer invoked by its parent"]
fn cold_restore_helper() {
    let root = std::path::PathBuf::from(std::env::var_os("FALLOUT_RESTORE_COLD_ROOT").unwrap());
    let catalogue = Arc::new(load(&root, &["FalloutNV.esm"]));
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let before = bytes(repository.path());
    let expected = Snapshot::decode(
        &fs::read(root.join("expected.current.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let identity = request(&expected, 31);
    let weak = Arc::downgrade(&catalogue);
    let mut task = RestoreTask::start(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        Recovery::Strict,
        identity.clone(),
    )
    .unwrap();
    let (world, receipt) = take(&mut task, &identity);
    assert_eq!(world.snapshot(), expected);
    assert_eq!(world.pending_events().len(), 2);
    let ordinary = repository
        .load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(world.snapshot(), ordinary.0.snapshot());
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        serde_json::to_value(&ordinary.1).unwrap()
    );
    let restored = world.snapshot();
    fs::write(
        root.join("cold.restored.json"),
        restored.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("cold.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    drop(ordinary);
    drop(world);
    drop(catalogue);
    drop(task);
    assert!(weak.upgrade().is_none());
    assert_eq!(bytes(repository.path()), before);
}
