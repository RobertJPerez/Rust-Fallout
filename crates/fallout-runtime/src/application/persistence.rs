//! Request admission and acknowledgements over the existing native writer.
//! The worker owns publication; a pending or joined ticket is not a saved result.
use super::{Failure, Host, HostIdentity, Result};
use crate::{
    identity::CampaignId,
    save::{Captured, CompletionError, SaveState, SaveStatus, SaveWorker, WriteReceipt},
};
use std::num::NonZeroU64;

/// A selection without capture or disk effects. It is consumed by submission;
/// accepted IDs advance, including when the writer later reports a failure.
#[derive(Debug)]
pub struct SaveRequest {
    request: NonZeroU64,
    host: HostIdentity,
    scene: NonZeroU64,
    revision: u64,
    campaign: CampaignId,
    catalogue: String,
}
impl SaveRequest {
    pub fn request_id(&self) -> NonZeroU64 {
        self.request
    }
    pub fn host_identity(&self) -> HostIdentity {
        self.host
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue
    }
}

#[derive(Debug)]
#[must_use = "observe this request's actual terminal save status"]
pub struct SaveSubmission {
    boundary: SaveRequest,
    status: SaveStatus,
}
impl SaveSubmission {
    pub fn boundary(&self) -> &SaveRequest {
        &self.boundary
    }
    pub fn state(&self) -> &SaveState {
        self.status.state()
    }
    pub fn poll(&mut self) -> &SaveState {
        self.status.poll()
    }
    /// Identity admission alone says nothing about whether storage succeeded.
    /// A scene must inspect Published before acknowledging the saved revision.
    pub fn belongs_to_host(&self, host: &Host<'_>) -> bool {
        self.boundary.host == host.identity()
            && self.boundary.scene == host.scene_generation()
            && self.boundary.campaign == host.world().campaign()
            && self.boundary.catalogue == host.world().catalogue_fingerprint()
    }
    /// A completed older capture may belong to this host while a newer tick has
    /// advanced the world. It cannot acknowledge that newer canonical revision.
    pub fn matches_current_boundary(&self, host: &Host<'_>) -> bool {
        self.belongs_to_host(host) && self.boundary.revision == host.world().revision()
    }
    /// Blocking collection is only for shutdown/offline consumers, outside the
    /// frame. This delegates to the existing ticket; it does not write again.
    pub fn wait(self) -> std::result::Result<WriteReceipt, CompletionError> {
        self.status.wait()
    }
}

impl Host<'_> {
    pub fn select_save(&self, request: NonZeroU64) -> Result<SaveRequest> {
        if request.get() <= self.last_save_request {
            return Err(Failure::Refused("Save request identity must advance"));
        }
        Ok(SaveRequest {
            request,
            host: self.identity(),
            scene: self.scene,
            revision: self.world.revision(),
            campaign: self.world.campaign(),
            catalogue: self.world.catalogue_fingerprint().into(),
        })
    }
    /// Execute on the canonical owner outside the render/input frame: capture
    /// copies the bounded World snapshot. Admission uses the existing worker's
    /// queue/byte bounds. A refusal advances no ID and preserves the returned
    /// capture in the existing SubmitFailure; accepted storage is never cancelled
    /// by dropping its observer or by a later scene/host change.
    pub fn submit_save(
        &mut self,
        request: SaveRequest,
        worker: &mut SaveWorker,
    ) -> Result<SaveSubmission> {
        if request.host != self.identity() || request.scene != self.scene {
            return Err(Failure::ExpiredSelection);
        }
        if request.revision != self.world.revision() {
            return Err(Failure::RevisionChanged);
        }
        if request.campaign != self.world.campaign()
            || request.catalogue != self.world.catalogue_fingerprint()
        {
            return Err(Failure::Refused("Save request canonical identity changed"));
        }
        if request.request.get() <= self.last_save_request {
            return Err(Failure::Refused("Save request identity must advance"));
        }
        let ticket = worker.try_submit(Captured::at_boundary(&self.world))?;
        self.last_save_request = request.request.get();
        Ok(SaveSubmission {
            boundary: request,
            status: SaveStatus::new(ticket),
        })
    }
}
