//! Read-only native dispatch over an exact prepared event. No branch selection,
//! numeric conversion, assignment, continuation or event acknowledgment occurs.
use crate::{
    World,
    events::{Context, Pending},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    preparation::{self, PreparedEvent},
    programs::PreparedSources,
    query,
};
use fallout_data::{
    loaded_scripts::Handle,
    obscript::{self, arguments, expression},
};
use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_event_instructions: usize,
    pub maximum_calls: usize,
    pub maximum_argument_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 262_144,
            maximum_calls: 65_536,
            maximum_argument_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Preparation(#[from] preparation::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("native dispatch budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("native occurrence {0} is outside this prepared event")]
    MissingCall(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Faithful,
    EngineeringObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub decoded_source: bool,
    pub engineering_host_read: bool,
    pub retail_executable: bool,
}
pub fn capability(command_id: u16) -> Capability {
    Capability {
        // Only a prepared call can establish decoded_source; an arbitrary ID
        // or a retail descriptor's handler presence cannot do so.
        decoded_source: false,
        engineering_host_read: command_id == query::GET_ITEM_COUNT_COMMAND,
        retail_executable: false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Location {
    Instruction,
    Expression { token_index: usize },
}

#[derive(Debug, Serialize)]
pub struct Call<'a> {
    pub command_id: u16,
    pub instruction_index: usize,
    pub scda_bytes: Range<usize>,
    pub instruction_scda_bytes: Range<usize>,
    pub argument_scda_offset: usize,
    pub calling_reference_index: Option<u16>,
    pub location: Location,
    pub raw_arguments: &'a [u8],
    pub capability: Capability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    MissingImplementation,
    UnverifiedRetailSemantics,
    MissingSubject,
    CallerResolution,
    CallerNeedsLiveReference,
    ArgumentEncoding,
    ArgumentSemantics,
    ArgumentResolution,
    ArgumentNeedsContentReference,
    UnverifiedFormList,
    HostQueryUnavailable,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Unsupported { reason: Unsupported, detail: String },
    EngineeringObservation { trace: Box<query::Trace> },
}
fn unsupported(reason: Unsupported, detail: impl ToString) -> Outcome {
    Outcome::Unsupported {
        reason,
        detail: detail.to_string(),
    }
}

// One source/caller/argument admission shared by immediate observation and the
// owned native plan. No external constructor or deserialization grants access.
pub(crate) struct ResolvedCall {
    pub subject: ReferenceId,
    pub argument: Value,
}
pub(crate) enum Admission {
    Unsupported { reason: Unsupported, detail: String },
    Resolved(ResolvedCall),
}
fn admission_unsupported(reason: Unsupported, detail: impl ToString) -> Result<Admission, Error> {
    Ok(Admission::Unsupported {
        reason,
        detail: detail.to_string(),
    })
}

/// Length metadata only; canonical resolution remains World-owned. Names have
/// no canonical length maximum, so charge a borrowed dynamic/static content key
/// before resolve_script_reference is allowed to clone it.
pub(crate) fn reference_variable_bytes(
    world: &World<'_>,
    instance: &crate::state::Instance,
    index: u16,
) -> usize {
    let Some(reference) = world
        .catalogue()
        .get_handle(instance.definition())
        .and_then(|script| script.reference(u32::from(index)))
    else {
        return 0;
    };
    if reference.status == fallout_data::loaded_scripts::ReferenceStatus::DynamicVariable {
        match instance.locals().get(&reference.value) {
            Some(Value::Reference {
                value: ReferenceValue::Content { key },
            }) => key.origin_plugin.len(),
            _ => 0,
        }
    } else {
        reference
            .form_key
            .as_ref()
            .map_or(0, |key| key.origin_plugin.len())
    }
}

/// All context roles remain distinct. supplied_subject is an explicit host
/// input for an unprefixed call, never a fallback from owner/player/target.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    pub supplied_subject: Option<ReferenceId>,
    pub player: Option<ReferenceId>,
}

#[derive(Debug, Serialize)]
pub struct Observation<'a> {
    pub campaign: CampaignId,
    pub source_cohort_sha256: &'a str,
    pub state_revision: u64,
    pub pending: &'a Pending,
    pub definition: &'a Handle,
    pub owner: &'a Owner,
    pub event_context: &'a Context,
    pub call: &'a Call<'a>,
    pub intent: Intent,
    pub supplied_subject: Option<ReferenceId>,
    pub explicit_player: Option<ReferenceId>,
    pub outcome: Outcome,
}

