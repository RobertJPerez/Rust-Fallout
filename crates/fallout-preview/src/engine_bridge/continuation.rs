//! Join the prepared canonical Continue with the existing renderer's owned stage.
use super::{Phase, SceneLifetime};
use fallout_data::identity::FormKey;
use fallout_runtime::{
    Error, World,
    application::{self, ContinueBoundary, Host, HostIdentity, ScenePublisher},
};
use std::marker::PhantomData;

/// Transient observation of the canonical result already shown by this scene.
/// Numeric revisions alone cannot distinguish two restores of the same save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayStamp {
    host: HostIdentity,
    scene_epoch: u64,
    revision: u64,
}
impl DisplayStamp {
    /// Capture after the initial scene's actual publication acknowledgement.
    pub fn from_host(host: &Host<'_>) -> Self {
        Self {
            host: host.identity(),
            scene_epoch: host.scene_generation().get(),
            revision: host.world().revision(),
        }
    }
    pub fn host_identity(&self) -> HostIdentity {
        self.host
    }
    pub fn scene_epoch(&self) -> u64 {
        self.scene_epoch
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// Retain the source borrow and exclusive display boundary across the entire
/// application publish_continue call. The renderer owns pose conversion, exact
/// entity/resource preparation, actual GPU checks and its bounded owned stage.
pub struct ContinueDisplay<'a, S, Prepare, Apply> {
    source: &'a SceneLifetime,
    display: &'a mut DisplayStamp,
    active: &'a [FormKey],
    prepare: Prepare,
    apply: Apply,
    stage: PhantomData<fn(S)>,
}
impl<'a, S, Prepare, Apply> ContinueDisplay<'a, S, Prepare, Apply>
where
    Prepare: Fn(&World<'_>, &ContinueBoundary) -> application::Result<(Vec<FormKey>, S)>,
    Apply: FnMut(S, &ContinueBoundary),
{
    pub fn new(
        source: &'a SceneLifetime,
        display: &'a mut DisplayStamp,
        active: &'a [FormKey],
        prepare: Prepare,
        apply: Apply,
    ) -> Self {
        Self {
            source,
            display,
            active,
            prepare,
            apply,
            stage: PhantomData,
        }
    }
}

/// Constructed only by this producer after every fallible preparation check.
/// The stage carries its exact canonical boundary through final publication.
pub struct PreparedDisplay<S> {
    stage: S,
    boundary: ContinueBoundary,
}

impl<S, Prepare, Apply> ScenePublisher for ContinueDisplay<'_, S, Prepare, Apply>
where
    Prepare: Fn(&World<'_>, &ContinueBoundary) -> application::Result<(Vec<FormKey>, S)>,
    Apply: FnMut(S, &ContinueBoundary),
{
    type Stage = PreparedDisplay<S>;

    fn prepare(
        &self,
        candidate: &World<'_>,
        boundary: &ContinueBoundary,
    ) -> application::Result<Self::Stage> {
        if self.display.host != boundary.prior_host_identity()
            || self.display.scene_epoch != boundary.scene_generation().get()
            || self.display.revision != boundary.prior_revision()
        {
            return Err(application::Failure::Refused(
                "display boundary changed before Continue",
            ));
        }
        if candidate.revision() != boundary.candidate_revision()
            || candidate.campaign() != boundary.request().campaign()
            || candidate.catalogue_fingerprint() != boundary.request().catalogue_fingerprint()
        {
            return Err(application::Failure::Refused(
                "projected candidate identity changed",
            ));
        }
        self.source.sources(self.display.scene_epoch).map_err(|e| {
            application::Failure::State(Error::Invalid(format!("scene source admission: {e}")))
        })?;
        if self.source.phase() != Phase::Uploaded {
            return Err(application::Failure::Refused(
                "Continue display needs an uploaded scene",
            ));
        }
        // This callback must prepare all real draws without changing the active
        // display. Unavailable canonical poses remain explicit hidden entries.
        let (observed, stage) = (self.prepare)(candidate, boundary)?;
        let _admission = self
            .source
            .admit_references(
                self.display.scene_epoch,
                boundary.prior_revision(),
                self.display.revision,
                self.active,
                &observed,
            )
            .map_err(|e| {
                application::Failure::State(Error::Invalid(format!(
                    "scene reference admission: {e}"
                )))
            })?;
        Ok(PreparedDisplay {
            stage,
            boundary: boundary.clone(),
        })
    }

    fn publish(&mut self, prepared: Self::Stage, _boundary: &ContinueBoundary) {
        // The canonical application has already replaced its World. Therefore
        // no ticket, revision, membership, lookup or allocation check can occur
        // here. Use the exact boundary and data admitted by prepare, once.
        (self.apply)(prepared.stage, &prepared.boundary);
        self.display.host = prepared.boundary.candidate_host_identity();
        self.display.scene_epoch = prepared.boundary.scene_generation().get();
        self.display.revision = prepared.boundary.candidate_revision();
    }
}
