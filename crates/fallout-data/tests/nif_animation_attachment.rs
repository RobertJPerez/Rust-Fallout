//! Two independently authored source spaces and literal attachment expectations.
use fallout_data::{
    nif_animation::attachment::{self, Limits, Request, SourcePolicy},
    nif_skin::pose::Affine,
};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
const NAME: &[u8] = b"Bip01 R Hand\0\xff";
const ROT: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
const HALF: [[f32; 3]; 3] = [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]];
const CALLER: Affine = [[-2., 0., 0., 7.], [0., 3., 0., 8.], [0., 0., 4., 9.]];
fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn node(
    name: u32,
    children: &[u32],
    translation: [f32; 3],
    rotation: [[f32; 3]; 3],
    scale: f32,
    controller: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[name, 0, controller, 0x1234_5678]);
    for value in translation
        .into_iter()
        .chain(rotation.into_iter().flatten())
        .chain([scale])
    {
        words(&mut out, &[value.to_bits()]);
    }
    words(&mut out, &[0, NULL, children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn container(blocks: &[(&str, Vec<u8>)], strings: &[&[u8]], roots: &[u32]) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, 34]);
    out.extend([0; 3]);
    let mut names = Vec::new();
    for (name, _) in blocks {
        if !names.contains(name) {
            names.push(*name);
        }
    }
    out.extend((names.len() as u16).to_le_bytes());
    for name in &names {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        out.extend((names.iter().position(|n| n == name).unwrap() as u16).to_le_bytes());
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
    for string in strings {
        words(&mut out, &[string.len() as u32]);
        out.extend(*string);
    }
    words(&mut out, &[0]);
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[roots.len() as u32]);
    words(&mut out, roots);
    out
}
fn skeleton(roots: &[u32], child_name: u32, root_name: &[u8], unknown_child: bool) -> Vec<u8> {
    container(
        &[
            (
                "NiNode",
                node(
                    1,
                    if unknown_child { &[1, 2] } else { &[1] },
                    [1., 2., 3.],
                    ROT,
                    2.,
                    NULL,
                ),
            ),
            ("NiNode", node(child_name, &[], [4., 5., 6.], HALF, 0.5, 2)),
            ("NiFloatInterpolator", vec![0]),
        ],
        &[NAME, root_name],
        roots,
    )
}
fn asset(translation: [f32; 3]) -> Vec<u8> {
    container(
        &[
            ("NiNode", node(0, &[], translation, ROT, 2., 1)),
            ("NiFloatInterpolator", vec![0]),
        ],
        &[b"attachment-root"],
        &[0],
    )
}
fn pair() -> (Vec<u8>, Vec<u8>) {
    (
        skeleton(&[0], 0, b"skeleton-root", false),
        asset([10., -20., 30.]),
    )
}
fn request<'a>(s: &[u8], a: &[u8]) -> Request<'a> {
    Request {
        expected_skeleton_sha256: Sha256::digest(s).into(),
        expected_attachment_sha256: Sha256::digest(a).into(),
        node: 1,
        node_name_bytes: NAME,
        attachment_root: 0,
        attachment_parent_to_node: CALLER,
        source_policy: SourcePolicy::StoredNiAvLocals,
    }
}
fn failure(s: &[u8], a: &[u8], request: Request<'_>, limits: Limits, reason: &str) {
    let (s_before, a_before) = (s.to_vec(), a.to_vec());
    let error = attachment::evaluate(s, a, "authored", request, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason}: {error}");
    assert_eq!(s, s_before);
    assert_eq!(a, a_before);
}
#[test]
fn literal_noncommuting_transform_order_applies_root_once_and_preserves_source() {
    let (s, a) = pair();
    let result =
        attachment::evaluate(&s, &a, "authored", request(&s, &a), Default::default()).unwrap();
    assert_eq!(
        result.attachment_source_to_skeleton_source,
        [[0., 3., 0., -1.], [2., 0., 0., 3.], [0., 0., 4., 24.]]
    );
    assert_eq!(
        result.root_to_skeleton_source,
        [[6., 0., 0., -61.], [0., -4., 0., 23.], [0., 0., 8., 144.]]
    );
    // Root-local [1,2,3] -> asset source [6,-18,36] -> skeleton [-55,15,168].
    // Applying root_to_skeleton to the already source-world point would yield
    // [-25,95,432], which is deliberately a different operation.
    assert_eq!(
        result
            .skeleton_path
            .iter()
            .map(|n| n.source.block)
            .collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(result.skeleton_path[0].unapplied_controller, Some(2));
    assert_eq!(result.attachment_root.unapplied_controller, Some(1));
    assert_eq!(result.node_name_bytes, NAME);
    assert_eq!(result.skeleton_path[0].flags, 0x1234_5678);
    assert_eq!(result.skeleton_sha256, format!("{:x}", Sha256::digest(&s)));
    assert_eq!(
        result.attachment_sha256,
        format!("{:x}", Sha256::digest(&a))
    );
    for (bytes, source) in [
        (&s, &result.skeleton_path[0].source),
        (&s, &result.skeleton_path[1].source),
        (&a, &result.attachment_root.source),
    ] {
        assert_eq!(
            source.sha256,
            format!(
                "{:x}",
                Sha256::digest(&bytes[source.offset..source.offset + source.bytes])
            )
        );
    }
    assert!(!result.retail_behavior_verified);
}
#[test]
fn both_expected_hashes_refuse_stale_source_before_source_decode_or_extra_allocation() {
    let (s, a) = pair();
    let r = request(&s, &a);
    let limits = Limits {
        array_bytes: 0,
        ..Default::default()
    };
    failure(
        b"changed skeleton",
        &a,
        r,
        limits,
        "skeleton source SHA256 differs",
    );
    failure(
        &s,
        b"changed attachment",
        r,
        limits,
        "attachment source SHA256 differs",
    );
}
#[test]
fn exact_node_id_and_raw_name_do_not_search_or_fold_names() {
    let (s, a) = pair();
    let r = request(&s, &a);
    failure(
        &s,
        &a,
        Request { node: 99, ..r },
        Default::default(),
        "node is not decoded",
    );
    failure(
        &s,
        &a,
        Request {
            node_name_bytes: b"bip01 r hand",
            ..r
        },
        Default::default(),
        "raw node name differs",
    );
    let s = skeleton(&[0], 0, b"wanted-root", false);
    failure(
        &s,
        &a,
        Request {
            node_name_bytes: b"wanted-root",
            ..request(&s, &a)
        },
        Default::default(),
        "raw node name differs",
    );
}
#[test]
fn duplicate_name_still_binds_exact_explicit_node_and_missing_name_refuses() {
    let (_, a) = pair();
    let s = skeleton(&[0], 0, NAME, false);
    let result =
        attachment::evaluate(&s, &a, "duplicates", request(&s, &a), Default::default()).unwrap();
    assert_eq!(result.skeleton_path[0].source.block, 1);
    let s = skeleton(&[0], NULL, NAME, false);
    failure(
        &s,
        &a,
        request(&s, &a),
        Default::default(),
        "no authored name",
    );
}
#[test]
fn disconnected_and_unresolved_source_forests_refuse() {
    let (_, a) = pair();
    let s = skeleton(&[], 0, b"root", false);
    failure(
        &s,
        &a,
        request(&s, &a),
        Default::default(),
        "not reachable from footer",
    );
    let s = skeleton(&[0], 0, b"root", true);
    failure(
        &s,
        &a,
        request(&s, &a),
        Default::default(),
        "unresolved scene ancestry",
    );
}
#[test]
fn attachment_requires_exact_footer_root_without_fallback() {
    let (s, a) = pair();
    let r = request(&s, &a);
    failure(
        &s,
        &a,
        Request {
            attachment_root: 1,
            ..r
        },
        Default::default(),
        "not an exact footer root",
    );
}
#[test]
fn finite_forward_mapping_permits_zero_and_refuses_nonfinite_or_overflow() {
    let (s, a) = pair();
    let r = request(&s, &a);
    for value in [f64::NAN, f64::INFINITY] {
        let mut matrix = CALLER;
        matrix[0][0] = value;
        failure(
            &s,
            &a,
            Request {
                attachment_parent_to_node: matrix,
                ..r
            },
            Default::default(),
            "nonfinite or overflowing",
        );
    }
    let mut matrix = CALLER;
    matrix[1][1] = f64::MAX;
    matrix[1][3] = f64::MAX;
    failure(
        &s,
        &a,
        Request {
            attachment_parent_to_node: matrix,
            ..r
        },
        Default::default(),
        "nonfinite or overflowing",
    );
    let zero = [[0.; 4]; 3];
    let result = attachment::evaluate(
        &s,
        &a,
        "zero",
        Request {
            attachment_parent_to_node: zero,
            ..r
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result.root_to_skeleton_source,
        [[0., 0., 0., -9.], [0., 0., 0., 10.], [0., 0., 0., 15.]]
    );
}
#[test]
fn exact_result_work_depth_and_input_budgets_with_one_under() {
    let (s, a) = pair();
    let r = request(&s, &a);
    let result = attachment::evaluate(&s, &a, "baseline", r, Default::default()).unwrap();
    let limits = Limits {
        array_bytes: result.retained_bytes,
        work_units: result.work_units,
        ancestry_depth: 2,
        combined_input_bytes: s.len() + a.len(),
        ..Default::default()
    };
    assert!(attachment::evaluate(&s, &a, "exact", r, limits).is_ok());
    failure(
        &s,
        &a,
        r,
        Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
        "array storage budget",
    );
    failure(
        &s,
        &a,
        r,
        Limits {
            work_units: limits.work_units - 1,
            ..limits
        },
        "work budget",
    );
    failure(
        &s,
        &a,
        r,
        Limits {
            ancestry_depth: 1,
            ..limits
        },
        "ancestry depth budget",
    );
    failure(
        &s,
        &a,
        r,
        Limits {
            combined_input_bytes: limits.combined_input_bytes - 1,
            ..limits
        },
        "input byte budget",
    );
    let scene = fallout_data::nif_scene::Limits {
        input_bytes: s.len().max(a.len()),
        ..limits.scene
    };
    assert!(attachment::evaluate(&s, &a, "per source", r, Limits { scene, ..limits }).is_ok());
    failure(
        &s,
        &a,
        r,
        Limits {
            scene: fallout_data::nif_scene::Limits {
                input_bytes: scene.input_bytes - 1,
                ..scene
            },
            ..limits
        },
        "input byte budget",
    );
}
#[test]
fn changed_attachment_keeps_skeleton_mapping_independent_and_changes_root_pose() {
    let (s, a) = pair();
    let first = attachment::evaluate(&s, &a, "first", request(&s, &a), Default::default()).unwrap();
    let changed = asset([11., -20., 30.]);
    let second = attachment::evaluate(
        &s,
        &changed,
        "second",
        request(&s, &changed),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        first.attachment_source_to_skeleton_source,
        second.attachment_source_to_skeleton_source
    );
    assert_ne!(first.attachment_sha256, second.attachment_sha256);
    assert_eq!(second.root_to_skeleton_source[1][3], 25.);
}

fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for &value in values {
        words(out, &[value.to_bits()]);
    }
}
fn sampled_blocks(scale: f32) -> Vec<(&'static str, Vec<u8>)> {
    let mut controller = Vec::new();
    words(&mut controller, &[NULL]);
    controller.extend(0xFFFFu16.to_le_bytes());
    floats(&mut controller, &[17., -9., 100., 101.]);
    words(&mut controller, &[1, 3]);
    let mut interpolator = Vec::new();
    floats(
        &mut interpolator,
        &[1000., 2000., 3000., 2., -3., 4., -5., 12.],
    );
    words(&mut interpolator, &[4]);
    let mut keys = Vec::new();
    words(&mut keys, &[0, 2, 1]);
    floats(&mut keys, &[0., 2., 4., 6., 2., 6., 8., 10.]);
    words(&mut keys, &[2, 1]);
    floats(&mut keys, &[0., scale, 2., scale]);
    vec![
        ("NiNode", node(1, &[1], [1., 2., 3.], ROT, 2., NULL)),
        ("NiNode", node(0, &[], [4., 5., 6.], HALF, 0.5, 2)),
        ("NiTransformController", controller),
        ("NiTransformInterpolator", interpolator),
        ("NiTransformData", keys),
    ]
}
fn sampled_asset(child_controller: bool, root_controller: bool) -> Vec<u8> {
    if child_controller {
        container(
            &[
                ("NiNode", node(0, &[1], [10., -20., 30.], ROT, 2., NULL)),
                ("NiNode", node(0, &[], [0.; 3], ROT, 1., 2)),
                ("NiFloatInterpolator", vec![0]),
            ],
            &[b"asset"],
            &[0],
        )
    } else {
        container(
            &[
                (
                    "NiNode",
                    node(
                        0,
                        &[],
                        [10., -20., 30.],
                        ROT,
                        2.,
                        if root_controller { 1 } else { NULL },
                    ),
                ),
                ("NiFloatInterpolator", vec![0]),
            ],
            &[b"asset"],
            &[0],
        )
    }
}
fn sampled_request<'a>(s: &[u8], a: &[u8]) -> attachment::SampledRequest<'a> {
    attachment::SampledRequest {
        binding: request(s, a),
        sample: fallout_data::nif_animation::pose::Request {
            object: 1,
            controller: 2,
            source_time: 1.,
        },
    }
}
fn sampled_failure(
    s: &[u8],
    a: &[u8],
    request: attachment::SampledRequest<'_>,
    limits: attachment::SampledLimits,
    reason: &str,
) {
    let error =
        attachment::evaluate_sampled(s, a, "sampled attachment", request, limits).unwrap_err();
    assert!(error.to_string().contains(reason), "{error}");
}
#[test]
fn sampled_attachment_noncommuting_literal_world_root_mapping_and_raw_source_provenance() {
    let blocks = sampled_blocks(2.);
    let s = container(&blocks, &[NAME, b"root"], &[0]);
    let a = sampled_asset(false, false);
    let result = attachment::evaluate_sampled(
        &s,
        &a,
        "sampled",
        sampled_request(&s, &a),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result.sample.local,
        [[-2., 0., 0., 4.], [0., -2., 0., 6.], [0., 0., 2., 8.]]
    );
    assert_eq!(
        result.sample.source_world,
        [[0., 4., 0., -11.], [-4., 0., 0., 10.], [0., 0., 4., 19.]]
    );
    assert_eq!(
        result.attachment_source_to_skeleton_source,
        [[0., 12., 0., 21.], [8., 0., 0., -18.], [0., 0., 16., 55.]]
    );
    assert_eq!(
        result.root_to_skeleton_source,
        [
            [24., 0., 0., -219.],
            [0., -16., 0., 62.],
            [0., 0., 32., 535.]
        ]
    );
    assert_eq!(
        result.stored_binding.attachment_source_to_skeleton_source,
        [[0., 3., 0., -1.], [2., 0., 0., 3.], [0., 0., 4., 24.]]
    );
    assert_eq!(
        result.stored_binding.root_to_skeleton_source,
        [[6., 0., 0., -61.], [0., -4., 0., 23.], [0., 0., 8., 144.]]
    );
    assert_eq!(result.sample.requested_time_f64_bits, 1f64.to_bits());
    assert_eq!(result.stored_binding.node_name_bytes, NAME);
    assert_eq!(
        result
            .sample
            .unapplied_interpolator_fields
            .rotation_wxyz_bits,
        [
            2f32.to_bits(),
            (-3f32).to_bits(),
            4f32.to_bits(),
            (-5f32).to_bits()
        ]
    );
    assert_eq!(
        result.sample.source_sha256,
        result.stored_binding.skeleton_sha256
    );
    for (source, bytes) in [
        (&result.sample.object, &s),
        (&result.sample.controller, &s),
        (&result.sample.interpolator, &s),
        (&result.sample.data, &s),
        (&result.stored_binding.attachment_root.source, &a),
    ] {
        assert_eq!(
            source.sha256,
            format!(
                "{:x}",
                Sha256::digest(&bytes[source.offset..source.offset + source.bytes])
            )
        );
    }
    assert!(!result.retail_behavior_verified);
}
#[test]
fn sampled_attachment_preserves_mirrored_nonunit_scale_and_zero_forward_mapping() {
    let a = sampled_asset(false, false);
    let s = container(&sampled_blocks(-2.), &[NAME, b"root"], &[0]);
    let reflected = attachment::evaluate_sampled(
        &s,
        &a,
        "mirrored",
        sampled_request(&s, &a),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        reflected.attachment_source_to_skeleton_source,
        [
            [0., -12., 0., -43.],
            [-8., 0., 0., 38.],
            [0., 0., -16., -17.]
        ]
    );
    assert_eq!(
        reflected.root_to_skeleton_source,
        [
            [-24., 0., 0., 197.],
            [0., 16., 0., -42.],
            [0., 0., -32., -497.]
        ]
    );
    let s = container(&sampled_blocks(0.), &[NAME, b"root"], &[0]);
    let zero =
        attachment::evaluate_sampled(&s, &a, "zero", sampled_request(&s, &a), Default::default())
            .unwrap();
    assert_eq!(
        zero.root_to_skeleton_source,
        [[0., 0., 0., -11.], [0., 0., 0., 10.], [0., 0., 0., 19.]]
    );
    let r = sampled_request(&s, &a);
    let mut forward = CALLER;
    forward[1][1] = f64::INFINITY;
    sampled_failure(
        &s,
        &a,
        attachment::SampledRequest {
            binding: Request {
                attachment_parent_to_node: forward,
                ..r.binding
            },
            ..r
        },
        Default::default(),
        "nonfinite or overflowing",
    );
}
#[test]
fn sampled_attachment_exact_sha_name_node_controller_time_chain_rotation_and_ancestors_refuse() {
    let blocks = sampled_blocks(2.);
    let s = container(&blocks, &[NAME, b"root"], &[0]);
    let a = sampled_asset(false, false);
    let r = sampled_request(&s, &a);
    let mut stale = r;
    stale.binding.expected_skeleton_sha256[0] ^= 1;
    sampled_failure(
        &s,
        &a,
        stale,
        Default::default(),
        "skeleton source SHA256 differs",
    );
    let mut stale = r;
    stale.binding.expected_attachment_sha256[0] ^= 1;
    sampled_failure(
        &s,
        &a,
        stale,
        Default::default(),
        "attachment source SHA256 differs",
    );
    sampled_failure(
        &s,
        &a,
        attachment::SampledRequest {
            binding: Request {
                node_name_bytes: b"bip01 r hand",
                ..r.binding
            },
            ..r
        },
        Default::default(),
        "raw node name differs",
    );
    for (object, controller, time, reason) in [
        (0, 2, 1., "object differs"),
        (1, 3, 1., "object.controller differs"),
        (1, 2, f64::INFINITY, "time must be finite"),
        (1, 2, 3., "extrapolate"),
    ] {
        sampled_failure(
            &s,
            &a,
            attachment::SampledRequest {
                sample: fallout_data::nif_animation::pose::Request {
                    object,
                    controller,
                    source_time: time,
                },
                ..r
            },
            Default::default(),
            reason,
        );
    }
    let mut ancestor = blocks.clone();
    ancestor[0].1[8..12].copy_from_slice(&2u32.to_le_bytes());
    let mut chain = blocks.clone();
    chain[2].1[0..4].copy_from_slice(&2u32.to_le_bytes());
    let mut rotation = blocks;
    rotation[4].1.clear();
    words(&mut rotation[4].1, &[1, 1]);
    floats(&mut rotation[4].1, &[0., 1., 0., 0., 0.]);
    words(&mut rotation[4].1, &[0, 0]);
    for (blocks, reason) in [
        (ancestor, "ancestor 0 controller is unapplied"),
        (chain, "controller chain is unapplied"),
        (rotation, "rotation key mapping is unapplied"),
    ] {
        let s = container(&blocks, &[NAME, b"root"], &[0]);
        sampled_failure(&s, &a, sampled_request(&s, &a), Default::default(), reason);
    }
}
#[test]
fn sampled_attachment_rejects_controlled_socket_children_and_attachment_root_or_children() {
    let s = container(&sampled_blocks(2.), &[NAME, b"root"], &[0]);
    for a in [sampled_asset(false, true), sampled_asset(true, false)] {
        sampled_failure(
            &s,
            &a,
            sampled_request(&s, &a),
            Default::default(),
            "descendant",
        );
    }
    let mut blocks = sampled_blocks(2.);
    blocks[1].1 = node(0, &[5], [4., 5., 6.], HALF, 0.5, 2);
    blocks.push(("NiNode", node(0, &[], [0.; 3], ROT, 1., 2)));
    let s = container(&blocks, &[NAME, b"root"], &[0]);
    let a = sampled_asset(false, false);
    sampled_failure(
        &s,
        &a,
        sampled_request(&s, &a),
        Default::default(),
        "descendant 5 controller 2 is unapplied",
    );
}
#[test]
fn sampled_attachment_phase_aggregate_source_and_sampling_limits_have_exact_ceilings() {
    use attachment::SampledLimits;
    let s = container(&sampled_blocks(2.), &[NAME, b"root"], &[0]);
    let a = sampled_asset(false, false);
    let r = sampled_request(&s, &a);
    let baseline = attachment::evaluate_sampled(&s, &a, "baseline", r, Default::default()).unwrap();
    let mut exact = SampledLimits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    exact.binding.array_bytes = baseline.stored_binding.retained_bytes;
    exact.binding.work_units = baseline.stored_binding.work_units;
    exact.sample.array_bytes = baseline.sample.retained_bytes;
    exact.sample.work_units = baseline.sample.work_units;
    exact.sample.sampling.validation_work = baseline.sample.sample_work.validation_units;
    exact.sample.sampling.sampling_work = baseline.sample.sample_work.sampling_units;
    // Declared sampler allowances above changed the combined admission, so retain
    // the independently sufficient original admission cap while testing counts.
    exact.binding.combined_input_bytes = s.len() + a.len();
    exact.binding.scene.input_bytes = s.len().max(a.len());
    exact.sample.scene.input_bytes = s.len();
    exact.sample.keys.animation.input_bytes = s.len();
    exact.binding.ancestry_depth = 2;
    exact.sample.ancestry_depth = 2;
    attachment::evaluate_sampled(&s, &a, "exact", r, exact).unwrap();
    for mode in 0..11 {
        let mut under = exact;
        match mode {
            0 => under.array_bytes -= 1,
            1 => under.work_units -= 1,
            2 => under.binding.array_bytes -= 1,
            3 => under.binding.work_units -= 1,
            4 => under.sample.array_bytes -= 1,
            5 => under.sample.work_units -= 1,
            6 => under.sample.sampling.validation_work -= 1,
            7 => under.sample.sampling.sampling_work -= 1,
            8 => under.binding.combined_input_bytes -= 1,
            9 => under.sample.ancestry_depth = 1,
            _ => under.binding.ancestry_depth = 1,
        }
        assert!(
            attachment::evaluate_sampled(&s, &a, "under", r, under).is_err(),
            "mode {mode}"
        );
    }
    for mode in 0..2 {
        let mut under = SampledLimits {
            decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
            decoder_check_admission_units: baseline.decoder_check_admission_units,
            ..Default::default()
        };
        if mode == 0 {
            under.decoder_array_admission_bytes -= 1;
        } else {
            under.decoder_check_admission_units -= 1;
        }
        assert!(attachment::evaluate_sampled(&s, &a, "decoder-under", r, under).is_err());
    }
}
#[test]
fn sampled_attachment_keeps_stored_reference_exact_and_time_bits_distinct_without_clock_semantics()
{
    let s = container(&sampled_blocks(2.), &[NAME, NAME], &[0]);
    let a = sampled_asset(false, false);
    let r = sampled_request(&s, &a);
    let old = attachment::evaluate(&s, &a, "stored", r.binding, Default::default()).unwrap();
    let value = attachment::evaluate_sampled(&s, &a, "sampled", r, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(old).unwrap(),
        serde_json::to_value(value.stored_binding).unwrap()
    );
    let mut negative_zero = r;
    negative_zero.sample.source_time = -0.;
    let first =
        attachment::evaluate_sampled(&s, &a, "negative zero", negative_zero, Default::default())
            .unwrap();
    assert_eq!(first.sample.requested_time_f64_bits, (-0f64).to_bits());
    assert_ne!(
        first.attachment_source_to_skeleton_source,
        value.attachment_source_to_skeleton_source
    );
    assert_eq!(
        first.sample.unapplied_controller_fields.frequency_bits,
        17f32.to_bits()
    );
}

#[test]
#[ignore = "Requires the retained first-person skeleton and razor source blobs plus a new local evidence directory"]
fn capture_original_first_person_weapon_attachment_sources() {
    use fallout_data::nif_scene::{self, ObjectKind};
    use serde_json::json;
    use std::{
        fs::OpenOptions,
        io::Write,
        path::{Path, PathBuf},
    };

    const SKELETON_SHA256: &str =
        "3fe5a3ef9718c8bff773b328c93bf6e522e85b16afd0de1b9af33cfba550b121";
    const WEAPON_SHA256: &str = "facb4a340f34c7217efb359390e3ff78613c56ca8eff3ecab2bd99feffc76b79";
    const SELECTED_NODE: u32 = 12;
    const NODE_CONTROLLER: u32 = 13;
    const EXPLICIT_PARENT_TO_NODE: Affine = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
    let limits = Limits::default();

    let skeleton_input = PathBuf::from(
        std::env::var("ASSET_ATTACHMENT_SKELETON_BLOB").expect("explicit skeleton blob required"),
    );
    let weapon_input = PathBuf::from(
        std::env::var("ASSET_ATTACHMENT_WEAPON_BLOB").expect("explicit weapon blob required"),
    );
    let output = PathBuf::from(
        std::env::var("ASSET_ATTACHMENT_EVIDENCE").expect("new evidence directory required"),
    );
    assert!(output.is_absolute(), "evidence path must be absolute");
    let local = std::env::current_dir()
        .unwrap()
        .join("../../local")
        .canonicalize()
        .unwrap();
    let parent = output.parent().unwrap().canonicalize().unwrap();
    assert!(
        parent.starts_with(&local),
        "evidence must stay in this lane's local tree"
    );
    std::fs::create_dir(&output).unwrap();
    let inputs = output.join("inputs");
    std::fs::create_dir(&inputs).unwrap();

    let read_exact_source = |path: &Path, expected: &str| {
        let bytes = std::fs::read(path).unwrap();
        assert!(
            bytes.len() <= 64 * 1024 * 1024,
            "source input exceeds per-file cap"
        );
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected);
        bytes
    };
    let skeleton_bytes = read_exact_source(&skeleton_input, SKELETON_SHA256);
    let weapon_bytes = read_exact_source(&weapon_input, WEAPON_SHA256);
    assert!(skeleton_bytes.len() + weapon_bytes.len() <= limits.combined_input_bytes);

    for (name, bytes) in [
        ("first-person-skeleton.nif", &skeleton_bytes),
        ("first-person-straight-razor.nif", &weapon_bytes),
    ] {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(inputs.join(name))
            .unwrap();
        file.write_all(bytes).unwrap();
    }

    let (skeleton_index, skeleton_scene) = nif_scene::decode_with_limits(
        &skeleton_bytes,
        "Fallout - Meshes.bsa:meshes\\characters\\_1stperson\\skeleton.nif",
        limits.scene,
    )
    .unwrap();
    let (weapon_index, weapon_scene) = nif_scene::decode_with_limits(
        &weapon_bytes,
        "Fallout - Meshes.bsa:meshes\\weapons\\1handmelee\\1stpersonstraightrazor.nif",
        limits.scene,
    )
    .unwrap();
    let selected_node = skeleton_scene
        .objects
        .iter()
        .find(|object| object.block == SELECTED_NODE)
        .expect("retained selected source node must decode");
    assert!(matches!(&selected_node.kind, ObjectKind::Node { .. }));
    assert_eq!(selected_node.controller, Some(NODE_CONTROLLER));
    let node_name = skeleton_index.strings[selected_node.name.unwrap() as usize].clone();
    assert_eq!(node_name.as_slice(), b"Bip01 Rotate");
    let node_world = skeleton_scene
        .world_transforms
        .iter()
        .find(|world| world.block == SELECTED_NODE)
        .expect("selected node must have a source-world transform");
    assert!(node_world.reachable_from_footer);

    let weapon_roots: Vec<u32> = weapon_index.roots.iter().flatten().copied().collect();
    assert_eq!(
        weapon_roots.len(),
        1,
        "weapon footer root must be unambiguous"
    );
    let weapon_root_id = weapon_roots[0];
    let weapon_root = weapon_scene
        .objects
        .iter()
        .find(|object| object.block == weapon_root_id)
        .expect("weapon footer root must decode");
    assert!(matches!(&weapon_root.kind, ObjectKind::Node { .. }));

    let request = Request {
        expected_skeleton_sha256: Sha256::digest(&skeleton_bytes).into(),
        expected_attachment_sha256: Sha256::digest(&weapon_bytes).into(),
        node: SELECTED_NODE,
        node_name_bytes: &node_name,
        attachment_root: weapon_root_id,
        attachment_parent_to_node: EXPLICIT_PARENT_TO_NODE,
        source_policy: SourcePolicy::StoredNiAvLocals,
    };
    let result = attachment::evaluate(
        &skeleton_bytes,
        &weapon_bytes,
        "retained first-person attachment pair",
        request,
        limits,
    )
    .unwrap();
    assert_eq!(result.skeleton_path[0].source.block, SELECTED_NODE);
    assert_eq!(
        result.skeleton_path[0].unapplied_controller,
        Some(NODE_CONTROLLER)
    );
    assert_eq!(result.skeleton_path.last().unwrap().parent, None);
    assert!(result.skeleton_path.len() > 1);
    assert!(result.skeleton_path.len() <= limits.ancestry_depth);
    assert_eq!(result.attachment_root.source.block, weapon_root_id);
    assert!(
        result
            .attachment_source_to_skeleton_source
            .iter()
            .flatten()
            .all(|value| value.is_finite())
    );
    assert!(
        result
            .root_to_skeleton_source
            .iter()
            .flatten()
            .all(|value| value.is_finite())
    );
    assert!(result.retained_bytes <= limits.array_bytes);
    assert!(result.work_units <= limits.work_units);
    assert!(!result.retail_behavior_verified);

    let absent_node = attachment::evaluate(
        &skeleton_bytes,
        &weapon_bytes,
        "missing explicit node",
        Request {
            node: u32::MAX,
            ..request
        },
        limits,
    )
    .unwrap_err()
    .to_string();
    assert!(absent_node.contains("selected skeleton node is not decoded"));
    let absent_name = attachment::evaluate(
        &skeleton_bytes,
        &weapon_bytes,
        "missing exact node name",
        Request {
            node_name_bytes: b"unbound-source-name",
            ..request
        },
        limits,
    )
    .unwrap_err()
    .to_string();
    assert!(absent_name.contains("selected skeleton raw node name differs"));
    let bounded = attachment::evaluate(
        &skeleton_bytes,
        &weapon_bytes,
        "under ancestry bound",
        request,
        Limits {
            ancestry_depth: 1,
            ..limits
        },
    )
    .unwrap_err()
    .to_string();
    assert!(bounded.contains("ancestry depth budget exceeded"));

    let sampled_refusal = attachment::evaluate_sampled(
        &skeleton_bytes,
        &weapon_bytes,
        "retained original sampled attachment",
        attachment::SampledRequest {
            binding: request,
            sample: fallout_data::nif_animation::pose::Request {
                object: SELECTED_NODE,
                controller: NODE_CONTROLLER,
                source_time: 0.,
            },
        },
        Default::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(sampled_refusal.contains("missing transform interpolator"));

    let receipt = json!({
        "schema_version": 1,
        "scope": "source-local first-person weapon attachment composition; no confirmed actor/equipment selection or retail alignment",
        "source_manifest": {
            "path": "G:\\Rust-Fallout-worktrees\\assets\\local\\asset-04-dev-20261003-01\\retail-sample-01\\source-manifest.json",
            "sha256": "4d53c00ecdc7217bf9acd8764543dea231e4f16d9714db1428e8b8cf321fcdb1"
        },
        "skeleton": {
            "archive": "G:\\SteamLibrary\\steamapps\\common\\Fallout New Vegas\\Data\\Fallout - Meshes.bsa",
            "entry_index": 10514,
            "path_bytes": b"meshes\\characters\\_1stperson\\skeleton.nif".to_vec(),
            "sha256": SKELETON_SHA256,
            "decoded_bytes": skeleton_bytes.len(),
            "node": SELECTED_NODE,
            "raw_name_bytes": node_name,
            "controller": NODE_CONTROLLER,
            "source_world": node_world
        },
        "attachment": {
            "archive": "G:\\SteamLibrary\\steamapps\\common\\Fallout New Vegas\\Data\\Fallout - Meshes.bsa",
            "entry_index": 4757,
            "path_bytes": b"meshes\\weapons\\1handmelee\\1stpersonstraightrazor.nif".to_vec(),
            "sha256": WEAPON_SHA256,
            "decoded_bytes": weapon_bytes.len(),
            "footer_roots": weapon_roots,
            "selected_root": weapon_root_id,
            "root_local": weapon_root.transform
        },
        "explicit_parent_to_node": EXPLICIT_PARENT_TO_NODE,
        "mapping_basis": "caller-supplied identity matrix recorded in this diagnostic request; not inferred by the API",
        "evaluation": result,
        "refusals": {
            "unknown_node_id": absent_node,
            "mismatched_raw_name": absent_name,
            "one_level_ancestry_limit": bounded,
            "sampled_node_controller": sampled_refusal
        },
        "retail_behavior_verified": false,
        "visible_consumer_verified": false
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("attachment-source-receipt.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &receipt).unwrap();
    writeln!(file).unwrap();
    println!(
        "Captured and composed exact source node12/controller13 with weapon root {weapon_root_id}; sampled path refusal preserved"
    );
}
