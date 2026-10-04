use super::{AudioError, PreparedSound, WaveError, source::MAX_PCM_CHUNK_FRAMES};
use crate::resource_jobs::JobToken;

/// Interleaved normalized samples for one bounded run of source PCM frames.
/// Channel count and sample rate remain those of the source; no mixing or
/// resampling is performed here.
#[derive(Debug, PartialEq)]
pub struct PcmChunk {
    pub first_frame: u64,
    pub frame_count: usize,
    pub interleaved_samples: Vec<f32>,
}

pub struct PcmStream<'a> {
    sound: &'a PreparedSound,
    token: JobToken,
    next_frame: u64,
    chunk_frames: usize,
}

impl<'a> PcmStream<'a> {
    pub(super) fn new(sound: &'a PreparedSound, token: JobToken, chunk_frames: usize) -> Self {
        debug_assert!(chunk_frames > 0 && chunk_frames <= MAX_PCM_CHUNK_FRAMES);
        Self {
            sound,
            token,
            next_frame: 0,
            chunk_frames,
        }
    }

    /// Decode at most the configured number of frames. Cancellation/staleness is
    /// checked before work, every 256 frames, and before the chunk is returned.
    pub fn next_chunk(&mut self) -> Result<Option<PcmChunk>, AudioError> {
        self.token.check()?;
        if self.next_frame >= self.sound.frame_count() {
            return Ok(None);
        }
        let remaining = self.sound.frame_count() - self.next_frame;
        let frame_count = remaining.min(self.chunk_frames as u64) as usize;
        let channels = usize::from(self.sound.info_format().channels);
        let sample_count = frame_count
            .checked_mul(channels)
            .ok_or_else(|| AudioError::Allocation("sample count overflow".into()))?;
        let bytes_per_sample = usize::from(self.sound.info_format().bits_per_sample / 8);
        let first_byte = usize::try_from(self.next_frame)
            .ok()
            .and_then(|frame| frame.checked_mul(usize::from(self.sound.info_format().block_align)))
            .and_then(|offset| self.sound.info_offset().checked_add(offset))
            .ok_or_else(|| AudioError::Allocation("source frame offset overflow".into()))?;
        let chunk_bytes = frame_count
            .checked_mul(usize::from(self.sound.info_format().block_align))
            .ok_or_else(|| AudioError::Allocation("source chunk size overflow".into()))?;
        let source = self
            .sound
            .bytes()
            .get(
                first_byte
                    ..first_byte.checked_add(chunk_bytes).ok_or_else(|| {
                        AudioError::Allocation("source chunk end overflow".into())
                    })?,
            )
            .ok_or_else(|| AudioError::Allocation("validated source range disappeared".into()))?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(sample_count)
            .map_err(|error| AudioError::Allocation(error.to_string()))?;
        for frame in 0..frame_count {
            if frame % 256 == 0 {
                self.token.check()?;
            }
            for channel in 0..channels {
                let sample_index = frame * channels + channel;
                let start = sample_index * bytes_per_sample;
                let sample = match bytes_per_sample {
                    1 => (f32::from(source[start]) - 128.0) / 128.0,
                    2 => {
                        let raw = i16::from_le_bytes([source[start], source[start + 1]]);
                        f32::from(raw) / 32768.0
                    }
                    bits => {
                        return Err(AudioError::Wave(WaveError::UnsupportedSampleDepth(
                            (bits * 8) as u16,
                        )));
                    }
                };
                samples.push(sample);
            }
        }
        self.token.check()?;
        let first_frame = self.next_frame;
        self.next_frame += frame_count as u64;
        Ok(Some(PcmChunk {
            first_frame,
            frame_count,
            interleaved_samples: samples,
        }))
    }

    /// Cancel only this stream's generation token. It does not drop the source
    /// lease; the owner releases that by dropping `PreparedSound` after drain.
    pub fn cancel(&self) {
        self.token.cancel();
    }
}
