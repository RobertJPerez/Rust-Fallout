//! Scene-host audio request admission and bounded PCM handoff.
//!
//! The host owns no device buffers here. A `PcmSink` is called on one bounded
//! worker; completion means all source frames were accepted by that sink, not
//! that a physical device rendered them or that an original playback clock ran.

#[cfg(test)]
mod tests;

use bevy::prelude::*;
use fallout_data::{
    audio::{AudioError, PcmChunk, PcmStream, PreparedSound, WaveFormat},
    resource_jobs::{Generation, JobError, JobToken},
};
use std::{
    collections::BTreeMap,
    error::Error,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Condvar, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DEFAULT_QUEUE_CAPACITY: usize = 8;
const MAX_QUEUE_CAPACITY: usize = 64;
const MUSIC_SEEK_QUEUE_CAPACITY: usize = 8;

/// A sink receives one source-format stream from the single audio worker.
/// `write` must return after one bounded chunk; device implementations must
/// apply their own finite buffering and backpressure policy.
pub(super) trait PcmSink: Send + 'static {
    fn begin(&mut self, format: WaveFormat) -> Result<(), String>;
    fn write(&mut self, chunk: PcmChunk) -> Result<(), String>;
    fn finish(&mut self) -> Result<(), String>;
}

/// The current preview build has no output-device backend. Requests are
/// admitted through the same queue contract, then complete with this explicit
/// error until a supported backend is installed.
struct UnavailableSink;

impl PcmSink for UnavailableSink {
    fn begin(&mut self, _format: WaveFormat) -> Result<(), String> {
        Err("preview audio output backend is unavailable".into())
    }

    fn write(&mut self, _chunk: PcmChunk) -> Result<(), String> {
        Err("preview audio output backend is unavailable".into())
    }

    fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum AudioAdmissionError {
    InvalidQueueCapacity,
    QueueFull,
    Closed,
    WorkerStart(String),
    InvalidSource(String),
    StaleScene {
        requested_generation: u64,
        current_generation: u64,
    },
    RequestIdsExhausted,
    HostPoisoned,
}

impl fmt::Display for AudioAdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidQueueCapacity => write!(
                f,
                "audio queue capacity must be between 1 and {MAX_QUEUE_CAPACITY}"
            ),
            Self::QueueFull => write!(f, "audio request queue is full"),
            Self::Closed => write!(f, "audio request worker is closed"),
            Self::WorkerStart(error) => write!(f, "audio worker could not start: {error}"),
            Self::InvalidSource(error) => write!(f, "audio source generation is invalid: {error}"),
            Self::StaleScene {
                requested_generation,
                current_generation,
            } => write!(
                f,
                "audio request scene generation {requested_generation} is stale; current generation is {current_generation}"
            ),
            Self::RequestIdsExhausted => write!(f, "audio request identifiers are exhausted"),
            Self::HostPoisoned => write!(f, "audio host state is unavailable"),
        }
    }
}

impl Error for AudioAdmissionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
// `StaleScene` is surfaced through the handle API used by the next cue consumer.
#[allow(dead_code)]
pub(super) enum PlaybackFailure {
    Cancelled,
    StaleSource,
    StaleScene {
        request_generation: u64,
        current_generation: u64,
    },
    Decode(String),
    Sink(String),
    WorkerPanicked,
}

impl fmt::Display for PlaybackFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => write!(f, "audio request was cancelled"),
            Self::StaleSource => write!(f, "audio request source generation is stale"),
            Self::StaleScene {
                request_generation,
                current_generation,
            } => write!(
                f,
                "audio completion belongs to scene generation {request_generation}; current generation is {current_generation}"
            ),
            Self::Decode(error) => write!(f, "audio source decode failed: {error}"),
            Self::Sink(error) => write!(f, "audio output failed: {error}"),
            Self::WorkerPanicked => write!(f, "audio worker panicked while processing a request"),
        }
    }
}

impl Error for PlaybackFailure {}

/// Completion reports source frames accepted by the sink. Device drain time
/// and audible playback time require a backend clock and are handled separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlaybackReceipt {
    pub request_id: u64,
    pub scene_generation: u64,
    pub frames_submitted: u64,
    pub format: WaveFormat,
}

