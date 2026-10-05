//! Bounded, source-qualified sound preparation. This module validates a narrow
//! PCM WAVE subset; it does not resolve dialogue cues or provide a device backend.

mod source;
mod stream;
mod wave;

pub use source::{
    AudioError, AudioLimits, AudioSource, PendingSound, PreparedSound, inspect_source_bytes,
    request_source_sound,
};
pub use stream::{PcmChunk, PcmStream};
pub use wave::{WaveError, WaveFormat, WaveInfo, inspect_wave};
