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

fn indexed_request(bytes: &[u8], start: f64, end: f64) -> markers::IntervalRequest {
    markers::IntervalRequest {
        expected_sha256: Sha256::digest(bytes).into(),
        sequence: 0,
        source_start: start,
        source_end: end,
    }
}
fn prepared_keys() -> Vec<(f32, u32)> {
    vec![
        (2., 1),
        (-0., 2),
        (1., 3),
        (1., 1),
        (-2., 4),
        (0., 2),
        (3., 5),
    ]
}
fn prepared_bytes() -> Vec<u8> {
    container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&prepared_keys())),
    ])
}
fn sequence_window(link: u32, start: f32, end: f32) -> Vec<u8> {
    let mut bytes = sequence(link);
    bytes[28..32].copy_from_slice(&start.to_bits().to_le_bytes());
    bytes[32..36].copy_from_slice(&end.to_bits().to_le_bytes());
    bytes
}
fn playback_bytes(start: f32, end: f32) -> Vec<u8> {
    container(&[
        ("NiControllerSequence", sequence_window(1, start, end)),
        (
            "NiTextKeyExtraData",
            text(&[(start, 4), (start + 1.0, 1), (end, 5)]),
        ),
    ])
}
fn playback_request(
    bytes: &[u8],
    start: f64,
    end: f64,
    repeat: markers::RepeatPolicy,
) -> markers::PlayRequest {
    markers::PlayRequest {
        expected_sha256: Sha256::digest(bytes).into(),
        sequence: 0,
        window: markers::SourceWindow { start, end },
        repeat,
        initial_boundary: markers::BoundaryDelivery::Emit,
        loop_start_boundary: markers::BoundaryDelivery::Emit,
    }
}
fn prepare_markers(bytes: &[u8]) -> markers::PreparedSequence {
    markers::PreparedSequence::prepare(
        bytes,
        "independent indexed source",
        Sha256::digest(bytes).into(),
        0,
        Default::default(),
    )
    .unwrap()
}
fn semantic_observation(value: &markers::Observation) -> serde_json::Value {
    let mut value = serde_json::to_value(value).unwrap();
    for field in [
        "retained_bytes",
        "decoded_source_retained_bytes",
        "combined_retained_bytes",
        "work_units",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    value
}

#[test]
fn prepared_index_preserves_literals_zeros_physical_order_and_old_semantic_fields() {
    let bytes = prepared_bytes();
    let prepared = prepare_markers(&bytes);
    assert_eq!(prepared.usage().animation_decodes, 1);
    assert_eq!(prepared.usage().full_source_sha256_traversals, 1);
    assert_eq!(prepared.usage().validated_keys, 7);
    assert_eq!(prepared.sequence(), 0);
    let cases = [
        (0., 2., vec![0, 1, 2, 3, 5]),
        (1., 1., vec![2, 3]),
        (0., -0., vec![1, 5]),
        (-9., 9., vec![0, 1, 2, 3, 4, 5, 6]),
        (4., 5., vec![]),
        (f64::from_bits(1f64.to_bits() + 1), 2., vec![0]),
    ];
    for (start, end, ordinals) in cases {
        let result = prepared
            .query(indexed_request(&bytes, start, end), Default::default())
            .unwrap();
        assert_eq!(
            result
                .observation
                .entries
                .iter()
                .map(|e| e.source_key_ordinal)
                .collect::<Vec<_>>(),
            ordinals
        );
        assert_eq!(
            semantic_observation(&result.observation),
            semantic_observation(&query(&bytes, start, end))
        );
        assert_eq!(result.usage.animation_decodes, 0);
        assert_eq!(result.usage.full_source_sha256_traversals, 0);
        assert_eq!(result.usage.full_key_validations, 0);
        assert_eq!(result.observation.decoded_source_retained_bytes, 0);
        assert_eq!(
            result.usage.matching_index_visits,
            result.observation.entries.len() * 2
        );
        assert_eq!(
            result.usage.combined_retained_bytes,
            prepared.usage().retained_bytes + result.usage.charged_bytes
        );
        assert_eq!(
            result.usage.output_bytes
                + result.observation.entries.len() * std::mem::size_of::<usize>(),
            result.usage.charged_bytes
        );
        assert!(!result.observation.retail_behavior_verified);
    }
    let point = prepared
        .query(indexed_request(&bytes, -0., 0.), Default::default())
        .unwrap();
    assert_eq!(point.observation.source_start_f64_bits, (-0f64).to_bits());
    assert_eq!(point.observation.entries[0].time_bits, (-0f32).to_bits());
    assert_eq!(point.observation.entries[1].time_bits, 0f32.to_bits());
    assert_eq!(point.observation.entries[0].raw_string_bytes, [255, 0, 0]);
    assert_eq!(point.observation.entries[1].raw_string_bytes, [255, 0, 0]);
}

#[test]
fn prepared_index_owns_selected_source_after_input_mutation_drop_and_refusals() {
    let mut bytes = prepared_bytes();
    let request = indexed_request(&bytes, -9., 9.);
    let prepared = prepare_markers(&bytes);
    let before =
        serde_json::to_value(prepared.query(request, Default::default()).unwrap()).unwrap();
    bytes.fill(0);
    drop(bytes);
    for bad in [
        markers::IntervalRequest {
            sequence: 1,
            ..request
        },
        markers::IntervalRequest {
            expected_sha256: [0; 32],
            ..request
        },
        markers::IntervalRequest {
            source_start: f64::NAN,
            ..request
        },
        markers::IntervalRequest {
            source_end: f64::INFINITY,
            ..request
        },
        markers::IntervalRequest {
            source_start: 10.,
            ..request
        },
    ] {
        assert!(prepared.query(bad, Default::default()).is_err());
    }
    assert!(
        prepared
            .query(
                request,
                markers::QueryLimits {
                    work_units: 1,
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(prepared.query(request, Default::default()).unwrap()).unwrap(),
        before
    );
    assert!(!format!("{prepared:?}").contains("raw_string_bytes"));
}

#[test]
fn prepared_index_preparation_validates_out_of_window_keys_links_and_exact_admission() {
    let bytes = prepared_bytes();
    let baseline = prepare_markers(&bytes).usage();
    let exact = markers::PreparationLimits {
        array_bytes: baseline.retained_bytes,
        work_units: baseline.work_units,
        max_combined_retained_bytes: baseline.decoder_array_admission_bytes
            + baseline.retained_bytes,
        keys: 7,
        ..Default::default()
    };
    assert!(
        markers::PreparedSequence::prepare(
            &bytes,
            "exact",
            Sha256::digest(&bytes).into(),
            0,
            exact
        )
        .is_ok()
    );
    for limits in [
        markers::PreparationLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        markers::PreparationLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        markers::PreparationLimits {
            max_combined_retained_bytes: exact.max_combined_retained_bytes - 1,
            ..exact
        },
        markers::PreparationLimits { keys: 6, ..exact },
        markers::PreparationLimits {
            source: fallout_data::nif_animation::Limits {
                input_bytes: bytes.len() - 1,
                ..exact.source
            },
            ..exact
        },
    ] {
        assert!(
            markers::PreparedSequence::prepare(
                &bytes,
                "one below",
                Sha256::digest(&bytes).into(),
                0,
                limits
            )
            .is_err()
        );
    }
    assert!(
        markers::PreparedSequence::prepare(&bytes, "foreign", [0; 32], 0, Default::default())
            .is_err()
    );
    for selected in [1, 99] {
        assert!(
            markers::PreparedSequence::prepare(
                &bytes,
                "wrong selection",
                Sha256::digest(&bytes).into(),
                selected,
                Default::default()
            )
            .is_err()
        );
    }
    for keys in [
        vec![(99., NULL)],
        vec![(f32::NAN, 1)],
        vec![(f32::INFINITY, 1)],
        vec![(2., 99)],
    ] {
        let bytes = container(&[
            ("NiControllerSequence", sequence(1)),
            ("NiTextKeyExtraData", text(&keys)),
        ]);
        assert!(
            markers::PreparedSequence::prepare(
                &bytes,
                "invalid selected key",
                Sha256::digest(&bytes).into(),
                0,
                Default::default()
            )
            .is_err()
        );
    }
    let bytes = container(&[
        ("NiControllerSequence", sequence(NULL)),
        ("NiTextKeyExtraData", text(&prepared_keys())),
    ]);
    assert!(
        markers::PreparedSequence::prepare(
            &bytes,
            "absent link",
            Sha256::digest(&bytes).into(),
            0,
            Default::default()
        )
        .is_err()
    );
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&prepared_keys())),
        ("NiTextKeyExtraData", text(&[(99., NULL)])),
    ]);
    assert_eq!(prepare_markers(&bytes).usage().validated_keys, 7);
}

