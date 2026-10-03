//! Bind one journaled event to its live instance and exact immutable source.
//! This produces a bounded source frame, not permission to execute bytecode.
use crate::{
    World,
    events::{Pending, Trigger},
    state::Instance,
};
use fallout_data::obscript::{
    Instruction, argument_census::Signatures, control_flow::Event, definition_plan,
    expression_plan::Model,
};

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
}

/// The frame borrows the world so its instance, context and source cannot change
/// during inspection. No queue entry is removed and no local value is assigned.
pub struct PreparedEvent<'a> {
    pending: &'a Pending,
    instance: &'a Instance,
    source: definition_plan::Plan<'a>,
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
        &self.source
    }
    pub fn selected(&self) -> &Event {
        &self.selected
    }
    /// Source order includes both delimiters. It does not choose VM successors.
    pub fn instructions(&self) -> &[Instruction<'a>] {
        &self.source.control().instructions()
            [self.selected.begin_instruction..=self.selected.end_instruction]
    }
}

impl World<'_> {
    pub fn prepare_event(
        &self,
        sequence: u64,
        model: &Model<'_>,
        signatures: &Signatures,
        limits: Limits,
    ) -> Result<PreparedEvent<'_>, Error> {
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
        let source = definition_plan::prepare(
            self.catalogue(),
            instance.definition(),
            model,
            signatures,
            limits.source,
        )?;
        let selected = source
            .event_at_scda_offset(begin_byte_offset as usize)
            .filter(|event| event.event_id == event_id)
            .ok_or(Error::EventBlockChanged)?
            .clone();
        let instruction_count = selected.end_instruction - selected.begin_instruction + 1;
        if instruction_count > limits.maximum_event_instructions {
            return Err(Error::Capacity);
        }
        Ok(PreparedEvent {
            pending,
            instance,
            source,
            selected,
        })
    }
}
