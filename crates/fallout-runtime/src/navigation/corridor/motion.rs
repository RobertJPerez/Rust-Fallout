//! Source-qualified local movement proposals over a verified corridor.
//! Requests still require a physics collision sweep before pose commit.
use super::{
    CorridorError, CorridorLimits, CorridorQuery, Link, Node, PlaneContract, VerifiedCorridor,
    debit,
};
use crate::navigation::{
    CostDecision, Route, RouteLimits, TriangleId,
    endpoint::{self, Classification, EndpointError, EndpointRequest},
};
use fallout_data::identity::FormKey;
use serde::{Deserialize, Serialize};
use std::mem::size_of;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalFootprint {
    /// Caller supplied circle radius in the NAVM source coordinate system.
    /// The actor/source collision owner must select this value explicitly.
    pub planar_radius: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalMotionLimits {
    pub segments: usize,
    pub endpoint_predicates: usize,
    pub portal_visits: usize,
    pub retained_bytes: usize,
}
impl Default for LocalMotionLimits {
    fn default() -> Self {
        Self {
            segments: 10_000,
            endpoint_predicates: 100_000,
            portal_visits: 10_000,
            retained_bytes: 8 * 1024 * 1024,
        }
    }
}
impl LocalMotionLimits {
    fn validate(self) -> Result<Self, CorridorError> {
        let cap = Self::default();
        if self.segments > cap.segments
            || self.endpoint_predicates > cap.endpoint_predicates
            || self.portal_visits > cap.portal_visits
            || self.retained_bytes > cap.retained_bytes
        {
            return Err(CorridorError::Invalid(
                "local motion limits may only reduce fixed ceilings",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocalMotionOptions {
    pub route_limits: RouteLimits,
    pub corridor_limits: CorridorLimits,
    pub footprint: LocalFootprint,
    pub motion_limits: LocalMotionLimits,
}

#[derive(Debug, Serialize)]
pub struct LocalMotionRequest {
    pub cell: FormKey,
    pub triangle: TriangleId,
    pub segment: LocalMotionSegment,
    pub source_plugin: String,
    pub source_sha256: String,
    pub decoded_navmesh_sha256: String,
    pub planar_radius: f64,
    /// Validate against the selected source collision scene before treating this
    /// proposal as executable movement.
    pub collision_sweep_required: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct LocalMotionSegment {
    pub start: [f64; 3],
    pub end: [f64; 3],
}

#[derive(Debug, Serialize)]
pub struct LocalMotionUsage {
    pub segments: usize,
    pub endpoint_predicates: usize,
    pub portal_visits: usize,
    pub retained_bytes: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LocalMotionState {
    Ready,
    AlreadyAtGoal,
    Refused { reason: &'static str },
}

#[derive(Debug, Serialize)]
pub struct LocalMotionOutcome {
    pub state: LocalMotionState,
    pub requests: Vec<LocalMotionRequest>,
    pub usage: LocalMotionUsage,
    pub collision_sweep_required: bool,
}
impl LocalMotionOutcome {
    fn refused(reason: &'static str) -> Self {
        Self {
            state: LocalMotionState::Refused { reason },
            requests: Vec::new(),
            usage: LocalMotionUsage {
                segments: 0,
                endpoint_predicates: 0,
                portal_visits: 0,
                retained_bytes: 0,
            },
            collision_sweep_required: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LocalRouteOutcome {
    pub route: Route,
    pub motion: LocalMotionOutcome,
    pub corridor_refusal: Option<&'static str>,
}

impl CorridorQuery {
    /// Uses this already source-loaded query for one bounded route and builds
    /// local segment proposals for supported same-cell geometry. A refusal is
    /// returned once; this API never retries a missing or unreachable destination.
    pub fn route_local_motion_requests(
        &self,
        start: &EndpointRequest,
        goal: &EndpointRequest,
        options: LocalMotionOptions,
        policy: impl FnMut(&Node, &Link, Option<&Node>) -> CostDecision,
    ) -> Result<LocalRouteOutcome, CorridorError> {
        let LocalMotionOptions {
            route_limits,
            corridor_limits,
            footprint,
            motion_limits,
        } = options;
        let PlaneContract::AxisAlignedDyadic { normal_axis } = start.plane;
        let PlaneContract::AxisAlignedDyadic {
            normal_axis: goal_axis,
        } = goal.plane;
        if normal_axis != goal_axis {
            return Err(CorridorError::Invalid(
                "local endpoints must use the same explicit plane",
            ));
        }
        if normal_axis > 2
            || start
                .point
                .iter()
                .chain(goal.point.iter())
                .any(|value| !value.is_finite())
            || !footprint.planar_radius.is_finite()
            || footprint.planar_radius < 0.
        {
            return Err(CorridorError::Invalid(
                "finite local endpoints and explicit nonnegative collision radius required",
            ));
        }
        motion_limits.validate()?;

        let routed = self.route(
            &start.triangle,
            &goal.triangle,
            route_limits,
            corridor_limits,
            start.plane,
            policy,
        )?;
        let motion = match routed.corridor.as_ref() {
            Some(corridor) => {
                corridor.local_motion_requests(start.point, goal.point, footprint, motion_limits)?
            }
            None => LocalMotionOutcome::refused(
                routed
                    .refusal
                    .unwrap_or("verified local corridor unavailable"),
            ),
        };
        Ok(LocalRouteOutcome {
            route: routed.route,
            motion,
            corridor_refusal: routed.refusal,
        })
    }
}

impl VerifiedCorridor {
    /// Makes one local segment per source triangle through each verified portal
    /// midpoint. The path stays in the authored corridor as a point path; radius
    /// checks portal aperture. Obstacle and actor-body collision checks remain
    /// the consumer's responsibility.
    pub fn local_motion_requests(
        &self,
        start: [f64; 3],
        goal: [f64; 3],
        footprint: LocalFootprint,
        limits: LocalMotionLimits,
    ) -> Result<LocalMotionOutcome, CorridorError> {
        let limits = limits.validate()?;
        if !footprint.planar_radius.is_finite() || footprint.planar_radius < 0. {
            return Err(CorridorError::Invalid(
                "explicit finite nonnegative planar collision radius required",
            ));
        }
        if self.triangles.is_empty() || self.portals.len() + 1 != self.triangles.len() {
            return Ok(LocalMotionOutcome::refused(
                "verified corridor segment topology differs",
            ));
        }
        let count = self.triangles.len();
        let mut predicates = limits.endpoint_predicates;
        let first = &self.triangles[0];
        let last = &self.triangles[count - 1];
        for (triangle, point, reason) in [
            (
                first,
                start,
                "start is outside or unsupported by its source triangle",
            ),
            (
                last,
                goal,
                "goal is outside or unsupported by its source triangle",
            ),
        ] {
            if !contained(triangle.vertices, point, self.plane, &mut predicates)? {
                return Ok(LocalMotionOutcome::refused(reason));
            }
        }
        if start == goal {
            return Ok(LocalMotionOutcome {
                state: LocalMotionState::AlreadyAtGoal,
                requests: Vec::new(),
                usage: LocalMotionUsage {
                    segments: 0,
                    endpoint_predicates: limits.endpoint_predicates - predicates,
                    portal_visits: 0,
                    retained_bytes: 0,
                },
                collision_sweep_required: false,
            });
        }

        if count > limits.segments {
            return Err(CorridorError::Budget("local motion segments"));
        }
        if self.portals.len() > limits.portal_visits {
            return Err(CorridorError::Budget("local motion portal visits"));
        }
        let estimated = retained_reservation(self, count)?;
        if estimated > limits.retained_bytes {
            return Err(CorridorError::Budget("local motion retained bytes"));
        }

        let PlaneContract::AxisAlignedDyadic { normal_axis } = self.plane;
        let mut points = Vec::with_capacity(count + 1);
        points.push(start);
        let mut portal_left = limits.portal_visits;
        for (index, portal) in self.portals.iter().enumerate() {
            debit(&mut portal_left, 1, "local motion portal visits")?;
            let axes = [(normal_axis + 1) % 3, (normal_axis + 2) % 3];
            let dx =
                f64::from(portal.vertices[1][axes[0]]) - f64::from(portal.vertices[0][axes[0]]);
            let dy =
                f64::from(portal.vertices[1][axes[1]]) - f64::from(portal.vertices[0][axes[1]]);
            let width = dx.hypot(dy);
            let diameter = footprint.planar_radius * 2.;
            let margin = width.max(diameter) * (8. * f64::EPSILON);
            if !width.is_finite() || !diameter.is_finite() || width - margin <= diameter {
                return Ok(LocalMotionOutcome::refused(
                    "source portal is narrower than the explicit collision footprint",
                ));
            }
            let point = std::array::from_fn(|axis| {
                (f64::from(portal.vertices[0][axis]) * 0.5)
                    + (f64::from(portal.vertices[1][axis]) * 0.5)
            });
            if !contained(
                self.triangles[index].vertices,
                point,
                self.plane,
                &mut predicates,
            )? || !contained(
                self.triangles[index + 1].vertices,
                point,
                self.plane,
                &mut predicates,
            )? {
                return Ok(LocalMotionOutcome::refused(
                    "source portal midpoint is not representable in both triangles",
                ));
            }
            points.push(point);
        }
        points.push(goal);

        let mut requests = Vec::with_capacity(count);
        for (index, triangle) in self.triangles.iter().enumerate() {
            requests.push(LocalMotionRequest {
                cell: self.cell.clone(),
                triangle: triangle.id.clone(),
                segment: LocalMotionSegment {
                    start: points[index],
                    end: points[index + 1],
                },
                source_plugin: triangle.source_plugin.clone(),
                source_sha256: triangle.source_sha256.clone(),
                decoded_navmesh_sha256: triangle.decoded_sha256.clone(),
                planar_radius: footprint.planar_radius,
                collision_sweep_required: true,
            });
        }
        Ok(LocalMotionOutcome {
            state: LocalMotionState::Ready,
            usage: LocalMotionUsage {
                segments: requests.len(),
                endpoint_predicates: limits.endpoint_predicates - predicates,
                portal_visits: limits.portal_visits - portal_left,
                retained_bytes: estimated,
            },
            requests,
            collision_sweep_required: true,
        })
    }
}

fn contained(
    vertices: [[f32; 3]; 3],
    point: [f64; 3],
    plane: PlaneContract,
    predicates: &mut usize,
) -> Result<bool, CorridorError> {
    match endpoint::classify(vertices, point, plane, predicates) {
        Ok(classification) => Ok(matches!(
            classification,
            Classification::Inside
                | Classification::OnEdge { .. }
                | Classification::OnVertex { .. }
        )),
        Err(EndpointError::Budget(_)) => Err(CorridorError::Budget("local endpoint predicates")),
        Err(_) => Err(CorridorError::Invalid("local endpoint source geometry")),
    }
}

fn retained_reservation(
    corridor: &VerifiedCorridor,
    segments: usize,
) -> Result<usize, CorridorError> {
    let point_bytes = segments
        .checked_add(1)
        .and_then(|count| size_of::<[f64; 3]>().checked_mul(count))
        .ok_or(CorridorError::Budget("local motion retained arithmetic"))?;
    let base = size_of::<LocalMotionRequest>()
        .checked_mul(segments)
        .and_then(|n| n.checked_add(point_bytes))
        .ok_or(CorridorError::Budget("local motion retained arithmetic"))?;
    corridor.triangles.iter().try_fold(base, |bytes, triangle| {
        let identity = triangle
            .source_plugin
            .len()
            .checked_add(triangle.source_sha256.len())
            .and_then(|n| n.checked_add(triangle.decoded_sha256.len()))
            .and_then(|n| n.checked_add(triangle.id.mesh.origin_plugin.len()))
            .and_then(|n| n.checked_add(corridor.cell.origin_plugin.len()))
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(512))
            .ok_or(CorridorError::Budget("local motion identity arithmetic"))?;
        bytes
            .checked_add(identity)
            .ok_or(CorridorError::Budget("local motion retained arithmetic"))
    })
}
