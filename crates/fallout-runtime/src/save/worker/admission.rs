//! One atomic count/byte decision, shared by submission and permit release.
//! The byte charge is the capture's declared serialized snapshot upper bound.
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Usage {
    requests: usize,
    bytes: usize,
}

pub(super) struct Admission {
    maximum_requests: usize,
    maximum_bytes: usize,
    usage: Mutex<Usage>,
}
impl Admission {
    pub(super) fn new(maximum_requests: usize, maximum_bytes: usize) -> Self {
        Self {
            maximum_requests,
            maximum_bytes,
            usage: Mutex::new(Usage::default()),
        }
    }
    pub(super) fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        let mut used = self.usage.lock().expect("save admission mutex poisoned");
        if used.requests >= self.maximum_requests || bytes > self.maximum_bytes - used.bytes {
            return None;
        }
        used.requests += 1;
        // The remaining-budget comparison proves this addition cannot overflow,
        // including an explicitly configured usize::MAX budget.
        used.bytes += bytes;
        Some(Permit {
            admission: Arc::clone(self),
            bytes,
        })
    }
    #[cfg(test)]
    pub(super) fn usage(&self) -> (usize, usize) {
        let used = self.usage.lock().unwrap();
        (used.requests, used.bytes)
    }
}

pub(super) struct Permit {
    admission: Arc<Admission>,
    bytes: usize,
}
impl Drop for Permit {
    fn drop(&mut self) {
        // No writer/caller callback runs under this lock. A writer panic cannot
        // poison admission or interrupt a partially applied count/byte update.
        let mut used = self
            .admission
            .usage
            .lock()
            .expect("save admission mutex poisoned");
        used.requests -= 1;
        used.bytes -= self.bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Barrier, thread};

    #[test]
    fn concurrent_admission_holds_both_bounds_until_permits_drop() {
        for (requests, bytes, expected) in [(4, 10, 3), (2, 100, 2)] {
            let admission = Arc::new(Admission::new(requests, bytes));
            let attempted = Arc::new(Barrier::new(9));
            let release = Arc::new(Barrier::new(9));
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    let admission = Arc::clone(&admission);
                    let attempted = Arc::clone(&attempted);
                    let release = Arc::clone(&release);
                    thread::spawn(move || {
                        let permit = admission.reserve(3);
                        attempted.wait();
                        release.wait();
                        permit.is_some()
                    })
                })
                .collect();
            attempted.wait();
            assert_eq!(admission.usage(), (expected, expected * 3));
            release.wait();
            assert_eq!(
                threads
                    .into_iter()
                    .map(|thread| thread.join().unwrap())
                    .filter(|accepted| *accepted)
                    .count(),
                expected
            );
            assert_eq!(admission.usage(), (0, 0));
        }
    }

    #[test]
    fn full_width_byte_bound_and_zero_charge_do_not_overflow_or_bypass_count() {
        let admission = Arc::new(Admission::new(2, usize::MAX));
        let full = admission.reserve(usize::MAX).unwrap();
        assert!(admission.reserve(1).is_none());
        let zero = admission.reserve(0).unwrap();
        assert_eq!(admission.usage(), (2, usize::MAX));
        assert!(admission.reserve(0).is_none());
        drop(full);
        let retry = admission.reserve(usize::MAX).unwrap();
        assert_eq!(admission.usage(), (2, usize::MAX));
        drop((zero, retry));
        assert_eq!(admission.usage(), (0, 0));
    }
}
