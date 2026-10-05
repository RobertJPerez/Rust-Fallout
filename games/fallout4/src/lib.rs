//! Fallout 4 content preparation. Source decoding does not execute gameplay.
pub mod archive;
pub mod attachments;
pub mod census;
pub mod condition;
pub mod executable;
pub mod formid;
pub mod link;
pub mod material;
pub mod materials;
pub mod pex;
pub mod pex_evidence;
pub mod profile;
pub mod vmad;
pub mod workshop;

pub use fallout_data::identity::{FormKey, ProfileId};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Shared(#[from] fallout_data::Error),
    #[error("{0}")]
    Archive(#[from] dream_archive::ba2::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{source_name} at byte 0x{offset:X}: {reason}")]
    Format {
        source_name: String,
        offset: usize,
        reason: String,
    },
    #[error("unsupported capability: {0}")]
    Unsupported(String),
}
pub type Result<T> = std::result::Result<T, Error>;
pub(crate) fn bad(name: &str, offset: usize, reason: impl Into<String>) -> Error {
    Error::Format {
        source_name: name.into(),
        offset,
        reason: reason.into(),
    }
}
