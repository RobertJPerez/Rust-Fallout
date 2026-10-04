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
fn prepared_source_reuses_admitted_catalogues_and_matches_one_shot_observations() {
    let bytes = container(&blocks());
    let original = bytes.clone();
    let prepared = pose::PreparedSource::prepare(&bytes, "prepared", Default::default()).unwrap();
    let usage = prepared.usage();
    assert_eq!(
        (
            usage.animation_key_decodes,
            usage.scene_decodes,
            usage.map_constructions,
            usage.source_sha256_computations
        ),
        (1, 1, 1, 1)
    );
    for time in [2., 0., 1., 1.] {
        let sampled = prepared.sample(request(time), Default::default()).unwrap();
        let one = pose::evaluate(&bytes, "prepared", request(time), Default::default()).unwrap();
        let mut sampled_json = serde_json::to_value(&sampled).unwrap();
        let mut one_json = serde_json::to_value(&one).unwrap();
        // Preparation holds the maps/source scans separately; numeric/source
        // observations and sampler usage are otherwise the same exact receipt.
        for field in ["retained_bytes", "work_units"] {
            sampled_json.as_object_mut().unwrap().remove(field);
            one_json.as_object_mut().unwrap().remove(field);
        }
        assert_eq!(sampled_json, one_json);
        assert!(sampled.retained_bytes < one.retained_bytes);
        assert!(sampled.work_units < one.work_units);
    }
    assert_eq!(
        serde_json::to_value(prepared.usage()).unwrap(),
        serde_json::to_value(usage).unwrap()
    );
    assert_eq!(bytes, original);
}

#[test]
fn prepared_source_owns_receipts_after_caller_bytes_are_changed_and_dropped() {
    let mut bytes = container(&blocks());
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let prepared = pose::PreparedSource::prepare(&bytes, "owned", Default::default()).unwrap();
    bytes.fill(0);
    drop(bytes);
    let sampled = prepared.sample(request(1.), Default::default()).unwrap();
    assert_eq!(sampled.source_sha256, hash);
    assert_eq!(prepared.source_sha256(), hash);
    assert_eq!(
        sampled.local,
        [[0., -2., 0., 5.], [2., 0., 0., 3.], [0., 0., 2., 6.]]
    );
    assert_eq!(
        sampled.source_world,
        [[0., -6., 0., 25.], [6., 0., 0., 29.], [0., 0., 6., 48.]]
    );
}

#[test]
fn prepared_admission_and_sample_output_have_separate_exact_ceilings() {
    let bytes = container(&blocks());
    let prepared = pose::PreparedSource::prepare(&bytes, "bounds", Default::default()).unwrap();
    let preparation = Limits {
        array_bytes: prepared.usage().extra_retained_bytes,
        work_units: prepared.usage().work_units,
        ..Default::default()
    };
    assert!(pose::PreparedSource::prepare(&bytes, "bounds", preparation).is_ok());
    for limits in [
        Limits {
            array_bytes: preparation.array_bytes - 1,
            ..preparation
        },
        Limits {
            work_units: preparation.work_units - 1,
            ..preparation
        },
    ] {
        assert!(pose::PreparedSource::prepare(&bytes, "bounds", limits).is_err());
    }
    let sample = prepared.sample(request(1.), Default::default()).unwrap();
    let exact = pose::SampleLimits {
        array_bytes: sample.retained_bytes,
        work_units: sample.work_units,
        ..Default::default()
    };
    assert!(prepared.sample(request(1.), exact).is_ok());
    for limits in [
        pose::SampleLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::SampleLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::SampleLimits {
            ancestry_depth: 1,
            ..exact
        },
    ] {
        assert!(prepared.sample(request(1.), limits).is_err());
    }
}

