use fallout_data::{
    actors::races::{Catalogue, Limits, SkillBoost, Value},
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
        b"RACE".as_slice(),
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
fn data() -> Vec<u8> {
    let boosts = [
        (-128i8, 127i8),
        (-1, 0),
        (37, -5),
        (37, 127),
        (0, -128),
        (126, 1),
        (-127, -1),
    ];
    let mut result: Vec<_> = boosts
        .into_iter()
        .flat_map(|(skill, boost)| [skill as u8, boost as u8])
        .collect();
    result.extend([0xA5, 0xCD]);
    for word in [0x8000_0000u32, 0x7f80_0000, 1, 0x7fc0_1234, 0xffff_ffff] {
        result.extend(word.to_le_bytes());
    }
    result
}
fn complete(marker: u32) -> Vec<u8> {
    [
        field(b"DATA", &data()),
        field(b"PNAM", &marker.to_le_bytes()),
        field(b"UNAM", &0xff80_0000u32.to_le_bytes()),
    ]
    .concat()
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
fn race_bytes_keep_signed_skill_order_unused_float_payloads_and_header_provenance() {
    let directory = tempfile::tempdir().unwrap();
    let body = complete(0x3f80_0001);
    let mut store = single(directory.path(), 15, &body);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let (key, definition) = catalogue.iter().next().unwrap();
    assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
    assert_eq!(
        definition.fields[0].value,
        Value::RaceData {
            skill_boosts: [
                SkillBoost {
                    skill: -128,
                    boost: 127
                },
                SkillBoost {
                    skill: -1,
                    boost: 0
                },
                SkillBoost {
                    skill: 37,
                    boost: -5
                },
                SkillBoost {
                    skill: 37,
                    boost: 127
                },
                SkillBoost {
                    skill: 0,
                    boost: -128
                },
                SkillBoost {
                    skill: 126,
                    boost: 1
                },
                SkillBoost {
                    skill: -127,
                    boost: -1
                }
            ],
            unused: [0xA5, 0xCD],
            height_bits: [0x8000_0000, 0x7f80_0000],
            weight_bits: [1, 0x7fc0_1234],
            flags: u32::MAX,
        }
    );
    assert_eq!(
        definition.fields[1].value,
        Value::MainClamp { bits: 0x3f80_0001 }
    );
    assert_eq!(
        definition.fields[2].value,
        Value::FaceClamp { bits: 0xff80_0000 }
    );
    assert_eq!(
        definition
            .fields
            .iter()
            .map(|field| field.decoded_offset)
            .collect::<Vec<_>>(),
        [0, 42, 52]
    );
    assert_eq!(definition.record().unwrap().payload, body);
    assert_eq!(definition.header.trailing_bytes, [0xA5, 0x5A]);
    assert_eq!(catalogue.counts().scalar_fields, 3);
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

#[test]
fn ordered_duplicate_clamps_extended_unknown_bytes_and_required_absence_are_retained() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"UNAM", &0u32.to_le_bytes()),
        field(b"XXXX", &6u32.to_le_bytes()),
        b"UNKN\0\0opaque".to_vec(),
        field(b"DATA", &data()),
        field(b"UNAM", &1u32.to_le_bytes()),
        field(b"DATA", &data()),
    ]
    .concat();
    let mut store = single(directory.path(), 15, &body);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(definition.fields[1].kind, *b"UNKN");
    assert_eq!(definition.fields[1].decoded_offset, 20);
    assert_eq!(definition.fields[1].bytes, 6);
    assert_eq!(definition.fields[1].value, Value::Opaque);
    assert_eq!(definition.record().unwrap().payload, body);
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        [
            "multiple_race_face_clamp_fields",
            "multiple_race_data_fields",
            "missing_race_main_clamp_field"
        ]
    );
    assert_eq!(definition.findings[0].field_decoded_offset, Some(74));
    assert_eq!(definition.findings[1].field_decoded_offset, Some(84));
    assert_eq!(definition.findings[2].field_decoded_offset, None);
    drop(store);
    let mut store = single(directory.path(), 15, &[]);
    let empty = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = empty.iter().next().unwrap().1;
    assert!(definition.fields.is_empty());
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        [
            "missing_race_data_field",
            "missing_race_main_clamp_field",
            "missing_race_face_clamp_field"
        ]
    );
}

#[test]
fn unobserved_versions_lengths_and_broken_frames_fail_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    for version in [0, 1, 14, 16] {
        let mut store = single(directory.path(), version, &complete(0));
        assert!(
            Catalogue::load(&mut store, Limits::default())
                .err()
                .unwrap()
                .to_string()
                .contains("RACE record version")
        );
    }
    for (kind, lengths) in [
        (b"DATA", vec![0, 35, 37]),
        (b"PNAM", vec![0, 3, 5]),
        (b"UNAM", vec![0, 3, 5]),
    ] {
        for length in lengths {
            let mut store = single(directory.path(), 15, &field(kind, &vec![0; length]));
            assert!(Catalogue::load(&mut store, Limits::default()).is_err());
        }
    }
    for body in [
        b"DAT".to_vec(),
        b"DATA\x24\0short".to_vec(),
        field(b"XXXX", &4u32.to_le_bytes()),
        [
            field(b"XXXX", &4u32.to_le_bytes()),
            field(b"XXXX", &4u32.to_le_bytes()),
        ]
        .concat(),
    ] {
        let mut store = single(directory.path(), 15, &body);
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
}

#[test]
fn candidate_body_aggregate_and_field_limits_precede_allocation() {
    let directory = tempfile::tempdir().unwrap();
    let body = complete(0);
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
    for limits in [
        Limits {
            max_records: 0,
            ..Default::default()
        },
        Limits {
            max_records: 1,
            ..Default::default()
        },
        Limits {
            max_record_bytes: body.len() - 1,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: body.len() * 2 - 1,
            ..Default::default()
        },
        Limits {
            max_fields: 0,
            ..Default::default()
        },
        Limits {
            max_fields: 5,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut store, limits).is_err());
    }
    let admitted = Catalogue::load(
        &mut store,
        Limits {
            max_records: 2,
            max_record_bytes: body.len() * 2,
            max_decoded_bytes: body.len() * 2,
            max_fields: 6,
        },
    )
    .unwrap();
    assert_eq!(admitted.counts().records, 2);
    assert_eq!(admitted.counts().fields, 6);
    assert_eq!(admitted.counts().decoded_bytes, body.len() * 2);
}

#[test]
fn winning_override_unread_tombstone_and_self_identity_survive_cache_reordering() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(0x100, 0, 15, &complete(0)),
            disk(0x101, 0, 15, &complete(0)),
        ]
        .concat(),
    )
    .unwrap();
    for (name, marker) in [("A.esm", 1), ("B.esm", 2)] {
        let mut bytes = [
            header(&["FalloutNV.esm"]),
            disk(0x0100_0100, plugin::COMPRESSED, 15, &complete(marker)),
        ]
        .concat();
        if name == "A.esm" {
            bytes.extend(disk(0x100, 0, 15, &complete(0x8000_0000)));
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
                assert_eq!(
                    definition.fields[1].value,
                    Value::MainClamp { bits: 0x8000_0000 }
                );
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