type PlaybackResult = Result<PlaybackReceipt, PlaybackFailure>;

struct PlaybackControl {
    request_id: u64,
    scene_generation: u64,
    token: Mutex<JobToken>,
    cancelled: AtomicBool,
    completion: Mutex<Option<PlaybackResult>>,
    completed: Condvar,
}

impl PlaybackControl {
    fn new(request_id: u64, scene_generation: u64, token: JobToken) -> Self {
        Self {
            request_id,
            scene_generation,
            token: Mutex::new(token),
            cancelled: AtomicBool::new(false),
            completion: Mutex::new(None),
            completed: Condvar::new(),
        }
    }

    fn current_token(&self) -> JobToken {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn check(&self) -> Result<JobToken, PlaybackFailure> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PlaybackFailure::Cancelled);
        }
        let token = self.current_token();
        token.check().map_err(map_resource_error)?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PlaybackFailure::Cancelled);
        }
        Ok(token)
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.current_token().cancel();
    }

    fn replace_token(&self, token: JobToken) -> Result<(), PlaybackFailure> {
        let mut current = self
            .token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.cancelled.load(Ordering::Acquire) {
            token.cancel();
            return Err(PlaybackFailure::Cancelled);
        }
        *current = token;
        Ok(())
    }

    fn complete(&self, result: PlaybackResult) {
        let mut completion = self
            .completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if completion.is_none() {
            *completion = Some(result);
            self.completed.notify_all();
        }
    }
}

pub(super) struct PlaybackHandle {
    control: Arc<PlaybackControl>,
}

// These lifecycle methods are consumed by the scene cue producer, which is not
// part of the current preview host yet.
#[allow(dead_code)]
impl PlaybackHandle {
    pub(super) fn request_id(&self) -> u64 {
        self.control.request_id
    }

    /// Cancel only this request. The worker observes the token at source chunk
    /// boundaries and returns a sticky `Cancelled` completion.
    pub(super) fn cancel(&self) {
        self.control.cancel();
    }

    /// Poll without waiting. A result from a replaced scene is refused even if
    /// it completed immediately before the host advanced its generation.
    pub(super) fn try_completion(&self, current_scene_generation: u64) -> Option<PlaybackResult> {
        if current_scene_generation != self.control.scene_generation {
            return Some(Err(PlaybackFailure::StaleScene {
                request_generation: self.control.scene_generation,
                current_generation: current_scene_generation,
            }));
        }
        self.control
            .completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Wait only from an audio/background thread. Scene systems should use
    /// `try_completion` and never stall a frame waiting for output.
    pub(super) fn wait(
        &self,
        current_scene_generation: u64,
        timeout: Duration,
    ) -> Option<PlaybackResult> {
        if current_scene_generation != self.control.scene_generation {
            return self.try_completion(current_scene_generation);
        }
        let started = Instant::now();
        let mut completion = self
            .control
            .completion
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(result) = completion.as_ref() {
                return Some(result.clone());
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return None;
            }
            let (next, timed) = self
                .control
                .completed
                .wait_timeout(completion, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            completion = next;
            if timed.timed_out() && completion.is_none() {
                return None;
            }
        }
    }
}

impl Drop for PlaybackHandle {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MusicSeekError {
    OutOfRange { frame: u64, frame_count: u64 },
    QueueFull,
    Closed,
    Cancelled,
}

impl fmt::Display for MusicSeekError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { frame, frame_count } => write!(
                f,
                "music seek frame {frame} exceeds the source frame count {frame_count}"
            ),
            Self::QueueFull => write!(f, "music seek queue is full"),
            Self::Closed => write!(f, "music stream is already closed"),
            Self::Cancelled => write!(f, "music stream was cancelled"),
        }
    }
}

impl Error for MusicSeekError {}

enum MusicCommand {
    Seek { frame: u64 },
}

