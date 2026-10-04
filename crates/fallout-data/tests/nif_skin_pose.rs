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

fn influence_fixture() -> Vec<(&'static str, Vec<u8>)> {
    let mut blocks = fixture();
    blocks[2].1 = node(R90, [0., 5., 0.], 2., &[]);
    blocks[5].1 = skin(
        &[
            vec![
                (2, 0.25),
                (0, 0.25),
                (0, 0.5),
                (1, 1.),
                (0, -0.),
                (0, 0.125),
                (0, 0.125),
                (0, 0.),
            ],
            vec![(0, 0.75), (2, 0.75)],
        ],
        None,
    );
    blocks
}

fn named_container(blocks: &[(&str, Vec<u8>)], roots: &[u32], strings: &[&[u8]]) -> Vec<u8> {
    // Same authored container layout, with an explicit physical string table.
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x14020007]);
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
    words(
        &mut out,
        &[
            strings.len() as u32,
            strings.iter().map(|s| s.len()).max().unwrap_or(0) as u32,
        ],
    );
    for value in strings {
        words(&mut out, &[value.len() as u32]);
        out.extend(*value);
    }
    words(&mut out, &[0]);
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[roots.len() as u32]);
    words(&mut out, roots);
    out
}

fn external_rig_blocks() -> Vec<(&'static str, Vec<u8>)> {
    let mut rig = vec![
        ("NiNode", node(ID, [0.; 3], 1., &[])),
        (
            "NiNode",
            node(
                [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]],
                [100., 200., 300.],
                5.,
                &[2, 4],
            ),
        ),
        ("NiNode", node(R90, [0., 4., 0.], 2., &[3])),
        ("NiNode", node(RM90, [2., 0., 1.], 3., &[])),
        ("NiNode", node(ID, [-5., 0., 2.], 4., &[])),
    ];
    rig[2].1[..4].copy_from_slice(&0u32.to_le_bytes());
    for id in [0, 3, 4] {
        rig[id].1[..4].copy_from_slice(&1u32.to_le_bytes());
    }
    rig
}
fn external_fixture() -> (Vec<u8>, Vec<u8>, fallout_data::nif_skin::external::Request) {
    use fallout_data::nif_skin::external::{BoneMapping, Request};
    use sha2::{Digest, Sha256};
    let mut skin = fixture();
    skin[1].1[..4].copy_from_slice(&0u32.to_le_bytes());
    skin[2].1[..4].copy_from_slice(&1u32.to_le_bytes());
    let skin = named_container(&skin, &[0], &[b"Skin-A\xff\0", b"Twin"]);
    let rig = named_container(&external_rig_blocks(), &[1], &[b"Rig-A\0", b"Twin"]);
    let request = Request {
        expected_skin_sha256: Sha256::digest(&skin).into(),
        expected_rig_sha256: Sha256::digest(&rig).into(),
        geometry: 3,
        rig_root: 1,
        explicit_bone_mapping: vec![
            BoneMapping {
                bone_ordinal: 0,
                rig_node: 2,
                expected_skin_bone_name_bytes: b"Skin-A\xff\0".to_vec(),
                expected_rig_node_name_bytes: b"Rig-A\0".to_vec(),
            },
            BoneMapping {
                bone_ordinal: 1,
                rig_node: 3,
                expected_skin_bone_name_bytes: b"Twin".to_vec(),
                expected_rig_node_name_bytes: b"Twin".to_vec(),
            },
        ],
        explicit_root_space_mapping: [[0., -1., 0., 3.], [1., 0., 0., -2.], [0., 0., 2., 1.]],
        weights: WeightPolicy::RequireUnitSum {
            absolute_tolerance: 0.,
        },
    };
    (skin, rig, request)
}

#[test]
fn external_root_mapping_and_explicit_bones_have_noncommuting_literal_expectations() {
    use fallout_data::nif_skin::external;
    let (skin, rig, request) = external_fixture();
    let result = external::evaluate(
        &skin,
        &rig,
        "two authored sources",
        &request,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result.skin.palette[0].matrix,
        [[-1., 0., 0., 1.5], [0., -1., 0., -1.], [0., 0., 2., 0.5]]
    );
    assert_eq!(
        result.skin.palette[1].matrix,
        [[1.5, 0., 0., -10.5], [0., 1.5, 0., -5.5], [0., 0., 3., 2.5]]
    );
    assert_eq!(
        result.skin.positions,
        [[-6.625, -2.625, 10.25], [-2.5, 0., 4.5], [-15., -5.5, 5.5]]
    );
    assert_eq!(
        result.skin.normals,
        [[0., 0.875, 0.], [0., -1., 0.], [0., 1.5, 0.]]
    );
    assert_eq!(result.skin.weight_sums, [1.; 3]);
    assert_eq!(
        result.skin.skin_to_source_world,
        [[0., -6., 0., 10.], [6., 0., 0., 32.], [0., 0., 6., 30.]]
    );
    assert_eq!(
        result
            .mappings
            .iter()
            .map(|m| (m.skin_node.block, m.rig_node.block))
            .collect::<Vec<_>>(),
        [(1, 2), (2, 3)]
    );
    assert_eq!(
        result.mappings[1].rig_bone_to_root,
        [[6., 0., 0., 0.], [0., 6., 0., 8.], [0., 0., 6., 2.]]
    );
    assert_eq!(
        result.unapplied_rig_root_local,
        [[-5., 0., 0., 100.], [0., -5., 0., 200.], [0., 0., 5., 300.]]
    );
    assert!(!result.retail_behavior_verified);
}