#[test]
fn prepared_exact_source_links_and_unsupported_channels_match_one_shot_refusal() {
    let bytes = container(&blocks());
    let prepared =
        pose::PreparedSource::prepare(&bytes, "same refusal", Default::default()).unwrap();
    for selected in [
        Request {
            object: 0,
            ..request(1.)
        },
        Request {
            controller: 3,
            ..request(1.)
        },
        request(-1.),
        request(f64::NAN),
    ] {
        let one = pose::evaluate(&bytes, "same refusal", selected, Default::default()).unwrap_err();
        let sampled = prepared.sample(selected, Default::default()).unwrap_err();
        assert_eq!(one.to_string(), sampled.to_string());
    }
    let mut source = blocks();
    set(&mut source[2].1, 0, 2);
    let bytes = container(&source);
    let prepared = pose::PreparedSource::prepare(&bytes, "chain", Default::default()).unwrap();
    assert_eq!(
        pose::evaluate(&bytes, "chain", request(1.), Default::default())
            .unwrap_err()
            .to_string(),
        prepared
            .sample(request(1.), Default::default())
            .unwrap_err()
            .to_string()
    );
}

#[test]
fn prepared_batch_preserves_decreasing_repeat_order_and_discards_later_failure() {
    let bytes = container(&blocks());
    let prepared = pose::PreparedSource::prepare(&bytes, "batch", Default::default()).unwrap();
    let selected = [request(2.), request(0.), request(1.), request(1.)];
    let batch = prepared.sample_many(&selected, Default::default()).unwrap();
    assert_eq!(
        batch
            .samples
            .iter()
            .map(|p| p.requested_time_f64_bits)
            .collect::<Vec<_>>(),
        [
            2f64.to_bits(),
            0f64.to_bits(),
            1f64.to_bits(),
            1f64.to_bits()
        ]
    );
    assert_eq!(
        batch.samples[0].local,
        [[0., -3., 0., 10.], [3., 0., 0., 4.], [0., 0., 3., 8.]]
    );
    assert_eq!(
        batch.samples[1].source_world,
        [[0., -3., 0., 10.], [3., 0., 0., 26.], [0., 0., 3., 42.]]
    );
    let failed = prepared
        .sample_many(&[request(0.), request(1.), request(3.)], Default::default())
        .unwrap_err();
    assert!(failed.to_string().contains("batch request 2"), "{failed}");
    assert!(failed.to_string().contains("extrapolate"), "{failed}");
    assert_eq!(
        prepared
            .sample_many(&[], Default::default())
            .unwrap()
            .samples
            .len(),
        0
    );
    assert_eq!(prepared.usage().animation_key_decodes, 1);
}

