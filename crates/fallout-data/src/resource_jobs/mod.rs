//! Bounded fixed archive-member work over the existing importer/cache readers.
//! Admission includes queued work and unconsumed results. No gameplay is emitted.
mod generation;
mod source;
pub use generation::{Generation, JobToken};
pub use source::{ArchiveInput, Member};

use crate::{
    Error,
    cache::{self, CacheResult},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("resource request is stale")]
    Stale,
    #[error("resource request cancelled")]
    Cancelled,
    #[error("resource queue/outstanding limit reached")]
    QueueFull,
    #[error("resource decoded-byte budget exceeded")]
    ByteBudget,
    #[error("resource worker closed")]
    Closed,
    #[error("invalid resource request: {0}")]
    Invalid(String),
    #[error(transparent)]
    Failed(#[from] Error),
}
pub type JobResult<T> = std::result::Result<T, JobError>;

#[derive(Clone, Copy)]
pub struct Limits {
    pub workers: usize,
    pub outstanding: usize,
    pub decoded_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            workers: 2,
            outstanding: 8,
            decoded_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Usage {
    pub outstanding: usize,
    pub decoded_bytes: usize,
}

struct Reservation {
    usage: Arc<Mutex<Usage>>,
    bytes: usize,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.usage.lock() {
            usage.outstanding -= 1;
            usage.decoded_bytes -= self.bytes;
        }
    }
}

/// Output is borrowed so callers cannot detach its allocation from its budget pin.
pub struct Artifact {
    bytes: Vec<u8>,
    cache: Option<CacheResult>,
    _reservation: Arc<Reservation>,
    _source: Arc<ArchiveInput>,
}
impl Artifact {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn take_cache_receipt(&mut self) -> Option<CacheResult> {
        self.cache.take()
    }
}

struct Work {
    member: Member,
    token: JobToken,
    cache: Option<(PathBuf, PathBuf)>,
    reservation: Arc<Reservation>,
    reply: SyncSender<JobResult<Artifact>>,
    #[cfg(test)]
    pause: Option<Arc<tests::Pause>>,
}

pub struct JobHandle {
    token: JobToken,
    reply: Receiver<JobResult<Artifact>>,
    reservation: Mutex<Option<Arc<Reservation>>>,
}
impl JobHandle {
    pub fn cancel(&self) {
        self.token.cancel();
        let _ = self.reply.try_recv();
        self.release();
    }
    pub fn try_take(&self) -> JobResult<Option<Artifact>> {
        let result = self.token.commit(|| match self.reply.try_recv() {
            Ok(result) => result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(JobError::Closed),
        });
        if result.is_err() {
            let _ = self.reply.try_recv();
        }
        if !matches!(&result, Ok(None)) {
            self.release();
        }
        result
    }
    /// The inspection caller waits; presentation can instead poll try_take.
    pub fn wait(&self) -> JobResult<Artifact> {
        loop {
            if let Err(error) = self.token.check() {
                let _ = self.reply.try_recv();
                self.release();
                return Err(error);
            }
            match self.reply.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => {
                    let result = self.token.commit(|| result);
                    self.release();
                    return result;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.release();
                    return Err(JobError::Closed);
                }
            }
        }
    }
    fn release(&self) {
        if let Ok(mut reservation) = self.reservation.lock() {
            reservation.take();
        }
    }
}
impl Drop for JobHandle {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

pub struct ResourceJobs {
    sender: Option<SyncSender<Work>>,
    workers: Vec<JoinHandle<()>>,
    limits: Limits,
    usage: Arc<Mutex<Usage>>,
    generation: Generation,
}
impl ResourceJobs {
    pub fn new(limits: Limits, generation: Generation) -> JobResult<Self> {
        if limits.workers == 0
            || limits.workers > 4
            || limits.outstanding == 0
            || limits.outstanding > 1024
            || limits.decoded_bytes == 0
        {
            return Err(JobError::Invalid("invalid worker/queue/byte limits".into()));
        }
        let (sender, receiver) = mpsc::sync_channel::<Work>(limits.outstanding);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut pool = Self {
            sender: Some(sender),
            workers: Vec::new(),
            limits,
            usage: Arc::new(Mutex::new(Usage::default())),
            generation,
        };
        for index in 0..limits.workers {
            let receiver = receiver.clone();
            let worker = thread::Builder::new()
                .name(format!("fallout-resource-{index}"))
                .spawn(move || {
                    loop {
                        let work = match receiver.lock() {
                            Ok(receiver) => receiver.recv(),
                            Err(_) => return,
                        };
                        let Ok(work) = work else { return };
                        let token = work.token.clone();
                        let reply = work.reply.clone();
                        #[cfg(test)]
                        let pause = work.pause.clone();
                        let result = execute(work);
                        // Serializing this one nonblocking completion with cancel
                        // means cancel can drain an already-sent payload, while a
                        // later send drops its payload instead of retaining a pin.
                        if let Err(error) = token.commit(|| {
                            let _ = reply.send(result);
                            Ok(())
                        }) {
                            let _ = reply.send(Err(error));
                        }
                        #[cfg(test)]
                        if let Some(pause) = pause {
                            pause.sent();
                        }
                    }
                })
                .map_err(|e| JobError::Invalid(format!("worker startup: {e}")))?;
            pool.workers.push(worker);
        }
        Ok(pool)
    }

