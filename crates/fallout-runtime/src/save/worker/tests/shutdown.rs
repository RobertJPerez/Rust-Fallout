//! New host shutdown API tests; existing blocking finish/Drop tests stay intact.
use super::*;

fn joined(worker: &mut SaveWorker) -> Result<bool, WorkerError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match worker.try_shutdown() {
            Ok(false) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "shutdown poll deadline"
                );
                thread::yield_now();
            }
            outcome => return outcome,
        }
    }
}
#[test]
fn close_and_pending_shutdown_keep_both_accepted_fifo_writes_and_refuse_third_intact() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(&directory.path().join("native"));
    let (mut worker, entered, release) = gated(repository.clone(), 2);
    let admission = Arc::clone(&worker.admission);
    let mut first = worker.try_submit(bounded_capture(1, 4096)).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut second = worker.try_submit(bounded_capture(2, 4096)).unwrap();
    assert!(!worker.try_shutdown().unwrap()); // polling itself does not close
    assert!(worker.sender.is_some());
    assert!(worker.close_admission());
    assert!(!worker.close_admission());
    let third = bounded_capture(3, 4096);
    let expected = third.snapshot().clone();
    let returned = worker.try_submit(third).unwrap_err();
    assert_eq!(returned.reason, Rejection::WorkerStopped);
    assert_eq!(returned.capture.snapshot(), &expected);
    assert_eq!(returned.capture.limits().max_snapshot_bytes, 4096);
    drop(returned);
    assert_eq!(admission.usage(), (2, 8192));
    for _ in 0..2 {
        assert!(!worker.try_shutdown().unwrap());
        assert!(first.try_wait().unwrap().is_none());
        assert!(second.try_wait().unwrap().is_none());
    }
    release.send(()).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(first.try_wait().unwrap().unwrap().metadata.generation, 1);
    assert!(!worker.try_shutdown().unwrap());
    assert_eq!(
        slot(&repository, "current.frsv").snapshot,
        capture(1).snapshot().clone()
    );
    release.send(()).unwrap();
    assert!(joined(&mut worker).unwrap());
    assert!(worker.thread.is_none());
    assert_eq!(admission.usage(), (0, 0));
    let terminal = std::fs::read(repository.path().join("current.frsv")).unwrap();
    for _ in 0..3 {
        assert!(worker.try_shutdown().unwrap());
        assert!(!worker.close_admission());
    }
    assert_eq!(second.try_wait().unwrap().unwrap().metadata.generation, 2);
    assert!(matches!(
        second.try_wait(),
        Err(CompletionError::AlreadyCollected)
    ));
    worker.finish().unwrap();
    assert_eq!(
        std::fs::read(repository.path().join("current.frsv")).unwrap(),
        terminal
    );
    assert_eq!(
        slot(&repository, "current.frsv").snapshot,
        capture(2).snapshot().clone()
    );
    assert_eq!(
        slot(&repository, "previous.frsv").snapshot,
        capture(1).snapshot().clone()
    );
}
#[test]
fn clean_join_is_distinct_from_failed_and_successful_individual_tickets() {
    use super::super::super::{SaveState, SaveStatus};
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(&directory.path().join("native"));
    let (mut worker, entered, release) = gated(repository.clone(), 2);
    let mut failed = SaveStatus::new(worker.try_submit(bounded_capture(1, 1)).unwrap());
    let mut published = SaveStatus::new(worker.try_submit(bounded_capture(2, 4096)).unwrap());
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(worker.close_admission());
    assert!(!worker.try_shutdown().unwrap());
    assert!(matches!(
        failed.poll(),
        SaveState::Failed(CompletionError::Save(_))
    ));
    assert!(matches!(published.poll(), SaveState::Pending));
    release.send(()).unwrap();
    assert!(joined(&mut worker).unwrap());
    assert!(matches!(
        failed.poll(),
        SaveState::Failed(CompletionError::Save(_))
    ));
    assert!(matches!(published.poll(), SaveState::Published(_)));
    assert_eq!(published.wait().unwrap().metadata.generation, 1);
    assert!(failed.wait().is_err());
    worker.finish().unwrap();
    assert!(!repository.path().join("previous.frsv").exists());
}
#[test]
fn joined_panic_is_latched_once_and_all_active_queued_permits_and_tickets_release() {
    let (entered, entries) = mpsc::sync_channel(1);
    let (release, releases) = mpsc::sync_channel(1);
    let mut worker = SaveWorker::spawn_with(2, 8192, move |_| {
        entered.send(()).unwrap();
        releases.recv_timeout(Duration::from_secs(10)).unwrap();
        panic!("injected shutdown writer panic")
    })
    .unwrap();
    let admission = Arc::clone(&worker.admission);
    let mut first = worker.try_submit(bounded_capture(1, 4096)).unwrap();
    entries.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut second = worker.try_submit(bounded_capture(2, 4096)).unwrap();
    assert!(worker.close_admission());
    assert!(!worker.try_shutdown().unwrap());
    release.send(()).unwrap();
    assert!(matches!(joined(&mut worker), Err(WorkerError::Panicked)));
    assert!(worker.thread.is_none());
    assert_eq!(admission.usage(), (0, 0));
    for _ in 0..3 {
        assert!(matches!(worker.try_shutdown(), Err(WorkerError::Panicked)));
    }
    assert!(matches!(
        first.try_wait(),
        Err(CompletionError::WorkerStopped)
    ));
    assert!(matches!(
        second.try_wait(),
        Err(CompletionError::WorkerStopped)
    ));
    assert!(matches!(worker.finish(), Err(WorkerError::Panicked)));
    assert_eq!(admission.usage(), (0, 0));
}
#[test]
fn public_observer_is_same_writer_and_panic_discards_owned_temp_without_publishing_success() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(&directory.path().join("native"));
    let mut worker = SaveWorker::start_observing(repository.clone(), 1, 4096, |stage| {
        if stage == super::super::super::Stage::CurrentTempWritten {
            panic!("injected stage observer panic")
        }
    })
    .unwrap();
    let admission = Arc::clone(&worker.admission);
    let ticket = worker.try_submit(bounded_capture(1, 4096)).unwrap();
    worker.close_admission();
    assert!(matches!(joined(&mut worker), Err(WorkerError::Panicked)));
    assert!(matches!(ticket.wait(), Err(CompletionError::WorkerStopped)));
    assert_eq!(admission.usage(), (0, 0));
    assert!(!repository.path().join("current.frsv").exists());
    assert_eq!(std::fs::read_dir(repository.path()).unwrap().count(), 2);
}
#[test]
fn close_before_work_joins_cleanly_and_drop_after_pending_poll_still_drains_accepted_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(&directory.path().join("native"));
    let mut empty = SaveWorker::start(repository.clone(), 1).unwrap();
    assert!(empty.close_admission());
    assert!(joined(&mut empty).unwrap());
    empty.finish().unwrap();
    assert!(!repository.path().join("current.frsv").exists());
    let (mut worker, entered, release) = gated(repository.clone(), 2);
    let first = worker.try_submit(bounded_capture(1, 4096)).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let second = worker.try_submit(bounded_capture(2, 4096)).unwrap();
    worker.close_admission();
    assert!(!worker.try_shutdown().unwrap());
    drop((first, second));
    let shutdown = thread::spawn(move || drop(worker));
    release.send(()).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    release.send(()).unwrap();
    shutdown.join().unwrap();
    assert_eq!(
        slot(&repository, "current.frsv").snapshot,
        capture(2).snapshot().clone()
    );
    assert_eq!(
        slot(&repository, "previous.frsv").snapshot,
        capture(1).snapshot().clone()
    );
}

#[test]
fn queued_capture_retains_one_shared_payload_until_result_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let repository = repository(&directory.path().join("native"));
    let (mut worker, entered, release) = gated(repository, 1);
    let capture = bounded_capture(1, 4096);
    let payload = Arc::downgrade(&capture.payload);
    let ticket = worker.try_submit(capture).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();

    assert_eq!(payload.strong_count(), 1);
    assert!(payload.upgrade().is_some());
    release.send(()).unwrap();
    worker.finish().unwrap();
    assert_eq!(ticket.wait().unwrap().metadata.generation, 1);
    assert!(payload.upgrade().is_none());
}
