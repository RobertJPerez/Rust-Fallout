//! Original-parameter geometry and independent point witnesses. No normalization,
//! tolerance expansion, source parser, save state, or imported certificate.
use super::{
    CoreGeometry, QueryError, QueryResult, Segment,
    enclosure::{self, Interval as I},
    math::{V, compose},
    shape::{Shape, difference_error, predicate_product, predicate_sum},
};
use fallout_data::coordinates::Affine;

pub(super) type IV = [I; 3];
fn uncertain() -> QueryError {
    QueryError::Invalid("finite geometry predicate is numerically uncertain")
}
fn checked(value: Option<I>) -> QueryResult<I> {
    value.ok_or_else(uncertain)
}
pub(super) fn add(a: I, b: I) -> QueryResult<I> {
    checked(predicate_sum(a, b))
}
fn subtract(a: I, b: I) -> QueryResult<I> {
    add(a, b.negate())
}
pub(super) fn product(a: I, b: I) -> QueryResult<I> {
    checked(predicate_product(a, b))
}
fn divide(a: I, b: I) -> QueryResult<I> {
    if b.lower <= 0. && b.upper >= 0. {
        return Err(uncertain());
    }
    if a.lower == a.upper && b.lower == b.upper {
        let q = a.lower / b.lower;
        if q.is_finite()
            && product(I::point(q), b).is_ok_and(|p| p.lower == a.lower && p.upper == a.upper)
        {
            return Ok(I::point(q));
        }
    }
    checked(a.divide(b))
}
fn square(a: I) -> QueryResult<I> {
    let lower = if a.lower <= 0. && a.upper >= 0. {
        0.
    } else {
        a.lower.abs().min(a.upper.abs())
    };
    let upper = a.lower.abs().max(a.upper.abs());
    checked(I::new(
        product(I::point(lower), I::point(lower))?.lower.max(0.),
        product(I::point(upper), I::point(upper))?.upper,
    ))
}
fn sqrt(a: I) -> QueryResult<I> {
    if a.lower < 0. {
        return Err(uncertain());
    }
    let endpoint = |value: f64, lower: bool| -> QueryResult<f64> {
        if value == 0. {
            return Ok(0.);
        }
        let estimate = value.sqrt();
        let exact = product(I::point(estimate), I::point(estimate))?;
        if exact.lower == value && exact.upper == value {
            return Ok(estimate);
        }
        // sqrt only supplies a candidate. Independent square enclosures prove
        // each endpoint, including platforms with a different last-bit result.
        let mut bound = estimate;
        for _ in 0..4 {
            bound = if lower {
                bound.next_down().max(0.)
            } else {
                bound.next_up()
            };
            let squared = product(I::point(bound), I::point(bound))?;
            if (lower && squared.upper <= value) || (!lower && squared.lower >= value) {
                return Ok(bound);
            }
        }
        Err(uncertain())
    };
    checked(I::new(endpoint(a.lower, true)?, endpoint(a.upper, false)?))
}
pub(super) fn points(v: V) -> IV {
    v.map(I::point)
}
fn point_values(v: IV) -> Option<V> {
    v.iter()
        .all(|x| x.lower == x.upper)
        .then(|| v.map(|x| x.lower))
}
fn vector_sub(a: IV, b: IV) -> QueryResult<IV> {
    Ok([
        subtract(a[0], b[0])?,
        subtract(a[1], b[1])?,
        subtract(a[2], b[2])?,
    ])
}
fn dot(a: IV, b: IV) -> QueryResult<I> {
    let mut sum = I::point(0.);
    for i in 0..3 {
        sum = add(sum, product(a[i], b[i])?)?;
    }
    Ok(sum)
}
fn norm2(a: IV) -> QueryResult<I> {
    let mut sum = I::point(0.);
    for v in a {
        sum = add(sum, square(v)?)?;
    }
    checked(I::new(sum.lower.max(0.), sum.upper))
}
fn cross(a: IV, b: IV) -> QueryResult<IV> {
    Ok([
        subtract(product(a[1], b[2])?, product(a[2], b[1])?)?,
        subtract(product(a[2], b[0])?, product(a[0], b[2])?)?,
        subtract(product(a[0], b[1])?, product(a[1], b[0])?)?,
    ])
}
fn triple(a: IV, b: IV, c: IV) -> QueryResult<I> {
    dot(a, cross(b, c)?)
}
fn determinant(a: IV, b: IV, i: usize, j: usize) -> QueryResult<I> {
    subtract(product(a[i], b[j])?, product(a[j], b[i])?)
}
fn mid(a: I) -> f64 {
    0.5 * a.lower + 0.5 * a.upper
}
pub(super) fn exact_delta(segment: Segment) -> QueryResult<V> {
    let delta = std::array::from_fn(|i| segment.end[i] - segment.start[i]);
    if (0..3).any(|i| {
        !delta[i].is_finite() || difference_error(segment.end[i], segment.start[i], delta[i]) != 0.
    }) {
        return Err(QueryError::Invalid(
            "finite endpoint subtraction is numerically uncertain",
        ));
    }
    Ok(delta)
}
pub(super) fn original_delta(segment: Segment) -> QueryResult<IV> {
    if let Ok(delta) = exact_delta(segment) {
        return Ok(points(delta));
    }
    let mut delta = [I::point(0.); 3];
    for (i, target) in delta.iter_mut().enumerate() {
        let value = segment.end[i] - segment.start[i];
        let residual = difference_error(segment.end[i], segment.start[i], value);
        *target = checked(I::new(
            if residual < 0. {
                value.next_down()
            } else {
                value
            },
            if residual > 0. {
                value.next_up()
            } else {
                value
            },
        ))?;
    }
    Ok(delta)
}
pub(super) fn exact_point(origin: V, direction: V, parameter: f64) -> QueryResult<V> {
    if parameter == 0. {
        return Ok(origin);
    }
    let mut result = [0.; 3];
    for i in 0..3 {
        let value = add(
            I::point(origin[i]),
            product(I::point(direction[i]), I::point(parameter))?,
        )?;
        if value.lower != value.upper {
            return Err(QueryError::Invalid(
                "original finite-line point is not exactly representable",
            ));
        }
        result[i] = value.lower;
    }
    Ok(result)
}
pub(super) fn segment_point(segment: Segment, parameter: f64) -> QueryResult<V> {
    if parameter == 0. {
        return Ok(segment.start);
    }
    if parameter == 1. {
        return Ok(segment.end);
    }
    let delta = original_delta(segment)?;
    let mut point = [0.; 3];
    for i in 0..3 {
        let direct = add(
            I::point(segment.start[i]),
            product(delta[i], I::point(parameter))?,
        )?;
        let value = if direct.lower == direct.upper {
            direct
        } else {
            // This is the same original interpolation, with independent
            // directed products. A second evaluation can certify exact
            // cancellation without replacing the endpoint line.
            add(
                product(
                    I::point(segment.start[i]),
                    subtract(I::point(1.), I::point(parameter))?,
                )?,
                product(I::point(segment.end[i]), I::point(parameter))?,
            )?
        };
        if value.lower != value.upper {
            return Err(QueryError::Invalid(
                "original finite-line point is not exactly representable",
            ));
        }
        point[i] = value.lower;
    }
    Ok(point)
}
pub(super) fn distance_bounds(direction: IV, parameter: f64) -> QueryResult<I> {
    if parameter == 0. {
        return Ok(I::point(0.));
    }
    let count = direction
        .iter()
        .filter(|v| v.lower != 0. || v.upper != 0.)
        .count();
    let length = if count == 1 {
        let value = direction
            .into_iter()
            .find(|v| v.lower != 0. || v.upper != 0.)
            .expect("one component");
        I {
            lower: if value.lower <= 0. && value.upper >= 0. {
                0.
            } else {
                value.lower.abs().min(value.upper.abs())
            },
            upper: value.absolute_upper(),
        }
    } else {
        sqrt(norm2(direction)?)?
    };
    product(length, I::point(parameter))
}

