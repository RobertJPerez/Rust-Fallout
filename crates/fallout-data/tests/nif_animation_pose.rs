//! Original authored source packets and analytic expectations; no retail timing.
use fallout_data::nif_animation::{
    pose::{self, Limits, ObjectPose, Request},
    sampling,
};
use sha2::{Digest, Sha256};

const NULL: u32 = u32::MAX;
const ID: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const R90: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
type Blocks = Vec<(&'static str, Vec<u8>)>;
fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        words(out, &[value.to_bits()]);
    }
}
fn set(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn node(
    controller: u32,
    children: &[u32],
    translation: [f32; 3],
    rotation: [[f32; 3]; 3],
    scale: f32,
) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL, 0, controller, 0xDEAD_BEEF]);
    floats(&mut out, &translation);
    for row in rotation {
        floats(&mut out, &row);
    }
    floats(&mut out, &[scale]);
    words(&mut out, &[0, NULL, children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn controller(target: u32, interpolator: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL]);
    out.extend(0xFFFFu16.to_le_bytes());
    // Clocks deliberately disagree with the requested key domain.
    floats(&mut out, &[17., -9., 100., 101.]);
    words(&mut out, &[target, interpolator]);
    out
}
fn interpolator(data: u32) -> Vec<u8> {
    let mut out = Vec::new();
    // Different constants and a nonunit raw WXYZ quaternion must stay unapplied.
    floats(&mut out, &[1000., 2000., 3000., 2., -3., 4., -5., 12.]);
    words(&mut out, &[data]);
    out
}
fn keys(tag: u32, present: bool) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0]); // rotation count, with absent tag
    if present {
        words(&mut out, &[2, tag]);
        floats(&mut out, &[0., 0., 2., 4., 2., 10., 4., 8.]);
        words(&mut out, &[2, tag]);
        floats(&mut out, &[0., 1., 2., 3.]);
    } else {
        words(&mut out, &[0, 0]);
    }
    out
}
fn blocks() -> Blocks {
    vec![
        ("NiNode", node(NULL, &[1], [10., 20., 30.], ID, 3.)),
        ("NiNode", node(2, &[], [99., 98., 97.], R90, 7.)),
        ("NiTransformController", controller(1, 3)),
        ("NiTransformInterpolator", interpolator(4)),
        ("NiTransformData", keys(1, true)),
    ]
}
fn container(blocks: &Blocks) -> Vec<u8> {
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
    out.extend((types.len() as u16).to_le_bytes());
    for name in &types {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        out.extend((types.iter().position(|n| n == name).unwrap() as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        words(&mut out, &[payload.len() as u32]);
    }
    words(&mut out, &[0, 0, 0]); // strings, max string, groups
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[1, 0]); // exact root 0
    out
}
fn request(time: f64) -> Request {
    Request {
        object: 1,
        controller: 2,
        source_time: time,
    }
}
fn evaluate(blocks: &Blocks, time: f64) -> ObjectPose {
    pose::evaluate(
        &container(blocks),
        "authored linked pose",
        request(time),
        Default::default(),
    )
    .unwrap()
}
fn failure(blocks: &Blocks, selected: Request, limits: Limits, reason: &str) {
    let bytes = container(blocks);
    let before = bytes.clone();
    let error = pose::evaluate(&bytes, "authored linked pose", selected, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason}: {error}");
    assert_eq!(bytes, before);
}

#[test]
fn linked_linear_pose_has_independent_local_and_parent_world_expectations() {
    let blocks = blocks();
    let bytes = container(&blocks);
    let pose = evaluate(&blocks, 1.);
    assert_eq!(
        pose.local,
        [[0., -2., 0., 5.], [2., 0., 0., 3.], [0., 0., 2., 6.]]
    );
    assert_eq!(
        pose.source_world,
        [[0., -6., 0., 25.], [6., 0., 0., 29.], [0., 0., 6., 48.]]
    );
    assert_eq!(pose.source_sha256, format!("{:x}", Sha256::digest(&bytes)));
    assert_eq!(
        [
            pose.object.block,
            pose.controller.block,
            pose.interpolator.block,
            pose.data.block
        ],
        [1, 2, 3, 4]
    );
    for source in [
        &pose.object,
        &pose.controller,
        &pose.interpolator,
        &pose.data,
        &pose.static_ancestors[0].source,
    ] {
        assert_eq!(
            source.sha256,
            format!(
                "{:x}",
                Sha256::digest(&bytes[source.offset..source.offset + source.bytes])
            )
        );
        assert_eq!(
            &bytes[source.offset..source.offset + source.bytes],
            blocks[source.block as usize].1
        );
    }
    assert_eq!(pose.static_ancestors.len(), 1);
    assert_eq!(pose.static_ancestors[0].source.block, 0);
    assert_eq!(pose.static_ancestors[0].parent, None);
    assert_eq!(pose.source_local.scale_bits, 7f32.to_bits());
    assert_eq!(pose.object_flags, 0xDEAD_BEEF);
    assert_eq!(pose.unapplied_controller_fields.flags, 0xFFFF);
    assert_eq!(
        pose.unapplied_controller_fields.frequency_bits,
        17f32.to_bits()
    );
    assert_eq!(
        pose.unapplied_interpolator_fields.rotation_wxyz_bits,
        [2f32, -3., 4., -5.].map(f32::to_bits)
    );
    assert!(!pose.retail_behavior_verified);
    assert!(!pose.translation.runtime_ready && !pose.scale.runtime_ready);
    assert_eq!(pose.contract, pose::CONTRACT);
}

#[test]
fn absent_groups_retain_av_locals_instead_of_interpolator_constants() {
    let mut blocks = blocks();
    blocks[4].1 = keys(1, false);
    let pose = evaluate(&blocks, 10000.);
    assert_eq!(
        pose.local,
        [[0., -7., 0., 99.], [7., 0., 0., 98.], [0., 0., 7., 97.]]
    );
    assert_eq!(
        pose.source_world,
        [
            [0., -21., 0., 307.],
            [21., 0., 0., 314.],
            [0., 0., 21., 321.]
        ]
    );
    assert!(matches!(
        pose.translation.evaluation,
        sampling::Evaluated::Translation { sample: None }
    ));
    assert!(matches!(
        pose.scale.evaluation,
        sampling::Evaluated::Scale { sample: None }
    ));
    assert_eq!(
        pose.unapplied_interpolator_fields.translation_bits,
        [1000f32, 2000., 3000.].map(f32::to_bits)
    );
}

#[test]
fn constant_keys_hold_authored_components_and_exact_endpoint_uses_last_key() {
    let mut blocks = blocks();
    blocks[4].1 = keys(5, true);
    assert_eq!(
        evaluate(&blocks, 1.).source_world,
        [[0., -3., 0., 10.], [3., 0., 0., 26.], [0., 0., 3., 42.]]
    );
    assert_eq!(
        evaluate(&blocks, 2.).source_world,
        [[0., -9., 0., 40.], [9., 0., 0., 32.], [0., 0., 9., 54.]]
    );
}

#[test]
fn source_time_is_direct_finite_no_extrapolation_and_signed_zero_is_recorded() {
    let blocks = blocks();
    for (time, reason) in [
        (f64::NAN, "must be finite"),
        (f64::INFINITY, "must be finite"),
        (-1., "extrapolate"),
        (3., "extrapolate"),
    ] {
        failure(&blocks, request(time), Default::default(), reason);
    }
    assert_eq!(
        evaluate(&blocks, -0.).requested_time_f64_bits,
        (-0f64).to_bits()
    );
    assert_eq!(
        evaluate(&blocks, 0.).local,
        [[0., -1., 0., 0.], [1., 0., 0., 2.], [0., 0., 1., 4.]]
    );
    assert_eq!(
        evaluate(&blocks, 2.).local,
        [[0., -3., 0., 10.], [3., 0., 0., 4.], [0., 0., 3., 8.]]
    );
}

#[test]
fn sub_binary32_time_resolution_survives_into_f64_matrix_components() {
    let delta = 2f64.powi(-40);
    let pose = evaluate(&blocks(), 1. + delta);
    assert_eq!(
        pose.local,
        [
            [0., -(2. + delta), 0., 5. + 5. * delta],
            [2. + delta, 0., 0., 3. + delta],
            [0., 0., 2. + delta, 6. + 2. * delta]
        ]
    );
}

#[test]
fn exact_controller_target_and_missing_links_never_fall_back_to_decoys() {
    let original = blocks();
    failure(
        &original,
        Request {
            controller: 0,
            ..request(1.)
        },
        Default::default(),
        "object.controller differs",
    );
    for target in [0, NULL] {
        let mut blocks = original.clone();
        set(&mut blocks[2].1, 22, target);
        failure(
            &blocks,
            request(1.),
            Default::default(),
            "controller.target differs",
        );
    }
    let mut blocks = original.clone();
    set(&mut blocks[2].1, 26, NULL);
    blocks.extend([
        ("NiTransformController", controller(1, 6)),
        ("NiTransformInterpolator", interpolator(7)),
        ("NiTransformData", keys(1, true)),
    ]);
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "missing transform interpolator",
    );
    let mut blocks = original.clone();
    set(&mut blocks[3].1, 32, NULL);
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "missing transform key data",
    );
    failure(
        &original,
        Request {
            object: u32::MAX,
            ..request(1.)
        },
        Default::default(),
        "object is not decoded",
    );
}