// This handle owns the bounded seek channel and cancellation for one music job.
// Dropping it cancels only its own playback via the contained PlaybackHandle.
#[allow(dead_code)]
pub(super) struct MusicHandle {
    playback: PlaybackHandle,
    commands: SyncSender<MusicCommand>,
    seek_closed: Arc<Mutex<bool>>,
    frame_count: u64,
}

#[allow(dead_code)]
impl MusicHandle {
    pub(super) fn request_id(&self) -> u64 {
        self.playback.request_id()
    }

    pub(super) fn seek(&self, frame: u64) -> Result<(), MusicSeekError> {
        if frame > self.frame_count {
            return Err(MusicSeekError::OutOfRange {
                frame,
                frame_count: self.frame_count,
            });
        }
        let closed = self
            .seek_closed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *closed {
            return Err(MusicSeekError::Closed);
        }
        if self.playback.control.cancelled.load(Ordering::Acquire) {
            return Err(MusicSeekError::Cancelled);
        }
        self.commands
            .try_send(MusicCommand::Seek { frame })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => MusicSeekError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => MusicSeekError::Closed,
            })
    }

    pub(super) fn stop(&self) {
        self.playback.cancel();
    }

    pub(super) fn try_completion(&self, current_scene_generation: u64) -> Option<PlaybackResult> {
        self.playback.try_completion(current_scene_generation)
    }

    pub(super) fn wait(
        &self,
        current_scene_generation: u64,
        timeout: Duration,
    ) -> Option<PlaybackResult> {
        self.playback.wait(current_scene_generation, timeout)
    }
}

struct PlaybackJob {
    sound: PreparedSound,
    control: Arc<PlaybackControl>,
}

struct MusicJob {
    sound: PreparedSound,
    control: Arc<PlaybackControl>,
    owner: Generation,
    source_identity: String,
    commands: Receiver<MusicCommand>,
    seek_closed: Arc<Mutex<bool>>,
}

enum AudioJob {
    Sound(PlaybackJob),
    Music(MusicJob),
}

impl AudioJob {
    fn control(&self) -> &Arc<PlaybackControl> {
        match self {
            Self::Sound(job) => &job.control,
            Self::Music(job) => &job.control,
        }
    }
}

struct AdapterState {
    scene_generation: u64,
    next_request_id: u64,
    sender: Option<SyncSender<AudioJob>>,
    startup_error: Option<String>,
}

