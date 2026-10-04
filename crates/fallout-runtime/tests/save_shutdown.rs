mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    save::{Captured, Rejection, Repository, SaveState, SaveStatus, SaveWorker, Stage, format},
    snapshot::Snapshot,
    state::initialization,
};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

fn seed(catalogue: &fallout_data::loaded_scripts::Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x33; 16]).unwrap(),
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
    world
}
fn directory_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
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
fn join_poll(worker: &mut SaveWorker) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !worker.try_shutdown().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}
#[test]
fn source_bound_shutdown_gates_both_fifo_captures_then_two_fresh_readers_keep_exact_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("FALLOUT_SHUTDOWN_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fs::create_dir_all(root).unwrap();
    write_fixture(root, false);
    let catalogue = load(root, &["FalloutNV.esm"]);
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    let mut world = seed(&catalogue);
    let before = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let (entered, entries) = mpsc::sync_channel(1);
    let (release, releases) = mpsc::sync_channel(1);
    let mut worker = SaveWorker::start_observing(
        repository.clone(),
        2,
        SaveWorker::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES,
        move |stage| {
            if stage == Stage::CurrentTempWritten {
                entered.send(()).unwrap();
                releases.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        },
    )
    .unwrap();
    let mut first = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    entries.recv_timeout(Duration::from_secs(10)).unwrap();
    world
        .assign(
            world.handle(before.instances[0].id).unwrap(),
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8123456789abc,
                },
            )],
        )
        .unwrap();
    let current = world.snapshot();
    let second_capture = Captured::at_boundary(&world);
    let mut second = SaveStatus::new(worker.try_submit(second_capture).unwrap());
    assert!(worker.close_admission());
    assert!(!worker.close_admission());
    let third = Captured::at_boundary(&world);
    let refused = worker.try_submit(third).unwrap_err();
    assert_eq!(refused.reason, Rejection::WorkerStopped);
    assert_eq!(refused.capture.snapshot(), &current);
    drop(refused);
    for _ in 0..2 {
        assert!(!worker.try_shutdown().unwrap());
        assert!(matches!(first.poll(), SaveState::Pending));
        assert!(matches!(second.poll(), SaveState::Pending));
    }
    release.send(()).unwrap();
    entries.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(first.poll(), SaveState::Published(_)));
    let first_receipt = first.wait().unwrap();
    assert_eq!(first_receipt.metadata.generation, 1);
    let decoded = format::decode(
        &fs::read(repository.path().join("current.frsv")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        World::restore(&catalogue, decoded.snapshot, Limits::default())
            .unwrap()
            .snapshot(),
        before
    );
    for _ in 0..2 {
        assert!(!worker.try_shutdown().unwrap());
        assert!(matches!(second.poll(), SaveState::Pending));
    }
    release.send(()).unwrap();
    join_poll(&mut worker);
    assert!(worker.try_shutdown().unwrap());
    assert!(!worker.close_admission());
    assert!(matches!(second.poll(), SaveState::Published(_)));
    let second_receipt = second.wait().unwrap();
    assert_eq!(second_receipt.metadata.generation, 2);
    let terminal = directory_bytes(repository.path());
    worker.finish().unwrap();
    assert_eq!(directory_bytes(repository.path()), terminal);
    for (phase, state) in [("before", before), ("current", current)] {
        fs::write(
            root.join(format!("{phase}.snapshot.json")),
            state.encode(1 << 20).unwrap(),
        )
        .unwrap();
    }
    fs::write(root.join("shutdown.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "held_first_pending_polls":2,"held_second_pending_polls":2,"first_write":first_receipt,"second_write":second_receipt,
        "third_refused_intact":true,"close_idempotent":true,"joined_clean_stable":true,"individual_tickets_published":true,
        "blocking_finish_compatible":true,"dispatch_occurred":false
    })).unwrap()).unwrap();
    drop(world);
    drop(catalogue);
    for mode in ["previous", "current"] {
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_shutdown_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_SHUTDOWN_COLD_ROOT", root)
            .env("FALLOUT_SHUTDOWN_COLD_MODE", mode)
            .output()
            .unwrap();
        fs::write(root.join(format!("cold-{mode}.stdout.txt")), &child.stdout).unwrap();
        fs::write(root.join(format!("cold-{mode}.stderr.txt")), &child.stderr).unwrap();
        assert!(
            child.status.success(),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
    assert_eq!(directory_bytes(repository.path()), terminal);
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}
#[test]
fn source_bound_close_before_first_admission_refuses_original_capture_and_writes_no_slot() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), false);
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let world = seed(&catalogue);
    let capture = Captured::at_boundary(&world);
    let snapshot = capture.snapshot().clone();
    let repository =
        Repository::create(&root.path().join("native"), &[], world.campaign()).unwrap();
    let before = directory_bytes(repository.path());
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    assert!(worker.close_admission());
    assert!(!worker.close_admission());
    let refused = worker.try_submit(capture).unwrap_err();
    assert_eq!(refused.reason, Rejection::WorkerStopped);
    assert_eq!(refused.capture.snapshot(), &snapshot);
    join_poll(&mut worker);
    assert!(worker.try_shutdown().unwrap());
    worker.finish().unwrap();
    assert_eq!(directory_bytes(repository.path()), before);
}
#[test]
#[ignore = "fresh source/native boundary reader invoked by parent"]
fn cold_shutdown_helper() {
    let root = std::path::PathBuf::from(std::env::var_os("FALLOUT_SHUTDOWN_COLD_ROOT").unwrap());
    let mode = std::env::var("FALLOUT_SHUTDOWN_COLD_MODE").unwrap();
    let slot = match mode.as_str() {
        "previous" | "cli-before" => "previous",
        "current" | "cli-current" => "current",
        _ => panic!("unknown cold shutdown mode"),
    };
    let phase = if slot == "previous" {
        "before"
    } else {
        "current"
    };
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let before = directory_bytes(repository.path());
    let expected = Snapshot::decode(
        &fs::read(root.join(format!("{phase}.snapshot.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let decoded = format::decode(
        &fs::read(repository.path().join(format!("{slot}.frsv"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        decoded.metadata.generation,
        if slot == "previous" { 1 } else { 2 }
    );
    let world = World::restore(&catalogue, decoded.snapshot, Limits::default()).unwrap();
    assert_eq!(world.snapshot(), expected);
    fs::write(
        root.join(format!("cold-{mode}.snapshot.json")),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(directory_bytes(repository.path()), before);
}
