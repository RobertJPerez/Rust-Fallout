use fallout_data::{
    actors::factions::{Catalogue, Limits, Value},
    inventory::Status,
    plugin, record_metadata,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};
fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn disk(kind: &[u8; 4], raw: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let payload = if flags & plugin::COMPRESSED != 0 {
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(body).unwrap();
        [
            &(body.len() as u32).to_le_bytes()[..],
            &compressor.finish().unwrap(),
        ]
        .concat()
    } else {
        body.to_vec()
    };
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0; 2],
        &payload,
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
    disk(b"TES4", 0, 0, 15, &body)
}
fn relation(raw: u32, modifier: i32, reaction: u32) -> Vec<u8> {
    field(
        b"XNAM",
        &[
            raw.to_le_bytes().as_slice(),
            &modifier.to_le_bytes(),
            &reaction.to_le_bytes(),
        ]
        .concat(),
    )
}
fn source(path: &Path, order: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn single(path: &Path, body: &[u8], version: u16) -> RecordStore {
    fs::write(
        path.join("FalloutNV.esm"),
        [header(&[]), disk(b"FACT", 0x100, 0, version, body)].concat(),
    )
    .unwrap();
    source(path, &["FalloutNV.esm"])
}
fn modern() -> Vec<u8> {
    field(b"DATA", &[255, 128, 0xaa, 0x55])
}

#[test]
fn legacy_absence_raw_unused_float_and_signed_rank_are_retained_without_editor_migration() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"DATA", &[255]),
        field(b"CNAM", &0x7fc0_1234u32.to_le_bytes()),
        field(b"RNAM", &i32::MIN.to_le_bytes()),
        field(b"MNAM", b"Male\0"),
        field(b"FNAM", b"Female\0"),
        field(b"INAM", b"Insignia\0"),
    ]
    .concat();
    let mut store = single(directory.path(), &body, 1);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let (key, definition) = catalogue.iter().next().unwrap();
    assert!(std::ptr::eq(catalogue.get(key).unwrap(), definition));
    assert!(matches!(
        definition.fields[0].value,
        Value::Flags {
            flags_1: 255,
            flags_2: None,
            unused: None
        }
    ));
    assert!(matches!(
        definition.fields[1].value,
        Value::UnusedFloat { bits: 0x7fc0_1234 }
    ));
    assert!(matches!(
        definition.fields[2].value,
        Value::RankNumber { rank: i32::MIN }
    ));
    assert!(
        definition.fields[3..]
            .iter()
            .all(|field| matches!(field.value, Value::Opaque))
    );
    assert_eq!(definition.record().unwrap().payload, body);
    assert_eq!(
        definition.source.sha256,
        catalogue.sources()[0].source_sha256
    );
    assert_eq!(
        catalogue.winning_content_sha256(),
        record_metadata::inspect(&store)
            .unwrap()
            .winning_definitions_sha256
    );
    assert!(definition.findings.is_empty());
}

