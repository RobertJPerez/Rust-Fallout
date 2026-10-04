//! Exact comparison of imported semantic observations against a prepared source.
//! A match is evidence about the supplied captures, never execution permission.
use fallout_data::{
    identity::FormKey,
    loaded_scripts::Handle,
    obscript::{self, arguments, expression},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_steps: usize,
    pub maximum_words: usize,
    pub maximum_writes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_steps: 4_096,
            maximum_words: 65_536,
            maximum_writes: 4_096,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("semantic trace budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("invalid semantic trace: {0}")]
    Invalid(String),
    #[error("semantic trace source does not match the prepared definition: {0}")]
    Source(&'static str),
    #[error(transparent)]
    CachedSource(#[from] crate::programs::LookupError),
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Binary64,
    Binary32,
    Signed32,
    Unsigned32,
    Unsigned64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Word {
    pub format: Format,
    /// Fixed-width lowercase hexadecimal avoids JSON number rounding. Signed
    /// zero and every NaN payload remain distinct; no float conversion occurs.
    pub bits: String,
}
impl Word {
    pub fn binary64(bits: u64) -> Self {
        Self {
            format: Format::Binary64,
            bits: format!("{bits:016x}"),
        }
    }
    fn validate(&self) -> Result<()> {
        let length = match self.format {
            Format::Binary64 | Format::Unsigned64 => 16,
            Format::Binary32 | Format::Signed32 | Format::Unsigned32 => 8,
        };
        if self.bits.len() != length
            || !self
                .bits
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Invalid("numeric word width/hex encoding".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Assignment,
    Conversion,
    Branch,
    GetItemCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caller {
    pub calling_reference: Option<FormKey>,
    pub containing_reference: Option<FormKey>,
    pub target: Option<FormKey>,
    /// Explicit fixture activation identity, normalized by the capture producer.
    /// It is not a pointer, ECS index, or an assumed original event-list ID.
    pub activation: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepInput {
    pub event_ordinal: u32,
    pub event_id: u16,
    pub begin_scda_offset: u32,
    pub scda_offset: u32,
    pub operation: Operation,
    pub caller: Caller,
    pub operands: Vec<Word>,
    pub item: Option<FormKey>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalWrite {
    pub index: u32,
    pub value: Word,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepOutput {
    pub return_value: Option<Word>,
    pub successor_scda_offset: Option<u32>,
    pub writes: Vec<LocalWrite>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub input: StepInput,
    pub output: StepOutput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub executable_sha256: String,
    pub profile_receipt_sha256: String,
    pub source_cohort_sha256: String,
    pub winning_content_sha256: String,
    pub definition: Handle,
    pub compiled_sha256: String,
    pub compiled_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub identity: Identity,
    pub purpose: Operation,
    pub steps: Vec<StepInput>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Producer {
    Original,
    Replacement,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finish {
    Completed,
    Interrupted,
    Unsupported,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub schema_version: u32,
    pub identity: Identity,
    pub producer: Producer,
    pub producer_executable_sha256: String,
    pub transport_receipt_sha256: String,
    pub instrumentation: String,
    pub finish: Finish,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Matched,
    Mismatched,
    Blocked,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    pub producer: Producer,
    pub step: Option<usize>,
    pub field: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub status: Status,
    pub compared_steps: usize,
    pub first_difference: Option<Difference>,
    /// Capture labels and digests are checked, but this API does not authenticate
    /// an external recorder or admit any retail semantics to the interpreter.
    pub gameplay_accepted: bool,
}
fn report(
    status: Status,
    count: usize,
    producer: Producer,
    step: Option<usize>,
    field: &'static str,
) -> Comparison {
    Comparison {
        status,
        compared_steps: count,
        first_difference: Some(Difference {
            producer,
            step,
            field,
        }),
        gameplay_accepted: false,
    }
}
fn sha(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid(
            "SHA-256 must be 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}
fn identity(identity: &Identity) -> Result<()> {
    for value in [
        &identity.executable_sha256,
        &identity.profile_receipt_sha256,
        &identity.source_cohort_sha256,
        &identity.winning_content_sha256,
        &identity.definition.version_sha256,
        &identity.compiled_sha256,
    ] {
        sha(value)?;
    }
    Ok(())
}
fn input(input: &StepInput, words: &mut usize, limits: Limits) -> Result<()> {
    if input.operands.len() > limits.maximum_words.saturating_sub(*words) {
        return Err(Error::Capacity("operand words"));
    }
    *words += input.operands.len();
    for word in &input.operands {
        word.validate()?;
    }
    for form in [
        input.caller.calling_reference.as_ref(),
        input.caller.containing_reference.as_ref(),
        input.caller.target.as_ref(),
        input.item.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        crate::identity::valid_form(form).map_err(|e| Error::Invalid(e.to_string()))?;
    }
    if (input.operation == Operation::GetItemCount) != input.item.is_some() {
        return Err(Error::Invalid(
            "only GetItemCount needs an explicit content item".into(),
        ));
    }
    Ok(())
}

/// Validate source positions through the existing plan. Conversion names the
/// probe's purpose; its assignment envelope does not prove a conversion rule.
pub fn validate_manifest(
    sources: &crate::programs::PreparedSources<'_>,
    manifest: &Manifest,
    limits: Limits,
) -> Result<()> {
    if manifest.schema_version != 1 {
        return Err(Error::Invalid("manifest schema version".into()));
    }
    identity(&manifest.identity)?;
    if manifest.identity.source_cohort_sha256 != sources.source_cohort_sha256() {
        return Err(Error::Source("source receipt cohort"));
    }
    let plan = sources.get(&manifest.identity.definition)?.plan();
    if &manifest.identity.definition != plan.handle() {
        return Err(Error::Source("definition version"));
    }
    if manifest.identity.winning_content_sha256 != plan.source_cohort_sha256() {
        return Err(Error::Source("winning content"));
    }
    let bytes = plan.control().bytes();
    if manifest.identity.compiled_bytes != bytes.len()
        || manifest.identity.compiled_sha256 != format!("{:x}", Sha256::digest(bytes))
    {
        return Err(Error::Source("compiled body"));
    }
    if manifest.steps.is_empty()
        || !manifest
            .steps
            .iter()
            .any(|s| s.operation == manifest.purpose)
    {
        return Err(Error::Invalid(
            "empty probe or missing purpose operation".into(),
        ));
    }
    if manifest.steps.len() > limits.maximum_steps {
        return Err(Error::Capacity("manifest steps"));
    }
    let mut words = 0;
    let mut event = 0;
    for (ordinal, step) in manifest.steps.iter().enumerate() {
        input(step, &mut words, limits)?;
        if step.event_ordinal < event
            || step.event_ordinal > event.saturating_add(1)
            || (ordinal == 0 && step.event_ordinal != 0)
        {
            return Err(Error::Invalid(
                "event ordinals must begin at zero and advance in order".into(),
            ));
        }
        event = step.event_ordinal;
        let source_event = plan
            .event_at_scda_offset(step.begin_scda_offset as usize)
            .filter(|source| source.event_id == step.event_id)
            .ok_or(Error::Source("event block"))?;
        let offset = step.scda_offset as usize;
        let instructions = plan.control().instructions();
        let index = instructions
            .partition_point(|instruction| instruction.bytes.start <= offset)
            .checked_sub(1)
            .ok_or(Error::Source("operation/SCDA position"))?;
        let instruction = &instructions[index];
        if index <= source_event.begin_instruction || index >= source_event.end_instruction {
            return Err(Error::Source("operation outside event block"));
        }
        let matches = {
            let at_instruction = instruction.bytes.start == step.scda_offset as usize;
            match step.operation {
                Operation::Assignment | Operation::Conversion => {
                    at_instruction
                        && plan.statement(index).is_some_and(|s| {
                            matches!(s.kind(), expression::StatementKind::Assignment(_))
                        })
                }
                Operation::Branch => {
                    at_instruction
                        && plan.statement(index).is_some_and(|s| {
                            matches!(s.kind(), expression::StatementKind::Conditional { .. })
                        })
                }
                Operation::GetItemCount => {
                    (at_instruction
                        && instruction.kind() == obscript::Kind::NativeCommand
                        && instruction.opcode == crate::query::GET_ITEM_COUNT_COMMAND)
                        || plan.statement(index).is_some_and(|s| {
                            s.plan().tokens().iter().any(|token| {
                                s.expression_scda_offset() + token.bytes.start
                                    == step.scda_offset as usize
                                    && matches!(
                                        token.kind,
                                        expression::Kind::Command {
                                            opcode: crate::query::GET_ITEM_COUNT_COMMAND,
                                            ..
                                        }
                                    )
                            })
                        })
                }
            }
        };
        if !matches {
            return Err(Error::Source("operation/SCDA position"));
        }
        if step.operation == Operation::GetItemCount {
            let raw = if instruction.kind() == obscript::Kind::NativeCommand {
                instruction.operands
            } else {
                plan.statement(index)
                    .and_then(|statement| {
                        statement.plan().tokens().iter().find_map(|token| {
                            if statement.expression_scda_offset() + token.bytes.start != offset {
                                return None;
                            }
                            match &token.kind {
                                expression::Kind::Command { arguments, .. } => Some(*arguments),
                                _ => None,
                            }
                        })
                    })
                    .ok_or(Error::Source("native argument position"))?
            };
            // Reuse the already evidenced vanilla inventory-object/form-list
            // descriptor. This is an encoded reference join, not retail coercion.
            let parameters = [arguments::Parameter {
                type_id: 50,
                optional_word: 0,
            }];
            let decoded = arguments::decode(
                raw,
                arguments::Signature {
                    convention: arguments::Convention::Default,
                    parameters: &parameters,
                },
                Default::default(),
            )
            .map_err(|error| Error::Invalid(error.to_string()))?;
            let [
                arguments::Argument {
                    value: arguments::Value::FormReference { reference_index },
                    ..
                },
            ] = decoded.arguments.as_slice()
            else {
                return Err(Error::Source(
                    "native item needs a source reference-table argument",
                ));
            };
            if !decoded.trailing.is_empty()
                || !decoded.message_arguments.is_empty()
                || plan
                    .source()
                    .reference(u32::from(*reference_index))
                    .and_then(|reference| reference.form_key.as_ref())
                    != step.item.as_ref()
            {
                return Err(Error::Source("native item source binding"));
            }
        }
    }
    Ok(())
}

fn validate_capture(capture: &Capture, limits: Limits) -> Result<()> {
    if capture.schema_version != 1 {
        return Err(Error::Invalid("capture schema version".into()));
    }
    identity(&capture.identity)?;
    sha(&capture.producer_executable_sha256)?;
    sha(&capture.transport_receipt_sha256)?;
    if capture.instrumentation.is_empty() || capture.instrumentation.len() > 4_096 {
        return Err(Error::Invalid("instrumentation disclosure".into()));
    }
    if capture.steps.len() > limits.maximum_steps {
        return Err(Error::Capacity("capture steps"));
    }
    let mut words = 0;
    let mut writes = 0;
    for step in &capture.steps {
        input(&step.input, &mut words, limits)?;
        if let Some(word) = &step.output.return_value {
            word.validate()?;
        }
        if step.output.writes.len() > limits.maximum_writes.saturating_sub(writes) {
            return Err(Error::Capacity("local writes"));
        }
        writes += step.output.writes.len();
        for write in &step.output.writes {
            if write.index == 0 {
                return Err(Error::Invalid("zero local index".into()));
            }
            write.value.validate()?;
        }
        if step
            .output
            .error
            .as_ref()
            .is_some_and(|e| e.is_empty() || e.len() > 4_096)
        {
            return Err(Error::Invalid("error code/description".into()));
        }
    }
    Ok(())
}

/// The manifest supplies independent case inputs/order; the original capture
/// supplies observed outputs. Neither absent traces nor two empty traces pass.
pub fn compare(
    sources: &crate::programs::PreparedSources<'_>,
    manifest: &Manifest,
    original: Option<&Capture>,
    replacement: Option<&Capture>,
    limits: Limits,
) -> Result<Comparison> {
    validate_manifest(sources, manifest, limits)?;
    let plan = sources.get(&manifest.identity.definition)?.plan();
    // Validate every supplied input even if the other capture is absent.
    for capture in [original, replacement].into_iter().flatten() {
        validate_capture(capture, limits)?;
    }
    let mut captures = Vec::with_capacity(2);
    for (producer, capture) in [
        (Producer::Original, original),
        (Producer::Replacement, replacement),
    ] {
        let Some(capture) = capture else {
            return Ok(report(
                Status::Blocked,
                0,
                producer,
                None,
                "missing_capture",
            ));
        };
        if capture.producer != producer || capture.identity != manifest.identity {
            return Ok(report(
                Status::Mismatched,
                0,
                producer,
                None,
                "capture_identity",
            ));
        }
        let retail_executable =
            capture.producer_executable_sha256 == manifest.identity.executable_sha256;
        if retail_executable != (producer == Producer::Original) {
            return Ok(report(
                Status::Mismatched,
                0,
                producer,
                None,
                "producer_executable",
            ));
        }
        if capture.finish != Finish::Completed {
            return Ok(report(
                Status::Blocked,
                0,
                producer,
                None,
                "incomplete_capture",
            ));
        }
        if capture.steps.len() != manifest.steps.len() {
            return Ok(report(Status::Mismatched, 0, producer, None, "step_count"));
        }
        for (index, (step, expected)) in capture.steps.iter().zip(&manifest.steps).enumerate() {
            if &step.input != expected {
                return Ok(report(
                    Status::Mismatched,
                    index,
                    producer,
                    Some(index),
                    "step_input",
                ));
            }
            if let Some(offset) = step.output.successor_scda_offset
                && plan
                    .control()
                    .instructions()
                    .binary_search_by_key(&(offset as usize), |instruction| instruction.bytes.start)
                    .is_err()
            {
                return Err(Error::Source(
                    "observed successor is not an instruction header",
                ));
            }
            let observed = step.output.error.is_some()
                || match step.input.operation {
                    Operation::Assignment | Operation::Conversion => !step.output.writes.is_empty(),
                    Operation::Branch => step.output.successor_scda_offset.is_some(),
                    Operation::GetItemCount => step.output.return_value.is_some(),
                };
            if !observed {
                return Ok(report(
                    Status::Blocked,
                    index,
                    producer,
                    Some(index),
                    "missing_observation",
                ));
            }
        }
        captures.push(capture);
    }
    for (index, (original, replacement)) in
        captures[0].steps.iter().zip(&captures[1].steps).enumerate()
    {
        let field = if original.output.return_value != replacement.output.return_value {
            "return_value"
        } else if original.output.successor_scda_offset != replacement.output.successor_scda_offset
        {
            "successor_scda_offset"
        } else if original.output.writes != replacement.output.writes {
            "local_writes"
        } else if original.output.error != replacement.output.error {
            "error"
        } else {
            continue;
        };
        return Ok(report(
            Status::Mismatched,
            index,
            Producer::Replacement,
            Some(index),
            field,
        ));
    }
    Ok(Comparison {
        status: Status::Matched,
        compared_steps: manifest.steps.len(),
        first_difference: None,
        gameplay_accepted: false,
    })
}
