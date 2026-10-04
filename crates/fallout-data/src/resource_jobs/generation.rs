use super::{JobError, JobResult};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Epoch {
    source: String,
    generation: u64,
    closed: bool,
}

/// A request owner's immutable source identity and monotonic residency epoch.
/// This is preparation state, never canonical world or persistent object state.
#[derive(Clone)]
pub struct Generation {
    epoch: Arc<Mutex<Epoch>>,
}

#[derive(Clone)]
pub struct JobToken {
    epoch: Arc<Mutex<Epoch>>,
    pub(crate) source: String,
    pub(crate) generation: u64,
    cancelled: Arc<AtomicBool>,
}

impl Generation {
    pub fn new(source: String) -> JobResult<Self> {
        validate_digest(&source)?;
        Ok(Self {
            epoch: Arc::new(Mutex::new(Epoch {
                source,
                generation: 1,
                closed: false,
            })),
        })
    }

    pub fn advance(&self, source: String) -> JobResult<()> {
        validate_digest(&source)?;
        let mut epoch = self.epoch.lock().map_err(|_| JobError::Closed)?;
        if epoch.closed {
            return Err(JobError::Closed);
        }
        let next = epoch.generation.checked_add(1).ok_or(JobError::Closed)?;
        epoch.source = source;
        epoch.generation = next;
        Ok(())
    }

    pub fn token(&self) -> JobResult<JobToken> {
        let epoch = self.epoch.lock().map_err(|_| JobError::Closed)?;
        if epoch.closed {
            return Err(JobError::Closed);
        }
        Ok(JobToken {
            epoch: self.epoch.clone(),
            source: epoch.source.clone(),
            generation: epoch.generation,
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(crate) fn close(&self) {
        if let Ok(mut epoch) = self.epoch.lock() {
            epoch.closed = true;
        }
    }

    pub(crate) fn owns(&self, token: &JobToken) -> bool {
        Arc::ptr_eq(&self.epoch, &token.epoch)
    }
}

impl JobToken {
    pub fn source_identity(&self) -> &str {
        &self.source
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Serializes cancellation with the final marker rename and result admission.
    pub fn cancel(&self) {
        let _guard = self.epoch.lock();
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn check(&self) -> JobResult<()> {
        self.commit(|| Ok(()))
    }

    pub(crate) fn commit<T>(&self, operation: impl FnOnce() -> JobResult<T>) -> JobResult<T> {
        let epoch = self.epoch.lock().map_err(|_| JobError::Closed)?;
        if epoch.closed {
            return Err(JobError::Closed);
        }
        if epoch.source != self.source || epoch.generation != self.generation {
            return Err(JobError::Stale);
        }
        if self.cancelled.load(Ordering::Acquire) {
            return Err(JobError::Cancelled);
        }
        operation()
    }

    pub(crate) fn checkpoint(&self) -> crate::Result<()> {
        self.check()
            .map_err(|e| crate::Error::Resolution(e.to_string()))
    }
}

fn validate_digest(value: &str) -> JobResult<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(JobError::Invalid(
            "request owner needs a SHA-256 source identity".into(),
        ));
    }
    Ok(())
}
