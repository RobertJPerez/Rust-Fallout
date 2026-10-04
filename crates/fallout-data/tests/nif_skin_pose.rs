//! Authored byte fixtures and analytic expectations independent of pose helpers.
use fallout_data::nif_skin::pose::{self, Limits, Request, WeightPolicy};
use sha2::Digest;

const NULL: u32 = u32::MAX;
const ID: [[f32; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
const R90: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
const RM90: [[f32; 3]; 3] = [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]];

fn bounds_request(
    bytes: &[u8],
    selected: fallout_data::nif_skin::bounds::Pose,
) -> fallout_data::nif_skin::bounds::Request {
    fallout_data::nif_skin::bounds::Request {
        expected_source_sha256: source_digest(bytes),
        skin: request(),
        pose: selected,
    }
}

#[test]
fn stored_bounds_have_literal_noncommuting_coordinates_exact_identity_and_separate_placement() {
    use fallout_data::nif_skin::bounds;
    let bytes = container(&subset_fixture(), &[0]);
    let selected = bounds_request(&bytes, bounds::Pose::Stored);
    let result = bounds::evaluate(&bytes, "bounds", selected, Default::default()).unwrap();
    assert_eq!(
        (
            result.geometry,
            result.geometry_data,
            result.instance,
            result.skin_data,
            result.skeleton_root,
            result.vertices
        ),
        (3, 6, 4, 5, 0, 3)
    );
    assert_eq!(result.coordinates.min, [-9., -3., 6.]);
    assert_eq!(result.coordinates.max, [0., 11., 10.]);
    assert_eq!(
        result.coordinates.min_f64_bits,
        [-9., -3., 6.].map(f64::to_bits)
    );
    assert_eq!(
        result.coordinates.max_f64_bits,
        [0., 11., 10.].map(f64::to_bits)
    );
    assert_eq!(
        result.skin_to_source_world,
        [[1.5, 0., 0., 13.], [0., 1.5, 0., 15.5], [0., 0., 1.5, 24.]]
    );
    assert_eq!(result.frame, bounds::FRAME);
    assert_eq!(
        result.source_sha256,
        format!("{:x}", sha2::Sha256::digest(&bytes))
    );
    assert_eq!(result.mode, "stored");
    assert!(result.sample.is_none());
    assert!(!result.retail_behavior_verified);
    let existing = pose::evaluate(&bytes, "bounds", selected.skin, Default::default()).unwrap();
    assert_eq!(
        (result.pose_retained_bytes, result.pose_work_units),
        (existing.retained_bytes, existing.work_units)
    );
    for point in existing.positions {
        for (axis, coordinate) in point.into_iter().enumerate() {
            assert!(
                coordinate >= result.coordinates.min[axis]
                    && coordinate <= result.coordinates.max[axis]
            );
        }
    }
}

#[test]
fn sampled_bounds_keep_complete_channel_provenance_and_root_motion_only_in_placement() {
    use fallout_data::nif_skin::bounds;
    for (object, minimum, maximum) in [
        (2, [-4.75, -0.5, 1.], [0., 1.1875, 2.8125]),
        (0, [-3.5, -0.5, 0.5], [0., 1., 1.5]),
    ] {
        let bytes = container(&sampled_fixture(object), &[0]);
        let channel = animation_request(object, 1.);
        let selected = bounds_request(
            &bytes,
            bounds::Pose::Sampled {
                controller_policy: pose::ControllerPolicy::RefuseOtherRequired,
                animation: channel,
            },
        );
        let before = pose::evaluate_sampled(
            &bytes,
            "bounds",
            sample_request(&bytes),
            channel,
            Default::default(),
        )
        .unwrap();
        let result = bounds::evaluate(&bytes, "bounds", selected, Default::default()).unwrap();
        assert_eq!(result.coordinates.min, minimum);
        assert_eq!(result.coordinates.max, maximum);
        assert_eq!(
            result.skin_to_source_world,
            before.skin.skin_to_source_world
        );
        assert_eq!(
            (result.pose_retained_bytes, result.pose_work_units),
            (before.retained_bytes, before.work_units)
        );
        assert_eq!(
            serde_json::to_value(result.sample.as_ref().unwrap()).unwrap(),
            serde_json::to_value(&before.sample).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&result.unapplied_controllers).unwrap(),
            serde_json::to_value(&before.skin.unapplied_controllers).unwrap()
        );
        let after = pose::evaluate_sampled(
            &bytes,
            "bounds",
            sample_request(&bytes),
            channel,
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&after).unwrap()
        );
    }
}

