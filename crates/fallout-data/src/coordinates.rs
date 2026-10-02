//! Source reference transforms used by the NV inspection adapter. Angles are in
//! radians. Retail camera/placement measurements are still an acceptance gate.
use crate::{Error, Result, world::Transform};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Affine {
    pub rows: [[f64; 4]; 3],
}

fn product(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

impl Affine {
    /// Static reference convention: clockwise X, then Y, then Z. This is kept
    /// separate from actor/controller rotations, which need their own rules.
    pub fn nv_reference(transform: &Transform, scale: f32) -> Result<Self> {
        if transform
            .position
            .iter()
            .chain(&transform.rotation)
            .any(|v| !v.is_finite())
            || !scale.is_finite()
            || scale <= 0.
        {
            return Err(Error::Resolution("invalid NV reference transform".into()));
        }
        let [(sx, cx), (sy, cy), (sz, cz)] = transform.rotation.map(|v| (-f64::from(v)).sin_cos());
        let x = [[1., 0., 0.], [0., cx, -sx], [0., sx, cx]];
        let y = [[cy, 0., sy], [0., 1., 0.], [-sy, 0., cy]];
        let z = [[cz, -sz, 0.], [sz, cz, 0.], [0., 0., 1.]];
        let rotation = product(z, product(y, x));
        Ok(Self {
            rows: std::array::from_fn(|i| {
                [
                    rotation[i][0] * f64::from(scale),
                    rotation[i][1] * f64::from(scale),
                    rotation[i][2] * f64::from(scale),
                    f64::from(transform.position[i]),
                ]
            }),
        })
    }

    pub fn point(&self, value: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            self.rows[i][3] + (0..3).map(|j| self.rows[i][j] * value[j]).sum::<f64>()
        })
    }

    /// Convert both sides of the linear map, and subtract the origin in f64
    /// before the renderer narrows its relative coordinates to f32.
    pub fn relative_view(&self, origin: [f64; 3]) -> Self {
        let order = [0, 2, 1];
        let sign = [1., 1., -1.];
        Self {
            rows: std::array::from_fn(|i| {
                std::array::from_fn(|j| {
                    if j == 3 {
                        sign[i] * (self.rows[order[i]][3] - origin[order[i]])
                    } else {
                        sign[i] * sign[j] * self.rows[order[i]][order[j]]
                    }
                })
            }),
        }
    }
}

pub fn source_to_view(value: [f64; 3], origin: [f64; 3]) -> [f64; 3] {
    [
        value[0] - origin[0],
        value[2] - origin[2],
        origin[1] - value[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: [f64; 3], b: [f64; 3]) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-6, "{a:?} != {b:?}");
        }
    }
    #[test]
    fn quarter_turns_scale_and_translation_match_analytic_basis_vectors() {
        let p = std::f32::consts::FRAC_PI_2;
        for (angles, point, expected) in [
            ([0., 0., p], [1., 0., 0.], [0., -1., 0.]),
            ([p, 0., 0.], [0., 1., 0.], [0., 0., -1.]),
            ([0., p, 0.], [0., 0., 1.], [-1., 0., 0.]),
            ([p, p, p], [0., 1., 0.], [0., -1., 0.]),
        ] {
            let affine = Affine::nv_reference(
                &Transform {
                    position: [10., 20., 30.],
                    rotation: angles,
                },
                2.,
            )
            .unwrap();
            close(
                affine.point(point),
                std::array::from_fn(|i| expected[i] * 2. + [10., 20., 30.][i]),
            );
        }
    }
    #[test]
    fn rebasing_preserves_small_offsets_at_large_world_coordinates() {
        let affine = Affine {
            rows: [
                [1., 0., 0., 1e12 + 0.25],
                [0., 1., 0., 2e12 + 0.5],
                [0., 0., 1., 3e12 + 0.75],
            ],
        };
        let view = affine.relative_view([1e12, 2e12, 3e12]);
        close(view.point([1., 2., 3.]), [1.25, 2.75, 2.5]);
        assert!(
            Affine::nv_reference(
                &Transform {
                    position: [f32::NAN, 0., 0.],
                    rotation: [0.; 3]
                },
                1.
            )
            .is_err()
        );
    }
}