    pub fn usage(&self) -> Usage {
        self.usage.lock().map(|v| *v).unwrap_or_default()
    }

    pub fn submit(
        &self,
        member: Member,
        token: JobToken,
        cache: Option<(PathBuf, PathBuf)>,
    ) -> JobResult<JobHandle> {
        self.submit_inner(
            member,
            token,
            cache,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn submit_paused(
        &self,
        member: Member,
        token: JobToken,
        cache: Option<(PathBuf, PathBuf)>,
        pause: Arc<tests::Pause>,
    ) -> JobResult<JobHandle> {
        self.submit_inner(member, token, cache, Some(pause))
    }

    fn submit_inner(
        &self,
        member: Member,
        token: JobToken,
        cache: Option<(PathBuf, PathBuf)>,
        #[cfg(test)] pause: Option<Arc<tests::Pause>>,
    ) -> JobResult<JobHandle> {
        token.check()?;
        // A token from a different owner controller must not escape pool shutdown.
        if !self.generation.owns(&token) {
            return Err(JobError::Invalid(
                "token belongs to another resource generation".into(),
            ));
        }
        let reservation = {
            let mut usage = self.usage.lock().map_err(|_| JobError::Closed)?;
            if usage.outstanding >= self.limits.outstanding {
                return Err(JobError::QueueFull);
            }
            if member.bytes
                > self
                    .limits
                    .decoded_bytes
                    .saturating_sub(usage.decoded_bytes)
            {
                return Err(JobError::ByteBudget);
            }
            usage.outstanding += 1;
            usage.decoded_bytes += member.bytes;
            Arc::new(Reservation {
                usage: self.usage.clone(),
                bytes: member.bytes,
            })
        };
        let (reply, receiver) = mpsc::sync_channel(1);
        let work = Work {
            member,
            token: token.clone(),
            cache,
            reservation: reservation.clone(),
            reply,
            #[cfg(test)]
            pause,
        };
        self.sender
            .as_ref()
            .ok_or(JobError::Closed)?
            .try_send(work)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => JobError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => JobError::Closed,
            })?;
        Ok(JobHandle {
            token,
            reply: receiver,
            reservation: Mutex::new(Some(reservation)),
        })
    }
}
impl Drop for ResourceJobs {
    fn drop(&mut self) {
        self.generation.close();
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn execute(work: Work) -> JobResult<Artifact> {
    work.token.check()?;
    #[cfg(test)]
    if let Some(pause) = &work.pause {
        pause.arrive(false);
    }
    work.token.check()?;
    let Member {
        input,
        id,
        bytes: maximum,
        identity,
    } = work.member;
    let cached = if let Some((root, source)) = &work.cache {
        cache::read_verified(root, source, &identity, maximum)?
    } else {
        None
    };
    work.token.check()?;
    let (receipt, bytes) = if let Some((receipt, bytes)) = cached {
        (Some(receipt), bytes)
    } else {
        // The pinned backend decompresses one admitted member without an internal
        // cancellation callback. The check afterward prevents publication/admission.
        let bytes = input.archive.read_bounded(id, maximum as u64)?;
        #[cfg(test)]
        if let Some(pause) = &work.pause {
            pause.arrive(true);
        }
        work.token.check()?;
        if bytes.len() != maximum {
            return Err(JobError::Invalid("decoded member length changed".into()));
        }
        let receipt = if let Some((root, source)) = &work.cache {
            Some(cache::publish_cancellable(
                root,
                source,
                identity,
                &bytes,
                &work.token,
            )?)
        } else {
            None
        };
        (receipt, bytes)
    };
    work.token.check()?;
    // A syntactically complete cache marker must still agree with the pinned
    // member's declared output size. A partial payload cannot become ready just
    // because its marker and its own checksum happen to agree.
    if bytes.len() != maximum {
        return Err(JobError::Invalid(
            "cached member decoded length differs from source".into(),
        ));
    }
    Ok(Artifact {
        bytes,
        cache: receipt,
        _reservation: work.reservation,
        _source: input,
    })
}

#[cfg(test)]
pub(crate) mod tests;
