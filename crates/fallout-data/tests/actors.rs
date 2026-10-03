use fallout_data::{
    actors::{
        self, Catalogue,
        fields::{self, Value},
    },
    inventory, plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], body: Vec<u8>, version: u16) -> plugin::Record {
    plugin::Record {
        header: plugin::RecordHeader {
            kind: *kind,
            offset: 24,
            stored_size: body.len() as u32,
            flags: 0,
            form_id: 0x100,
            revision: [0; 4],
            version,
            trailing_bytes: [0; 2],
        },
        payload: body,
        integrity_issue: None,
    }
}
fn decode(source: &plugin::Record) -> fields::Document {
    fields::decode(source, "authored", fields::Limits::default()).unwrap()
}
fn disk_record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
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
    disk_record(b"TES4", 0, 0, &body)
}
fn store(path: &Path, order: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn npc_body(health: i32) -> Vec<u8> {
    [
        field(b"ACBS", &[0; 24]),
        field(
            b"DATA",
            &[health.to_le_bytes().as_slice(), &[1, 2, 3, 4, 5, 6, 7]].concat(),
        ),
    ]
    .concat()
}

#[test]
fn configuration_keeps_unclamped_words_and_exact_float_bits() {
    let acbs = [
        0x80, 0, 0, 0, 0xfe, 0xff, 0x34, 0x12, 0xff, 0xff, 0, 0x80, 2, 1, 0xff, 0xff, 0x34, 0x12,
        0xc0, 0x7f, 0, 0x80, 0xff, 0xff,
    ];
    let source = record(
        b"NPC_",
        [field(b"ACBS", &acbs), field(b"DATA", &[0; 11])].concat(),
        15,
    );
    let document = decode(&source);
    assert_eq!(
        document.fields[0].value,
        Value::Configuration {
            fatigue: 65534,
            barter_gold: 0x1234,
            level_word: 65535,
            player_level_multiplier_flag: true,
            calc_min: 32768,
            calc_max: 258,
            speed_multiplier: 65535,
            karma_bits: 0x7fc0_1234,
            disposition_base: i16::MIN,
        }
    );
    assert!(document.findings.is_empty());
    let mut literal = source;
    literal.payload[6] = 0;
    assert!(matches!(
        decode(&literal).fields[0].value,
        Value::Configuration {
            level_word: 65535,
            player_level_multiplier_flag: false,
            ..
        }
    ));
}

#[test]
fn npc_health_attributes_skill_offsets_and_legacy_unused_tail_remain_authored() {
    let data = [vec![0, 0, 0, 0x80, 1, 2, 3, 4, 5, 6, 255], vec![0xaa; 14]].concat();
    let skills = [
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 255],
        vec![
            255, 254, 253, 252, 251, 250, 249, 248, 247, 246, 245, 244, 243, 128,
        ],
    ]
    .concat();
    let source = record(
        b"NPC_",
        [
            field(b"ACBS", &[0; 24]),
            field(b"DATA", &data),
            field(b"DNAM", &skills),
            field(b"UNKN", &[9, 8, 7]),
        ]
        .concat(),
        14,
    );
    let document = decode(&source);
    assert_eq!(
        document.fields[1].value,
        Value::NpcData {
            base_health: i32::MIN,
            attributes: [1, 2, 3, 4, 5, 6, 255],
            unused_tail: vec![0xaa; 14]
        }
    );
    assert_eq!(
        document.fields[2].value,
        Value::NpcSkills {
            skill_values: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 255],
            skill_offsets: [
                255, 254, 253, 252, 251, 250, 249, 248, 247, 246, 245, 244, 243, 128
            ]
        }
    );
    assert_eq!(document.fields[3].value, Value::Opaque);
    assert_eq!(document.fields[1].decoded_offset, 30);
    assert_eq!(document.fields[1].bytes, 25);
    assert_eq!(source.payload[36..61], data);
    for tail_len in [0, 1, 3, 14, 20] {
        let document = decode(&record(
            b"NPC_",
            field(b"DATA", &vec![0; 11 + tail_len]),
            15,
        ));
        assert!(
            matches!(&document.fields[0].value,Value::NpcData { unused_tail,.. } if unused_tail.len()==tail_len)
        );
    }
}

#[test]
fn creature_data_preserves_signed_health_damage_and_unused_bytes() {
    let bytes = [
        255, 254, 253, 252, 0, 0x80, 0xab, 0xcd, 0xff, 0xff, 1, 2, 3, 4, 5, 6, 7,
    ];
    let document = decode(&record(
        b"CREA",
        [
            field(b"ACBS", &[0; 24]),
            field(b"DATA", &bytes),
            field(b"DNAM", &[9]),
        ]
        .concat(),
        9,
    ));
    assert_eq!(
        document.fields[1].value,
        Value::CreatureData {
            creature_type: 255,
            combat_skill: 254,
            magic_skill: 253,
            stealth_skill: 252,
            health: i16::MIN,
            unused: [0xab, 0xcd],
            damage: -1,
            attributes: [1, 2, 3, 4, 5, 6, 7]
        }
    );
    assert_eq!(document.fields[2].value, Value::Opaque);
}