#[test]
fn aggregate_batch_output_traversal_and_sampler_limits_have_exact_ceilings() {
    let bytes = container(&blocks());
    let prepared =
        pose::PreparedSource::prepare(&bytes, "batch bounds", Default::default()).unwrap();
    let selected = [request(0.), request(1.), request(2.)];
    let batch = prepared.sample_many(&selected, Default::default()).unwrap();
    let exact = pose::BatchLimits {
        samples: 3,
        array_bytes: batch.retained_bytes,
        work_units: batch.work_units,
        sampling: sampling::Limits {
            validation_work: batch.sample_work.validation_units,
            sampling_work: batch.sample_work.sampling_units,
        },
        ..Default::default()
    };
    assert!(prepared.sample_many(&selected, exact).is_ok());
    for limits in [
        pose::BatchLimits {
            samples: 2,
            ..exact
        },
        pose::BatchLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::BatchLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::BatchLimits {
            sampling: sampling::Limits {
                validation_work: exact.sampling.validation_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        pose::BatchLimits {
            sampling: sampling::Limits {
                sampling_work: exact.sampling.sampling_work - 1,
                ..exact.sampling
            },
            ..exact
        },
    ] {
        assert!(prepared.sample_many(&selected, limits).is_err());
    }
}

fn set_fixture() -> Blocks {
    let mut source = blocks();
    source[0].1 = node(NULL, &[1], [10., 20., 30.], R90, 3.);
    source[1].1 = node(2, &[5], [99., 98., 97.], R90, 7.);
    let mut child_keys = words_vec(&[0, 2, 1]);
    floats(&mut child_keys, &[0., 1., 0., 2., 2., 3., 2., 4.]);
    words(&mut child_keys, &[2, 1]);
    floats(&mut child_keys, &[0., 2., 2., 4.]);
    source.extend([
        (
            "NiNode",
            node(
                6,
                &[],
                [44., 45., 46.],
                [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]],
                9.,
            ),
        ),
        ("NiTransformController", controller(5, 7)),
        ("NiTransformInterpolator", interpolator(8)),
        ("NiTransformData", child_keys),
    ]);
    source
}
fn words_vec(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    words(&mut bytes, values);
    bytes
}
fn set_requests() -> [Request; 2] {
    [
        request(1.),
        Request {
            object: 5,
            controller: 6,
            source_time: 1.,
        },
    ]
}

fn set_bindings(requests: &[Request]) -> Vec<pose::ChannelBinding> {
    requests
        .iter()
        .map(|r| pose::ChannelBinding {
            object: r.object,
            controller: r.controller,
        })
        .collect()
}
fn set_times(
    bytes: &[u8],
    requests: &[Request],
    parent: f64,
    child: f64,
) -> Vec<pose::ExplicitChannelTime> {
    let expected_source_sha256 = Sha256::digest(bytes).into();
    requests
        .iter()
        .enumerate()
        .map(|(i, r)| pose::ExplicitChannelTime {
            expected_source_sha256,
            object: r.object,
            controller: r.controller,
            source_time: if i == 0 { parent } else { child },
        })
        .collect()
}

#[test]
fn prepared_set_compiles_higher_parent_once_and_samples_literal_time_vectors() {
    let (mut bytes, selected) = higher_parent_set_fixture();
    let original = bytes.clone();
    let prepared = pose::PreparedSource::prepare(&bytes, "cached", Default::default()).unwrap();
    bytes.fill(0); // The sealed source owns its data; caller bytes are irrelevant.
    drop(bytes);
    let bindings = set_bindings(&selected);
    let plan = prepared.prepare_set(&bindings, Default::default()).unwrap();
    let reverse = prepared
        .prepare_set(&[bindings[1], bindings[0]], Default::default())
        .unwrap();
    let usage = plan.usage();
    let expected = [
        (
            0.,
            0.,
            [[-3., 0., 0., 4.], [0., -3., 0., 20.], [0., 0., 3., 42.]],
            [[0., -6., 0., 1.], [6., 0., 0., 20.], [0., 0., 6., 48.]],
        ),
        (
            1.,
            1.,
            [[-6., 0., 0., 1.], [0., -6., 0., 35.], [0., 0., 6., 48.]],
            [[0., -18., 0., -11.], [18., 0., 0., 29.], [0., 0., 18., 66.]],
        ),
        (
            2.,
            2.,
            [[-9., 0., 0., -2.], [0., -9., 0., 50.], [0., 0., 9., 54.]],
            [[0., -36., 0., -29.], [36., 0., 0., 32.], [0., 0., 36., 90.]],
        ),
        (
            2.,
            0.,
            [[-9., 0., 0., -2.], [0., -9., 0., 50.], [0., 0., 9., 54.]],
            [[0., -18., 0., -11.], [18., 0., 0., 50.], [0., 0., 18., 72.]],
        ),
    ];
    for (parent, child, parent_world, child_world) in expected {
        let mut times = set_times(&original, &selected, parent, child);
        times.reverse(); // Time rows identify objects, rather than vector position.
        let result = plan.sample(&times, Default::default()).unwrap();
        assert_eq!(result.objects[0].source_world, parent_world);
        assert_eq!(result.objects[1].source_world, child_world);
        assert_eq!(result.propagated_objects, 3);
        let requests = [
            Request {
                source_time: parent,
                ..selected[0]
            },
            Request {
                source_time: child,
                ..selected[1]
            },
        ];
        let one_shot =
            pose::evaluate_set(&original, "cached", &requests, Default::default()).unwrap();
        assert_eq!(
            serde_json::to_value(&result.objects).unwrap(),
            serde_json::to_value(&one_shot.objects).unwrap()
        );
        let permuted = reverse.sample(&times, Default::default()).unwrap();
        assert_eq!(
            serde_json::to_value(&result.objects[0]).unwrap(),
            serde_json::to_value(&permuted.objects[1]).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&result.objects[1]).unwrap(),
            serde_json::to_value(&permuted.objects[0]).unwrap()
        );
        assert_eq!(
            (result.retained_bytes, result.work_units),
            (permuted.retained_bytes, permuted.work_units)
        );
        assert!(!result.retail_behavior_verified);
    }
    assert_eq!(usage.required_forest_constructions, 1);
    assert_eq!(usage.source_preparation.animation_key_decodes, 1);
    assert_eq!(usage.source_preparation.scene_decodes, 1);
    assert_eq!(usage.source_preparation.source_sha256_computations, 1);
    assert_eq!(usage.source_preparation.block_sha256_computations, 9);
    assert_eq!(
        serde_json::to_value(usage).unwrap(),
        serde_json::to_value(plan.usage()).unwrap()
    );
    assert_eq!(plan.source_sha256(), prepared.source_sha256());
}

