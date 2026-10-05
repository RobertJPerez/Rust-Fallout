//! Bevy playback for bounded PCM chunks.
//!
//! The resource worker prepares each source and feeds one bounded queue. Bevy's
//! Rodio source consumes that queue on its audio thread; `AudioSinkPlayback`
//! supplies the observable source position. A drained Bevy source is not proof
//! that hardware buffers drained or that audio was audible.

use super::{
    AudioAdapter, AudioAdmissionError, DevicePlaybackPhase, PcmSink, PlaybackControl,
    PlaybackFailure,
};
use bevy::{
    asset::{Assets, Handle},
    audio::{
        AddAudioSource, AudioPlayer, AudioSink, AudioSinkPlayback, ChannelCount, Decodable,
        PlaybackSettings, SampleRate, Source,
    },
    prelude::{App, Commands, Component, Last, Res, ResMut, Resource, TypePath, Update},
};
use fallout_data::audio::{PcmChunk, WaveFormat};
use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

const DEVICE_CHUNK_CAPACITY: usize = 4;
const DEVICE_START_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Resource)]
pub(super) struct DeviceRequests(Mutex<Receiver<DeviceStart>>);

struct DeviceStart {
    format: WaveFormat,
    frame_count: u64,
    chunks: Receiver<PcmChunk>,
    control: Arc<PlaybackControl>,
    queued_at: Instant,
}

/// One queued, interleaved PCM source. Samples stay bounded to four producer
/// chunks, regardless of the prepared archive member's length.
#[derive(bevy::asset::Asset, TypePath)]
pub(super) struct DevicePcmSource {
    format: WaveFormat,
    frame_count: u64,
    chunks: Arc<Mutex<Receiver<PcmChunk>>>,
    control: Arc<PlaybackControl>,
    decoder_started: AtomicBool,
}

impl DevicePcmSource {
    pub(super) fn new(
        format: WaveFormat,
        frame_count: u64,
        chunks: Receiver<PcmChunk>,
        control: Arc<PlaybackControl>,
    ) -> Self {
        Self {
            format,
            frame_count,
            chunks: Arc::new(Mutex::new(chunks)),
            control,
            decoder_started: AtomicBool::new(false),
        }
    }
}

impl Decodable for DevicePcmSource {
    type Decoder = DevicePcmDecoder;

    fn decoder(&self) -> Self::Decoder {
        if self.decoder_started.swap(true, Ordering::AcqRel) {
            self.control.fail_device(PlaybackFailure::Sink(
                "one-shot PCM source was opened more than once".into(),
            ));
            return DevicePcmDecoder::empty(self.format, self.frame_count, self.control.clone());
        }
        DevicePcmDecoder::new(
            self.format,
            self.frame_count,
            self.chunks.clone(),
            self.control.clone(),
        )
    }
}

pub(super) struct DevicePcmDecoder {
    format: WaveFormat,
    channels: ChannelCount,
    sample_rate: SampleRate,
    frame_count: u64,
    chunks: Option<Arc<Mutex<Receiver<PcmChunk>>>>,
    current: Option<PcmChunk>,
    sample_index: usize,
    next_frame: u64,
    control: Arc<PlaybackControl>,
    ended: bool,
}

impl DevicePcmDecoder {
    fn new(
        format: WaveFormat,
        frame_count: u64,
        chunks: Arc<Mutex<Receiver<PcmChunk>>>,
        control: Arc<PlaybackControl>,
    ) -> Self {
        Self {
            format,
            channels: NonZeroU16::new(format.channels).expect("prepared channel count is nonzero"),
            sample_rate: NonZeroU32::new(format.sample_rate)
                .expect("prepared sample rate is nonzero"),
            frame_count,
            chunks: Some(chunks),
            current: None,
            sample_index: 0,
            next_frame: 0,
            control,
            ended: false,
        }
    }

    fn empty(format: WaveFormat, frame_count: u64, control: Arc<PlaybackControl>) -> Self {
        let mut decoder = Self::new(
            format,
            frame_count,
            Arc::new(Mutex::new(mpsc::sync_channel(1).1)),
            control,
        );
        decoder.chunks = None;
        decoder.ended = true;
        decoder
    }

    fn fail(&mut self, failure: PlaybackFailure) {
        self.control.fail_device(failure);
        self.ended = true;
        self.current = None;
        self.chunks = None;
    }

