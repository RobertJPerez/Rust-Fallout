use super::{QueryError, QueryResult, math::*};

#[derive(Debug)]
pub(super) enum Shape {
    Sphere(f64),
    Box(V),
    ConvexCuboid {
        minimum: V,
        maximum: V,
        // Retain the authored certificate verbatim, including signed zero.
        _source_vertices: Box<[[f32; 4]; 8]>,
        _source_planes: Box<[[f32; 4]; 6]>,
    },
    Capsule {
        a: V,
        b: V,
        radius: f64,
    },
    Triangle([V; 3]),
}

fn length(v: V) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}

// FMA recovers the rounding error of the second product before subtraction.
// This avoids losing the perpendicular offset of a long, nearly axial ray.
fn round_bound(v: f64) -> f64 {
    f64::EPSILON * v.abs() + f64::from_bits(1)
}
fn difference_of_products(a: f64, b: f64, c: f64, d: f64) -> (f64, f64) {
    let product = c * d;
    let error = (-c).mul_add(d, product);
    let difference = a.mul_add(b, -product);
    let result = difference + error;
    (
        result,
        round_bound(difference) + round_bound(error) + round_bound(result),
    )
}
fn cross_with_error(a: V, b: V) -> (V, V) {
    let components = [
        difference_of_products(a[1], b[2], a[2], b[1]),
        difference_of_products(a[2], b[0], a[0], b[2]),
        difference_of_products(a[0], b[1], a[1], b[0]),
    ];
    (components.map(|v| v.0), components.map(|v| v.1))
}
fn difference_error(a: f64, b: f64, difference: f64) -> f64 {
    let virtual_b = a - difference;
    let virtual_a = difference + virtual_b;
    (a - virtual_a) + (virtual_b - b)
}
fn sum_error(a: f64, b: f64, sum: f64) -> f64 {
    let virtual_b = sum - a;
    let virtual_a = sum - virtual_b;
    (a - virtual_a) + (b - virtual_b)
}

fn sphere_overlap(p: V, query_radius: f64, source_radius: f64) -> QueryResult<bool> {
    use super::enclosure::Interval as I;
    let uncertain = || QueryError::Invalid("sphere overlap predicate is numerically uncertain");
    let radius = query_radius + source_radius;
    if p.iter().filter(|v| **v != 0.).count() <= 1 {
        // The norm is an exact absolute component, even for subnormals. When
        // it equals the rounded radius sum, TwoSum retains the true sum's sign
        // relative to that component. No squared distance erases a small gap.
        let distance = p.into_iter().map(f64::abs).fold(0., f64::max);
        return Ok(radius > distance
            || (radius == distance && sum_error(query_radius, source_radius, radius) >= 0.));
    }
    if radius == 0. {
        // Both original nonnegative radii are zero. A nonzero source point is
        // outside, regardless of whether its squared components underflow.
        return Ok(false);
    }
    let radius_bounds =
        predicate_sum(I::point(query_radius), I::point(source_radius)).ok_or_else(uncertain)?;
    if p.into_iter().any(|v| v.abs() > radius_bounds.upper) {
        return Ok(false);
    }
    let mut distance_squared = I::point(0.);
    for v in p {
        let square = predicate_product(I::point(v), I::point(v)).ok_or_else(uncertain)?;
        distance_squared = predicate_sum(distance_squared, square).ok_or_else(uncertain)?;
    }
    let radius_squared = predicate_product(radius_bounds, radius_bounds).ok_or_else(uncertain)?;
    let separation =
        predicate_sum(radius_squared, distance_squared.negate()).ok_or_else(uncertain)?;
    if separation.lower >= 0. {
        Ok(true)
    } else if separation.upper < 0. {
        Ok(false)
    } else {
        Err(uncertain())
    }
}

fn sphere_contains(p: V, radius: f64) -> QueryResult<bool> {
    let distance = length(p);
    // A one-component norm is exact, including source-axis surface points.
    if p.iter().filter(|v| **v != 0.).count() <= 1 {
        return Ok(distance <= radius);
    }
    let uncertainty = 8. * f64::EPSILON * distance + f64::from_bits(1);
    if (distance - radius).abs() <= uncertainty {
        return Err(QueryError::Invalid(
            "sphere containment predicate is numerically uncertain",
        ));
    }
    Ok(distance < radius)
}

