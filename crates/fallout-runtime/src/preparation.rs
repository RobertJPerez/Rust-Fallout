//! Bind one journaled event to its live instance and exact immutable source.
//! This produces a bounded source frame, not permission to execute bytecode.
use crate::{
    World,
    events::{Pending, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    programs::{self, PreparedDefinition, PreparedSources},
    state::Instance,
};
use fallout_data::obscript::{
    Instruction, argument_census::Signatures, control_flow::Event, definition_plan,
    expression_plan::Model, operand_binding::Use,
};
use serde::Serialize;
use std::borrow::Cow;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub source: definition_plan::Limits,
    /// Includes the authored begin/end headers; a frame is never truncated.
    pub maximum_event_instructions: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: definition_plan::Limits::default(),
            maximum_event_instructions: 262_144,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("pending event {0} is missing or has already been acknowledged")]
    MissingPending(u64),
    #[error("object-event mask 0x{0:X} has no verified source-block mapping")]
    ObjectEventMapping(u32),
    #[error("pending block does not match the prepared source event")]
    EventBlockChanged,
    #[error("prepared event instruction budget exceeded")]
    Capacity,
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Source(#[from] definition_plan::Error),
    #[error(transparent)]
    CachedSource(#[from] programs::LookupError),
}

enum Source<'a> {
    Fresh(Box<definition_plan::Plan<'a>>),
    Prepared(&'a PreparedDefinition<'a>),
}
impl<'a> Source<'a> {
    fn plan(&self) -> &definition_plan::Plan<'a> {
        match self {
            Self::Fresh(plan) => plan,
            Self::Prepared(prepared) => prepared.plan(),
        }
    }
}

/// The frame borrows the world so its instance, context and source cannot change
/// during inspection. No queue entry is removed and no local value is assigned.
pub struct PreparedEvent<'a> {
    campaign: CampaignId,
    state_revision: u64,
    catalogue_sha256: &'a str,
    pending: &'a Pending,
    instance: &'a Instance,
    source: Source<'a>,
    selected: Event,
}
impl<'a> PreparedEvent<'a> {
    pub fn pending(&self) -> &'a Pending {
        self.pending
    }
    pub fn instance(&self) -> &'a Instance {
        self.instance
    }
    pub fn source(&self) -> &definition_plan::Plan<'a> {
        self.source.plan()
    }
    pub fn binding_sha256(&self) -> Cow<'_, str> {
        match &self.source {
            Source::Fresh(plan) => Cow::Owned(fallout_data::obscript::operand_binding::digest(
                &plan.bindings().uses,
            )),
            Source::Prepared(prepared) => Cow::Borrowed(prepared.binding_sha256()),
        }
    }
    pub fn selected(&self) -> &Event {
        &self.selected
    }
    /// Source order includes both delimiters. It does not choose VM successors.
    pub fn instructions(&self) -> &[Instruction<'a>] {
        &self.source().control().instructions()
            [self.selected.begin_instruction..=self.selected.end_instruction]
    }
}

