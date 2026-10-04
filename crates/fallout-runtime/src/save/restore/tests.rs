use super::*;
use crate::{
    events::{Context, Trigger},
    identity::{Owner, ReferenceValue, Value},
    save::{Captured, Slot, test_source::*},
    snapshot::Snapshot,
    state::initialization,
};
use std::{collections::BTreeMap, fs, path::Path, sync::mpsc, time::Duration};

fn seed(catalogue: Arc<Catalogue>) -> World<'static> {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes([0x21; 16]).unwrap(),
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
            &definition(&catalogue),
            &Owner::Placed { reference },
            &context,
            &[
                (2, Value::Number { bits: 1 << 63 }),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9abc,
                    },
                ),
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
fn setup(root: &Path) -> (Arc<Catalogue>, Repository, World<'static>) {
    write_fixture(root, false);
    let catalogue = Arc::new(load(root, &["FalloutNV.esm"]));
    let world = seed(Arc::clone(&catalogue));
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    (catalogue, repository, world)
}
fn request(world: &World<'_>, id: u64) -> RequestIdentity {
    RequestIdentity::new(
        NonZeroU64::new(id).unwrap(),
        world.campaign(),
        world.catalogue_fingerprint(),
    )
    .unwrap()
}
fn ready(task: &mut RestoreTask) -> RestoredCandidate {
    match task.try_poll() {
        RestorePoll::Ready(candidate) => *candidate,
        state => panic!("expected candidate: {state:?}"),
    }
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
fn pending_poll_does_not_join_a_real_held_load_and_acceptance_matches_complete_ordinary_restore() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, world) = setup(temp.path());
    let expected = world.snapshot();
    let before = bytes(repository.path());
    let old_stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &Owner::Fragment {
                activation: NonZeroU64::new(9).unwrap(),
            },
            &Context::default(),
            &[],
            initialization::Limits::default(),
        )
        .unwrap();
    let identity = request(&world, 1);
    let (entered, entries) = mpsc::sync_channel(1);
    let (release, releases) = mpsc::sync_channel(1);
    let worker_repository = repository.clone();
    let worker_catalogue = Arc::clone(&catalogue);
    let mut task = RestoreTask::spawn_with(identity.clone(), move || {
        entered.send(()).unwrap();
        releases.recv_timeout(Duration::from_secs(10)).unwrap();
        worker_repository.load(worker_catalogue, Limits::default(), Recovery::Strict)
    })
    .unwrap();
    entries.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(task.try_poll(), RestorePoll::Pending));
    assert!(matches!(task.try_poll(), RestorePoll::Pending));
    assert_eq!(world.snapshot(), expected);
    release.send(()).unwrap();
    task.finish().unwrap();
    let candidate = ready(&mut task);
    assert_eq!(candidate.request(), &identity);
    assert_eq!(candidate.receipt().slot, Slot::Current);
    assert!(matches!(task.try_poll(), RestorePoll::Delivered));
    let (mut restored, receipt) = candidate.take_for(&identity).unwrap();
    assert!(!task.cancel());
    assert_eq!(restored.snapshot(), expected);
    assert_eq!(
        restored.snapshot(),
        repository
            .load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot()
    );
    assert_eq!(receipt.metadata.state_revision, expected.state_revision);
    assert!(matches!(
        restored.commit_instance_initialization(old_stage),
        Err(crate::Error::StaleHandle)
    ));
    assert_eq!(restored.snapshot(), expected);
    assert_eq!(bytes(repository.path()), before);
}

#[test]
fn superseded_request_campaign_or_source_consumes_candidate_without_changing_active_world() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, world) = setup(temp.path());
    let expected = world.snapshot();
    let original = request(&world, 1);
    let foreign_campaign = RequestIdentity::new(
        original.request_id(),
        CampaignId::from_bytes([0x22; 16]).unwrap(),
        original.catalogue_fingerprint(),
    )
    .unwrap();
    let foreign_source =
        RequestIdentity::new(original.request_id(), original.campaign(), &"a".repeat(64)).unwrap();
    for current in [request(&world, 2), foreign_campaign, foreign_source] {
        let mut task = RestoreTask::start(
            repository.clone(),
            Arc::clone(&catalogue),
            Limits::default(),
            Recovery::Strict,
            original.clone(),
        )
        .unwrap();
        task.finish().unwrap();
        assert!(matches!(
            ready(&mut task).take_for(&current),
            Err(RestoreError::Superseded)
        ));
        assert_eq!(world.snapshot(), expected);
        assert_eq!(Arc::strong_count(&catalogue), 2);
    }
}

