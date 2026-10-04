//! One exactly representable integral source token under an explicit host policy.
//! This does not establish original parsing, conversion or event lifetime rules.
use super::local_copy;
use crate::{
    World,
    foreign::Content,
    identity::Value,
    preparation,
    programs::PreparedSources,
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
    EngineeringExactIntegralDecimal,
}
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
    pub maximum_literal_bytes: usize,
    pub observation: preparation::ObservationLimits,
    pub maximum_stage_variable_bytes: usize,
    pub maximum_trace_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 4096,
            maximum_operand_uses: 1,
            maximum_statement_bytes: 65_539,
            maximum_literal_bytes: 128,
            observation: Default::default(),
            maximum_stage_variable_bytes: 3 * 1024 * 1024,
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
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("literal assignment must name the existing journal head")]
    HeadChanged,
    #[error("literal assignment budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
}
pub enum Preparation {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'static str,
    },
    Staged(Box<StagedLiteral>),
}
fn unsupported(reason: local_copy::Unsupported, detail: &'static str) -> Preparation {
    Preparation::Unsupported { reason, detail }
}
#[derive(Debug, Serialize)]
pub struct Trace {
    pub intent: Intent,
    pub frame: preparation::EventObservation,
    pub source_cohort_sha256: String,
    pub decoder_sha256: String,
    pub instruction_index: usize,
    pub statement_scda_bytes: Range<usize>,
    pub literal_scda_bytes: Range<usize>,
    pub literal_bytes: Vec<u8>,
    pub destination_index: u32,
    pub assigned_bits: u64,
    pub stage_variable_reservation: usize,
    pub original_behavior_verified: bool,
}
#[derive(Debug)]
#[must_use = "the source literal has no effects until its opaque stage commits"]
pub struct StagedLiteral {
    changes: StagedEventChanges,
    trace: Trace,
}
#[derive(Debug, Serialize)]
pub struct Committed {
    pub trace: Trace,
    pub receipt: Receipt,
}
impl StagedLiteral {
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

// Convert only bytes already admitted as one immutable Number token. The
// bounded integer domain and exact significand construction are host policy,
// not a replacement for CRT strtod or a claim about original rounding.
fn exact_integral_bits(bytes: &[u8]) -> Result<u64, &'static str> {
    let (negative, digits) = match bytes.first() {
        Some(b'-') => (true, &bytes[1..]),
        Some(b'+') => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err("Only ASCII integral decimal spellings are admitted");
    }
    let mut magnitude = 0_u128;
    for &digit in digits {
        magnitude = magnitude
            .checked_mul(10)
            .and_then(|n| n.checked_add(u128::from(digit - b'0')))
            .ok_or("Integral decimal magnitude exceeds the checked u128 domain")?;
    }
    let sign = u64::from(negative) << 63;
    if magnitude == 0 {
        return Ok(sign);
    }
    let exponent = 127 - magnitude.leading_zeros();
    let significand = if exponent > 52 {
        let discarded = exponent - 52;
        if magnitude.trailing_zeros() < discarded {
            return Err("Integral decimal value is not exactly representable as binary64");
        }
        magnitude >> discarded
    } else {
        magnitude
            .checked_shl(52 - exponent)
            .ok_or("Integral decimal significand shift overflow")?
    };
    let fraction = significand
        .checked_sub(1_u128 << 52)
        .and_then(|n| u64::try_from(n).ok())
        .filter(|&n| n < (1_u64 << 52))
        .ok_or("Integral decimal significand is out of range")?;
    let biased = exponent
        .checked_add(1023)
        .filter(|&n| n < 2047)
        .ok_or("Integral decimal exponent is out of range")?;
    Ok(sign | (u64::from(biased) << 52) | fraction)
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
            return Err(std::io::Error::other("literal trace byte budget exceeded"));
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
        return Ok(unsupported(
            local_copy::Unsupported::UnverifiedRetailSemantics,
            "Original literal conversion and event lifetime are unverified",
        ));
    }
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
    content.validate_world(world)?;
    let [_, instruction, _] = frame.instructions() else {
        return Ok(unsupported(
            local_copy::Unsupported::EventShape,
            "Engineering literal assignment requires Begin/one Assignment/End",
        ));
    };
    if instruction.bytes.len() > limits.maximum_statement_bytes {
        return Err(Error::Capacity("statement bytes"));
    }
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
            "Only own declared numeric destinations are admitted",
        ));
    };
    let [token] = statement.plan().tokens() else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only one Number token is admitted; operators and extra operands refuse",
        ));
    };
    let expression::Kind::Number(literal) = token.kind else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only a source Number token is admitted",
        ));
    };
    if literal.len() > limits.maximum_literal_bytes {
        return Err(Error::Capacity("literal bytes"));
    }
    let bits = match exact_integral_bits(literal) {
        Ok(bits) => bits,
        Err(detail) => {
            return Ok(unsupported(
                local_copy::Unsupported::ExpressionShape,
                detail,
            ));
        }
    };
    let destination_index = u32::from(*destination);
    let Some(declaration) = frame
        .instance()
        .definition_schema
        .locals
        .get(&destination_index)
    else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Own destination declaration is missing",
        ));
    };
    if !matches!(declaration.kind, Kind::Float | Kind::Integer) {
        return Ok(unsupported(
            local_copy::Unsupported::NonNumericLocal,
            "Actual source destination is not a numeric declaration",
        ));
    }
    let all = &frame.source().bindings().uses;
    if all.len() > limits.observation.maximum_binding_uses {
        return Err(Error::Capacity("binding uses"));
    }
    let begin = frame.instructions()[0].bytes.start;
    let end = frame.instructions()[2].bytes.end;
    let mut bindings = all.iter().filter(|b| (begin..end).contains(&b.scda_offset));
    let Some(write) = bindings.next() else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Exact destination binding is missing",
        ));
    };
    if limits.maximum_operand_uses < 1 {
        return Err(Error::Capacity("operand uses"));
    }
    if bindings.next().is_some()
        || !local_copy::own_binding(
            write,
            destination_index,
            instruction.operand_offset + 1,
            2,
            declaration,
        )
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Exactly one current own numeric destination binding is required",
        ));
    }
    let observation =
        preparation::EventObservation::capture(&frame, &[destination_index], limits.observation)?;
    // The bounded projection covers canonical stage identity/context strings.
    // Reserve another copy plus both additional diagnostic digests before any
    // stage/trace retention. This is logical payload, not allocator accounting.
    let reservation = observation
        .counts
        .variable_bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(sources.source_cohort_sha256().len()))
        .and_then(|n| n.checked_add(sources.decoder_sha256().len()))
        .filter(|&n| n <= limits.maximum_stage_variable_bytes)
        .ok_or(Error::Capacity("stage variable bytes"))?;
    let trace = Trace {
        intent: selection.intent,
        frame: observation,
        source_cohort_sha256: sources.source_cohort_sha256().into(),
        decoder_sha256: sources.decoder_sha256().into(),
        instruction_index,
        statement_scda_bytes: instruction.bytes.clone(),
        literal_scda_bytes: statement.expression_scda_offset() + token.bytes.start
            ..statement.expression_scda_offset() + token.bytes.end,
        literal_bytes: literal.to_vec(),
        destination_index,
        assigned_bits: bits,
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
    Ok(Preparation::Staged(Box::new(StagedLiteral {
        changes,
        trace,
    })))
}
