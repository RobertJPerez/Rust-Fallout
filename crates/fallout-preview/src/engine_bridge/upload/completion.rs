//! A queue callback cannot revive a cancelled request or an old render device.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

pub(super) struct Completion {
    backend: Arc<AtomicU64>,
    epoch: u64,
    cancelled: AtomicBool,
    submitted: AtomicBool,
    completed: AtomicBool,
}

impl Completion {
    pub fn new(backend: Arc<AtomicU64>, epoch: u64) -> Self {
        Self {
            backend,
            epoch,
            cancelled: AtomicBool::new(false),
            submitted: AtomicBool::new(false),
            completed: AtomicBool::new(false),
        }
    }

    pub fn current(&self) -> bool {
        self.epoch != 0
            && self.backend.load(Ordering::Acquire) == self.epoch
            && !self.cancelled.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn ready(&self) -> bool {
        self.current() && self.completed.load(Ordering::Acquire)
    }

    /// The render observer calls this only after checking real GPU resources.
    /// Returning a callback does not acknowledge submission or completion.
    pub fn callback(self: &Arc<Self>) -> Option<impl FnOnce() + Send + 'static> {
        if !self.current()
            || self
                .submitted
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return None;
        }
        let weak = Arc::downgrade(self);
        Some(move || {
            if let Some(completion) = weak.upgrade()
                && completion.current()
            {
                completion.completed.store(true, Ordering::Release);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_waits_for_callback_and_arms_once() {
        let backend = Arc::new(AtomicU64::new(1));
        let completion = Arc::new(Completion::new(backend, 1));
        assert!(!completion.ready());
        let callback = completion.callback().unwrap();
        assert!(!completion.ready());
        assert!(completion.callback().is_none());
        callback();
        assert!(completion.ready());
    }

    #[test]
    fn late_callback_cannot_revive_cancelled_request() {
        let backend = Arc::new(AtomicU64::new(1));
        let completion = Arc::new(Completion::new(backend, 1));
        let callback = completion.callback().unwrap();
        completion.cancel();
        callback();
        assert!(!completion.current());
        assert!(!completion.ready());
    }

    #[test]
    fn device_recovery_revokes_pending_and_completed_requests() {
        let backend = Arc::new(AtomicU64::new(1));
        let pending = Arc::new(Completion::new(Arc::clone(&backend), 1));
        let callback = pending.callback().unwrap();
        let completed = Arc::new(Completion::new(Arc::clone(&backend), 1));
        completed.callback().unwrap()();
        backend.store(2, Ordering::Release);
        callback();
        assert!(!pending.ready());
        assert!(!completed.ready());
        let next = Arc::new(Completion::new(backend, 2));
        assert!(!next.ready());
        next.callback().unwrap()();
        assert!(next.ready());
    }

    #[test]
    fn dropped_request_is_not_retained_by_queue_callback() {
        let backend = Arc::new(AtomicU64::new(1));
        let completion = Arc::new(Completion::new(backend, 1));
        let weak = Arc::downgrade(&completion);
        let callback = completion.callback().unwrap();
        drop(completion);
        assert!(weak.upgrade().is_none());
        callback();
    }
}