#[test]
fn cancel_and_drop_revoke_already_delivered_candidates_and_release_their_source() {
    for explicit in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (catalogue, repository, world) = setup(temp.path());
        let weak = Arc::downgrade(&catalogue);
        let identity = request(&world, 1);
        drop(world);
        let mut task = RestoreTask::start(
            repository,
            catalogue,
            Limits::default(),
            Recovery::Strict,
            identity.clone(),
        )
        .unwrap();
        task.finish().unwrap();
        let candidate = ready(&mut task);
        assert!(weak.upgrade().is_some());
        if explicit {
            assert!(task.cancel());
            assert!(!task.cancel());
            assert!(matches!(task.try_poll(), RestorePoll::Cancelled));
        } else {
            drop(task);
        }
        assert!(matches!(
            candidate.take_for(&identity),
            Err(RestoreError::Cancelled)
        ));
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn cancelled_or_dropped_task_does_not_wait_for_io_and_discards_late_complete_world() {
    for explicit in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (catalogue, repository, world) = setup(temp.path());
        let before = bytes(repository.path());
        let weak = Arc::downgrade(&catalogue);
        let identity = request(&world, 1);
        let expected = world.snapshot();
        drop(world);
        let worker_repository = repository.clone();
        let (entered, entries) = mpsc::sync_channel(1);
        let (release, releases) = mpsc::sync_channel(1);
        let mut task = RestoreTask::spawn_with(identity, move || {
            let restored =
                worker_repository.load(catalogue, Limits::default(), Recovery::Strict)?;
            assert_eq!(restored.0.snapshot(), expected);
            entered.send(()).unwrap();
            releases.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(restored)
        })
        .unwrap();
        entries.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(weak.upgrade().is_some());
        // Retain only the owned thread handle to prove destruction before the
        // release, then join off frame after allowing actual IO to finish.
        let thread = task.thread.take().unwrap();
        if explicit {
            assert!(task.cancel());
            assert!(matches!(task.try_poll(), RestorePoll::Cancelled));
        }
        drop(task);
        release.send(()).unwrap();
        thread.join().unwrap();
        assert!(weak.upgrade().is_none());
        assert_eq!(bytes(repository.path()), before);
    }
}

#[test]
fn unpolled_result_retains_only_one_world_and_cancel_releases_it_exactly_once() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, world) = setup(temp.path());
    let identity = request(&world, 1);
    let weak = Arc::downgrade(&catalogue);
    drop(world);
    let mut task = RestoreTask::start(
        repository,
        catalogue,
        Limits::default(),
        Recovery::Strict,
        identity,
    )
    .unwrap();
    task.finish().unwrap();
    assert_eq!(weak.strong_count(), 1);
    assert!(task.cancel());
    assert_eq!(weak.strong_count(), 0);
    assert!(!task.cancel());
    assert!(matches!(task.try_poll(), RestorePoll::Cancelled));
    drop(task);
    assert!(weak.upgrade().is_none());
}

#[test]
fn actual_busy_and_worker_panic_are_explicit_sticky_failures_with_no_source_retention() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, world) = setup(temp.path());
    let identity = request(&world, 1);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let mut task = RestoreTask::start(
        repository,
        Arc::clone(&catalogue),
        Limits::default(),
        Recovery::Strict,
        identity.clone(),
    )
    .unwrap();
    task.finish().unwrap();
    let RestorePoll::Failed(first) = task.try_poll() else {
        panic!("Busy load must fail")
    };
    assert!(matches!(
        &*first,
        RestoreError::Save(super::super::Error::Busy)
    ));
    let RestorePoll::Failed(second) = task.try_poll() else {
        panic!("failure must stay visible")
    };
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(Arc::strong_count(&catalogue), 2);
    drop(lock);
    let mut panicking =
        RestoreTask::spawn_with(identity, || panic!("injected inbound failure")).unwrap();
    panicking.finish().unwrap();
    assert!(
        matches!(panicking.try_poll(), RestorePoll::Failed(error) if matches!(&*error, RestoreError::Panicked))
    );
    assert!(
        matches!(panicking.try_poll(), RestorePoll::Failed(error) if matches!(&*error, RestoreError::Panicked))
    );
}