pub(super) fn sphere_ray(o: V, d: V, center: V, radius: f64) -> QueryResult<Option<f64>> {
    let relative = sub(o, center);
    if sphere_contains(relative, radius)? {
        return Ok(Some(0.));
    }
    let speed = length(d);
    if speed == 0. || !speed.is_finite() {
        return Err(QueryError::Invalid("unrepresentable local ray speed"));
    }
    // Subtracting squared axial distances in the quadratic discriminant can
    // erase a finite transverse miss. Measure that distance directly instead.
    // Keep the ORIGINAL direction: normalizing each component changes a skew
    // distant ray's orientation. Divide its compensated cross norm by speed.
    let axial = d.iter().filter(|v| **v != 0.).count() == 1;
    let (perpendicular, uncertainty, along) = if axial {
        let index = d
            .iter()
            .position(|v| *v != 0.)
            .expect("checked nonzero speed");
        let mut transverse = relative;
        transverse[index] = 0.;
        let distance = length(transverse);
        let uncertainty = if transverse.iter().filter(|v| **v != 0.).count() <= 1 {
            0.
        } else {
            8. * f64::EPSILON * distance
        };
        (distance, uncertainty, -relative[index] * d[index].signum())
    } else {
        let (cross, error) = cross_with_error(relative, d);
        let distance = length(cross) / speed;
        (
            distance,
            length(error) / speed + 8. * f64::EPSILON * distance,
            -dot(relative, d) / speed,
        )
    };
    if along <= 0. || perpendicular - radius > uncertainty {
        return Ok(None);
    }
    if uncertainty > 0. && (perpendicular - radius).abs() <= uncertainty {
        return Err(QueryError::Invalid(
            "sphere grazing predicate is numerically uncertain",
        ));
    }
    let chord = ((radius - perpendicular) * (radius + perpendicular)).sqrt();
    let entry = along - chord;
    // A distant true intersection can still have an unrepresentable surface
    // entry. Refuse it instead of reporting a rounded point inside the solid.
    if chord > 0. && (entry == along || (!axial && 64. * f64::EPSILON * length(relative) >= chord))
    {
        return Err(QueryError::Invalid(
            "ray surface entry exceeds numerical precision",
        ));
    }
    let t = entry / speed;
    if !t.is_finite() || entry < 0. {
        return Err(QueryError::Invalid("unrepresentable ray surface entry"));
    }
    Ok(Some(t))
}

fn segment_delta(p: V, a: V, b: V) -> V {
    let edge = sub(b, a);
    let relative = sub(p, a);
    let length2 = dot(edge, edge);
    let t = if length2 == 0. {
        0.
    } else {
        (dot(relative, edge) / length2).clamp(0., 1.)
    };
    // Stay relative to the source endpoint, avoiding a large anchor addition
    // followed by cancellation against p.
    sub(relative, mul(edge, t))
}
fn segment_distance2(p: V, a: V, b: V) -> f64 {
    let delta = segment_delta(p, a, b);
    dot(delta, delta)
}

fn predicate_dot(a: V, b: V) -> Option<super::enclosure::Interval> {
    use super::enclosure::Interval as I;
    let mut sum = I::point(0.);
    for i in 0..3 {
        sum = predicate_sum(sum, predicate_product(I::point(a[i]), I::point(b[i]))?)?;
    }
    Some(sum)
}

