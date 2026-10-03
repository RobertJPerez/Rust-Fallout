//! Associate encoded operands with their owning script tables. Foreign locals
//! retain a context association; their declaration needs a loaded target script.
use super::{
    argument_census::Signatures,
    arguments::{self, Signature, Value},
    expression::{self, Kind, Operators, StatementKind, Target},
};
use crate::script_units::{Reference, Unit};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct Use {
    /// Offset of the encoded u16 index, relative to SCDA.
    pub scda_offset: usize,
    pub role: u8,
    pub index: u16,
    pub context_reference: Option<u16>,
    /// 1 local, 2 form, 3 reference variable, 4 foreign local; 5..7 missing.
    pub status: u8,
    pub target_value: Option<u32>,
    pub reference_field_decoded_offset: Option<usize>,
    pub local_declaration_decoded_offset: Option<usize>,
    pub local_type_byte: Option<u8>,
    /// Zero absent, one SCRO form, two SCRV variable. Neither is a runtime value.
    pub context_target_kind: u8,
    pub context_target_value: u32,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub uses: u64,
    pub roles: BTreeMap<u8, u64>,
    pub statuses: BTreeMap<u8, u64>,
    pub instruction_calls: u64,
    pub expression_calls: u64,
    pub regular_arguments: u64,
    pub message_arguments: u64,
    pub expressions: u64,
    pub deferred_foreign_locals: u64,
    pub missing_bindings: u64,
}

#[derive(Debug, Serialize)]
pub struct Issue {
    pub instruction_scda_offset: usize,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct Binding {
    pub uses: Vec<Use>,
    pub counts: Counts,
    pub decode_issues: Vec<Issue>,
}

/// Bind an encoded local index to the first authored declaration. A context must
/// first bind to this unit's reference table; the foreign index never aliases an
/// equally numbered local in this unit.
pub fn local(unit: &Unit<'_>, offset: usize, role: u8, index: u16, context: Option<u16>) -> Use {
    let mut row = empty(offset, role, index, context);
    if let Some(context) = context {
        let bound = reference(unit, offset, role, context);
        row.status = if bound.status < 5 { 4 } else { bound.status };
        row.target_value = Some(u32::from(index));
        row.reference_field_decoded_offset = bound.reference_field_decoded_offset;
        row.context_target_kind = match bound.status {
            2 => 1,
            3 => 2,
            _ => 0,
        };
        row.context_target_value = bound.target_value.unwrap_or(0);
        return row;
    }
    if let Some(variable) = unit.variable(u32::from(index)) {
        row.status = 1;
        row.target_value = Some(variable.index);
        row.local_declaration_decoded_offset = Some(variable.declaration.offset);
        row.local_type_byte = Some(variable.type_byte);
    } else {
        row.status = 5;
    }
    row
}

pub fn reference(unit: &Unit<'_>, offset: usize, role: u8, index: u16) -> Use {
    let mut row = empty(offset, role, index, None);
    let Some(entry) = unit.reference(u32::from(index)) else {
        row.status = 6;
        return row;
    };
    row.reference_field_decoded_offset = Some(entry.field.offset);
    match entry.target {
        Reference::Form(form) => {
            row.status = 2;
            row.target_value = Some(form);
        }
        Reference::Variable(index) => {
            row.target_value = Some(index);
            if let Some(variable) = unit.variable(index) {
                row.status = 3;
                row.local_declaration_decoded_offset = Some(variable.declaration.offset);
                row.local_type_byte = Some(variable.type_byte);
            } else {
                row.status = 7;
            }
        }
    }
    row
}

fn empty(offset: usize, role: u8, index: u16, context_reference: Option<u16>) -> Use {
    Use {
        scda_offset: offset,
        role,
        index,
        context_reference,
        status: 0,
        target_value: None,
        reference_field_decoded_offset: None,
        local_declaration_decoded_offset: None,
        local_type_byte: None,
        context_target_kind: 0,
        context_target_value: 0,
    }
}

/// Fixed 33-byte tuples preserve exact associations and optional-field presence.
/// Source/table hashes separately bind the bytes being associated.
pub fn digest(uses: &[Use]) -> String {
    let mut hash = Sha256::new();
    for row in uses {
        hash.update((row.scda_offset as u32).to_le_bytes());
        hash.update([row.role]);
        hash.update(row.index.to_le_bytes());
        hash.update([u8::from(row.context_reference.is_some())]);
        hash.update(row.context_reference.unwrap_or(0).to_le_bytes());
        hash.update([row.status, u8::from(row.target_value.is_some())]);
        hash.update(row.target_value.unwrap_or(0).to_le_bytes());
        hash.update([u8::from(row.reference_field_decoded_offset.is_some())]);
        hash.update((row.reference_field_decoded_offset.unwrap_or(0) as u32).to_le_bytes());
        hash.update([u8::from(row.local_declaration_decoded_offset.is_some())]);
        hash.update((row.local_declaration_decoded_offset.unwrap_or(0) as u32).to_le_bytes());
        hash.update([
            u8::from(row.local_type_byte.is_some()),
            row.local_type_byte.unwrap_or(0),
            row.context_target_kind,
        ]);
        hash.update(row.context_target_value.to_le_bytes());
    }
    format!("{:x}", hash.finalize())
}

impl Binding {
    fn push(&mut self, row: Use, maximum: usize) -> crate::Result<()> {
        if self.uses.len() >= maximum {
            return Err(crate::Error::Unsupported(
                "operand binding use budget exceeded".into(),
            ));
        }
        self.counts.uses += 1;
        *self.counts.roles.entry(row.role).or_default() += 1;
        *self.counts.statuses.entry(row.status).or_default() += 1;
        self.counts.deferred_foreign_locals += u64::from(row.status == 4);
        self.counts.missing_bindings += u64::from(row.status >= 5);
        self.uses.push(row);
        Ok(())
    }

