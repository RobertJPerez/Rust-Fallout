//! Expression-envelope and token inspection, separate from runtime evaluation.
use super::expression::{Kind, Limits, Operators, StatementKind, Target, Token};
use crate::{Result, obscript_census};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub statements: u64,
    pub expression_bytes: u64,
    pub tokens: u64,
    pub command_calls: BTreeMap<u16, u64>,
    pub operator_codes: BTreeMap<u32, u64>,
    pub token_kinds: BTreeMap<u8, u64>,
    pub trailing_operand_bytes: u64,
    pub expressions_with_pending_context: u64,
}

#[derive(Debug, Serialize)]
pub struct StatementReport {
    pub instruction_scda_offset: usize,
    pub opcode: u16,
    pub operand_bytes: usize,
    pub target_kind: Option<&'static str>,
    pub target_type_byte: Option<u8>,
    pub target_index: Option<u16>,
    pub target_context_reference: Option<u16>,
    pub false_jump_bytes: Option<u16>,
    pub expression_operand_offset: Option<usize>,
    pub expression_bytes: Option<usize>,
    pub tokens: Option<usize>,
    pub token_sha256: Option<String>,
    pub pending_context_reference: Option<u16>,
    pub trailing_operand_bytes: Option<usize>,
    pub trailing_operand_sha256: Option<String>,
    pub issue: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Body {
    pub site: obscript_census::Site,
    pub bytes: usize,
    pub sha256: String,
    pub statements: Vec<StatementReport>,
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
    pub expression_issues: usize,
    pub counts: Counts,
    pub bodies: Vec<Body>,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
}

pub fn token_tag(kind: &Kind<'_>) -> u8 {
    match kind {
        Kind::Local { .. } => 1,
        Kind::Global { .. } => 2,
        Kind::ReferenceLiteral { .. } => 3,
        Kind::ReferencePrefix { .. } => 4,
        Kind::Command { .. } => 5,
        Kind::String(_) => 6,
        Kind::Number(_) => 7,
        Kind::Operator { .. } => 8,
    }
}

/// Fixed 58-byte tuples include token extents and all parsed fields. Opaque
/// string/number/argument data contributes length and SHA256; other payload slots
/// are zero. Whole SCDA hashes separately bind every source byte and whitespace.
pub fn token_digest(tokens: &[Token<'_>]) -> String {
    let mut hash = Sha256::new();
    for token in tokens {
        let mut type_byte = 0;
        let mut index = 0_u16;
        let mut context = None;
        let mut operator = 0_u32;
        let mut precedence = 0;
        let mut opcode = 0_u16;
        let mut payload = None;
        match &token.kind {
            Kind::Local {
                type_byte: t,
                index: i,
                context_reference: c,
            } => {
                type_byte = *t;
                index = *i;
                context = *c;
            }
            Kind::Global { reference_index }
            | Kind::ReferenceLiteral { reference_index }
            | Kind::ReferencePrefix { reference_index } => index = *reference_index,
            Kind::Command {
                opcode: o,
                context_reference: c,
                arguments,
            } => {
                opcode = *o;
                context = *c;
                payload = Some(*arguments);
            }
            Kind::String(bytes) | Kind::Number(bytes) => payload = Some(*bytes),
            Kind::Operator {
                code,
                precedence: p,
            } => {
                operator = *code;
                precedence = *p;
            }
        }
        hash.update((token.bytes.start as u32).to_le_bytes());
        hash.update((token.bytes.end as u32).to_le_bytes());
        hash.update([token_tag(&token.kind), type_byte]);
        hash.update(index.to_le_bytes());
        hash.update([u8::from(context.is_some())]);
        hash.update(context.unwrap_or(0).to_le_bytes());
        hash.update(operator.to_le_bytes());
        hash.update([precedence]);
        hash.update(opcode.to_le_bytes());
        hash.update((payload.map_or(0, |data| data.len()) as u32).to_le_bytes());
        if let Some(payload) = payload {
            hash.update(Sha256::digest(payload));
        } else {
            hash.update([0; 32]);
        }
    }
    format!("{:x}", hash.finalize())
}

pub fn inspect(
    path: &Path,
    focused: bool,
    operators: &Operators,
    mut observe: impl FnMut(&[u8]) -> Result<()>,
) -> Result<Report> {
    let mut bodies = Vec::new();
    let mut counts = Counts::default();
    let mut expression_issues = 0;
    let framing = obscript_census::inspect(path, focused, |site, bytes, program| {
        observe(bytes)?;
        let mut body = Body {
            site: site.clone(),
            bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            statements: Vec::new(),
        };
        for instruction in &program.instructions {
            if !matches!(instruction.opcode, 0x15 | 0x16 | 0x18) {
                continue;
            }
            counts.statements += 1;
            let mut row = StatementReport {
                instruction_scda_offset: instruction.bytes.start,
                opcode: instruction.opcode,
                operand_bytes: instruction.operands.len(),
                target_kind: None,
                target_type_byte: None,
                target_index: None,
                target_context_reference: None,
                false_jump_bytes: None,
                expression_operand_offset: None,
                expression_bytes: None,
                tokens: None,
                token_sha256: None,
                pending_context_reference: None,
                trailing_operand_bytes: None,
                trailing_operand_sha256: None,
                issue: None,
            };
            match super::expression::statement(instruction, operators, Limits::default()) {
                Err(error) => {
                    row.issue = Some(error.to_string());
                    expression_issues += 1;
                }
                Ok(Some(statement)) => {
                    match statement.kind {
                        StatementKind::Conditional { false_jump_bytes } => {
                            row.false_jump_bytes = Some(false_jump_bytes)
                        }
                        StatementKind::Assignment(Target::Global { reference_index }) => {
                            row.target_kind = Some("global");
                            row.target_index = Some(reference_index);
                        }
                        StatementKind::Assignment(Target::Local {
                            type_byte,
                            index,
                            context_reference,
                        }) => {
                            row.target_kind = Some("local");
                            row.target_type_byte = Some(type_byte);
                            row.target_index = Some(index);
                            row.target_context_reference = context_reference;
                        }
                    }
                    row.expression_operand_offset = Some(statement.expression_operand_offset);
                    row.expression_bytes = Some(statement.expression.bytes.len());
                    row.tokens = Some(statement.expression.tokens.len());
                    row.token_sha256 = Some(token_digest(&statement.expression.tokens));
                    row.pending_context_reference = statement.expression.pending_context_reference;
                    row.trailing_operand_bytes = Some(statement.trailing.len());
                    row.trailing_operand_sha256 =
                        Some(format!("{:x}", Sha256::digest(statement.trailing)));
                    counts.expression_bytes += statement.expression.bytes.len() as u64;
                    counts.tokens += statement.expression.tokens.len() as u64;
                    counts.trailing_operand_bytes += statement.trailing.len() as u64;
                    counts.expressions_with_pending_context +=
                        u64::from(row.pending_context_reference.is_some());
                    for token in statement.expression.tokens {
                        *counts
                            .token_kinds
                            .entry(token_tag(&token.kind))
                            .or_default() += 1;
                        match token.kind {
                            Kind::Command { opcode, .. } => {
                                *counts.command_calls.entry(opcode).or_default() += 1
                            }
                            Kind::Operator { code, .. } => {
                                *counts.operator_codes.entry(code).or_default() += 1
                            }
                            _ => {}
                        }
                    }
                }
                Ok(None) => unreachable!("selected expression statement"),
            }
            body.statements.push(row);
        }
        bodies.push(body);
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
        expression_issues,
        counts,
        bodies,
        execution_ready: false,
        retail_parity_accepted: false,
    })
}
