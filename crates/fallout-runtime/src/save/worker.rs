//! One writer owns publication order. Accepted captures never borrow the world
//! and remain counted until their storage has been released, including the save
//! currently on disk. Each caller owns its result receiver; there is no growing
//! shared completion queue.
use super::{Captured, Repository, WriteReceipt};
mod admission;
#[cfg(test)]
mod source_context_tests;
use admission::{Admission, Permit};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
};
use std::thread::{self, JoinHandle};

const MAX_IN_FLIGHT: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("save worker capacity must be between 1 and {MAX_IN_FLIGHT}, got {0}")]
    Capacity(usize),
    #[error("save worker snapshot-byte budget must be positive, got {0}")]
    ByteBudget(usize),
    #[error("could not start native save worker: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("native save worker panicked; inspect outstanding tickets")]
    Panicked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Capacity,
    WorkerStopped,
}

/// Rejection transfers the original capture back, without cloning or changing
/// its boundary. The host decides whether to retry it or capture a later state.
#[derive(Debug)]
pub struct SubmitFailure {
    pub reason: Rejection,
    pub capture: Captured,
}
impl std::fmt::Display for SubmitFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "native save request rejected: {:?}", self.reason)
    }
}
impl std::error::Error for SubmitFailure {}

#[derive(Debug, thiserror::Error)]
pub enum CompletionError {
    #[error(transparent)]
    Save(#[from] super::Error),
    #[error("native save worker stopped before returning this request's result")]
    WorkerStopped,
    #[error("this native save result has already been collected")]
    AlreadyCollected,
}
type Completion = std::result::Result<WriteReceipt, CompletionError>;

#[derive(Debug)]
#[must_use = "collect the ticket to learn whether this save was published"]
pub struct SaveTicket {
    result: Receiver<super::Result<WriteReceipt>>,
    collected: bool,
}
impl SaveTicket {
    /// Poll from the host without waiting for filesystem work. A terminal
    /// success or error is delivered once; subsequent polls report consumption.
    pub fn try_wait(&mut self) -> std::result::Result<Option<WriteReceipt>, CompletionError> {
        if self.collected {
            return Err(CompletionError::AlreadyCollected);
        }
        match self.result.try_recv() {
            Ok(result) => {
                self.collected = true;
                result.map(Some).map_err(CompletionError::Save)
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                self.collected = true;
                Err(CompletionError::WorkerStopped)
            }
        }
    }
    /// Blocking collection is useful at shutdown or in an offline tool. A game
    /// frame should use `try_wait` instead.
    pub fn wait(self) -> Completion {
        if self.collected {
            return Err(CompletionError::AlreadyCollected);
        }
        self.result
            .recv()
            .map_err(|_| CompletionError::WorkerStopped)?
            .map_err(CompletionError::Save)
    }
}

struct Job {
    capture: Captured,
    result: SyncSender<super::Result<WriteReceipt>>,
    permit: Permit,
}

/// A bounded FIFO for one repository. Capacity includes the active write.
/// Count and declared snapshot byte reservations include active and queued jobs.
/// These are logical serialized bounds, not a process memory ceiling. Admission
/// does not encode a snapshot. The worker never coalesces or retries saves.
pub struct SaveWorker {
    sender: Option<SyncSender<Job>>,
    thread: Option<JoinHandle<()>>,
    admission: Arc<Admission>,
}
impl SaveWorker {
    pub const DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;

