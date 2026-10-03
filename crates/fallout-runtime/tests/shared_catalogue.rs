mod common;
use common::*;
use fallout_data::loaded_scripts::Catalogue;
use fallout_runtime::{
    Error, Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, Value},
    save::{Captured, Recovery, Repository, Slot, format},
};
use std::{fs, sync::Arc};

fn seeded<'a>(source: impl Into<fallout_runtime::SourceCatalogue<'a>>) -> World<'a> {
    let mut world = World::with_campaign(
        source,
        Limits::default(),
        CampaignId::from_bytes([0x42; 16]).unwrap(),
    )
    .unwrap();
    let definition = definition(world.catalogue());
    let handle = world
        .create_instance(
            &definition,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 17 })])
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

fn owned_fixture() -> (tempfile::TempDir, Arc<Catalogue>) {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    (directory, catalogue)
}

#[test]
fn world_keeps_loaded_records_alive_and_releases_them_on_drop() {
    let (directory, catalogue) = owned_fixture();
    let weak = Arc::downgrade(&catalogue);
    let source_address = Arc::as_ptr(&catalogue);
    let world: World<'static> = seeded(catalogue);
    drop(directory);
    assert!(weak.upgrade().is_some());
    assert!(std::ptr::eq(world.catalogue(), source_address));
    let source = world
        .catalogue()
        .get_handle(&definition(world.catalogue()))
        .unwrap();
    assert_eq!(source.compiled().unwrap().len(), 14);
    assert_eq!(world.snapshot().instances[0].locals.len(), 3);
    drop(world);
    assert!(weak.upgrade().is_none());
}

#[test]
fn owned_world_moves_to_a_worker_and_returns_without_a_loader_borrow() {
    fn transferable<T: Send + Sync + 'static>() {}
    transferable::<World<'static>>();
    let (directory, catalogue) = owned_fixture();
    let world: World<'static> = seeded(catalogue);
    drop(directory);
    let before = world.snapshot();
    let expected = before.clone();
    let worker = std::thread::spawn(move || {
        assert_eq!(world.snapshot(), expected);
        assert!(
            world
                .catalogue()
                .get_handle(&expected.instances[0].definition)
                .is_some()
        );
        world
    });
    assert_eq!(worker.join().unwrap().snapshot(), before);
}

#[test]
fn borrowed_and_owned_worlds_produce_identical_snapshots_and_save_bytes() {
    let (_directory, catalogue) = owned_fixture();
    let borrowed = seeded(catalogue.as_ref());
    let shared = seeded(Arc::clone(&catalogue));
    assert_eq!(borrowed.snapshot(), shared.snapshot());
    let borrowed_bytes = format::encode(&Captured::at_boundary(&borrowed), 1).unwrap();
    let shared_bytes = format::encode(&Captured::at_boundary(&shared), 1).unwrap();
    assert_eq!(borrowed_bytes, shared_bytes);
    assert!(std::ptr::eq(borrowed.catalogue(), shared.catalogue()));
}

#[test]
fn shared_sources_do_not_share_mutable_campaign_banks_or_transient_handles() {
    let (_directory, catalogue) = owned_fixture();
    let first = seeded(Arc::clone(&catalogue));
    let mut second = seeded(catalogue);
    let id = first.snapshot().instances[0].id;
    let first_handle = first.handle(id).unwrap();
    let second_handle = second.handle(id).unwrap();
    assert!(matches!(
        second.instance(first_handle),
        Err(Error::StaleHandle)
    ));
    let before = first.snapshot();
    second
        .assign(second_handle, &[(42, Value::Number { bits: 29 })])
        .unwrap();
    assert_eq!(first.snapshot(), before);
    assert_ne!(first.snapshot(), second.snapshot());
    assert!(std::ptr::eq(first.catalogue(), second.catalogue()));
}