    fn next_chunk(&mut self) -> bool {
        let Some(chunks) = self.chunks.as_ref() else {
            self.ended = true;
            return false;
        };
        let received = match chunks.lock() {
            Ok(receiver) => receiver.try_recv(),
            Err(_) => {
                self.fail(PlaybackFailure::Sink(
                    "PCM output queue lock was poisoned".into(),
                ));
                return false;
            }
        };
        match received {
            Ok(chunk) => {
                let sample_count = chunk
                    .frame_count
                    .checked_mul(usize::from(self.format.channels));
                if chunk.first_frame != self.next_frame
                    || sample_count != Some(chunk.interleaved_samples.len())
                    || chunk.frame_count == 0
                {
                    self.fail(PlaybackFailure::Decode(
                        "PCM output chunk has invalid frame layout".into(),
                    ));
                    return false;
                }
                let Some(next_frame) = self.next_frame.checked_add(chunk.frame_count as u64) else {
                    self.fail(PlaybackFailure::Decode(
                        "PCM output frame count overflow".into(),
                    ));
                    return false;
                };
                self.next_frame = next_frame;
                self.current = Some(chunk);
                self.sample_index = 0;
                true
            }
            Err(TryRecvError::Empty) => {
                self.fail(PlaybackFailure::Sink(
                    "bounded PCM output queue underrun".into(),
                ));
                false
            }
            Err(TryRecvError::Disconnected) => {
                if self.next_frame != self.frame_count {
                    self.fail(PlaybackFailure::Decode(
                        "PCM output ended before the prepared frame count".into(),
                    ));
                    return false;
                }
                self.ended = true;
                self.chunks = None;
                false
            }
        }
    }
}

impl Iterator for DevicePcmDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ended || self.control.cancelled.load(Ordering::Acquire) {
            self.ended = true;
            return None;
        }
        loop {
            if let Some(chunk) = self.current.as_ref()
                && self.sample_index < chunk.interleaved_samples.len()
            {
                let sample = chunk.interleaved_samples[self.sample_index];
                self.sample_index += 1;
                return Some(sample);
            }
            self.current = None;
            if !self.next_chunk() {
                return None;
            }
        }
    }
}

impl Source for DevicePcmDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        let rate = u128::from(self.format.sample_rate);
        let nanos = u128::from(self.frame_count).checked_mul(1_000_000_000)? / rate;
        Some(Duration::new(
            u64::try_from(nanos / 1_000_000_000).ok()?,
            u32::try_from(nanos % 1_000_000_000).ok()?,
        ))
    }
}

#[derive(Component)]
struct DevicePlayback {
    control: Arc<PlaybackControl>,
    source: Handle<DevicePcmSource>,
    queued_at: Instant,
}

pub(super) struct BevyPcmSink {
    requests: SyncSender<DeviceStart>,
    chunks: Option<SyncSender<PcmChunk>>,
    receiver: Option<Receiver<PcmChunk>>,
    control: Option<Arc<PlaybackControl>>,
    format: Option<WaveFormat>,
    frame_count: u64,
    queued_chunks: usize,
    started: bool,
    queued_at: Option<Instant>,
}

impl BevyPcmSink {
    pub(super) fn channel() -> (Self, DeviceRequests) {
        let (request_sender, request_receiver) = mpsc::sync_channel(DEFAULT_DEVICE_QUEUE_CAPACITY);
        (
            Self {
                requests: request_sender,
                chunks: None,
                receiver: None,
                control: None,
                format: None,
                frame_count: 0,
                queued_chunks: 0,
                started: false,
                queued_at: None,
            },
            DeviceRequests(Mutex::new(request_receiver)),
        )
    }

    fn start(&mut self) -> Result<(), PlaybackFailure> {
        if self.started {
            return Ok(());
        }
        let (Some(receiver), Some(control), Some(format), Some(queued_at)) = (
            self.receiver.take(),
            self.control.as_ref(),
            self.format,
            self.queued_at,
        ) else {
            return Err(PlaybackFailure::Sink(
                "PCM playback request was not initialized".into(),
            ));
        };
        let request = DeviceStart {
            format,
            frame_count: self.frame_count,
            chunks: receiver,
            control: control.clone(),
            queued_at,
        };
        self.requests
            .try_send(request)
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    PlaybackFailure::Sink("Bevy audio start queue is full".into())
                }
                TrySendError::Disconnected(_) => {
                    PlaybackFailure::Sink("Bevy audio start queue is closed".into())
                }
            })?;
        self.started = true;
        Ok(())
    }

    fn clear(&mut self) {
        self.chunks.take();
        self.receiver.take();
        self.control.take();
        self.format.take();
        self.frame_count = 0;
        self.queued_chunks = 0;
        self.started = false;
        self.queued_at.take();
    }
}

impl PcmSink for BevyPcmSink {
    fn begin(
        &mut self,
        format: WaveFormat,
        frame_count: u64,
        control: Arc<PlaybackControl>,
    ) -> Result<(), PlaybackFailure> {
        self.clear();
        let (sender, receiver) = mpsc::sync_channel(DEVICE_CHUNK_CAPACITY);
        self.chunks = Some(sender);
        self.receiver = Some(receiver);
        self.control = Some(control);
        self.format = Some(format);
        self.frame_count = frame_count;
        self.queued_at = Some(Instant::now());
        Ok(())
    }

