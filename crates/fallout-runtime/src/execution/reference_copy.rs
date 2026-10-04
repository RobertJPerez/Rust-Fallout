//! Exact own reference identity copy through existing source binding and commit.
use super::local_copy;
use crate::{
    World, event_operands,
    foreign::Content,
    identity::{ReferenceValue, Value},
    preparation,
    programs::PreparedSources,
    schema::Kind,
    state::event_commit::{Receipt, StagedEventChanges},
};
use fallout_data::obscript::expression::{self, StatementKind, Target};
pub use local_copy::Intent;
use serde::Serialize;
use std::{io::Write, ops::Range};

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub sequence: u64,
    pub intent: Intent,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_event_instructions: usize,
    pub maximum_operand_uses: usize,
    pub maximum_statement_bytes: usize,
    pub observation: preparation::ObservationLimits,
    pub maximum_probe_variable_bytes: usize,
    pub maximum_trace_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 4096,
            maximum_operand_uses: 2,
            maximum_statement_bytes: 65_539,
            observation: Default::default(),
            maximum_probe_variable_bytes: 3 * 1024 * 1024,
            maximum_trace_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Preparation(#[from] preparation::Error),
    #[error(transparent)]
    Observation(#[from] preparation::ObservationError),
    #[error(transparent)]
    Operands(#[from] event_operands::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("reference-copy must name the existing journal head")]
    HeadChanged,
    #[error("reference-copy budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("reference-copy trace encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
}
pub enum Preparation {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: String,
    },
    Staged(Box<StagedReferenceCopy>),
}
fn unsupported(reason: local_copy::Unsupported, detail: &'static str) -> Preparation {
    Preparation::Unsupported {
        reason,
        detail: detail.into(),
    }
}
#[derive(Debug, Serialize)]
pub struct Trace {
    pub intent: Intent,
    pub frame: preparation::EventObservation,
    pub instruction_index: usize,
    pub statement_scda_bytes: Range<usize>,
    pub source_token_scda_bytes: Range<usize>,
    pub source_index: u32,
    pub destination_index: u32,
    pub probe_variable_reservation: usize,
    pub original_behavior_verified: bool,
}
impl Trace {
    /// Historical source value already present in the bounded projection.
    pub fn copied_reference(&self) -> Option<&ReferenceValue> {
        match &self.frame.locals.first()?.value {
            Value::Reference { value } => Some(value),
            _ => None,
        }
    }
}
#[derive(Debug)]
#[must_use = "reference copy has no effects until its opaque own stage commits"]
pub struct StagedReferenceCopy {
    changes: StagedEventChanges,
    trace: Trace,
}
#[derive(Debug, Serialize)]
pub struct Committed {
    pub trace: Trace,
    pub receipt: Receipt,
}
impl StagedReferenceCopy {
    pub fn trace(&self) -> &Trace {
        &self.trace
    }
    pub fn changes(&self) -> &StagedEventChanges {
        &self.changes
    }
    pub fn commit(self, world: &mut World<'_>) -> Result<Committed, Error> {
        let receipt = world.commit_event_changes(self.changes)?;
        Ok(Committed {
            trace: self.trace,
            receipt,
        })
    }
}
struct TraceAdmission {
    bytes: usize,
    maximum: usize,
    exceeded: bool,
}
impl Write for TraceAdmission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "reference-copy trace byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn stage(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    selection: Selection,
    limits: Limits,
) -> Result<Preparation, Error> {
    if world
        .pending_events()
        .next()
        .is_none_or(|p| p.sequence != selection.sequence)
    {
        return Err(Error::HeadChanged);
    }
    let frame = world.prepare_event_with_sources(
        selection.sequence,
        sources,
        limits.maximum_event_instructions,
    )?;
    if selection.intent == Intent::Faithful {
        return Ok(unsupported(
            local_copy::Unsupported::UnverifiedRetailSemantics,
            "Original reference assignment and event lifetime are unverified",
        ));
    }
    content.validate_world(world)?;
    let [_, instruction, _] = frame.instructions() else {
        return Ok(unsupported(
            local_copy::Unsupported::EventShape,
            "Engineering reference copy requires Begin/one Assignment/End",
        ));
    };
    let instruction_index = frame.selected().begin_instruction + 1;
    let Some(statement) = frame.source().statement(instruction_index) else {
        return Ok(unsupported(
            local_copy::Unsupported::EventShape,
            "The sole body instruction must be an assignment",
        ));
    };
    let StatementKind::Assignment(Target::Local {
        type_byte: b'f' | b's',
        index: destination,
        context_reference: None,
    }) = statement.kind()
    else {
        return Ok(unsupported(
            local_copy::Unsupported::DestinationShape,
            "Only an own compiled local destination is admitted",
        ));
    };
    let [token] = statement.plan().tokens() else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only one own compiled local source is admitted",
        ));
    };
    let expression::Kind::Local {
        type_byte: b'f' | b's' | b'l',
        index: source,
        context_reference: None,
    } = token.kind
    else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "No literal, context, operator, global or call is admitted",
        ));
    };
    let source_index = u32::from(source);
    let destination_index = u32::from(*destination);
    if instruction.bytes.len() > limits.maximum_statement_bytes {
        return Err(Error::Capacity("statement bytes"));
    }
    let all = &frame.source().bindings().uses;
    if all.len() > limits.observation.maximum_binding_uses {
        return Err(Error::Capacity("binding uses"));
    }
    let begin = frame.instructions()[0].bytes.start;
    let end = frame.instructions()[2].bytes.end;
    let mut bindings = all.iter().filter(|b| (begin..end).contains(&b.scda_offset));
    let (Some(write), Some(read)) = (bindings.next(), bindings.next()) else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Missing own source binding",
        ));
    };
    if limits.maximum_operand_uses < 2 {
        return Err(Error::Capacity("operand uses"));
    }
    if bindings.next().is_some() {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Unexpected extra source binding",
        ));
    }
    let valid = |binding: &fallout_data::obscript::operand_binding::Use,
                 index: u32,
                 offset: usize,
                 role: u8| {
        let Some(schema) = frame.instance().definition_schema.locals.get(&index) else {
            return false;
        };
        let Some(authored) = frame.source().source().declaration(index) else {
            return false;
        };
        schema.kind == Kind::Reference
            && authored.decoded_offset == schema.declaration_decoded_offset
            && binding.role == role
            && binding.scda_offset == offset
            && u32::from(binding.index) == index
            && binding.context_reference.is_none()
            && binding.status == 1
            && binding.target_value == Some(index)
            && binding.local_declaration_decoded_offset == Some(authored.decoded_offset as usize)
            && binding.local_type_byte == Some(authored.type_byte)
    };
    let expression = statement.expression_scda_offset();
    if !valid(write, destination_index, instruction.operand_offset + 1, 2)
        || !valid(read, source_index, expression + token.bytes.start + 1, 4)
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Both actual source-bound declarations must be own Reference locals",
        ));
    }
    let selected = [source_index, destination_index];
    let observation = preparation::EventObservation::capture(
        &frame,
        if source_index == destination_index {
            &selected[..1]
        } else {
            &selected
        },
        limits.observation,
    )?;
    let reservation = observation
        .counts
        .variable_bytes
        .checked_mul(3)
        .filter(|&bytes| bytes <= limits.maximum_probe_variable_bytes)
        .ok_or(Error::Capacity("probe variable bytes"))?;
    let probe = world.probe_event_operands_with_sources(
        selection.sequence,
        sources,
        content,
        None,
        event_operands::CachedLimits {
            maximum_event_instructions: limits.maximum_event_instructions,
            maximum_uses: limits.maximum_operand_uses,
        },
    )?;
    let mut operands = probe.operands.into_iter();
    let (Some(own), Some(read)) = (operands.next(), operands.next()) else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Incomplete own operand probe",
        ));
    };
    if operands.next().is_some()
        || !matches!(own.outcome,event_operands::Outcome::Resolved {access:event_operands::Access::Destination,resolution:event_operands::Resolution::Local {instance,declaration,..}} if instance == frame.instance().id() && declaration.index == destination_index && declaration.kind == Kind::Reference)
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Own reference destination did not resolve",
        ));
    }
    let event_operands::Outcome::Resolved {
        access: event_operands::Access::Read,
        resolution:
            event_operands::Resolution::Local {
                instance,
                declaration,
                value: Some(value @ Value::Reference { .. }),
            },
    } = read.outcome
    else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Source is not an initialized typed reference",
        ));
    };
    if instance != frame.instance().id()
        || declaration.index != source_index
        || declaration.kind != Kind::Reference
        || value != observation.locals[0].value
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Current source reference differs from its projection",
        ));
    }
    let trace = Trace {
        intent: selection.intent,
        frame: observation,
        instruction_index,
        statement_scda_bytes: instruction.bytes.clone(),
        source_token_scda_bytes: expression + token.bytes.start..expression + token.bytes.end,
        source_index,
        destination_index,
        probe_variable_reservation: reservation,
        original_behavior_verified: false,
    };
    let mut admitted = TraceAdmission {
        bytes: 0,
        maximum: limits.maximum_trace_bytes,
        exceeded: false,
    };
    if let Err(error) = serde_json::to_writer(&mut admitted, &trace) {
        return Err(if admitted.exceeded {
            Error::Capacity("trace bytes")
        } else {
            Error::Encoding(error)
        });
    }
    let changes =
        world.stage_event_changes(selection.sequence, &[(destination_index, value)], true)?;
    Ok(Preparation::Staged(Box::new(StagedReferenceCopy {
        changes,
        trace,
    })))
}
