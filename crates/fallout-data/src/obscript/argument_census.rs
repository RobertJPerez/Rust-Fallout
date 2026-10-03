//! Inspect native operands without resolving references or invoking handlers.
//! Each call retains its source extent and an exact digest of the typed operands.
use super::{
    Program,
    arguments::{self, Convention, Parameter, Signature, Value},
    expression::{self, Kind, Operators},
};
use crate::{Result, obscript_census};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone)]
pub struct CommandSignature {
    pub convention: Convention,
    pub parameters: Vec<Parameter>,
}

pub type Signatures = BTreeMap<u16, CommandSignature>;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bodies: usize,
    pub max_calls: usize,
    pub operands: arguments::Limits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bodies: 65_536,
            max_calls: 262_144,
            operands: arguments::Limits::default(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub top_level_calls: u64,
    pub expression_calls: u64,
    pub operand_bytes: u64,
    pub arguments: u64,
    pub message_arguments: u64,
    pub trailing_operand_bytes: u64,
    pub command_calls: BTreeMap<u16, u64>,
    pub value_kinds: BTreeMap<u8, u64>,
}

#[derive(Debug, Serialize)]
pub struct Call {
    pub instruction_scda_offset: usize,
    pub expression_token_offset: Option<usize>,
    pub arguments_scda_offset: usize,
    pub command_id: u16,
    pub calling_reference: Option<u16>,
    pub operand_bytes: usize,
    pub operand_sha256: String,
    pub declared_count: Option<u16>,
    pub arguments: Option<usize>,
    pub argument_sha256: Option<String>,
    pub declared_message_count: Option<u16>,
    pub message_arguments: Option<usize>,
    pub message_argument_sha256: Option<String>,
    pub trailing_operand_bytes: Option<usize>,
    pub trailing_operand_sha256: Option<String>,
    pub issue: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExpressionIssue {
    pub instruction_scda_offset: usize,
    pub issue: String,
}

#[derive(Debug, Serialize)]
pub struct Body {
    pub site: obscript_census::Site,
    pub bytes: usize,
    pub sha256: String,
    pub calls: Vec<Call>,
    pub expression_issues: Vec<ExpressionIssue>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub focused_scan: bool,
    pub record_payloads_decoded: u64,
    pub record_payloads_deferred: u64,
    pub framing_issues: usize,
    pub framing_counts: obscript_census::Counts,
    pub argument_issues: usize,
    pub expression_issues: usize,
    pub counts: Counts,
    pub bodies: Vec<Body>,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
}

pub fn value_tag(value: &Value<'_>) -> u8 {
    match value {
        Value::String(_) => 1,
        Value::SignedInteger(_) => 2,
        Value::DoubleBits(_) => 3,
        Value::Short(_) => 4,
        Value::Byte(_) => 5,
        Value::Global { .. } => 6,
        Value::Variable { .. } => 7,
        Value::FormReference { .. } => 8,
        Value::FormVariable { .. } => 9,
    }
}

/// The 63-byte little-endian tuple binds ranges, parameter type, value tag,
/// variable context and exact numeric bits. Strings contribute their raw hash.
/// Message substitutions use u32::MAX for the absent parameter type.
pub fn argument_digest<'a>(
    rows: impl IntoIterator<Item = (usize, usize, u32, &'a Value<'a>)>,
) -> String {
    let mut hash = Sha256::new();
    for (start, end, parameter, value) in rows {
        let (mut type_byte, mut index, mut context, mut bits, mut payload) =
            (0, 0_u16, None, 0_u64, None);
        match value {
            Value::String(bytes) => payload = Some(*bytes),
            Value::SignedInteger(value) => bits = u64::from(*value as u32),
            Value::DoubleBits(value) => bits = *value,
            Value::Short(value) => bits = u64::from(*value),
            Value::Byte(value) => bits = u64::from(*value),
            Value::Global { reference_index } | Value::FormReference { reference_index } => {
                index = *reference_index
            }
            Value::FormVariable { index: local } => index = *local,
            Value::Variable {
                type_byte: source_type,
                index: local,
                context_reference,
            } => {
                type_byte = *source_type;
                index = *local;
                context = *context_reference;
            }
        }
        hash.update((start as u32).to_le_bytes());
        hash.update((end as u32).to_le_bytes());
        hash.update(parameter.to_le_bytes());
        hash.update([value_tag(value), type_byte]);
        hash.update(index.to_le_bytes());
        hash.update([u8::from(context.is_some())]);
        hash.update(context.unwrap_or(0).to_le_bytes());
        hash.update(bits.to_le_bytes());
        hash.update((payload.map_or(0, |bytes| bytes.len()) as u32).to_le_bytes());
        hash.update(payload.map_or([0; 32], |bytes| Sha256::digest(bytes).into()));
    }
    format!("{:x}", hash.finalize())
}

struct Collector<'a> {
    signatures: &'a Signatures,
    limits: Limits,
    counts: Counts,
    calls: usize,
    argument_issues: usize,
    expression_issues: usize,
}

