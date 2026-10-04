//! A single source-preparation worker for the inspection window. This is host
//! transport, not an archive importer or a replacement for world resource jobs.
use bevy::prelude::Resource;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Context {
    cancelled: Arc<AtomicBool>,
    latest: Arc<Mutex<String>>,
}

impl Context {
    pub fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Source preparation cancelled".into())
        } else {
            Ok(())
        }
    }

    /// Replace one bounded status rather than queuing progress faster than the
    /// window can consume it. Checking here makes source stage boundaries cancel.
    pub fn stage(&self, message: impl AsRef<str>) -> Result<(), String> {
        self.check()?;
        *self
            .latest
            .lock()
            .map_err(|_| "Loading status unavailable")? =
            message.as_ref().chars().take(200).collect();
        Ok(())
    }
}

pub enum Poll<T> {
    Pending,
    Ready(T),
    Failed(String),
    Cancelled,
    /// The successful result was already consumed, never another success.
    Finished,
}

pub enum Retirement<T> {
    Pending,
    Done(Option<T>),
}

enum Terminal {
    Taken,
    Failed(String),
    Cancelled,
}

#[derive(Resource)]
pub struct Job<T: Send + 'static> {
    epoch: u64,
    context: Context,
    receiver: Mutex<Receiver<Result<T, String>>>,
    worker: Option<JoinHandle<()>>,
    terminal: Option<Terminal>,
    started: Instant,
}

impl<T: Send + 'static> Job<T> {
    pub fn start(
        epoch: u64,
        prepare: impl FnOnce(Context) -> Result<T, String> + Send + 'static,
    ) -> std::io::Result<Self> {
        let context = Context {
            cancelled: Arc::new(AtomicBool::new(false)),
            latest: Arc::new(Mutex::new("Preparing source data".into())),
        };
        // At most one completed scene awaits the host; progress occupies one
        // separate string. A dropped receiver releases an unadmitted scene.
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_context = context.clone();
        let worker = thread::Builder::new()
            .name("preview-source".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| prepare(worker_context.clone())))
                    .unwrap_or_else(|_| Err("Source preparation worker panicked".into()));
                // Cancellation may happen inside an opaque bounded decoder.
                // Recheck after it returns, before sending its large result.
                if worker_context.check().is_ok() {
                    let result = result.map_err(|error| error.chars().take(4096).collect());
                    let _ = sender.send(result);
                }
            })?;
        Ok(Self {
            epoch,
            context,
            receiver: Mutex::new(receiver),
            worker: Some(worker),
            terminal: None,
            started: Instant::now(),
        })
    }

    pub fn status(&self) -> (String, Duration) {
        let message = self
            .context
            .latest
            .lock()
            .map(|latest| latest.clone())
            .unwrap_or_else(|_| "Loading status unavailable".into());
        (message, self.started.elapsed())
    }

    pub fn cancel(&mut self) {
        self.context.cancelled.store(true, Ordering::Release);
        self.terminal = Some(Terminal::Cancelled);
    }

    fn reap_finished(&mut self) -> bool {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return false;
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        true
    }

    /// Retain the cancelled owner until its bounded decoder returns. A result
    /// queued just before cancellation belongs to disposal, never publication.
    pub fn retire(&mut self) -> Retirement<T> {
        self.cancel();
        if !self.reap_finished() {
            return Retirement::Pending;
        }
        let value = self
            .receiver
            .get_mut()
            .ok()
            .and_then(|receiver| receiver.try_recv().ok().and_then(Result::ok));
        Retirement::Done(value)
    }

    /// Main-thread polling never waits for extraction or joins an active worker.
    /// Epoch mismatch discards work even if its result arrived before cancellation.
    pub fn poll(&mut self, expected_epoch: u64) -> Poll<T> {
        if self.epoch != expected_epoch {
            self.cancel();
        }
        if let Some(terminal) = &self.terminal {
            return match terminal {
                Terminal::Taken => Poll::Finished,
                Terminal::Failed(error) => Poll::Failed(error.clone()),
                Terminal::Cancelled => Poll::Cancelled,
            };
        }
        // Receipt delivery precedes thread return by a few instructions. Keep
        // ownership until return so a failed request cannot overlap its retry.
        if !self.reap_finished() {
            return Poll::Pending;
        }
        let result = self.receiver.lock().map(|receiver| receiver.try_recv());
        match result {
            Ok(Ok(Ok(value))) => {
                self.terminal = Some(Terminal::Taken);
                Poll::Ready(value)
            }
            Ok(Err(TryRecvError::Empty)) => Poll::Pending,
            Ok(Ok(Err(error))) => {
                self.terminal = Some(Terminal::Failed(error.clone()));
                Poll::Failed(error)
            }
            _ => {
                let error = "Source preparation stopped without a result".to_owned();
                self.terminal = Some(Terminal::Failed(error.clone()));
                Poll::Failed(error)
            }
        }
    }
}

