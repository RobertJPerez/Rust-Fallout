//! Original plugins exercise cached lookup through the real record store.
use fallout_data::{content, index_cache, plugin, store::RecordStore, vfs::MountIndex, world};
use flate2::{Compression, write::ZlibEncoder};
use serde_json::Value;
use std::{fs, io::Write, path::Path};

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
    for name in masters {
        body.extend(sub(b"MAST", &[name.as_bytes(), &[0]].concat()));
        body.extend(sub(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn group(label: u32, kind: i32, children: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend((children.len() as u32 + 24).to_le_bytes());
    out.extend(label.to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend([0; 8]);
    out.extend(children);
    out
}
fn cell(name: &[u8], id: u32) -> Vec<u8> {
    let mut body = sub(b"EDID", &[name, &[0]].concat());
    body.extend(sub(b"DATA", &[1]));
    record(b"CELL", id, 0, &body)
}
fn placement(position: f32) -> Vec<u8> {
    let mut body = sub(b"NAME", &0x800u32.to_le_bytes());
    let transform: Vec<_> = [position, -2., -3., 0., 0., 0.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    body.extend(sub(b"DATA", &transform));
    body
}
fn reference(position: f32) -> Vec<u8> {
    record(b"REFR", 0x900, plugin::PERSISTENT, &placement(position))
}
fn base(model: &[u8]) -> Vec<u8> {
    let mut bytes = header(&[]);
    bytes.extend(record(
        b"STAT",
        0x800,
        0,
        &sub(b"MODL", &[model, &[0]].concat()),
    ));
    bytes.extend(cell(b"FirstRoom", 0xa00));
    bytes.extend(cell(b"SecondRoom", 0xa01));
    bytes.extend(group(0xa00, 6, &group(0xa00, 8, &reference(1.))));
    bytes
}
fn patch(position: f32, parent: u32) -> Vec<u8> {
    let mut bytes = header(&["Base.esm"]);
    bytes.extend(group(parent, 6, &group(parent, 8, &reference(position))));
    bytes
}
fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("Data")).unwrap();
    fs::write(source.path().join("Data/Base.esm"), base(b"old.nif")).unwrap();
    (source, tempfile::tempdir().unwrap())
}
fn cached(source: &Path, cache: &Path, names: &[&str]) -> fallout_data::Result<RecordStore> {
    RecordStore::open_nv_headers_cached(
        &source.join("Data"),
        &names.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        plugin::Limits::default(),
        cache,
    )
}
fn view(store: &mut RecordStore, name: &[u8]) -> world::CellReport {
    world::inspect_cell(store, name, &MountIndex::default()).unwrap()
}
fn payload(report: &world::CellReport) -> Value {
    let mut value = serde_json::to_value(report).unwrap();
    value.as_object_mut().unwrap().remove("index_cache");
    value
}

#[test]
fn cold_and_warm_indices_preserve_the_uncached_cell_and_deferred_scope() {
    let (source, cache) = setup();
    let mut ordinary = RecordStore::open_nv_headers(
        &source.path().join("Data"),
        &["Base.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let expected = view(&mut ordinary, b"FirstRoom");
    let mut cold = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    let cold_view = view(&mut cold, b"FirstRoom");
    assert_eq!(payload(&cold_view), payload(&expected));
    assert!(!cold_view.index_cache.as_ref().unwrap().plugins[0].reused);
    drop(cold);
    let mut warm = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    let warm_view = view(&mut warm, b"FirstRoom");
    assert_eq!(payload(&warm_view), payload(&expected));
    let receipt = warm_view.index_cache.as_ref().unwrap();
    assert!(receipt.plugins[0].reused);
    assert_eq!(
        warm_view.references[0]
            .placement
            .as_ref()
            .unwrap()
            .transform
            .value
            .position,
        [1., -2., -3.]
    );
    assert_eq!(warm_view.references[0].record_flags, plugin::PERSISTENT);
    assert_eq!(warm_view.index_payloads_deferred, 2);
    assert_eq!(
        receipt.source_bytes_hashed,
        fs::metadata(source.path().join("Data/Base.esm"))
            .unwrap()
            .len()
    );
}

#[test]
fn changed_bytes_of_the_same_length_get_a_new_source_identity() {
    let (source, cache) = setup();
    let old = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    let old_key = old.index_cache_report().unwrap().plugins[0].key.clone();
    drop(old);
    let path = source.path().join("Data/Base.esm");
    let old_size = fs::metadata(&path).unwrap().len();
    fs::write(&path, base(b"new.nif")).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), old_size);
    let mut changed = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    let receipt = &changed.index_cache_report().unwrap().plugins[0];
    assert!(!receipt.reused);
    assert_ne!(receipt.key, old_key);
    assert_eq!(
        view(&mut changed, b"FirstRoom").models[0]
            .model_field
            .as_ref()
            .unwrap()
            .value,
        b"new.nif"
    );
}

#[test]
fn reordered_cached_plugins_rebuild_winners_and_winning_parent_membership() {
    let (source, cache) = setup();
    fs::write(source.path().join("Data/A.esp"), patch(10., 0xa00)).unwrap();
    fs::write(source.path().join("Data/B.esp"), patch(20., 0xa01)).unwrap();
    let mut first = cached(source.path(), cache.path(), &["Base.esm", "A.esp", "B.esp"]).unwrap();
    let identity = first
        .index_cache_report()
        .unwrap()
        .ordered_source_sha256
        .clone();
    assert!(view(&mut first, b"FirstRoom").references.is_empty());
    assert_eq!(
        view(&mut first, b"SecondRoom").references[0].source_plugin,
        "B.esp"
    );
    drop(first);
    let mut reordered =
        cached(source.path(), cache.path(), &["Base.esm", "B.esp", "A.esp"]).unwrap();
    let receipt = reordered.index_cache_report().unwrap();
    assert!(receipt.plugins.iter().all(|v| v.reused));
    assert_ne!(receipt.ordered_source_sha256, identity);
    assert!(view(&mut reordered, b"SecondRoom").references.is_empty());
    let first_room = view(&mut reordered, b"FirstRoom");
    assert_eq!(first_room.references[0].source_plugin, "A.esp");
    assert_eq!(
        first_room.references[0]
            .placement
            .as_ref()
            .unwrap()
            .transform
            .value
            .position[0],
        10.
    );
}

#[test]
fn changed_master_rebinds_live_dependencies_while_unchanged_child_metadata_reuses() {
    let (source, cache) = setup();
    fs::write(source.path().join("Data/Patch.esp"), patch(10., 0xa00)).unwrap();
    let first = cached(source.path(), cache.path(), &["Base.esm", "Patch.esp"]).unwrap();
    let original_view_key = first
        .index_cache_report()
        .unwrap()
        .ordered_source_sha256
        .clone();
    drop(first);
    fs::write(source.path().join("Data/Base.esm"), base(b"new.nif")).unwrap();
    let mut changed = cached(source.path(), cache.path(), &["Base.esm", "Patch.esp"]).unwrap();
    let receipt = changed.index_cache_report().unwrap();
    assert!(!receipt.plugins[0].reused);
    assert!(receipt.plugins[1].reused);
    assert_ne!(receipt.ordered_source_sha256, original_view_key);
    let room = view(&mut changed, b"FirstRoom");
    assert_eq!(room.references[0].source_plugin, "Patch.esp");
    assert_eq!(
        room.models[0].model_field.as_ref().unwrap().value,
        b"new.nif"
    );
}

#[test]
fn relocating_identical_sources_preserves_cache_keys_and_provenance() {
    let (source, cache) = setup();
    let first = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    let original_key = first
        .index_cache_report()
        .unwrap()
        .ordered_source_sha256
        .clone();
    drop(first);
    let relocated = tempfile::tempdir().unwrap();
    fs::create_dir(relocated.path().join("Data")).unwrap();
    fs::copy(
        source.path().join("Data/Base.esm"),
        relocated.path().join("Data/Base.esm"),
    )
    .unwrap();
    let mut reused = cached(relocated.path(), cache.path(), &["Base.esm"]).unwrap();
    assert!(reused.index_cache_report().unwrap().plugins[0].reused);
    assert_eq!(
        reused.index_cache_report().unwrap().ordered_source_sha256,
        original_key
    );
    assert_eq!(view(&mut reused, b"FirstRoom").source_plugin, "Base.esm");
}

#[test]
fn warm_cache_still_rejects_bad_master_order_and_stricter_parser_limits() {
    let (source, cache) = setup();
    fs::write(source.path().join("Data/Patch.esp"), patch(10., 0xa00)).unwrap();
    drop(cached(source.path(), cache.path(), &["Base.esm", "Patch.esp"]).unwrap());
    assert!(cached(source.path(), cache.path(), &["Patch.esp", "Base.esm"]).is_err());
    assert!(
        RecordStore::open_nv_headers_cached(
            &source.path().join("Data"),
            &["Base.esm".into()],
            plugin::Limits {
                max_records: 1,
                ..Default::default()
            },
            cache.path(),
        )
        .is_err()
    );
    assert!(
        RecordStore::open_nv_headers_cached(
            &source.path().join("Data"),
            &["Base.esm".into()],
            plugin::Limits {
                inspect_checksum_mismatches: true,
                ..Default::default()
            },
            cache.path(),
        )
        .is_err()
    );
}

#[test]
fn corrupt_cache_entries_fail_and_verified_orphans_resume() {
    let (source, cache) = setup();
    let first = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    let key = first.index_cache_report().unwrap().plugins[0].key.clone();
    drop(first);
    let marker = cache.path().join(format!("{key}.json"));
    let blob = cache.path().join(format!("{key}.blob"));
    fs::remove_file(&marker).unwrap(); // Simulate a blob with no commit marker.
    let resumed = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    assert!(!resumed.index_cache_report().unwrap().plugins[0].reused);
    assert!(marker.exists());
    drop(resumed);
    let original_marker = fs::read(&marker).unwrap();
    let mut document: Value = serde_json::from_slice(&original_marker).unwrap();
    document["identity"]["profile"] = "fo3-original".into();
    fs::write(&marker, serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(cached(source.path(), cache.path(), &["Base.esm"]).is_err());
    fs::write(&marker, original_marker).unwrap();
    fs::write(&blob, b"damaged index").unwrap();
    assert!(cached(source.path(), cache.path(), &["Base.esm"]).is_err());
    fs::remove_file(&marker).unwrap();
    assert!(cached(source.path(), cache.path(), &["Base.esm"]).is_err());
    assert!(!marker.exists());
}

#[test]
fn metadata_framing_rejects_truncation_count_lies_surplus_and_invalid_option_tags() {
    let (source, _) = setup();
    let path = source.path().join("Data/Base.esm");
    let index = content::index_plugin_headers(&path, plugin::Limits::default()).unwrap();
    let bytes = index_cache::encode(&index).unwrap();
    let source_bytes = fs::metadata(&path).unwrap().len();
    for end in 0..bytes.len() {
        assert!(
            index_cache::decode(
                &bytes[..end],
                "Base.esm",
                source_bytes,
                plugin::Limits::default()
            )
            .is_err()
        );
    }
    let mut bad = bytes.clone();
    bad[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        index_cache::decode(&bad, "Base.esm", source_bytes, plugin::Limits::default()).is_err()
    );
    let first = 20 + u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    let mut bad = bytes.clone();
    bad[first + 32] = 2;
    assert!(
        index_cache::decode(&bad, "Base.esm", source_bytes, plugin::Limits::default()).is_err()
    );
    let mut bad = bytes.clone();
    bad.push(0);
    assert!(
        index_cache::decode(&bad, "Base.esm", source_bytes, plugin::Limits::default()).is_err()
    );
    for n in 0..256 {
        let mut bad = bytes.clone();
        let offset = (n * 997 + 13) % bad.len();
        bad[offset] ^= n as u8 | 1;
        assert!(
            std::panic::catch_unwind(|| index_cache::decode(
                &bad,
                "Base.esm",
                source_bytes,
                plugin::Limits::default()
            ))
            .is_ok()
        );
    }
}

#[test]
fn cache_destination_cannot_enter_the_source_installation() {
    let (source, _) = setup();
    let root = source.path().join("cache");
    fs::create_dir(&root).unwrap();
    assert!(cached(source.path(), &root, &["Base.esm"]).is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}

#[test]
fn deleted_winners_remain_tombstones_when_their_metadata_is_reused() {
    let (source, cache) = setup();
    let mut bytes = header(&["Base.esm"]);
    bytes.extend(group(
        0xa00,
        6,
        &group(0xa00, 8, &record(b"REFR", 0x900, plugin::DELETED, &[])),
    ));
    fs::write(source.path().join("Data/Deleted.esp"), bytes).unwrap();
    drop(cached(source.path(), cache.path(), &["Base.esm", "Deleted.esp"]).unwrap());
    let mut warm = cached(source.path(), cache.path(), &["Base.esm", "Deleted.esp"]).unwrap();
    assert!(
        warm.index_cache_report()
            .unwrap()
            .plugins
            .iter()
            .all(|entry| entry.reused)
    );
    let room = view(&mut warm, b"FirstRoom");
    assert_eq!(room.references.len(), 1);
    assert_eq!(room.references[0].source_plugin, "Deleted.esp");
    assert_eq!(room.references[0].record_flags, plugin::DELETED);
    assert!(room.references[0].placement.is_none());
    assert!(room.references[0].base.is_none());
}

#[test]
fn cached_deferred_records_still_fail_strict_payload_validation_when_accessed() {
    let (source, cache) = setup();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&placement(1.)).unwrap();
    let mut compressed = (placement(1.).len() as u32).to_le_bytes().to_vec();
    compressed.extend(encoder.finish().unwrap());
    *compressed.last_mut().unwrap() ^= 1;
    let reference = record(b"REFR", 0x900, plugin::COMPRESSED, &compressed);
    let mut bytes = header(&[]);
    bytes.extend(cell(b"FirstRoom", 0xa00));
    bytes.extend(group(0xa00, 6, &group(0xa00, 8, &reference)));
    fs::write(source.path().join("Data/Base.esm"), bytes).unwrap();
    drop(cached(source.path(), cache.path(), &["Base.esm"]).unwrap());
    let mut warm = cached(source.path(), cache.path(), &["Base.esm"]).unwrap();
    assert!(warm.index_cache_report().unwrap().plugins[0].reused);
    assert!(world::inspect_cell(&mut warm, b"FirstRoom", &MountIndex::default()).is_err());
}
