//! Independent authored constant Boolean packets and literal held-key results.
use fallout_data::nif_animation::visibility::{self, Limits, Request, Selection};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
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
fn set(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn controller() -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL]);
    out.extend(0x004Cu16.to_le_bytes());
    floats(&mut out, &[17., -9., 100., 101.]);
    words(&mut out, &[0, 2]);
    out
}
fn group(times: &[f32], values: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[times.len() as u32]);
    if !times.is_empty() {
        words(&mut out, &[5]);
    }
    for (&time, &value) in times.iter().zip(values) {
        floats(&mut out, &[time]);
        out.push(value);
    }
    out
}
fn blocks() -> Blocks {
    let mut node = Vec::new();
    words(&mut node, &[NULL, 0, 1, 0xFFFF]);
    floats(
        &mut node,
        &[0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.],
    );
    words(&mut node, &[0, NULL, 0, 0]);
    let mut interpolator = vec![2];
    words(&mut interpolator, &[3]);
    vec![
        ("NiNode", node),
        ("NiVisController", controller()),
        ("NiBoolInterpolator", interpolator),
        ("NiBoolData", group(&[-0., 2., 4.], &[0, 1, 0])),
    ]
}
fn container(blocks: &Blocks) -> Vec<u8> {
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
    words(&mut out, &[0, 0, 0]);
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[1, 0]);
    out
}
fn request(time: f64) -> Request {
    Request {
        object: 0,
        controller: 1,
        source_time: time,
    }
}
fn evaluate(blocks: &Blocks, time: f64) -> visibility::Evaluation {
    visibility::evaluate(
        &container(blocks),
        "authored visibility",
        request(time),
        Default::default(),
    )
    .unwrap()
}
fn failure(blocks: &Blocks, request: Request, limits: Limits, reason: &str) {
    let bytes = container(blocks);
    let before = bytes.clone();
    let error = visibility::evaluate(&bytes, "authored visibility", request, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason}: {error}");
    assert_eq!(bytes, before);
}
#[test]
fn held_keys_exact_endpoints_and_source_identity_are_independent_expectations() {
    let blocks = blocks();
    let bytes = container(&blocks);
    for (time, index, raw) in [
        (0., 0, 0),
        (-0., 0, 0),
        (1., 0, 0),
        (2., 1, 1),
        (3., 1, 1),
        (4., 2, 0),
    ] {
        let result = evaluate(&blocks, time);
        assert_eq!(result.raw_value, raw);
        assert_eq!(result.local_visible, raw == 1);
        assert!(matches!(result.selection,Selection::HeldKey { index:actual,.. } if actual==index));
        assert_eq!(
            result.source_sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
        assert_eq!(result.requested_time_f64_bits, time.to_bits());
        for span in [
            &result.object,
            &result.controller,
            &result.interpolator,
            result.data.as_ref().unwrap(),
        ] {
            assert_eq!(
                span.sha256,
                format!(
                    "{:x}",
                    Sha256::digest(&bytes[span.offset..span.offset + span.bytes])
                )
            );
            assert_eq!(
                &bytes[span.offset..span.offset + span.bytes],
                blocks[span.block as usize].1
            );
        }
        assert_eq!(
            result.unapplied_controller_fields.frequency_bits,
            17f32.to_bits()
        );
        assert_eq!(
            result.unapplied_controller_fields.start_bits,
            100f32.to_bits()
        );
        assert_eq!(result.raw_interpolator_value, 2);
        assert_eq!(result.object_flags, 0xFFFF);
        assert!(!result.retail_behavior_verified);
    }
}
#[test]
fn binary64_boundary_before_key_does_not_round_into_binary32_next_key() {
    let blocks = blocks();
    let step = 2f64.powi(-40);
    assert!(!evaluate(&blocks, 2. - step).local_visible);
    assert!(evaluate(&blocks, 2. + step).local_visible);
}
#[test]
fn explicit_authored_pose_uses_only_zero_or_one_when_data_is_absent() {
    let mut blocks = blocks();
    set(&mut blocks[2].1, 1, NULL);
    for value in [0, 1] {
        blocks[2].1[0] = value;
        let result = evaluate(&blocks, f64::MAX);
        assert_eq!(result.local_visible, value == 1);
        assert!(matches!(result.selection, Selection::AuthoredPose));
        assert!(result.data.is_none());
    }
    for value in [2, 3, 255] {
        blocks[2].1[0] = value;
        failure(
            &blocks,
            request(0.),
            Default::default(),
            "pose Boolean value unavailable",
        );
    }
}
#[test]
fn empty_key_data_never_falls_back_to_raw_pose_value() {
    let mut blocks = blocks();
    blocks[2].1[0] = 1;
    blocks[3].1 = group(&[], &[]);
    failure(
        &blocks,
        request(0.),
        Default::default(),
        "constant key group unavailable",
    );
}
#[test]
fn duplicate_decreasing_times_and_unavailable_values_refuse_without_repair() {
    for times in [[0., 0., 4.], [0., -0., 4.], [0., 4., 2.]] {
        let mut blocks = blocks();
        blocks[3].1 = group(&times, &[0, 1, 0]);
        failure(
            &blocks,
            request(1.),
            Default::default(),
            "strictly increasing",
        );
    }
    for value in [2, 255] {
        let mut blocks = blocks();
        blocks[3].1 = group(&[0., 2., 4.], &[0, value, 0]);
        failure(
            &blocks,
            request(0.),
            Default::default(),
            "key Boolean value unavailable",
        );
    }
}
#[test]
fn unknown_flags_undefined_cycle_and_manager_state_refuse() {
    for flags in [0x014Cu16, 0x004E, 0x006C] {
        let mut blocks = blocks();
        blocks[1].1[4..6].copy_from_slice(&flags.to_le_bytes());
        failure(
            &blocks,
            request(1.),
            Default::default(),
            "flags require unavailable semantics",
        );
    }
}
#[test]
fn exact_object_controller_target_chain_and_interpolator_links_do_not_guess() {
    let original = blocks();
    failure(
        &original,
        Request {
            controller: 2,
            ..request(1.)
        },
        Default::default(),
        "object.controller differs",
    );
    let mut blocks = original.clone();
    set(&mut blocks[1].1, 22, NULL);
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "controller.target differs",
    );
    let mut blocks = original.clone();
    set(&mut blocks[1].1, 0, 1);
    failure(&blocks, request(1.), Default::default(), "controller chain");
    let mut blocks = original.clone();
    set(&mut blocks[1].1, 26, NULL);
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "missing Boolean interpolator",
    );
    let mut blocks = original;
    blocks[1].0 = "NiTransformController";
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "not NiVisController",
    );
}
#[test]
fn timeline_crossing_is_unavailable_even_when_constant_source_keys_are_valid() {
    let mut blocks = blocks();
    blocks[2].0 = "NiBoolTimelineInterpolator";
    failure(
        &blocks,
        request(1.),
        Default::default(),
        "timeline key crossing is unapplied",
    );
}
#[test]
fn finite_time_no_extrapolation_and_single_key_exact_time_are_explicit() {
    let mut blocks = blocks();
    for (time, reason) in [
        (f64::NAN, "must be finite"),
        (f64::INFINITY, "must be finite"),
        (-1., "extrapolate"),
        (5., "extrapolate"),
    ] {
        failure(&blocks, request(time), Default::default(), reason);
    }
    blocks[3].1 = group(&[2.], &[1]);
    assert!(evaluate(&blocks, 2.).local_visible);
    failure(&blocks, request(1.), Default::default(), "extrapolate");
}
#[test]
fn exact_additional_storage_and_work_limits_and_one_under() {
    let blocks = blocks();
    let result = evaluate(&blocks, 1.);
    let limits = Limits {
        array_bytes: result.retained_bytes,
        work_units: result.work_units,
        ..Default::default()
    };
    assert!(
        !visibility::evaluate(&container(&blocks), "exact", request(1.), limits)
            .unwrap()
            .local_visible
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
        "array storage budget",
    );
    failure(
        &blocks,
        request(1.),
        Limits {
            work_units: limits.work_units - 1,
            ..limits
        },
        "work budget",
    );
}
#[test]
fn changed_source_value_changes_visible_result_and_whole_hash_without_mutation() {
    let original = blocks();
    let first = evaluate(&original, 0.);
    let mut changed = original;
    changed[3].1 = group(&[0., 2., 4.], &[1, 1, 0]);
    let second = evaluate(&changed, 0.);
    assert!(!first.local_visible && second.local_visible);
    assert_ne!(first.source_sha256, second.source_sha256);
    assert_ne!(first.data.unwrap().sha256, second.data.unwrap().sha256);
}

