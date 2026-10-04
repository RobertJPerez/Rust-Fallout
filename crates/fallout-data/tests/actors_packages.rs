use fallout_data::{
    actors::packages::{Catalogue, GeneralTail, Limits, Value},
    plugin, record_metadata,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};

fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn disk(raw: u32, flags: u32, version: u16, data: &[u8]) -> Vec<u8> {
    let body = if flags & plugin::COMPRESSED != 0 {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        [
            &(data.len() as u32).to_le_bytes()[..],
            &encoder.finish().unwrap(),
        ]
        .concat()
    } else {
        data.to_vec()
    };
    [
        b"PACK".as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0xA5, 0x5A],
        &body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    [
        b"TES4".as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 12],
        &15u16.to_le_bytes(),
        &[0; 2],
        &body,
    ]
    .concat()
}
const LEGACY: [u8; 8] = [1, 0, 0, 0x80, 255, 0xCD, 0xFF, 0xFF];
const MODERN: [u8; 12] = [
    0xFF, 0xFF, 0xFF, 0xFF, 254, 0xA5, 1, 0x80, 0xDC, 0xFE, 0xA5, 0xCD,
];
const SCHEDULE: [u8; 8] = [0x80, 0x7F, 0xFF, 0x81, 0xFF, 0xFF, 0xFF, 0xFF];
fn complete(general: &[u8]) -> Vec<u8> {
    [field(b"PKDT", general), field(b"PSDT", &SCHEDULE)].concat()
}
fn source(path: &Path, order: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn single(path: &Path, version: u16, body: &[u8]) -> RecordStore {
    fs::write(
        path.join("FalloutNV.esm"),
        [header(&[]), disk(0x100, 0, version, body)].concat(),
    )
    .unwrap();
    source(path, &["FalloutNV.esm"])
}

#[test]
fn both_general_layouts_keep_raw_words_absent_tail_signed_schedule_and_provenance() {
    let directory = tempfile::tempdir().unwrap();
    for general in [LEGACY.as_slice(), MODERN.as_slice()] {
        let body = complete(general);
        let mut store = single(directory.path(), 15, &body);
        let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
        let (key, definition) = catalogue.iter().next().unwrap();
        assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
        let expected = if general.len() == 8 {
            Value::General {
                flags: 0x8000_0001,
                package_type: 255,
                unused: 0xCD,
                behavior_flags: u16::MAX,
                tail: None,
            }
        } else {
            Value::General {
                flags: u32::MAX,
                package_type: 254,
                unused: 0xA5,
                behavior_flags: 0x8001,
                tail: Some(GeneralTail {
                    type_specific_flags: 0xFEDC,
                    unused: [0xA5, 0xCD],
                }),
            }
        };
        assert_eq!(definition.fields[0].value, expected);
        assert_eq!(
            definition.fields[1].value,
            Value::Schedule {
                month: -128,
                weekday: 127,
                date: -1,
                hour: -127,
                duration: u32::MAX
            }
        );
        assert_eq!(definition.fields[0].decoded_offset, 0);
        assert_eq!(
            definition.fields[1].decoded_offset,
            (general.len() + 6) as u32
        );
        assert_eq!(definition.record().unwrap().payload, body);
        assert_eq!(definition.header.trailing_bytes, [0xA5, 0x5A]);
        assert_eq!(catalogue.counts().scalar_fields, 2);
        assert!(definition.findings.is_empty());
        assert_eq!(
            catalogue.sources()[0].source_sha256,
            definition.source.sha256
        );
        assert_eq!(
            catalogue.winning_content_sha256(),
            record_metadata::inspect(&store)
                .unwrap()
                .winning_definitions_sha256
        );
    }
}

#[test]
fn order_duplicate_occurrences_extended_opaque_fields_and_required_absence_survive() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"PSDT", &SCHEDULE),
        field(b"XXXX", &6u32.to_le_bytes()),
        b"UNKN\0\0opaque".to_vec(),
        field(b"PKDT", &LEGACY),
        field(b"PSDT", &[0; 8]),
        field(b"PKDT", &MODERN),
        field(b"CTDA", &[0; 20]),
    ]
    .concat();
    let mut store = single(directory.path(), 15, &body);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(
        definition
            .fields
            .iter()
            .map(|field| field.decoded_offset)
            .collect::<Vec<_>>(),
        [0, 24, 36, 50, 64, 82]
    );
    assert_eq!(definition.fields[1].kind, *b"UNKN");
    assert_eq!(definition.fields[1].bytes, 6);
    assert_eq!(definition.fields[1].value, Value::Opaque);
    assert_eq!(definition.fields[5].kind, *b"CTDA");
    assert_eq!(definition.fields[5].value, Value::Opaque);
    assert_eq!(definition.record().unwrap().payload, body);
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| (finding.field_decoded_offset, finding.code))
            .collect::<Vec<_>>(),
        [
            (Some(50), "multiple_package_schedule_fields"),
            (Some(64), "multiple_package_general_fields")
        ]
    );
    drop(store);
    for (body, expected) in [
        (
            Vec::new(),
            vec![
                "missing_package_general_field",
                "missing_package_schedule_field",
            ],
        ),
        (
            field(b"PKDT", &LEGACY),
            vec!["missing_package_schedule_field"],
        ),
        (
            field(b"PSDT", &SCHEDULE),
            vec!["missing_package_general_field"],
        ),
    ] {
        let mut store = single(directory.path(), 15, &body);
        let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
        let definition = catalogue.iter().next().unwrap().1;
        assert_eq!(
            definition
                .findings
                .iter()
                .map(|finding| finding.code)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(
            definition
                .findings
                .iter()
                .all(|finding| finding.field_decoded_offset.is_none())
        );
        assert_eq!(definition.fields.len(), usize::from(!body.is_empty()));
    }
}