fn capsule_overlap(p: V, a: V, b: V, query_radius: f64, source_radius: f64) -> QueryResult<bool> {
    use super::enclosure::Interval as I;
    let uncertain = || QueryError::Invalid("capsule overlap predicate is numerically uncertain");
    let relative = |anchor: V| -> QueryResult<V> {
        let delta = sub(p, anchor);
        if (0..3).any(|i| difference_error(p[i], anchor[i], delta[i]) != 0.) {
            return Err(uncertain());
        }
        Ok(delta)
    };
    let endpoint = |anchor: V| {
        sphere_overlap(relative(anchor)?, query_radius, source_radius).map_err(|_| uncertain())
    };
    let edge = sub(b, a);
    if edge.iter().filter(|v| **v != 0.).count() <= 1 {
        // Clamp in the original authored axis before subtraction. A rounded
        // projection and reconstructed closest point cannot change this line.
        let mut closest = a;
        if let Some(axis) = edge.iter().position(|v| *v != 0.) {
            closest[axis] = p[axis].clamp(a[axis].min(b[axis]), a[axis].max(b[axis]));
        }
        return endpoint(closest);
    }
    if (0..3).any(|i| difference_error(b[i], a[i], edge[i]) != 0.) {
        return Err(uncertain());
    }
    let w = relative(a)?;
    let length_squared = predicate_dot(edge, edge).ok_or_else(uncertain)?;
    let projection = predicate_dot(w, edge).ok_or_else(uncertain)?;
    if projection.upper <= 0. {
        return endpoint(a);
    }
    if projection.lower >= length_squared.upper {
        return endpoint(b);
    }
    let remainder = predicate_sum(length_squared, projection.negate()).ok_or_else(uncertain)?;
    if length_squared.lower <= 0. || projection.lower < 0. || remainder.lower < 0. {
        return Err(uncertain());
    }
    // For an interior projection, distance^2 * |edge|^2 equals
    // |w|^2 * |edge|^2 - (w.edge)^2. Compare without dividing, normalizing
    // the axis, or rounding an interpolated closest point into the capsule.
    let norm_squared = predicate_dot(w, w).ok_or_else(uncertain)?;
    let norm_product = predicate_product(norm_squared, length_squared).ok_or_else(uncertain)?;
    let projection_squared = predicate_product(projection, projection).ok_or_else(uncertain)?;
    let distance_numerator =
        predicate_sum(norm_product, projection_squared.negate()).ok_or_else(uncertain)?;
    let radius =
        predicate_sum(I::point(query_radius), I::point(source_radius)).ok_or_else(uncertain)?;
    let radius_squared = predicate_product(radius, radius).ok_or_else(uncertain)?;
    let radius_numerator =
        predicate_product(radius_squared, length_squared).ok_or_else(uncertain)?;
    let separation =
        predicate_sum(radius_numerator, distance_numerator.negate()).ok_or_else(uncertain)?;
    if separation.lower >= 0. {
        Ok(true)
    } else if separation.upper < 0. {
        Ok(false)
    } else {
        Err(uncertain())
    }
}

fn capsule_contains(p: V, a: V, b: V, radius: f64) -> QueryResult<bool> {
    let edge = sub(b, a);
    let relative = sub(p, a);
    if edge.iter().filter(|v| **v != 0.).count() <= 1 {
        // Exact axis-clamping needs no normalized segment projection.
        let mut delta = relative;
        let mut error = std::array::from_fn(|i| difference_error(p[i], a[i], delta[i]));
        if let Some(i) = edge.iter().position(|v| *v != 0.) {
            let closest = p[i].clamp(a[i].min(b[i]), a[i].max(b[i]));
            delta[i] = p[i] - closest;
            error[i] = difference_error(p[i], closest, delta[i]);
        }
        if error.iter().any(|v| *v != 0.) {
            let distance = length(delta);
            let uncertainty = length(error) + 8. * f64::EPSILON * distance + f64::from_bits(1);
            if (distance - radius).abs() <= uncertainty {
                return Err(QueryError::Invalid(
                    "capsule containment predicate is numerically uncertain",
                ));
            }
        }
        return sphere_contains(delta, radius).map_err(|_| {
            QueryError::Invalid("capsule containment predicate is numerically uncertain")
        });
    }
    let distance = length(segment_delta(p, a, b));
    let uncertainty = 128. * f64::EPSILON * (length(relative) + length(edge))
        + 8. * f64::EPSILON * distance
        + f64::from_bits(1);
    if (distance - radius).abs() <= uncertainty {
        return Err(QueryError::Invalid(
            "capsule containment predicate is numerically uncertain",
        ));
    }
    Ok(distance < radius)
}

