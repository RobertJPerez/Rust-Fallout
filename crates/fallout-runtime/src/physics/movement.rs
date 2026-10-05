//! Pure movement admission over the caller's existing explicit sphere sweep.
//!
//! This module returns a proposal only. It does not write canonical pose or
//! camera state, infer actor dimensions, or interpret support normals, gravity,
//! steps, slopes, filters, or retail movement policy. Query errors are refusals
//! for a movement consumer to handle; unsupported source shapes never become a
//! clear path.
use super::{
    QueryResult, Segment, SegmentIntersection, SegmentQueryLimits, SourceId, StaticScene,
    sweep::{SphereSweep, SweepLimits, SweepProposal, SweepState},
};
use serde::Serialize;

/// Exact caller-supplied source segment and source identity required for motion
/// admission. A matching hit proves only that this segment intersects this
/// source core; it does not establish a support normal or movement policy.
#[derive(Clone, Debug)]
pub struct SupportRequirement {
    pub segment: Segment,
    pub source: SourceId,
    pub limits: SegmentQueryLimits,
}

/// Inputs for a bounded body movement proposal. The `body` is the existing
/// caller-authored [`SphereSweep`] profile, including its explicit query-frame
/// radius, endpoints, tolerance, and scope. No dimensions or budgets are
/// inferred by this adapter.
#[derive(Clone, Debug)]
pub struct MovementRequest {
    pub body: SphereSweep,
    pub sweep_limits: SweepLimits,
    pub required_support: Option<SupportRequirement>,
}

/// Why a valid query refused the proposed movement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum MovementRefusal {
    SweepNotClear { state: SweepState },
    RequiredSupportMissing { expected_source: SourceId },
}

/// Immutable result for a caller to consume before it considers a pose commit.
#[derive(Debug, Serialize)]
pub struct MovementProposal {
    sweep: SweepProposal,
    support_witness: Option<SegmentIntersection>,
    accepted_center: Option<[f64; 3]>,
    refusal: Option<MovementRefusal>,
}

impl MovementProposal {
    /// Destination is present only when the complete sweep is clear and any
    /// requested exact support-source witness was found.
    pub fn accepted_center(&self) -> Option<[f64; 3]> {
        self.accepted_center
    }

    pub fn sweep(&self) -> &SweepProposal {
        &self.sweep
    }

    pub fn support_witness(&self) -> Option<&SegmentIntersection> {
        self.support_witness.as_ref()
    }

    pub fn refusal(&self) -> Option<&MovementRefusal> {
        self.refusal.as_ref()
    }
}

/// Evaluate one explicit body sweep and optional exact support-source witness.
///
/// A non-clear sweep or missing required witness returns a proposal without an
/// accepted destination. Unsupported geometry, numeric uncertainty, and budget
/// exhaustion return [`super::QueryError`] through `Err`; callers must treat
/// those errors as refused motion. The returned center is a proposal only and
/// carries no canonical pose or camera authority.
pub fn propose_movement(
    scene: &StaticScene,
    request: MovementRequest,
) -> QueryResult<MovementProposal> {
    let sweep = scene.sweep_sphere(request.body, request.sweep_limits)?;
    if sweep.state() != SweepState::Clear {
        return Ok(MovementProposal {
            refusal: Some(MovementRefusal::SweepNotClear {
                state: sweep.state(),
            }),
            sweep,
            support_witness: None,
            accepted_center: None,
        });
    }

    let (support_witness, refusal) = match request.required_support {
        Some(required) => {
            let expected_source = required.source;
            let report = scene.segment_cast(required.segment, required.limits)?;
            let witness = report
                .results
                .into_iter()
                .find(|hit| hit.provenance.source == expected_source);
            match witness {
                Some(witness) => (Some(witness), None),
                None => (
                    None,
                    Some(MovementRefusal::RequiredSupportMissing { expected_source }),
                ),
            }
        }
        None => (None, None),
    };

    let accepted_center = if refusal.is_none() {
        Some(sweep.proposed_center())
    } else {
        None
    };
    Ok(MovementProposal {
        sweep,
        support_witness,
        accepted_center,
        refusal,
    })
}
