//! One read-only availability job. Slot observations remain separate moments;
//! an admitted report neither selects a save nor replaces the active World.
use super::{Repository, SlotAvailabilityReport};
use crate::{Limits, identity::CampaignId};
use fallout_data::loaded_scripts::Catalogue;
use serde::Serialize;
use std::{
    fmt,
    num::NonZeroU64,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
};

const ACTIVE: u8 = 0;
const CANCELLED: u8 = 1;
const ACCEPTED: u8 = 2;

/// Use a new request ID whenever the host supersedes a menu observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AvailabilityRequest {
    request_id: NonZeroU64,
    campaign: CampaignId,
    catalogue_sha256: String,
}
impl AvailabilityRequest {
    pub fn new(
        request_id: NonZeroU64,
        campaign: CampaignId,
        catalogue_sha256: &str,
    ) -> Result<Self, AvailabilityError> {
        if CampaignId::from_bytes(campaign.bytes()).is_err()
            || catalogue_sha256.len() != 64
            || !catalogue_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(AvailabilityError::InvalidIdentity);
        }
        Ok(Self {
            request_id,
            campaign,
            catalogue_sha256: catalogue_sha256.into(),
        })
    }
    pub fn request_id(&self) -> NonZeroU64 {
        self.request_id
    }
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
}
#[derive(Debug, thiserror::Error)]
pub enum AvailabilityError {
    #[error("availability request needs a valid campaign and canonical source digest")]
    InvalidIdentity,
    #[error("could not start native availability worker: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("native availability: {0}")]
    Save(#[from] super::Error),
    #[error("native availability worker panicked")]
    Panicked,
    #[error("native availability worker disconnected without a result")]
    WorkerStopped,
    #[error("availability report campaign/source differs from the request")]
    IdentityMismatch,
    #[error("availability candidate belongs to a superseded menu request")]
    Superseded,
    #[error("native availability request was cancelled")]
    Cancelled,
}
struct Control {
    state: AtomicU8,
    gate: Mutex<Option<SyncSender<()>>>,
}
impl Control {
    fn cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLED
    }
    fn cancel(&self) -> bool {
        let changed = self
            .state
            .compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        // The external token owns no sender. Closing this task-owned sender
        // releases a held worker even while its external token survives.
        self.gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        changed
    }
}
/// Single-use read gate. Drop cancels; release never waits for the worker.
pub struct AvailabilityAdmission {
    control: Arc<Control>,
    armed: bool,
}
impl AvailabilityAdmission {
    pub fn release(mut self) -> Result<(), AvailabilityError> {
        self.armed = false;
        if self.control.cancelled() {
            return Err(AvailabilityError::Cancelled);
        }
        let sender = self
            .control
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or(AvailabilityError::Cancelled)?;
        if sender.try_send(()).is_err() {
            self.control.cancel();
            return Err(AvailabilityError::Cancelled);
        }
        Ok(())
    }
}
impl Drop for AvailabilityAdmission {
    fn drop(&mut self) {
        if self.armed {
            self.control.cancel();
        }
    }
}
/// Report storage is inaccessible until the current identity is checked.
// Cancellation/drop of the task also revokes an already delivered candidate.
pub struct AvailabilityCandidate {
    request: AvailabilityRequest,
    report: SlotAvailabilityReport,
    control: Arc<Control>,
}
impl fmt::Debug for AvailabilityCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AvailabilityCandidate")
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}
impl AvailabilityCandidate {
    pub fn request(&self) -> &AvailabilityRequest {
        &self.request
    }
    pub fn take_for(
        self,
        current: &AvailabilityRequest,
    ) -> Result<SlotAvailabilityReport, AvailabilityError> {
        if &self.request != current {
            return Err(AvailabilityError::Superseded);
        }
        self.control
            .state
            .compare_exchange(ACTIVE, ACCEPTED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| AvailabilityError::Cancelled)?;
        Ok(self.report)
    }
}
#[derive(Debug)]
pub enum AvailabilityPoll {
    Pending,
    Ready(Box<AvailabilityCandidate>),
    Failed(Arc<AvailabilityError>),
    Cancelled,
    /// The one candidate was delivered, without asserting menu admission.
    Delivered,
}
type Job = Box<dyn FnOnce() + Send + 'static>;
/// Exactly one thread/job/result. Existing inspector limits and bounded typed
/// slot reasons apply. Neither reports nor failures retain a temporary World.
// Drop cancels and detaches without joining IO; finish is only for off-frame
// lifecycle management. In-progress IO can finish later and discard its result.
pub struct AvailabilityTask {
    request: AvailabilityRequest,
    control: Arc<Control>,
    result: Option<Receiver<Result<AvailabilityCandidate, AvailabilityError>>>,
    failure: Option<Arc<AvailabilityError>>,
    thread: Option<JoinHandle<()>>,
}
impl AvailabilityTask {
    pub fn start(
        repository: Repository,
        catalogue: Arc<Catalogue>,
        limits: Limits,
        request: AvailabilityRequest,
    ) -> Result<Self, AvailabilityError> {
        Self::spawn_with(
            request,
            move || repository.inspect_availability(catalogue, limits),
            false,
            |job| {
                thread::Builder::new()
                    .name("native-availability".into())
                    .spawn(job)
            },
        )
        .map(|(task, _)| task)
    }
    /// Hold immediately before the exact inspector runs. No slot IO, cohort
    /// calculation or temporary restore happens until release.
    pub fn start_gated(
        repository: Repository,
        catalogue: Arc<Catalogue>,
        limits: Limits,
        request: AvailabilityRequest,
    ) -> Result<(Self, AvailabilityAdmission), AvailabilityError> {
        let (task, admission) = Self::spawn_with(
            request,
            move || repository.inspect_availability(catalogue, limits),
            true,
            |job| {
                thread::Builder::new()
                    .name("native-availability".into())
                    .spawn(job)
            },
        )?;
        Ok((task, admission.expect("gated job owns one read admission")))
    }
    fn spawn_with(
        request: AvailabilityRequest,
        inspect: impl FnOnce() -> super::Result<SlotAvailabilityReport> + Send + 'static,
        gated: bool,
        spawn: impl FnOnce(Job) -> std::io::Result<JoinHandle<()>>,
    ) -> Result<(Self, Option<AvailabilityAdmission>), AvailabilityError> {
        let (gate_sender, gate_receiver) = if gated {
            let (sender, receiver) = mpsc::sync_channel(1);
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        let control = Arc::new(Control {
            state: AtomicU8::new(ACTIVE),
            gate: Mutex::new(gate_sender),
        });
        let worker_control = Arc::clone(&control);
        let worker_request = request.clone();
        let (sender, result) = mpsc::sync_channel(1);
        let job: Job = Box::new(move || {
            if let Some(gate) = gate_receiver
                && gate.recv().is_err()
            {
                return;
            }
            let observation = catch_unwind(AssertUnwindSafe(|| {
                if worker_control.cancelled() {
                    return Err(AvailabilityError::Cancelled);
                }
                let report = inspect()?;
                if report.campaign() != worker_request.campaign
                    || report.catalogue_fingerprint() != worker_request.catalogue_sha256
                {
                    return Err(AvailabilityError::IdentityMismatch);
                }
                Ok(AvailabilityCandidate {
                    request: worker_request,
                    report,
                    control: Arc::clone(&worker_control),
                })
            }))
            .unwrap_or(Err(AvailabilityError::Panicked));
            if !worker_control.cancelled() {
                // One result/one place: try_send cannot wait for host polling.
                // Abandonment or cancellation drops the report and source.
                let _ = sender.try_send(observation);
            }
        });
        let thread = spawn(job).map_err(AvailabilityError::Spawn)?;
        let admission = gated.then(|| AvailabilityAdmission {
            control: Arc::clone(&control),
            armed: true,
        });
        Ok((
            Self {
                request,
                control,
                result: Some(result),
                failure: None,
                thread: Some(thread),
            },
            admission,
        ))
    }
    pub fn request(&self) -> &AvailabilityRequest {
        &self.request
    }
    pub fn try_poll(&mut self) -> AvailabilityPoll {
        if self.control.cancelled() {
            return AvailabilityPoll::Cancelled;
        }
        if let Some(error) = &self.failure {
            return AvailabilityPoll::Failed(Arc::clone(error));
        }
        let Some(receiver) = &self.result else {
            return AvailabilityPoll::Delivered;
        };
        match receiver.try_recv() {
            Ok(Ok(candidate)) => {
                self.result = None;
                AvailabilityPoll::Ready(Box::new(candidate))
            }
            Ok(Err(error)) => self.fail(error),
            Err(TryRecvError::Empty) => AvailabilityPoll::Pending,
            Err(TryRecvError::Disconnected) => self.fail(AvailabilityError::WorkerStopped),
        }
    }
    fn fail(&mut self, error: AvailabilityError) -> AvailabilityPoll {
        let error = Arc::new(error);
        self.result = None;
        self.failure = Some(Arc::clone(&error));
        AvailabilityPoll::Failed(error)
    }
    pub fn cancel(&mut self) -> bool {
        let changed = self.control.cancel();
        self.result = None;
        self.failure = None;
        changed
    }
    /// Off-frame join. Thread completion is not successful report admission.
    pub fn finish(&mut self) -> Result<(), AvailabilityError> {
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| AvailabilityError::Panicked)?;
        }
        Ok(())
    }
}
impl Drop for AvailabilityTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
