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
