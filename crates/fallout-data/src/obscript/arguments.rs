//! Vanilla native argument framing. This retains typed source operands without
//! resolving forms, converting numeric values or applying command side effects.
//! Classification/layout: pinned xNVSE GameAPI and ScriptAnalyzer. The caller
//! must supply a signature and parsing convention verified for its game profile.

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Convention {
    Default,
    Message,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct Parameter {
    pub type_id: u32,
    pub optional_word: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Signature<'a> {
    pub convention: Convention,
    pub parameters: &'a [Parameter],
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_arguments: usize,
    pub max_string_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 65535,
            max_arguments: 64,
            max_string_bytes: 512,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Value<'a> {
    String(&'a [u8]),
    SignedInteger(i32),
    /// Exact IEEE words, including nonfinite patterns. Numerical conversion and
    /// suitability for a particular native command belong to a later boundary.
    DoubleBits(u64),
    Short(u16),
    Byte(u8),
    Global {
        reference_index: u16,
    },
    Variable {
        type_byte: u8,
        index: u16,
        context_reference: Option<u16>,
    },
    FormReference {
        reference_index: u16,
    },
    FormVariable {
        index: u16,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Argument<'a> {
    pub bytes: Range<usize>,
    pub parameter_type_id: u32,
    pub value: Value<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MessageArgument<'a> {
    pub bytes: Range<usize>,
    pub value: Value<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Arguments<'a> {
    pub bytes: &'a [u8],
    /// Absent payload and an explicit zero count remain distinguishable.
    pub declared_count: Option<u16>,
    pub arguments: Vec<Argument<'a>>,
    pub declared_message_count: Option<u16>,
    pub message_arguments: Vec<MessageArgument<'a>>,
    pub trailing: &'a [u8],
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("native argument byte 0x{offset:X}: need {needed} bytes, only {remaining} remain")]
    Truncated {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error("native argument byte 0x{offset:X}: {kind} budget exceeded")]
    Limit { offset: usize, kind: &'static str },
    #[error("unverified native parsing convention")]
    Convention,
    #[error("native parameter {index}: optional word {word} has unverified semantics")]
    OptionalWord { index: usize, word: u32 },
    #[error("native parameter {index}: type ID {type_id} has no supported operand classification")]
    ParameterType { index: usize, type_id: u32 },
    #[error(
        "native argument count {declared} is outside required/signature range {minimum}..={maximum}"
    )]
    ArgumentCount {
        declared: usize,
        minimum: usize,
        maximum: usize,
    },
    #[error("native argument byte 0x{offset:X}: extension expression encoding is unsupported")]
    Extension { offset: usize },
    #[error("native argument byte 0x{offset:X}: unsupported operand prefix 0x{byte:02X}")]
    Prefix { offset: usize, byte: u8 },
    #[error("message argument count {count} exceeds the inspected limit of nine")]
    MessageCount { count: u16 },
}

// These are format classification facts from kClassifyParamExtract, not native
// implementations or a promise that the original extraction semantics match
// every extension. Values 8 are intentionally unsupported by this decoder.
const CLASSIFICATION: [u8; 70] = [
    0, 1, 4, 6, 6, 2, 6, 6, 3, 6, 2, 6, 6, 6, 6, 6, 6, 6, 2, 6, 6, 6, 8, 1, 6, 6, 6, 6, 2, 6, 6, 6,
    3, 6, 6, 6, 6, 6, 6, 6, 6, 2, 6, 6, 5, 7, 8, 6, 6, 6, 6, 2, 2, 6, 6, 2, 6, 6, 6, 6, 6, 6, 6, 6,
    6, 6, 6, 6, 6, 6,
];

pub fn classification(type_id: u32) -> Option<u8> {
    CLASSIFICATION
        .get(type_id as usize)
        .copied()
        .filter(|class| *class < 8)
}

fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8], DecodeError> {
    if count > bytes.len().saturating_sub(*at) {
        return Err(DecodeError::Truncated {
            offset: *at,
            needed: count,
            remaining: bytes.len().saturating_sub(*at),
        });
    }
    let start = *at;
    *at += count;
    Ok(&bytes[start..*at])
}
fn word(bytes: &[u8], at: &mut usize) -> Result<u16, DecodeError> {
    Ok(u16::from_le_bytes(
        take(bytes, at, 2)?.try_into().expect("checked word"),
    ))
}
fn byte(bytes: &[u8], at: &mut usize) -> Result<u8, DecodeError> {
    Ok(take(bytes, at, 1)?[0])
}
fn extension(bytes: &[u8], at: usize) -> Result<(), DecodeError> {
    if bytes
        .get(at..)
        .is_some_and(|bytes| bytes.starts_with(&[0xff, 0xff]))
    {
        Err(DecodeError::Extension { offset: at })
    } else {
        Ok(())
    }
}

