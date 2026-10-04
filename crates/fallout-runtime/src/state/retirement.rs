//! Explicit instance retirement does not acknowledge events or remove items.
//! Each admission scans journal/items once, regardless of selected group size.
use super::{InstanceHandle, World};
use crate::{
    Error, Result,
    identity::{CampaignId, InstanceId, Owner},
};
use fallout_data::loaded_scripts::Handle;
use serde::Serialize;
use std::{collections::BTreeSet, mem::size_of};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_instances: usize,
    pub max_visited_events: usize,
    pub max_visited_items: usize,
    pub max_removed_locals: usize,
    /// Logical stage/receipt rows, owner/definition strings, selected-ID sets
    /// and added free-slot records. No locals, context, Facts or schema copies;
    /// allocator overhead and process peak are excluded.
    pub max_copied_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_instances: 256,
            max_visited_events: 1_000_000,
            max_visited_items: 1_000_000,
            max_removed_locals: 1_000_000,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub instances: usize,
    pub visited_events: usize,
    pub visited_items: usize,
    pub removed_locals: usize,
    pub copied_bytes: usize,
}
#[derive(Debug)]
pub struct Row {
    handle: InstanceHandle,
    instance: InstanceId,
    owner: Owner,
    definition: Handle,
    locals: usize,
    next_generation: u64,
}
impl Row {
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn owner(&self) -> &Owner {
        &self.owner
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn local_count(&self) -> usize {
        self.locals
    }
}
#[derive(Debug)]
#[must_use = "staging does not retire instances or reserve free slots"]
pub struct StagedGroup {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    next_instance: u64,
    instance_count: usize,
    local_count: usize,
    free_count: usize,
    limits: Limits,
    rows: Vec<Row>,
    usage: Usage,
}
impl StagedGroup {
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    pub fn base_revision(&self) -> u64 {
        self.revision
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
    fn check(&self, world: &World<'_>) -> Result<()> {
        if self.epoch != world.epoch {
            return Err(Error::StaleHandle);
        }
        if self.campaign != world.campaign || self.cohort != world.cohort {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != world.revision
            || self.next_instance != world.next_instance
            || self.instance_count != world.instances.len()
            || self.local_count != world.local_count
            || self.free_count != world.free.len()
        {
            return Err(Error::Invalid(
                "instance retirement boundary changed".into(),
            ));
        }
        Ok(())
    }
}
/// Observation of persistent identities; never a saved handle or replay token.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ReceiptRow {
    instance: InstanceId,
    owner: Owner,
    definition: Handle,
    locals: usize,
}
impl ReceiptRow {
    pub fn instance(&self) -> InstanceId {
        self.instance
    }
    pub fn owner(&self) -> &Owner {
        &self.owner
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn local_count(&self) -> usize {
        self.locals
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    before_revision: u64,
    after_revision: u64,
    next_instance: u64,
    before_instances: usize,
    after_instances: usize,
    before_locals: usize,
    after_locals: usize,
    rows: Vec<ReceiptRow>,
    usage: Usage,
}
impl Receipt {
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn rows(&self) -> &[ReceiptRow] {
        &self.rows
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}
pub(super) fn next_generation(generation: u64) -> Result<u64> {
    generation
        .checked_add(1)
        .ok_or(Error::Capacity("slot generations"))
}
fn charge(used: &mut usize, count: usize, width: usize) -> Result<()> {
    *used = used
        .checked_add(
            count
                .checked_mul(width)
                .ok_or(Error::Capacity("instance retirement copied bytes"))?,
        )
        .ok_or(Error::Capacity("instance retirement copied bytes"))?;
    Ok(())
}
impl World<'_> {
    pub(super) fn admit_instance_retirement_links(
        &self,
        selected: impl Fn(InstanceId) -> bool,
        max_events: usize,
        max_items: usize,
    ) -> Result<(usize, usize)> {
        if self.pending.len() > max_events {
            return Err(Error::Capacity("instance retirement visited events"));
        }
        for event in &self.pending {
            if selected(event.instance) {
                return Err(Error::Invalid(
                    "instance has pending events; acknowledge them explicitly before removal"
                        .into(),
                ));
            }
        }
        if self.items.len() > max_items {
            return Err(Error::Capacity("instance retirement visited items"));
        }
        for item in self.items.values() {
            if item.facts.script_instance.is_some_and(&selected) {
                return Err(Error::Invalid(
                    "script instance is still linked by an inventory item".into(),
                ));
            }
        }
        Ok((self.pending.len(), self.items.len()))
    }
    pub fn stage_instance_retirement_group(
        &self,
        handles: &[InstanceHandle],
        limits: Limits,
    ) -> Result<StagedGroup> {
        if handles.len() > limits.max_instances {
            return Err(Error::Capacity("instance retirement instances"));
        }
        let mut usage = Usage {
            instances: handles.len(),
            ..Usage::default()
        };
        for width in [
            size_of::<StagedGroup>(),
            size_of::<Receipt>(),
            self.cohort.len(),
            2 * size_of::<BTreeSet<InstanceId>>(),
        ] {
            charge(&mut usage.copied_bytes, 1, width)?;
        }
        for width in [
            size_of::<Row>(),
            size_of::<ReceiptRow>(),
            2 * size_of::<InstanceId>(),
            size_of::<usize>(),
        ] {
            charge(&mut usage.copied_bytes, handles.len(), width)?;
        }
        // All dynamic metadata is borrowed during this pass. Two copies of
        // each owner/definition feed the stage and prebuilt terminal receipt.
        for &handle in handles {
            let instance = self.instance(handle)?;
            usage.removed_locals = usage
                .removed_locals
                .checked_add(instance.locals.len())
                .ok_or(Error::Capacity("instance retirement locals"))?;
            if usage.removed_locals > limits.max_removed_locals {
                return Err(Error::Capacity("instance retirement locals"));
            }
            charge(
                &mut usage.copied_bytes,
                2,
                instance.definition.key.record.origin_plugin.len(),
            )?;
            charge(
                &mut usage.copied_bytes,
                2,
                instance.definition.version_sha256.len(),
            )?;
            if let Owner::Quest { key } = &instance.owner {
                charge(&mut usage.copied_bytes, 2, key.origin_plugin.len())?;
            }
        }
        if usage.copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("instance retirement copied bytes"));
        }
        let mut selected = BTreeSet::new();
        for &handle in handles {
            let instance = self.instance(handle)?;
            if !selected.insert(instance.id) {
                return Err(Error::Invalid("duplicate retirement instance".into()));
            }
        }
        if !handles.is_empty() {
            (usage.visited_events, usage.visited_items) = self.admit_instance_retirement_links(
                |id| selected.contains(&id),
                limits.max_visited_events,
                limits.max_visited_items,
            )?;
            self.next_revision()?;
        }
        self.local_count
            .checked_sub(usage.removed_locals)
            .ok_or(Error::Capacity("instance retirement local charge"))?;
        self.free
            .len()
            .checked_add(handles.len())
            .ok_or(Error::Capacity("instance retirement free slots"))?;
        // Generation checks precede every owned metadata copy, including the
        // final row; no earlier instance is torn down if a generation is full.
        for &handle in handles {
            next_generation(self.slots[self.slot(handle)?].generation)?;
        }
        let rows = handles
            .iter()
            .map(|&handle| {
                let instance = self.instance(handle).expect("validated source instance");
                Row {
                    handle,
                    instance: instance.id,
                    owner: instance.owner.clone(),
                    definition: instance.definition.clone(),
                    locals: instance.locals.len(),
                    next_generation: next_generation(self.slots[handle.slot].generation)
                        .expect("validated generation"),
                }
            })
            .collect();
        Ok(StagedGroup {
            epoch: self.epoch,
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            next_instance: self.next_instance,
            instance_count: self.instances.len(),
            local_count: self.local_count,
            free_count: self.free.len(),
            limits,
            rows,
            usage,
        })
    }
    pub fn commit_instance_retirement_group(&mut self, stage: StagedGroup) -> Result<Receipt> {
        stage.check(self)?;
        let mut selected = BTreeSet::new();
        let mut removed_locals = 0usize;
        for row in &stage.rows {
            let slot = self.slot(row.handle)?;
            let instance = self.instance(row.handle)?;
            if instance.id != row.instance
                || instance.owner != row.owner
                || instance.definition != row.definition
                || instance.locals.len() != row.locals
                || self.instances.get(&row.instance) != Some(&slot)
                || self.owners.get(&row.owner) != Some(&row.instance)
                || next_generation(self.slots[slot].generation)? != row.next_generation
                || !selected.insert(row.instance)
            {
                return Err(Error::Invalid(
                    "instance retirement original changed".into(),
                ));
            }
            removed_locals = removed_locals
                .checked_add(row.locals)
                .ok_or(Error::Capacity("instance retirement locals"))?;
        }
        if removed_locals != stage.usage.removed_locals
            || removed_locals > stage.limits.max_removed_locals
        {
            return Err(Error::Invalid(
                "instance retirement local observation changed".into(),
            ));
        }
        let after_revision = if stage.rows.is_empty() {
            self.revision
        } else {
            let observed = self.admit_instance_retirement_links(
                |id| selected.contains(&id),
                stage.limits.max_visited_events,
                stage.limits.max_visited_items,
            )?;
            if observed != (stage.usage.visited_events, stage.usage.visited_items) {
                return Err(Error::Invalid(
                    "instance retirement link observations changed".into(),
                ));
            }
            self.next_revision()?
        };
        let after_locals = self
            .local_count
            .checked_sub(removed_locals)
            .ok_or(Error::Capacity("instance retirement local charge"))?;
        let after_instances = self
            .instances
            .len()
            .checked_sub(stage.rows.len())
            .ok_or(Error::Capacity("instance retirement instance charge"))?;
        self.free
            .len()
            .checked_add(stage.rows.len())
            .ok_or(Error::Capacity("instance retirement free slots"))?;
        let receipt = Receipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.cohort,
            before_revision: stage.revision,
            after_revision,
            next_instance: stage.next_instance,
            before_instances: stage.instance_count,
            after_instances,
            before_locals: stage.local_count,
            after_locals,
            rows: stage
                .rows
                .iter()
                .map(|row| ReceiptRow {
                    instance: row.instance,
                    owner: row.owner.clone(),
                    definition: row.definition.clone(),
                    locals: row.locals,
                })
                .collect(),
            usage: stage.usage,
        };
        self.free
            .try_reserve(stage.rows.len())
            .map_err(|_| Error::Capacity("instance retirement free slots"))?;
        // Every validation/arithmetic/receipt allocation is complete. Preserve
        // remove_instance's caller-order free stack and retained definition cache.
        for row in stage.rows {
            let instance = self.slots[row.handle.slot]
                .value
                .take()
                .expect("validated occupied slot");
            self.instances.remove(&instance.id);
            self.owners.remove(&instance.owner);
            self.slots[row.handle.slot].generation = row.next_generation;
            self.free.push(row.handle.slot);
        }
        self.local_count = after_locals;
        self.revision = after_revision;
        Ok(receipt)
    }
}
