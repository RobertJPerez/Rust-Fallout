//! Bounded vanilla expression tokens. Numeric lexemes and command arguments stay
//! borrowed bytes: this module does not evaluate, round or execute them.
//! Layout evidence: pinned xNVSE ScriptAnalyzer, with a separately fingerprinted
//! operator table supplied by the caller. Unsupported tokens return an error.

use super::Instruction;
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug, Clone)]
pub struct Operator {
    pub code: u32,
    pub precedence: u8,
    pub spelling: Vec<u8>,
}

#[derive(Debug)]
pub struct Operators {
    entries: Vec<Operator>,
    by_spelling: BTreeMap<Vec<u8>, usize>,
}

impl Operators {
    pub fn new(entries: Vec<Operator>) -> Result<Self, DecodeError> {
        if entries.is_empty() || entries.len() > 64 {
            return Err(DecodeError::OperatorTable);
        }
        let mut codes = BTreeMap::new();
        let mut by_spelling = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            if entry.spelling.is_empty()
                || entry.spelling.len() > 2
                || !entry.spelling.iter().all(u8::is_ascii_punctuation)
                || codes.insert(entry.code, index).is_some()
                || by_spelling.insert(entry.spelling.clone(), index).is_some()
            {
                return Err(DecodeError::OperatorTable);
            }
        }
        Ok(Self {
            entries,
            by_spelling,
        })
    }

    fn at(&self, bytes: &[u8]) -> Option<&Operator> {
        for length in [2, 1] {
            if let Some(prefix) = bytes.get(..length)
                && let Some(&index) = self.by_spelling.get(prefix)
            {
                return Some(&self.entries[index]);
            }
        }
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_tokens: usize,
    pub max_literal_bytes: usize,
    pub max_numeric_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 65_535,
            max_tokens: 65_536,
            max_literal_bytes: 512,
            max_numeric_bytes: 512,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Kind<'a> {
    Local {
        type_byte: u8,
        index: u16,
        context_reference: Option<u16>,
    },
    Global {
        reference_index: u16,
    },
    ReferenceLiteral {
        reference_index: u16,
    },
    ReferencePrefix {
        reference_index: u16,
    },
    Command {
        opcode: u16,
        context_reference: Option<u16>,
        arguments: &'a [u8],
    },
    String(&'a [u8]),
    Number(&'a [u8]),
    Operator {
        code: u32,
        precedence: u8,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Token<'a> {
    pub bytes: Range<usize>,
    pub kind: Kind<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Expression<'a> {
    pub bytes: &'a [u8],
    pub tokens: Vec<Token<'a>>,
    /// A prefix without a consuming variable/command is retained for diagnostics.
    pub pending_context_reference: Option<u16>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Local {
        type_byte: u8,
        index: u16,
        context_reference: Option<u16>,
    },
    Global {
        reference_index: u16,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum StatementKind {
    Assignment(Target),
    Conditional { false_jump_bytes: u16 },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Statement<'a> {
    pub kind: StatementKind,
    pub expression_operand_offset: usize,
    pub expression: Expression<'a>,
    pub trailing: &'a [u8],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("invalid or ambiguous expression operator table")]
    OperatorTable,
    #[error("expression byte 0x{offset:X}: need {needed} bytes, only {remaining} remain")]
    Truncated {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error("expression byte 0x{offset:X}: unsupported token 0x{byte:02X}")]
    Unsupported { offset: usize, byte: u8 },
    #[error("expression byte 0x{offset:X}: {kind} budget exceeded")]
    Limit { offset: usize, kind: &'static str },
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, length: usize) -> Result<&'a [u8], DecodeError> {
    if length > bytes.len().saturating_sub(*at) {
        return Err(DecodeError::Truncated {
            offset: *at,
            needed: length,
            remaining: bytes.len().saturating_sub(*at),
        });
    }
    let start = *at;
    *at += length;
    Ok(&bytes[start..*at])
}
fn word(bytes: &[u8], at: &mut usize) -> Result<u16, DecodeError> {
    Ok(u16::from_le_bytes(
        take(bytes, at, 2)?.try_into().expect("checked word"),
    ))
}

// Recognize a decimal lexeme without converting it. The source reference uses
// strtod; this deliberately supports its decimal subset only. Hexadecimal, INF,
// NAN and historical CRT extensions require separate source/corpus evidence.
fn decimal(bytes: &[u8]) -> usize {
    let mut at = 0;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let mut digits = at;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let fraction = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        digits += at - fraction;
    }
    if digits == 0 {
        return 0;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        let before = at;
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == exponent {
            at = before;
        }
    }
    at
}

pub fn decode<'a>(
    bytes: &'a [u8],
    operators: &Operators,
    limits: Limits,
) -> Result<Expression<'a>, DecodeError> {
    if bytes.len() > limits.max_bytes {
        return Err(DecodeError::Limit {
            offset: 0,
            kind: "byte",
        });
    }
    let mut at = 0;
    let mut tokens = Vec::new();
    let mut context = None;
    while at < bytes.len() {
        if bytes[at] <= 0x20 {
            at += 1;
            continue;
        }
        if tokens.len() >= limits.max_tokens {
            return Err(DecodeError::Limit {
                offset: at,
                kind: "token",
            });
        }
        let start = at;
        let byte = bytes[at];
        at += 1;
        let kind = match byte {
            b's' | b'l' | b'f' => Kind::Local {
                type_byte: byte,
                index: word(bytes, &mut at)?,
                context_reference: context.take(),
            },
            b'G' => Kind::Global {
                reference_index: word(bytes, &mut at)?,
            },
            b'Z' => Kind::ReferenceLiteral {
                reference_index: word(bytes, &mut at)?,
            },
            b'r' => {
                let reference_index = word(bytes, &mut at)?;
                context = Some(reference_index);
                Kind::ReferencePrefix { reference_index }
            }
            b'X' => {
                let opcode = word(bytes, &mut at)?;
                let length = usize::from(word(bytes, &mut at)?);
                let arguments = take(bytes, &mut at, length)?;
                Kind::Command {
                    opcode,
                    context_reference: context.take(),
                    arguments,
                }
            }
            b'"' => {
                let length = usize::from(word(bytes, &mut at)?);
                if length > limits.max_literal_bytes {
                    return Err(DecodeError::Limit {
                        offset: start,
                        kind: "literal",
                    });
                }
                Kind::String(take(bytes, &mut at, length)?)
            }
            // Their meaning is explicitly unknown in the inspected expression
            // reader. Native-argument n/z layouts cannot be substituted here.
            b'n' | b'z' => {
                return Err(DecodeError::Unsupported {
                    offset: start,
                    byte,
                });
            }
            _ => {
                if let Some(operator) = operators.at(&bytes[start..]) {
                    at = start + operator.spelling.len();
                    Kind::Operator {
                        code: operator.code,
                        precedence: operator.precedence,
                    }
                } else {
                    let length = decimal(&bytes[start..]);
                    if length == 0 {
                        return Err(DecodeError::Unsupported {
                            offset: start,
                            byte,
                        });
                    }
                    if length > limits.max_numeric_bytes {
                        return Err(DecodeError::Limit {
                            offset: start,
                            kind: "numeric lexeme",
                        });
                    }
                    at = start + length;
                    Kind::Number(&bytes[start..at])
                }
            }
        };
        tokens.push(Token {
            bytes: start..at,
            kind,
        });
    }
    Ok(Expression {
        bytes,
        tokens,
        pending_context_reference: context,
    })
}

pub fn statement<'a>(
    instruction: &Instruction<'a>,
    operators: &Operators,
    limits: Limits,
) -> Result<Option<Statement<'a>>, DecodeError> {
    let bytes = instruction.operands;
    let mut at = 0;
    let kind = match instruction.opcode {
        0x16 | 0x18 => StatementKind::Conditional {
            false_jump_bytes: word(bytes, &mut at)?,
        },
        0x15 => {
            let mut byte = take(bytes, &mut at, 1)?[0];
            let context_reference = if byte == b'r' {
                let index = word(bytes, &mut at)?;
                byte = take(bytes, &mut at, 1)?[0];
                Some(index)
            } else {
                None
            };
            let target = match byte {
                b'f' | b's' => Target::Local {
                    type_byte: byte,
                    index: word(bytes, &mut at)?,
                    context_reference,
                },
                b'G' if context_reference.is_none() => Target::Global {
                    reference_index: word(bytes, &mut at)?,
                },
                _ => {
                    return Err(DecodeError::Unsupported {
                        offset: at - 1,
                        byte,
                    });
                }
            };
            StatementKind::Assignment(target)
        }
        _ => return Ok(None),
    };
    let length = usize::from(word(bytes, &mut at)?);
    let expression_operand_offset = at;
    let expression = decode(take(bytes, &mut at, length)?, operators, limits)?;
    Ok(Some(Statement {
        kind,
        expression_operand_offset,
        expression,
        trailing: &bytes[at..],
    }))
}
