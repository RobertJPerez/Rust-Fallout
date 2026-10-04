//! Independent classic NV source words, physical topology and literal CPU pose.
use fallout_data::{
    nif_scene,
    nif_skin::{influences, pose, streams},
};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
fn w(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_le_bytes()).collect()
}
fn h(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_le_bytes()).collect()
}
fn f(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_le_bytes()).collect()
}
fn av(translation: [f32; 3]) -> Vec<u8> {
    let mut out = w(&[NULL, 0, NULL, 0]);
    out.extend(f(&translation));
    out.extend(f(&[1., 0., 0., 0., 1., 0., 0., 0., 1., 1.]));
    out.extend(w(&[0, NULL]));
    out
}
fn node(translation: [f32; 3], children: &[u32]) -> Vec<u8> {
    let mut out = av(translation);
    out.extend(w(&[children.len() as u32]));
    out.extend(w(children));
    out.extend(w(&[0]));
    out
}
fn transform(translation: [f32; 3]) -> Vec<u8> {
    let mut out = f(&[1., 0., 0., 0., 1., 0., 0., 0., 1.]);
    out.extend(f(&translation));
    out.extend(f(&[1.]));
    out
}
fn skin(translation: [f32; 3]) -> Vec<u8> {
    let mut out = transform(translation);
    out.extend(w(&[2]));
    out.push(1);
    for weights in [
        vec![
            (0, 0.25),
            (0, 0.125),
            (0, -0.),
            (0, 0.),
            (0, 0.125),
            (1, 1.),
            (3, 1.),
        ],
        vec![(0, 0.25), (0, 0.25), (2, 1.)],
    ] {
        out.extend(transform([0.; 3]));
        out.extend(f(&[0., 0., 0., 10.]));
        out.extend(h(&[weights.len() as u16]));
        for (vertex, weight) in weights {
            out.extend(h(&[vertex]));
            out.extend(f(&[weight]));
        }
    }
    out
}
fn positions() -> Vec<[f32; 3]> {
    vec![[1., -0., 0.], [-1., 2., 0.], [3., -2., 1.], [0., 1., 2.]]
}
fn normals() -> Vec<[f32; 3]> {
    vec![[0., -0., 1.], [1., 0., 0.], [0., 1., -0.], [1., 2., 3.]]
}
fn mesh(present: bool, strips: bool, extra: bool, second_uv: bool) -> Vec<u8> {
    let mut out = w(&[17]);
    out.extend(h(&[4]));
    out.extend([7, 9, 1]);
    for p in positions() {
        out.extend(f(&p));
    }
    out.extend(h(&[if present { 0x1003 } else { 2 }]));
    out.push(u8::from(present));
    if present {
        for p in normals() {
            out.extend(f(&p));
        }
        for p in [[1., -0., 0.]; 4] {
            out.extend(f(&p));
        }
        for p in [[-0., 1., 0.]; 4] {
            out.extend(f(&p));
        }
    }
    out.extend(f(&[-0., 2., 3., 10.]));
    out.push(u8::from(present));
    if present {
        for _ in 0..4 {
            out.extend(f(&[-0., 0.25, 0.5, 1.]));
        }
        let uv = f(&[-0., 0., 0.25, -0., 1., 2., -1., 0.5]);
        out.extend(&uv);
        if second_uv {
            out.extend(&uv);
        }
    }
    out.extend(h(&[0x8000]));
    out.extend(w(&[if extra { 10 } else { NULL }]));
    if strips {
        out.extend(h(&[7, 2, 8, 3]));
        out.push(1);
        out.extend(h(&[0, 1, 2, 2, 1, 1, 2, 3, 3, 2, 0]));
    } else {
        out.extend(h(&[2]));
        out.extend(w(&[6]));
        out.push(1);
        out.extend(h(&[0, 1, 2, 3, 2, 0]));
        out.extend(h(&[1, 3, 2, 2, 0]));
    }
    out
}
fn fixture(present: bool, strips: bool, extra: bool, second_uv: bool) -> Vec<u8> {
    let geometry = |instance| {
        let mut out = av([777., 888., 999.]);
        out.extend(w(&[4, instance, 0, NULL]));
        out.push(0);
        out
    };
    let mut blocks = vec![
        ("NiNode", node([9., 8., 7.], &[1, 2, 3, 7])),
        ("NiNode", node([2., 0., 0.], &[])),
        ("NiNode", node([0., 3., 0.], &[])),
        (
            if strips { "NiTriStrips" } else { "NiTriShape" },
            geometry(5),
        ),
        (
            if strips {
                "NiTriStripsData"
            } else {
                "NiTriShapeData"
            },
            mesh(present, strips, extra, second_uv),
        ),
        ("NiSkinInstance", w(&[6, NULL, 0, 2, 1, 2])),
        ("NiSkinData", skin([0.; 3])),
        (
            if strips { "NiTriStrips" } else { "NiTriShape" },
            geometry(8),
        ),
        ("NiSkinInstance", w(&[9, NULL, 0, 2, 2, 1])),
        ("NiSkinData", skin([1., 0., 0.])),
    ];
    if extra {
        blocks.push(("NiAdditionalGeometryData", w(&[0x12345678, 0xabcdef01])));
    }
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    out.extend(w(&[0x14020007]));
    out.push(1);
    out.extend(w(&[11, blocks.len() as u32, 34]));
    out.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in &blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    out.extend(h(&[types.len() as u16]));
    for name in &types {
        out.extend(w(&[name.len() as u32]));
        out.extend(name.as_bytes());
    }
    for (name, _) in &blocks {
        out.extend(h(&[types.iter().position(|n| n == name).unwrap() as u16]));
    }
    for (_, payload) in &blocks {
        out.extend(w(&[payload.len() as u32]));
    }
    out.extend(w(&[0, 0, 0]));
    for (_, payload) in blocks {
        out.extend(payload);
    }
    out.extend(w(&[1, 0]));
    out
}
fn request(bytes: &[u8], geometry: u32) -> streams::Request {
    streams::Request {
        expected_source_sha256: Sha256::digest(bytes).into(),
        geometry,
    }
}
fn prepare(bytes: &[u8], geometry: u32) -> streams::PreparedGeometryStreams {
    streams::prepare(
        bytes,
        "independent whole geometry",
        request(bytes, geometry),
        Default::default(),
    )
    .unwrap()
}
fn eval_request(bytes: &[u8], geometry: u32, instance: u32) -> streams::EvaluationRequest {
    streams::EvaluationRequest {
        expected_source_sha256: Sha256::digest(bytes).into(),
        geometry,
        instance,
        weights: pose::WeightPolicy::PreserveRawNonnegative,
    }
}

