//! Authored byte fixtures and analytic expectations independent of pose helpers.
use fallout_data::nif_skin::pose::{self, Limits, Request, WeightPolicy};

const NULL: u32 = u32::MAX;
const ID: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const R90: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
const RM90: [[f32; 3]; 3] = [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]];

fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn shorts(out: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn transform(out: &mut Vec<u8>, r: [[f32; 3]; 3], t: [f32; 3], s: f32) {
    for row in r {
        floats(out, &row);
    }
    floats(out, &t);
    floats(out, &[s]);
}
fn av(r: [[f32; 3]; 3], t: [f32; 3], s: f32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL, 0, NULL, 0]);
    floats(&mut out, &t);
    for row in r {
        floats(&mut out, &row);
    }
    floats(&mut out, &[s]);
    words(&mut out, &[0, NULL]);
    out
}
fn node(r: [[f32; 3]; 3], t: [f32; 3], s: f32, children: &[u32]) -> Vec<u8> {
    let mut out = av(r, t, s);
    words(&mut out, &[children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn mesh() -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0]);
    shorts(&mut out, &[3]);
    out.extend([0, 0, 1]);
    for point in [[1., 2., 3.], [4., -1., 2.], [-3., 0., 1.]] {
        floats(&mut out, &point);
    }
    shorts(&mut out, &[0]);
    out.push(1);
    for _ in 0..3 {
        floats(&mut out, &[0., 1., 0.]);
    }
    floats(&mut out, &[0., 0., 0., 10.]);
    out.push(0);
    shorts(&mut out, &[0]);
    words(&mut out, &[NULL]);
    shorts(&mut out, &[1]);
    words(&mut out, &[3]);
    out.push(1);
    shorts(&mut out, &[0, 1, 2, 0]);
    out
}
fn skin(
    weights: &[Vec<(u16, f32)>],
    transform_override: Option<([[f32; 3]; 3], [f32; 3], f32)>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let (r, t, s) = transform_override.unwrap_or((ID, [-2., 0., 0.], 0.5));
    transform(&mut out, r, t, s);
    words(&mut out, &[2]);
    out.push(1);
    for (ordinal, influences) in weights.iter().enumerate() {
        if ordinal == 0 {
            transform(&mut out, ID, [-4., 0., 0.], 1.);
        } else {
            transform(&mut out, RM90, [-1.5, 2., 0.], 0.5);
        }
        floats(&mut out, &[0., 0., 0., 10.]);
        shorts(&mut out, &[influences.len() as u16]);
        for &(vertex, weight) in influences {
            shorts(&mut out, &[vertex]);
            floats(&mut out, &[weight]);
        }
    }
    out
}
fn fixture() -> Vec<(&'static str, Vec<u8>)> {
    let mut shape = av(ID, [999., 999., 999.], 7.);
    words(&mut shape, &[6, 4, 0, NULL]);
    shape.push(0);
    let mut instance = Vec::new();
    words(&mut instance, &[5, NULL, 0, 2, 1, 2]);
    vec![
        ("NiNode", node(R90, [10., 20., 30.], 3., &[1, 3])),
        ("NiNode", node(ID, [4., 0., 0.], 1., &[2])),
        ("NiNode", node(R90, [0., 3., 0.], 2., &[])),
        ("NiTriShape", shape),
        ("NiSkinInstance", instance),
        (
            "NiSkinData",
            skin(&[vec![(0, 0.25), (1, 1.)], vec![(0, 0.75), (2, 1.)]], None),
        ),
        ("NiTriShapeData", mesh()),
    ]
}
fn container(blocks: &[(&str, Vec<u8>)], roots: &[u32]) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, 34]);
    out.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    shorts(&mut out, &[types.len() as u16]);
    for name in &types {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        shorts(
            &mut out,
            &[types.iter().position(|n| n == name).unwrap() as u16],
        );
    }
    for (_, payload) in blocks {
        words(&mut out, &[payload.len() as u32]);
    }
    words(&mut out, &[0, 0, 0]);
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[roots.len() as u32]);
    words(&mut out, roots);
    out
}
fn request() -> Request {
    Request {
        geometry: 3,
        weights: WeightPolicy::RequireUnitSum {
            absolute_tolerance: 0.,
        },
    }
}
fn evaluate(blocks: &[(&str, Vec<u8>)]) -> pose::Evaluation {
    pose::evaluate(
        &container(blocks, &[0]),
        "analytic authored",
        request(),
        Limits::default(),
    )
    .unwrap()
}
fn refusal(blocks: &[(&str, Vec<u8>)], expected: &str) {
    let error = pose::evaluate(
        &container(blocks, &[0]),
        "authored refusal",
        request(),
        Limits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn bind_palette_uses_skin_transform_bone_order_and_root_frame_once() {
    let pose = evaluate(&fixture());
    assert_eq!(
        pose.palette
            .iter()
            .map(|p| (p.ordinal, p.node))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2)]
    );
    for bone in &pose.palette {
        assert_eq!(
            bone.matrix,
            [[0.5, 0., 0., -2.], [0., 0.5, 0., 0.], [0., 0., 0.5, 0.]]
        );
    }
    assert_eq!(
        pose.positions,
        [[-1.5, 1., 1.5], [0., -0.5, 1.], [-3.5, 0., 0.5]]
    );
    assert_eq!(pose.normals, [[0., 0.5, 0.]; 3]);
    assert_eq!(pose.weight_sums, [1., 1., 1.]);
    assert_eq!(
        pose.skin_to_source_world,
        [[0., -6., 0., 10.], [6., 0., 0., 32.], [0., 0., 6., 30.]]
    );
    assert_eq!(
        (
            pose.geometry,
            pose.geometry_data,
            pose.instance,
            pose.skin_data,
            pose.skeleton_root
        ),
        (3, 6, 4, 5, 0)
    );
    assert!(!pose.retail_behavior_verified);
}