#[test]
fn controller_chains_and_controlled_ancestors_are_explicit_refusals() {
    let mut blocks = blocks();
    set(&mut blocks[2].1, 0, 2);
    failure(&blocks, request(1.), Default::default(), "controller chain");
    set(&mut blocks[2].1, 0, NULL);
    set(&mut blocks[0].1, 8, 2);
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "ancestor 0 controller is unapplied",
    );
}

#[test]
fn unknown_scene_edges_and_orphan_selected_objects_cannot_certify_ancestry() {
    let original = blocks();
    let mut blocks = original.clone();
    blocks[0].1 = node(NULL, &[1, 5], [10., 20., 30.], ID, 3.);
    blocks.push(("UnknownNode", vec![]));
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "unresolved scene ancestry",
    );
    let mut blocks = original;
    blocks[0].1 = node(NULL, &[], [10., 20., 30.], ID, 3.);
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "not reachable from footer",
    );
}

#[test]
fn rotation_keys_refuse_instead_of_quaternion_normalization_or_angle_guess() {
    let mut blocks = blocks();
    let mut data = Vec::new();
    words(&mut data, &[1, 1]);
    floats(&mut data, &[0., 2., 3., 4., 5.]);
    words(&mut data, &[0, 0]);
    blocks[4].1 = data;
    failure(
        &blocks,
        request(0.),
        Default::default(),
        "rotation key mapping is unapplied",
    );
}

