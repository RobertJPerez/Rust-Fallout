//! Small authored files exercise the parser without distributing retail assets.
use fallout_data::nif_collision::{self, Data, Limits};

const NULL: u32 = u32::MAX;
fn words(bytes: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}
fn shorts(bytes: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}
fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        bytes.extend(value.to_le_bytes());
    }
}
fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut bytes, &[0x1402_0007]);
    bytes.push(1);
    words(&mut bytes, &[11, blocks.len() as u32, stream]);
    bytes.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    shorts(&mut bytes, &[types.len() as u16]);
    for name in &types {
        words(&mut bytes, &[name.len() as u32]);
        bytes.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        shorts(
            &mut bytes,
            &[types.iter().position(|v| v == name).unwrap() as u16],
        );
    }
    for (_, payload) in blocks {
        words(&mut bytes, &[payload.len() as u32]);
    }
    words(&mut bytes, &[0, 0, 0]); // No strings or groups.
    for (_, payload) in blocks {
        bytes.extend(payload);
    }
    words(&mut bytes, &[0]); // No visual roots are needed for a shape fixture.
    bytes
}
fn sphere() -> Vec<u8> {
    let mut bytes = Vec::new();
    words(&mut bytes, &[0x8000_0007]);
    floats(&mut bytes, &[0.125]);
    bytes
}
fn transform(shape: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    words(&mut bytes, &[shape, 9]);
    floats(&mut bytes, &[0.25]);
    bytes.extend([0x81; 8]);
    // Deliberately asymmetric: a silent transpose must not pass.
    floats(
        &mut bytes,
        &[
            1., 2., 3., 4., 5., 6., 7., 8., 9., 10., 11., 12., 13., 14., 15., 16.,
        ],
    );
    bytes
}
fn list(shapes: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::new();
    words(&mut bytes, &[shapes.len() as u32]);
    words(&mut bytes, shapes);
    words(&mut bytes, &[7, 0, 0, 0x8000_0001, 0, 0, 0x8000_0002, 1]);
    bytes.extend([3, 0xe1, 0x34, 0x12]);
    bytes
}
fn body(shape: u32, constraints: &[u32]) -> Vec<u8> {
    let mut bytes = vec![0; 232];
    put(&mut bytes, 0, shape);
    bytes[4..8].copy_from_slice(&[5, 0xe7, 0x34, 0x12]);
    bytes[13..16].copy_from_slice(&[0x80, 0x81, 0x82]);
    put(&mut bytes, 24, 0x8000_0009);
    put(&mut bytes, 52, (-0.0f32).to_bits());
    put(&mut bytes, 56, 125.0f32.to_bits());
    put(&mut bytes, 80, 1.0f32.to_bits()); // Quaternion w, source order xyzw.
    for offset in [128, 144, 160] {
        put(&mut bytes, offset, 0x7fc0_1234); // Alignment words may contain NaN bits.
    }
    put(&mut bytes, 180, 3.5f32.to_bits());
    bytes[212..216].copy_from_slice(&[7, 2, 1, 4]);
    put(&mut bytes, 228, constraints.len() as u32);
    words(&mut bytes, constraints);
    words(&mut bytes, &[0x8000_0020]);
    bytes
}
fn packed(compressed: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    words(&mut bytes, &[2]);
    shorts(&mut bytes, &[2, 0, 1, 0xe123, 1, 1, 2, 0x4321]);
    words(&mut bytes, &[3]);
    bytes.push(u8::from(compressed));
    if compressed {
        shorts(
            &mut bytes,
            &[
                0x8000, 0x7c01, 0x03ff, 0x1234, 0xffff, 0, 0xabcd, 0x4000, 0x3c00,
            ],
        );
    } else {
        floats(&mut bytes, &[-0., 0., 0., 25., 0., 0., 0., 100., 0.]);
    }
    shorts(&mut bytes, &[1]);
    bytes.extend([7, 0xf0, 0x34, 0x12]);
    words(&mut bytes, &[3, 0x8000_002a]);
    bytes
}
fn error(blocks: &[(&str, Vec<u8>)]) -> String {
    nif_collision::decode(&container(blocks, 34), "authored collision")
        .unwrap_err()
        .to_string()
}

