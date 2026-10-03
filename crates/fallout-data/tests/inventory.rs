use fallout_data::{
    inventory::{
        self, Catalogue, ExtraWord, Status, Value,
        fields::{self, Raw},
    },
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], body: Vec<u8>) -> plugin::Record {
    plugin::Record {
        header: plugin::RecordHeader {
            kind: *kind,
            offset: 24,
            stored_size: body.len() as u32,
            flags: 0,
            form_id: 0x100,
            revision: [0; 4],
            version: 0,
            trailing_bytes: [0; 2],
        },
        payload: body,
        integrity_issue: None,
    }
}
fn disk_record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    disk_record(b"TES4", 0, 0, &body)
}
fn cnto(id: u32, count: i32) -> Vec<u8> {
    field(b"CNTO", &[id.to_le_bytes(), count.to_le_bytes()].concat())
}
fn coed(owner: u32, word: u32, condition: u32) -> Vec<u8> {
    field(
        b"COED",
        &[
            owner.to_le_bytes(),
            word.to_le_bytes(),
            condition.to_le_bytes(),
        ]
        .concat(),
    )
}
fn store(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}

#[test]
fn ordered_entries_keep_signed_counts_duplicate_items_and_condition_bits() {
    let body = [
        cnto(0x200, i32::MIN),
        coed(0, u32::MAX, 0x7fc0_1234),
        cnto(0x200, 0),
        cnto(0x200, i32::MAX),
        coed(0x300, 0x8000_0000, 0x8000_0000),
        field(b"UNKN", &[1, 2, 3]),
    ]
    .concat();
    let source = record(b"CONT", body.clone());
    let doc = fields::decode(&source, "authored", fields::Limits::default()).unwrap();
    assert_eq!(doc.items.len(), 3);
    assert_eq!(doc.items[0].cnto_field, 0);
    assert_eq!(doc.items[0].coed_fields, vec![1]);
    assert_eq!(doc.items[1].cnto_field, 2);
    assert!(doc.items[1].coed_fields.is_empty());
    assert_eq!(doc.items[2].coed_fields, vec![4]);
    assert!(matches!(
        doc.fields[0].value,
        Raw::Item {
            count: i32::MIN,
            ..
        }
    ));
    assert!(matches!(
        doc.fields[1].value,
        Raw::Extra {
            union_word: u32::MAX,
            condition_bits: 0x7fc0_1234,
            ..
        }
    ));
    assert!(matches!(
        doc.fields[4].value,
        Raw::Extra {
            condition_bits: 0x8000_0000,
            ..
        }
    ));
    assert!(matches!(doc.fields[5].value, Raw::Opaque));
    assert_eq!(source.payload, body);
    assert!(doc.findings.is_empty());
}

