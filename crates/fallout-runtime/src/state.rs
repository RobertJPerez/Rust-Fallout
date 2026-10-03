use crate::{
    Error, Result,
    events::{Clocks, Context, Pending, Trigger},
    identity::{CampaignId, InstanceId, Owner, ReferenceId, ReferenceValue, Value, valid_form},
    schema::{self, Kind, Local},
};
use fallout_data::{
    identity::FormKey,
    loaded_scripts::{Catalogue, Handle, ReferenceStatus, ScriptKey},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT_WORLD: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_instances: usize,
    pub max_references: usize,
    pub max_locals: usize,
    pub max_pending_events: usize,
    pub max_event_blocks: usize,
    pub max_event_arguments: usize,
    pub max_snapshot_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_instances: 65_536,
            max_references: 262_144,
            max_locals: 1_000_000,
            max_pending_events: 65_536,
            max_event_blocks: 1_000_000,
            max_event_arguments: 64,
            max_snapshot_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Transient handles never serialize. A fresh world epoch also prevents a
/// handle from naming a restored slot with the same index and generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceHandle {
    pub(crate) world: u64,
    pub(crate) slot: usize,
    pub(crate) generation: u64,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) id: InstanceId,
    pub(crate) definition: Handle,
    pub(crate) owner: Owner,
    pub(crate) context: Context,
    pub(crate) definition_schema: Arc<DefinitionSchema>,
    pub(crate) locals: BTreeMap<u32, Value>,
}
impl Instance {
    pub fn id(&self) -> InstanceId {
        self.id
    }
    pub fn definition(&self) -> &Handle {
        &self.definition
    }
    pub fn owner(&self) -> &Owner {
        &self.owner
    }
    pub fn context(&self) -> &Context {
        &self.context
    }
    pub fn local(&self, index: u32) -> Result<&Value> {
        let value = self.locals.get(&index).ok_or(Error::MissingLocal(index))?;
        if *value == Value::Uninitialized {
            return Err(Error::UninitializedLocal(index));
        }
        Ok(value)
    }
    pub fn locals(&self) -> &BTreeMap<u32, Value> {
        &self.locals
    }
}

#[derive(Debug)]
pub(crate) struct DefinitionSchema {
    pub locals: BTreeMap<u32, Local>,
    pub blocks: BTreeSet<(u32, u16)>,
}

pub(crate) struct Slot {
    pub generation: u64,
    pub value: Option<Instance>,
}

pub struct World<'a> {
    pub(crate) campaign: CampaignId,
    pub(crate) revision: u64,
    pub(crate) catalogue: &'a Catalogue,
    pub(crate) definitions: BTreeMap<ScriptKey, Arc<DefinitionSchema>>,
    pub(crate) block_count: usize,
    pub(crate) limits: Limits,
    pub(crate) cohort: String,
    pub(crate) epoch: u64,
    pub(crate) slots: Vec<Slot>,
    pub(crate) free: Vec<usize>,
    pub(crate) instances: BTreeMap<InstanceId, usize>,
    pub(crate) owners: BTreeMap<Owner, InstanceId>,
    pub(crate) references: BTreeMap<ReferenceId, Option<FormKey>>,
    pub(crate) authored_references: BTreeMap<FormKey, ReferenceId>,
    pub(crate) next_instance: u64,
    pub(crate) next_reference: u64,
    pub(crate) next_sequence: u64,
    pub(crate) local_count: usize,
    pub(crate) clocks: Clocks,
    pub(crate) pending: VecDeque<Pending>,
}