#[test]
fn whole_geometry_preserves_all_raw_words_presence_flags_csr_and_source_spans() {
    for present in [false, true] {
        let bytes = fixture(present, false, false, false);
        let packet = prepare(&bytes, 3);
        let a = packet.attributes();
        assert_eq!(
            a.positions_bits,
            positions()
                .iter()
                .map(|p| p.map(f32::to_bits))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            a.normals_bits,
            if present {
                normals()
                    .iter()
                    .map(|p| p.map(f32::to_bits))
                    .collect::<Vec<_>>()
            } else {
                vec![]
            }
        );
        assert_eq!(
            a.tangents_bits,
            if present {
                vec![[0x3f800000, 0x80000000, 0]; 4]
            } else {
                vec![]
            }
        );
        assert_eq!(
            a.bitangents_bits,
            if present {
                vec![[0x80000000, 0x3f800000, 0]; 4]
            } else {
                vec![]
            }
        );
        assert_eq!(
            a.colors_bits,
            if present {
                vec![[0x80000000, 0x3e800000, 0x3f000000, 0x3f800000]; 4]
            } else {
                vec![]
            }
        );
        assert_eq!(
            a.uv_sets_bits,
            if present {
                vec![vec![
                    [0x80000000, 0],
                    [0x3e800000, 0x80000000],
                    [0x3f800000, 0x40000000],
                    [0xbf800000, 0x3f000000],
                ]]
            } else {
                vec![]
            }
        );
        assert_eq!(packet.presence().uv_sets, usize::from(present));
        assert_eq!(packet.presence().normals, present);
        assert_eq!(packet.presence().colors, present);
        assert_eq!(packet.presence().tangents, present);
        let m = packet.metadata();
        assert_eq!(
            (
                m.group_id,
                m.keep_flags,
                m.compress_flags,
                m.consistency_flags
            ),
            (17, 7, 9, 0x8000)
        );
        assert_eq!(m.data_flags, if present { 0x1003 } else { 2 });
        assert_eq!(
            m.bound_bits,
            [0x80000000, 0x40000000, 0x40400000, 0x41200000]
        );
        assert!(m.source_triangle_count_matches);
        assert_eq!(m.strip_degenerate_triangles, 0);
        assert_eq!(packet.influences().vertex_offsets(), [0, 7, 8, 9, 10]);
        let entries = packet.influences().entries();
        assert_eq!(entries.len(), 10);
        assert_eq!(
            entries
                .iter()
                .map(|e| (e.bone_ordinal, e.source_weight_ordinal, e.weight_bits))
                .collect::<Vec<_>>(),
            [
                (0, 0, 0x3e800000),
                (0, 1, 0x3e000000),
                (0, 2, 0x80000000),
                (0, 3, 0),
                (0, 4, 0x3e000000),
                (1, 0, 0x3e800000),
                (1, 1, 0x3e800000),
                (0, 5, 0x3f800000),
                (1, 2, 0x3f800000),
                (0, 6, 0x3f800000)
            ]
        );
        assert_eq!(
            packet
                .vertices()
                .iter()
                .map(|v| (v.source_vertex, v.influence_start, v.influence_count))
                .collect::<Vec<_>>(),
            [(0, 0, 7), (1, 7, 1), (2, 8, 1), (3, 9, 1)]
        );
        let index = fallout_data::nif::inspect(&bytes, "span input").unwrap();
        let identity = packet.identity();
        for span in [
            &identity.geometry,
            &identity.geometry_data,
            &identity.instance,
            &identity.skin_data,
        ] {
            let raw = &index.blocks[span.block as usize];
            assert_eq!((span.offset, span.bytes), (raw.offset, raw.bytes));
            assert_eq!(
                span.sha256,
                format!(
                    "{:x}",
                    Sha256::digest(&bytes[raw.offset..raw.offset + raw.bytes])
                )
            );
        }
        assert_eq!(
            (
                identity.geometry.block,
                identity.geometry_data.block,
                identity.instance.block,
                identity.skin_data.block
            ),
            (3, 4, 5, 6)
        );
        match packet.topology() {
            streams::Topology::Triangles {
                declared_points,
                present,
                indices,
                match_groups,
            } => {
                assert_eq!(*declared_points, 6);
                assert!(*present);
                assert_eq!(indices, &[[0, 1, 2], [3, 2, 0]]);
                assert_eq!(match_groups, &[vec![2, 2, 0]]);
            }
            _ => panic!("wrong topology"),
        }
        assert_eq!(
            packet
                .triangles()
                .iter()
                .map(|t| (t.vertices, t.source_primitive_ordinal, t.strip_ordinal))
                .collect::<Vec<_>>(),
            [([0, 1, 2], 0, None), ([3, 2, 0], 1, None)]
        );
    }
}

