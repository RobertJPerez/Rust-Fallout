//! Strict inspection of the existing FROBS001 offline comparison bundle. Its
//! bodies are bound by hashes; plugin provenance is supplied by the extraction
//! receipt, not inferred from this interchange format.
use super::{Limits, expression, expression_census, expression_plan};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MAX_BUNDLE_BYTES: usize = 66 * 1024 * 1024;
struct Budget {
    bytes: usize,
    bodies: usize,
    statements: usize,
    nodes: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: MAX_BUNDLE_BYTES,
            bodies: 65_536,
            statements: 262_144,
            nodes: 2_000_000,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub statements: usize,
    pub complete_plans: usize,
    pub structural_issues: usize,
    pub nodes: usize,
    pub maximum_stack: usize,
    pub maximum_height: usize,
}
#[derive(Debug, Serialize)]
pub struct Statement {
    pub instruction_scda_offset: usize,
    pub opcode: u16,
    pub expression_operand_offset: usize,
    pub expression_sha256: String,
    pub token_sha256: String,
    pub plan: Option<Facts>,
    pub issue: Option<expression_plan::Diagnostic>,
}
#[derive(Debug, Serialize)]
pub struct Facts {
    pub shape_sha256: String,
    pub nodes: usize,
    pub root: usize,
    pub maximum_stack: usize,
    pub height: usize,
}
#[derive(Debug, Serialize)]
pub struct Body {
    pub bytes: usize,
    pub sha256: String,
    pub statements: Vec<Statement>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub bundle_sha256: String,
    pub compiled_bodies: usize,
    pub counts: Counts,
    pub bodies: Vec<Body>,
    pub execution_ready: bool,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid, truncated or oversized FROBS001 bundle")]
    Bundle,
    #[error("compiled body {body}, SCDA byte 0x{offset:X}: {message}")]
    Body {
        body: usize,
        offset: usize,
        message: String,
    },
}
pub fn inspect(bytes: &[u8], model: &expression_plan::Model<'_>) -> Result<Report, Error> {
    inspect_mode(bytes, model, false, Budget::default())
}
/// Diagnostics retain source statements which cannot produce a complete plan.
/// This does not admit them to execution; callers must surface the issue count.
pub fn inspect_diagnostic(
    bytes: &[u8],
    model: &expression_plan::Model<'_>,
) -> Result<Report, Error> {
    inspect_mode(bytes, model, true, Budget::default())
}
fn inspect_mode(
    bytes: &[u8],
    model: &expression_plan::Model<'_>,
    diagnostic: bool,
    budget: Budget,
) -> Result<Report, Error> {
    if bytes.len() > budget.bytes || bytes.get(..8) != Some(b"FROBS001") {
        return Err(Error::Bundle);
    }
    let mut at = 8;
    let mut bodies = Vec::new();
    let mut counts = Counts::default();
    while at < bytes.len() {
        if bodies.len() >= budget.bodies {
            return Err(Error::Bundle);
        }
        let word: [u8; 4] = bytes
            .get(at..at + 4)
            .ok_or(Error::Bundle)?
            .try_into()
            .expect("checked length");
        at += 4;
        let length = u32::from_le_bytes(word) as usize;
        if length > Limits::default().max_bytes || length > bytes.len() - at {
            return Err(Error::Bundle);
        }
        let data = &bytes[at..at + length];
        at += length;
        let failure = |offset, message| Error::Body {
            body: bodies.len(),
            offset,
            message,
        };
        let program =
            super::decode(data, Limits::default()).map_err(|e| failure(0, e.to_string()))?;
        let mut statements = Vec::new();
        for instruction in &program.instructions {
            let statement = expression::statement(
                instruction,
                model.operators(),
                expression::Limits::default(),
            )
            .map_err(|e| failure(instruction.bytes.start, e.to_string()))?;
            let Some(statement) = statement else {
                continue;
            };
            if counts.statements >= budget.statements {
                return Err(failure(
                    instruction.bytes.start,
                    "aggregate statement budget exceeded".into(),
                ));
            }
            let offset = instruction.operand_offset + statement.expression_operand_offset;
            if !statement.trailing.is_empty() {
                return Err(failure(
                    offset,
                    "uninterpreted expression statement tail".into(),
                ));
            }
            let plan = expression_plan::decode(
                statement.expression.bytes,
                model,
                expression_plan::Limits::default(),
            );
            let (facts, issue) = match plan {
                Ok(plan) => {
                    if plan.nodes().len() > budget.nodes.saturating_sub(counts.nodes) {
                        return Err(failure(
                            offset,
                            "aggregate plan node budget exceeded".into(),
                        ));
                    }
                    counts.complete_plans += 1;
                    counts.nodes += plan.nodes().len();
                    counts.maximum_stack = counts.maximum_stack.max(plan.maximum_stack());
                    counts.maximum_height = counts.maximum_height.max(plan.height());
                    (
                        Some(Facts {
                            shape_sha256: plan.shape_sha256(),
                            nodes: plan.nodes().len(),
                            root: plan.root(),
                            maximum_stack: plan.maximum_stack(),
                            height: plan.height(),
                        }),
                        None,
                    )
                }
                Err(error) if diagnostic => {
                    counts.structural_issues += 1;
                    (None, Some(error.diagnostic()))
                }
                Err(error) => return Err(failure(offset, error.to_string())),
            };
            counts.statements += 1;
            statements.push(Statement {
                instruction_scda_offset: instruction.bytes.start,
                opcode: instruction.opcode,
                expression_operand_offset: statement.expression_operand_offset,
                expression_sha256: format!("{:x}", Sha256::digest(statement.expression.bytes)),
                token_sha256: expression_census::token_digest(&statement.expression.tokens),
                plan: facts,
                issue,
            });
        }
        bodies.push(Body {
            bytes: data.len(),
            sha256: format!("{:x}", Sha256::digest(data)),
            statements,
        });
    }
    Ok(Report {
        schema_version: 1,
        bundle_sha256: format!("{:x}", Sha256::digest(bytes)),
        compiled_bodies: bodies.len(),
        counts,
        bodies,
        execution_ready: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_bundle_framing_and_statement_tails_fail_before_success() {
        let spellings = [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ];
        let operators = expression::Operators::new(
            spellings
                .iter()
                .enumerate()
                .map(|(code, s)| expression::Operator {
                    code: code as u32,
                    precedence: 0,
                    spelling: s.as_bytes().to_vec(),
                })
                .collect(),
        )
        .unwrap();
        let model = expression_plan::Model::vanilla(&operators).unwrap();
        for bytes in [
            b"".as_slice(),
            b"FROBS",
            b"XROBS001",
            b"FROBS001\x01",
            b"FROBS001\xff\xff\xff\xff",
            b"FROBS001\x01\0\0\0",
        ] {
            assert!(inspect(bytes, &model).is_err());
        }
        let data = [0x16, 0, 6, 0, 0, 0, 1, 0, b'1', 0xff];
        let bytes = [
            b"FROBS001".as_slice(),
            &(data.len() as u32).to_le_bytes(),
            &data,
        ]
        .concat();
        assert!(matches!(inspect(&bytes, &model), Err(Error::Body { .. })));
        let mut valid = bytes;
        valid.pop();
        valid[8..12].copy_from_slice(&9_u32.to_le_bytes());
        valid[14..16].copy_from_slice(&5_u16.to_le_bytes());
        let report = inspect(&valid, &model).unwrap();
        assert_eq!(report.counts.statements, 1);
        assert_eq!(report.counts.nodes, 1);
        assert!(!report.execution_ready);
    }

    #[test]
    fn diagnostic_retention_never_turns_a_residual_stack_into_a_complete_plan() {
        let spellings = [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ];
        let operators = expression::Operators::new(
            spellings
                .iter()
                .enumerate()
                .map(|(code, s)| expression::Operator {
                    code: code as u32,
                    precedence: 0,
                    spelling: s.as_bytes().to_vec(),
                })
                .collect(),
        )
        .unwrap();
        let model = expression_plan::Model::vanilla(&operators).unwrap();
        let data = [
            0x16, 0, 7, 0, 0, 0, 3, 0, b'1', b' ', b'2', 0x18, 0, 5, 0, 0, 0, 1, 0, b'3',
        ];
        let bytes = [
            b"FROBS001".as_slice(),
            &(data.len() as u32).to_le_bytes(),
            &data,
        ]
        .concat();
        assert!(matches!(
            inspect(&bytes, &model),
            Err(Error::Body {
                body: 0,
                offset: 8,
                ..
            })
        ));
        let report = inspect_diagnostic(&bytes, &model).unwrap();
        assert_eq!(report.counts.statements, 2);
        assert_eq!(report.counts.complete_plans, 1);
        assert_eq!(report.counts.structural_issues, 1);
        assert!(report.bodies[0].statements[0].plan.is_none());
        let issue = report.bodies[0].statements[0].issue.as_ref().unwrap();
        assert_eq!(issue.kind, "residual");
        assert_eq!(issue.available_operands, Some(2));
        assert!(report.bodies[0].statements[1].issue.is_none());
        assert!(!report.execution_ready);
    }

    #[test]
    fn aggregate_report_capacity_is_checked_independently_of_each_expression() {
        let spellings = [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ];
        let operators = expression::Operators::new(
            spellings
                .iter()
                .enumerate()
                .map(|(code, s)| expression::Operator {
                    code: code as u32,
                    precedence: 0,
                    spelling: s.as_bytes().to_vec(),
                })
                .collect(),
        )
        .unwrap();
        let model = expression_plan::Model::vanilla(&operators).unwrap();
        let data = [0x16, 0, 5, 0, 0, 0, 1, 0, b'1'];
        let bytes = [
            b"FROBS001".as_slice(),
            &(data.len() as u32).to_le_bytes(),
            &data,
        ]
        .concat();
        let exact = || Budget {
            bytes: bytes.len(),
            bodies: 1,
            statements: 1,
            nodes: 1,
        };
        assert!(inspect_mode(&bytes, &model, false, exact()).is_ok());
        for budget in [
            Budget {
                bytes: bytes.len() - 1,
                ..exact()
            },
            Budget {
                bodies: 0,
                ..exact()
            },
            Budget {
                statements: 0,
                ..exact()
            },
            Budget {
                nodes: 0,
                ..exact()
            },
        ] {
            assert!(inspect_mode(&bytes, &model, true, budget).is_err());
        }
        let empty_bodies = [b"FROBS001".as_slice(), &[0; 8]].concat();
        assert!(
            inspect_mode(
                &empty_bodies,
                &model,
                false,
                Budget {
                    bodies: 1,
                    ..Budget::default()
                }
            )
            .is_err()
        );
    }
}
