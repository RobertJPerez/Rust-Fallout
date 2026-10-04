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

/// Separate opt-in bounds for a complete engineering event. Statement bytes
/// are cumulative; source/context copying reuses bounded EventObservation.
#[derive(Debug, Clone, Copy)]
pub struct MultiLimits {
    pub maximum_event_instructions: usize,
    pub maximum_statements: usize,
    pub maximum_operand_uses: usize,
    pub maximum_statement_bytes: usize,
    pub observation: preparation::ObservationLimits,
}
impl Default for MultiLimits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 262_144,
            maximum_statements: 64,
            maximum_operand_uses: 128,
            maximum_statement_bytes: 65_539,
            observation: preparation::ObservationLimits::default(),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum MultiError {
    #[error(transparent)]
    Preparation(#[from] preparation::Error),
    #[error(transparent)]
    Observation(#[from] preparation::ObservationError),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("engineering multi-copy budget exceeded: {0}")]
    Capacity(&'static str),
}
#[derive(Debug, Serialize)]
pub struct MultiStatement {
    pub instruction_index: usize,
    pub statement_scda_bytes: Range<usize>,
    pub source_token_scda_bytes: Range<usize>,
    pub destination_operand: usize,
    pub source_operand: usize,
    pub source_index: u32,
    pub destination_index: u32,
    pub canonical_source_before: Value,
    pub source_from_overlay: bool,
    pub destination_before: Value,
    pub copied_bits: u64,
}
#[derive(Debug, Serialize)]
pub struct MultiTrace {
    pub intent: Intent,
    pub frame: preparation::EventObservation,
    pub statements: Vec<MultiStatement>,
    pub statement_bytes: usize,
    pub original_behavior_verified: bool,
}
#[derive(Debug)]
pub enum MultiPreparation {
    Unsupported { reason: Unsupported, detail: String },
    Staged(Box<StagedMultiCopy>),
}
#[derive(Debug)]
#[must_use = "a staged multi-copy has no effects until committed"]
pub struct StagedMultiCopy {
    changes: StagedEventChanges,
    trace: MultiTrace,
}
#[derive(Debug, Serialize)]
pub struct CommittedMultiCopy {
    pub trace: MultiTrace,
    pub receipt: Receipt,
}
impl StagedMultiCopy {
    pub fn trace(&self) -> &MultiTrace {
        &self.trace
    }
    pub fn changes(&self) -> &StagedEventChanges {
        &self.changes
    }
    pub fn commit(self, world: &mut World<'_>) -> Result<CommittedMultiCopy, MultiError> {
        let receipt = world.commit_event_changes(self.changes)?;
        Ok(CommittedMultiCopy {
            trace: self.trace,
            receipt,
        })
    }
}
fn multi_unsupported(reason: Unsupported, detail: &'static str) -> MultiPreparation {
    MultiPreparation::Unsupported {
        reason,
        detail: detail.into(),
    }
}
pub(crate) fn own_binding(
    binding: &fallout_data::obscript::operand_binding::Use,
    index: u32,
    offset: usize,
    role: u8,
    declaration: &crate::schema::Local,
) -> bool {
    binding.role == role
        && binding.scda_offset == offset
        && u32::from(binding.index) == index
        && binding.context_reference.is_none()
        && binding.status == 1
        && binding.target_value == Some(index)
        && binding.local_declaration_decoded_offset
            == Some(declaration.declaration_decoded_offset as usize)
        && binding.local_type_byte
            == Some(match declaration.kind {
                Kind::Float => 0,
                Kind::Integer => 1,
                _ => return false,
            })
}

impl World<'_> {
    /// Evaluate only explicit engineering own numeric bit copies, in physical
    /// source order. A private overlay is discarded on any refusal; only the
    /// final duplicate-free destinations enter one existing opaque transaction.
    pub fn stage_source_multi_copy_with_sources(
        &self,
        sequence: u64,
        sources: &PreparedSources<'_>,
        content: &Content,
        intent: Intent,
        limits: MultiLimits,
    ) -> Result<MultiPreparation, MultiError> {
        let frame =
            self.prepare_event_with_sources(sequence, sources, limits.maximum_event_instructions)?;
        if intent == Intent::Faithful {
            return Ok(multi_unsupported(
                Unsupported::UnverifiedRetailSemantics,
                "Original conversion and event lifecycle rules are unverified",
            ));
        }
        content.validate_world(self)?;
        let instructions = frame.instructions();
        let statement_count = instructions.len() - 2;
        if statement_count == 0 {
            return Ok(multi_unsupported(
                Unsupported::EventShape,
                "Engineering multi-copy needs at least one complete assignment",
            ));
        }
        if statement_count > limits.maximum_statements {
            return Err(MultiError::Capacity("statements"));
        }
        let begin = instructions[0].bytes.start;
        let end = instructions.last().expect("checked end").bytes.end;
        let all_bindings = &frame.source().bindings().uses;
        if all_bindings.len() > limits.observation.maximum_binding_uses {
            return Err(MultiError::Capacity("binding uses"));
        }
        let mut uses = 0_usize;
        for binding in all_bindings {
            if (begin..end).contains(&binding.scda_offset) {
                if uses >= limits.maximum_operand_uses {
                    return Err(MultiError::Capacity("operand uses"));
                }
                uses += 1;
            }
        }
        // Charge temporary binding-index retention before allocation.
        uses.checked_add(frame.pending().context.arguments.len())
            .and_then(|rows| rows.checked_add(frame.instance().context().arguments.len()))
            .filter(|&rows| rows <= limits.observation.maximum_rows)
            .ok_or(MultiError::Capacity("observation rows"))?;
        let bindings: Vec<_> = all_bindings
            .iter()
            .filter(|binding| (begin..end).contains(&binding.scda_offset))
            .collect();
        let mut statements = Vec::with_capacity(statement_count);
        let mut overlay = std::collections::BTreeMap::<u32, u64>::new();
        let mut statement_bytes = 0_usize;
        let instance = frame.instance();
        for (ordinal, instruction) in instructions[1..instructions.len() - 1].iter().enumerate() {
            let index = frame.selected().begin_instruction + 1 + ordinal;
            let Some(statement) = frame.source().statement(index) else {
                return Ok(multi_unsupported(
                    Unsupported::EventShape,
                    "Every body instruction must be a supported own assignment",
                ));
            };
            let StatementKind::Assignment(Target::Local {
                type_byte: b'f' | b's',
                index: destination,
                context_reference: None,
            }) = statement.kind()
            else {
                return Ok(multi_unsupported(
                    Unsupported::DestinationShape,
                    "Only own numeric destinations are admitted",
                ));
            };
            let [token] = statement.plan().tokens() else {
                return Ok(multi_unsupported(
                    Unsupported::ExpressionShape,
                    "Only one own numeric read is admitted",
                ));
            };
            let expression::Kind::Local {
                type_byte: b'f' | b's' | b'l',
                index: source,
                context_reference: None,
            } = &token.kind
            else {
                return Ok(multi_unsupported(
                    Unsupported::ExpressionShape,
                    "Literal, conversion, operator, call, global and foreign reads remain unsupported",
                ));
            };
            statement_bytes = statement_bytes
                .checked_add(instruction.bytes.len())
                .filter(|&bytes| bytes <= limits.maximum_statement_bytes)
                .ok_or(MultiError::Capacity("statement bytes"))?;
            let (source, destination) = (u32::from(*source), u32::from(*destination));
            let declarations = &instance.definition_schema.locals;
            let (Some(read), Some(write)) =
                (declarations.get(&source), declarations.get(&destination))
            else {
                return Ok(multi_unsupported(
                    Unsupported::LiveOperandUnavailable,
                    "A local declaration is missing",
                ));
            };
            if !matches!(read.kind, Kind::Float | Kind::Integer)
                || !matches!(write.kind, Kind::Float | Kind::Integer)
            {
                return Ok(multi_unsupported(
                    Unsupported::NonNumericLocal,
                    "Both slots must be own numeric declarations",
                ));
            }
            let Some(pair) = bindings.get(ordinal * 2..ordinal * 2 + 2) else {
                return Ok(multi_unsupported(
                    Unsupported::LiveOperandUnavailable,
                    "An exact source binding pair is missing",
                ));
            };
            if !own_binding(
                pair[0],
                destination,
                instruction.operand_offset + 1,
                2,
                write,
            ) || !own_binding(
                pair[1],
                source,
                statement.expression_scda_offset() + token.bytes.start + 1,
                4,
                read,
            ) {
                return Ok(multi_unsupported(
                    Unsupported::LiveOperandUnavailable,
                    "Source bindings differ from the exact assignment",
                ));
            }
            let canonical = instance
                .locals()
                .get(&source)
                .ok_or(crate::Error::MissingLocal(source))?;
            let from_overlay = overlay.get(&source).copied();
            let bits = match (from_overlay, canonical) {
                (Some(bits), _) => bits,
                (None, Value::Number { bits }) => *bits,
                _ => {
                    return Ok(multi_unsupported(
                        Unsupported::LiveOperandUnavailable,
                        "An own source has no initialized Number or preceding overlay write",
                    ));
                }
            };
            let destination_before = overlay.get(&destination).map_or_else(
                || {
                    instance
                        .locals()
                        .get(&destination)
                        .cloned()
                        .ok_or(crate::Error::MissingLocal(destination))
                },
                |bits| Ok(Value::Number { bits: *bits }),
            )?;
            statements.push(MultiStatement {
                instruction_index: index,
                statement_scda_bytes: instruction.bytes.clone(),
                source_token_scda_bytes: (statement.expression_scda_offset() + token.bytes.start)
                    ..(statement.expression_scda_offset() + token.bytes.end),
                destination_operand: ordinal * 2,
                source_operand: ordinal * 2 + 1,
                source_index: source,
                destination_index: destination,
                canonical_source_before: canonical.clone(),
                source_from_overlay: from_overlay.is_some(),
                destination_before,
                copied_bits: bits,
            });
            overlay.insert(destination, bits);
        }
        if bindings.len() != statement_count * 2 {
            return Ok(multi_unsupported(
                Unsupported::LiveOperandUnavailable,
                "Unexpected extra event source bindings",
            ));
        }
        let observation = preparation::EventObservation::capture(&frame, &[], limits.observation)?;
        let assignments: Vec<_> = overlay
            .into_iter()
            .map(|(index, bits)| (index, Value::Number { bits }))
            .collect();
        let changes = self.stage_event_changes(sequence, &assignments, true)?;
        Ok(MultiPreparation::Staged(Box::new(StagedMultiCopy {
            changes,
            trace: MultiTrace {
                intent,
                frame: observation,
                statements,
                statement_bytes,
                original_behavior_verified: false,
            },
        })))
    }
}

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
