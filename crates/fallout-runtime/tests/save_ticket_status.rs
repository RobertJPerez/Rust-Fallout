mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    save::{
        self, Captured, CompletionError, Recovery, Repository, SaveState, SaveStatus, SaveWorker,
    },
};
use std::fs;

#[test]
fn host_status_requires_a_receipt_and_keeps_it_after_orderly_shutdown() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = World::new(&catalogue, Limits::default()).unwrap();
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let ticket = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let mut status = SaveStatus::new(ticket);
    assert!(matches!(status.state(), SaveState::Pending));
    worker.finish().unwrap();
    // Draining alone does not change the host's observed status.
    assert!(matches!(status.state(), SaveState::Pending));
    let SaveState::Published(receipt) = status.poll() else {
        panic!("missing publication receipt")
    };
    assert_eq!(receipt.metadata.generation, 1);
    assert_eq!(receipt.metadata.state_revision, world.revision());
    let digest = receipt.metadata.container_sha256.clone();
    for _ in 0..3 {
        let SaveState::Published(receipt) = status.poll() else {
            panic!("published status was lost")
        };
        assert_eq!(receipt.metadata.container_sha256, digest);
    }
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
fn failed_publication_is_sticky_and_a_new_ticket_can_report_a_successful_retry() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = World::new(&catalogue, Limits::default()).unwrap();
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let mut failed = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    worker.finish().unwrap();
    for _ in 0..3 {
        assert!(matches!(
            failed.poll(),
            SaveState::Failed(CompletionError::Save(save::Error::Busy))
        ));
    }
    assert!(!repository.path().join("current.frsv").exists());
    drop(lock);
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let mut retried = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    worker.finish().unwrap();
    assert!(matches!(retried.poll(), SaveState::Published(_)));
    assert!(matches!(
        failed.poll(),
        SaveState::Failed(CompletionError::Save(save::Error::Busy))
    ));
    assert!(matches!(
        failed.wait(),
        Err(CompletionError::Save(save::Error::Busy))
    ));
}

#[test]
fn consumed_tickets_cannot_be_presented_as_saved_and_dropping_status_does_not_cancel() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = World::new(&catalogue, Limits::default()).unwrap();
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let mut ticket = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    worker.finish().unwrap();
    ticket.try_wait().unwrap().unwrap();
    let mut consumed = SaveStatus::new(ticket);
    assert!(matches!(
        consumed.poll(),
        SaveState::Failed(CompletionError::AlreadyCollected)
    ));
    assert!(matches!(
        consumed.poll(),
        SaveState::Failed(CompletionError::AlreadyCollected)
    ));
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    drop(SaveStatus::new(
        worker.try_submit(Captured::at_boundary(&world)).unwrap(),
    ));
    worker.finish().unwrap();
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .1
            .metadata
            .generation,
        2
    );
}
