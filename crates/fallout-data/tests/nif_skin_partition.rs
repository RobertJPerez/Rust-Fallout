//! Original authored partition packets; no retail bytes or regenerated faces.
use fallout_data::nif_skin::{
    self,
    partition::{self, Dependency, Limits},
};

const NULL: u32 = u32::MAX;
const STREAMS: [u32; 12] = [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34];

fn shorts(out: &mut Vec<u8>, words: &[u16]) {
    for word in words {
        out.extend(word.to_le_bytes());
    }
}
fn words(out: &mut Vec<u8>, words: &[u32]) {
    for word in words {
        out.extend(word.to_le_bytes());
    }
}
fn set_short(out: &mut [u8], offset: usize, word: u16) {
    out[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, stream]);
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
            &[types.iter().position(|v| v == name).unwrap() as u16],
        );
    }
    for (_, data) in blocks {
        words(&mut out, &[data.len() as u32]);
    }
    words(&mut out, &[0, 0, 0]);
    for (_, data) in blocks {
        out.extend(data);
    }
    words(&mut out, &[0]);
    out
}
fn av(stream: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL, 0, NULL]);
    if stream <= 26 {
        shorts(&mut out, &[14]);
    } else {
        words(&mut out, &[14]);
    }
    for value in [0f32, 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.] {
        words(&mut out, &[value.to_bits()]);
    }
    words(&mut out, &[0, NULL]);
    out
}
fn shape(stream: u32, data: u32, skin: u32) -> Vec<u8> {
    let mut out = av(stream);
    words(&mut out, &[data, skin, 0, NULL]);
    out.push(0);
    out
}
fn geometry(vertices: u16) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0]);
    shorts(&mut out, &[vertices]);
    out.extend([0, 0, 0]);
    shorts(&mut out, &[0]);
    out.push(0);
    words(&mut out, &[0, 0, 0, 1f32.to_bits()]);
    out.push(0);
    shorts(&mut out, &[0]);
    words(&mut out, &[NULL]);
    shorts(&mut out, &[0]);
    words(&mut out, &[0]);
    out.push(0);
    shorts(&mut out, &[0]);
    out
}
fn instance(bones: u32) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL, 4, 0, bones]);
    for _ in 0..bones {
        words(&mut out, &[0]);
    }
    out
}
// Three local vertices deliberately map in authored order 2,0,1. The same
// signed-zero, negative and over-one weights appear on every vertex.
fn packet(strips: bool) -> Vec<u8> {
    let mut out = Vec::new();
    shorts(&mut out, &[3, 1, 2, u16::from(strips), 4, 1, 0]);
    out.push(1);
    shorts(&mut out, &[2, 0, 1]);
    out.push(1);
    for _ in 0..3 {
        words(
            &mut out,
            &[0x8000_0000, 0xBE80_0000, 0x3FA0_0000, 0x3F80_0000],
        );
    }
    if strips {
        shorts(&mut out, &[4]);
    }
    out.push(1);
    shorts(&mut out, if strips { &[2, 0, 1, 1] } else { &[2, 0, 1] });
    out.push(1);
    out.extend([1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0]);
    out
}
fn payload(packets: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[packets.len() as u32]);
    for packet in packets {
        out.extend(packet);
    }
    out
}
fn fixture(stream: u32, packets: &[Vec<u8>]) -> Vec<(&'static str, Vec<u8>)> {
    let mut node = av(stream);
    words(&mut node, &[0, 0]);
    vec![
        ("NiNode", node),
        ("NiSkinInstance", instance(2)),
        ("NiTriShapeData", geometry(3)),
        ("NiTriShape", shape(stream, 2, 1)),
        ("NiSkinPartition", payload(packets)),
    ]
}
fn failure(blocks: &[(&str, Vec<u8>)], expected: &str) {
    let error = partition::decode(&container(blocks, 34), "authored partition")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}

fn stream_fixture(packets: &[Vec<u8>]) -> Vec<(&'static str, Vec<u8>)> {
    let mut blocks = fixture(34, packets);
    blocks[1].1.clear();
    words(&mut blocks[1].1, &[NULL, 4, 0, 2, 5, 6]);
    let mut leaf = av(34);
    words(&mut leaf, &[0, 0]);
    blocks.extend([("NiNode", leaf.clone()), ("NiNode", leaf)]);
    blocks
}
fn stream_request(bytes: &[u8], ordinal: usize) -> partition::streams::Request {
    use sha2::{Digest, Sha256};
    partition::streams::Request {
        expected_source_sha256: Sha256::digest(bytes).into(),
        geometry: 3,
        partition_block: 4,
        partition_ordinal: ordinal,
    }
}
fn stream_prepare(blocks: &[(&str, Vec<u8>)], ordinal: usize) -> partition::streams::Streams {
    let bytes = container(blocks, 34);
    partition::streams::prepare(
        &bytes,
        "authored streams",
        stream_request(&bytes, ordinal),
        Default::default(),
    )
    .unwrap()
}