#[test]
fn prepared_set_rejects_foreign_duplicate_missing_identity_and_time_without_mutation() {
    let (bytes, selected) = higher_parent_set_fixture();
    let prepared =
        pose::PreparedSource::prepare(&bytes, "set refusals", Default::default()).unwrap();
    let bindings = set_bindings(&selected);
    let plan = prepared.prepare_set(&bindings, Default::default()).unwrap();
    let times = set_times(&bytes, &selected, 1., 1.);
    let earlier = plan.sample(&times, Default::default()).unwrap();
    let saved = serde_json::to_value(&earlier).unwrap();
    let usage = serde_json::to_value(plan.usage()).unwrap();
    let mut invalid = Vec::new();
    invalid.push((times[..1].to_vec(), "missing or excess"));
    invalid.push((vec![times[0], times[0]], "duplicate"));
    for (field, reason) in [
        (0, "foreign"),
        (1, "controller differs"),
        (2, "SHA256 differs"),
        (3, "finite"),
        (4, "extrapolate"),
    ] {
        let mut changed = times.clone();
        match field {
            0 => changed[1].object = u32::MAX,
            1 => changed[1].controller = 2,
            2 => changed[1].expected_source_sha256[31] ^= 1,
            3 => changed[1].source_time = f64::NAN,
            _ => changed[1].source_time = 3.,
        }
        invalid.push((changed, reason));
    }
    for (times, reason) in invalid {
        let error = plan
            .sample(&times, Default::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "{error}");
        assert_eq!(serde_json::to_value(&earlier).unwrap(), saved);
        assert_eq!(serde_json::to_value(plan.usage()).unwrap(), usage);
        assert_eq!(
            serde_json::to_value(
                plan.sample(&set_times(&bytes, &selected, 1., 1.), Default::default())
                    .unwrap()
            )
            .unwrap(),
            saved
        );
    }
    for bindings in [
        vec![],
        vec![bindings[1]],
        vec![bindings[0], bindings[0]],
        vec![
            pose::ChannelBinding {
                controller: 6,
                ..bindings[0]
            },
            bindings[1],
        ],
    ] {
        assert!(prepared.prepare_set(&bindings, Default::default()).is_err());
    }
    let mut source = set_fixture();
    source[8].1 = keys(5, true); // A supported constant group remains admitted.
    let supported =
        pose::PreparedSource::prepare(&container(&source), "constant", Default::default()).unwrap();
    assert!(
        supported
            .prepare_set(&set_bindings(&set_requests()), Default::default())
            .is_ok()
    );
    let mut rotation = words_vec(&[1, 1]);
    floats(&mut rotation, &[0., 1., 0., 0., 0.]);
    rotation.extend_from_slice(&source[8].1[4..]);
    source[8].1 = rotation;
    let unsupported =
        pose::PreparedSource::prepare(&container(&source), "rotation", Default::default()).unwrap();
    let error = unsupported
        .prepare_set(&set_bindings(&set_requests()), Default::default())
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("rotation key mapping is unapplied"),
        "{error}"
    );
}