#[test]
fn bounds_keep_raw_duplicate_nonunit_zero_terms_and_refuse_zero_total_or_nonfinite_vertices() {
    use fallout_data::nif_skin::bounds;
    let mut blocks = fixture();
    blocks[5].1 = skin(
        &[
            vec![(0, 0.25), (0, 0.5), (1, 1.), (2, 0.)],
            vec![(0, 0.75), (2, 1.)],
        ],
        None,
    );
    let bytes = container(&blocks, &[0]);
    let mut selected = bounds_request(&bytes, bounds::Pose::Stored);
    assert!(bounds::evaluate(&bytes, "unit", selected, Default::default()).is_err());
    selected.skin.weights = WeightPolicy::PreserveRawNonnegative;
    let with_normals = bounds::evaluate(&bytes, "raw", selected, Default::default()).unwrap();
    assert_eq!(with_normals.coordinates.min, [-3.5, -0.5, 0.5]);
    assert_eq!(with_normals.coordinates.max, [0., 1.5, 2.25]);
    let before = pose::evaluate(&bytes, "raw", selected.skin, Default::default()).unwrap();
    assert_eq!(before.positions[0], [-2.25, 1.5, 2.25]);
    assert_eq!(before.weight_sums, [1.5, 1., 1.]);
    assert_eq!(before.normals[0], [0., 0.75, 0.]);
    blocks[6].1[47] = 0;
    blocks[6].1.drain(48..84);
    let absent_bytes = container(&blocks, &[0]);
    let absent = bounds::evaluate(
        &absent_bytes,
        "absent",
        bounds::Request {
            expected_source_sha256: source_digest(&absent_bytes),
            ..selected
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        absent.coordinates.min_f64_bits,
        with_normals.coordinates.min_f64_bits
    );
    assert_eq!(
        absent.coordinates.max_f64_bits,
        with_normals.coordinates.max_f64_bits
    );
    blocks[5].1 = skin(&[vec![(0, 1.), (1, 1.), (2, 0.)], vec![]], None);
    let zero = container(&blocks, &[0]);
    let error = bounds::evaluate(
        &zero,
        "zero total",
        bounds::Request {
            expected_source_sha256: source_digest(&zero),
            ..selected
        },
        Default::default(),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("no positive finite weight sum"),
        "{error}"
    );
    blocks = fixture();
    blocks[6].1[9..13].copy_from_slice(&f32::INFINITY.to_le_bytes());
    let nonfinite = container(&blocks, &[0]);
    assert!(
        bounds::evaluate(
            &nonfinite,
            "nonfinite",
            bounds_request(&nonfinite, bounds::Pose::Stored),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn bounds_phase_aggregate_admission_vertex_input_and_full_scan_caps_have_exact_ceilings() {
    use fallout_data::nif_skin::bounds;
    for sampled in [false, true] {
        let bytes = container(&sampled_fixture(2), &[0]);
        let mode = if sampled {
            bounds::Pose::Sampled {
                controller_policy: pose::ControllerPolicy::RefuseOtherRequired,
                animation: animation_request(2, 1.),
            }
        } else {
            bounds::Pose::Stored
        };
        let selected = bounds_request(&bytes, mode);
        let baseline = bounds::evaluate(&bytes, "budget", selected, Default::default()).unwrap();
        assert_eq!(
            baseline.retained_bytes,
            baseline.extra_retained_bytes + baseline.pose_retained_bytes
        );
        assert_eq!(
            baseline.work_units,
            baseline.extra_work_units + baseline.pose_work_units
        );
        assert_eq!(
            baseline.extra_work_units,
            6 * bytes.len() + 9 * baseline.vertices
        );
        let mut exact = bounds::Limits {
            input_bytes: bytes.len(),
            vertices: 3,
            extra_array_bytes: baseline.extra_retained_bytes,
            extra_work_units: baseline.extra_work_units,
            array_bytes: baseline.retained_bytes,
            work_units: baseline.work_units,
            decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
            decoder_check_admission_units: baseline.decoder_check_admission_units,
            ..Default::default()
        };
        if sampled {
            exact.sampled.array_bytes = baseline.pose_retained_bytes;
            exact.sampled.work_units = baseline.pose_work_units;
        } else {
            exact.stored.array_bytes = baseline.pose_retained_bytes;
            exact.stored.work_units = baseline.pose_work_units;
        }
        bounds::evaluate(&bytes, "exact", selected, exact).unwrap();
        for ceiling in 0..10 {
            let mut under = exact;
            match ceiling {
                0 => under.input_bytes -= 1,
                1 => under.vertices -= 1,
                2 => under.extra_array_bytes -= 1,
                3 => under.extra_work_units -= 1,
                4 => under.array_bytes -= 1,
                5 => under.work_units -= 1,
                6 => under.decoder_array_admission_bytes -= 1,
                7 => under.decoder_check_admission_units -= 1,
                8 if sampled => under.sampled.array_bytes -= 1,
                9 if sampled => under.sampled.work_units -= 1,
                8 => under.stored.array_bytes -= 1,
                _ => under.stored.work_units -= 1,
            }
            assert!(
                bounds::evaluate(&bytes, "one under", selected, under).is_err(),
                "ceiling {ceiling}, sampled {sampled}"
            );
        }
        let mut stale = selected;
        stale.expected_source_sha256[0] ^= 1;
        assert!(
            bounds::evaluate(&bytes, "stale", stale, exact)
                .unwrap_err()
                .to_string()
                .contains("SHA256 differs")
        );
        for phase in 0..if sampled { 3 } else { 1 } {
            let mut limits = exact;
            match phase {
                0 if sampled => {
                    limits.sampled.skin.source.partition.skin.scene.input_bytes = bytes.len() - 1
                }
                0 => limits.stored.source.partition.skin.scene.input_bytes = bytes.len() - 1,
                1 => limits.sampled.animation.scene.input_bytes = bytes.len() - 1,
                _ => limits.sampled.animation.keys.animation.input_bytes = bytes.len() - 1,
            }
            assert!(
                bounds::evaluate(&bytes, "before SHA", stale, limits)
                    .unwrap_err()
                    .to_string()
                    .contains("source input byte budget exceeded")
            );
        }
        let mut overflow = exact;
        if sampled {
            overflow.sampled.skin.source.array_bytes = usize::MAX;
        } else {
            overflow.stored.source.array_bytes = usize::MAX;
        }
        assert!(bounds::evaluate(&bytes, "overflow", selected, overflow).is_err());
    }
}

#[test]
fn compact_bounds_charge_the_complete_large_skin_deformation_even_after_vertex_arrays_are_dropped()
{
    use fallout_data::nif_skin::bounds;
    let count = 4096u16;
    let mut blocks = fixture();
    let mut data = Vec::new();
    words(&mut data, &[0]);
    shorts(&mut data, &[count]);
    data.extend([0, 0, 1]);
    for _ in 0..count {
        floats(&mut data, &[1., 2., 3.]);
    }
    shorts(&mut data, &[0]);
    data.push(0);
    floats(&mut data, &[0., 0., 0., 10.]);
    data.push(0);
    shorts(&mut data, &[0]);
    words(&mut data, &[NULL]);
    shorts(&mut data, &[0]);
    words(&mut data, &[0]);
    data.push(0);
    shorts(&mut data, &[0]);
    blocks[6].1 = data;
    blocks[5].1 = skin(&[(0..count).map(|v| (v, 1.)).collect(), vec![]], None);
    let bytes = container(&blocks, &[0]);
    let selected = bounds_request(&bytes, bounds::Pose::Stored);
    let result = bounds::evaluate(&bytes, "large", selected, Default::default()).unwrap();
    assert_eq!(result.coordinates.min, [-1.5, 1., 1.5]);
    assert_eq!(result.coordinates.max, result.coordinates.min);
    assert_eq!(result.vertices, usize::from(count));
    assert!(result.pose_retained_bytes >= usize::from(count) * 32);
    assert_eq!(
        result.extra_work_units,
        6 * bytes.len() + usize::from(count) * 9
    );
    assert!(
        bounds::evaluate(
            &bytes,
            "intermediates",
            selected,
            bounds::Limits {
                array_bytes: result.extra_retained_bytes,
                ..Default::default()
            }
        )
        .is_err()
    );
    let mut insufficient = bounds::Limits::default();
    insufficient.stored.array_bytes = result.pose_retained_bytes - 1;
    assert!(bounds::evaluate(&bytes, "phase", selected, insufficient).is_err());
}

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

fn shared_fixture() -> Vec<(&'static str, Vec<u8>)> {
    let mut blocks = fixture();
    blocks[0].1 = node(R90, [10., 20., 30.], 3., &[1, 3, 7]);
    let mut shape = av(RM90, [777., -777., 1000.], 11.);
    words(&mut shape, &[10, 8, 0, NULL]);
    shape.push(0);
    let mut instance = Vec::new();
    words(&mut instance, &[9, NULL, 0, 2, 2, 1]);
    let mut second_skin = Vec::new();
    transform(&mut second_skin, RM90, [3., 4., 5.], 2.);
    words(&mut second_skin, &[2]);
    second_skin.push(1);
    for weight in [0.75, 0.25] {
        transform(&mut second_skin, ID, [0., 0., 0.], 1.);
        floats(&mut second_skin, &[0., 0., 0., 10.]);
        shorts(&mut second_skin, &[3]);
        for vertex in 0..3 {
            shorts(&mut second_skin, &[vertex]);
            floats(&mut second_skin, &[weight]);
        }
    }
    blocks.extend([
        ("NiTriShape", shape),
        ("NiSkinInstance", instance),
        ("NiSkinData", second_skin),
        ("NiTriShapeData", mesh()),
    ]);
    blocks
}
fn shared_requests() -> [Request; 2] {
    [
        request(),
        Request {
            geometry: 7,
            weights: WeightPolicy::PreserveRawNonnegative,
        },
    ]
}
fn source_digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

fn subset_packet(map: &[u16], strip: bool) -> Vec<u8> {
    let mut out = Vec::new();
    shorts(
        &mut out,
        &[map.len() as u16, 1, 2, u16::from(strip), 4, 1, 0],
    );
    out.push(1);
    shorts(&mut out, map);
    out.push(1);
    // Deliberately differs from NiSkinData: this producer projects that existing
    // CPU geometry pose, without substituting a hardware partition weight rule.
    for _ in map {
        floats(&mut out, &[0.75, 0., 0.25, 0.]);
    }
    if strip {
        shorts(&mut out, &[4]);
    }
    out.push(1);
    shorts(&mut out, if strip { &[1, 0, 0, 1] } else { &[2, 0, 1] });
    out.push(1);
    for _ in map {
        out.extend([0, 1, 0, 1]);
    }
    out
}
fn subset_fixture() -> Vec<(&'static str, Vec<u8>)> {
    let mut blocks = fixture();
    blocks[2].1 = node(R90, [0., 5., 0.], 2., &[]);
    blocks[5].1 = skin(
        &[vec![(0, 0.25), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        Some((R90, [-2., 3., 4.], 2.)),
    );
    blocks[4].1[4..8].copy_from_slice(&7u32.to_le_bytes());
    let mut part = Vec::new();
    words(&mut part, &[2]);
    part.extend(subset_packet(&[2, 0, 2], false));
    part.extend(subset_packet(&[1, 2], true));
    blocks.push(("NiSkinPartition", part));
    blocks
}
fn subset_request(bytes: &[u8], ordinal: usize) -> pose::partition::Request {
    pose::partition::Request {
        expected_source_sha256: source_digest(bytes),
        skin: request(),
        partition_block: 7,
        partition_ordinal: ordinal,
    }
}
#[test]
fn partition_subset_noncommuting_source_pose_has_literal_coordinates_palette_frame_and_identity() {
    let bytes = container(&subset_fixture(), &[0]);
    let result = pose::partition::evaluate(
        &bytes,
        "subset",
        subset_request(&bytes, 0),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        (
            result.geometry,
            result.geometry_data,
            result.instance,
            result.skin_data,
            result.skeleton_root,
            result.partition.block,
            result.partition.ordinal,
            result.source_vertex_count
        ),
        (3, 6, 4, 5, 0, 7, 0, 3)
    );
    assert_eq!(
        result
            .vertices
            .iter()
            .map(|v| (
                v.partition_vertex,
                v.source_vertex,
                v.position,
                v.normal,
                v.weight_sum
            ))
            .collect::<Vec<_>>(),
        [
            (0, 2, [-6., -3., 6.], Some([-2., 0., 0.]), 1.),
            (1, 0, [-9., 5., 10.], Some([-2., 0., 0.]), 1.),
            (2, 2, [-6., -3., 6.], Some([-2., 0., 0.]), 1.)
        ]
    );
    assert_eq!(
        result.palette[0].matrix,
        [[0., -2., 0., -2.], [2., 0., 0., 3.], [0., 0., 2., 4.]]
    );
    assert_eq!(
        result.palette[1].matrix,
        [[0., -2., 0., -6.], [2., 0., 0., 3.], [0., 0., 2., 4.]]
    );
    assert_eq!(
        result.skin_to_source_world,
        [[1.5, 0., 0., 13.], [0., 1.5, 0., 15.5], [0., 0., 1.5, 24.]]
    );
    assert_eq!(
        result
            .partition_palette
            .iter()
            .map(|p| (p.local_bone, p.global_bone_ordinal, p.source_bone_node))
            .collect::<Vec<_>>(),
        [(0, 1, 2), (1, 0, 1)]
    );
    assert_eq!(result.source_to_partition_offsets, [0, 1, 1, 3]);
    assert_eq!(result.source_to_partition_vertices, [1, 0, 2]);
    assert_eq!(
        result.source_sha256,
        format!("{:x}", sha2::Sha256::digest(&bytes))
    );
    let payload = &subset_fixture()[7].1;
    let offset = bytes
        .windows(payload.len())
        .position(|p| p == payload)
        .unwrap();
    assert_eq!(
        (result.partition.offset, result.partition.bytes),
        (offset, payload.len())
    );
    assert_eq!(
        (
            result.usage.binding_decodes,
            result.usage.scene_decodes,
            result.usage.geometry_deformations
        ),
        (1, 1, 1)
    );
    assert!(!result.retail_behavior_verified);
}
#[test]
fn partition_subset_retains_triangle_strip_order_repeated_vertices_and_observed_body_parts() {
    let mut blocks = subset_fixture();
    blocks[4].0 = "BSDismemberSkinInstance";
    words(&mut blocks[4].1, &[2]);
    shorts(&mut blocks[4].1, &[257, 7000, 1, 10]);
    let bytes = container(&blocks, &[0]);
    let tri =
        pose::partition::evaluate(&bytes, "tri", subset_request(&bytes, 0), Default::default())
            .unwrap();
    let strip = pose::partition::evaluate(
        &bytes,
        "strip",
        subset_request(&bytes, 1),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(tri.topology).unwrap(),
        serde_json::json!({"kind":"triangles","triangles":[[2,0,1]]})
    );
    assert_eq!(
        serde_json::to_value(strip.topology).unwrap(),
        serde_json::json!({"kind":"strips","lengths":[4],"strips":[[1,0,0,1]]})
    );
    assert_eq!(
        (
            tri.body_part.unwrap().flags,
            tri.body_part.unwrap().body_part
        ),
        (257, 7000)
    );
    assert_eq!(
        (
            strip.body_part.unwrap().flags,
            strip.body_part.unwrap().body_part
        ),
        (1, 10)
    );
    assert_eq!(
        strip
            .vertices
            .iter()
            .map(|v| v.position)
            .collect::<Vec<_>>(),
        [[0., 11., 8.], [-6., -3., 6.]]
    );
    assert_eq!(strip.source_to_partition_offsets, [0, 0, 1, 2]);
    assert_eq!(strip.source_to_partition_vertices, [0, 1]);
    assert_eq!(strip.declared_triangles, 1);
}
#[test]
fn partition_subset_identity_membership_body_count_and_missing_arrays_refuse_atomically() {
    let blocks = subset_fixture();
    let bytes = container(&blocks, &[0]);
    let request = subset_request(&bytes, 0);
    let mut stale = request;
    stale.expected_source_sha256[0] ^= 1;
    for (request, expected) in [
        (stale, "SHA256 differs"),
        (
            pose::partition::Request {
                skin: Request {
                    geometry: 0,
                    ..request.skin
                },
                ..request
            },
            "no decoded skin owner",
        ),
        (
            pose::partition::Request {
                partition_block: 5,
                ..request
            },
            "partition link differs",
        ),
        (
            pose::partition::Request {
                partition_ordinal: 2,
                ..request
            },
            "ordinal unavailable",
        ),
    ] {
        let error =
            pose::partition::evaluate(&bytes, "identity", request, Default::default()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut body = blocks.clone();
    body[4].0 = "BSDismemberSkinInstance";
    words(&mut body[4].1, &[1]);
    shorts(&mut body[4].1, &[257, 7000]);
    let mut bad_palette = blocks.clone();
    bad_palette[7].1[14..16].copy_from_slice(&2u16.to_le_bytes());
    let mut absent = blocks;
    let mut part = Vec::new();
    words(&mut part, &[1]);
    shorts(&mut part, &[3, 0, 2, 0, 4, 1, 0]);
    part.extend([0, 0, 0, 0]);
    absent[7].1 = part;
    for (blocks, expected) in [
        (body, "body-part association unavailable"),
        (bad_palette, "outside linked instance"),
        (absent, "requires authored vertex map and faces"),
    ] {
        let bytes = container(&blocks, &[0]);
        let error = pose::partition::evaluate(
            &bytes,
            "source",
            subset_request(&bytes, 0),
            Default::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}
#[test]
fn partition_subset_full_pose_subset_aggregate_and_source_bounds_have_exact_ceilings() {
    use pose::partition as subset;
    let bytes = container(&subset_fixture(), &[0]);
    let request = subset_request(&bytes, 1);
    let baseline = subset::evaluate(&bytes, "baseline", request, Default::default()).unwrap();
    let usage = baseline.usage;
    assert_eq!(
        usage.retained_bytes,
        usage.full_pose_retained_bytes + usage.subset_retained_bytes
    );
    assert_eq!(
        usage.work_units,
        usage.full_pose_work_units + usage.subset_work_units
    );
    let mut exact = subset::Limits {
        vertices: 2,
        draw_indices: 4,
        subset_array_bytes: usage.subset_retained_bytes,
        subset_work_units: usage.subset_work_units,
        array_bytes: usage.retained_bytes,
        work_units: usage.work_units,
        decoder_array_admission_bytes: usage.decoder_array_admission_bytes,
        decoder_check_admission_units: usage.decoder_check_admission_units,
        ..Default::default()
    };
    exact.skin.array_bytes = usage.full_pose_retained_bytes;
    exact.skin.work_units = usage.full_pose_work_units;
    exact.skin.source.partition.skin.scene.input_bytes = bytes.len();
    subset::evaluate(&bytes, "exact", request, exact).unwrap();
    for limits in [
        subset::Limits {
            vertices: 1,
            ..exact
        },
        subset::Limits {
            draw_indices: 3,
            ..exact
        },
        subset::Limits {
            subset_array_bytes: exact.subset_array_bytes - 1,
            ..exact
        },
        subset::Limits {
            subset_work_units: exact.subset_work_units - 1,
            ..exact
        },
        subset::Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        subset::Limits {
            work_units: exact.work_units - 1,
            ..exact
        },
        subset::Limits {
            decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
            ..exact
        },
        subset::Limits {
            decoder_check_admission_units: exact.decoder_check_admission_units - 1,
            ..exact
        },
    ] {
        assert!(subset::evaluate(&bytes, "under", request, limits).is_err());
    }
    for mode in 0..3 {
        let mut under = exact;
        match mode {
            0 => under.skin.array_bytes -= 1,
            1 => under.skin.work_units -= 1,
            _ => under.skin.source.partition.skin.scene.input_bytes -= 1,
        }
        assert!(subset::evaluate(&bytes, "phase-under", request, under).is_err());
    }
    let mut overflow = exact;
    overflow.skin.source.array_bytes = usize::MAX;
    assert!(
        subset::evaluate(&bytes, "overflow", request, overflow)
            .unwrap_err()
            .to_string()
            .contains("decoder array admission")
    );
}
#[test]
fn partition_subset_keeps_raw_nonunit_weights_normals_and_missing_normals_as_full_pose_observations()
 {
    let mut blocks = subset_fixture();
    blocks[5].1 = skin(
        &[vec![(0, 0.25), (0, 0.5), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        Some((R90, [-2., 3., 4.], 2.)),
    );
    let bytes = container(&blocks, &[0]);
    let mut request = subset_request(&bytes, 0);
    assert!(
        pose::partition::evaluate(&bytes, "unit", request, Default::default())
            .unwrap_err()
            .to_string()
            .contains("raw weight sum 1.5")
    );
    request.skin.weights = WeightPolicy::PreserveRawNonnegative;
    let result = pose::partition::evaluate(&bytes, "raw", request, Default::default()).unwrap();
    assert_eq!(
        (
            result.vertices[1].position,
            result.vertices[1].normal,
            result.vertices[1].weight_sum
        ),
        ([-12., 7.5, 15.], Some([-3., 0., 0.]), 1.5)
    );
    let mut no_normals = mesh();
    no_normals[47] = 0;
    no_normals.drain(48..84);
    blocks[6].1 = no_normals;
    let bytes = container(&blocks, &[0]);
    let request = pose::partition::Request {
        skin: Request {
            weights: WeightPolicy::PreserveRawNonnegative,
            ..request.skin
        },
        ..subset_request(&bytes, 0)
    };
    let result =
        pose::partition::evaluate(&bytes, "absent normals", request, Default::default()).unwrap();
    assert!(result.vertices.iter().all(|v| v.normal.is_none()));
}
#[test]
fn partition_subset_tiny_output_still_charges_large_full_deformation_and_source_index_map() {
    use pose::partition as subset;
    let count = 4096u16;
    let mut blocks = subset_fixture();
    let mut mesh = Vec::new();
    words(&mut mesh, &[0]);
    shorts(&mut mesh, &[count]);
    mesh.extend([0, 0, 1]);
    for _ in 0..count {
        floats(&mut mesh, &[1., 2., 3.]);
    }
    shorts(&mut mesh, &[0]);
    mesh.push(0);
    floats(&mut mesh, &[0., 0., 0., 10.]);
    mesh.push(0);
    shorts(&mut mesh, &[0]);
    words(&mut mesh, &[NULL]);
    shorts(&mut mesh, &[0]);
    words(&mut mesh, &[0]);
    mesh.push(0);
    shorts(&mut mesh, &[0]);
    blocks[6].1 = mesh;
    blocks[5].1 = skin(
        &[(0..count).map(|v| (v, 1.)).collect(), vec![]],
        Some((R90, [-2., 3., 4.], 2.)),
    );
    let mut part = Vec::new();
    words(&mut part, &[1]);
    shorts(&mut part, &[1, 0, 1, 0, 4, 0]);
    part.push(1);
    shorts(&mut part, &[count - 1]);
    part.push(1);
    floats(&mut part, &[1., 0., 0., 0.]);
    part.push(1);
    part.push(1);
    part.extend([0; 4]);
    blocks[7].1 = part;
    let bytes = container(&blocks, &[0]);
    let request = subset_request(&bytes, 0);
    let result = subset::evaluate(&bytes, "large", request, Default::default()).unwrap();
    assert_eq!(result.vertices.len(), 1);
    assert_eq!(result.vertices[0].source_vertex, count - 1);
    assert_eq!(result.vertices[0].position, [-6., 5., 10.]);
    assert!(result.usage.full_pose_retained_bytes >= usize::from(count) * 32);
    assert!(result.usage.subset_retained_bytes >= usize::from(count) * 16);
    let mut insufficient = subset::Limits {
        array_bytes: result.usage.subset_retained_bytes,
        ..Default::default()
    };
    assert!(subset::evaluate(&bytes, "full intermediate", request, insufficient).is_err());
    insufficient = subset::Limits::default();
    insufficient.skin.array_bytes = result.usage.full_pose_retained_bytes - 1;
    assert!(subset::evaluate(&bytes, "full cap", request, insufficient).is_err());
}

#[test]
fn shared_geometries_have_independent_literal_palettes_weights_frames_and_source_ids() {
    let bytes = container(&shared_fixture(), &[0]);
    let requests = shared_requests();
    let batch = pose::evaluate_many(
        &bytes,
        "shared",
        source_digest(&bytes),
        &requests,
        Default::default(),
    )
    .unwrap();
    assert_eq!(batch.geometries.len(), 2);
    for (value, request) in batch.geometries.iter().zip(requests) {
        let one = pose::evaluate(&bytes, "one", request, Default::default()).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::to_value(one).unwrap()
        );
        assert_eq!(value.skeleton_root, 0);
        assert_eq!(value.source_sha256, batch.source_sha256);
    }
    let first = &batch.geometries[0];
    assert_eq!(
        (
            first.geometry,
            first.geometry_data,
            first.instance,
            first.skin_data
        ),
        (3, 6, 4, 5)
    );
    assert_eq!(
        first.positions,
        [[-1.5, 1., 1.5], [0., -0.5, 1.], [-3.5, 0., 0.5]]
    );
    let second = &batch.geometries[1];
    assert_eq!(
        (
            second.geometry,
            second.geometry_data,
            second.instance,
            second.skin_data
        ),
        (7, 10, 8, 9)
    );
    assert_eq!(
        second
            .palette
            .iter()
            .map(|p| (p.ordinal, p.node))
            .collect::<Vec<_>>(),
        [(0, 2), (1, 1)]
    );
    assert_eq!(
        second.palette[0].matrix,
        [[4., 0., 0., 9.], [0., 4., 0., -4.], [0., 0., 4., 5.]]
    );
    assert_eq!(
        second.palette[1].matrix,
        [[0., 2., 0., 3.], [-2., 0., 0., -4.], [0., 0., 2., 5.]]
    );
    assert_eq!(
        second.positions,
        [[11.5, 1.5, 15.5], [19., -9., 12.], [-1.5, -2.5, 8.5]]
    );
    assert_eq!(second.normals, [[0.5, 3., 0.]; 3]);
    assert_eq!(second.weight_sums, [1., 1., 1.]);
    assert_eq!(
        second.skin_to_source_world,
        [
            [-1.5, 0., 0., 14.5],
            [0., -1.5, 0., 26.],
            [0., 0., 1.5, 22.5]
        ]
    );
    assert!(
        !batch.retail_behavior_verified
            && batch.geometries.iter().all(|p| !p.retail_behavior_verified)
    );
}

#[test]
fn shared_full_observations_permute_without_bone_or_budget_priority() {
    let bytes = container(&shared_fixture(), &[0]);
    let requests = shared_requests();
    let forward = pose::evaluate_many(
        &bytes,
        "order",
        source_digest(&bytes),
        &requests,
        Default::default(),
    )
    .unwrap();
    let reverse = pose::evaluate_many(
        &bytes,
        "order",
        source_digest(&bytes),
        &[requests[1], requests[0]],
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&forward.geometries).unwrap(),
        serde_json::to_value(reverse.geometries.iter().rev().collect::<Vec<_>>()).unwrap()
    );
    assert_eq!(
        (forward.retained_bytes, forward.work_units),
        (reverse.retained_bytes, reverse.work_units)
    );
    let mut changed = shared_fixture();
    changed[9].1[129..133].copy_from_slice(&0.5f32.to_le_bytes());
    let changed = container(&changed, &[0]);
    let value = pose::evaluate_many(
        &changed,
        "changed",
        source_digest(&changed),
        &requests,
        Default::default(),
    )
    .unwrap();
    let mut old_first = serde_json::to_value(&forward.geometries[0]).unwrap();
    let mut new_first = serde_json::to_value(&value.geometries[0]).unwrap();
    old_first.as_object_mut().unwrap().remove("source_sha256");
    new_first.as_object_mut().unwrap().remove("source_sha256");
    assert_eq!(old_first, new_first);
    assert_ne!(
        forward.geometries[1].positions,
        value.geometries[1].positions
    );
    assert_eq!(value.geometries[1].weight_sums, [0.75, 1., 1.]);
}

#[test]
fn shared_stale_identity_duplicate_empty_and_later_failure_never_return_a_batch() {
    let bytes = container(&shared_fixture(), &[0]);
    let digest = source_digest(&bytes);
    let requests = shared_requests();
    let mut wrong = digest;
    wrong[0] ^= 1;
    assert!(
        pose::evaluate_many(&bytes, "identity", wrong, &requests, Default::default())
            .unwrap_err()
            .to_string()
            .contains("SHA256 differs")
    );
    let mut changed = bytes.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert!(
        pose::evaluate_many(&changed, "stale", digest, &requests, Default::default())
            .unwrap_err()
            .to_string()
            .contains("SHA256 differs")
    );
    for (cases, expected) in [
        (vec![], "nonempty"),
        (vec![requests[0], requests[0]], "duplicate geometry"),
        (
            vec![
                requests[0],
                Request {
                    geometry: 999,
                    ..requests[1]
                },
            ],
            "selected geometry has no decoded skin owner",
        ),
        (
            vec![
                requests[0],
                Request {
                    weights: WeightPolicy::RequireUnitSum {
                        absolute_tolerance: f64::NAN,
                    },
                    ..requests[1]
                },
            ],
            "weight tolerance",
        ),
    ] {
        assert!(
            pose::evaluate_many(&bytes, "atomic", digest, &cases, Default::default())
                .unwrap_err()
                .to_string()
                .contains(expected)
        );
    }
    let mut blocks = shared_fixture();
    blocks[9].1[129..133].copy_from_slice(&(-0.5f32).to_le_bytes());
    let bad = container(&blocks, &[0]);
    assert!(
        pose::evaluate_many(
            &bad,
            "later",
            source_digest(&bad),
            &requests,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("negative")
    );
}

#[test]
fn shared_aggregate_per_geometry_source_and_decoder_bounds_have_exact_ceilings() {
    let bytes = container(&shared_fixture(), &[0]);
    let digest = source_digest(&bytes);
    let requests = shared_requests();
    let baseline =
        pose::evaluate_many(&bytes, "bounds", digest, &requests, Default::default()).unwrap();
    let mut per = Limits {
        array_bytes: baseline
            .geometries
            .iter()
            .map(|p| p.retained_bytes)
            .max()
            .unwrap(),
        work_units: baseline
            .geometries
            .iter()
            .map(|p| p.work_units)
            .max()
            .unwrap(),
        ancestry_depth: 2,
        ..Default::default()
    };
    per.source.partition.skin.scene.input_bytes = bytes.len();
    let exact = pose::BatchLimits {
        pose: per,
        geometries: 2,
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        preparation_array_bytes: baseline.preparation.retained_bytes,
        preparation_work_units: baseline.preparation.work_units,
    };
    pose::evaluate_many(&bytes, "exact", digest, &requests, exact).unwrap();
    for (limit, expected) in [
        (
            pose::BatchLimits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage budget",
        ),
        (
            pose::BatchLimits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            pose::BatchLimits {
                geometries: 1,
                ..exact
            },
            "bounded geometry",
        ),
        (
            pose::BatchLimits {
                decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
                ..exact
            },
            "decoder array admission",
        ),
        (
            pose::BatchLimits {
                decoder_check_admission_units: exact.decoder_check_admission_units - 1,
                ..exact
            },
            "decoder check admission",
        ),
        (
            pose::BatchLimits {
                pose: Limits {
                    array_bytes: per.array_bytes - 1,
                    ..per
                },
                ..exact
            },
            "array storage budget",
        ),
        (
            pose::BatchLimits {
                pose: Limits {
                    work_units: per.work_units - 1,
                    ..per
                },
                ..exact
            },
            "work budget",
        ),
        (
            pose::BatchLimits {
                pose: Limits {
                    ancestry_depth: 1,
                    ..per
                },
                ..exact
            },
            "ancestry depth budget",
        ),
    ] {
        let error = pose::evaluate_many(&bytes, "under", digest, &requests, limit).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut short = exact;
    short.pose.source.partition.skin.scene.input_bytes -= 1;
    assert!(
        pose::evaluate_many(&bytes, "input", digest, &requests, short)
            .unwrap_err()
            .to_string()
            .contains("input byte budget")
    );
    let mut overflow = exact;
    overflow.pose.source.array_bytes = usize::MAX;
    assert!(
        pose::evaluate_many(&bytes, "overflow", digest, &requests, overflow)
            .unwrap_err()
            .to_string()
            .contains("decoder array admission")
    );
}

#[test]
fn shared_root_and_distinct_owner_controllers_remain_independent_unapplied_observations() {
    let mut blocks = shared_fixture();
    blocks[0].1[8..12].copy_from_slice(&11u32.to_le_bytes());
    blocks[7].1[8..12].copy_from_slice(&12u32.to_le_bytes());
    for target in [0, 7] {
        let mut controller = Vec::new();
        words(&mut controller, &[NULL]);
        shorts(&mut controller, &[0]);
        floats(&mut controller, &[1., 0., 0., 1.]);
        words(&mut controller, &[target, NULL]);
        blocks.push(("NiTransformController", controller));
    }
    let bytes = container(&blocks, &[0]);
    let batch = pose::evaluate_many(
        &bytes,
        "controllers",
        source_digest(&bytes),
        &shared_requests(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        batch.geometries[0]
            .unapplied_controllers
            .iter()
            .map(|p| (p.object, p.controller))
            .collect::<Vec<_>>(),
        [(0, 11)]
    );
    assert_eq!(
        batch.geometries[1]
            .unapplied_controllers
            .iter()
            .map(|p| (p.object, p.controller))
            .collect::<Vec<_>>(),
        [(0, 11), (7, 12)]
    );
    assert_eq!(
        batch.geometries[1].positions,
        [[11.5, 1.5, 15.5], [19., -9., 12.], [-1.5, -2.5, 8.5]]
    );
}

#[test]
fn shared_preparation_owns_sources_after_input_mutation_drop_and_multiple_evaluations() {
    let mut bytes = container(&shared_fixture(), &[0]);
    let digest = source_digest(&bytes);
    let requests = shared_requests();
    let expected =
        pose::evaluate_many(&bytes, "expected", digest, &requests, Default::default()).unwrap();
    let prepared =
        pose::PreparedSkinSource::prepare(&bytes, "prepare", Default::default()).unwrap();
    let usage = prepared.usage();
    let source_sha = prepared.source_sha256().to_owned();
    assert_eq!((usage.binding_decodes, usage.scene_decodes), (1, 1));
    bytes.fill(0);
    drop(bytes);
    for _ in 0..3 {
        let batch = prepared
            .evaluate_many("reused", digest, &requests, Default::default())
            .unwrap();
        assert_eq!(
            serde_json::to_value(&batch).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let reversed = prepared
            .evaluate_many(
                "reverse",
                digest,
                &[requests[1], requests[0]],
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(reversed.geometries.iter().rev().collect::<Vec<_>>()).unwrap(),
            serde_json::to_value(&expected.geometries).unwrap()
        );
    }
    let mut stale = digest;
    stale[31] ^= 1;
    assert!(
        prepared
            .evaluate_many("stale", stale, &requests, Default::default())
            .unwrap_err()
            .to_string()
            .contains("SHA256 differs")
    );
    assert_eq!(prepared.source_sha256(), source_sha);
    assert_eq!(
        serde_json::to_value(prepared.usage()).unwrap(),
        serde_json::to_value(usage).unwrap()
    );
}

#[test]
fn shared_preparation_storage_hash_map_and_source_allowances_are_bounded_separately() {
    let bytes = container(&shared_fixture(), &[0]);
    let digest = source_digest(&bytes);
    let requests = shared_requests();
    let source = pose::PreparedSkinSource::prepare(&bytes, "prepare", Default::default()).unwrap();
    let usage = source.usage();
    let mut exact = pose::PreparationLimits {
        array_bytes: usage.retained_bytes,
        work_units: usage.work_units,
        decoder_array_admission_bytes: usage.decoder_array_admission_bytes,
        decoder_check_admission_units: usage.decoder_check_admission_units,
        ..Default::default()
    };
    exact.source.partition.skin.scene.input_bytes = bytes.len();
    pose::PreparedSkinSource::prepare(&bytes, "exact", exact).unwrap();
    for (limits, expected) in [
        (
            pose::PreparationLimits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage budget",
        ),
        (
            pose::PreparationLimits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            pose::PreparationLimits {
                decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
                ..exact
            },
            "decoder array admission",
        ),
        (
            pose::PreparationLimits {
                decoder_check_admission_units: exact.decoder_check_admission_units - 1,
                ..exact
            },
            "decoder check admission",
        ),
    ] {
        let error = pose::PreparedSkinSource::prepare(&bytes, "under", limits).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut short = exact;
    short.source.partition.skin.scene.input_bytes -= 1;
    assert!(
        pose::PreparedSkinSource::prepare(&bytes, "input", short)
            .unwrap_err()
            .to_string()
            .contains("input byte budget")
    );
    let first = source
        .evaluate_many("one", digest, &requests[..1], Default::default())
        .unwrap();
    let per = pose::GeometryLimits {
        array_bytes: first.geometries[0].retained_bytes,
        work_units: first.geometries[0].work_units,
        ancestry_depth: 2,
    };
    let one = pose::BatchEvaluationLimits {
        geometry: per,
        geometries: 1,
        array_bytes: first.retained_bytes,
        work_units: first.work_units,
    };
    source
        .evaluate_many("one exact", digest, &requests[..1], one)
        .unwrap();
    assert!(
        source
            .evaluate_many(
                "later bounded",
                digest,
                &requests,
                pose::BatchEvaluationLimits {
                    geometries: 2,
                    ..one
                }
            )
            .is_err()
    );
    source
        .evaluate_many("still complete", digest, &requests, Default::default())
        .unwrap();
    let mut blocks = shared_fixture();
    blocks[9].1[129..133].copy_from_slice(&0.5f32.to_le_bytes());
    let modified = container(&blocks, &[0]);
    let altered =
        pose::PreparedSkinSource::prepare(&modified, "nonunit", Default::default()).unwrap();
    let strict = [
        requests[0],
        Request {
            weights: WeightPolicy::RequireUnitSum {
                absolute_tolerance: 0.,
            },
            ..requests[1]
        },
    ];
    assert!(
        altered
            .evaluate_many(
                "later sum",
                source_digest(&modified),
                &strict,
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("sum")
    );
    altered
        .evaluate_many(
            "raw remains exact",
            source_digest(&modified),
            &requests,
            Default::default(),
        )
        .unwrap();
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

fn external_sample_keys(scales: [f32; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0, 2, 1]);
    floats(&mut out, &[0., -1., 2., 0., 1., 3., 6., 4.]);
    words(&mut out, &[2, 1]);
    floats(&mut out, &[0., scales[0], 1., scales[1]]);
    out
}
fn external_sample_clip_blocks(scales: [f32; 2]) -> Vec<(&'static str, Vec<u8>)> {
    let mut sequence = Vec::new();
    words(&mut sequence, &[NULL, 1, 7, 1, NULL]);
    sequence.push(25);
    words(&mut sequence, &[0, NULL, 1, NULL, NULL]);
    floats(&mut sequence, &[0.25]);
    words(&mut sequence, &[NULL, 3]);
    floats(&mut sequence, &[17., 100., 101.]);
    words(&mut sequence, &[NULL, NULL]);
    shorts(&mut sequence, &[2]);
    words(&mut sequence, &[NULL, NULL]);
    let mut interpolator = Vec::new();
    floats(
        &mut interpolator,
        &[100., 200., 300., 2., -3., 4., -5., 12.],
    );
    words(&mut interpolator, &[2]);
    vec![
        ("NiControllerSequence", sequence),
        ("NiTransformInterpolator", interpolator),
        ("NiTransformData", external_sample_keys(scales)),
    ]
}
fn external_sample_clip(scales: [f32; 2]) -> Vec<u8> {
    named_container(
        &external_sample_clip_blocks(scales),
        &[0],
        &[b"Rig-A\0", b"NiTransformController"],
    )
}
fn external_sample_rig_blocks() -> Vec<(&'static str, Vec<u8>)> {
    let old = external_rig_blocks();
    let mut blocks = vec![
        old[3].clone(),
        old[0].clone(),
        old[4].clone(),
        old[2].clone(),
        old[1].clone(),
    ];
    blocks[3].1 = node(R90, [0., 4., 0.], 2., &[0]);
    blocks[3].1[..4].copy_from_slice(&0u32.to_le_bytes());
    blocks[3].1[8..12].copy_from_slice(&5u32.to_le_bytes());
    blocks[4].1 = node(
        [[-1., 0., 0.], [0., -1., 0.], [0., 0., 1.]],
        [100., 200., 300.],
        5.,
        &[3, 2],
    );
    let mut controller = Vec::new();
    words(&mut controller, &[NULL]);
    shorts(&mut controller, &[0x004C]);
    floats(&mut controller, &[17., -9., 100., 101.]);
    words(&mut controller, &[3, 6]);
    let mut interpolator = Vec::new();
    floats(
        &mut interpolator,
        &[1000., 2000., 3000., 2., -3., 4., -5., 12.],
    );
    words(&mut interpolator, &[7]);
    blocks.extend([
        ("NiTransformController", controller),
        ("NiTransformInterpolator", interpolator),
        ("NiTransformData", external_sample_keys([88., 99.])),
    ]);
    blocks
}
fn external_sample_fixture() -> (
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    fallout_data::nif_skin::external::Request,
) {
    let (skin, _, mut mapping) = external_fixture();
    let rig = named_container(&external_sample_rig_blocks(), &[4], &[b"Rig-A\0", b"Twin"]);
    mapping.expected_rig_sha256 = source_digest(&rig);
    mapping.rig_root = 4;
    mapping.explicit_bone_mapping[0].rig_node = 3;
    mapping.explicit_bone_mapping[1].rig_node = 0;
    (skin, rig, external_sample_clip([1., 3.]), mapping)
}
fn external_sample_request(
    rig: &[u8],
    clip: &[u8],
    time: f64,
) -> fallout_data::nif_animation::clip::Request<'static> {
    fallout_data::nif_animation::clip::Request {
        expected_skeleton_sha256: source_digest(rig),
        expected_clip_sha256: source_digest(clip),
        object: 3,
        node_name_bytes: b"Rig-A\0",
        sequence: 0,
        controlled_ordinal: 0,
        source_time: time,
    }
}
fn evaluate_external_sample(
    skin: &[u8],
    rig: &[u8],
    clip: &[u8],
    mapping: &fallout_data::nif_skin::external::Request,
    time: f64,
    limits: fallout_data::nif_skin::external::sampled::Limits,
) -> fallout_data::Result<fallout_data::nif_skin::external::sampled::Evaluation> {
    fallout_data::nif_skin::external::sampled::evaluate(
        skin,
        rig,
        clip,
        "literal three-source palette",
        fallout_data::nif_skin::external::sampled::Request {
            mapping,
            clip: external_sample_request(rig, clip, time),
        },
        limits,
    )
}
#[test]
fn external_clip_sample_has_literal_noncommuting_higher_id_palette_vertices_and_world_frame() {
    let (skin, rig, clip, mapping) = external_sample_fixture();
    let result =
        evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, Default::default()).unwrap();
    assert_eq!(
        result.external.skin.palette[0].matrix,
        [[-1., 0., 0., 1.5], [0., -1., 0., -0.5], [0., 0., 2., 2.5]]
    );
    assert_eq!(
        result.external.skin.palette[1].matrix,
        [[1.5, 0., 0., -10.5], [0., 1.5, 0., -5.], [0., 0., 3., 4.5]]
    );
    assert_eq!(
        result.external.skin.positions,
        [[-6.625, -2.125, 12.25], [-2.5, 0.5, 6.5], [-15., -5., 7.5]]
    );
    assert_eq!(
        result.external.skin.normals,
        [[0., 0.875, 0.], [0., -1., 0.], [0., 1.5, 0.]]
    );
    assert_eq!(
        result.external.skin.skin_to_source_world,
        [[0., -6., 0., 10.], [6., 0., 0., 32.], [0., 0., 6., 30.]]
    );
    assert_eq!(
        result.external.mappings[1].rig_bone_to_root,
        [[6., 0., 0., 1.], [0., 6., 0., 8.], [0., 0., 6., 4.]]
    );
    assert_eq!(
        result.sample.source_world,
        [
            [0., 10., 0., 95.],
            [-10., 0., 0., 180.],
            [0., 0., 10., 310.]
        ]
    );
    assert_eq!(result.sample.object.block, 3);
    assert_eq!(result.external.rig_root.block, 4);
    assert_eq!(result.sample.node_name_bytes, b"Rig-A\0");
    assert_eq!(result.sample.unapplied_object_controller, Some(5));
    assert_eq!(
        result
            .external
            .mappings
            .iter()
            .map(|m| m.rig_node.block)
            .collect::<Vec<_>>(),
        [3, 0]
    );
    assert_eq!(
        (result.rig_scene_decodes, result.skin_scene_decodes),
        (1, 1)
    );
    assert!(result.external.rig_unapplied_controllers.is_empty());
    let prior = fallout_data::nif_animation::clip::evaluate(
        &rig,
        &clip,
        "literal three-source palette",
        external_sample_request(&rig, &clip, 0.5),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&result.sample).unwrap(),
        serde_json::to_vec(&prior).unwrap()
    );
    assert!(!result.retail_behavior_verified);
}
#[test]
fn external_clip_endpoints_signed_zero_zero_scale_and_reflection_preserve_forward_source_rules() {
    let (skin, rig, clip, mapping) = external_sample_fixture();
    for (time, positions) in [
        (
            0.,
            [
                [-3.5625, -2.3125, 5.375],
                [-1.5, -1., 2.5],
                [-7.75, -3.75, 3.],
            ],
        ),
        (
            -0.,
            [
                [-3.5625, -2.3125, 5.375],
                [-1.5, -1., 2.5],
                [-7.75, -3.75, 3.],
            ],
        ),
        (
            1.,
            [
                [-9.6875, -1.9375, 19.125],
                [-3.5, 2., 10.5],
                [-22.25, -6.25, 12.],
            ],
        ),
    ] {
        let result =
            evaluate_external_sample(&skin, &rig, &clip, &mapping, time, Default::default())
                .unwrap();
        assert_eq!(result.external.skin.positions, positions);
        assert_eq!(result.sample.requested_time_f64_bits, time.to_bits());
    }
    for (scales, positions, normals) in [
        ([-1., 1.], [[-2.5, -0.5, 2.5]; 3], [[0.; 3]; 3]),
        (
            [-3., -1.],
            [[1.625, 1.125, -7.25], [-2.5, -1.5, -1.5], [10., 4., -2.5]],
            [[0., -0.875, 0.], [0., 1., 0.], [0., -1.5, 0.]],
        ),
    ] {
        let clip = external_sample_clip(scales);
        let result =
            evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, Default::default())
                .unwrap();
        assert_eq!(result.external.skin.positions, positions);
        assert_eq!(result.external.skin.normals, normals);
        assert_eq!(
            result.external.skin.skin_to_source_world,
            [[0., -6., 0., 10.], [6., 0., 0., 32.], [0., 0., 6., 30.]]
        );
    }
}
#[test]
fn external_clip_three_identities_raw_names_packet_mapping_and_late_failures_are_atomic() {
    use fallout_data::nif_skin::external::sampled;
    let (skin, rig, clip, mut mapping) = external_sample_fixture();
    for time in [f64::NAN, -1., 2.] {
        assert!(
            evaluate_external_sample(&skin, &rig, &clip, &mapping, time, Default::default())
                .is_err()
        );
    }
    let request = external_sample_request(&rig, &clip, 0.5);
    mapping.expected_skin_sha256[0] ^= 1;
    assert!(
        evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, Default::default())
            .unwrap_err()
            .to_string()
            .contains("skin source SHA256 differs")
    );
    mapping.expected_skin_sha256[0] ^= 1;
    let mut wrong = request;
    wrong.expected_clip_sha256[0] ^= 1;
    assert!(
        sampled::evaluate(
            &skin,
            &rig,
            &clip,
            "wrong clip",
            sampled::Request {
                mapping: &mapping,
                clip: wrong
            },
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("clip source SHA256 differs")
    );
    wrong = request;
    wrong.expected_skeleton_sha256[0] ^= 1;
    assert!(
        sampled::evaluate(
            &skin,
            &rig,
            &clip,
            "different rig",
            sampled::Request {
                mapping: &mapping,
                clip: wrong
            },
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("identity differs")
    );
    mapping.expected_rig_sha256[0] ^= 1;
    wrong = request;
    wrong.expected_skeleton_sha256 = mapping.expected_rig_sha256;
    assert!(
        sampled::evaluate(
            &skin,
            &rig,
            &clip,
            "stale rig",
            sampled::Request {
                mapping: &mapping,
                clip: wrong
            },
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("rig source SHA256 differs")
    );
    mapping.expected_rig_sha256[0] ^= 1;
    wrong = request;
    wrong.node_name_bytes = b"Rig-A";
    assert!(
        sampled::evaluate(
            &skin,
            &rig,
            &clip,
            "raw name",
            sampled::Request {
                mapping: &mapping,
                clip: wrong
            },
            Default::default()
        )
        .is_err()
    );
    wrong = request;
    wrong.controlled_ordinal = 1;
    assert!(
        sampled::evaluate(
            &skin,
            &rig,
            &clip,
            "ordinal",
            sampled::Request {
                mapping: &mapping,
                clip: wrong
            },
            Default::default()
        )
        .is_err()
    );
    let mut rotated = external_sample_clip_blocks([1., 3.]);
    let mut keys = Vec::new();
    words(&mut keys, &[1, 1]);
    floats(&mut keys, &[0., 1., 0., 0., 0.]);
    keys.extend(&rotated[2].1[4..]);
    rotated[2].1 = keys;
    let clip_rot = named_container(&rotated, &[0], &[b"Rig-A\0", b"NiTransformController"]);
    assert!(
        evaluate_external_sample(&skin, &rig, &clip_rot, &mapping, 0.5, Default::default())
            .unwrap_err()
            .to_string()
            .contains("rotation key mapping is unapplied")
    );
    mapping.explicit_bone_mapping[1].bone_ordinal = 0;
    assert!(
        evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, Default::default()).is_err()
    );
    mapping.explicit_bone_mapping[1].bone_ordinal = 1;
    mapping.explicit_bone_mapping[1]
        .expected_skin_bone_name_bytes
        .push(0);
    assert!(
        evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, Default::default()).is_err()
    );
}
#[test]
fn external_clip_other_required_controller_ambiguous_name_and_unrelated_sample_refuse() {
    let (skin, rig, clip, mut mapping) = external_sample_fixture();
    let mut blocks = external_sample_rig_blocks();
    blocks[0].1[8..12].copy_from_slice(&5u32.to_le_bytes());
    let child_controlled = named_container(&blocks, &[4], &[b"Rig-A\0", b"Twin"]);
    mapping.expected_rig_sha256 = source_digest(&child_controlled);
    assert!(
        evaluate_external_sample(
            &skin,
            &child_controlled,
            &clip,
            &mapping,
            0.5,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("other required rig controller")
    );
    blocks = external_sample_rig_blocks();
    blocks[4].1[8..12].copy_from_slice(&5u32.to_le_bytes());
    let root_controlled = named_container(&blocks, &[4], &[b"Rig-A\0", b"Twin"]);
    mapping.expected_rig_sha256 = source_digest(&root_controlled);
    assert!(
        evaluate_external_sample(
            &skin,
            &root_controlled,
            &clip,
            &mapping,
            0.5,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("ancestor 4 controller")
    );
    blocks = external_sample_rig_blocks();
    blocks[2].1[..4].copy_from_slice(&0u32.to_le_bytes());
    let ambiguous = named_container(&blocks, &[4], &[b"Rig-A\0", b"Twin"]);
    mapping.expected_rig_sha256 = source_digest(&ambiguous);
    assert!(
        evaluate_external_sample(&skin, &ambiguous, &clip, &mapping, 0.5, Default::default())
            .unwrap_err()
            .to_string()
            .contains("not unique")
    );
    // Exact selected packet remains supported, but neither mapped bone uses it.
    blocks = external_sample_rig_blocks();
    blocks[3].1[8..12].copy_from_slice(&NULL.to_le_bytes());
    blocks[2].1[..4].copy_from_slice(&2u32.to_le_bytes());
    let outside = named_container(&blocks, &[4], &[b"Rig-A\0", b"Twin", b"Unrelated"]);
    mapping.expected_rig_sha256 = source_digest(&outside);
    let unrelated_clip = named_container(
        &external_sample_clip_blocks([1., 3.]),
        &[0],
        &[b"Unrelated", b"NiTransformController"],
    );
    let request = fallout_data::nif_animation::clip::Request {
        object: 2,
        node_name_bytes: b"Unrelated",
        ..external_sample_request(&outside, &unrelated_clip, 0.5)
    };
    let error = fallout_data::nif_skin::external::sampled::evaluate(
        &skin,
        &outside,
        &unrelated_clip,
        "outside exact map",
        fallout_data::nif_skin::external::sampled::Request {
            mapping: &mapping,
            clip: request,
        },
        Default::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("outside strict mapped"));
    // An unrelated stored controller never becomes a required channel.
    blocks = external_sample_rig_blocks();
    blocks[2].1[8..12].copy_from_slice(&5u32.to_le_bytes());
    let sibling = named_container(&blocks, &[4], &[b"Rig-A\0", b"Twin"]);
    mapping.expected_rig_sha256 = source_digest(&sibling);
    assert!(
        evaluate_external_sample(&skin, &sibling, &clip, &mapping, 0.5, Default::default()).is_ok()
    );
    assert_eq!(rig, external_sample_fixture().1);
}
#[test]
fn external_clip_aggregate_phase_source_admission_sampler_names_and_depth_caps_are_exact() {
    use fallout_data::nif_skin::external::sampled::Limits;
    let (skin, rig, clip, mapping) = external_sample_fixture();
    let baseline =
        evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, Default::default()).unwrap();
    let mut exact = Limits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        source_bytes: skin.len() + rig.len() + clip.len(),
        raw_name_bytes: b"Rig-A\0".len()
            + mapping
                .explicit_bone_mapping
                .iter()
                .map(|m| {
                    m.expected_skin_bone_name_bytes.len() + m.expected_rig_node_name_bytes.len()
                })
                .sum::<usize>(),
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    exact.external.array_bytes = baseline.external.retained_bytes;
    exact.external.work_units = baseline.external.work_units;
    exact.external.ancestry_depth = 2;
    exact.clip.pose.array_bytes = baseline.sample.retained_bytes;
    exact.clip.pose.work_units = baseline.sample.work_units;
    exact.clip.pose.ancestry_depth = 2;
    exact.clip.pose.sampling.validation_work = baseline.sample.sample_work.validation_units;
    exact.clip.pose.sampling.sampling_work = baseline.sample.sample_work.sampling_units;
    let admitted = evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, exact).unwrap();
    let mut under = exact;
    under.array_bytes -= 1;
    let mut cases = vec![under];
    under = exact;
    under.work_units -= 1;
    cases.push(under);
    under = exact;
    under.source_bytes -= 1;
    cases.push(under);
    under = exact;
    under.raw_name_bytes -= 1;
    cases.push(under);
    under = exact;
    under.external.array_bytes -= 1;
    cases.push(under);
    under = exact;
    under.external.work_units -= 1;
    cases.push(under);
    under = exact;
    under.external.ancestry_depth -= 1;
    cases.push(under);
    under = exact;
    under.clip.pose.array_bytes -= 1;
    cases.push(under);
    under = exact;
    under.clip.pose.work_units -= 1;
    cases.push(under);
    under = exact;
    under.clip.pose.ancestry_depth -= 1;
    cases.push(under);
    under = exact;
    under.clip.pose.sampling.validation_work -= 1;
    cases.push(under);
    under = exact;
    under.clip.pose.sampling.sampling_work -= 1;
    cases.push(under);
    under = exact;
    under.decoder_array_admission_bytes -= 1;
    cases.push(under);
    under = exact;
    under.decoder_check_admission_units = admitted.decoder_check_admission_units - 1;
    cases.push(under);
    under = exact;
    under.external.rig.input_bytes = rig.len() - 1;
    cases.push(under);
    under = exact;
    under.clip.pose.scene.input_bytes = rig.len() - 1;
    cases.push(under);
    under = exact;
    under.clip.pose.keys.animation.input_bytes = clip.len() - 1;
    cases.push(under);
    under = exact;
    under.external.skin.partition.skin.scene.input_bytes = skin.len() - 1;
    cases.push(under);
    under = exact;
    under.external.mapping_bones = 1;
    cases.push(under);
    under = exact;
    under.external.rig.blocks = 7;
    cases.push(under);
    for (ordinal, limits) in cases.into_iter().enumerate() {
        assert!(
            evaluate_external_sample(&skin, &rig, &clip, &mapping, 0.5, limits).is_err(),
            "case {ordinal}"
        );
    }
}
#[test]
fn external_clip_same_stored_local_preserves_old_palette_raw_weights_and_missing_normals() {
    let (skin_bytes, rig, _, mut mapping) = external_sample_fixture();
    let mut blocks = external_sample_clip_blocks([2., 2.]);
    let mut keys = Vec::new();
    words(&mut keys, &[0, 2, 1]);
    floats(&mut keys, &[0., 0., 4., 0., 1., 0., 4., 0.]);
    words(&mut keys, &[2, 1]);
    floats(&mut keys, &[0., 2., 1., 2.]);
    blocks[2].1 = keys;
    let clip = named_container(&blocks, &[0], &[b"Rig-A\0", b"NiTransformController"]);
    let sample =
        evaluate_external_sample(&skin_bytes, &rig, &clip, &mapping, 0.5, Default::default())
            .unwrap();
    let old = fallout_data::nif_skin::external::evaluate(
        &skin_bytes,
        &rig,
        "stored",
        &mapping,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&sample.external.skin.palette).unwrap(),
        serde_json::to_vec(&old.skin.palette).unwrap()
    );
    assert_eq!(sample.external.skin.positions, old.skin.positions);
    assert_eq!(sample.external.skin.normals, old.skin.normals);
    assert_eq!(
        sample.external.skin.skin_to_source_world,
        old.skin.skin_to_source_world
    );
    let mut skin_blocks = fixture();
    skin_blocks[1].1[..4].copy_from_slice(&0u32.to_le_bytes());
    skin_blocks[2].1[..4].copy_from_slice(&1u32.to_le_bytes());
    skin_blocks[5].1 = skin(
        &[vec![(0, 0.25), (0, 0.5), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        None,
    );
    let raw_skin = named_container(&skin_blocks, &[0], &[b"Skin-A\xff\0", b"Twin"]);
    mapping.expected_skin_sha256 = source_digest(&raw_skin);
    mapping.weights = WeightPolicy::PreserveRawNonnegative;
    let clip = external_sample_clip([1., 3.]);
    let result =
        evaluate_external_sample(&raw_skin, &rig, &clip, &mapping, 0.5, Default::default())
            .unwrap();
    assert_eq!(result.external.skin.positions[0], [-6.375, -3.375, 16.5]);
    assert_eq!(result.external.skin.normals[0], [0., 0.375, 0.]);
    assert_eq!(result.external.skin.weight_sums, [1.5, 1., 1.]);
    skin_blocks[6].1[47] = 0;
    skin_blocks[6].1.drain(48..84);
    let no_normals = named_container(&skin_blocks, &[0], &[b"Skin-A\xff\0", b"Twin"]);
    mapping.expected_skin_sha256 = source_digest(&no_normals);
    assert!(
        evaluate_external_sample(&no_normals, &rig, &clip, &mapping, 0.5, Default::default())
            .unwrap()
            .external
            .skin
            .normals
            .is_empty()
    );
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

#[test]
fn prepared_influence_bridge_owns_source_and_preserves_all_raw_entries_and_observations() {
    use fallout_data::nif_skin::influences;
    let mut bytes = container(&influence_fixture(), &[0]);
    let digest = source_digest(&bytes);
    let source = pose::PreparedSkinSource::prepare(&bytes, "sealed", Default::default()).unwrap();
    let table = influences::prepare(&bytes, "sealed", 3, Default::default()).unwrap();
    let raw = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let cold = table
        .evaluate(&bytes, "sealed", raw, Default::default())
        .unwrap();
    let preparation_before = serde_json::to_value(source.usage()).unwrap();
    let table_before = serde_json::to_value(&table).unwrap();
    bytes.fill(0xff);
    drop(bytes);
    let prepared = source
        .evaluate_table(digest, raw, &table, Default::default())
        .unwrap();
    assert_eq!(
        prepared.positions,
        [[-2.625, 2.5, 2.625], [0., -0.5, 1.], [-3.5, 0.75, 0.5]]
    );
    assert_eq!(
        prepared.normals,
        [[0., 0.875, 0.], [0., 0.5, 0.], [0., 0.5, 0.]]
    );
    assert_eq!(prepared.weight_sums, [1.75, 1., 1.]);
    assert_eq!(
        prepared
            .positions
            .iter()
            .chain(&prepared.normals)
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        cold.positions
            .iter()
            .chain(&cold.normals)
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
    assert_eq!(table.vertex_offsets(), [0, 7, 8, 10]);
    assert_eq!(table.entries()[2].weight_bits, 0x8000_0000);
    assert_eq!(table.entries()[5].weight_bits, 0);
    assert_eq!(table.entries()[6].bone_ordinal, 1);
    assert_eq!(table.entries()[6].source_weight_ordinal, 0);
    let mut cold_observations = serde_json::to_value(cold).unwrap();
    let mut reused_observations = serde_json::to_value(&prepared).unwrap();
    for field in ["retained_bytes", "work_units"] {
        cold_observations.as_object_mut().unwrap().remove(field);
        reused_observations.as_object_mut().unwrap().remove(field);
    }
    assert_eq!(cold_observations, reused_observations);
    for _ in 0..3 {
        let repeated = source
            .evaluate_table(digest, raw, &table, Default::default())
            .unwrap();
        assert_eq!(
            serde_json::to_value(repeated).unwrap(),
            serde_json::to_value(&prepared).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(source.usage()).unwrap(),
        preparation_before
    );
    assert_eq!(serde_json::to_value(&table).unwrap(), table_before);
    assert_eq!(source.usage().binding_decodes, 1);
    assert_eq!(source.usage().scene_decodes, 1);
    assert!(!prepared.retail_behavior_verified);
}

#[test]
fn prepared_influence_bridge_refuses_foreign_source_geometry_and_policy_without_mutation() {
    use fallout_data::nif_skin::influences;
    let blocks = influence_fixture();
    let bytes = container(&blocks, &[0]);
    let digest = source_digest(&bytes);
    let source = pose::PreparedSkinSource::prepare(&bytes, "sealed", Default::default()).unwrap();
    let table = influences::prepare(&bytes, "sealed", 3, Default::default()).unwrap();
    let raw = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let first = source
        .evaluate_table(digest, raw, &table, Default::default())
        .unwrap();
    let mut changed = blocks;
    changed[2].1 = node(R90, [0., 7., 0.], 2., &[]);
    let foreign = influences::prepare(
        &container(&changed, &[0]),
        "same block IDs",
        3,
        Default::default(),
    )
    .unwrap();
    assert_eq!(foreign.geometry(), table.geometry());
    assert_eq!(foreign.instance(), table.instance());
    assert!(
        source
            .evaluate_table(digest, raw, &foreign, Default::default())
            .unwrap_err()
            .to_string()
            .contains("table source SHA256 differs")
    );
    let mut altered_digest = digest;
    altered_digest[31] ^= 1;
    assert!(
        source
            .evaluate_table(altered_digest, raw, &table, Default::default())
            .unwrap_err()
            .to_string()
            .contains("prepared table source SHA256 differs")
    );
    assert!(
        source
            .evaluate_table(
                digest,
                Request { geometry: 2, ..raw },
                &table,
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("table geometry differs")
    );
    assert!(
        source
            .evaluate_table(digest, request(), &table, Default::default())
            .unwrap_err()
            .to_string()
            .contains("raw weight sum 1.75")
    );
    for absolute_tolerance in [-1., 1.01, f64::INFINITY, f64::NAN] {
        assert!(
            source
                .evaluate_table(
                    digest,
                    Request {
                        weights: WeightPolicy::RequireUnitSum { absolute_tolerance },
                        ..raw
                    },
                    &table,
                    Default::default()
                )
                .unwrap_err()
                .to_string()
                .contains("tolerance must be finite")
        );
    }
    let valid = source
        .evaluate_table(
            digest,
            Request {
                weights: WeightPolicy::RequireUnitSum {
                    absolute_tolerance: 0.75,
                },
                ..raw
            },
            &table,
            Default::default(),
        )
        .unwrap();
    assert_eq!(valid.positions, first.positions);
    assert_eq!(valid.normals, first.normals);
    assert_eq!(valid.weight_sums, first.weight_sums);
    assert_eq!(
        serde_json::to_value(
            source
                .evaluate_table(digest, raw, &table, Default::default())
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(first).unwrap()
    );
}

#[test]
fn prepared_influence_bridge_admits_concurrent_live_source_table_output_and_exact_work_depth() {
    use fallout_data::nif_skin::influences;
    let bytes = container(&influence_fixture(), &[0]);
    let digest = source_digest(&bytes);
    let source = pose::PreparedSkinSource::prepare(&bytes, "bounded", Default::default()).unwrap();
    let table = influences::prepare(&bytes, "bounded", 3, Default::default()).unwrap();
    let raw = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let evaluated = source
        .evaluate_table(digest, raw, &table, Default::default())
        .unwrap();
    let cold = table
        .evaluate(&bytes, "bounded", raw, Default::default())
        .unwrap();
    assert!(
        evaluated.retained_bytes
            > source.usage().retained_bytes
                + source.usage().source_binding_retained_bytes
                + table.usage().output_bytes
                + cold.retained_bytes
    );
    let exact = pose::GeometryLimits {
        array_bytes: evaluated.retained_bytes,
        work_units: evaluated.work_units,
        ancestry_depth: 2,
    };
    source.evaluate_table(digest, raw, &table, exact).unwrap();
    for (short, reason) in [
        (
            pose::GeometryLimits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage budget",
        ),
        (
            pose::GeometryLimits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            pose::GeometryLimits {
                ancestry_depth: 1,
                ..exact
            },
            "ancestry depth budget",
        ),
        (
            pose::GeometryLimits {
                array_bytes: table.usage().output_bytes + cold.retained_bytes,
                ..exact
            },
            "array storage budget",
        ),
    ] {
        assert!(
            source
                .evaluate_table(digest, raw, &table, short)
                .unwrap_err()
                .to_string()
                .contains(reason),
            "{reason}"
        );
    }
    assert_eq!(
        serde_json::to_value(source.evaluate_table(digest, raw, &table, exact).unwrap()).unwrap(),
        serde_json::to_value(evaluated).unwrap()
    );
}

#[test]
fn prepared_influence_bridge_counts_unused_index_strings_and_material_texture_payloads() {
    use fallout_data::nif_skin::influences;
    let blocks = influence_fixture();
    let raw = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let evaluate = |bytes: &[u8], limits| {
        let source =
            pose::PreparedSkinSource::prepare(bytes, "large retained source", Default::default())
                .unwrap();
        let table =
            influences::prepare(bytes, "large retained source", 3, Default::default()).unwrap();
        source.evaluate_table(source_digest(bytes), raw, &table, limits)
    };
    let first = evaluate(&container(&blocks, &[0]), Default::default()).unwrap();
    let unused_name = vec![b'x'; 65_536];
    let named_bytes = named_container(&blocks, &[0], &[&unused_name]);
    let named = evaluate(&named_bytes, Default::default()).unwrap();
    assert_eq!(named.positions, first.positions);
    assert_eq!(named.normals, first.normals);
    assert_eq!(
        named.retained_bytes - first.retained_bytes,
        65_536 + std::mem::size_of::<Vec<u8>>()
    );
    assert_eq!(named.work_units - first.work_units, 1);
    assert!(
        evaluate(
            &named_bytes,
            pose::GeometryLimits {
                array_bytes: named.retained_bytes - 1,
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("array storage budget")
    );
    let mut with_material = blocks;
    let mut texture_set = Vec::new();
    words(&mut texture_set, &[32]);
    for _ in 0..32 {
        words(&mut texture_set, &[4096]);
        texture_set.extend(std::iter::repeat_n(b'x', 4096));
    }
    with_material.push(("BSShaderTextureSet", texture_set));
    let textured_bytes = container(&with_material, &[0]);
    let textured = evaluate(&textured_bytes, Default::default()).unwrap();
    assert_eq!(textured.positions, first.positions);
    assert_eq!(textured.normals, first.normals);
    // Decoded texture-set bytes and resolved raw/asset paths coexist in Scene.
    assert!(textured.retained_bytes - first.retained_bytes >= 3 * 131_072);
    assert!(
        evaluate(
            &textured_bytes,
            pose::GeometryLimits {
                array_bytes: first.retained_bytes + 131_072,
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("array storage budget")
    );
    assert!(textured.work_units - first.work_units < 300);
}

#[test]
fn prepared_influence_bridge_preserves_absent_normals_and_selected_controller_observations() {
    use fallout_data::nif_skin::influences;
    let mut blocks = influence_fixture();
    blocks[6].1[47] = 0;
    blocks[6].1.drain(48..84);
    blocks[1].1[8..12].copy_from_slice(&7u32.to_le_bytes());
    blocks.push(("UnimplementedController", vec![]));
    let bytes = container(&blocks, &[0]);
    let source =
        pose::PreparedSkinSource::prepare(&bytes, "stored source", Default::default()).unwrap();
    let table = influences::prepare(&bytes, "stored source", 3, Default::default()).unwrap();
    let raw = Request {
        geometry: 3,
        weights: WeightPolicy::PreserveRawNonnegative,
    };
    let first = source
        .evaluate_table(source_digest(&bytes), raw, &table, Default::default())
        .unwrap();
    let cold = table
        .evaluate(&bytes, "stored source", raw, Default::default())
        .unwrap();
    assert!(first.normals.is_empty());
    assert_eq!(first.positions, cold.positions);
    assert_eq!(
        serde_json::to_value(first.unapplied_controllers).unwrap(),
        serde_json::to_value(cold.unapplied_controllers).unwrap()
    );
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
fn external_exact_singular_large_integer_mapping_refuses_and_nearby_invertible_mappings_pass() {
    use fallout_data::nif_skin::external;
    let (skin, rig, mut request) = external_fixture();
    let mapping = [
        [478384076., 548195520., 675781114., 0.],
        [95565559., 470460823., 587107439., 0.],
        [573949635., 1018656343., 1262888553., 0.],
    ];
    let sum: [f64; 4] = std::array::from_fn(|c| mapping[0][c] + mapping[1][c]);
    assert_eq!(mapping[2], sum);
    request.explicit_root_space_mapping = mapping;
    let error = external::evaluate(&skin, &rig, "exact singular", &request, Default::default())
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("determinant cannot be certified nonzero"),
        "{error}"
    );
    let minor = 478384076i128 * 470460823 - 548195520i128 * 95565559;
    assert_ne!(minor, 0);
    for delta in [-1., 1.] {
        request.explicit_root_space_mapping = mapping;
        request.explicit_root_space_mapping[2][2] += delta;
        let value = external::evaluate(
            &skin,
            &rig,
            "nearby invertible",
            &request,
            Default::default(),
        )
        .unwrap();
        assert_eq!(
            value.explicit_root_space_mapping,
            request.explicit_root_space_mapping
        );
        assert!(
            value
                .skin
                .palette
                .iter()
                .flat_map(|p| p.matrix.iter().flatten())
                .all(|v| v.is_finite())
        );
    }
}

#[test]
fn external_mapping_certification_bounds_extreme_scales_and_uncertain_cancellation() {
    use fallout_data::nif_skin::external;
    let (skin, rig, mut request) = external_fixture();
    for scale in [1e-100, 1e100] {
        for sign in [-1., 1.] {
            request.explicit_root_space_mapping = [
                [sign * scale, 0., 0., 0.],
                [0., scale, 0., 0.],
                [0., 0., scale, 0.],
            ];
            external::evaluate(
                &skin,
                &rig,
                "certified extreme",
                &request,
                Default::default(),
            )
            .unwrap();
        }
    }
    for scale in [1e-200, 1e200, f64::from_bits(1), f64::MAX] {
        request.explicit_root_space_mapping = [
            [scale, 0., 0., 0.],
            [0., scale, 0., 0.],
            [0., 0., scale, 0.],
        ];
        let error = external::evaluate(
            &skin,
            &rig,
            "uncertain extreme",
            &request,
            Default::default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("determinant cannot be certified nonzero"),
            "{error}"
        );
    }
    let ulp = 2f64.powi(-52);
    request.explicit_root_space_mapping = [
        [1., 1., 1., 0.],
        [1., 1. + ulp, 1., 0.],
        [1., 1., 1. + ulp, 0.],
    ];
    let error = external::evaluate(
        &skin,
        &rig,
        "uncertain nonzero",
        &request,
        Default::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("determinant cannot be certified nonzero"),
        "{error}"
    );
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

fn set_keys(a: [f32; 3], b: [f32; 3], scales: [f32; 2]) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0, 2, 1]);
    floats(&mut out, &[0.]);
    floats(&mut out, &a);
    floats(&mut out, &[1.]);
    floats(&mut out, &b);
    words(&mut out, &[2, 1]);
    floats(&mut out, &[0., scales[0], 1., scales[1]]);
    out
}
fn set_controller(target: u32, interpolator: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL]);
    shorts(&mut out, &[0x004C]);
    floats(&mut out, &[17., -9., 100., 101.]);
    words(&mut out, &[target, interpolator]);
    out
}
fn set_interpolator(data: u32) -> Vec<u8> {
    let mut out = Vec::new();
    floats(&mut out, &[1000., 2000., 3000., 2., -3., 4., -5., 12.]);
    words(&mut out, &[data]);
    out
}
fn controlled_node(
    r: [[f32; 3]; 3],
    t: [f32; 3],
    scale: f32,
    children: &[u32],
    controller: u32,
) -> Vec<u8> {
    let mut out = node(r, t, scale, children);
    out[8..12].copy_from_slice(&controller.to_le_bytes());
    out
}
fn set_skin_fixture(root_scale: f32) -> Vec<(&'static str, Vec<u8>)> {
    let mut shape = av(ID, [999., 999., 999.], 7.);
    words(&mut shape, &[1, 4, 0, NULL]);
    shape.push(0);
    let mut instance = Vec::new();
    words(&mut instance, &[2, NULL, 11, 2, 7, 0]);
    vec![
        ("NiNode", controlled_node(R90, [0., 3., 0.], 2., &[], 8)),
        ("NiTriShapeData", mesh()),
        (
            "NiSkinData",
            skin(&[vec![(0, 0.25), (1, 1.)], vec![(0, 0.75), (2, 1.)]], None),
        ),
        ("NiTriShape", shape),
        ("NiSkinInstance", instance),
        (
            "NiTransformData",
            set_keys([-3., 2., 0.], [1., 6., 4.], [2., 4.]),
        ),
        ("NiTransformInterpolator", set_interpolator(5)),
        ("NiNode", controlled_node(RM90, [4., 0., 0.], 1., &[0], 9)),
        ("NiTransformController", set_controller(0, 6)),
        ("NiTransformController", set_controller(7, 10)),
        ("NiTransformInterpolator", set_interpolator(12)),
        (
            "NiNode",
            controlled_node(R90, [10., 20., 30.], 3., &[7, 3], 13),
        ),
        (
            "NiTransformData",
            set_keys([1., -4., 0.], [5., 0., 2.], [1., 3.]),
        ),
        ("NiTransformController", set_controller(11, 15)),
        ("NiNode", node(RM90, [-8., 9., 10.], 0.5, &[11])),
        ("NiTransformInterpolator", set_interpolator(16)),
        (
            "NiTransformData",
            set_keys([5., -7., 3.], [5., -7., 3.], [root_scale, root_scale]),
        ),
    ]
}
fn set_channels() -> [fallout_data::nif_animation::pose::Request; 3] {
    [
        fallout_data::nif_animation::pose::Request {
            object: 0,
            controller: 8,
            source_time: 0.5,
        },
        fallout_data::nif_animation::pose::Request {
            object: 11,
            controller: 13,
            source_time: 0.5,
        },
        fallout_data::nif_animation::pose::Request {
            object: 7,
            controller: 9,
            source_time: 0.5,
        },
    ]
}
fn set_skin(
    bytes: &[u8],
    channels: &[fallout_data::nif_animation::pose::Request],
    limits: pose::SetCombinedLimits,
) -> fallout_data::Result<pose::EvaluationWithSet> {
    pose::evaluate_set_sampled(
        bytes,
        "literal complete skin set",
        pose::SetRequest {
            expected_source_sha256: source_digest(bytes),
            skin: request(),
        },
        channels,
        limits,
    )
}
#[test]
fn complete_pose_set_skin_has_literal_noncommuting_parent_child_palette_and_vertices() {
    let blocks = set_skin_fixture(2.);
    let bytes = container(&blocks, &[14]);
    let result = set_skin(&bytes, &set_channels(), Default::default()).unwrap();
    assert_eq!(
        result.skin.palette[0].matrix,
        [[0., 1., 0., -0.5], [-1., 0., 0., 3.], [0., 0., 1., 0.5]]
    );
    assert_eq!(
        result.skin.palette[1].matrix,
        [[0., 1.5, 0., -1.], [-1.5, 0., 0., 6.], [0., 0., 1.5, 2.5]]
    );
    assert_eq!(
        result.skin.positions,
        [[1.875, 3.875, 6.125], [-1.5, -1., 2.5], [-1., 10.5, 4.]]
    );
    assert_eq!(
        result.skin.normals,
        [[1.375, 0., 0.], [1., 0., 0.], [1.5, 0., 0.]]
    );
    assert_eq!(result.skin.weight_sums, [1., 1., 1.]);
    assert_eq!(
        result.skin.skin_to_source_world,
        [[2., 0., 0., -7.5], [0., 2., 0., 6.5], [0., 0., 2., 11.5]]
    );
    assert_eq!(
        (
            result.skin.geometry,
            result.skin.geometry_data,
            result.skin.instance,
            result.skin.skin_data,
            result.skin.skeleton_root
        ),
        (3, 1, 4, 2, 11)
    );
    assert_eq!(
        result
            .skin
            .palette
            .iter()
            .map(|p| p.node)
            .collect::<Vec<_>>(),
        [7, 0]
    );
    assert_eq!(
        result.pose_set.objects[0].source_world,
        [[6., 0., 0., -0.5], [0., 6., 0., 6.5], [0., 0., 6., 16.5]]
    );
    assert_eq!(
        result.pose_set.objects[1].source_world,
        [[1., 0., 0., -11.5], [0., 1., 0., 6.5], [0., 0., 1., 11.5]]
    );
    assert_eq!(
        result.pose_set.objects[2].source_world,
        [[0., 2., 0., -8.5], [-2., 0., 0., 4.5], [0., 0., 2., 12.5]]
    );
    assert_eq!(result.pose_set.propagated_objects, 5);
    assert_eq!(result.skin_scene_decodes, 1);
    assert_eq!(result.pose_set.preparation.scene_decodes, 0);
    assert_eq!(result.pose_set.preparation.animation_key_decodes, 1);
    assert!(result.skin.unapplied_controllers.is_empty());
    for object in &result.pose_set.objects {
        for span in [
            &object.channel.object,
            &object.channel.controller,
            &object.channel.interpolator,
            &object.channel.data,
        ] {
            assert_eq!(
                &bytes[span.offset..span.offset + span.bytes],
                blocks[span.block as usize].1
            );
        }
    }
    assert!(!result.retail_behavior_verified);
}
#[test]
fn complete_skin_set_permutation_preserves_skin_and_existing_channel_observations() {
    let bytes = container(&set_skin_fixture(2.), &[14]);
    let channels = set_channels();
    let result = set_skin(&bytes, &channels, Default::default()).unwrap();
    let reverse = [channels[2], channels[1], channels[0]];
    let reversed = set_skin(&bytes, &reverse, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_vec(&result.skin).unwrap(),
        serde_json::to_vec(&reversed.skin).unwrap()
    );
    assert_eq!(result.retained_bytes, reversed.retained_bytes);
    assert_eq!(result.work_units, reversed.work_units);
    let old = fallout_data::nif_animation::pose::evaluate_set(
        &bytes,
        "literal",
        &channels,
        Default::default(),
    )
    .unwrap();
    for selected in &result.pose_set.objects {
        let matching = old
            .objects
            .iter()
            .find(|o| o.channel.object.block == selected.channel.object.block)
            .unwrap();
        assert_eq!(
            serde_json::to_vec(selected).unwrap(),
            serde_json::to_vec(matching).unwrap()
        );
    }
    assert_eq!(old.propagated_objects, 4);
    let mut static_blocks = set_skin_fixture(2.);
    for id in [0, 7, 11] {
        static_blocks[id].1[8..12].copy_from_slice(&NULL.to_le_bytes());
    }
    let static_bytes = container(&static_blocks, &[14]);
    let static_set = set_skin(&static_bytes, &[], Default::default()).unwrap();
    let stored = pose::evaluate(&static_bytes, "static", request(), Default::default()).unwrap();
    assert!(static_set.pose_set.objects.is_empty());
    assert_eq!(static_set.pose_set.propagated_objects, 5);
    assert_eq!(
        serde_json::to_vec(&static_set.skin.palette).unwrap(),
        serde_json::to_vec(&stored.palette).unwrap()
    );
    assert_eq!(static_set.skin.positions, stored.positions);
    assert_eq!(static_set.skin.normals, stored.normals);
    assert_eq!(
        static_set.skin.skin_to_source_world,
        stored.skin_to_source_world
    );
}
#[test]
fn complete_skin_set_coverage_wrong_links_and_late_failures_never_return_a_deformation() {
    let blocks = set_skin_fixture(2.);
    let bytes = container(&blocks, &[14]);
    let channels = set_channels();
    for selected in [
        vec![channels[0], channels[1]],
        vec![channels[0], channels[2]],
        vec![channels[1], channels[2]],
        vec![channels[0], channels[0]],
    ] {
        assert!(set_skin(&bytes, &selected, Default::default()).is_err());
    }
    for time in [f64::NAN, 2.] {
        let mut late = channels;
        late[0].source_time = time;
        let ordered = [late[1], late[2], late[0]];
        assert!(set_skin(&bytes, &ordered, Default::default()).is_err());
    }
    let mut wrong = channels;
    wrong[0].controller = 9;
    assert!(
        set_skin(&bytes, &wrong, Default::default())
            .unwrap_err()
            .to_string()
            .contains("object.controller differs")
    );
    let mut chained = blocks.clone();
    chained[8].1[..4].copy_from_slice(&9u32.to_le_bytes());
    assert!(
        set_skin(&container(&chained, &[14]), &channels, Default::default())
            .unwrap_err()
            .to_string()
            .contains("controller chain")
    );
    let mut outside = blocks;
    outside.extend([
        ("NiNode", controlled_node(ID, [0.; 3], 1., &[], 18)),
        ("NiTransformController", set_controller(17, 6)),
    ]);
    let bytes = container(&outside, &[14, 17]);
    let mut more = channels.to_vec();
    more.push(fallout_data::nif_animation::pose::Request {
        object: 17,
        controller: 18,
        source_time: 0.5,
    });
    assert!(
        set_skin(&bytes, &more, Default::default())
            .unwrap_err()
            .to_string()
            .contains("outside required skin forest")
    );
    assert!(set_skin(&bytes, &channels, Default::default()).is_ok());
}
#[test]
fn complete_skin_set_zero_and_reflected_root_preserve_relative_palette_without_root_inverse() {
    for (scale, frame) in [
        (
            0.,
            [[0., 0., 0., -11.5], [0., 0., 0., 6.5], [0., 0., 0., 11.5]],
        ),
        (
            -2.,
            [
                [-2., 0., 0., -15.5],
                [0., -2., 0., 6.5],
                [0., 0., -2., 11.5],
            ],
        ),
    ] {
        let bytes = container(&set_skin_fixture(scale), &[14]);
        let result = set_skin(&bytes, &set_channels(), Default::default()).unwrap();
        assert_eq!(result.skin.skin_to_source_world, frame);
        assert_eq!(
            result.skin.palette[0].matrix,
            [[0., 1., 0., -0.5], [-1., 0., 0., 3.], [0., 0., 1., 0.5]]
        );
        assert_eq!(
            result.skin.palette[1].matrix,
            [[0., 1.5, 0., -1.], [-1.5, 0., 0., 6.], [0., 0., 1.5, 2.5]]
        );
        assert_eq!(
            result.skin.positions,
            [[1.875, 3.875, 6.125], [-1.5, -1., 2.5], [-1., 10.5, 4.]]
        );
    }
}
#[test]
fn complete_skin_set_phase_aggregate_source_sampler_and_depth_caps_are_exact() {
    let bytes = container(&set_skin_fixture(2.), &[14]);
    let channels = set_channels();
    let baseline = set_skin(&bytes, &channels, Default::default()).unwrap();
    let mut exact = pose::SetCombinedLimits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        decoder_array_admission_bytes: baseline.decoder_array_admission_bytes,
        decoder_check_admission_units: baseline.decoder_check_admission_units,
        ..Default::default()
    };
    exact.skin.array_bytes = baseline.skin.retained_bytes;
    exact.skin.work_units = baseline.skin.work_units;
    exact.animation.array_bytes = baseline.pose_set.retained_bytes;
    exact.animation.work_units = baseline.pose_set.work_units;
    exact.animation.requests = 3;
    exact.animation.ancestry_depth = 4;
    exact.skin.ancestry_depth = 3;
    exact.animation.sampling.validation_work = baseline.pose_set.sample_work.validation_units;
    exact.animation.sampling.sampling_work = baseline.pose_set.sample_work.sampling_units;
    let admitted = set_skin(&bytes, &channels, exact).unwrap();
    let mut under = exact;
    under.array_bytes -= 1;
    let mut cases = vec![under];
    under = exact;
    under.work_units -= 1;
    cases.push(under);
    under = exact;
    under.skin.array_bytes -= 1;
    cases.push(under);
    under = exact;
    under.skin.work_units -= 1;
    cases.push(under);
    under = exact;
    under.animation.array_bytes -= 1;
    cases.push(under);
    under = exact;
    under.animation.work_units -= 1;
    cases.push(under);
    under = exact;
    under.animation.requests -= 1;
    cases.push(under);
    under = exact;
    under.animation.ancestry_depth -= 1;
    cases.push(under);
    under = exact;
    under.skin.ancestry_depth -= 1;
    cases.push(under);
    under = exact;
    under.decoder_array_admission_bytes -= 1;
    cases.push(under);
    under = exact;
    under.decoder_check_admission_units = admitted.decoder_check_admission_units - 1;
    cases.push(under);
    under = exact;
    under.animation.sampling.validation_work -= 1;
    cases.push(under);
    under = exact;
    under.animation.sampling.sampling_work -= 1;
    cases.push(under);
    under = exact;
    under.animation.source.array_bytes = baseline.pose_set.preparation.extra_retained_bytes - 1;
    cases.push(under);
    under = exact;
    under.animation.source.work_units = baseline.pose_set.preparation.work_units - 1;
    cases.push(under);
    under = exact;
    under.skin.source.partition.skin.scene.input_bytes = bytes.len() - 1;
    cases.push(under);
    under = exact;
    under.animation.source.scene.input_bytes = bytes.len() - 1;
    cases.push(under);
    for limits in cases {
        assert!(set_skin(&bytes, &channels, limits).is_err());
    }
    let wrong = pose::SetRequest {
        expected_source_sha256: [0; 32],
        skin: request(),
    };
    assert!(
        pose::evaluate_set_sampled(
            &bytes,
            "stale",
            wrong,
            &channels,
            pose::SetCombinedLimits {
                array_bytes: 0,
                ..Default::default()
            }
        )
        .unwrap_err()
        .to_string()
        .contains("source SHA256 differs")
    );
}
#[test]
fn complete_skin_set_preserves_nonunit_raw_weights_and_absent_normals() {
    let mut blocks = set_skin_fixture(2.);
    blocks[2].1 = skin(
        &[vec![(0, 0.25), (0, 0.5), (1, 1.)], vec![(0, 0.75), (2, 1.)]],
        None,
    );
    let bytes = container(&blocks, &[14]);
    let request = pose::SetRequest {
        expected_source_sha256: source_digest(&bytes),
        skin: Request {
            weights: WeightPolicy::PreserveRawNonnegative,
            ..request()
        },
    };
    let result = pose::evaluate_set_sampled(
        &bytes,
        "raw set",
        request,
        &set_channels(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.skin.positions[0], [2.625, 4.875, 7.875]);
    assert_eq!(result.skin.normals[0], [1.875, 0., 0.]);
    assert_eq!(result.skin.weight_sums, [1.5, 1., 1.]);
    assert!(
        set_skin(&bytes, &set_channels(), Default::default())
            .unwrap_err()
            .to_string()
            .contains("raw weight sum 1.5")
    );
    blocks = set_skin_fixture(2.);
    blocks[1].1[47] = 0;
    blocks[1].1.drain(48..84);
    let result = set_skin(
        &container(&blocks, &[14]),
        &set_channels(),
        Default::default(),
    )
    .unwrap();
    assert!(result.skin.normals.is_empty());
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