#[test]
fn external_same_raw_names_require_explicit_distinct_target_ids_and_permutation_has_no_priority() {
    use fallout_data::nif_skin::external;
    let (skin, rig, mut request) = external_fixture();
    let first =
        external::evaluate(&skin, &rig, "same sources", &request, Default::default()).unwrap();
    request.explicit_bone_mapping.reverse();
    let permuted =
        external::evaluate(&skin, &rig, "same sources", &request, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&permuted).unwrap()
    );
    request.explicit_bone_mapping[0].rig_node = 4;
    let second = external::evaluate(
        &skin,
        &rig,
        "same raw Twin name",
        &request,
        Default::default(),
    )
    .unwrap();
    assert_eq!(second.skin.positions[2], [-7.5, -6.5, 4.5]);
    assert_ne!(first.skin.positions, second.skin.positions);
    assert_eq!(
        second.mappings[1].raw_rig_node_name_bytes,
        first.mappings[1].raw_rig_node_name_bytes
    );
}

#[test]
fn external_duplicate_incomplete_name_sha_root_and_space_requests_refuse() {
    use fallout_data::nif_skin::external;
    let (skin, rig, mut request) = external_fixture();
    request.explicit_bone_mapping[1].bone_ordinal = 0;
    assert!(
        external::evaluate(
            &skin,
            &rig,
            "duplicate ordinal",
            &request,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("duplicate skin bone ordinal")
    );
    request.explicit_bone_mapping[1].bone_ordinal = 1;
    request.explicit_bone_mapping[1].rig_node = 2;
    assert!(
        external::evaluate(
            &skin,
            &rig,
            "duplicate target",
            &request,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("duplicate rig node target")
    );
    request.explicit_bone_mapping[1].rig_node = 3;
    request.explicit_bone_mapping[1]
        .expected_rig_node_name_bytes
        .push(0);
    assert!(
        external::evaluate(
            &skin,
            &rig,
            "raw NUL is exact",
            &request,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("raw name differs")
    );
    request.explicit_bone_mapping[1]
        .expected_rig_node_name_bytes
        .pop();
    request.expected_skin_sha256[0] ^= 1;
    assert!(
        external::evaluate(&skin, &rig, "skin SHA", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("skin source SHA256 differs")
    );
    request.expected_skin_sha256[0] ^= 1;
    request.expected_rig_sha256[0] ^= 1;
    assert!(
        external::evaluate(&skin, &rig, "rig SHA", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("rig source SHA256 differs")
    );
    request.expected_rig_sha256[0] ^= 1;
    request.rig_root = 3;
    assert!(
        external::evaluate(&skin, &rig, "chosen root", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("does not reach chosen rig root")
    );
    request.rig_root = 0;
    assert!(
        external::evaluate(&skin, &rig, "orphan root", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("no reachable ancestry")
    );
    request.rig_root = 1;
    let original = request.explicit_root_space_mapping;
    request.explicit_root_space_mapping[0] = [0.; 4];
    assert!(
        external::evaluate(&skin, &rig, "singular map", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("root-space mapping is singular")
    );
    request.explicit_root_space_mapping = original;
    request.explicit_root_space_mapping[0][0] = f64::NAN;
    assert!(
        external::evaluate(&skin, &rig, "nonfinite map", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("root-space mapping must be finite")
    );
    request.explicit_root_space_mapping = original;
    request.explicit_bone_mapping.pop();
    assert!(
        external::evaluate(&skin, &rig, "incomplete", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("must cover every")
    );
}

#[test]
fn external_combined_arrays_work_input_map_names_and_source_allowances_have_exact_ceilings() {
    use fallout_data::nif_skin::external::{self, Limits};
    let (skin, rig, request) = external_fixture();
    let result = external::evaluate(&skin, &rig, "bounded", &request, Default::default()).unwrap();
    let exact = Limits {
        array_bytes: result.retained_bytes,
        work_units: result.work_units,
        source_bytes: skin.len() + rig.len(),
        mapping_bones: 2,
        raw_name_bytes: request
            .explicit_bone_mapping
            .iter()
            .map(|m| m.expected_skin_bone_name_bytes.len() + m.expected_rig_node_name_bytes.len())
            .sum(),
        decoder_array_admission_bytes: result.decoder_array_admission_bytes,
        decoder_check_admission_units: result.decoder_check_admission_units,
        ancestry_depth: 3,
        ..Default::default()
    };
    external::evaluate(&skin, &rig, "bounded", &request, exact).unwrap();
    for (limits, error) in [
        (
            Limits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage budget",
        ),
        (
            Limits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            Limits {
                source_bytes: exact.source_bytes - 1,
                ..exact
            },
            "combined source input",
        ),
        (
            Limits {
                mapping_bones: 1,
                ..exact
            },
            "mapping count budget",
        ),
        (
            Limits {
                raw_name_bytes: exact.raw_name_bytes - 1,
                ..exact
            },
            "raw name byte budget",
        ),
        (
            Limits {
                decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
                ..exact
            },
            "combined decoder array",
        ),
        (
            Limits {
                decoder_check_admission_units: exact.decoder_check_admission_units - 1,
                ..exact
            },
            "combined decoder check",
        ),
        (
            Limits {
                ancestry_depth: 1,
                ..exact
            },
            "ancestry depth budget",
        ),
    ] {
        assert!(
            external::evaluate(&skin, &rig, "bounded", &request, limits)
                .unwrap_err()
                .to_string()
                .contains(error)
        );
    }
}

#[test]
fn external_cycle_unresolved_link_orphan_and_missing_raw_name_never_return_a_palette() {
    use fallout_data::nif_skin::external;
    use sha2::{Digest, Sha256};
    let (skin, rig, mut request) = external_fixture();
    let index = fallout_data::nif::inspect(&rig, "authored mutation offsets").unwrap();
    let mut cycle = rig.clone();
    let first_child = index.blocks[1].offset + 80;
    cycle[first_child..first_child + 4].copy_from_slice(&1u32.to_le_bytes());
    request.expected_rig_sha256 = Sha256::digest(&cycle).into();
    let early = external::evaluate(
        &skin,
        &cycle,
        "footer-root self edge",
        &request,
        Default::default(),
    )
    .unwrap_err();
    assert!(
        early
            .to_string()
            .contains("footer root also has a scene parent"),
        "{early}"
    );
    // Preserve that stronger root refusal, then make root0 the footer root so
    // this disconnected self-cycle reaches the decoder's general cycle check.
    let footer = cycle.len() - 4;
    cycle[footer..].copy_from_slice(&0u32.to_le_bytes());
    request.expected_rig_sha256 = Sha256::digest(&cycle).into();
    assert!(
        external::evaluate(&skin, &cycle, "cyclic rig", &request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    let mut unresolved = rig.clone();
    unresolved[first_child..first_child + 4].copy_from_slice(&999u32.to_le_bytes());
    request.expected_rig_sha256 = Sha256::digest(&unresolved).into();
    assert!(
        external::evaluate(
            &skin,
            &unresolved,
            "invalid rig child",
            &request,
            Default::default()
        )
        .is_err()
    );
    request.expected_rig_sha256 = Sha256::digest(&rig).into();
    request.explicit_bone_mapping[1].rig_node = 0;
    assert!(
        external::evaluate(
            &skin,
            &rig,
            "same name orphan",
            &request,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("no reachable ancestry")
    );
    request.explicit_bone_mapping[1].rig_node = 3;
    let mut no_name = rig.clone();
    let target = index.blocks[3].offset;
    no_name[target..target + 4].copy_from_slice(&NULL.to_le_bytes());
    request.expected_rig_sha256 = Sha256::digest(&no_name).into();
    assert!(
        external::evaluate(
            &skin,
            &no_name,
            "missing raw name",
            &request,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("no authored raw name")
    );
}

#[test]
fn external_root_local_is_unapplied_and_skin_rig_controllers_are_source_observations() {
    use fallout_data::nif_skin::external;
    use sha2::{Digest, Sha256};
    let (skin, _, mut request) = external_fixture();
    let mut rig = external_rig_blocks();
    rig[1].1 = node(ID, [999., 888., 777.], 7., &[2, 4]);
    rig[2].1[8..12].copy_from_slice(&5u32.to_le_bytes());
    let mut controller = Vec::new();
    words(&mut controller, &[NULL]);
    shorts(&mut controller, &[0xffff]);
    floats(&mut controller, &[17., -9., 100., 101.]);
    words(&mut controller, &[2, NULL]);
    rig.push(("NiTransformController", controller));
    let rig = named_container(&rig, &[1], &[b"Rig-A\0", b"Twin"]);
    request.expected_rig_sha256 = Sha256::digest(&rig).into();
    let result = external::evaluate(
        &skin,
        &rig,
        "unapplied rig local/controller",
        &request,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result.skin.positions,
        [[-6.625, -2.625, 10.25], [-2.5, 0., 4.5], [-15., -5.5, 5.5]]
    );
    assert_eq!(
        result
            .rig_unapplied_controllers
            .iter()
            .map(|c| (c.object, c.controller))
            .collect::<Vec<_>>(),
        [(2, 5)]
    );
    assert!(result.skin.unapplied_controllers.is_empty());
    let mut skin = fixture();
    skin[0].1[8..12].copy_from_slice(&7u32.to_le_bytes());
    skin[1].1[..4].copy_from_slice(&0u32.to_le_bytes());
    skin[2].1[..4].copy_from_slice(&1u32.to_le_bytes());
    let mut controller = Vec::new();
    words(&mut controller, &[NULL]);
    shorts(&mut controller, &[0xffff]);
    floats(&mut controller, &[17., -9., 100., 101.]);
    words(&mut controller, &[0, NULL]);
    skin.push(("NiTransformController", controller));
    let skin = named_container(&skin, &[0], &[b"Skin-A\xff\0", b"Twin"]);
    request.expected_skin_sha256 = Sha256::digest(&skin).into();
    let result = external::evaluate(
        &skin,
        &rig,
        "both controllers unapplied",
        &request,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result
            .skin
            .unapplied_controllers
            .iter()
            .map(|c| (c.object, c.controller))
            .collect::<Vec<_>>(),
        [(0, 7)]
    );
    assert_eq!(
        result.skin.positions,
        [[-6.625, -2.625, 10.25], [-2.5, 0., 4.5], [-15., -5.5, 5.5]]
    );
}

#[test]
fn influence_csr_preserves_sparse_duplicate_zero_raw_bits_and_source_order() {
    use fallout_data::nif_skin::influences;
    let bytes = container(&influence_fixture(), &[0]);
    let table = influences::prepare(&bytes, "independent sparse", 3, Default::default()).unwrap();
    assert_eq!(table.vertex_offsets(), &[0, 7, 8, 10]);
    assert_eq!(table.geometry(), 3);
    assert_eq!(table.instance(), 4);
    let actual = table
        .entries()
        .iter()
        .map(|e| (e.bone_ordinal, e.source_weight_ordinal, e.weight_bits))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (0, 1, 0.25f32.to_bits()),
            (0, 2, 0.5f32.to_bits()),
            (0, 4, (-0.0f32).to_bits()),
            (0, 5, 0.125f32.to_bits()),
            (0, 6, 0.125f32.to_bits()),
            (0, 7, 0.0f32.to_bits()),
            (1, 0, 0.75f32.to_bits()),
            (0, 3, 1.0f32.to_bits()),
            (0, 0, 0.25f32.to_bits()),
            (1, 1, 0.75f32.to_bits())
        ]
    );
    assert_eq!(table.usage().entries, 10);
    assert!(table.usage().array_bytes > table.usage().output_bytes);
}

#[test]
fn influence_count_storage_work_and_decoder_admissions_have_exact_ceilings() {
    use fallout_data::nif_skin::influences::{self, Limits};
    let bytes = container(&influence_fixture(), &[0]);
    let usage = influences::prepare(&bytes, "bounded", 3, Default::default())
        .unwrap()
        .usage();
    let exact = Limits {
        entries: 10,
        array_bytes: usage.array_bytes,
        work_units: usage.work_units,
        decoder_array_admission_bytes: usage.decoder_array_admission_bytes,
        decoder_check_admission_units: usage.decoder_check_admission_units,
        ..Default::default()
    };
    influences::prepare(&bytes, "bounded", 3, exact).unwrap();
    for (limits, error) in [
        (
            Limits {
                entries: 9,
                ..exact
            },
            "entry budget",
        ),
        (
            Limits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage",
        ),
        (
            Limits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            Limits {
                decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
                ..exact
            },
            "decoder array admission",
        ),
        (
            Limits {
                decoder_check_admission_units: exact.decoder_check_admission_units - 1,
                ..exact
            },
            "decoder check admission",
        ),
    ] {
        assert!(
            influences::prepare(&bytes, "bounded", 3, limits)
                .unwrap_err()
                .to_string()
                .contains(error)
        );
    }
    let mut overflow = Limits::default();
    overflow.source.array_bytes = usize::MAX;
    assert!(
        influences::prepare(&bytes, "bounded", 3, overflow)
            .unwrap_err()
            .to_string()
            .contains("decoder array admission")
    );
    let mut overflow = Limits::default();
    overflow.source.graph_checks = usize::MAX;
    assert!(
        influences::prepare(&bytes, "bounded", 3, overflow)
            .unwrap_err()
            .to_string()
            .contains("decoder check admission")
    );
    let mut short = Limits::default();
    short.source.partition.skin.scene.input_bytes = bytes.len() - 1;
    assert!(
        influences::prepare(&bytes, "bounded", 3, short)
            .unwrap_err()
            .to_string()
            .contains("input byte budget")
    );
}

#[test]
fn influence_csr_reconstruction_uses_every_raw_entry_and_matches_source_pose() {
    use fallout_data::nif_skin::influences;
    let bytes = container(&influence_fixture(), &[0]);
    let table = influences::prepare(&bytes, "sparse", 3, Default::default()).unwrap();
    let raw = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let rebuilt = table
        .evaluate(&bytes, "sparse", raw, Default::default())
        .unwrap();
    let old = pose::evaluate(&bytes, "sparse", raw, Default::default()).unwrap();
    assert_eq!(
        rebuilt.positions,
        [[-2.625, 2.5, 2.625], [0., -0.5, 1.], [-3.5, 0.75, 0.5]]
    );
    assert_eq!(
        rebuilt.normals,
        [[0., 0.875, 0.], [0., 0.5, 0.], [0., 0.5, 0.]]
    );
    assert_eq!(rebuilt.weight_sums, [1.75, 1., 1.]);
    assert_eq!(rebuilt.positions, old.positions);
    assert_eq!(rebuilt.normals, old.normals);
    assert_eq!(rebuilt.weight_sums, old.weight_sums);
    assert_eq!(rebuilt.skin_to_source_world, old.skin_to_source_world);
    assert_eq!(
        serde_json::to_value(&rebuilt.palette).unwrap(),
        serde_json::to_value(&old.palette).unwrap()
    );
    assert_eq!(rebuilt.retained_bytes, old.retained_bytes);
    assert_eq!(rebuilt.work_units, old.work_units + 3);
    assert_eq!(
        rebuilt.contract,
        "engineering-exact-csr-source-local-skin-v1"
    );
    assert!(
        table
            .evaluate(&bytes, "sparse", request(), Default::default())
            .unwrap_err()
            .to_string()
            .contains("raw weight sum 1.75")
    );
    let tolerance = Request {
        weights: WeightPolicy::RequireUnitSum {
            absolute_tolerance: 0.75,
        },
        ..request()
    };
    assert_eq!(
        table
            .evaluate(&bytes, "sparse", tolerance, Default::default())
            .unwrap()
            .positions,
        old.positions
    );
}

#[test]
fn influence_table_cannot_be_reused_as_another_source_geometry_or_instance() {
    use fallout_data::nif_skin::influences;
    let blocks = influence_fixture();
    let bytes = container(&blocks, &[0]);
    let table = influences::prepare(&bytes, "identity", 3, Default::default()).unwrap();
    let request = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    assert!(
        table
            .evaluate(
                &bytes,
                "identity",
                Request {
                    geometry: 2,
                    ..request
                },
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("table geometry differs")
    );
    let mut changed = blocks.clone();
    changed[4].1[16..20].copy_from_slice(&2u32.to_le_bytes());
    assert!(
        table
            .evaluate(
                &container(&changed, &[0]),
                "changed instance",
                request,
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("table source SHA256 differs")
    );
    changed = blocks;
    changed[2].1 = node(R90, [0., 7., 0.], 2., &[]);
    assert!(
        table
            .evaluate(
                &container(&changed, &[0]),
                "changed local",
                request,
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("table source SHA256 differs")
    );
    let mut copied = bytes.clone();
    copied.fill(0);
    drop(copied);
    assert_eq!(
        table
            .evaluate(&bytes, "another source label", request, Default::default())
            .unwrap()
            .weight_sums,
        [1.75, 1., 1.]
    );
    let mut input_limit = Limits::default();
    input_limit.source.partition.skin.scene.input_bytes = bytes.len() - 1;
    assert!(
        table
            .evaluate(&bytes, "identity", request, input_limit)
            .unwrap_err()
            .to_string()
            .contains("input byte budget")
    );
}

#[test]
fn influence_reconstruction_admits_table_plus_pose_storage_and_preserves_missing_normals() {
    use fallout_data::nif_skin::influences;
    let bytes = container(&influence_fixture(), &[0]);
    let table = influences::prepare(&bytes, "aggregate", 3, Default::default()).unwrap();
    let request = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let result = table
        .evaluate(&bytes, "aggregate", request, Default::default())
        .unwrap();
    let exact = Limits {
        array_bytes: table.usage().output_bytes + result.retained_bytes,
        work_units: result.work_units,
        ..Default::default()
    };
    table.evaluate(&bytes, "aggregate", request, exact).unwrap();
    assert!(
        table
            .evaluate(
                &bytes,
                "aggregate",
                request,
                Limits {
                    array_bytes: exact.array_bytes - 1,
                    ..exact
                }
            )
            .unwrap_err()
            .to_string()
            .contains("array storage budget")
    );
    assert!(
        table
            .evaluate(
                &bytes,
                "aggregate",
                request,
                Limits {
                    work_units: exact.work_units - 1,
                    ..exact
                }
            )
            .unwrap_err()
            .to_string()
            .contains("work budget")
    );
    assert!(
        table
            .evaluate(
                &bytes,
                "aggregate",
                request,
                Limits {
                    array_bytes: table.usage().output_bytes - 1,
                    ..exact
                }
            )
            .unwrap_err()
            .to_string()
            .contains("table plus pose")
    );
    let mut blocks = influence_fixture();
    blocks[6].1[47] = 0;
    blocks[6].1.drain(48..84);
    let bytes = container(&blocks, &[0]);
    let table = influences::prepare(&bytes, "no normals", 3, Default::default()).unwrap();
    assert!(
        table
            .evaluate(&bytes, "no normals", request, Default::default())
            .unwrap()
            .normals
            .is_empty()
    );
}

#[test]
fn influence_missing_vertices_invalid_raw_weights_and_bone_identity_refuse() {
    use fallout_data::nif_skin::influences;
    let mut blocks = fixture();
    for (weights, error) in [
        ([vec![(0, 1.)], vec![(2, 1.)]], "vertex 1 has no positive"),
        (
            [vec![(0, -0.25), (1, 1.)], vec![(0, 1.25), (2, 1.)]],
            "negative or nonfinite",
        ),
        ([vec![(0, f32::NAN), (1, 1.)], vec![(2, 1.)]], "nonfinite"),
        (
            [vec![(3, 1.), (1, 1.)], vec![(2, 1.)]],
            "skin vertex index exceeds",
        ),
    ] {
        blocks[5].1 = skin(&weights, None);
        let result = influences::prepare(
            &container(&blocks, &[0]),
            "invalid influences",
            3,
            Default::default(),
        );
        assert!(result.unwrap_err().to_string().contains(error));
    }
    blocks = fixture();
    blocks[4].1[16..20].copy_from_slice(&NULL.to_le_bytes());
    assert!(
        influences::prepare(
            &container(&blocks, &[0]),
            "missing bone",
            3,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("unresolved root, bone")
    );
    blocks = fixture();
    blocks[4].1[16..20].copy_from_slice(&999u32.to_le_bytes());
    assert!(
        influences::prepare(
            &container(&blocks, &[0]),
            "outside bone",
            3,
            Default::default()
        )
        .is_err()
    );
    assert!(
        influences::prepare(
            &container(&fixture(), &[0]),
            "wrong geometry",
            2,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("no decoded skin owner")
    );
    blocks = fixture();
    blocks[6].1[8] = 0;
    blocks[6].1.drain(9..45);
    assert!(
        influences::prepare(
            &container(&blocks, &[0]),
            "missing positions",
            3,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("vertex positions unavailable")
    );
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

fn sampled_fixture(object: u32) -> Vec<(&'static str, Vec<u8>)> {
    let mut blocks = fixture();
    blocks[object as usize].1[8..12].copy_from_slice(&7u32.to_le_bytes());
    let mut controller = Vec::new();
    words(&mut controller, &[NULL]);
    shorts(&mut controller, &[0xffff]);
    floats(&mut controller, &[17., -9., 100., 101.]);
    words(&mut controller, &[object, 8]);
    let mut interpolator = Vec::new();
    floats(
        &mut interpolator,
        &[1000., 2000., 3000., 2., -3., 4., -5., 12.],
    );
    words(&mut interpolator, &[9]);
    let mut keys = Vec::new();
    words(&mut keys, &[0, 2, 1]);
    floats(&mut keys, &[0., 0., 3., 0., 2., 2., 5., 4.]);
    words(&mut keys, &[2, 1]);
    floats(&mut keys, &[0., 2., 2., 4.]);
    blocks.extend([
        ("NiTransformController", controller),
        ("NiTransformInterpolator", interpolator),
        ("NiTransformData", keys),
    ]);
    blocks
}

fn sample_request(bytes: &[u8]) -> pose::SampledRequest {
    use sha2::{Digest, Sha256};
    pose::SampledRequest {
        expected_source_sha256: Sha256::digest(bytes).into(),
        skin: request(),
        controller_policy: pose::ControllerPolicy::RefuseOtherRequired,
    }
}

fn animation_request(object: u32, time: f64) -> fallout_data::nif_animation::pose::Request {
    fallout_data::nif_animation::pose::Request {
        object,
        controller: 7,
        source_time: time,
    }
}

#[test]
fn source_linked_child_sample_changes_one_palette_and_weighted_vertices() {
    let bytes = container(&sampled_fixture(2), &[0]);
    let evaluated = pose::evaluate_sampled(
        &bytes,
        "sampled child",
        sample_request(&bytes),
        animation_request(2, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap();
    assert_eq!(
        evaluated.skin.palette[0].matrix,
        [[0.5, 0., 0., -2.], [0., 0.5, 0., 0.], [0., 0., 0.5, 0.]]
    );
    assert_eq!(
        evaluated.skin.palette[1].matrix,
        [
            [0.75, 0., 0., -2.5],
            [0., 0.75, 0., -0.25],
            [0., 0., 0.75, 1.]
        ]
    );
    assert_eq!(
        evaluated.skin.positions,
        [
            [-1.6875, 1.1875, 2.8125],
            [0., -0.5, 1.],
            [-4.75, -0.25, 1.75]
        ]
    );
    assert_eq!(
        evaluated.skin.normals,
        [[0., 0.6875, 0.], [0., 0.5, 0.], [0., 0.75, 0.]]
    );
    assert_eq!(evaluated.skin.weight_sums, [1., 1., 1.]);
    assert_eq!(
        evaluated.sample.source_world,
        [[-9., 0., 0., -2.], [0., -9., 0., 35.], [0., 0., 9., 36.]]
    );
    assert_eq!(
        evaluated.skin.skin_to_source_world,
        [[0., -6., 0., 10.], [6., 0., 0., 32.], [0., 0., 6., 30.]]
    );
    assert_eq!(evaluated.sample.requested_time_f64_bits, 1f64.to_bits());
    assert_eq!(
        (
            evaluated.sample.object.block,
            evaluated.sample.controller.block,
            evaluated.sample.interpolator.block,
            evaluated.sample.data.block
        ),
        (2, 7, 8, 9)
    );
    assert_eq!(evaluated.sample.source_sha256, evaluated.skin.source_sha256);
    assert!(evaluated.skin.unapplied_controllers.is_empty());
    assert!(!evaluated.retail_behavior_verified);
    assert!(!evaluated.skin.retail_behavior_verified);
}

#[test]
fn source_linked_sample_uses_exact_endpoints_without_mutating_stored_local_api() {
    let bytes = container(&sampled_fixture(2), &[0]);
    let before = pose::evaluate(&bytes, "stored", request(), Limits::default()).unwrap();
    let first = pose::evaluate_sampled(
        &bytes,
        "first",
        sample_request(&bytes),
        animation_request(2, 0.),
        pose::CombinedLimits::default(),
    )
    .unwrap();
    assert_eq!(first.skin.positions, before.positions);
    let last = pose::evaluate_sampled(
        &bytes,
        "last",
        sample_request(&bytes),
        animation_request(2, 2.),
        pose::CombinedLimits::default(),
    )
    .unwrap();
    assert_eq!(
        last.skin.positions,
        [[-1.875, 1.375, 4.125], [0., -0.5, 1.], [-6., -0.5, 3.]]
    );
    let after = pose::evaluate(&bytes, "stored", request(), Limits::default()).unwrap();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap()
    );
    assert_eq!(before.unapplied_controllers[0].object, 2);
}

#[test]
fn sampled_root_moves_display_frame_once_without_changing_relative_palettes() {
    let bytes = container(&sampled_fixture(0), &[0]);
    let evaluated = pose::evaluate_sampled(
        &bytes,
        "root",
        sample_request(&bytes),
        animation_request(0, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap();
    assert_eq!(
        evaluated.skin.positions,
        [[-1.5, 1., 1.5], [0., -0.5, 1.], [-3.5, 0., 0.5]]
    );
    assert_eq!(
        evaluated.skin.skin_to_source_world,
        [[0., -6., 0., 1.], [6., 0., 0., 16.], [0., 0., 6., 2.]]
    );
}

#[test]
fn sampled_ancestor_above_skin_root_changes_root_frame_in_exact_order() {
    let mut blocks = sampled_fixture(0);
    blocks[0].1[8..12].copy_from_slice(&NULL.to_le_bytes());
    let mut ancestor = node(RM90, [99., 98., 97.], 2., &[0]);
    ancestor[8..12].copy_from_slice(&7u32.to_le_bytes());
    blocks.push(("NiNode", ancestor));
    blocks[7].1[22..26].copy_from_slice(&10u32.to_le_bytes());
    let bytes = container(&blocks, &[10]);
    let evaluated = pose::evaluate_sampled(
        &bytes,
        "ancestor",
        sample_request(&bytes),
        animation_request(10, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap();
    assert_eq!(
        evaluated.skin.positions,
        [[-1.5, 1., 1.5], [0., -0.5, 1.], [-3.5, 0., 0.5]]
    );
    assert_eq!(
        evaluated.skin.skin_to_source_world,
        [[18., 0., 0., 97.], [0., 18., 0., -26.], [0., 0., 18., 92.]]
    );
}

#[test]
fn sampled_skin_refuses_wrong_sha_links_time_rotation_and_other_required_controller() {
    let bytes = container(&sampled_fixture(2), &[0]);
    let mut stale = sample_request(&bytes);
    stale.expected_source_sha256[0] ^= 1;
    let error = pose::evaluate_sampled(
        &bytes,
        "stale",
        stale,
        animation_request(2, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("source SHA256 differs"));
    for (object, time, expected) in [
        (1, 1., "object.controller differs"),
        (2, -1., "extrapolate"),
        (2, f64::NAN, "source time must be finite"),
    ] {
        let error = pose::evaluate_sampled(
            &bytes,
            "bad request",
            sample_request(&bytes),
            animation_request(object, time),
            pose::CombinedLimits::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut blocks = sampled_fixture(2);
    blocks[3].1[8..12].copy_from_slice(&7u32.to_le_bytes());
    let bytes = container(&blocks, &[0]);
    let error = pose::evaluate_sampled(
        &bytes,
        "other controller",
        sample_request(&bytes),
        animation_request(2, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("required object 3 controller 7 is unapplied"),
        "{error}"
    );
    blocks = sampled_fixture(2);
    let mut rotation = Vec::new();
    words(&mut rotation, &[1, 1]);
    floats(&mut rotation, &[0., 1., 0., 0., 0.]);
    rotation.extend_from_slice(&blocks[9].1[4..]);
    blocks[9].1 = rotation;
    let bytes = container(&blocks, &[0]);
    let error = pose::evaluate_sampled(
        &bytes,
        "rotation",
        sample_request(&bytes),
        animation_request(2, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("rotation key mapping is unapplied"),
        "{error}"
    );
}

#[test]
fn valid_sample_outside_selected_skin_paths_and_missing_bone_refuse() {
    let mut blocks = sampled_fixture(2);
    blocks[2].1[8..12].copy_from_slice(&NULL.to_le_bytes());
    let mut unrelated = node(ID, [0.; 3], 1., &[]);
    unrelated[8..12].copy_from_slice(&7u32.to_le_bytes());
    blocks.push(("NiNode", unrelated));
    blocks[0].1 = node(R90, [10., 20., 30.], 3., &[1, 3, 10]);
    blocks[7].1[22..26].copy_from_slice(&10u32.to_le_bytes());
    let bytes = container(&blocks, &[0]);
    let error = pose::evaluate_sampled(
        &bytes,
        "unrelated",
        sample_request(&bytes),
        animation_request(10, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("node 10 is outside selected skin root/bone ancestry"),
        "{error}"
    );
    blocks = sampled_fixture(2);
    blocks[4].1[16..20].copy_from_slice(&NULL.to_le_bytes());
    let bytes = container(&blocks, &[0]);
    let error = pose::evaluate_sampled(
        &bytes,
        "missing bone",
        sample_request(&bytes),
        animation_request(2, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unresolved root, bone or owner ancestry"),
        "{error}"
    );
}

#[test]
fn combined_sampled_storage_work_and_decoder_admission_have_exact_ceilings() {
    let bytes = container(&sampled_fixture(2), &[0]);
    let baseline = pose::evaluate_sampled(
        &bytes,
        "bounds",
        sample_request(&bytes),
        animation_request(2, 1.),
        pose::CombinedLimits::default(),
    )
    .unwrap();
    let exact = pose::CombinedLimits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    assert!(
        pose::evaluate_sampled(
            &bytes,
            "exact",
            sample_request(&bytes),
            animation_request(2, 1.),
            exact
        )
        .is_ok()
    );
    for limits in [
        pose::CombinedLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::CombinedLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::CombinedLimits {
            decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
            ..exact
        },
        pose::CombinedLimits {
            decoder_check_admission_units: exact.decoder_check_admission_units - 1,
            ..exact
        },
    ] {
        assert!(
            pose::evaluate_sampled(
                &bytes,
                "one over",
                sample_request(&bytes),
                animation_request(2, 1.),
                limits
            )
            .is_err()
        );
    }
    let mut overflow = exact;
    overflow.animation.keys.max_combined_retained_bytes = usize::MAX;
    assert!(
        pose::evaluate_sampled(
            &bytes,
            "overflow",
            sample_request(&bytes),
            animation_request(2, 1.),
            overflow
        )
        .unwrap_err()
        .to_string()
        .contains("decoder array admission exceeded")
    );
}
