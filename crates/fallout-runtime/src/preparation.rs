//! Bind one journaled event to its live instance and exact immutable source.
//! This produces a bounded source frame, not permission to execute bytecode.
use crate::{
    World,
    events::{Pending, Trigger},
    programs::{self, PreparedDefinition, PreparedSources},
    state::Instance,
};
use fallout_data::obscript::{
    Instruction, argument_census::Signatures, control_flow::Event, definition_plan,
    expression_plan::Model,
};
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

fn select<'a>(
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
            pending,
            instance,
            Source::Prepared(source),
            event_id,
            begin_byte_offset,
            maximum_event_instructions,
        )
    }
}