#[test]
fn body_layout_preserves_source_units_padding_flags_and_transform_activation() {
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        for (name, active) in [("bhkRigidBody", false), ("bhkRigidBodyT", true)] {
            let (_, collision) = nif_collision::decode(
                &container(
                    &[(name, body(1, &[])), ("bhkSphereShape", sphere())],
                    stream,
                ),
                "body",
            )
            .unwrap();
            let Data::RigidBody { body } = &collision.blocks[0].data else {
                panic!("body missing");
            };
            assert_eq!(body.transform_active, active);
            assert_eq!(body.translation[0].to_bits(), 0x8000_0000);
            assert_eq!(body.translation[1], 125.);
            assert_eq!(body.rotation, [0., 0., 0., 1.]);
            assert_eq!(body.inertia_padding, [0x7fc0_1234; 3]);
            assert_eq!(body.world.filter.flags_and_parts, 0xe7);
            assert_eq!(body.world.filter.group, 0x1234);
            assert_eq!(body.world.padding, [0x80, 0x81, 0x82]);
            assert_eq!(body.world.property.capacity_and_flags, 0x8000_0009);
            assert_eq!(body.mass, 3.5);
            assert_eq!(body.motion_system, 7);
            assert_eq!(body.flags, 0x8000_0020);
            assert!(!collision.physics_ready);
        }
    }
    for stream in [15, 35] {
        assert!(
            nif_collision::decode(
                &container(&[("bhkSphereShape", sphere())], stream),
                "wrong stream"
            )
            .is_err()
        );
    }
}

#[test]
fn primitives_keep_asymmetric_coordinates_planes_and_opaque_alignment_words() {
    let mut bx = sphere();
    bx.extend([0xa5; 8]);
    floats(&mut bx, &[2., 3., 5.]);
    words(&mut bx, &[0x7fc0_5678]);
    let mut capsule = sphere();
    capsule.extend([0xb6; 8]);
    floats(&mut capsule, &[-5., 7., 11., 0.4, 13., -17., 19., 0.6]);
    let mut convex = sphere();
    words(&mut convex, &[1, 2, 0x8000_0003, 4, 5, 0x8000_0006, 2]);
    floats(&mut convex, &[1., 2., 3., 4., 5., 6., 7., -0.]);
    words(&mut convex, &[1]);
    floats(&mut convex, &[0., 1., 0., -3.]);
    let (_, collision) = nif_collision::decode(
        &container(
            &[
                ("bhkBoxShape", bx),
                ("bhkCapsuleShape", capsule),
                ("bhkConvexVerticesShape", convex),
                ("bhkConvexTransformShape", transform(2)),
            ],
            34,
        ),
        "primitives",
    )
    .unwrap();
    assert!(matches!(&collision.blocks[0].data, Data::Box {
        half_extents, unused_w: 0x7fc0_5678, padding, ..
    } if *half_extents == [2.,3.,5.] && *padding == [0xa5;8]));
    assert!(matches!(&collision.blocks[1].data, Data::Capsule {
        first, second, first_radius, second_radius, ..
    } if *first == [-5.,7.,11.] && *second == [13.,-17.,19.] &&
        *first_radius == 0.4 && *second_radius == 0.6));
    let Data::ConvexVertices {
        vertices, planes, ..
    } = &collision.blocks[2].data
    else {
        panic!("convex missing");
    };
    assert_eq!(vertices[1][3].to_bits(), 0x8000_0000);
    assert_eq!(planes, &[[0., 1., 0., -3.]]);
    assert!(matches!(&collision.blocks[3].data, Data::Transform {
        matrix, convex_only: true, ..
    } if matrix[0] == [1.,2.,3.,4.] && matrix[3] == [13.,14.,15.,16.]));
}

#[test]
fn packed_triangles_keep_winding_degenerates_welding_and_compressed_words() {
    for compressed in [false, true] {
        let (_, collision) = nif_collision::decode(
            &container(&[("hkPackedNiTriStripsData", packed(compressed))], 34),
            "packed",
        )
        .unwrap();
        let Data::PackedData {
            triangles,
            vertices,
            compressed_words,
            subparts,
            ..
        } = &collision.blocks[0].data
        else {
            panic!("packed data missing");
        };
        assert_eq!(triangles[0].indices, [2, 0, 1]);
        assert_eq!(triangles[0].welding, 0xe123);
        assert_eq!(triangles[1].indices, [1, 1, 2]);
        assert_eq!(subparts[0].filter.flags_and_parts, 0xf0);
        assert_eq!(subparts[0].vertices, 3);
        assert_eq!(subparts[0].material, 0x8000_002a);
        if compressed {
            assert!(vertices.is_empty());
            assert_eq!(compressed_words[0], [0x8000, 0x7c01, 0x03ff]);
            assert_eq!(compressed_words[1], [0x1234, 0xffff, 0]);
        } else {
            assert!(compressed_words.is_empty());
            assert_eq!(vertices[0][0].to_bits(), 0x8000_0000);
            assert_eq!(vertices[2], [0., 100., 0.]);
        }
    }
}

