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

fn sphere_ray(o: V, d: V, center: V, radius: f64) -> QueryResult<Option<f64>> {
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

fn cuboid_distance2(p: V, minimum: V, maximum: V) -> f64 {
    (0..3)
        .map(|i| (p[i] - p[i].clamp(minimum[i], maximum[i])).powi(2))
        .sum()
}

fn cuboid_ray(o: V, d: V, minimum: V, maximum: V) -> Option<f64> {
    let mut enter: f64 = 0.;
    let mut exit = f64::INFINITY;
    for i in 0..3 {
        if d[i] == 0. {
            if o[i] < minimum[i] || o[i] > maximum[i] {
                return None;
            }
        } else {
            let a = (minimum[i] - o[i]) / d[i];
            let b = (maximum[i] - o[i]) / d[i];
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
        }
    }
    (enter <= exit).then_some(enter)
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
    pub fn cost(&self) -> usize {
        if matches!(self, Self::Triangle(_)) {
            1
        } else {
            0
        }
    }
    pub fn overlap(&self, p: V, r: f64) -> bool {
        match *self {
            Self::Sphere(radius) => dot(p, p) <= (r + radius) * (r + radius),
            Self::Box(extents) => cuboid_distance2(p, extents.map(|v| -v), extents) <= r * r,
            Self::ConvexCuboid {
                minimum, maximum, ..
            } => cuboid_distance2(p, minimum, maximum) <= r * r,
            Self::Capsule { a, b, radius } => {
                segment_distance2(p, a, b) <= (r + radius) * (r + radius)
            }
            Self::Triangle(vertices) => triangle_distance2(p, vertices) <= r * r,
        }
    }
    pub fn ray(&self, o: V, d: V) -> QueryResult<Option<f64>> {
        if !query_domain(o) || !query_domain(d) || dot(d, d) == 0. || !dot(d, d).is_finite() {
            return Err(QueryError::Invalid("overflowing local ray"));
        }
        Ok(match *self {
            Self::Sphere(radius) => sphere_ray(o, d, [0.; 3], radius)?,
            Self::Box(extents) => cuboid_ray(o, d, extents.map(|v| -v), extents),
            Self::ConvexCuboid {
                minimum, maximum, ..
            } => cuboid_ray(o, d, minimum, maximum),
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
            Self::Triangle([a, b, c]) => {
                let e1 = sub(b, a);
                let e2 = sub(c, a);
                let h = cross(d, e2);
                let determinant = dot(e1, h);
                // Two-sided authored triangles. Parallel/coplanar rays do not
                // define a unique crossing; no arbitrary thickness is invented.
                if determinant == 0. {
                    return Ok(None);
                }
                let s = sub(o, a);
                let u = dot(s, h) / determinant;
                let q = cross(s, e1);
                let v = dot(d, q) / determinant;
                let t = dot(e2, q) / determinant;
                (u >= 0. && v >= 0. && u + v <= 1. && t >= 0.).then_some(t)
            }
        })
    }
}
