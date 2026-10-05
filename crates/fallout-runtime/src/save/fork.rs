//! Save As selects one existing source-bound load; it never repairs its source.
//! Creation/publication failures can leave destination artifacts for diagnosis.
use super::{
    Captured, Error, LoadReceipt, Recovery, Repository, SlotRejection, Stage, WriteReceipt,
};
use crate::{Limits, SourceCatalogue, identity::CampaignId};
use serde::Serialize;
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct ForkSelection {
    campaign: CampaignId,
    cohort: String,
    raw_stat_sha256: String,
}
impl ForkSelection {
    pub fn new(campaign: CampaignId, cohort: &str, raw_stat_sha256: &str) -> super::Result<Self> {
        CampaignId::from_bytes(campaign.bytes())?;
        for digest in [cohort, raw_stat_sha256] {
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Error::Format(
                    "fork selection requires canonical SHA-256 identities".into(),
                ));
            }
        }
        Ok(Self {
            campaign,
            cohort: cohort.into(),
            raw_stat_sha256: raw_stat_sha256.into(),
        })
    }
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.cohort
    }
    pub fn snapshot_sha256(&self) -> &str {
        &self.raw_stat_sha256
    }
}
#[derive(Debug, Clone, Copy)]
pub struct ForkLimits {
    pub max_protected_paths: usize,
    /// Encoded OsStr bytes, including the resolved destination, rather than
    /// Unicode character counts. Filesystem internals/allocator overhead excluded.
    pub max_path_bytes: usize,
    pub max_total_path_bytes: usize,
}
impl Default for ForkLimits {
    fn default() -> Self {
        Self {
            max_protected_paths: 32,
            max_path_bytes: 4096,
            max_total_path_bytes: 65536,
        }
    }
}
pub struct ForkRequest<'a> {
    pub destination: &'a Path,
    pub protected: &'a [PathBuf],
    pub recovery: Recovery,
    pub expected: &'a ForkSelection,
    pub world_limits: Limits,
    pub limits: ForkLimits,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkStage {
    Admission,
    SourceLoad,
    Selection,
    Capture,
    DestinationCreation,
    Publication,
}
#[derive(Debug, Serialize)]
pub struct ForkFailure {
    stage: ForkStage,
    reason: SlotRejection,
}
impl ForkFailure {
    pub fn stage(&self) -> ForkStage {
        self.stage
    }
    pub fn reason(&self) -> &SlotRejection {
        &self.reason
    }
    fn new(stage: ForkStage, error: Error) -> Self {
        Self {
            stage,
            reason: SlotRejection::from_error(error),
        }
    }
}
impl fmt::Display for ForkFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "native boundary fork {:?}: {}",
            self.stage,
            self.reason.message()
        )
    }
}
impl std::error::Error for ForkFailure {}
#[derive(Debug, Serialize)]
pub struct ForkReceipt {
    source: LoadReceipt,
    current_failure_truncated: bool,
    destination: WriteReceipt,
}
impl ForkReceipt {
    pub fn source(&self) -> &LoadReceipt {
        &self.source
    }
    pub fn current_failure_truncated(&self) -> bool {
        self.current_failure_truncated
    }
    pub fn destination(&self) -> &WriteReceipt {
        &self.destination
    }
}
fn admit_path(path: &Path, used: &mut usize, limits: ForkLimits) -> super::Result<()> {
    let bytes = path.as_os_str().as_encoded_bytes().len();
    if bytes > limits.max_path_bytes {
        return Err(crate::Error::Capacity("fork path bytes").into());
    }
    *used = used
        .checked_add(bytes)
        .ok_or(crate::Error::Capacity("fork total path bytes"))?;
    if *used > limits.max_total_path_bytes {
        return Err(crate::Error::Capacity("fork total path bytes").into());
    }
    Ok(())
}
impl Repository {
    pub fn fork_boundary<'a>(
        &self,
        catalogue: impl Into<SourceCatalogue<'a>>,
        request: ForkRequest<'_>,
    ) -> Result<ForkReceipt, ForkFailure> {
        self.fork_boundary_observing(catalogue, request, |_| {})
    }
    pub fn fork_boundary_observing<'a>(
        &self,
        catalogue: impl Into<SourceCatalogue<'a>>,
        request: ForkRequest<'_>,
        observe: impl FnMut(Stage),
    ) -> Result<ForkReceipt, ForkFailure> {
        let failure = |stage, error| ForkFailure::new(stage, error);
        if request.protected.len() > request.limits.max_protected_paths {
            return Err(failure(
                ForkStage::Admission,
                crate::Error::Capacity("fork protected paths").into(),
            ));
        }
        let mut path_bytes = 0;
        for path in std::iter::once(self.path())
            .chain(std::iter::once(request.destination))
            .chain(request.protected.iter().map(PathBuf::as_path))
        {
            admit_path(path, &mut path_bytes, request.limits)
                .map_err(|e| failure(ForkStage::Admission, e))?;
        }
        // The source itself is a protected input for this operation. Creating
        // a child inside it would change its directory entries even before save.
        let mut protected = request.protected.to_vec();
        protected.push(self.path().into());
        let destination = super::repository::new_root(request.destination, &protected)
            .map_err(|e| failure(ForkStage::Admission, e))?;
        admit_path(&destination, &mut path_bytes, request.limits)
            .map_err(|e| failure(ForkStage::Admission, e))?;
        match fs::symlink_metadata(&destination) {
            Ok(_) => {
                return Err(failure(
                    ForkStage::Admission,
                    Error::Format("fork destination already exists".into()),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(failure(ForkStage::Admission, super::io(&destination, e))),
        }
        let (world, mut source) = self
            .load(catalogue, request.world_limits, request.recovery)
            .map_err(|e| failure(ForkStage::SourceLoad, e))?;
        // This raw STAT identity belongs to this restored World and this load.
        // A second read or canonical reencoding could identify different bytes.
        if source.metadata.campaign != request.expected.campaign
            || source.metadata.catalogue_sha256 != request.expected.cohort
            || source.metadata.snapshot_sha256 != request.expected.raw_stat_sha256
            || world.campaign() != request.expected.campaign
            || world.catalogue_fingerprint() != request.expected.cohort
        {
            return Err(failure(
                ForkStage::Selection,
                Error::Format("selected native boundary differs from fork expectation".into()),
            ));
        }
        let capture = Captured::at_boundary(&world);
        capture
            .validate(capture.snapshot())
            .map_err(|e| failure(ForkStage::Capture, e.into()))?;
        drop(world);
        // Recovery keeps the ordinary load behavior. Bound only the retained
        // fork report; load's existing diagnostic work is not a realtime promise.
        let mut current_failure_truncated = false;
        if let Some(message) = &mut source.current_failure
            && message.len() > 1024
        {
            let mut end = 1024;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            current_failure_truncated = true;
        }
        // Recheck absence through create_new directory semantics. A concurrent
        // destination creator wins cleanly; source publication is never changed.
        let repository = Repository::create(&destination, &protected, capture.snapshot().campaign)
            .map_err(|e| failure(ForkStage::DestinationCreation, e))?;
        let destination = repository
            .commit_observing(&capture, observe)
            .map_err(|e| failure(ForkStage::Publication, e))?;
        Ok(ForkReceipt {
            source,
            current_failure_truncated,
            destination,
        })
    }
}
