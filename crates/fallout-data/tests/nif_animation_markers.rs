//! Authored byte source and physical expectations independent of query code.
use fallout_data::nif_animation::markers::{self, Limits, Request};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_le_bytes()).collect()
}
fn sequence(link: u32) -> Vec<u8> {
    let mut bytes = words(&[
        0,
        0,
        17,
        0.5f32.to_bits(),
        link,
        37,
        7f32.to_bits(),
        100f32.to_bits(),
        101f32.to_bits(),
        NULL,
        NULL,
    ]);
    bytes.extend(0u16.to_le_bytes());
    bytes
}
fn text(keys: &[(f32, u32)]) -> Vec<u8> {
    let mut bytes = words(&[NULL, keys.len() as u32]);
    for &(time, string) in keys {
        bytes.extend(words(&[time.to_bits(), string]));
    }
    bytes
}
fn keys() -> Vec<(f32, u32)> {
    vec![(2., 1), (-0., 2), (1., 3), (1., 1), (-2., 4), (3., 5)]
}
fn container(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let strings: [&[u8]; 6] = [b"sequence", b"dup", b"\xff\0\0", b"", b"early", b"late"];
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend(words(&[0x14020007]));
    bytes.push(1);
    bytes.extend(words(&[11, blocks.len() as u32, 34]));
    bytes.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    bytes.extend((types.len() as u16).to_le_bytes());
    for name in &types {
        bytes.extend(words(&[name.len() as u32]));
        bytes.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        bytes.extend((types.iter().position(|n| n == name).unwrap() as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        bytes.extend(words(&[payload.len() as u32]));
    }
    bytes.extend(words(&[strings.len() as u32, 8]));
    for string in strings {
        bytes.extend(words(&[string.len() as u32]));
        bytes.extend(string);
    }
    bytes.extend(words(&[0]));
    for (_, payload) in blocks {
        bytes.extend(payload);
    }
    bytes.extend(words(&[1, 0]));
    bytes
}
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&keys())),
    ]
}
fn request(bytes: &[u8], start: f64, end: f64) -> Request {
    Request {
        expected_sha256: Sha256::digest(bytes).into(),
        sequence: 0,
        source_start: start,
        source_end: end,
    }
}
fn query(bytes: &[u8], start: f64, end: f64) -> markers::Observation {
    markers::query(
        bytes,
        "authored",
        request(bytes, start, end),
        Limits::default(),
    )
    .unwrap()
}