#[test]
fn ordered_relations_keep_modifier_reaction_and_null_missing_deleted_wrong_kind_states() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        modern(),
        relation(0x101, i32::MIN, u32::MAX),
        relation(0x200, i32::MAX, 0),
        relation(0, -1, 1),
        relation(0x999, 1, 2),
        relation(0x102, 2, 3),
        relation(0x201, 3, 4),
        field(b"WMI1", &0x202u32.to_le_bytes()),
    ]
    .concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"FACT", 0x100, plugin::COMPRESSED, 15, &body),
            disk(b"FACT", 0x101, 0, 15, &modern()),
            disk(b"FACT", 0x102, plugin::DELETED, 16, b"unread"),
            disk(b"RACE", 0x200, 0, 15, &[]),
            disk(b"WEAP", 0x201, 0, 15, &[]),
            disk(b"REPU", 0x202, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = source(directory.path(), &["FalloutNV.esm"]);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue
        .iter()
        .find(|(key, _)| key.local_id == 0x100)
        .unwrap()
        .1;
    assert!(matches!(
        definition.fields[0].value,
        Value::Flags {
            flags_1: 255,
            flags_2: Some(128),
            unused: Some([0xaa, 0x55])
        }
    ));
    let links = definition
        .fields
        .iter()
        .filter_map(|field| {
            if let Value::Relation {
                faction,
                modifier,
                group_combat_reaction,
                schema_kind_allowed,
            } = &field.value
            {
                Some((
                    faction.status,
                    *modifier,
                    *group_combat_reaction,
                    *schema_kind_allowed,
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        links,
        vec![
            (Status::Defined, i32::MIN, u32::MAX, Some(true)),
            (Status::Defined, i32::MAX, 0, Some(true)),
            (Status::Null, -1, 1, None),
            (Status::Missing, 1, 2, None),
            (Status::Deleted, 2, 3, Some(true)),
            (Status::Defined, 3, 4, Some(false))
        ]
    );
    assert!(
        matches!(&definition.fields[7].value, Value::Reputation { reputation, schema_kind_allowed: Some(true) } if reputation.status == Status::Defined)
    );
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        vec![
            "faction_target_missing",
            "faction_target_deleted",
            "faction_target_wrong_kind"
        ]
    );
    assert_eq!(catalogue.counts().bindings, 7);
    assert_eq!(catalogue.counts().binding_statuses["defined"], 4);
    assert_eq!(definition.record().unwrap().payload, body);
}

#[test]
fn repeated_singletons_and_missing_flags_are_findings_but_rank_and_relation_order_stays_authored() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        modern(),
        field(b"DATA", &[1]),
        field(b"CNAM", &[0; 4]),
        field(b"CNAM", &[1; 4]),
        field(b"WMI1", &[0; 4]),
        field(b"WMI1", &[0; 4]),
        field(b"RNAM", &[0; 4]),
        field(b"RNAM", &[0; 4]),
        relation(0, 1, 0),
        relation(0, 1, 0),
    ]
    .concat();
    let mut store = single(directory.path(), &body, 15);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(definition.fields.len(), 10);
    assert_eq!(
        definition
            .findings
            .iter()
            .map(|finding| finding.code)
            .collect::<Vec<_>>(),
        vec![
            "multiple_faction_data_fields",
            "multiple_faction_unused_float_fields",
            "multiple_faction_reputation_fields"
        ]
    );
    drop(store);
    let mut missing = single(directory.path(), &field(b"UNKN", &[9]), 15);
    let catalogue = Catalogue::load(&mut missing, Limits::default()).unwrap();
    assert_eq!(catalogue.counts().selected_fields, 0);
    assert_eq!(
        catalogue.iter().next().unwrap().1.findings[0].code,
        "missing_faction_data_field"
    );
}

#[test]
fn extended_opaque_fields_preserve_physical_offsets_and_bytes_between_declarations() {
    let directory = tempfile::tempdir().unwrap();
    let extended = [
        field(b"XXXX", &6u32.to_le_bytes()),
        b"UNKN\0\0opaque".to_vec(),
    ]
    .concat();
    let body = [modern(), extended, field(b"RNAM", &(-1i32).to_le_bytes())].concat();
    let mut store = single(directory.path(), &body, 15);
    let catalogue = Catalogue::load(&mut store, Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert_eq!(definition.fields[1].kind, *b"UNKN");
    assert_eq!(definition.fields[1].decoded_offset, 20);
    assert_eq!(definition.fields[1].bytes, 6);
    assert!(matches!(definition.fields[1].value, Value::Opaque));
    assert_eq!(definition.record().unwrap().payload, body);
    assert!(definition.findings.is_empty());
}

#[test]
fn observed_versions_accept_exact_selected_shapes_and_other_versions_lengths_frames_fail() {
    let directory = tempfile::tempdir().unwrap();
    for version in [1, 2, 3, 4, 8, 9, 10, 11, 13, 14, 15] {
        let mut store = single(directory.path(), &modern(), version);
        assert!(Catalogue::load(&mut store, Limits::default()).is_ok());
    }
    for version in [0, 5, 6, 7, 12, 16] {
        let mut store = single(directory.path(), &modern(), version);
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
    let mut cases = vec![
        b"DAT".to_vec(),
        b"DATA\x04\0\0".to_vec(),
        field(b"XXXX", &4u32.to_le_bytes()),
    ];
    for length in [0, 2, 3, 5] {
        cases.push(field(b"DATA", &vec![0; length]));
    }
    for (kind, lengths) in [
        (b"CNAM", [3, 5]),
        (b"RNAM", [3, 5]),
        (b"WMI1", [3, 5]),
        (b"XNAM", [11, 13]),
    ] {
        for length in lengths {
            cases.push(field(kind, &vec![0; length]));
        }
    }
    for body in cases {
        let mut store = single(directory.path(), &body, 15);
        assert!(Catalogue::load(&mut store, Limits::default()).is_err());
    }
}

#[test]
fn records_bodies_fields_and_bindings_obey_independent_limits() {
    let directory = tempfile::tempdir().unwrap();
    let body = [modern(), relation(0, 1, 0)].concat();
    let mut store = single(directory.path(), &body, 15);
    for limits in [
        Limits {
            max_records: 0,
            ..Default::default()
        },
        Limits {
            max_record_bytes: body.len() - 1,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: body.len() - 1,
            ..Default::default()
        },
        Limits {
            max_fields: 1,
            ..Default::default()
        },
        Limits {
            max_bindings: 0,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut store, limits).is_err());
    }
}

#[test]
fn cold_warm_reordering_uses_current_owner_namespace_and_keeps_unread_deleted_override() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"FACT", 0x100, 0, 1, &field(b"DATA", &[1])),
            disk(b"FACT", 0x101, 0, 15, &modern()),
        ]
        .concat(),
    )
    .unwrap();
    for name in ["A.esm", "B.esm"] {
        let mut rows = [
            header(&["FalloutNV.esm"]),
            disk(
                b"FACT",
                0x0100_0100,
                plugin::COMPRESSED,
                15,
                &[modern(), relation(0x0100_0101, -1, 3)].concat(),
            ),
            disk(b"FACT", 0x0100_0101, 0, 15, &modern()),
        ]
        .concat();
        if name == "A.esm" {
            rows.extend(disk(
                b"FACT",
                0x101,
                plugin::DELETED,
                16,
                b"unread bad body",
            ));
        }
        fs::write(directory.path().join(name), rows).unwrap();
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
        assert_eq!(catalogue.counts().records, 6);
        assert_eq!(catalogue.counts().deleted_records, 1);
        for (key, definition) in catalogue.iter() {
            if definition.deleted {
                assert!(
                    definition.record().is_none()
                        && definition.fields.is_empty()
                        && definition.findings.is_empty()
                );
                assert_eq!(definition.header.version, 16);
                assert_eq!(definition.source.plugin, "A.esm");
            }
            if key.local_id == 0x100 && key.origin_plugin != "falloutnv.esm" {
                let Value::Relation { faction, .. } = &definition.fields[1].value else {
                    panic!("relation")
                };
                assert_eq!(
                    faction.key.as_ref().unwrap().origin_plugin,
                    key.origin_plugin
                );
                assert_eq!(
                    faction
                        .target
                        .as_ref()
                        .unwrap()
                        .source_plugin
                        .to_lowercase(),
                    key.origin_plugin
                );
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
