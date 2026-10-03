//! Hash-bound structural comparison over the existing offline SCDA bundle.
use super::{control_flow, expression_plan_bundle::MAX_BUNDLE_BYTES};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub instructions: usize,
    pub complete_bodies: usize,
    pub structural_issues: usize,
    pub events: usize,
    pub arms: usize,
    pub links: usize,
    pub maximum_depth: usize,
}
#[derive(Debug, Serialize)]
pub struct Structure {
    pub events: Vec<control_flow::Event>,
    pub arms: Vec<control_flow::Arm>,
    pub links: Vec<control_flow::Link>,
    pub maximum_depth: usize,
}
#[derive(Debug, Serialize)]
pub struct Body {
    pub bytes: usize,
    pub sha256: String,
    pub instructions: usize,
    pub structure: Option<Structure>,
    pub issue: Option<control_flow::Diagnostic>,
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
    #[error("compiled body {body}: {message}")]
    Body { body: usize, message: String },
}

#[derive(Clone, Copy)]
struct Budget {
    bytes: usize,
    bodies: usize,
    instructions: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: MAX_BUNDLE_BYTES,
            bodies: 65_536,
            instructions: 2_000_000,
        }
    }
}

pub fn inspect(bytes: &[u8]) -> Result<Report, Error> {
    inspect_mode(bytes, false, Budget::default())
}
/// A full diagnostic report can contain unresolved bodies. Callers must return
/// failure when issues are present; no partial structure is returned for them.
pub fn inspect_diagnostic(bytes: &[u8]) -> Result<Report, Error> {
    inspect_mode(bytes, true, Budget::default())
}
fn inspect_mode(bytes: &[u8], diagnostic: bool, budget: Budget) -> Result<Report, Error> {
    if bytes.len() > budget.bytes || bytes.get(..8) != Some(b"FROBS001") {
        return Err(Error::Bundle);
    }
    let mut cursor = 8;
    let mut bodies = Vec::new();
    let mut counts = Counts::default();
    while cursor < bytes.len() {
        if bodies.len() >= budget.bodies {
            return Err(Error::Bundle);
        }
        let length = u32::from_le_bytes(
            bytes
                .get(cursor..cursor + 4)
                .ok_or(Error::Bundle)?
                .try_into()
                .expect("checked word"),
        ) as usize;
        cursor += 4;
        if length > super::Limits::default().max_bytes || length > bytes.len() - cursor {
            return Err(Error::Bundle);
        }
        let data = &bytes[cursor..cursor + length];
        cursor += length;
        let failure = |message| Error::Body {
            body: bodies.len(),
            message,
        };
        // Count freshly decoded instructions even for unresolved structure.
        // Diagnostic mode never converts malformed framing into a valid body.
        let decoded =
            super::decode(data, super::Limits::default()).map_err(|e| failure(e.to_string()))?;
        if decoded.instructions.len() > budget.instructions.saturating_sub(counts.instructions) {
            return Err(failure("aggregate instruction budget exceeded".into()));
        }
        counts.instructions += decoded.instructions.len();
        let (structure, issue) = match control_flow::decode(data, control_flow::Limits::default()) {
            Ok(plan) => {
                counts.complete_bodies += 1;
                counts.events += plan.events().len();
                counts.arms += plan.arms().len();
                counts.links += plan.links().len();
                counts.maximum_depth = counts.maximum_depth.max(plan.maximum_depth());
                (
                    Some(Structure {
                        events: plan.events().to_vec(),
                        arms: plan.arms().to_vec(),
                        links: plan.links().to_vec(),
                        maximum_depth: plan.maximum_depth(),
                    }),
                    None,
                )
            }
            Err(control_flow::Error::Structure(issue)) if diagnostic => {
                counts.structural_issues += 1;
                (None, Some(issue))
            }
            Err(error) => return Err(failure(error.to_string())),
        };
        bodies.push(Body {
            bytes: data.len(),
            sha256: format!("{:x}", Sha256::digest(data)),
            instructions: decoded.instructions.len(),
            structure,
            issue,
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
    fn bundle(bodies: &[&[u8]]) -> Vec<u8> {
        let mut bytes = b"FROBS001".to_vec();
        for body in bodies {
            bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
            bytes.extend_from_slice(body);
        }
        bytes
    }
    #[test]
    fn diagnostic_preserves_unresolved_bodies_and_rejects_bad_framing() {
        let invalid = [0x19, 0, 0, 0];
        let bytes = bundle(&[&[], &invalid]);
        assert!(inspect(&bytes).is_err());
        let result = inspect_diagnostic(&bytes).unwrap();
        assert_eq!(result.counts.complete_bodies, 1);
        assert_eq!(result.counts.structural_issues, 1);
        assert_eq!(result.counts.instructions, 1);
        assert!(!result.execution_ready);
        assert!(result.bodies[1].structure.is_none());
        assert_eq!(
            result.bodies[1].issue.as_ref().unwrap().kind,
            "orphan_end_if"
        );
        assert!(inspect_diagnostic(&bundle(&[&[0x16, 0]])).is_err());
        for end in 0..bytes.len() {
            if ![8, 12].contains(&end) {
                assert!(inspect_diagnostic(&bytes[..end]).is_err());
            }
        }
    }
    #[test]
    fn aggregate_limits_are_exact_and_apply_to_unresolved_bodies() {
        let bytes = bundle(&[&[0x19, 0, 0, 0]]);
        let budget = Budget {
            bytes: bytes.len(),
            bodies: 1,
            instructions: 1,
        };
        assert!(inspect_mode(&bytes, true, budget).is_ok());
        for budget in [
            Budget {
                bytes: bytes.len() - 1,
                ..budget
            },
            Budget {
                bodies: 0,
                ..budget
            },
            Budget {
                instructions: 0,
                ..budget
            },
        ] {
            assert!(inspect_mode(&bytes, true, budget).is_err());
        }
        assert!(inspect_mode(&bundle(&[&[], &[]]), true, budget).is_err());
    }
}
