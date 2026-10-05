use super::*;
use bevy::audio::Source;
use fallout_data::{
    audio::{PcmChunk, WaveFormat},
    resource_jobs::Generation,
};
use std::sync::{Arc, mpsc};

fn playback_control() -> Arc<PlaybackControl> {
    let generation = Generation::new("a".repeat(64)).unwrap();
    Arc::new(PlaybackControl::new(1, 7, generation.token().unwrap()))
}

fn mono_format() -> WaveFormat {
    WaveFormat {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        block_align: 2,
        bytes_per_second: 96_000,
    }
}

fn pcm_chunk(first_frame: u64, samples: &[f32]) -> PcmChunk {
    PcmChunk {
        first_frame,
        frame_count: samples.len(),
        interleaved_samples: samples.to_vec(),
    }
}

#[test]
fn custom_bevy_source_preserves_source_samples_rate_and_duration() {
    let (sender, receiver) = mpsc::sync_channel(2);
    sender.send(pcm_chunk(0, &[0.25, -0.5])).unwrap();
    drop(sender);
    let control = playback_control();
    let source = device::DevicePcmSource::new(mono_format(), 2, receiver, control);
    let mut decoder = source.decoder();

    assert_eq!(decoder.channels().get(), 1);
    assert_eq!(decoder.sample_rate().get(), 48_000);
    assert_eq!(decoder.total_duration(), Some(Duration::from_nanos(41_666)));
    assert_eq!(decoder.by_ref().collect::<Vec<_>>(), [0.25, -0.5]);
}

#[test]
fn custom_bevy_source_refuses_gaps_and_underflow_as_errors() {
    let (sender, receiver) = mpsc::sync_channel(2);
    sender.send(pcm_chunk(1, &[0.25])).unwrap();
    drop(sender);
    let control = playback_control();
    let source = device::DevicePcmSource::new(mono_format(), 2, receiver, control.clone());
    let mut decoder = source.decoder();
    assert_eq!(decoder.next(), None);
    assert!(matches!(
        control.device_outcome(),
        Some(Err(PlaybackFailure::Decode(_)))
    ));

    let (sender, receiver) = mpsc::sync_channel(2);
    sender.send(pcm_chunk(0, &[0.25])).unwrap();
    let control = playback_control();
    let source = device::DevicePcmSource::new(mono_format(), 2, receiver, control.clone());
    let mut decoder = source.decoder();
    assert_eq!(decoder.next(), Some(0.25));
    assert_eq!(decoder.next(), None);
    assert!(matches!(
        control.device_outcome(),
        Some(Err(PlaybackFailure::Sink(_)))
    ));
    drop(sender);
}

#[test]
fn playback_clock_comes_from_observed_backend_position_and_refuses_stale_scene() {
    let control = playback_control();
    control.update_device_position(Duration::from_millis(875));
    let handle = PlaybackHandle { control };

    assert_eq!(
        handle.playback_clock(7).unwrap(),
        Some(Duration::from_millis(875))
    );
    assert!(matches!(
        handle.playback_clock(8),
        Err(PlaybackFailure::StaleScene {
            request_generation: 7,
            current_generation: 8,
        })
    ));
}
