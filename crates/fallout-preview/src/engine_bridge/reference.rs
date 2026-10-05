//! Full reference membership is checked before an infallible display commit.
use super::{Phase, SceneLifetime};
use fallout_data::{
    identity::FormKey,
    resource_jobs::{JobError, JobResult},
};
use std::collections::BTreeSet;

const MAX_BINDINGS: usize = 10_000;

/// A one-use, scoped admission over this residency owner and immutable key sets.
/// The borrow prevents retirement during publication. This is not an off-thread
/// restore token: the canonical application still owns request/campaign checks,
/// the prepared World and its commit. All fallible draw preparation precedes it.
#[must_use = "admission has no effects until publication"]
pub struct ReferenceAdmission<'a> {
    scene: &'a SceneLifetime,
    prior_revision: u64,
    active: &'a [FormKey],
    observed: &'a [FormKey],
}

impl SceneLifetime {
    /// Preflight a complete prepared observation against protected source members.
    /// Keys must be unique and sorted; unknown/hidden poses still have a key.
    /// This source graph work belongs outside the render/input frame. It neither
    /// evaluates enable parents nor fabricates missing canonical state.
    pub fn admit_references<'a>(
        &'a self,
        scene_epoch: u64,
        expected_prior_revision: u64,
        current_revision: u64,
        active: &'a [FormKey],
        observed: &'a [FormKey],
    ) -> JobResult<ReferenceAdmission<'a>> {
        self.validate_display(scene_epoch, expected_prior_revision, current_revision)?;
        for keys in [active, observed] {
            if keys.len() > MAX_BINDINGS || keys.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(JobError::Invalid(
                    "reference admission needs at most 10000 unique sorted keys".into(),
                ));
            }
        }
        let plan = self.sources(scene_epoch)?.plan()?;
        let members: BTreeSet<_> = plan
            .graph()
            .edges
            .iter()
            .filter(|edge| {
                edge.owner == *plan.root()
                    && edge.role == "member"
                    && edge.target.status == "resolved"
            })
            .filter_map(|edge| edge.target.key.as_ref())
            .collect();
        if active
            .iter()
            .chain(observed)
            .any(|key| !members.contains(key))
        {
            return Err(JobError::Invalid(
                "display reference is outside the protected CELL membership".into(),
            ));
        }
        if active
            .iter()
            .any(|key| observed.binary_search(key).is_err())
        {
            return Err(JobError::Invalid(
                "prepared observation does not bind every active source view".into(),
            ));
        }
        Ok(ReferenceAdmission {
            scene: self,
            prior_revision: expected_prior_revision,
            active,
            observed,
        })
    }

    fn validate_display(&self, epoch: u64, expected: u64, current: u64) -> JobResult<()> {
        self.validate(epoch)?;
        if expected != current {
            return Err(JobError::Stale);
        }
        if self.phase != Phase::Uploaded {
            return Err(JobError::Invalid(
                "reference publication needs an uploaded scene".into(),
            ));
        }
        Ok(())
    }
}

impl ReferenceAdmission<'_> {
    pub fn scene_epoch(&self) -> u64 {
        self.scene.scene_epoch
    }
    pub fn source_generation(&self) -> u64 {
        self.scene.ticket.generation()
    }
    pub fn source_identity(&self) -> &str {
        self.scene.ticket.identity()
    }
    pub fn observed_keys(&self) -> &[FormKey] {
        self.observed
    }

    /// Recheck the current host/display boundary immediately before one commit.
    /// The callback has no failure result and receives the admitted active set.
    /// It must publish the already prepared canonical/display result together;
    /// no later membership check or other fallible operation may follow mutation.
    /// This synchronous borrow cannot authorize a separate thread's later write.
    pub fn publish(
        self,
        current_scene_epoch: u64,
        current_revision: u64,
        apply: impl FnOnce(&[FormKey]),
    ) -> JobResult<()> {
        self.scene
            .validate_display(current_scene_epoch, self.prior_revision, current_revision)?;
        apply(self.active);
        Ok(())
    }
}
