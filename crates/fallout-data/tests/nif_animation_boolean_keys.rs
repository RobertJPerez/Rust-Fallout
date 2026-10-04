//! Constant Boolean raw key source order, source links and allocation guards.
use fallout_data::nif_animation::boolean::{self, keyframes};
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
fn group(keys: &[(u32, u8)]) -> Vec<u8> {
    let mut bytes = words(&[keys.len() as u32]);
    if !keys.is_empty() {
        bytes.extend(words(&[5]));
    }
    for (time, value) in keys {
        bytes.extend(words(&[*time]));
        bytes.push(*value);
    }
    bytes
}
fn interpolator(value: u8, target: u32) -> Vec<u8> {
    let mut bytes = vec![value];
    bytes.extend(words(&[target]));
    bytes
}
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiBoolInterpolator", interpolator(2, 2)),
        ("NiBoolTimelineInterpolator", interpolator(255, 3)),
        (
            "NiBoolData",
            group(&[(0x4000_0000, 255), (0x8000_0000, 2), (0x4000_0000, 1)]),
        ),
        ("NiBoolData", group(&[])),
        ("FutureBoolData", vec![255]),
    ]
}
fn fails(bytes: &[u8], limits: keyframes::Limits, reason: &str) {
    let error = keyframes::decode_with_limits(bytes, "bool-keys.kf", limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason:?}, got {error}");
    assert!(error.contains("bool-keys.kf"));
}
#[test]
fn exact_zero_and_tag5_keys_preserve_order_bits_and_raw_values_all_streams() {
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        let (index, source) = keyframes::decode(&container(&blocks(), stream), "keys.kf").unwrap();
        assert_eq!(
            serde_json::to_value(&source.keys.blocks[0].data).unwrap(),
            json!({
                "declared_keys":3,"key_type":5,"keys":[
                    {"time_bits":0x4000_0000u32,"raw_value":255},
                    {"time_bits":0x8000_0000u32,"raw_value":2},
                    {"time_bits":0x4000_0000u32,"raw_value":1}
                ]
            })
        );
        assert_eq!(
            serde_json::to_value(&source.keys.blocks[1].data).unwrap(),
            json!({"declared_keys":0,"key_type":null,"keys":[]})
        );
        assert_eq!(source.keys.work_units, 11);
        assert!(source.source.booleans.dependencies.is_empty());
        assert!(!source.keys.runtime_ready);
        for (ordinal, block) in source.keys.blocks.iter().enumerate() {
            let id = ordinal + 2;
            assert_eq!(block.block, id as u32);
            assert_eq!(block.offset, index.blocks[id].offset);
            assert_eq!(block.bytes, index.blocks[id].bytes);
        }
    }
}
#[test]
fn all_256_raw_byte_values_and_finite_time_extremes_remain_source() {
    let keys: Vec<_> = (0..=255)
        .map(|value| {
            (
                if value % 2 == 0 {
                    0xff7f_ffff
                } else {
                    0x7f7f_ffff
                },
                value,
            )
        })
        .collect();
    let (_, source) =
        keyframes::decode(&container(&[("NiBoolData", group(&keys))], 34), "values.kf").unwrap();
    for (key, (time, value)) in source.keys.blocks[0].data.keys.iter().zip(keys) {
        assert_eq!(key.time_bits, time);
        assert_eq!(key.raw_value, value);
    }
}
#[test]
fn previous_catalogues_are_unchanged_except_complete_data_dependency_retirement() {
    let bytes = container(&blocks(), 34);
    let (_, prior) = boolean::decode(&bytes, "prior.kf").unwrap();
    let (_, source) = keyframes::decode(&bytes, "keys.kf").unwrap();
    assert_eq!(prior.booleans.dependencies.len(), 2);
    assert_eq!(
        serde_json::to_value(&prior.source).unwrap(),
        serde_json::to_value(&source.source.source).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&prior.booleans.blocks).unwrap(),
        serde_json::to_value(&source.source.booleans.blocks).unwrap()
    );
    assert_eq!(
        prior.booleans.retained_bytes - source.source.booleans.retained_bytes,
        2 * "NiBoolData".len()
    );
    let mut future = blocks();
    future[0].1 = interpolator(2, 4);
    let (_, source) = keyframes::decode(&container(&future, 34), "future.kf").unwrap();
    assert_eq!(
        serde_json::to_value(source.source.booleans.dependencies).unwrap(),
        json!([
            {"block":0,"target":4,"target_type":"FutureBoolData","status":"unknown_class"}
        ])
    );
}
#[test]
fn work_and_combined_storage_are_admitted_at_exact_boundaries() {
    let bytes = container(&blocks(), 34);
    let (_, prior) = boolean::decode(&bytes, "prior.kf").unwrap();
    let (_, source) = keyframes::decode(&bytes, "source.kf").unwrap();
    let cap = prior.source.source.animation.retained_bytes
        + prior.source.source.keys.retained_bytes
        + prior.source.source.splines.retained_bytes
        + prior.source.components.retained_bytes
        + prior.booleans.retained_bytes
        + source.keys.retained_bytes;
    let mut exact = keyframes::Limits::default();
    exact
        .booleans
        .components
        .splines
        .keyframes
        .max_combined_retained_bytes = cap;
    exact.array_bytes = source.keys.retained_bytes;
    exact.key_work = 11;
    keyframes::decode_with_limits(&bytes, "exact.kf", exact).unwrap();
    fails(
        &bytes,
        keyframes::Limits {
            key_work: 10,
            ..exact
        },
        "Boolean-key work budget exceeded",
    );
    fails(
        &bytes,
        keyframes::Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        "payload array storage budget exceeded",
    );
    let mut under = exact;
    under
        .booleans
        .components
        .splines
        .keyframes
        .max_combined_retained_bytes -= 1;
    fails(&bytes, under, "payload array storage budget exceeded");
    fails(
        &bytes,
        keyframes::Limits {
            key_work: 1,
            array_bytes: 0,
            ..exact
        },
        "Boolean-key work budget exceeded",
    );
}
#[test]
fn unsupported_tags_stay_opaque_in_prior_schema_and_never_become_constant() {
    for tag in [0, 1, 2, 3, 4, 6, u32::MAX] {
        let bytes = container(&[("NiBoolData", words(&[1, tag, 0]))], 34);
        fails(
            &bytes,
            Default::default(),
            &format!("key type {tag} unsupported"),
        );
        boolean::decode(&bytes, "opaque.kf").unwrap();
    }
}
#[test]
fn counts_spans_surplus_and_nonfinite_time_refuse_without_key_repair() {
    for (payload, reason) in [
        (words(&[1]), "field exceeds"),
        (words(&[1, 5]), "array exceeds"),
        (words(&[2_000_001, 5]), "array exceeds"),
        (words(&[0, 5]), "unconsumed bytes"),
        (group(&[(0x7fc0_0001, 255)]), "nonfinite"),
        (group(&[(0x7f80_0000, 2)]), "nonfinite"),
        (group(&[(0xff80_0000, 0)]), "nonfinite"),
    ] {
        let bytes = container(&[("NiBoolData", payload)], 34);
        fails(&bytes, Default::default(), reason);
        boolean::decode(&bytes, "opaque.kf").unwrap();
    }
    let mut surplus = group(&[(0, 2)]);
    surplus.push(255);
    fails(
        &container(&[("NiBoolData", surplus)], 34),
        Default::default(),
        "unconsumed bytes",
    );
}
#[test]
fn no_selected_data_needs_no_key_work_or_storage() {
    let bytes = container(&[("FutureBoolData", vec![255])], 34);
    let (_, source) = keyframes::decode_with_limits(
        &bytes,
        "absent.kf",
        keyframes::Limits {
            key_work: 0,
            array_bytes: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(source.keys.blocks.is_empty());
    assert_eq!(source.keys.work_units, 0);
    assert_eq!(source.keys.retained_bytes, 0);
}
