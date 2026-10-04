//! One explicit strict partial quantity moves without an intermediate split.
//! Existing facts are retained exactly; no merge, equipment or owner policy.
use super::{Item, ItemHandle, ItemId, facts_copy_payload};
use crate::{
    Error, Result, World,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::{mem::size_of, num::NonZeroU32};

#[derive(Debug, Clone, Copy)]
pub struct PartialTransferLimits {
    pub max_links: usize,
    pub max_extra_bytes: usize,
    /// Logical fixed records, copied Facts/key/opaque payload and both
    /// temporary indexed lookup keys. Excludes allocator overhead/peak memory.
    pub max_copied_bytes: usize,
}
impl Default for PartialTransferLimits {
    fn default() -> Self {
        Self {
            max_links: 128,
            max_extra_bytes: 64 * 1024,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PartialTransferUsage {
    pub links: usize,
    pub extra_bytes: usize,
    pub copied_bytes: usize,
}
#[derive(Debug)]
#[must_use = "staging does not split, reserve an ID or move any inventory"]
pub struct StagedPartialItemTransfer {
    epoch: u64,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    next_item: u64,
    source: ItemHandle,
    original: Item,
    quantity: NonZeroU32,
    target: ReferenceId,
    source_total: u64,
    target_total: u64,
    usage: PartialTransferUsage,
}
impl StagedPartialItemTransfer {
    pub fn original(&self) -> &Item {
        &self.original
    }
    pub fn target(&self) -> ReferenceId {
        self.target
    }
    pub fn quantity(&self) -> NonZeroU32 {
        self.quantity
    }
    pub fn base_revision(&self) -> u64 {
        self.revision
    }
    pub fn usage(&self) -> PartialTransferUsage {
        self.usage
    }
    fn check(
        &self,
        epoch: u64,
        campaign: CampaignId,
        cohort: &str,
        revision: u64,
        next_item: u64,
    ) -> Result<()> {
        if self.epoch != epoch {
            return Err(Error::StaleHandle);
        }
        if self.campaign != campaign || self.cohort != cohort {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != revision || self.next_item != next_item {
            return Err(Error::Invalid(
                "partial item transfer revision or allocator changed".into(),
            ));
        }
        Ok(())
    }
}
/// Persistable observation, with no replay authority or transient handles.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PartialTransferReceipt {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub before_revision: u64,
    pub after_revision: u64,
    pub next_item_before: u64,
    pub next_item_after: u64,
    pub source_item: ItemId,
    pub destination_item: ItemId,
    pub source_owner: ReferenceId,
    pub destination_owner: ReferenceId,
    pub original_quantity: u32,
    pub remaining_quantity: u32,
    pub moved_quantity: u32,
    pub base: FormKey,
    pub source_total_before: u64,
    pub source_total_after: u64,
    pub destination_total_before: u64,
    pub destination_total_after: u64,
    pub usage: PartialTransferUsage,
}
struct Admission {
    remaining: NonZeroU32,
    new_id: ItemId,
    next: u64,
    revision: u64,
    links: usize,
    bytes: usize,
    source_total: u64,
    target_total: u64,
    source_after: u64,
    target_after: u64,
}
fn charge(total: &mut usize, count: usize, width: usize) -> Result<()> {
    *total = total
        .checked_add(
            count
                .checked_mul(width)
                .ok_or(Error::Capacity("partial item transfer copied bytes"))?,
        )
        .ok_or(Error::Capacity("partial item transfer copied bytes"))?;
    Ok(())
}
impl World<'_> {
    fn partial_item_admission(
        &self,
        source: ItemHandle,
        quantity: NonZeroU32,
        target: ReferenceId,
    ) -> Result<Admission> {
        let item = self.item_by_handle(source)?;
        if item.id != source.id {
            return Err(Error::Invalid(
                "partial item transfer source identity changed".into(),
            ));
        }
        self.inventory_owner(item.owner)?;
        self.inventory_owner(target)?;
        if item.owner == target {
            return Err(Error::Invalid(
                "partial item transfer needs a different destination".into(),
            ));
        }
        if quantity.get() >= item.count.get() {
            return Err(Error::Invalid(
                "partial item transfer needs a strict partial quantity".into(),
            ));
        }
        if !self.inventory_banks[&item.owner].contains(&item.id)
            || self.inventory_banks[&target].contains(&item.id)
        {
            return Err(Error::Invalid(
                "partial item transfer bank membership inconsistent".into(),
            ));
        }
        self.validate_item_facts(&item.facts)?;
        let (links, bytes) = self.item_capacity(&item.facts, None)?;
        let (new_id, next) = self.allocate_item()?;
        if self.items.contains_key(&new_id) || self.inventory_banks[&target].contains(&new_id) {
            return Err(Error::Invalid(
                "partial item transfer identity already occupied".into(),
            ));
        }
        let revision = self.next_revision()?;
        let source_total = self.count_total(item.owner, &item.facts.base);
        let target_total = self.count_total(target, &item.facts.base);
        if source_total < u64::from(item.count.get()) {
            return Err(Error::Invalid(
                "partial item transfer source count index inconsistent".into(),
            ));
        }
        let source_after = source_total
            .checked_sub(u64::from(quantity.get()))
            .ok_or_else(|| Error::Invalid("partial item transfer count underflow".into()))?;
        let target_after = target_total
            .checked_add(u64::from(quantity.get()))
            .ok_or(Error::Capacity("partial item transfer count"))?;
        Ok(Admission {
            remaining: NonZeroU32::new(item.count.get() - quantity.get())
                .expect("strict partial quantity"),
            new_id,
            next,
            revision,
            links,
            bytes,
            source_total,
            target_total,
            source_after,
            target_after,
        })
    }
    pub fn stage_partial_item_transfer(
        &self,
        source: ItemHandle,
        quantity: NonZeroU32,
        target: ReferenceId,
        limits: PartialTransferLimits,
    ) -> Result<StagedPartialItemTransfer> {
        let item = self.item_by_handle(source)?;
        // Charge all dynamic data before validators/indexed lookup can clone a
        // source key, and before the single original-Facts copy is allocated.
        let links = item.facts.links();
        let extra_bytes = item.facts.extra_bytes()?;
        if links > limits.max_links {
            return Err(Error::Capacity("partial item transfer links"));
        }
        if extra_bytes > limits.max_extra_bytes {
            return Err(Error::Capacity("partial item transfer opaque bytes"));
        }
        let mut copied_bytes = 0;
        for bytes in [
            size_of::<StagedPartialItemTransfer>(),
            size_of::<PartialTransferReceipt>(),
            size_of::<Item>(),
            self.cohort.len(),
            facts_copy_payload(&item.facts)?,
        ] {
            charge(&mut copied_bytes, 1, bytes)?;
        }
        // Stage/commit each use two indexed lookup temporary FormKeys. Final
        // count keys and the receipt base are prebuilt before publication.
        charge(&mut copied_bytes, 4, size_of::<(ReferenceId, FormKey)>())?;
        charge(
            &mut copied_bytes,
            2,
            size_of::<((ReferenceId, FormKey), u64)>(),
        )?;
        charge(&mut copied_bytes, 7, item.facts.base.origin_plugin.len())?;
        charge(&mut copied_bytes, 2, size_of::<Admission>())?;
        // The existing Facts validator uses a temporary set of supplied
        // equipment-slot words during both admissions. Charge its logical
        // records and keys; BTree node/allocator overhead remains excluded.
        charge(
            &mut copied_bytes,
            2,
            size_of::<std::collections::BTreeSet<u16>>(),
        )?;
        if let Some(slots) = &item.facts.equipped_slots {
            for _ in 0..2 {
                charge(&mut copied_bytes, slots.len(), size_of::<u16>())?;
            }
        }
        if copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("partial item transfer copied bytes"));
        }
        let admitted = self.partial_item_admission(source, quantity, target)?;
        Ok(StagedPartialItemTransfer {
            epoch: self.epoch,
            campaign: self.campaign,
            cohort: self.cohort.clone(),
            revision: self.revision,
            next_item: self.next_item,
            source,
            original: item.clone(),
            quantity,
            target,
            source_total: admitted.source_total,
            target_total: admitted.target_total,
            usage: PartialTransferUsage {
                links,
                extra_bytes,
                copied_bytes,
            },
        })
    }
    pub fn commit_partial_item_transfer(
        &mut self,
        stage: StagedPartialItemTransfer,
    ) -> Result<(PartialTransferReceipt, ItemHandle)> {
        stage.check(
            self.epoch,
            self.campaign,
            &self.cohort,
            self.revision,
            self.next_item,
        )?;
        if self.item_by_handle(stage.source)? != &stage.original {
            return Err(Error::Invalid(
                "partial item transfer original lot changed".into(),
            ));
        }
        let admitted = self.partial_item_admission(stage.source, stage.quantity, stage.target)?;
        if admitted.source_total != stage.source_total
            || admitted.target_total != stage.target_total
        {
            return Err(Error::Invalid(
                "partial item transfer count observation changed".into(),
            ));
        }
        let source_key = (stage.original.owner, stage.original.facts.base.clone());
        let target_key = (stage.target, stage.original.facts.base.clone());
        let receipt = PartialTransferReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.cohort,
            before_revision: stage.revision,
            after_revision: admitted.revision,
            next_item_before: stage.next_item,
            next_item_after: admitted.next,
            source_item: stage.original.id,
            destination_item: admitted.new_id,
            source_owner: stage.original.owner,
            destination_owner: stage.target,
            original_quantity: stage.original.count.get(),
            remaining_quantity: admitted.remaining.get(),
            moved_quantity: stage.quantity.get(),
            base: stage.original.facts.base.clone(),
            source_total_before: admitted.source_total,
            source_total_after: admitted.source_after,
            destination_total_before: admitted.target_total,
            destination_total_after: admitted.target_after,
            usage: stage.usage,
        };
        let destination = Item {
            id: admitted.new_id,
            owner: stage.target,
            count: stage.quantity,
            facts: stage.original.facts,
        };
        // Validation, arithmetic and all owned key/Facts/receipt copies are
        // complete. Existing BTree allocation retains ordinary abort-on-OOM.
        self.items
            .get_mut(&stage.source.id)
            .expect("validated source lot")
            .count = admitted.remaining;
        self.items.insert(admitted.new_id, destination);
        self.inventory_banks
            .get_mut(&stage.target)
            .expect("validated destination bank")
            .insert(admitted.new_id);
        self.item_counts.insert(source_key, admitted.source_after);
        self.item_counts.insert(target_key, admitted.target_after);
        self.next_item = admitted.next;
        self.item_links = admitted.links;
        self.item_bytes = admitted.bytes;
        self.revision = admitted.revision;
        Ok((
            receipt,
            ItemHandle {
                world: self.epoch,
                id: admitted.new_id,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_records_refuse_multiplication_and_addition_overflow_without_changing_usage() {
        let mut used = 1;
        assert!(charge(&mut used, usize::MAX, 2).is_err());
        assert_eq!(used, 1);
        assert!(charge(&mut used, 1, usize::MAX).is_err());
        assert_eq!(used, 1);
        charge(&mut used, 3, 2).unwrap();
        assert_eq!(used, 7);
    }
    #[test]
    fn private_transfer_checks_all_bindings_before_original_lot_admission() {
        let campaign = CampaignId::from_bytes([1; 16]).unwrap();
        let owner = ReferenceId(1.try_into().unwrap());
        let item = ItemId(1.try_into().unwrap());
        let stage = StagedPartialItemTransfer {
            epoch: 7,
            campaign,
            cohort: "a".repeat(64),
            revision: 9,
            next_item: 4,
            source: ItemHandle { world: 7, id: item },
            original: Item {
                id: item,
                owner,
                count: 8.try_into().unwrap(),
                facts: super::super::Facts::unknown(FormKey {
                    profile: fallout_data::identity::ProfileId::NvOriginal,
                    origin_plugin: "falloutnv.esm".into(),
                    local_id: 0x100,
                }),
            },
            quantity: 3.try_into().unwrap(),
            target: ReferenceId(2.try_into().unwrap()),
            source_total: 8,
            target_total: 0,
            usage: PartialTransferUsage {
                links: 0,
                extra_bytes: 0,
                copied_bytes: 0,
            },
        };
        assert!(stage.check(7, campaign, &"a".repeat(64), 9, 4).is_ok());
        assert!(matches!(
            stage.check(8, campaign, &"a".repeat(64), 9, 4),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            stage.check(
                7,
                CampaignId::from_bytes([2; 16]).unwrap(),
                &"a".repeat(64),
                9,
                4
            ),
            Err(Error::DefinitionChanged)
        ));
        assert!(matches!(
            stage.check(7, campaign, &"b".repeat(64), 9, 4),
            Err(Error::DefinitionChanged)
        ));
        assert!(stage.check(7, campaign, &"a".repeat(64), 10, 4).is_err());
        assert!(stage.check(7, campaign, &"a".repeat(64), 9, 5).is_err());
    }
}
