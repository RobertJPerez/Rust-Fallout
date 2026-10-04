//! Explicit host item state. Unknown facts stay absent and stacks never merge.
use crate::{
    Error, Result, World,
    events::Clocks,
    identity::{CampaignId, InstanceId, ReferenceId, valid_form},
};
use fallout_data::identity::FormKey;
use serde::{Deserialize, Serialize};
use std::num::{NonZeroU32, NonZeroU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub NonZeroU64);
/// UI/storage handles expire when the world is restored. Persistent ItemId
/// values remain campaign-scoped canonical identities inside the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemHandle {
    world: u64,
    id: ItemId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ammo {
    pub base: FormKey,
    pub count: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueExtra {
    pub tag: [u8; 4],
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Ownership {
    Unowned,
    Actor { key: FormKey },
    Faction { key: FormKey, rank: i32 },
    Live { reference: ReferenceId },
}
/// Preserve the supplied storage width and every bit, including NaN payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Float32 { bits: u32 },
    Float64 { bits: u64 },
}
/// These are supplied host facts. None means unknown, not an original default.
/// Slot IDs, ammo counts and modification keys do not decode opaque engine words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub base: FormKey,
    pub condition: Option<Condition>,
    pub ownership: Option<Ownership>,
    pub equipped_slots: Option<Vec<u16>>,
    pub ammo: Option<Ammo>,
    pub modifications: Option<Vec<FormKey>>,
    pub quest_item: Option<bool>,
    pub script_instance: Option<InstanceId>,
    pub extra_fields: Vec<OpaqueExtra>,
}
impl Facts {
    pub fn unknown(base: FormKey) -> Self {
        Self {
            base,
            condition: None,
            ownership: None,
            equipped_slots: None,
            ammo: None,
            modifications: None,
            quest_item: None,
            script_instance: None,
            extra_fields: Vec::new(),
        }
    }
    pub(crate) fn links(&self) -> usize {
        self.equipped_slots.as_ref().map_or(0, Vec::len)
            + self.modifications.as_ref().map_or(0, Vec::len)
            + self.extra_fields.len()
            + usize::from(self.ammo.is_some())
            + usize::from(self.script_instance.is_some())
            + usize::from(self.ownership.is_some())
    }
    pub(crate) fn extra_bytes(&self) -> Result<usize> {
        self.extra_fields.iter().try_fold(0_usize, |n, f| {
            n.checked_add(f.bytes.len())
                .ok_or(Error::Capacity("item extra bytes"))
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub(crate) id: ItemId,
    pub(crate) owner: ReferenceId,
    pub(crate) count: NonZeroU32,
    pub(crate) facts: Facts,
}
impl Item {
    pub fn id(&self) -> ItemId {
        self.id
    }
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn count(&self) -> u32 {
        self.count.get()
    }
    pub fn facts(&self) -> &Facts {
        &self.facts
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bank {
    pub owner: ReferenceId,
    pub items: Vec<Item>,
}
#[derive(Debug, Serialize)]
pub struct CountTrace {
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub boundary: Clocks,
    pub subject: ReferenceId,
    pub item: FormKey,
    pub result: u64,
    pub contributions: Vec<(ItemId, u32)>,
}

impl World<'_> {
    pub fn item_handle(&self, id: ItemId) -> Result<ItemHandle> {
        self.item(id)?;
        Ok(ItemHandle {
            world: self.epoch,
            id,
        })
    }
    pub fn item_by_handle(&self, handle: ItemHandle) -> Result<&Item> {
        if handle.world != self.epoch {
            return Err(Error::StaleHandle);
        }
        self.item(handle.id)
    }
    pub fn item_id(&self, handle: ItemHandle) -> Result<ItemId> {
        Ok(self.item_by_handle(handle)?.id)
    }
    pub(crate) fn restore_item_banks(&mut self, banks: Vec<Bank>, next: u64) -> Result<()> {
        // Snapshot preflight has already validated identities, owners and facts.
        self.next_item = next;
        for bank in banks {
            self.inventory_banks
                .insert(bank.owner, std::collections::BTreeSet::new());
            for item in bank.items {
                let (links, bytes) = self.item_capacity(&item.facts, None)?;
                let total = self
                    .count_total(item.owner, &item.facts.base)
                    .checked_add(u64::from(item.count.get()))
                    .ok_or(Error::Capacity("saved item count"))?;
                self.set_count_total(item.owner, item.facts.base.clone(), total);
                self.inventory_banks
                    .get_mut(&item.owner)
                    .expect("saved bank exists")
                    .insert(item.id);
                self.items.insert(item.id, item);
                self.item_links = links;
                self.item_bytes = bytes;
            }
        }
        Ok(())
    }
    fn count_total(&self, owner: ReferenceId, base: &FormKey) -> u64 {
        self.item_counts
            .get(&(owner, base.clone()))
            .copied()
            .unwrap_or(0)
    }
    fn set_count_total(&mut self, owner: ReferenceId, base: FormKey, total: u64) {
        if total == 0 {
            self.item_counts.remove(&(owner, base));
        } else {
            self.item_counts.insert((owner, base), total);
        }
    }
    pub fn initialize_inventory(&mut self, owner: ReferenceId) -> Result<()> {
        self.reference_origin(owner)?;
        if self.inventory_banks.contains_key(&owner) {
            return Err(Error::Invalid("inventory already initialized".into()));
        }
        if self.inventory_banks.len() >= self.limits.max_inventory_banks {
            return Err(Error::Capacity("inventory banks"));
        }
        let revision = self.next_revision()?;
        self.inventory_banks
            .insert(owner, std::collections::BTreeSet::new());
        self.revision = revision;
        Ok(())
    }
    pub(crate) fn inventory_owner(&self, owner: ReferenceId) -> Result<()> {
        self.reference_origin(owner)?;
        if !self.inventory_banks.contains_key(&owner) {
            return Err(Error::Invalid(
                "inventory has no explicit initialization".into(),
            ));
        }
        Ok(())
    }
    pub fn item(&self, id: ItemId) -> Result<&Item> {
        self.items
            .get(&id)
            .ok_or_else(|| Error::Invalid("item instance missing".into()))
    }
    pub fn inventory_items(&self, owner: ReferenceId) -> Result<impl Iterator<Item = &Item>> {
        self.inventory_owner(owner)?;
        Ok(self.inventory_banks[&owner]
            .iter()
            .map(|id| &self.items[id]))
    }
    pub(crate) fn validate_item_facts(&self, f: &Facts) -> Result<()> {
        check_facts(
            f,
            self.limits,
            &|id| self.reference_origin(id).map(|_| ()),
            &|id| self.handle(id).map(|_| ()),
        )
    }
    fn item_capacity(&self, f: &Facts, replacing: Option<&Facts>) -> Result<(usize, usize)> {
        let links = self
            .item_links
            .checked_sub(replacing.map_or(0, Facts::links))
            .and_then(|n| n.checked_add(f.links()))
            .ok_or(Error::Capacity("total item links"))?;
        let prior = replacing.map(Facts::extra_bytes).transpose()?.unwrap_or(0);
        let bytes = self
            .item_bytes
            .checked_sub(prior)
            .and_then(|n| n.checked_add(f.extra_bytes().ok()?))
            .ok_or(Error::Capacity("total item extra bytes"))?;
        if links > self.limits.max_total_item_links || bytes > self.limits.max_total_item_bytes {
            return Err(Error::Capacity("total item extra state"));
        }
        Ok((links, bytes))
    }
    fn allocate_item(&self) -> Result<(ItemId, u64)> {
        if self.items.len() >= self.limits.max_item_instances {
            return Err(Error::Capacity("item instances"));
        }
        let next = self
            .next_item
            .checked_add(1)
            .ok_or(Error::Capacity("item identities"))?;
        let id = ItemId(
            NonZeroU64::new(self.next_item)
                .ok_or_else(|| Error::Invalid("zero item allocator".into()))?,
        );
        Ok((id, next))
    }
    pub fn add_item(
        &mut self,
        owner: ReferenceId,
        facts: Facts,
        count: NonZeroU32,
    ) -> Result<ItemId> {
        self.inventory_owner(owner)?;
        self.validate_item_facts(&facts)?;
        let (links, bytes) = self.item_capacity(&facts, None)?;
        let (id, next) = self.allocate_item()?;
        let revision = self.next_revision()?;
        let total = self
            .count_total(owner, &facts.base)
            .checked_add(u64::from(count.get()))
            .ok_or(Error::Capacity("inventory count"))?;
        let base = facts.base.clone();
        self.items.insert(
            id,
            Item {
                id,
                owner,
                count,
                facts,
            },
        );
        self.inventory_banks
            .get_mut(&owner)
            .expect("checked inventory")
            .insert(id);
        self.set_count_total(owner, base, total);
        self.next_item = next;
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(id)
    }
    pub fn split_item(&mut self, id: ItemId, count: NonZeroU32) -> Result<ItemId> {
        let item = self.item(id)?;
        if count.get() >= item.count.get() {
            return Err(Error::Invalid(
                "split needs a strict partial quantity".into(),
            ));
        }
        let remaining =
            NonZeroU32::new(item.count.get() - count.get()).expect("strict partial split");
        let facts = item.facts.clone();
        let owner = item.owner;
        let (links, bytes) = self.item_capacity(&facts, None)?;
        let (new, next) = self.allocate_item()?;
        let revision = self.next_revision()?;
        self.items.get_mut(&id).expect("checked item").count = remaining;
        self.items.insert(
            new,
            Item {
                id: new,
                owner,
                count,
                facts,
            },
        );
        self.inventory_banks
            .get_mut(&owner)
            .expect("checked inventory")
            .insert(new);
        self.next_item = next;
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(new)
    }
    pub fn transfer_item(&mut self, id: ItemId, target: ReferenceId) -> Result<()> {
        self.inventory_owner(target)?;
        let item = self.item(id)?;
        if item.owner == target {
            return Ok(());
        }
        let owner = item.owner;
        let base = item.facts.base.clone();
        let quantity = u64::from(item.count.get());
        let old = self
            .count_total(owner, &base)
            .checked_sub(quantity)
            .ok_or_else(|| Error::Invalid("item count index inconsistent".into()))?;
        let new = self
            .count_total(target, &base)
            .checked_add(quantity)
            .ok_or(Error::Capacity("inventory count"))?;
        let revision = self.next_revision()?;
        self.inventory_banks
            .get_mut(&owner)
            .expect("checked old inventory")
            .remove(&id);
        self.inventory_banks
            .get_mut(&target)
            .expect("checked target inventory")
            .insert(id);
        self.items.get_mut(&id).expect("checked item").owner = target;
        self.set_count_total(owner, base.clone(), old);
        self.set_count_total(target, base, new);
        self.revision = revision;
        Ok(())
    }
    pub fn remove_item_quantity(&mut self, id: ItemId, count: NonZeroU32) -> Result<()> {
        let item = self.item(id)?;
        if count.get() > item.count.get() {
            return Err(Error::Invalid("removal exceeds item quantity".into()));
        }
        let remaining = item.count.get() - count.get();
        let links = item.facts.links();
        let bytes = item.facts.extra_bytes()?;
        let owner = item.owner;
        let base = item.facts.base.clone();
        let total = self
            .count_total(owner, &base)
            .checked_sub(u64::from(count.get()))
            .ok_or_else(|| Error::Invalid("item count index inconsistent".into()))?;
        let revision = self.next_revision()?;
        if let Some(count) = NonZeroU32::new(remaining) {
            self.items.get_mut(&id).expect("checked item").count = count;
        } else {
            self.items.remove(&id);
            self.inventory_banks
                .get_mut(&owner)
                .expect("checked inventory")
                .remove(&id);
            self.item_links -= links;
            self.item_bytes -= bytes;
        }
        self.set_count_total(owner, base, total);
        self.revision = revision;
        Ok(())
    }
    pub fn replace_item_facts(&mut self, id: ItemId, facts: Facts) -> Result<()> {
        self.validate_item_facts(&facts)?;
        let item = self.item(id)?;
        let (links, bytes) = self.item_capacity(&facts, Some(&item.facts))?;
        let owner = item.owner;
        let old_base = item.facts.base.clone();
        let new_base = facts.base.clone();
        let quantity = u64::from(item.count.get());
        let change = if old_base != new_base {
            Some((
                self.count_total(owner, &old_base)
                    .checked_sub(quantity)
                    .ok_or_else(|| Error::Invalid("item count index inconsistent".into()))?,
                self.count_total(owner, &new_base)
                    .checked_add(quantity)
                    .ok_or(Error::Capacity("inventory count"))?,
            ))
        } else {
            None
        };
        let revision = self.next_revision()?;
        self.items.get_mut(&id).expect("checked item").facts = facts;
        if let Some((old, new)) = change {
            self.set_count_total(owner, old_base, old);
            self.set_count_total(owner, new_base, new);
        }
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(())
    }
    pub fn inventory_count(&self, owner: ReferenceId, base: &FormKey) -> Result<u64> {
        self.inventory_owner(owner)?;
        valid_form(base)?;
        Ok(self.count_total(owner, base))
    }
    pub fn inventory_count_trace(&self, owner: ReferenceId, base: &FormKey) -> Result<CountTrace> {
        self.inventory_count_trace_bounded(owner, base, self.limits.max_item_instances)
    }
    pub fn inventory_count_trace_bounded(
        &self,
        owner: ReferenceId,
        base: &FormKey,
        maximum: usize,
    ) -> Result<CountTrace> {
        let result = self.inventory_count(owner, base)?;
        let mut contributions = Vec::new();
        for id in &self.inventory_banks[&owner] {
            let item = &self.items[id];
            if item.facts.base == *base {
                if contributions.len() >= maximum {
                    return Err(Error::Capacity("inventory query trace contributions"));
                }
                contributions.push((item.id, item.count.get()));
            }
        }
        Ok(CountTrace {
            campaign: self.campaign,
            state_revision: self.revision,
            boundary: self.clocks,
            subject: owner,
            item: base.clone(),
            result,
            contributions,
        })
    }
}

pub(crate) fn check_facts(
    f: &Facts,
    limits: crate::Limits,
    reference_exists: &impl Fn(ReferenceId) -> Result<()>,
    instance_exists: &impl Fn(InstanceId) -> Result<()>,
) -> Result<()> {
    valid_form(&f.base)?;
    if f.links() > limits.max_item_links {
        return Err(Error::Capacity("item extra links"));
    }
    if f.extra_bytes()? > limits.max_item_bytes {
        return Err(Error::Capacity("per-item extra bytes"));
    }
    if let Some(owner) = &f.ownership {
        match owner {
            Ownership::Actor { key } | Ownership::Faction { key, .. } => valid_form(key)?,
            Ownership::Live { reference } => {
                reference_exists(*reference)?;
            }
            Ownership::Unowned => {}
        }
    }
    if let Some(ammo) = &f.ammo {
        valid_form(&ammo.base)?;
    }
    if let Some(mods) = &f.modifications {
        for key in mods {
            valid_form(key)?;
        }
    }
    if let Some(id) = f.script_instance {
        instance_exists(id)?;
    }
    if let Some(slots) = &f.equipped_slots {
        let mut unique = std::collections::BTreeSet::new();
        for slot in slots {
            if !unique.insert(*slot) {
                return Err(Error::Invalid("duplicate supplied equipment slot".into()));
            }
        }
    }
    Ok(())
}