#[test]
fn duplicates_missing_required_fields_and_optional_skill_absence_are_explicit() {
    let document = decode(&record(
        b"NPC_",
        [
            field(b"DATA", &[0; 11]),
            field(b"DATA", &[1; 11]),
            field(b"DNAM", &[0; 28]),
            field(b"DNAM", &[1; 28]),
        ]
        .concat(),
        15,
    ));
    assert_eq!(
        document
            .findings
            .iter()
            .map(|f| (f.field_decoded_offset, f.code))
            .collect::<Vec<_>>(),
        vec![
            (Some(17), "multiple_actor_data_fields"),
            (Some(68), "multiple_npc_skill_fields"),
            (None, "missing_configuration_field")
        ]
    );
    assert_eq!(document.fields.len(), 4);
    let missing = decode(&record(b"CREA", vec![], 15));
    assert_eq!(
        missing.findings.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec!["missing_configuration_field", "missing_actor_data_field"]
    );
    assert!(
        decode(&record(b"NPC_", npc_body(10), 15))
            .findings
            .is_empty()
    );
    let repeated = decode(&record(
        b"NPC_",
        [npc_body(10), field(b"ACBS", &[0; 24])].concat(),
        15,
    ));
    assert_eq!(repeated.findings[0].code, "multiple_configuration_fields");
}

#[test]
fn malformed_extents_truncations_tombstones_and_integrity_failures_are_rejected() {
    for (kind, signature, good) in [
        (b"NPC_", b"ACBS", 24),
        (b"NPC_", b"DNAM", 28),
        (b"CREA", b"DATA", 17),
    ] {
        for size in [good - 1, good + 1] {
            assert!(
                fields::decode(
                    &record(kind, field(signature, &vec![0; size]), 15),
                    "bad",
                    fields::Limits::default()
                )
                .is_err()
            );
        }
    }
    for size in 0..11 {
        assert!(
            fields::decode(
                &record(b"NPC_", field(b"DATA", &vec![0; size]), 15),
                "bad",
                fields::Limits::default()
            )
            .is_err()
        );
    }
    let source = record(b"NPC_", npc_body(123), 15);
    for end in 1..source.payload.len() {
        if end == 30 {
            continue;
        } // A complete ACBS alone is preserved with a missing DATA finding.
        assert!(
            fields::decode(
                &record(b"NPC_", source.payload[..end].to_vec(), 15),
                "truncated",
                fields::Limits::default()
            )
            .is_err()
        );
    }
    let mut source = source;
    source.header.flags = plugin::DELETED;
    assert!(fields::decode(&source, "deleted", fields::Limits::default()).is_err());
    source.header.flags = 0;
    source.integrity_issue = Some(plugin::ChecksumMismatch {
        file_offset: 24,
        form_id: 1,
        stored_adler32: 2,
        calculated_adler32: 3,
    });
    assert!(fields::decode(&source, "tainted", fields::Limits::default()).is_err());
}