#[test]
fn interval_preserves_physical_nonmonotonic_order_duplicates_and_exact_raw_strings() {
    let bytes = container(&blocks());
    let result = query(&bytes, 0., 2.);
    assert_eq!(
        result
            .entries
            .iter()
            .map(|e| (
                e.source_key_ordinal,
                e.time_bits,
                e.string_index,
                e.raw_string_bytes.clone()
            ))
            .collect::<Vec<_>>(),
        [
            (0, 2f32.to_bits(), 1, b"dup".to_vec()),
            (1, (-0f32).to_bits(), 2, b"\xff\0\0".to_vec()),
            (2, 1f32.to_bits(), 3, vec![]),
            (3, 1f32.to_bits(), 1, b"dup".to_vec())
        ]
    );
    assert_eq!(
        (
            result.sequence.block,
            result.text_keys.block,
            result.declared_source_keys
        ),
        (0, 1, 6)
    );
    assert_eq!(result.text_keys.bytes, 56);
    assert_eq!(result.unapplied_sequence_clock_fields.cycle_type, 37);
    assert_eq!(
        result.unapplied_sequence_clock_fields.start_bits,
        100f32.to_bits()
    );
    assert!(!result.retail_behavior_verified);
}
#[test]
fn closed_point_boundaries_use_exact_promoted_binary32_times() {
    let bytes = container(&blocks());
    let point = query(&bytes, 1., 1.);
    assert_eq!(
        point
            .entries
            .iter()
            .map(|e| e.source_key_ordinal)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    let just_after = f64::from_bits(1f64.to_bits() + 1);
    let after = query(&bytes, just_after, 2.);
    assert_eq!(
        after
            .entries
            .iter()
            .map(|e| e.source_key_ordinal)
            .collect::<Vec<_>>(),
        [0]
    );
    assert_eq!(after.source_start_f64_bits, just_after.to_bits());
    let signed = query(&bytes, 0., -0.);
    assert_eq!(signed.entries[0].time_bits, (-0f32).to_bits());
    assert_eq!(signed.source_end_f64_bits, (-0f64).to_bits());
}
#[test]
fn empty_interval_and_authored_empty_array_are_explicit_observations() {
    let bytes = container(&blocks());
    assert!(query(&bytes, 4., 5.).entries.is_empty());
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&[])),
    ]);
    let result = query(&bytes, -9., 9.);
    assert_eq!(result.declared_source_keys, 0);
    assert!(result.entries.is_empty());
}
#[test]
fn invalid_request_sha_sequence_or_missing_text_link_refuses() {
    let bytes = container(&blocks());
    for (start, end) in [(f64::NAN, 1.), (0., f64::INFINITY), (2., 1.)] {
        assert!(
            markers::query(
                &bytes,
                "bad bounds",
                request(&bytes, start, end),
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("finite ordered bounds")
        );
    }
    let mut stale = request(&bytes, 0., 2.);
    stale.expected_sha256[0] ^= 1;
    assert!(
        markers::query(&bytes, "stale", stale, Default::default())
            .unwrap_err()
            .to_string()
            .contains("SHA256 differs")
    );
    for id in [1, 99] {
        let mut selected = request(&bytes, 0., 2.);
        selected.sequence = id;
        assert!(
            markers::query(&bytes, "wrong sequence", selected, Default::default())
                .unwrap_err()
                .to_string()
                .contains(&format!("sequence {id} is not decoded"))
        );
    }
    let bytes = container(&[
        ("NiControllerSequence", sequence(NULL)),
        ("NiTextKeyExtraData", text(&keys())),
    ]);
    assert!(
        markers::query(
            &bytes,
            "missing link",
            request(&bytes, 0., 2.),
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("sequence 0 has no text_keys link")
    );
}
#[test]
fn invalid_authored_time_or_absent_string_refuses_even_outside_interval() {
    let mut source_keys = keys();
    source_keys[5].0 = f32::INFINITY;
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&source_keys)),
    ]);
    let error = markers::query(
        &bytes,
        "bad time",
        request(&bytes, 4., 5.),
        Default::default(),
    )
    .unwrap_err();
    match error {
        fallout_data::Error::Format { offset, reason, .. } => {
            assert_eq!(offset, (bytes.len() - 12) as u64);
            assert_eq!(reason, "nonfinite NIF float");
        }
        other => panic!("{other}"),
    }
    source_keys = keys();
    source_keys[4].1 = NULL;
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&source_keys)),
    ]);
    assert!(
        markers::query(
            &bytes,
            "missing string",
            request(&bytes, 4., 5.),
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("text_keys 1 key 4 has no authored string")
    );
}
#[test]
fn exact_output_combined_work_input_caps_and_one_over_are_checked() {
    let bytes = container(&blocks());
    let baseline = query(&bytes, 0., 2.);
    let exact = Limits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        max_combined_retained_bytes: baseline.combined_retained_bytes,
        ..Default::default()
    };
    assert!(markers::query(&bytes, "exact", request(&bytes, 0., 2.), exact).is_ok());
    for limits in [
        Limits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        Limits {
            max_combined_retained_bytes: exact.max_combined_retained_bytes - 1,
            ..exact
        },
    ] {
        let error =
            markers::query(&bytes, "one over", request(&bytes, 0., 2.), limits).unwrap_err();
        assert!(error.to_string().contains("text_keys 1 key 3"), "{error}");
    }
    let error = markers::query(
        &bytes,
        "work",
        request(&bytes, 0., 2.),
        Limits {
            work_units: exact.work_units - 1,
            ..exact
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("work budget"));
    let mut input = exact;
    input.source.input_bytes = bytes.len();
    assert!(markers::query(&bytes, "exact input", request(&bytes, 0., 2.), input).is_ok());
    input.source.input_bytes -= 1;
    assert!(
        markers::query(&bytes, "input over", request(&bytes, 0., 2.), input)
            .unwrap_err()
            .to_string()
            .contains("input byte budget")
    );
}
#[test]
fn unlinked_text_block_is_never_merged_or_selected_instead() {
    let mut source = blocks();
    source.push(("NiTextKeyExtraData", text(&[(9., NULL)])));
    let bytes = container(&source);
    assert_eq!(query(&bytes, 0., 2.).entries.len(), 4);
    assert_eq!(query(&bytes, 0., 2.).text_keys.block, 1);
}
#[test]
fn wrong_text_key_type_invalid_string_reference_and_truncated_source_refuse() {
    let mut source = blocks();
    source[0].1 = sequence(0);
    let bytes = container(&source);
    assert!(
        markers::query(
            &bytes,
            "wrong type",
            request(&bytes, 0., 2.),
            Default::default()
        )
        .is_err()
    );
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&[(1., 99)])),
    ]);
    assert!(
        markers::query(
            &bytes,
            "bad reference",
            request(&bytes, 0., 2.),
            Default::default()
        )
        .is_err()
    );
    let mut bytes = container(&blocks());
    bytes.pop();
    assert!(
        markers::query(
            &bytes,
            "truncated",
            request(&bytes, 0., 2.),
            Default::default()
        )
        .is_err()
    );
}