impl<T: Send + 'static> Drop for Job<T> {
    fn drop(&mut self) {
        self.context.cancelled.store(true, Ordering::Release);
        // An active read-only decoder reaches its existing bounded return,
        // observes cancellation, then drops its result. Window shutdown must
        // not join that operation on the event thread.
        if let Some(worker) = self.worker.take()
            && worker.is_finished()
        {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_poll_stays_pending_while_source_worker_is_gated() {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut job = Job::start(7, move |context| {
            context.stage("Selected source model")?;
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(42)
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(job.poll(7), Poll::Pending));
        assert_eq!(job.status().0, "Selected source model");
        release.send(()).unwrap();
        job.worker.take().unwrap().join().unwrap();
        assert!(matches!(job.poll(7), Poll::Ready(42)));
        assert!(matches!(job.poll(7), Poll::Finished));
    }

    #[test]
    fn stale_completed_result_and_cancelled_active_result_never_publish() {
        let mut old = Job::start(7, |_| Ok(42)).unwrap();
        old.worker.take().unwrap().join().unwrap();
        assert!(matches!(old.poll(8), Poll::Cancelled));
        assert!(matches!(old.poll(7), Poll::Cancelled));

        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let mut active = Job::start(9, move |context| {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            assert!(context.stage("Must not start another stage").is_err());
            Ok(99)
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        active.cancel();
        assert!(matches!(active.poll(9), Poll::Cancelled));
        release.send(()).unwrap();
        active.worker.take().unwrap().join().unwrap();
        assert!(matches!(active.poll(9), Poll::Cancelled));
    }

    #[test]
    fn cancelled_queued_result_is_returned_only_for_retirement() {
        let mut job = Job::start(7, |_| Ok(42)).unwrap();
        job.worker.take().unwrap().join().unwrap();
        job.cancel();
        assert!(matches!(job.poll(8), Poll::Cancelled));
        assert!(matches!(job.retire(), Retirement::Done(Some(42))));
        assert!(matches!(job.retire(), Retirement::Done(None)));
        assert!(matches!(job.poll(7), Poll::Cancelled));
    }

    #[test]
    fn primary_close_retains_queued_scene_for_bounded_retirement_or_app_teardown() {
        use bevy::{
            prelude::*,
            window::{PrimaryWindow, WindowCloseRequested},
        };
        let mut job = Job::start(7, |_| Ok(crate::tests::ready_fixture(7))).unwrap();
        // The worker has returned, with its full unadmitted draw scene queued.
        job.worker.take().unwrap().join().unwrap();
        let mut app = crate::tests::loading_app(crate::Phase::Preparing(job));
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        assert!(
            matches!(
                app.world().resource::<crate::Loading>().phase,
                crate::Phase::Draining(_)
            ),
            "close released the queued scene directly inside its update"
        );
        assert_eq!(app.world().resource::<crate::Loading>().epoch, 8);
        let mut updates = 0;
        while !matches!(
            app.world().resource::<crate::Loading>().phase,
            crate::Phase::Cancelled
        ) {
            assert!(updates < 32, "controlled retirement did not complete");
            app.update();
            updates += 1;
            assert!(!matches!(
                app.world().resource::<crate::Loading>().phase,
                crate::Phase::Ready(_)
            ));
            assert!(app.world().resource::<Assets<Mesh>>().is_empty());
            assert_eq!(app.world().resource::<crate::Capture>().frame, 63);
        }
        assert!(
            updates > 1,
            "queued source scene bypassed retirement batches"
        );
        assert_eq!(
            *app.world().resource::<crate::input::Context>(),
            crate::input::Context::Suspended
        );
    }

    #[test]
    fn dropping_active_request_returns_before_gated_decoder_finishes() {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (finished, completion) = mpsc::channel();
        let job = Job::start(7, move |context| {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            assert!(context.check().is_err());
            finished.send(()).unwrap();
            Ok(42)
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let (dropped, done) = mpsc::channel();
        let dropper = thread::spawn(move || {
            drop(job);
            dropped.send(()).unwrap();
        });
        // Release the source gate even on failure, so a future erroneous join
        // cannot deadlock the test process or leave a worker behind.
        let returned = done.recv_timeout(Duration::from_secs(2));
        release.send(()).unwrap();
        dropper.join().unwrap();
        completion.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            returned.is_ok(),
            "Dropping the window request joined an active decoder"
        );
    }

    #[test]
    fn worker_failure_is_sticky_and_status_is_bounded_on_characters() {
        let mut job = Job::<()>::start(1, |context| {
            context.stage("界".repeat(300))?;
            Err("error".repeat(1000))
        })
        .unwrap();
        job.worker.take().unwrap().join().unwrap();
        assert_eq!(job.status().0, "界".repeat(200));
        for _ in 0..2 {
            let Poll::Failed(error) = job.poll(1) else {
                panic!("failed source load must remain failed");
            };
            assert_eq!(error.chars().count(), 4096);
        }
        let mut panic = Job::<()>::start(2, |_| panic!("source test panic")).unwrap();
        panic.worker.take().unwrap().join().unwrap();
        assert!(matches!(panic.poll(2), Poll::Failed(_)));
    }
}