#[test]
fn owned_restoration_retains_sources_and_rejects_old_world_handles() {
    let (_directory, catalogue) = owned_fixture();
    let original = seeded(Arc::clone(&catalogue));
    let snapshot = original.snapshot();
    let old_handle = original.handle(snapshot.instances[0].id).unwrap();
    let weak = Arc::downgrade(&catalogue);
    let restored: World<'static> =
        World::restore(catalogue, snapshot.clone(), Limits::default()).unwrap();
    drop(original);
    assert!(weak.upgrade().is_some());
    assert_eq!(restored.snapshot(), snapshot);
    assert!(matches!(
        restored.instance(old_handle),
        Err(Error::StaleHandle)
    ));
    drop(restored);
    assert!(weak.upgrade().is_none());
}

#[test]
fn replacement_is_atomic_and_keeps_one_shared_source_owner() {
    let (_directory, catalogue) = owned_fixture();
    let mut world = seeded(Arc::clone(&catalogue));
    let before = world.snapshot();
    let old_handle = world.handle(before.instances[0].id).unwrap();
    let mut bad = before.clone();
    bad.catalogue_sha256 = "0".repeat(64);
    assert!(matches!(
        world.replace_from_snapshot(bad),
        Err(Error::DefinitionChanged)
    ));
    assert_eq!(world.snapshot(), before);
    assert_eq!(Arc::strong_count(&catalogue), 2);
    world.replace_from_snapshot(before.clone()).unwrap();
    assert_eq!(world.snapshot(), before);
    assert!(matches!(
        world.instance(old_handle),
        Err(Error::StaleHandle)
    ));
    assert_eq!(Arc::strong_count(&catalogue), 2);
}

#[test]
fn repository_load_and_explicit_recovery_can_return_owned_worlds() {
    let (directory, catalogue) = owned_fixture();
    let mut world = seeded(Arc::clone(&catalogue));
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let previous = world.snapshot();
    let handle = world.handle(previous.instances[0].id).unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 31 })])
        .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let (current, receipt): (World<'static>, _) = repository
        .load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.slot, Slot::Current);
    assert_eq!(current.snapshot(), world.snapshot());
    drop(current);
    fs::write(repository.path().join("current.frsv"), b"truncated").unwrap();
    assert!(
        repository
            .load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict)
            .is_err()
    );
    assert_eq!(Arc::strong_count(&catalogue), 2);
    let (fallback, receipt): (World<'static>, _) = repository
        .load(
            Arc::clone(&catalogue),
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(fallback.snapshot(), previous);
    assert_eq!(receipt.slot, Slot::Previous);
    assert!(!receipt.current_repaired);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        b"truncated"
    );
    drop(fallback);
    let weak = Arc::downgrade(&catalogue);
    let (repaired, receipt): (World<'static>, _) = repository
        .recover_previous(catalogue, Limits::default())
        .unwrap();
    assert!(receipt.current_repaired);
    assert_eq!(repaired.snapshot(), previous);
    drop(world);
    assert!(weak.upgrade().is_some());
    drop(repaired);
    assert!(weak.upgrade().is_none());
}

#[test]
fn owned_restoration_preserves_full_source_cohort_checks() {
    let (directory, catalogue) = owned_fixture();
    let world = seeded(Arc::clone(&catalogue));
    let unchanged = Arc::new(load(directory.path(), &["Other.esm", "FalloutNV.esm"]));
    assert_eq!(
        World::restore(unchanged, world.snapshot(), Limits::default())
            .unwrap()
            .snapshot(),
        world.snapshot()
    );
    fs::write(
        directory.path().join("Other.esm"),
        [header(&[]), record(b"ACTI", 0x999, 0, &[])].concat(),
    )
    .unwrap();
    let changed = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    let weak = Arc::downgrade(&changed);
    assert!(matches!(
        World::restore(changed, world.snapshot(), Limits::default()),
        Err(Error::DefinitionChanged)
    ));
    assert!(weak.upgrade().is_none());
}
