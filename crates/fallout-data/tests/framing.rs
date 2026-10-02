//! Original fixtures: small byte streams, never snippets of retail plugins.
use fallout_data::{
    content,
    identity::ProfileId,
    plugin::{self, Event, Limits},
};
use flate2::{Compression, write::ZlibEncoder};
use std::io::Write;

fn sub(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((data.len() as u16).to_le_bytes());
    out.extend(data);
    out
}
fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend([0; 4]);
    out.extend(15u16.to_le_bytes());
    out.extend([0; 2]);
    out.extend(body);
    out
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut hedr = 1.34f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let mut body = sub(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        body.extend(sub(b"MAST", &name));
        body.extend(sub(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn group(body: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend((body.len() as u32 + 24).to_le_bytes());
    out.extend(b"MISC");
    out.extend([0; 12]);
    out.extend(body);
    out
}
fn validate(bytes: &[u8]) -> fallout_data::Result<()> {
    plugin::visit(
        &mut &bytes[..],
        bytes.len() as u64,
        "fixture",
        Limits::default(),
        |event| {
            if let Event::Record(r) = event {
                plugin::visit_subrecords(r, "fixture", |_| Ok(()))?;
            }
            Ok(())
        },
    )
}

#[test]
fn compressed_nested_record_retains_unknown_fields_and_exact_offsets() {
    let body = sub(b"UNKN", &[1, 2, 3, 4]);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&body).unwrap();
    let mut packed = (body.len() as u32).to_le_bytes().to_vec();
    packed.extend(z.finish().unwrap());
    let mut bytes = header(&[]);
    let expected_offset = bytes.len() as u64 + 48;
    bytes.extend(group(&group(&record(
        b"MISC",
        0x123,
        plugin::COMPRESSED,
        &packed,
    ))));
    let mut seen = 0;
    plugin::visit(
        &mut &bytes[..],
        bytes.len() as u64,
        "fixture",
        Limits::default(),
        |event| {
            if let Event::Record(r) = event
                && r.header.kind == *b"MISC"
            {
                assert_eq!(r.payload, body);
                assert_eq!(r.header.offset, expected_offset);
                seen += 1;
                plugin::visit_subrecords(r, "fixture", |s| {
                    assert_eq!(s.kind, *b"UNKN");
                    assert_eq!(s.data, [1, 2, 3, 4]);
                    Ok(())
                })?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(seen, 1);
}

#[test]
fn extended_subrecord_ignores_short_size_and_rejects_orphans() {
    let mut body = sub(b"XXXX", &70000u32.to_le_bytes());
    body.extend(b"DATA\0\0");
    body.extend(vec![0x42; 70000]);
    let mut bytes = header(&[]);
    bytes.extend(record(b"MISC", 1, 0, &body));
    validate(&bytes).unwrap();
    let mut orphan = header(&[]);
    orphan.extend(record(b"MISC", 1, 0, &sub(b"XXXX", &4u32.to_le_bytes())));
    assert!(validate(&orphan).is_err());
}

#[test]
fn truncation_and_bad_parent_bounds_return_errors() {
    let mut bytes = header(&[]);
    let prefix = bytes.len();
    bytes.extend(group(&record(b"MISC", 1, 0, &sub(b"EDID", b"Test\0"))));
    for cut in 0..bytes.len() {
        if cut != prefix {
            assert!(
                validate(&bytes[..cut]).is_err(),
                "accepted truncated length {cut}"
            );
        }
    }
    bytes[prefix + 4..prefix + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(validate(&bytes).is_err());
}

#[test]
fn rejects_zlib_size_lies_trailing_bytes_and_checksum_damage() {
    let body = sub(b"DATA", &[1; 40]);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&body).unwrap();
    let packed = z.finish().unwrap();
    for claimed in [0, 1, 45, 47, u32::MAX] {
        let mut payload = claimed.to_le_bytes().to_vec();
        payload.extend(&packed);
        let mut bytes = header(&[]);
        bytes.extend(record(b"MISC", 1, plugin::COMPRESSED, &payload));
        assert!(validate(&bytes).is_err());
    }
    for suffix in [vec![0], vec![1, 2, 3]] {
        let mut payload = (body.len() as u32).to_le_bytes().to_vec();
        payload.extend(&packed);
        payload.extend(suffix);
        let mut bytes = header(&[]);
        bytes.extend(record(b"MISC", 1, plugin::COMPRESSED, &payload));
        assert!(validate(&bytes).is_err());
    }
    let mut payload = (body.len() as u32).to_le_bytes().to_vec();
    payload.extend(packed);
    let last = payload.len() - 1;
    payload[last] ^= 0xff;
    let mut bytes = header(&[]);
    bytes.extend(record(b"MISC", 1, plugin::COMPRESSED, &payload));
    assert!(validate(&bytes).is_err());
}

#[test]
fn group_depth_limit_is_enforced_without_recursion() {
    let mut nested = record(b"MISC", 1, 0, &[]);
    for _ in 0..65 {
        nested = group(&nested);
    }
    let mut bytes = header(&[]);
    bytes.extend(nested);
    assert!(validate(&bytes).is_err());
}

#[test]
fn adversarial_small_inputs_never_panic() {
    // Deterministic mutation sweep, not a substitute for sustained fuzzing.
    let mut seed = header(&[]);
    seed.extend(group(&record(b"MISC", 1, 0, &sub(b"DATA", &[5; 32]))));
    for pos in 0..seed.len() {
        for value in [0, 0xff, 0x80, 0x24] {
            let mut input = seed.clone();
            input[pos] = value;
            let _ = validate(&input);
        }
    }
}

#[test]
fn explicit_override_order_preserves_deletion_and_rejects_missing_masters() {
    let dir = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(record(b"MISC", 0x123, 0, &sub(b"EDID", b"Original\0")));
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(b"MISC", 0x123, plugin::DELETED, &[]));
    std::fs::write(dir.path().join("Base.esm"), base).unwrap();
    std::fs::write(dir.path().join("Patch.esp"), patch).unwrap();
    let indices = vec![
        content::index_plugin(&dir.path().join("Base.esm")).unwrap(),
        content::index_plugin(&dir.path().join("Patch.esp")).unwrap(),
    ];
    let result = content::resolve(&indices, ProfileId::NvOriginal).unwrap();
    let chain = result.chains.values().next().unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[1].plugin, "Patch.esp");
    assert_eq!(chain[1].flags, plugin::DELETED);
    assert!(content::resolve(&indices[1..], ProfileId::NvOriginal).is_err());
}

#[test]
fn diagnostic_checksum_recovery_stays_tainted_and_cannot_resolve() {
    let dir = tempfile::tempdir().unwrap();
    let body = sub(b"DATA", &[1; 40]);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&body).unwrap();
    let mut payload = (body.len() as u32).to_le_bytes().to_vec();
    payload.extend(z.finish().unwrap());
    let last = payload.len() - 1;
    payload[last] ^= 0x08;
    let mut bytes = header(&[]);
    bytes.extend(record(b"MISC", 1, plugin::COMPRESSED, &payload));
    let path = dir.path().join("Bad.esm");
    std::fs::write(&path, bytes).unwrap();
    assert!(content::index_plugin(&path).is_err());
    let index = content::index_plugin_with_limits(
        &path,
        Limits {
            inspect_checksum_mismatches: true,
            ..Limits::default()
        },
    )
    .unwrap();
    assert_eq!(index.census.integrity_issues.len(), 1);
    assert!(content::resolve(&[index], ProfileId::NvOriginal).is_err());
}

#[test]
fn two_pass_script_links_allow_forward_cycles_and_report_missing_targets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Cycle.esm");
    let mut bytes = header(&[]);
    bytes.extend(record(b"SCPT", 1, 0, &sub(b"SCRO", &2u32.to_le_bytes())));
    bytes.extend(record(b"SCPT", 2, 0, &sub(b"SCRO", &1u32.to_le_bytes())));
    bytes.extend(record(b"SCPT", 3, 0, &sub(b"SCRO", &999u32.to_le_bytes())));
    std::fs::write(&path, bytes).unwrap();
    let indices = vec![content::index_plugin(&path).unwrap()];
    let report = content::resolve(&indices, ProfileId::Fo3Original)
        .unwrap()
        .report(&indices);
    assert_eq!(report.script_links.resolved, 2);
    assert_eq!(report.script_links.unresolved.len(), 1);
    assert_eq!(
        report.script_links.unresolved[0].reference.target_raw_form,
        999
    );
}

#[test]
fn condition_inventory_keeps_function_ids_separate_from_script_opcodes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Condition.esm");
    let mut data = vec![0; 28];
    data[8..10].copy_from_slice(&573u16.to_le_bytes());
    let mut bytes = header(&[]);
    bytes.extend(record(b"QUST", 1, 0, &sub(b"CTDA", &data)));
    std::fs::write(&path, bytes).unwrap();
    let index = content::index_plugin(&path).unwrap();
    let usage = &index.census.scripts.condition_functions[&573];
    assert_eq!(usage.occurrences, 1);
    assert_eq!(usage.examples[0].form_id, 1);
    assert_eq!(index.census.scripts.compiled_bodies, 0);
}

