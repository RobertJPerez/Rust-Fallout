//! Skyrim SE/AE content preparation atop the existing shared Rust data layer.
//! Decoding source data does not establish runtime or campaign compatibility.
pub mod archive;
pub(crate) mod asset_lookup;
pub mod bindings;
pub mod census;
pub mod generic_models;
pub mod landscape_links;
pub mod movement_profile;
pub mod nif_header;
pub mod nif_index;
pub mod pex_header;
pub mod placed_uses;
pub mod plugin;
pub mod profile;
pub mod static_asset_trace;
pub mod static_models;
pub mod texture_sets;
pub mod trace;
pub mod vmad;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Shared(#[from] fallout_data::Error),
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