    pub fn start(repository: Repository, max_in_flight: usize) -> Result<Self, WorkerError> {
        Self::start_with_budget(
            repository,
            max_in_flight,
            Self::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES,
        )
    }
    /// Reserve each capture's max_snapshot_bytes until its storage is released.
    /// Configure a positive aggregate limit; count and byte admission are atomic.
    pub fn start_with_budget(
        repository: Repository,
        max_in_flight: usize,
        maximum_reserved_snapshot_bytes: usize,
    ) -> Result<Self, WorkerError> {
        Self::spawn_with(
            max_in_flight,
            maximum_reserved_snapshot_bytes,
            move |capture| repository.commit(capture),
        )
    }
    fn spawn_with(
        maximum: usize,
        maximum_bytes: usize,
        mut commit: impl FnMut(&Captured) -> super::Result<WriteReceipt> + Send + 'static,
    ) -> Result<Self, WorkerError> {
        if !(1..=MAX_IN_FLIGHT).contains(&maximum) {
            return Err(WorkerError::Capacity(maximum));
        }
        if maximum_bytes == 0 {
            return Err(WorkerError::ByteBudget(maximum_bytes));
        }
        let (sender, receiver) = mpsc::sync_channel::<Job>(maximum);
        let admission = Arc::new(Admission::new(maximum, maximum_bytes));
        let thread = thread::Builder::new()
            .name("fallout-native-save".into())
            .spawn(move || {
                for job in receiver {
                    // Keep fields together across the fallible call. On panic,
                    // struct field order releases capture before its permit.
                    let receipt = commit(&job.capture);
                    let Job {
                        capture,
                        result,
                        permit,
                    } = job;
                    // Make admission available only after releasing the owned
                    // snapshot. Result receivers never retain a capture.
                    drop(capture);
                    drop(permit);
                    // Dropping a ticket does not cancel an accepted save. This
                    // one-element channel cannot block on an unread result.
                    let _ = result.send(receipt);
                }
            })
            .map_err(WorkerError::Spawn)?;
        Ok(Self {
            sender: Some(sender),
            thread: Some(thread),
            admission,
        })
    }
    pub fn try_submit(&mut self, capture: Captured) -> Result<SaveTicket, Box<SubmitFailure>> {
        let Some(sender) = &self.sender else {
            return Err(Box::new(SubmitFailure {
                reason: Rejection::WorkerStopped,
                capture,
            }));
        };
        let Some(permit) = self.admission.reserve(capture.limits().max_snapshot_bytes) else {
            return Err(Box::new(SubmitFailure {
                reason: Rejection::Capacity,
                capture,
            }));
        };
        let (result, receiver) = mpsc::sync_channel(1);
        match sender.try_send(Job {
            capture,
            result,
            permit,
        }) {
            Ok(()) => Ok(SaveTicket {
                result: receiver,
                collected: false,
            }),
            Err(error) => {
                let (reason, job) = match error {
                    TrySendError::Full(job) => (Rejection::Capacity, job),
                    TrySendError::Disconnected(job) => (Rejection::WorkerStopped, job),
                };
                Err(Box::new(SubmitFailure {
                    reason,
                    capture: job.capture,
                }))
            }
        }
    }
    /// Close admission, drain accepted requests and join the thread. Tickets
    /// still contain their individual results after this returns. Success here
    /// confirms joining, not successful publication of every request. Call
    /// outside the frame loop: a slow filesystem can delay shutdown.
    pub fn finish(mut self) -> Result<(), WorkerError> {
        self.drain()
    }
    fn drain(&mut self) -> Result<(), WorkerError> {
        self.sender.take();
        match self.thread.take() {
            Some(thread) => thread.join().map_err(|_| WorkerError::Panicked),
            None => Ok(()),
        }
    }
}
impl Drop for SaveWorker {
    fn drop(&mut self) {
        // An ordinary early return must not detach the writer or discard jobs.
        // Explicit finish is preferred because it reports a worker panic.
        let _ = self.drain();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Limits, events::Clocks, identity::CampaignId, save::format, snapshot::Snapshot};
    use fallout_data::identity::ProfileId;
    use std::time::Duration;

