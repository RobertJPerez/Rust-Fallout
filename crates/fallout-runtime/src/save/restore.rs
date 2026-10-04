//! One inbound job owns its source and result. A candidate is distinct from the
//! active host world; matching a current request is an explicit consuming step.
use super::{LoadReceipt, Recovery, Repository};
use crate::{Limits, World, identity::CampaignId};
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

/// The host supplies a distinct ID for each request and retains the current
/// identity. This observation is not a persisted continuation or execution
/// permission. Reusing an ID for the same campaign/source defeats supersession.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestIdentity {
    request_id: NonZeroU64,
    campaign: CampaignId,
    catalogue_sha256: String,
}
impl RequestIdentity {
    pub fn new(
        request_id: NonZeroU64,
        campaign: CampaignId,
        catalogue_sha256: &str,
    ) -> Result<Self, RestoreError> {
        if CampaignId::from_bytes(campaign.bytes()).is_err()
            || catalogue_sha256.len() != 64
            || !catalogue_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(RestoreError::InvalidIdentity);
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
pub enum RestoreError {
    #[error("restore request needs a valid campaign and canonical source digest")]
    InvalidIdentity,
    #[error("could not start native restore worker: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("native restore: {0}")]
    Save(#[from] super::Error),
    #[error("native restore worker panicked")]
    Panicked,
    #[error("native restore worker disconnected without a result")]
    WorkerStopped,
    #[error("restored campaign/source differs from the requested identity")]
    IdentityMismatch,
    #[error("restored candidate belongs to a superseded host request")]
    Superseded,
    #[error("native restore request was cancelled")]
    Cancelled,
}

struct Control {
    state: AtomicU8,
    admission: Mutex<Option<SyncSender<()>>>,
}
impl Control {
    fn cancel(&self) -> bool {
        let changed = self
            .state
            .compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        // The token has no sender copy. Closing this task-owned sender releases
        // a held worker even when an external admission token survives.
        self.admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        changed
    }
    fn cancelled(&self) -> bool {
        self.state.load(Ordering::Acquire) == CANCELLED
    }
}

/// A single-use read gate for an explicitly held job. It cannot authorize a
/// world replacement or callback. Dropping it cancels the request. Ordinary
/// start does not use a gate and begins loading immediately.
pub struct RestoreAdmission {
    control: Arc<Control>,
    armed: bool,
}
impl RestoreAdmission {
    pub fn release(mut self) -> Result<(), RestoreError> {
        self.armed = false;
        if self.control.cancelled() {
            return Err(RestoreError::Cancelled);
        }
        let sender = self
            .control
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or(RestoreError::Cancelled)?;
        // One sender/token exists and the channel has one place. try_send keeps
        // this host operation explicitly nonblocking.
        if sender.try_send(()).is_err() {
            self.control.cancel();
            return Err(RestoreError::Cancelled);
        }
        Ok(())
    }
}
impl Drop for RestoreAdmission {
    fn drop(&mut self) {
        if self.armed {
            self.control.cancel();
        }
    }
}

/// No mutable world is exposed before take_for compares the host's current
/// identity. Keep the task alive until this step: cancel/drop revokes candidates
/// that have already been delivered but have not yet been accepted.
pub struct RestoredCandidate {
    request: RequestIdentity,
    world: World<'static>,
    receipt: LoadReceipt,
    control: Arc<Control>,
}
impl fmt::Debug for RestoredCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RestoredCandidate")
            .field("request", &self.request)
            .field("receipt", &self.receipt)
            .finish_non_exhaustive()
    }
}
impl RestoredCandidate {
    pub fn request(&self) -> &RequestIdentity {
        &self.request
    }
    pub fn receipt(&self) -> &LoadReceipt {
        &self.receipt
    }
    pub fn take_for(
        self,
        current_request: &RequestIdentity,
    ) -> Result<(World<'static>, LoadReceipt), RestoreError> {
        if &self.request != current_request {
            return Err(RestoreError::Superseded);
        }
        self.control
            .state
            .compare_exchange(ACTIVE, ACCEPTED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| RestoreError::Cancelled)?;
        Ok((self.world, self.receipt))
    }
}

#[derive(Debug)]
pub enum RestorePoll {
    Pending,
    Ready(Box<RestoredCandidate>),
    Failed(Arc<RestoreError>),
    Cancelled,
    /// The single candidate has been handed to the caller. This does not say it
    /// was accepted, nor that any host replaced its world.
    Delivered,
}

/// One thread/job/result per task, bounded by the existing restore Limits. Drop
/// cancels and detaches without joining disk IO. IO already in progress may
/// finish later; its candidate/source storage is then discarded, never applied.
pub struct RestoreTask {
    request: RequestIdentity,
    control: Arc<Control>,
    result: Option<Receiver<Result<RestoredCandidate, RestoreError>>>,
    failure: Option<Arc<RestoreError>>,
    thread: Option<JoinHandle<()>>,
}
impl RestoreTask {
    pub fn start(
        repository: Repository,
        catalogue: Arc<Catalogue>,
        limits: Limits,
        recovery: Recovery,
        request: RequestIdentity,
    ) -> Result<Self, RestoreError> {
        Self::spawn_with(request, move || {
            repository.load(catalogue, limits, recovery)
        })
    }
    /// Spawn the same worker held immediately before its repository read. The
    /// host can poll Pending while deferring that one read. No restoration or
    /// canonical initialization happens before release.
    pub fn start_gated(
        repository: Repository,
        catalogue: Arc<Catalogue>,
        limits: Limits,
        recovery: Recovery,
        request: RequestIdentity,
    ) -> Result<(Self, RestoreAdmission), RestoreError> {
        let (task, admission) = Self::spawn_with_gate(
            request,
            move || repository.load(catalogue, limits, recovery),
            true,
        )?;
        Ok((task, admission.expect("gated spawn owns one admission")))
    }
    fn spawn_with(
        request: RequestIdentity,
        load: impl FnOnce() -> super::Result<(World<'static>, LoadReceipt)> + Send + 'static,
    ) -> Result<Self, RestoreError> {
        Self::spawn_with_gate(request, load, false).map(|(task, _)| task)
    }
    fn spawn_with_gate(
        request: RequestIdentity,
        load: impl FnOnce() -> super::Result<(World<'static>, LoadReceipt)> + Send + 'static,
        gated: bool,
    ) -> Result<(Self, Option<RestoreAdmission>), RestoreError> {
        let (admission_sender, admission_receiver) = if gated {
            let (sender, receiver) = mpsc::sync_channel(1);
            (Some(sender), Some(receiver))
        } else {
            (None, None)
        };
        let control = Arc::new(Control {
            state: AtomicU8::new(ACTIVE),
            admission: Mutex::new(admission_sender),
        });
        let worker_control = Arc::clone(&control);
        let worker_request = request.clone();
        let (sender, result) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("native-restore".into())
            .spawn(move || {
                if let Some(admission) = admission_receiver
                    && admission.recv().is_err()
                {
                    return;
                }
                let restored = catch_unwind(AssertUnwindSafe(|| {
                    if worker_control.cancelled() {
                        return Err(RestoreError::Cancelled);
                    }
                    let (world, receipt) = load()?;
                    if world.campaign() != worker_request.campaign
                        || receipt.metadata.campaign != worker_request.campaign
                        || world.catalogue_fingerprint() != worker_request.catalogue_sha256
                        || receipt.metadata.catalogue_sha256 != worker_request.catalogue_sha256
                    {
                        return Err(RestoreError::IdentityMismatch);
                    }
                    Ok(RestoredCandidate {
                        request: worker_request,
                        world,
                        receipt,
                        control: Arc::clone(&worker_control),
                    })
                }))
                .unwrap_or(Err(RestoreError::Panicked));
                if !worker_control.cancelled() {
                    // Only one result can be sent, so this one-place channel
                    // never waits for a caller to poll. Disconnection drops it.
                    let _ = sender.send(restored);
                }
            })
            .map_err(RestoreError::Spawn)?;
        let admission = gated.then(|| RestoreAdmission {
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
    pub fn request(&self) -> &RequestIdentity {
        &self.request
    }
    pub fn try_poll(&mut self) -> RestorePoll {
        if self.control.cancelled() {
            return RestorePoll::Cancelled;
        }
        if let Some(failure) = &self.failure {
            return RestorePoll::Failed(Arc::clone(failure));
        }
        let Some(receiver) = &self.result else {
            return RestorePoll::Delivered;
        };
        match receiver.try_recv() {
            Ok(Ok(candidate)) => {
                self.result = None;
                RestorePoll::Ready(Box::new(candidate))
            }
            Ok(Err(error)) => self.fail(error),
            Err(TryRecvError::Empty) => RestorePoll::Pending,
            Err(TryRecvError::Disconnected) => self.fail(RestoreError::WorkerStopped),
        }
    }
    fn fail(&mut self, error: RestoreError) -> RestorePoll {
        let error = Arc::new(error);
        self.result = None;
        self.failure = Some(Arc::clone(&error));
        RestorePoll::Failed(error)
    }
    /// Revokes any not-yet-accepted candidate and closes result storage. The
    /// bool reports whether this call won cancellation against acceptance.
    pub fn cancel(&mut self) -> bool {
        let changed = self.control.cancel();
        self.result = None;
        self.failure = None;
        changed
    }
    /// Off-frame join only. Completion of the thread is not load success;
    /// try_poll still reports the real result and explicit candidate admission.
    pub fn finish(&mut self) -> Result<(), RestoreError> {
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| RestoreError::Panicked)?;
        }
        Ok(())
    }
}
impl Drop for RestoreTask {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
