//! Exact two-container binding, independent source packets and literal matrices.
use fallout_data::nif_animation::{
    clip::{self, Limits, Request},
    keyframe,
};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
const NAME: &[u8] = b"Bip01 Rotate\0\xff";

const SET_NAMES: [&[u8]; 7] = [
    b"Child\0\xff",
    b"Sibling",
    b"Root",
    b"Parent",
    b"OtherRoot",
    b"Other\x80",
    b"NiTransformController",
];
fn set_node(
    name: u32,
    controller: u32,
    rotation: [[f32; 3]; 3],
    translation: [f32; 3],
    scale: f32,
    children: &[u32],
) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[name, 0, controller, 0x89AB_CDEF]);
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
fn set_skeleton_blocks() -> Blocks {
    let id = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let r90 = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
    let rm90 = [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]];
    let r180 = [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]];
    let mut result = vec![
        ("NiNode", set_node(0, 6, r90, [99., 98., 97.], 9., &[])),
        ("NiNode", set_node(1, 7, id, [77., 76., 75.], 8., &[])),
        ("NiNode", set_node(2, NULL, r90, [10., -2., 3.], 2., &[3])),
        ("NiNode", set_node(3, 8, rm90, [99., 98., 97.], 3., &[0, 1])),
        ("NiNode", set_node(4, NULL, rm90, [-5., 7., 9.], 1., &[5])),
        ("NiNode", set_node(5, 9, r180, [9., 9., 9.], 7., &[])),
    ];
    for target in [0, 1, 3, 5] {
        let mut data = Vec::new();
        words(&mut data, &[NULL]);
        data.extend(0x004cu16.to_le_bytes());
        floats(&mut data, &[17., -9., 100., 101.]);
        words(&mut data, &[target, NULL]);
        result.push(("NiTransformController", data));
    }
    result
}
fn set_keys(a: [f32; 3], b: [f32; 3], scales: [f32; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0, 2, 1]);
    floats(&mut out, &[-2.]);
    floats(&mut out, &a);
    floats(&mut out, &[2.]);
    floats(&mut out, &b);
    words(&mut out, &[2, 1]);
    floats(&mut out, &[-2., scales[0], 2., scales[1]]);
    out
}
fn set_clip_blocks() -> Blocks {
    let mut sequence = Vec::new();
    words(&mut sequence, &[NULL, 4, 7]);
    for (ordinal, name) in [3, 0, 1, 5].into_iter().enumerate() {
        words(&mut sequence, &[1 + 2 * ordinal as u32, NULL]);
        sequence.push(11 * (ordinal as u8 + 1));
        words(&mut sequence, &[name, NULL, 6, NULL, NULL]);
    }
    floats(&mut sequence, &[0.25]);
    words(&mut sequence, &[NULL, 3]);
    floats(&mut sequence, &[17., 100., 101.]);
    words(&mut sequence, &[NULL, NULL]);
    sequence.extend(2u16.to_le_bytes());
    words(&mut sequence, &[NULL, NULL]);
    let mut result = vec![("NiControllerSequence", sequence)];
    for (ordinal, (a, b, scales)) in [
        ([1., 2., 3.], [3., -2., 5.], [1., 3.]),
        ([-1., 3., 1.], [3., 1., -1.], [2., 4.]),
        ([2., -4., -2.], [0., 2., 2.], [1., -1.]),
        ([-2., 1., 3.], [2., 3., 1.], [-2., -4.]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut interp = Vec::new();
        floats(&mut interp, &[100., 200., 300., 2., -3., 4., -5., 12.]);
        words(&mut interp, &[2 + 2 * ordinal as u32]);
        result.push(("NiTransformInterpolator", interp));
        result.push(("NiTransformData", set_keys(a, b, scales)));
    }
    result
}
fn set_pair() -> (Vec<u8>, Vec<u8>) {
    (
        container(&set_skeleton_blocks(), &SET_NAMES, &[2, 4]),
        container(&set_clip_blocks(), &SET_NAMES, &[0]),
    )
}
fn set_requests(s: &[u8], c: &[u8]) -> [Request<'static>; 4] {
    [(0, 1), (3, 0), (1, 2), (5, 3)].map(|(object, controlled_ordinal)| Request {
        expected_skeleton_sha256: Sha256::digest(s).into(),
        expected_clip_sha256: Sha256::digest(c).into(),
        object,
        node_name_bytes: SET_NAMES[object as usize],
        sequence: 0,
        controlled_ordinal,
        source_time: 0.,
    })
}

#[test]
fn external_packet_set_has_literal_higher_id_parent_child_shared_ancestor_and_separate_root_worlds()
{
    let (s, c) = set_pair();
    let requests = set_requests(&s, &c);
    let result = clip::evaluate_set(&s, &c, "literal set", &requests, Default::default()).unwrap();
    assert_eq!(
        result
            .objects
            .iter()
            .map(|v| v.channel.object.block)
            .collect::<Vec<_>>(),
        [0, 3, 1, 5]
    );
    assert_eq!(
        result
            .objects
            .iter()
            .map(|v| v.channel.local)
            .collect::<Vec<_>>(),
        [
            [[0., -3., 0., 1.], [3., 0., 0., 2.], [0., 0., 3., 0.]],
            [[0., 2., 0., 2.], [-2., 0., 0., 0.], [0., 0., 2., 4.]],
            [[0., 0., 0., 1.], [0., 0., 0., -1.], [0., 0., 0., 0.]],
            [[3., 0., 0., 0.], [0., 3., 0., 2.], [0., 0., -3., 2.]],
        ]
    );
    assert_eq!(
        result
            .objects
            .iter()
            .map(|v| v.source_world)
            .collect::<Vec<_>>(),
        [
            [[0., -12., 0., 14.], [12., 0., 0., 10.], [0., 0., 12., 11.]],
            [[4., 0., 0., 10.], [0., 4., 0., 2.], [0., 0., 4., 11.]],
            [[0., 0., 0., 14.], [0., 0., 0., -2.], [0., 0., 0., 11.]],
            [[0., 3., 0., -3.], [-3., 0., 0., 7.], [0., 0., -3., 11.]],
        ]
    );
    assert_eq!(
        result.objects[0]
            .ancestors
            .iter()
            .map(|a| (a.source.block, a.parent, a.applied_object))
            .collect::<Vec<_>>(),
        [(3, Some(2), Some(3)), (2, None, None)]
    );
    assert_eq!(
        result.objects[0].ancestors[0].effective_local,
        result.objects[1].channel.local
    );
    assert_eq!(
        (
            result.scene_decodes,
            result.animation_key_decodes,
            result.whole_source_sha256_computations,
            result.additional_span_hashes,
            result.propagated_objects
        ),
        (1, 1, 2, 19, 6)
    );
    assert_eq!(result.skeleton_sha256, format!("{:x}", Sha256::digest(&s)));
    assert_eq!(result.clip_sha256, format!("{:x}", Sha256::digest(&c)));
    for (value, request) in result.objects.iter().zip(requests) {
        assert_eq!(value.channel.node_name_bytes, request.node_name_bytes);
        assert_eq!(
            value.channel.requested_time_f64_bits,
            request.source_time.to_bits()
        );
        assert_eq!(
            value.channel.controlled_packet.priority,
            11 * (request.controlled_ordinal as u8 + 1)
        );
        assert_eq!(
            value.channel.unapplied_sequence_fields.weight_bits,
            0.25f32.to_bits()
        );
        assert_eq!(
            value.channel.unapplied_sequence_fields.frequency_bits,
            17f32.to_bits()
        );
        assert_eq!(
            value.channel.unapplied_interpolator_fields.scale_bits,
            12f32.to_bits()
        );
        let sb = &skeleton_span(&s, request.object);
        assert_eq!(
            (
                value.channel.object.offset,
                value.channel.object.bytes,
                value.channel.object.sha256.as_str()
            ),
            (sb.0, sb.1, sb.2.as_str())
        );
    }
    assert!(!result.retail_behavior_verified);
}
fn skeleton_span(bytes: &[u8], id: u32) -> (usize, usize, String) {
    let payloads = set_skeleton_blocks();
    // The authored writer has two footer roots. Derive offsets from its own
    // payload lengths, independently of the production container inspector.
    let start = bytes.len() - 12 - payloads.iter().map(|b| b.1.len()).sum::<usize>();
    let offset = start
        + payloads[..id as usize]
            .iter()
            .map(|b| b.1.len())
            .sum::<usize>();
    let size = payloads[id as usize].1.len();
    (
        offset,
        size,
        format!("{:x}", Sha256::digest(&bytes[offset..offset + size])),
    )
}

#[test]
fn external_packet_set_request_order_and_signed_zero_do_not_choose_channel_priority() {
    let (s, c) = set_pair();
    let requests = set_requests(&s, &c);
    let baseline = clip::evaluate_set(&s, &c, "set", &requests, Default::default()).unwrap();
    for order in [[3, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]] {
        let selected = order.map(|i| requests[i]);
        let result = clip::evaluate_set(&s, &c, "set", &selected, Default::default()).unwrap();
        for (value, index) in result.objects.iter().zip(order) {
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                serde_json::to_value(&baseline.objects[index]).unwrap()
            );
        }
        assert_eq!(
            (
                result.retained_bytes,
                result.work_units,
                result.propagated_objects
            ),
            (
                baseline.retained_bytes,
                baseline.work_units,
                baseline.propagated_objects
            )
        );
    }
    let mut minus_zero = requests;
    minus_zero[0].source_time = -0.;
    let result =
        clip::evaluate_set(&s, &c, "negative zero", &minus_zero, Default::default()).unwrap();
    assert_eq!(
        result.objects[0].channel.requested_time_f64_bits,
        (-0f64).to_bits()
    );
    assert_eq!(
        result.objects[0].source_world,
        baseline.objects[0].source_world
    );
}