/// An independent certificate for the actual source matrix recipe. It does not
/// depend on the held moving-sphere fixture/tag profile or its math helpers.
pub(super) fn exact_compose(a: Affine, b: Affine) -> bool {
    let output = compose(a, b);
    let verify = || -> QueryResult<bool> {
        for i in 0..3 {
            for j in 0..4 {
                let mut value = I::point(0.);
                for k in 0..3 {
                    value = add(
                        value,
                        product(I::point(a.rows[i][k]), I::point(b.rows[k][j]))?,
                    )?;
                }
                if j == 3 {
                    value = add(value, I::point(a.rows[i][3]))?;
                }
                if value.lower != value.upper || value.lower != output.rows[i][j] {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    };
    verify().unwrap_or(false)
}
pub(super) fn quaternion_recipe_exact([x, y, z, w]: [f32; 4]) -> bool {
    let [x, y, z, w] = [x, y, z, w].map(|v| I::point(f64::from(v)));
    let verify = || -> QueryResult<bool> {
        let two = I::point(2.);
        let diagonal = |a, b| {
            subtract(
                I::point(1.),
                product(two, add(product(a, a)?, product(b, b)?)?)?,
            )
        };
        let difference = |a, b, c, d| product(two, subtract(product(a, b)?, product(c, d)?)?);
        let sum = |a, b, c, d| product(two, add(product(a, b)?, product(c, d)?)?);
        let values = [
            diagonal(y, z)?,
            difference(x, y, z, w)?,
            sum(x, z, y, w)?,
            sum(x, y, z, w)?,
            diagonal(x, z)?,
            difference(y, z, x, w)?,
            difference(x, z, y, w)?,
            sum(y, z, x, w)?,
            diagonal(x, y)?,
        ];
        Ok(values.iter().all(|v| v.lower == v.upper))
    };
    verify().unwrap_or(false)
}
fn power_two(value: f64) -> bool {
    let bits = value.abs().to_bits();
    let exponent = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1u64 << 52) - 1);
    value.is_finite()
        && value != 0.
        && if exponent == 0 {
            fraction.count_ones() == 1
        } else {
            fraction == 0
        }
}
pub(super) fn signed_axis_frame(frame: Affine) -> bool {
    let mut columns = 0u8;
    let mut scale = None;
    for row in frame.rows {
        let mut axis = None;
        for (j, value) in row[..3].iter().enumerate() {
            if *value != 0. {
                if axis.is_some() || !power_two(*value) || scale.is_some_and(|s| s != value.abs()) {
                    return false;
                }
                axis = Some(j);
                scale = Some(value.abs());
            }
        }
        let Some(j) = axis else {
            return false;
        };
        if columns & (1 << j) != 0 || !row[3].is_finite() {
            return false;
        }
        columns |= 1 << j;
    }
    columns == 7
}
pub(super) fn inverse(frame: Affine) -> QueryResult<[[I; 3]; 3]> {
    if signed_axis_frame(frame) {
        let mut inverse = [[I::point(0.); 3]; 3];
        for i in 0..3 {
            for (j, target) in inverse.iter_mut().enumerate() {
                let value = frame.rows[i][j];
                if value != 0. {
                    let reciprocal = 1. / value;
                    let proof = product(I::point(value), I::point(reciprocal))?;
                    if !reciprocal.is_finite() || proof.lower != 1. || proof.upper != 1. {
                        return Err(uncertain());
                    }
                    target[i] = I::point(reciprocal);
                }
            }
        }
        return Ok(inverse);
    }
    enclosure::inverse(std::array::from_fn(|i| {
        std::array::from_fn(|j| frame.rows[i][j])
    }))
    .ok_or_else(uncertain)
}
pub(super) fn local(frame: Affine, input: V, point: bool) -> QueryResult<IV> {
    local_bounds(frame, points(input), point)
}
pub(super) fn local_bounds(frame: Affine, input: IV, point: bool) -> QueryResult<IV> {
    let inverse = inverse(frame)?;
    let mut values = input;
    if point {
        values = vector_sub(values, points(frame.rows.map(|row| row[3])))?;
    }
    let result = [
        dot(inverse[0], values)?,
        dot(inverse[1], values)?,
        dot(inverse[2], values)?,
    ];
    if result
        .iter()
        .any(|v| !v.lower.is_finite() || !v.upper.is_finite() || v.absolute_upper() > 2e50)
    {
        return Err(QueryError::Invalid(
            "finite source conversion exceeds numerical domain",
        ));
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Span {
    pub entry: I,
    pub exit: I,
    pub seed: f64,
}
impl Span {
    pub fn full(max: f64) -> Self {
        Self {
            entry: I::point(0.),
            exit: I::point(max),
            seed: 0.,
        }
    }
    pub fn clip(entry: I, exit: I, max: f64, seed: f64) -> Option<Self> {
        let entry = I {
            lower: entry.lower.max(0.),
            upper: entry.upper.max(0.),
        };
        let exit = I {
            lower: exit.lower.min(max),
            upper: exit.upper.min(max),
        };
        (entry.lower <= exit.upper).then_some(Self {
            entry,
            exit,
            seed: seed.clamp(0., max),
        })
    }
    pub fn candidates(self, max: f64) -> [f64; 5] {
        [
            self.entry.upper,
            0.5 * self.entry.upper + 0.5 * self.exit.lower,
            self.seed,
            self.exit.lower,
            0.,
        ]
        .map(|v| v.clamp(0., max))
    }
}
fn clip_linear(mut span: Span, constant: I, slope: I, max: f64) -> QueryResult<Option<Span>> {
    let over_range = add(
        constant,
        product(
            slope,
            I {
                lower: 0.,
                upper: max,
            },
        )?,
    )?;
    if over_range.upper < 0. {
        return Ok(None);
    }
    if over_range.lower >= 0. {
        return Ok(Some(span));
    }
    let boundary = divide(constant.negate(), slope)?;
    if slope.lower > 0. {
        span.entry.lower = span.entry.lower.max(boundary.lower);
        span.entry.upper = span.entry.upper.max(boundary.upper);
    } else if slope.upper < 0. {
        span.exit.lower = span.exit.lower.min(boundary.lower);
        span.exit.upper = span.exit.upper.min(boundary.upper);
    } else {
        return Err(uncertain());
    }
    Ok(Span::clip(span.entry, span.exit, max, span.seed))
}
fn roots(a: I, b: I, discriminant: I, max: f64) -> QueryResult<Option<Span>> {
    if a.lower <= 0. {
        return Err(uncertain());
    }
    if discriminant.upper < 0. {
        return Ok(None);
    }
    // A conditional root enclosure is usable only after a separate actual
    // original-line/source-core witness proves this span is nonempty.
    let root = sqrt(I {
        lower: discriminant.lower.max(0.),
        upper: discriminant.upper,
    })?;
    let entry = divide(subtract(b.negate(), root)?, a)?;
    let exit = divide(add(b.negate(), root)?, a)?;
    let seed = mid(divide(b.negate(), a)?);
    Ok(Span::clip(entry, exit, max, seed))
}
fn quadratic(mut a: I, mut b: I, mut c: I, max: f64) -> QueryResult<Option<Span>> {
    if a.lower == 0. && a.upper == 0. {
        return clip_linear(Span::full(max), c.negate(), product(I::point(-2.), b)?, max);
    }
    if a.lower <= 0. {
        return Err(uncertain());
    }
    let magnitude = a
        .absolute_upper()
        .max(b.absolute_upper())
        .max(c.absolute_upper());
    let exponent = (((magnitude.to_bits() >> 52) & 0x7ff) as i32 - 1023).clamp(-1022, 1022);
    let scale = f64::from_bits(((-exponent + 1023) as u64) << 52);
    a = product(a, I::point(scale))?;
    b = product(b, I::point(scale))?;
    c = product(c, I::point(scale))?;
    roots(a, b, subtract(square(b)?, product(a, c)?)?, max)
}
fn sphere_span(o: IV, d: IV, radius: f64, max: f64) -> QueryResult<Option<Span>> {
    // An original bounded affine coordinate attains its extrema at endpoints.
    // This exact-sided rejection handles solids just beyond a closed segment
    // without turning an overlapping rounded root enclosure into a clear claim.
    for i in 0..3 {
        let end = add(o[i], product(d[i], I::point(max))?)?;
        if (o[i].lower > radius && end.lower > radius)
            || (o[i].upper < -radius && end.upper < -radius)
        {
            return Ok(None);
        }
    }
    if d.iter().all(|v| v.lower == 0. && v.upper == 0.) {
        return sphere_contains(o, radius).map(|inside| inside.then(|| Span::full(max)));
    }
    let a = norm2(d)?;
    let b = dot(o, d)?;
    // Compensated original-direction cross products retain distant transverse
    // misses; subtracting two huge axial squared distances does not.
    let discriminant = subtract(product(square(I::point(radius))?, a)?, norm2(cross(o, d)?)?)?;
    roots(a, b, discriminant, max)
}
fn box_span(o: IV, d: IV, minimum: V, maximum: V, max: f64) -> QueryResult<Option<Span>> {
    if let (Some(o), Some(d)) = (point_values(o), point_values(d)) {
        return super::shape::finite_cuboid_span(o, d, minimum, maximum, max);
    }
    let mut span = Span::full(max);
    for i in 0..3 {
        let Some(next) = clip_linear(span, subtract(o[i], I::point(minimum[i]))?, d[i], max)?
        else {
            return Ok(None);
        };
        span = next;
        let Some(next) = clip_linear(
            span,
            subtract(I::point(maximum[i]), o[i])?,
            d[i].negate(),
            max,
        )?
        else {
            return Ok(None);
        };
        span = next;
    }
    Ok(Some(span))
}

fn union(spans: [Option<Span>; 3], max: f64) -> Option<Span> {
    let mut result: Option<Span> = None;
    for span in spans.into_iter().flatten() {
        result = Some(match result {
            None => span,
            Some(old) => Span {
                entry: I {
                    lower: old.entry.lower.min(span.entry.lower),
                    upper: old.entry.upper.min(span.entry.upper),
                },
                exit: I {
                    lower: old.exit.lower.max(span.exit.lower),
                    upper: old.exit.upper.max(span.exit.upper),
                },
                seed: old.seed,
            },
        });
    }
    result.and_then(|s| Span::clip(s.entry, s.exit, max, s.seed))
}
fn capsule_span(o: IV, d: IV, a: V, b: V, radius: f64, max: f64) -> QueryResult<Option<Span>> {
    let first = vector_sub(o, points(a))?;
    let second = vector_sub(o, points(b))?;
    if a == b {
        return sphere_span(first, d, radius, max);
    }
    let edge = vector_sub(points(b), points(a))?;
    let length = norm2(edge)?;
    if length.lower <= 0. {
        return Err(uncertain());
    }
    let along = dot(first, edge)?;
    let speed = dot(d, edge)?;
    let coefficient_a = subtract(product(norm2(d)?, length)?, square(speed)?)?;
    let coefficient_b = subtract(product(dot(first, d)?, length)?, product(along, speed)?)?;
    let coefficient_c = subtract(
        subtract(product(norm2(first)?, length)?, square(along)?)?,
        product(square(I::point(radius))?, length)?,
    )?;
    let cylinder = quadratic(coefficient_a, coefficient_b, coefficient_c, max)?;
    let cylinder = if let Some(span) = cylinder {
        if let Some(span) = clip_linear(span, along, speed, max)? {
            clip_linear(span, subtract(length, along)?, speed.negate(), max)?
        } else {
            None
        }
    } else {
        None
    };
    // The actual capsule is the union of both endpoint balls and the finite
    // cylinder. Its intersection with a line is convex. Every component is
    // solved before combining bounds; an uncertain component is never skipped.
    Ok(union(
        [
            sphere_span(first, d, radius, max)?,
            sphere_span(second, d, radius, max)?,
            cylinder,
        ],
        max,
    ))
}
fn projection_axis(normal: IV) -> QueryResult<(usize, usize, bool)> {
    for (axis, value) in normal.iter().enumerate() {
        if value.lower > 0. || value.upper < 0. {
            return Ok(((axis + 1) % 3, (axis + 2) % 3, value.lower > 0.));
        }
    }
    Err(uncertain())
}
fn triangle_span(o: IV, d: IV, vertices: [V; 3], max: f64) -> QueryResult<Option<Span>> {
    let [a, b, c] = vertices;
    let e1 = vector_sub(points(b), points(a))?;
    let e2 = vector_sub(points(c), points(a))?;
    let normal = cross(e1, e2)?;
    if normal.iter().all(|v| v.lower == 0. && v.upper == 0.) {
        return Ok(union(
            [
                capsule_span(o, d, a, b, 0., max)?,
                capsule_span(o, d, b, c, 0., max)?,
                capsule_span(o, d, c, a, 0., max)?,
            ],
            max,
        ));
    }
    let (i, j, positive) = projection_axis(normal)?;
    let relative = vector_sub(o, points(a))?;
    let plane = triple(relative, e1, e2)?;
    let plane_direction = triple(d, e1, e2)?;
    let mut span = if plane_direction.lower == 0. && plane_direction.upper == 0. {
        if plane.lower > 0. || plane.upper < 0. {
            return Ok(None);
        }
        if plane.lower != 0. || plane.upper != 0. {
            return Err(uncertain());
        }
        Span::full(max)
    } else {
        let at_end = add(plane, product(plane_direction, I::point(max))?)?;
        if (plane.lower > 0. && at_end.lower > 0.) || (plane.upper < 0. && at_end.upper < 0.) {
            return Ok(None);
        }
        let parameter = divide(plane.negate(), plane_direction)?;
        let Some(span) = Span::clip(parameter, parameter, max, mid(parameter)) else {
            return Ok(None);
        };
        span
    };
    for (x, y) in [(a, b), (b, c), (c, a)] {
        let edge = vector_sub(points(y), points(x))?;
        let relative = vector_sub(o, points(x))?;
        let constant = determinant(edge, relative, i, j)?;
        let slope = determinant(edge, d, i, j)?;
        let Some(next) = clip_linear(
            span,
            if positive {
                constant
            } else {
                constant.negate()
            },
            if positive { slope } else { slope.negate() },
            max,
        )?
        else {
            return Ok(None);
        };
        span = next;
    }
    Ok(Some(span))
}
pub(super) fn span(shape: &Shape, o: IV, d: IV, max: f64) -> QueryResult<Option<Span>> {
    match *shape {
        Shape::Sphere(radius) => sphere_span(o, d, radius, max),
        Shape::Box(extents) => box_span(o, d, extents.map(|v| -v), extents, max),
        Shape::ConvexCuboid {
            minimum, maximum, ..
        } => box_span(o, d, minimum, maximum, max),
        Shape::Capsule { a, b, radius } => capsule_span(o, d, a, b, radius, max),
        Shape::Triangle(vertices) => triangle_span(o, d, vertices, max),
    }
}
fn sign(value: I) -> QueryResult<bool> {
    if value.lower >= 0. {
        Ok(true)
    } else if value.upper < 0. {
        Ok(false)
    } else {
        Err(uncertain())
    }
}
fn sphere_contains(p: IV, radius: f64) -> QueryResult<bool> {
    if let Some(p) = point_values(p) {
        return Shape::Sphere(radius).overlap(p, 0.);
    }
    if radius == 0. && p.iter().any(|v| v.lower > 0. || v.upper < 0.) {
        return Ok(false);
    }
    sign(subtract(square(I::point(radius))?, norm2(p)?)?)
}
fn box_contains(p: IV, minimum: V, maximum: V) -> QueryResult<bool> {
    if (0..3).any(|i| p[i].lower > maximum[i] || p[i].upper < minimum[i]) {
        return Ok(false);
    }
    if (0..3).all(|i| p[i].lower >= minimum[i] && p[i].upper <= maximum[i]) {
        return Ok(true);
    }
    Err(uncertain())
}
fn capsule_contains(p: IV, a: V, b: V, radius: f64) -> QueryResult<bool> {
    if let Some(p) = point_values(p) {
        return Shape::Capsule { a, b, radius }.overlap(p, 0.);
    }
    let first = vector_sub(p, points(a))?;
    let second = vector_sub(p, points(b))?;
    if sphere_contains(first, radius).is_ok_and(|inside| inside)
        || sphere_contains(second, radius).is_ok_and(|inside| inside)
    {
        return Ok(true);
    }
    if a == b {
        return sphere_contains(first, radius);
    }
    let edge = vector_sub(points(b), points(a))?;
    let length = norm2(edge)?;
    let projection = dot(first, edge)?;
    if projection.upper <= 0. {
        return sphere_contains(first, radius);
    }
    if projection.lower >= length.upper {
        return sphere_contains(second, radius);
    }
    if length.lower <= 0. || projection.lower < 0. || subtract(length, projection)?.lower < 0. {
        return Err(uncertain());
    }
    let distance = subtract(product(norm2(first)?, length)?, square(projection)?)?;
    sign(subtract(
        product(square(I::point(radius))?, length)?,
        distance,
    )?)
}
fn triangle_contains(p: IV, vertices: [V; 3]) -> QueryResult<bool> {
    let [a, b, c] = vertices;
    let e1 = vector_sub(points(b), points(a))?;
    let e2 = vector_sub(points(c), points(a))?;
    let normal = cross(e1, e2)?;
    if normal.iter().all(|v| v.lower == 0. && v.upper == 0.) {
        let mut uncertain_member = false;
        for (a, b) in [(a, b), (b, c), (c, a)] {
            match capsule_contains(p, a, b, 0.) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(_) => uncertain_member = true,
            }
        }
        return if uncertain_member {
            Err(uncertain())
        } else {
            Ok(false)
        };
    }
    let plane = triple(vector_sub(p, points(a))?, e1, e2)?;
    if plane.lower > 0. || plane.upper < 0. {
        return Ok(false);
    }
    if plane.lower != 0. || plane.upper != 0. {
        return Err(uncertain());
    }
    let (i, j, positive) = projection_axis(normal)?;
    for (x, y) in [(a, b), (b, c), (c, a)] {
        let side = determinant(
            vector_sub(points(y), points(x))?,
            vector_sub(p, points(x))?,
            i,
            j,
        )?;
        if !sign(if positive { side } else { side.negate() })? {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(super) fn contains(shape: &Shape, p: IV) -> QueryResult<bool> {
    match *shape {
        Shape::Sphere(radius) => sphere_contains(p, radius),
        Shape::Box(extents) => box_contains(p, extents.map(|v| -v), extents),
        Shape::ConvexCuboid {
            minimum, maximum, ..
        } => box_contains(p, minimum, maximum),
        Shape::Capsule { a, b, radius } => capsule_contains(p, a, b, radius),
        Shape::Triangle(vertices) => triangle_contains(p, vertices),
    }
}
pub(super) fn solid_kind(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::Sphere(_) | Shape::Box(_) | Shape::ConvexCuboid { .. }
    )
}
pub(super) fn core(shape: &Shape) -> CoreGeometry {
    match shape {
        Shape::Sphere(radius) => CoreGeometry::Sphere {
            radius_binary32: (*radius as f32).to_bits(),
        },
        Shape::Box(extents) => CoreGeometry::Box {
            half_extents_binary32: extents.map(|v| (v as f32).to_bits()),
        },
        Shape::ConvexCuboid {
            minimum,
            maximum,
            _source_vertices,
            _source_planes,
        } => CoreGeometry::ConvexCuboid {
            minimum_binary64: minimum.map(f64::to_bits),
            maximum_binary64: maximum.map(f64::to_bits),
            vertices_binary32: _source_vertices.as_ref().map(|v| v.map(f32::to_bits)),
            planes_binary32: _source_planes.as_ref().map(|v| v.map(f32::to_bits)),
        },
        Shape::Capsule { a, b, radius } => CoreGeometry::Capsule {
            first_binary32: a.map(|v| (v as f32).to_bits()),
            second_binary32: b.map(|v| (v as f32).to_bits()),
            radius_binary32: (*radius as f32).to_bits(),
        },
        Shape::Triangle(vertices) => CoreGeometry::Triangle {
            vertices_binary32: vertices.map(|v| v.map(|x| (x as f32).to_bits())),
        },
    }
}