#[test]
fn player_reference_is_an_explicit_nv_runtime_dependency() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("FalloutNV.esm");
    let mut bytes = header(&[]);
    bytes.extend(record(
        b"SCPT",
        0x800,
        0,
        &sub(b"SCRO", &0x14u32.to_le_bytes()),
    ));
    // Other absent low IDs stay unresolved; this is not a blanket built-in range.
    bytes.extend(record(
        b"SCPT",
        0x801,
        0,
        &sub(b"SCRO", &0x15u32.to_le_bytes()),
    ));
    std::fs::write(&path, bytes).unwrap();
    let indices = vec![content::index_plugin(&path).unwrap()];
    let report = content::resolve(&indices, ProfileId::NvOriginal)
        .unwrap()
        .report(&indices);
    assert_eq!(report.script_links.resolved, 0);
    assert_eq!(
        report.script_links.runtime_dependencies[&content::RuntimeBinding::NvPlayerReference]
            .occurrences,
        1
    );
    assert_eq!(report.script_links.unresolved.len(), 1);
    let other_profile = content::resolve(&indices, ProfileId::Fo3Original)
        .unwrap()
        .report(&indices);
    assert_eq!(other_profile.script_links.unresolved.len(), 2);
}

#[test]
fn conflicting_noncanonical_self_ids_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Alias.esm");
    let mut bytes = header(&[]);
    bytes.extend(record(b"MISC", 0x00000801, 0, &[]));
    bytes.extend(record(b"MISC", 0x01000801, 0, &[]));
    std::fs::write(&path, bytes).unwrap();
    let index = content::index_plugin(&path).unwrap();
    let error = content::resolve(&[index], ProfileId::NvOriginal)
        .err()
        .unwrap();
    assert!(error.to_string().contains("alias"));
}