#[test]
fn external_packet_set_missing_required_parent_duplicate_packet_identity_and_late_time_refuse_atomically()
 {
    let (s, c) = set_pair();
    let requests = set_requests(&s, &c);
    let before = (s.clone(), c.clone());
    let missing = [requests[0], requests[2], requests[3]];
    assert!(
        clip::evaluate_set(&s, &c, "missing", &missing, Default::default())
            .unwrap_err()
            .to_string()
            .contains("required ancestor 3 controller 8")
    );
    for kind in 0..9 {
        let mut bad = requests;
        match kind {
            0 => bad[3] = bad[0],
            1 => bad[3].controlled_ordinal = bad[0].controlled_ordinal,
            2 => bad[3].expected_skeleton_sha256[0] ^= 1,
            3 => bad[3].expected_clip_sha256[0] ^= 1,
            4 => bad[3].source_time = 3.,
            5 => bad[3].source_time = f64::NAN,
            6 => bad[3].node_name_bytes = b"wrong",
            7 => bad[3].sequence = 1,
            _ => bad[3].controlled_ordinal = 4,
        }
        assert!(
            clip::evaluate_set(&s, &c, "bad", &bad, Default::default()).is_err(),
            "case {kind}"
        );
    }
    assert_eq!((s, c), before);
}

