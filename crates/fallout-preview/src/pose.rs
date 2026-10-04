//! Draw transport for the existing source-pose producer; no skin/link evaluator.
use crate::model::{self, Result};
use bevy::math::DMat4;
use bevy::prelude::*;
use fallout_data::{coordinates, nif_skin::pose as source};
use fallout_data::{nif_animation::pose as animation, nif_scene};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};

pub use animation::Request as ObjectRequest;

#[derive(Clone, Copy)]
pub enum Request {
    Skin(SkinRequest),
    Object(ObjectRequest),
}

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
    values: impl IntoIterator<Item = [f64; 3]>,
    directions: bool,
) -> Result<Vec<[f32; 3]>> {
    let mut affine = coordinates::Affine { rows };
    if directions {
        for row in &mut affine.rows {
            row[3] = 0.;
        }
    }
    values
        .into_iter()
        .map(|value| {
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
    let positions = draw_vectors(
        evaluated.skin_to_source_world,
        evaluated.positions.iter().copied(),
        false,
    )?;
    let normals = draw_vectors(
        evaluated.skin_to_source_world,
        evaluated.normals.iter().copied(),
        true,
    )?;
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

#[derive(Serialize)]
pub struct ObjectSummary {
    pub evaluation: animation::ObjectPose,
    pub draw_meshes: Vec<ObjectMesh>,
    pub normal_convention: &'static str,
}

#[derive(Serialize)]
pub struct ObjectMesh {
    pub geometry: u32,
    pub geometry_data: u32,
    pub source_world: source::Affine,
    pub positions_sha256: String,
    pub normals_sha256: String,
}

pub struct Object {
    pub summary: ObjectSummary,
    pub worlds: BTreeMap<u32, source::Affine>,
}

#[derive(Clone, Copy)]
struct DrawLimits {
    objects: usize,
    work: usize,
    depth: usize,
}

impl Default for DrawLimits {
    fn default() -> Self {
        Self {
            objects: 16_384,
            work: 32_768,
            depth: 1024,
        }
    }
}

/// Select static descendants of one freshly evaluated source object. The
/// producer owns sampled poses; the renderer assembles static draw transforms.
fn object_worlds(
    scene: &nif_scene::Scene,
    pose: &animation::ObjectPose,
    mut limits: DrawLimits,
) -> Result<BTreeMap<u32, source::Affine>> {
    if !scene.unsupported_scene_edges.is_empty() || scene.objects.len() > 16_384 {
        return Err(
            "Selected pose draw hierarchy is unresolved or exceeds its object bound".into(),
        );
    }
    let objects: BTreeMap<_, _> = scene.objects.iter().map(|v| (v.block, v)).collect();
    let mut pending = VecDeque::from([(pose.object.block, pose.source_world, 1)]);
    let mut worlds = BTreeMap::new();
    while let Some((id, world, depth)) = pending.pop_front() {
        if depth > limits.depth || limits.objects == 0 || limits.work == 0 {
            return Err("Selected pose draw hierarchy exceeded depth/object/work budget".into());
        }
        limits.objects -= 1;
        limits.work -= 1;
        let object = objects
            .get(&id)
            .ok_or("Selected pose draw object is unresolved")?;
        if id != pose.object.block && object.controller.is_some() {
            return Err(
                format!("Selected pose descendant {id} has an unapplied controller").into(),
            );
        }
        if matches!(
            object.kind,
            nif_scene::ObjectKind::Mesh { skin: Some(_), .. }
        ) {
            return Err(
                format!("Selected pose descendant {id} requires an animated skin palette").into(),
            );
        }
        if !world.iter().flatten().all(|value| value.is_finite())
            || worlds.insert(id, world).is_some()
        {
            return Err("Selected pose draw transform is nonfinite or repeated".into());
        }
        if let nif_scene::ObjectKind::Node { children, .. } = &object.kind {
            for &child in children.iter().flatten() {
                if limits.work == 0 || pending.len() >= limits.objects {
                    return Err(
                        "Selected pose pending hierarchy exceeded object/work budget".into(),
                    );
                }
                limits.work -= 1;
                let child_object = objects
                    .get(&child)
                    .ok_or("Selected pose draw child is unresolved")?;
                let world = static_child_world(world, child_object.transform);
                pending.push_back((child, world, depth + 1));
            }
        }
    }
    Ok(worlds)
}

fn static_child_world(parent: source::Affine, child: nif_scene::Transform) -> source::Affine {
    let matrix = |rows: source::Affine| {
        DMat4::from_cols_array_2d(&std::array::from_fn(|column| {
            std::array::from_fn(|row| {
                if row == 3 {
                    f64::from(column == 3)
                } else {
                    rows[row][column]
                }
            })
        }))
    };
    // NiAV rows/scale/translation are already source-decoded declarations.
    // No child channel, clock, quaternion or palette interpretation occurs here.
    let local = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if column == 3 {
                f64::from(child.translation[row])
            } else {
                f64::from(child.rotation[row][column]) * f64::from(child.scale)
            }
        })
    });
    let columns = (matrix(parent) * matrix(local)).to_cols_array_2d();
    std::array::from_fn(|row| std::array::from_fn(|column| columns[column][row]))
}