#[test]
fn shape_dag_allows_sharing_and_orders_dependencies_before_parents() {
    let (_, collision) = nif_collision::decode(
        &container(
            &[
                ("bhkSphereShape", sphere()),
                ("bhkTransformShape", transform(0)),
                ("bhkTransformShape", transform(0)),
                ("bhkListShape", list(&[1, 2])),
            ],
            34,
        ),
        "shared",
    )
    .unwrap();
    assert_eq!(collision.shape_order, [0, 1, 2, 3]);
    assert!(error(&[("bhkTransformShape", transform(0))]).contains("cycle"));
    assert!(
        error(&[
            ("bhkSphereShape", sphere()),
            ("bhkTransformShape", transform(2)),
            ("bhkTransformShape", transform(1)),
        ])
        .contains("cycle")
    );
}

#[test]
fn deeply_nested_shapes_use_an_iterative_walk() {
    let mut blocks: Vec<_> = (0..10_000)
        .map(|id| ("bhkTransformShape", transform(id + 1)))
        .collect();
    blocks.push(("bhkSphereShape", sphere()));
    let (_, collision) =
        nif_collision::decode(&container(&blocks, 34), "deep shape chain").unwrap();
    assert_eq!(collision.shape_order.len(), 10_001);
    assert_eq!(collision.shape_order[0], 10_000);
    assert_eq!(collision.shape_order.last(), Some(&0));
}

#[test]
fn wrong_reference_roles_fail_and_unsupported_types_stay_explicit() {
    assert!(error(&[("bhkTransformShape", transform(1))]).contains("block index"));
    assert!(
        error(&[
            ("bhkTransformShape", transform(1)),
            ("bhkRigidBody", body(NULL, &[]))
        ])
        .contains("expects shape")
    );
    assert!(
        error(&[
            ("bhkConvexTransformShape", transform(1)),
            ("bhkListShape", list(&[]))
        ])
        .contains("expects convex-shape")
    );
    let (_, collision) = nif_collision::decode(
        &container(
            &[
                ("bhkRigidBody", body(2, &[1])),
                ("bhkHingeConstraint", vec![1, 2, 3]),
                ("bhkFutureShape", vec![4, 5, 6]),
            ],
            34,
        ),
        "unsupported",
    )
    .unwrap();
    assert_eq!(collision.unsupported_blocks["bhkHingeConstraint"], [1]);
    assert_eq!(collision.unsupported_blocks["bhkFutureShape"], [2]);
    assert_eq!(collision.unsupported_links.len(), 2);
    assert!(!collision.unsupported_links[0].type_verified);
    assert!(collision.unsupported_links[1].type_verified);
    assert!(
        error(&[
            ("bhkRigidBody", body(NULL, &[1])),
            ("bhkSphereShape", sphere())
        ])
        .contains("expects constraint")
    );
}

#[test]
fn collision_attachments_preserve_flags_targets_and_blend_gains() {
    for name in [
        "bhkCollisionObject",
        "bhkPCollisionObject",
        "bhkSPCollisionObject",
        "bhkBlendCollisionObject",
    ] {
        let mut bytes = Vec::new();
        words(&mut bytes, &[2]);
        shorts(&mut bytes, &[0x8201]);
        words(&mut bytes, &[1]);
        if name == "bhkBlendCollisionObject" {
            floats(&mut bytes, &[0.125, 0.875]);
        }
        let (_, collision) = nif_collision::decode(
            &container(
                &[
                    (name, bytes),
                    ("bhkRigidBody", body(NULL, &[])),
                    ("NiNode", vec![]),
                ],
                34,
            ),
            "attachment",
        )
        .unwrap();
        assert!(matches!(&collision.blocks[0].data, Data::CollisionObject {
            target: Some(2), body: Some(1), flags: 0x8201, blend_gains
        } if *blend_gains == (name == "bhkBlendCollisionObject").then_some([0.125,0.875])));
    }
}

