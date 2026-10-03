//! Independent authored source words. Opaque referenced payloads deliberately
//! remain undecoded, so their kind names cannot certify runtime readiness.
use fallout_data::nif_animation::{
    self, Data, Dependency, Diagnostic, Limits, LinkStatus, NoteLinks,
};

const NULL: u32 = u32::MAX;
const STREAMS: [u32; 12] = [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34];
fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn shorts(out: &mut Vec<u8>, values: &[u16]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn set(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn strings() -> Vec<Vec<u8>> {
    [
        b"clip".as_slice(),
        b"Bip01",
        b"Property",
        b"NiTransformController",
        b"controller\nid",
        b"interp",
        b"accum",
        b"\x80\xff",
        b"a\0b\0\0",
        b"\r\n\"\\",
    ]
    .into_iter()
    .map(Vec::from)
    .collect()
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32, strings: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, stream]);
    out.extend([0; 3]);
    let mut types = Vec::new();
    for (name, _) in blocks {
        if !types.contains(name) {
            types.push(*name);
        }
    }
    shorts(&mut out, &[types.len() as u16]);
    for name in &types {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        shorts(
            &mut out,
            &[types.iter().position(|n| n == name).unwrap() as u16],
        );
    }
    for (_, payload) in blocks {
        words(&mut out, &[payload.len() as u32]);
    }
    words(
        &mut out,
        &[
            strings.len() as u32,
            strings.iter().map(Vec::len).max().unwrap_or(0) as u32,
        ],
    );
    for string in strings {
        words(&mut out, &[string.len() as u32]);
        out.extend(string);
    }
    words(&mut out, &[0]);
    for (_, payload) in blocks {
        out.extend(payload);
    }
    words(&mut out, &[1, 1]);
    out
}
fn controller() -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[NULL]);
    shorts(&mut out, &[0xFFFF]);
    words(&mut out, &[0x8000_0000, 0x7F7F_FFFF, 0xFF7F_FFFF, 1, 6, 2]);
    out
}
fn sequence(stream: u32, empty: bool) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0, if empty { 0 } else { 2 }, 0x8765_4321]);
    if !empty {
        for (priority, property) in [(255, 2), (3, NULL)] {
            words(&mut out, &[2, 0]);
            out.push(priority);
            words(&mut out, &[1, property, 3, 4, 5]);
        }
    }
    words(
        &mut out,
        &[
            (-0.25f32).to_bits(),
            3,
            2,
            1.25f32.to_bits(),
            (-0.5f32).to_bits(),
            4f32.to_bits(),
            NULL,
            6,
        ],
    );
    if (24..=28).contains(&stream) {
        words(&mut out, &[if empty { NULL } else { 4 }]);
    } else if stream > 28 {
        shorts(&mut out, &[if empty { 0 } else { 3 }]);
        if !empty {
            words(&mut out, &[4, NULL, 4]);
        }
    }
    out
}
fn transform() -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0x8000_0000, 0x7F7F_FFFF, 0xFF7F_FFFF]);
    for v in [2f32, -3., 4., -5.] {
        words(&mut out, &[v.to_bits()]);
    }
    words(&mut out, &[1, 5]);
    out
}
fn text_keys(empty: bool) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[0, if empty { 0 } else { 3 }]);
    if !empty {
        for (time, value) in [(2f32, 8), (-0., 9), (2., 7)] {
            words(&mut out, &[time.to_bits(), value]);
        }
    }
    out
}
fn fixture(stream: u32, empty: bool) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("NiTransformController", controller()),
        ("NiControllerSequence", sequence(stream, empty)),
        ("NiTransformInterpolator", transform()),
        ("NiTextKeyExtraData", text_keys(empty)),
        ("BSAnimNotes", Vec::new()),
        ("NiTransformData", Vec::new()),
        ("NiNode", Vec::new()),
        ("FutureAnimationSource", b"opaque".to_vec()),
    ]
}
fn failure(blocks: &[(&str, Vec<u8>)], expected: &str) {
    let error = nif_animation::decode(&container(blocks, 34, &strings()), "authored animation")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(expected),
        "expected {expected:?}, got {error:?}"
    );
}

