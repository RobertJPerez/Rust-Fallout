//! Independent authored bytes/expectations from pinned source metadata.
use fallout_data::nif_animation::{self, Dependency, keyframe, spline};
use serde_json::json;

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    out.extend(words(&[0x1402_0007]));
    out.push(1);
    out.extend(words(&[11, blocks.len() as u32, stream]));
    out.extend([0; 3]);
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
    out.extend(words(&[0, 0, 0]));
    for (_, payload) in blocks {
        out.extend(payload);
    }
    out.extend(words(&[1, 0]));
    out
}
fn control_points() -> Vec<u8> {
    let mut result = words(&[5, 0x8000_0000, 1, 0x7f7f_ffff, 0xff7f_ffff, 0x3f80_0000, 7]);
    for short in [i16::MIN, -32767, -1, 0, 1, i16::MAX, -1] {
        result.extend(short.to_le_bytes());
    }
    result
}
fn interpolator(data: u32, basis: u32) -> Vec<u8> {
    words(&[
        0x7f7f_ffff,
        0xff7f_ffff,
        data,
        basis,
        0x8000_0000,
        1,
        0xff7f_ffff,
        0,
        0x8000_0000,
        0x4000_0000,
        0xc040_0000,
        0x7f7f_ffff,
        0x8000_0000,
        0xffff_0000,
        u32::MAX,
        0x8000_0000,
        1,
        0xff7f_ffff,
        0x7f7f_ffff,
        0x3f80_0000,
        0,
    ])
}
fn controller(interpolator: u32) -> Vec<u8> {
    let mut result = words(&[u32::MAX]);
    result.extend(0u16.to_le_bytes());
    result.extend(words(&[
        0x3f80_0000,
        0,
        0,
        0x3f80_0000,
        u32::MAX,
        interpolator,
    ]));
    result
}
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiBSplineCompTransformInterpolator", interpolator(2, 1)),
        ("NiBSplineBasisData", words(&[u32::MAX])),
        ("NiBSplineData", control_points()),
        ("NiTransformController", controller(0)),
        ("NiTransformData", words(&[0, 0, 0])),
        (
            "NiBSplineCompPoint3Interpolator",
            b"opaque-unsupported".to_vec(),
        ),
        ("NiTransformController", controller(5)),
    ]
}
fn fails(bytes: &[u8], limits: spline::Limits, expected: &str) {
    let error = spline::decode_with_limits(bytes, "authored-spline.kf", limits)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error}"
    );
    assert!(error.contains("authored-spline.kf"));
}
#[test]
fn exact_source_words_signed_extremes_and_uninterpreted_handles_all_streams() {
    let expected = json!({"kind":"compact_transform", "start_bits":0x7f7f_ffffu32,"stop_bits":0xff7f_ffffu32,
        "spline_data":2,"basis_data":1,"translation_bits":[0x8000_0000u32,1,0xff7f_ffffu32],
        "rotation_wxyz_bits":[0,0x8000_0000u32,0x4000_0000u32,0xc040_0000u32], "scale_bits":0x7f7f_ffffu32,
        "translation_handle":0x8000_0000u32,"rotation_handle":0xffff_0000u32,"scale_handle":u32::MAX,
        "translation_offset_bits":0x8000_0000u32,"translation_half_range_bits":1,
        "rotation_offset_bits":0xff7f_ffffu32,"rotation_half_range_bits":0x7f7f_ffffu32,
        "scale_offset_bits":0x3f80_0000u32,"scale_half_range_bits":0});
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        let bytes = container(&blocks(), stream);
        let (index, source) = spline::decode(&bytes, "spline.kf").unwrap();
        assert_eq!(
            serde_json::to_value(&source.splines.blocks[0].data).unwrap(),
            expected
        );
        assert_eq!(
            serde_json::to_value(&source.splines.blocks[1].data).unwrap(),
            json!({"kind":"basis","num_control_points":u32::MAX})
        );
        assert_eq!(
            serde_json::to_value(&source.splines.blocks[2].data).unwrap(),
            json!({"kind":"control_points",
            "declared_float_count":5,"float_bits":[0x8000_0000u32,1,0x7f7f_ffffu32,0xff7f_ffffu32,0x3f80_0000u32],
            "declared_compact_count":7,"compact":[-32768,-32767,-1,0,1,32767,-1]})
        );
        assert_eq!(source.splines.blocks[0].offset, index.blocks[0].offset);
        assert_eq!(source.splines.blocks[0].bytes, 84);
        assert_eq!(source.splines.work_units, 39); // 3 blocks+21 fields+1 basis+2 counts+12 array values.
        assert!(source.splines.dependencies.is_empty());
        assert!(
            !source.animation.runtime_ready
                && !source.keys.runtime_ready
                && !source.splines.runtime_ready
        );
    }
}

