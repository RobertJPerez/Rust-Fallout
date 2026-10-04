//! Independently authored source words from pinned XML, without evaluated poses.
use fallout_data::nif_animation::{self, Dependency, LinkRole, keyframe};
use serde_json::json;

const STREAMS: [u32; 12] = [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34];
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    out.extend(words(&[0x1402_0007]));
    out.push(1);
    out.extend(words(&[11, blocks.len() as u32, stream]));
    out.extend([0; 3]);
    // Repeated type names are legal and keep this authored container independent
    // of production catalogue construction.
    out.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        out.extend(words(&[name.len() as u32]));
        out.extend(name.as_bytes());
    }
    for id in 0..blocks.len() {
        out.extend((id as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        out.extend(words(&[payload.len() as u32]));
    }
    out.extend(words(&[0, 0, 0])); // Empty string table and group array.
    for (_, payload) in blocks {
        out.extend(payload);
    }
    out.extend(words(&[1, 0]));
    out
}
fn one(payload: Vec<u8>) -> Vec<u8> {
    container(&[("NiTransformData", payload)], 34)
}
fn mixed() -> Vec<u8> {
    words(&[
        2,
        2, // Quaternion tag 2 has no stored tangents.
        0x4000_0000,
        0,
        0x8000_0000,
        0x7f7f_ffff,
        0xff7f_ffff,
        0x3f80_0000,
        0x4000_0000,
        0xc040_0000,
        0x4080_0000,
        0xc0a0_0000,
        1,
        2, // A vector key with ordered forward/backward tangents.
        0x3f00_0000,
        0x3f80_0000,
        0x8000_0000,
        1,
        0x4000_0000,
        0x4040_0000,
        0x4080_0000,
        0xc000_0000,
        0xc040_0000,
        0xc080_0000,
        1,
        3, // A scalar TBC key.
        0x3f40_0000,
        0xbf80_0000,
        0x3e80_0000,
        0xbf00_0000,
        0x3f80_0000,
    ])
}
fn interpolator(target: u32) -> Vec<u8> {
    words(&[0, 0, 0, 0x4000_0000, 0, 0, 0, 0x3f80_0000, target])
}
fn fails(bytes: &[u8], limits: keyframe::Limits, reason: &str) {
    let error = keyframe::decode_with_limits(bytes, "authored.kf", limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "expected {reason:?}, got {error}");
    assert!(error.contains("authored.kf"), "missing source: {error}");
}

#[test]
fn exact_quaternion_vector_scalar_words_across_admitted_streams() {
    let expected = json!({
        "declared_rotation_keys": 2,
        "rotation": {"layout":"quaternion", "key_type":2, "keys":[
            {"time_bits":0x4000_0000u32,"value_wxyz_bits":[0,0x8000_0000u32,0x7f7f_ffffu32,0xff7f_ffffu32],"tbc_bits":null},
            {"time_bits":0x3f80_0000u32,"value_wxyz_bits":[0x4000_0000u32,0xc040_0000u32,0x4080_0000u32,0xc0a0_0000u32],"tbc_bits":null}
        ]},
        "translations":{"declared_keys":1,"key_type":2,"keys":[
            {"time_bits":0x3f00_0000u32,"value_bits":[0x3f80_0000u32,0x8000_0000u32,1],"forward_bits":[0x4000_0000u32,0x4040_0000u32,0x4080_0000u32],"backward_bits":[0xc000_0000u32,0xc040_0000u32,0xc080_0000u32],"tbc_bits":null}
        ]},
        "scales":{"declared_keys":1,"key_type":3,"keys":[
            {"time_bits":0x3f40_0000u32,"value_bits":[0xbf80_0000u32],"forward_bits":null,"backward_bits":null,"tbc_bits":[0x3e80_0000u32,0xbf00_0000u32,0x3f80_0000u32]}
        ]}
    });
    for stream in STREAMS {
        let bytes = container(&[("NiTransformData", mixed())], stream);
        let (index, source) = keyframe::decode(&bytes, "mixed.kf").unwrap();
        assert_eq!(
            serde_json::to_value(&source.keys.blocks[0].data).unwrap(),
            expected
        );
        assert_eq!(source.keys.blocks[0].offset, index.blocks[0].offset);
        assert_eq!(source.keys.blocks[0].bytes, 124);
        assert!(!source.keys.runtime_ready && !source.animation.runtime_ready);
    }
}

#[test]
fn quaternion_tags_have_only_their_actual_source_fields() {
    for tag in [1, 2, 3, 5] {
        let mut payload = words(&[1, tag, 0x8000_0000, 0, 0, 0, 0]);
        if tag == 3 {
            payload.extend(words(&[1, 0x8000_0000, 0x7f7f_ffff]));
        }
        payload.extend(words(&[0, 0]));
        let (_, source) = keyframe::decode(&one(payload), "quaternion.kf").unwrap();
        let keyframe::Rotation::Quaternion { key_type, keys } =
            &source.keys.blocks[0].data.rotation
        else {
            panic!("wrong layout")
        };
        assert_eq!(*key_type, tag);
        assert_eq!(keys[0].time_bits, 0x8000_0000);
        assert_eq!(keys[0].value_wxyz_bits, [0; 4]);
        assert_eq!(
            keys[0].tbc_bits,
            if tag == 3 {
                Some([1, 0x8000_0000, 0x7f7f_ffff])
            } else {
                None
            }
        );
    }
}

#[test]
fn xyz_axes_preserve_independent_tags_counts_and_missing_axes() {
    let bytes = one(words(&[
        1,
        4,
        2,
        1,
        0x4000_0000,
        0x3f80_0000,
        0x3f80_0000,
        0xbf80_0000,
        0,
        1,
        3,
        0,
        0x8000_0000,
        0x3e80_0000,
        0xbf00_0000,
        0x3f80_0000,
        0,
        0,
    ]));
    let (_, source) = keyframe::decode(&bytes, "xyz.kf").unwrap();
    let data = &source.keys.blocks[0].data;
    assert_eq!(data.declared_rotation_keys, 1);
    let keyframe::Rotation::Xyz { axes } = &data.rotation else {
        panic!("wrong layout")
    };
    assert_eq!(axes[0].declared_keys, 2);
    assert_eq!(axes[0].key_type, Some(1));
    assert_eq!(axes[0].keys[0].time_bits, 0x4000_0000);
    assert_eq!(axes[0].keys[1].time_bits, 0x3f80_0000);
    assert_eq!(axes[1].key_type, None);
    assert!(axes[1].keys.is_empty());
    assert_eq!(axes[2].key_type, Some(3));
    assert_eq!(axes[2].keys[0].value_bits, [0x8000_0000]);
    assert_eq!(
        axes[2].keys[0].tbc_bits,
        Some([0x3e80_0000, 0xbf00_0000, 0x3f80_0000])
    );
}

#[test]
fn scalar_and_vector_tags_keep_tangents_tbc_and_duplicates() {
    for tag in [1, 2, 3, 5] {
        let mut payload = words(&[0, 2, tag]);
        for value in [0x4000_0000, 0xbf80_0000] {
            payload.extend(words(&[0x3f80_0000, value, 0, 0]));
            if tag == 2 {
                payload.extend(words(&[1, 2, 3, 4, 5, 6]));
            }
            if tag == 3 {
                payload.extend(words(&[7, 8, 9]));
            }
        }
        payload.extend(words(&[1, tag, 0x8000_0000, 0xff7f_ffff]));
        if tag == 2 {
            payload.extend(words(&[10, 11]));
        }
        if tag == 3 {
            payload.extend(words(&[12, 13, 14]));
        }
        let (_, source) = keyframe::decode(&one(payload), "groups.kf").unwrap();
        let data = &source.keys.blocks[0].data;
        assert_eq!(
            data.translations.keys[0].time_bits,
            data.translations.keys[1].time_bits
        );
        assert_eq!(data.translations.keys[0].value_bits, [0x4000_0000, 0, 0]);
        assert_eq!(data.translations.keys[1].value_bits, [0xbf80_0000, 0, 0]);
        assert_eq!(
            data.translations.keys[0].forward_bits,
            if tag == 2 { Some([1, 2, 3]) } else { None }
        );
        assert_eq!(
            data.translations.keys[0].backward_bits,
            if tag == 2 { Some([4, 5, 6]) } else { None }
        );
        assert_eq!(
            data.translations.keys[0].tbc_bits,
            if tag == 3 { Some([7, 8, 9]) } else { None }
        );
        assert_eq!(data.scales.keys[0].value_bits, [0xff7f_ffff]);
        assert_eq!(
            data.scales.keys[0].forward_bits,
            if tag == 2 { Some([10]) } else { None }
        );
        assert_eq!(
            data.scales.keys[0].backward_bits,
            if tag == 2 { Some([11]) } else { None }
        );
        assert_eq!(
            data.scales.keys[0].tbc_bits,
            if tag == 3 { Some([12, 13, 14]) } else { None }
        );
    }
}

#[test]
fn zero_counts_omit_tags_and_keep_absent_distinct_from_empty_xyz() {
    let (_, source) = keyframe::decode(&one(words(&[0, 0, 0])), "absent.kf").unwrap();
    assert!(matches!(
        source.keys.blocks[0].data.rotation,
        keyframe::Rotation::Absent
    ));
    assert_eq!(source.keys.blocks[0].data.translations.key_type, None);
    assert_eq!(source.keys.blocks[0].data.scales.key_type, None);
    let (_, source) =
        keyframe::decode(&one(words(&[1, 4, 0, 0, 0, 0, 0])), "empty_xyz.kf").unwrap();
    let keyframe::Rotation::Xyz { axes } = &source.keys.blocks[0].data.rotation else {
        panic!("wrong layout")
    };
    assert!(
        axes.iter()
            .all(|g| g.key_type.is_none() && g.keys.is_empty())
    );
}

#[test]
fn original_entrypoint_and_dependencies_remain_schema_one() {
    let mut controller = words(&[u32::MAX]);
    controller.extend(0u16.to_le_bytes());
    controller.extend(words(&[0x3f80_0000, 0, 0, 0x3f80_0000, u32::MAX, 4]));
    let bytes = container(
        &[
            ("NiTransformInterpolator", interpolator(3)),
            ("NiTransformController", controller),
            ("NiTransformInterpolator", interpolator(5)),
            ("NiTransformData", mixed()),
            ("NiBSplineCompTransformInterpolator", vec![255]),
            ("UnknownData", vec![1, 2, 3]),
        ],
        34,
    );
    let (_, original) = nif_animation::decode(&bytes, "links.kf").unwrap();
    let original_blocks = serde_json::to_value(&original.blocks).unwrap();
    let (_, combined) = keyframe::decode(&bytes, "links.kf").unwrap();
    assert_eq!(
        original_blocks,
        serde_json::to_value(&combined.animation.blocks).unwrap()
    );
    assert_eq!(original.dependencies.len(), 3);
    assert_eq!(combined.animation.dependencies.len(), 2);
    assert!(combined.animation.dependencies.iter().all(|d| matches!(
        d,
        Dependency::Link {
            role: LinkRole::Interpolator,
            target: 4,
            ..
        } | Dependency::Link {
            role: LinkRole::TransformData,
            target: 5,
            ..
        }
    )));
    assert_eq!(
        combined.animation.retained_bytes,
        original.retained_bytes - "NiTransformData".len()
    );
    let invalid = container(
        &[
            ("NiTransformInterpolator", interpolator(1)),
            ("NiTransformData", vec![255]),
        ],
        34,
    );
    assert!(nif_animation::decode(&invalid, "opaque.kf").is_ok());
    fails(&invalid, keyframe::Limits::default(), "field exceeds");
}

#[test]
fn unsupported_tags_and_xyz_leading_counts_fail_explicitly() {
    for tag in [0, 6, u32::MAX] {
        fails(
            &one(words(&[1, tag])),
            keyframe::Limits::default(),
            "unadmitted quaternion key tag",
        );
        fails(
            &one(words(&[0, 1, tag])),
            keyframe::Limits::default(),
            "unadmitted transform key-group tag",
        );
        fails(
            &one(words(&[0, 0, 1, tag])),
            keyframe::Limits::default(),
            "unadmitted transform key-group tag",
        );
    }
    for count in [2, 3, u32::MAX] {
        fails(
            &one(words(&[count, 4, 0, 0, 0, 0, 0])),
            keyframe::Limits::default(),
            "XYZ rotation source count other than one",
        );
    }
    fails(
        &one(words(&[0, 1, 4])),
        keyframe::Limits::default(),
        "unadmitted transform key-group tag",
    );
}

#[test]
fn truncation_surplus_and_huge_counts_are_rejected() {
    let payload = mixed();
    for length in 0..payload.len() {
        assert!(
            keyframe::decode(&one(payload[..length].to_vec()), "truncated.kf").is_err(),
            "accepted prefix {length}"
        );
    }
    let mut surplus = payload;
    surplus.push(0);
    fails(
        &one(surplus),
        keyframe::Limits::default(),
        "unconsumed bytes",
    );
    for payload in [
        words(&[u32::MAX, 1]),
        words(&[0, u32::MAX, 1]),
        words(&[0, 0, u32::MAX, 1]),
    ] {
        fails(
            &one(payload),
            keyframe::Limits::default(),
            "array exceeds block or element budget",
        );
    }
}

#[test]
fn nonfinite_source_words_are_rejected_without_repair() {
    for offset in (8..48)
        .step_by(4)
        .chain((56..96).step_by(4))
        .chain((104..124).step_by(4))
    {
        for bits in [0x7f80_0000u32, 0xff80_0000, 0x7fc0_0001] {
            let mut payload = mixed();
            payload[offset..offset + 4].copy_from_slice(&bits.to_le_bytes());
            fails(
                &one(payload),
                keyframe::Limits::default(),
                "nonfinite NIF float",
            );
        }
    }
}

#[test]
fn exact_storage_and_work_limits_and_one_under() {
    let bytes = one(mixed());
    let (_, source) = keyframe::decode(&bytes, "measure.kf").unwrap();
    assert_eq!(source.keys.work_units, 29);
    let limits = keyframe::Limits {
        array_bytes: source.keys.retained_bytes,
        key_work: 29,
        ..Default::default()
    };
    assert!(keyframe::decode_with_limits(&bytes, "exact.kf", limits).is_ok());
    fails(
        &bytes,
        keyframe::Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
        "storage budget exceeded",
    );
    fails(
        &bytes,
        keyframe::Limits {
            key_work: 28,
            ..limits
        },
        "transform-key work budget exceeded",
    );
    fails(
        &bytes,
        keyframe::Limits {
            array_bytes: 0,
            key_work: 0,
            ..limits
        },
        "transform-key work budget exceeded",
    );
    // Both key storage and word work are insufficient; word admission rejects
    // before the first quaternion vector can be allocated.
    fails(
        &bytes,
        keyframe::Limits {
            array_bytes: std::mem::size_of::<keyframe::Block>() + 64,
            key_work: 11,
            ..limits
        },
        "transform-key work budget exceeded",
    );
}

#[test]
fn empty_products_and_multiple_blocks_charge_work_and_storage() {
    let empty = one(words(&[0, 0, 0]));
    let limits = keyframe::Limits {
        key_work: 4,
        ..Default::default()
    };
    assert!(keyframe::decode_with_limits(&empty, "exact.kf", limits).is_ok());
    fails(
        &empty,
        keyframe::Limits {
            key_work: 3,
            ..limits
        },
        "transform-key work budget exceeded",
    );
    let xyz = one(words(&[1, 4, 0, 0, 0, 0, 0]));
    assert!(
        keyframe::decode_with_limits(
            &xyz,
            "exact.kf",
            keyframe::Limits {
                key_work: 7,
                ..limits
            }
        )
        .is_ok()
    );
    fails(
        &xyz,
        keyframe::Limits {
            key_work: 6,
            ..limits
        },
        "transform-key work budget exceeded",
    );
    let bytes = container(
        &[("NiTransformData", mixed()), ("NiTransformData", mixed())],
        34,
    );
    let (_, source) = keyframe::decode(&bytes, "twice.kf").unwrap();
    assert_eq!(source.keys.work_units, 58);
    let limits = keyframe::Limits {
        array_bytes: source.keys.retained_bytes,
        key_work: 58,
        ..limits
    };
    assert!(keyframe::decode_with_limits(&bytes, "exact.kf", limits).is_ok());
    fails(
        &bytes,
        keyframe::Limits {
            key_work: 57,
            ..limits
        },
        "transform-key work budget exceeded",
    );
    fails(
        &bytes,
        keyframe::Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
        "storage budget exceeded",
    );
    let opaque = container(&[("UnknownData", vec![])], 34);
    let (_, source) = keyframe::decode_with_limits(
        &opaque,
        "opaque.kf",
        keyframe::Limits {
            array_bytes: 0,
            key_work: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(source.keys.retained_bytes, 0);
}

#[test]
fn unchanged_animation_input_index_and_reference_budgets_apply() {
    let bytes = container(
        &[
            ("NiTransformInterpolator", interpolator(1)),
            ("NiTransformData", mixed()),
        ],
        34,
    );
    let mut limits = keyframe::Limits::default();
    limits.animation.input_bytes = bytes.len() - 1;
    fails(&bytes, limits, "input");
    limits = keyframe::Limits::default();
    limits.animation.blocks = 1;
    fails(&bytes, limits, "container table exceeds input/count budget");
    limits = keyframe::Limits::default();
    limits.animation.array_bytes = 0;
    fails(&bytes, limits, "storage budget exceeded");
    limits = keyframe::Limits::default();
    limits.animation.reference_checks = 0;
    fails(&bytes, limits, "reference-check budget exceeded");
}

#[test]
fn combined_retention_cap_is_admitted_before_key_arrays() {
    let bytes = one(mixed());
    let (_, source) = keyframe::decode(&bytes, "measure.kf").unwrap();
    let required = source.animation.retained_bytes + source.keys.retained_bytes;
    let limits = keyframe::Limits {
        max_combined_retained_bytes: required,
        ..Default::default()
    };
    let (_, exact) = keyframe::decode_with_limits(&bytes, "exact.kf", limits).unwrap();
    assert_eq!(
        exact.animation.retained_bytes + exact.keys.retained_bytes,
        required
    );
    fails(
        &bytes,
        keyframe::Limits {
            max_combined_retained_bytes: required - 1,
            ..limits
        },
        "storage budget exceeded",
    );
    fails(
        &bytes,
        keyframe::Limits {
            max_combined_retained_bytes: 0,
            ..limits
        },
        "storage budget exceeded",
    );
}