pub fn object(
    bytes: &[u8],
    name: &str,
    request: ObjectRequest,
    scene: &nif_scene::Scene,
) -> Result<Object> {
    let evaluation = animation::evaluate(bytes, name, request, Default::default())?;
    let worlds = object_worlds(scene, &evaluation, Default::default())?;
    Ok(Object {
        summary: ObjectSummary {
            evaluation,
            draw_meshes: Vec::new(),
            normal_convention: "raw source normal directions through posed linear world map, translation excluded, no normalization/inverse-transpose; unlit inspection",
        },
        worlds,
    })
}

pub fn object_vectors(
    world: source::Affine,
    values: &[[f32; 3]],
    directions: bool,
) -> Result<Vec<[f32; 3]>> {
    draw_vectors(
        world,
        values.iter().map(|value| value.map(f64::from)),
        directions,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_decoded_draw_hierarchy_obeys_exact_depth_object_and_work_boundaries() {
        let bytes = include_bytes!("testdata/source-pose-triangle.packet");
        let (_, scene) = nif_scene::decode(bytes, "authored hierarchy").unwrap();
        let pose = animation::evaluate(
            bytes,
            "authored hierarchy",
            ObjectRequest {
                object: 1,
                controller: 2,
                source_time: 0.,
            },
            Default::default(),
        )
        .unwrap();
        let exact = DrawLimits {
            objects: 2,
            work: 3,
            depth: 2,
        };
        let worlds = object_worlds(&scene, &pose, exact).unwrap();
        assert_eq!(worlds.len(), 2);
        assert!(!worlds.contains_key(&0));
        assert_eq!(
            worlds[&5],
            [[0., 0., 1., -6.], [1., 0., 0., 5.], [0., -1., 0., -1.]]
        );
        for limits in [
            DrawLimits {
                objects: 1,
                ..exact
            },
            DrawLimits { work: 2, ..exact },
            DrawLimits { depth: 1, ..exact },
        ] {
            assert!(object_worlds(&scene, &pose, limits).is_err());
        }
    }

    #[test]
    fn source_world_and_basis_apply_once_and_directions_exclude_translation() {
        let rows = [[0., -2., 0., 10.], [3., 0., 0., 20.], [0., 0., 4., 30.]];
        assert_eq!(
            draw_vectors(rows, [[1., 2., 3.]], false).unwrap(),
            [[6., 42., -23.]]
        );
        assert_eq!(
            draw_vectors(rows, [[2., 1., 0.5]], true).unwrap(),
            [[-2., 2., -6.]]
        );
    }

    #[test]
    fn draw_mapping_keeps_binary64_until_narrowing_and_refuses_overflow() {
        let rows = [[1., 0., 0., 1e12], [0., 1., 0., 0.], [0., 0., 1., 0.]];
        assert_eq!(
            draw_vectors(rows, [[-1e12 + 0.25, 2., 3.]], false).unwrap(),
            [[0.25, 3., -2.]]
        );
        assert!(draw_vectors(rows, [[f64::MAX, 0., 0.]], false).is_err());
        assert!(draw_vectors(rows, [[f64::NAN, 0., 0.]], true).is_err());
    }
}
