//! Original source fixtures. Expected source words are specified independently
//! of decoding, and no retail NIF bytes are committed.
use fallout_data::{
    nif,
    nif_skin::{self, Data, Dependency, Limits},
};

const NULL: u32 = u32::MAX;
const STREAMS: [u32; 12] = [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34];

fn u16s(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}
fn u32s(bytes: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}
fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}
fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    u32s(&mut bytes, &[0x1402_0007]);
    bytes.push(1);
    u32s(&mut bytes, &[11, blocks.len() as u32, stream]);
    bytes.extend([0; 3]);
    let mut names = Vec::new();
    for (name, _) in blocks {
        if !names.contains(name) {
            names.push(*name);
        }
    }
    u16s(&mut bytes, &[names.len() as u16]);
    for name in &names {
        u32s(&mut bytes, &[name.len() as u32]);
        bytes.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        u16s(
            &mut bytes,
            &[names.iter().position(|entry| entry == name).unwrap() as u16],
        );
    }
    for (_, block) in blocks {
        u32s(&mut bytes, &[block.len() as u32]);
    }
    u32s(&mut bytes, &[0, 0, 0]); // no strings or groups
    for (_, block) in blocks {
        bytes.extend(block);
    }
    u32s(&mut bytes, &[0]); // no footer roots
    bytes
}

fn av(stream: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    u32s(&mut bytes, &[NULL, 0, NULL]);
    if stream <= 26 {
        u16s(&mut bytes, &[14]);
    } else {
        u32s(&mut bytes, &[14]);
    }
    floats(
        &mut bytes,
        &[0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 1., 1.],
    );
    u32s(&mut bytes, &[0, NULL]);
    bytes
}
fn node(stream: u32) -> Vec<u8> {
    let mut bytes = av(stream);
    u32s(&mut bytes, &[0, 0]);
    bytes
}
fn shape(stream: u32, geometry: u32, skin: u32) -> Vec<u8> {
    let mut bytes = av(stream);
    u32s(&mut bytes, &[geometry, skin, 0, NULL]);
    bytes.push(0);
    bytes
}
fn geometry(vertices: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    u32s(&mut bytes, &[0]);
    u16s(&mut bytes, &[vertices]);
    bytes.extend([0, 0, 0]); // flags and absent positions
    u16s(&mut bytes, &[0]);
    bytes.push(0); // absent normals
    floats(&mut bytes, &[0., 0., 0., 1.]);
    bytes.push(0); // absent colors
    u16s(&mut bytes, &[0]);
    u32s(&mut bytes, &[NULL]);
    u16s(&mut bytes, &[0]); // triangles
    u32s(&mut bytes, &[0]); // triangle points
    bytes.push(0); // absent triangle indices
    u16s(&mut bytes, &[0]); // match groups
    bytes
}
fn source_transform(bytes: &mut Vec<u8>) {
    floats(
        bytes,
        &[1., -0., 3., 4., 5., 6., 7., 8., 9., -10., 11., 12., 0.5],
    );
}
fn skin_data(flag: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    source_transform(&mut bytes);
    u32s(&mut bytes, &[2]);
    bytes.push(flag);
    for (vertices, weights) in [
        (2u16, vec![(0u16, -0.0f32), (2, 1.25)]),
        (1u16, vec![(1u16, -0.25f32)]),
    ] {
        source_transform(&mut bytes);
        floats(&mut bytes, &[13., 14., 15., 16.]);
        u16s(
            &mut bytes,
            &[if flag == 0 { vertices + 7 } else { vertices }],
        );
        if flag != 0 {
            for (vertex, weight) in weights {
                u16s(&mut bytes, &[vertex]);
                floats(&mut bytes, &[weight]);
            }
        }
    }
    bytes
}
fn instance(dismember: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    u32s(&mut bytes, &[1, 5, 0, 2, 0, 0]); // authored duplicate bone pointers
    if dismember {
        u32s(&mut bytes, &[2]);
        u16s(&mut bytes, &[0x8101, 65000, 0xFFFF, 201]);
    }
    bytes
}
fn fixture(stream: u32, flag: u8, dismember: bool) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiNode", node(stream)),
        ("NiSkinData", skin_data(flag)),
        (
            if dismember {
                "BSDismemberSkinInstance"
            } else {
                "NiSkinInstance"
            },
            instance(dismember),
        ),
        ("NiTriShapeData", geometry(3)),
        ("NiTriShape", shape(stream, 3, 2)),
        ("NiSkinPartition", Vec::new()), // deliberately undecoded first-slice dependency
    ]
}
fn failure(blocks: &[(&str, Vec<u8>)], expected: &str) {
    let error = nif_skin::decode(&container(blocks, 34), "authored")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}

