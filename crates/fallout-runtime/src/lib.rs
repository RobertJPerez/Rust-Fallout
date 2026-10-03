//! Canonical runtime state. Presentation and transient storage handles stay out
//! of snapshots; immutable script definitions remain in `fallout-data`.
//! This crate stores explicit host inputs, not guessed retail initialization or
//! an implementation of the original game's scheduler.
pub mod events;
pub mod identity;
pub mod schema;
pub mod snapshot;
pub mod state;

pub use state::{Limits, World};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runtime state budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("runtime state is invalid: {0}")]
    Invalid(String),
    #[error("script definition is missing or has changed")]
    DefinitionChanged,
    #[error("runtime handle is stale or belongs to another world")]
    StaleHandle,
    #[error("script instance is missing")]
    MissingInstance,
    #[error("local variable {0} is not declared")]
    MissingLocal(u32),
    #[error("local variable {0} has no captured or explicitly assigned value")]
    UninitializedLocal(u32),
    #[error("local variable {0} has a different storage kind")]
    IncompatibleLocal(u32),
    #[error("local variable {0} has an unverified declaration type")]
    UnsupportedLocal(u32),
    #[error("live reference identity is missing")]
    MissingReference,
    #[error("reference resolution needs an explicit binding: {0}")]
    UnresolvedDependency(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
