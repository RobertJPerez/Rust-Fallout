use super::*;
use crate::{identity::ProfileId, inventory::Status};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
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
fn member(id: u32) -> Vec<u8> {
    field(b"LNAM", &id.to_le_bytes())
}
fn key(plugin: &str, id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: plugin.into(),
        local_id: id,
    }
}
fn store(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn write(path: &Path, records: &[Vec<u8>]) {
    fs::write(
        path.join("Base.esm"),
        [header(&[]), records.concat()].concat(),
    )
    .unwrap();
}
#[test]
fn physical_order_duplicates_nulls_and_unknown_bytes_survive_store_lifetime() {
    let folder = tempfile::tempdir().unwrap();
    let body = [
        field(b"EDID", b"AuthoredOrderedList\0"),
        member(0x202),
        field(b"ZZZZ", &[255, 0, 1]),
        member(0),
        member(0x201),
        member(0x202),
    ]
    .concat();
    write(
        folder.path(),
        &[
            disk(b"MISC", 0x201, 0, &[]),
            disk(b"NPC_", 0x202, 0, &[]),
            disk(b"FLST", 0x100, 0, &body),
        ],
    );
    let mut source = store(folder.path(), &["Base.esm"]);
    let lists = Catalogue::load(&mut source, Limits::default()).unwrap();
    drop(source);
    let list = lists.get(&key("base.esm", 0x100)).unwrap();
    assert_eq!(list.entries, vec![1, 3, 4, 5]);
    assert_eq!(list.record().unwrap().payload, body);
    let forms = list
        .entries
        .iter()
        .map(|&i| match &list.fields[i].value {
            Value::Member { form } => form.raw_form,
            _ => panic!("member expected"),
        })
        .collect::<Vec<_>>();
    assert_eq!(forms, vec![0x202, 0, 0x201, 0x202]);
    assert_eq!(lists.counts.binding_statuses["null"], 1);
    assert_eq!(lists.counts.binding_statuses["defined"], 3);
    assert!(matches!(list.fields[2].value, Value::Opaque));
    assert_eq!(
        list.fields[2].sha256,
        format!("{:x}", Sha256::digest([255, 0, 1]))
    );
}
#[test]
fn source_namespace_overrides_whole_records_and_tombstones_are_exact() {
    let folder = tempfile::tempdir().unwrap();
    write(
        folder.path(),
        &[
            disk(b"MISC", 0x200, 0, &[]),
            disk(b"FLST", 0x100, 0, &member(0x200)),
            disk(b"FLST", 0x101, 0, &member(0x200)),
        ],
    );
    fs::write(
        folder.path().join("Other.esm"),
        [
            header(&[]),
            disk(b"WEAP", 0x200, 0, &[]),
            disk(b"FLST", 0x100, 0, &member(0x200)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        folder.path().join("Patch.esp"),
        [
            header(&["Base.esm", "Other.esm"]),
            disk(b"FLST", 0x100, 0, &member(0x01000200)),
            disk(b"FLST", 0x101, plugin::DELETED, b"bad tombstone body"),
            disk(b"MISC", 0x01000100, 0, &[]),
            disk(b"FLST", 0x02000100, 0, &member(0x200)),
        ]
        .concat(),
    )
    .unwrap();
    let mut source = store(folder.path(), &["Base.esm", "Other.esm", "Patch.esp"]);
    let lists = Catalogue::load(&mut source, Limits::default()).unwrap();
    assert_eq!(lists.counts.records, 3);
    assert_eq!(lists.counts.entries, 2);
    assert!(lists.get(&key("other.esm", 0x100)).is_none());
    let overridden = lists.get(&key("base.esm", 0x100)).unwrap();
    assert_eq!(overridden.source.plugin, "Patch.esp");
    let Value::Member { form } = &overridden.fields[0].value else {
        panic!("member expected")
    };
    assert_eq!(form.key, Some(key("other.esm", 0x200)));
    assert_eq!(form.target.as_ref().unwrap().kind, *b"WEAP");
    let deleted = lists.get(&key("base.esm", 0x101)).unwrap();
    assert!(deleted.deleted && deleted.record().is_none() && deleted.fields.is_empty());
    assert!(deleted.source.decoded_record_sha256.is_none());
}
#[test]
fn structural_cycles_keep_duplicate_edges_and_unresolved_terminals() {
    let folder = tempfile::tempdir().unwrap();
    write(
        folder.path(),
        &[
            disk(b"MISC", 0x200, 0, &[]),
            disk(
                b"FLST",
                0x100,
                0,
                &[
                    member(0x101),
                    member(0x101),
                    member(0x200),
                    member(0),
                    member(0x999),
                    member(0x103),
                ]
                .concat(),
            ),
            disk(b"FLST", 0x101, 0, &member(0x100)),
            disk(b"FLST", 0x102, 0, &member(0x102)),
            disk(b"FLST", 0x103, plugin::DELETED, b"bad"),
            disk(b"FLST", 0x104, 0, &[]),
        ],
    );
    let mut source = store(folder.path(), &["Base.esm"]);
    let lists = Catalogue::load(&mut source, Limits::default()).unwrap();
    let graph = graph::Graph::build(&lists, graph::Limits::default()).unwrap();
    assert_eq!(
        graph.cycles(),
        vec![
            vec![key("base.esm", 0x100), key("base.esm", 0x101)],
            vec![key("base.esm", 0x102)]
        ]
    );
    assert_eq!(
        (
            graph.counts().internal_edges,
            graph.counts().terminal_edges,
            graph.counts().unresolved_edges
        ),
        (4, 1, 3)
    );
    assert_eq!(graph.edges()[3].status, Status::Null);
    assert_eq!(graph.edges()[4].status, Status::Missing);
    assert_eq!(graph.edges()[5].status, Status::Deleted);
    let closure = graph
        .closure(&key("base.esm", 0x100), graph::Limits::default())
        .unwrap();
    assert_eq!(
        closure.nodes,
        vec![key("base.esm", 0x100), key("base.esm", 0x101)]
    );
    assert_eq!(closure.edge_indices, vec![0, 1, 2, 3, 4, 5, 6]);
    assert!(
        graph
            .closure(&key("base.esm", 0x103), graph::Limits::default())
            .is_err()
    );
    assert!(
        graph
            .closure(&key("base.esm", 0x999), graph::Limits::default())
            .is_err()
    );
    assert!(
        graph
            .closure(&key("base.esm", 0x104), graph::Limits::default())
            .unwrap()
            .edge_indices
            .is_empty()
    );
}
#[test]
fn budgets_reject_incomplete_catalogues_and_closures() {
    let folder = tempfile::tempdir().unwrap();
    write(
        folder.path(),
        &[
            disk(b"FLST", 0x100, 0, &member(0x101)),
            disk(b"FLST", 0x101, 0, &member(0x100)),
        ],
    );
    let mut source = store(folder.path(), &["Base.esm"]);
    for limits in [
        Limits {
            max_records: 1,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: 19,
            ..Default::default()
        },
        Limits {
            max_fields: 1,
            ..Default::default()
        },
        Limits {
            max_entries: 1,
            ..Default::default()
        },
    ] {
        assert!(Catalogue::load(&mut source, limits).is_err());
    }
    let lists = Catalogue::load(
        &mut source,
        Limits {
            max_records: 2,
            max_decoded_bytes: 20,
            max_fields: 2,
            max_entries: 2,
        },
    )
    .unwrap();
    for limits in [
        graph::Limits {
            max_nodes: 1,
            max_edges: 2,
        },
        graph::Limits {
            max_nodes: 2,
            max_edges: 1,
        },
    ] {
        assert!(graph::Graph::build(&lists, limits).is_err());
    }
    let graph = graph::Graph::build(&lists, graph::Limits::default()).unwrap();
    for limits in [
        graph::Limits {
            max_nodes: 0,
            max_edges: 2,
        },
        graph::Limits {
            max_nodes: 1,
            max_edges: 2,
        },
        graph::Limits {
            max_nodes: 2,
            max_edges: 1,
        },
    ] {
        assert!(graph.closure(&key("base.esm", 0x100), limits).is_err());
    }
}
#[test]
fn malformed_member_extents_and_extended_framing_are_rejected() {
    for body in [
        field(b"LNAM", &[]),
        field(b"LNAM", &[0; 3]),
        field(b"LNAM", &[0; 5]),
        field(b"LNAM", &[0; 8]),
        b"LNAM".to_vec(),
        [b"LNAM".as_slice(), &4_u16.to_le_bytes(), &[0; 3]].concat(),
        field(b"XXXX", &4_u32.to_le_bytes()),
        [
            field(b"XXXX", &4_u32.to_le_bytes()),
            field(b"XXXX", &4_u32.to_le_bytes()),
            member(0),
        ]
        .concat(),
        field(b"XXXX", &[0; 3]),
    ] {
        let folder = tempfile::tempdir().unwrap();
        write(folder.path(), &[disk(b"FLST", 0x100, 0, &body)]);
        let mut source = store(folder.path(), &["Base.esm"]);
        assert!(Catalogue::load(&mut source, Limits::default()).is_err());
    }
}
#[test]
fn extended_member_keeps_physical_offset_and_empty_list_is_explicit() {
    let folder = tempfile::tempdir().unwrap();
    write(
        folder.path(),
        &[
            disk(
                b"FLST",
                0x100,
                0,
                &[field(b"XXXX", &4_u32.to_le_bytes()), member(0)].concat(),
            ),
            disk(b"FLST", 0x101, 0, &[]),
        ],
    );
    let mut source = store(folder.path(), &["Base.esm"]);
    let lists = Catalogue::load(&mut source, Limits::default()).unwrap();
    assert_eq!(
        lists.get(&key("base.esm", 0x100)).unwrap().fields[0].decoded_offset,
        10
    );
    let empty = lists.get(&key("base.esm", 0x101)).unwrap();
    assert!(!empty.deleted && empty.record().is_some() && empty.entries.is_empty());
    assert!(empty.source.decoded_record_sha256.is_some());
}
#[test]
fn diagnostic_checksum_inputs_cannot_supply_form_lists() {
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let folder = tempfile::tempdir().unwrap();
    let body = member(0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&body).unwrap();
    let mut frame = encoder.finish().unwrap();
    *frame.last_mut().unwrap() ^= 1;
    write(
        folder.path(),
        &[disk(
            b"FLST",
            0x100,
            plugin::COMPRESSED,
            &[&(body.len() as u32).to_le_bytes(), frame.as_slice()].concat(),
        )],
    );
    assert!(
        RecordStore::open_nv_headers(
            folder.path(),
            &["Base.esm".into()],
            plugin::Limits {
                inspect_checksum_mismatches: true,
                ..Default::default()
            },
        )
        .is_err()
    );
    let mut source = store(folder.path(), &["Base.esm"]);
    assert!(Catalogue::load(&mut source, Limits::default()).is_err());
}

#[test]
fn graph_budgets_use_immutable_members_instead_of_editable_census_counts() {
    let folder = tempfile::tempdir().unwrap();
    write(
        folder.path(),
        &[
            disk(b"FLST", 0x100, 0, &member(0x101)),
            disk(b"FLST", 0x101, 0, &member(0x100)),
        ],
    );
    let mut source = store(folder.path(), &["Base.esm"]);
    let mut lists = Catalogue::load(&mut source, Limits::default()).unwrap();
    lists.counts.records = 0;
    lists.counts.entries = 0;
    assert!(
        graph::Graph::build(
            &lists,
            graph::Limits {
                max_nodes: 1,
                max_edges: 2
            }
        )
        .is_err()
    );
    assert!(
        graph::Graph::build(
            &lists,
            graph::Limits {
                max_nodes: 2,
                max_edges: 1
            }
        )
        .is_err()
    );
}
