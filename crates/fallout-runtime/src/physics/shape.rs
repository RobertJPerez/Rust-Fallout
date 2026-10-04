use super::{QueryError, QueryResult, math::*};

#[derive(Debug)]
pub(super) enum Shape {
    Sphere(f64),
    Box(V),
    Capsule { a: V, b: V, radius: f64 },
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
fn precise_cross(a: V, b: V) -> V {
    cross_with_error(a, b).0
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

fn segment_distance2(p: V, a: V, b: V) -> f64 {
    let edge = sub(b, a);
    let length2 = dot(edge, edge);
    let t = if length2 == 0. {
        0.
    } else {
        (dot(sub(p, a), edge) / length2).clamp(0., 1.)
    };
    let delta = sub(p, add(a, mul(edge, t)));
    dot(delta, delta)
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
            Self::Box(extents) => {
                (0..3)
                    .map(|i| (p[i].abs() - extents[i]).max(0.).powi(2))
                    .sum::<f64>()
                    <= r * r
            }
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
            Self::Box(extents) => {
                let mut enter: f64 = 0.;
                let mut exit = f64::INFINITY;
                for i in 0..3 {
                    if d[i] == 0. {
                        if o[i].abs() > extents[i] {
                            return Ok(None);
                        }
                    } else {
                        let a = (-extents[i] - o[i]) / d[i];
                        let b = (extents[i] - o[i]) / d[i];
                        enter = enter.max(a.min(b));
                        exit = exit.min(a.max(b));
                    }
                }
                (enter <= exit).then_some(enter)
            }
            Self::Capsule { a, b, radius } => {
                // Axis projection loses source offsets for long/thin geometry.
                if 128. * f64::EPSILON * (length(sub(o, a)) + length(sub(b, a))) > radius {
                    return Err(QueryError::Invalid(
                        "capsule projection exceeds numerical precision",
                    ));
                }
                if segment_distance2(o, a, b) <= radius * radius {
                    return Ok(Some(0.));
                }
                let edge = sub(b, a);
                let edge_length = length(edge);
                let mut nearest = [sphere_ray(o, d, a, radius)?, sphere_ray(o, d, b, radius)?]
                    .into_iter()
                    .flatten()
                    .min_by(f64::total_cmp);
                if edge_length > 0. {
                    let axis = edge.map(|v| v / edge_length);
                    let relative = sub(o, a);
                    let dp = precise_cross(axis, precise_cross(d, axis));
                    let op = precise_cross(axis, precise_cross(relative, axis));
                    if length(dp) > 0.
                        && let Some(t) = sphere_ray(op, dp, [0.; 3], radius)?
                    {
                        let axial = t.mul_add(dot(d, axis), dot(relative, axis));
                        if (0. ..=edge_length).contains(&axial) && nearest.is_none_or(|old| t < old)
                        {
                            nearest = Some(t);
                        }
                    }
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
