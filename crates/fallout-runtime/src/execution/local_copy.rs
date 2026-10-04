//! One source-bound engineering own-local bit copy, through canonical staging.
//! Original assignment conversions and event lifecycle remain unsupported.
use crate::{
    World, event_operands,
    foreign::Content,
    identity::Value,
    preparation,
    programs::PreparedSources,
    schema::Kind,
    state::event_commit::{Receipt, StagedEventChanges},
};
use fallout_data::obscript::expression::{self, StatementKind, Target};
use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Faithful,
    Engineering,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_event_instructions: usize,
    pub maximum_operand_uses: usize,
    pub maximum_statement_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 262_144,
            maximum_operand_uses: 2,
            maximum_statement_bytes: 65_539,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Preparation(#[from] preparation::Error),
    #[error(transparent)]
    Operands(#[from] event_operands::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error("local-copy statement-byte budget exceeded")]
    Capacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    UnverifiedRetailSemantics,
    EventShape,
    DestinationShape,
    ExpressionShape,
    NonNumericLocal,
    LiveOperandUnavailable,
}

#[derive(Debug)]
pub enum Preparation {
    Unsupported { reason: Unsupported, detail: String },
    Staged(Box<StagedCopy>),
}
fn unsupported(reason: Unsupported, detail: impl ToString) -> Preparation {
    Preparation::Unsupported {
        reason,
        detail: detail.to_string(),
    }
}

/// Owned observations are not execution authority or a saved continuation.
#[derive(Debug, Serialize)]
pub struct Trace {
    pub intent: Intent,
    pub operands: event_operands::Probe,
    pub instruction_index: usize,
    pub statement_scda_bytes: Range<usize>,
    pub statement_bytes: Vec<u8>,
    pub source_token_scda_bytes: Range<usize>,
    pub source_index: u32,
    pub destination_index: u32,
    pub copied_value: Value,
    pub destination_before: Value,
    pub original_behavior_verified: bool,
}

/// Only runtime's opaque proposal authorizes commit; the trace is diagnostic.
#[derive(Debug)]
#[must_use = "a staged copy has no effects until committed"]
pub struct StagedCopy {
    changes: StagedEventChanges,
    trace: Trace,
}
impl StagedCopy {
    pub fn trace(&self) -> &Trace {
        &self.trace
    }
    pub fn changes(&self) -> &StagedEventChanges {
        &self.changes
    }
    pub fn commit(self, world: &mut World<'_>) -> Result<CommittedCopy, Error> {
        let receipt = world.commit_event_changes(self.changes)?;
        Ok(CommittedCopy {
            trace: self.trace,
            receipt,
        })
    }
}
#[derive(Debug, Serialize)]
pub struct CommittedCopy {
    pub trace: Trace,
    pub receipt: Receipt,
}

impl World<'_> {
    pub fn stage_source_local_copy_with_sources(
        &self,
        sequence: u64,
        sources: &PreparedSources<'_>,
        content: &Content,
        intent: Intent,
        limits: Limits,
    ) -> Result<Preparation, Error> {
        let frame =
            self.prepare_event_with_sources(sequence, sources, limits.maximum_event_instructions)?;
        if intent == Intent::Faithful {
            return Ok(unsupported(
                Unsupported::UnverifiedRetailSemantics,
                "Original assignment conversion and event lifecycle rules are unverified",
            ));
        }
        let [_, instruction, _] = frame.instructions() else {
            return Ok(unsupported(
                Unsupported::EventShape,
                "Engineering copy requires Begin/one Assignment/End",
            ));
        };
        let index = frame.selected().begin_instruction + 1;
        let Some(statement) = frame.source().statement(index) else {
            return Ok(unsupported(
                Unsupported::EventShape,
                "The sole instruction is not an assignment",
            ));
        };
        let StatementKind::Assignment(Target::Local {
            type_byte: b'f' | b's',
            index: destination,
            context_reference: None,
        }) = statement.kind()
        else {
            return Ok(unsupported(
                Unsupported::DestinationShape,
                "Only own numeric-local destinations are admitted",
            ));
        };
        let [token] = statement.plan().tokens() else {
            return Ok(unsupported(
                Unsupported::ExpressionShape,
                "Only one own numeric-local operand is admitted",
            ));
        };
        let expression::Kind::Local {
            type_byte: b'f' | b's' | b'l',
            index: source,
            context_reference: None,
        } = &token.kind
        else {
            return Ok(unsupported(
                Unsupported::ExpressionShape,
                "No literal, conversion, operator, call, global or foreign read is admitted",
            ));
        };
        if instruction.bytes.len() > limits.maximum_statement_bytes {
            return Err(Error::Capacity);
        }
        let probe = self.probe_event_operands_with_sources(
            sequence,
            sources,
            content,
            None,
            event_operands::CachedLimits {
                maximum_event_instructions: limits.maximum_event_instructions,
                maximum_uses: limits.maximum_operand_uses,
            },
        )?;
        let destination = u32::from(*destination);
        let source = u32::from(*source);
        let mut source_value = None;
        let mut destination_found = false;
        for operand in &probe.operands {
            let event_operands::Outcome::Resolved {
                access,
                resolution:
                    event_operands::Resolution::Local {
                        instance,
                        declaration,
                        value,
                    },
            } = &operand.outcome
            else {
                return Ok(unsupported(
                    Unsupported::LiveOperandUnavailable,
                    format!(
                        "Current canonical operand could not resolve: {:?}",
                        operand.outcome
                    ),
                ));
            };
            if *instance != frame.instance().id()
                || !matches!(declaration.kind, Kind::Float | Kind::Integer)
            {
                return Ok(unsupported(
                    Unsupported::NonNumericLocal,
                    "Both source and destination must be own declared numeric slots",
                ));
            }
            match access {
                event_operands::Access::Destination if declaration.index == destination => {
                    destination_found = true
                }
                event_operands::Access::Read if declaration.index == source => {
                    source_value = value.clone()
                }
                _ => {
                    return Ok(unsupported(
                        Unsupported::LiveOperandUnavailable,
                        "Unexpected bound operand role",
                    ));
                }
            }
        }
        let Some(value @ Value::Number { .. }) = source_value else {
            return Ok(unsupported(
                Unsupported::LiveOperandUnavailable,
                "Current source is not an initialized Number",
            ));
        };
        if !destination_found || probe.operands.len() != 2 {
            return Ok(unsupported(
                Unsupported::LiveOperandUnavailable,
                "Assignment must have one destination and one read",
            ));
        }
        let trace = Trace {
            intent,
            instruction_index: index,
            statement_scda_bytes: instruction.bytes.clone(),
            statement_bytes: frame.source().control().bytes()[instruction.bytes.clone()].to_vec(),
            source_token_scda_bytes: (statement.expression_scda_offset() + token.bytes.start)
                ..(statement.expression_scda_offset() + token.bytes.end),
            source_index: source,
            destination_index: destination,
            copied_value: value.clone(),
            destination_before: frame
                .instance()
                .locals()
                .get(&destination)
                .ok_or(crate::Error::MissingLocal(destination))?
                .clone(),
            operands: probe,
            original_behavior_verified: false,
        };
        let changes = self.stage_event_changes(sequence, &[(destination, value)], true)?;
        Ok(Preparation::Staged(Box::new(StagedCopy { changes, trace })))
    }
}
