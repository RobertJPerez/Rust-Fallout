//! Explicit direct source occurrences applied only to a new private candidate.
use crate::{
    World,
    foreign::Content,
    identity::ReferenceId,
    inventory::{Facts, ItemId},
    snapshot::Snapshot,
};
use fallout_data::{actors, identity::FormKey, inventory, loaded_scripts, store::SourceReceipt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    num::NonZeroU32,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_field_visits: usize,
    pub max_lots: usize,
    pub max_fact_links: usize,
    pub max_extra_bytes: usize,
    pub max_snapshot_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_field_visits: 200_000,
            max_lots: 4096,
            max_fact_links: 100_000,
            max_extra_bytes: 16 * 1024 * 1024,
            max_snapshot_bytes: 32 * 1024 * 1024,
            max_projection_bytes: 64 * 1024 * 1024,
        }
    }
}
/// All host counts/facts are explicit engineering input. A claim concerns the
/// separately retained source word only; it never derives the host count.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub field_index: usize,
    pub host_count: NonZeroU32,
    pub facts: Facts,
    pub source_count_claim: Option<i32>,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor inventory boot source or input snapshot changed")]
    ContextChanged,
    #[error("actor inventory boot source unavailable: {0}")]
    Source(&'static str),
    #[error("actor inventory boot owner inventory already initialized")]
    AlreadyInitialized,
    #[error("actor inventory boot {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}
#[derive(Debug, Serialize)]
struct Entry {
    choice: Choice,
    source_count: i32,
}
/// Construction authority stays private and cannot be deserialized. A plan
/// pins a complete canonical input snapshot and has no live-world apply method.
#[derive(Debug, Serialize)]
pub struct Plan<'a> {
    source_definition: &'a inventory::Definition,
    owner: ReferenceId,
    owner_authored: Option<FormKey>,
    input_snapshot_sha256: String,
    entries: Vec<Entry>,
    field_visits: usize,
    fact_links: usize,
    extra_bytes: usize,
    #[serde(skip)]
    limits: Limits,
}
#[derive(Debug, Serialize)]
pub struct Mapping {
    pub source_field_index: usize,
    pub source_signed_count: i32,
    pub explicit_host_count: u32,
    pub item: ItemId,
}
#[derive(Debug, Serialize)]
pub struct BootResult<'a> {
    pub source_definition: &'a inventory::Definition,
    pub owner: ReferenceId,
    pub owner_authored: Option<FormKey>,
    pub input_snapshot_sha256: String,
    pub candidate_snapshot: Snapshot,
    pub mappings: Vec<Mapping>,
    pub faithful_initialization_supported: bool,
    pub scope: &'static str,
}
struct Admission {
    bytes: usize,
    maximum: usize,
    hash: Sha256,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("inventory boot byte budget"));
        }
        self.bytes += bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn digest(value: &impl Serialize, maximum: usize, label: &'static str) -> Result<String, Error> {
    let mut admission = Admission {
        bytes: 0,
        maximum,
        hash: Sha256::new(),
    };
    serde_json::to_writer(&mut admission, value).map_err(|_| Error::Capacity(label))?;
    Ok(format!("{:x}", admission.hash.finalize()))
}
fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}

