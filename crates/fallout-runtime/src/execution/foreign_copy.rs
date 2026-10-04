//! Explicit engineering foreign numeric read -> own numeric bit copy. Canonical
//! live target resolution and the opaque own-event transaction remain unchanged.
use super::{local_copy, native};
use crate::{
    World, event_operands,
    foreign::{self, Content},
    identity::{ReferenceId, ReferenceValue, Value},
    preparation,
    programs::PreparedSources,
    schema::Kind,
    state::event_commit::{Receipt, StagedEventChanges},
};
use fallout_data::{
    loaded_scripts::ReferenceStatus,
    obscript::expression::{self, StatementKind, Target},
};
use serde::Serialize;
use std::{io::Write, ops::Range};

pub use local_copy::Intent;
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub sequence: u64,
    pub explicit_player: Option<ReferenceId>,
    pub intent: Intent,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_event_instructions: usize,
    pub maximum_operand_uses: usize,
    pub maximum_statement_bytes: usize,
    pub observation: preparation::ObservationLimits,
    pub maximum_metadata_rows: usize,
    /// Conservative UTF-8 reservation for transient probe/resolver copies.
    pub maximum_probe_variable_bytes: usize,
    /// Whole serialized diagnostic trace, charged before staging.
    pub maximum_trace_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 4096,
            maximum_operand_uses: 3,
            maximum_statement_bytes: 65_539,
            observation: Default::default(),
            maximum_metadata_rows: 4096,
            maximum_probe_variable_bytes: 1024 * 1024,
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
    Content(#[from] foreign::Failure),
    #[error("foreign-copy must name the existing journal head")]
    HeadChanged,
    #[error("foreign-copy budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("foreign-copy trace encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
}
pub enum Preparation {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: String,
    },
    Staged(Box<StagedForeignCopy>),
}
fn unsupported(reason: local_copy::Unsupported, detail: impl ToString) -> Preparation {
    Preparation::Unsupported {
        reason,
        detail: detail.to_string(),
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Counts {
    pub metadata_rows: usize,
    pub probe_variable_reservation: usize,
}
#[derive(Debug, Serialize)]
pub struct Trace {
    pub intent: Intent,
    pub frame: preparation::EventObservation,
    pub explicit_player: Option<ReferenceId>,
    pub instruction_index: usize,
    pub statement_scda_bytes: Range<usize>,
    pub context_token_scda_bytes: Range<usize>,
    pub source_token_scda_bytes: Range<usize>,
    pub source_reference_index: u16,
    pub source_index: u32,
    pub destination_index: u32,
    pub foreign_read: foreign::Read,
    pub copied_bits: u64,
    pub counts: Counts,
    pub original_behavior_verified: bool,
}
#[derive(Debug)]
#[must_use = "a foreign read has no effects until the own-instance stage commits"]
pub struct StagedForeignCopy {
    changes: StagedEventChanges,
    trace: Trace,
}
#[derive(Debug, Serialize)]
pub struct Committed {
    pub trace: Trace,
    pub receipt: Receipt,
}
impl StagedForeignCopy {
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
                "foreign-copy trace byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Size metadata only. A live context's authored origin may itself be large and
// later appear in a resolver failure. No value/owner is resolved or selected here.
fn context_name_bytes(
    world: &World<'_>,
    instance: &crate::state::Instance,
    index: u16,
    player: Option<ReferenceId>,
) -> usize {
    let payload = native::reference_variable_bytes(world, instance, index);
    let reference = world
        .catalogue()
        .get_handle(instance.definition())
        .and_then(|script| script.reference(u32::from(index)));
    let live = match reference {
        Some(reference) if reference.status == ReferenceStatus::DynamicVariable => {
            match instance.locals().get(&reference.value) {
                Some(Value::Reference {
                    value: ReferenceValue::Live { id },
                }) => Some(*id),
                _ => None,
            }
        }
        Some(reference) if reference.status == ReferenceStatus::RuntimeDependency => player,
        _ => None,
    };
    let origin = live
        .and_then(|id| world.reference_origin(id).ok().flatten())
        .map_or(0, |key| key.origin_plugin.len());
    // A dynamic Content value can itself name the player binding. Charge an
    // explicitly supplied player's origin before either canonical resolver.
    let player_origin = player
        .and_then(|id| world.reference_origin(id).ok().flatten())
        .map_or(0, |key| key.origin_plugin.len());
    payload.max(origin).max(player_origin)
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
        .is_none_or(|pending| pending.sequence != selection.sequence)
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
            "Original foreign assignment conversion and event lifetime are unverified",
        ));
    }
    content.validate_world(world)?;
    let [_, instruction, _] = frame.instructions() else {
        return Ok(unsupported(
            local_copy::Unsupported::EventShape,
            "Engineering foreign copy requires Begin/one Assignment/End",
        ));
    };
    let instruction_index = frame.selected().begin_instruction + 1;
    let Some(statement) = frame.source().statement(instruction_index) else {
        return Ok(unsupported(
            local_copy::Unsupported::EventShape,
            "The sole source body instruction is not an assignment",
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
            "Only an own numeric destination is admitted",
        ));
    };
    let [prefix, token] = statement.plan().tokens() else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only an exact source reference prefix and one foreign numeric local are admitted",
        ));
    };
    let expression::Kind::ReferencePrefix {
        reference_index: context,
    } = prefix.kind
    else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "A source reference prefix is required",
        ));
    };
    let expression::Kind::Local {
        type_byte: b'f' | b's' | b'l',
        index: source,
        context_reference: Some(consumed),
    } = token.kind
    else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only a qualified numeric local read is admitted",
        ));
    };
    if context != consumed {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "The source prefix is not consumed by this local",
        ));
    }
    if instruction.bytes.len() > limits.maximum_statement_bytes {
        return Err(Error::Capacity("statement bytes"));
    }
    let all = &frame.source().bindings().uses;
    if all.len() > limits.observation.maximum_binding_uses {
        return Err(Error::Capacity("binding uses"));
    }
    let begin = frame.instructions()[0].bytes.start;
    let end = frame.instructions()[2].bytes.end;
    let mut bindings = all
        .iter()
        .filter(|binding| (begin..end).contains(&binding.scda_offset));
    let Some(write) = bindings.next() else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Missing destination binding",
        ));
    };
    let Some(reference) = bindings.next() else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Missing prefix binding",
        ));
    };
    let Some(read) = bindings.next() else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Missing foreign read binding",
        ));
    };
    if limits.maximum_operand_uses < 3 {
        return Err(Error::Capacity("operand uses"));
    }
    if bindings.next().is_some() {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Unexpected extra source binding",
        ));
    }
    let Some(declaration) = frame
        .instance()
        .definition_schema
        .locals
        .get(&u32::from(*destination))
    else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Missing own destination declaration",
        ));
    };
    if !matches!(declaration.kind, Kind::Float | Kind::Integer) {
        return Ok(unsupported(
            local_copy::Unsupported::NonNumericLocal,
            "Destination is not a numeric declaration",
        ));
    }
    let expression = statement.expression_scda_offset();
    if !local_copy::own_binding(
        write,
        u32::from(*destination),
        instruction.operand_offset + 1,
        2,
        declaration,
    ) || reference.role != 7
        || reference.scda_offset != expression + prefix.bytes.start + 1
        || reference.index != context
        || reference.context_reference.is_some()
        || !matches!(reference.status, 2 | 3)
        || reference.reference_field_decoded_offset.is_none()
        || read.role != 4
        || read.scda_offset != expression + token.bytes.start + 1
        || read.index != source
        || read.context_reference != Some(context)
        || read.status != 4
        || read.target_value != Some(u32::from(source))
        || read.reference_field_decoded_offset != reference.reference_field_decoded_offset
        || read.context_target_kind != if reference.status == 2 { 1 } else { 2 }
        || Some(read.context_target_value) != reference.target_value
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Exact source binding association differs",
        ));
    }
    // The source projection charges source/context/own-value payload before it
    // copies. Bound every source-name metadata row and borrowed context name;
    // canonical names have no maximum. The existing probe temporarily retains
    // both its target and read_foreign's target (three names each), plus the
    // prefix value: seven names and fixed failure text, beyond frame strings.
    let observation = preparation::EventObservation::capture(
        &frame,
        &[u32::from(*destination)],
        limits.observation,
    )?;
    let receipts = &world.catalogue().sources;
    if receipts.len() > limits.maximum_metadata_rows {
        return Err(Error::Capacity("metadata rows"));
    }
    let maximum_name = receipts
        .iter()
        .map(|receipt| receipt.source_name.len())
        .max()
        .unwrap_or(0)
        .max(context_name_bytes(
            world,
            frame.instance(),
            context,
            selection.explicit_player,
        ));
    let reservation = observation
        .counts
        .variable_bytes
        .checked_mul(2)
        .and_then(|bytes| {
            maximum_name
                .checked_mul(7)
                .and_then(|names| bytes.checked_add(names))
        })
        .and_then(|bytes| bytes.checked_add(1024))
        .filter(|&bytes| bytes <= limits.maximum_probe_variable_bytes)
        .ok_or(Error::Capacity("probe variable bytes"))?;
    let request = foreign::Request {
        source: world.handle(frame.instance().id())?,
        context_reference: context,
        local_index: source,
        player: selection.explicit_player,
    };
    let target = match world.foreign_target(content, request) {
        Ok(target) => target,
        Err(error) => {
            return Ok(unsupported(
                local_copy::Unsupported::LiveOperandUnavailable,
                error,
            ));
        }
    };
    // Check declaration before the general existing probe can clone a nonnumeric
    // foreign reference value. A numeric slot contains only Number/Uninitialized.
    if !matches!(target.declaration.kind, Kind::Float | Kind::Integer) {
        return Ok(unsupported(
            local_copy::Unsupported::NonNumericLocal,
            "Foreign declaration is not numeric",
        ));
    }
    drop(target);
    let probe = world.probe_event_operands_with_sources(
        selection.sequence,
        sources,
        content,
        selection.explicit_player,
        event_operands::CachedLimits {
            maximum_event_instructions: limits.maximum_event_instructions,
            maximum_uses: limits.maximum_operand_uses,
        },
    )?;
    let mut operands = probe.operands.into_iter();
    let (Some(own), Some(caller), Some(foreign)) =
        (operands.next(), operands.next(), operands.next())
    else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Incomplete current operand probe",
        ));
    };
    if operands.next().is_some() {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Unexpected current operand",
        ));
    }
    if !matches!(own.outcome,event_operands::Outcome::Resolved {access:event_operands::Access::Destination,
        resolution:event_operands::Resolution::Local {instance,..}} if instance==frame.instance().id())
        || !matches!(
            caller.outcome,
            event_operands::Outcome::Resolved {
                access: event_operands::Access::Reference,
                resolution: event_operands::Resolution::Reference { .. }
            }
        )
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Own destination or explicit source context did not resolve",
        ));
    }
    let event_operands::Outcome::Resolved {
        access: event_operands::Access::Read,
        resolution:
            event_operands::Resolution::Foreign {
                target,
                value: Some(value @ Value::Number { .. }),
            },
    } = foreign.outcome
    else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Foreign numeric local has no initialized Number",
        ));
    };
    let Value::Number { bits } = value else {
        unreachable!("matched Number")
    };
    let trace = Trace {
        intent: selection.intent,
        frame: observation,
        explicit_player: selection.explicit_player,
        instruction_index,
        statement_scda_bytes: instruction.bytes.clone(),
        context_token_scda_bytes: expression + prefix.bytes.start..expression + prefix.bytes.end,
        source_token_scda_bytes: expression + token.bytes.start..expression + token.bytes.end,
        source_reference_index: context,
        source_index: u32::from(source),
        destination_index: u32::from(*destination),
        foreign_read: foreign::Read {
            target,
            value: Value::Number { bits },
        },
        copied_bits: bits,
        counts: Counts {
            metadata_rows: receipts.len(),
            probe_variable_reservation: reservation,
        },
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
    let changes = world.stage_event_changes(
        selection.sequence,
        &[(u32::from(*destination), Value::Number { bits })],
        true,
    )?;
    Ok(Preparation::Staged(Box::new(StagedForeignCopy {
        changes,
        trace,
    })))
}