/// The scene host's single bounded audio worker. `submit` never waits for queue
/// space; a full queue refuses the request and drops its retained source lease.
#[derive(Resource)]
pub(super) struct AudioAdapter {
    state: Mutex<AdapterState>,
    active: Arc<Mutex<BTreeMap<u64, Weak<PlaybackControl>>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl AudioAdapter {
    fn with_sink<S: PcmSink>(capacity: usize, sink: S) -> Result<Self, AudioAdmissionError> {
        if capacity == 0 || capacity > MAX_QUEUE_CAPACITY {
            return Err(AudioAdmissionError::InvalidQueueCapacity);
        }
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let active = Arc::new(Mutex::new(BTreeMap::new()));
        let worker_active = active.clone();
        let worker = thread::Builder::new()
            .name("fallout-audio".into())
            .spawn(move || audio_worker(receiver, worker_active, sink))
            .map_err(|error| AudioAdmissionError::WorkerStart(error.to_string()))?;
        Ok(Self {
            state: Mutex::new(AdapterState {
                scene_generation: 0,
                next_request_id: 1,
                sender: Some(sender),
                startup_error: None,
            }),
            active,
            worker: Mutex::new(Some(worker)),
        })
    }

    fn unavailable(error: String) -> Self {
        Self {
            state: Mutex::new(AdapterState {
                scene_generation: 0,
                next_request_id: 1,
                sender: None,
                startup_error: Some(error),
            }),
            active: Arc::new(Mutex::new(BTreeMap::new())),
            worker: Mutex::new(None),
        }
    }

    /// Set the host scene epoch and cancel every older active or queued request.
    fn set_scene_generation(&self, generation: u64) -> Result<(), AudioAdmissionError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AudioAdmissionError::HostPoisoned)?;
            if state.scene_generation == generation {
                return Ok(());
            }
            state.scene_generation = generation;
        }
        let mut active = self
            .active
            .lock()
            .map_err(|_| AudioAdmissionError::HostPoisoned)?;
        active.retain(|_, weak| {
            let Some(control) = weak.upgrade() else {
                return false;
            };
            if control.scene_generation != generation {
                control.cancel();
            }
            true
        });
        Ok(())
    }

    // Later scene cue producers consume this API; the current inspection host
    // has no authored sound events to call it yet.
    #[allow(dead_code)]
    pub(super) fn submit(
        &self,
        sound: PreparedSound,
        scene_generation: u64,
    ) -> Result<PlaybackHandle, AudioAdmissionError> {
        let owner = Generation::new(sound.source().archive_sha256.clone())
            .map_err(|error| AudioAdmissionError::InvalidSource(error.to_string()))?;
        let token = owner
            .token()
            .map_err(|error| AudioAdmissionError::InvalidSource(error.to_string()))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| AudioAdmissionError::HostPoisoned)?;
        if let Some(error) = &state.startup_error {
            return Err(AudioAdmissionError::WorkerStart(error.clone()));
        }
        if state.sender.is_none() {
            return Err(AudioAdmissionError::Closed);
        }
        if scene_generation != state.scene_generation {
            return Err(AudioAdmissionError::StaleScene {
                requested_generation: scene_generation,
                current_generation: state.scene_generation,
            });
        }
        let request_id = state.next_request_id;
        let next_request_id = request_id
            .checked_add(1)
            .ok_or(AudioAdmissionError::RequestIdsExhausted)?;
        let control = Arc::new(PlaybackControl::new(request_id, scene_generation, token));
        let job = PlaybackJob {
            sound,
            control: control.clone(),
        };
        let sender = state.sender.as_ref().ok_or(AudioAdmissionError::Closed)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| AudioAdmissionError::HostPoisoned)?;
        active.insert(request_id, Arc::downgrade(&control));
        match sender.try_send(AudioJob::Sound(job)) {
            Ok(()) => {
                state.next_request_id = next_request_id;
                Ok(PlaybackHandle { control })
            }
            Err(TrySendError::Full(job)) => {
                drop(job);
                active.remove(&request_id);
                Err(AudioAdmissionError::QueueFull)
            }
            Err(TrySendError::Disconnected(job)) => {
                drop(job);
                active.remove(&request_id);
                Err(AudioAdmissionError::Closed)
            }
        }
    }

    /// Admit one source-bounded music stream on the same finite audio queue and
    /// worker as short sound requests.
    #[allow(dead_code)]
    pub(super) fn submit_music(
        &self,
        sound: PreparedSound,
        scene_generation: u64,
    ) -> Result<MusicHandle, AudioAdmissionError> {
        let source_identity = sound.source().archive_sha256.clone();
        let owner = Generation::new(source_identity.clone())
            .map_err(|error| AudioAdmissionError::InvalidSource(error.to_string()))?;
        let token = owner
            .token()
            .map_err(|error| AudioAdmissionError::InvalidSource(error.to_string()))?;
        let frame_count = sound.frame_count();
        let mut state = self
            .state
            .lock()
            .map_err(|_| AudioAdmissionError::HostPoisoned)?;
        if let Some(error) = &state.startup_error {
            return Err(AudioAdmissionError::WorkerStart(error.clone()));
        }
        if state.sender.is_none() {
            return Err(AudioAdmissionError::Closed);
        }
        if scene_generation != state.scene_generation {
            return Err(AudioAdmissionError::StaleScene {
                requested_generation: scene_generation,
                current_generation: state.scene_generation,
            });
        }
        let request_id = state.next_request_id;
        let next_request_id = request_id
            .checked_add(1)
            .ok_or(AudioAdmissionError::RequestIdsExhausted)?;
        let control = Arc::new(PlaybackControl::new(request_id, scene_generation, token));
        let (commands, command_receiver) = mpsc::sync_channel(MUSIC_SEEK_QUEUE_CAPACITY);
        let seek_closed = Arc::new(Mutex::new(false));
        let job = AudioJob::Music(MusicJob {
            sound,
            control: control.clone(),
            owner,
            source_identity,
            commands: command_receiver,
            seek_closed: seek_closed.clone(),
        });
        let sender = state.sender.as_ref().ok_or(AudioAdmissionError::Closed)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| AudioAdmissionError::HostPoisoned)?;
        active.insert(request_id, Arc::downgrade(&control));
        match sender.try_send(job) {
            Ok(()) => {
                state.next_request_id = next_request_id;
                Ok(MusicHandle {
                    playback: PlaybackHandle { control },
                    commands,
                    seek_closed,
                    frame_count,
                })
            }
            Err(TrySendError::Full(job)) => {
                drop(job);
                active.remove(&request_id);
                Err(AudioAdmissionError::QueueFull)
            }
            Err(TrySendError::Disconnected(job)) => {
                drop(job);
                active.remove(&request_id);
                Err(AudioAdmissionError::Closed)
            }
        }
    }

    /// Explicit shutdown for callers outside the render/update thread. App
    /// teardown merely cancels and detaches; it never joins a live decoder on a frame.
    // Kept for a future off-render host shutdown path; the current host relies on Drop.
    #[allow(dead_code)]
    pub(super) fn shutdown(&self) {
        self.cancel_active();
        if let Ok(mut state) = self.state.lock() {
            state.sender.take();
        }
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            let _ = worker.join();
        }
    }

    fn cancel_active(&self) {
        if let Ok(mut active) = self.active.lock() {
            active.retain(|_, weak| {
                let Some(control) = weak.upgrade() else {
                    return false;
                };
                control.cancel();
                true
            });
        }
    }
}