#[test]
fn external_packet_set_ambiguous_absent_names_unreachable_nodes_property_type_and_rotation_refuse()
{
    let (s, c) = set_pair();
    for kind in 0..3 {
        let mut source = set_skeleton_blocks();
        match kind {
            0 => set(&mut source[1].1, 0, 0),
            1 => set(&mut source[5].1, 0, NULL),
            _ => {}
        }
        let bytes = container(&source, &SET_NAMES, if kind == 2 { &[4] } else { &[2, 4] });
        assert!(
            clip::evaluate_set(
                &bytes,
                &c,
                "source",
                &set_requests(&bytes, &c),
                Default::default()
            )
            .is_err()
        );
    }
    for kind in 0..4 {
        let mut packets = set_clip_blocks();
        match kind {
            0 => set(&mut packets[0].1, 25 + 3 * 29, 0),
            1 => set(&mut packets[0].1, 29 + 3 * 29, 2),
            2 => set(&mut packets[0].1, 21 + 3 * 29, NULL),
            _ => {
                let mut rotation = Vec::new();
                words(&mut rotation, &[1, 1]);
                floats(&mut rotation, &[0., 1., 0., 0., 0.]);
                words(&mut rotation, &[0, 0]);
                packets[8].1 = rotation;
            }
        }
        let bytes = container(&packets, &SET_NAMES, &[0]);
        assert!(
            clip::evaluate_set(
                &s,
                &bytes,
                "packet",
                &set_requests(&s, &bytes),
                Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn external_packet_set_source_input_names_sampling_forest_and_aggregate_ceilings_are_exact() {
    let (s, c) = set_pair();
    let requests = set_requests(&s, &c);
    let baseline = clip::evaluate_set(&s, &c, "baseline", &requests, Default::default()).unwrap();
    let names = requests
        .iter()
        .map(|r| r.node_name_bytes.len())
        .sum::<usize>();
    let mut exact = clip::SetLimits {
        combined_input_bytes: s.len() + c.len(),
        requests: 4,
        raw_name_bytes: names,
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        ancestry_depth: 3,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    exact.scene.input_bytes = s.len();
    exact.keys.animation.input_bytes = c.len();
    exact.scene.blocks = 10;
    exact.keys.animation.blocks = 9;
    clip::evaluate_set(&s, &c, "exact", &requests, exact).unwrap();
    for cap in 0..11 {
        let mut under = exact;
        match cap {
            0 => under.combined_input_bytes -= 1,
            1 => under.requests -= 1,
            2 => under.raw_name_bytes -= 1,
            3 => under.array_bytes -= 1,
            4 => under.work_units -= 1,
            5 => under.ancestry_depth -= 1,
            6 => under.scene.input_bytes -= 1,
            7 => under.keys.animation.input_bytes -= 1,
            8 => under.scene.blocks -= 1,
            9 => under.keys.animation.blocks -= 1,
            _ => under.decoder_array_admission_bytes -= 1,
        }
        assert!(
            clip::evaluate_set(&s, &c, "under", &requests, under).is_err(),
            "cap {cap}"
        );
    }
    let mut under = exact;
    under.decoder_check_admission_units -= 1;
    assert!(clip::evaluate_set(&s, &c, "decoder", &requests, under).is_err());
    let mut sampler = exact;
    sampler.sampling.validation_work = baseline.sample_work.validation_units;
    sampler.sampling.sampling_work = baseline.sample_work.sampling_units;
    clip::evaluate_set(&s, &c, "exact sampler", &requests, sampler).unwrap();
    for validation in [false, true] {
        let mut under = sampler;
        if validation {
            under.sampling.validation_work -= 1;
        } else {
            under.sampling.sampling_work -= 1;
        }
        assert!(clip::evaluate_set(&s, &c, "sampler under", &requests, under).is_err());
    }
    for malformed in [
        clip::SetLimits {
            keys: keyframe::Limits {
                max_combined_retained_bytes: usize::MAX,
                ..exact.keys
            },
            ..exact
        },
        clip::SetLimits {
            sampling: fallout_data::nif_animation::sampling::Limits {
                validation_work: usize::MAX,
                ..exact.sampling
            },
            ..exact
        },
        clip::SetLimits {
            scene: fallout_data::nif_scene::Limits {
                array_bytes: 0,
                ..exact.scene
            },
            ..exact
        },
    ] {
        assert!(clip::evaluate_set(&s, &c, "overflow/phase", &requests, malformed).is_err());
    }
}
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

fn prepared(s: &[u8], c: &[u8]) -> clip::PreparedClipSource {
    clip::PreparedClipSource::prepare(
        s,
        c,
        "prepared literal clip",
        request(s, c, 0.).into(),
        Default::default(),
    )
    .ok()
    .unwrap()
}
fn sample(
    prepared: &clip::PreparedClipSource,
    s: [u8; 32],
    c: [u8; 32],
    times: &[f64],
    limits: clip::BatchLimits,
) -> fallout_data::Result<clip::ClipBatch> {
    prepared.sample_many("prepared literal clip", s, c, times, limits)
}
fn normalized(result: &clip::Evaluation) -> serde_json::Value {
    let mut value = serde_json::to_value(result).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("retained_bytes");
    object.remove("work_units");
    value
}
#[test]
fn prepared_clip_owns_two_sources_and_one_binding_across_ordered_reuse() {
    let (mut s, mut c) = pair();
    let r = request(&s, &c, 0.);
    let times = [3., -1., 0., -0., 3., 1.];
    let legacy: Vec<_> = times
        .iter()
        .map(|&time| {
            clip::evaluate(
                &s,
                &c,
                "literal",
                Request {
                    source_time: time,
                    ..r
                },
                Default::default(),
            )
            .unwrap()
        })
        .collect();
    let p = prepared(&s, &c);
    let initial = serde_json::to_value(p.usage()).unwrap();
    s.fill(0xA5);
    c.fill(0x5A);
    drop(s);
    drop(c);
    let batch = sample(
        &p,
        r.expected_skeleton_sha256,
        r.expected_clip_sha256,
        &times,
        Default::default(),
    )
    .unwrap();
    let local = [
        [[-2., 0., 0., 0.], [0., -2., 0., 8.], [0., 0., 2., 6.]],
        [[2., 0., 0., 4.], [0., 2., 0., 0.], [0., 0., -2., -2.]],
        [[1., 0., 0., 3.], [0., 1., 0., 2.], [0., 0., -1., 0.]],
        [[1., 0., 0., 3.], [0., 1., 0., 2.], [0., 0., -1., 0.]],
        [[-2., 0., 0., 0.], [0., -2., 0., 8.], [0., 0., 2., 6.]],
        [[0., 0., 0., 2.], [0., 0., 0., 4.], [0., 0., 0., 2.]],
    ];
    let world = [
        [[0., 4., 0., -14.], [-4., 0., 0., -3.], [0., 0., 4., 17.]],
        [[0., -4., 0., 2.], [4., 0., 0., 5.], [0., 0., -4., 1.]],
        [[0., -2., 0., -2.], [2., 0., 0., 3.], [0., 0., -2., 5.]],
        [[0., -2., 0., -2.], [2., 0., 0., 3.], [0., 0., -2., 5.]],
        [[0., 4., 0., -14.], [-4., 0., 0., -3.], [0., 0., 4., 17.]],
        [[0., 0., 0., -6.], [0., 0., 0., 1.], [0., 0., 0., 9.]],
    ];
    for (ordinal, result) in batch.samples.iter().enumerate() {
        assert_eq!(result.local, local[ordinal]);
        assert_eq!(result.source_world, world[ordinal]);
        assert_eq!(result.requested_time_f64_bits, times[ordinal].to_bits());
        assert_eq!(normalized(result), normalized(&legacy[ordinal]));
        assert!(result.work_units < legacy[ordinal].work_units);
    }
    assert_eq!(batch.preparation.animation_key_decodes, 1);
    assert_eq!(batch.preparation.scene_decodes, 1);
    assert_eq!(batch.preparation.target_bindings, 1);
    assert_eq!(batch.preparation.whole_source_sha256_computations, 2);
    assert_eq!(batch.preparation.additional_span_hashes, 5);
    assert_eq!(p.skeleton_sha256(), batch.skeleton_sha256);
    assert_eq!(p.clip_sha256(), batch.clip_sha256);
    let again = sample(
        &p,
        r.expected_skeleton_sha256,
        r.expected_clip_sha256,
        &times,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&batch).unwrap(),
        serde_json::to_vec(&again).unwrap()
    );
    assert_eq!(initial, serde_json::to_value(p.usage()).unwrap());
    assert!(!batch.retail_behavior_verified);
}
#[test]
fn prepared_clip_source_binding_refusals_preserve_inputs() {
    let (s, c) = pair();
    let r: clip::BindingRequest = request(&s, &c, 0.).into();
    let fail = |s: &[u8], c: &[u8], r: clip::BindingRequest<'_>, reason: &str| {
        let before = (s.to_vec(), c.to_vec());
        let error = clip::PreparedClipSource::prepare(s, c, "refusal", r, Default::default())
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(reason), "{reason}: {error}");
        assert_eq!(s, before.0);
        assert_eq!(c, before.1);
    };
    fail(b"changed skeleton", &c, r, "skeleton source SHA256 differs");
    fail(&s, b"changed clip", r, "clip source SHA256 differs");
    fail(
        &s,
        &c,
        clip::BindingRequest {
            node_name_bytes: b"absent",
            ..r
        },
        "raw node name differs",
    );
    fail(
        &s,
        &c,
        clip::BindingRequest { object: 99, ..r },
        "object is not decoded",
    );
    fail(
        &s,
        &c,
        clip::BindingRequest { sequence: 1, ..r },
        "not decoded NiControllerSequence",
    );
    fail(
        &s,
        &c,
        clip::BindingRequest {
            controlled_ordinal: 1,
            ..r
        },
        "ordinal is out of range",
    );
    for (s, reason) in [
        (skeleton(true, false, &[0]), "not unique"),
        (
            skeleton(false, true, &[0]),
            "ancestor 0 controller is unapplied",
        ),
        (skeleton(false, false, &[]), "not reachable from footer"),
    ] {
        fail(&s, &c, request(&s, &c, 0.).into(), reason);
    }
    for (offset, value, reason) in [
        (21, 2, "packet raw node name differs"),
        (25, 0, "property/controller/identifier"),
        (29, 2, "transform-controller type is unavailable"),
        (12, NULL, "interpolator is absent"),
    ] {
        let mut packets = blocks();
        set(&mut packets[0].1, offset, value);
        let c = clip_bytes(&packets);
        fail(&s, &c, request(&s, &c, 0.).into(), reason);
    }
    let mut packets = blocks();
    let mut data = Vec::new();
    words(&mut data, &[1, 1]);
    floats(&mut data, &[0., 1., 0., 0., 0.]);
    words(&mut data, &[0, 0]);
    packets[2].1 = data;
    let c = clip_bytes(&packets);
    fail(
        &s,
        &c,
        request(&s, &c, 0.).into(),
        "rotation key mapping is unapplied",
    );
}
#[test]
fn prepared_clip_preparation_limits_are_separate_and_exact() {
    let (s, c) = pair();
    let r: clip::BindingRequest = request(&s, &c, 0.).into();
    let baseline = prepared(&s, &c).usage();
    let mut exact = clip::PreparationLimits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    exact.source.combined_input_bytes = s.len() + c.len();
    exact.source.pose.ancestry_depth = 2;
    assert!(clip::PreparedClipSource::prepare(&s, &c, "exact", r, exact).is_ok());
    let mut under = exact;
    under.array_bytes -= 1;
    let mut cases = vec![(under, "array storage budget")];
    under = exact;
    under.work_units -= 1;
    cases.push((under, "work budget"));
    under = exact;
    under.decoder_array_admission_bytes -= 1;
    cases.push((under, "decoder array admission"));
    under = exact;
    under.decoder_check_admission_units -= 1;
    cases.push((under, "decoder check admission"));
    under = exact;
    under.source.combined_input_bytes -= 1;
    cases.push((under, "input byte budget"));
    under = exact;
    under.source.pose.ancestry_depth = 1;
    cases.push((under, "ancestry depth budget"));
    under = exact;
    under.source.pose.scene.input_bytes = s.len() - 1;
    cases.push((under, "source input byte budget"));
    under = exact;
    under.source.pose.keys.animation.input_bytes = c.len() - 1;
    cases.push((under, "source input byte budget"));
    for (limits, reason) in cases {
        let error = clip::PreparedClipSource::prepare(&s, &c, "one under", r, limits)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(reason), "{reason}: {error}");
    }
}
#[test]
fn prepared_clip_batch_and_sample_caps_refuse_without_complete_output() {
    let (s, c) = pair();
    let p = prepared(&s, &c);
    let r = request(&s, &c, 0.);
    let times = [-1., 0., 3.];
    let baseline = sample(
        &p,
        r.expected_skeleton_sha256,
        r.expected_clip_sha256,
        &times,
        Default::default(),
    )
    .unwrap();
    let mut exact = clip::BatchLimits {
        samples: 3,
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        sampling: fallout_data::nif_animation::sampling::Limits {
            validation_work: baseline.sample_work.validation_units,
            sampling_work: baseline.sample_work.sampling_units,
        },
        ..Default::default()
    };
    exact.sample.array_bytes = baseline
        .samples
        .iter()
        .map(|s| s.retained_bytes)
        .max()
        .unwrap();
    exact.sample.work_units = baseline.samples.iter().map(|s| s.work_units).max().unwrap();
    exact.sample.sampling.validation_work = baseline
        .samples
        .iter()
        .map(|s| s.sample_work.validation_units)
        .max()
        .unwrap();
    exact.sample.sampling.sampling_work = baseline
        .samples
        .iter()
        .map(|s| s.sample_work.sampling_units)
        .max()
        .unwrap();
    exact.sample.ancestry_depth = 2;
    assert!(
        sample(
            &p,
            r.expected_skeleton_sha256,
            r.expected_clip_sha256,
            &times,
            exact
        )
        .is_ok()
    );
    let mut under = exact;
    under.array_bytes -= 1;
    let mut cases = vec![under];
    under = exact;
    under.work_units -= 1;
    cases.push(under);
    under = exact;
    under.samples = 2;
    cases.push(under);
    under = exact;
    under.sample.array_bytes -= 1;
    cases.push(under);
    under = exact;
    under.sample.work_units -= 1;
    cases.push(under);
    under = exact;
    under.sampling.validation_work -= 1;
    cases.push(under);
    under = exact;
    under.sampling.sampling_work -= 1;
    cases.push(under);
    under = exact;
    under.sample.sampling.validation_work -= 1;
    cases.push(under);
    under = exact;
    under.sample.sampling.sampling_work -= 1;
    cases.push(under);
    under = exact;
    under.sample.ancestry_depth = 1;
    cases.push(under);
    for limits in cases {
        assert!(
            sample(
                &p,
                r.expected_skeleton_sha256,
                r.expected_clip_sha256,
                &times,
                limits
            )
            .is_err()
        );
    }
    assert!(
        sample(
            &p,
            r.expected_skeleton_sha256,
            r.expected_clip_sha256,
            &[],
            exact
        )
        .is_err()
    );
    assert!(
        sample(&p, [0; 32], r.expected_clip_sha256, &times, exact)
            .unwrap_err()
            .to_string()
            .contains("skeleton source SHA256 differs")
    );
    assert!(
        sample(&p, r.expected_skeleton_sha256, [0; 32], &times, exact)
            .unwrap_err()
            .to_string()
            .contains("clip source SHA256 differs")
    );
}
#[test]
fn prepared_clip_late_failure_does_not_mutate_sealed_binding() {
    let (s, c) = pair();
    let p = prepared(&s, &c);
    let r = request(&s, &c, 0.);
    let before = sample(
        &p,
        r.expected_skeleton_sha256,
        r.expected_clip_sha256,
        &[0.],
        Default::default(),
    )
    .unwrap();
    for (times, reason) in [
        ([0., 3., f64::NAN], "must be finite"),
        ([0., 3., 4.], "extrapolate"),
        ([0., 3., f64::INFINITY], "must be finite"),
    ] {
        let error = sample(
            &p,
            r.expected_skeleton_sha256,
            r.expected_clip_sha256,
            &times,
            Default::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains(reason), "{error}");
    }
    let after = sample(
        &p,
        r.expected_skeleton_sha256,
        r.expected_clip_sha256,
        &[0.],
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&after).unwrap()
    );
}
#[test]
fn prepared_clip_missing_groups_preserve_raw_local_and_unapplied_clocks() {
    let (s, _) = pair();
    let mut packets = blocks();
    let mut data = Vec::new();
    words(&mut data, &[0, 1, 5]);
    floats(&mut data, &[0., 4., 5., 6.]);
    words(&mut data, &[0]);
    packets[2].1 = data;
    let c = clip_bytes(&packets);
    let p = prepared(&s, &c);
    let r = request(&s, &c, 0.);
    let batch = sample(
        &p,
        r.expected_skeleton_sha256,
        r.expected_clip_sha256,
        &[-0., 0.],
        Default::default(),
    )
    .unwrap();
    for entry in &batch.samples {
        assert_eq!(
            entry.local,
            [[-6., 0., 0., 4.], [0., -6., 0., 5.], [0., 0., 6., 6.]]
        );
        assert_eq!(
            entry.source_world,
            [[0., 12., 0., -8.], [-12., 0., 0., 5.], [0., 0., 12., 17.]]
        );
        assert_eq!(
            entry.unapplied_interpolator_fields.scale_bits,
            12f32.to_bits()
        );
        assert_eq!(
            entry.unapplied_sequence_fields.frequency_bits,
            17f32.to_bits()
        );
        assert_eq!(entry.unapplied_sequence_fields.start_bits, 100f32.to_bits());
        assert_eq!(entry.unapplied_sequence_fields.cycle_type, 3);
    }
    assert_eq!(batch.samples[0].requested_time_f64_bits, (-0f64).to_bits());
}
