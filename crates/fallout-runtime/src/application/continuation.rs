//! A native candidate stays separate from the active world until the complete
//! scene is prepared. The scene owner is borrowed across admission/publication.
use super::{Failure, Host, HostIdentity, NEXT_HOST, Result};
use crate::{
    World,
    save::{LoadReceipt, RequestIdentity, RestoredCandidate},
};
use std::{num::NonZeroU64, sync::atomic::Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueRequest {
    host: u64,
    scene: NonZeroU64,
    revision: u64,
    identity: RequestIdentity,
}
impl ContinueRequest {
    pub fn identity(&self) -> &RequestIdentity {
        &self.identity
    }
    pub fn prior_revision(&self) -> u64 {
        self.revision
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
}

#[must_use = "preparation has no active-world effects; publish with complete scene admission or drop"]
pub struct PreparedContinue {
    request: ContinueRequest,
    world: World<'static>,
    native: LoadReceipt,
}
impl PreparedContinue {
    pub fn world(&self) -> &World<'static> {
        &self.world
    }
    pub fn native_receipt(&self) -> &LoadReceipt {
        &self.native
    }
    pub fn request(&self) -> &ContinueRequest {
        &self.request
    }
}

/// These identities come from the active request and the admitted native world.
/// A scene producer must validate its current generation/revision and every
/// required member/resource, not merely the number of matching references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueBoundary {
    request: RequestIdentity,
    scene: NonZeroU64,
    prior_revision: u64,
    candidate_revision: u64,
    prior_host: HostIdentity,
    candidate_host: HostIdentity,
}
impl ContinueBoundary {
    pub fn request(&self) -> &RequestIdentity {
        &self.request
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
    pub fn prior_revision(&self) -> u64 {
        self.prior_revision
    }
    pub fn candidate_revision(&self) -> u64 {
        self.candidate_revision
    }
    pub fn prior_host_identity(&self) -> HostIdentity {
        self.prior_host
    }
    pub fn candidate_host_identity(&self) -> HostIdentity {
        self.candidate_host
    }
}

/// The scene's stage owns all prepared observations. prepare must leave its
/// active display unchanged. publish consumes that exact stage without lookup,
/// allocation/admission checks, async work, or another fallible membership test.
/// Actual scene/GPU readiness belongs to its producer, not to a boolean supplied
/// by the application. This synchronous borrow prevents ordinary scene mutation
/// between preparation and publication.
pub trait ScenePublisher {
    type Stage;
    fn prepare(&self, candidate: &World<'_>, boundary: &ContinueBoundary) -> Result<Self::Stage>;
    fn publish(&mut self, stage: Self::Stage, boundary: &ContinueBoundary);
}

pub struct ContinueReceipt {
    pub boundary: ContinueBoundary,
    pub native: LoadReceipt,
}

impl Host<'static> {
    /// The caller starts/owns the existing RestoreTask with this identity and
    /// keeps it alive until candidate admission. Supersession changes only the
    /// application's transient request; it never replaces canonical state.
    pub fn begin_continue(&mut self, request_id: NonZeroU64) -> Result<ContinueRequest> {
        if request_id.get() <= self.last_continue_request {
            return Err(Failure::Refused("Continue request identity must advance"));
        }
        let request = ContinueRequest {
            host: self.epoch,
            scene: self.scene,
            revision: self.world.revision(),
            identity: RequestIdentity::new(
                request_id,
                self.world.campaign(),
                self.world.catalogue_fingerprint(),
            )?,
        };
        self.last_continue_request = request_id.get();
        self.pending_continue = Some(request.clone());
        Ok(request)
    }
    pub fn cancel_continue(&mut self, request: &ContinueRequest) -> bool {
        if self.pending_continue.as_ref() != Some(request) {
            return false;
        }
        self.pending_continue = None;
        true
    }
    fn check_continue(&self, request: &ContinueRequest) -> Result<()> {
        if request.host != self.epoch || request.scene != self.scene {
            return Err(Failure::ExpiredSelection);
        }
        if self.pending_continue.as_ref() != Some(request) {
            return Err(Failure::Refused(
                "Continue request is cancelled or superseded",
            ));
        }
        if request.revision != self.world.revision() {
            return Err(Failure::RevisionChanged);
        }
        if request.identity.campaign() != self.world.campaign()
            || request.identity.catalogue_fingerprint() != self.world.catalogue_fingerprint()
        {
            return Err(Failure::Refused(
                "Continue request canonical identity changed",
            ));
        }
        Ok(())
    }
    pub fn prepare_continue(
        &self,
        request: ContinueRequest,
        candidate: RestoredCandidate,
    ) -> Result<PreparedContinue> {
        self.check_continue(&request)?;
        // Accepting the runtime job only releases its opaque result. This World
        // remains private and has no authority over the active host or display.
        let (world, native) = candidate.take_for(request.identity())?;
        self.content.validate_world(&world)?;
        if world.campaign() != self.world.campaign() {
            return Err(Failure::Refused("Continue candidate campaign changed"));
        }
        Ok(PreparedContinue {
            request,
            world,
            native,
        })
    }
    pub fn publish_continue<S: ScenePublisher>(
        &mut self,
        prepared: PreparedContinue,
        scene: &mut S,
    ) -> Result<ContinueReceipt> {
        self.check_continue(&prepared.request)?;
        self.content.validate_world(&prepared.world)?;
        let epoch = NEXT_HOST
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Failure::Capacity("host identities"))?;
        let boundary = ContinueBoundary {
            request: prepared.request.identity,
            scene: prepared.request.scene,
            prior_revision: prepared.request.revision,
            candidate_revision: prepared.world.revision(),
            prior_host: self.identity(),
            candidate_host: HostIdentity(epoch),
        };
        // This is the last fallible operation. The borrowed scene validates its
        // complete destination before either authority changes. A refusal drops
        // only the private candidate and preserves the world and visible revision.
        let stage = scene.prepare(&prepared.world, &boundary)?;
        self.world = prepared.world;
        self.epoch = epoch;
        self.pending_continue = None;
        self.accepted.clear();
        self.retained_bytes = 0;
        scene.publish(stage, &boundary);
        Ok(ContinueReceipt {
            boundary,
            native: prepared.native,
        })
    }
}
