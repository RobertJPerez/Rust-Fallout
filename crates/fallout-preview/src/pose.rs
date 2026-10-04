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
    SampledSkin(SampledSkinRequest),
    Object(ObjectRequest),
}

#[derive(Clone, Copy)]
pub struct SkinRequest {
    pub geometry: u32,
    pub absolute_weight_tolerance: f64,
}

#[derive(Clone, Copy)]
pub struct SampledSkinRequest {
    pub skin: SkinRequest,
    pub expected_source_sha256: [u8; 32],
    pub animation: ObjectRequest,
    pub controller_policy: source::ControllerPolicy,
}

#[derive(Serialize)]
pub struct SampledSkinSummary {
    pub contract: &'static str,
    pub controller_policy: source::ControllerPolicy,
    pub sample: animation::ObjectPose,
    pub palette: Vec<source::BonePalette>,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    /// Producer element accounting includes scratch already released.
    pub producer_charged_array_bytes: usize,
    pub producer_work_units: usize,
    pub retail_behavior_verified: bool,
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
    pub sample: Option<SampledSkinSummary>,
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
    draw_skin(evaluated, None)
}

pub fn sampled_skin(bytes: &[u8], name: &str, request: SampledSkinRequest) -> Result<Skin> {
    let evaluated = source::evaluate_sampled(
        bytes,
        name,
        source::SampledRequest {
            expected_source_sha256: request.expected_source_sha256,
            skin: source::Request {
                geometry: request.skin.geometry,
                weights: source::WeightPolicy::RequireUnitSum {
                    absolute_tolerance: request.skin.absolute_weight_tolerance,
                },
            },
            controller_policy: request.controller_policy,
        },
        request.animation,
        Default::default(),
    )?;
    let sample = SampledSkinSummary {
        contract: evaluated.contract,
        controller_policy: evaluated.controller_policy,
        sample: evaluated.sample,
        palette: Vec::new(),
        decoder_array_admission_bytes: evaluated.decoder_array_admission_bytes,
        decoder_check_admission_units: evaluated.decoder_check_admission_units,
        producer_charged_array_bytes: evaluated.retained_bytes,
        producer_work_units: evaluated.work_units,
        retail_behavior_verified: false,
    };
    draw_skin(evaluated.skin, Some(sample))
}

