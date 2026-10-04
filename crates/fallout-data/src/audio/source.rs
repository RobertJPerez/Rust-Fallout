use super::{PcmStream, WaveError, WaveFormat, WaveInfo, inspect_wave};
use crate::{
    Error,
    identity::ProfileId,
    resource_jobs::{
        ArchiveInput, Artifact, Generation, JobError, JobHandle, JobToken, ResourceJobs,
    },
    vfs::{AssetPath, AssetSource, MountIndex},
};
use std::sync::Arc;
use thiserror::Error as ThisError;

pub const MAX_PCM_CHUNK_FRAMES: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct AudioLimits {
    /// PCM frames emitted by one bounded pull from the stream.
    pub chunk_frames: usize,
}
impl Default for AudioLimits {
    fn default() -> Self {
        Self { chunk_frames: 1024 }
    }
}
impl AudioLimits {
    pub(crate) fn validate(self) -> Result<(), AudioError> {
        if self.chunk_frames == 0 || self.chunk_frames > MAX_PCM_CHUNK_FRAMES {
            return Err(AudioError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AudioSource {
    pub profile: ProfileId,
    pub path: AssetPath,
    pub original_path: Vec<u8>,
    pub container: String,
    pub entry_index: usize,
    pub archive_sha256: String,
}

#[derive(Debug, ThisError)]
pub enum AudioError {
    #[error(transparent)]
    Data(#[from] Error),
    #[error(transparent)]
    Resource(#[from] JobError),
    #[error(transparent)]
    Wave(#[from] WaveError),
    #[error("audio source not found in the mounted VFS: {0:?}")]
    MissingSource(Vec<u8>),
    #[error("invalid audio preparation limits")]
    InvalidLimits,
    #[error("PCM chunk allocation failed: {0}")]
    Allocation(String),
}

/// An admitted archive source request. The source is read on the bounded
/// ResourceJobs pool and is not exposed as prepared until all bounds validate.
pub struct PendingSound {
    job: JobHandle,
    source: AudioSource,
    limits: AudioLimits,
}

impl PendingSound {
    /// Wait for the resource worker; call off the render/update thread. On success
    /// the returned sound owns the Artifact, keeping its source and byte-budget
    /// lease alive until the sound and any borrowed stream are dropped.
    pub fn wait(self) -> Result<PreparedSound, AudioError> {
        let artifact = self.job.wait()?;
        let info = inspect_wave(artifact.bytes(), self.limits)?;
        Ok(PreparedSound {
            _lease: artifact,
            source: self.source,
            info,
            chunk_frames: self.limits.chunk_frames,
        })
    }
}

pub struct PreparedSound {
    // Retains the extracted source bytes, archive handle and ResourceJobs budget pin.
    _lease: Artifact,
    source: AudioSource,
    info: WaveInfo,
    chunk_frames: usize,
}

impl PreparedSound {
    pub fn source(&self) -> &AudioSource {
        &self.source
    }

    pub fn format(&self) -> WaveFormat {
        self.info.format
    }

    pub fn frame_count(&self) -> u64 {
        self.info.frame_count
    }

    pub(crate) fn info_format(&self) -> WaveFormat {
        self.info.format
    }

    pub(crate) fn info_offset(&self) -> usize {
        self.info.data_offset
    }

    /// Create a bounded PCM pull stream owned by the caller's playback
    /// generation. The caller keeps this sound alive and drains the stream from
    /// an audio worker; a frame loop should not decode chunks itself.
    pub fn stream(&self, token: JobToken) -> Result<PcmStream<'_>, AudioError> {
        token.check()?;
        Ok(PcmStream::new(self, token, self.chunk_frames))
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        self._lease.bytes()
    }
}

/// Resolve one unambiguous archive member through the existing MountIndex and
/// ResourceJobs admission path. Archive collisions remain a refusal until a
/// profile's lookup precedence is verified.
pub fn request_source_sound(
    jobs: &ResourceJobs,
    resource_generation: &Generation,
    archive: &Arc<ArchiveInput>,
    mounts: &MountIndex,
    raw_path: &[u8],
    limits: AudioLimits,
) -> Result<PendingSound, AudioError> {
    limits.validate()?;
    let path = AssetPath::new(raw_path)?;
    let asset_source = mounts
        .unique(path.bytes())?
        .ok_or_else(|| AudioError::MissingSource(raw_path.to_vec()))?;
    let member = archive.member(&path, asset_source)?;
    let token = resource_generation.token()?;
    let job = jobs.submit(member, token, None)?;
    Ok(PendingSound {
        job,
        source: AudioSource {
            profile: ProfileId::NvOriginal,
            path,
            original_path: asset_source.original_path.clone(),
            container: asset_source.container.clone(),
            entry_index: asset_source.entry_index,
            archive_sha256: archive.source_sha256().to_owned(),
        },
        limits,
    })
}

pub fn inspect_source_bytes(bytes: &[u8], limits: AudioLimits) -> Result<WaveInfo, WaveError> {
    inspect_wave(bytes, limits)
}
