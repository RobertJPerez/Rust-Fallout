//! New Vegas compiled-script framing. Operands remain source bytes until their
//! individual formats and behavior have evidence. No instruction is executed here.
//!
//! Header and event layouts: xNVSE 0ccd23ad, ScriptAnalyzer::{ReadLine,
//! BeginStatement}; see docs/compiled-scripts.md for the precise reference scope.

use std::ops::Range;

pub const REFERENCE_CALL: u16 = 0x1c;
pub const BEGIN: u16 = 0x10;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    pub max_instructions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_instructions: 262_144,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Statement(&'static str),
    NativeCommand,
    Unknown,
}

/// These values name statement headers, not condition functions or event IDs.
pub fn kind(opcode: u16) -> Kind {
    match opcode {
        0x10 => Kind::Statement("begin"),
        0x11 => Kind::Statement("end"),
        0x12 => Kind::Statement("short"),
        0x13 => Kind::Statement("long"),
        0x14 => Kind::Statement("float"),
        0x15 => Kind::Statement("set_to"),
        0x16 => Kind::Statement("if"),
        0x17 => Kind::Statement("else"),
        0x18 => Kind::Statement("else_if"),
        0x19 => Kind::Statement("end_if"),
        0x1d => Kind::Statement("script_name"),
        0x1e => Kind::Statement("return"),
        0x1f => Kind::Statement("ref"),
        0x1000.. => Kind::NativeCommand,
        _ => Kind::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventHeader {
    pub id: u16,
    /// Preserve the authored jump field. Its execution origin is not inferred.
    pub end_jump_bytes: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Instruction<'a> {
    /// Offsets are relative to SCDA, including a reference-call prefix if present.
    pub bytes: Range<usize>,
    pub operand_offset: usize,
    pub opcode: u16,
    /// A reference-list index is not a FormID. Preserve an explicit zero too.
    pub calling_reference: Option<u16>,
    pub operands: &'a [u8],
    pub event: Option<EventHeader>,
}

impl Instruction<'_> {
    pub fn kind(&self) -> Kind {
        kind(self.opcode)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Program<'a> {
    pub bytes: &'a [u8],
    pub instructions: Vec<Instruction<'a>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("SCDA exceeds byte limit: {bytes} > {maximum}")]
    ByteLimit { bytes: usize, maximum: usize },
    #[error("SCDA byte 0x{offset:X}: instruction limit {maximum} exceeded")]
    InstructionLimit { offset: usize, maximum: usize },
    #[error("SCDA byte 0x{offset:X}: need {needed} bytes, only {remaining} remain")]
    Truncated {
        offset: usize,
        needed: usize,
        remaining: usize,
    },
    #[error("SCDA byte 0x{offset:X}: begin requires a six-byte event header, got {bytes}")]
    ShortEvent { offset: usize, bytes: usize },
}

fn take<'a>(bytes: &'a [u8], offset: &mut usize, count: usize) -> Result<&'a [u8], DecodeError> {
    let remaining = bytes.len().saturating_sub(*offset);
    if count > remaining {
        return Err(DecodeError::Truncated {
            offset: *offset,
            needed: count,
            remaining,
        });
    }
    let start = *offset;
    *offset += count;
    Ok(&bytes[start..*offset])
}

fn word(bytes: &[u8], offset: &mut usize) -> Result<u16, DecodeError> {
    let field = take(bytes, offset, 2)?;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

/// Read the complete stream without recursion or copying operand buffers. Unknown
/// opcodes retain their exact extent; a caller must not treat them as executable.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Program<'_>, DecodeError> {
    if bytes.len() > limits.max_bytes {
        return Err(DecodeError::ByteLimit {
            bytes: bytes.len(),
            maximum: limits.max_bytes,
        });
    }
    let mut offset = 0;
    let mut instructions = Vec::new();
    while offset < bytes.len() {
        if instructions.len() >= limits.max_instructions {
            return Err(DecodeError::InstructionLimit {
                offset,
                maximum: limits.max_instructions,
            });
        }
        let start = offset;
        let mut opcode = word(bytes, &mut offset)?;
        let calling_reference = if opcode == REFERENCE_CALL {
            let reference = word(bytes, &mut offset)?;
            opcode = word(bytes, &mut offset)?;
            Some(reference)
        } else {
            None
        };
        let length = usize::from(word(bytes, &mut offset)?);
        let operand_offset = offset;
        let operands = take(bytes, &mut offset, length)?;
        let event = if opcode == BEGIN {
            if operands.len() < 6 {
                return Err(DecodeError::ShortEvent {
                    offset: operand_offset,
                    bytes: operands.len(),
                });
            }
            Some(EventHeader {
                id: u16::from_le_bytes([operands[0], operands[1]]),
                end_jump_bytes: u32::from_le_bytes(
                    operands[2..6].try_into().expect("checked event header"),
                ),
            })
        } else {
            None
        };
        instructions.push(Instruction {
            bytes: start..offset,
            operand_offset,
            opcode,
            calling_reference,
            operands,
            event,
        });
    }
    Ok(Program {
        bytes,
        instructions,
    })
}
