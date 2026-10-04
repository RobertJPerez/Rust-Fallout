//! Draw transport for the existing source-pose producer; no skin/link evaluator.
use crate::model::{self, Result};
use bevy::prelude::*;
use fallout_data::{coordinates, nif_skin::pose as source};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
pub struct SkinRequest {
    pub geometry: u32,
    pub absolute_weight_tolerance: f64,
}

#[derive(Serialize)]
pub struct SkinSummary {
    pub contract: &'static str,
    pub source_sha256: String,
    pub geometry: u32,
    pub geometry_data: u32,
    pub instance: u32,
    pub skin_data: u32,
    pub skeleton_root: u32,
    pub weights: source::WeightPolicy,
    pub skin_to_source_world: source::Affine,
    pub palette_entries: usize,
    pub vertices: usize,
    pub raw_weight_sum_range: [f64; 2],
    pub draw_positions_sha256: String,
    pub draw_normals_sha256: String,
    pub unapplied_controllers: Vec<source::UnappliedController>,
    pub normal_convention: &'static str,
    pub retail_behavior_verified: bool,
}

pub struct Skin {
    pub summary: SkinSummary,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Only used for the existing inspection winding/representability check.
    pub world: Mat4,
}

/// Hash the exact GPU attribute order, with explicit little-endian binary32.
pub fn draw_hash(values: &[[f32; 3]]) -> String {
    let mut digest = Sha256::new();
    for value in values.iter().flatten() {
        digest.update(value.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn draw_vectors(
    rows: source::Affine,
    values: &[[f64; 3]],
    directions: bool,
) -> Result<Vec<[f32; 3]>> {
    let mut affine = coordinates::Affine { rows };
    if directions {
        for row in &mut affine.rows {
            row[3] = 0.;
        }
    }
    values
        .iter()
        .map(|&value| {
            let point = affine.point(value);
            let view = coordinates::source_to_view(point, [0.; 3]);
            let narrowed = view.map(|value| value as f32);
            if view.iter().any(|value| !value.is_finite())
                || narrowed.iter().any(|value| !value.is_finite())
            {
                return Err("Source pose draw vector is not finite/representable".into());
            }
            Ok(narrowed)
        })
        .collect()
}

pub fn skin(bytes: &[u8], name: &str, request: SkinRequest) -> Result<Skin> {
    let evaluated = source::evaluate(
        bytes,
        name,
        source::Request {
            geometry: request.geometry,
            weights: source::WeightPolicy::RequireUnitSum {
                absolute_tolerance: request.absolute_weight_tolerance,
            },
        },
        Default::default(),
    )?;
    let positions = draw_vectors(evaluated.skin_to_source_world, &evaluated.positions, false)?;
    let normals = draw_vectors(evaluated.skin_to_source_world, &evaluated.normals, true)?;
    let world = model::affine(evaluated.skin_to_source_world);
    let summary = SkinSummary {
        contract: evaluated.contract,
        source_sha256: evaluated.source_sha256,
        geometry: evaluated.geometry,
        geometry_data: evaluated.geometry_data,
        instance: evaluated.instance,
        skin_data: evaluated.skin_data,
        skeleton_root: evaluated.skeleton_root,
        weights: evaluated.weights,
        skin_to_source_world: evaluated.skin_to_source_world,
        palette_entries: evaluated.palette.len(),
        vertices: evaluated.positions.len(),
        raw_weight_sum_range: evaluated
            .weight_sums
            .iter()
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[min, max], &value| {
                [min.min(value), max.max(value)]
            }),
        draw_positions_sha256: draw_hash(&positions),
        draw_normals_sha256: draw_hash(&normals),
        unapplied_controllers: evaluated.unapplied_controllers,
        normal_convention: "raw weighted linear skin directions, source-world linear map once, no normalization or inverse-transpose; unlit inspection",
        retail_behavior_verified: false,
    };
    Ok(Skin {
        summary,
        positions,
        normals,
        world,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_world_and_basis_apply_once_and_directions_exclude_translation() {
        let rows = [[0., -2., 0., 10.], [3., 0., 0., 20.], [0., 0., 4., 30.]];
        assert_eq!(
            draw_vectors(rows, &[[1., 2., 3.]], false).unwrap(),
            [[6., 42., -23.]]
        );
        assert_eq!(
            draw_vectors(rows, &[[2., 1., 0.5]], true).unwrap(),
            [[-2., 2., -6.]]
        );
    }

    #[test]
    fn draw_mapping_keeps_binary64_until_narrowing_and_refuses_overflow() {
        let rows = [[1., 0., 0., 1e12], [0., 1., 0., 0.], [0., 0., 1., 0.]];
        assert_eq!(
            draw_vectors(rows, &[[-1e12 + 0.25, 2., 3.]], false).unwrap(),
            [[0.25, 3., -2.]]
        );
        assert!(draw_vectors(rows, &[[f64::MAX, 0., 0.]], false).is_err());
        assert!(draw_vectors(rows, &[[f64::NAN, 0., 0.]], true).is_err());
    }
}
