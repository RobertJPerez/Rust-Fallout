use fallout_data::{
    audio::{
        AudioError, AudioLimits, PreparedSound, WaveError, inspect_source_bytes,
        request_source_sound,
    },
    identity::ProfileId,
    resource_jobs::{ArchiveInput, Generation, Limits as JobLimits, ResourceJobs},
    vfs::{AssetPath, AssetSource, MountIndex},
};
use std::{fs, sync::Arc};

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn wave(channels: u16, sample_rate: u32, bits: u16, samples: &[u8]) -> Vec<u8> {
    wave_with_tag(1, channels, sample_rate, bits, samples)
}

fn wave_with_tag(
    format_tag: u16,
    channels: u16,
    sample_rate: u32,
    bits: u16,
    samples: &[u8],
) -> Vec<u8> {
    let align = channels.saturating_mul(bits / 8);
    let byte_rate = sample_rate.saturating_mul(u32::from(align));
    let mut format = Vec::with_capacity(16);
    format.extend(format_tag.to_le_bytes());
    format.extend(channels.to_le_bytes());
    format.extend(sample_rate.to_le_bytes());
    format.extend(byte_rate.to_le_bytes());
    format.extend(align.to_le_bytes());
    format.extend(bits.to_le_bytes());
    riff_wave(&[(b"fmt ", &format), (b"data", samples)])
}

