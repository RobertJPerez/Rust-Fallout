mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    inventory::{Condition, Facts, OpaqueExtra},
    save::{self, Captured, CompletionError, Recovery, Repository, SaveWorker, format},
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
