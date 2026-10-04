//! Immutable, explicitly scoped engineering proposals. No actor or World writes.
use super::{Hit, QueryError, QueryResult, enclosure::Interval as I, math::*, shape::sphere_ray};
use fallout_data::{coordinates::Affine, nif_collision::Body};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepScope {
    /// A literal fixture profile, not an interpretation of retail motion tags.
    ZeroTaggedFrozenSphereCores,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SphereSweep {
    pub start: [f64; 3],
    pub end: [f64; 3],
    pub radius: f64,
    /// Maximum certified surface-distance and center-rounding error, query units.
    pub contact_tolerance: f64,
    pub scope: SweepScope,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SweepLimits {
    pub primitive_tests: usize,
    pub predicate_tests: usize,
    pub iterations: usize,
    pub contacts: usize,
}
impl Default for SweepLimits {
    fn default() -> Self {
        Self {
            primitive_tests: 10_000,
            predicate_tests: 10_240_000,
            iterations: 10_000,
            contacts: 10_000,
        }
    }
}
impl SweepLimits {
    pub(super) fn validate(self) -> QueryResult<()> {
        let max = Self::default();
        if self.primitive_tests > max.primitive_tests
            || self.predicate_tests > max.predicate_tests
            || self.iterations > max.iterations
            || self.contacts > max.contacts
        {
            return Err(QueryError::Budget(
                "sweep limits may only lower fixed ceilings",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SweepState {
    Clear,
    Contact,
    StartOverlap,
    StartTangent,
}
#[derive(Debug, Serialize)]
pub struct SweepContact {
    pub(super) provenance: Hit,
    pub(super) parameter_bounds: [f64; 2],
    pub(super) source_center: V,
    pub(super) source_radius: f64,
    pub(super) expanded_radius: f64,
    pub(super) separation_bounds: [f64; 2],
    pub(super) distance_bounds: [f64; 2],
}
#[derive(Debug, Serialize)]
pub struct SweepProposal {
    pub(super) state: SweepState,
    pub(super) parameter: f64,
    pub(super) proposed_center: V,
    pub(super) center_error_bounds: V,
    pub(super) contacts: Vec<SweepContact>,
    pub(super) work: SweepLimits,
}
impl SweepProposal {
    pub fn state(&self) -> SweepState {
        self.state
    }
    pub fn proposed_center(&self) -> V {
        self.proposed_center
    }
    pub fn parameter(&self) -> f64 {
        self.parameter
    }
}
fn uncertain() -> QueryError {
    QueryError::Invalid("sweep numeric certificate is uncertain or unrepresentable")
}
fn checked(value: Option<I>) -> QueryResult<I> {
    value.ok_or_else(uncertain)
}
pub(super) fn exact_sum(a: f64, b: f64, result: f64) -> bool {
    if !result.is_finite() || result != a + b {
        return false;
    }
    let back = result - a;
    (a - (result - back)) + (b - back) == 0.
}
pub(super) fn exact_product(a: f64, b: f64, result: f64) -> bool {
    result.is_finite()
        && result == a * b
        && (a == 0. || b == 0. || (result.abs() >= 1e-250 && a.mul_add(b, -result) == 0.))
}
fn point(i: I) -> bool {
    i.lower == i.upper
}
fn add(a: I, b: I) -> QueryResult<I> {
    let value = a.lower + b.lower;
    if point(a) && point(b) && exact_sum(a.lower, b.lower, value) {
        Ok(I::point(value))
    } else {
        checked(a.add(b))
    }
}
fn subtract(a: I, b: I) -> QueryResult<I> {
    add(a, b.negate())
}
fn multiply(a: I, b: I) -> QueryResult<I> {
    let value = a.lower * b.lower;
    if point(a) && point(b) && exact_product(a.lower, b.lower, value) {
        Ok(I::point(value))
    } else {
        checked(a.multiply(b))
    }
}
fn divide(a: I, b: I) -> QueryResult<I> {
    let value = a.lower / b.lower;
    if point(a)
        && point(b)
        && b.lower != 0.
        && value.is_finite()
        && exact_product(value, b.lower, a.lower)
    {
        Ok(I::point(value))
    } else {
        checked(a.divide(b))
    }
}
fn square(a: I) -> QueryResult<I> {
    if point(a) {
        return multiply(a, a);
    }
    let lower = if a.lower <= 0. && a.upper >= 0. {
        0.
    } else {
        (a.lower.abs().min(a.upper.abs()) * a.lower.abs().min(a.upper.abs()))
            .next_down()
            .max(0.)
    };
    checked(I::new(
        lower,
        (a.absolute_upper() * a.absolute_upper()).next_up(),
    ))
}
fn sqrt(a: I) -> QueryResult<I> {
    if a.lower < 0. {
        return Err(uncertain());
    }
    let value = a.lower.sqrt();
    if point(a) && exact_product(value, value, a.lower) {
        return Ok(I::point(value));
    }
    checked(I::new(value.next_down().max(0.), a.upper.sqrt().next_up()))
}
fn dot_bounds(a: [I; 3], b: [I; 3]) -> QueryResult<I> {
    let mut result = I::point(0.);
    for i in 0..3 {
        result = add(result, multiply(a[i], b[i])?)?;
    }
    Ok(result)
}
fn norm(a: [I; 3]) -> QueryResult<I> {
    let mut result = I::point(0.);
    for v in a {
        result = add(result, square(v)?)?;
    }
    // Directed operations may widen an exactly zero lower bound below zero.
    result.lower = result.lower.max(0.);
    sqrt(result)
}
pub(super) fn profile(body: &Body) -> bool {
    let filter = |f: &fallout_data::nif_collision::Filter| {
        f.layer == 0 && f.flags_and_parts == 0 && f.group == 0
    };
    filter(&body.world.filter)
        && filter(&body.filter_copy)
        && body.world.broad_phase == 0
        && body.entity_response == 0
        && body.collision_response == 0
        && body.motion_system == 0
        && body.quality == 0
        && body.deactivator == 0
        && body.solver_deactivation == 0
        && body.flags == 0
        && body.constraints.is_empty()
        && body
            .linear_velocity
            .iter()
            .chain(&body.angular_velocity)
            .chain(&body.center)
            .chain(body.inertia.iter().flatten())
            .all(|v| *v == 0.)
        && [
            body.mass,
            body.linear_damping,
            body.angular_damping,
            body.friction,
            body.restitution,
            body.max_linear_velocity,
            body.max_angular_velocity,
            body.penetration_depth,
        ]
        .iter()
        .all(|v| *v == 0.)
}
pub(super) fn sphere_frame(frame: Affine, radius: f64) -> QueryResult<(V, f64)> {
    let mut scale = None;
    let mut columns = [false; 3];
    for row in frame.rows {
        let nonzero: Vec<_> = row[..3]
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0.)
            .collect();
        if nonzero.len() != 1 {
            return Err(uncertain());
        }
        let (column, value) = nonzero[0];
        let value = value.abs();
        if !value.is_normal()
            || value.to_bits() & ((1u64 << 52) - 1) != 0
            || columns[column]
            || scale.is_some_and(|s| s != value)
        {
            return Err(uncertain());
        }
        columns[column] = true;
        scale = Some(value);
    }
    let scale = scale.ok_or_else(uncertain)?;
    let result = radius * scale;
    let center = frame.rows.map(|row| row[3]);
    if !query_domain(center) || !exact_product(radius, scale, result) || !scalar(result) {
        return Err(uncertain());
    }
    Ok((center, result))
}
fn scalar(v: f64) -> bool {
    v.is_finite() && (v == 0. || (1e-100..=1e50).contains(&v.abs()))
}
pub(super) fn trajectory(request: SphereSweep) -> QueryResult<V> {
    match request.scope {
        SweepScope::ZeroTaggedFrozenSphereCores => (),
    }
    if !request.start.into_iter().chain(request.end).all(scalar)
        || !scalar(request.radius)
        || request.radius < 0.
        || !scalar(request.contact_tolerance)
        || request.contact_tolerance <= 0.
    {
        return Err(uncertain());
    }
    let delta = sub(request.end, request.start);
    for (i, value) in delta.iter().enumerate() {
        if !scalar(*value) || !exact_sum(request.end[i], -request.start[i], *value) {
            return Err(uncertain());
        }
    }
    Ok(delta)
}
pub(super) fn witness(request: SphereSweep, delta: V, parameter: f64) -> QueryResult<(V, V)> {
    if !(0. ..=1.).contains(&parameter) {
        return Err(uncertain());
    }
    let center = std::array::from_fn(|i| delta[i].mul_add(parameter, request.start[i]));
    let mut error = [0.; 3];
    for i in 0..3 {
        let exact = add(
            I::point(request.start[i]),
            multiply(I::point(delta[i]), I::point(parameter))?,
        )?;
        let difference = subtract(exact, I::point(center[i]))?;
        error[i] = difference.absolute_upper();
        if !center[i].is_finite() || error[i] > request.contact_tolerance {
            return Err(uncertain());
        }
    }
    Ok((center, error))
}
pub(super) struct Crossing {
    pub state: SweepState,
    pub bounds: I,
    pub expanded_radius: f64,
}
pub(super) fn crossing(
    request: SphereSweep,
    delta: V,
    center: V,
    radius: f64,
) -> QueryResult<Option<Crossing>> {
    let expanded = request.radius + radius;
    if !scalar(expanded) || !exact_sum(request.radius, radius, expanded) {
        return Err(uncertain());
    }
    let relative = sub(request.start, center);
    for i in 0..3 {
        if !scalar(relative[i]) || !exact_sum(request.start[i], -center[i], relative[i]) {
            return Err(uncertain());
        }
    }
    let p = relative.map(I::point);
    let d = delta.map(I::point);
    let c = subtract(dot_bounds(p, p)?, square(I::point(expanded))?)?;
    let initial = if c.upper < 0. {
        Some(SweepState::StartOverlap)
    } else if c.lower == 0. && c.upper == 0. {
        Some(SweepState::StartTangent)
    } else if c.lower > 0. {
        None
    } else {
        return Err(uncertain());
    };
    if let Some(state) = initial {
        return Ok(Some(Crossing {
            state,
            bounds: I::point(0.),
            expanded_radius: expanded,
        }));
    }
    if delta == [0.; 3] {
        return Ok(None);
    }
    // Existing compensated perpendicular-distance ray predicate is an additional
    // refusal/estimate, never the authority for full segment clearance.
    let estimate = sphere_ray(request.start, delta, center, expanded)?;
    let a = dot_bounds(d, d)?;
    if a.lower <= 0. {
        return Err(uncertain());
    }
    let b = dot_bounds(p, d)?;
    if b.lower >= 0. {
        return Ok(None);
    }
    let discriminant = subtract(square(b)?, multiply(a, c)?)?;
    if discriminant.upper < 0. {
        return Ok(None);
    }
    if discriminant.lower < 0. {
        return Err(uncertain());
    }
    let bounds = divide(subtract(b.negate(), sqrt(discriminant)?)?, a)?;
    if bounds.lower > 1. {
        return Ok(None);
    }
    if bounds.lower < 0. || bounds.upper > 1. {
        return Err(uncertain());
    }
    // A conflicting estimate cannot be promoted into a contact.
    if estimate.is_none_or(|t| t < bounds.lower || t > bounds.upper) {
        return Err(uncertain());
    }
    Ok(Some(Crossing {
        state: SweepState::Contact,
        bounds,
        expanded_radius: expanded,
    }))
}
pub(super) fn separation(
    request: SphereSweep,
    delta: V,
    parameter: f64,
    center: V,
    expanded: f64,
) -> QueryResult<I> {
    let mut p = [I::point(0.); 3];
    for i in 0..3 {
        p[i] = subtract(
            add(
                I::point(request.start[i]),
                multiply(I::point(delta[i]), I::point(parameter))?,
            )?,
            I::point(center[i]),
        )?;
    }
    subtract(norm(p)?, I::point(expanded))
}
pub(super) fn distance_bounds(delta: V, parameter: f64) -> QueryResult<I> {
    multiply(norm(delta.map(I::point))?, I::point(parameter))
}
