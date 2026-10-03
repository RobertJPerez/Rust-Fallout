//! Read-only content tools. Decoding a file does not imply gameplay support.

#[cfg(not(target_pointer_width = "64"))]
compile_error!("The initial runtime targets 64-bit processes.");

pub mod archive;
pub mod assets;
pub mod baseline;
pub mod cache;
pub mod content;
pub mod coordinates;
pub mod identity;
pub mod index_cache;
pub mod model_probe;
pub mod nif;
pub mod nif_census;
pub mod nif_collision;
pub mod nif_scene;
pub mod parity;
pub mod planning;
pub mod plugin;
pub mod script_inventory;
pub mod store;
pub mod terrain;
pub mod texture_probe;
pub mod vfs;
pub mod world;

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{source_name} at 0x{offset:X}: {reason}")]
    Format {
        source_name: String,
        offset: u64,
        reason: String,
    },
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("content resolution: {0}")]
    Resolution(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

pub(crate) fn malformed(name: &str, offset: u64, reason: impl Into<String>) -> Error {
    Error::Format {
        source_name: name.into(),
        offset,
        reason: reason.into(),
    }
}