#[test]
fn four_exact_inherited_layouts_preserve_bits_order_and_all_note_branches() {
    for stream in STREAMS {
        let (index, source) = nif_animation::decode(
            &container(&fixture(stream, false), stream, &strings()),
            "authored",
        )
        .unwrap();
        assert_eq!(source.blocks.len(), 4);
        for b in &source.blocks {
            let span = &index.blocks[b.block as usize];
            assert_eq!((b.offset, b.bytes), (span.offset, span.bytes));
            assert_eq!(b.sha256.len(), 64);
        }
        let Data::TransformController { controller: c } = &source.blocks[0].data else {
            panic!()
        };
        assert_eq!(
            (
                c.flags,
                c.frequency_bits,
                c.phase_bits,
                c.start_bits,
                c.stop_bits
            ),
            (65535, 0x8000_0000, 0x7F7F_FFFF, 0xFF7F_FFFF, 1)
        );
        assert_eq!(
            (c.next_controller, c.target, c.interpolator),
            (None, Some(6), Some(2))
        );
        let Data::ControllerSequence { sequence: s } = &source.blocks[1].data else {
            panic!()
        };
        assert_eq!(
            (s.name, s.declared_controlled_blocks, s.array_grow_by),
            (Some(0), 2, 0x8765_4321)
        );
        assert_eq!(
            s.controlled_blocks
                .iter()
                .map(|p| (p.interpolator, p.controller, p.priority, p.property_type))
                .collect::<Vec<_>>(),
            [
                (Some(2), Some(0), 255, Some(2)),
                (Some(2), Some(0), 3, None)
            ]
        );
        assert_eq!(
            (
                s.weight_bits,
                s.text_keys,
                s.cycle_type,
                s.manager,
                s.accum_root_name
            ),
            ((-0.25f32).to_bits(), Some(3), 2, None, Some(6))
        );
        match (&s.notes, stream) {
            (NoteLinks::Absent, 14 | 21) => {}
            (NoteLinks::Single { target }, 24..=28) => assert_eq!(*target, Some(4)),
            (
                NoteLinks::Array {
                    declared_count,
                    targets,
                },
                30..=34,
            ) => {
                assert_eq!(*declared_count, 3);
                assert_eq!(targets, &[Some(4), None, Some(4)]);
            }
            _ => panic!("wrong note branch"),
        }
        let Data::TransformInterpolator { interpolator: t } = &source.blocks[2].data else {
            panic!()
        };
        assert_eq!(t.translation_bits, [0x8000_0000, 0x7F7F_FFFF, 0xFF7F_FFFF]);
        assert_eq!(t.rotation_wxyz_bits, [2f32, -3., 4., -5.].map(f32::to_bits));
        assert_eq!((t.scale_bits, t.data), (1, Some(5)));
        let Data::TextKeyExtraData { text_keys: t } = &source.blocks[3].data else {
            panic!()
        };
        assert_eq!((t.name, t.declared_keys), (Some(0), 3));
        assert_eq!(
            t.keys
                .iter()
                .map(|k| (k.time_bits, k.value))
                .collect::<Vec<_>>(),
            [
                (2f32.to_bits(), Some(8)),
                (0x8000_0000, Some(9)),
                (2f32.to_bits(), Some(7))
            ]
        );
        assert_eq!(index.strings, strings());
        assert!(!source.runtime_ready);
    }
}

#[test]
fn raw_strings_are_arbitrary_bytes_with_no_encoding_or_trailing_nul_repair() {
    let mut raw = strings();
    raw.extend([Vec::new(), vec![0], b"two\0\0".to_vec()]);
    let (index, _) =
        nif_animation::decode(&container(&fixture(34, false), 34, &raw), "raw strings").unwrap();
    assert_eq!(index.strings, raw);
}

#[test]
fn unknown_classes_and_known_unparsed_payloads_are_distinct_dependencies() {
    let mut blocks = fixture(34, false);
    set(&mut blocks[0].1, 0, 7); // Unknown next controller, never guessed by prefix.
    blocks[7].0 = "NiTransformControllerFuture";
    let (_, source) =
        nif_animation::decode(&container(&blocks, 34, &strings()), "unknown").unwrap();
    assert!(source.dependencies.iter().any(|d| matches!(
        d,
        Dependency::Link {
            target: 7,
            status: LinkStatus::UnknownClass,
            ..
        }
    )));
    assert!(source.dependencies.iter().any(|d| matches!(
        d,
        Dependency::Link {
            target: 5,
            status: LinkStatus::UndecodedPayload,
            ..
        }
    )));
    assert_eq!(
        source
            .dependencies
            .iter()
            .filter(|d| matches!(d, Dependency::ExternalBinding { .. }))
            .count(),
        2
    );
    blocks[2].0 = "BSRotAccumTransfInterpolator";
    assert!(nif_animation::decode(&container(&blocks, 34, &strings()), "known subtype").is_ok());
}