#[test]
fn budgets_at_exact_storage_work_sampling_depth_and_one_under() {
    let blocks = blocks();
    let pose = evaluate(&blocks, 1.);
    let exact = Limits {
        array_bytes: pose.retained_bytes,
        work_units: pose.work_units,
        ancestry_depth: 2,
        sampling: sampling::Limits {
            validation_work: pose.sample_work.validation_units,
            sampling_work: pose.sample_work.sampling_units,
        },
        ..Default::default()
    };
    let repeated =
        pose::evaluate(&container(&blocks), "exact budgets", request(1.), exact).unwrap();
    assert_eq!(repeated.local, pose.local);
    failure(
        &blocks,
        request(1.),
        Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        "array storage budget",
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            work_units: exact.work_units - 1,
            ..exact
        },
        "work budget",
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            ancestry_depth: 1,
            ..exact
        },
        "ancestry depth budget",
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            sampling: sampling::Limits {
                validation_work: exact.sampling.validation_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        "validation-work budget",
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            sampling: sampling::Limits {
                sampling_work: exact.sampling.sampling_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        "request-work budget",
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            ancestry_depth: 0,
            ..exact
        },
        "ancestry depth budget",
    );
}

#[test]
fn source_reflection_and_parent_rotation_are_composed_without_repair() {
    let mut blocks = blocks();
    blocks[0].1 = node(NULL, &[1], [1., 2., 3.], R90, -2.);
    assert_eq!(
        evaluate(&blocks, 1.).source_world,
        [[4., 0., 0., 7.], [0., 4., 0., -8.], [0., 0., -4., -9.]]
    );
}

#[test]
fn selected_footer_root_uses_evaluated_local_once_without_static_root_matrix() {
    let mut blocks = blocks();
    set(&mut blocks[0].1, 8, 2);
    set(&mut blocks[1].1, 8, NULL);
    set(&mut blocks[2].1, 22, 0);
    let pose = pose::evaluate(
        &container(&blocks),
        "selected root",
        Request {
            object: 0,
            ..request(1.)
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        pose.source_world,
        [[2., 0., 0., 5.], [0., 2., 0., 3.], [0., 0., 2., 6.]]
    );
    assert_eq!(pose.source_world, pose.local);
    assert!(pose.static_ancestors.is_empty());
}