fn riff_wave(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    for (id, payload) in chunks {
        body.extend_from_slice(*id);
        body.extend((payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn authored_bsa(payload: &[u8]) -> Vec<u8> {
    // Synthetic BSA104 with one sound/tone.wav entry and no retail bytes.
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
        put_u32(&mut bytes, offset, value);
    }
    bytes[folder_name_offset] = folder.len() as u8;
    bytes[folder_name_offset + 1..folder_name_offset + 1 + folder.len()].copy_from_slice(folder);
    bytes[filename_offset..filename_offset + filename.len()].copy_from_slice(filename);
    bytes.extend(payload);
    bytes
}

struct Fixture {
    _root: tempfile::TempDir,
    archive_path: std::path::PathBuf,
    archive: Arc<ArchiveInput>,
    mounts: MountIndex,
}

impl Fixture {
    fn new(payload: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let archive_path = root.path().join("sound.bsa");
        fs::write(&archive_path, authored_bsa(payload)).unwrap();
        let archive = ArchiveInput::open(&archive_path).unwrap();
        let mut mounts = MountIndex::default();
        mounts
            .insert(AssetSource {
                container: archive_path.display().to_string(),
                entry_index: 0,
                original_path: b"sound\\tone.wav".to_vec(),
            })
            .unwrap();
        Self {
            _root: root,
            archive_path,
            archive,
            mounts,
        }
    }

    fn jobs(&self) -> (Generation, ResourceJobs) {
        let generation = Generation::new("a".repeat(64)).unwrap();
        let jobs = ResourceJobs::new(
            JobLimits {
                workers: 1,
                outstanding: 2,
                decoded_bytes: 1024 * 1024,
            },
            generation.clone(),
        )
        .unwrap();
        (generation, jobs)
    }

    fn request(&self, jobs: &ResourceJobs, generation: &Generation) -> PreparedSound {
        request_source_sound(
            jobs,
            generation,
            &self.archive,
            &self.mounts,
            b"Sound/TONE.wav",
            AudioLimits::default(),
        )
        .unwrap()
        .wait()
        .unwrap()
    }
}

#[test]
fn source_request_uses_unique_vfs_member_and_keeps_resource_lease_alive() {
    let fixture = Fixture::new(&wave(2, 44_100, 16, &[0, 0, 0, 0, 0xff, 0x7f, 0, 0x80]));
    let (generation, jobs) = fixture.jobs();
    let prepared = fixture.request(&jobs, &generation);
    assert_eq!(prepared.source().profile, ProfileId::NvOriginal);
    assert_eq!(
        prepared.source().path,
        AssetPath::new(b"sound/tone.wav").unwrap()
    );
    assert_eq!(prepared.source().original_path, b"sound\\tone.wav");
    assert_eq!(
        prepared.source().container,
        fixture.archive_path.display().to_string()
    );
    assert_eq!(prepared.source().entry_index, 0);
    assert_eq!(
        prepared.source().archive_sha256,
        fixture.archive.source_sha256()
    );
    assert_eq!(prepared.format().channels, 2);
    assert_eq!(prepared.format().sample_rate, 44_100);
    assert_eq!(prepared.frame_count(), 2);
    assert_eq!(jobs.usage().outstanding, 1);
    drop(prepared);
    assert_eq!(jobs.usage().outstanding, 0);
    assert_eq!(jobs.usage().decoded_bytes, 0);
}

#[test]
fn pcm_chunks_are_bounded_interleaved_and_cancellable() {
    let fixture = Fixture::new(&wave(
        1,
        22_050,
        16,
        &[0, 0, 0x00, 0x40, 0x00, 0xc0, 0xff, 0x7f],
    ));
    let (generation, jobs) = fixture.jobs();
    let prepared = fixture.request(&jobs, &generation);
    let stream_generation = Generation::new("b".repeat(64)).unwrap();
    let token = stream_generation.token().unwrap();
    let mut stream = prepared.stream(token).unwrap();
    let first = stream.next_chunk().unwrap().unwrap();
    assert_eq!(first.first_frame, 0);
    assert_eq!(first.frame_count, 4);
    assert_eq!(
        first.interleaved_samples,
        [0.0, 0.5, -0.5, 32767.0 / 32768.0]
    );
    assert!(stream.next_chunk().unwrap().is_none());
    drop(stream);
    drop(prepared);
    assert_eq!(jobs.usage().outstanding, 0);

    let fixture = Fixture::new(&wave(1, 22_050, 8, &[0, 64, 128, 192, 255, 128]));
    let (generation, jobs) = fixture.jobs();
    let prepared = fixture.request(&jobs, &generation);
    let stream_generation = Generation::new("c".repeat(64)).unwrap();
    let mut stream = prepared.stream(stream_generation.token().unwrap()).unwrap();
    assert_eq!(
        stream.next_chunk().unwrap().unwrap().interleaved_samples,
        [-1.0, -0.5, 0.0, 0.5, 127.0 / 128.0, 0.0]
    );
    drop(stream);
}

#[test]
fn cancellation_stops_the_next_bounded_decode_chunk() {
    let mut pcm = Vec::new();
    for _ in 0..4096 {
        pcm.extend(0i16.to_le_bytes());
    }
    let fixture = Fixture::new(&wave(1, 44_100, 16, &pcm));
    let (generation, jobs) = fixture.jobs();
    let prepared = fixture.request(&jobs, &generation);
    let stream_generation = Generation::new("d".repeat(64)).unwrap();
    let token = stream_generation.token().unwrap();
    let mut stream = prepared.stream(token).unwrap();
    assert_eq!(stream.next_chunk().unwrap().unwrap().frame_count, 1024);
    stream.cancel();
    assert!(matches!(
        stream.next_chunk(),
        Err(AudioError::Resource(
            fallout_data::resource_jobs::JobError::Cancelled
        ))
    ));
}

#[test]
fn malformed_and_unsupported_waves_are_explicitly_rejected() {
    let short_format = vec![1u8, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16];
    let short = riff_wave(&[(b"fmt ", &short_format), (b"data", &[0, 0])]);
    assert!(matches!(
        inspect_source_bytes(&short, AudioLimits::default()),
        Err(WaveError::Truncated { .. })
    ));

    let partial_format = vec![
        1u8, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, 0,
    ];
    let partial_extension = riff_wave(&[(b"fmt ", &partial_format), (b"data", &[0, 0])]);
    assert!(matches!(
        inspect_source_bytes(&partial_extension, AudioLimits::default()),
        Err(WaveError::Truncated { needed: 1, .. })
    ));

    let zero_channels = wave(0, 44_100, 16, &[0, 0]);
    assert!(matches!(
        inspect_source_bytes(&zero_channels, AudioLimits::default()),
        Err(WaveError::Invalid {
            reason: "channel count is zero",
            ..
        })
    ));
    let zero_rate = wave(1, 0, 16, &[0, 0]);
    assert!(matches!(
        inspect_source_bytes(&zero_rate, AudioLimits::default()),
        Err(WaveError::Invalid {
            reason: "sample rate is zero",
            ..
        })
    ));

    let mut oversized = wave(1, 44_100, 16, &[0, 0]);
    put_u32(&mut oversized, 40, u32::MAX - 3);
    assert!(matches!(
        inspect_source_bytes(&oversized, AudioLimits::default()),
        Err(WaveError::Invalid {
            reason: "declared chunk payload exceeds the RIFF form",
            ..
        })
    ));

    let adpcm = wave_with_tag(2, 1, 44_100, 4, &[0, 0]);
    assert!(matches!(
        inspect_source_bytes(&adpcm, AudioLimits::default()),
        Err(WaveError::UnsupportedCodec { tag: 2, .. })
    ));
}

#[test]
fn vfs_collision_and_missing_member_do_not_choose_a_source() {
    let fixture = Fixture::new(&wave(1, 44_100, 8, &[128]));
    let (generation, jobs) = fixture.jobs();
    let mut ambiguous = MountIndex::default();
    for container in ["base.bsa", "patch.bsa"] {
        ambiguous
            .insert(AssetSource {
                container: container.into(),
                entry_index: 0,
                original_path: b"sound\\tone.wav".to_vec(),
            })
            .unwrap();
    }
    assert!(matches!(
        request_source_sound(
            &jobs,
            &generation,
            &fixture.archive,
            &ambiguous,
            b"sound/tone.wav",
            AudioLimits::default(),
        ),
        Err(AudioError::Data(fallout_data::Error::Unsupported(_)))
    ));
    assert!(matches!(
        request_source_sound(
            &jobs,
            &generation,
            &fixture.archive,
            &fixture.mounts,
            b"sound/missing.wav",
            AudioLimits::default(),
        ),
        Err(AudioError::MissingSource(_))
    ));
}
