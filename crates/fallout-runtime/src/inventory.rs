//! Explicit host item state. Unknown facts stay absent and stacks never merge.
use crate::{
    Error, Result, World,
    events::Clocks,
    identity::{CampaignId, InstanceId, ReferenceId, valid_form},
};
use fallout_data::identity::FormKey;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    mem::size_of,
    num::{NonZeroU32, NonZeroU64},
    ops::Bound,
};

#[path = "state/inventory_transfers.rs"]
mod transfers;
pub use transfers::{
    StagedInventoryTransfers, TransferCountChange, TransferLimits, TransferReceipt, TransferRow,
    TransferUsage,
};

#[path = "state/inventory_partial_transfer.rs"]
mod partial_transfer;
pub use partial_transfer::{
    PartialTransferLimits, PartialTransferReceipt, PartialTransferUsage, StagedPartialItemTransfer,
};

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

/// Logical owned-observation limits, independent of canonical item admission.
/// Links include existing Facts::links; opaque bytes include their exact payload.
/// These bounds do not claim a total allocator/process-memory ceiling.
#[derive(Debug, Clone, Copy)]
pub struct ViewLimits {
    pub max_items: usize,
    pub max_links: usize,
    pub max_extra_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ViewUsage {
    pub items: usize,
    pub links: usize,
    pub extra_bytes: usize,
}

/// A read-only owned observation, never mutation authority or an item rule.
/// None means uninitialized; Some(empty) is an explicit observed empty bank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InventoryView {
    campaign: CampaignId,
    catalogue_sha256: String,
    state_revision: u64,
    boundary: Clocks,
    owner: ReferenceId,
    authored: Option<FormKey>,
    items: Option<Vec<Item>>,
    usage: ViewUsage,
}
impl InventoryView {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.state_revision
    }
    pub fn boundary(&self) -> Clocks {
        self.boundary
    }
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn authored(&self) -> Option<&FormKey> {
        self.authored.as_ref()
    }
    pub fn items(&self) -> Option<&[Item]> {
        self.items.as_deref()
    }
    pub fn usage(&self) -> ViewUsage {
        self.usage
    }
}