#[test]
fn source_streams_keep_every_raw_slot_nonidentity_map_palette_and_independent_span() {
    let blocks = stream_fixture(&[packet(false), packet(true)]);
    let bytes = container(&blocks, 34);
    let streams = stream_prepare(&blocks, 0);
    let identity = streams.identity();
    assert_eq!(
        (
            identity.geometry.block,
            identity.geometry_data.block,
            identity.instance.block,
            identity.partition.block,
            identity.partition_ordinal,
            identity.source_vertex_count
        ),
        (3, 2, 1, 4, 0, 3)
    );
    let span_offset = bytes
        .windows(blocks[4].1.len())
        .position(|v| v == blocks[4].1)
        .unwrap();
    assert_eq!(
        (identity.partition.offset, identity.partition.bytes),
        (span_offset, blocks[4].1.len())
    );
    assert_eq!(
        streams
            .palette()
            .iter()
            .map(|p| (p.local_bone, p.global_bone_ordinal, p.source_bone_node))
            .collect::<Vec<_>>(),
        [(0, 1, 6), (1, 0, 5)]
    );
    assert_eq!(
        streams
            .vertices()
            .iter()
            .map(|v| (
                v.local_vertex,
                v.source_vertex,
                v.influence_start,
                v.influence_count
            ))
            .collect::<Vec<_>>(),
        [(0, 2, 0, 4), (1, 0, 4, 4), (2, 1, 8, 4)]
    );
    let expected = [
        (0, 0, 2, 0, 1, 0, 5, 0x8000_0000),
        (1, 0, 2, 1, 0, 1, 6, 0xBE80_0000),
        (2, 0, 2, 2, 1, 0, 5, 0x3FA0_0000),
        (3, 0, 2, 3, 0, 1, 6, 0x3F80_0000),
        (4, 1, 0, 0, 0, 1, 6, 0x8000_0000),
        (5, 1, 0, 1, 1, 0, 5, 0xBE80_0000),
        (6, 1, 0, 2, 0, 1, 6, 0x3FA0_0000),
        (7, 1, 0, 3, 1, 0, 5, 0x3F80_0000),
        (8, 2, 1, 0, 1, 0, 5, 0x8000_0000),
        (9, 2, 1, 1, 1, 0, 5, 0xBE80_0000),
        (10, 2, 1, 2, 0, 1, 6, 0x3FA0_0000),
        (11, 2, 1, 3, 0, 1, 6, 0x3F80_0000),
    ];
    assert_eq!(
        streams
            .influences()
            .iter()
            .map(|v| (
                v.source_weight_ordinal,
                v.local_vertex,
                v.source_vertex,
                v.slot,
                v.local_bone,
                v.global_bone_ordinal,
                v.source_bone_node,
                v.weight_bits
            ))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(serde_json::to_value(&streams).unwrap()["retail_behavior_verified"] == false);
}

#[test]
fn source_streams_preserve_typed_triangles_strips_lengths_and_degenerate_source_order() {
    use partition::streams::Topology;
    let blocks = stream_fixture(&[packet(false), packet(true)]);
    let triangles = stream_prepare(&blocks, 0);
    match triangles.topology() {
        Topology::Triangles { triangles } => {
            assert_eq!(triangles.len(), 1);
            assert_eq!(triangles[0].local_vertices, [2, 0, 1]);
            assert_eq!(triangles[0].source_vertices, [1, 2, 0]);
        }
        _ => panic!("authored triangle branch changed"),
    }
    let strips = stream_prepare(&blocks, 1);
    match strips.topology() {
        Topology::Strips { lengths, strips } => {
            assert_eq!(lengths, &[4]);
            assert_eq!(strips.len(), 1);
            assert_eq!(strips[0].local_vertices, [2, 0, 1, 1]);
            assert_eq!(strips[0].source_vertices, [1, 2, 0, 0]);
        }
        _ => panic!("authored strip branch triangulated"),
    }
    let observation = serde_json::to_value(&strips).unwrap();
    assert_eq!(observation["declared_triangles"], 1);
    assert_eq!(
        (triangles.usage().draw_indices, strips.usage().draw_indices),
        (3, 4)
    );
}

#[test]
fn source_streams_require_exact_identity_owner_partition_ordinal_and_complete_presence() {
    use partition::streams;
    let blocks = stream_fixture(&[packet(false)]);
    let bytes = container(&blocks, 34);
    let request = stream_request(&bytes, 0);
    let mut wrong = request;
    wrong.expected_source_sha256[0] ^= 1;
    for (request, expected) in [
        (wrong, "SHA256 differs"),
        (
            streams::Request {
                geometry: 0,
                ..request
            },
            "no decoded skin owner",
        ),
        (
            streams::Request {
                partition_block: 5,
                ..request
            },
            "partition link differs",
        ),
        (
            streams::Request {
                partition_ordinal: 1,
                ..request
            },
            "ordinal unavailable",
        ),
    ] {
        let error = streams::prepare(&bytes, "refusal", request, Default::default()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut absent = Vec::new();
    shorts(&mut absent, &[3, 0, 2, 0, 4, 1, 0]);
    absent.extend([0, 0, 0, 0]);
    let absent = container(&stream_fixture(&[absent]), 34);
    let error = streams::prepare(
        &absent,
        "absent",
        stream_request(&absent, 0),
        Default::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("requires authored"), "{error}");
    let mut body = blocks;
    body[1].0 = "BSDismemberSkinInstance";
    words(&mut body[1].1, &[2]);
    shorts(&mut body[1].1, &[1, 0, 257, 42]);
    let body = container(&body, 34);
    let error = streams::prepare(
        &body,
        "dismember",
        stream_request(&body, 0),
        Default::default(),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("body-part association unavailable"),
        "{error}"
    );
}

#[test]
fn source_streams_refuse_out_of_range_domains_and_unresolved_or_wrong_type_bone_links() {
    use partition::streams;
    let mut bad_local = packet(false);
    bad_local[78] = 2;
    let mut bad_global = packet(false);
    set_short(&mut bad_global, 10, 2);
    let mut bad_vertex = packet(false);
    set_short(&mut bad_vertex, 15, 3);
    let mut bad_draw = packet(false);
    set_short(&mut bad_draw, 71, 3);
    for (packet, expected) in [
        (bad_local, "outside palette"),
        (bad_global, "outside linked instance"),
        (bad_vertex, "outside linked geometry"),
        (bad_draw, "outside local vertices"),
    ] {
        let bytes = container(&stream_fixture(&[packet]), 34);
        let error = streams::prepare(
            &bytes,
            "domain",
            stream_request(&bytes, 0),
            Default::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    for (target, expected) in [
        (NULL, "no source bone link"),
        (2, "skin bone link has wrong target kind"),
    ] {
        let mut blocks = stream_fixture(&[packet(false)]);
        blocks[1].1[16..20].copy_from_slice(&target.to_le_bytes());
        let bytes = container(&blocks, 34);
        let error = streams::prepare(
            &bytes,
            "bone",
            stream_request(&bytes, 0),
            Default::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn source_streams_rows_draw_storage_work_and_decoder_admissions_have_exact_ceilings() {
    use partition::streams;
    let bytes = container(&stream_fixture(&[packet(false), packet(true)]), 34);
    let request = stream_request(&bytes, 1);
    let baseline = streams::prepare(&bytes, "baseline", request, Default::default()).unwrap();
    let usage = baseline.usage();
    let mut exact = streams::Limits {
        influence_rows: 12,
        draw_indices: 4,
        array_bytes: usage.retained_bytes,
        work_units: usage.work_units,
        decoder_array_admission_bytes: usage.decoder_array_admission_bytes,
        decoder_check_admission_units: usage.decoder_check_admission_units,
        ..Default::default()
    };
    exact.source.skin.scene.input_bytes = bytes.len();
    streams::prepare(&bytes, "exact", request, exact).unwrap();
    for (limits, expected) in [
        (
            streams::Limits {
                influence_rows: 11,
                ..exact
            },
            "row product budget",
        ),
        (
            streams::Limits {
                draw_indices: 3,
                ..exact
            },
            "draw index count budget",
        ),
        (
            streams::Limits {
                array_bytes: exact.array_bytes - 1,
                ..exact
            },
            "array storage budget",
        ),
        (
            streams::Limits {
                work_units: exact.work_units - 1,
                ..exact
            },
            "work budget",
        ),
        (
            streams::Limits {
                decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
                ..exact
            },
            "decoder array admission",
        ),
        (
            streams::Limits {
                decoder_check_admission_units: exact.decoder_check_admission_units - 1,
                ..exact
            },
            "decoder check admission",
        ),
    ] {
        let error = streams::prepare(&bytes, "under", request, limits).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    let mut short = exact;
    short.source.skin.scene.input_bytes -= 1;
    assert!(
        streams::prepare(&bytes, "input", request, short)
            .unwrap_err()
            .to_string()
            .contains("input byte budget")
    );
    let mut overflow = exact;
    overflow.source.array_bytes = usize::MAX;
    assert!(
        streams::prepare(&bytes, "overflow", request, overflow)
            .unwrap_err()
            .to_string()
            .contains("decoder array admission")
    );
}

#[test]
fn source_streams_empty_present_rows_keep_unused_width_and_do_not_synthesize_arrays() {
    let mut empty = Vec::new();
    shorts(&mut empty, &[0, 0, 0, 0, 17]);
    empty.extend([1, 1, 1, 1]);
    let streams = stream_prepare(&stream_fixture(&[empty]), 0);
    assert!(
        streams.vertices().is_empty()
            && streams.influences().is_empty()
            && streams.palette().is_empty()
    );
    assert_eq!(
        serde_json::to_value(&streams).unwrap()["weights_per_vertex"],
        17
    );
    assert_eq!(
        (streams.usage().influence_rows, streams.usage().draw_indices),
        (0, 0)
    );
}

#[test]
fn exact_raw_fields_on_all_admitted_streams_and_both_face_branches() {
    for stream in STREAMS {
        let bytes = container(&fixture(stream, &[packet(false), packet(true)]), stream);
        let (index, source) = partition::decode(&bytes, "authored").unwrap();
        let block = &source.partitions.blocks[0];
        assert_eq!(
            (block.block, block.offset, block.bytes),
            (4, index.blocks[4].offset, index.blocks[4].bytes)
        );
        assert_eq!(block.sha256.len(), 64);
        for part in &block.partitions {
            assert_eq!(
                (
                    part.num_vertices,
                    part.num_triangles,
                    part.num_bones,
                    part.weights_per_vertex
                ),
                (3, 1, 2, 4)
            );
            assert_eq!(part.bone_palette, [1, 0]);
            assert_eq!(part.vertex_map, [2, 0, 1]);
            assert_eq!(
                (
                    part.has_vertex_map,
                    part.has_vertex_weights,
                    part.has_faces,
                    part.has_bone_indices
                ),
                (1, 1, 1, 1)
            );
            assert_eq!(
                part.weight_bits,
                [0x8000_0000, 0xBE80_0000, 0x3FA0_0000, 0x3F80_0000].repeat(3)
            );
            assert_eq!(part.bone_indices, [1, 0, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0]);
        }
        assert_eq!(block.partitions[0].triangles, [[2, 0, 1]]);
        assert!(block.partitions[0].strips.is_empty());
        assert_eq!(block.partitions[1].strip_lengths, [4]);
        assert_eq!(block.partitions[1].strips, [vec![2, 0, 1, 1]]);
        assert!(block.partitions[1].triangles.is_empty());
        assert!(source.partitions.dependencies.is_empty());
        assert!(!source.partitions.runtime_ready);
        assert!(
            !source
                .skin
                .dependencies
                .iter()
                .any(|d| matches!(d, nif_skin::Dependency::PartitionPayload { .. }))
        );
        let (_, original) = nif_skin::decode(&bytes, "original scope").unwrap();
        assert!(
            original
                .dependencies
                .iter()
                .any(|d| matches!(d, nif_skin::Dependency::PartitionPayload { target: 4, .. }))
        );
    }
}

#[test]
fn authored_absent_arrays_and_non_four_unused_width_remain_explicit() {
    let mut packet = Vec::new();
    shorts(&mut packet, &[3, 9, 2, 2, 17, 1, 0]);
    packet.extend([0, 0]);
    shorts(&mut packet, &[6, 8]);
    packet.extend([0, 0]);
    let (_, source) = partition::decode(&container(&fixture(34, &[packet]), 34), "absent").unwrap();
    let p = &source.partitions.blocks[0].partitions[0];
    assert_eq!(
        (p.num_vertices, p.num_triangles, p.weights_per_vertex),
        (3, 9, 17)
    );
    assert_eq!(p.strip_lengths, [6, 8]);
    assert!(
        p.vertex_map.is_empty()
            && p.weight_bits.is_empty()
            && p.strips.is_empty()
            && p.bone_indices.is_empty()
    );
    assert!(matches!(
        source.partitions.dependencies.as_slice(),
        [Dependency::AbsentArrays { fields: 15, .. }]
    ));
}

#[test]
fn empty_present_arrays_preserve_width_and_flags() {
    let mut packet = Vec::new();
    shorts(&mut packet, &[0, 0, 0, 0, 65535]);
    packet.extend([1; 4]);
    let (_, source) = partition::decode(&container(&fixture(34, &[packet]), 34), "empty").unwrap();
    let p = &source.partitions.blocks[0].partitions[0];
    assert_eq!(p.weights_per_vertex, 65535);
    assert_eq!(p.has_bone_indices, 1);
    assert!(source.partitions.dependencies.is_empty());
}

#[test]
fn noncanonical_presence_and_nonempty_unverified_widths_are_unsupported() {
    // Packet offsets before the 4-byte block packet count.
    for offset in [14, 21, 70, 77] {
        for value in [2, 255] {
            let mut p = packet(false);
            p[offset] = value;
            failure(&fixture(34, &[p]), "noncanonical partition presence");
        }
    }
    for width in [0, 1, 3, 5, 65535] {
        let mut p = packet(false);
        set_short(&mut p, 8, width);
        failure(&fixture(34, &[p]), "requires four");
    }
}

#[test]
fn every_truncated_prefix_and_surplus_are_rejected() {
    for strips in [false, true] {
        let original = payload(&[packet(strips)]);
        for length in 0..original.len() {
            let mut blocks = fixture(34, &[]);
            blocks[4].1 = original[..length].to_vec();
            assert!(
                partition::decode(&container(&blocks, 34), "truncated").is_err(),
                "length {length}"
            );
        }
        let mut blocks = fixture(34, &[]);
        blocks[4].1 = original;
        blocks[4].1.push(0);
        failure(&blocks, "unconsumed bytes");
    }
}

#[test]
fn local_triangle_strip_and_palette_byte_indices_are_checked() {
    for (strips, offset, reason) in [
        (false, 71, "triangle index"),
        (true, 73, "vertex index"),
        (false, 78, "byte bone index"),
    ] {
        let mut p = packet(strips);
        if offset == 78 {
            p[offset] = 2;
        } else {
            set_short(&mut p, offset, 3);
        }
        failure(&fixture(34, &[p]), reason);
    }
}

#[test]
fn nonfinite_weights_are_rejected_without_normalizing_finite_words() {
    for word in [0x7F80_0000u32, 0xFF80_0000, 0x7FC0_0001] {
        let mut p = packet(false);
        p[22..26].copy_from_slice(&word.to_le_bytes());
        failure(&fixture(34, &[p]), "nonfinite");
    }
}

#[test]
fn all_shared_geometry_owners_constrain_vertex_map() {
    let mut blocks = fixture(34, &[packet(false)]);
    blocks.push(("NiTriShapeData", geometry(2)));
    blocks.push(("NiTriShape", shape(34, 5, 1)));
    failure(&blocks, "vertex map index outside linked geometry");
    blocks[5].1 = geometry(3);
    assert!(partition::decode(&container(&blocks, 34), "shared valid").is_ok());
}

#[test]
fn all_instances_sharing_partition_constrain_palette() {
    let mut blocks = fixture(34, &[packet(false)]);
    blocks.push(("NiSkinInstance", instance(1)));
    failure(&blocks, "palette index outside linked instance");
}

#[test]
fn missing_owners_geometry_and_unowned_partitions_are_dependencies() {
    let mut blocks = fixture(34, &[packet(false)]);
    blocks[3].1 = shape(34, NULL, 1);
    let (_, s) = partition::decode(&container(&blocks, 34), "unknown").unwrap();
    assert!(
        s.partitions
            .dependencies
            .iter()
            .any(|d| matches!(d, Dependency::UnknownGeometry { geometry: 3, .. }))
    );
    blocks[3].1 = shape(34, 2, NULL);
    let (_, s) = partition::decode(&container(&blocks, 34), "missing owner").unwrap();
    assert!(
        s.partitions
            .dependencies
            .iter()
            .any(|d| matches!(d, Dependency::MissingOwner { instance: 1, .. }))
    );
    blocks[1].1[4..8].copy_from_slice(&NULL.to_le_bytes());
    let (_, s) = partition::decode(&container(&blocks, 34), "unowned").unwrap();
    assert!(
        s.partitions
            .dependencies
            .iter()
            .any(|d| matches!(d, Dependency::UnownedPartition { partition: 4 }))
    );
}

#[test]
fn dismember_count_mismatch_is_diagnostic_and_not_repaired() {
    let mut blocks = fixture(34, &[packet(false)]);
    blocks[1].0 = "BSDismemberSkinInstance";
    words(&mut blocks[1].1, &[2]);
    shorts(&mut blocks[1].1, &[1, 201, 2, 202]);
    let (_, s) = partition::decode(&container(&blocks, 34), "count mismatch").unwrap();
    assert!(matches!(
        s.partitions.dependencies.as_slice(),
        [Dependency::BodyPartCountMismatch {
            body_parts: 2,
            partitions: 1,
            ..
        }]
    ));
}

#[test]
fn aggregate_storage_and_repeated_relation_work_budgets_are_explicit() {
    let bytes = container(&fixture(34, &[packet(false)]), 34);
    let error = partition::decode_with_limits(
        &bytes,
        "storage",
        Limits {
            array_bytes: 1,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("budget"));
    // Palette2+relation1 plus map3+relation1 = seven checks.
    assert!(
        partition::decode_with_limits(
            &bytes,
            "seven",
            Limits {
                index_checks: 7,
                ..Default::default()
            }
        )
        .is_ok()
    );
    let error = partition::decode_with_limits(
        &bytes,
        "six",
        Limits {
            index_checks: 6,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("index-check budget"));
    let mut empty = Vec::new();
    shorts(&mut empty, &[0; 5]);
    empty.extend([0; 4]);
    let mut blocks = fixture(34, &[empty.clone(), empty]);
    blocks.push(("NiTriShape", shape(34, 2, 1)));
    // Two empty partitions still cost six relation checks across two owners.
    let error = partition::decode_with_limits(
        &container(&blocks, 34),
        "empty relations",
        Limits {
            index_checks: 5,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("index-check budget"));
}

#[test]
fn malicious_counts_are_bounded_before_allocation() {
    let mut blocks = fixture(34, &[packet(false)]);
    blocks[4].1[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    failure(&blocks, "budget");
    for offset in [0, 2, 4, 6] {
        let mut p = packet(false);
        set_short(&mut p, offset, 65535);
        assert!(partition::decode(&container(&fixture(34, &[p]), 34), "oversized counts").is_err());
    }
}

#[test]
fn zero_partitions_is_preserved() {
    let (_, s) = partition::decode(&container(&fixture(34, &[]), 34), "zero").unwrap();
    assert!(s.partitions.blocks[0].partitions.is_empty());
}

#[test]
fn retained_arrays_and_relation_map_source_counts_have_distinct_boundaries() {
    let blocks = fixture(34, &[packet(false)]);
    let bytes = container(&blocks, 34);
    let (_, decoded) = partition::decode(&bytes, "measured retained charge").unwrap();
    let charge = decoded.partitions.retained_bytes;
    let mut limits = Limits {
        array_bytes: charge,
        ..Default::default()
    };
    limits.skin.scene.blocks = blocks.len();
    assert!(partition::decode_with_limits(&bytes, "exact boundaries", limits).is_ok());
    limits.array_bytes -= 1;
    assert!(partition::decode_with_limits(&bytes, "one byte short", limits).is_err());
    limits.array_bytes = charge;
    limits.skin.scene.blocks -= 1;
    assert!(partition::decode_with_limits(&bytes, "one source block short", limits).is_err());

    // Existing source owners still populate temporary relation maps even when
    // no partition record is retained. A zero array budget is not a scratch cap.
    let mut blocks = fixture(34, &[]);
    blocks.pop();
    blocks[1].1[4..8].copy_from_slice(&NULL.to_le_bytes());
    let (_, decoded) = partition::decode_with_limits(
        &container(&blocks, 34),
        "empty partition catalogue",
        Limits {
            array_bytes: 0,
            index_checks: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(decoded.skin.owners.len(), 1);
    assert!(decoded.partitions.blocks.is_empty());
    assert_eq!(decoded.partitions.retained_bytes, 0);
}