impl Drop for AudioAdapter {
    fn drop(&mut self) {
        self.cancel_active();
        if let Ok(state) = self.state.get_mut() {
            state.sender.take();
        }
        // Dropping a JoinHandle detaches. The closed finite queue and request
        // tokens let the worker drain and exit without blocking app teardown.
        if let Ok(worker) = self.worker.get_mut() {
            worker.take();
        }
    }
}

fn audio_worker<S: PcmSink>(
    receiver: Receiver<AudioJob>,
    active: Arc<Mutex<BTreeMap<u64, Weak<PlaybackControl>>>>,
    mut sink: S,
) {
    while let Ok(job) = receiver.recv() {
        let control = job.control().clone();
        let seek_closed = match &job {
            AudioJob::Sound(_) => None,
            AudioJob::Music(job) => Some(job.seek_closed.clone()),
        };
        let result = catch_unwind(AssertUnwindSafe(|| match job {
            AudioJob::Sound(job) => consume_sound(job.sound, &job.control, &mut sink),
            AudioJob::Music(job) => consume_music(job, &mut sink),
        }))
        .unwrap_or(Err(PlaybackFailure::WorkerPanicked));
        if let Some(seek_closed) = seek_closed {
            close_music_seek_queue(&seek_closed);
        }
        control.complete(result);
        if let Ok(mut active) = active.lock() {
            active.remove(&control.request_id);
        }
    }
}

fn consume_sound<S: PcmSink>(
    sound: PreparedSound,
    control: &PlaybackControl,
    sink: &mut S,
) -> PlaybackResult {
    let token = control.check()?;
    let format = sound.format();
    sink.begin(format).map_err(PlaybackFailure::Sink)?;
    control.check()?;
    let mut stream = sound.stream(token).map_err(map_audio_error)?;
    let mut frames_submitted = 0u64;
    while let Some(chunk) = stream.next_chunk().map_err(map_audio_error)? {
        control.check()?;
        let next_frames = frames_submitted
            .checked_add(chunk.frame_count as u64)
            .ok_or_else(|| PlaybackFailure::Decode("frame count overflow".into()))?;
        sink.write(chunk).map_err(PlaybackFailure::Sink)?;
        control.check()?;
        frames_submitted = next_frames;
    }
    control.check()?;
    sink.finish().map_err(PlaybackFailure::Sink)?;
    Ok(PlaybackReceipt {
        request_id: control.request_id,
        scene_generation: control.scene_generation,
        frames_submitted,
        format,
    })
}