impl Collector<'_> {
    fn call(&mut self, mut row: Call, bytes: &[u8]) -> Result<Call> {
        if self.calls >= self.limits.max_calls {
            return Err(crate::Error::Unsupported(
                "native call row budget exceeded".into(),
            ));
        }
        self.calls += 1;
        if row.expression_token_offset.is_some() {
            self.counts.expression_calls += 1;
        } else {
            self.counts.top_level_calls += 1;
        }
        self.counts.operand_bytes += bytes.len() as u64;
        *self.counts.command_calls.entry(row.command_id).or_default() += 1;
        let decoded = self.signatures.get(&row.command_id).map(|signature| {
            arguments::decode(
                bytes,
                Signature {
                    convention: signature.convention,
                    parameters: &signature.parameters,
                },
                self.limits.operands,
            )
        });
        match decoded {
            None => row.issue = Some("command has no verified signature".into()),
            Some(Err(error)) => row.issue = Some(error.to_string()),
            Some(Ok(arguments)) => {
                row.declared_count = arguments.declared_count;
                row.arguments = Some(arguments.arguments.len());
                row.argument_sha256 =
                    Some(argument_digest(arguments.arguments.iter().map(|arg| {
                        (
                            arg.bytes.start,
                            arg.bytes.end,
                            arg.parameter_type_id,
                            &arg.value,
                        )
                    })));
                row.declared_message_count = arguments.declared_message_count;
                row.message_arguments = Some(arguments.message_arguments.len());
                row.message_argument_sha256 =
                    Some(argument_digest(arguments.message_arguments.iter().map(
                        |arg| (arg.bytes.start, arg.bytes.end, u32::MAX, &arg.value),
                    )));
                row.trailing_operand_bytes = Some(arguments.trailing.len());
                row.trailing_operand_sha256 =
                    Some(format!("{:x}", Sha256::digest(arguments.trailing)));
                self.counts.arguments += arguments.arguments.len() as u64;
                self.counts.message_arguments += arguments.message_arguments.len() as u64;
                self.counts.trailing_operand_bytes += arguments.trailing.len() as u64;
                for value in arguments
                    .arguments
                    .iter()
                    .map(|arg| &arg.value)
                    .chain(arguments.message_arguments.iter().map(|arg| &arg.value))
                {
                    *self.counts.value_kinds.entry(value_tag(value)).or_default() += 1;
                }
            }
        }
        self.argument_issues += usize::from(row.issue.is_some());
        Ok(row)
    }

    fn body(
        &mut self,
        site: &obscript_census::Site,
        program: &Program<'_>,
        operators: &Operators,
    ) -> Result<Body> {
        let mut body = Body {
            site: site.clone(),
            bytes: program.bytes.len(),
            sha256: format!("{:x}", Sha256::digest(program.bytes)),
            calls: Vec::new(),
            expression_issues: Vec::new(),
        };
        for instruction in &program.instructions {
            if instruction.kind() == super::Kind::NativeCommand {
                let row = call_row(
                    instruction.bytes.start,
                    None,
                    instruction.operand_offset,
                    instruction.opcode,
                    instruction.calling_reference,
                    instruction.operands,
                );
                body.calls.push(self.call(row, instruction.operands)?);
                continue;
            }
            match expression::statement(instruction, operators, expression::Limits::default()) {
                Ok(Some(statement)) => {
                    for token in statement.expression.tokens {
                        if let Kind::Command {
                            opcode,
                            context_reference,
                            arguments,
                        } = token.kind
                        {
                            let offset = instruction.operand_offset
                                + statement.expression_operand_offset
                                + token.bytes.start
                                + 5;
                            let row = call_row(
                                instruction.bytes.start,
                                Some(token.bytes.start),
                                offset,
                                opcode,
                                context_reference,
                                arguments,
                            );
                            body.calls.push(self.call(row, arguments)?);
                        }
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    self.expression_issues += 1;
                    body.expression_issues.push(ExpressionIssue {
                        instruction_scda_offset: instruction.bytes.start,
                        issue: error.to_string(),
                    });
                }
            }
        }
        Ok(body)
    }
}

fn call_row(
    instruction_scda_offset: usize,
    expression_token_offset: Option<usize>,
    arguments_scda_offset: usize,
    command_id: u16,
    calling_reference: Option<u16>,
    bytes: &[u8],
) -> Call {
    Call {
        instruction_scda_offset,
        expression_token_offset,
        arguments_scda_offset,
        command_id,
        calling_reference,
        operand_bytes: bytes.len(),
        operand_sha256: format!("{:x}", Sha256::digest(bytes)),
        declared_count: None,
        arguments: None,
        argument_sha256: None,
        declared_message_count: None,
        message_arguments: None,
        message_argument_sha256: None,
        trailing_operand_bytes: None,
        trailing_operand_sha256: None,
        issue: None,
    }
}

pub fn inspect(
    path: &Path,
    focused: bool,
    operators: &Operators,
    signatures: &Signatures,
    limits: Limits,
    mut observe: impl FnMut(&[u8]) -> Result<()>,
) -> Result<Report> {
    let mut collector = Collector {
        signatures,
        limits,
        counts: Counts::default(),
        calls: 0,
        argument_issues: 0,
        expression_issues: 0,
    };
    let mut bodies = Vec::new();
    let framing = obscript_census::inspect(path, focused, |site, bytes, program| {
        if bodies.len() >= limits.max_bodies {
            return Err(crate::Error::Unsupported(
                "native argument body budget exceeded".into(),
            ));
        }
        observe(bytes)?;
        bodies.push(collector.body(site, program, operators)?);
        Ok(())
    })?;
    Ok(Report {
        schema_version: 1,
        source_name: framing.source_name,
        source_bytes: framing.source_bytes,
        source_sha256: framing.source_sha256,
        focused_scan: focused,
        record_payloads_decoded: framing.record_payloads_decoded,
        record_payloads_deferred: framing.record_payloads_deferred,
        framing_issues: framing.bodies_with_issues,
        framing_counts: framing.counts,
        argument_issues: collector.argument_issues,
        expression_issues: collector.expression_issues,
        counts: collector.counts,
        bodies,
        execution_ready: false,
        retail_parity_accepted: false,
    })
}
