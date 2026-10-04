mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    inventory::{Condition, Facts, OpaqueExtra},
    save::{self, Captured, CompletionError, Recovery, Repository, SaveWorker, Slot, format},
};
use std::fs::{self, OpenOptions};

fn seed<'a>(catalogue: &'a fallout_data::loaded_scripts::Catalogue) -> World<'a> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x47; 16]).unwrap(),
    )
    .unwrap();
    let owner = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(owner),
        containing_reference: Some(owner),
        target: Some(ReferenceValue::Live { id: owner }),
        arguments: vec![
            ReferenceValue::Null,
            ReferenceValue::Content { key: form(0x100) },
        ],
    };
    let instance = world
        .create_instance(
            &definition(catalogue),
            Owner::Placed { reference: owner },
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            instance,
            &[
                (
                    42,
                    Value::Number {
                        bits: 0x8000_0000_0000_0000,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: owner },
                    },
                ),
            ],
        )
        .unwrap();
    world.initialize_inventory(owner).unwrap();
    let mut facts = Facts::unknown(form(0x100));
    facts.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abc,
    });
    facts.script_instance = Some(world.instance(instance).unwrap().id());
    facts.extra_fields.push(OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 255, 1],
    });
    world
        .add_item(owner, facts, 17.try_into().unwrap())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 10,
            menu_nanoseconds: 20,
            real_nanoseconds: 30,
        })
        .unwrap();
    world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    world
}
fn change(world: &mut World<'_>, bits: u64) {
    let id = world.snapshot().instances[0].id;
    world
        .assign(world.handle(id).unwrap(), &[(42, Value::Number { bits })])
        .unwrap();
}
fn fixture() -> (tempfile::TempDir, fallout_data::loaded_scripts::Catalogue) {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    (directory, catalogue)
}

fn transaction_phase(
    root: &std::path::Path,
    name: &str,
    repository: &Repository,
    world: &World<'_>,
) -> format::Metadata {
    let bytes = fs::read(repository.path().join("current.frsv")).unwrap();
    let decoded = format::decode(&bytes, Limits::default()).unwrap();
    assert_eq!(decoded.snapshot, world.snapshot());
    fs::write(root.join(format!("{name}.frsv")), bytes).unwrap();
    fs::write(
        root.join(format!("{name}.snapshot.json")),
        world.snapshot().encode(8192).unwrap(),
    )
    .unwrap();
    decoded.metadata
}

fn transaction_limits() -> Limits {
    Limits {
        max_snapshot_bytes: 8192,
        ..Limits::default()
    }
}