    fn write(&mut self, mut chunk: PcmChunk) -> Result<(), PlaybackFailure> {
        let sender = self
            .chunks
            .as_ref()
            .cloned()
            .ok_or_else(|| PlaybackFailure::Sink("PCM output queue is not open".into()))?;
        let control = self
            .control
            .as_ref()
            .cloned()
            .ok_or_else(|| PlaybackFailure::Sink("PCM playback control is missing".into()))?;
        let queued_at = self
            .queued_at
            .ok_or_else(|| PlaybackFailure::Sink("PCM playback deadline is missing".into()))?;
        loop {
            if control.cancelled.load(Ordering::Acquire) {
                return Err(PlaybackFailure::Cancelled);
            }
            if let Some(Err(error)) = control.device_outcome() {
                return Err(error);
            }
            if !self.started && queued_at.elapsed() >= DEVICE_START_TIMEOUT {
                let error = PlaybackFailure::Sink(
                    "Bevy audio output did not start before the bounded startup timeout".into(),
                );
                control.fail_device(error.clone());
                return Err(error);
            }
            match sender.try_send(chunk) {
                Ok(()) => {
                    self.queued_chunks += 1;
                    if !self.started && self.queued_chunks >= DEVICE_CHUNK_CAPACITY {
                        self.start()?;
                    }
                    return Ok(());
                }
                Err(TrySendError::Full(returned)) => {
                    chunk = returned;
                    if !self.started {
                        self.start()?;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(PlaybackFailure::Sink(
                        "Bevy PCM consumer disconnected".into(),
                    ));
                }
            }
        }
    }

    fn finish(&mut self) -> Result<(), PlaybackFailure> {
        if self.queued_chunks == 0 {
            self.clear();
            return Ok(());
        }
        self.start()?;
        self.chunks.take();
        let result = self
            .control
            .as_ref()
            .ok_or_else(|| PlaybackFailure::Sink("PCM playback control is missing".into()))?
            .wait_for_device_end();
        self.clear();
        result
    }

    fn supports_music(&self) -> bool {
        false
    }
}

pub(super) fn start_device_requests(
    mut commands: Commands,
    mut sources: ResMut<Assets<DevicePcmSource>>,
    requests: Res<DeviceRequests>,
) {
    let Ok(receiver) = requests.0.lock() else {
        return;
    };
    while let Ok(request) = receiver.try_recv() {
        if request.control.cancelled.load(Ordering::Acquire) {
            continue;
        }
        let source = sources.add(DevicePcmSource::new(
            request.format,
            request.frame_count,
            request.chunks,
            request.control.clone(),
        ));
        commands.spawn((
            AudioPlayer(source.clone()),
            PlaybackSettings::ONCE,
            DevicePlayback {
                control: request.control,
                source,
                queued_at: request.queued_at,
            },
        ));
    }
}

pub(super) fn observe_device_playbacks(
    mut commands: Commands,
    mut sources: ResMut<Assets<DevicePcmSource>>,
    mut playbacks: bevy::prelude::Query<(
        bevy::prelude::Entity,
        &DevicePlayback,
        Option<&AudioSink>,
    )>,
) {
    for (entity, playback, sink) in &mut playbacks {
        let control = &playback.control;
        if control.cancelled.load(Ordering::Acquire) {
            control.fail_device(PlaybackFailure::Cancelled);
        } else if let Some(sink) = sink {
            control.update_device_position(sink.position());
            if sink.empty() {
                control.end_device_playback();
            }
        } else if playback.queued_at.elapsed() >= DEVICE_START_TIMEOUT {
            control.fail_device(PlaybackFailure::Sink(
                "Bevy audio output device did not create a playback sink".into(),
            ));
        }

        if let Some(outcome) = control.device_outcome() {
            if outcome.is_err()
                && let Some(sink) = sink
            {
                sink.stop();
            }
            commands.entity(entity).despawn();
            sources.remove(playback.source.id());
        }
    }
}

const DEFAULT_DEVICE_QUEUE_CAPACITY: usize = 8;

pub(super) fn install(app: &mut App) -> Result<(), AudioAdmissionError> {
    let (sink, requests) = BevyPcmSink::channel();
    let adapter = AudioAdapter::with_sink(super::DEFAULT_QUEUE_CAPACITY, sink)?;
    app.add_audio_source::<DevicePcmSource>()
        .insert_resource(requests)
        .insert_resource(adapter)
        .add_systems(Update, start_device_requests.after(super::drive_loading))
        .add_systems(
            Update,
            super::synchronize_scene_generation.after(super::drive_loading),
        )
        .add_systems(Last, observe_device_playbacks);
    Ok(())
}