fn exact_products_equal(a: f64, b: f64, c: f64, d: f64) -> bool {
    if a == 0. || b == 0. {
        return c == 0. || d == 0.;
    }
    if c == 0. || d == 0. {
        return false;
    }
    let left = a * b;
    let right = c * d;
    // Above this conservative exponent floor, both parts of a binary64
    // product are representable and FMA recovers the exact low part.
    left.abs() >= 1e-270 && left == right && a.mul_add(b, -left) == c.mul_add(d, -right)
}
fn exactly_parallel(a: V, b: V) -> bool {
    [(1, 2), (2, 0), (0, 1)]
        .iter()
        .all(|&(i, j)| exact_products_equal(a[i], b[j], a[j], b[i]))
}

// A rounded zero determinant alone cannot distinguish a parallel ray from a
// nearly parallel crossing. Preserve source edges only when their subtraction
// is exact, then accept the simple exact parallel/degenerate certificates.
fn triangle_parallel(e1: V, e2: V, direction: V) -> bool {
    if exactly_parallel(e1, e2)
        || exactly_parallel(direction, e1)
        || exactly_parallel(direction, e2)
    {
        return true;
    }
    if direction.iter().filter(|v| **v != 0.).count() == 1 {
        let axis = direction
            .iter()
            .position(|v| *v != 0.)
            .expect("one ray axis");
        let i = (axis + 1) % 3;
        let j = (axis + 2) % 3;
        return exact_products_equal(e1[i], e2[j], e1[j], e2[i]);
    }
    false
}
fn predicate_product(
    a: super::enclosure::Interval,
    b: super::enclosure::Interval,
) -> Option<super::enclosure::Interval> {
    // An exact normal-range product can remain a point, preserving ordinary
    // closed edge/vertex contacts. Underflowed FMA residuals prove nothing.
    if a.lower == a.upper && b.lower == b.upper {
        let product = a.lower * b.lower;
        if product.is_finite()
            && product.abs() >= 1e-270
            && a.lower.mul_add(b.lower, -product) == 0.
        {
            return Some(super::enclosure::Interval::point(product));
        }
    }
    a.multiply(b)
}
fn predicate_sum(
    a: super::enclosure::Interval,
    b: super::enclosure::Interval,
) -> Option<super::enclosure::Interval> {
    if a.lower == a.upper && b.lower == b.upper {
        let sum = a.lower + b.lower;
        let residual = sum_error(a.lower, b.lower, sum);
        if sum.is_finite() && residual == 0. {
            return Some(super::enclosure::Interval::point(sum));
        }
    }
    a.add(b)
}
fn triangle_triple_bounds(e1: V, direction: V, e2: V) -> QueryResult<super::enclosure::Interval> {
    use super::enclosure::Interval as I;
    let uncertain = || QueryError::Invalid("triangle ray predicate is numerically uncertain");
    let mut determinant = I::point(0.);
    for (i, j, k) in [(0, 1, 2), (1, 2, 0), (2, 0, 1)] {
        let first =
            predicate_product(I::point(direction[j]), I::point(e2[k])).ok_or_else(uncertain)?;
        let second =
            predicate_product(I::point(direction[k]), I::point(e2[j])).ok_or_else(uncertain)?;
        let component = predicate_sum(first, second.negate()).ok_or_else(uncertain)?;
        let term = predicate_product(I::point(e1[i]), component).ok_or_else(uncertain)?;
        determinant = predicate_sum(determinant, term).ok_or_else(uncertain)?;
    }
    Ok(determinant)
}