#[test]
fn version_dispatch_extended_offsets_and_parser_budgets_are_bounded() {
    for version in [14, 15] {
        decode(&record(b"NPC_", npc_body(10), version));
    }
    for version in [9, 11, 13, 14, 15] {
        decode(&record(b"CREA", field(b"DATA", &[0; 17]), version));
    }
    for (kind, version) in [
        (b"NPC_", 13),
        (b"NPC_", 16),
        (b"CREA", 10),
        (b"CREA", 16),
        (b"ACTI", 15),
    ] {
        assert!(
            fields::decode(
                &record(kind, vec![], version),
                "version",
                fields::Limits::default()
            )
            .is_err()
        );
    }
    let extended = record(
        b"NPC_",
        [
            field(b"XXXX", &11u32.to_le_bytes()),
            field(b"DATA", &[0; 11]),
        ]
        .concat(),
        15,
    );
    assert_eq!(decode(&extended).fields[0].decoded_offset, 10);
    for limits in [
        fields::Limits {
            max_fields: 0,
            ..Default::default()
        },
        fields::Limits {
            max_record_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(fields::decode(&extended, "budget", limits).is_err());
    }
    for body in [
        field(b"XXXX", &11u32.to_le_bytes()),
        [
            field(b"XXXX", &11u32.to_le_bytes()),
            field(b"XXXX", &11u32.to_le_bytes()),
        ]
        .concat(),
    ] {
        assert!(
            fields::decode(
                &record(b"NPC_", body, 15),
                "extended",
                fields::Limits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn catalogue_borrows_exact_winning_inventory_sources_and_retains_deleted_winners() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk_record(b"NPC_", 0x100, 0, &npc_body(100)),
            disk_record(b"NPC_", 0x101, 0, &npc_body(50)),
            disk_record(b"CONT", 0x102, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Addon.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk_record(b"NPC_", 0x100, 0, &npc_body(200)),
            disk_record(b"NPC_", 0x101, plugin::DELETED, b"unparsed tombstone"),
            disk_record(b"NPC_", 0x0100_0100, 0, &npc_body(300)),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = store(directory.path(), &["FalloutNV.esm", "Addon.esm"]);
    let inventory = inventory::Catalogue::load(&mut store, inventory::Limits::default()).unwrap();
    drop(store);
    let catalogue = Catalogue::load(&inventory, actors::Limits::default()).unwrap();
    assert_eq!(catalogue.counts().records, 3);
    assert_eq!(catalogue.counts().deleted_records, 1);
    assert_eq!(
        catalogue.winning_content_sha256(),
        inventory.winning_content_sha256()
    );
    assert!(std::ptr::eq(
        catalogue.sources(),
        inventory.sources.as_slice()
    ));
    let mut health = Vec::new();
    for (key, definition) in catalogue.iter() {
        assert!(std::ptr::eq(
            definition.inventory_definition(),
            inventory.get(key).unwrap()
        ));
        assert!(std::ptr::eq(
            definition.source,
            &inventory.get(key).unwrap().source
        ));
        assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
        if definition.deleted {
            assert!(definition.record().is_none());
            assert!(definition.record_version.is_none());
            assert!(definition.fields.is_empty());
        } else {
            assert_eq!(definition.record_version, Some(15));
            let Value::NpcData { base_health, .. } = definition.fields[1].value else {
                panic!("NPC DATA")
            };
            health.push((key.origin_plugin.clone(), base_health));
        }
    }
    assert_eq!(
        health,
        vec![("addon.esm".into(), 300), ("falloutnv.esm".into(), 200)]
    );
    for limits in [
        actors::Limits {
            max_records: 0,
            ..Default::default()
        },
        actors::Limits {
            max_decoded_bytes: 1,
            ..Default::default()
        },
        actors::Limits {
            max_fields: 0,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&inventory, limits).is_err());
    }
}

#[test]
fn cold_warm_and_unrelated_order_rebuilds_preserve_actor_content_with_exact_cohort_identity() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), disk_record(b"NPC_", 0x100, 0, &npc_body(100))].concat(),
    )
    .unwrap();
    for (name, health) in [("A.esm", 200), ("B.esm", 300)] {
        fs::write(
            directory.path().join(name),
            [
                header(&["FalloutNV.esm"]),
                disk_record(b"NPC_", 0x0100_0100, 0, &npc_body(health)),
            ]
            .concat(),
        )
        .unwrap();
    }
    let cache_directory = tempfile::tempdir().unwrap();
    let cache = cache_directory.path();
    let mut observed = Vec::new();
    for (phase, order) in [
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "B.esm", "A.esm"],
    ]
    .into_iter()
    .enumerate()
    {
        let names = order.iter().map(|s| (*s).into()).collect::<Vec<_>>();
        let mut source = RecordStore::open_nv_headers_cached(
            directory.path(),
            &names,
            plugin::Limits::default(),
            cache,
        )
        .unwrap();
        assert!(
            source
                .index_cache_report()
                .unwrap()
                .plugins
                .iter()
                .all(|receipt| receipt.reused == (phase != 0))
        );
        let input = inventory::Catalogue::load(&mut source, inventory::Limits::default()).unwrap();
        let catalogue = Catalogue::load(&input, actors::Limits::default()).unwrap();
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
    let mut before = store(directory.path(), &["FalloutNV.esm", "A.esm", "B.esm"]);
    let prior = inventory::Catalogue::load(&mut before, inventory::Limits::default()).unwrap();
    drop(before);
    fs::write(
        directory.path().join("A.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk_record(b"NPC_", 0x0100_0100, 0, &npc_body(201)),
        ]
        .concat(),
    )
    .unwrap();
    let mut after = store(directory.path(), &["FalloutNV.esm", "A.esm", "B.esm"]);
    let changed = inventory::Catalogue::load(&mut after, inventory::Limits::default()).unwrap();
    assert_eq!(prior.counts.records, changed.counts.records);
    assert_ne!(
        prior.sources[1].source_sha256,
        changed.sources[1].source_sha256
    );
    let old = Catalogue::load(&prior, actors::Limits::default()).unwrap();
    let new = Catalogue::load(&changed, actors::Limits::default()).unwrap();
    assert_ne!(
        serde_json::to_value(old.iter().map(|(_, d)| d).collect::<Vec<_>>()).unwrap(),
        serde_json::to_value(new.iter().map(|(_, d)| d).collect::<Vec<_>>()).unwrap()
    );
}
