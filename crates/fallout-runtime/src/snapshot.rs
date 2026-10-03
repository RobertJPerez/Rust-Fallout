//! Versioned, bounded canonical snapshots. This is native runtime state, not a
//! Bethesda save reader. Restores build a new world and validate everything
//! before replacing the caller's state.
use crate::{
    Error, Result,
    events::{Clocks, Context, Pending},
    identity::{InstanceId, Owner, ReferenceId, Value},
    state::{Instance, Limits, Slot, World},
};
use fallout_data::{
    identity::{FormKey, ProfileId, plugin_name},
    loaded_scripts::{Catalogue, Handle},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: ReferenceId,
    pub authored: Option<FormKey>,
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
    pub profile: ProfileId,
    pub catalogue_sha256: String,
    pub next_instance: u64,
    pub next_reference: u64,
    pub next_event_sequence: u64,
    pub clocks: Clocks,
    pub references: Vec<Reference>,
    pub instances: Vec<ScriptInstance>,
    pub pending_events: Vec<Pending>,
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
        let snapshot: Self = serde_json::from_slice(bytes)?;
        snapshot.check_budgets(limits)?;
        Ok(snapshot)
    }
    fn check_budgets(&self, limits: Limits) -> Result<()> {
        if self.references.len() > limits.max_references {
            return Err(Error::Capacity("saved references"));
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
        }
    }
    pub fn restore(catalogue: &'a Catalogue, snapshot: Snapshot, limits: Limits) -> Result<Self> {
        snapshot.check_budgets(limits)?;
        if snapshot.schema_version != SCHEMA_VERSION || snapshot.profile != ProfileId::NvOriginal {
            return Err(Error::Invalid(
                "snapshot schema or profile is unsupported".into(),
            ));
        }
        let mut world = Self::new(catalogue, limits)?;
        if snapshot.catalogue_sha256 != world.cohort {
            return Err(Error::DefinitionChanged);
        }
        if snapshot.next_instance == 0
            || snapshot.next_reference == 0
            || snapshot.next_event_sequence == 0
        {
            return Err(Error::Invalid("snapshot allocator cannot be zero".into()));
        }
        world.next_instance = snapshot.next_instance;
        world.next_reference = snapshot.next_reference;
        world.next_sequence = snapshot.next_event_sequence;
        world.clocks = snapshot.clocks;
        for reference in snapshot.references {
            if reference.id.0.get() >= world.next_reference
                || world.references.contains_key(&reference.id)
            {
                return Err(Error::Invalid(
                    "duplicate reference or allocator would reuse an identity".into(),
                ));
            }
            if let Some(key) = &reference.authored {
                crate::identity::valid_form(key)?;
                if world
                    .authored_references
                    .insert(key.clone(), reference.id)
                    .is_some()
                {
                    return Err(Error::Invalid(
                        "duplicate authored reference identity".into(),
                    ));
                }
            }
            world.references.insert(reference.id, reference.authored);
        }
        for saved in snapshot.instances {
            if saved.id.0.get() >= world.next_instance || world.instances.contains_key(&saved.id) {
                return Err(Error::Invalid(
                    "duplicate script instance or allocator would reuse an identity".into(),
                ));
            }
            catalogue
                .get_handle(&saved.definition)
                .ok_or(Error::DefinitionChanged)?;
            world.validate_owner(&saved.owner)?;
            world.validate_context(&saved.context)?;
            if world.owners.contains_key(&saved.owner) {
                return Err(Error::Invalid("duplicate saved script owner".into()));
            }
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
                if locals.insert(local.index, local.value).is_some() {
                    return Err(Error::Invalid("duplicate saved local".into()));
                }
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
        let mut prior_sequence = 0;
        let mut prior_clocks = Clocks::default();
        let mut observed = BTreeSet::new();
        for event in snapshot.pending_events {
            if event.sequence <= prior_sequence
                || event.sequence >= world.next_sequence
                || !observed.insert(event.sequence)
                || !event.arrived.no_later_than(world.clocks)
                || !prior_clocks.no_later_than(event.arrived)
            {
                return Err(Error::Invalid(
                    "saved event order, clocks or allocator is invalid".into(),
                ));
            }
            let instance = world.instance(world.handle(event.instance)?)?;
            Self::validate_trigger(&instance.definition_schema, &event.trigger)?;
            world.validate_context(&event.context)?;
            prior_sequence = event.sequence;
            prior_clocks = event.arrived;
            world.pending.push_back(event);
        }
        Ok(world)
    }
    pub fn replace_from_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        let restored = Self::restore(self.catalogue, snapshot, self.limits)?;
        *self = restored;
        Ok(())
    }
}
