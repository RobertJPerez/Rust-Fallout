//! Exact two-container binding, independent source packets and literal matrices.
use fallout_data::nif_animation::{
    clip::{self, Limits, Request},
    keyframe,
};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
const NAME: &[u8] = b"Bip01 Rotate\0\xff";
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
fn node(name: u32, controller: u32, children: &[u32], root: bool) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[name, 0, controller, 0x89AB_CDEF]);
    if root {
        floats(
            &mut out,
            &[2., -3., 5., 0., -1., 0., 1., 0., 0., 0., 0., 1., 2.],
        );
    } else {
        floats(
            &mut out,
            &[11., 12., 13., -1., 0., 0., 0., -1., 0., 0., 0., 1., 6.],
        );
    }
    words(&mut out, &[0, NULL, children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn container(blocks: &Blocks, strings: &[&[u8]], roots: &[u32]) -> Vec<u8> {
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
fn skeleton(duplicate: bool, ancestor_controller: bool, roots: &[u32]) -> Vec<u8> {
    container(
        &vec![
            (
                "NiNode",
                node(0, if ancestor_controller { 2 } else { NULL }, &[1], true),
            ),
            ("NiNode", node(1, 2, &[], false)),
            ("NiFloatInterpolator", vec![0]),
        ],
        &[if duplicate { NAME } else { b"root" }, NAME],
        roots,
    )
}
fn blocks() -> Blocks {
    let mut sequence = Vec::new();
    words(&mut sequence, &[NULL, 1, 7, 1, NULL]);
    sequence.push(25);
    words(&mut sequence, &[0, NULL, 1, NULL, NULL]);
    floats(&mut sequence, &[0.25]);
    words(&mut sequence, &[NULL, 3]);
    floats(&mut sequence, &[17., 100., 101.]);
    words(&mut sequence, &[NULL, NULL]);
    sequence.extend(2u16.to_le_bytes());
    words(&mut sequence, &[NULL, NULL]);
    let mut interpolator = Vec::new();
    floats(
        &mut interpolator,
        &[100., 200., 300., 2., -3., 4., -5., 12.],
    );
    words(&mut interpolator, &[2]);
    let mut data = Vec::new();
    words(&mut data, &[0, 2, 1]);
    floats(&mut data, &[-1., 4., 0., -2., 3., 0., 8., 6.]);
    words(&mut data, &[2, 1]);
    floats(&mut data, &[-1., -2., 3., 2.]);
    vec![
        ("NiControllerSequence", sequence),
        ("NiTransformInterpolator", interpolator),
        ("NiTransformData", data),
    ]
}
fn clip_bytes(blocks: &Blocks) -> Vec<u8> {
    container(
        blocks,
        &[NAME, b"NiTransformController", b"wrong-node"],
        &[0],
    )
}
fn pair() -> (Vec<u8>, Vec<u8>) {
    (skeleton(false, false, &[0]), clip_bytes(&blocks()))
}
fn request<'a>(s: &[u8], c: &[u8], time: f64) -> Request<'a> {
    Request {
        expected_skeleton_sha256: Sha256::digest(s).into(),
        expected_clip_sha256: Sha256::digest(c).into(),
        object: 1,
        node_name_bytes: NAME,
        sequence: 0,
        controlled_ordinal: 0,
        source_time: time,
    }
}
fn failure(s: &[u8], c: &[u8], request: Request<'_>, limits: Limits, reason: &str) {
    let before = (s.to_vec(), c.to_vec());
    let error = clip::evaluate(s, c, "authored external clip", request, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason}: {error}");
    assert_eq!(s, before.0);
    assert_eq!(c, before.1);
}
#[test]
fn exact_external_binding_changes_local_and_world_with_literal_expectations() {
    let (s, c) = pair();
    let result =
        clip::evaluate(&s, &c, "authored", request(&s, &c, 0.), Default::default()).unwrap();
    assert_eq!(
        result.local,
        [[1., 0., 0., 3.], [0., 1., 0., 2.], [0., 0., -1., 0.]]
    );
    assert_eq!(
        result.source_world,
        [[0., -2., 0., -2.], [2., 0., 0., 3.], [0., 0., -2., 5.]]
    );
    assert_eq!(result.node_name_bytes, NAME);
    assert_eq!(result.controlled_packet.priority, 25);
    assert_eq!(result.unapplied_object_controller, Some(2));
    assert_eq!(
        result.unapplied_sequence_fields.frequency_bits,
        17f32.to_bits()
    );
    assert_eq!(
        result.unapplied_sequence_fields.start_bits,
        100f32.to_bits()
    );
    assert_eq!(result.unapplied_sequence_fields.cycle_type, 3);
    assert_eq!(
        result.unapplied_interpolator_fields.rotation_wxyz_bits,
        [2., -3., 4., -5.].map(f32::to_bits)
    );
    for (bytes, span) in [
        (&s, &result.object),
        (&c, &result.sequence),
        (&c, &result.interpolator),
        (&c, &result.data),
    ] {
        assert_eq!(
            span.sha256,
            format!(
                "{:x}",
                Sha256::digest(&bytes[span.offset..span.offset + span.bytes])
            )
        );
    }
    assert_eq!(result.skeleton_sha256, format!("{:x}", Sha256::digest(&s)));
    assert_eq!(result.clip_sha256, format!("{:x}", Sha256::digest(&c)));
    assert!(!result.retail_behavior_verified);
}
#[test]
fn both_sources_require_exact_identity_before_decode_or_extra_allocation() {
    let (s, c) = pair();
    let r = request(&s, &c, 0.);
    let mut limits = Limits::default();
    limits.pose.array_bytes = 0;
    failure(
        b"different skeleton",
        &c,
        r,
        limits,
        "skeleton source SHA256 differs",
    );
    failure(
        &s,
        b"different clip",
        r,
        limits,
        "clip source SHA256 differs",
    );
}
#[test]
fn name_equality_and_uniqueness_are_checked_without_search_or_default() {
    let (s, c) = pair();
    let r = request(&s, &c, 0.);
    failure(
        &s,
        &c,
        Request { object: 0, ..r },
        Default::default(),
        "skeleton raw node name differs",
    );
    failure(
        &s,
        &c,
        Request {
            node_name_bytes: b"bip01 rotate",
            ..r
        },
        Default::default(),
        "skeleton raw node name differs",
    );
    let duplicate = skeleton(true, false, &[0]);
    failure(
        &duplicate,
        &c,
        request(&duplicate, &c, 0.),
        Default::default(),
        "not unique",
    );
    let mut packets = blocks();
    set(&mut packets[0].1, 21, 2);
    let c = clip_bytes(&packets);
    failure(
        &s,
        &c,
        request(&s, &c, 0.),
        Default::default(),
        "packet raw node name differs",
    );
}
#[test]
fn exact_sequence_packet_type_property_and_interpolator_links_refuse() {
    let (s, c) = pair();
    let r = request(&s, &c, 0.);
    failure(
        &s,
        &c,
        Request { sequence: 1, ..r },
        Default::default(),
        "not decoded NiControllerSequence",
    );
    failure(
        &s,
        &c,
        Request {
            controlled_ordinal: 1,
            ..r
        },
        Default::default(),
        "ordinal is out of range",
    );
    for (offset, value, reason) in [
        (25, 0, "property/controller/identifier"),
        (29, 2, "transform-controller type is unavailable"),
        (12, NULL, "interpolator is absent"),
    ] {
        let mut packets = blocks();
        set(&mut packets[0].1, offset, value);
        let c = clip_bytes(&packets);
        failure(&s, &c, request(&s, &c, 0.), Default::default(), reason);
    }
    let mut packets = blocks();
    set(&mut packets[1].1, 32, NULL);
    let c = clip_bytes(&packets);
    failure(
        &s,
        &c,
        request(&s, &c, 0.),
        Default::default(),
        "no authored key data",
    );
}
#[test]
fn rotation_keys_refuse_after_binding_without_silent_channel_drop() {
    let (s, _) = pair();
    let mut packets = blocks();
    let mut data = Vec::new();
    words(&mut data, &[1, 1]);
    floats(&mut data, &[0., 1., 0., 0., 0.]);
    words(&mut data, &[0, 0]);
    packets[2].1 = data;
    let c = clip_bytes(&packets);
    let (_, keys) = keyframe::decode(&c, "source").unwrap();
    assert!(matches!(
        keys.keys.blocks[0].data.rotation,
        keyframe::Rotation::Quaternion { .. }
    ));
    failure(
        &s,
        &c,
        request(&s, &c, 0.),
        Default::default(),
        "sequence 0 controlled 0 interpolator 1 data 2: rotation key mapping is unapplied",
    );
}
#[test]
fn missing_component_groups_retain_exact_declared_niav_not_interpolator_constants() {
    let (s, _) = pair();
    let mut packets = blocks();
    let mut data = Vec::new();
    words(&mut data, &[0, 1, 5]);
    floats(&mut data, &[0., 4., 5., 6.]);
    words(&mut data, &[0]);
    packets[2].1 = data;
    let c = clip_bytes(&packets);
    let result = clip::evaluate(
        &s,
        &c,
        "absent scale",
        request(&s, &c, 0.),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        result.local,
        [[-6., 0., 0., 4.], [0., -6., 0., 5.], [0., 0., 6., 6.]]
    );
    assert_eq!(
        result.unapplied_interpolator_fields.scale_bits,
        12f32.to_bits()
    );
}
#[test]
fn explicit_time_endpoints_nonfinite_and_outside_source_keys() {
    let (s, c) = pair();
    for time in [-1., 3.] {
        assert!(
            clip::evaluate(
                &s,
                &c,
                "endpoint",
                request(&s, &c, time),
                Default::default()
            )
            .is_ok()
        );
    }
    for (time, reason) in [
        (f64::NAN, "must be finite"),
        (-2., "extrapolate"),
        (4., "extrapolate"),
    ] {
        failure(&s, &c, request(&s, &c, time), Default::default(), reason);
    }
    let result = clip::evaluate(
        &s,
        &c,
        "negative zero",
        request(&s, &c, -0.),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.requested_time_f64_bits, (-0f64).to_bits());
}
#[test]
fn controlled_ancestors_and_disconnected_destinations_remain_unavailable() {
    let (_, c) = pair();
    let s = skeleton(false, true, &[0]);
    failure(
        &s,
        &c,
        request(&s, &c, 0.),
        Default::default(),
        "ancestor 0 controller is unapplied",
    );
    let s = skeleton(false, false, &[]);
    failure(
        &s,
        &c,
        request(&s, &c, 0.),
        Default::default(),
        "not reachable from footer",
    );
}
#[test]
fn exact_retention_work_depth_and_input_limits_refuse_one_under() {
    let (s, c) = pair();
    let r = request(&s, &c, 0.);
    let result = clip::evaluate(&s, &c, "baseline", r, Default::default()).unwrap();
    let mut limits = Limits {
        combined_input_bytes: s.len() + c.len(),
        ..Default::default()
    };
    limits.pose.array_bytes = result.retained_bytes;
    limits.pose.work_units = result.work_units;
    limits.pose.ancestry_depth = 2;
    assert!(clip::evaluate(&s, &c, "exact", r, limits).is_ok());
    let mut under = limits;
    under.pose.array_bytes -= 1;
    failure(&s, &c, r, under, "array storage budget");
    let mut under = limits;
    under.pose.work_units -= 1;
    failure(&s, &c, r, under, "work budget");
    let mut under = limits;
    under.pose.ancestry_depth = 1;
    failure(&s, &c, r, under, "ancestry depth budget");
    failure(
        &s,
        &c,
        r,
        Limits {
            combined_input_bytes: limits.combined_input_bytes - 1,
            ..limits
        },
        "input byte budget",
    );
}
