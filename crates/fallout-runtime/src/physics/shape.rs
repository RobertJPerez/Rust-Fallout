use super::{QueryError, QueryResult, math::*};

#[derive(Debug)]
pub(super) enum Shape {
    Sphere(f64),
    Box(V),
    Capsule { a: V, b: V, radius: f64 },
    Triangle([V; 3]),
}

fn sphere_ray(o: V, d: V, center: V, radius: f64) -> Option<f64> {
    let relative = sub(o, center);
    let c = dot(relative, relative) - radius * radius;
    if c <= 0. {
        return Some(0.);
    }
    let a = dot(d, d);
    let b = dot(relative, d);
    let disc = b * b - a * c;
    if disc < 0. {
        return None;
    }
    // Stable smaller quadratic root when a ray faces toward the sphere.
    let q = -b + disc.sqrt();
    let t = if q != 0. { c / q } else { -b / a };
    (t >= 0.).then_some(t)
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
            Self::Sphere(radius) => sphere_ray(o, d, [0.; 3], radius),
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
                if segment_distance2(o, a, b) <= radius * radius {
                    return Ok(Some(0.));
                }
                let edge = sub(b, a);
                let edge2 = dot(edge, edge);
                let mut nearest = [sphere_ray(o, d, a, radius), sphere_ray(o, d, b, radius)]
                    .into_iter()
                    .flatten()
                    .min_by(f64::total_cmp);
                if edge2 > 0. {
                    let relative = sub(o, a);
                    let axial_d = dot(d, edge) / edge2;
                    let axial_o = dot(relative, edge) / edge2;
                    let dp = sub(d, mul(edge, axial_d));
                    let op = sub(relative, mul(edge, axial_o));
                    let aa = dot(dp, dp);
                    let bb = dot(dp, op);
                    let cc = dot(op, op) - radius * radius;
                    let discriminant = bb * bb - aa * cc;
                    if aa > 0. && discriminant >= 0. {
                        for t in [
                            (-bb - discriminant.sqrt()) / aa,
                            (-bb + discriminant.sqrt()) / aa,
                        ] {
                            let axial = axial_o + t * axial_d;
                            if t >= 0.
                                && (0. ..=1.).contains(&axial)
                                && nearest.is_none_or(|old| t < old)
                            {
                                nearest = Some(t);
                            }
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
