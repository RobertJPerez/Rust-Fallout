//! Explicit engineering transactions over own-instance locals and the existing
//! pending-event head. This does not execute bytecode or establish retail order.
use super::{InstanceHandle, World};
use crate::{
    Error, Result,
    events::{Clocks, Pending},
    identity::{CampaignId, InstanceId, Value},
};
use fallout_data::loaded_scripts::Handle;
use serde::Serialize;

/// An ephemeral, runtime-created proposal. Dropping it has no effects. Private
/// identity and assignment fields prevent callers from forging or editing its
/// authority; consuming commit and an exact revision guard prevent replay.
/// Stages never serialize, survive restoration, or allocate identities.
#[derive(Debug)]
#[must_use = "staging does not change state; commit the stage or drop it explicitly"]
pub struct StagedEventChanges {
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    handle: InstanceHandle,
    definition: Handle,
    pending: Pending,
    assignments: Vec<(u32, Value)>,
    acknowledge: bool,
}
impl StagedEventChanges {
    pub fn base_revision(&self) -> u64 {
        self.revision
    }
    pub fn sequence(&self) -> u64 {
        self.pending.sequence
    }
    pub fn instance(&self) -> InstanceId {
        self.pending.instance
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn assignments(&self) -> &[(u32, Value)] {
        &self.assignments
    }
    pub fn acknowledges(&self) -> bool {
        self.acknowledge
    }
}

/// An observation of one committed explicit host transaction. This receipt is
/// not a saved continuation, an executable plan or permission for another write.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub definition: Handle,
    pub instance: InstanceId,
    pub sequence: u64,
    pub before_revision: u64,
    pub after_revision: u64,
    pub boundary: Clocks,
    pub assignments: usize,
    pub acknowledged: Option<Pending>,
}

impl World<'_> {
    /// Stage a bounded duplicate-free batch for the exact journal head's own
    /// instance. All existing declaration, kind and reference checks apply.
    /// Values are explicit host inputs; no original defaults/coercion are inferred.
    /// The world is untouched, including its revision and pending journal.
    pub fn stage_event_changes(
        &self,
        sequence: u64,
        assignments: &[(u32, Value)],
        acknowledge: bool,
    ) -> Result<StagedEventChanges> {
        let pending = self
            .pending
            .front()
            .filter(|event| event.sequence == sequence)
            .ok_or_else(|| Error::Invalid("staging must name the first pending event".into()))?;
        let handle = self.handle(pending.instance)?;
        self.validate_assignments(handle, assignments)?;
        let instance = self.instance(handle)?;
        // Validation bounds cardinality before cloning the caller's batch.
        Ok(StagedEventChanges {
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            handle,
            definition: instance.definition.clone(),
            pending: pending.clone(),
            assignments: assignments.to_vec(),
            acknowledge,
        })
    }

    /// Commit exactly one previously staged host transaction. An intervening
    /// mutation, different world, restore, changed source/head or replay rejects
    /// before any effect. Locals and optional head acknowledgment commit together.
    /// Every successful explicit commit increments revision once, including an
    /// empty batch without acknowledgment; dropping a stage is the no-op path.
    pub fn commit_event_changes(&mut self, stage: StagedEventChanges) -> Result<Receipt> {
        let instance = self.instance(stage.handle)?;
        if self.campaign != stage.campaign
            || self.cohort != stage.cohort
            || instance.definition != stage.definition
        {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != stage.revision {
            return Err(Error::Invalid("staged event revision changed".into()));
        }
        if self.pending.front() != Some(&stage.pending) || instance.id != stage.pending.instance {
            return Err(Error::Invalid("staged event head changed".into()));
        }
        self.validate_assignments(stage.handle, &stage.assignments)?;
        let revision = self.next_revision()?;
        let assignment_count = stage.assignments.len();
        let slot = self.slot(stage.handle)?;
        // Nothing below can return an error or allocate: owned staged values move
        // into validated existing slots, and the checked journal head moves out.
        let instance = self.slots[slot]
            .value
            .as_mut()
            .expect("validated staged instance");
        for (index, value) in stage.assignments {
            *instance
                .locals
                .get_mut(&index)
                .expect("validated staged local") = value;
        }
        let acknowledged = stage
            .acknowledge
            .then(|| self.pending.pop_front().expect("validated staged head"));
        self.revision = revision;
        Ok(Receipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.cohort,
            definition: stage.definition,
            instance: stage.pending.instance,
            sequence: stage.pending.sequence,
            before_revision: stage.revision,
            after_revision: revision,
            boundary: self.clocks,
            assignments: assignment_count,
            acknowledged,
        })
    }
}
