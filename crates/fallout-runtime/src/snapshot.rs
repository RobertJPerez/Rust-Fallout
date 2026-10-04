//! Versioned, bounded canonical snapshots. This is native runtime state, not a
//! Bethesda save reader. Restores build a new world and validate everything
//! before replacing the caller's state.
use crate::{
    Error, Result,
    events::{Clocks, Context, Pending},
    identity::{CampaignId, InstanceId, Owner, ReferenceId, Value},
    state::{Instance, Limits, Slot, World},
};
use fallout_data::{
    identity::{FormKey, ProfileId, plugin_name},
    loaded_scripts::{Catalogue, Handle},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

mod admission;
mod relationships;

pub const SCHEMA_VERSION: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: ReferenceId,
    pub authored: Option<FormKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceState {
    pub id: ReferenceId,
    pub state: crate::reference_state::State,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Local {
    pub index: u32,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptInstance {
    pub id: InstanceId,
    pub definition: Handle,
    pub owner: Owner,
    pub context: Context,
    pub locals: Vec<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub profile: ProfileId,
    pub catalogue_sha256: String,
    pub next_item: u64,
    pub inventory_banks: Vec<crate::inventory::Bank>,
    pub next_instance: u64,
    pub next_reference: u64,
    pub next_event_sequence: u64,
    pub clocks: Clocks,
    pub references: Vec<Reference>,
    pub instances: Vec<ScriptInstance>,
    pub pending_events: Vec<Pending>,
    pub reference_states: Vec<ReferenceState>,
}

/// Checkpoint 28's in-memory schema had no campaign namespace or state revision.
/// This DTO is deliberately separate so duplicate/unknown legacy fields are
/// rejected before migration; parsing through a JSON map would lose duplicates.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySnapshot {
    schema_version: u32,
    profile: ProfileId,
    catalogue_sha256: String,
    next_instance: u64,
    next_reference: u64,
    next_event_sequence: u64,
    clocks: Clocks,
    references: Vec<Reference>,
    instances: Vec<ScriptInstance>,
    pending_events: Vec<Pending>,
}

/// Schema 2 contains script/reference state but no initialized inventory banks.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV2 {
    schema_version: u32,
    campaign: CampaignId,
    state_revision: u64,
    profile: ProfileId,
    catalogue_sha256: String,
    next_instance: u64,
    next_reference: u64,
    next_event_sequence: u64,
    clocks: Clocks,
    references: Vec<Reference>,
    instances: Vec<ScriptInstance>,
    pending_events: Vec<Pending>,
}

/// Schema 3 has initialized inventory but no canonical pose/enable component.
/// Keep the DTO exact: a forged legacy component must never migrate as live state.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV3 {
    schema_version: u32,
    campaign: CampaignId,
    state_revision: u64,
    profile: ProfileId,
    catalogue_sha256: String,
    next_item: u64,
    inventory_banks: Vec<crate::inventory::Bank>,
    next_instance: u64,
    next_reference: u64,
    next_event_sequence: u64,
    clocks: Clocks,
    references: Vec<Reference>,
    instances: Vec<ScriptInstance>,
    pending_events: Vec<Pending>,
}

