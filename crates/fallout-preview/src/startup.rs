//! Show real startup activity while source preparation runs before the window.
use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub fn stage(message: impl std::fmt::Display) {
    eprintln!("[startup] {message}");
}

pub struct Progress {
    started: Instant,
    headless: bool,
    stop: Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Progress {
    pub fn start(headless: bool) -> Self {
        let started = Instant::now();
        let (stop, receiver) = mpsc::channel();
        stage(if headless {
            "Loading source data for an offscreen capture."
        } else {
            "Loading source data. The 3D viewer opens in a separate window after loading."
        });
        let worker = thread::spawn(move || {
            while receiver.recv_timeout(Duration::from_secs(5))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                stage(format!(
                    "Still preparing source data ({} seconds elapsed).",
                    started.elapsed().as_secs()
                ));
            }
        });
        Self {
            started,
            headless,
            stop,
            worker: Some(worker),
        }
    }

    pub fn finish(self) {
        stage(format!(
            "Source data ready in {:.1} seconds. {}",
            self.started.elapsed().as_secs_f64(),
            if self.headless {
                "Starting offscreen graphics..."
            } else {
                "Starting graphics and opening the viewer..."
            }
        ));
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