#[test]
fn observed_versions_admit_source_fields_unsupported_versions_and_widths_are_contextual() {
    let directory = tempfile::tempdir().unwrap();
    for version in [1, 2, 3, 9, 10, 11, 13, 14, 15] {
        let body = complete(if version < 9 { &LEGACY } else { &MODERN });
        let mut store = single(directory.path(), version, &body);
        let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
        assert_eq!(catalogue.iter().next().unwrap().1.header.version, version);
    }
    for version in [0, 4, 8, 12, 16] {
        let mut store = single(directory.path(), version, &complete(&MODERN));
        let error = Catalogue::load(&mut store, Limits::default())
            .err()
            .unwrap()
            .to_string();
        assert!(
            error.contains(&format!("PACK record version {version}"))
                && error.contains("FalloutNV.esm:0x"),
            "{error}"
        );
    }
    for (kind, lengths, expected) in [
        (b"PKDT", vec![0, 7, 9, 10, 11, 13], "needs 8 or 12 bytes"),
        (b"PSDT", vec![0, 7, 9], "needs 8 bytes"),
    ] {
        for length in lengths {
            let mut store = single(directory.path(), 15, &field(kind, &vec![0; length]));
            let error = Catalogue::load(&mut store, Limits::default())
                .err()
                .unwrap()
                .to_string();
            assert!(
                error.contains(expected)
                    && error.contains("decoded +0x0")
                    && error.contains(&format!("found {length}")),
                "{error}"
            );
        }
    }
    for body in [
        b"PKD".to_vec(),
        b"PKDT\x0c\0short".to_vec(),
        field(b"XXXX", &12u32.to_le_bytes()),
        [
            field(b"XXXX", &12u32.to_le_bytes()),
            field(b"XXXX", &12u32.to_le_bytes()),
        ]
        .concat(),
    ] {
        let mut store = single(directory.path(), 15, &body);
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
}

#[test]
fn candidate_stored_decoded_aggregate_and_field_budgets_bound_admission() {
    let directory = tempfile::tempdir().unwrap();
    let body = complete(&MODERN);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(0x100, plugin::COMPRESSED, 15, &body),
            disk(0x101, 0, 15, &body),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    for (limits, diagnostic) in [
        (
            Limits {
                max_records: 0,
                ..Default::default()
            },
            "package record budget",
        ),
        (
            Limits {
                max_records: 1,
                ..Default::default()
            },
            "package record budget",
        ),
        (
            Limits {
                max_record_bytes: 0,
                ..Default::default()
            },
            "budget",
        ),
        (
            Limits {
                max_decoded_bytes: body.len() * 2 - 1,
                ..Default::default()
            },
            "budget",
        ),
        (
            Limits {
                max_fields: 0,
                ..Default::default()
            },
            "package field budget",
        ),
        (
            Limits {
                max_fields: 3,
                ..Default::default()
            },
            "package field budget",
        ),
    ] {
        let error = Catalogue::load(&mut store, limits)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(diagnostic), "{error}");
    }
    let catalogue = Catalogue::load(
        &mut store,
        Limits {
            max_records: 2,
            max_record_bytes: body.len() * 2,
            max_decoded_bytes: body.len() * 2,
            max_fields: 4,
        },
    )
    .unwrap();
    assert_eq!(catalogue.counts().records, 2);
    assert_eq!(catalogue.counts().fields, 4);
    assert_eq!(catalogue.counts().decoded_bytes, body.len() * 2);
    drop(store);
    let mut store = single(directory.path(), 15, &field(b"PKDT", &[0; 9]));
    let error = Catalogue::load(
        &mut store,
        Limits {
            max_fields: 0,
            ..Default::default()
        },
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("package field budget"), "{error}");
}