#[test]
fn whole_geometry_strip_connectors_preserve_physical_slots_and_existing_winding() {
    let bytes = fixture(true, true, false, false);
    let packet = prepare(&bytes, 3);
    assert_eq!(packet.metadata().declared_triangles, 7);
    assert_eq!(packet.metadata().strip_degenerate_triangles, 4);
    assert!(packet.metadata().source_triangle_count_matches);
    match packet.topology() {
        streams::Topology::Strips {
            lengths,
            present,
            indices,
        } => {
            assert_eq!(lengths, &[8, 3]);
            assert!(*present);
            assert_eq!(indices, &[vec![0, 1, 2, 2, 1, 1, 2, 3], vec![3, 2, 0]]);
        }
        _ => panic!("wrong topology"),
    }
    assert_eq!(
        packet
            .triangles()
            .iter()
            .map(|t| (
                t.vertices,
                t.source_primitive_ordinal,
                t.strip_ordinal,
                t.strip_step_ordinal
            ))
            .collect::<Vec<_>>(),
        [
            ([0, 1, 2], 0, Some(0), Some(0)),
            ([1, 3, 2], 5, Some(0), Some(5)),
            ([3, 2, 0], 6, Some(1), Some(0))
        ]
    );
    let (_, scene) = nif_scene::decode(&bytes, "existing scene").unwrap();
    assert_eq!(
        packet
            .triangles()
            .iter()
            .map(|t| t.vertices)
            .collect::<Vec<_>>(),
        scene.meshes[0].triangles
    );
}