#[test]
fn repeated_stages_across_native_restores_reject_stale_proposals_and_preserve_slots() {
    let (_temporary, root) = match std::env::var_os("FALLOUT_STAGED_RESTORE_EVIDENCE") {
        Some(root) => {
            let root = std::path::PathBuf::from(root).join("authored");
            fs::create_dir(&root).unwrap();
            (None, root)
        }
        None => {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_path_buf();
            (Some(directory), root)
        }
    };
    write_fixture(&root, false);
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let mut world = World::restore(
        &catalogue,
        seed(&catalogue).snapshot(),
        transaction_limits(),
    )
    .unwrap();
    let handle = world.handle(world.snapshot().instances[0].id).unwrap();
    let context = world.pending_events().next().unwrap().context.clone();
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
    let before = world.snapshot();
    let changes = [(42, Value::Number { bits: u64::MAX })];
    let for_restore = world.stage_event_changes(1, &changes, true).unwrap();
    let first = world.stage_event_changes(1, &changes, true).unwrap();
    let duplicate = world.stage_event_changes(1, &changes, true).unwrap();
    assert_eq!(world.snapshot(), before);
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start_with_budget(repository.clone(), 2, 16384).unwrap();
    let prewrite = worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap()
        .wait()
        .unwrap();
    let prebytes = fs::read(repository.path().join("current.frsv")).unwrap();
    let premeta = transaction_phase(&root, "before", &repository, &world);
    let mut cold = repository
        .load(&catalogue, transaction_limits(), Recovery::Strict)
        .unwrap()
        .0;
    assert_eq!(cold.snapshot(), before);
    assert!(matches!(
        cold.commit_event_changes(for_restore),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(cold.snapshot(), before);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        prebytes
    );
    let first_commit = world.commit_event_changes(first).unwrap();
    let after_first = world.snapshot();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected.pending_events.remove(0);
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = changes[0].1.clone();
    assert_eq!(after_first, expected);
    let duplicate_error = world
        .commit_event_changes(duplicate)
        .unwrap_err()
        .to_string();
    assert_eq!(
        duplicate_error,
        "runtime state is invalid: staged event revision changed"
    );
    assert_eq!(world.snapshot(), after_first);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        prebytes
    );
    let first_write = worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap()
        .wait()
        .unwrap();
    let firstbytes = fs::read(repository.path().join("current.frsv")).unwrap();
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        prebytes
    );
    let firstmeta = transaction_phase(&root, "after-first", &repository, &world);
    let mut current = repository
        .load(&catalogue, transaction_limits(), Recovery::Strict)
        .unwrap()
        .0;
    assert_eq!(current.snapshot(), after_first);
    let replay_error = current
        .stage_event_changes(1, &changes, true)
        .unwrap_err()
        .to_string();
    assert_eq!(
        replay_error,
        "runtime state is invalid: staging must name the first pending event"
    );
    assert_eq!(current.snapshot(), after_first);
    let across_epoch = current.stage_event_changes(2, &changes, true).unwrap();
    current.replace_from_snapshot(after_first.clone()).unwrap();
    assert!(matches!(
        current.commit_event_changes(across_epoch),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(current.snapshot(), after_first);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        firstbytes
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        prebytes
    );
    let mut changed = repository
        .load(&catalogue, transaction_limits(), Recovery::Strict)
        .unwrap()
        .0;
    let before_mutation = changed.stage_event_changes(2, &changes, true).unwrap();
    change(&mut changed, 17);
    let changed_snapshot = changed.snapshot();
    assert_eq!(
        changed
            .commit_event_changes(before_mutation)
            .unwrap_err()
            .to_string(),
        duplicate_error
    );
    assert_eq!(changed.snapshot(), changed_snapshot);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        firstbytes
    );
    let second = current
        .stage_event_changes(
            2,
            &[(
                42,
                Value::Number {
                    bits: 0x8000_0000_0000_0000,
                },
            )],
            true,
        )
        .unwrap();
    let duplicate_second = current.stage_event_changes(2, &[], true).unwrap();
    let second_commit = current.commit_event_changes(second).unwrap();
    let after_second = current.snapshot();
    expected.state_revision += 1;
    expected.pending_events.remove(0);
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = Value::Number {
        bits: 0x8000_0000_0000_0000,
    };
    assert_eq!(after_second, expected);
    assert_eq!(
        current
            .commit_event_changes(duplicate_second)
            .unwrap_err()
            .to_string(),
        duplicate_error
    );
    assert!(current.stage_event_changes(2, &changes, true).is_err());
    assert_eq!(current.snapshot(), after_second);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        firstbytes
    );
    let second_write = worker.try_submit(Captured::at_boundary(&current)).unwrap();
    worker.finish().unwrap();
    let second_write = second_write.wait().unwrap();
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        firstbytes
    );
    let secondmeta = transaction_phase(&root, "after-second", &repository, &current);
    assert_eq!(
        repository
            .load(&catalogue, transaction_limits(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        after_second
    );
    let fallback = Repository::create(&root.join("recovery"), &[], world.campaign()).unwrap();
    fs::write(fallback.path().join("current.frsv"), b"truncated").unwrap();
    fs::write(fallback.path().join("previous.frsv"), &firstbytes).unwrap();
    let for_previous = world.stage_event_changes(2, &changes, true).unwrap();
    let (mut previous, receipt) = fallback
        .load(
            &catalogue,
            transaction_limits(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(previous.snapshot(), after_first);
    assert_eq!(receipt.slot, Slot::Previous);
    assert!(matches!(
        previous.commit_event_changes(for_previous),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(previous.snapshot(), after_first);
    assert_eq!(
        fs::read(fallback.path().join("current.frsv")).unwrap(),
        b"truncated"
    );
    assert_eq!(
        fs::read(fallback.path().join("previous.frsv")).unwrap(),
        firstbytes
    );
    let (mut repaired, repair) = fallback
        .recover_previous(&catalogue, transaction_limits())
        .unwrap();
    assert!(repair.current_repaired);
    let second = repaired
        .stage_event_changes(
            2,
            &[(
                42,
                Value::Number {
                    bits: 0x8000_0000_0000_0000,
                },
            )],
            true,
        )
        .unwrap();
    repaired.commit_event_changes(second).unwrap();
    assert_eq!(repaired.snapshot(), after_second);
    let mut retry = SaveWorker::start_with_budget(fallback.clone(), 1, 8192).unwrap();
    let saved = retry.try_submit(Captured::at_boundary(&repaired)).unwrap();
    retry.finish().unwrap();
    assert_eq!(saved.wait().unwrap().metadata, second_write.metadata);
    for slot in ["current.frsv", "previous.frsv"] {
        assert_eq!(
            fs::read(fallback.path().join(slot)).unwrap(),
            fs::read(repository.path().join(slot)).unwrap()
        );
    }
    fs::write(root.join("transaction-receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "before":premeta,"after_first":firstmeta,"after_second":secondmeta,"prewrite":prewrite,"first_write":first_write,"second_write":second_write,
        "first_commit":first_commit,"second_commit":second_commit,"revision_rejection":duplicate_error,"head_rejection":replay_error,
        "exact_native_load_epoch_rejected":true,"exact_previous_epoch_rejected":true,"rejected_world_and_slots_unchanged":true,
        "previous_fresh_retry_equal":true,"engineering_only":true
    })).unwrap()).unwrap();
}

#[test]
fn queued_captures_outlive_the_world_and_restore_exact_script_item_and_event_state() {
    let (directory, catalogue) = fixture();
    let mut world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = world.snapshot();
    let mut first_ticket = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    change(&mut world, 0x7ff8_5555_6666_7777);
    let second = world.snapshot();
    let second_ticket = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    change(&mut world, 3);
    drop(world);
    drop(catalogue);
    worker.finish().unwrap();
    assert_eq!(
        first_ticket
            .try_wait()
            .unwrap()
            .unwrap()
            .metadata
            .generation,
        1
    );
    assert!(matches!(
        first_ticket.try_wait(),
        Err(CompletionError::AlreadyCollected)
    ));
    assert!(matches!(
        first_ticket.wait(),
        Err(CompletionError::AlreadyCollected)
    ));
    assert_eq!(second_ticket.wait().unwrap().metadata.generation, 2);
    let previous = format::decode(
        &fs::read(repository.path().join("previous.frsv")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(previous.snapshot, first);
    let reopened = Repository::open(repository.path(), &[]).unwrap();
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let (restored, receipt) = reopened
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    assert_eq!(restored.snapshot(), second);
    assert_eq!(restored.pending_events().len(), 1);
    assert_eq!(
        restored
            .inventory_count(second.references[0].id, &form(0x100))
            .unwrap(),
        17
    );
}

#[test]
fn stale_write_errors_are_delivered_and_do_not_stop_later_requests() {
    let (directory, catalogue) = fixture();
    let mut world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let old = Captured::at_boundary(&world);
    change(&mut world, 2);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let before = fs::read(repository.path().join("current.frsv")).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let stale = worker.try_submit(old).unwrap();
    assert!(matches!(
        stale.wait(),
        Err(CompletionError::Save(save::Error::Format(_)))
    ));
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        before
    );
    change(&mut world, 3);
    let expected = world.snapshot();
    let valid = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    worker.finish().unwrap();
    assert_eq!(valid.wait().unwrap().metadata.generation, 2);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected
    );
}

#[test]
fn repository_lock_contention_is_a_request_error_and_requires_an_explicit_retry() {
    let (directory, catalogue) = fixture();
    let world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let blocked = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    assert!(matches!(
        blocked.wait(),
        Err(CompletionError::Save(save::Error::Busy))
    ));
    assert!(!repository.path().join("current.frsv").exists());
    drop(lock);
    let retry = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    worker.finish().unwrap();
    assert_eq!(retry.wait().unwrap().metadata.generation, 1);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        world.snapshot()
    );
}

#[test]
fn staged_worker_boundaries_recover_exact_pending_and_committed_state() {
    let (directory, catalogue) = fixture();
    let mut world = seed(&catalogue);
    let repository = Repository::create(
        &directory.path().join("native-staged"),
        &[],
        world.campaign(),
    )
    .unwrap();
    let before = world.snapshot();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let stage = world
        .stage_event_changes(1, &[(42, Value::Number { bits: u64::MAX })], true)
        .unwrap();
    world.commit_event_changes(stage).unwrap();
    let after = world.snapshot();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected.pending_events.remove(0);
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = Value::Number { bits: u64::MAX };
    assert_eq!(after, expected);
    let second = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    change(&mut world, 3);
    drop(world);
    drop(catalogue);
    worker.finish().unwrap();
    assert_eq!(
        first.wait().unwrap().metadata.state_revision,
        before.state_revision
    );
    assert_eq!(
        second.wait().unwrap().metadata.state_revision,
        after.state_revision
    );
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        after
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
    fs::write(
        repository.path().join("current.frsv"),
        b"truncated staged current",
    )
    .unwrap();
    let failure = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .err()
        .unwrap();
    assert_eq!(
        failure.to_string(),
        "native save format: container byte budget/extent"
    );
    let (recovered, receipt) = repository
        .load(
            &catalogue,
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(recovered.snapshot(), before);
    assert_eq!(receipt.slot, Slot::Previous);
    assert_eq!(
        receipt.current_failure.as_deref(),
        Some("native save format: container byte budget/extent")
    );
    assert!(!receipt.current_repaired);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        b"truncated staged current"
    );
    let (mut repaired, repair) = repository
        .recover_previous(&catalogue, Limits::default())
        .unwrap();
    assert!(repair.current_repaired);
    assert_eq!(repaired.snapshot(), before);
    let stage = repaired
        .stage_event_changes(1, &[(42, Value::Number { bits: u64::MAX })], true)
        .unwrap();
    repaired.commit_event_changes(stage).unwrap();
    assert_eq!(repaired.snapshot(), after);
    let mut restarted = SaveWorker::start(repository.clone(), 1).unwrap();
    let resumed = restarted
        .try_submit(Captured::at_boundary(&repaired))
        .unwrap();
    restarted.finish().unwrap();
    assert_eq!(resumed.wait().unwrap().metadata.generation, 2);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        after
    );
}