fn path_node(controller: u32, flags: u32, children: &[u32]) -> Vec<u8> {
    let mut out = blocks()[0].1.clone();
    set(&mut out, 8, controller);
    set(&mut out, 12, flags);
    out.truncate(76);
    words(&mut out, &[children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn path_controller(target: u32, interpolator: u32) -> Vec<u8> {
    let mut out = controller();
    set(&mut out, 22, target);
    set(&mut out, 26, interpolator);
    out
}
fn path_blocks() -> Blocks {
    let mut leaf = vec![2];
    words(&mut leaf, &[3]);
    let mut pose = vec![1];
    words(&mut pose, &[NULL]);
    let mut root = vec![2];
    words(&mut root, &[10]);
    let mut outside = path_controller(11, NULL);
    set(&mut outside, 0, 12);
    outside[4..6].copy_from_slice(&0x006Cu16.to_le_bytes());
    vec![
        ("NiNode", path_node(1, 0xAAAA_BB01, &[])),
        ("NiVisController", path_controller(0, 2)),
        ("NiBoolInterpolator", leaf),
        ("NiBoolData", group(&[-0., 2., 4.], &[0, 1, 0])),
        ("NiNode", path_node(5, 0x1234_5602, &[0])),
        ("NiVisController", path_controller(4, 6)),
        ("NiBoolInterpolator", pose),
        ("NiNode", path_node(8, 0xFEDC_BAFF, &[4, 11])),
        ("NiVisController", path_controller(7, 9)),
        ("NiBoolInterpolator", root),
        ("NiBoolData", group(&[-0., 2., 4.], &[1, 0, 1])),
        ("NiNode", path_node(12, 0xDEAD_BEEF, &[])),
        ("NiVisController", outside),
    ]
}
fn path_bytes(blocks: &Blocks) -> Vec<u8> {
    let mut out = container(blocks);
    let root_offset = out.len() - 4;
    set(&mut out, root_offset, 7);
    out
}
fn path_channels() -> [Request; 3] {
    [
        Request {
            object: 0,
            controller: 1,
            source_time: 3.,
        },
        Request {
            object: 7,
            controller: 8,
            source_time: 2.,
        },
        Request {
            object: 4,
            controller: 5,
            source_time: -0.,
        },
    ]
}
fn path_evaluate(
    bytes: &[u8],
    object: u32,
    channels: &[Request],
    limits: visibility::path::Limits,
) -> fallout_data::Result<visibility::path::Observation> {
    visibility::path::evaluate(
        bytes,
        "literal visibility path",
        visibility::path::Request {
            expected_source_sha256: Sha256::digest(bytes).into(),
            object,
            channels,
        },
        limits,
    )
}
fn path_failure(
    blocks: &Blocks,
    object: u32,
    channels: &[Request],
    limits: visibility::path::Limits,
    reason: &str,
) {
    let bytes = path_bytes(blocks);
    let before = bytes.clone();
    let error = path_evaluate(&bytes, object, channels, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "{reason}: {error}");
    assert_eq!(bytes, before);
}
#[test]
fn required_visibility_path_uses_higher_id_parents_and_preserves_exact_local_results() {
    let blocks = path_blocks();
    let bytes = path_bytes(&blocks);
    let channels = path_channels();
    let result = path_evaluate(&bytes, 0, &channels, Default::default()).unwrap();
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| n.object.block)
            .collect::<Vec<_>>(),
        [7, 4, 0]
    );
    assert_eq!(
        result.nodes.iter().map(|n| n.parent).collect::<Vec<_>>(),
        [None, Some(7), Some(4)]
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| n.object_flags)
            .collect::<Vec<_>>(),
        [0xFEDC_BAFF, 0x1234_5602, 0xAAAA_BB01]
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| n.local.as_ref().unwrap().raw_value)
            .collect::<Vec<_>>(),
        [0, 1, 1]
    );
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| n.local.as_ref().unwrap().local_visible)
            .collect::<Vec<_>>(),
        [false, true, true]
    );
    assert!(
        matches!(result.nodes[0].local.as_ref().unwrap().selection,Selection::HeldKey{index:1,time_bits} if time_bits==2f32.to_bits())
    );
    assert!(matches!(
        result.nodes[1].local.as_ref().unwrap().selection,
        Selection::AuthoredPose
    ));
    assert_eq!(
        result.nodes[1]
            .local
            .as_ref()
            .unwrap()
            .requested_time_f64_bits,
        (-0f64).to_bits()
    );
    assert!(
        matches!(result.nodes[2].local.as_ref().unwrap().selection,Selection::HeldKey{index:1,time_bits} if time_bits==2f32.to_bits())
    );
    for node in &result.nodes {
        let local = node.local.as_ref().unwrap();
        let channel = channels
            .iter()
            .find(|r| r.object == node.object.block)
            .unwrap();
        let old =
            visibility::evaluate(&bytes, "literal local", *channel, Default::default()).unwrap();
        assert_eq!(
            serde_json::to_vec(local).unwrap(),
            serde_json::to_vec(&old).unwrap()
        );
        assert_eq!(
            local.unapplied_controller_fields.frequency_bits,
            17f32.to_bits()
        );
        assert_eq!(
            local.unapplied_controller_fields.start_bits,
            100f32.to_bits()
        );
        assert_eq!(
            &bytes[node.object.offset..node.object.offset + node.object.bytes],
            blocks[node.object.block as usize].1
        );
        assert_eq!(
            node.object.sha256,
            format!(
                "{:x}",
                Sha256::digest(&blocks[node.object.block as usize].1)
            )
        );
    }
    let reversed = [channels[2], channels[1], channels[0]];
    let permuted = path_evaluate(&bytes, 0, &reversed, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_vec(&result).unwrap(),
        serde_json::to_vec(&permuted).unwrap()
    );
    assert_eq!(result.boolean_source_decodes, 1);
    assert_eq!(result.scene_decodes, 1);
    assert_eq!(result.selected.block, 0);
    assert!(!result.retail_behavior_verified);
}
#[test]
fn path_exact_channel_coverage_rejects_missing_duplicate_unrelated_and_wrong_links() {
    let blocks = path_blocks();
    let channels = path_channels();
    path_failure(
        &blocks,
        0,
        &channels[..2],
        Default::default(),
        "no explicit channel",
    );
    path_failure(
        &blocks,
        0,
        &[channels[0], channels[0]],
        Default::default(),
        "duplicate explicit channel",
    );
    let mut extra = channels.to_vec();
    extra.push(Request {
        object: 11,
        controller: 12,
        source_time: 0.,
    });
    path_failure(
        &blocks,
        0,
        &extra,
        Default::default(),
        "outside required ancestry path",
    );
    let mut wrong = channels;
    wrong[0].controller = 5;
    path_failure(
        &blocks,
        0,
        &wrong,
        Default::default(),
        "object.controller differs",
    );
    wrong = channels;
    wrong[0].object = 99;
    path_failure(
        &blocks,
        0,
        &wrong,
        Default::default(),
        "object is not decoded",
    );
    let mut static_mid = blocks.clone();
    set(&mut static_mid[4].1, 8, NULL);
    path_failure(
        &static_mid,
        0,
        &channels,
        Default::default(),
        "without a controller",
    );
    let bytes = path_bytes(&static_mid);
    let result = path_evaluate(&bytes, 0, &channels[..2], Default::default()).unwrap();
    assert!(result.nodes[1].local.is_none());
    assert_eq!(result.nodes[1].object_flags, 0x1234_5602);
    // The sibling's manager flags/chained controller never become path authority.
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| n.object.block)
            .collect::<Vec<_>>(),
        [7, 4, 0]
    );
}
#[test]
fn path_local_channel_refusals_are_atomic_and_never_repair_late_inputs() {
    let original = path_blocks();
    let channels = path_channels();
    for (offset, value, reason) in [
        (0, 1, "controller chain"),
        (22, NULL, "controller.target differs"),
        (26, NULL, "missing Boolean interpolator"),
    ] {
        let mut blocks = original.clone();
        set(&mut blocks[1].1, offset, value);
        path_failure(&blocks, 0, &channels, Default::default(), reason);
    }
    let mut blocks = original.clone();
    blocks[1].1[4..6].copy_from_slice(&0x006Cu16.to_le_bytes());
    path_failure(
        &blocks,
        0,
        &channels,
        Default::default(),
        "flags require unavailable semantics",
    );
    let mut blocks = original.clone();
    blocks[1].0 = "NiTransformController";
    path_failure(
        &blocks,
        0,
        &channels,
        Default::default(),
        "not supported NiVisController",
    );
    let mut blocks = original.clone();
    blocks[2].0 = "NiBoolTimelineInterpolator";
    path_failure(
        &blocks,
        0,
        &channels,
        Default::default(),
        "timeline key crossing is unapplied",
    );
    for group_bytes in [
        group(&[-0., 2., 4.], &[0, 2, 0]),
        group(&[0., 0., 4.], &[0, 1, 0]),
        group(&[], &[]),
    ] {
        let mut blocks = original.clone();
        blocks[3].1 = group_bytes;
        assert!(path_evaluate(&path_bytes(&blocks), 0, &channels, Default::default()).is_err());
    }
    for time in [f64::NAN, f64::INFINITY, 5.] {
        let mut changed = channels;
        changed[0].source_time = time;
        path_failure(
            &original,
            0,
            &changed,
            Default::default(),
            if time.is_finite() {
                "extrapolate"
            } else {
                "must be finite"
            },
        );
    }
    assert!(path_evaluate(&path_bytes(&original), 0, &channels, Default::default()).is_ok());
}
#[test]
fn path_source_graph_and_exact_identity_are_mandatory_before_observations() {
    let blocks = path_blocks();
    let bytes = path_bytes(&blocks);
    let channels = path_channels();
    let wrong = visibility::path::Request {
        expected_source_sha256: [0; 32],
        object: 0,
        channels: &channels,
    };
    let error = visibility::path::evaluate(
        &bytes,
        "identity",
        wrong,
        visibility::path::Limits {
            array_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("source SHA256 differs"), "{error}");
    let mut orphan = blocks.clone();
    orphan[4].1 = path_node(5, 0x1234_5602, &[]);
    path_failure(
        &orphan,
        0,
        &channels,
        Default::default(),
        "not reachable from footer",
    );
    let mut unresolved = blocks.clone();
    set(&mut unresolved[7].1, 84, 12);
    path_failure(
        &unresolved,
        0,
        &channels,
        Default::default(),
        "unresolved scene ancestry",
    );
    let mut cycle = blocks.clone();
    set(&mut cycle[4].1, 80, 7);
    path_failure(
        &cycle,
        0,
        &channels,
        Default::default(),
        "footer root also has a scene parent",
    );
    path_failure(
        &blocks,
        99,
        &channels,
        Default::default(),
        "ancestry is unavailable",
    );
    let mut changed = blocks;
    changed[3].1 = group(&[-0., 2., 4.], &[1, 0, 1]);
    let changed = path_bytes(&changed);
    let stale = visibility::path::Request {
        expected_source_sha256: Sha256::digest(&bytes).into(),
        object: 0,
        channels: &channels,
    };
    assert!(
        visibility::path::evaluate(&changed, "stale", stale, Default::default())
            .unwrap_err()
            .to_string()
            .contains("source SHA256 differs")
    );
}
#[test]
fn path_full_storage_work_source_admission_and_local_caps_have_exact_ceilings() {
    let blocks = path_blocks();
    let bytes = path_bytes(&blocks);
    let channels = path_channels();
    let baseline = path_evaluate(&bytes, 0, &channels, Default::default()).unwrap();
    let mut exact = visibility::path::Limits {
        channels: 3,
        path_nodes: 3,
        ancestry_depth: 3,
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    exact.local.array_bytes = baseline
        .nodes
        .iter()
        .filter_map(|n| n.local.as_ref())
        .map(|n| n.retained_bytes)
        .max()
        .unwrap();
    exact.local.work_units = baseline
        .nodes
        .iter()
        .filter_map(|n| n.local.as_ref())
        .map(|n| n.work_units)
        .max()
        .unwrap();
    let admitted = path_evaluate(&bytes, 0, &channels, exact).unwrap();
    assert_eq!(admitted.retained_bytes, baseline.retained_bytes);
    assert_eq!(admitted.work_units, baseline.work_units);
    let mut under = exact;
    under.array_bytes -= 1;
    let mut cases = vec![(under, "array storage budget")];
    under = exact;
    under.work_units -= 1;
    cases.push((under, "work budget"));
    under = exact;
    under.local.array_bytes -= 1;
    cases.push((under, "array storage budget"));
    under = exact;
    under.local.work_units -= 1;
    cases.push((under, "work budget"));
    under = exact;
    under.channels -= 1;
    cases.push((under, "channel count budget"));
    under = exact;
    under.ancestry_depth -= 1;
    cases.push((under, "path depth/node budget"));
    under = exact;
    under.path_nodes -= 1;
    cases.push((under, "path depth/node budget"));
    under = exact;
    under.decoder_array_admission_bytes -= 1;
    cases.push((under, "decoder array admission"));
    under = exact;
    under.decoder_check_admission_units -= 1;
    cases.push((under, "decoder check admission"));
    under = exact;
    under.local.scene.input_bytes = bytes.len() - 1;
    cases.push((under, "source input byte budget"));
    under = exact;
    under
        .local
        .keys
        .booleans
        .components
        .splines
        .keyframes
        .animation
        .input_bytes = bytes.len() - 1;
    cases.push((under, "source input byte budget"));
    for (limits, reason) in cases {
        path_failure(&blocks, 0, &channels, limits, reason);
    }
}
#[test]
fn path_static_nodes_preserve_flag_words_without_synthesizing_local_truth() {
    let mut blocks = path_blocks();
    for id in [0, 4, 7] {
        set(&mut blocks[id].1, 8, NULL);
    }
    let bytes = path_bytes(&blocks);
    let result = path_evaluate(&bytes, 0, &[], Default::default()).unwrap();
    assert_eq!(
        result
            .nodes
            .iter()
            .map(|n| n.object_flags)
            .collect::<Vec<_>>(),
        [0xFEDC_BAFF, 0x1234_5602, 0xAAAA_BB01]
    );
    assert!(
        result
            .nodes
            .iter()
            .all(|n| n.local.is_none() && n.object_controller.is_none())
    );
    let json = serde_json::to_value(&result).unwrap();
    assert!(json.get("visible").is_none());
    assert!(json.get("effective_visible").is_none());
    let root = path_evaluate(&bytes, 7, &[], Default::default()).unwrap();
    assert_eq!(root.nodes.len(), 1);
    assert_eq!(root.nodes[0].object.block, 7);
}
