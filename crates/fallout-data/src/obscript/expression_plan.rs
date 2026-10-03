//! Structural vanilla postfix plans. Nodes retain source tokens, never values.
//! The pinned analyzer documents unary tilde and binary operand order; it does
//! not establish original rounding, short-circuiting or command side effects.

use super::expression::{self, Expression, Kind, Operators, Token};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::ops::Range;

const SPELLINGS: [&[u8]; 16] = [
    b"(", b")", b"&&", b"||", b"<=", b"<", b">=", b">", b"==", b"!=", b"-", b"+", b"*", b"/", b"%",
    b"~",
];

/// This model is deliberately specific. An extension or another executable's
/// table needs its own verified model instead of inheriting these arities.
pub struct Model<'a> {
    operators: &'a Operators,
}
impl<'a> Model<'a> {
    pub(super) fn operators(&self) -> &Operators {
        self.operators
    }
    pub fn vanilla(operators: &'a Operators) -> Result<Self, Error> {
        if operators.entries().len() != SPELLINGS.len()
            || operators.entries().iter().any(|entry| {
                SPELLINGS.get(entry.code as usize).copied() != Some(entry.spelling.as_slice())
            })
        {
            return Err(Error::OperatorModel);
        }
        Ok(Self { operators })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub decoding: expression::Limits,
    pub max_nodes: usize,
    pub max_stack: usize,
    pub max_height: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            decoding: expression::Limits::default(),
            max_nodes: 65_536,
            max_stack: 65_536,
            max_height: 65_536,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Inputs {
    Operand,
    Unary { operand: usize },
    Binary { left: usize, right: usize },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Node {
    pub token_index: usize,
    pub inputs: Inputs,
    /// The semantic subtree is contiguous in this flat postfix arena. Context
    /// prefixes stay in the token stream and do not allocate operand nodes.
    pub subtree_start: usize,
    pub height: usize,
}

#[derive(Debug)]
pub struct Plan<'a> {
    expression: Expression<'a>,
    nodes: Vec<Node>,
    maximum_stack: usize,
}
impl<'a> Plan<'a> {
    pub fn tokens(&self) -> &[Token<'a>] {
        &self.expression.tokens
    }
    pub fn source_bytes(&self) -> &'a [u8] {
        self.expression.bytes
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
    pub fn root(&self) -> usize {
        self.nodes.len() - 1
    }
    pub fn maximum_stack(&self) -> usize {
        self.maximum_stack
    }
    pub fn height(&self) -> usize {
        self.nodes[self.root()].height
    }
    pub fn subtree(&self, node: usize) -> Option<Range<usize>> {
        self.nodes.get(node).map(|n| n.subtree_start..node + 1)
    }
    /// Each 33-byte tuple binds token index/extents, arity, operator ID, ordered
    /// children, subtree start and height. Absent children use u32::MAX. Raw
    /// expression and token hashes separately bind lexemes and command payloads.
    pub fn shape_sha256(&self) -> String {
        let mut hash = Sha256::new();
        for node in &self.nodes {
            let token = &self.expression.tokens[node.token_index];
            let code = match token.kind {
                Kind::Operator { code, .. } => code,
                _ => 0,
            };
            let (arity, left, right) = match node.inputs {
                Inputs::Operand => (0, u32::MAX, u32::MAX),
                Inputs::Unary { operand } => (1, operand as u32, u32::MAX),
                Inputs::Binary { left, right } => (2, left as u32, right as u32),
            };
            for value in [node.token_index, token.bytes.start, token.bytes.end] {
                hash.update((value as u32).to_le_bytes());
            }
            hash.update([arity]);
            for value in [
                code,
                left,
                right,
                node.subtree_start as u32,
                node.height as u32,
            ] {
                hash.update(value.to_le_bytes());
            }
        }
        format!("{:x}", hash.finalize())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error(transparent)]
    Decode(#[from] expression::DecodeError),
    #[error("operator descriptors do not match the selected vanilla structural model")]
    OperatorModel,
    #[error("expression byte 0x{offset:X}: unsupported structural operator {code}")]
    Operator { offset: usize, code: u32 },
    #[error(
        "expression byte 0x{offset:X}: operator {code} needs {needed} operands; found {available}"
    )]
    Underflow {
        offset: usize,
        code: u32,
        needed: usize,
        available: usize,
    },
    #[error("expression byte 0x{offset:X}: unconsumed context prefix")]
    Context { offset: usize },
    #[error("expression ends with {operands} operands instead of one")]
    Residual { operands: usize },
    #[error("expression byte 0x{offset:X}: {kind} structural budget exceeded")]
    Limit { offset: usize, kind: &'static str },
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub kind: &'static str,
    pub expression_byte_offset: Option<usize>,
    pub operator_code: Option<u32>,
    pub needed_operands: Option<usize>,
    pub available_operands: Option<usize>,
    pub budget: Option<&'static str>,
}
impl Error {
    pub fn diagnostic(&self) -> Diagnostic {
        let mut row = Diagnostic {
            kind: "decode",
            expression_byte_offset: None,
            operator_code: None,
            needed_operands: None,
            available_operands: None,
            budget: None,
        };
        match self {
            Self::OperatorModel => row.kind = "operator_model",
            Self::Operator { offset, code } => {
                row.kind = "operator";
                row.expression_byte_offset = Some(*offset);
                row.operator_code = Some(*code);
            }
            Self::Underflow {
                offset,
                code,
                needed,
                available,
            } => {
                row.kind = "underflow";
                row.expression_byte_offset = Some(*offset);
                row.operator_code = Some(*code);
                row.needed_operands = Some(*needed);
                row.available_operands = Some(*available);
            }
            Self::Context { offset } => {
                row.kind = "context";
                row.expression_byte_offset = Some(*offset);
            }
            Self::Residual { operands } => {
                row.kind = "residual";
                row.needed_operands = Some(1);
                row.available_operands = Some(*operands);
            }
            Self::Limit { offset, kind } => {
                row.kind = "limit";
                row.expression_byte_offset = Some(*offset);
                row.budget = Some(kind);
            }
            Self::Decode(_) => {}
        }
        row
    }
}

/// Decode once into a private token stream and an iterative arena. Successful
/// plans cannot be assembled with forged public token fields or mutable counters.
pub fn decode<'a>(bytes: &'a [u8], model: &Model<'_>, limits: Limits) -> Result<Plan<'a>, Error> {
    let expression = expression::decode(bytes, model.operators, limits.decoding)?;
    // All tuple fields fit u32 even if a caller raises the decoder limits.
    if expression.bytes.len() > u32::MAX as usize || expression.tokens.len() > u32::MAX as usize {
        return Err(Error::Limit {
            offset: 0,
            kind: "tuple width",
        });
    }
    let mut nodes = Vec::<Node>::new();
    let mut stack = Vec::<usize>::new();
    let mut maximum_stack = 0;
    let mut context: Option<usize> = None;
    for (token_index, token) in expression.tokens.iter().enumerate() {
        let offset = token.bytes.start;
        if matches!(token.kind, Kind::ReferencePrefix { .. }) {
            if let Some(offset) = context {
                return Err(Error::Context { offset });
            }
            context = Some(offset);
            continue;
        }
        if matches!(token.kind, Kind::Local { .. } | Kind::Command { .. }) {
            context = None;
        }
        if nodes.len() >= limits.max_nodes {
            return Err(Error::Limit {
                offset,
                kind: "node",
            });
        }
        let (inputs, subtree_start, height) = if let Kind::Operator { code, .. } = token.kind {
            let needed = match code {
                2..=14 => 2,
                15 => 1,
                _ => return Err(Error::Operator { offset, code }),
            };
            if stack.len() < needed {
                return Err(Error::Underflow {
                    offset,
                    code,
                    needed,
                    available: stack.len(),
                });
            }
            let right = stack.pop().expect("checked operand count");
            if needed == 1 {
                (
                    Inputs::Unary { operand: right },
                    nodes[right].subtree_start,
                    nodes[right].height + 1,
                )
            } else {
                let left = stack.pop().expect("checked operand count");
                (
                    Inputs::Binary { left, right },
                    nodes[left].subtree_start,
                    nodes[left].height.max(nodes[right].height) + 1,
                )
            }
        } else {
            (Inputs::Operand, nodes.len(), 1)
        };
        if height > limits.max_height {
            return Err(Error::Limit {
                offset,
                kind: "height",
            });
        }
        if stack.len() >= limits.max_stack {
            return Err(Error::Limit {
                offset,
                kind: "stack",
            });
        }
        stack.push(nodes.len());
        nodes.push(Node {
            token_index,
            inputs,
            subtree_start,
            height,
        });
        maximum_stack = maximum_stack.max(stack.len());
    }
    if let Some(offset) = context {
        return Err(Error::Context { offset });
    }
    if stack.len() != 1 {
        return Err(Error::Residual {
            operands: stack.len(),
        });
    }
    Ok(Plan {
        expression,
        nodes,
        maximum_stack,
    })
}

#[cfg(test)]
mod tests;