#[test]
fn prepared_set_preparation_sample_live_storage_and_sampler_caps_are_exact() {
    let (bytes, selected) = higher_parent_set_fixture();
    let prepared = pose::PreparedSource::prepare(&bytes, "set limits", Default::default()).unwrap();
    let bindings = set_bindings(&selected);
    let plan = prepared.prepare_set(&bindings, Default::default()).unwrap();
    let used = plan.usage();
    let exact = pose::SetPreparationLimits {
        channels: 2,
        ancestry_depth: 3,
        array_bytes: used.extra_retained_bytes,
        work_units: used.work_units,
        live_array_bytes: used.live_retained_bytes,
        sampling: sampling::Limits {
            validation_work: used.sample_work.validation_units,
            sampling_work: 0,
        },
    };
    assert!(prepared.prepare_set(&bindings, exact).is_ok());
    for limits in [
        pose::SetPreparationLimits {
            channels: 1,
            ..exact
        },
        pose::SetPreparationLimits {
            ancestry_depth: 2,
            ..exact
        },
        pose::SetPreparationLimits {
            ancestry_depth: 0,
            ..exact
        },
        pose::SetPreparationLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::SetPreparationLimits {
            array_bytes: 0,
            ..exact
        },
        pose::SetPreparationLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::SetPreparationLimits {
            live_array_bytes: exact.live_array_bytes - 1,
            ..exact
        },
        pose::SetPreparationLimits {
            sampling: sampling::Limits {
                validation_work: exact.sampling.validation_work - 1,
                sampling_work: 0,
            },
            ..exact
        },
    ] {
        assert!(prepared.prepare_set(&bindings, limits).is_err());
    }
    let times = set_times(&bytes, &selected, 1., 1.);
    let result = plan.sample(&times, Default::default()).unwrap();
    let exact = pose::SetSampleLimits {
        array_bytes: result.retained_bytes,
        work_units: result.work_units,
        live_array_bytes: used.live_retained_bytes,
        sampling: sampling::Limits {
            validation_work: result.sample_work.validation_units,
            sampling_work: result.sample_work.sampling_units,
        },
    };
    assert!(plan.sample(&times, exact).is_ok());
    for limits in [
        pose::SetSampleLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::SetSampleLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::SetSampleLimits {
            live_array_bytes: exact.live_array_bytes - 1,
            ..exact
        },
        pose::SetSampleLimits {
            sampling: sampling::Limits {
                validation_work: exact.sampling.validation_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        pose::SetSampleLimits {
            sampling: sampling::Limits {
                sampling_work: exact.sampling.sampling_work - 1,
                ..exact.sampling
            },
            ..exact
        },
    ] {
        assert!(plan.sample(&times, limits).is_err());
    }
}

#[test]
fn prepared_set_batch_charges_live_plan_once_full_key_scans_and_all_outputs() {
    let (bytes, selected) = higher_parent_set_fixture();
    let prepared = pose::PreparedSource::prepare(&bytes, "set batch", Default::default()).unwrap();
    let plan = prepared
        .prepare_set(&set_bindings(&selected), Default::default())
        .unwrap();
    let vectors = vec![
        set_times(&bytes, &selected, 2., 0.),
        set_times(&bytes, &selected, 1., 2.),
        set_times(&bytes, &selected, 1., 2.),
        set_times(&bytes, &selected, -0., 0.),
    ];
    let batch = plan.sample_many(&vectors, Default::default()).unwrap();
    assert_eq!(
        batch.samples[3].objects[0].channel.requested_time_f64_bits,
        (-0f64).to_bits()
    );
    let outputs = batch
        .samples
        .iter()
        .map(|s| s.retained_bytes)
        .sum::<usize>();
    assert_eq!(
        batch.retained_bytes,
        plan.usage().live_retained_bytes
            + std::mem::size_of::<pose::PreparedSetBatch>()
            + 64
            + vectors.len() * std::mem::size_of::<pose::PoseSet>()
            + outputs
    );
    let validation = batch
        .samples
        .iter()
        .map(|s| s.sample_work.validation_units)
        .sum::<usize>();
    assert_eq!(
        batch.sample_work.validation_units,
        plan.usage().sample_work.validation_units + validation
    );
    assert_eq!(plan.usage().sample_work.validation_units, 28);
    assert_eq!(validation, vectors.len() * 28); // Every sample revalidates four full groups.
    for (times, result) in vectors.iter().zip(&batch.samples) {
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::to_value(plan.sample(times, Default::default()).unwrap()).unwrap()
        );
    }
    let mut twice = vectors.clone();
    twice.extend(vectors.clone());
    let larger = plan.sample_many(&twice, Default::default()).unwrap();
    assert_eq!(larger.preparation.required_forest_constructions, 1);
    assert_eq!(
        larger.retained_bytes - batch.retained_bytes,
        outputs + vectors.len() * std::mem::size_of::<pose::PoseSet>()
    );
    assert_eq!(
        larger.work_units - batch.work_units,
        batch.samples.iter().map(|s| s.work_units).sum::<usize>()
    );
    assert_eq!(
        larger.sample_work.validation_units - batch.sample_work.validation_units,
        validation
    );
    let exact = pose::SetBatchLimits {
        samples: vectors.len(),
        array_bytes: batch.retained_bytes,
        work_units: batch.work_units,
        sampling: sampling::Limits {
            validation_work: batch.sample_work.validation_units,
            sampling_work: batch.sample_work.sampling_units,
        },
        ..Default::default()
    };
    assert!(plan.sample_many(&vectors, exact).is_ok());
    for limits in [
        pose::SetBatchLimits {
            samples: vectors.len() - 1,
            ..exact
        },
        pose::SetBatchLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::SetBatchLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::SetBatchLimits {
            sampling: sampling::Limits {
                validation_work: exact.sampling.validation_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        pose::SetBatchLimits {
            sampling: sampling::Limits {
                sampling_work: exact.sampling.sampling_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        pose::SetBatchLimits {
            sample: pose::SetSampleLimits {
                live_array_bytes: plan.usage().live_retained_bytes - 1,
                ..exact.sample
            },
            ..exact
        },
    ] {
        assert!(plan.sample_many(&vectors, limits).is_err());
    }
}

#[test]
fn prepared_set_late_vector_failure_keeps_plan_and_earlier_owned_results() {
    let (bytes, selected) = higher_parent_set_fixture();
    let prepared = pose::PreparedSource::prepare(&bytes, "set atomic", Default::default()).unwrap();
    let plan = prepared
        .prepare_set(&set_bindings(&selected), Default::default())
        .unwrap();
    let times = set_times(&bytes, &selected, 1., 1.);
    let earlier = plan.sample(&times, Default::default()).unwrap();
    let saved = serde_json::to_value(&earlier).unwrap();
    let mut vectors = vec![times.clone(), times.clone(), times.clone()];
    vectors[2][1].source_time = 3.;
    let error = plan
        .sample_many(&vectors, Default::default())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("vector 2") && error.contains("extrapolate"),
        "{error}"
    );
    assert_eq!(serde_json::to_value(&earlier).unwrap(), saved);
    assert_eq!(
        serde_json::to_value(plan.sample(&times, Default::default()).unwrap()).unwrap(),
        saved
    );
    assert_eq!(plan.usage().required_forest_constructions, 1);
    assert!(plan.sample_many(&[], Default::default()).is_err());
}

#[test]
fn explicit_parent_child_set_composes_noncommuting_three_node_source_once() {
    let bytes = container(&set_fixture());
    let selected = set_requests();
    assert!(
        pose::evaluate(&bytes, "one child", selected[1], Default::default())
            .unwrap_err()
            .to_string()
            .contains("ancestor 1 controller is unapplied")
    );
    let set = pose::evaluate_set(&bytes, "set", &selected, Default::default()).unwrap();
    assert_eq!(
        set.objects[0].channel.local,
        [[0., -2., 0., 5.], [2., 0., 0., 3.], [0., 0., 2., 6.]]
    );
    assert_eq!(
        set.objects[0].source_world,
        [[-6., 0., 0., 1.], [0., -6., 0., 35.], [0., 0., 6., 48.]]
    );
    assert_eq!(
        set.objects[1].channel.local,
        [[0., 3., 0., 2.], [-3., 0., 0., 1.], [0., 0., 3., 3.]]
    );
    assert_eq!(
        set.objects[1].source_world,
        [[0., -18., 0., -11.], [18., 0., 0., 29.], [0., 0., 18., 66.]]
    );
    assert_eq!(
        set.objects[1]
            .ancestors
            .iter()
            .map(|a| (a.source.block, a.applied_object))
            .collect::<Vec<_>>(),
        [(1, Some(1)), (0, None)]
    );
    assert_eq!(
        set.objects[1].ancestors[0].effective_local,
        set.objects[0].channel.local
    );
    assert_eq!(set.propagated_objects, 3);
    assert!(!set.retail_behavior_verified);
}

#[test]
fn pose_set_request_permutation_changes_only_observation_order() {
    let bytes = container(&set_fixture());
    let selected = set_requests();
    let first = pose::evaluate_set(&bytes, "permutation", &selected, Default::default()).unwrap();
    let second = pose::evaluate_set(
        &bytes,
        "permutation",
        &[selected[1], selected[0]],
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&first.objects[0]).unwrap(),
        serde_json::to_value(&second.objects[1]).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first.objects[1]).unwrap(),
        serde_json::to_value(&second.objects[0]).unwrap()
    );
    assert_eq!(
        (first.retained_bytes, first.work_units),
        (second.retained_bytes, second.work_units)
    );
}

fn higher_parent_set_fixture() -> (Vec<u8>, [Request; 2]) {
    let mut source = set_fixture();
    source.swap(0, 5);
    set(&mut source[1].1, 80, 0); // parent1 now points to child0
    set(&mut source[6].1, 22, 0); // exact child controller target
    let mut bytes = container(&source);
    let footer = bytes.len() - 4;
    set(&mut bytes, footer, 5);
    (
        bytes,
        [
            request(1.),
            Request {
                object: 0,
                controller: 6,
                source_time: 1.,
            },
        ],
    )
}

#[test]
fn pose_set_higher_id_parents_propagate_once_and_permute_only_report_order() {
    let (bytes, selected) = higher_parent_set_fixture();
    let (_, scene) = fallout_data::nif_scene::decode(&bytes, "higher-parent source").unwrap();
    assert_eq!(
        scene
            .world_transforms
            .iter()
            .map(|w| (w.block, w.parent))
            .collect::<Vec<_>>(),
        [(0, Some(1)), (1, Some(5)), (5, None)]
    );
    let first =
        pose::evaluate_set(&bytes, "higher-parent set", &selected, Default::default()).unwrap();
    let second = pose::evaluate_set(
        &bytes,
        "higher-parent set",
        &[selected[1], selected[0]],
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        first.objects[0].source_world,
        [[-6., 0., 0., 1.], [0., -6., 0., 35.], [0., 0., 6., 48.]]
    );
    assert_eq!(
        first.objects[1].source_world,
        [[0., -18., 0., -11.], [18., 0., 0., 29.], [0., 0., 18., 66.]]
    );
    assert_eq!(
        first.objects[1]
            .ancestors
            .iter()
            .map(|a| (a.source.block, a.applied_object))
            .collect::<Vec<_>>(),
        [(1, Some(1)), (5, None)]
    );
    assert_eq!(first.propagated_objects, 3);
    assert_eq!(
        serde_json::to_value(&first.objects[0]).unwrap(),
        serde_json::to_value(&second.objects[1]).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&first.objects[1]).unwrap(),
        serde_json::to_value(&second.objects[0]).unwrap()
    );
    assert_eq!(
        (
            first.retained_bytes,
            first.work_units,
            first.propagated_objects
        ),
        (
            second.retained_bytes,
            second.work_units,
            second.propagated_objects
        )
    );
}

#[test]
fn pose_set_higher_parent_traversal_storage_work_and_sampler_ceilings_are_aggregate() {
    let (bytes, selected) = higher_parent_set_fixture();
    let baseline = pose::evaluate_set(
        &bytes,
        "higher-parent bounds",
        &selected,
        Default::default(),
    )
    .unwrap();
    let exact = pose::SetLimits {
        requests: 2,
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        ancestry_depth: 3,
        sampling: sampling::Limits {
            validation_work: baseline.sample_work.validation_units,
            sampling_work: baseline.sample_work.sampling_units,
        },
        ..Default::default()
    };
    let passed = pose::evaluate_set(&bytes, "higher-parent bounds", &selected, exact).unwrap();
    assert_eq!(
        passed.objects[1].source_world,
        baseline.objects[1].source_world
    );
    for (limits, detail) in [
        (
            pose::SetLimits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage budget",
        ),
        (
            pose::SetLimits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            pose::SetLimits {
                ancestry_depth: 2,
                ..exact
            },
            "ancestry depth budget",
        ),
        (
            pose::SetLimits {
                sampling: sampling::Limits {
                    validation_work: exact.sampling.validation_work - 1,
                    ..exact.sampling
                },
                ..exact
            },
            "validation-work budget",
        ),
        (
            pose::SetLimits {
                sampling: sampling::Limits {
                    sampling_work: exact.sampling.sampling_work - 1,
                    ..exact.sampling
                },
                ..exact
            },
            "request-work budget",
        ),
    ] {
        let error =
            pose::evaluate_set(&bytes, "higher-parent bounds", &selected, limits).unwrap_err();
        assert!(error.to_string().contains(detail), "{error}");
    }
}

#[test]
fn pose_set_duplicates_missing_controlled_ancestor_and_bad_later_channel_refuse() {
    let bytes = container(&set_fixture());
    let selected = set_requests();
    for (requests, expected) in [
        (
            vec![selected[0], selected[0]],
            "duplicate pose set object 1",
        ),
        (
            vec![selected[1]],
            "required ancestor 1 controller 2 is not explicitly selected",
        ),
        (
            vec![
                selected[0],
                Request {
                    controller: 7,
                    ..selected[1]
                },
            ],
            "object.controller differs",
        ),
        (
            vec![
                selected[0],
                Request {
                    source_time: 3.,
                    ..selected[1]
                },
            ],
            "extrapolate",
        ),
        (
            vec![
                selected[0],
                Request {
                    source_time: f64::NAN,
                    ..selected[1]
                },
            ],
            "source time must be finite",
        ),
    ] {
        let error =
            pose::evaluate_set(&bytes, "refusal", &requests, Default::default()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut source = set_fixture();
    source[5].1 = node(6, &[0], [44., 45., 46.], ID, 9.);
    assert!(
        pose::evaluate_set(&container(&source), "cycle", &selected, Default::default()).is_err()
    );
}

#[test]
fn pose_set_exact_aggregate_arrays_work_sampling_depth_and_count_limits() {
    let bytes = container(&set_fixture());
    let selected = set_requests();
    let baseline = pose::evaluate_set(&bytes, "bounds", &selected, Default::default()).unwrap();
    let exact = pose::SetLimits {
        requests: 2,
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        ancestry_depth: 3,
        sampling: sampling::Limits {
            validation_work: baseline.sample_work.validation_units,
            sampling_work: baseline.sample_work.sampling_units,
        },
        ..Default::default()
    };
    assert!(pose::evaluate_set(&bytes, "exact", &selected, exact).is_ok());
    for limits in [
        pose::SetLimits {
            requests: 1,
            ..exact
        },
        pose::SetLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        pose::SetLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        pose::SetLimits {
            ancestry_depth: 2,
            ..exact
        },
        pose::SetLimits {
            sampling: sampling::Limits {
                validation_work: exact.sampling.validation_work - 1,
                ..exact.sampling
            },
            ..exact
        },
        pose::SetLimits {
            sampling: sampling::Limits {
                sampling_work: exact.sampling.sampling_work - 1,
                ..exact.sampling
            },
            ..exact
        },
    ] {
        assert!(pose::evaluate_set(&bytes, "one over", &selected, limits).is_err());
    }
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
