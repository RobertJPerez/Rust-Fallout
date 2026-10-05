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

/// An explicit replacement for the supplied equipped-slot facts of one item.
/// The runtime stores this value exactly; it does not infer equipment policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EquippedSlotsUpdate {
    pub item: ItemId,
    pub equipped_slots: Option<Vec<u16>>,
}

#[derive(Debug)]
struct StagedEquippedSlotsChange {
    original: Item,
    facts: Facts,
}

#[derive(Debug)]
#[must_use = "staging has no effects; commit the inventory transaction or drop it"]
pub struct StagedInventoryTransaction {
    transfers: StagedInventoryTransfers,
    equipment: Vec<StagedEquippedSlotsChange>,
    original_item_links: usize,
    original_item_bytes: usize,
    final_item_links: usize,
    final_item_bytes: usize,
    usage: TransferUsage,
}
impl StagedInventoryTransaction {
    pub fn usage(&self) -> TransferUsage {
        self.usage
    }
    pub fn transfer_rows(&self) -> &[TransferRow] {
        &self.transfers.rows
    }
    pub fn equipped_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.equipment.iter().map(|change| change.original.id)
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

/// Result of one canonical transaction. Equipment entries identify the items
/// whose caller-supplied equipped-slot facts were applied; query the World for
/// their resulting Facts. This observation grants no later mutation authority.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct InventoryTransactionReceipt {
    transfer: TransferReceipt,
    equipped_items: Vec<ItemId>,
    usage: TransferUsage,
}
impl InventoryTransactionReceipt {
    pub fn transfer(&self) -> &TransferReceipt {
        &self.transfer
    }
    pub fn equipped_items(&self) -> &[ItemId] {
        &self.equipped_items
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
    facts_copies_with_equipped_slots(facts, &facts.equipped_slots, usage, limits)
}
fn facts_copies_with_equipped_slots(
    facts: &Facts,
    equipped_slots: &Option<Vec<u16>>,
    usage: &mut TransferUsage,
    limits: TransferLimits,
) -> Result<()> {
    let old_slots = facts.equipped_slots.as_ref().map_or(0, Vec::len);
    let new_slots = equipped_slots.as_ref().map_or(0, Vec::len);
    usage.links = usage
        .links
        .checked_add(
            facts
                .links()
                .checked_sub(old_slots)
                .and_then(|links| links.checked_add(new_slots))
                .ok_or(Error::Capacity("inventory transfer links"))?,
        )
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
    if let Some(slots) = equipped_slots {
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
    /// Stage whole-lot ownership changes together with caller-supplied
    /// `equipped_slots` replacements. All revision, row, copy, link, count and
    /// item-capacity checks complete before commit; no equipment eligibility or
    /// slot compatibility rule is inferred here.
    pub fn stage_inventory_transaction(
        &self,
        transfers: &[(ItemId, ReferenceId)],
        equipment: &[EquippedSlotsUpdate],
        limits: TransferLimits,
    ) -> Result<StagedInventoryTransaction> {
        if transfers.is_empty() && equipment.is_empty() {
            return Err(Error::Invalid("empty inventory transaction".into()));
        }
        let rows = transfers
            .len()
            .checked_add(equipment.len())
            .ok_or(Error::Capacity("inventory transfer rows"))?;
        if rows > limits.max_rows {
            return Err(Error::Capacity("inventory transfer rows"));
        }

        let mut transfers = self.stage_inventory_transfers_inner(transfers, limits, true)?;
        let mut usage = transfers.usage;
        usage.rows = rows;
        charge(
            &mut usage,
            size_of::<StagedInventoryTransaction>()
                .checked_add(size_of::<InventoryTransactionReceipt>())
                .ok_or(Error::Capacity("inventory transfer copied bytes"))?,
            limits,
        )?;
        table(
            &mut usage,
            equipment.len(),
            size_of::<ItemId>(),
            limits,
        )?;
        table(
            &mut usage,
            equipment.len(),
            size_of::<ItemId>(),
            limits,
        )?;

        let mut unique = BTreeSet::new();
        let mut staged_equipment = Vec::with_capacity(equipment.len());
        let mut removed_links = 0_usize;
        let mut added_links = 0_usize;
        let mut removed_bytes = 0_usize;
        let mut added_bytes = 0_usize;
        for update in equipment {
            if !unique.insert(update.item) {
                return Err(Error::Invalid(
                    "duplicate equipped-slots item in inventory transaction".into(),
                ));
            }
            let original = self.item(update.item)?;
            self.transfer_membership(original, original.owner)?;
            charge(&mut usage, size_of::<StagedEquippedSlotsChange>(), limits)?;
            facts_copies(&original.facts, &mut usage, limits)?;
            facts_copies_with_equipped_slots(
                &original.facts,
                &update.equipped_slots,
                &mut usage,
                limits,
            )?;

            let facts = Facts {
                base: original.facts.base.clone(),
                condition: original.facts.condition.clone(),
                ownership: original.facts.ownership.clone(),
                equipped_slots: update.equipped_slots.clone(),
                ammo: original.facts.ammo.clone(),
                modifications: original.facts.modifications.clone(),
                quest_item: original.facts.quest_item,
                script_instance: original.facts.script_instance,
                extra_fields: original.facts.extra_fields.clone(),
            };
            self.validate_item_facts(&facts)?;
            removed_links = removed_links
                .checked_add(original.facts.links())
                .ok_or(Error::Capacity("total item links"))?;
            added_links = added_links
                .checked_add(facts.links())
                .ok_or(Error::Capacity("total item links"))?;
            removed_bytes = removed_bytes
                .checked_add(original.facts.extra_bytes()?)
                .ok_or(Error::Capacity("total item extra bytes"))?;
            added_bytes = added_bytes
                .checked_add(facts.extra_bytes()?)
                .ok_or(Error::Capacity("total item extra bytes"))?;
            staged_equipment.push(StagedEquippedSlotsChange {
                original: original.clone(),
                facts,
            });
        }
        let (final_item_links, final_item_bytes) = self.item_capacity_changes(
            added_links,
            added_bytes,
            removed_links,
            removed_bytes,
        )?;
        // The transfer stage keeps its own accounting; the outer view adds
        // staged equipment Facts copies and total action rows.
        Ok(StagedInventoryTransaction {
            transfers,
            equipment: staged_equipment,
            original_item_links: self.item_links,
            original_item_bytes: self.item_bytes,
            final_item_links,
            final_item_bytes,
            usage,
        })
    }

    /// Empty groups and repeated lot IDs refuse. Same-target rows follow the
    /// existing single-transfer no-op behavior while still consuming copy/row
    /// bounds. A group with any actual move publishes one global revision.
    pub fn stage_inventory_transfers(
        &self,
        transfers: &[(ItemId, ReferenceId)],
        limits: TransferLimits,
    ) -> Result<StagedInventoryTransfers> {
        self.stage_inventory_transfers_inner(transfers, limits, false)
    }

    fn stage_inventory_transfers_inner(
        &self,
        transfers: &[(ItemId, ReferenceId)],
        limits: TransferLimits,
        allow_empty: bool,
    ) -> Result<StagedInventoryTransfers> {
        if transfers.is_empty() && !allow_empty {
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
        self.validate_inventory_transfer_stage(&stage)?;
        let revision = if stage.usage.moved_rows == 0 {
            self.revision
        } else {
            self.next_revision()?
        };
        let before_revision = self.revision;
        self.apply_inventory_transfer_stage(&stage);
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

    pub fn commit_inventory_transaction(
        &mut self,
        stage: StagedInventoryTransaction,
    ) -> Result<InventoryTransactionReceipt> {
        self.validate_inventory_transfer_stage(&stage.transfers)?;
        if self.item_links != stage.original_item_links
            || self.item_bytes != stage.original_item_bytes
        {
            return Err(Error::Invalid(
                "inventory transaction capacity observation changed".into(),
            ));
        }
        for change in &stage.equipment {
            let current = self.item(change.original.id)?;
            if current != &change.original {
                return Err(Error::Invalid(
                    "inventory equipped-slots item changed".into(),
                ));
            }
            self.transfer_membership(current, current.owner)?;
        }
        let equipment_changed = stage
            .equipment
            .iter()
            .any(|change| change.original.facts.equipped_slots != change.facts.equipped_slots);
        let changed = stage.transfers.usage.moved_rows > 0 || equipment_changed;
        let revision = if changed {
            self.next_revision()?
        } else {
            self.revision
        };
        let before_revision = self.revision;
        let mut equipped_items = Vec::with_capacity(stage.equipment.len());
        self.apply_inventory_transfer_stage(&stage.transfers);
        for change in stage.equipment {
            let id = change.original.id;
            self.items
                .get_mut(&id)
                .expect("validated inventory item")
                .facts = change.facts;
            equipped_items.push(id);
        }
        self.item_links = stage.final_item_links;
        self.item_bytes = stage.final_item_bytes;
        self.revision = revision;
        Ok(InventoryTransactionReceipt {
            transfer: TransferReceipt {
                campaign: stage.transfers.campaign,
                catalogue_sha256: stage.transfers.catalogue_sha256,
                before_revision,
                after_revision: revision,
                changes: stage.transfers.rows,
                counts: stage.transfers.counts,
                usage: stage.transfers.usage,
            },
            equipped_items,
            usage: stage.usage,
        })
    }

    fn validate_inventory_transfer_stage(&self, stage: &StagedInventoryTransfers) -> Result<()> {
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
        Ok(())
    }

    fn apply_inventory_transfer_stage(&mut self, stage: &StagedInventoryTransfers) {
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
