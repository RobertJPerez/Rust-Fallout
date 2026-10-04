use super::*;
use crate::{
    World,
    save::{Captured, test_source::*},
};
use std::{fs, path::Path, sync::mpsc, time::Duration};

fn setup(root: &Path) -> (Arc<Catalogue>, Repository, AvailabilityRequest) {
    write_fixture(root, false);
    let catalogue = Arc::new(load(root, &["FalloutNV.esm"]));
    let world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes([0x32; 16]).unwrap(),
    )
    .unwrap();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let request = AvailabilityRequest::new(
        1.try_into().unwrap(),
        world.campaign(),
        world.catalogue_fingerprint(),
    )
    .unwrap();
    (catalogue, repository, request)
}
fn launch(job: Job) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new().spawn(job)
}
fn candidate(task: &mut AvailabilityTask) -> AvailabilityCandidate {
    match task.try_poll() {
        AvailabilityPoll::Ready(candidate) => *candidate,
        state => panic!("{state:?}"),
    }
}

#[test]
fn held_gate_poll_is_nonblocking_and_cancel_drop_close_sender_without_source_retention() {
    for action in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let (catalogue, repository, request) = setup(root.path());
        let weak = Arc::downgrade(&catalogue);
        let before = fs::read(repository.path().join("current.frsv")).unwrap();
        let (mut task, admission) = AvailabilityTask::start_gated(
            repository.clone(),
            catalogue,
            Limits::default(),
            request.clone(),
        )
        .unwrap();
        assert!(matches!(task.try_poll(), AvailabilityPoll::Pending));
        assert!(matches!(task.try_poll(), AvailabilityPoll::Pending));
        assert_eq!(weak.strong_count(), 1);
        if action == 0 {
            admission.release().unwrap();
            task.finish().unwrap();
            assert!(weak.upgrade().is_none());
            let report = candidate(&mut task).take_for(&request).unwrap();
            assert!(matches!(
                report.current(),
                super::super::SlotAvailability::Loadable { .. }
            ));
            assert!(matches!(task.try_poll(), AvailabilityPoll::Delivered));
            assert!(!task.cancel());
        } else if action == 1 {
            drop(admission);
            task.finish().unwrap();
            assert!(matches!(task.try_poll(), AvailabilityPoll::Cancelled));
        } else {
            let worker = task.thread.take().unwrap();
            if action == 2 {
                assert!(task.cancel());
                assert!(!task.cancel());
            }
            drop(task);
            let (done, completion) = mpsc::sync_channel(1);
            let joiner = thread::spawn(move || {
                worker.join().unwrap();
                done.send(()).unwrap();
            });
            completion.recv_timeout(Duration::from_secs(10)).unwrap();
            joiner.join().unwrap();
            assert!(matches!(
                admission.release(),
                Err(AvailabilityError::Cancelled)
            ));
        }
        assert!(weak.upgrade().is_none());
        assert_eq!(
            fs::read(repository.path().join("current.frsv")).unwrap(),
            before
        );
    }
}
#[test]
fn delivered_unadmitted_report_is_revoked_by_task_cancel_drop_or_superseding_identity() {
    for action in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let (catalogue, repository, request) = setup(root.path());
        let weak = Arc::downgrade(&catalogue);
        let mut task =
            AvailabilityTask::start(repository, catalogue, Limits::default(), request.clone())
                .unwrap();
        task.finish().unwrap();
        assert!(weak.upgrade().is_none());
        let report = candidate(&mut task);
        assert_eq!(report.request(), &request);
        assert!(matches!(task.try_poll(), AvailabilityPoll::Delivered));
        if action == 0 {
            assert!(task.cancel());
            assert!(matches!(
                report.take_for(&request),
                Err(AvailabilityError::Cancelled)
            ));
        } else if action == 1 {
            drop(task);
            assert!(matches!(
                report.take_for(&request),
                Err(AvailabilityError::Cancelled)
            ));
        } else if action == 2 {
            let superseded = AvailabilityRequest::new(
                2.try_into().unwrap(),
                request.campaign(),
                request.catalogue_fingerprint(),
            )
            .unwrap();
            assert!(matches!(
                report.take_for(&superseded),
                Err(AvailabilityError::Superseded)
            ));
        } else {
            let report = report.take_for(&request).unwrap();
            assert!(!task.cancel());
            drop(task);
            assert_eq!(report.campaign(), request.campaign());
        }
    }
}
#[test]
fn abandoned_in_progress_inspector_is_detached_and_drops_result_and_source_after_io_finishes() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, request) = setup(root.path());
    let weak = Arc::downgrade(&catalogue);
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let (task, _) = AvailabilityTask::spawn_with(
        request,
        move || {
            entered.send(()).unwrap();
            held.recv().unwrap();
            repository.inspect_availability(catalogue, Limits::default())
        },
        false,
        launch,
    )
    .unwrap();
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut task = task;
    let worker = task.thread.take().unwrap();
    let (done, completion) = mpsc::sync_channel(1);
    let dropper = thread::spawn(move || {
        drop(task);
        done.send(()).unwrap();
    });
    let dropped = completion.recv_timeout(Duration::from_secs(10));
    // Always release on a broken-drop timeout, so the test cleans up its job.
    release.send(()).unwrap();
    worker.join().unwrap();
    dropper.join().unwrap();
    dropped.unwrap();
    assert!(weak.upgrade().is_none());
}
#[test]
fn panic_spawn_failure_and_disconnection_are_typed_and_release_captured_source() {
    let root = tempfile::tempdir().unwrap();
    for action in 0..3 {
        let fixture = tempfile::tempdir_in(root.path()).unwrap();
        let (catalogue, _repository, request) = setup(fixture.path());
        let weak = Arc::downgrade(&catalogue);
        let inspect = move || {
            let _owned_source = catalogue;
            panic!("injected availability inspection panic");
        };
        let spawn = move |job: Job| match action {
            0 => launch(job),
            1 => {
                drop(job);
                Err(std::io::Error::other("injected spawn failure"))
            }
            _ => {
                drop(job);
                launch(Box::new(|| {}))
            }
        };
        let result = AvailabilityTask::spawn_with(request, inspect, false, spawn);
        if action == 1 {
            assert!(matches!(result, Err(AvailabilityError::Spawn(_))));
        } else {
            let (mut task, _) = result.unwrap();
            task.finish().unwrap();
            let AvailabilityPoll::Failed(first) = task.try_poll() else {
                panic!("failure required")
            };
            assert!(if action == 0 {
                matches!(&*first, AvailabilityError::Panicked)
            } else {
                matches!(&*first, AvailabilityError::WorkerStopped)
            });
            let AvailabilityPoll::Failed(second) = task.try_poll() else {
                panic!("sticky failure required")
            };
            assert!(Arc::ptr_eq(&first, &second));
        }
        assert!(weak.upgrade().is_none());
    }
}
#[test]
fn identity_validation_and_actual_report_mismatch_never_expose_report_or_retain_source() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, request) = setup(root.path());
    for digest in ["", "a", &"a".repeat(65), &"G".repeat(64), &"A".repeat(64)] {
        assert!(matches!(
            AvailabilityRequest::new(1.try_into().unwrap(), request.campaign(), digest),
            Err(AvailabilityError::InvalidIdentity)
        ));
    }
    for wrong in [
        AvailabilityRequest::new(1.try_into().unwrap(), request.campaign(), &"f".repeat(64))
            .unwrap(),
        AvailabilityRequest::new(
            1.try_into().unwrap(),
            CampaignId::from_bytes([99; 16]).unwrap(),
            request.catalogue_fingerprint(),
        )
        .unwrap(),
    ] {
        let mut task = AvailabilityTask::start(
            repository.clone(),
            Arc::clone(&catalogue),
            Limits::default(),
            wrong,
        )
        .unwrap();
        task.finish().unwrap();
        assert!(
            matches!(task.try_poll(),AvailabilityPoll::Failed(error) if matches!(&*error,AvailabilityError::IdentityMismatch))
        );
        assert_eq!(Arc::strong_count(&catalogue), 1);
    }
}
#[test]
fn cancellation_after_inspection_started_discards_complete_observation_without_source_retention() {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, repository, request) = setup(root.path());
    let weak = Arc::downgrade(&catalogue);
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, held) = mpsc::sync_channel(1);
    let (mut task, _) = AvailabilityTask::spawn_with(
        request,
        move || {
            let report = repository.inspect_availability(catalogue, Limits::default())?;
            entered.send(()).unwrap();
            held.recv().unwrap();
            Ok(report)
        },
        false,
        launch,
    )
    .unwrap();
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(weak.upgrade().is_none());
    assert!(task.cancel());
    release.send(()).unwrap();
    task.finish().unwrap();
    assert!(matches!(task.try_poll(), AvailabilityPoll::Cancelled));
}