fn triangle_ray(o: V, d: V, [a, b, c]: [V; 3], max: f64) -> QueryResult<Option<f64>> {
    use super::enclosure::Interval as I;
    let uncertain = || QueryError::Invalid("triangle ray predicate is numerically uncertain");
    let e1 = sub(b, a);
    let e2 = sub(c, a);
    if (0..3).any(|i| {
        difference_error(b[i], a[i], e1[i]) != 0. || difference_error(c[i], a[i], e2[i]) != 0.
    }) {
        return Err(QueryError::Invalid(
            "triangle source edge subtraction is numerically uncertain",
        ));
    }
    // Only a proven parallel/degenerate triangle has no unique crossing.
    if triangle_parallel(e1, e2, d) {
        return Ok(None);
    }
    let determinant_bounds = triangle_triple_bounds(e1, d, e2)?;
    if determinant_bounds.lower <= 0. && determinant_bounds.upper >= 0. {
        return Err(uncertain());
    }
    let h = cross(d, e2);
    let determinant = dot(e1, h);
    // The scalar evaluation can round to zero even when its mathematical
    // enclosure excludes zero. Division must use a finite, consistent sign.
    if !determinant.is_finite()
        || determinant == 0.
        || (determinant > 0.) != (determinant_bounds.lower > 0.)
    {
        return Err(uncertain());
    }
    let s = sub(o, a);
    if (0..3).any(|i| difference_error(o[i], a[i], s[i]) != 0.) {
        return Err(QueryError::Invalid(
            "triangle ray origin subtraction is numerically uncertain",
        ));
    }
    let positive = |bounds: I| {
        if determinant > 0. {
            bounds
        } else {
            bounds.negate()
        }
    };
    let den = positive(determinant_bounds);
    let u_bounds = positive(triangle_triple_bounds(s, d, e2)?);
    let v_bounds = positive(triangle_triple_bounds(d, s, e1)?);
    let t_bounds = positive(triangle_triple_bounds(e2, s, e1)?);
    let w_bounds = predicate_sum(
        predicate_sum(den, u_bounds.negate()).ok_or_else(uncertain)?,
        v_bounds.negate(),
    )
    .ok_or_else(uncertain)?;
    let range_bounds = predicate_sum(
        predicate_product(den, I::point(max)).ok_or_else(uncertain)?,
        t_bounds.negate(),
    )
    .ok_or_else(uncertain)?;
    let predicates = [u_bounds, v_bounds, w_bounds, t_bounds, range_bounds];
    if predicates.iter().any(|bounds| bounds.upper < 0.) {
        return Ok(None);
    }
    if predicates.iter().any(|bounds| bounds.lower < 0.) {
        return Err(uncertain());
    }
    let u = dot(s, h) / determinant;
    let q = cross(s, e1);
    let v = dot(d, q) / determinant;
    let t = dot(e2, q) / determinant;
    // Keep the original scalar entry, but never let disagreement with the
    // admitted closed comparisons silently become an empty result.
    if !u.is_finite()
        || !v.is_finite()
        || !t.is_finite()
        || u < 0.
        || v < 0.
        || u + v > 1.
        || t < 0.
        || t > max
    {
        return Err(uncertain());
    }
    Ok(Some(t))
}