impl<'a> World<'a> {
    pub fn new(catalogue: &'a Catalogue, limits: Limits) -> Result<Self> {
        Self::with_campaign(catalogue, limits, CampaignId::generate()?)
    }
    pub fn with_campaign(
        catalogue: &'a Catalogue,
        limits: Limits,
        campaign: CampaignId,
    ) -> Result<Self> {
        CampaignId::from_bytes(campaign.bytes())?;
        let epoch = NEXT_WORLD
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::Capacity("world epochs"))?;
        Ok(Self {
            campaign,
            revision: 0,
            catalogue,
            definitions: BTreeMap::new(),
            block_count: 0,
            limits,
            cohort: crate::snapshot::cohort(catalogue)?,
            epoch,
            slots: Vec::new(),
            free: Vec::new(),
            instances: BTreeMap::new(),
            owners: BTreeMap::new(),
            references: BTreeMap::new(),
            authored_references: BTreeMap::new(),
            next_instance: 1,
            next_reference: 1,
            next_sequence: 1,
            local_count: 0,
            clocks: Clocks::default(),
            pending: VecDeque::new(),
        })
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.cohort
    }
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn next_revision(&self) -> Result<u64> {
        self.revision
            .checked_add(1)
            .ok_or(Error::Capacity("state revisions"))
    }
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
    pub fn reference_count(&self) -> usize {
        self.references.len()
    }
    pub fn clocks(&self) -> Clocks {
        self.clocks
    }
    pub fn advance_clocks(&mut self, clocks: Clocks) -> Result<()> {
        if !clocks.follows(self.clocks) {
            return Err(Error::Invalid(
                "clocks must advance monotonically with a new tick".into(),
            ));
        }
        let revision = self.next_revision()?;
        self.clocks = clocks;
        self.revision = revision;
        Ok(())
    }
    pub fn register_reference(&mut self, authored: Option<FormKey>) -> Result<ReferenceId> {
        if let Some(key) = &authored {
            valid_form(key)?;
            if self.authored_references.contains_key(key) {
                return Err(Error::Invalid(
                    "authored reference already registered".into(),
                ));
            }
        }
        if self.references.len() >= self.limits.max_references {
            return Err(Error::Capacity("live references"));
        }
        let next = self
            .next_reference
            .checked_add(1)
            .ok_or(Error::Capacity("reference identities"))?;
        let id = ReferenceId(
            NonZeroU64::new(self.next_reference)
                .ok_or_else(|| Error::Invalid("zero reference allocator".into()))?,
        );
        let revision = self.next_revision()?;
        if let Some(key) = &authored {
            self.authored_references.insert(key.clone(), id);
        }
        self.references.insert(id, authored);
        self.next_reference = next;
        self.revision = revision;
        Ok(id)
    }
    pub fn authored_reference(&self, key: &FormKey) -> Option<ReferenceId> {
        self.authored_references.get(key).copied()
    }
    pub fn reference_origin(&self, id: ReferenceId) -> Result<Option<&FormKey>> {
        self.references
            .get(&id)
            .map(Option::as_ref)
            .ok_or(Error::MissingReference)
    }
    pub(crate) fn validate_reference(&self, value: &ReferenceValue) -> Result<()> {
        match value {
            ReferenceValue::Null => Ok(()),
            ReferenceValue::Content { key } => valid_form(key),
            ReferenceValue::Live { id } => self.reference_origin(*id).map(|_| ()),
        }
    }
    pub(crate) fn validate_context(&self, context: &Context) -> Result<()> {
        if context.arguments.len() > self.limits.max_event_arguments {
            return Err(Error::Capacity("event arguments"));
        }
        for id in [context.calling_reference, context.containing_reference]
            .into_iter()
            .flatten()
        {
            self.reference_origin(id)?;
        }
        for value in context.target.iter().chain(context.arguments.iter()) {
            self.validate_reference(value)?;
        }
        Ok(())
    }
    pub(crate) fn validate_owner(&self, owner: &Owner) -> Result<()> {
        match owner {
            Owner::Quest { key } => valid_form(key),
            Owner::Placed { reference } => self.reference_origin(*reference).map(|_| ()),
            Owner::Fragment { .. } => Ok(()),
        }
    }
    pub fn create_instance(
        &mut self,
        definition: &Handle,
        owner: Owner,
        context: Context,
    ) -> Result<InstanceHandle> {
        self.catalogue
            .get_handle(definition)
            .ok_or(Error::DefinitionChanged)?;
        self.validate_owner(&owner)?;
        self.validate_context(&context)?;
        if self.owners.contains_key(&owner) {
            return Err(Error::Invalid(
                "owner already has a live script instance".into(),
            ));
        }
        if self.instances.len() >= self.limits.max_instances {
            return Err(Error::Capacity("script instances"));
        }
        let definition_schema = self.runtime_definition(definition)?;
        let schema = &definition_schema.locals;
        if schema.len() > self.limits.max_locals.saturating_sub(self.local_count) {
            return Err(Error::Capacity("local variables"));
        }
        let next = self
            .next_instance
            .checked_add(1)
            .ok_or(Error::Capacity("instance identities"))?;
        let id = InstanceId(
            NonZeroU64::new(self.next_instance)
                .ok_or_else(|| Error::Invalid("zero instance allocator".into()))?,
        );
        let locals = schema
            .keys()
            .map(|&index| (index, Value::Uninitialized))
            .collect();
        let instance = Instance {
            id,
            definition: definition.clone(),
            owner: owner.clone(),
            context,
            definition_schema,
            locals,
        };
        let revision = self.next_revision()?;
        self.local_count += instance.locals.len();
        let slot = if let Some(slot) = self.free.pop() {
            self.slots[slot].value = Some(instance);
            slot
        } else {
            let slot = self.slots.len();
            self.slots.push(Slot {
                generation: 1,
                value: Some(instance),
            });
            slot
        };
        self.instances.insert(id, slot);
        self.owners.insert(owner, id);
        self.next_instance = next;
        self.revision = revision;
        Ok(self.handle_for_slot(slot))
    }
    fn handle_for_slot(&self, slot: usize) -> InstanceHandle {
        InstanceHandle {
            world: self.epoch,
            slot,
            generation: self.slots[slot].generation,
        }
    }
    pub fn handle(&self, id: InstanceId) -> Result<InstanceHandle> {
        self.instances
            .get(&id)
            .map(|&slot| self.handle_for_slot(slot))
            .ok_or(Error::MissingInstance)
    }
    pub fn owner_instance(&self, owner: &Owner) -> Option<InstanceId> {
        self.owners.get(owner).copied()
    }
    fn slot(&self, handle: InstanceHandle) -> Result<usize> {
        if handle.world != self.epoch {
            return Err(Error::StaleHandle);
        }
        let slot = self.slots.get(handle.slot).ok_or(Error::StaleHandle)?;
        if slot.generation != handle.generation || slot.value.is_none() {
            return Err(Error::StaleHandle);
        }
        Ok(handle.slot)
    }
    pub fn instance(&self, handle: InstanceHandle) -> Result<&Instance> {
        let instance = self.slots[self.slot(handle)?]
            .value
            .as_ref()
            .expect("checked occupied slot");
        self.catalogue
            .get_handle(&instance.definition)
            .ok_or(Error::DefinitionChanged)?;
        Ok(instance)
    }
    pub fn remove_instance(&mut self, handle: InstanceHandle) -> Result<()> {
        let slot = self.slot(handle)?;
        let instance = self.instance(handle)?;
        if self
            .pending
            .iter()
            .any(|event| event.instance == instance.id)
        {
            return Err(Error::Invalid(
                "instance has pending events; acknowledge them explicitly before removal".into(),
            ));
        }
        let generation = self.slots[slot]
            .generation
            .checked_add(1)
            .ok_or(Error::Capacity("slot generations"))?;
        let revision = self.next_revision()?;
        let instance = self.slots[slot]
            .value
            .take()
            .expect("checked occupied slot");
        self.instances.remove(&instance.id);
        self.owners.remove(&instance.owner);
        self.local_count -= instance.locals.len();
        self.slots[slot].generation = generation;
        self.free.push(slot);
        self.revision = revision;
        Ok(())
    }
    pub(crate) fn validate_value(&self, local: &Local, value: &Value) -> Result<()> {
        match (local.kind, value) {
            (_, Value::Uninitialized) => Ok(()),
            (Kind::Float | Kind::Integer, Value::Number { .. }) => Ok(()),
            (Kind::Reference, Value::Reference { value }) => self.validate_reference(value),
            (Kind::Unsupported { .. } | Kind::UnverifiedZeroIndex { .. }, _) => {
                Err(Error::UnsupportedLocal(local.index))
            }
            _ => Err(Error::IncompatibleLocal(local.index)),
        }
    }
    /// Validate the entire batch before touching state. Duplicate assignments
    /// are rejected instead of introducing an undocumented last-write rule.
    pub fn assign(&mut self, handle: InstanceHandle, assignments: &[(u32, Value)]) -> Result<()> {
        let instance = self.instance(handle)?;
        if assignments.len() > instance.locals.len() {
            return Err(Error::Capacity("assignment batch"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (index, value) in assignments {
            if !seen.insert(*index) {
                return Err(Error::Invalid("duplicate local assignment".into()));
            }
            self.validate_value(
                instance
                    .definition_schema
                    .locals
                    .get(index)
                    .ok_or(Error::MissingLocal(*index))?,
                value,
            )?;
        }
        if assignments.is_empty() {
            return Ok(());
        }
        let revision = self.next_revision()?;
        let slot = self.slot(handle)?;
        let instance = self.slots[slot]
            .value
            .as_mut()
            .expect("checked occupied slot");
        for (index, value) in assignments {
            *instance.locals.get_mut(index).expect("checked local") = value.clone();
        }
        self.revision = revision;
        Ok(())
    }
    /// This explicit operation models the reviewed ResetAllVariables storage
    /// effect. It does not establish constructor defaults or retail timing.
    pub fn reset_locals(&mut self, handle: InstanceHandle) -> Result<usize> {
        let instance = self.instance(handle)?;
        let mut assignments = Vec::with_capacity(instance.locals.len());
        for local in instance.definition_schema.locals.values() {
            let value = match local.kind {
                Kind::Float | Kind::Integer => Value::Number { bits: 0 },
                Kind::Reference => Value::Reference {
                    value: ReferenceValue::Null,
                },
                _ => return Err(Error::UnsupportedLocal(local.index)),
            };
            assignments.push((local.index, value));
        }
        self.assign(handle, &assignments)?;
        Ok(assignments.len())
    }
    pub fn resolve_script_reference(
        &self,
        handle: InstanceHandle,
        index: u32,
        player: Option<ReferenceId>,
    ) -> Result<ReferenceValue> {
        let instance = self.instance(handle)?;
        let script = self
            .catalogue
            .get_handle(&instance.definition)
            .ok_or(Error::DefinitionChanged)?;
        let reference = script.reference(index).ok_or_else(|| {
            Error::UnresolvedDependency(
                "reference-list index is absent (indices are one-based)".into(),
            )
        })?;
        match reference.status {
            ReferenceStatus::DefinedForm => Ok(ReferenceValue::Content {
                key: reference.form_key.clone().ok_or(Error::DefinitionChanged)?,
            }),
            ReferenceStatus::NullForm => Ok(ReferenceValue::Null),
            ReferenceStatus::DynamicVariable => match instance.local(reference.value)? {
                Value::Reference { value } => {
                    self.validate_reference(value)?;
                    Ok(value.clone())
                }
                _ => Err(Error::IncompatibleLocal(reference.value)),
            },
            ReferenceStatus::RuntimeDependency => {
                let id =
                    player.ok_or_else(|| Error::UnresolvedDependency("player reference".into()))?;
                self.reference_origin(id)?;
                Ok(ReferenceValue::Live { id })
            }
            status => Err(Error::UnresolvedDependency(format!(
                "authored reference status {status:?}"
            ))),
        }
    }
    pub(crate) fn runtime_definition(&mut self, handle: &Handle) -> Result<Arc<DefinitionSchema>> {
        let script = self
            .catalogue
            .get_handle(handle)
            .ok_or(Error::DefinitionChanged)?;
        if let Some(schema) = self.definitions.get(&handle.key) {
            return Ok(Arc::clone(schema));
        }
        let program = script
            .program()
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let blocks: BTreeSet<_> = program
            .iter()
            .flat_map(|program| program.instructions.iter())
            .filter_map(|instruction| {
                instruction
                    .event
                    .map(|event| (instruction.bytes.start as u32, event.id))
            })
            .collect();
        if blocks.len()
            > self
                .limits
                .max_event_blocks
                .saturating_sub(self.block_count)
        {
            return Err(Error::Capacity("compiled event blocks"));
        }
        let schema = Arc::new(DefinitionSchema {
            locals: schema::locals(script),
            blocks,
        });
        self.block_count += schema.blocks.len();
        self.definitions
            .insert(handle.key.clone(), Arc::clone(&schema));
        Ok(schema)
    }
    pub(crate) fn validate_trigger(schema: &DefinitionSchema, trigger: &Trigger) -> Result<()> {
        match trigger {
            Trigger::ObjectEvent { mask } if *mask != 0 => Ok(()),
            Trigger::ObjectEvent { .. } => Err(Error::Invalid("empty object-event mask".into())),
            Trigger::Block {
                event_id,
                begin_byte_offset,
            } if schema.blocks.contains(&(*begin_byte_offset, *event_id)) => Ok(()),
            Trigger::Block { .. } => Err(Error::Invalid(
                "event block identity does not match the loaded definition".into(),
            )),
        }
    }
    pub fn enqueue(
        &mut self,
        handle: InstanceHandle,
        trigger: Trigger,
        context: Context,
    ) -> Result<u64> {
        let instance = self.instance(handle)?;
        let id = instance.id;
        Self::validate_trigger(&instance.definition_schema, &trigger)?;
        self.validate_context(&context)?;
        if self.pending.len() >= self.limits.max_pending_events {
            return Err(Error::Capacity("pending events"));
        }
        let next = self
            .next_sequence
            .checked_add(1)
            .ok_or(Error::Capacity("event sequences"))?;
        let revision = self.next_revision()?;
        let sequence = self.next_sequence;
        self.pending.push_back(Pending {
            sequence,
            instance: id,
            trigger,
            context,
            arrived: self.clocks,
        });
        self.next_sequence = next;
        self.revision = revision;
        Ok(sequence)
    }
    pub fn pending_events(&self) -> impl ExactSizeIterator<Item = &Pending> {
        self.pending.iter()
    }
    /// Acknowledgment is an explicit host action. This does not execute the
    /// event; FIFO here is the canonical journal's order, not retail scheduling.
    pub fn acknowledge(&mut self, sequence: u64) -> Result<Pending> {
        if self
            .pending
            .front()
            .is_none_or(|event| event.sequence != sequence)
        {
            return Err(Error::Invalid(
                "acknowledgment must name the first pending event".into(),
            ));
        }
        let revision = self.next_revision()?;
        let event = self.pending.pop_front().expect("checked pending head");
        self.revision = revision;
        Ok(event)
    }
}
