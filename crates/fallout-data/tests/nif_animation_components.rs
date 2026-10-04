//! Independently authored exact compact scalar/vector source fields and limits.
use fallout_data::nif_animation::{self, keyframe, spline, spline::components};
use serde_json::json;

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend(words(&[0x1402_0007]));
    bytes.push(1);
    bytes.extend(words(&[11, blocks.len() as u32, stream]));
    bytes.extend([0; 3]);
    bytes.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        bytes.extend(words(&[name.len() as u32]));
        bytes.extend(name.as_bytes());
    }
    for id in 0..blocks.len() {
        bytes.extend((id as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        bytes.extend(words(&[payload.len() as u32]));
    }
    bytes.extend(words(&[0, 0, 0]));
    for (_, payload) in blocks {
        bytes.extend(payload);
    }
    bytes.extend(words(&[1, 0]));
    bytes
}
fn scalar(data: u32, basis: u32) -> Vec<u8> {
    words(&[
        0x7f7f_ffff,
        0xff7f_ffff,
        data,
        basis,
        0x8000_0000,
        u32::MAX,
        1,
        0xff7f_ffff,
    ])
}
fn vector(data: u32, basis: u32) -> Vec<u8> {
    words(&[
        0x8000_0000,
        1,
        data,
        basis,
        0x7f7f_ffff,
        0xff7f_ffff,
        1,
        0x8000_0000,
        0,
        0x8000_0000,
    ])
}
fn controller(target: u32) -> Vec<u8> {
    let mut bytes = words(&[u32::MAX]);
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(words(&[0x3f80_0000, 0, 0, 0x3f80_0000, u32::MAX, target]));
    bytes
}
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiBSplineCompFloatInterpolator", scalar(2, 3)),
        ("NiBSplineCompPoint3Interpolator", vector(2, 3)),
        ("NiBSplineData", words(&[0, 0])),
        ("NiBSplineBasisData", words(&[u32::MAX])),
        ("FutureSplineSource", b"opaque".to_vec()),
        ("NiTransformController", controller(0)),
        ("NiTransformController", controller(1)),
    ]
}
fn fails(bytes: &[u8], limits: components::Limits, reason: &str) {
    let error = components::decode_with_limits(bytes, "components.kf", limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason:?}, got {error}");
    assert!(error.contains("components.kf"));
}
#[test]
fn complete_exact_source_fields_all_admitted_streams() {
    let expected = json!([
        {"kind":"compact_float","start_bits":0x7f7f_ffffu32,"stop_bits":0xff7f_ffffu32,"spline_data":2,"basis_data":3,"value_bits":0x8000_0000u32,"handle":u32::MAX,"float_offset_bits":1,"float_half_range_bits":0xff7f_ffffu32},
        {"kind":"compact_point3","start_bits":0x8000_0000u32,"stop_bits":1,"spline_data":2,"basis_data":3,"value_bits":[0x7f7f_ffffu32,0xff7f_ffffu32,1],"handle":0x8000_0000u32,"position_offset_bits":0,"position_half_range_bits":0x8000_0000u32}
    ]);
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        let (index, source) =
            components::decode(&container(&blocks(), stream), "components.kf").unwrap();
        let actual: Vec<_> = source
            .components
            .blocks
            .iter()
            .map(|b| serde_json::to_value(&b.data).unwrap())
            .collect();
        assert_eq!(json!(actual), expected);
        assert_eq!(source.components.work_units, 20);
        for (id, block) in source.components.blocks.iter().enumerate() {
            assert_eq!(block.block, id as u32);
            assert_eq!(block.offset, index.blocks[id].offset);
            assert_eq!(block.bytes, if id == 0 { 32 } else { 40 });
        }
        assert!(source.components.dependencies.is_empty());
        assert!(source.source.animation.dependencies.is_empty());
        assert!(!source.components.runtime_ready && !source.source.splines.runtime_ready);
    }
}
#[test]
fn earlier_source_entrypoints_remain_exact_and_opaque() {
    let bytes = container(&blocks(), 34);
    let (_, old) = spline::decode(&bytes, "old.kf").unwrap();
    let (_, new) = components::decode(&bytes, "new.kf").unwrap();
    assert_eq!(old.animation.dependencies.len(), 2);
    assert!(new.source.animation.dependencies.is_empty());
    assert_eq!(
        serde_json::to_value(&old.splines).unwrap(),
        serde_json::to_value(&new.source.splines).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&old.keys).unwrap(),
        serde_json::to_value(&new.source.keys).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&old.animation.blocks).unwrap(),
        serde_json::to_value(&new.source.animation.blocks).unwrap()
    );
    assert_eq!(
        old.animation.retained_bytes - new.source.animation.retained_bytes,
        "NiBSplineCompFloatInterpolator".len() + "NiBSplineCompPoint3Interpolator".len()
    );
}
#[test]
fn nulls_and_unknown_target_order_are_preserved() {
    let mut blocks = blocks();
    blocks[0].1 = scalar(4, 4);
    blocks[1].1 = vector(4, 4);
    let (_, source) = components::decode(&container(&blocks, 34), "future.kf").unwrap();
    let deps = serde_json::to_value(source.components.dependencies).unwrap();
    assert_eq!(
        deps,
        json!([
            {"block":0,"role":"spline_data","target":4,"target_type":"FutureSplineSource","status":"unknown_class"},
            {"block":0,"role":"basis_data","target":4,"target_type":"FutureSplineSource","status":"unknown_class"},
            {"block":1,"role":"spline_data","target":4,"target_type":"FutureSplineSource","status":"unknown_class"},
            {"block":1,"role":"basis_data","target":4,"target_type":"FutureSplineSource","status":"unknown_class"}
        ])
    );
    blocks[0].1 = scalar(u32::MAX, u32::MAX);
    blocks[1].1 = vector(u32::MAX, u32::MAX);
    assert!(
        components::decode(&container(&blocks, 34), "null.kf")
            .unwrap()
            .1
            .components
            .dependencies
            .is_empty()
    );
}
#[test]
fn known_wrong_links_and_out_of_range_refs_fail_contextually() {
    for (data, basis, reason) in [
        (3, 3, "SplineData link has wrong target kind"),
        (2, 2, "BasisData link has wrong target kind"),
        (7, 3, "block index out of range"),
    ] {
        for id in [0, 1] {
            let mut blocks = blocks();
            blocks[id].1 = if id == 0 {
                scalar(data, basis)
            } else {
                vector(data, basis)
            };
            fails(&container(&blocks, 34), Default::default(), reason);
        }
    }
}
#[test]
fn work_and_storage_preflight_exact_boundaries() {
    let bytes = container(&blocks(), 34);
    let (_, before) = spline::decode(&bytes, "before.kf").unwrap();
    let (_, source) = components::decode(&bytes, "source.kf").unwrap();
    let cap = before.animation.retained_bytes
        + before.keys.retained_bytes
        + before.splines.retained_bytes
        + source.components.retained_bytes;
    let exact = components::Limits {
        splines: spline::Limits {
            keyframes: keyframe::Limits {
                max_combined_retained_bytes: cap,
                ..Default::default()
            },
            ..Default::default()
        },
        array_bytes: source.components.retained_bytes,
        component_work: 20,
    };
    components::decode_with_limits(&bytes, "exact.kf", exact).unwrap();
    fails(
        &bytes,
        components::Limits {
            component_work: 19,
            ..exact
        },
        "spline-component work budget exceeded",
    );
    fails(
        &bytes,
        components::Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        "animation retained storage budget exceeded",
    );
    fails(
        &bytes,
        components::Limits {
            splines: spline::Limits {
                keyframes: keyframe::Limits {
                    max_combined_retained_bytes: cap - 1,
                    ..exact.splines.keyframes
                },
                ..exact.splines
            },
            ..exact
        },
        "animation retained storage budget exceeded",
    );
    fails(
        &bytes,
        components::Limits {
            array_bytes: 0,
            component_work: 1,
            ..exact
        },
        "spline-component work budget exceeded",
    );
}
#[test]
fn malformed_source_remains_opaque_in_all_earlier_entrypoints() {
    for (kind, original) in [
        ("NiBSplineCompFloatInterpolator", scalar(u32::MAX, u32::MAX)),
        (
            "NiBSplineCompPoint3Interpolator",
            vector(u32::MAX, u32::MAX),
        ),
    ] {
        let mut cases = vec![
            (original[..original.len() - 1].to_vec(), "field exceeds"),
            (
                original.iter().copied().chain([0]).collect(),
                "unconsumed bytes",
            ),
        ];
        let handle = if original.len() == 32 { 20 } else { 28 };
        for offset in (0..original.len())
            .step_by(4)
            .filter(|o| ![8, 12, handle].contains(o))
        {
            let mut raw = original.clone();
            raw[offset..offset + 4].copy_from_slice(&0x7fc0_0001u32.to_le_bytes());
            cases.push((raw, "nonfinite"));
        }
        for (raw, reason) in cases {
            let bytes = container(&[(kind, raw)], 34);
            fails(&bytes, Default::default(), reason);
            nif_animation::decode(&bytes, "opaque.kf").unwrap();
            keyframe::decode(&bytes, "opaque.kf").unwrap();
            spline::decode(&bytes, "opaque.kf").unwrap();
        }
    }
}
#[test]
fn raw_handle_bits_do_not_imply_channel_validity() {
    for handle in [
        0,
        65535,
        65536,
        0x8000_0000,
        0xffff_0000,
        u32::MAX,
        0x7fc0_0001,
    ] {
        let mut raw = scalar(u32::MAX, u32::MAX);
        raw[20..24].copy_from_slice(&handle.to_le_bytes());
        let (_, source) = components::decode(
            &container(&[("NiBSplineCompFloatInterpolator", raw)], 34),
            "handle.kf",
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&source.components.blocks[0].data).unwrap()["handle"],
            handle
        );
    }
}
#[test]
fn inherited_tuple_and_block_limits_still_apply() {
    fails(
        &container(&blocks(), 35),
        Default::default(),
        "NIF tuple version=",
    );
    fails(
        &container(&blocks(), 34),
        components::Limits {
            splines: spline::Limits {
                keyframes: keyframe::Limits {
                    animation: nif_animation::Limits {
                        blocks: 6,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        "input/count budget",
    );
}