#[test]
fn multiple_compact_transforms_share_one_data_and_basis_catalogue_row() {
    let mut blocks = blocks();
    blocks.push(("NiBSplineCompTransformInterpolator", interpolator(2, 1)));
    let bytes = container(&blocks, 34);
    let (_, source) = spline::decode(&bytes, "shared-spline.kf").unwrap();

    assert_eq!(
        source
            .splines
            .blocks
            .iter()
            .map(|block| block.block)
            .collect::<Vec<_>>(),
        [0, 1, 2, 7]
    );
    assert_eq!(
        source
            .splines
            .blocks
            .iter()
            .filter(|block| block.block_type == "NiBSplineCompTransformInterpolator")
            .count(),
        2
    );
    assert_eq!(
        source
            .splines
            .blocks
            .iter()
            .filter(|block| block.block_type == "NiBSplineData")
            .count(),
        1
    );
    assert_eq!(
        source
            .splines
            .blocks
            .iter()
            .filter(|block| block.block_type == "NiBSplineBasisData")
            .count(),
        1
    );
    assert!(source.splines.dependencies.is_empty());
    assert_eq!(source.splines.work_units, 61);
    assert!(!source.splines.runtime_ready);
}

#[test]
fn schema1_and_schema2_still_leave_splines_opaque_and_keys_exact() {
    let bytes = container(&blocks(), 34);
    let (_, old) = nif_animation::decode(&bytes, "spline.kf").unwrap();
    let (_, keys) = keyframe::decode(&bytes, "spline.kf").unwrap();
    let (_, source) = spline::decode(&bytes, "spline.kf").unwrap();
    assert_eq!(old.dependencies.len(), 2);
    assert_eq!(keys.animation.dependencies.len(), 2);
    assert_eq!(source.animation.dependencies.len(), 1);
    assert!(matches!(
        &source.animation.dependencies[0],
        Dependency::Link { target: 5, .. }
    ));
    assert_eq!(
        serde_json::to_value(keys.keys).unwrap(),
        serde_json::to_value(source.keys).unwrap()
    );
    assert_eq!(
        serde_json::to_value(old.blocks).unwrap(),
        serde_json::to_value(source.animation.blocks).unwrap()
    );
}
#[test]
fn unknown_targets_remain_ordered_dependencies_and_nulls_are_absent() {
    let mut blocks = blocks();
    blocks.push(("FutureSplineSource", b"opaque".to_vec()));
    blocks[0].1 = interpolator(7, 7);
    let (_, source) = spline::decode(&container(&blocks, 34), "future.kf").unwrap();
    let deps = serde_json::to_value(source.splines.dependencies).unwrap();
    assert_eq!(
        deps,
        json!([
            {"block":0,"role":"spline_data","target":7,"target_type":"FutureSplineSource","status":"unknown_class"},
            {"block":0,"role":"basis_data","target":7,"target_type":"FutureSplineSource","status":"unknown_class"}
        ])
    );
    blocks[0].1 = interpolator(u32::MAX, u32::MAX);
    assert!(
        spline::decode(&container(&blocks, 34), "null.kf")
            .unwrap()
            .1
            .splines
            .dependencies
            .is_empty()
    );
}
#[test]
fn known_wrong_types_and_out_of_range_references_rejected() {
    for (data, basis, reason) in [
        (1, 1, "SplineData link has wrong target kind"),
        (2, 2, "BasisData link has wrong target kind"),
        (7, 1, "block index out of range"),
    ] {
        let mut blocks = blocks();
        blocks[0].1 = interpolator(data, basis);
        fails(&container(&blocks, 34), Default::default(), reason);
    }
}
#[test]
fn empty_arrays_and_arbitrary_basis_scalars_are_source_values() {
    for count in [0, 1, 2, 3, 4, u32::MAX] {
        let bytes = container(
            &[
                ("NiBSplineData", words(&[0, 0])),
                ("NiBSplineBasisData", words(&[count])),
            ],
            34,
        );
        let (_, source) = spline::decode(&bytes, "empty.kf").unwrap();
        assert_eq!(source.splines.work_units, 5);
        assert_eq!(
            serde_json::to_value(&source.splines.blocks[1].data).unwrap(),
            json!({"kind":"basis","num_control_points":count})
        );
    }
}
#[test]
fn exact_storage_work_and_combined_caps_admitted_one_under_rejected() {
    let bytes = container(&blocks(), 34);
    let (_, source) = spline::decode(&bytes, "budgets.kf").unwrap();
    let (_, before) = keyframe::decode(&bytes, "budgets.kf").unwrap();
    let cap = before.animation.retained_bytes
        + before.keys.retained_bytes
        + source.splines.retained_bytes;
    let exact = spline::Limits {
        keyframes: keyframe::Limits {
            max_combined_retained_bytes: cap,
            ..Default::default()
        },
        array_bytes: source.splines.retained_bytes,
        spline_work: 39,
    };
    spline::decode_with_limits(&bytes, "exact.kf", exact).unwrap();
    fails(
        &bytes,
        spline::Limits {
            spline_work: 38,
            ..exact
        },
        "spline-source work budget exceeded",
    );
    fails(
        &bytes,
        spline::Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        "array storage budget exceeded",
    );
    fails(
        &bytes,
        spline::Limits {
            keyframes: keyframe::Limits {
                max_combined_retained_bytes: cap - 1,
                ..exact.keyframes
            },
            ..exact
        },
        "array storage budget exceeded",
    );
    fails(
        &bytes,
        spline::Limits {
            spline_work: 0,
            ..exact
        },
        "spline-source work budget exceeded",
    );
}
#[test]
fn control_point_work_admission_precedes_storage_allocation() {
    let bytes = container(&[("NiBSplineData", control_points())], 34);
    let (_, source) = spline::decode(&bytes, "points.kf").unwrap();
    let empty_storage = std::mem::size_of::<spline::Block>() + 64;
    // Enough for the block but not floats; zero array work must reject work
    // first. A later storage failure would reveal allocation-order regression.
    fails(
        &bytes,
        spline::Limits {
            array_bytes: empty_storage,
            spline_work: 2,
            ..Default::default()
        },
        "spline-source work budget exceeded",
    );
    let float_storage = empty_storage + 5 * 4;
    fails(
        &bytes,
        spline::Limits {
            array_bytes: float_storage,
            spline_work: 8,
            ..Default::default()
        },
        "spline-source work budget exceeded",
    );
    assert_eq!(source.splines.work_units, 15);
}
#[test]
fn malformed_payloads_rejected_without_schema1_or_schema2_behavior_change() {
    let mut cases = vec![
        ("NiBSplineData", words(&[u32::MAX]), "array exceeds"),
        ("NiBSplineData", words(&[0, u32::MAX]), "array exceeds"),
        ("NiBSplineData", words(&[1, 0x7fc0_0001, 0]), "nonfinite"),
        ("NiBSplineBasisData", vec![0; 3], "field exceeds"),
        ("NiBSplineBasisData", vec![0; 5], "unconsumed bytes"),
    ];
    let mut truncated = control_points();
    truncated.pop();
    cases.push(("NiBSplineData", truncated, "array exceeds"));
    let mut surplus = control_points();
    surplus.push(0);
    cases.push(("NiBSplineData", surplus, "unconsumed bytes"));
    let mut truncated = interpolator(u32::MAX, u32::MAX);
    truncated.pop();
    cases.push((
        "NiBSplineCompTransformInterpolator",
        truncated,
        "field exceeds",
    ));
    let mut nan = interpolator(u32::MAX, u32::MAX);
    nan[60..64].copy_from_slice(&0x7fc0_0001u32.to_le_bytes());
    cases.push(("NiBSplineCompTransformInterpolator", nan, "nonfinite"));
    for (kind, payload, reason) in cases {
        let bytes = container(&[(kind, payload)], 34);
        fails(&bytes, Default::default(), reason);
        nif_animation::decode(&bytes, "opaque.kf").unwrap();
        keyframe::decode(&bytes, "opaque.kf").unwrap();
    }
}
#[test]
fn existing_container_tuple_and_block_limits_still_apply() {
    fails(
        &container(&blocks(), 35),
        Default::default(),
        "NIF tuple version=",
    );
    let limits = spline::Limits {
        keyframes: keyframe::Limits {
            animation: nif_animation::Limits {
                blocks: 6,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    fails(&container(&blocks(), 34), limits, "input/count budget");
}