#[test]
fn wrong_declared_identity_and_noncanonical_descriptors_refuse_without_exposing_a_world() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, repository, world) = setup(temp.path());
    let expected: Snapshot = world.snapshot();
    let id = NonZeroU64::new(1).unwrap();
    for digest in ["", "a", &"a".repeat(65), &"G".repeat(64), &"A".repeat(64)] {
        assert!(matches!(
            RequestIdentity::new(id, world.campaign(), digest),
            Err(RestoreError::InvalidIdentity)
        ));
    }
    for identity in [
        RequestIdentity::new(id, world.campaign(), &"b".repeat(64)).unwrap(),
        RequestIdentity::new(
            id,
            CampaignId::from_bytes([0x22; 16]).unwrap(),
            world.catalogue_fingerprint(),
        )
        .unwrap(),
    ] {
        let mut task = RestoreTask::start(
            repository.clone(),
            Arc::clone(&catalogue),
            Limits::default(),
            Recovery::Strict,
            identity,
        )
        .unwrap();
        task.finish().unwrap();
        assert!(
            matches!(task.try_poll(), RestorePoll::Failed(error) if matches!(&*error, RestoreError::IdentityMismatch))
        );
        assert_eq!(world.snapshot(), expected);
        assert_eq!(Arc::strong_count(&catalogue), 2);
    }
}

#[test]
fn public_gate_holds_actual_read_until_release_and_dropped_admission_cancels_without_loading() {
    for release in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (catalogue, repository, world) = setup(temp.path());
        let before = bytes(repository.path());
        let expected = world.snapshot();
        let identity = request(&world, 1);
        let (mut task, admission) = RestoreTask::start_gated(
            repository.clone(),
            Arc::clone(&catalogue),
            Limits::default(),
            Recovery::Strict,
            identity.clone(),
        )
        .unwrap();
        assert!(matches!(task.try_poll(), RestorePoll::Pending));
        assert!(matches!(task.try_poll(), RestorePoll::Pending));
        assert_eq!(Arc::strong_count(&catalogue), 3);
        if release {
            admission.release().unwrap();
            task.finish().unwrap();
            let (restored, _) = ready(&mut task).take_for(&identity).unwrap();
            assert_eq!(restored.snapshot(), expected);
            drop(restored);
            assert!(!task.cancel());
        } else {
            drop(admission);
            task.finish().unwrap();
            assert!(matches!(task.try_poll(), RestorePoll::Cancelled));
        }
        assert_eq!(Arc::strong_count(&catalogue), 2);
        assert_eq!(bytes(repository.path()), before);
    }
}

#[test]
fn task_cancel_or_drop_releases_held_worker_while_external_admission_token_is_retained() {
    for explicit in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (catalogue, repository, world) = setup(temp.path());
        let before = bytes(repository.path());
        let identity = request(&world, 1);
        let weak = Arc::downgrade(&catalogue);
        drop(world);
        let (mut task, admission) = RestoreTask::start_gated(
            repository.clone(),
            catalogue,
            Limits::default(),
            Recovery::Strict,
            identity,
        )
        .unwrap();
        assert!(matches!(task.try_poll(), RestorePoll::Pending));
        let worker = task.thread.take().unwrap();
        if explicit {
            assert!(task.cancel());
            assert!(matches!(task.try_poll(), RestorePoll::Cancelled));
        }
        drop(task);
        let (done, completion) = mpsc::sync_channel(1);
        let joiner = std::thread::spawn(move || {
            worker.join().unwrap();
            done.send(()).unwrap();
        });
        // Timeout bounds a broken cancellation test, not disk timing. The token
        // remains alive throughout; a hidden sender copy would leave IO gated.
        completion.recv_timeout(Duration::from_secs(10)).unwrap();
        joiner.join().unwrap();
        assert!(weak.upgrade().is_none());
        assert!(matches!(admission.release(), Err(RestoreError::Cancelled)));
        assert_eq!(bytes(repository.path()), before);
    }
}