#[test]
fn whole_geometry_stored_pose_is_literal_and_reuses_private_decoded_source_and_csr() {
    let mut bytes = fixture(true, true, false, false);
    let req = eval_request(&bytes, 3, 5);
    let packet = prepare(&bytes, 3);
    let old = influences::prepare(&bytes, "old CSR", 3, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(packet.influences()).unwrap(),
        serde_json::to_value(&old).unwrap()
    );
    let baseline = old
        .evaluate(
            &bytes,
            "old pose",
            pose::Request {
                geometry: 3,
                weights: req.weights,
            },
            Default::default(),
        )
        .unwrap();
    let result = packet.evaluate_stored(req, Default::default()).unwrap();
    assert_eq!(
        result.skin.positions,
        [[2., 1.5, 0.], [1., 2., 0.], [3., 1., 1.], [2., 1., 2.]]
    );
    assert_eq!(
        result.skin.palette[0].matrix,
        [[1., 0., 0., 2.], [0., 1., 0., 0.], [0., 0., 1., 0.]]
    );
    assert_eq!(
        result.skin.palette[1].matrix,
        [[1., 0., 0., 0.], [0., 1., 0., 3.], [0., 0., 1., 0.]]
    );
    assert_eq!(
        result.skin.skin_to_source_world,
        [[1., 0., 0., 9.], [0., 1., 0., 8.], [0., 0., 1., 7.]]
    );
    assert_eq!(
        serde_json::to_value(&result.skin).unwrap(),
        serde_json::to_value(baseline).unwrap()
    );
    assert_eq!(
        (
            result.usage.binding_decodes,
            result.usage.scene_decodes,
            result.usage.full_source_sha256_traversals,
            result.usage.csr_builds
        ),
        (0, 0, 0, 0)
    );
    let before = serde_json::to_value(&packet).unwrap();
    bytes.fill(0);
    drop(bytes);
    assert_eq!(
        serde_json::to_value(packet.evaluate_stored(req, Default::default()).unwrap()).unwrap(),
        serde_json::to_value(result).unwrap()
    );
    assert_eq!(serde_json::to_value(&packet).unwrap(), before);
    assert!(before.get("authority").is_none());
    assert_eq!(before["faithful_renderer_ready"], false);
}