/// Construction is atomic and bounded. The current world borrow prevents using
/// old operand observations, recycled handles or a different world's banks.
pub struct NativeCalls<'a, 'source> {
    world: &'a World<'source>,
    frame: PreparedEvent<'a>,
    calls: Vec<Call<'a>>,
}
impl<'source> World<'source> {
    pub fn prepare_native_calls_with_sources<'a>(
        &'a self,
        sequence: u64,
        sources: &'a PreparedSources<'_>,
        limits: Limits,
    ) -> Result<NativeCalls<'a, 'source>, Error> {
        let frame =
            self.prepare_event_with_sources(sequence, sources, limits.maximum_event_instructions)?;
        let mut calls = Vec::new();
        let mut argument_bytes = 0;
        let mut retain = |mut call: Call<'a>| -> Result<(), Error> {
            if calls.len() >= limits.maximum_calls {
                return Err(Error::Capacity("native occurrences"));
            }
            if call.raw_arguments.len()
                > limits.maximum_argument_bytes.saturating_sub(argument_bytes)
            {
                return Err(Error::Capacity("argument bytes"));
            }
            argument_bytes += call.raw_arguments.len();
            call.capability.decoded_source = true;
            calls.push(call);
            Ok(())
        };
        for (relative, instruction) in frame.instructions().iter().enumerate() {
            let index = frame.selected().begin_instruction + relative;
            if instruction.kind() == obscript::Kind::NativeCommand {
                retain(Call {
                    command_id: instruction.opcode,
                    instruction_index: index,
                    scda_bytes: instruction.bytes.clone(),
                    instruction_scda_bytes: instruction.bytes.clone(),
                    argument_scda_offset: instruction.operand_offset,
                    calling_reference_index: instruction.calling_reference,
                    location: Location::Instruction,
                    raw_arguments: instruction.operands,
                    capability: capability(instruction.opcode),
                })?;
            }
            if let Some(statement) = frame.source().statement(index) {
                for (token_index, token) in statement.plan().tokens().iter().enumerate() {
                    if let expression::Kind::Command {
                        opcode,
                        context_reference,
                        arguments,
                    } = &token.kind
                    {
                        let start = statement.expression_scda_offset() + token.bytes.start;
                        let end = statement.expression_scda_offset() + token.bytes.end;
                        retain(Call {
                            command_id: *opcode,
                            instruction_index: index,
                            scda_bytes: start..end,
                            instruction_scda_bytes: instruction.bytes.clone(),
                            argument_scda_offset: end - arguments.len(),
                            calling_reference_index: *context_reference,
                            location: Location::Expression { token_index },
                            raw_arguments: arguments,
                            capability: capability(*opcode),
                        })?;
                    }
                }
            }
        }
        Ok(NativeCalls {
            world: self,
            frame,
            calls,
        })
    }
}

impl NativeCalls<'_, '_> {
    /// Physical source order, including duplicates and branch-contained calls.
    /// This is a capability inventory, never an executed instruction trace.
    pub fn calls(&self) -> &[Call<'_>] {
        &self.calls
    }