fn cell_group(cell: u32, body: &[u8]) -> Vec<u8> {
    let mut children = group(body);
    children[8..12].copy_from_slice(&cell.to_le_bytes());
    children[12..16].copy_from_slice(&9i32.to_le_bytes());
    let mut outer = group(&children);
    outer[8..12].copy_from_slice(&cell.to_le_bytes());
    outer[12..16].copy_from_slice(&6i32.to_le_bytes());
    outer
}

fn placed(base: u32, x: f32) -> Vec<u8> {
    let mut body = sub(b"NAME", &base.to_le_bytes());
    let mut data = [0; 24];
    data[..4].copy_from_slice(&x.to_le_bytes());
    body.extend(sub(b"DATA", &data));
    body
}

#[test]
fn cell_membership_uses_winning_parent_and_does_not_resurrect_deleted_references() {
    use fallout_data::{store::RecordStore, vfs::MountIndex, world};
    let dir = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(record(b"STAT", 0x800, 0, &sub(b"MODL", b"fixture.nif\0")));
    for (id, name) in [(0x900u32, b"RoomA\0"), (0x901, b"RoomB\0")] {
        let mut body = sub(b"EDID", name);
        body.extend(sub(b"DATA", &[1]));
        base.extend(record(b"CELL", id, 0, &body));
    }
    let mut children = record(b"REFR", 0x1000, 0, &placed(0x800, 1.0));
    children.extend(record(b"REFR", 0x1001, 0, &placed(0x800, 2.0)));
    base.extend(cell_group(0x900, &children));
    // The next top-level record must not inherit the preceding cell's context.
    base.extend(record(b"MISC", 0x2000, 0, &[]));
    let mut patch = header(&["Base.esm"]);
    patch.extend(cell_group(
        0x901,
        &record(b"REFR", 0x1000, 0, &placed(0x800, 42.0)),
    ));
    patch.extend(cell_group(
        0x900,
        &record(b"REFR", 0x1001, plugin::DELETED, &[]),
    ));
    std::fs::write(dir.path().join("Base.esm"), base).unwrap();
    std::fs::write(dir.path().join("Patch.esp"), patch).unwrap();
    let mut store = RecordStore::open_nv(
        dir.path(),
        &["Base.esm".into(), "Patch.esp".into()],
        Limits::default(),
    )
    .unwrap();
    let a = world::inspect_cell(&mut store, b"RoomA", &MountIndex::default()).unwrap();
    assert_eq!(a.references.len(), 1);
    assert_eq!(a.references[0].key.local_id, 0x1001);
    assert!(a.references[0].placement.is_none());
    assert_eq!(a.references[0].record_flags, plugin::DELETED);
    let b = world::inspect_cell(&mut store, b"RoomB", &MountIndex::default()).unwrap();
    assert_eq!(b.references.len(), 1);
    assert_eq!(
        b.references[0]
            .placement
            .as_ref()
            .unwrap()
            .transform
            .value
            .position[0],
        42.0
    );
    assert_eq!(
        b.references[0]
            .base
            .as_ref()
            .unwrap()
            .key
            .as_ref()
            .unwrap()
            .origin_plugin,
        "base.esm"
    );
    assert_eq!(
        b.models[0].asset_path.as_ref().unwrap().bytes(),
        b"meshes/fixture.nif"
    );
    assert!(
        store.indices()[0]
            .records
            .last()
            .unwrap()
            .parent
            .cell
            .is_none()
    );
}