#[test]
fn hostile_compressed_length_fails_before_inflate_and_deleted_bodies_stay_unread() {
    let directory = tempfile::tempdir().unwrap();
    // A valid empty zlib stream carries a hostile declared decoded length.
    let hostile = [
        &(64u32 * 1024 * 1024 + 1).to_le_bytes()[..],
        &[0x78, 0x9c, 3, 0, 0, 0, 0, 1],
    ]
    .concat();
    let mut bytes = disk(0x100, 0, 15, &hostile);
    bytes[8..12].copy_from_slice(&plugin::COMPRESSED.to_le_bytes());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), bytes].concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let error = Catalogue::load(&mut store, Limits::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("decompression budget"), "{error}");
    drop(store);
    let mut bytes = disk(0x100, plugin::DELETED, 99, b"hostile unread bytes");
    bytes[8..12].copy_from_slice(&(plugin::DELETED | plugin::COMPRESSED).to_le_bytes());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), bytes].concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let catalogue = Catalogue::load(
        &mut store,
        Limits {
            max_record_bytes: 0,
            max_decoded_bytes: 0,
            max_fields: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert!(
        definition.deleted
            && definition.record().is_none()
            && definition.fields.is_empty()
            && definition.findings.is_empty()
    );
    assert_eq!(definition.header.version, 99);
    assert_eq!(catalogue.counts().decoded_bytes, 0);
}

#[test]
fn winning_override_tombstone_and_self_identity_survive_cold_warm_reordered_caches() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(0x100, 0, 1, &complete(&LEGACY)),
            disk(0x101, 0, 15, &complete(&MODERN)),
        ]
        .concat(),
    )
    .unwrap();
    for (name, general) in [("A.esm", LEGACY.as_slice()), ("B.esm", MODERN.as_slice())] {
        let mut bytes = [
            header(&["FalloutNV.esm"]),
            disk(0x0100_0100, plugin::COMPRESSED, 15, &complete(general)),
        ]
        .concat();
        if name == "A.esm" {
            bytes.extend(disk(0x100, 0, 15, &complete(&MODERN)));
            bytes.extend(disk(
                0x101,
                plugin::DELETED | plugin::COMPRESSED,
                99,
                b"unread hostile body",
            ));
        }
        fs::write(directory.path().join(name), bytes).unwrap();
    }
    let cache = tempfile::tempdir().unwrap();
    let mut observed = Vec::new();
    let mut digests = Vec::new();
    for (phase, order) in [
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "B.esm", "A.esm"],
    ]
    .into_iter()
    .enumerate()
    {
        let mut store = RecordStore::open_nv_headers_cached(
            directory.path(),
            &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
            plugin::Limits::default(),
            cache.path(),
        )
        .unwrap();
        assert!(
            store
                .index_cache_report()
                .unwrap()
                .plugins
                .iter()
                .all(|receipt| receipt.reused == (phase != 0))
        );
        let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
        assert_eq!(catalogue.counts().records, 4);
        assert_eq!(catalogue.counts().deleted_records, 1);
        for (key, definition) in catalogue.iter() {
            if definition.deleted {
                assert_eq!(key.local_id, 0x101);
                assert_eq!(definition.header.version, 99);
                assert!(
                    definition.record().is_none()
                        && definition.fields.is_empty()
                        && definition.findings.is_empty()
                );
                assert!(definition.source.decoded_record_sha256.is_none());
            } else if key.origin_plugin == "falloutnv.esm" {
                assert_eq!(definition.source.plugin, "A.esm");
                assert_eq!(definition.fields[0].bytes, 12);
            } else {
                assert_eq!(key.origin_plugin, definition.source.plugin.to_lowercase());
                assert_eq!(definition.header.form_id, 0x0100_0100);
            }
        }
        digests.push(catalogue.winning_content_sha256().to_owned());
        observed.push(
            serde_json::to_value(
                catalogue
                    .iter()
                    .map(|(_, definition)| definition)
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        );
    }
    assert_eq!(observed[0], observed[1]);
    assert_eq!(observed[0], observed[2]);
    assert_eq!(digests[0], digests[1]);
    assert_eq!(digests[0], digests[2]);
}