#[test]
fn exact_source_fields_and_all_admitted_streams() {
    for stream in STREAMS {
        for dismember in [false, true] {
            let bytes = container(&fixture(stream, 255, dismember), stream);
            let (index, skin) = nif_skin::decode(&bytes, "authored").unwrap();
            assert_eq!(skin.blocks.len(), 2);
            assert_eq!(skin.owners.len(), 1);
            assert_eq!(skin.owners[0].vertex_count, Some(3));
            assert!(!skin.runtime_ready);
            let block = &skin.blocks[0];
            assert_eq!(
                (block.offset, block.bytes),
                (index.blocks[1].offset, index.blocks[1].bytes)
            );
            assert_eq!(block.sha256.len(), 64);
            let Data::SkinData {
                transform,
                has_vertex_weights,
                bones,
            } = &block.data
            else {
                panic!()
            };
            assert_eq!(*has_vertex_weights, 255);
            assert_eq!(
                transform.rotation_bits[0],
                [0x3F80_0000, 0x8000_0000, 0x4040_0000]
            );
            assert_eq!(
                transform.translation_bits,
                [0xC120_0000, 0x4130_0000, 0x4140_0000]
            );
            assert_eq!(transform.scale_bits, 0x3F00_0000);
            assert_eq!(
                bones[0].center_bits,
                [0x4150_0000, 0x4160_0000, 0x4170_0000]
            );
            assert_eq!(bones[0].radius_bits, 0x4180_0000);
            assert_eq!(bones[0].declared_vertices, 2);
            assert_eq!(bones[0].weights[0].weight_bits, 0x8000_0000);
            assert_eq!(bones[0].weights[1].weight_bits, 0x3FA0_0000);
            assert_eq!(bones[1].weights[0].weight_bits, 0xBE80_0000);
            let Data::Instance { instance } = &skin.blocks[1].data else {
                panic!()
            };
            assert_eq!(instance.bones, [Some(0), Some(0)]);
            if dismember {
                let parts = instance.body_parts.as_ref().unwrap();
                assert_eq!((parts[0].flags, parts[0].body_part), (0x8101, 65000));
                assert_eq!((parts[1].flags, parts[1].body_part), (0xFFFF, 201));
            } else {
                assert!(instance.body_parts.is_none());
            }
            assert!(matches!(
                skin.dependencies.as_slice(),
                [Dependency::PartitionPayload {
                    instance: 2,
                    target: 5
                }]
            ));
        }
    }
}

#[test]
fn absent_weight_flag_preserves_authored_nonzero_counts() {
    let (_, skin) = nif_skin::decode(&container(&fixture(34, 0, true), 34), "no weights").unwrap();
    let Data::SkinData {
        has_vertex_weights,
        bones,
        ..
    } = &skin.blocks[0].data
    else {
        panic!()
    };
    assert_eq!(*has_vertex_weights, 0);
    assert_eq!(
        (bones[0].declared_vertices, bones[1].declared_vertices),
        (9, 8)
    );
    assert!(bones.iter().all(|bone| bone.weights.is_empty()));
}

#[test]
fn null_links_are_preserved_as_dependencies() {
    let mut blocks = fixture(34, 1, false);
    for offset in [0, 4, 8, 16, 20] {
        set_u32(&mut blocks[2].1, offset, NULL);
    }
    let (_, skin) = nif_skin::decode(&container(&blocks, 34), "null links").unwrap();
    let Data::Instance { instance } = &skin.blocks[1].data else {
        panic!()
    };
    assert_eq!(
        (instance.data, instance.partition, instance.skeleton_root),
        (None, None, None)
    );
    assert_eq!(instance.bones, [None, None]);
    assert!(
        skin.dependencies
            .iter()
            .any(|value| matches!(value, Dependency::MissingData { instance: 2 }))
    );
    assert!(skin.dependencies.iter().any(|value| matches!(
        value,
        Dependency::MissingBone {
            instance: 2,
            ordinal: 1
        }
    )));
    assert!(!skin.runtime_ready);
}

