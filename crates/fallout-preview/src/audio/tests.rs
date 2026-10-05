use super::*;
use fallout_data::{
    resource_jobs::{ArchiveInput, Limits as JobLimits, ResourceJobs},
    vfs::{AssetSource, MountIndex},
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("fallout-audio-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct SoundFixture {
    // Keep the artifact first and the temporary directory last so archive
    // handles close before the synthetic file is removed on Windows.
    sound: Option<PreparedSound>,
    _jobs: ResourceJobs,
    _generation: Generation,
    _mounts: MountIndex,
    _archive: Arc<ArchiveInput>,
    _root: TempRoot,
}

impl SoundFixture {
    fn new(frames: usize) -> Self {
        let root = TempRoot::new();
        let archive_path = root.0.join("audio.bsa");
        let wave = pcm_wave(frames);
        fs::write(&archive_path, authored_bsa(&wave)).unwrap();
        let archive = ArchiveInput::open(&archive_path).unwrap();
        let mut mounts = MountIndex::default();
        let source = AssetSource {
            container: archive_path.display().to_string(),
            entry_index: 0,
            original_path: b"sound\\tone.wav".to_vec(),
        };
        mounts.insert(source).unwrap();
        let generation = Generation::new("a".repeat(64)).unwrap();
        let jobs = ResourceJobs::new(
            JobLimits {
                workers: 1,
                outstanding: 4,
                decoded_bytes: 4 * 1024 * 1024,
            },
            generation.clone(),
        )
        .unwrap();
        let sound = fallout_data::audio::request_source_sound(
            &jobs,
            &generation,
            &archive,
            &mounts,
            b"sound/tone.wav",
            fallout_data::audio::AudioLimits { chunk_frames: 128 },
        )
        .unwrap()
        .wait()
        .unwrap();
        Self {
            sound: Some(sound),
            _jobs: jobs,
            _generation: generation,
            _mounts: mounts,
            _archive: archive,
            _root: root,
        }
    }

    fn take(&mut self) -> PreparedSound {
        self.sound.take().unwrap()
    }
}

#[derive(Default)]
struct Observation {
    format: Option<WaveFormat>,
    frames: usize,
    finished: bool,
}

struct ProbeSink {
    observation: Arc<Mutex<Observation>>,
    started: Option<mpsc::Sender<()>>,
    gate: Option<mpsc::Receiver<()>>,
    fail_write: bool,
}

impl ProbeSink {
    fn new(
        started: Option<mpsc::Sender<()>>,
        gate: Option<mpsc::Receiver<()>>,
        fail_write: bool,
    ) -> (Self, Arc<Mutex<Observation>>) {
        let observation = Arc::new(Mutex::new(Observation::default()));
        (
            Self {
                observation: observation.clone(),
                started,
                gate,
                fail_write,
            },
            observation,
        )
    }
}

impl PcmSink for ProbeSink {
    fn begin(&mut self, format: WaveFormat) -> Result<(), String> {
        self.observation.lock().unwrap().format = Some(format);
        Ok(())
    }

    fn write(&mut self, chunk: PcmChunk) -> Result<(), String> {
        if let Some(started) = self.started.take() {
            started
                .send(())
                .map_err(|error| format!("test start signal failed: {error}"))?;
        }
        if let Some(gate) = self.gate.take() {
            gate.recv_timeout(Duration::from_secs(3))
                .map_err(|error| format!("test output gate failed: {error}"))?;
        }
        if self.fail_write {
            return Err("synthetic sink failure".into());
        }
        self.observation.lock().unwrap().frames += chunk.frame_count;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.observation.lock().unwrap().finished = true;
        Ok(())
    }
}

fn pcm_wave(frames: usize) -> Vec<u8> {
    let samples = vec![0u8; frames * 2];
    let mut body = b"WAVE".to_vec();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&44_100u32.to_le_bytes());
    body.extend_from_slice(&88_200u32.to_le_bytes());
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&16u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    body.extend_from_slice(&samples);
    let mut wave = b"RIFF".to_vec();
    wave.extend_from_slice(&(body.len() as u32).to_le_bytes());
    wave.extend_from_slice(&body);
    wave
}