#[test]
fn mopp_is_opaque_and_packed_shape_keeps_both_source_scale_fields() {
    let mut shape = Vec::new();
    words(&mut shape, &[7, 0x1234_5678]);
    floats(&mut shape, &[0.25]);
    words(&mut shape, &[0x8765_4321]);
    floats(&mut shape, &[1., 2., 3., 4., 0.5, 5., 6., 7., 8.]);
    words(&mut shape, &[0]);
    let mut mopp = Vec::new();
    words(&mut mopp, &[1, 2, 3, 4]);
    floats(&mut mopp, &[0.75]);
    words(&mut mopp, &[4]);
    floats(&mut mopp, &[11., 13., 17., 19.]);
    mopp.extend([0x00, 0xff, 0x80, 0x42]);
    let (_, collision) = nif_collision::decode(
        &container(
            &[
                ("hkPackedNiTriStripsData", packed(false)),
                ("bhkPackedNiTriStripsShape", shape),
                ("bhkMoppBvTreeShape", mopp),
            ],
            34,
        ),
        "mopp",
    )
    .unwrap();
    assert_eq!(collision.shape_order, [0, 1, 2]);
    assert!(matches!(&collision.blocks[1].data, Data::PackedShape {
        scale, scale_copy, radius_copy, ..
    } if *scale == [1.,2.,3.,4.] && *scale_copy == [5.,6.,7.,8.] && *radius_copy == 0.5));
    assert!(matches!(&collision.blocks[2].data, Data::Mopp {
        code, offset, ..
    } if *code == [0,0xff,0x80,0x42] && *offset == [11.,13.,17.,19.]));
}

#[test]
fn truncated_or_surplus_block_payloads_never_borrow_from_neighbors() {
    for (name, payload) in [
        ("bhkRigidBody", body(NULL, &[])),
        ("bhkSphereShape", sphere()),
        ("bhkTransformShape", transform(NULL)),
        ("bhkListShape", list(&[])),
        ("hkPackedNiTriStripsData", packed(false)),
    ] {
        for end in 0..payload.len() {
            assert!(
                nif_collision::decode(
                    &container(
                        &[
                            (name, payload[..end].to_vec()),
                            ("bhkSphereShape", sphere())
                        ],
                        34
                    ),
                    "truncated"
                )
                .is_err(),
                "{name} truncated at {end}"
            );
        }
        let mut surplus = payload;
        surplus.push(0);
        assert!(error(&[(name, surplus)]).contains("unconsumed"));
    }
}

#[test]
fn invalid_indices_booleans_floats_and_oversized_arrays_fail() {
    let mut bad = packed(false);
    bad[4..6].copy_from_slice(&3u16.to_le_bytes());
    assert!(error(&[("hkPackedNiTriStripsData", bad)]).contains("index"));
    let mut bad = packed(false);
    bad[24] = 2;
    assert!(error(&[("hkPackedNiTriStripsData", bad)]).contains("boolean"));
    let mut bad = sphere();
    put(&mut bad, 4, f32::NAN.to_bits());
    assert!(error(&[("bhkSphereShape", bad)]).contains("nonfinite"));
    let mut bad = body(NULL, &[]);
    put(&mut bad, 116, f32::INFINITY.to_bits());
    assert!(error(&[("bhkRigidBody", bad)]).contains("nonfinite"));
    let mut bad = packed(false);
    put(&mut bad, 0, u32::MAX);
    assert!(error(&[("hkPackedNiTriStripsData", bad)]).contains("block"));
}

#[test]
fn decoder_budgets_cover_input_blocks_and_owned_array_storage() {
    let bytes = container(&[("hkPackedNiTriStripsData", packed(false))], 34);
    for limits in [
        Limits {
            input_bytes: bytes.len() - 1,
            ..Limits::default()
        },
        Limits {
            blocks: 0,
            ..Limits::default()
        },
        Limits {
            array_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(nif_collision::decode_with_limits(&bytes, "budget", limits).is_err());
    }
    let (_, collision) = nif_collision::decode(&bytes, "budget").unwrap();
    assert!(collision.retained_bytes >= std::mem::size_of_val(&collision));
}

#[test]
fn deterministic_byte_mutations_return_results_without_panicking() {
    let bytes = container(
        &[
            ("bhkRigidBody", body(1, &[])),
            ("bhkTransformShape", transform(3)),
            ("hkPackedNiTriStripsData", packed(false)),
            ("bhkSphereShape", sphere()),
        ],
        34,
    );
    assert!(nif_collision::decode(&bytes, "before mutation").is_ok());
    for n in 0..512 {
        let mut mutated = bytes.clone();
        let offset = (n * 997 + 17) % mutated.len();
        mutated[offset] ^= (n as u8).wrapping_mul(31) | 1;
        assert!(std::panic::catch_unwind(|| nif_collision::decode(&mutated, "mutation")).is_ok());
    }
}
