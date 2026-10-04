use super::{QueryError, QueryResult};
use fallout_data::coordinates::Affine;

pub(super) type V = [f64; 3];
pub(super) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(super) fn mul(a: V, s: f64) -> V {
    a.map(|x| x * s)
}
pub(super) fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub(super) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(super) fn finite(v: V) -> bool {
    v.iter().all(|x| x.is_finite())
}
// Intersections multiply dot/cross products. Restrict the engineering numerical
// domain so finite input cannot silently overflow those higher-order expressions.
pub(super) fn query_domain(v: V) -> bool {
    finite(v) && v.iter().all(|x| x.abs() <= 1e50)
}
pub(super) fn identity() -> Affine {
    Affine {
        rows: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
    }
}
pub(super) fn compose(a: Affine, b: Affine) -> Affine {
    Affine {
        rows: std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (0..3).map(|k| a.rows[i][k] * b.rows[k][j]).sum::<f64>()
                    + if j == 3 { a.rows[i][3] } else { 0. }
            })
        }),
    }
}

/// Binary32-authored rotations can have rounding error. This check admits only
/// similarities within the declared engineering tolerance; it never repairs them.
#[derive(Debug)]
pub(super) struct Similarity {
    inverse: Affine,
    pub scale: f64,
}
impl Similarity {
    pub fn is_identity(&self) -> bool {
        self.inverse.rows == identity().rows
    }
    pub fn inverse_rows(&self) -> [[f64; 4]; 3] {
        self.inverse.rows
    }
    pub fn new(forward: Affine, tolerance: f64) -> QueryResult<Self> {
        if forward.rows.iter().flatten().any(|x| !x.is_finite()) {
            return Err(QueryError::Invalid("nonfinite transform"));
        }
        let cols: [V; 3] = std::array::from_fn(|j| std::array::from_fn(|i| forward.rows[i][j]));
        let scale2 = dot(cols[0], cols[0]);
        if scale2 <= 0. || !scale2.is_finite() {
            return Err(QueryError::Invalid("singular or overflowing transform"));
        }
        for i in 0..3 {
            if (dot(cols[i], cols[i]) / scale2 - 1.).abs() > tolerance {
                return Err(QueryError::Invalid("nonuniform scale"));
            }
            for j in 0..i {
                if (dot(cols[i], cols[j]) / scale2).abs() > tolerance {
                    return Err(QueryError::Invalid("sheared transform"));
                }
            }
        }
        // Compute the actual inverse, rather than transposing an approximately
        // orthogonal source rotation. Reflections keep their authored handedness.
        let determinant = dot(cols[0], cross(cols[1], cols[2]));
        if determinant == 0. || !determinant.is_finite() {
            return Err(QueryError::Invalid("singular transform"));
        }
        let rows = [
            cross(cols[1], cols[2]),
            cross(cols[2], cols[0]),
            cross(cols[0], cols[1]),
        ]
        .map(|v| mul(v, 1. / determinant));
        let translation = forward.rows.map(|r| r[3]);
        let inverse = Affine {
            rows: std::array::from_fn(|i| {
                [
                    rows[i][0],
                    rows[i][1],
                    rows[i][2],
                    -dot(rows[i], translation),
                ]
            }),
        };
        if inverse.rows.iter().flatten().any(|x| !x.is_finite()) {
            return Err(QueryError::Invalid("overflowing inverse transform"));
        }
        Ok(Self {
            inverse,
            scale: scale2.sqrt(),
        })
    }
    pub fn local_point(&self, p: V) -> V {
        self.inverse.point(p)
    }
    pub fn local_vector(&self, v: V) -> V {
        std::array::from_fn(|i| (0..3).map(|j| self.inverse.rows[i][j] * v[j]).sum())
    }
}