#[test]
fn whole_geometry_shared_data_keeps_exact_owner_instance_and_refuses_foreign_identity() {
    let bytes = fixture(false, false, false, false);
    let a = prepare(&bytes, 3);
    let b = prepare(&bytes, 7);
    assert_eq!(
        a.identity().geometry_data.block,
        b.identity().geometry_data.block
    );
    assert_eq!(
        (a.identity().instance.block, b.identity().instance.block),
        (5, 8)
    );
    let req = eval_request(&bytes, 3, 5);
    let before = serde_json::to_value(a.evaluate_stored(req, Default::default()).unwrap()).unwrap();
    for bad in [
        streams::EvaluationRequest { instance: 8, ..req },
        streams::EvaluationRequest { geometry: 7, ..req },
        streams::EvaluationRequest {
            expected_source_sha256: [0; 32],
            ..req
        },
        streams::EvaluationRequest {
            weights: pose::WeightPolicy::RequireUnitSum {
                absolute_tolerance: f64::NAN,
            },
            ..req
        },
    ] {
        assert!(a.evaluate_stored(bad, Default::default()).is_err());
    }
    assert!(b.evaluate_stored(req, Default::default()).is_err());
    let other = b
        .evaluate_stored(eval_request(&bytes, 7, 8), Default::default())
        .unwrap();
    assert_ne!(
        serde_json::to_value(other.skin.positions).unwrap(),
        before["skin"]["positions"]
    );
    let mut changed = bytes.clone();
    let index = fallout_data::nif::inspect(&bytes, "offset").unwrap();
    changed[index.blocks[3].offset + 16] ^= 1;
    assert!(
        streams::prepare(
            &changed,
            "foreign same IDs",
            request(&bytes, 3),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(a.evaluate_stored(req, Default::default()).unwrap()).unwrap(),
        before
    );
    assert!(a.attributes().normals_bits.is_empty());
    assert!(
        a.evaluate_stored(req, Default::default())
            .unwrap()
            .skin
            .normals
            .is_empty()
    );
}

#[test]
fn whole_geometry_unsupported_additional_data_and_malformed_second_uv_remain_explicit() {
    let bytes = fixture(true, false, true, false);
    let packet = prepare(&bytes, 3);
    let additional = packet.additional_geometry_data().unwrap();
    assert_eq!(additional.link, 10);
    assert_eq!(additional.span.block, 10);
    assert_eq!(additional.block_type, "NiAdditionalGeometryData");
    assert!(!additional.payload_decoded);
    assert_eq!(
        additional.span.sha256,
        format!("{:x}", Sha256::digest(w(&[0x12345678, 0xabcdef01])))
    );
    assert_eq!(
        serde_json::to_value(&packet).unwrap()["faithful_renderer_ready"],
        false
    );
    assert!(
        !packet
            .evaluate_stored(eval_request(&bytes, 3, 5), Default::default())
            .unwrap()
            .skin
            .retail_behavior_verified
    );
    let bytes = fixture(true, false, false, true);
    assert!(
        streams::prepare(
            &bytes,
            "malformed second UV",
            request(&bytes, 3),
            Default::default()
        )
        .is_err()
    );
    let bytes = fixture(true, false, false, false);
    assert_eq!(prepare(&bytes, 3).presence().uv_sets, 1);
}

#[test]
fn whole_geometry_exact_admission_metadata_csr_output_work_and_combined_limits() {
    let bytes = fixture(true, true, false, false);
    let packet = prepare(&bytes, 3);
    let usage = packet.usage();
    let source = streams::Limits::default();
    let own = usage.charged_bytes
        - usage.source_binding_retained_bytes
        - usage.source_metadata_retained_bytes;
    let exact = streams::Limits {
        array_bytes: own,
        work_units: usage.work_units,
        source_metadata_array_bytes: usage.source_metadata_retained_bytes,
        source_metadata_work_units: usage.source_metadata_work_units,
        decoder_array_admission_bytes: usage.decoder_array_admission_bytes,
        decoder_check_admission_units: usage.decoder_check_admission_units,
        max_combined_retained_bytes: usage.decoder_array_admission_bytes
            + usage.source_metadata_retained_bytes
            + own,
        influence_entries: 10,
        ..source
    };
    assert!(streams::prepare(&bytes, "exact", request(&bytes, 3), exact).is_ok());
    for limits in [
        streams::Limits {
            array_bytes: own - 1,
            ..exact
        },
        streams::Limits {
            work_units: exact.work_units - 1,
            ..exact
        },
        streams::Limits {
            source_metadata_array_bytes: exact.source_metadata_array_bytes - 1,
            ..exact
        },
        streams::Limits {
            source_metadata_work_units: exact.source_metadata_work_units - 1,
            ..exact
        },
        streams::Limits {
            decoder_array_admission_bytes: exact.decoder_array_admission_bytes - 1,
            ..exact
        },
        streams::Limits {
            decoder_check_admission_units: exact.decoder_check_admission_units - 1,
            ..exact
        },
        streams::Limits {
            max_combined_retained_bytes: exact.max_combined_retained_bytes - 1,
            ..exact
        },
        streams::Limits {
            influence_entries: 9,
            ..exact
        },
    ] {
        assert!(streams::prepare(&bytes, "one below", request(&bytes, 3), limits).is_err());
    }
    let req = eval_request(&bytes, 3, 5);
    let evaluated = packet.evaluate_stored(req, Default::default()).unwrap();
    let exact = streams::EvaluationLimits {
        array_bytes: evaluated.usage.charged_output_bytes,
        work_units: evaluated.usage.work_units,
        max_combined_retained_bytes: evaluated.usage.combined_retained_bytes,
        ..Default::default()
    };
    assert!(packet.evaluate_stored(req, exact).is_ok());
    for limits in [
        streams::EvaluationLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        streams::EvaluationLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        streams::EvaluationLimits {
            max_combined_retained_bytes: exact.max_combined_retained_bytes - 1,
            ..exact
        },
        streams::EvaluationLimits {
            ancestry_depth: 0,
            ..exact
        },
    ] {
        assert!(packet.evaluate_stored(req, limits).is_err());
    }
    assert_eq!(
        serde_json::to_value(packet.evaluate_stored(req, Default::default()).unwrap()).unwrap(),
        serde_json::to_value(evaluated).unwrap()
    );
}
