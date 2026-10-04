//! Explicit ownership changes over existing lots. Facts and quantities are
//! observations, not new source item policy, equipment intent or merge rules.
use super::{Facts, Item, ItemId, OpaqueExtra, Ownership};
use crate::{
    Error, Result, World,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    mem::size_of,
};

/// Logical staging copies, including temporary duplicate/count lookup storage.
/// This does not describe allocator overhead or the process/commit memory peak.
#[derive(Debug, Clone, Copy)]
pub struct TransferLimits {
    pub max_rows: usize,
    pub max_links: usize,
    pub max_copied_bytes: usize,
}
impl Default for TransferLimits {
    fn default() -> Self {
        Self {
            max_rows: 256,
            max_links: 32_768,
            max_copied_bytes: 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TransferUsage {
    pub rows: usize,
    pub moved_rows: usize,
    pub links: usize,
    pub copied_bytes: usize,
}

/// One immutable original lot and its explicit destination. Order is the
/// caller's request order; no selection, ownership or equipment rule is inferred.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TransferRow {
    original: Item,
    target: ReferenceId,
}
impl TransferRow {
    pub fn original(&self) -> &Item {
        &self.original
    }
    pub fn target(&self) -> ReferenceId {
        self.target
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TransferCountChange {
    owner: ReferenceId,
    base: FormKey,
    before: u64,
    after: u64,
}
impl TransferCountChange {
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn base(&self) -> &FormKey {
        &self.base
    }
    pub fn before(&self) -> u64 {
        self.before
    }
    pub fn after(&self) -> u64 {
        self.after
    }
}

#[derive(Debug)]
#[must_use = "staging has no effects; commit the transfers or drop them"]
pub struct StagedInventoryTransfers {
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    epoch: u64,
    rows: Vec<TransferRow>,
    counts: Vec<TransferCountChange>,
    usage: TransferUsage,
}
impl StagedInventoryTransfers {
    pub fn rows(&self) -> &[TransferRow] {
        &self.rows
    }
    pub fn count_changes(&self) -> &[TransferCountChange] {
        &self.counts
    }
    pub fn usage(&self) -> TransferUsage {
        self.usage
    }
}

/// Observations only: deserialization cannot mint a stage or mutation authority.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct TransferReceipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    before_revision: u64,
    after_revision: u64,
    changes: Vec<TransferRow>,
    counts: Vec<TransferCountChange>,
    usage: TransferUsage,
}
impl TransferReceipt {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn changes(&self) -> &[TransferRow] {
        &self.changes
    }
    pub fn count_changes(&self) -> &[TransferCountChange] {
        &self.counts
    }
    pub fn usage(&self) -> TransferUsage {
        self.usage
    }
}

#[derive(Default)]
struct Delta {
    before: u64,
    outgoing: u64,
    incoming: u64,
}
impl Delta {
    fn after(&self) -> Result<u64> {
        self.before
            .checked_sub(self.outgoing)
            .ok_or_else(|| Error::Invalid("inventory transfer count index inconsistent".into()))?
            .checked_add(self.incoming)
            .ok_or(Error::Capacity("inventory transfer count"))
    }
}
fn charge(usage: &mut TransferUsage, bytes: usize, limits: TransferLimits) -> Result<()> {
    usage.copied_bytes = usage
        .copied_bytes
        .checked_add(bytes)
        .ok_or(Error::Capacity("inventory transfer copied bytes"))?;
    if usage.copied_bytes > limits.max_copied_bytes {
        return Err(Error::Capacity("inventory transfer copied bytes"));
    }
    Ok(())
}
fn table(
    usage: &mut TransferUsage,
    count: usize,
    width: usize,
    limits: TransferLimits,
) -> Result<()> {
    charge(
        usage,
        count
            .checked_mul(width)
            .ok_or(Error::Capacity("inventory transfer copied bytes"))?,
        limits,
    )
}
fn facts_copies(facts: &Facts, usage: &mut TransferUsage, limits: TransferLimits) -> Result<()> {
    usage.links = usage
        .links
        .checked_add(facts.links())
        .ok_or(Error::Capacity("inventory transfer links"))?;
    if usage.links > limits.max_links {
        return Err(Error::Capacity("inventory transfer links"));
    }
    charge(usage, facts.base.origin_plugin.len(), limits)?;
    if let Some(Ownership::Actor { key } | Ownership::Faction { key, .. }) = &facts.ownership {
        charge(usage, key.origin_plugin.len(), limits)?;
    }
    if let Some(ammo) = &facts.ammo {
        charge(usage, ammo.base.origin_plugin.len(), limits)?;
    }
    if let Some(slots) = &facts.equipped_slots {
        table(usage, slots.len(), size_of::<u16>(), limits)?;
    }
    if let Some(modifications) = &facts.modifications {
        table(usage, modifications.len(), size_of::<FormKey>(), limits)?;
        for key in modifications {
            charge(usage, key.origin_plugin.len(), limits)?;
        }
    }
    table(
        usage,
        facts.extra_fields.len(),
        size_of::<OpaqueExtra>(),
        limits,
    )?;
    for field in &facts.extra_fields {
        charge(usage, field.bytes.len(), limits)?;
    }
    Ok(())
}

impl World<'_> {
    fn transfer_membership(&self, item: &Item, target: ReferenceId) -> Result<()> {
        self.inventory_owner(item.owner)?;
        self.inventory_owner(target)?;
        if !self.inventory_banks[&item.owner].contains(&item.id)
            || (item.owner != target && self.inventory_banks[&target].contains(&item.id))
        {
            return Err(Error::Invalid(
                "inventory transfer bank membership inconsistent".into(),
            ));
        }
        Ok(())
    }
    /// Empty groups and repeated lot IDs refuse. Same-target rows follow the
    /// existing single-transfer no-op behavior while still consuming copy/row
    /// bounds. A group with any actual move publishes one global revision.
    pub fn stage_inventory_transfers(
        &self,
        transfers: &[(ItemId, ReferenceId)],
        limits: TransferLimits,
    ) -> Result<StagedInventoryTransfers> {
        if transfers.is_empty() {
            return Err(Error::Invalid("empty inventory transfers".into()));
        }
        if transfers.len() > limits.max_rows {
            return Err(Error::Capacity("inventory transfer rows"));
        }
        let mut usage = TransferUsage {
            rows: transfers.len(),
            ..TransferUsage::default()
        };
        for bytes in [
            size_of::<StagedInventoryTransfers>(),
            size_of::<BTreeSet<ItemId>>(),
            size_of::<BTreeMap<(ReferenceId, &FormKey), Delta>>(),
            self.cohort.len(),
        ] {
            charge(&mut usage, bytes, limits)?;
        }
        // Read borrowed facts first. No source-key, item vector or opaque data
        // clone is made before its complete logical copy charge is admitted.
        for &(id, target) in transfers {
            let item = self.item(id)?;
            self.transfer_membership(item, target)?;
            if item.owner != target {
                usage.moved_rows += 1;
            }
            charge(&mut usage, size_of::<TransferRow>(), limits)?;
            charge(&mut usage, size_of::<ItemId>(), limits)?;
            facts_copies(&item.facts, &mut usage, limits)?;
        }
        let mut unique = BTreeSet::new();
        let mut deltas = BTreeMap::<(ReferenceId, &FormKey), Delta>::new();
        for &(id, target) in transfers {
            if !unique.insert(id) {
                return Err(Error::Invalid("duplicate inventory transfer item".into()));
            }
            let item = self.item(id)?;
            if item.owner == target {
                continue;
            }
            for (owner, outgoing) in [(item.owner, true), (target, false)] {
                let key = (owner, &item.facts.base);
                let delta = match deltas.entry(key) {
                    Entry::Vacant(entry) => {
                        // Charge final owned key, temporary borrowed map entry and
                        // the existing count_total lookup's temporary key clone.
                        for bytes in [
                            size_of::<TransferCountChange>(),
                            item.facts.base.origin_plugin.len(),
                            size_of::<(ReferenceId, &FormKey)>(),
                            size_of::<Delta>(),
                            size_of::<(ReferenceId, FormKey)>(),
                            item.facts.base.origin_plugin.len(),
                        ] {
                            charge(&mut usage, bytes, limits)?;
                        }
                        let before = self.count_total(owner, &item.facts.base);
                        entry.insert(Delta {
                            before,
                            ..Delta::default()
                        })
                    }
                    Entry::Occupied(entry) => entry.into_mut(),
                };
                let amount = if outgoing {
                    &mut delta.outgoing
                } else {
                    &mut delta.incoming
                };
                *amount = amount
                    .checked_add(u64::from(item.count.get()))
                    .ok_or(Error::Capacity("inventory transfer count"))?;
            }
        }
        // Aggregate outgoing before incoming. Cycles/swaps do not depend on
        // row order and cannot falsely overflow an intermediate per-row total.
        let mut counts = Vec::with_capacity(deltas.len());
        for ((owner, base), delta) in deltas {
            let after = delta.after()?;
            counts.push(TransferCountChange {
                owner,
                base: base.clone(),
                before: delta.before,
                after,
            });
        }
        drop(unique);
        Ok(StagedInventoryTransfers {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            epoch: self.epoch,
            rows: transfers
                .iter()
                .map(|&(id, target)| TransferRow {
                    original: self.items[&id].clone(),
                    target,
                })
                .collect(),
            counts,
            usage,
        })
    }

    pub fn commit_inventory_transfers(
        &mut self,
        stage: StagedInventoryTransfers,
    ) -> Result<TransferReceipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision {
            return Err(Error::Invalid("inventory transfer revision changed".into()));
        }
        for row in &stage.rows {
            if self.item(row.original.id)? != &row.original {
                return Err(Error::Invalid("inventory transfer lot changed".into()));
            }
            self.transfer_membership(&row.original, row.target)?;
        }
        for count in &stage.counts {
            if self.count_total(count.owner, &count.base) != count.before {
                return Err(Error::Invalid(
                    "inventory transfer count observation changed".into(),
                ));
            }
        }
        let revision = if stage.usage.moved_rows == 0 {
            self.revision
        } else {
            self.next_revision()?
        };
        let before_revision = self.revision;
        // No fallible admission remains. IDs, quantities, Facts, item budgets
        // and allocators remain unchanged; only bank membership/owner/count do.
        for row in &stage.rows {
            if row.original.owner == row.target {
                continue;
            }
            self.inventory_banks
                .get_mut(&row.original.owner)
                .expect("observed bank")
                .remove(&row.original.id);
            self.inventory_banks
                .get_mut(&row.target)
                .expect("observed destination")
                .insert(row.original.id);
            self.items
                .get_mut(&row.original.id)
                .expect("observed lot")
                .owner = row.target;
        }
        for count in &stage.counts {
            self.set_count_total(count.owner, count.base.clone(), count.after);
        }
        self.revision = revision;
        Ok(TransferReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.catalogue_sha256,
            before_revision,
            after_revision: revision,
            changes: stage.rows,
            counts: stage.counts,
            usage: stage.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_count_boundary_accepts_net_swap_and_refuses_actual_overflow_or_underflow() {
        assert_eq!(
            Delta {
                before: u64::MAX,
                outgoing: 1,
                incoming: 1
            }
            .after()
            .unwrap(),
            u64::MAX
        );
        assert_eq!(
            Delta {
                before: u64::MAX,
                outgoing: u64::MAX,
                incoming: u64::MAX
            }
            .after()
            .unwrap(),
            u64::MAX
        );
        assert!(matches!(
            Delta {
                before: u64::MAX,
                outgoing: 0,
                incoming: 1
            }
            .after(),
            Err(Error::Capacity("inventory transfer count"))
        ));
        assert!(matches!(
            Delta {
                before: 0,
                outgoing: 1,
                incoming: 0
            }
            .after(),
            Err(Error::Invalid(_))
        ));
    }
    #[test]
    fn logical_copy_arithmetic_refuses_add_and_table_multiplication_overflow() {
        let limits = TransferLimits {
            max_copied_bytes: usize::MAX,
            ..TransferLimits::default()
        };
        let mut usage = TransferUsage {
            copied_bytes: 1,
            ..TransferUsage::default()
        };
        assert!(matches!(
            charge(&mut usage, usize::MAX, limits),
            Err(Error::Capacity("inventory transfer copied bytes"))
        ));
        assert!(matches!(
            table(&mut TransferUsage::default(), usize::MAX, 2, limits),
            Err(Error::Capacity("inventory transfer copied bytes"))
        ));
    }
}