#[test]
fn wrong_kind_and_out_of_range_instance_links_fail() {
    for (offset, target, expected) in [
        (0, 0, "data link"),
        (4, 0, "partition link"),
        (8, 1, "skeleton-root link"),
        (16, 1, "bone link"),
        (20, 6, "index out of range"),
    ] {
        let mut blocks = fixture(34, 1, true);
        set_u32(&mut blocks[2].1, offset, target);
        failure(&blocks, expected);
    }
}

#[test]
fn geometry_skin_link_requires_an_instance() {
    let mut blocks = fixture(34, 1, false);
    blocks[4].1 = shape(34, 3, 1);
    failure(&blocks, "skin link targets NiSkinData");
    blocks[4].1 = shape(34, 3, 0);
    failure(&blocks, "supported skin instance");
}

#[test]
fn bone_count_mismatch_fails_without_repair() {
    let mut blocks = fixture(34, 1, false);
    set_u32(&mut blocks[2].1, 12, 1);
    blocks[2].1.truncate(20);
    failure(&blocks, "bone-count mismatch");
}

#[test]
fn every_shared_owner_constrains_weight_indices() {
    let mut blocks = fixture(34, 1, true);
    blocks.push(("NiTriShapeData", geometry(3)));
    blocks.push(("NiTriShape", shape(34, 6, 2)));
    let (_, skin) = nif_skin::decode(&container(&blocks, 34), "shared").unwrap();
    assert_eq!(
        skin.owners
            .iter()
            .map(|owner| owner.geometry)
            .collect::<Vec<_>>(),
        [4, 7]
    );
    blocks[6].1 = geometry(2);
    failure(&blocks, "owner geometry 7 with 2 vertices");
}

#[test]
fn repeated_shared_owner_validation_has_a_work_budget() {
    let mut blocks = fixture(34, 1, false);
    blocks.push(("NiTriShape", shape(34, 3, 2)));
    let bytes = container(&blocks, 34);
    let limits = Limits {
        weight_index_checks: 6,
        ..Default::default()
    };
    assert!(nif_skin::decode_with_limits(&bytes, "six checks", limits).is_ok());
    let limits = Limits {
        weight_index_checks: 5,
        ..Default::default()
    };
    let error = nif_skin::decode_with_limits(&bytes, "five checks", limits).unwrap_err();
    assert!(error.to_string().contains("index-check budget"));
}

#[test]
fn shared_data_is_checked_through_distinct_instances() {
    let mut blocks = fixture(34, 1, false);
    blocks.push(("NiSkinInstance", instance(false)));
    blocks.push(("NiTriShapeData", geometry(1)));
    blocks.push(("NiTriShape", shape(34, 7, 6)));
    failure(&blocks, "owner geometry 8 with 1 vertices");
}

#[test]
fn unowned_sources_and_missing_geometry_cannot_certify_vertex_bounds() {
    let mut blocks = fixture(34, 1, false);
    blocks[4].1 = shape(34, NULL, 2);
    let (_, skin) = nif_skin::decode(&container(&blocks, 34), "missing geometry").unwrap();
    assert!(
        skin.dependencies
            .iter()
            .any(|value| matches!(value, Dependency::MissingGeometryData { geometry: 4 }))
    );
    blocks[4].1 = shape(34, 3, NULL);
    let (_, skin) = nif_skin::decode(&container(&blocks, 34), "unowned").unwrap();
    assert!(
        skin.dependencies
            .iter()
            .any(|value| matches!(value, Dependency::UnownedInstance { instance: 2 }))
    );
    assert!(
        skin.dependencies
            .iter()
            .any(|value| matches!(value, Dependency::UnownedData { data: 1 }))
    );
}

#[test]
fn known_undecoded_node_subtype_remains_a_dependency() {
    let mut blocks = fixture(34, 1, false);
    blocks[0] = ("NiBillboardNode", Vec::new());
    let (_, skin) = nif_skin::decode(&container(&blocks, 34), "subtype").unwrap();
    assert_eq!(
        skin.dependencies
            .iter()
            .filter(|value| matches!(value, Dependency::UndecodedNode { target: 0, .. }))
            .count(),
        3
    );
}

