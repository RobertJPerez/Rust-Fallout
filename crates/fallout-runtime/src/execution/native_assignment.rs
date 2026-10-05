//! One source-native inventory read feeds an explicit engineering own assignment.
//! Original numeric returns, conversions and event lifetime remain unverified.
use super::{local_copy, native, native_plan::CallProof};
use crate::{
    World,
    foreign::Content,
    identity::Value,
    preparation,
    programs::PreparedSources,
    query,
    schema::Kind,
    state::event_commit::{Receipt, StagedEventChanges},
};
use fallout_data::obscript::expression::{self, StatementKind, Target};
use serde::Serialize;
use std::{io::Write, ops::Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Faithful,
    EngineeringExactCountToNumber,
}
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub sequence: u64,
    pub inputs: native::Inputs,
    pub intent: Intent,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub native: native::Limits,
    pub maximum_operand_uses: usize,
    pub maximum_statement_bytes: usize,
    pub observation: preparation::ObservationLimits,
    pub maximum_query_variable_bytes: usize,
    pub maximum_stage_variable_bytes: usize,
    pub maximum_inventory_visits: usize,
    pub maximum_contributions: usize,
    pub maximum_trace_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            native: native::Limits {
                maximum_event_instructions: 4096,
                maximum_calls: 1,
                maximum_argument_bytes: 1024,
            },
            maximum_operand_uses: 3,
            maximum_statement_bytes: 65_539,
            observation: Default::default(),
            maximum_query_variable_bytes: 1024,
            maximum_stage_variable_bytes: 3 * 1024 * 1024,
            maximum_inventory_visits: 65_536,
            maximum_contributions: 4096,
            maximum_trace_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Native(#[from] native::Error),
    #[error(transparent)]
    Observation(#[from] preparation::ObservationError),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("native assignment must name the existing journal head")]
    HeadChanged,
    #[error("native assignment budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unsupported {
    Source { reason: local_copy::Unsupported },
    Native { reason: native::Unsupported },
    InexactCount,
}
pub enum Preparation {
    Unsupported { reason: Unsupported, detail: String },
    Staged(Box<StagedAssignment>),
}
fn source_unsupported(reason: local_copy::Unsupported, detail: &'static str) -> Preparation {
    Preparation::Unsupported {
        reason: Unsupported::Source { reason },
        detail: detail.into(),
    }
}
fn native_unsupported(reason: native::Unsupported, detail: impl ToString) -> Preparation {
    Preparation::Unsupported {
        reason: Unsupported::Native { reason },
        detail: detail.to_string(),
    }
}
#[derive(Debug, Serialize)]
pub struct Trace {
    pub intent: Intent,
    pub frame: preparation::EventObservation,
    pub source_cohort_sha256: String,
    pub decoder_sha256: String,
    pub call: CallProof,
    pub supplied_subject: Option<crate::identity::ReferenceId>,
    pub explicit_player: Option<crate::identity::ReferenceId>,
    pub statement_scda_bytes: Range<usize>,
    pub destination_index: u32,
    pub query: query::Trace,
    pub assigned_bits: u64,
    pub inventory_visits: usize,
    pub stage_variable_reservation: usize,
    pub original_behavior_verified: bool,
}
#[derive(Debug)]
#[must_use = "a native assignment has no effects until its opaque stage commits"]
pub struct StagedAssignment {
    changes: StagedEventChanges,
    trace: Trace,
}
#[derive(Debug, Serialize)]
pub struct Committed {
    pub trace: Trace,
    pub receipt: Receipt,
}
impl StagedAssignment {
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
// Exact positive integer words, without a floating-point cast or rounding.
fn exact_count_bits(count: u64) -> Option<u64> {
    if count == 0 {
        return Some(0);
    }
    let exponent = 63 - count.leading_zeros();
    let significand = if exponent > 52 {
        let discard = exponent - 52;
        if count.trailing_zeros() < discard {
            return None;
        }
        count >> discard
    } else {
        count.checked_shl(52 - exponent)?
    };
    let fraction = significand.checked_sub(1_u64 << 52)?;
    Some((u64::from(exponent.checked_add(1023)?) << 52) | fraction)
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
                "native assignment trace byte budget exceeded",
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
    if selection.intent == Intent::Faithful {
        return Ok(source_unsupported(
            local_copy::Unsupported::UnverifiedRetailSemantics,
            "Original native numeric return, assignment conversion and event lifetime are unverified",
        ));
    }
    if world
        .pending_events()
        .next()
        .is_none_or(|p| p.sequence != selection.sequence)
    {
        return Err(Error::HeadChanged);
    }
    content.validate_world(world)?;
    let calls =
        world.prepare_native_calls_with_sources(selection.sequence, sources, limits.native)?;
    let frame = calls.frame();
    let [_, instruction, _] = frame.instructions() else {
        return Ok(source_unsupported(
            local_copy::Unsupported::EventShape,
            "Exactly Begin/one Assignment/End is required",
        ));
    };
    if instruction.bytes.len() > limits.maximum_statement_bytes {
        return Err(Error::Capacity("statement bytes"));
    }
    let instruction_index = frame.selected().begin_instruction + 1;
    let Some(statement) = frame.source().statement(instruction_index) else {
        return Ok(source_unsupported(
            local_copy::Unsupported::EventShape,
            "The sole body instruction is not an assignment",
        ));
    };
    let StatementKind::Assignment(Target::Local {
        type_byte: b'f' | b's',
        index: destination,
        context_reference: None,
    }) = statement.kind()
    else {
        return Ok(source_unsupported(
            local_copy::Unsupported::DestinationShape,
            "Only an own numeric destination is admitted",
        ));
    };
    let (prefix, token_index, token) = match statement.plan().tokens() {
        [token] => (None, 0, token),
        [prefix, token] if matches!(prefix.kind, expression::Kind::ReferencePrefix { .. }) => {
            (Some(prefix), 1, token)
        }
        _ => {
            return Ok(source_unsupported(
                local_copy::Unsupported::ExpressionShape,
                "Only one source command with an optional caller prefix is admitted",
            ));
        }
    };
    let expression::Kind::Command {
        opcode,
        context_reference,
        ..
    } = token.kind
    else {
        return Ok(source_unsupported(
            local_copy::Unsupported::ExpressionShape,
            "One GetItemCount Command token is required",
        ));
    };
    let [call] = calls.calls() else {
        return Ok(source_unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Exactly one source-native occurrence is required",
        ));
    };
    if opcode != query::GET_ITEM_COUNT_COMMAND {
        return Ok(native_unsupported(
            native::Unsupported::MissingImplementation,
            "The engineering assignment supports only GetItemCount",
        ));
    }
    let expression = statement.expression_scda_offset();
    if call.command_id != opcode
        || call.instruction_index != instruction_index
        || call.location != (native::Location::Expression { token_index })
        || call.calling_reference_index != context_reference
        || call.scda_bytes != (expression + token.bytes.start..expression + token.bytes.end)
    {
        return Ok(source_unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Native occurrence differs from the exact source token",
        ));
    }
    let destination_index = u32::from(*destination);
    let Some(declaration) = frame
        .instance()
        .definition_schema
        .locals
        .get(&destination_index)
    else {
        return Ok(source_unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Own destination declaration is absent",
        ));
    };
    if !matches!(declaration.kind, Kind::Float | Kind::Integer) {
        return Ok(source_unsupported(
            local_copy::Unsupported::NonNumericLocal,
            "Destination is not a numeric declaration",
        ));
    }
    let all = &frame.source().bindings().uses;
    if all.len() > limits.observation.maximum_binding_uses {
        return Err(Error::Capacity("binding uses"));
    }
    let begin = frame.instructions()[0].bytes.start;
    let end = frame.instructions()[2].bytes.end;
    let mut bindings = all.iter().filter(|b| (begin..end).contains(&b.scda_offset));
    let expected_uses = 2 + usize::from(prefix.is_some());
    if expected_uses > limits.maximum_operand_uses {
        return Err(Error::Capacity("operand uses"));
    }
    let Some(write) = bindings.next() else {
        return Ok(source_unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Destination binding is absent",
        ));
    };
    if !local_copy::own_binding(
        write,
        destination_index,
        instruction.operand_offset + 1,
        2,
        declaration,
    ) {
        return Ok(source_unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Exact own numeric destination binding is required",
        ));
    }
    let mut maximum_name = 0_usize;
    if let Some(prefix) = prefix {
        let expression::Kind::ReferencePrefix { reference_index } = prefix.kind else {
            unreachable!("checked source prefix")
        };
        let Some(binding) = bindings.next() else {
            return Ok(source_unsupported(
                local_copy::Unsupported::LiveOperandUnavailable,
                "Caller prefix binding is missing",
            ));
        };
        if context_reference != Some(reference_index)
            || binding.role != 7
            || binding.index != reference_index
            || binding.scda_offset != expression + prefix.bytes.start + 1
        {
            return Ok(source_unsupported(
                local_copy::Unsupported::LiveOperandUnavailable,
                "Caller prefix and source binding differ",
            ));
        }
        maximum_name = native::reference_variable_bytes(world, frame.instance(), reference_index);
    } else if context_reference.is_some() {
        return Ok(source_unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Source caller prefix is absent",
        ));
    }
    let Some(argument) = bindings.next() else {
        return Ok(source_unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Source argument binding is missing",
        ));
    };
    if bindings.next().is_some()
        || argument.role != 10
        || argument.scda_offset != call.argument_scda_offset + 3
    {
        return Ok(source_unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "One source reference-table argument binding is required",
        ));
    }
    maximum_name = maximum_name.max(native::reference_variable_bytes(
        world,
        frame.instance(),
        argument.index,
    ));
    if maximum_name > limits.maximum_query_variable_bytes {
        return Err(Error::Capacity("query variable bytes"));
    }
    let observation =
        preparation::EventObservation::capture(frame, &[destination_index], limits.observation)?;
    // Conservative logical reservation covers projection/stage identity, fresh
    // resolver/query/count-key clones and both added source diagnostic digests.
    let reservation = observation
        .counts
        .variable_bytes
        .checked_mul(2)
        .and_then(|n| maximum_name.checked_mul(7).and_then(|m| n.checked_add(m)))
        .and_then(|n| n.checked_add(sources.source_cohort_sha256().len()))
        .and_then(|n| n.checked_add(sources.decoder_sha256().len()))
        .and_then(|n| n.checked_add(1024))
        .filter(|&n| n <= limits.maximum_stage_variable_bytes)
        .ok_or(Error::Capacity("stage variable bytes"))?;
    let resolved = match calls.admit_occurrence(
        0,
        selection.inputs,
        native::Intent::EngineeringObservation,
        limits.maximum_query_variable_bytes,
    )? {
        native::Admission::Unsupported { reason, detail } => {
            return Ok(native_unsupported(reason, detail));
        }
        native::Admission::Resolved(resolved) => resolved,
    };
    let inventory = match world.inventory_items(resolved.subject) {
        Ok(items) => items,
        Err(error) => {
            return Ok(native_unsupported(
                native::Unsupported::HostQueryUnavailable,
                error,
            ));
        }
    };
    let mut inventory_visits = 0_usize;
    for _ in inventory {
        inventory_visits = inventory_visits
            .checked_add(1)
            .filter(|&n| n <= limits.maximum_inventory_visits)
            .ok_or(Error::Capacity("inventory visits"))?;
    }
    // The existing query scans this bank again. Admit both passes before it
    // allocates contributions; the trace reports their cumulative row visits.
    inventory_visits = inventory_visits
        .checked_mul(2)
        .filter(|&n| n <= limits.maximum_inventory_visits)
        .ok_or(Error::Capacity("inventory visits"))?;
    let result = query::Request::prepare(
        world,
        query::Entry::Native { command_id: opcode },
        Some(resolved.subject),
        std::slice::from_ref(&resolved.argument),
    )
    .and_then(|request| request.evaluate(world, content, limits.maximum_contributions));
    let query = match result {
        Ok(trace) => trace,
        Err(query::Failure::UnverifiedFormList) => {
            return Ok(native_unsupported(
                native::Unsupported::UnverifiedFormList,
                "Original form-list expansion remains unverified",
            ));
        }
        Err(query::Failure::State(crate::Error::Capacity(_))) => {
            return Err(Error::Capacity("query contributions"));
        }
        Err(error) => {
            return Ok(native_unsupported(
                native::Unsupported::HostQueryUnavailable,
                error,
            ));
        }
    };
    let Some(bits) = exact_count_bits(query.query.result) else {
        return Ok(Preparation::Unsupported {
            reason: Unsupported::InexactCount,
            detail: "Inventory count is not exactly representable as binary64".into(),
        });
    };
    let trace = Trace {
        intent: selection.intent,
        frame: observation,
        source_cohort_sha256: sources.source_cohort_sha256().into(),
        decoder_sha256: sources.decoder_sha256().into(),
        call: CallProof {
            command_id: call.command_id,
            instruction_index: call.instruction_index,
            scda_bytes: call.scda_bytes.clone(),
            instruction_scda_bytes: call.instruction_scda_bytes.clone(),
            argument_scda_bytes: call.argument_scda_offset
                ..call.argument_scda_offset + call.raw_arguments.len(),
            calling_reference_index: call.calling_reference_index,
            location: call.location.clone(),
        },
        supplied_subject: selection.inputs.supplied_subject,
        explicit_player: selection.inputs.player,
        statement_scda_bytes: instruction.bytes.clone(),
        destination_index,
        query,
        assigned_bits: bits,
        inventory_visits,
        stage_variable_reservation: reservation,
        original_behavior_verified: false,
    };
    let mut admitted = TraceAdmission {
        bytes: 0,
        maximum: limits.maximum_trace_bytes,
        exceeded: false,
    };
    let encoded = serde_json::to_writer(&mut admitted, &trace);
    if admitted.exceeded {
        return Err(Error::Capacity("trace bytes"));
    }
    encoded?;
    let changes = world.stage_event_changes(
        selection.sequence,
        &[(destination_index, Value::Number { bits })],
        true,
    )?;
    Ok(Preparation::Staged(Box::new(StagedAssignment {
        changes,
        trace,
    })))
}

#[cfg(test)]
mod tests {
    use super::exact_count_bits;
    #[test]
    fn exact_count_conversion_has_independent_boundary_words_and_refuses_loss() {
        for (count, bits) in [
            (0, 0),
            (1, 0x3ff0000000000000),
            (19, 0x4033000000000000),
            (9_007_199_254_740_991, 0x433fffffffffffff),
            (9_007_199_254_740_992, 0x4340000000000000),
            (9_007_199_254_740_994, 0x4340000000000001),
            (18_446_744_073_709_549_568, 0x43efffffffffffff),
        ] {
            assert_eq!(exact_count_bits(count), Some(bits));
        }
        assert_eq!(exact_count_bits(9_007_199_254_740_993), None);
        assert_eq!(exact_count_bits(u64::MAX), None);
    }
}