#[test]
fn all_typed_roles_reject_known_wrong_families() {
    for (block, offset, target) in [
        (0, 0, 3),
        (0, 22, 2),
        (0, 26, 6),
        (1, 12, 6),
        (1, 16, 2),
        (1, 74, 6),
        (1, 94, 6),
        (1, 104, 6),
        (2, 32, 6),
    ] {
        let mut blocks = fixture(34, false);
        set(&mut blocks[block].1, offset, target);
        failure(&blocks, "wrong target kind");
    }
}

#[test]
fn null_links_and_self_controller_links_are_retained_without_runtime_policy() {
    let mut blocks = fixture(34, false);
    for (block, offsets) in [
        (0, vec![0, 22, 26]),
        (1, vec![12, 16, 41, 45, 74, 94, 104, 108, 112]),
        (2, vec![32]),
    ] {
        for offset in offsets {
            set(&mut blocks[block].1, offset, NULL);
        }
    }
    let (_, source) = nif_animation::decode(&container(&blocks, 34, &strings()), "null").unwrap();
    assert!(
        !source
            .dependencies
            .iter()
            .any(|d| matches!(d, Dependency::Link { .. }))
    );
    set(&mut blocks[0].1, 0, 0);
    assert!(nif_animation::decode(&container(&blocks, 34, &strings()), "self source link").is_ok());
}

#[test]
fn empty_arrays_keep_absent_present_null_and_present_empty_distinct() {
    for stream in STREAMS {
        let (_, source) = nif_animation::decode(
            &container(&fixture(stream, true), stream, &strings()),
            "empty",
        )
        .unwrap();
        let Data::ControllerSequence { sequence: s } = &source.blocks[1].data else {
            panic!()
        };
        assert!(s.controlled_blocks.is_empty());
        match (&s.notes, stream) {
            (NoteLinks::Absent, 14 | 21) => {}
            (NoteLinks::Single { target: None }, 24..=28) => {}
            (
                NoteLinks::Array {
                    declared_count: 0,
                    targets,
                },
                30..=34,
            ) => assert!(targets.is_empty()),
            _ => panic!("empty note layout lost"),
        }
    }
}

#[test]
fn unknown_cycle_words_are_diagnosed_and_never_normalized() {
    let mut blocks = fixture(34, false);
    set(&mut blocks[1].1, 78, NULL);
    let (_, source) =
        nif_animation::decode(&container(&blocks, 34, &strings()), "cycle tag").unwrap();
    assert!(matches!(
        source.diagnostics.as_slice(),
        [Diagnostic::UnverifiedCycleType {
            sequence: 1,
            value: NULL
        }]
    ));
    let Data::ControllerSequence { sequence: s } = &source.blocks[1].data else {
        panic!()
    };
    assert_eq!(s.cycle_type, NULL);
}

#[test]
fn every_selected_truncated_prefix_and_surplus_is_rejected() {
    for stream in [14, 24, 34] {
        let original = fixture(stream, false);
        for block in 0..4 {
            for length in 0..original[block].1.len() {
                let mut blocks = original.clone();
                blocks[block].1.truncate(length);
                assert!(
                    nif_animation::decode(&container(&blocks, stream, &strings()), "truncated")
                        .is_err(),
                    "stream{stream} block{block} length{length}"
                );
            }
            let mut blocks = original.clone();
            blocks[block].1.push(0);
            assert!(
                nif_animation::decode(&container(&blocks, stream, &strings()), "surplus").is_err()
            );
        }
    }
}

#[test]
fn block_and_string_link_ranges_are_checked_even_without_payload_support() {
    for (block, offset) in [
        (0, 0),
        (0, 22),
        (0, 26),
        (1, 12),
        (1, 16),
        (1, 74),
        (1, 94),
        (1, 104),
        (2, 32),
    ] {
        let mut blocks = fixture(34, false);
        set(&mut blocks[block].1, offset, 8);
        failure(&blocks, "block index out of range");
    }
    for (block, offset) in [
        (1, 0),
        (1, 21),
        (1, 25),
        (1, 29),
        (1, 33),
        (1, 37),
        (1, 98),
        (3, 0),
        (3, 12),
        (3, 20),
        (3, 28),
    ] {
        let mut blocks = fixture(34, false);
        set(&mut blocks[block].1, offset, 10);
        failure(&blocks, "string index out of range");
    }
}