fn draw_skin(evaluated: source::Evaluation, sample: Option<SampledSkinSummary>) -> Result<Skin> {
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
    // Move the validated palette into the optional sample receipt. The default
    // stored-pose report remains identical and retains no extra palette copy.
    let sample = sample.map(|mut sample| {
        sample.palette = evaluated.palette;
        sample
    });
    Ok(Skin {
        summary,
        sample,
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
pub(crate) mod tests {
    use super::*;

    const NULL: u32 = u32::MAX;
    const ID: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    const R90: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
    const RM90: [[f32; 3]; 3] = [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]];

    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    fn shorts(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    fn av(
        rotation: [[f32; 3]; 3],
        translation: [f32; 3],
        scale: f32,
        controller: u32,
        properties: &[u32],
    ) -> Vec<u8> {
        [
            words(&[NULL, 0, controller, 0]),
            floats(&translation),
            floats(&rotation.concat()),
            floats(&[scale]),
            words(&[properties.len() as u32]),
            words(properties),
            words(&[NULL]),
        ]
        .concat()
    }
    fn node(
        rotation: [[f32; 3]; 3],
        translation: [f32; 3],
        scale: f32,
        controller: u32,
        children: &[u32],
    ) -> Vec<u8> {
        [
            av(rotation, translation, scale, controller, &[]),
            words(&[children.len() as u32]),
            words(children),
            words(&[0]),
        ]
        .concat()
    }
    fn skin_transform(rotation: [[f32; 3]; 3], translation: [f32; 3], scale: f32) -> Vec<u8> {
        [
            floats(&rotation.concat()),
            floats(&translation),
            floats(&[scale]),
        ]
        .concat()
    }

    /// Authored byte declarations only; the existing production decoder/sampler
    /// owns all interpretation. Matches the second published producer oracle,
    /// with explicit untextured cyan/two-sided material for a visible GPU draw.
    pub(crate) fn sampled_blocks() -> Vec<(&'static str, Vec<u8>)> {
        let geometry = [
            av(ID, [999., 888., 777.], 99., NULL, &[10, 11]),
            words(&[6, 4, 0, NULL]),
            vec![0],
        ]
        .concat();
        let data = [
            words(&[0]),
            shorts(&[3]),
            vec![0, 0, 1],
            floats(&[2., -1., 3., -2., 4., 1., 0., 3., -1.]),
            shorts(&[0]),
            vec![1],
            floats(&[1., 2., 0., 1., 2., 0., 1., 2., 0.]),
            floats(&[0., 0., 0., 10.]),
            vec![0],
            shorts(&[0]),
            words(&[NULL]),
            shorts(&[1]),
            words(&[3]),
            vec![1],
            shorts(&[0, 1, 2, 0]),
        ]
        .concat();
        let mut skin = [skin_transform(ID, [1., -2., 3.], 0.5), words(&[2]), vec![1]].concat();
        for (rotation, translation, scale, influences) in [
            (ID, [-1., 0., 2.], 1., [(0u16, 0.25), (1, 1.)]),
            (R90, [0., -1., 0.], 2., [(0u16, 0.75), (2, 1.)]),
        ] {
            skin.extend(skin_transform(rotation, translation, scale));
            skin.extend(floats(&[0., 0., 0., 10.]));
            skin.extend(shorts(&[2]));
            for (vertex, weight) in influences {
                skin.extend(shorts(&[vertex]));
                skin.extend(floats(&[weight]));
            }
        }
        let controller = [
            words(&[NULL]),
            shorts(&[0xffff]),
            floats(&[17., -9., 100., 101.]),
            words(&[1, 8]),
        ]
        .concat();
        let interpolator = [
            floats(&[1000., 2000., 3000., 2., -3., 4., -5., 12.]),
            words(&[9]),
        ]
        .concat();
        let keys = [
            words(&[0, 2, 1]),
            floats(&[-2., -1., 2., 4., 2., 5., -4., 0.]),
            words(&[2, 1]),
            floats(&[-2., -2., 2., 2.]),
        ]
        .concat();
        vec![
            ("NiNode", node(R90, [4., -2., 8.], 2., NULL, &[1, 2, 3])),
            ("NiNode", node(RM90, [3., 1., 0.], 2., 7, &[])),
            ("NiNode", node(ID, [-2., 4., 1.], 3., NULL, &[])),
            ("NiTriShape", geometry),
            ("NiSkinInstance", words(&[5, NULL, 0, 2, 1, 2])),
            ("NiSkinData", skin),
            ("NiTriShapeData", data),
            ("NiTransformController", controller),
            ("NiTransformInterpolator", interpolator),
            ("NiTransformData", keys),
            (
                "NiMaterialProperty",
                [
                    words(&[NULL, 0, NULL]),
                    floats(&[0., 0., 0., 0., 0., 0., 0., 1., 1.]),
                ]
                .concat(),
            ),
            (
                "NiStencilProperty",
                [words(&[NULL, 0, NULL]), shorts(&[3 << 10]), words(&[0, 0])].concat(),
            ),
        ]
    }

    pub(crate) fn packet(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut types = Vec::new();
        for (name, _) in blocks {
            if !types.contains(name) {
                types.push(*name);
            }
        }
        let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        bytes.extend(words(&[0x14020007]));
        bytes.push(1);
        bytes.extend(words(&[11, blocks.len() as u32, 34]));
        bytes.extend([0; 3]);
        bytes.extend(shorts(&[types.len() as u16]));
        for name in &types {
            bytes.extend(words(&[name.len() as u32]));
            bytes.extend(name.as_bytes());
        }
        for (name, _) in blocks {
            bytes.extend(shorts(&[
                types.iter().position(|v| v == name).unwrap() as u16
            ]));
        }
        for (_, data) in blocks {
            bytes.extend(words(&[data.len() as u32]));
        }
        bytes.extend(words(&[0, 0, 0]));
        for (_, data) in blocks {
            bytes.extend(data);
        }
        bytes.extend(words(&[1, 0]));
        bytes
    }

    pub(crate) fn sampled_request(bytes: &[u8], time: f64) -> SampledSkinRequest {
        SampledSkinRequest {
            skin: SkinRequest {
                geometry: 3,
                absolute_weight_tolerance: 0.,
            },
            expected_source_sha256: Sha256::digest(bytes).into(),
            animation: ObjectRequest {
                object: 1,
                controller: 7,
                source_time: time,
            },
            controller_policy: source::ControllerPolicy::RefuseOtherRequired,
        }
    }

    pub(crate) const SAMPLED_POSITIONS: [[[f32; 3]; 3]; 3] = [
        [[-17.5, 33.5, -4.5], [12., 4., 20.], [2., -2., 42.]],
        [[-15., 37.5, -5.], [6., 12., -2.], [2., -2., 42.]],
        [[-12.5, 41.5, -5.5], [0., 20., -24.], [2., -2., 42.]],
    ];
    pub(crate) const SAMPLED_NORMALS: [[[f32; 3]; 3]; 3] = [
        [[-10., 0., 20.], [-4., 0., 8.], [-12., 0., 24.]],
        [[-9., 0., 18.], [0., 0., 0.], [-12., 0., 24.]],
        [[-8., 0., 16.], [4., 0., -8.], [-12., 0., 24.]],
    ];

    #[test]
    fn sampled_palette_literal_vectors_source_basis_once_and_zero_scale_are_preserved() {
        let bytes = packet(&sampled_blocks());
        let palettes = [
            [[0., -1., 0., 0.5], [1., 0., 0., -2.], [0., 0., -1., 3.]],
            [[0., 0., 0., 2.], [0., 0., 0., -2.5], [0., 0., 0., 4.]],
            [[0., 1., 0., 3.5], [-1., 0., 0., -3.], [0., 0., 1., 5.]],
        ];
        for (index, time) in [-2., 0., 2.].into_iter().enumerate() {
            let skin = sampled_skin(
                &bytes,
                "authored sampled skin",
                sampled_request(&bytes, time),
            )
            .unwrap();
            assert_eq!(skin.positions, SAMPLED_POSITIONS[index]);
            assert_eq!(skin.normals, SAMPLED_NORMALS[index]);
            assert_eq!(
                skin.summary.draw_positions_sha256,
                draw_hash(&SAMPLED_POSITIONS[index])
            );
            assert_eq!(
                skin.summary.draw_normals_sha256,
                draw_hash(&SAMPLED_NORMALS[index])
            );
            assert_eq!(
                skin.summary.skin_to_source_world,
                [[0., -4., 0., -4.], [4., 0., 0., -6.], [0., 0., 4., -4.]]
            );
            assert_eq!(skin.summary.raw_weight_sum_range, [1., 1.]);
            let sample = skin.sample.unwrap();
            assert_eq!(sample.palette.len(), 2);
            assert_eq!((sample.palette[0].ordinal, sample.palette[0].node), (0, 1));
            assert_eq!(sample.palette[0].matrix, palettes[index]);
            assert_eq!(
                sample.palette[1].matrix,
                [[0., -3., 0., 0.], [3., 0., 0., -1.5], [0., 0., 3., 3.5]]
            );
            assert_eq!(sample.sample.requested_time_f64_bits, time.to_bits());
            assert_eq!(sample.sample.source_sha256, skin.summary.source_sha256);
            assert_eq!(sample.sample.unapplied_controller_fields.flags, 0xffff);
            assert_eq!(
                sample.sample.unapplied_controller_fields.frequency_bits,
                17f32.to_bits()
            );
            assert!(!sample.retail_behavior_verified && !sample.sample.retail_behavior_verified);
            assert!(skin.summary.unapplied_controllers.is_empty());
            assert!(
                sample.decoder_array_admission_bytes > 0
                    && sample.producer_charged_array_bytes > 0
                    && sample.producer_work_units > 0
            );
        }
        let zero =
            sampled_skin(&bytes, "negative zero time", sampled_request(&bytes, -0.)).unwrap();
        assert_eq!(zero.positions, SAMPLED_POSITIONS[1]);
        assert_eq!(
            zero.sample.unwrap().sample.requested_time_f64_bits,
            (-0f64).to_bits()
        );
    }

    #[test]
    fn sampled_transport_refuses_source_time_links_required_controllers_and_missing_bones() {
        let bytes = packet(&sampled_blocks());
        let baseline = sampled_request(&bytes, 0.);
        let mut stale = baseline;
        stale.expected_source_sha256[0] ^= 1;
        let mut controller = baseline;
        controller.animation.controller = 8;
        let mut object = baseline;
        object.animation.object = 2;
        let mut geometry = baseline;
        geometry.skin.geometry = 1;
        for (request, expected) in [
            (stale, "source SHA256 differs"),
            (controller, "object.controller differs"),
            (object, "object.controller differs"),
            (geometry, "selected geometry has no decoded skin owner"),
        ] {
            let error = sampled_skin(&bytes, "refuse sampled request", request)
                .err()
                .expect("Invalid request must refuse");
            assert!(error.to_string().contains(expected), "{error}");
        }
        for time in [-3., 3., f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(sampled_skin(&bytes, "invalid time", sampled_request(&bytes, time)).is_err());
        }
        for failure in ["bone", "controller", "rotation", "draw-overflow"] {
            let mut blocks = sampled_blocks();
            let expected = match failure {
                "bone" => {
                    blocks[4].1[16..20].copy_from_slice(&NULL.to_le_bytes());
                    "unresolved root, bone or owner ancestry"
                }
                "controller" => {
                    blocks[2].1[8..12].copy_from_slice(&7u32.to_le_bytes());
                    "required object 2 controller 7 is unapplied"
                }
                "rotation" => {
                    blocks[9].1 = [
                        words(&[1, 1]),
                        floats(&[0., 1., 0., 0., 0.]),
                        blocks[9].1[4..].to_vec(),
                    ]
                    .concat();
                    "rotation key mapping is unapplied"
                }
                "draw-overflow" => {
                    blocks[0].1[64..68].copy_from_slice(&f32::MAX.to_le_bytes());
                    "draw vector is not finite/representable"
                }
                _ => unreachable!(),
            };
            let bytes = packet(&blocks);
            let error = sampled_skin(&bytes, "invalid source", sampled_request(&bytes, 0.))
                .err()
                .expect("Unsupported source must refuse");
            assert!(error.to_string().contains(expected), "{failure}: {error}");
        }
    }

    #[test]
    fn sampled_unit_weight_tolerance_is_explicit_and_never_repairs_raw_weights() {
        let mut blocks = sampled_blocks();
        // Second bind's first influence weight, after the two literal transforms.
        blocks[5].1[211..215].copy_from_slice(&0.5f32.to_le_bytes());
        let bytes = packet(&blocks);
        let mut request = sampled_request(&bytes, 0.);
        assert!(sampled_skin(&bytes, "nonunit", request).is_err());
        for tolerance in [-1., f64::NAN, f64::INFINITY, 0.249] {
            request.skin.absolute_weight_tolerance = tolerance;
            assert!(sampled_skin(&bytes, "invalid/exhausted tolerance", request).is_err());
        }
        request.skin.absolute_weight_tolerance = 0.25;
        let skin = sampled_skin(&bytes, "explicit raw tolerance", request).unwrap();
        assert_eq!(skin.summary.raw_weight_sum_range, [0.75, 1.]);
        assert_eq!(skin.positions[0], [-10.5, 25., -2.]);
        assert_eq!(skin.normals[0], [-6., 0., 12.]);
        assert!(matches!(
            skin.summary.weights,
            source::WeightPolicy::RequireUnitSum {
                absolute_tolerance: 0.25
            }
        ));
    }

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
