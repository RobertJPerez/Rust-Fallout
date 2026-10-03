//! Original exterior plugins exercise source-local membership and strict reads.
use fallout_data::{
    plugin::{self, Record, RecordHeader},
    store::RecordStore,
    terrain::{self, Fields},
};
use flate2::{Compression, write::ZlibEncoder};
use std::{fs, io::Write};

fn sub(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
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
    let mut body = sub(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(sub(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(sub(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn group(label: u32, kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn decoded(kind: &[u8; 4], body: Vec<u8>) -> Record {
    Record {
        header: RecordHeader {
            kind: *kind,
            offset: 24,
            stored_size: body.len() as u32,
            flags: 0,
            form_id: 0x300,
            revision: [0; 4],
            version: 15,
            trailing_bytes: [0; 2],
        },
        payload: body,
        integrity_issue: None,
    }
}
fn parse(kind: &[u8; 4], body: Vec<u8>) -> fallout_data::Result<Fields> {
    terrain::decode(&decoded(kind, body), "fixture.esm")
}
fn cell(name: &[u8], id: u32) -> Vec<u8> {
    let body = [
        sub(b"EDID", &[name, &[0]].concat()),
        sub(b"DATA", &[0]),
        sub(
            b"XCLC",
            &[
                (-4i32).to_le_bytes(),
                7i32.to_le_bytes(),
                0x1234u32.to_le_bytes(),
            ]
            .concat(),
        ),
    ]
    .concat();
    record(b"CELL", id, 0, &body)
}
fn base() -> Vec<u8> {
    let mut bytes = header(&[]);
    bytes.extend(record(
        b"WRLD",
        0x100,
        0,
        &[sub(b"EDID", b"TestWorld\0"), sub(b"DATA", &[0])].concat(),
    ));
    bytes.extend(record(b"LTEX", 0x400, 0, &[]));
    let land = record(
        b"LAND",
        0x300,
        0,
        &[
            sub(b"DATA", &7u32.to_le_bytes()),
            sub(b"BTXT", &[0, 4, 0, 0, 0, 19, 0xff, 0xff]),
        ]
        .concat(),
    );
    let cells = [
        cell(b"FirstOutside", 0x200),
        group(0x200, 6, &group(0x200, 9, &land)),
        cell(b"SecondOutside", 0x201),
    ]
    .concat();
    bytes.extend(group(0x100, 1, &cells));
    bytes
}
fn setup(base: &[u8], patch: Option<&[u8]>) -> (tempfile::TempDir, tempfile::TempDir, RecordStore) {
    let source = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("Data")).unwrap();
    fs::write(source.path().join("Data/Base.esm"), base).unwrap();
    let mut names = vec!["Base.esm".into()];
    if let Some(patch) = patch {
        fs::write(source.path().join("Data/Patch.esp"), patch).unwrap();
        names.push("Patch.esp".into());
    }
    let store = RecordStore::open_nv_headers_cached(
        &source.path().join("Data"),
        &names,
        plugin::Limits::default(),
        cache.path(),
    )
    .unwrap();
    (source, cache, store)
}

#[test]
fn source_bits_signed_deltas_padding_and_unknown_fields_survive() {
    let mut height = 0x80000000u32.to_le_bytes().to_vec();
    height.extend([0; 1089]);
    height[4] = 128;
    height[5] = 255;
    height[6] = 127;
    height.extend([11, 22, 33]);
    let body = [
        sub(b"VHGT", &height),
        sub(b"VNML", &vec![255; 3267]),
        sub(b"VCLR", &vec![7; 3267]),
        sub(b"ZZZZ", &[0, 255]),
    ]
    .concat();
    let Fields::Land(land) = parse(b"LAND", body).unwrap() else {
        panic!()
    };
    let heights = land.heights.unwrap();
    assert_eq!(heights.decoded_offset, 0);
    assert_eq!(heights.value.offset_bits, 0x80000000);
    assert_eq!(&heights.value.deltas[..3], &[-128, -1, 127]);
    assert_eq!(heights.value.deltas.len(), 1089);
    assert_eq!(heights.value.unused, [11, 22, 33]);
    assert_eq!(land.normals.unwrap().value[0], [255; 3]);
    assert_eq!(land.colors.unwrap().value.len(), 1089);
    assert_eq!(land.unhandled[0].bytes, [0, 255]);
    assert!(land.flags.is_none());
}

#[test]
fn texture_layer_order_signed_layer_and_alpha_extremes_survive() {
    let header = [0, 4, 0, 0, 3, 37, 0xff, 0xff];
    let alpha = [
        0u16.to_le_bytes().as_slice(),
        &[8, 9],
        &0.25f32.to_le_bytes(),
        &288u16.to_le_bytes(),
        &[10, 11],
        &(-0.0f32).to_le_bytes(),
    ]
    .concat();
    let body = [
        sub(b"BTXT", &header),
        sub(b"ATXT", &header),
        sub(b"VTXT", &alpha),
    ]
    .concat();
    let Fields::Land(land) = parse(b"LAND", body).unwrap() else {
        panic!()
    };
    assert_eq!(land.layers.len(), 2);
    assert_eq!(land.layers[0].kind, "BTXT");
    assert_eq!(land.layers[1].texture_raw, 0x400);
    assert_eq!(land.layers[1].quadrant, 3);
    assert_eq!(land.layers[1].unused, 37);
    assert_eq!(land.layers[1].layer, -1);
    let alpha = land.layers[1].alpha.as_ref().unwrap();
    assert_eq!(alpha.value[1].position, 288);
    assert_eq!(alpha.value[1].unused, [10, 11]);
    assert_eq!(alpha.value[1].opacity_bits, 0x80000000);
}

#[test]
fn absent_world_fields_do_not_acquire_editor_defaults() {
    let Fields::World(world) = parse(b"WRLD", sub(b"EDID", b"World\0")).unwrap() else {
        panic!()
    };
    assert!(world.default_height_bits.is_none());
    assert!(world.climate.is_none());
    assert!(world.water.is_none());
    assert!(world.flags.is_none());
    let Fields::World(world) = parse(
        b"WRLD",
        sub(
            b"DNAM",
            &[(-0.0f32).to_le_bytes(), 128f32.to_le_bytes()].concat(),
        ),
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        world.default_height_bits.unwrap().value,
        [0x80000000, 0x43000000]
    );
}

#[test]
fn malformed_fixed_fields_duplicates_and_nonfinite_numbers_fail() {
    for (kind, sig, size) in [
        (b"LAND", b"VHGT", 1096),
        (b"LAND", b"VNML", 3267),
        (b"LAND", b"VCLR", 3267),
        (b"LAND", b"BTXT", 8),
        (b"WRLD", b"DNAM", 8),
        (b"WRLD", b"PNAM", 2),
    ] {
        for bad in [0, size - 1, size + 1] {
            assert!(parse(kind, sub(sig, &vec![0; bad])).is_err());
        }
    }
    assert!(
        parse(
            b"LAND",
            [sub(b"DATA", &[0; 4]), sub(b"DATA", &[0; 4])].concat()
        )
        .is_err()
    );
    assert!(
        parse(
            b"CELL",
            [sub(b"XCLC", &[0; 8]), sub(b"XCLC", &[0; 8])].concat()
        )
        .is_err()
    );
    for bits in [
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        f32::NAN.to_bits(),
    ] {
        assert!(parse(b"WRLD", sub(b"NAM4", &bits.to_le_bytes())).is_err());
        let mut height = vec![0; 1096];
        height[..4].copy_from_slice(&bits.to_le_bytes());
        assert!(parse(b"LAND", sub(b"VHGT", &height)).is_err());
    }
    assert!(parse(b"WRLD", sub(b"EDID", b"missing terminator")).is_err());
}

#[test]
fn alpha_framing_positions_and_quadrants_are_bounded() {
    let header = [0; 8];
    assert!(parse(b"LAND", sub(b"VTXT", &[0; 8])).is_err());
    assert!(
        parse(
            b"LAND",
            [sub(b"BTXT", &header), sub(b"VTXT", &[0; 8])].concat()
        )
        .is_err()
    );
    assert!(
        parse(
            b"LAND",
            [sub(b"ATXT", &header), sub(b"VTXT", &[0; 7])].concat()
        )
        .is_err()
    );
    let bad = [289u16.to_le_bytes().as_slice(), &[0; 6]].concat();
    assert!(
        parse(
            b"LAND",
            [sub(b"ATXT", &header), sub(b"VTXT", &bad)].concat()
        )
        .is_err()
    );
    assert!(parse(b"LAND", sub(b"ATXT", &[0, 0, 0, 0, 4, 0, 0, 0])).is_err());
    assert!(
        parse(
            b"LAND",
            [sub(b"ATXT", &header), sub(b"VTXT", &[]), sub(b"VTXT", &[])].concat()
        )
        .is_err()
    );
}

#[test]
fn indexed_exterior_retains_source_grid_and_correct_land_links() {
    let (source, _cache, mut store) = setup(&base(), None);
    let bodies = tempfile::tempdir().unwrap();
    let report = terrain::inspect_cell(
        &mut store,
        b"FirstOutside",
        Some((bodies.path(), source.path())),
    )
    .unwrap();
    assert!(!report.runtime_ready);
    assert_eq!(report.link_failures, 0);
    assert_eq!(report.world_chain.len(), 1);
    assert_eq!(report.landscapes.len(), 1);
    let Some(Fields::Cell(cell)) = report.cell.fields else {
        panic!()
    };
    assert_eq!(cell.grid.unwrap().value, [-4, 7]);
    assert_eq!(cell.quadrant_flags.unwrap().value, 0x1234);
    assert_eq!(
        report.landscapes[0].links["layer[0].texture"]
            .key
            .as_ref()
            .unwrap()
            .local_id,
        0x400
    );
    let receipt = report.landscapes[0].body_cache.as_ref().unwrap();
    assert!(
        fs::read(bodies.path().join(format!("{}.blob", receipt.key)))
            .unwrap()
            .starts_with(b"LAND")
    );
    assert_eq!(
        terrain::inspect_cell(&mut store, b"SecondOutside", None)
            .unwrap()
            .landscapes
            .len(),
        0
    );
}

#[test]
fn moved_land_override_uses_its_winning_cell_and_replaces_whole_body() {
    let mut patch = header(&["Base.esm"]);
    patch.extend(group(
        0x100,
        1,
        &group(
            0x201,
            6,
            &group(
                0x201,
                9,
                &record(b"LAND", 0x300, 0, &sub(b"DATA", &11u32.to_le_bytes())),
            ),
        ),
    ));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    assert!(
        terrain::inspect_cell(&mut store, b"FirstOutside", None)
            .unwrap()
            .landscapes
            .is_empty()
    );
    let report = terrain::inspect_cell(&mut store, b"SecondOutside", None).unwrap();
    let Some(Fields::Land(land)) = &report.landscapes[0].fields else {
        panic!()
    };
    assert_eq!(land.flags.as_ref().unwrap().value, 11);
    assert!(land.layers.is_empty());
    assert_eq!(report.landscapes[0].source_plugin, "Patch.esp");
}

#[test]
fn deleted_land_is_a_tombstone_and_never_falls_back() {
    let mut patch = header(&["Base.esm"]);
    patch.extend(group(
        0x100,
        1,
        &group(
            0x200,
            6,
            &group(0x200, 9, &record(b"LAND", 0x300, plugin::DELETED, &[])),
        ),
    ));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    let report = terrain::inspect_cell(&mut store, b"FirstOutside", None).unwrap();
    assert_eq!(report.landscapes.len(), 1);
    assert!(report.landscapes[0].fields.is_none());
    assert!(report.landscapes[0].body_cache.is_none());
    assert_eq!(report.landscapes[0].source_plugin, "Patch.esp");
}

#[test]
fn parent_world_cycles_missing_and_wrong_kinds_fail_without_recursion() {
    for target_kind in [b"WRLD", b"SCPT"] {
        let mut bytes = base();
        bytes.extend(record(
            target_kind,
            0x101,
            0,
            &sub(b"WNAM", &0x100u32.to_le_bytes()),
        ));
        let mut patch = header(&["Base.esm"]);
        patch.extend(record(
            b"WRLD",
            0x100,
            0,
            &sub(b"WNAM", &0x101u32.to_le_bytes()),
        ));
        let (_source, _cache, mut store) = setup(&bytes, Some(&patch));
        let error = terrain::inspect_cell(&mut store, b"FirstOutside", None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(if target_kind == b"WRLD" {
            "cycle"
        } else {
            "wrong-record-kind"
        }));
    }
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(
        b"WRLD",
        0x100,
        0,
        &sub(b"WNAM", &0x999u32.to_le_bytes()),
    ));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    assert!(
        terrain::inspect_cell(&mut store, b"FirstOutside", None)
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
}

#[test]
fn optional_texture_links_are_checked_for_record_kind() {
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(b"SCPT", 0x400, 0, &[]));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    let report = terrain::inspect_cell(&mut store, b"FirstOutside", None).unwrap();
    assert_eq!(report.link_failures, 1);
    assert_eq!(
        report.landscapes[0].links["layer[0].texture"].status,
        "wrong-record-kind"
    );
}

#[test]
fn checksum_damage_in_selected_land_remains_a_strict_failure() {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&sub(b"DATA", &[0; 4])).unwrap();
    let mut compressed = encoder.finish().unwrap();
    *compressed.last_mut().unwrap() ^= 1;
    let stored = [10u32.to_le_bytes().as_slice(), &compressed].concat();
    let mut patch = header(&["Base.esm"]);
    patch.extend(group(
        0x100,
        1,
        &group(
            0x200,
            6,
            &group(
                0x200,
                9,
                &record(b"LAND", 0x300, plugin::COMPRESSED, &stored),
            ),
        ),
    ));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    assert!(terrain::inspect_cell(&mut store, b"FirstOutside", None).is_err());
}

#[test]
fn body_exports_refuse_the_source_tree_and_mutated_fields_never_panic() {
    let (source, _cache, mut store) = setup(&base(), None);
    assert!(
        terrain::inspect_cell(
            &mut store,
            b"FirstOutside",
            Some((source.path(), source.path()))
        )
        .is_err()
    );
    let body = [
        sub(b"VHGT", &vec![0; 1096]),
        sub(b"ATXT", &[0; 8]),
        sub(b"VTXT", &[0; 8]),
    ]
    .concat();
    for i in 0..256usize {
        let mut mutated = body.clone();
        mutated[i * 7 % body.len()] ^= (i as u8).wrapping_add(1);
        let result = std::panic::catch_unwind(|| parse(b"LAND", mutated));
        assert!(result.is_ok());
    }
}

#[test]
fn master_relative_group_labels_rebind_to_the_current_winning_cell() {
    let source = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("Data")).unwrap();
    fs::write(source.path().join("Data/Aux.esm"), header(&[])).unwrap();
    fs::write(source.path().join("Data/Base.esm"), base()).unwrap();
    let mut patch = header(&["Aux.esm", "Base.esm"]);
    patch.extend(group(
        0x01000100,
        1,
        &group(
            0x01000201,
            6,
            &group(
                0x01000201,
                9,
                &record(b"LAND", 0x01000300, 0, &sub(b"DATA", &15u32.to_le_bytes())),
            ),
        ),
    ));
    fs::write(source.path().join("Data/Patch.esp"), patch).unwrap();
    let names = vec!["Aux.esm".into(), "Base.esm".into(), "Patch.esp".into()];
    for _ in 0..2 {
        let mut store = RecordStore::open_nv_headers_cached(
            &source.path().join("Data"),
            &names,
            plugin::Limits::default(),
            cache.path(),
        )
        .unwrap();
        assert!(
            terrain::inspect_cell(&mut store, b"FirstOutside", None)
                .unwrap()
                .landscapes
                .is_empty()
        );
        let report = terrain::inspect_cell(&mut store, b"SecondOutside", None).unwrap();
        assert_eq!(report.landscapes[0].key.origin_plugin, "base.esm");
        assert_eq!(report.landscapes[0].key.local_id, 0x300);
        assert_eq!(report.world_chain[0].key.origin_plugin, "base.esm");
    }
}

#[test]
fn parent_world_fields_remain_separate_and_source_digests_match() {
    use sha2::{Digest, Sha256};
    let mut bytes = base();
    bytes.extend(record(
        b"WRLD",
        0x101,
        0,
        &sub(
            b"DNAM",
            &[128f32.to_le_bytes(), 32f32.to_le_bytes()].concat(),
        ),
    ));
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(
        b"WRLD",
        0x100,
        0,
        &[
            sub(b"WNAM", &0x101u32.to_le_bytes()),
            sub(b"PNAM", &1u16.to_le_bytes()),
        ]
        .concat(),
    ));
    let (source, _cache, mut store) = setup(&bytes, Some(&patch));
    let report = terrain::inspect_cell(&mut store, b"FirstOutside", None).unwrap();
    assert_eq!(report.world_chain.len(), 2);
    let Some(Fields::World(child)) = &report.world_chain[0].fields else {
        panic!()
    };
    let Some(Fields::World(parent)) = &report.world_chain[1].fields else {
        panic!()
    };
    assert!(child.default_height_bits.is_none());
    assert_eq!(child.parent_flags.as_ref().unwrap().value, 1);
    assert!(parent.default_height_bits.is_some());
    assert_eq!(
        report.world_chain[0].source_sha256,
        format!(
            "{:x}",
            Sha256::digest(fs::read(source.path().join("Data/Patch.esp")).unwrap())
        )
    );
    drop(store);
    let mut ordinary = RecordStore::open_nv_headers(
        &source.path().join("Data"),
        &["Base.esm".into(), "Patch.esp".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let actual = terrain::inspect_cell(&mut ordinary, b"FirstOutside", None).unwrap();
    let mut expected = serde_json::to_value(report).unwrap();
    expected.as_object_mut().unwrap().remove("index_cache");
    assert_eq!(serde_json::to_value(actual).unwrap(), expected);
}

#[test]
fn long_world_chains_are_bounded_and_bad_land_world_membership_fails() {
    let mut bytes = base();
    for id in 0x101u32..=0x201 {
        bytes.extend(record(
            b"WRLD",
            id + 0x1000,
            0,
            &sub(
                b"WNAM",
                &(if id == 0x201 { 0 } else { id + 0x1001 }).to_le_bytes(),
            ),
        ));
    }
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(
        b"WRLD",
        0x100,
        0,
        &sub(b"WNAM", &0x1101u32.to_le_bytes()),
    ));
    let (_source, _cache, mut store) = setup(&bytes, Some(&patch));
    assert!(
        terrain::inspect_cell(&mut store, b"FirstOutside", None)
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    let mut patch = header(&["Base.esm"]);
    patch.extend(group(
        0x999,
        1,
        &group(0x200, 6, &group(0x200, 9, &record(b"LAND", 0x300, 0, &[]))),
    ));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    assert!(
        terrain::inspect_cell(&mut store, b"FirstOutside", None)
            .unwrap_err()
            .to_string()
            .contains("world group")
    );
}

#[test]
fn many_tiny_fields_cannot_expand_into_unbounded_owned_metadata() {
    let empty = sub(b"ZZZZ", &[]);
    let mut body = Vec::with_capacity(6_000_000);
    for _ in 0..1_000_000 {
        body.extend(&empty);
    }
    let error = parse(b"LAND", body).unwrap_err().to_string();
    assert!(error.contains("retained-field budget"));
}

#[test]
fn missing_and_deleted_land_do_not_acquire_default_heights() {
    let (_source, _cache, mut store) = setup(&base(), None);
    let report = terrain::inspect_cell(&mut store, b"FirstOutside", None).unwrap();
    let surface = terrain::reconstruct_cell(&report).unwrap();
    assert_eq!(surface.landscapes[0].status, "missing_vhgt");
    assert!(surface.landscapes[0].height_grid.is_none());
    let mut patch = header(&["Base.esm"]);
    patch.extend(group(
        0x100,
        1,
        &group(
            0x200,
            6,
            &group(0x200, 9, &record(b"LAND", 0x300, plugin::DELETED, &[])),
        ),
    ));
    let (_source, _cache, mut store) = setup(&base(), Some(&patch));
    let report = terrain::inspect_cell(&mut store, b"FirstOutside", None).unwrap();
    let surface = terrain::reconstruct_cell(&report).unwrap();
    assert_eq!(surface.landscapes[0].status, "deleted");
    assert!(surface.landscapes[0].height_field_offset.is_none());
    assert!(surface.landscapes[0].height_grid.is_none());
    assert!(terrain::compare_neighbor(&surface, &surface).is_err());
}

#[test]
fn unnamed_exterior_cell_uses_origin_identity_and_rejects_wrong_targets() {
    use fallout_data::identity::{FormKey, ProfileId};
    let mut bytes = header(&[]);
    bytes.extend(record(b"WRLD", 0x100, 0, &[]));
    let cell = record(
        b"CELL",
        0x200,
        0,
        &[
            sub(b"DATA", &[0]),
            sub(
                b"XCLC",
                &[(-18i32).to_le_bytes(), 0i32.to_le_bytes()].concat(),
            ),
        ]
        .concat(),
    );
    bytes.extend(group(0x100, 1, &cell));
    let (_source, _cache, mut store) = setup(&bytes, None);
    let mut key = FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: 0x200,
    };
    let report = terrain::inspect_cell_key(&mut store, &key, None).unwrap();
    let surface = terrain::reconstruct_cell(&report).unwrap();
    assert_eq!(surface.coordinates, [-18, 0]);
    assert!(surface.landscapes.is_empty());
    key.local_id = 0x100;
    assert!(terrain::inspect_cell_key(&mut store, &key, None).is_err());
    key.local_id = 0x999;
    assert!(terrain::inspect_cell_key(&mut store, &key, None).is_err());
}

#[test]
fn edge_comparison_rejects_world_mismatch_and_ambiguous_land() {
    let (_source, _cache, mut store) = setup(&base(), None);
    let report = terrain::inspect_cell(&mut store, b"FirstOutside", None).unwrap();
    let mut first = terrain::reconstruct_cell(&report).unwrap();
    let mut second = terrain::reconstruct_cell(&report).unwrap();
    second.world.local_id += 1;
    assert!(
        terrain::compare_neighbor(&first, &second)
            .unwrap_err()
            .to_string()
            .contains("different worldspaces")
    );
    second.world = first.world.clone();
    first.landscapes.push(terrain::SurfaceLand {
        key: first.landscapes[0].key.clone(),
        decoded_sha256: None,
        height_field_offset: None,
        status: "deleted",
        height_grid: None,
    });
    assert!(
        terrain::compare_neighbor(&first, &second)
            .unwrap_err()
            .to_string()
            .contains("exactly one LAND")
    );
}
