//! Engine-native saves. Capture canonical state at a host boundary, then hand
//! the owned request to a worker. Container integrity and filesystem publication
//! are separate from original `.fos` compatibility and power-loss guarantees.
pub mod format;
mod repository;
mod restore;
mod source_validation;
mod status;
mod worker;

#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod test_source;

use crate::{Limits, World, snapshot::Snapshot};
pub use repository::{
    LoadReceipt, Recovery, Repository, Slot, SlotAvailability, SlotAvailabilityReport,
    SlotRejection, SlotRejectionCode, Stage, WriteReceipt,
};
pub use restore::{
    RequestIdentity, RestoreAdmission, RestoreError, RestorePoll, RestoreTask, RestoredCandidate,
};
pub use status::{SaveState, SaveStatus};
pub use worker::{CompletionError, Rejection, SaveTicket, SaveWorker, SubmitFailure, WorkerError};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native save format: {0}")]
    Format(String),
    #[error("native save repository is busy")]
    Busy,
    #[error("native save repository has no current slot")]
    MissingCurrent,
    #[error("native save filesystem error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    State(#[from] crate::Error),
}
fn io(path: &std::path::Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

/// The fields are private so publication cannot receive a partially edited
/// snapshot. The request owns its data and can outlive the capture boundary.
#[derive(Debug, Clone)]
pub struct Captured {
    pub(crate) snapshot: Snapshot,
    pub(crate) limits: Limits,
    source_validation: source_validation::SourceValidation,
}
impl Captured {
    pub fn at_boundary(world: &World<'_>) -> Self {
        Self {
            snapshot: world.snapshot(),
            limits: world.limits,
            source_validation: source_validation::SourceValidation::capture(world),
        }
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
}