fn consume_music<S: PcmSink>(job: MusicJob, sink: &mut S) -> PlaybackResult {
    let MusicJob {
        sound,
        control,
        owner,
        source_identity,
        commands,
        seek_closed,
    } = job;
    let format = sound.format();
    let mut token = control.check()?;
    sink.begin(format).map_err(PlaybackFailure::Sink)?;
    let mut stream = sound.stream(token.clone()).map_err(map_audio_error)?;
    let mut frames_submitted = 0u64;

    loop {
        control.check()?;
        for _ in 0..MUSIC_SEEK_QUEUE_CAPACITY {
            match commands.try_recv() {
                Ok(MusicCommand::Seek { frame }) => {
                    seek_music_stream(
                        frame,
                        &mut stream,
                        &owner,
                        &source_identity,
                        &control,
                        &mut token,
                    )?;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(PlaybackFailure::Cancelled);
                }
            }
        }

        control.check()?;
        match stream.next_chunk().map_err(map_audio_error)? {
            Some(chunk) => {
                control.check()?;
                let next_frames = frames_submitted
                    .checked_add(chunk.frame_count as u64)
                    .ok_or_else(|| PlaybackFailure::Decode("frame count overflow".into()))?;
                sink.write(chunk).map_err(PlaybackFailure::Sink)?;
                control.check()?;
                frames_submitted = next_frames;
            }
            None => {
                let mut closed = seek_closed
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                control.check()?;
                match commands.try_recv() {
                    Ok(MusicCommand::Seek { frame }) => {
                        drop(closed);
                        seek_music_stream(
                            frame,
                            &mut stream,
                            &owner,
                            &source_identity,
                            &control,
                            &mut token,
                        )?;
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        *closed = true;
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        *closed = true;
                        return Err(PlaybackFailure::Cancelled);
                    }
                }
            }
        }
    }

    sink.finish().map_err(PlaybackFailure::Sink)?;
    Ok(PlaybackReceipt {
        request_id: control.request_id,
        scene_generation: control.scene_generation,
        frames_submitted,
        format,
    })
}

fn seek_music_stream(
    frame: u64,
    stream: &mut PcmStream<'_>,
    owner: &Generation,
    source_identity: &str,
    control: &PlaybackControl,
    token: &mut JobToken,
) -> Result<(), PlaybackFailure> {
    control.check()?;
    owner
        .advance(source_identity.to_owned())
        .map_err(map_resource_error)?;
    let next_token = owner.token().map_err(map_resource_error)?;
    control.replace_token(next_token.clone())?;
    stream
        .seek(frame, next_token.clone())
        .map_err(map_audio_error)?;
    *token = next_token;
    Ok(())
}

fn close_music_seek_queue(seek_closed: &Mutex<bool>) {
    *seek_closed
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
}

fn map_audio_error(error: AudioError) -> PlaybackFailure {
    match error {
        AudioError::Resource(JobError::Cancelled) => PlaybackFailure::Cancelled,
        AudioError::Resource(JobError::Stale) => PlaybackFailure::StaleSource,
        other => PlaybackFailure::Decode(other.to_string()),
    }
}

fn map_resource_error(error: JobError) -> PlaybackFailure {
    match error {
        JobError::Cancelled => PlaybackFailure::Cancelled,
        JobError::Stale => PlaybackFailure::StaleSource,
        other => PlaybackFailure::Decode(other.to_string()),
    }
}

pub(super) struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        let adapter = AudioAdapter::with_sink(DEFAULT_QUEUE_CAPACITY, UnavailableSink)
            .unwrap_or_else(|error| AudioAdapter::unavailable(error.to_string()));
        app.insert_resource(adapter).add_systems(
            Update,
            synchronize_scene_generation.after(super::drive_loading),
        );
    }
}

fn synchronize_scene_generation(loading: Res<super::Loading>, adapter: Res<AudioAdapter>) {
    if let Err(error) = adapter.set_scene_generation(loading.epoch) {
        error!("Audio scene generation update failed: {error}");
    }
}