fn capsule_side_ray(o: V, d: V, a: V, b: V, radius: f64) -> QueryResult<Option<f64>> {
    let edge = sub(b, a);
    if (0..3).all(|i| difference_error(b[i], a[i], edge[i]) == 0.) && exactly_parallel(d, edge) {
        return Ok(None);
    }
    let relative = sub(o, a);
    let axis = if edge.iter().filter(|v| **v != 0.).count() == 1 {
        edge.iter().position(|v| *v != 0.)
    } else {
        None
    };
    let (op, dp, projected_radius, uncertainty) = if let Some(axis) = axis {
        // Deleting the source-axis component is exact and preserves ordinary
        // axial/side/cap cases without normalizing any direction or shape axis.
        let mut op = relative;
        op[axis] = 0.;
        let mut dp = d;
        dp[axis] = 0.;
        let errors: V = std::array::from_fn(|i| {
            if i == axis {
                0.
            } else {
                difference_error(o[i], a[i], relative[i])
            }
        });
        (op, dp, radius, length(errors))
    } else {
        // |(relative+t*d) x ORIGINAL edge| <= radius*|edge| is the cylinder.
        // A normalized double-cross changes a distant skew axis before the
        // sphere solver can observe its error. Keep the source axis unchanged.
        let op = cross_with_error(relative, edge).0;
        let dp = cross_with_error(d, edge).0;
        let edge_length = length(edge);
        let speed = length(dp);
        if speed <= 128. * f64::EPSILON * edge_length * length(d) {
            return Err(QueryError::Invalid(
                "capsule side direction is numerically uncertain",
            ));
        }
        // Position/projection errors have source scale EPS*(|relative|+|edge|).
        // Cross scaling contributes |edge|; direction sensitivity contributes
        // 1/sin(angle). Refuse inconclusive predicates, never inflate geometry.
        let condition = edge_length * length(d) / speed;
        let projected_radius = radius * edge_length;
        let uncertainty =
            128. * f64::EPSILON * (length(relative) + edge_length) * edge_length * (1. + condition)
                + 16. * f64::EPSILON * projected_radius
                + f64::from_bits(1);
        if !uncertainty.is_finite() || uncertainty >= projected_radius {
            return Err(QueryError::Invalid(
                "capsule side projection exceeds numerical precision",
            ));
        }
        (op, dp, projected_radius, uncertainty)
    };
    let speed = length(dp);
    if speed == 0. {
        return Ok(None);
    }
    if uncertainty > 0. {
        let perpendicular = length(cross_with_error(op, dp).0) / speed;
        if perpendicular - projected_radius > uncertainty {
            return Ok(None);
        }
        if (perpendicular - projected_radius).abs() <= uncertainty
            || (length(op) - projected_radius).abs() <= uncertainty
        {
            return Err(QueryError::Invalid(
                "capsule side predicate is numerically uncertain",
            ));
        }
    }
    sphere_ray(op, dp, [0.; 3], projected_radius)
}

fn capsule_segment_accepts(o: V, d: V, a: V, b: V, t: f64) -> QueryResult<bool> {
    let edge = sub(b, a);
    if edge.iter().filter(|v| **v != 0.).count() == 1 {
        let i = edge.iter().position(|v| *v != 0.).expect("one source axis");
        if d[i] == 0. {
            return Ok((a[i].min(b[i])..=a[i].max(b[i])).contains(&o[i]));
        }
    }
    let relative = sub(o, a);
    let axial = t.mul_add(dot(d, edge), dot(relative, edge));
    let end = dot(edge, edge);
    let uncertainty = 128.
        * f64::EPSILON
        * (length(relative) + t.abs() * length(d) + length(edge))
        * length(edge);
    if axial < -uncertainty || axial > end + uncertainty {
        return Ok(false);
    }
    if axial.abs() <= uncertainty || (axial - end).abs() <= uncertainty {
        return Err(QueryError::Invalid(
            "capsule segment endpoint predicate is numerically uncertain",
        ));
    }
    Ok((0. ..=end).contains(&axial))
}

fn cuboid_contains(p: V, minimum: V, maximum: V) -> bool {
    (0..3).all(|i| minimum[i] <= p[i] && p[i] <= maximum[i])
}

fn cuboid_overlap(p: V, radius: f64, minimum: V, maximum: V) -> QueryResult<bool> {
    // Squaring a minimum subnormal outside offset would erase it entirely.
    if radius == 0. || cuboid_contains(p, minimum, maximum) {
        return Ok(cuboid_contains(p, minimum, maximum));
    }
    let closest: V = std::array::from_fn(|i| p[i].clamp(minimum[i], maximum[i]));
    let delta = sub(p, closest);
    let error: V = std::array::from_fn(|i| difference_error(p[i], closest[i], delta[i]));
    let distance = length(delta);
    if error.iter().all(|v| *v == 0.) && delta.iter().filter(|v| **v != 0.).count() <= 1 {
        return Ok(distance <= radius);
    }
    let uncertainty = length(error) + 8. * f64::EPSILON * distance + f64::from_bits(1);
    if (distance - radius).abs() <= uncertainty {
        return Err(QueryError::Invalid(
            "cuboid overlap predicate is numerically uncertain",
        ));
    }
    Ok(distance < radius)
}

#[derive(Clone, Copy)]
struct Interval {
    lower: f64,
    upper: f64,
}