/// Logical copied payload bounds, not a measurement of allocator peak memory.
#[derive(Debug, Clone, Copy)]
pub struct ObservationLimits {
    pub maximum_source_bytes: usize,
    /// Source operand rows, explicitly selected locals and both context argument lists.
    pub maximum_rows: usize,
    /// UTF-8 bytes of every copied string, including identity/hash strings.
    pub maximum_variable_bytes: usize,
    /// Complete definition binding table checked before selecting the event window.
    pub maximum_binding_uses: usize,
}
impl Default for ObservationLimits {
    fn default() -> Self {
        Self {
            maximum_source_bytes: 1024 * 1024,
            maximum_rows: 65_536,
            maximum_variable_bytes: 1024 * 1024,
            maximum_binding_uses: 262_144,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObservationCounts {
    pub source_bytes: usize,
    pub rows: usize,
    pub variable_bytes: usize,
    pub binding_uses: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("owned event observation budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    State(#[from] crate::Error),
}

#[derive(Debug, Serialize)]
pub struct LocalObservation {
    pub declaration: crate::schema::Local,
    /// Captured storage, including Uninitialized, never an inferred initial value.
    pub value: Value,
}

/// Historical read-only data suitable for another thread after the World dies.
/// Source bindings do not resolve foreign/native semantics. This type has no
/// deserialization or commit path and is never accepted as current authority.
#[derive(Debug, Serialize)]
pub struct EventObservation {
    pub historical: bool,
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub catalogue_sha256: String,
    pub pending: Pending,
    pub instance_owner: Owner,
    pub instance_context: crate::events::Context,
    pub definition: fallout_data::loaded_scripts::Handle,
    pub full_definition_binding_sha256: String,
    pub selected: Event,
    pub begin_scda_offset: usize,
    pub end_scda_offset: usize,
    pub source_bytes: Vec<u8>,
    pub source_operands: Vec<Use>,
    pub locals: Vec<LocalObservation>,
    pub counts: ObservationCounts,
}

fn charge(
    used: &mut usize,
    amount: usize,
    maximum: usize,
    kind: &'static str,
) -> Result<(), ObservationError> {
    *used = used
        .checked_add(amount)
        .filter(|&total| total <= maximum)
        .ok_or(ObservationError::Capacity(kind))?;
    Ok(())
}
fn reference_string_bytes(value: &ReferenceValue) -> usize {
    match value {
        ReferenceValue::Content { key } => key.origin_plugin.len(),
        _ => 0,
    }
}
fn charge_context(
    context: &crate::events::Context,
    counts: &mut ObservationCounts,
    limits: ObservationLimits,
) -> Result<(), ObservationError> {
    charge(
        &mut counts.rows,
        context.arguments.len(),
        limits.maximum_rows,
        "rows",
    )?;
    for value in context.target.iter().chain(context.arguments.iter()) {
        charge(
            &mut counts.variable_bytes,
            reference_string_bytes(value),
            limits.maximum_variable_bytes,
            "variable bytes",
        )?;
    }
    Ok(())
}

impl EventObservation {
    pub fn capture(
        frame: &PreparedEvent<'_>,
        selected_locals: &[u32],
        limits: ObservationLimits,
    ) -> Result<Self, ObservationError> {
        let begin = frame
            .instructions()
            .first()
            .expect("checked begin")
            .bytes
            .start;
        let end = frame.instructions().last().expect("checked end").bytes.end;
        let bindings = &frame.source().bindings().uses;
        let mut counts = ObservationCounts {
            source_bytes: 0,
            rows: 0,
            variable_bytes: 0,
            binding_uses: 0,
        };
        charge(
            &mut counts.source_bytes,
            end - begin,
            limits.maximum_source_bytes,
            "source bytes",
        )?;
        charge(
            &mut counts.binding_uses,
            bindings.len(),
            limits.maximum_binding_uses,
            "binding uses",
        )?;
        charge(
            &mut counts.rows,
            selected_locals.len(),
            limits.maximum_rows,
            "rows",
        )?;
        for binding in bindings {
            if (begin..end).contains(&binding.scda_offset) {
                charge(&mut counts.rows, 1, limits.maximum_rows, "rows")?;
            }
        }
        let instance = frame.instance();
        let definition = frame.source().handle();
        for length in [
            frame.catalogue_sha256.len(),
            definition.version_sha256.len(),
            definition.key.record.origin_plugin.len(),
            64,
        ] {
            charge(
                &mut counts.variable_bytes,
                length,
                limits.maximum_variable_bytes,
                "variable bytes",
            )?;
        }
        if let Owner::Quest { key } = instance.owner() {
            charge(
                &mut counts.variable_bytes,
                key.origin_plugin.len(),
                limits.maximum_variable_bytes,
                "variable bytes",
            )?;
        }
        charge_context(&frame.pending().context, &mut counts, limits)?;
        charge_context(instance.context(), &mut counts, limits)?;
        // Validate and charge every selected value before cloning any payload.
        // Repeated local indices preserve the explicit requested order.
        for index in selected_locals {
            if !instance.definition_schema.locals.contains_key(index) {
                return Err(crate::Error::MissingLocal(*index).into());
            }
            let value = instance
                .locals()
                .get(index)
                .ok_or(crate::Error::MissingLocal(*index))?;
            if let Value::Reference { value } = value {
                charge(
                    &mut counts.variable_bytes,
                    reference_string_bytes(value),
                    limits.maximum_variable_bytes,
                    "variable bytes",
                )?;
            }
        }
        // Admission is complete. No snapshot or unrelated instance is copied.
        Ok(Self {
            historical: true,
            campaign: frame.campaign,
            state_revision: frame.state_revision,
            catalogue_sha256: frame.catalogue_sha256.into(),
            pending: frame.pending().clone(),
            instance_owner: instance.owner().clone(),
            instance_context: instance.context().clone(),
            definition: definition.clone(),
            full_definition_binding_sha256: frame.binding_sha256().into_owned(),
            selected: frame.selected().clone(),
            begin_scda_offset: begin,
            end_scda_offset: end,
            source_bytes: frame.source().control().bytes()[begin..end].to_vec(),
            source_operands: bindings
                .iter()
                .filter(|binding| (begin..end).contains(&binding.scda_offset))
                .cloned()
                .collect(),
            locals: selected_locals
                .iter()
                .map(|index| LocalObservation {
                    declaration: instance.definition_schema.locals[index].clone(),
                    value: instance.locals()[index].clone(),
                })
                .collect(),
            counts,
        })
    }
}

fn select<'a>(
    world: &'a World<'_>,
    pending: &'a Pending,
    instance: &'a Instance,
    source: Source<'a>,
    event_id: u16,
    begin_byte_offset: u32,
    maximum_event_instructions: usize,
) -> Result<PreparedEvent<'a>, Error> {
    let selected = source
        .plan()
        .event_at_scda_offset(begin_byte_offset as usize)
        .filter(|event| event.event_id == event_id)
        .ok_or(Error::EventBlockChanged)?
        .clone();
    let instruction_count = selected.end_instruction - selected.begin_instruction + 1;
    if instruction_count > maximum_event_instructions {
        return Err(Error::Capacity);
    }
    Ok(PreparedEvent {
        campaign: world.campaign(),
        state_revision: world.revision(),
        catalogue_sha256: world.catalogue_fingerprint(),
        pending,
        instance,
        source,
        selected,
    })
}

impl World<'_> {
    fn pending_source(&self, sequence: u64) -> Result<(&Pending, &Instance, u16, u32), Error> {
        // Sequence order is checked on restore and preserved by enqueue/acknowledge.
        // Binary search also works when the deque's allocation has wrapped.
        let index = self
            .pending
            .binary_search_by_key(&sequence, |event| event.sequence)
            .map_err(|_| Error::MissingPending(sequence))?;
        let pending = &self.pending[index];
        let (event_id, begin_byte_offset) = match pending.trigger {
            Trigger::Block {
                event_id,
                begin_byte_offset,
            } => (event_id, begin_byte_offset),
            Trigger::ObjectEvent { mask } => return Err(Error::ObjectEventMapping(mask)),
        };
        let instance = self.instance(self.handle(pending.instance)?)?;
        self.validate_context(&pending.context)?;
        Ok((pending, instance, event_id, begin_byte_offset))
    }
    pub fn prepare_event(
        &self,
        sequence: u64,
        model: &Model<'_>,
        signatures: &Signatures,
        limits: Limits,
    ) -> Result<PreparedEvent<'_>, Error> {
        let (pending, instance, event_id, begin_byte_offset) = self.pending_source(sequence)?;
        let source = definition_plan::prepare(
            self.catalogue(),
            instance.definition(),
            model,
            signatures,
            limits.source,
        )?;
        select(
            self,
            pending,
            instance,
            Source::Fresh(Box::new(source)),
            event_id,
            begin_byte_offset,
            limits.maximum_event_instructions,
        )
    }
    /// The source admission policy was fixed when `sources` was built. This
    /// call separately bounds its event window and checks current live identity.
    pub fn prepare_event_with_sources<'a>(
        &'a self,
        sequence: u64,
        sources: &'a PreparedSources<'_>,
        maximum_event_instructions: usize,
    ) -> Result<PreparedEvent<'a>, Error> {
        sources.validate_world(self)?;
        let (pending, instance, event_id, begin_byte_offset) = self.pending_source(sequence)?;
        let source = sources.get(instance.definition())?;
        select(
            self,
            pending,
            instance,
            Source::Prepared(source),
            event_id,
            begin_byte_offset,
            maximum_event_instructions,
        )
    }
}