#[test]
fn prepared_index_query_and_batch_exact_caps_are_atomic_and_permutation_preserving() {
    let bytes = prepared_bytes();
    let prepared = prepare_markers(&bytes);
    let request = indexed_request(&bytes, 0., 2.);
    let baseline = prepared.query(request, Default::default()).unwrap().usage;
    let exact = markers::QueryLimits {
        entries: 5,
        array_bytes: baseline.charged_bytes,
        work_units: baseline.work_units,
        max_combined_retained_bytes: baseline.combined_retained_bytes,
    };
    assert!(prepared.query(request, exact).is_ok());
    for limits in [
        markers::QueryLimits {
            entries: 4,
            ..exact
        },
        markers::QueryLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        markers::QueryLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        markers::QueryLimits {
            max_combined_retained_bytes: exact.max_combined_retained_bytes - 1,
            ..exact
        },
    ] {
        assert!(prepared.query(request, limits).is_err());
    }
    let requests = [
        request,
        indexed_request(&bytes, -0., 0.),
        indexed_request(&bytes, 4., 5.),
    ];
    let batch = prepared.query_many(&requests, Default::default()).unwrap();
    for (result, request) in batch.intervals.iter().zip(requests) {
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::to_value(prepared.query(request, Default::default()).unwrap()).unwrap()
        );
    }
    let perm = prepared
        .query_many(&[requests[2], requests[0], requests[1]], Default::default())
        .unwrap();
    for (result, ordinal) in perm.intervals.iter().zip([2, 0, 1]) {
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            serde_json::to_value(&batch.intervals[ordinal]).unwrap()
        );
    }
    let exact = markers::BatchLimits {
        intervals: 3,
        array_bytes: batch.charged_bytes,
        work_units: batch.work_units,
        max_combined_retained_bytes: batch.combined_retained_bytes,
        ..Default::default()
    };
    assert!(prepared.query_many(&requests, exact).is_ok());
    for limits in [
        markers::BatchLimits {
            intervals: 2,
            ..exact
        },
        markers::BatchLimits {
            array_bytes: exact.array_bytes - 1,
            ..exact
        },
        markers::BatchLimits {
            work_units: exact.work_units - 1,
            ..exact
        },
        markers::BatchLimits {
            max_combined_retained_bytes: exact.max_combined_retained_bytes - 1,
            ..exact
        },
        markers::BatchLimits {
            query: markers::QueryLimits {
                entries: 1,
                ..Default::default()
            },
            ..exact
        },
    ] {
        assert!(prepared.query_many(&requests, limits).is_err());
    }
    assert!(prepared.query_many(&[], Default::default()).is_err());
    assert!(
        prepared
            .query_many(
                &[
                    requests[0],
                    markers::IntervalRequest {
                        source_start: 10.,
                        ..requests[2]
                    }
                ],
                Default::default()
            )
            .is_err()
    );
    assert!(
        prepared
            .query_many(
                &[requests[2], requests[1], requests[0]],
                markers::BatchLimits {
                    query: markers::QueryLimits {
                        entries: 2,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(prepared.query(request, Default::default()).unwrap())
            .unwrap()
            .get("usage")
            .unwrap(),
        &serde_json::to_value(baseline).unwrap()
    );
}

#[test]
fn prepared_index_small_query_visits_boundaries_without_scanning_thousands_of_keys() {
    let source_keys = (0..4096).rev().map(|i| (i as f32, 1)).collect::<Vec<_>>();
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&source_keys)),
    ]);
    let prepared = prepare_markers(&bytes);
    assert_eq!(prepared.usage().validated_keys, 4096);
    for (start, end, ordinals) in [
        (9000., 9001., vec![]),
        (2000., 2000., vec![2095]),
        (1999., 2001., vec![2094, 2095, 2096]),
    ] {
        let value = prepared
            .query(
                indexed_request(&bytes, start, end),
                markers::QueryLimits {
                    work_units: 100,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(value.usage.boundary_probes <= 26);
        assert_eq!(value.usage.matching_index_visits, ordinals.len() * 2);
        assert_eq!(
            value
                .observation
                .entries
                .iter()
                .map(|e| e.source_key_ordinal)
                .collect::<Vec<_>>(),
            ordinals
        );
    }
    let bytes = container(&[
        ("NiControllerSequence", sequence(1)),
        ("NiTextKeyExtraData", text(&[])),
    ]);
    let value = prepare_markers(&bytes)
        .query(indexed_request(&bytes, -9., 9.), Default::default())
        .unwrap();
    assert!(value.observation.entries.is_empty());
    assert_eq!(value.usage.boundary_probes, 0);
}

#[test]
fn playback_uses_nonzero_source_start_and_emits_final_key_once_at_window_end() {
    let bytes = playback_bytes(100.0, 102.0);
    let prepared = prepare_markers(&bytes);
    let mut playback = markers::PlaybackController::new();
    let started = playback
        .start(
            &prepared,
            playback_request(&bytes, 100.0, 102.0, markers::RepeatPolicy::Once),
            Default::default(),
        )
        .unwrap();
    assert_eq!(started.source_time_bits, 100.0f64.to_bits());
    assert_eq!(started.events.len(), 1);
    assert_eq!(started.events[0].source_key_ordinal, 0);
    assert_eq!(started.events[0].boundary, markers::MarkerBoundary::Initial);
    assert!(!started.retail_behavior_verified);

    let middle = playback
        .advance_by_source_delta(started.generation, &prepared, 1.0, Default::default())
        .unwrap();
    assert_eq!(middle.source_time_after_bits, 101.0f64.to_bits());
    assert_eq!(middle.state, markers::PlaybackState::Playing);
    assert_eq!(middle.events.len(), 1);
    assert_eq!(middle.events[0].source_key_ordinal, 1);
    assert_eq!(middle.events[0].boundary, markers::MarkerBoundary::Interval);
    assert!(!middle.sequence_clock_fields_applied);

    let no_change = playback
        .advance_by_source_delta(started.generation, &prepared, 0.0, Default::default())
        .unwrap();
    assert!(no_change.events.is_empty());
    assert_eq!(
        no_change.source_time_before_bits,
        no_change.source_time_after_bits
    );

    let final_key = playback
        .advance_by_source_delta(started.generation, &prepared, 1.0, Default::default())
        .unwrap();
    assert_eq!(final_key.source_time_after_bits, 102.0f64.to_bits());
    assert_eq!(final_key.state, markers::PlaybackState::Completed);
    assert_eq!(final_key.events.len(), 1);
    assert_eq!(final_key.events[0].source_key_ordinal, 2);
    assert_eq!(
        final_key.events[0].boundary,
        markers::MarkerBoundary::WindowEnd
    );
    assert!(
        playback
            .advance_by_source_delta(started.generation, &prepared, 0.25, Default::default())
            .unwrap_err()
            .to_string()
            .contains("already complete")
    );
}

#[test]
fn playback_loop_crossings_are_partition_independent_and_emit_each_boundary_once() {
    let bytes = playback_bytes(100.0, 102.0);
    let prepared = prepare_markers(&bytes);
    let request = playback_request(&bytes, 100.0, 102.0, markers::RepeatPolicy::Loop);
    let mut one_step = markers::PlaybackController::new();
    let started = one_step
        .start(&prepared, request, Default::default())
        .unwrap();
    let mut one_step_signature = started
        .events
        .iter()
        .map(|event| (event.cycle_index, event.source_key_ordinal, event.boundary))
        .collect::<Vec<_>>();
    let result = one_step
        .advance_by_source_delta(started.generation, &prepared, 4.0, Default::default())
        .unwrap();
    assert_eq!(result.cycles_crossed, 2);
    assert_eq!(result.cycles_completed, 2);
    assert_eq!(result.state, markers::PlaybackState::Playing);
    assert_eq!(result.source_time_after_bits, 100.0f64.to_bits());
    one_step_signature.extend(
        result
            .events
            .iter()
            .map(|event| (event.cycle_index, event.source_key_ordinal, event.boundary)),
    );

    let mut split_steps = markers::PlaybackController::new();
    let split_start = split_steps
        .start(&prepared, request, Default::default())
        .unwrap();
    let mut split_signature = split_start
        .events
        .iter()
        .map(|event| (event.cycle_index, event.source_key_ordinal, event.boundary))
        .collect::<Vec<_>>();
    for _ in 0..4 {
        let step = split_steps
            .advance_by_source_delta(split_start.generation, &prepared, 1.0, Default::default())
            .unwrap();
        split_signature.extend(
            step.events
                .iter()
                .map(|event| (event.cycle_index, event.source_key_ordinal, event.boundary)),
        );
    }
    assert_eq!(split_signature, one_step_signature);
    assert_eq!(
        split_steps
            .advance_by_source_delta(split_start.generation, &prepared, 0.0, Default::default())
            .unwrap()
            .events
            .len(),
        0
    );
}

#[test]
fn playback_replacement_cancellation_and_stale_generations_are_explicit() {
    let bytes = playback_bytes(100.0, 102.0);
    let prepared = prepare_markers(&bytes);
    let mut playback = markers::PlaybackController::new();
    let request = playback_request(&bytes, 100.0, 102.0, markers::RepeatPolicy::Once);
    let first = playback
        .start(&prepared, request, Default::default())
        .unwrap();
    playback
        .advance_by_source_delta(first.generation, &prepared, 0.5, Default::default())
        .unwrap();

    let mut wrong_identity = request;
    wrong_identity.expected_sha256[0] ^= 1;
    assert!(
        playback
            .start(&prepared, wrong_identity, Default::default())
            .is_err()
    );
    assert_eq!(playback.active_generation(), Some(first.generation));

    let replacement = playback
        .start(&prepared, request, Default::default())
        .unwrap();
    assert_eq!(replacement.interrupted_generation, Some(first.generation));
    assert!(
        playback
            .advance_by_source_delta(first.generation, &prepared, 1.0, Default::default())
            .unwrap_err()
            .to_string()
            .contains("stale request generation")
    );
    let step = playback
        .advance_by_source_delta(replacement.generation, &prepared, 1.5, Default::default())
        .unwrap();
    assert_eq!(
        step.events
            .iter()
            .map(|event| event.source_key_ordinal)
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(step.state, markers::PlaybackState::Playing);
    assert!(playback.cancel(first.generation).is_err());
    assert_eq!(playback.active_generation(), Some(replacement.generation));
    let cancelled = playback.cancel(replacement.generation).unwrap();
    assert_eq!(cancelled.source_time_bits, 101.5f64.to_bits());
    assert_eq!(
        cancelled.state_before_cancel,
        markers::PlaybackState::Playing
    );
    assert_eq!(playback.active_generation(), None);
    assert!(
        playback
            .advance_by_source_delta(replacement.generation, &prepared, 0.5, Default::default())
            .is_err()
    );
}

#[test]
fn playback_budget_refusal_does_not_advance_or_partially_publish_markers() {
    let bytes = playback_bytes(100.0, 102.0);
    let prepared = prepare_markers(&bytes);
    let mut playback = markers::PlaybackController::new();
    let mut request = playback_request(&bytes, 100.0, 102.0, markers::RepeatPolicy::Loop);
    request.initial_boundary = markers::BoundaryDelivery::Skip;
    request.loop_start_boundary = markers::BoundaryDelivery::Skip;
    let started = playback
        .start(&prepared, request, Default::default())
        .unwrap();

    let no_event_budget = markers::AdvanceLimits {
        max_events: 0,
        ..Default::default()
    };
    assert!(
        playback
            .advance_by_source_delta(started.generation, &prepared, 1.0, no_event_budget)
            .unwrap_err()
            .to_string()
            .contains("event count budget")
    );
    assert_eq!(playback.active_generation(), Some(started.generation));

    let one_loop = markers::AdvanceLimits {
        max_loop_crossings: 1,
        ..Default::default()
    };
    assert!(
        playback
            .advance_by_source_delta(started.generation, &prepared, 8.0, one_loop)
            .unwrap_err()
            .to_string()
            .contains("loop crossing budget")
    );
    assert_eq!(playback.active_generation(), Some(started.generation));
    let retry = playback
        .advance_by_source_delta(started.generation, &prepared, 1.0, Default::default())
        .unwrap();
    assert_eq!(retry.source_time_after_bits, 101.0f64.to_bits());
    assert_eq!(retry.events.len(), 1);
    assert_eq!(retry.events[0].source_key_ordinal, 1);
}