pub fn prepare<'a>(
    world: &World<'_>,
    content: &Content,
    actors: &'a actors::Catalogue<'a>,
    root: &FormKey,
    owner: ReferenceId,
    choices: &[Choice],
    limits: Limits,
) -> Result<Plan<'a>, Error> {
    content.validate_world(world)?;
    admit(actors.sources().len(), limits.max_sources, "source")?;
    if !same_sources(actors.sources(), &world.catalogue().sources)
        || actors.winning_content_sha256() != world.catalogue().winning_content_sha256()
    {
        return Err(Error::ContextChanged);
    }
    let actor = actors
        .get(root)
        .ok_or(Error::Source("actor root missing"))?;
    let source = actor.inventory_definition();
    if actor.deleted || source.deleted {
        return Err(Error::Source("actor root deleted"));
    }
    let header = content.source_form(world, root)?;
    if !matches!(&header.kind, b"NPC_" | b"CREA")
        || header.kind != actor.kind
        || source.key != *root
        || header.flags != source.source.record_flags
    {
        return Err(Error::Source("actor source identity"));
    }
    let owner_authored = world.reference_origin(owner)?.cloned();
    if world.inventory_items(owner).is_ok() {
        return Err(Error::AlreadyInitialized);
    }
    let field_visits = source
        .fields
        .len()
        .checked_add(source.items.len())
        .and_then(|n| n.checked_add(choices.len()))
        .ok_or(Error::Capacity("field visit"))?;
    admit(field_visits, limits.max_field_visits, "field visit")?;
    admit(choices.len(), limits.max_lots, "lot")?;
    if choices.is_empty() {
        return Err(Error::Source("no explicit direct source occurrences"));
    }
    let mut selected = BTreeSet::new();
    let mut occurrences = BTreeMap::<usize, usize>::new();
    for item in &source.items {
        *occurrences.entry(item.cnto_field).or_default() += 1;
    }
    let mut entries = Vec::new();
    let mut fact_links = 0usize;
    let mut extra_bytes = 0usize;
    for choice in choices {
        if !selected.insert(choice.field_index) {
            return Err(Error::Source("duplicate physical selection"));
        }
        if occurrences.get(&choice.field_index) != Some(&1) {
            return Err(Error::Source("selection is not one physical CNTO"));
        }
        let field = source
            .fields
            .get(choice.field_index)
            .ok_or(Error::Source("source field missing"))?;
        let inventory::Value::Item {
            item,
            count,
            schema_kind_allowed,
        } = &field.value
        else {
            return Err(Error::Source("source field is not CNTO"));
        };
        if field.kind != *b"CNTO"
            || item.status != inventory::Status::Defined
            || *schema_kind_allowed != Some(true)
        {
            return Err(Error::Source("item binding unavailable"));
        }
        let base = item
            .key
            .as_ref()
            .ok_or(Error::Source("item base missing"))?;
        let target = item
            .target
            .as_ref()
            .ok_or(Error::Source("item header missing"))?;
        if matches!(&target.kind, b"LVLI" | b"LVLC" | b"LVLN") {
            return Err(Error::Source("leveled item requires unsupported selection"));
        }
        let form = content.source_form(world, base)?;
        if form.kind != target.kind
            || form.flags != target.record_flags
            || choice.facts.base != *base
        {
            return Err(Error::Source("explicit lot base differs"));
        }
        if choice
            .source_count_claim
            .is_some_and(|claimed| claimed != *count)
        {
            return Err(Error::Source("signed source count claim differs"));
        }
        world.validate_item_facts(&choice.facts)?;
        fact_links = fact_links
            .checked_add(choice.facts.links())
            .ok_or(Error::Capacity("fact link"))?;
        extra_bytes = extra_bytes
            .checked_add(choice.facts.extra_bytes()?)
            .ok_or(Error::Capacity("extra byte"))?;
        admit(fact_links, limits.max_fact_links, "fact link")?;
        admit(extra_bytes, limits.max_extra_bytes, "extra byte")?;
        entries.push(Entry {
            choice: choice.clone(),
            source_count: *count,
        });
    }
    let input_snapshot_sha256 = digest(
        &world.snapshot(),
        limits.max_snapshot_bytes,
        "input snapshot byte",
    )?;
    let result = Plan {
        source_definition: source,
        owner,
        owner_authored,
        input_snapshot_sha256,
        entries,
        field_visits,
        fact_links,
        extra_bytes,
        limits,
    };
    digest(&result, limits.max_projection_bytes, "plan projection byte")?;
    Ok(result)
}
impl<'a> Plan<'a> {
    /// Strict restore always constructs a new candidate. Every operation can
    /// fail; a late failure drops it and returns no partial candidate snapshot.
    pub fn apply_private(
        self,
        scripts: &loaded_scripts::Catalogue,
        content: &Content,
        input: &Snapshot,
        world_limits: crate::Limits,
    ) -> Result<BootResult<'a>, Error> {
        if digest(input, self.limits.max_snapshot_bytes, "input snapshot byte")?
            != self.input_snapshot_sha256
        {
            return Err(Error::ContextChanged);
        }
        let mut candidate = World::restore(scripts, input.clone(), world_limits)?;
        content.validate_world(&candidate)?;
        candidate.reference_origin(self.owner)?;
        if candidate.inventory_items(self.owner).is_ok() {
            return Err(Error::AlreadyInitialized);
        }
        candidate.initialize_inventory(self.owner)?;
        let mut mappings = Vec::new();
        for entry in self.entries {
            let source_field_index = entry.choice.field_index;
            let explicit_host_count = entry.choice.host_count.get();
            let item =
                candidate.add_item(self.owner, entry.choice.facts, entry.choice.host_count)?;
            mappings.push(Mapping {
                source_field_index,
                source_signed_count: entry.source_count,
                explicit_host_count,
                item,
            });
        }
        let candidate_snapshot = candidate.snapshot();
        digest(
            &candidate_snapshot,
            self.limits.max_snapshot_bytes,
            "candidate snapshot byte",
        )?;
        let result = BootResult {
            source_definition: self.source_definition,
            owner: self.owner,
            owner_authored: self.owner_authored,
            input_snapshot_sha256: self.input_snapshot_sha256,
            candidate_snapshot,
            mappings,
            faithful_initialization_supported: false,
            scope: "Private engineering boot from explicit physical direct CNTO choices and explicit positive host counts/facts; source signed counts and COED declarations retained separately, no live partial mutation, template inheritance, leveled rolls, respawn, default ammo/equipment or faithful actor initialization",
        };
        digest(
            &result,
            self.limits.max_projection_bytes,
            "result projection byte",
        )?;
        Ok(result)
    }
}
