//! Scene lifetime admission over the existing source residency owner.
//! CPU submission, GPU preparation, render publication and simulation readiness
//! remain separate. This adapter owns no canonical World or save repository.
use fallout_data::{
    resource_jobs::{JobError, JobResult},
    world::residency::{self, CellResidency, Readiness, ResidentSources, ResidentTextures, Ticket},
};
use serde::Serialize;
use std::sync::Arc;

mod reference;
pub use reference::ReferenceAdmission;
mod continuation;
pub use continuation::{ContinueDisplay, DisplayStamp};
pub mod upload;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Phase {
    Prepared,
    Submitted,
    Uploaded,
    Retiring,
    Released,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub scene_epoch: u64,
    pub phase: Phase,
    pub simulation_ready: bool,
    pub source: residency::Snapshot,
}

/// Source leases retained until the draw consumer finishes bounded disposal.
pub struct SceneLifetime {
    owner: CellResidency,
    ticket: Ticket,
    models: Option<Arc<ResidentSources>>,
    textures: Option<Arc<ResidentTextures>>,
    scene_epoch: u64,
    phase: Phase,
}

impl SceneLifetime {
    /// Capture through validated owner APIs. Matching numeric generations and
    /// source hashes cannot admit a ticket from a different residency owner.
    pub fn prepared(owner: CellResidency, ticket: Ticket, scene_epoch: u64) -> JobResult<Self> {
        let models = owner.sources(&ticket)?;
        let textures = owner.texture_sources(&ticket)?;
        let source = owner.snapshot();
        if source.completed_models == 0
            || !source.complete_texture_coverage
            || source.dependencies != Readiness::Ready
            || source.render_published
        {
            return Err(JobError::Invalid(
                "prepared scene needs nonempty models, complete textures and unpublished dependencies".into(),
            ));
        }
        Ok(Self {
            owner,
            ticket,
            models: Some(models),
            textures: Some(textures),
            scene_epoch,
            phase: Phase::Prepared,
        })
    }

    pub fn ticket(&self) -> &Ticket {
        &self.ticket
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn sources(&self, scene_epoch: u64) -> JobResult<&ResidentSources> {
        self.validate(scene_epoch)?;
        self.models.as_deref().ok_or(JobError::Closed)
    }

    pub fn textures(&self, scene_epoch: u64) -> JobResult<&ResidentTextures> {
        self.validate(scene_epoch)?;
        self.textures.as_deref().ok_or(JobError::Closed)
    }

    fn validate(&self, scene_epoch: u64) -> JobResult<()> {
        if scene_epoch != self.scene_epoch {
            return Err(JobError::Stale);
        }
        if matches!(self.phase, Phase::Retiring | Phase::Released) {
            return Err(JobError::Closed);
        }
        self.ticket.check()
    }

    pub fn submission_complete(&mut self, scene_epoch: u64) -> JobResult<()> {
        self.validate(scene_epoch)?;
        if self.phase != Phase::Prepared {
            return Err(JobError::Invalid(
                "scene submission already completed".into(),
            ));
        }
        self.phase = Phase::Submitted;
        Ok(())
    }

    /// The consumer must inspect actual GPU resources in `uploaded`; neither ECS
    /// spawn nor elapsed frames proves upload completion. False keeps the scene
    /// hidden. Publication runs once through the existing cancellation gate.
    /// Failed callbacks preserve admission state; they must not have partial
    /// external effects. This does not itself prove a captured visible frame.
    pub fn publish_uploaded<T>(
        &mut self,
        scene_epoch: u64,
        uploaded: impl FnOnce() -> JobResult<bool>,
        publish: impl FnOnce() -> JobResult<T>,
    ) -> JobResult<Option<T>> {
        self.validate(scene_epoch)?;
        if self.phase != Phase::Submitted {
            return Err(JobError::Invalid(
                "scene publication needs complete submission".into(),
            ));
        }
        if !uploaded()? {
            return Ok(None);
        }
        let value = self.owner.publish_render(&self.ticket, publish)?;
        self.phase = Phase::Uploaded;
        Ok(Some(value))
    }

    /// Physics reports actual query readiness; there is no visual-bounds fallback.
    pub fn report_collision(&mut self, scene_epoch: u64, readiness: Readiness) -> JobResult<()> {
        self.validate(scene_epoch)?;
        self.owner.report_collision(&self.ticket, readiness)
    }

    /// The canonical application reports admitted behavior readiness.
    pub fn report_behavior(&mut self, scene_epoch: u64, readiness: Readiness) -> JobResult<()> {
        self.validate(scene_epoch)?;
        self.owner.report_behavior(&self.ticket, readiness)
    }

    pub fn snapshot(&self) -> Snapshot {
        let source = self.owner.snapshot();
        Snapshot {
            scene_epoch: self.scene_epoch,
            phase: self.phase,
            simulation_ready: self.phase == Phase::Uploaded
                && self.ticket.check().is_ok()
                && source.render_published
                && source.simulation_ready,
            source,
        }
    }

    /// Revoke publication immediately, retaining resource-bearing leases while
    /// the consumer retires submitted GPU resources and unsubmitted CPU work.
    pub fn begin_retirement(&mut self, scene_epoch: u64) -> JobResult<()> {
        if scene_epoch != self.scene_epoch {
            return Err(JobError::Stale);
        }
        if matches!(self.phase, Phase::Retiring | Phase::Released) {
            return Ok(());
        }
        self.owner.unload()?;
        self.phase = Phase::Retiring;
        Ok(())
    }

    /// Call after completed draw disposal. External leases stay charged and can
    /// prevent Released; this acknowledgement never resets residency counters.
    pub fn draw_disposal_complete(&mut self, scene_epoch: u64) -> JobResult<Snapshot> {
        self.check_retirement(scene_epoch)?;
        self.textures.take();
        self.models.take();
        self.poll_retirement(scene_epoch)
    }

    fn check_retirement(&self, scene_epoch: u64) -> JobResult<()> {
        if scene_epoch != self.scene_epoch {
            return Err(JobError::Stale);
        }
        if !matches!(self.phase, Phase::Retiring | Phase::Released) {
            return Err(JobError::Invalid("scene is not retiring".into()));
        }
        Ok(())
    }

    /// Nonblocking drain of this owner's existing jobs and retained pins.
    pub fn poll_retirement(&mut self, scene_epoch: u64) -> JobResult<Snapshot> {
        self.check_retirement(scene_epoch)?;
        let source = self.owner.poll()?;
        if self.models.is_none()
            && self.textures.is_none()
            && source.stage == residency::Stage::Unrequested
            && source.outstanding == 0
            && source.pinned_source_bytes == 0
            && source.retained_plans == 0
            && source.plan_metadata_bytes == 0
            && source.mapped_source_bytes == 0
        {
            self.phase = Phase::Released;
        }
        Ok(Snapshot {
            scene_epoch: self.scene_epoch,
            phase: self.phase,
            simulation_ready: false,
            source,
        })
    }
}