fn quotient_interval(numerator: f64, denominator: f64) -> QueryResult<Interval> {
    let quotient = numerator / denominator;
    if !quotient.is_finite() {
        return Err(QueryError::Invalid("unrepresentable cuboid slab quotient"));
    }
    // Exact products retain ordinary closed face contacts. Otherwise the
    // adjacent representable numbers enclose division, including underflow.
    let exact = numerator == 0.
        || denominator.abs() == 1.
        || exact_products_equal(quotient, denominator, numerator, 1.);
    Ok(Interval {
        lower: if exact {
            quotient
        } else {
            quotient.next_down()
        },
        upper: if exact { quotient } else { quotient.next_up() },
    })
}

fn slab_bound(bound: f64, origin: f64, direction: f64) -> QueryResult<Interval> {
    let difference = bound - origin;
    let error = difference_error(bound, origin, difference);
    if !difference.is_finite() || !error.is_finite() {
        return Err(QueryError::Invalid(
            "unrepresentable cuboid slab subtraction",
        ));
    }
    let lower = if error < 0. {
        difference.next_down()
    } else {
        difference
    };
    let upper = if error > 0. {
        difference.next_up()
    } else {
        difference
    };
    let (near, far) = if direction > 0. {
        (lower, upper)
    } else {
        (upper, lower)
    };
    Ok(Interval {
        lower: quotient_interval(near, direction)?.lower,
        upper: quotient_interval(far, direction)?.upper,
    })
}

fn cuboid_ray(o: V, d: V, minimum: V, maximum: V, max: f64) -> QueryResult<Option<f64>> {
    if cuboid_contains(o, minimum, maximum) {
        return Ok(Some(0.));
    }
    let Some((enter, exit)) = cuboid_interval(o, d, minimum, maximum, max)? else {
        return Ok(None);
    };
    if enter.upper > exit.lower {
        return Err(QueryError::Invalid(
            "cuboid slab predicate is numerically uncertain",
        ));
    }
    // A possible intersection is insufficient. The upper entry bound lies
    // inside every guaranteed slab and the caller's exact distance interval.
    let witness: V = std::array::from_fn(|i| d[i].mul_add(enter.upper, o[i]));
    if !cuboid_contains(witness, minimum, maximum) {
        return Err(QueryError::Invalid(
            "cuboid entry witness is numerically uncertain",
        ));
    }
    Ok(Some(enter.upper))
}

fn cuboid_interval(
    o: V,
    d: V,
    minimum: V,
    maximum: V,
    max: f64,
) -> QueryResult<Option<(Interval, Interval)>> {
    let mut enter = Interval {
        lower: 0.,
        upper: 0.,
    };
    let mut exit = Interval {
        lower: max,
        upper: max,
    };
    for i in 0..3 {
        if d[i] == 0. {
            if o[i] < minimum[i] || o[i] > maximum[i] {
                return Ok(None);
            }
        } else {
            if (o[i] < minimum[i] && d[i] < 0.) || (o[i] > maximum[i] && d[i] > 0.) {
                return Ok(None);
            }
            let (near, far) = if d[i] > 0. {
                (minimum[i], maximum[i])
            } else {
                (maximum[i], minimum[i])
            };
            let a = slab_bound(near, o[i], d[i])?;
            let b = slab_bound(far, o[i], d[i])?;
            enter.lower = enter.lower.max(a.lower);
            enter.upper = enter.upper.max(a.upper);
            exit.lower = exit.lower.min(b.lower);
            exit.upper = exit.upper.min(b.upper);
        }
    }
    if enter.lower > exit.upper {
        return Ok(None);
    }
    Ok(Some((enter, exit)))
}

/// Culling is permitted only for a certified miss. Uncertain or overflowing
/// enclosures keep the source primitive for its actual geometry predicate.
pub(super) fn cuboid_may_ray(o: V, d: V, minimum: V, maximum: V, max: f64) -> bool {
    !matches!(cuboid_interval(o, d, minimum, maximum, max), Ok(None))
}