#[test]
fn stored_source_locals_need_not_equal_bind_pose_and_nested_translation_is_weighted() {
    let mut blocks = fixture();
    blocks[2].1 = node(R90, [0., 5., 0.], 2., &[]);
    let pose = evaluate(&blocks);
    assert_eq!(
        pose.palette[1].matrix,
        [[0.5, 0., 0., -2.], [0., 0.5, 0., 1.], [0., 0., 0.5, 0.]]
    );
    assert_eq!(
        pose.positions,
        [[-1.5, 1.75, 1.5], [0., -0.5, 1.], [-3.5, 1., 0.5]]
    );
}

#[test]
fn reflection_is_preserved_and_inverse_does_not_assume_rotation_transpose() {
    let mut blocks = fixture();
    blocks[5].1 = skin(
        &[vec![(0, 0.25), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        Some((
            [[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            [2., 3., 4.],
            2.,
        )),
    );
    let pose = evaluate(&blocks);
    assert_eq!(pose.positions, [[0., 7., 10.], [-6., 1., 8.], [8., 3., 6.]]);
    assert_eq!(
        pose.skin_to_source_world,
        [
            [0., -1.5, 0., 14.5],
            [-1.5, 0., 0., 23.],
            [0., 0., 1.5, 24.]
        ]
    );
}

#[test]
fn raw_duplicate_weights_are_added_without_sorting_pruning_or_normalizing() {
    let mut blocks = fixture();
    blocks[5].1 = skin(
        &[vec![(0, 0.25), (0, 0.5), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        None,
    );
    let bytes = container(&blocks, &[0]);
    let pose = pose::evaluate(
        &bytes,
        "raw",
        Request {
            weights: WeightPolicy::PreserveRawNonnegative,
            ..request()
        },
        Limits::default(),
    )
    .unwrap();
    assert_eq!(pose.weight_sums, [1.5, 1., 1.]);
    assert_eq!(pose.positions[0], [-2.25, 1.5, 2.25]);
    refusal(&blocks, "raw weight sum 1.5");
}

#[test]
fn missing_negative_and_nonunit_weights_refuse_for_the_intended_reason() {
    let mut blocks = fixture();
    blocks[5].1 = skin(&[vec![(0, 0.25)], vec![(0, 0.75), (2, 1.)]], None);
    refusal(&blocks, "vertex 1 has no positive");
    blocks[5].1 = skin(&[vec![(0, -0.25), (1, 1.)], vec![(0, 1.25), (2, 1.)]], None);
    refusal(&blocks, "negative or nonfinite raw weight");
    blocks[5].1 = skin(
        &[vec![(0, 0.25), (1, 0.75)], vec![(0, 0.75), (2, 1.)]],
        None,
    );
    refusal(&blocks, "raw weight sum 0.75");
    blocks[5].1 = skin(&[vec![], vec![]], None);
    blocks[5].1[56] = 0;
    refusal(&blocks, "NiSkinData vertex weights unavailable");
}

#[test]
fn unresolved_and_outside_bones_and_unknown_scene_edges_never_get_default_matrices() {
    let mut blocks = fixture();
    blocks[4].1[16..20].copy_from_slice(&NULL.to_le_bytes());
    refusal(&blocks, "unresolved root, bone or owner ancestry");
    blocks = fixture();
    blocks.push(("NiNode", node(ID, [0.; 3], 1., &[])));
    blocks[4].1[16..20].copy_from_slice(&7u32.to_le_bytes());
    let error = pose::evaluate(
        &container(&blocks, &[0, 7]),
        "outside",
        request(),
        Limits::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unresolved root, bone or owner ancestry")
    );
    blocks = fixture();
    blocks.push(("NiBillboardNode", Vec::new()));
    blocks[0].1 = node(R90, [10., 20., 30.], 3., &[1, 3, 7]);
    refusal(&blocks, "unresolved scene ancestry");
}

#[test]
fn singular_skin_mapping_and_invalid_request_tolerance_refuse() {
    let mut blocks = fixture();
    blocks[5].1 = skin(
        &[vec![(0, 0.25), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        Some((ID, [0.; 3], 0.)),
    );
    refusal(&blocks, "singular or overflowing SkinTransform");
    let bytes = container(&fixture(), &[0]);
    for tolerance in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        let error = pose::evaluate(
            &bytes,
            "bad tolerance",
            Request {
                weights: WeightPolicy::RequireUnitSum {
                    absolute_tolerance: tolerance,
                },
                ..request()
            },
            Limits::default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("weight tolerance must be finite")
        );
    }
}

#[test]
fn declared_array_work_and_depth_boundaries_are_enforced() {
    let bytes = container(&fixture(), &[0]);
    let baseline = pose::evaluate(&bytes, "budget", request(), Limits::default()).unwrap();
    let limits = Limits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        ..Limits::default()
    };
    assert!(pose::evaluate(&bytes, "exact", request(), limits).is_ok());
    let error = pose::evaluate(
        &bytes,
        "under storage",
        request(),
        Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("array storage budget exceeded"));
    let error = pose::evaluate(
        &bytes,
        "under work",
        request(),
        Limits {
            work_units: limits.work_units - 1,
            ..limits
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("work budget exceeded"));
    let error = pose::evaluate(
        &bytes,
        "under depth",
        request(),
        Limits {
            ancestry_depth: 1,
            ..limits
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("ancestry depth budget exceeded"));
}

#[test]
fn ancestor_and_bone_controllers_are_explicitly_unapplied_and_identity_changes() {
    let plain = evaluate(&fixture());
    let mut blocks = fixture();
    blocks.push(("NiControllerManager", vec![0; 4]));
    for id in [0, 1, 3] {
        blocks[id].1[8..12].copy_from_slice(&7u32.to_le_bytes());
    }
    let pose = evaluate(&blocks);
    assert_eq!(pose.positions, plain.positions);
    assert_ne!(pose.source_sha256, plain.source_sha256);
    assert_eq!(
        pose.unapplied_controllers
            .iter()
            .map(|c| (c.object, c.controller))
            .collect::<Vec<_>>(),
        [(0, 7), (3, 7), (1, 7)]
    );
}

#[test]
fn root_below_footer_keeps_the_same_palette_and_includes_ancestors_once() {
    let mut blocks = fixture();
    blocks.push(("NiNode", node(ID, [-5., 1., 0.], 2., &[0])));
    let pose = pose::evaluate(
        &container(&blocks, &[7]),
        "parent frame",
        request(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        pose.positions,
        [[-1.5, 1., 1.5], [0., -0.5, 1.], [-3.5, 0., 0.5]]
    );
    assert_eq!(
        pose.skin_to_source_world,
        [[0., -12., 0., 15.], [12., 0., 0., 65.], [0., 0., 12., 60.]]
    );
}

#[test]
fn missing_normals_remain_absent_and_missing_positions_refuse() {
    let mut blocks = fixture();
    blocks[6].1[47] = 0;
    blocks[6].1.drain(48..84);
    assert!(evaluate(&blocks).normals.is_empty());
    blocks = fixture();
    blocks[6].1[8] = 0;
    blocks[6].1.drain(9..45);
    refusal(&blocks, "vertex positions unavailable");
}