fn variable<'a>(bytes: &'a [u8], at: &mut usize) -> Result<Value<'a>, DecodeError> {
    let mut prefix = byte(bytes, at)?;
    let context_reference = if prefix == b'r' {
        let index = word(bytes, at)?;
        prefix = byte(bytes, at)?;
        Some(index)
    } else {
        None
    };
    if !matches!(prefix, b'f' | b's') {
        return Err(DecodeError::Prefix {
            offset: *at - 1,
            byte: prefix,
        });
    }
    Ok(Value::Variable {
        type_byte: prefix,
        index: word(bytes, at)?,
        context_reference,
    })
}

fn numeric<'a>(bytes: &'a [u8], at: &mut usize) -> Result<Value<'a>, DecodeError> {
    extension(bytes, *at)?;
    let start = *at;
    match byte(bytes, at)? {
        b'n' => Ok(Value::SignedInteger(i32::from_le_bytes(
            take(bytes, at, 4)?.try_into().expect("checked integer"),
        ))),
        b'z' => Ok(Value::DoubleBits(u64::from_le_bytes(
            take(bytes, at, 8)?.try_into().expect("checked double"),
        ))),
        b'G' => Ok(Value::Global {
            reference_index: word(bytes, at)?,
        }),
        _ => {
            *at = start;
            variable(bytes, at)
        }
    }
}

pub fn decode<'a>(
    bytes: &'a [u8],
    signature: Signature<'_>,
    limits: Limits,
) -> Result<Arguments<'a>, DecodeError> {
    if bytes.len() > limits.max_bytes {
        return Err(DecodeError::Limit {
            offset: 0,
            kind: "byte",
        });
    }
    if signature.convention == Convention::Unknown {
        return Err(DecodeError::Convention);
    }
    if signature.parameters.len() > limits.max_arguments {
        return Err(DecodeError::Limit {
            offset: 0,
            kind: "signature",
        });
    }
    let mut minimum = 0;
    for (index, param) in signature.parameters.iter().enumerate() {
        match param.optional_word {
            0 => minimum = index + 1,
            1 => {}
            word => return Err(DecodeError::OptionalWord { index, word }),
        }
    }
    let mut result = Arguments {
        bytes,
        declared_count: None,
        arguments: Vec::new(),
        declared_message_count: None,
        message_arguments: Vec::new(),
        trailing: &[],
    };
    let mut at = 0;
    if bytes.is_empty() {
        if minimum > 0 {
            return Err(DecodeError::ArgumentCount {
                declared: 0,
                minimum,
                maximum: signature.parameters.len(),
            });
        }
        return Ok(result);
    }
    let count = word(bytes, &mut at)?;
    result.declared_count = Some(count);
    if count > 0x7fff {
        return Err(DecodeError::Extension { offset: 0 });
    }
    let count = usize::from(count);
    if count < minimum || count > signature.parameters.len() {
        return Err(DecodeError::ArgumentCount {
            declared: count,
            minimum,
            maximum: signature.parameters.len(),
        });
    }
    for (index, param) in signature.parameters[..count].iter().enumerate() {
        extension(bytes, at)?;
        let start = at;
        let class = classification(param.type_id).ok_or(DecodeError::ParameterType {
            index,
            type_id: param.type_id,
        })?;
        let value = match class {
            0 => {
                let size = usize::from(word(bytes, &mut at)?);
                if size > limits.max_string_bytes {
                    return Err(DecodeError::Limit {
                        offset: start,
                        kind: "string",
                    });
                }
                Value::String(take(bytes, &mut at, size)?)
            }
            1 | 4 | 5 => numeric(bytes, &mut at)?,
            2 => Value::Short(word(bytes, &mut at)?),
            3 => Value::Byte(byte(bytes, &mut at)?),
            6 => match byte(bytes, &mut at)? {
                b'r' => Value::FormReference {
                    reference_index: word(bytes, &mut at)?,
                },
                b'f' => Value::FormVariable {
                    index: word(bytes, &mut at)?,
                },
                prefix => {
                    return Err(DecodeError::Prefix {
                        offset: start,
                        byte: prefix,
                    });
                }
            },
            7 => variable(bytes, &mut at)?,
            _ => unreachable!("supported classification"),
        };
        result.arguments.push(Argument {
            bytes: start..at,
            parameter_type_id: param.type_id,
            value,
        });
    }
    if signature.convention == Convention::Message && at < bytes.len() {
        let count = word(bytes, &mut at)?;
        result.declared_message_count = Some(count);
        if count > 9 {
            return Err(DecodeError::MessageCount { count });
        }
        if usize::from(count) > limits.max_arguments.saturating_sub(result.arguments.len()) {
            return Err(DecodeError::Limit {
                offset: at - 2,
                kind: "argument",
            });
        }
        for _ in 0..count {
            let start = at;
            let value = numeric(bytes, &mut at)?;
            result.message_arguments.push(MessageArgument {
                bytes: start..at,
                value,
            });
        }
    }
    result.trailing = &bytes[at..];
    Ok(result)
}