/// Closest point lies either on a triangle edge or inside its perpendicular
/// projection. Degenerate triangles remain lines/points for distance queries.
fn triangle_distance2(p: V, [a, b, c]: [V; 3]) -> f64 {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let n = cross(ab, ac);
    let n2 = dot(n, n);
    let edges = [
        segment_distance2(p, a, b),
        segment_distance2(p, b, c),
        segment_distance2(p, c, a),
    ];
    if n2 > 0. {
        let projection = sub(p, mul(n, dot(sub(p, a), n) / n2));
        if [(a, b), (b, c), (c, a)]
            .iter()
            .all(|&(x, y)| dot(cross(sub(y, x), sub(projection, x)), n) >= 0.)
        {
            let distance = dot(sub(p, a), n);
            return distance * distance / n2;
        }
    }
    edges.into_iter().fold(f64::INFINITY, f64::min)
}

impl Shape {
    /// Bounds of authored core geometry, used only as culling supersets.
    pub fn bounds(&self) -> (V, V) {
        match *self {
            Self::Sphere(r) => ([-r; 3], [r; 3]),
            Self::Box(extents) => (extents.map(|v| -v), extents),
            Self::ConvexCuboid {
                minimum, maximum, ..
            } => (minimum, maximum),
            Self::Capsule { a, b, radius } => (
                std::array::from_fn(|i| (a[i].min(b[i]) - radius).next_down()),
                std::array::from_fn(|i| (a[i].max(b[i]) + radius).next_up()),
            ),
            Self::Triangle(vertices) => (
                std::array::from_fn(|i| {
                    vertices.iter().map(|v| v[i]).fold(f64::INFINITY, f64::min)
                }),
                std::array::from_fn(|i| {
                    vertices
                        .iter()
                        .map(|v| v[i])
                        .fold(f64::NEG_INFINITY, f64::max)
                }),
            ),
        }
    }
    pub fn cost(&self) -> usize {
        if matches!(self, Self::Triangle(_)) {
            1
        } else {
            0
        }
    }
    pub fn overlap(&self, p: V, r: f64) -> QueryResult<bool> {
        Ok(match *self {
            Self::Sphere(radius) => sphere_overlap(p, r, radius)?,
            Self::Box(extents) => cuboid_overlap(p, r, extents.map(|v| -v), extents)?,
            Self::ConvexCuboid {
                minimum, maximum, ..
            } => cuboid_overlap(p, r, minimum, maximum)?,
            Self::Capsule { a, b, radius } => capsule_overlap(p, a, b, r, radius)?,
            Self::Triangle(vertices) => triangle_distance2(p, vertices) <= r * r,
        })
    }
    pub fn ray(&self, o: V, d: V, max: f64) -> QueryResult<Option<f64>> {
        if !query_domain(o) || !query_domain(d) || dot(d, d) == 0. || !dot(d, d).is_finite() {
            return Err(QueryError::Invalid("overflowing local ray"));
        }
        Ok(match *self {
            Self::Sphere(radius) => sphere_ray(o, d, [0.; 3], radius)?,
            Self::Box(extents) => cuboid_ray(o, d, extents.map(|v| -v), extents, max)?,
            Self::ConvexCuboid {
                minimum, maximum, ..
            } => cuboid_ray(o, d, minimum, maximum, max)?,
            Self::Capsule { a, b, radius } => {
                // Axis projection loses source offsets for long/thin geometry.
                if 128. * f64::EPSILON * (length(sub(o, a)) + length(sub(b, a))) > radius {
                    return Err(QueryError::Invalid(
                        "capsule projection exceeds numerical precision",
                    ));
                }
                if capsule_contains(o, a, b, radius)? {
                    return Ok(Some(0.));
                }
                let edge = sub(b, a);
                let edge_length = length(edge);
                let mut nearest = [sphere_ray(o, d, a, radius)?, sphere_ray(o, d, b, radius)?]
                    .into_iter()
                    .flatten()
                    .min_by(f64::total_cmp);
                if edge_length > 0.
                    && let Some(t) = capsule_side_ray(o, d, a, b, radius)?
                    && capsule_segment_accepts(o, d, a, b, t)?
                    && nearest.is_none_or(|old| t < old)
                {
                    nearest = Some(t);
                }
                nearest
            }
            Self::Triangle(vertices) => return triangle_ray(o, d, vertices, max),
        })
    }
}