#[test]
fn all_source_float_fields_reject_nonfinite_words() {
    let fields = [
        (0, vec![6, 10, 14, 18]),
        (1, vec![70, 82, 86, 90]),
        (2, (0..32).step_by(4).collect()),
        (3, vec![8, 16, 24]),
    ];
    for (block, offsets) in fields {
        for offset in offsets {
            for bits in [0x7F80_0000, 0xFF80_0000, 0x7FC0_0001] {
                let mut blocks = fixture(34, false);
                set(&mut blocks[block].1, offset, bits);
                failure(&blocks, "nonfinite");
            }
        }
    }
}

#[test]
fn malicious_counts_fail_before_source_array_allocation() {
    for block in [1, 3] {
        let mut blocks = fixture(34, false);
        set(&mut blocks[block].1, 4, NULL);
        failure(&blocks, "budget");
    }
    let mut blocks = fixture(34, false);
    blocks[1].1[102..104].copy_from_slice(&u16::MAX.to_le_bytes());
    failure(&blocks, "budget");
}

#[test]
fn input_block_storage_and_reference_budgets_have_exact_boundaries() {
    let bytes = container(&fixture(34, false), 34, &strings());
    let (_, source) = nif_animation::decode(&bytes, "measure").unwrap();
    let limits = Limits {
        input_bytes: bytes.len(),
        blocks: 8,
        array_bytes: source.retained_bytes,
        reference_checks: 38,
    };
    assert!(nif_animation::decode_with_limits(&bytes, "exact", limits).is_ok());
    for smaller in [
        Limits {
            input_bytes: bytes.len() - 1,
            ..limits
        },
        Limits {
            blocks: 7,
            ..limits
        },
        Limits {
            array_bytes: limits.array_bytes - 1,
            ..limits
        },
        Limits {
            reference_checks: 37,
            ..limits
        },
    ] {
        assert!(nif_animation::decode_with_limits(&bytes, "one short", smaller).is_err());
    }
    let empty = container(&fixture(34, true), 34, &strings());
    assert!(
        nif_animation::decode_with_limits(
            &empty,
            "empty products exact",
            Limits {
                reference_checks: 16,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(
        nif_animation::decode_with_limits(
            &empty,
            "empty products short",
            Limits {
                reference_checks: 15,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn temporary_index_vectors_are_bounded_but_not_reported_as_retained() {
    let mut bytes = container(&[("NiNode", Vec::new()), ("Opaque", Vec::new())], 34, &[]);
    // The container helper's footer points at block1, an opaque valid index.
    let (_, source) = nif_animation::decode(&bytes, "opaque index").unwrap();
    assert!(source.blocks.is_empty());
    let scratch = 2 * (std::mem::size_of::<u16>() + std::mem::size_of::<usize>()) + "Opaque".len();
    let limits = Limits {
        array_bytes: source.retained_bytes + scratch,
        reference_checks: 0,
        ..Default::default()
    };
    assert!(nif_animation::decode_with_limits(&bytes, "scratch exact", limits).is_ok());
    assert!(
        nif_animation::decode_with_limits(
            &bytes,
            "scratch short",
            Limits {
                array_bytes: limits.array_bytes - 1,
                ..limits
            }
        )
        .is_err()
    );
    // No header-sized allocation bomb can enter nif::inspect's index vectors.
    let count_offset = b"Gamebryo File Format, Version 20.2.0.7\n".len() + 9;
    set(&mut bytes, count_offset, 100_001);
    assert!(nif_animation::decode(&bytes, "index count bomb").is_err());
}

#[test]
fn unsupported_container_tuples_have_no_layout_fallback() {
    let original = container(&fixture(34, false), 34, &strings());
    let base = b"Gamebryo File Format, Version 20.2.0.7\n".len();
    for (offset, value) in [(base, 0x1402_0008), (base + 5, 12), (base + 13, 29)] {
        let mut bytes = original.clone();
        set(&mut bytes, offset, value);
        assert!(nif_animation::decode(&bytes, "unsupported").is_err());
    }
    let mut bytes = original;
    bytes[base + 4] = 0;
    assert!(nif_animation::decode(&bytes, "endian").is_err());
}