    fn capture(revision: u64) -> Captured {
        Captured::from_parts(
            Snapshot {
                reference_states: Vec::new(),
                schema_version: crate::snapshot::SCHEMA_VERSION,
                campaign: CampaignId::from_bytes([1; 16]).unwrap(),
                state_revision: revision,
                profile: ProfileId::NvOriginal,
                catalogue_sha256: "00".repeat(32),
                next_item: 1,
                inventory_banks: Vec::new(),
                next_instance: 1,
                next_reference: 1,
                next_event_sequence: 1,
                clocks: Clocks {
                    tick: revision,
                    ..Clocks::default()
                },
                references: Vec::new(),
                instances: Vec::new(),
                pending_events: Vec::new(),
            },
            Limits::default(),
            Default::default(),
        )
    }
    fn bounded_capture(revision: u64, maximum_bytes: usize) -> Captured {
        let mut capture = capture(revision);
        capture.limits_mut().max_snapshot_bytes = maximum_bytes;
        capture
    }
    fn repository(path: &std::path::Path) -> Repository {
        Repository::create(path, &[], capture(1).snapshot().clone().campaign).unwrap()
    }
    fn slot(repository: &Repository, name: &str) -> format::Decoded {
        format::decode(
            &std::fs::read(repository.path().join(name)).unwrap(),
            Limits::default(),
        )
        .unwrap()
    }
    // Handshakes hold actual repository publication at a known point. These
    // tests do not assume that a thread or disk operation takes a minimum time.
    fn gated(repository: Repository, maximum: usize) -> (SaveWorker, Receiver<()>, SyncSender<()>) {
        gated_with_budget(
            repository,
            maximum,
            SaveWorker::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES,
        )
    }
    fn gated_with_budget(
        repository: Repository,
        maximum: usize,
        maximum_bytes: usize,
    ) -> (SaveWorker, Receiver<()>, SyncSender<()>) {
        let (entered, entries) = mpsc::sync_channel(1);
        let (release, releases) = mpsc::sync_channel(1);
        let worker = SaveWorker::spawn_with(maximum, maximum_bytes, move |capture| {
            repository.commit_observing(capture, |stage| {
                if stage == super::super::Stage::CurrentTempWritten {
                    entered.send(()).unwrap();
                    releases.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            })
        })
        .unwrap();
        (worker, entries, release)
    }
    #[test]
    fn active_and_queued_captures_share_the_bound_and_rejection_returns_exact_state() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(&directory.path().join("native"));
        let (mut worker, entered, release) = gated(repository.clone(), 2);
        let mut first = worker.try_submit(capture(1)).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        let second = worker.try_submit(capture(2)).unwrap();
        let third = capture(3);
        let expected = third.snapshot().clone();
        let rejected = worker.try_submit(third).unwrap_err();
        assert_eq!(rejected.reason, Rejection::Capacity);
        assert_eq!(rejected.capture.snapshot(), &expected);
        assert!(first.try_wait().unwrap().is_none());
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(first.wait().unwrap().metadata.generation, 1);
        let third = worker.try_submit(rejected.capture).unwrap();
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(second.wait().unwrap().metadata.generation, 2);
        release.send(()).unwrap();
        assert_eq!(third.wait().unwrap().metadata.generation, 3);
        worker.finish().unwrap();
        assert_eq!(slot(&repository, "current.frsv").snapshot, expected);
        assert_eq!(
            slot(&repository, "previous.frsv").snapshot.state_revision,
            2
        );
    }
    #[test]
    fn host_status_poll_does_not_wait_for_an_actual_gated_publication() {
        use super::super::{SaveState, SaveStatus};
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(&directory.path().join("native"));
        let (mut worker, entered, release) = gated(repository, 1);
        let mut status = SaveStatus::new(worker.try_submit(capture(1)).unwrap());
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        // Publication cannot finish until after these polls. No timing threshold
        // or racing "slow disk" assumption is needed to prove pending behavior.
        assert!(matches!(status.poll(), SaveState::Pending));
        assert!(matches!(status.poll(), SaveState::Pending));
        release.send(()).unwrap();
        worker.finish().unwrap();
        assert!(matches!(status.poll(), SaveState::Published(_)));
        assert_eq!(status.wait().unwrap().metadata.generation, 1);
    }
    #[test]
    fn host_status_retains_worker_stopped_after_actual_writer_panic() {
        use super::super::{SaveState, SaveStatus};
        let mut worker =
            SaveWorker::spawn_with(1, 4096, |_| panic!("injected host-status writer failure"))
                .unwrap();
        let mut status = SaveStatus::new(worker.try_submit(bounded_capture(1, 4096)).unwrap());
        assert!(matches!(worker.finish(), Err(WorkerError::Panicked)));
        assert!(matches!(
            status.poll(),
            SaveState::Failed(CompletionError::WorkerStopped)
        ));
        assert!(matches!(
            status.poll(),
            SaveState::Failed(CompletionError::WorkerStopped)
        ));
        assert!(matches!(status.wait(), Err(CompletionError::WorkerStopped)));
    }
    #[test]
    fn dropped_tickets_do_not_cancel_saves_and_drop_drains_the_queue() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(&directory.path().join("native"));
        let (mut worker, entered, release) = gated(repository.clone(), 2);
        let admission = Arc::clone(&worker.admission);
        drop(worker.try_submit(capture(1)).unwrap());
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        drop(worker.try_submit(capture(2)).unwrap());
        assert_eq!(
            admission.usage(),
            (2, 2 * Limits::default().max_snapshot_bytes)
        );
        let shutdown = thread::spawn(move || drop(worker));
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        release.send(()).unwrap();
        shutdown.join().unwrap();
        assert_eq!(admission.usage(), (0, 0));
        assert_eq!(slot(&repository, "current.frsv").snapshot.state_revision, 2);
        assert_eq!(
            slot(&repository, "previous.frsv").snapshot.state_revision,
            1
        );
    }
    #[test]
    fn panic_is_reported_on_tickets_shutdown_and_later_submission() {
        let (entered, entries) = mpsc::sync_channel(1);
        let (release, releases) = mpsc::sync_channel(1);
        let mut worker = SaveWorker::spawn_with(
            2,
            SaveWorker::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES,
            move |_| {
                entered.send(()).unwrap();
                releases.recv_timeout(Duration::from_secs(10)).unwrap();
                panic!("injected worker failure")
            },
        )
        .unwrap();
        let admission = Arc::clone(&worker.admission);
        let ticket = worker.try_submit(capture(1)).unwrap();
        entries.recv_timeout(Duration::from_secs(10)).unwrap();
        let queued = worker.try_submit(capture(2)).unwrap();
        release.send(()).unwrap();
        assert!(matches!(ticket.wait(), Err(CompletionError::WorkerStopped)));
        // The queued result closes when the receiver is torn down. Waiting for
        // it avoids assuming that the active result implies thread termination.
        assert!(matches!(queued.wait(), Err(CompletionError::WorkerStopped)));
        let rejected = worker.try_submit(capture(3)).unwrap_err();
        assert_eq!(rejected.reason, Rejection::WorkerStopped);
        assert_eq!(rejected.capture.snapshot().state_revision, 3);
        assert!(matches!(worker.finish(), Err(WorkerError::Panicked)));
        assert_eq!(admission.usage(), (0, 0));
    }
    #[test]
    fn invalid_capacity_is_rejected_before_starting_a_writer() {
        for capacity in [0, MAX_IN_FLIGHT + 1, usize::MAX] {
            assert!(
                matches!(SaveWorker::spawn_with(capacity, SaveWorker::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES, |_| unreachable!()),
                Err(WorkerError::Capacity(actual)) if actual == capacity)
            );
        }
        assert!(matches!(
            SaveWorker::spawn_with(1, 0, |_| unreachable!()),
            Err(WorkerError::ByteBudget(0))
        ));
    }