struct DigestWriter(Sha256);
impl Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn cohort(catalogue: &Catalogue) -> Result<String> {
    let mut sources = catalogue
        .sources
        .iter()
        .map(|source| {
            Ok((
                plugin_name(&source.source_name).map_err(|e| Error::Invalid(e.to_string()))?,
                source.source_bytes,
                &source.source_sha256,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    sources.sort();
    // Winning handles and resolved reference provenance also matter. Equal
    // source sets alone would miss a different override order.
    let scripts: Vec<_> = catalogue
        .iter()
        .map(|(_, script)| (script.handle(), script.references()))
        .collect();
    let mut writer = DigestWriter(Sha256::new());
    writer.0.update(b"FRCSTATE1");
    writer
        .0
        .update(catalogue.winning_content_sha256().as_bytes());
    serde_json::to_writer(&mut writer, &(sources, scripts))?;
    Ok(format!("{:x}", writer.0.finalize()))
}

struct BoundedBytes {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("snapshot byte budget exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Snapshot {
    /// Migration needs an explicit campaign identity. It preserves every old
    /// gameplay field and starts the newly introduced bookkeeping revision at
    /// zero. The result still needs normal content-bound restoration validation.
    pub fn migrate_v1(bytes: &[u8], limits: Limits, campaign: CampaignId) -> Result<Self> {
        if bytes.len() > limits.max_snapshot_bytes {
            return Err(Error::Capacity("legacy snapshot bytes"));
        }
        CampaignId::from_bytes(campaign.bytes())?;
        admission::check(bytes, limits, admission::Schema::V1)?;
        let old: LegacySnapshot = serde_json::from_slice(bytes)?;
        if old.schema_version != 1 || old.profile != ProfileId::NvOriginal {
            return Err(Error::Invalid(
                "legacy snapshot schema/profile is unsupported".into(),
            ));
        }
        let snapshot = Self {
            schema_version: SCHEMA_VERSION,
            campaign,
            state_revision: 0,
            next_item: 1,
            inventory_banks: Vec::new(),
            profile: old.profile,
            catalogue_sha256: old.catalogue_sha256,
            next_instance: old.next_instance,
            next_reference: old.next_reference,
            next_event_sequence: old.next_event_sequence,
            clocks: old.clocks,
            references: old.references,
            instances: old.instances,
            pending_events: old.pending_events,
            reference_states: Vec::new(),
        };
        snapshot.check_budgets(limits)?;
        Ok(snapshot)
    }
    /// Explicit migration keeps schema-2 state intact. Inventory remains
    /// uninitialized; an empty initialized bank would manufacture a live value.
    pub fn migrate_v2(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.max_snapshot_bytes {
            return Err(Error::Capacity("legacy snapshot bytes"));
        }
        admission::check(bytes, limits, admission::Schema::V2)?;
        let old: LegacyV2 = serde_json::from_slice(bytes)?;
        if old.schema_version != 2 || old.profile != ProfileId::NvOriginal {
            return Err(Error::Invalid(
                "legacy schema/profile is unsupported".into(),
            ));
        }
        CampaignId::from_bytes(old.campaign.bytes())?;
        let value = Self {
            schema_version: SCHEMA_VERSION,
            campaign: old.campaign,
            state_revision: old.state_revision,
            profile: old.profile,
            catalogue_sha256: old.catalogue_sha256,
            next_instance: old.next_instance,
            next_reference: old.next_reference,
            next_event_sequence: old.next_event_sequence,
            clocks: old.clocks,
            references: old.references,
            instances: old.instances,
            pending_events: old.pending_events,
            next_item: 1,
            inventory_banks: Vec::new(),
            reference_states: Vec::new(),
        };
        value.check_budgets(limits)?;
        Ok(value)
    }
    /// Explicitly retain schema-3 state. Pose and enable remain unavailable;
    /// source initialization belongs to the caller after source-bound restore.
    pub fn migrate_v3(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.max_snapshot_bytes {
            return Err(Error::Capacity("legacy snapshot bytes"));
        }
        admission::check(bytes, limits, admission::Schema::V3)?;
        let old: LegacyV3 = serde_json::from_slice(bytes)?;
        if old.schema_version != 3 || old.profile != ProfileId::NvOriginal {
            return Err(Error::Invalid(
                "legacy schema/profile is unsupported".into(),
            ));
        }
        CampaignId::from_bytes(old.campaign.bytes())?;
        let value = Self {
            schema_version: SCHEMA_VERSION,
            campaign: old.campaign,
            state_revision: old.state_revision,
            profile: old.profile,
            catalogue_sha256: old.catalogue_sha256,
            next_item: old.next_item,
            inventory_banks: old.inventory_banks,
            next_instance: old.next_instance,
            next_reference: old.next_reference,
            next_event_sequence: old.next_event_sequence,
            clocks: old.clocks,
            references: old.references,
            instances: old.instances,
            pending_events: old.pending_events,
            reference_states: Vec::new(),
        };
        value.check_budgets(limits)?;
        Ok(value)
    }
    pub fn encode(&self, maximum_bytes: usize) -> Result<Vec<u8>> {
        let mut writer = BoundedBytes {
            bytes: Vec::new(),
            maximum: maximum_bytes,
        };
        serde_json::to_writer(&mut writer, self)?;
        Ok(writer.bytes)
    }
    pub fn decode(bytes: &[u8], limits: Limits) -> Result<Self> {
        if bytes.len() > limits.max_snapshot_bytes {
            return Err(Error::Capacity("snapshot bytes"));
        }
        admission::check(bytes, limits, admission::Schema::Current)?;
        let snapshot: Self = serde_json::from_slice(bytes)?;
        snapshot.check_budgets(limits)?;
        Ok(snapshot)
    }
    pub(crate) fn validate_intrinsic(&self, limits: Limits) -> Result<()> {
        self.check_budgets(limits)?;
        relationships::check(self, limits)
    }
    fn check_budgets(&self, limits: Limits) -> Result<()> {
        if self.inventory_banks.len() > limits.max_inventory_banks {
            return Err(Error::Capacity("saved inventory banks"));
        }
        let mut items = 0_usize;
        let mut links = 0_usize;
        let mut bytes = 0_usize;
        for bank in &self.inventory_banks {
            if bank.items.len() > limits.max_item_instances.saturating_sub(items) {
                return Err(Error::Capacity("saved items"));
            }
            items += bank.items.len();
            for item in &bank.items {
                let item_links = item.facts.links();
                let item_bytes = item.facts.extra_bytes()?;
                if item_links > limits.max_item_links
                    || item_bytes > limits.max_item_bytes
                    || item_links > limits.max_total_item_links.saturating_sub(links)
                    || item_bytes > limits.max_total_item_bytes.saturating_sub(bytes)
                {
                    return Err(Error::Capacity("saved item extra state"));
                }
                links += item_links;
                bytes += item_bytes;
            }
        }
        if self.references.len() > limits.max_references {
            return Err(Error::Capacity("saved references"));
        }
        if self.reference_states.len() > limits.max_references {
            return Err(Error::Capacity("saved reference states"));
        }
        if self.instances.len() > limits.max_instances {
            return Err(Error::Capacity("saved instances"));
        }
        if self.pending_events.len() > limits.max_pending_events {
            return Err(Error::Capacity("saved events"));
        }
        let mut locals = 0;
        for instance in &self.instances {
            if instance.locals.len() > limits.max_locals.saturating_sub(locals) {
                return Err(Error::Capacity("saved locals"));
            }
            locals += instance.locals.len();
            if instance.context.arguments.len() > limits.max_event_arguments {
                return Err(Error::Capacity("saved instance context arguments"));
            }
        }
        if self
            .pending_events
            .iter()
            .any(|event| event.context.arguments.len() > limits.max_event_arguments)
        {
            return Err(Error::Capacity("saved event context arguments"));
        }
        Ok(())
    }
}

impl<'a> World<'a> {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            schema_version: SCHEMA_VERSION,
            campaign: self.campaign,
            state_revision: self.revision,
            next_item: self.next_item,
            inventory_banks: self
                .inventory_banks
                .iter()
                .map(|(&owner, ids)| crate::inventory::Bank {
                    owner,
                    items: ids.iter().map(|id| self.items[id].clone()).collect(),
                })
                .collect(),
            profile: ProfileId::NvOriginal,
            catalogue_sha256: self.cohort.clone(),
            next_instance: self.next_instance,
            next_reference: self.next_reference,
            next_event_sequence: self.next_sequence,
            clocks: self.clocks,
            references: self
                .references
                .iter()
                .map(|(&id, authored)| Reference {
                    id,
                    authored: authored.clone(),
                })
                .collect(),
            instances: self
                .instances
                .iter()
                .map(|(_, &slot)| {
                    let instance = self.slots[slot]
                        .value
                        .as_ref()
                        .expect("live instance index");
                    ScriptInstance {
                        id: instance.id,
                        definition: instance.definition.clone(),
                        owner: instance.owner.clone(),
                        context: instance.context.clone(),
                        locals: instance
                            .locals
                            .iter()
                            .map(|(&index, value)| Local {
                                index,
                                value: value.clone(),
                            })
                            .collect(),
                    }
                })
                .collect(),
            pending_events: self.pending.iter().cloned().collect(),
            reference_states: self
                .reference_states
                .iter()
                .map(|(&id, state)| ReferenceState {
                    id,
                    state: state.clone(),
                })
                .collect(),
        }
    }
    pub fn restore(
        catalogue: impl Into<crate::SourceCatalogue<'a>>,
        snapshot: Snapshot,
        limits: Limits,
    ) -> Result<Self> {
        snapshot.check_budgets(limits)?;
        if snapshot.schema_version != SCHEMA_VERSION || snapshot.profile != ProfileId::NvOriginal {
            return Err(Error::Invalid(
                "snapshot schema or profile is unsupported".into(),
            ));
        }
        let mut world = Self::with_campaign(catalogue, limits, snapshot.campaign)?;
        if snapshot.catalogue_sha256 != world.cohort {
            return Err(Error::DefinitionChanged);
        }
        relationships::check(&snapshot, limits)?;
        world.next_instance = snapshot.next_instance;
        world.next_reference = snapshot.next_reference;
        world.next_sequence = snapshot.next_event_sequence;
        world.clocks = snapshot.clocks;
        world.revision = snapshot.state_revision;
        for reference in snapshot.references {
            if let Some(key) = &reference.authored {
                world.authored_references.insert(key.clone(), reference.id);
            }
            world.references.insert(reference.id, reference.authored);
        }
        for saved in snapshot.reference_states {
            world.reference_states.insert(saved.id, saved.state);
        }
        for saved in snapshot.instances {
            world
                .catalogue
                .get_handle(&saved.definition)
                .ok_or(Error::DefinitionChanged)?;
            let definition_schema = world.runtime_definition(&saved.definition)?;
            let schema = &definition_schema.locals;
            if schema.len() != saved.locals.len() {
                return Err(Error::Invalid(
                    "saved local bank does not match its declaration schema".into(),
                ));
            }
            let mut locals = BTreeMap::new();
            for local in saved.locals {
                world.validate_value(
                    schema
                        .get(&local.index)
                        .ok_or(Error::MissingLocal(local.index))?,
                    &local.value,
                )?;
                locals.insert(local.index, local.value);
            }
            // Equal cardinality plus checked unique indices proves full schema
            // coverage. Unknown declarations are retained uninitialized.
            world.local_count += locals.len();
            let slot = world.slots.len();
            world.instances.insert(saved.id, slot);
            world.owners.insert(saved.owner.clone(), saved.id);
            world.slots.push(Slot {
                generation: 1,
                value: Some(Instance {
                    id: saved.id,
                    definition: saved.definition,
                    owner: saved.owner,
                    context: saved.context,
                    definition_schema,
                    locals,
                }),
            });
        }
        world.restore_item_banks(snapshot.inventory_banks, snapshot.next_item)?;
        for event in snapshot.pending_events {
            let instance = world.instance(world.handle(event.instance)?)?;
            Self::validate_trigger(&instance.definition_schema, &event.trigger)?;
            world.pending.push_back(event);
        }
        Ok(world)
    }
    pub fn replace_from_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        let restored = Self::restore(self.catalogue.clone(), snapshot, self.limits)?;
        *self = restored;
        Ok(())
    }
}