fn authored_bsa(payload: &[u8]) -> Vec<u8> {
    let folder = b"sound\0";
    let filename = b"tone.wav\0";
    let folder_name_offset = 52usize;
    let file_record_offset = folder_name_offset + 1 + folder.len();
    let filename_offset = file_record_offset + 16;
    let data_offset = filename_offset + filename.len();
    let mut bytes = vec![0u8; data_offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (offset, value) in [
        (4, 104u32),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 1),
        (24, folder.len() as u32),
        (28, filename.len() as u32),
        (44, 1),
        (48, folder_name_offset as u32),
        (file_record_offset + 8, payload.len() as u32),
        (file_record_offset + 12, data_offset as u32),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[folder_name_offset] = folder.len() as u8;
    bytes[folder_name_offset + 1..folder_name_offset + 1 + folder.len()].copy_from_slice(folder);
    bytes[filename_offset..filename_offset + filename.len()].copy_from_slice(filename);
    bytes.extend_from_slice(payload);
    bytes
}

fn make_adapter(capacity: usize, sink: impl PcmSink) -> AudioAdapter {
    AudioAdapter::with_sink(capacity, sink).unwrap()
}

fn admission_error(result: Result<PlaybackHandle, AudioAdmissionError>) -> AudioAdmissionError {
    match result {
        Err(error) => error,
        Ok(handle) => {
            drop(handle);
            panic!("request was unexpectedly admitted")
        }
    }
}

#[test]
fn successful_request_submits_bounded_source_format_frames_and_sticky_completion() {
    let (sink, observation) = ProbeSink::new(None, None, false);
    let adapter = make_adapter(2, sink);
    adapter.set_scene_generation(7).unwrap();
    let mut fixture = SoundFixture::new(1500);
    let handle = adapter.submit(fixture.take(), 7).unwrap();
    let completion = handle.wait(7, Duration::from_secs(2)).unwrap().unwrap();
    assert_eq!(completion.request_id, handle.request_id());
    assert_eq!(completion.scene_generation, 7);
    assert_eq!(completion.frames_submitted, 1500);
    assert_eq!(completion.format.sample_rate, 44_100);
    assert_eq!(completion.format.channels, 1);
    assert_eq!(handle.try_completion(7), Some(Ok(completion)));
    let observed = observation.lock().unwrap();
    assert_eq!(observed.format.unwrap().sample_rate, 44_100);
    assert_eq!(observed.frames, 1500);
    assert!(observed.finished);
    drop(observed);
    adapter.shutdown();
}

#[test]
fn finite_queue_refuses_full_admission_and_cancellation_has_error_completion() {
    let (started, began) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (sink, _) = ProbeSink::new(Some(started), Some(gate), false);
    let adapter = make_adapter(1, sink);
    adapter.set_scene_generation(4).unwrap();
    let mut first_source = SoundFixture::new(2048);
    let first = adapter.submit(first_source.take(), 4).unwrap();
    began.recv_timeout(Duration::from_secs(2)).unwrap();

    let mut queued_source = SoundFixture::new(2);
    let queued = adapter.submit(queued_source.take(), 4).unwrap();
    let mut overflow_source = SoundFixture::new(2);
    assert_eq!(
        admission_error(adapter.submit(overflow_source.take(), 4)),
        AudioAdmissionError::QueueFull
    );

    first.cancel();
    queued.cancel();
    release.send(()).unwrap();
    assert_eq!(
        first.wait(4, Duration::from_secs(2)),
        Some(Err(PlaybackFailure::Cancelled))
    );
    assert_eq!(
        queued.wait(4, Duration::from_secs(2)),
        Some(Err(PlaybackFailure::Cancelled))
    );
    adapter.shutdown();
}

#[test]
fn new_scene_cancels_old_requests_and_old_completion_is_refused() {
    let (started, began) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let (sink, _) = ProbeSink::new(Some(started), Some(gate), false);
    let adapter = make_adapter(2, sink);
    adapter.set_scene_generation(11).unwrap();
    let mut fixture = SoundFixture::new(2048);
    let handle = adapter.submit(fixture.take(), 11).unwrap();
    began.recv_timeout(Duration::from_secs(2)).unwrap();

    adapter.set_scene_generation(12).unwrap();
    assert_eq!(
        handle.try_completion(12),
        Some(Err(PlaybackFailure::StaleScene {
            request_generation: 11,
            current_generation: 12,
        }))
    );
    let mut stale_source = SoundFixture::new(2);
    assert_eq!(
        admission_error(adapter.submit(stale_source.take(), 11)),
        AudioAdmissionError::StaleScene {
            requested_generation: 11,
            current_generation: 12,
        }
    );

    release.send(()).unwrap();
    assert_eq!(
        handle.wait(11, Duration::from_secs(2)),
        Some(Err(PlaybackFailure::Cancelled))
    );
    adapter.shutdown();
}

#[test]
fn sink_failure_is_a_sticky_error_and_unavailable_output_never_succeeds() {
    let (sink, _) = ProbeSink::new(None, None, true);
    let adapter = make_adapter(2, sink);
    adapter.set_scene_generation(3).unwrap();
    let mut fixture = SoundFixture::new(4);
    let handle = adapter.submit(fixture.take(), 3).unwrap();
    let first = handle.wait(3, Duration::from_secs(2)).unwrap();
    assert_eq!(
        first,
        Err(PlaybackFailure::Sink("synthetic sink failure".into()))
    );
    assert_eq!(handle.try_completion(3), Some(first));
    adapter.shutdown();

    let adapter = make_adapter(2, UnavailableSink);
    adapter.set_scene_generation(3).unwrap();
    let mut fixture = SoundFixture::new(4);
    let handle = adapter.submit(fixture.take(), 3).unwrap();
    assert_eq!(
        handle.wait(3, Duration::from_secs(2)).unwrap(),
        Err(PlaybackFailure::Sink(
            "preview audio output backend is unavailable".into()
        ))
    );
    adapter.shutdown();
}
