use fallout_data::{
    navigation::{self, Limits},
    plugin,
};
fn word(b: &mut Vec<u8>, v: u32) {
    b.extend(v.to_le_bytes());
}
fn half(b: &mut Vec<u8>, v: u16) {
    b.extend(v.to_le_bytes());
}
fn float(b: &mut Vec<u8>, v: f32) {
    b.extend(v.to_le_bytes());
}
fn field(b: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    b.extend(kind);
    half(b, data.len() as u16);
    b.extend(data);
}
fn record(kind: [u8; 4], payload: Vec<u8>) -> plugin::Record {
    plugin::Record {
        header: plugin::RecordHeader {
            kind,
            offset: 123,
            stored_size: payload.len() as u32,
            flags: 0,
            form_id: 0x123,
            revision: [0; 4],
            version: 15,
            trailing_bytes: [0; 2],
        },
        payload,
        integrity_issue: None,
    }
}
fn wire(kind: [u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(kind);
    word(&mut b, body.len() as u32);
    word(&mut b, 0);
    word(&mut b, id);
    word(&mut b, 0);
    half(&mut b, 15);
    half(&mut b, 0);
    b.extend(body);
    b
}
fn source_fixture(wrong_cell: bool) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Data")).unwrap();
    let mut header = Vec::new();
    let mut hedr = Vec::new();
    float(&mut hedr, 1.34);
    word(&mut hedr, 0);
    word(&mut hedr, 0);
    field(&mut header, b"HEDR", &hedr);
    let mut plugin = wire(*b"TES4", 0, &header);
    let mut cell = Vec::new();
    field(&mut cell, b"EDID", b"AuthoredCell\0");
    field(&mut cell, b"DATA", &[1]);
    plugin.extend(wire(*b"CELL", 0x10, &cell));
    let mut nav = mesh();
    if wrong_cell {
        nav.payload[16..20].copy_from_slice(&0x20u32.to_le_bytes());
    }
    // This file has no masters; its own NAVM word uses source-local index zero.
    let mut edge_at = 0;
    plugin::visit_subrecords(&nav, "fixture", |s| {
        if s.kind == *b"NVEX" {
            edge_at = s.payload_offset + 6 + 4;
        }
        Ok(())
    })
    .unwrap();
    nav.payload[edge_at..edge_at + 4].copy_from_slice(&0x123u32.to_le_bytes());
    let body = wire(*b"NAVM", 0x123, &nav.payload);
    let mut group = b"GRUP".to_vec();
    word(&mut group, (body.len() + 24) as u32);
    word(&mut group, 0x10);
    word(&mut group, 6);
    group.extend([0; 8]);
    group.extend(body);
    plugin.extend(group);
    std::fs::write(root.path().join("Data").join("Authored.esm"), plugin).unwrap();
    root
}
fn mesh() -> plugin::Record {
    let mut b = Vec::new();
    field(&mut b, b"NVER", &11u32.to_le_bytes());
    let mut data = Vec::new();
    for v in [0x10, 4, 2, 1, 1, 1] {
        word(&mut data, v);
    }
    field(&mut b, b"DATA", &data);
    let mut vertices = Vec::new();
    for v in [0., 0., -0., 2., 0., 0., 0., 2., 0., 2., 2., 0.] {
        float(&mut vertices, v);
    }
    field(&mut b, b"NVVX", &vertices);
    let mut triangles = Vec::new();
    for v in [
        0, 1, 2, 0xffff, 1, 0xffff, 0, 0xabcd, 1, 3, 2, 0, 0xffff, 0, 1, 0x1234,
    ] {
        half(&mut triangles, v);
    }
    field(&mut b, b"NVTR", &triangles);
    let mut edges = Vec::new();
    word(&mut edges, 2);
    word(&mut edges, 0x0100_0123);
    half(&mut edges, 7);
    field(&mut b, b"NVEX", &edges);
    field(&mut b, b"NVCA", &1u16.to_le_bytes());
    let mut doors = Vec::new();
    word(&mut doors, 0x456);
    half(&mut doors, 1);
    doors.extend([0x81, 0x82]);
    field(&mut b, b"NVDP", &doors);
    field(&mut b, b"NVGD", &[9, 8, 7, 6, 5]);
    field(&mut b, b"ZZZZ", &[1, 2]);
    record(*b"NAVM", b)
}
#[test]
fn source_mesh_preserves_signed_edges_filters_special_links_door_bytes_and_unknown_fields() {
    let raw = mesh();
    let mesh = navigation::decode_mesh(&raw, "authored", Limits::default()).unwrap();
    assert_eq!(mesh.version.value, 11);
    assert_eq!(mesh.cell_raw.value, 0x10);
    assert_eq!(mesh.vertices[0][2].to_bits(), 0x8000_0000);
    assert_eq!(mesh.triangles[0].edges, [-1, 1, -1]);
    assert_eq!(mesh.triangles[0].cover_flags, 0xabcd);
    assert_eq!(mesh.edge_links[0].link_type, 2);
    assert_eq!(mesh.edge_links[0].navmesh_raw, 0x0100_0123);
    assert_eq!(mesh.edge_links[0].triangle, 7);
    assert_eq!(mesh.door_links[0].unused, [0x81, 0x82]);
    assert_eq!(mesh.fields.last().unwrap().bytes, [1, 2]);
    assert_eq!(mesh.fields[7].kind, *b"NVGD");
    assert_eq!(mesh.fields[7].bytes, [9, 8, 7, 6, 5]);
}
#[test]
fn source_mesh_rejects_counts_indices_duplicates_nonfinite_tainted_and_budgeted_inputs() {
    let raw = mesh();
    for limits in [
        Limits {
            record_bytes: 1,
            ..Limits::default()
        },
        Limits {
            fields: 1,
            ..Limits::default()
        },
        Limits {
            elements: 1,
            ..Limits::default()
        },
    ] {
        assert!(navigation::decode_mesh(&raw, "budget", limits).is_err());
    }
    for (kind, offset, value) in [
        (*b"DATA", 4, 5u32),
        (*b"NVVX", 0, f32::NAN.to_bits()),
        (*b"NVTR", 0, 99u32),
    ] {
        let mut altered = mesh();
        let mut at = 0;
        while at < altered.payload.len() {
            let len =
                u16::from_le_bytes(altered.payload[at + 4..at + 6].try_into().unwrap()) as usize;
            if altered.payload[at..at + 4] == kind {
                altered.payload[at + 6 + offset..at + 10 + offset]
                    .copy_from_slice(&value.to_le_bytes());
                break;
            }
            at += 6 + len;
        }
        assert!(navigation::decode_mesh(&altered, "altered", Limits::default()).is_err());
    }
    let mut altered = mesh();
    field(&mut altered.payload, b"NVER", &11u32.to_le_bytes());
    assert!(navigation::decode_mesh(&altered, "duplicate", Limits::default()).is_err());
    let mut altered = mesh();
    altered.header.flags = plugin::DELETED;
    assert!(navigation::decode_mesh(&altered, "deleted", Limits::default()).is_err());
    let mut altered = mesh();
    altered.integrity_issue = Some(plugin::ChecksumMismatch {
        file_offset: 123,
        form_id: 0x123,
        stored_adler32: 1,
        calculated_adler32: 2,
    });
    assert!(navigation::decode_mesh(&altered, "tainted", Limits::default()).is_err());
    // The first 170 bytes contain all required counted fields. Grid/unknown
    // optional fields can be absent at a valid subrecord boundary.
    for end in 0..170 {
        let mut truncated = mesh();
        truncated.payload.truncate(end);
        assert!(
            navigation::decode_mesh(&truncated, "truncated", Limits::default()).is_err(),
            "{end}"
        );
    }
}
#[test]
fn navi_islands_count_prefixes_and_authored_connection_order_are_preserved() {
    let mut payload = Vec::new();
    field(&mut payload, b"NVER", &11u32.to_le_bytes());
    let mut info = Vec::new();
    for v in [0x20, 0x123, 0x10] {
        word(&mut info, v);
    }
    half(&mut info, (-3i16) as u16);
    half(&mut info, 7);
    for v in [1., 2., 3., 0., 0., 0., 2., 2., 0.] {
        float(&mut info, v);
    }
    half(&mut info, 3);
    half(&mut info, 1);
    for v in [0., 0., 0., 2., 0., 0., 0., 2., 0.] {
        float(&mut info, v);
    }
    for v in [0, 1, 2] {
        half(&mut info, v);
    }
    float(&mut info, 0.75);
    field(&mut payload, b"NVMI", &info);
    let mut connection = Vec::new();
    for v in [0x123, 3, 0x234, 0x345, 0x234, 1, 0x345, 2, 0x456, 0x567] {
        word(&mut connection, v);
    }
    field(&mut payload, b"NVCI", &connection);
    let record = record(*b"NAVI", payload);
    let map = navigation::decode_info_map(&record, "navi", Limits::default()).unwrap();
    assert_eq!(map.infos[0].grid_y, -3);
    assert_eq!(map.infos[0].grid_x, 7);
    assert_eq!(map.infos[0].island.as_ref().unwrap().triangles, [[0, 1, 2]]);
    assert_eq!(map.infos[0].preferred_percent, 0.75);
    assert_eq!(map.connections[0].standard, [0x234, 0x345, 0x234]);
    assert_eq!(map.connections[0].preferred, [0x345]);
    assert_eq!(map.connections[0].doors, [0x456, 0x567]);
}

#[test]
fn selected_cell_loader_uses_winning_source_identity_and_checks_group_against_data() {
    use fallout_data::{
        identity::{FormKey, ProfileId},
        store::RecordStore,
    };
    let cell = FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "authored.esm".into(),
        local_id: 0x10,
    };
    for wrong in [false, true] {
        let root = source_fixture(wrong);
        let mut store = RecordStore::open_nv_headers(
            &root.path().join("Data"),
            &["Authored.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap();
        let result = navigation::load_cell(&mut store, &cell, 4, Limits::default());
        if wrong {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("differs from source GRUP")
            );
        } else {
            let sources = result.unwrap();
            assert_eq!(sources.len(), 1);
            assert_eq!(sources[0].key.local_id, 0x123);
            assert_eq!(sources[0].cell, Some(cell.clone()));
            assert_eq!(
                sources[0].external_targets[0]
                    .as_ref()
                    .unwrap()
                    .origin_plugin,
                "authored.esm"
            );
            assert_eq!(sources[0].source_sha256.len(), 64);
        }
    }
}