#[test]
fn orphan_extra_fields_and_repeated_headers_are_findings_without_normalization() {
    let mut acbs = [0; 24];
    acbs[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    acbs[22..].copy_from_slice(&0xffff_u16.to_le_bytes());
    let source = record(
        b"NPC_",
        [
            coed(0, 1, 2),
            cnto(0x200, 1),
            coed(0, 3, 4),
            coed(0, 5, 6),
            field(b"ABCD", &[7]),
            coed(0, 8, 9),
            field(b"ACBS", &acbs),
            field(b"ACBS", &acbs),
            field(b"TPLT", &1_u32.to_le_bytes()),
            field(b"TPLT", &2_u32.to_le_bytes()),
        ]
        .concat(),
    );
    let doc = fields::decode(&source, "authored", fields::Limits::default()).unwrap();
    assert_eq!(doc.fields.len(), 10);
    assert_eq!(doc.items[0].coed_fields, vec![2, 3]);
    assert_eq!(
        doc.findings.iter().map(|row| row.code).collect::<Vec<_>>(),
        vec![
            "orphan_item_extra_field",
            "multiple_item_extra_fields",
            "orphan_item_extra_field",
            "multiple_actor_base_fields",
            "multiple_actor_template_fields"
        ]
    );
    assert!(matches!(
        doc.fields[6].value,
        Raw::ActorBase {
            flags: u32::MAX,
            template_flags: u16::MAX
        }
    ));
}

#[test]
fn malformed_known_fields_extensions_and_tainted_records_are_rejected() {
    for (record_kind, kind, good_size) in [
        (b"CONT", b"CNTO", 8),
        (b"CONT", b"COED", 12),
        (b"CONT", b"DATA", 5),
        (b"NPC_", b"ACBS", 24),
        (b"CREA", b"TPLT", 4),
    ] {
        for size in [good_size - 1, good_size + 1] {
            assert!(
                fields::decode(
                    &record(record_kind, field(kind, &vec![0; size])),
                    "bad",
                    fields::Limits::default()
                )
                .is_err()
            );
        }
    }
    for body in [
        b"short".to_vec(),
        field(b"XXXX", &8_u32.to_le_bytes()),
        [
            field(b"XXXX", &8_u32.to_le_bytes()),
            field(b"XXXX", &8_u32.to_le_bytes()),
            cnto(1, 1),
        ]
        .concat(),
    ] {
        assert!(fields::decode(&record(b"CONT", body), "bad", fields::Limits::default()).is_err());
    }
    let mut tainted = record(b"CONT", cnto(1, 1));
    tainted.integrity_issue = Some(plugin::ChecksumMismatch {
        file_offset: 24,
        form_id: 1,
        stored_adler32: 2,
        calculated_adler32: 3,
    });
    assert!(fields::decode(&tainted, "bad", fields::Limits::default()).is_err());
    assert!(
        fields::decode(
            &record(b"ACTI", cnto(1, 1)),
            "bad",
            fields::Limits::default()
        )
        .is_err()
    );
}

#[test]
fn extended_fields_preserve_physical_offsets_and_explicit_budgets() {
    let source = record(
        b"CONT",
        [
            field(b"XXXX", &8_u32.to_le_bytes()),
            field(
                b"CNTO",
                &[1_u32.to_le_bytes(), 2_u32.to_le_bytes()].concat(),
            ),
        ]
        .concat(),
    );
    let doc = fields::decode(&source, "authored", fields::Limits::default()).unwrap();
    assert_eq!(doc.fields[0].decoded_offset, 10);
    assert_eq!(doc.fields[0].bytes, 8);
    for limits in [
        fields::Limits {
            max_fields: 0,
            ..Default::default()
        },
        fields::Limits {
            max_items: 0,
            ..Default::default()
        },
        fields::Limits {
            max_record_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(fields::decode(&source, "bounded", limits).is_err());
    }
}

#[test]
fn owner_words_bind_to_source_namespaces_and_winning_owner_kind() {
    let directory = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(disk_record(b"NPC_", 0x300, 0, &[]));
    base.extend(disk_record(b"FACT", 0x301, 0, &[]));
    base.extend(disk_record(b"GLOB", 0x302, 0, &[]));
    base.extend(disk_record(b"WEAP", 0x200, 0, &[]));
    base.extend(disk_record(b"ACTI", 0x303, 0, &[]));
    base.extend(disk_record(b"NPC_", 0x304, plugin::DELETED, &[]));
    fs::write(directory.path().join("FalloutNV.esm"), base).unwrap();
    let mut addon = header(&["FalloutNV.esm"]);
    let body = [
        cnto(0x200, 2),
        coed(0x300, 0x302, 0x7fc0_1234),
        cnto(0x0100_0200, 3),
        coed(0x301, u32::MAX, 1),
        cnto(0, 0),
        coed(0, 0xffff_ffff, 2),
        cnto(0x999, -1),
        coed(0x303, 0x302, 3),
        cnto(0x200, 1),
        coed(0x304, 0x302, 4),
    ]
    .concat();
    addon.extend(disk_record(b"CONT", 0x0100_0100, 0, &body));
    addon.extend(disk_record(b"AMMO", 0x0100_0200, 0, &[]));
    fs::write(directory.path().join("Addon.esm"), addon).unwrap();
    let mut store = store(directory.path(), &["FalloutNV.esm", "Addon.esm"]);
    let catalogue = Catalogue::load(&mut store, inventory::Limits::default()).unwrap();
    let definition = catalogue
        .iter()
        .find(|(_, row)| row.kind == *b"CONT")
        .unwrap()
        .1;
    assert_eq!(definition.items.len(), 5);
    assert!(definition.record().is_some());
    let Value::Item {
        item,
        schema_kind_allowed,
        ..
    } = &definition.fields[0].value
    else {
        panic!("item");
    };
    assert_eq!(item.key.as_ref().unwrap().origin_plugin, "falloutnv.esm");
    assert_eq!(*schema_kind_allowed, Some(true));
    let Value::Extra {
        union_word: ExtraWord::Global { binding },
        condition_bits,
        ..
    } = &definition.fields[1].value
    else {
        panic!("global");
    };
    assert_eq!(binding.status, Status::Defined);
    assert_eq!(binding.target.as_ref().unwrap().kind, *b"GLOB");
    assert_eq!(*condition_bits, 0x7fc0_1234);
    let Value::Item { item, .. } = &definition.fields[2].value else {
        panic!("addon item");
    };
    assert_eq!(item.key.as_ref().unwrap().origin_plugin, "addon.esm");
    assert!(matches!(
        definition.fields[3].value,
        Value::Extra {
            union_word: ExtraWord::RequiredRank { value: -1, .. },
            ..
        }
    ));
    assert!(matches!(
        definition.fields[5].value,
        Value::Extra {
            union_word: ExtraWord::Unused { raw_word: u32::MAX },
            ..
        }
    ));
    assert!(matches!(
        definition.fields[7].value,
        Value::Extra {
            union_word: ExtraWord::UnresolvedOwner { .. },
            ..
        }
    ));
    assert!(matches!(
        definition.fields[9].value,
        Value::Extra {
            union_word: ExtraWord::UnresolvedOwner { .. },
            ..
        }
    ));
    assert_eq!(catalogue.counts.non_positive_counts, 2);
}

#[test]
fn deleted_winners_do_not_fall_back_and_catalogue_limits_are_enforced() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk_record(b"CONT", 0x100, 0, &cnto(0x200, 1)),
            disk_record(b"WEAP", 0x200, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Addon.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk_record(b"CONT", 0x100, plugin::DELETED, b"unparsed tombstone"),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = store(directory.path(), &["FalloutNV.esm", "Addon.esm"]);
    let catalogue = Catalogue::load(&mut store, inventory::Limits::default()).unwrap();
    let definition = catalogue.iter().next().unwrap().1;
    assert!(definition.deleted);
    assert!(definition.items.is_empty());
    assert!(definition.record().is_none());
    assert_eq!(catalogue.counts.deleted_records, 1);
    assert!(
        Catalogue::load(
            &mut store,
            inventory::Limits {
                max_records: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
    drop(store);
    let mut base_store = self::store(directory.path(), &["FalloutNV.esm"]);
    for limits in [
        inventory::Limits {
            max_fields: 0,
            ..Default::default()
        },
        inventory::Limits {
            max_items: 0,
            ..Default::default()
        },
        inventory::Limits {
            max_decoded_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut base_store, limits).is_err());
    }
}

#[test]
fn actor_template_and_container_flags_remain_raw_inputs() {
    let mut acbs = [0; 24];
    acbs[..4].copy_from_slice(&0x8000_0100_u32.to_le_bytes());
    acbs[22..].copy_from_slice(&0x8100_u16.to_le_bytes());
    let actor = fields::decode(
        &record(
            b"CREA",
            [
                field(b"ACBS", &acbs),
                field(b"TPLT", &0x1234_u32.to_le_bytes()),
            ]
            .concat(),
        ),
        "authored",
        fields::Limits::default(),
    )
    .unwrap();
    assert!(matches!(
        actor.fields[0].value,
        Raw::ActorBase {
            flags: 0x8000_0100,
            template_flags: 0x8100
        }
    ));
    let container = fields::decode(
        &record(
            b"CONT",
            field(
                b"DATA",
                &[&[0xff][..], &0x7f80_0000_u32.to_le_bytes()].concat(),
            ),
        ),
        "authored",
        fields::Limits::default(),
    )
    .unwrap();
    assert!(matches!(
        container.fields[0].value,
        Raw::ContainerData {
            flags: 0xff,
            weight_bits: 0x7f80_0000
        }
    ));
}
