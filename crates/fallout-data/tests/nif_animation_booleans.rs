//! Authored raw Boolean bytes, unsupported links and admission boundaries.
use fallout_data::nif_animation::{self, boolean, keyframe, spline};
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
    for (kind, _) in blocks {
        bytes.extend(words(&[kind.len() as u32]));
        bytes.extend(kind.as_bytes());
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
fn payload(value: u8, link: u32) -> Vec<u8> {
    let mut bytes = vec![value];
    bytes.extend(words(&[link]));
    bytes
}
fn controller(interpolator: u32) -> Vec<u8> {
    let mut bytes = words(&[u32::MAX]);
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(words(&[
        0x3f80_0000,
        0,
        0,
        0x3f80_0000,
        u32::MAX,
        interpolator,
    ]));
    bytes
}
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiBoolInterpolator", payload(2, 2)),
        ("NiBoolTimelineInterpolator", payload(255, 3)),
        ("NiBoolData", b"unparsed Boolean keys".to_vec()),
        ("FutureBoolSource", b"opaque".to_vec()),
        ("NiFloatData", b"unparsed wrong kind".to_vec()),
        ("NiTransformController", controller(0)),
        ("NiTransformController", controller(1)),
    ]
}
fn fails(bytes: &[u8], limits: boolean::Limits, reason: &str) {
    let error = boolean::decode_with_limits(bytes, "raw-bool.kf", limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason:?}, got {error}");
    assert!(error.contains("raw-bool.kf"));
}
#[test]
fn every_raw_byte_stays_exact_for_both_classes_and_all_admitted_streams() {
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        for value in 0..=255 {
            let bytes = container(
                &[
                    ("NiBoolInterpolator", payload(value, u32::MAX)),
                    ("NiBoolTimelineInterpolator", payload(value, u32::MAX)),
                ],
                stream,
            );
            let (index, source) = boolean::decode(&bytes, "raw.kf").unwrap();
            assert_eq!(source.booleans.work_units, 10);
            for (id, block) in source.booleans.blocks.iter().enumerate() {
                assert_eq!(block.block, id as u32);
                assert_eq!(block.offset, index.blocks[id].offset);
                assert_eq!(block.bytes, 5);
                assert_eq!(block.sha256.len(), 64);
                assert_eq!(
                    serde_json::to_value(&block.data).unwrap(),
                    json!({"raw_value":value,"data":null})
                );
            }
            assert!(!source.booleans.runtime_ready);
            assert!(source.booleans.dependencies.is_empty());
        }
    }
}
#[test]
fn ordered_known_undecoded_and_unknown_dependencies_remain_explicit() {
    let (_, source) = boolean::decode(&container(&blocks(), 34), "raw.kf").unwrap();
    assert_eq!(
        serde_json::to_value(source.booleans.dependencies).unwrap(),
        json!([
            {"block":0,"target":2,"target_type":"NiBoolData","status":"undecoded_payload"},
            {"block":1,"target":3,"target_type":"FutureBoolSource","status":"unknown_class"}
        ])
    );
    assert!(source.source.source.animation.dependencies.is_empty());
}
#[test]
fn prior_source_catalogues_and_storage_receipts_stay_exact() {
    let bytes = container(&blocks(), 34);
    let (_, before) = spline::components::decode(&bytes, "before.kf").unwrap();
    let (_, source) = boolean::decode(&bytes, "after.kf").unwrap();
    assert_eq!(before.source.animation.dependencies.len(), 2);
    assert_eq!(
        serde_json::to_value(&before.components).unwrap(),
        serde_json::to_value(&source.source.components).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&before.source.keys).unwrap(),
        serde_json::to_value(&source.source.source.keys).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&before.source.splines).unwrap(),
        serde_json::to_value(&source.source.source.splines).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&before.source.animation.blocks).unwrap(),
        serde_json::to_value(&source.source.source.animation.blocks).unwrap()
    );
    assert_eq!(
        before.source.animation.retained_bytes - source.source.source.animation.retained_bytes,
        "NiBoolInterpolator".len() + "NiBoolTimelineInterpolator".len()
    );
}
#[test]
fn exact_combined_storage_and_work_boundaries_are_admitted_before_construction() {
    let bytes = container(&blocks(), 34);
    let (_, before) = spline::components::decode(&bytes, "before.kf").unwrap();
    let (_, source) = boolean::decode(&bytes, "source.kf").unwrap();
    let cap = before.source.animation.retained_bytes
        + before.source.keys.retained_bytes
        + before.source.splines.retained_bytes
        + before.components.retained_bytes
        + source.booleans.retained_bytes;
    let exact = boolean::Limits {
        components: spline::components::Limits {
            splines: spline::Limits {
                keyframes: keyframe::Limits {
                    max_combined_retained_bytes: cap,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        },
        array_bytes: source.booleans.retained_bytes,
        boolean_work: 10,
    };
    boolean::decode_with_limits(&bytes, "exact.kf", exact).unwrap();
    fails(
        &bytes,
        boolean::Limits {
            boolean_work: 9,
            array_bytes: 0,
            ..exact
        },
        "Boolean source work budget exceeded",
    );
    fails(
        &bytes,
        boolean::Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        "animation retained storage budget exceeded",
    );
    let mut under = exact;
    under
        .components
        .splines
        .keyframes
        .max_combined_retained_bytes -= 1;
    fails(&bytes, under, "animation retained storage budget exceeded");
}
#[test]
fn known_wrong_target_types_and_out_of_range_links_refuse() {
    for id in [0, 1] {
        for (target, reason) in [
            (4, "Boolean data link has wrong target kind"),
            (0, "Boolean data link has wrong target kind"),
            (7, "block index out of range"),
        ] {
            let mut authored = blocks();
            authored[id].1 = payload(2, target);
            fails(&container(&authored, 34), Default::default(), reason);
        }
    }
}
#[test]
fn malformed_selected_payloads_remain_opaque_in_earlier_source_entrypoints() {
    for kind in ["NiBoolInterpolator", "NiBoolTimelineInterpolator"] {
        for (raw, reason) in [
            (vec![2], "field exceeds"),
            (vec![2, 255, 255, 255, 255, 0], "unconsumed bytes"),
        ] {
            let bytes = container(&[(kind, raw)], 34);
            fails(&bytes, Default::default(), reason);
            nif_animation::decode(&bytes, "opaque.kf").unwrap();
            keyframe::decode(&bytes, "opaque.kf").unwrap();
            spline::decode(&bytes, "opaque.kf").unwrap();
            spline::components::decode(&bytes, "opaque.kf").unwrap();
        }
    }
}
#[test]
fn absent_selected_blocks_need_no_boolean_work_or_storage() {
    let bytes = container(&[("FutureBoolSource", vec![255])], 34);
    let (_, source) = boolean::decode_with_limits(
        &bytes,
        "absent.kf",
        boolean::Limits {
            boolean_work: 0,
            array_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(source.booleans.blocks.is_empty() && source.booleans.dependencies.is_empty());
    assert_eq!(source.booleans.retained_bytes, 0);
    assert_eq!(source.booleans.work_units, 0);
}
#[test]
fn inherited_version_and_block_limits_still_refuse() {
    fails(
        &container(&blocks(), 35),
        Default::default(),
        "NIF tuple version=",
    );
    let mut limits = boolean::Limits::default();
    limits.components.splines.keyframes.animation.blocks = 6;
    fails(&container(&blocks(), 34), limits, "input/count budget");
}