/// Request the next `rows` lots, or the remaining tail when shorter. This is
/// separate from admission limits: a required lot never silently disappears
/// from a successful page because its facts exceed a bound.
#[derive(Debug, Clone, Copy)]
pub struct PageRequest<'a> {
    pub owner: ReferenceId,
    pub after: Option<&'a Cursor>,
    pub rows: usize,
}
#[derive(Debug, Clone, Copy)]
pub struct PageLimits {
    pub max_visited: usize,
    pub max_rows: usize,
    pub max_links: usize,
    pub max_extra_bytes: usize,
    /// Logical owned values, tables, UTF-8 and opaque payloads. Includes page
    /// metadata and its cursor; excludes allocator overhead and process peak.
    pub max_copied_bytes: usize,
}
impl Default for PageLimits {
    fn default() -> Self {
        Self {
            max_visited: 256,
            max_rows: 256,
            max_links: 32_768,
            max_extra_bytes: 1024 * 1024,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PageUsage {
    /// Lots whose facts were inspected; reading the bank's last ID to establish
    /// completion does not inspect another lot or copy any facts.
    pub visited: usize,
    pub returned: usize,
    pub links: usize,
    pub extra_bytes: usize,
    pub copied_bytes: usize,
}
/// Constructed only by this producer, never decoded as authority or saved.
#[derive(Debug, Clone)]
pub struct Cursor {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    owner: ReferenceId,
    consumed: Option<ItemId>,
}
impl Cursor {
    fn check(
        &self,
        epoch: u64,
        campaign: CampaignId,
        cohort: &str,
        revision: u64,
        owner: ReferenceId,
    ) -> Result<()> {
        if self.epoch != epoch {
            return Err(Error::StaleHandle);
        }
        if self.campaign != campaign || self.catalogue_sha256 != cohort {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != revision {
            return Err(Error::Invalid("inventory page revision changed".into()));
        }
        if self.owner != owner {
            return Err(Error::Invalid("inventory page owner changed".into()));
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct Page {
    campaign: CampaignId,
    catalogue_sha256: String,
    state_revision: u64,
    boundary: Clocks,
    owner: ReferenceId,
    authored: Option<FormKey>,
    start_after: Option<ItemId>,
    items: Option<Vec<Item>>,
    complete: bool,
    usage: PageUsage,
    #[serde(skip)]
    cursor: Cursor,
}
impl Page {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.state_revision
    }
    pub fn boundary(&self) -> Clocks {
        self.boundary
    }
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn authored(&self) -> Option<&FormKey> {
        self.authored.as_ref()
    }
    pub fn start_after(&self) -> Option<ItemId> {
        self.start_after
    }
    pub fn items(&self) -> Option<&[Item]> {
        self.items.as_deref()
    }
    pub fn is_complete(&self) -> bool {
        self.complete
    }
    /// Final position is also available on a complete page, so a caller can
    /// explicitly observe its empty tail without fabricating a position.
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }
    pub fn next_cursor(&self) -> Option<&Cursor> {
        (!self.complete).then_some(&self.cursor)
    }
    pub fn usage(&self) -> PageUsage {
        self.usage
    }
}

fn page_add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right)
        .ok_or(Error::Capacity("inventory page copied bytes"))
}
fn page_table(count: usize, width: usize) -> Result<usize> {
    count
        .checked_mul(width)
        .ok_or(Error::Capacity("inventory page copied bytes"))
}
fn page_charge(usage: &mut PageUsage, bytes: usize, limits: PageLimits) -> Result<()> {
    usage.copied_bytes = page_add(usage.copied_bytes, bytes)?;
    if usage.copied_bytes > limits.max_copied_bytes {
        return Err(Error::Capacity("inventory page copied bytes"));
    }
    Ok(())
}
fn facts_copy_payload(facts: &Facts) -> Result<usize> {
    let mut bytes = 0_usize;
    let mut add = |value| -> Result<()> {
        bytes = bytes
            .checked_add(value)
            .ok_or(Error::Capacity("item copied bytes"))?;
        Ok(())
    };
    let table = |count: usize, width: usize| {
        page_table(count, width).map_err(|_| Error::Capacity("item copied bytes"))
    };
    add(facts.base.origin_plugin.len())?;
    if let Some(Ownership::Actor { key } | Ownership::Faction { key, .. }) = &facts.ownership {
        add(key.origin_plugin.len())?;
    }
    if let Some(ammo) = &facts.ammo {
        add(ammo.base.origin_plugin.len())?;
    }
    if let Some(slots) = &facts.equipped_slots {
        add(table(slots.len(), size_of::<u16>())?)?;
    }
    if let Some(modifications) = &facts.modifications {
        add(table(modifications.len(), size_of::<FormKey>())?)?;
        for key in modifications {
            add(key.origin_plugin.len())?;
        }
    }
    add(table(facts.extra_fields.len(), size_of::<OpaqueExtra>())?)?;
    for field in &facts.extra_fields {
        add(field.bytes.len())?;
    }
    Ok(bytes)
}
fn page_fact_copies(facts: &Facts, usage: &mut PageUsage, limits: PageLimits) -> Result<()> {
    usage.links = usage
        .links
        .checked_add(facts.links())
        .ok_or(Error::Capacity("inventory page links"))?;
    if usage.links > limits.max_links {
        return Err(Error::Capacity("inventory page links"));
    }
    usage.extra_bytes = usage
        .extra_bytes
        .checked_add(facts.extra_bytes()?)
        .ok_or(Error::Capacity("inventory page extra bytes"))?;
    if usage.extra_bytes > limits.max_extra_bytes {
        return Err(Error::Capacity("inventory page extra bytes"));
    }
    page_charge(usage, size_of::<Item>(), limits)?;
    page_charge(
        usage,
        facts_copy_payload(facts).map_err(|_| Error::Capacity("inventory page copied bytes"))?,
        limits,
    )
}

#[derive(Debug, Clone, Copy)]
pub struct SourceInventoryLimits {
    pub max_lots: usize,
    pub max_source_checks: usize,
    /// Logical staged values and described temporary lookup/validation copies;
    /// excludes allocator overhead and commit/process peak memory.
    pub max_copied_bytes: usize,
}
impl Default for SourceInventoryLimits {
    fn default() -> Self {
        Self {
            max_lots: 256,
            max_source_checks: 32_768,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SourceInventoryUsage {
    pub lots: usize,
    pub source_checks: usize,
    pub links: usize,
    pub extra_bytes: usize,
    pub copied_bytes: usize,
}
#[derive(Debug)]
#[must_use = "staging reserves no bank or IDs; commit initialization or drop it"]
pub struct StagedSourceInventory {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    policy_sha256: String,
    revision: u64,
    next_item: u64,
    final_next_item: u64,
    owner: ReferenceId,
    authored: Option<FormKey>,
    lots: Vec<(Facts, NonZeroU32)>,
    proofs: Vec<crate::source_items::Proof>,
    counts: Vec<(FormKey, u64)>,
    usage: SourceInventoryUsage,
}
impl StagedSourceInventory {
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn lots(&self) -> &[(Facts, NonZeroU32)] {
        &self.lots
    }
    pub fn usage(&self) -> SourceInventoryUsage {
        self.usage
    }
}
/// Original source checks and published IDs are observations, not a reusable
/// source policy or a way to deserialize initialization authority.
#[derive(Debug, Serialize)]
pub struct SourceInventoryReceipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    policy_sha256: String,
    before_revision: u64,
    after_revision: u64,
    owner: ReferenceId,
    item_ids: Vec<ItemId>,
    proofs: Vec<crate::source_items::Proof>,
    usage: SourceInventoryUsage,
}
impl SourceInventoryReceipt {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn item_ids(&self) -> &[ItemId] {
        &self.item_ids
    }
    pub fn proofs(&self) -> &[crate::source_items::Proof] {
        &self.proofs
    }
    pub fn usage(&self) -> SourceInventoryUsage {
        self.usage
    }
}
fn initialization_charge(
    usage: &mut SourceInventoryUsage,
    bytes: usize,
    limits: SourceInventoryLimits,
) -> Result<()> {
    usage.copied_bytes = usage
        .copied_bytes
        .checked_add(bytes)
        .ok_or(Error::Capacity("source inventory copied bytes"))?;
    if usage.copied_bytes > limits.max_copied_bytes {
        return Err(Error::Capacity("source inventory copied bytes"));
    }
    Ok(())
}
fn initialization_table(
    usage: &mut SourceInventoryUsage,
    count: usize,
    width: usize,
    limits: SourceInventoryLimits,
) -> Result<()> {
    initialization_charge(
        usage,
        count
            .checked_mul(width)
            .ok_or(Error::Capacity("source inventory copied bytes"))?,
        limits,
    )
}
fn initialization_source_key(
    usage: &mut SourceInventoryUsage,
    key: &FormKey,
    limits: SourceInventoryLimits,
) -> Result<()> {
    usage.source_checks = usage
        .source_checks
        .checked_add(1)
        .ok_or(Error::Capacity("source inventory source checks"))?;
    if usage.source_checks > limits.max_source_checks {
        return Err(Error::Capacity("source inventory source checks"));
    }
    // One retained CheckedForm key plus the two existing canonical plugin-name
    // temporaries in Facts validation and Content::source_form. No header body
    // or second content index is copied.
    for bytes in [
        size_of::<crate::source_items::CheckedForm>(),
        key.origin_plugin.len(),
        size_of::<String>(),
        key.origin_plugin.len(),
        size_of::<String>(),
        key.origin_plugin.len(),
    ] {
        initialization_charge(usage, bytes, limits)?;
    }
    Ok(())
}
fn initialization_count(total: u64, added: u64) -> Result<u64> {
    total
        .checked_add(added)
        .ok_or(Error::Capacity("inventory count"))
}
fn initialization_next_item(next: u64, lots: usize) -> Result<u64> {
    if next == 0 {
        return Err(Error::Invalid("zero item allocator".into()));
    }
    next.checked_add(u64::try_from(lots).map_err(|_| Error::Capacity("item identities"))?)
        .ok_or(Error::Capacity("item identities"))
}
fn source_fact_copies(
    facts: &Facts,
    cohort_bytes: usize,
    policy_bytes: usize,
    usage: &mut SourceInventoryUsage,
    limits: SourceInventoryLimits,
) -> Result<()> {
    usage.links = usage
        .links
        .checked_add(facts.links())
        .ok_or(Error::Capacity("total item links"))?;
    usage.extra_bytes = usage
        .extra_bytes
        .checked_add(facts.extra_bytes()?)
        .ok_or(Error::Capacity("total item extra bytes"))?;
    for bytes in [
        size_of::<crate::source_items::Proof>(),
        cohort_bytes,
        policy_bytes,
        facts_copy_payload(facts)?,
        size_of::<BTreeSet<u16>>(),
    ] {
        initialization_charge(usage, bytes, limits)?;
    }
    initialization_table(
        usage,
        facts.equipped_slots.as_ref().map_or(0, Vec::len),
        size_of::<u16>(),
        limits,
    )?;
    initialization_source_key(usage, &facts.base, limits)?;
    if let Some(Ownership::Actor { key } | Ownership::Faction { key, .. }) = &facts.ownership {
        initialization_source_key(usage, key, limits)?;
    }
    if let Some(ammo) = &facts.ammo {
        initialization_source_key(usage, &ammo.base, limits)?;
    }
    if let Some(modifications) = &facts.modifications {
        for key in modifications {
            initialization_source_key(usage, key, limits)?;
        }
    }
    Ok(())
}

// Source publications reuse the single-owner initializer's source/copy authority.
// Counts remain distinct per owner/base; lots are never inferred or merged.
#[derive(Debug)]
struct SourcePublicationOwner {
    owner: ReferenceId,
    authored: Option<FormKey>,
    lots: Vec<(Facts, NonZeroU32)>,
    proofs: Vec<crate::source_items::Proof>,
}
#[derive(Debug)]
struct SourcePublication {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    policy_sha256: String,
    revision: u64,
    next_item: u64,
    final_next_item: u64,
    original_banks: usize,
    original_items: usize,
    original_links: usize,
    original_bytes: usize,
    initialize: bool,
    owners: Vec<SourcePublicationOwner>,
    counts: Vec<SourceFactsCountChange>,
    usage: SourceInventoryUsage,
}
#[derive(Debug)]
#[must_use = "staging appends no lots; commit additions or drop them"]
pub struct StagedSourceInventoryAdditions(SourcePublication);
impl StagedSourceInventoryAdditions {
    pub fn owner(&self) -> ReferenceId {
        self.0.owners[0].owner
    }
    pub fn lots(&self) -> &[(Facts, NonZeroU32)] {
        &self.0.owners[0].lots
    }
    pub fn usage(&self) -> SourceInventoryUsage {
        self.0.usage
    }
}
#[cfg(test)]
mod source_publication_tests {
    use super::*;
    use crate::save::test_source as common;
    fn fixture() -> (
        tempfile::TempDir,
        fallout_data::loaded_scripts::Catalogue,
        crate::foreign::Content,
    ) {
        let dir = tempfile::tempdir().unwrap();
        common::write_fixture(dir.path(), false);
        let mut store = fallout_data::store::RecordStore::open_nv_headers(
            dir.path(),
            &["FalloutNV.esm".into()],
            fallout_data::plugin::Limits::default(),
        )
        .unwrap();
        let catalogue = fallout_data::loaded_scripts::Catalogue::load(
            &mut store,
            fallout_data::loaded_scripts::Limits::default(),
            |_, _| Ok(()),
        )
        .unwrap();
        let content = crate::foreign::Content::load(&mut store, &catalogue, 100).unwrap();
        (dir, catalogue, content)
    }
    fn policy() -> crate::source_items::Policy {
        crate::source_items::Policy::new(&[(crate::source_items::Role::Base, &[*b"ACTI"])]).unwrap()
    }
    #[test]
    fn additions_private_bindings_and_last_identity_collision_refuse_without_a_prefix() {
        let (_dir, catalogue, content) = fixture();
        let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
        let owner = world.register_reference(None).unwrap();
        world.initialize_inventory(owner).unwrap();
        let facts = Facts::unknown(common::form(0x100));
        world
            .add_item(owner, facts.clone(), 1.try_into().unwrap())
            .unwrap();
        let lots = [
            (facts.clone(), 2.try_into().unwrap()),
            (facts, 3.try_into().unwrap()),
        ];
        let before = world.snapshot();
        let counts = world.item_counts.clone();
        let links = world.item_links;
        let bytes = world.item_bytes;
        for fault in 0..13 {
            let mut stage = world
                .stage_source_inventory_additions(
                    &content,
                    &policy(),
                    owner,
                    &lots,
                    SourceInventoryLimits::default(),
                )
                .unwrap();
            match fault {
                0 => stage.0.epoch += 1,
                1 => stage.0.campaign = CampaignId::from_bytes([99; 16]).unwrap(),
                2 => stage.0.catalogue_sha256 = "b".repeat(64),
                3 => stage.0.revision += 1,
                4 => stage.0.next_item += 1,
                5 => stage.0.final_next_item += 1,
                6 => stage.0.original_banks += 1,
                7 => stage.0.original_items += 1,
                8 => stage.0.original_links += 1,
                9 => stage.0.original_bytes += 1,
                10 => stage.0.counts[0].before += 1,
                11 => stage.0.owners[0].authored = Some(common::form(0x100)),
                _ => stage.0.usage.links += 1,
            }
            assert!(
                world
                    .commit_source_inventory_additions(&content, &policy(), stage)
                    .is_err()
            );
            assert_eq!(world.snapshot(), before);
            assert_eq!(world.item_counts, counts);
            assert_eq!((world.item_links, world.item_bytes), (links, bytes));
        }
        // A corrupt allocator intersects the final existing ID only after a
        // valid first row. Admission still publishes no earlier item.
        let existing = world.items.remove(&ItemId(1.try_into().unwrap())).unwrap();
        let id = ItemId(3.try_into().unwrap());
        world.items.insert(id, Item { id, ..existing });
        let bank = world.inventory_banks.get_mut(&owner).unwrap();
        bank.remove(&ItemId(1.try_into().unwrap()));
        bank.insert(id);
        let before = world.snapshot();
        let stage = world
            .stage_source_inventory_additions(
                &content,
                &policy(),
                owner,
                &lots,
                SourceInventoryLimits::default(),
            )
            .unwrap();
        assert!(
            world
                .commit_source_inventory_additions(&content, &policy(), stage)
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    #[test]
    fn additions_allocator_count_revision_and_copy_arithmetic_never_wrap() {
        let (_dir, catalogue, content) = fixture();
        let mut world = World::new(&catalogue, crate::Limits::default()).unwrap();
        let owner = world.register_reference(None).unwrap();
        world.initialize_inventory(owner).unwrap();
        let key = common::form(0x100);
        let lots = [
            (Facts::unknown(key.clone()), 1.try_into().unwrap()),
            (Facts::unknown(key.clone()), 1.try_into().unwrap()),
        ];
        world.next_item = u64::MAX - 1;
        let before = world.snapshot();
        assert!(
            world
                .stage_source_inventory_additions(
                    &content,
                    &policy(),
                    owner,
                    &lots,
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
        world.next_item = 1;
        world.item_counts.insert((owner, key.clone()), u64::MAX - 1);
        let before = world.snapshot();
        assert!(
            world
                .stage_source_inventory_additions(
                    &content,
                    &policy(),
                    owner,
                    &lots,
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
        world.item_counts.clear();
        world.revision = u64::MAX;
        let before = world.snapshot();
        assert!(
            world
                .stage_source_inventory_additions(
                    &content,
                    &policy(),
                    owner,
                    &lots,
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
        let empty = world
            .stage_source_inventory_additions(
                &content,
                &policy(),
                owner,
                &[],
                SourceInventoryLimits::default(),
            )
            .unwrap();
        world
            .commit_source_inventory_additions(&content, &policy(), empty)
            .unwrap();
        assert_eq!(world.snapshot(), before);
        let mut usage = SourceInventoryUsage {
            copied_bytes: usize::MAX,
            ..Default::default()
        };
        assert!(
            initialization_charge(
                &mut usage,
                1,
                SourceInventoryLimits {
                    max_copied_bytes: usize::MAX,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}
#[derive(Debug, Serialize)]
pub struct SourceInventoryOwnerReceipt {
    owner: ReferenceId,
    item_ids: Vec<ItemId>,
    proofs: Vec<crate::source_items::Proof>,
}
impl SourceInventoryOwnerReceipt {
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn item_ids(&self) -> &[ItemId] {
        &self.item_ids
    }
    pub fn proofs(&self) -> &[crate::source_items::Proof] {
        &self.proofs
    }
}
struct SourcePublicationReceipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    policy_sha256: String,
    before_revision: u64,
    after_revision: u64,
    owners: Vec<SourceInventoryOwnerReceipt>,
    usage: SourceInventoryUsage,
}
impl World<'_> {
    fn source_publication_capacity(
        &self,
        owners: usize,
        initialize: bool,
        usage: SourceInventoryUsage,
    ) -> Result<(usize, usize, u64, u64)> {
        let banks = self
            .inventory_banks
            .len()
            .checked_add(if initialize { owners } else { 0 })
            .ok_or(Error::Capacity("inventory banks"))?;
        if banks > self.limits.max_inventory_banks {
            return Err(Error::Capacity("inventory banks"));
        }
        let items = self
            .items
            .len()
            .checked_add(usage.lots)
            .ok_or(Error::Capacity("item instances"))?;
        if items > self.limits.max_item_instances {
            return Err(Error::Capacity("item instances"));
        }
        let next = initialization_next_item(self.next_item, usage.lots)?;
        let (links, bytes) = self.item_capacity_changes(usage.links, usage.extra_bytes, 0, 0)?;
        let changes = if initialize {
            owners != 0
        } else {
            usage.lots != 0
        };
        let revision = if changes {
            self.next_revision()?
        } else {
            self.revision
        };
        Ok((links, bytes, next, revision))
    }
    fn stage_source_publication<'a, I>(
        &self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        requests: I,
        admission: (bool, usize, usize),
        limits: SourceInventoryLimits,
    ) -> crate::source_items::Result<SourcePublication>
    where
        I: ExactSizeIterator<Item = (ReferenceId, &'a [(Facts, NonZeroU32)])> + Clone,
    {
        let (initialize, max_owners, wrapper_bytes) = admission;
        content.validate_world(self)?;
        let owners = requests.len();
        if owners > max_owners {
            return Err(Error::Capacity("source inventory owners").into());
        }
        let mut usage = SourceInventoryUsage::default();
        for bytes in [
            size_of::<SourcePublication>(),
            wrapper_bytes,
            size_of::<SourcePublicationReceipt>(),
            2 * size_of::<BTreeSet<ReferenceId>>(),
            size_of::<BTreeMap<(ReferenceId, &FormKey), (u64, u64)>>(),
            self.cohort.len(),
            policy.sha256().len(),
        ] {
            initialization_charge(&mut usage, bytes, limits)?;
        }
        initialization_table(&mut usage, owners, 2 * size_of::<ReferenceId>(), limits)?;
        let mut selected = BTreeSet::new();
        // All logical bounds and cumulative canonical capacity precede source
        // diagnostics or retained Facts/key copies, including the last owner.
        for (owner, lots) in requests.clone() {
            let authored = self.reference_origin(owner)?;
            if !selected.insert(owner) {
                return Err(Error::Invalid("duplicate inventory owner".into()).into());
            }
            if initialize {
                if self.inventory_banks.contains_key(&owner) {
                    return Err(Error::Invalid("inventory already initialized".into()).into());
                }
            } else {
                self.inventory_owner(owner)?;
            }
            usage.lots = usage
                .lots
                .checked_add(lots.len())
                .ok_or(Error::Capacity("source inventory lots"))?;
            if usage.lots > limits.max_lots {
                return Err(Error::Capacity("source inventory lots").into());
            }
            for bytes in [
                size_of::<SourcePublicationOwner>(),
                size_of::<SourceInventoryOwnerReceipt>(),
                size_of::<(ReferenceId, BTreeSet<ItemId>)>(),
                authored.map_or(0, |key| key.origin_plugin.len()),
            ] {
                initialization_charge(&mut usage, bytes, limits)?;
            }
            for (facts, _) in lots {
                for bytes in [
                    size_of::<(Facts, NonZeroU32)>(),
                    size_of::<Item>(),
                    2 * size_of::<ItemId>(),
                ] {
                    initialization_charge(&mut usage, bytes, limits)?;
                }
                source_fact_copies(
                    facts,
                    self.cohort.len(),
                    policy.sha256().len(),
                    &mut usage,
                    limits,
                )?;
                // Commit revalidates canonical key syntax and equipment slots;
                // source proofs themselves move without another source clone.
                initialization_charge(&mut usage, size_of::<BTreeSet<u16>>(), limits)?;
                initialization_table(
                    &mut usage,
                    facts.equipped_slots.as_ref().map_or(0, Vec::len),
                    size_of::<u16>(),
                    limits,
                )?;
                let ownership = match &facts.ownership {
                    Some(Ownership::Actor { key } | Ownership::Faction { key, .. }) => Some(key),
                    _ => None,
                };
                for key in std::iter::once(&facts.base)
                    .chain(ownership)
                    .chain(facts.ammo.as_ref().map(|ammo| &ammo.base))
                    .chain(facts.modifications.iter().flatten())
                {
                    initialization_charge(&mut usage, size_of::<String>(), limits)?;
                    initialization_charge(&mut usage, key.origin_plugin.len(), limits)?;
                }
            }
        }
        let (_, _, final_next_item, _) =
            self.source_publication_capacity(owners, initialize, usage)?;
        let mut totals = BTreeMap::<(ReferenceId, &FormKey), (u64, u64)>::new();
        for (owner, lots) in requests.clone() {
            for (facts, count) in lots {
                let (_, after) = match totals.entry((owner, &facts.base)) {
                    Entry::Vacant(entry) => {
                        for bytes in [
                            size_of::<((ReferenceId, &FormKey), (u64, u64))>(),
                            size_of::<SourceFactsCountChange>(),
                            facts.base.origin_plugin.len(),
                            // Stage and commit each use the existing owned-key lookup.
                            2 * size_of::<(ReferenceId, FormKey)>(),
                            2 * facts.base.origin_plugin.len(),
                        ] {
                            initialization_charge(&mut usage, bytes, limits)?;
                        }
                        let before = self.count_total(owner, &facts.base);
                        if initialize && before != 0 {
                            return Err(Error::Invalid(
                                "uninitialized inventory count index inconsistent".into(),
                            )
                            .into());
                        }
                        entry.insert((before, before))
                    }
                    Entry::Occupied(entry) => entry.into_mut(),
                };
                *after = initialization_count(*after, u64::from(count.get()))?;
            }
        }
        let mut retained = Vec::with_capacity(owners);
        for (owner, lots) in requests {
            let mut proofs = Vec::with_capacity(lots.len());
            for (facts, _) in lots {
                proofs.push(crate::source_items::validate(self, content, policy, facts)?);
            }
            retained.push(SourcePublicationOwner {
                owner,
                authored: self.reference_origin(owner)?.cloned(),
                lots: lots.to_vec(),
                proofs,
            });
        }
        Ok(SourcePublication {
            epoch: self.epoch,
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            policy_sha256: policy.sha256().into(),
            revision: self.revision,
            next_item: self.next_item,
            final_next_item,
            original_banks: self.inventory_banks.len(),
            original_items: self.items.len(),
            original_links: self.item_links,
            original_bytes: self.item_bytes,
            initialize,
            owners: retained,
            counts: totals
                .into_iter()
                .map(|((owner, base), (before, after))| SourceFactsCountChange {
                    owner,
                    base: base.clone(),
                    before,
                    after,
                })
                .collect(),
            usage,
        })
    }
    fn commit_source_publication(
        &mut self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        stage: SourcePublication,
    ) -> crate::source_items::Result<SourcePublicationReceipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle.into());
        }
        content.validate_world(self)?;
        if stage.campaign != self.campaign || stage.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged.into());
        }
        if stage.policy_sha256 != policy.sha256() {
            return Err(crate::source_items::Failure::Policy(
                "inventory publication policy changed",
            ));
        }
        if stage.revision != self.revision
            || stage.next_item != self.next_item
            || stage.original_banks != self.inventory_banks.len()
            || stage.original_items != self.items.len()
            || stage.original_links != self.item_links
            || stage.original_bytes != self.item_bytes
        {
            return Err(
                Error::Invalid("source inventory publication boundary changed".into()).into(),
            );
        }
        let mut selected = BTreeSet::new();
        let mut lots = 0usize;
        let mut added_links = 0usize;
        let mut added_bytes = 0usize;
        for row in &stage.owners {
            if self.reference_origin(row.owner)? != row.authored.as_ref()
                || !selected.insert(row.owner)
            {
                return Err(
                    Error::Invalid("source inventory owner observation changed".into()).into(),
                );
            }
            if stage.initialize {
                if self.inventory_banks.contains_key(&row.owner) {
                    return Err(Error::Invalid("inventory already initialized".into()).into());
                }
            } else {
                self.inventory_owner(row.owner)?;
            }
            if row.proofs.len() != row.lots.len() {
                return Err(Error::Invalid("source inventory proof count changed".into()).into());
            }
            lots = lots
                .checked_add(row.lots.len())
                .ok_or(Error::Capacity("source inventory lots"))?;
            for (facts, _) in &row.lots {
                // The retained proof binds immutable Content+Policy+Facts. Live
                // identity checks run again without cloning source metadata.
                self.validate_item_facts(facts)?;
                added_links = added_links
                    .checked_add(facts.links())
                    .ok_or(Error::Capacity("total item links"))?;
                added_bytes = added_bytes
                    .checked_add(facts.extra_bytes()?)
                    .ok_or(Error::Capacity("total item extra bytes"))?;
            }
        }
        if (lots, added_links, added_bytes)
            != (stage.usage.lots, stage.usage.links, stage.usage.extra_bytes)
        {
            return Err(Error::Invalid("source inventory usage changed".into()).into());
        }
        let (links, bytes, next_item, revision) =
            self.source_publication_capacity(stage.owners.len(), stage.initialize, stage.usage)?;
        if next_item != stage.final_next_item {
            return Err(Error::Invalid("source inventory allocator changed".into()).into());
        }
        for count in &stage.counts {
            if self.count_total(count.owner, &count.base) != count.before {
                return Err(Error::Invalid("source inventory count changed".into()).into());
            }
        }
        let mut items = Vec::with_capacity(lots);
        let mut receipts = Vec::with_capacity(stage.owners.len());
        let mut banks = Vec::with_capacity(stage.owners.len());
        let mut next = stage.next_item;
        for row in stage.owners {
            let mut item_ids = Vec::with_capacity(row.lots.len());
            for (facts, count) in row.lots {
                let id = ItemId(
                    NonZeroU64::new(next)
                        .ok_or_else(|| Error::Invalid("zero item allocator".into()))?,
                );
                next = next
                    .checked_add(1)
                    .ok_or(Error::Capacity("item identities"))?;
                if self.items.contains_key(&id) {
                    return Err(
                        Error::Invalid("source inventory item identity occupied".into()).into(),
                    );
                }
                items.push(Item {
                    id,
                    owner: row.owner,
                    count,
                    facts,
                });
                item_ids.push(id);
            }
            if stage.initialize {
                banks.push((row.owner, item_ids.iter().copied().collect::<BTreeSet<_>>()));
            }
            receipts.push(SourceInventoryOwnerReceipt {
                owner: row.owner,
                item_ids,
                proofs: row.proofs,
            });
        }
        let receipt = SourcePublicationReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.catalogue_sha256,
            policy_sha256: stage.policy_sha256,
            before_revision: self.revision,
            after_revision: revision,
            owners: receipts,
            usage: stage.usage,
        };
        // Every fallible admission, identity calculation and owned receipt/key
        // preparation is complete. Move the admitted records into canonical state.
        for (owner, bank) in banks {
            self.inventory_banks.insert(owner, bank);
        }
        for item in items {
            if !stage.initialize {
                self.inventory_banks
                    .get_mut(&item.owner)
                    .expect("admitted bank")
                    .insert(item.id);
            }
            self.items.insert(item.id, item);
        }
        for count in stage.counts {
            self.set_count_total(count.owner, count.base, count.after);
        }
        self.next_item = next_item;
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(receipt)
    }
    /// Append explicit distinct lots to one already initialized bank. An empty
    /// list changes neither revision nor allocator; existing lots stay untouched.
    pub fn stage_source_inventory_additions(
        &self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        owner: ReferenceId,
        lots: &[(Facts, NonZeroU32)],
        limits: SourceInventoryLimits,
    ) -> crate::source_items::Result<StagedSourceInventoryAdditions> {
        self.stage_source_publication(
            content,
            policy,
            std::iter::once((owner, lots)),
            (false, 1, size_of::<StagedSourceInventoryAdditions>()),
            limits,
        )
        .map(StagedSourceInventoryAdditions)
    }
    pub fn commit_source_inventory_additions(
        &mut self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        stage: StagedSourceInventoryAdditions,
    ) -> crate::source_items::Result<SourceInventoryReceipt> {
        let receipt = self.commit_source_publication(content, policy, stage.0)?;
        let row = receipt
            .owners
            .into_iter()
            .next()
            .expect("one admitted owner");
        Ok(SourceInventoryReceipt {
            campaign: receipt.campaign,
            catalogue_sha256: receipt.catalogue_sha256,
            policy_sha256: receipt.policy_sha256,
            before_revision: receipt.before_revision,
            after_revision: receipt.after_revision,
            owner: row.owner,
            item_ids: row.item_ids,
            proofs: row.proofs,
            usage: receipt.usage,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SourceFactsLimits {
    pub max_lots: usize,
    /// Original and replacement Facts links, including repeated supplied values.
    pub max_links: usize,
    pub max_source_checks: usize,
    /// Logical stage and described validation/lookup copies, not process peak.
    pub max_copied_bytes: usize,
}
impl Default for SourceFactsLimits {
    fn default() -> Self {
        Self {
            max_lots: 256,
            max_links: 32_768,
            max_source_checks: 32_768,
            max_copied_bytes: 2 * 1024 * 1024,
        }
    }
}
/// Lots counts edits; links and opaque bytes include originals and replacements.
/// Source checks count replacement roles only. Copy admission reuses the source
/// inventory staging charges and their capacity failure categories.
pub type SourceFactsUsage = SourceInventoryUsage;
#[derive(Debug)]
pub struct SourceFactsRow {
    original: Item,
    replacement: Facts,
}
impl SourceFactsRow {
    pub fn original(&self) -> &Item {
        &self.original
    }
    pub fn replacement(&self) -> &Facts {
        &self.replacement
    }
}
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct SourceFactsCountChange {
    owner: ReferenceId,
    base: FormKey,
    before: u64,
    after: u64,
}
impl SourceFactsCountChange {
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
#[must_use = "staging edits no lots; commit the facts batch or drop it"]
pub struct StagedSourceFacts {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    policy_sha256: String,
    revision: u64,
    next_item: u64,
    rows: Vec<SourceFactsRow>,
    proofs: Vec<crate::source_items::Proof>,
    counts: Vec<SourceFactsCountChange>,
    added_links: usize,
    added_bytes: usize,
    removed_links: usize,
    removed_bytes: usize,
    usage: SourceFactsUsage,
}
impl StagedSourceFacts {
    pub fn rows(&self) -> &[SourceFactsRow] {
        &self.rows
    }
    pub fn count_changes(&self) -> &[SourceFactsCountChange] {
        &self.counts
    }
    pub fn usage(&self) -> SourceFactsUsage {
        self.usage
    }
}
#[derive(Debug, Serialize)]
pub struct SourceFactsReceipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    policy_sha256: String,
    before_revision: u64,
    after_revision: u64,
    item_ids: Vec<ItemId>,
    proofs: Vec<crate::source_items::Proof>,
    counts: Vec<SourceFactsCountChange>,
    usage: SourceFactsUsage,
}
impl SourceFactsReceipt {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn item_ids(&self) -> &[ItemId] {
        &self.item_ids
    }
    pub fn proofs(&self) -> &[crate::source_items::Proof] {
        &self.proofs
    }
    pub fn count_changes(&self) -> &[SourceFactsCountChange] {
        &self.counts
    }
    pub fn usage(&self) -> SourceFactsUsage {
        self.usage
    }
}
#[derive(Default)]
struct SourceFactsDelta {
    before: u64,
    outgoing: u64,
    incoming: u64,
}
fn source_facts_count_after(before: u64, outgoing: u64, incoming: u64) -> Result<u64> {
    before
        .checked_sub(outgoing)
        .ok_or_else(|| Error::Invalid("source facts count index inconsistent".into()))?
        .checked_add(incoming)
        .ok_or(Error::Capacity("source facts count"))
}

impl World<'_> {
    fn source_facts_membership(&self, item: &Item) -> Result<()> {
        self.inventory_owner(item.owner)?;
        if !self.inventory_banks[&item.owner].contains(&item.id) {
            return Err(Error::Invalid(
                "source facts bank membership inconsistent".into(),
            ));
        }
        Ok(())
    }
    /// Empty or duplicate groups refuse. A nonempty explicit edit publishes one
    /// revision even when the supplied facts equal the originals, matching the
    /// existing single-lot replacement operation.
    pub fn stage_source_item_facts(
        &self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        edits: &[(ItemHandle, Facts)],
        limits: SourceFactsLimits,
    ) -> crate::source_items::Result<StagedSourceFacts> {
        content.validate_world(self)?;
        if edits.is_empty() {
            return Err(Error::Invalid("empty source facts edits".into()).into());
        }
        if edits.len() > limits.max_lots {
            return Err(Error::Capacity("source facts lots").into());
        }
        let copies = SourceInventoryLimits {
            max_lots: limits.max_lots,
            max_source_checks: limits.max_source_checks,
            max_copied_bytes: limits.max_copied_bytes,
        };
        let mut usage = SourceFactsUsage {
            lots: edits.len(),
            ..SourceFactsUsage::default()
        };
        for bytes in [
            size_of::<StagedSourceFacts>(),
            size_of::<BTreeSet<ItemId>>(),
            size_of::<BTreeMap<(ReferenceId, &FormKey), SourceFactsDelta>>(),
            self.cohort.len(),
            policy.sha256().len(),
        ] {
            initialization_charge(&mut usage, bytes, copies)?;
        }
        let mut removed_links = 0_usize;
        let mut removed_bytes = 0_usize;
        // Borrowed-only precharge. Original facts are retained to compare exact
        // lot authority at commit, and replacement source roles use the same
        // bounded admission as source inventory initialization.
        for (handle, replacement) in edits {
            let item = self.item_by_handle(*handle)?;
            self.source_facts_membership(item)?;
            for bytes in [
                size_of::<SourceFactsRow>(),
                size_of::<ItemId>(),
                facts_copy_payload(&item.facts)?,
            ] {
                initialization_charge(&mut usage, bytes, copies)?;
            }
            removed_links = removed_links
                .checked_add(item.facts.links())
                .ok_or(Error::Capacity("source facts links"))?;
            removed_bytes = removed_bytes
                .checked_add(item.facts.extra_bytes()?)
                .ok_or(Error::Capacity("source facts extra bytes"))?;
            source_fact_copies(
                replacement,
                self.cohort.len(),
                policy.sha256().len(),
                &mut usage,
                copies,
            )?;
        }
        let added_links = usage.links;
        let added_bytes = usage.extra_bytes;
        usage.links = usage
            .links
            .checked_add(removed_links)
            .ok_or(Error::Capacity("source facts links"))?;
        usage.extra_bytes = usage
            .extra_bytes
            .checked_add(removed_bytes)
            .ok_or(Error::Capacity("source facts extra bytes"))?;
        if usage.links > limits.max_links {
            return Err(Error::Capacity("source facts links").into());
        }
        self.item_capacity_changes(added_links, added_bytes, removed_links, removed_bytes)?;
        self.next_revision()?;
        let mut unique = BTreeSet::new();
        let mut deltas = BTreeMap::<(ReferenceId, &FormKey), SourceFactsDelta>::new();
        for (handle, facts) in edits {
            let item = self.item_by_handle(*handle)?;
            if !unique.insert(item.id) {
                return Err(Error::Invalid("duplicate source facts item".into()).into());
            }
            if item.facts.base == facts.base {
                continue;
            }
            for (base, outgoing) in [(&item.facts.base, true), (&facts.base, false)] {
                let delta = match deltas.entry((item.owner, base)) {
                    Entry::Vacant(entry) => {
                        for bytes in [
                            size_of::<((ReferenceId, &FormKey), SourceFactsDelta)>(),
                            size_of::<SourceFactsCountChange>(),
                            base.origin_plugin.len(),
                            size_of::<(ReferenceId, FormKey)>(),
                            base.origin_plugin.len(),
                        ] {
                            initialization_charge(&mut usage, bytes, copies)?;
                        }
                        entry.insert(SourceFactsDelta {
                            before: self.count_total(item.owner, base),
                            ..SourceFactsDelta::default()
                        })
                    }
                    Entry::Occupied(entry) => entry.into_mut(),
                };
                let value = if outgoing {
                    &mut delta.outgoing
                } else {
                    &mut delta.incoming
                };
                *value = value
                    .checked_add(u64::from(item.count.get()))
                    .ok_or(Error::Capacity("source facts count"))?;
            }
        }
        // Check all net totals before cloning keys or producing owned diagnostics.
        for delta in deltas.values() {
            source_facts_count_after(delta.before, delta.outgoing, delta.incoming)?;
        }
        let mut proofs = Vec::with_capacity(edits.len());
        for (_, facts) in edits {
            proofs.push(crate::source_items::validate(self, content, policy, facts)?);
        }
        let counts = deltas
            .into_iter()
            .map(|((owner, base), delta)| {
                Ok(SourceFactsCountChange {
                    owner,
                    base: base.clone(),
                    before: delta.before,
                    after: source_facts_count_after(delta.before, delta.outgoing, delta.incoming)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(StagedSourceFacts {
            epoch: self.epoch,
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            policy_sha256: policy.sha256().into(),
            revision: self.revision,
            next_item: self.next_item,
            rows: edits
                .iter()
                .map(|(handle, replacement)| SourceFactsRow {
                    original: self.items[&handle.id].clone(),
                    replacement: replacement.clone(),
                })
                .collect(),
            proofs,
            counts,
            added_links,
            added_bytes,
            removed_links,
            removed_bytes,
            usage,
        })
    }
    pub fn commit_source_item_facts(
        &mut self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        stage: StagedSourceFacts,
    ) -> crate::source_items::Result<SourceFactsReceipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle.into());
        }
        content.validate_world(self)?;
        if stage.campaign != self.campaign || stage.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged.into());
        }
        if stage.policy_sha256 != policy.sha256() {
            return Err(crate::source_items::Failure::Policy(
                "facts edit policy changed",
            ));
        }
        if stage.revision != self.revision || stage.next_item != self.next_item {
            return Err(Error::Invalid("source facts boundary changed".into()).into());
        }
        for row in &stage.rows {
            if self.item(row.original.id)? != &row.original {
                return Err(Error::Invalid("source facts lot changed".into()).into());
            }
            self.source_facts_membership(&row.original)?;
        }
        for count in &stage.counts {
            if self.count_total(count.owner, &count.base) != count.before {
                return Err(Error::Invalid("source facts count observation changed".into()).into());
            }
        }
        let (links, bytes) = self.item_capacity_changes(
            stage.added_links,
            stage.added_bytes,
            stage.removed_links,
            stage.removed_bytes,
        )?;
        let revision = self.next_revision()?;
        let item_ids = stage.rows.iter().map(|row| row.original.id).collect();
        let before_revision = self.revision;
        // All admission is complete. Move facts into the exact existing lots;
        // quantities, owners, bank membership, IDs and allocators stay intact.
        for row in stage.rows {
            self.items
                .get_mut(&row.original.id)
                .expect("observed lot")
                .facts = row.replacement;
        }
        for count in &stage.counts {
            self.set_count_total(count.owner, count.base.clone(), count.after);
        }
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(SourceFactsReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.catalogue_sha256,
            policy_sha256: stage.policy_sha256,
            before_revision,
            after_revision: revision,
            item_ids,
            proofs: stage.proofs,
            counts: stage.counts,
            usage: stage.usage,
        })
    }
    fn source_inventory_capacity(
        &self,
        owner: ReferenceId,
        lots: usize,
        links: usize,
        bytes: usize,
    ) -> Result<(usize, usize, u64)> {
        self.reference_origin(owner)?;
        if self.inventory_banks.contains_key(&owner) {
            return Err(Error::Invalid("inventory already initialized".into()));
        }
        if self.inventory_banks.len() >= self.limits.max_inventory_banks {
            return Err(Error::Capacity("inventory banks"));
        }
        if self
            .items
            .len()
            .checked_add(lots)
            .ok_or(Error::Capacity("item instances"))?
            > self.limits.max_item_instances
        {
            return Err(Error::Capacity("item instances"));
        }
        let next = initialization_next_item(self.next_item, lots)?;
        let (links, bytes) = self.item_capacity_changes(links, bytes, 0, 0)?;
        self.next_revision()?;
        Ok((links, bytes, next))
    }
    /// Every caller-supplied lot is checked before creating the bank. An empty
    /// list explicitly initializes an empty bank; it still publishes one revision.
    pub fn stage_source_inventory_initialization(
        &self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        owner: ReferenceId,
        lots: &[(Facts, NonZeroU32)],
        limits: SourceInventoryLimits,
    ) -> crate::source_items::Result<StagedSourceInventory> {
        content.validate_world(self)?;
        let authored = self.reference_origin(owner)?;
        if lots.len() > limits.max_lots {
            return Err(Error::Capacity("source inventory lots").into());
        }
        let mut usage = SourceInventoryUsage {
            lots: lots.len(),
            ..SourceInventoryUsage::default()
        };
        for bytes in [
            size_of::<StagedSourceInventory>(),
            size_of::<BTreeMap<&FormKey, u64>>(),
            self.cohort.len(),
            policy.sha256().len(),
            authored.map_or(0, |key| key.origin_plugin.len()),
        ] {
            initialization_charge(&mut usage, bytes, limits)?;
        }
        // Borrowed-only admission first: no Facts, Proof or diagnostic source
        // key is cloned until all described logical staging copies fit.
        for (facts, _) in lots {
            initialization_charge(&mut usage, size_of::<(Facts, NonZeroU32)>(), limits)?;
            source_fact_copies(
                facts,
                self.cohort.len(),
                policy.sha256().len(),
                &mut usage,
                limits,
            )?;
        }
        let (_, _, final_next_item) =
            self.source_inventory_capacity(owner, lots.len(), usage.links, usage.extra_bytes)?;
        let mut totals = BTreeMap::<&FormKey, u64>::new();
        for (facts, count) in lots {
            let total = match totals.entry(&facts.base) {
                Entry::Vacant(entry) => {
                    for bytes in [
                        size_of::<(&FormKey, u64)>(),
                        size_of::<(FormKey, u64)>(),
                        facts.base.origin_plugin.len(),
                        size_of::<(ReferenceId, FormKey)>(),
                        facts.base.origin_plugin.len(),
                    ] {
                        initialization_charge(&mut usage, bytes, limits)?;
                    }
                    if self.count_total(owner, &facts.base) != 0 {
                        return Err(Error::Invalid(
                            "uninitialized inventory count index inconsistent".into(),
                        )
                        .into());
                    }
                    entry.insert(0)
                }
                Entry::Occupied(entry) => entry.into_mut(),
            };
            *total = initialization_count(*total, u64::from(count.get()))?;
        }
        let mut proofs = Vec::with_capacity(lots.len());
        for (facts, _) in lots {
            proofs.push(crate::source_items::validate(self, content, policy, facts)?);
        }
        Ok(StagedSourceInventory {
            epoch: self.epoch,
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            policy_sha256: policy.sha256().into(),
            revision: self.revision,
            next_item: self.next_item,
            final_next_item,
            owner,
            authored: authored.cloned(),
            lots: lots.to_vec(),
            proofs,
            counts: totals
                .into_iter()
                .map(|(key, total)| (key.clone(), total))
                .collect(),
            usage,
        })
    }
    pub fn commit_source_inventory_initialization(
        &mut self,
        content: &crate::foreign::Content,
        policy: &crate::source_items::Policy,
        stage: StagedSourceInventory,
    ) -> crate::source_items::Result<SourceInventoryReceipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle.into());
        }
        content.validate_world(self)?;
        if stage.campaign != self.campaign || stage.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged.into());
        }
        if stage.policy_sha256 != policy.sha256() {
            return Err(crate::source_items::Failure::Policy(
                "initialization policy changed",
            ));
        }
        if stage.revision != self.revision
            || stage.next_item != self.next_item
            || self.reference_origin(stage.owner)? != stage.authored.as_ref()
        {
            return Err(
                Error::Invalid("source inventory initialization boundary changed".into()).into(),
            );
        }
        let (links, bytes, next_item) = self.source_inventory_capacity(
            stage.owner,
            stage.lots.len(),
            stage.usage.links,
            stage.usage.extra_bytes,
        )?;
        if next_item != stage.final_next_item {
            return Err(Error::Invalid("source inventory allocator changed".into()).into());
        }
        for (base, _) in &stage.counts {
            if self.count_total(stage.owner, base) != 0 {
                return Err(
                    Error::Invalid("uninitialized inventory count index changed".into()).into(),
                );
            }
        }
        let revision = self.next_revision()?;
        let mut items = Vec::with_capacity(stage.lots.len());
        let mut item_ids = Vec::with_capacity(stage.lots.len());
        let mut next = stage.next_item;
        // Allocate local rows/IDs before publishing; every fallible calculation
        // is complete before the bank or any canonical counter changes.
        for (facts, count) in stage.lots {
            let id = ItemId(
                NonZeroU64::new(next)
                    .ok_or_else(|| Error::Invalid("zero item allocator".into()))?,
            );
            next = next
                .checked_add(1)
                .ok_or(Error::Capacity("item identities"))?;
            if self.items.contains_key(&id) {
                return Err(
                    Error::Invalid("source inventory item identity occupied".into()).into(),
                );
            }
            items.push(Item {
                id,
                owner: stage.owner,
                count,
                facts,
            });
            item_ids.push(id);
        }
        let bank = item_ids.iter().copied().collect();
        let before_revision = self.revision;
        self.inventory_banks.insert(stage.owner, bank);
        for item in items {
            self.items.insert(item.id, item);
        }
        for (base, total) in stage.counts {
            self.set_count_total(stage.owner, base, total);
        }
        self.next_item = next_item;
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(SourceInventoryReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.catalogue_sha256,
            policy_sha256: stage.policy_sha256,
            before_revision,
            after_revision: revision,
            owner: stage.owner,
            item_ids,
            proofs: stage.proofs,
            usage: stage.usage,
        })
    }
    /// Two bounded BTreeSet ranges: precharge borrowed rows, then copy only the
    /// admitted slice. No Snapshot, whole-bank view, skipped prefix or retained
    /// temporary row/key list is materialized.
    pub fn inventory_page(&self, request: PageRequest<'_>, limits: PageLimits) -> Result<Page> {
        if let Some(cursor) = request.after {
            cursor.check(
                self.epoch,
                self.campaign,
                &self.cohort,
                self.revision,
                request.owner,
            )?;
        }
        let authored = self.reference_origin(request.owner)?;
        if request.rows == 0 || request.rows > limits.max_rows {
            return Err(Error::Capacity("inventory page rows"));
        }
        let start_after = request.after.and_then(|cursor| cursor.consumed);
        // Bound::Excluded avoids adding one to a potentially maximal ItemId.
        let start = start_after.map_or(Bound::Unbounded, Bound::Excluded);
        let bank = self.inventory_banks.get(&request.owner);
        let mut usage = PageUsage::default();
        for bytes in [
            size_of::<Page>(),
            self.cohort.len(),
            self.cohort.len(),
            authored.map_or(0, |key| key.origin_plugin.len()),
        ] {
            page_charge(&mut usage, bytes, limits)?;
        }
        let mut consumed = start_after;
        if let Some(ids) = bank {
            for id in ids.range((start, Bound::Unbounded)).take(request.rows) {
                usage.visited = usage
                    .visited
                    .checked_add(1)
                    .ok_or(Error::Capacity("inventory page visited"))?;
                if usage.visited > limits.max_visited {
                    return Err(Error::Capacity("inventory page visited"));
                }
                let item = self.item(*id)?;
                if item.owner != request.owner || item.id != *id {
                    return Err(Error::Invalid(
                        "inventory page bank membership inconsistent".into(),
                    ));
                }
                page_fact_copies(&item.facts, &mut usage, limits)?;
                usage.returned += 1;
                consumed = Some(*id);
            }
        }
        let complete = bank.is_none_or(|ids| ids.last().is_none_or(|last| Some(*last) == consumed));
        let items = bank.map(|ids| {
            let mut items = Vec::with_capacity(usage.returned);
            for id in ids.range((start, Bound::Unbounded)).take(usage.returned) {
                items.push(self.items[id].clone());
            }
            items
        });
        Ok(Page {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            state_revision: self.revision,
            boundary: self.clocks,
            owner: request.owner,
            authored: authored.cloned(),
            start_after,
            items,
            complete,
            usage,
            cursor: Cursor {
                epoch: self.epoch,
                campaign: self.campaign,
                catalogue_sha256: self.cohort.clone(),
                revision: self.revision,
                owner: request.owner,
                consumed,
            },
        })
    }
    /// Scan limits before cloning any source key, opaque payload or item vector.
    /// The live bank remains the only authority; observations do not initialize it.
    pub fn inventory_view(&self, owner: ReferenceId, limits: ViewLimits) -> Result<InventoryView> {
        let authored = self.reference_origin(owner)?;
        let bank = self.inventory_banks.get(&owner);
        let mut usage = ViewUsage::default();
        if let Some(ids) = bank {
            if ids.len() > limits.max_items {
                return Err(Error::Capacity("inventory view items"));
            }
            usage.items = ids.len();
            for id in ids {
                let facts = self.item(*id)?.facts();
                usage.links = usage
                    .links
                    .checked_add(facts.links())
                    .ok_or(Error::Capacity("inventory view links"))?;
                if usage.links > limits.max_links {
                    return Err(Error::Capacity("inventory view links"));
                }
                usage.extra_bytes = usage
                    .extra_bytes
                    .checked_add(facts.extra_bytes()?)
                    .ok_or(Error::Capacity("inventory view extra bytes"))?;
                if usage.extra_bytes > limits.max_extra_bytes {
                    return Err(Error::Capacity("inventory view extra bytes"));
                }
            }
        }
        Ok(InventoryView {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            state_revision: self.revision,
            boundary: self.clocks,
            owner,
            authored: authored.cloned(),
            items: bank.map(|ids| ids.iter().map(|id| self.items[id].clone()).collect()),
            usage,
        })
    }
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
        self.item_capacity_changes(
            f.links(),
            f.extra_bytes()?,
            replacing.map_or(0, Facts::links),
            replacing.map(Facts::extra_bytes).transpose()?.unwrap_or(0),
        )
    }
    fn item_capacity_changes(
        &self,
        added_links: usize,
        added_bytes: usize,
        removed_links: usize,
        removed_bytes: usize,
    ) -> Result<(usize, usize)> {
        let links = self
            .item_links
            .checked_sub(removed_links)
            .and_then(|n| n.checked_add(added_links))
            .ok_or(Error::Capacity("total item links"))?;
        let bytes = self
            .item_bytes
            .checked_sub(removed_bytes)
            .and_then(|n| n.checked_add(added_bytes))
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

#[derive(Debug, Clone, Copy)]
pub struct RemovalLimits {
    pub max_rows: usize,
    /// Logical metadata and described lookup/duplicate copies. Facts and opaque
    /// payloads are never cloned; allocator overhead and process peak are excluded.
    pub max_copied_bytes: usize,
}
impl Default for RemovalLimits {
    fn default() -> Self {
        Self {
            max_rows: 256,
            max_copied_bytes: 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct RemovalUsage {
    pub rows: usize,
    pub removed_lots: usize,
    pub quantity: u64,
    pub released_links: usize,
    pub released_extra_bytes: usize,
    pub copied_bytes: usize,
}
/// Observed metadata only. A later Facts mutation changes the global revision
/// and expires the stage without retaining any condition/opaque payload copy.
#[derive(Debug, Serialize)]
pub struct RemovalRow {
    id: ItemId,
    owner: ReferenceId,
    base: FormKey,
    before: NonZeroU32,
    requested: NonZeroU32,
    remaining: u32,
    links: usize,
    extra_bytes: usize,
}
impl RemovalRow {
    pub fn id(&self) -> ItemId {
        self.id
    }
    pub fn owner(&self) -> ReferenceId {
        self.owner
    }
    pub fn base(&self) -> &FormKey {
        &self.base
    }
    pub fn before(&self) -> u32 {
        self.before.get()
    }
    pub fn requested(&self) -> u32 {
        self.requested.get()
    }
    pub fn remaining(&self) -> u32 {
        self.remaining
    }
    pub fn links(&self) -> usize {
        self.links
    }
    pub fn extra_bytes(&self) -> usize {
        self.extra_bytes
    }
}
#[derive(Debug, Serialize)]
pub struct RemovalCountChange {
    owner: ReferenceId,
    base: FormKey,
    before: u64,
    after: u64,
}
impl RemovalCountChange {
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
#[must_use = "staging removes nothing; commit the removals or drop them"]
pub struct StagedInventoryRemovals {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    next_item: u64,
    rows: Vec<RemovalRow>,
    counts: Vec<RemovalCountChange>,
    usage: RemovalUsage,
}
impl StagedInventoryRemovals {
    pub fn rows(&self) -> &[RemovalRow] {
        &self.rows
    }
    pub fn count_changes(&self) -> &[RemovalCountChange] {
        &self.counts
    }
    pub fn usage(&self) -> RemovalUsage {
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
        if self.campaign != campaign || self.catalogue_sha256 != cohort {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != revision || self.next_item != next_item {
            return Err(Error::Invalid("inventory removal boundary changed".into()));
        }
        Ok(())
    }
}
/// Publication observations, with no deserialization or mutation authority.
#[derive(Debug, Serialize)]
pub struct RemovalReceipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    before_revision: u64,
    after_revision: u64,
    rows: Vec<RemovalRow>,
    counts: Vec<RemovalCountChange>,
    usage: RemovalUsage,
}
impl RemovalReceipt {
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
    pub fn rows(&self) -> &[RemovalRow] {
        &self.rows
    }
    pub fn count_changes(&self) -> &[RemovalCountChange] {
        &self.counts
    }
    pub fn usage(&self) -> RemovalUsage {
        self.usage
    }
}
struct RemovalDelta {
    before: u64,
    removed: u64,
}
fn removal_charge(usage: &mut RemovalUsage, bytes: usize, limits: RemovalLimits) -> Result<()> {
    usage.copied_bytes = usage
        .copied_bytes
        .checked_add(bytes)
        .ok_or(Error::Capacity("inventory removal copied bytes"))?;
    if usage.copied_bytes > limits.max_copied_bytes {
        return Err(Error::Capacity("inventory removal copied bytes"));
    }
    Ok(())
}
fn removal_sum(total: u64, added: u64) -> Result<u64> {
    total
        .checked_add(added)
        .ok_or(Error::Capacity("inventory removal quantity"))
}
fn removal_count_after(before: u64, removed: u64) -> Result<u64> {
    before
        .checked_sub(removed)
        .ok_or_else(|| Error::Invalid("inventory removal count index inconsistent".into()))
}
impl World<'_> {
    fn removal_membership(&self, item: &Item) -> Result<()> {
        self.inventory_owner(item.owner)?;
        if !self.inventory_banks[&item.owner].contains(&item.id) {
            return Err(Error::Invalid(
                "inventory removal bank membership inconsistent".into(),
            ));
        }
        Ok(())
    }
    /// Caller order selects the exact lots. Empty/duplicate requests refuse;
    /// valid nonzero removals publish once and never choose or merge survivors.
    pub fn stage_inventory_removals(
        &self,
        removals: &[(ItemHandle, NonZeroU32)],
        limits: RemovalLimits,
    ) -> Result<StagedInventoryRemovals> {
        if removals.is_empty() {
            return Err(Error::Invalid("empty inventory removals".into()));
        }
        if removals.len() > limits.max_rows {
            return Err(Error::Capacity("inventory removal rows"));
        }
        let mut usage = RemovalUsage {
            rows: removals.len(),
            ..RemovalUsage::default()
        };
        for bytes in [
            size_of::<StagedInventoryRemovals>(),
            self.cohort.len(),
            size_of::<BTreeSet<ItemId>>(),
            size_of::<BTreeMap<(ReferenceId, &FormKey), RemovalDelta>>(),
        ] {
            removal_charge(&mut usage, bytes, limits)?;
        }
        // Admit metadata for every borrowed lot before cloning any source key.
        // Checking exact quantity/link/byte observations needs no owned Facts.
        for &(handle, requested) in removals {
            let item = self.item_by_handle(handle)?;
            self.removal_membership(item)?;
            if requested.get() > item.count.get() {
                return Err(Error::Invalid("removal exceeds item quantity".into()));
            }
            for bytes in [
                size_of::<RemovalRow>(),
                item.facts.base.origin_plugin.len(),
                size_of::<ItemId>(),
            ] {
                removal_charge(&mut usage, bytes, limits)?;
            }
            usage.quantity = removal_sum(usage.quantity, u64::from(requested.get()))?;
            if requested == item.count {
                usage.removed_lots = usage
                    .removed_lots
                    .checked_add(1)
                    .ok_or(Error::Capacity("inventory removal rows"))?;
                usage.released_links = usage
                    .released_links
                    .checked_add(item.facts.links())
                    .ok_or(Error::Capacity("inventory removal links"))?;
                usage.released_extra_bytes = usage
                    .released_extra_bytes
                    .checked_add(item.facts.extra_bytes()?)
                    .ok_or(Error::Capacity("inventory removal extra bytes"))?;
            }
        }
        let mut unique = BTreeSet::new();
        let mut deltas = BTreeMap::<(ReferenceId, &FormKey), RemovalDelta>::new();
        for &(handle, requested) in removals {
            let item = self.item_by_handle(handle)?;
            if !unique.insert(item.id) {
                return Err(Error::Invalid("duplicate inventory removal item".into()));
            }
            let delta = match deltas.entry((item.owner, &item.facts.base)) {
                Entry::Vacant(entry) => {
                    for bytes in [
                        size_of::<((ReferenceId, &FormKey), RemovalDelta)>(),
                        size_of::<RemovalCountChange>(),
                        item.facts.base.origin_plugin.len(),
                        size_of::<(ReferenceId, FormKey)>(),
                        item.facts.base.origin_plugin.len(),
                    ] {
                        removal_charge(&mut usage, bytes, limits)?;
                    }
                    entry.insert(RemovalDelta {
                        before: self.count_total(item.owner, &item.facts.base),
                        removed: 0,
                    })
                }
                Entry::Occupied(entry) => entry.into_mut(),
            };
            delta.removed = removal_sum(delta.removed, u64::from(requested.get()))?;
        }
        for delta in deltas.values() {
            removal_count_after(delta.before, delta.removed)?;
        }
        self.item_capacity_changes(0, 0, usage.released_links, usage.released_extra_bytes)?;
        self.next_revision()?;
        let counts = deltas
            .into_iter()
            .map(|((owner, base), delta)| {
                Ok(RemovalCountChange {
                    owner,
                    base: base.clone(),
                    before: delta.before,
                    after: removal_count_after(delta.before, delta.removed)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let rows = removals
            .iter()
            .map(|&(handle, requested)| {
                let item = &self.items[&handle.id];
                Ok(RemovalRow {
                    id: item.id,
                    owner: item.owner,
                    base: item.facts.base.clone(),
                    before: item.count,
                    requested,
                    remaining: item.count.get() - requested.get(),
                    links: item.facts.links(),
                    extra_bytes: item.facts.extra_bytes()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(StagedInventoryRemovals {
            epoch: self.epoch,
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            next_item: self.next_item,
            rows,
            counts,
            usage,
        })
    }
    pub fn commit_inventory_removals(
        &mut self,
        stage: StagedInventoryRemovals,
    ) -> Result<RemovalReceipt> {
        stage.check(
            self.epoch,
            self.campaign,
            &self.cohort,
            self.revision,
            self.next_item,
        )?;
        for row in &stage.rows {
            let item = self.item(row.id)?;
            if item.owner != row.owner
                || item.count != row.before
                || item.facts.base != row.base
                || item.facts.links() != row.links
                || item.facts.extra_bytes()? != row.extra_bytes
            {
                return Err(Error::Invalid("inventory removal lot changed".into()));
            }
            self.removal_membership(item)?;
        }
        for count in &stage.counts {
            if self.count_total(count.owner, &count.base) != count.before {
                return Err(Error::Invalid(
                    "inventory removal count observation changed".into(),
                ));
            }
        }
        let (links, bytes) = self.item_capacity_changes(
            0,
            0,
            stage.usage.released_links,
            stage.usage.released_extra_bytes,
        )?;
        let revision = self.next_revision()?;
        let before_revision = self.revision;
        // All fallible admission is complete. Partial survivors retain their
        // exact Facts; full removals release only their original lot charges.
        for row in &stage.rows {
            if let Some(remaining) = NonZeroU32::new(row.remaining) {
                self.items.get_mut(&row.id).expect("observed lot").count = remaining;
            } else {
                self.items.remove(&row.id);
                self.inventory_banks
                    .get_mut(&row.owner)
                    .expect("observed bank")
                    .remove(&row.id);
            }
        }
        for count in &stage.counts {
            self.set_count_total(count.owner, count.base.clone(), count.after);
        }
        self.item_links = links;
        self.item_bytes = bytes;
        self.revision = revision;
        Ok(RemovalReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.catalogue_sha256,
            before_revision,
            after_revision: revision,
            rows: stage.rows,
            counts: stage.counts,
            usage: stage.usage,
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

#[cfg(test)]
mod page_tests {
    use super::*;
    #[test]
    fn opaque_cursor_checks_every_binding_before_copy_admission() {
        let campaign = CampaignId::from_bytes([1; 16]).unwrap();
        let owner = ReferenceId(1.try_into().unwrap());
        let cursor = Cursor {
            epoch: 7,
            campaign,
            catalogue_sha256: "a".repeat(64),
            revision: 9,
            owner,
            consumed: Some(ItemId(5.try_into().unwrap())),
        };
        assert!(cursor.check(7, campaign, &"a".repeat(64), 9, owner).is_ok());
        assert!(matches!(
            cursor.check(8, campaign, &"a".repeat(64), 9, owner),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            cursor.check(
                7,
                CampaignId::from_bytes([2; 16]).unwrap(),
                &"a".repeat(64),
                9,
                owner
            ),
            Err(Error::DefinitionChanged)
        ));
        assert!(matches!(
            cursor.check(7, campaign, &"b".repeat(64), 9, owner),
            Err(Error::DefinitionChanged)
        ));
        assert!(matches!(
            cursor.check(7, campaign, &"a".repeat(64), 10, owner),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            cursor.check(
                7,
                campaign,
                &"a".repeat(64),
                9,
                ReferenceId(2.try_into().unwrap())
            ),
            Err(Error::Invalid(_))
        ));
    }
    #[test]
    fn copied_arithmetic_and_excluded_maximum_cursor_do_not_wrap() {
        assert!(page_add(1, usize::MAX).is_err());
        assert!(page_table(usize::MAX, 2).is_err());
        let last = ItemId(NonZeroU64::new(u64::MAX).unwrap());
        let ids = std::collections::BTreeSet::from([last]);
        assert_eq!(
            ids.range((Bound::Excluded(last), Bound::Unbounded)).count(),
            0
        );
    }
}

#[cfg(test)]
mod source_inventory_tests {
    use super::*;
    #[test]
    fn logical_staging_copy_arithmetic_refuses_addition_and_table_overflow() {
        let limits = SourceInventoryLimits {
            max_copied_bytes: usize::MAX,
            ..SourceInventoryLimits::default()
        };
        let mut usage = SourceInventoryUsage {
            copied_bytes: 1,
            ..SourceInventoryUsage::default()
        };
        assert!(initialization_charge(&mut usage, usize::MAX, limits).is_err());
        assert!(
            initialization_table(&mut SourceInventoryUsage::default(), usize::MAX, 2, limits)
                .is_err()
        );
    }
    #[test]
    fn cumulative_counts_and_whole_id_span_do_not_wrap() {
        assert_eq!(initialization_count(u64::MAX - 1, 1).unwrap(), u64::MAX);
        assert!(initialization_count(u64::MAX - 1, 2).is_err());
        assert_eq!(initialization_next_item(u64::MAX - 2, 2).unwrap(), u64::MAX);
        assert!(initialization_next_item(u64::MAX - 2, 3).is_err());
        assert_eq!(initialization_next_item(u64::MAX, 0).unwrap(), u64::MAX);
        assert!(initialization_next_item(0, 0).is_err());
    }
    #[test]
    fn source_facts_net_counts_accept_full_swap_and_refuse_overflow_or_underflow() {
        assert_eq!(source_facts_count_after(u64::MAX, 1, 1).unwrap(), u64::MAX);
        assert_eq!(
            source_facts_count_after(u64::MAX, u64::MAX, u64::MAX).unwrap(),
            u64::MAX
        );
        assert!(source_facts_count_after(u64::MAX, 0, 1).is_err());
        assert!(source_facts_count_after(0, 1, 0).is_err());
    }
}

#[cfg(test)]
mod removal_tests {
    use super::*;
    #[test]
    fn aggregate_quantity_and_count_subtraction_boundaries_never_wrap() {
        assert_eq!(removal_sum(u64::MAX - 1, 1).unwrap(), u64::MAX);
        assert!(removal_sum(u64::MAX, 1).is_err());
        assert_eq!(removal_count_after(u64::MAX, u64::MAX).unwrap(), 0);
        assert!(removal_count_after(0, 1).is_err());
    }
    #[test]
    fn logical_removal_metadata_addition_refuses_overflow() {
        let mut usage = RemovalUsage {
            copied_bytes: 1,
            ..RemovalUsage::default()
        };
        assert!(
            removal_charge(
                &mut usage,
                usize::MAX,
                RemovalLimits {
                    max_rows: usize::MAX,
                    max_copied_bytes: usize::MAX
                }
            )
            .is_err()
        );
    }
    #[test]
    fn every_private_removal_binding_is_checked_before_lot_admission() {
        let campaign = CampaignId::from_bytes([1; 16]).unwrap();
        let stage = StagedInventoryRemovals {
            epoch: 7,
            campaign,
            catalogue_sha256: "a".repeat(64),
            revision: 9,
            next_item: 4,
            rows: Vec::new(),
            counts: Vec::new(),
            usage: RemovalUsage::default(),
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