    pub(crate) fn frame(&self) -> &PreparedEvent<'_> {
        &self.frame
    }
    pub(crate) fn admit_occurrence(
        &self,
        occurrence: usize,
        inputs: Inputs,
        intent: Intent,
        maximum_reference_variable_bytes: usize,
    ) -> Result<Admission, Error> {
        let call = self
            .calls
            .get(occurrence)
            .ok_or(Error::MissingCall(occurrence))?;
        self.admit(call, inputs, intent, maximum_reference_variable_bytes)
    }

    pub fn observe(
        &self,
        occurrence: usize,
        content: &Content,
        inputs: Inputs,
        intent: Intent,
        maximum_contributions: usize,
    ) -> Result<Observation<'_>, Error> {
        content.validate_world(self.world)?;
        let call = self
            .calls
            .get(occurrence)
            .ok_or(Error::MissingCall(occurrence))?;
        let outcome = self.dispatch(call, content, inputs, intent, maximum_contributions)?;
        Ok(Observation {
            campaign: self.world.campaign(),
            source_cohort_sha256: self.world.catalogue_fingerprint(),
            state_revision: self.world.revision(),
            pending: self.frame.pending(),
            definition: self.frame.source().handle(),
            owner: self.frame.instance().owner(),
            event_context: &self.frame.pending().context,
            call,
            intent,
            supplied_subject: inputs.supplied_subject,
            explicit_player: inputs.player,
            outcome,
        })
    }

    fn dispatch(
        &self,
        call: &Call<'_>,
        content: &Content,
        inputs: Inputs,
        intent: Intent,
        maximum_contributions: usize,
    ) -> Result<Outcome, Error> {
        let resolved = match self.admit(call, inputs, intent, usize::MAX)? {
            Admission::Unsupported { reason, detail } => {
                return Ok(Outcome::Unsupported { reason, detail });
            }
            Admission::Resolved(resolved) => resolved,
        };
        let query = query::Request::prepare(
            self.world,
            query::Entry::Native {
                command_id: call.command_id,
            },
            Some(resolved.subject),
            &[resolved.argument],
        )
        .and_then(|request| request.evaluate(self.world, content, maximum_contributions));
        Ok(match query {
            Ok(trace) => Outcome::EngineeringObservation {
                trace: Box::new(trace),
            },
            Err(query::Failure::UnverifiedFormList) => unsupported(
                Unsupported::UnverifiedFormList,
                "Original GetItemCount form-list expansion is unverified",
            ),
            Err(query::Failure::State(crate::Error::Capacity(_))) => {
                return Err(Error::Capacity("query contributions"));
            }
            Err(error) => unsupported(Unsupported::HostQueryUnavailable, error),
        })
    }

    fn admit(
        &self,
        call: &Call<'_>,
        inputs: Inputs,
        intent: Intent,
        maximum_reference_variable_bytes: usize,
    ) -> Result<Admission, Error> {
        if !call.capability.engineering_host_read {
            return admission_unsupported(
                Unsupported::MissingImplementation,
                "Native command has no replacement handler",
            );
        }
        if intent == Intent::Faithful {
            return admission_unsupported(
                Unsupported::UnverifiedRetailSemantics,
                "Original GetItemCount admission, list and numeric return semantics are unverified",
            );
        }
        let handle = match self.world.handle(self.frame.instance().id()) {
            Ok(handle) => handle,
            Err(error) => return admission_unsupported(Unsupported::CallerResolution, error),
        };
        let subject = match call.calling_reference_index {
            Some(index) => {
                if reference_variable_bytes(self.world, self.frame.instance(), index)
                    > maximum_reference_variable_bytes
                {
                    return Err(Error::Capacity("reference variable bytes"));
                }
                match self
                    .world
                    .resolve_script_reference(handle, u32::from(index), inputs.player)
                {
                    Ok(ReferenceValue::Live { id }) => id,
                    Ok(_) => {
                        return admission_unsupported(
                            Unsupported::CallerNeedsLiveReference,
                            "Source caller is not an explicitly resolved live reference",
                        );
                    }
                    Err(error) => {
                        return admission_unsupported(Unsupported::CallerResolution, error);
                    }
                }
            }
            None => match inputs.supplied_subject {
                Some(id) => id,
                None => {
                    return admission_unsupported(
                        Unsupported::MissingSubject,
                        "Unprefixed call requires an explicit host subject",
                    );
                }
            },
        };
        // This descriptor is already evidenced by the primitive-query slice.
        // No signature for an unsupported command is invented here.
        let parameters = [arguments::Parameter {
            type_id: 50,
            optional_word: 0,
        }];
        let decoded = match arguments::decode(
            call.raw_arguments,
            arguments::Signature {
                convention: arguments::Convention::Default,
                parameters: &parameters,
            },
            arguments::Limits::default(),
        ) {
            Ok(decoded) => decoded,
            Err(error) => return admission_unsupported(Unsupported::ArgumentEncoding, error),
        };
        let [
            arguments::Argument {
                value: arguments::Value::FormReference { reference_index },
                ..
            },
        ] = decoded.arguments.as_slice()
        else {
            return admission_unsupported(
                Unsupported::ArgumentSemantics,
                "Only one source reference-table argument is admitted for engineering observation",
            );
        };
        if !decoded.trailing.is_empty() || !decoded.message_arguments.is_empty() {
            return admission_unsupported(
                Unsupported::ArgumentSemantics,
                "Uninterpreted native argument tail",
            );
        }
        if reference_variable_bytes(self.world, self.frame.instance(), *reference_index)
            > maximum_reference_variable_bytes
        {
            return Err(Error::Capacity("reference variable bytes"));
        }
        let argument = match self.world.resolve_script_reference(
            handle,
            u32::from(*reference_index),
            inputs.player,
        ) {
            Ok(value @ ReferenceValue::Content { .. }) => Value::Reference { value },
            Ok(_) => {
                return admission_unsupported(
                    Unsupported::ArgumentNeedsContentReference,
                    "Engineering item query needs a content key, not null or live identity",
                );
            }
            Err(error) => return admission_unsupported(Unsupported::ArgumentResolution, error),
        };
        Ok(Admission::Resolved(ResolvedCall { subject, argument }))
    }
}
