//! Exact source reference identity under an explicit engineering host policy.
//! Source layout and canonical storage do not establish original VM semantics.
use super::{local_copy, native};
use crate::{
    World,
    foreign::Content,
    identity::{ReferenceId, ReferenceValue, Value},
    preparation,
    programs::PreparedSources,
    schema::Kind,
    state::event_commit::{Receipt, StagedEventChanges},
};
use fallout_data::{
    loaded_scripts::{Declaration, Reference, Version},
    obscript::expression::{self, StatementKind, Target},
};
use serde::Serialize;
use std::{io::Write, ops::Range};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Faithful,
    EngineeringIdentityAssignment,
}
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
    pub maximum_reference_variable_bytes: usize,
    pub maximum_stage_variable_bytes: usize,
    pub maximum_trace_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_event_instructions: 4096,
            maximum_operand_uses: 2,
            maximum_statement_bytes: 65_539,
            observation: Default::default(),
            maximum_reference_variable_bytes: 64 * 1024,
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
    #[error("reference literal assignment must name the existing journal head")]
    HeadChanged,
    #[error("reference literal assignment budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
}
pub enum Preparation {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'static str,
    },
    Staged(Box<StagedReferenceLiteral>),
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
    pub script_source: Version,
    pub instruction_index: usize,
    pub statement_scda_bytes: Range<usize>,
    pub literal_scda_bytes: Range<usize>,
    pub source_reference_field_decoded_bytes: Range<usize>,
    pub source_reference: Reference,
    pub source_local_declaration: Option<Declaration>,
    pub destination_index: u32,
    pub explicit_player: Option<ReferenceId>,
    pub assigned_reference: ReferenceValue,
    pub stage_variable_reservation: usize,
    pub original_behavior_verified: bool,
}
#[derive(Debug)]
#[must_use = "a reference literal has no effects until its opaque stage commits"]
pub struct StagedReferenceLiteral {
    changes: StagedEventChanges,
    trace: Trace,
}
#[derive(Debug, Serialize)]
pub struct Committed {
    pub trace: Trace,
    pub receipt: Receipt,
}
impl StagedReferenceLiteral {
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
                "reference literal trace byte budget exceeded",
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
        return Ok(unsupported(
            local_copy::Unsupported::UnverifiedRetailSemantics,
            "Original reference literal assignment and event lifetime are unverified",
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
            "Engineering reference literal assignment requires Begin/one Assignment/End",
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
            "Only an own reference local destination is admitted",
        ));
    };
    let [token] = statement.plan().tokens() else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only one source ReferenceLiteral token is admitted",
        ));
    };
    let expression::Kind::ReferenceLiteral { reference_index } = token.kind else {
        return Ok(unsupported(
            local_copy::Unsupported::ExpressionShape,
            "Only a source ReferenceLiteral token is admitted",
        ));
    };
    let destination_index = u32::from(*destination);
    let script = frame.source().source();
    let Some(destination_schema) = frame
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
    let Some(destination_authored) = script.declaration(destination_index) else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Authored destination declaration is missing",
        ));
    };
    if destination_schema.kind != Kind::Reference {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Actual own destination is not Reference storage",
        ));
    }
    let all = &frame.source().bindings().uses;
    if all.len() > limits.observation.maximum_binding_uses {
        return Err(Error::Capacity("binding uses"));
    }
    let begin = frame.instructions()[0].bytes.start;
    let end = frame.instructions()[2].bytes.end;
    let mut bindings = all.iter().filter(|b| (begin..end).contains(&b.scda_offset));
    let (Some(write), Some(literal)) = (bindings.next(), bindings.next()) else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Exact destination/literal bindings are missing",
        ));
    };
    if limits.maximum_operand_uses < 2 {
        return Err(Error::Capacity("operand uses"));
    }
    let Some(reference) = script.reference(u32::from(reference_index)) else {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Source reference table index is absent",
        ));
    };
    let source_declaration = if reference.source_kind == "SCRV" {
        script.declaration(reference.value)
    } else {
        None
    };
    let source_valid = match reference.source_kind.as_str() {
        "SCRO" => {
            literal.status == 2
                && literal.local_declaration_decoded_offset.is_none()
                && literal.local_type_byte.is_none()
        }
        "SCRV" => source_declaration.is_some_and(|declaration| {
            literal.status == 3
                && literal.local_declaration_decoded_offset
                    == Some(declaration.decoded_offset as usize)
                && reference.variable_declaration_offset == Some(declaration.decoded_offset)
                && literal.local_type_byte == Some(declaration.type_byte)
                && frame
                    .instance()
                    .definition_schema
                    .locals
                    .get(&reference.value)
                    .is_some_and(|schema| {
                        schema.kind == Kind::Reference
                            && schema.declaration_decoded_offset == declaration.decoded_offset
                    })
        }),
        _ => false,
    };
    let expression_offset = statement.expression_scda_offset();
    if bindings.next().is_some()
        || destination_authored.decoded_offset != destination_schema.declaration_decoded_offset
        || write.role != 2
        || write.scda_offset != instruction.operand_offset + 1
        || u32::from(write.index) != destination_index
        || write.context_reference.is_some()
        || write.status != 1
        || write.target_value != Some(destination_index)
        || write.local_declaration_decoded_offset
            != Some(destination_authored.decoded_offset as usize)
        || write.local_type_byte != Some(destination_authored.type_byte)
        || literal.role != 6
        || literal.scda_offset != expression_offset + token.bytes.start + 1
        || literal.index != reference_index
        || literal.context_reference.is_some()
        || literal.target_value != Some(reference.value)
        || literal.reference_field_decoded_offset != Some(reference.decoded_offset as usize)
        || !source_valid
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Current source reference table and own declaration bindings differ",
        ));
    }
    let reference_bytes =
        native::reference_variable_bytes(world, frame.instance(), reference_index);
    if reference_bytes > limits.maximum_reference_variable_bytes {
        return Err(Error::Capacity("reference variable bytes"));
    }
    let version = script.version();
    let mut source_bytes = 0usize;
    for value in [
        reference.source_kind.as_str(),
        reference
            .form_key
            .as_ref()
            .map_or("", |key| key.origin_plugin.as_str()),
        reference
            .target
            .as_ref()
            .map_or("", |target| target.source_plugin.as_str()),
        reference
            .target
            .as_ref()
            .map_or("", |target| target.record_kind.as_str()),
        source_declaration.map_or("", |declaration| declaration.name_sha256.as_str()),
        version.source_plugin.as_str(),
        version.source_sha256.as_str(),
        version.decoded_record_sha256.as_str(),
        version.metadata_sha256.as_str(),
        version.compiled_sha256.as_deref().unwrap_or(""),
    ] {
        source_bytes = source_bytes
            .checked_add(value.len())
            .ok_or(Error::Capacity("stage variable bytes"))?;
    }
    // Charge borrowed extents before canonical resolution can clone a content
    // key. Cover trace/stage identity, source metadata, normalizations and the
    // resolved value copies. This is logical payload, not peak heap accounting.
    let fixed_reservation = source_bytes
        .checked_mul(4)
        .and_then(|n| {
            reference_bytes
                .checked_mul(6)
                .and_then(|m| n.checked_add(m))
        })
        .and_then(|n| n.checked_add(sources.source_cohort_sha256().len()))
        .and_then(|n| n.checked_add(sources.decoder_sha256().len()))
        .and_then(|n| n.checked_add(256))
        .filter(|&n| n <= limits.maximum_stage_variable_bytes)
        .ok_or(Error::Capacity("stage variable bytes"))?;
    let selected = [destination_index, reference.value];
    let locals = if source_declaration.is_some() && reference.value != destination_index {
        &selected[..]
    } else {
        &selected[..1]
    };
    // Restrict the existing observation's preflight with the remaining global
    // reservation before it clones any selected strings or contexts.
    let observation_limits = preparation::ObservationLimits {
        maximum_variable_bytes: limits
            .observation
            .maximum_variable_bytes
            .min((limits.maximum_stage_variable_bytes - fixed_reservation) / 2),
        ..limits.observation
    };
    let observation = preparation::EventObservation::capture(&frame, locals, observation_limits)?;
    let reservation = observation
        .counts
        .variable_bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(fixed_reservation))
        .ok_or(Error::Capacity("stage variable bytes"))?;
    let value = match world.resolve_script_reference(
        world.handle(frame.instance().id())?,
        u32::from(reference_index),
        selection.explicit_player,
    ) {
        Ok(value) => value,
        Err(_) => {
            return Ok(unsupported(
                local_copy::Unsupported::LiveOperandUnavailable,
                "Canonical source reference resolution is unavailable",
            ));
        }
    };
    if let ReferenceValue::Content { key } = &value
        && content.source_form(world, key).is_err()
    {
        return Ok(unsupported(
            local_copy::Unsupported::LiveOperandUnavailable,
            "Resolved content reference is absent or deleted",
        ));
    }
    let field_start = reference.decoded_offset as usize;
    let field_end = field_start
        .checked_add(10)
        .ok_or(Error::Capacity("source field range"))?;
    let trace = Trace {
        intent: selection.intent,
        frame: observation,
        source_cohort_sha256: sources.source_cohort_sha256().into(),
        decoder_sha256: sources.decoder_sha256().into(),
        script_source: version.clone(),
        instruction_index,
        statement_scda_bytes: instruction.bytes.clone(),
        literal_scda_bytes: expression_offset + token.bytes.start
            ..expression_offset + token.bytes.end,
        source_reference_field_decoded_bytes: field_start..field_end,
        source_reference: reference.clone(),
        source_local_declaration: source_declaration.cloned(),
        destination_index,
        explicit_player: selection.explicit_player,
        assigned_reference: value.clone(),
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
        &[(destination_index, Value::Reference { value })],
        true,
    )?;
    Ok(Preparation::Staged(Box::new(StagedReferenceLiteral {
        changes,
        trace,
    })))
}