#[test]
fn truncation_and_surplus_are_rejected_for_each_supported_block() {
    for block in [1, 2] {
        for dismember in [false, true] {
            let original = fixture(34, 1, dismember);
            for length in 0..original[block].1.len() {
                let mut blocks = original.clone();
                blocks[block].1.truncate(length);
                assert!(nif_skin::decode(&container(&blocks, 34), "truncated").is_err());
            }
            let mut blocks = original;
            blocks[block].1.push(0);
            failure(&blocks, "unconsumed bytes");
        }
    }
}

#[test]
fn malicious_counts_fail_before_allocation() {
    for (block, offset) in [(1, 52), (2, 12), (2, 24)] {
        let mut blocks = fixture(34, 1, true);
        set_u32(&mut blocks[block].1, offset, u32::MAX);
        failure(&blocks, "budget");
    }
}

#[test]
fn nonfinite_transform_bound_and_weight_fields_fail() {
    // Data transform, first bone transform, center, radius and weight positions.
    for offset in [0, 36, 48, 57, 109, 121, 129] {
        for value in [0x7FC0_0001, 0x7F80_0000, 0xFF80_0000] {
            let mut blocks = fixture(34, 1, false);
            set_u32(&mut blocks[1].1, offset, value);
            failure(&blocks, "nonfinite");
        }
    }
}

#[test]
fn negative_radius_fails_and_out_of_range_weight_fails() {
    let mut blocks = fixture(34, 1, false);
    set_u32(&mut blocks[1].1, 121, (-1.0f32).to_bits());
    failure(&blocks, "negative skin bone bounding radius");
    blocks = fixture(34, 1, false);
    blocks[1].1[127..129].copy_from_slice(&3u16.to_le_bytes());
    failure(&blocks, "skin vertex index");
}

#[test]
fn skin_storage_input_and_block_limits_fail_explicitly() {
    let bytes = container(&fixture(34, 1, true), 34);
    let limits = Limits {
        skin_array_bytes: 1,
        ..Default::default()
    };
    assert!(
        nif_skin::decode_with_limits(&bytes, "budget", limits)
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    let limits = Limits {
        scene: fallout_data::nif_scene::Limits {
            input_bytes: bytes.len() - 1,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(nif_skin::decode_with_limits(&bytes, "input budget", limits).is_err());
    let limits = Limits {
        scene: fallout_data::nif_scene::Limits {
            blocks: 5,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(nif_skin::decode_with_limits(&bytes, "block budget", limits).is_err());
}

#[test]
fn unsupported_version_user_stream_and_endian_have_no_fallback() {
    let original = container(&fixture(34, 1, false), 34);
    let header_bytes = b"Gamebryo File Format, Version 20.2.0.7\n".len();
    for (offset, value) in [
        (header_bytes, 0x1400_0004),
        (header_bytes + 5, 12),
        (header_bytes + 13, 100),
    ] {
        let mut bytes = original.clone();
        set_u32(&mut bytes, offset, value);
        assert!(nif_skin::decode(&bytes, "unsupported").is_err());
    }
    let mut bytes = original;
    bytes[header_bytes + 4] = 0;
    assert!(nif_skin::decode(&bytes, "big endian").is_err());
}

#[test]
fn empty_dismember_array_is_preserved_as_present() {
    let mut blocks = fixture(34, 1, true);
    set_u32(&mut blocks[2].1, 24, 0);
    blocks[2].1.truncate(28);
    let (_, skin) = nif_skin::decode(&container(&blocks, 34), "empty parts").unwrap();
    let Data::Instance { instance } = &skin.blocks[1].data else {
        panic!()
    };
    assert!(instance.body_parts.as_ref().unwrap().is_empty());
}

#[test]
fn source_hash_changes_when_only_raw_presence_changes() {
    let first = container(&fixture(34, 1, false), 34);
    let second = container(&fixture(34, 2, false), 34);
    let index = nif::inspect(&first, "hash").unwrap();
    assert_eq!(first[index.blocks[1].offset + 56], 1);
    let (_, left) = nif_skin::decode(&first, "hash").unwrap();
    let (_, right) = nif_skin::decode(&second, "hash").unwrap();
    assert_ne!(left.blocks[0].sha256, right.blocks[0].sha256);
    assert_eq!(left.blocks[1].sha256, right.blocks[1].sha256);
}