    fn value(
        &mut self,
        unit: &Unit<'_>,
        offset: usize,
        value: &Value<'_>,
        maximum: usize,
    ) -> crate::Result<()> {
        let row = match value {
            Value::Global { reference_index } => reference(unit, offset + 1, 9, *reference_index),
            Value::FormReference { reference_index } => {
                reference(unit, offset + 1, 10, *reference_index)
            }
            Value::FormVariable { index } => local(unit, offset + 1, 11, *index, None),
            Value::Variable {
                index,
                context_reference,
                ..
            } => {
                if let Some(context) = context_reference {
                    self.push(reference(unit, offset + 1, 12, *context), maximum)?;
                }
                local(
                    unit,
                    offset + if context_reference.is_some() { 4 } else { 1 },
                    8,
                    *index,
                    *context_reference,
                )
            }
            _ => return Ok(()),
        };
        self.push(row, maximum)
    }

    fn call(
        &mut self,
        unit: &Unit<'_>,
        bytes: &[u8],
        site: CallSite,
        signatures: &Signatures,
        maximum: usize,
    ) -> crate::Result<()> {
        let decoded = signatures.get(&site.opcode).map(|signature| {
            arguments::decode(
                bytes,
                Signature {
                    convention: signature.convention,
                    parameters: &signature.parameters,
                },
                arguments::Limits::default(),
            )
        });
        let arguments = match decoded {
            Some(Ok(arguments)) => arguments,
            Some(Err(error)) => {
                self.decode_issues.push(Issue {
                    instruction_scda_offset: site.instruction,
                    reason: error.to_string(),
                });
                return Ok(());
            }
            None => {
                self.decode_issues.push(Issue {
                    instruction_scda_offset: site.instruction,
                    reason: "command has no verified signature".into(),
                });
                return Ok(());
            }
        };
        self.counts.regular_arguments += arguments.arguments.len() as u64;
        self.counts.message_arguments += arguments.message_arguments.len() as u64;
        for arg in arguments.arguments {
            self.value(unit, site.arguments + arg.bytes.start, &arg.value, maximum)?;
        }
        for arg in arguments.message_arguments {
            self.value(unit, site.arguments + arg.bytes.start, &arg.value, maximum)?;
        }
        Ok(())
    }
}

struct CallSite {
    instruction: usize,
    arguments: usize,
    opcode: u16,
}

pub fn bind(
    unit: &Unit<'_>,
    program: &super::Program<'_>,
    operators: &Operators,
    signatures: &Signatures,
    max_uses: usize,
) -> crate::Result<Binding> {
    if unit.compiled.map(|field| field.data) != Some(program.bytes) {
        return Err(crate::Error::Resolution(
            "compiled program does not belong to this script unit".into(),
        ));
    }
    let mut result = Binding {
        uses: Vec::new(),
        counts: Counts::default(),
        decode_issues: Vec::new(),
    };
    for instruction in &program.instructions {
        let offset = instruction.operand_offset;
        if let Some(index) = instruction.calling_reference {
            result.push(
                reference(unit, instruction.bytes.start + 2, 1, index),
                max_uses,
            )?;
        }
        if instruction.kind() == super::Kind::NativeCommand {
            result.counts.instruction_calls += 1;
            result.call(
                unit,
                instruction.operands,
                CallSite {
                    instruction: instruction.bytes.start,
                    arguments: offset,
                    opcode: instruction.opcode,
                },
                signatures,
                max_uses,
            )?;
            continue;
        }
        let statement =
            match expression::statement(instruction, operators, expression::Limits::default()) {
                Ok(Some(statement)) => statement,
                Ok(None) => continue,
                Err(error) => {
                    result.decode_issues.push(Issue {
                        instruction_scda_offset: instruction.bytes.start,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
        result.counts.expressions += 1;
        match statement.kind {
            StatementKind::Assignment(Target::Local {
                index,
                context_reference,
                ..
            }) => {
                if let Some(context) = context_reference {
                    result.push(reference(unit, offset + 1, 7, context), max_uses)?;
                }
                result.push(
                    local(
                        unit,
                        offset + if context_reference.is_some() { 4 } else { 1 },
                        2,
                        index,
                        context_reference,
                    ),
                    max_uses,
                )?;
            }
            StatementKind::Assignment(Target::Global { reference_index }) => {
                result.push(reference(unit, offset + 1, 3, reference_index), max_uses)?
            }
            _ => {}
        }
        for token in statement.expression.tokens {
            let token_offset = offset + statement.expression_operand_offset + token.bytes.start;
            let row = match token.kind {
                Kind::Local {
                    index,
                    context_reference,
                    ..
                } => local(unit, token_offset + 1, 4, index, context_reference),
                Kind::Global { reference_index } => {
                    reference(unit, token_offset + 1, 5, reference_index)
                }
                Kind::ReferenceLiteral { reference_index } => {
                    reference(unit, token_offset + 1, 6, reference_index)
                }
                Kind::ReferencePrefix { reference_index } => {
                    reference(unit, token_offset + 1, 7, reference_index)
                }
                Kind::Command {
                    opcode, arguments, ..
                } => {
                    result.counts.expression_calls += 1;
                    result.call(
                        unit,
                        arguments,
                        CallSite {
                            instruction: instruction.bytes.start,
                            arguments: token_offset + 5,
                            opcode,
                        },
                        signatures,
                        max_uses,
                    )?;
                    continue;
                }
                _ => continue,
            };
            result.push(row, max_uses)?;
        }
    }
    Ok(result)
}