    #[test]
    fn mixed_declared_budgets_bound_active_and_queued_captures_and_retry_intact() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(&directory.path().join("native"));
        let (mut worker, entered, release) = gated_with_budget(repository.clone(), 8, 3000);
        let admission = Arc::clone(&worker.admission);
        let first = worker.try_submit(bounded_capture(1, 1000)).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        let second = worker.try_submit(bounded_capture(2, 2000)).unwrap();
        assert_eq!(admission.usage(), (2, 3000));
        let third = bounded_capture(3, 1000);
        let expected = third.snapshot().clone();
        let original_pointer = third.snapshot().catalogue_sha256.as_ptr();
        let rejected = worker.try_submit(third).unwrap_err();
        assert_eq!(rejected.reason, Rejection::Capacity);
        assert_eq!(rejected.capture.snapshot(), &expected);
        assert_eq!(
            rejected.capture.snapshot().catalogue_sha256.as_ptr(),
            original_pointer
        );
        assert_eq!(rejected.capture.limits().max_snapshot_bytes, 1000);
        assert_eq!(admission.usage(), (2, 3000));
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(first.wait().unwrap().metadata.generation, 1);
        assert_eq!(admission.usage(), (1, 2000));
        let third = worker.try_submit(rejected.capture).unwrap();
        assert_eq!(admission.usage(), (2, 3000));
        release.send(()).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(second.wait().unwrap().metadata.generation, 2);
        assert_eq!(admission.usage(), (1, 1000));
        release.send(()).unwrap();
        assert_eq!(third.wait().unwrap().metadata.generation, 3);
        worker.finish().unwrap();
        assert_eq!(admission.usage(), (0, 0));
        assert_eq!(slot(&repository, "current.frsv").snapshot, expected);
        assert_eq!(
            slot(&repository, "previous.frsv").snapshot.state_revision,
            2
        );
    }

    #[test]
    fn oversized_declared_capture_rejects_before_publication_with_no_reservation() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(&directory.path().join("native"));
        let mut worker = SaveWorker::start_with_budget(repository.clone(), 8, 1000).unwrap();
        for maximum in [1001, usize::MAX] {
            let capture = bounded_capture(1, maximum);
            let expected = capture.snapshot().clone();
            let original_pointer = capture.snapshot().catalogue_sha256.as_ptr();
            let rejected = worker.try_submit(capture).unwrap_err();
            assert_eq!(rejected.reason, Rejection::Capacity);
            assert_eq!(rejected.capture.snapshot(), &expected);
            assert_eq!(
                rejected.capture.snapshot().catalogue_sha256.as_ptr(),
                original_pointer
            );
            assert_eq!(rejected.capture.limits().max_snapshot_bytes, maximum);
            assert_eq!(worker.admission.usage(), (0, 0));
        }
        assert!(!repository.path().join("current.frsv").exists());
        let retry = worker.try_submit(bounded_capture(1, 1000)).unwrap();
        worker.finish().unwrap();
        assert_eq!(retry.wait().unwrap().metadata.generation, 1);
    }

    #[test]
    fn writer_errors_release_budget_before_results_and_do_not_stop_retry() {
        let directory = tempfile::tempdir().unwrap();
        let repository = repository(&directory.path().join("native"));
        let mut worker = SaveWorker::start_with_budget(repository.clone(), 2, 1000).unwrap();
        let admission = Arc::clone(&worker.admission);
        let failed = worker.try_submit(bounded_capture(1, 1)).unwrap();
        assert!(
            matches!(failed.wait(), Err(CompletionError::Save(super::super::Error::State(crate::Error::Json(error)))) if error.to_string().contains("snapshot byte budget exceeded"))
        );
        assert_eq!(admission.usage(), (0, 0));
        assert!(!repository.path().join("current.frsv").exists());
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(repository.path().join("writer.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let busy = worker.try_submit(bounded_capture(1, 1000)).unwrap();
        assert!(matches!(
            busy.wait(),
            Err(CompletionError::Save(super::super::Error::Busy))
        ));
        assert_eq!(admission.usage(), (0, 0));
        drop(lock);
        let retry = worker.try_submit(bounded_capture(1, 1000)).unwrap();
        worker.finish().unwrap();
        assert_eq!(retry.wait().unwrap().metadata.generation, 1);
        assert_eq!(admission.usage(), (0, 0));
    }

    #[test]
    fn mixed_budget_panic_and_disconnected_send_release_all_reservations() {
        let (entered, entries) = mpsc::sync_channel(1);
        let (release, releases) = mpsc::sync_channel(1);
        let mut worker = SaveWorker::spawn_with(2, 3000, move |_| {
            entered.send(()).unwrap();
            releases.recv_timeout(Duration::from_secs(10)).unwrap();
            panic!("injected mixed budget failure")
        })
        .unwrap();
        let admission = Arc::clone(&worker.admission);
        let active = worker.try_submit(bounded_capture(1, 1000)).unwrap();
        entries.recv_timeout(Duration::from_secs(10)).unwrap();
        let queued = worker.try_submit(bounded_capture(2, 2000)).unwrap();
        assert_eq!(admission.usage(), (2, 3000));
        release.send(()).unwrap();
        assert!(matches!(active.wait(), Err(CompletionError::WorkerStopped)));
        assert!(matches!(queued.wait(), Err(CompletionError::WorkerStopped)));
        let capture = bounded_capture(3, 1000);
        let expected = capture.snapshot().clone();
        let original_pointer = capture.snapshot().catalogue_sha256.as_ptr();
        let rejected = worker.try_submit(capture).unwrap_err();
        assert_eq!(rejected.reason, Rejection::WorkerStopped);
        assert_eq!(rejected.capture.snapshot(), &expected);
        assert_eq!(
            rejected.capture.snapshot().catalogue_sha256.as_ptr(),
            original_pointer
        );
        assert!(matches!(worker.drain(), Err(WorkerError::Panicked)));
        assert_eq!(admission.usage(), (0, 0));
        let stopped = worker.try_submit(bounded_capture(4, 1000)).unwrap_err();
        assert_eq!(stopped.reason, Rejection::WorkerStopped);
        assert_eq!(admission.usage(), (0, 0));
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
}
