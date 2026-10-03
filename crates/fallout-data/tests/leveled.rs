use fallout_data::{
    identity::{FormKey, ProfileId},
    inventory,
    leveled::{
        self, Catalogue, Value,
        fields::{self, Raw},
        graph::{Graph, Limits as GraphLimits},
    },
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
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
fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
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
    for m in masters {
        body.extend(field(b"MAST", &[m.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, &body)
}
fn entry(id: u32, level: u16, count: Option<u16>, padding: bool) -> Vec<u8> {
    let mut data = [
        level.to_le_bytes().as_slice(),
        &0xabcd_u16.to_le_bytes(),
        &id.to_le_bytes(),
    ]
    .concat();
    if let Some(c) = count {
        data.extend(c.to_le_bytes());
        if padding {
            data.extend(0x9876_u16.to_le_bytes());
        }
    }
    field(b"LVLO", &data)
}
fn extra(owner: u32, word: u32) -> Vec<u8> {
    field(
        b"COED",
        &[
            owner.to_le_bytes(),
            word.to_le_bytes(),
            0x7fc0_1234_u32.to_le_bytes(),
        ]
        .concat(),
    )
}
fn store(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|n| (*n).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}

#[test]
fn legacy_and_full_entries_keep_absence_duplicates_high_bits_and_padding() {
    let body = [
        entry(0x200, 0xffff, None, false),
        entry(0x200, 1, Some(0), false),
        entry(0x200, 0x8000, Some(0xffff), true),
        extra(0, 0xdead_beef),
        field(b"ZZZZ", &[1, 2, 3]),
    ]
    .concat();
    let source = record(b"LVLI", body.clone());
    let doc = fields::decode(&source, "authored", fields::Limits::default()).unwrap();
    assert_eq!(doc.entries.len(), 3);
    assert!(doc.findings.is_empty());
    assert_eq!(doc.entries[2].coed_fields, vec![3]);
    assert!(matches!(
        doc.fields[0].value,
        Raw::Entry {
            level_bits: 0xffff,
            count_bits: None,
            count_padding: None,
            ..
        }
    ));
    assert!(matches!(
        doc.fields[1].value,
        Raw::Entry {
            count_bits: Some(0),
            count_padding: None,
            ..
        }
    ));
    assert!(matches!(
        doc.fields[2].value,
        Raw::Entry {
            level_bits: 0x8000,
            level_padding: 0xabcd,
            count_bits: Some(0xffff),
            count_padding: Some(0x9876),
            ..
        }
    ));
    assert!(matches!(
        doc.fields[3].value,
        Raw::Extra {
            condition_bits: 0x7fc0_1234,
            ..
        }
    ));
    assert_eq!(source.payload, body);
}
#[test]
fn duplicate_controls_and_ambiguous_extras_are_retained() {
    let body = [
        extra(0, 0),
        entry(0, 0, None, false),
        extra(0, 0),
        extra(0, 1),
        field(b"LVLD", &[255]),
        field(b"LVLD", &[0]),
        field(b"LVLF", &[255]),
        field(b"LVLF", &[0]),
        field(b"LVLG", &[0; 4]),
        field(b"LVLG", &[0; 4]),
    ]
    .concat();
    let doc = fields::decode(
        &record(b"LVLI", body),
        "authored",
        fields::Limits::default(),
    )
    .unwrap();
    assert_eq!(
        doc.findings.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec![
            "orphan_entry_extra_field",
            "multiple_entry_extra_fields",
            "multiple_chance_none_fields",
            "multiple_list_flag_fields",
            "multiple_chance_global_fields"
        ]
    );
    assert_eq!(doc.entries[0].coed_fields, vec![2, 3]);
    let actor = fields::decode(
        &record(b"LVLC", field(b"LVLG", &[1])),
        "authored",
        fields::Limits::default(),
    )
    .unwrap();
    assert!(matches!(actor.fields[0].value, Raw::Opaque));
}
#[test]
fn known_shapes_extents_and_limits_fail_without_coercion() {
    for (kind, sizes) in [
        (b"LVLO", vec![0, 7, 9, 11, 13]),
        (b"COED", vec![11, 13]),
        (b"LVLD", vec![0, 2]),
        (b"LVLF", vec![0, 2]),
        (b"LVLG", vec![3, 5]),
    ] {
        for n in sizes {
            assert!(
                fields::decode(
                    &record(b"LVLI", field(kind, &vec![0; n])),
                    "authored",
                    fields::Limits::default()
                )
                .is_err()
            );
        }
    }
    for body in [
        b"LVLO".to_vec(),
        field(b"XXXX", &[0; 4]),
        [
            field(b"XXXX", &8_u32.to_le_bytes()),
            field(b"XXXX", &8_u32.to_le_bytes()),
            entry(0, 0, None, false),
        ]
        .concat(),
    ] {
        assert!(
            fields::decode(
                &record(b"LVLI", body),
                "authored",
                fields::Limits::default()
            )
            .is_err()
        );
    }
    let source = record(b"LVLI", entry(0, 0, None, false));
    for limits in [
        fields::Limits {
            max_record_bytes: 0,
            ..Default::default()
        },
        fields::Limits {
            max_fields: 0,
            ..Default::default()
        },
        fields::Limits {
            max_entries: 0,
            ..Default::default()
        },
    ] {
        assert!(fields::decode(&source, "authored", limits).is_err());
    }
    assert!(
        fields::decode(
            &record(b"CONT", Vec::new()),
            "authored",
            fields::Limits::default()
        )
        .is_err()
    );
}
#[test]
fn extended_entry_keeps_physical_header_offset() {
    let ordinary = entry(0, 5, None, false);
    let body = [field(b"XXXX", &8_u32.to_le_bytes()), ordinary].concat();
    let doc = fields::decode(
        &record(b"LVLN", body),
        "authored",
        fields::Limits::default(),
    )
    .unwrap();
    assert_eq!(doc.fields[0].decoded_offset, 10);
    assert_eq!(doc.entries[0].lvlo_field, 0);
}
fn fixture(path: &Path) {
    let base = [
        header(&[]),
        disk(b"MISC", 0x104, 0, &[]),
        disk(b"NPC_", 0x101, 0, &[]),
        disk(b"FACT", 0x102, 0, &[]),
        disk(b"GLOB", 0x103, 0, &[]),
        disk(
            b"CONT",
            0x200,
            0,
            &field(
                b"CNTO",
                &[0x300_u32.to_le_bytes(), 2_i32.to_le_bytes()].concat(),
            ),
        ),
        disk(
            b"LVLI",
            0x300,
            0,
            &[
                entry(0x301, 1, Some(1), true),
                entry(0x301, 1, Some(1), true),
                entry(0x104, 1, Some(0xffff), true),
                extra(0x101, 0x103),
                entry(0x777, 0, None, false),
            ]
            .concat(),
        ),
        disk(
            b"LVLI",
            0x301,
            0,
            &[
                field(b"LVLF", &[255]),
                entry(0x300, 0x8000, Some(0), true),
                extra(0x102, u32::MAX),
            ]
            .concat(),
        ),
        disk(b"LVLI", 0x302, 0, &entry(0x302, 0, Some(1), true)),
        disk(b"LVLI", 0x303, plugin::DELETED, b"bad body"),
    ]
    .concat();
    fs::write(path.join("FalloutNV.esm"), base).unwrap();
    fs::write(
        path.join("Other.esm"),
        [header(&[]), disk(b"MISC", 0x104, 0, &[])].concat(),
    )
    .unwrap();
    fs::write(
        path.join("Addon.esm"),
        [
            header(&["FalloutNV.esm", "Other.esm"]),
            disk(
                b"LVLI",
                0x0200_0300,
                0,
                &[
                    entry(0x0100_0104, 1, Some(1), true),
                    entry(0x303, 0, None, false),
                    extra(0x303, 0xdead_beef),
                    field(b"LVLG", &0x103_u32.to_le_bytes()),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
}
#[test]
fn catalogue_preserves_namespaces_owner_unions_and_deleted_targets() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut store = store(dir.path(), &["FalloutNV.esm", "Other.esm", "Addon.esm"]);
    let c = Catalogue::load(&mut store, leveled::Limits::default()).unwrap();
    assert_eq!(c.counts.records, 5);
    assert_eq!(c.counts.deleted_records, 1);
    assert_eq!(c.counts.high_bit_levels, 1);
    assert_eq!(c.counts.high_bit_counts, 1);
    assert_eq!(c.counts.absent_counts, 2);
    let d = c.get(&key(0x300)).unwrap();
    assert_eq!(d.entries.len(), 4);
    assert!(d.record().is_some());
    assert!(
        matches!(&d.fields[3].value,Value::Extra{union_word:inventory::ExtraWord::Global{binding},..} if binding.key==Some(key(0x103)))
    );
    let d = c.get(&key(0x301)).unwrap();
    assert!(matches!(
        &d.fields[2].value,
        Value::Extra {
            union_word: inventory::ExtraWord::RequiredRank { value: -1, .. },
            ..
        }
    ));
    let addon = c
        .iter()
        .find(|(k, _)| k.origin_plugin == "addon.esm")
        .unwrap()
        .1;
    assert!(
        matches!(&addon.fields[0].value,Value::Entry{item,..} if item.key.as_ref().unwrap().origin_plugin=="other.esm")
    );
    assert!(matches!(
        &addon.fields[2].value,
        Value::Extra {
            union_word: inventory::ExtraWord::UnresolvedOwner {
                raw_word: 0xdead_beef
            },
            ..
        }
    ));
    assert!(c.get(&key(0x303)).unwrap().record().is_none());
}
#[test]
fn graph_closure_keeps_duplicate_edges_terminal_dependencies_and_cycles() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut store = store(dir.path(), &["FalloutNV.esm", "Other.esm", "Addon.esm"]);
    let c = Catalogue::load(&mut store, leveled::Limits::default()).unwrap();
    let base = inventory::Catalogue::load(&mut store, inventory::Limits::default()).unwrap();
    let graph = Graph::build(&base, &c, GraphLimits::default()).unwrap();
    assert_eq!(
        graph.cycles,
        vec![vec![key(0x300), key(0x301)], vec![key(0x302)]]
    );
    let closure = graph.closure(&key(0x200), GraphLimits::default()).unwrap();
    assert_eq!(closure.nodes, vec![key(0x200), key(0x300), key(0x301)]);
    assert_eq!(closure.edge_indices.len(), 6);
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|e| e.source == key(0x300) && e.target == Some(key(0x301)))
            .count(),
        2
    );
    assert_eq!(graph.counts.terminal_edges, 2);
    assert_eq!(graph.counts.unresolved_edges, 2);
    assert!(graph.closure(&key(0x303), GraphLimits::default()).is_err());
    assert!(graph.closure(&key(0x999), GraphLimits::default()).is_err());
    for limits in [
        GraphLimits {
            max_nodes: 0,
            ..Default::default()
        },
        GraphLimits {
            max_nodes: 2,
            ..Default::default()
        },
        GraphLimits {
            max_edges: 5,
            ..Default::default()
        },
    ] {
        assert!(graph.closure(&key(0x200), limits).is_err());
    }
    for limits in [
        GraphLimits {
            max_nodes: 0,
            ..Default::default()
        },
        GraphLimits {
            max_edges: 0,
            ..Default::default()
        },
    ] {
        assert!(Graph::build(&base, &c, limits).is_err());
    }
}
#[test]
fn changed_cohort_is_rejected_and_catalogue_limits_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut source = store(dir.path(), &["FalloutNV.esm", "Other.esm", "Addon.esm"]);
    let base = inventory::Catalogue::load(&mut source, inventory::Limits::default()).unwrap();
    for limits in [
        leveled::Limits {
            max_records: 0,
            ..Default::default()
        },
        leveled::Limits {
            max_decoded_bytes: 0,
            ..Default::default()
        },
        leveled::Limits {
            max_fields: 0,
            ..Default::default()
        },
        leveled::Limits {
            max_entries: 0,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut source, limits).is_err());
    }
    drop(source);
    let mut bytes = fs::read(dir.path().join("Other.esm")).unwrap();
    bytes.extend(disk(b"MISC", 0x888, 0, &[]));
    fs::write(dir.path().join("Other.esm"), bytes).unwrap();
    let mut source = store(dir.path(), &["FalloutNV.esm", "Other.esm", "Addon.esm"]);
    let changed = Catalogue::load(&mut source, leveled::Limits::default()).unwrap();
    assert!(Graph::build(&base, &changed, GraphLimits::default()).is_err());
}