#[test]
fn indexed_reads_match_streamed_payloads_and_reject_stale_headers() {
    use std::io::Cursor;
    let payload = sub(b"UNKN", &[7; 800]);
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&payload).unwrap();
    let mut packed = (payload.len() as u32).to_le_bytes().to_vec();
    packed.extend(z.finish().unwrap());
    let mut bytes = header(&[]);
    bytes.extend(record(b"MISC", 0x800, plugin::COMPRESSED, &packed));
    let mut indexed = None;
    plugin::visit(
        &mut &bytes[..],
        bytes.len() as u64,
        "fixture",
        Limits::default(),
        |event| {
            if let Event::Record(record) = event
                && record.header.form_id == 0x800
            {
                indexed = Some(record.header.clone());
            }
            Ok(())
        },
    )
    .unwrap();
    let indexed = indexed.unwrap();
    let read = plugin::read_indexed(
        &mut Cursor::new(&bytes),
        bytes.len() as u64,
        &indexed,
        "fixture",
        Limits::default(),
    )
    .unwrap();
    assert_eq!(read.payload, payload);
    bytes[indexed.offset as usize + 12] ^= 1;
    assert!(
        plugin::read_indexed(
            &mut Cursor::new(&bytes),
            bytes.len() as u64,
            &indexed,
            "fixture",
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn placed_field_validation_rejects_duplicate_truncated_and_nonfinite_transforms() {
    use fallout_data::world;
    for mode in 0..3 {
        let mut body = placed(0x800, if mode == 2 { f32::NAN } else { 1.0 });
        if mode == 0 {
            body.extend(sub(b"NAME", &0x800u32.to_le_bytes()));
        }
        if mode == 1 {
            body.pop();
        }
        let mut bytes = header(&[]);
        bytes.extend(record(b"REFR", 0x900, 0, &body));
        let result = plugin::visit(
            &mut &bytes[..],
            bytes.len() as u64,
            "fixture",
            Limits::default(),
            |event| {
                if let Event::Record(record) = event
                    && record.header.kind == *b"REFR"
                {
                    world::decode_placement(record, "fixture")?;
                }
                Ok(())
            },
        );
        assert!(result.is_err());
    }
}

#[test]
fn present_but_wrong_kind_targets_do_not_count_as_resolved() {
    use fallout_data::{store::RecordStore, vfs::MountIndex, world};
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = header(&[]);
    let mut cell = sub(b"EDID", b"TypedRoom\0");
    cell.extend(sub(b"DATA", &[1]));
    bytes.extend(record(b"CELL", 0x900, 0, &cell));
    bytes.extend(record(b"SCPT", 0x800, 0, &[]));
    let mut body = placed(0x800, 1.0);
    let mut enable = 0x999u32.to_le_bytes().to_vec();
    enable.extend([1, 0, 0, 0]);
    body.extend(sub(b"XESP", &enable));
    let mut teleport = vec![0; 32];
    teleport[..4].copy_from_slice(&0x800u32.to_le_bytes());
    body.extend(sub(b"XTEL", &teleport));
    bytes.extend(cell_group(0x900, &record(b"REFR", 0x1000, 0, &body)));
    std::fs::write(dir.path().join("Typed.esm"), bytes).unwrap();
    let mut store =
        RecordStore::open_nv(dir.path(), &["Typed.esm".into()], Limits::default()).unwrap();
    let report = world::inspect_cell(&mut store, b"TypedRoom", &MountIndex::default()).unwrap();
    assert_eq!(report.link_failures, 3);
    let reference = &report.references[0];
    assert_eq!(reference.base.as_ref().unwrap().status, "wrong-record-kind");
    assert_eq!(reference.enable_parent.as_ref().unwrap().status, "missing");
    assert_eq!(
        reference.teleport_door.as_ref().unwrap().status,
        "wrong-record-kind"
    );
    assert!(report.models.is_empty());
}
