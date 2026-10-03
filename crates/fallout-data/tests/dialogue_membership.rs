use fallout_data::{
    dialogue_membership::{MembershipIndex, Status},
    identity::{FormKey, ProfileId},
    index_cache, plugin,
    store::RecordStore,
};
use std::fs;

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], form: u32, flags: u32) -> Vec<u8> {
    [
        kind.as_slice(),
        &0_u32.to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 8],
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut payload = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        payload.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        payload.extend(field(b"DATA", &[0; 8]));
    }
    [
        b"TES4".as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &[0; 16],
        &payload,
    ]
    .concat()
}
fn group(topic: u32, records: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(records.len() as u32 + 24).to_le_bytes(),
        &topic.to_le_bytes(),
        &7_i32.to_le_bytes(),
        &[0; 8],
        records,
    ]
    .concat()
}
fn key(form: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: form,
    }
}
fn store(directory: &std::path::Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        directory,
        &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}

#[test]
fn winning_info_moves_topic_and_deleted_topic_never_falls_back() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"DIAL", 0x100, 0),
            record(b"DIAL", 0x200, 0),
            group(0x100, &record(b"INFO", 0x300, 0)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Move.esp"),
        [
            header(&["Base.esm"]),
            group(0x200, &record(b"INFO", 0x300, 0)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Delete.esp"),
        [
            header(&["Base.esm"]),
            record(b"DIAL", 0x200, plugin::DELETED),
        ]
        .concat(),
    )
    .unwrap();
    let first = MembershipIndex::build(&store(directory.path(), &["Base.esm"]), 10).unwrap();
    assert_eq!(first.topic_infos(&key(0x100)), &[key(0x300)]);
    let moved =
        MembershipIndex::build(&store(directory.path(), &["Base.esm", "Move.esp"]), 10).unwrap();
    assert!(moved.topic_infos(&key(0x100)).is_empty());
    assert_eq!(moved.topic_infos(&key(0x200)), &[key(0x300)]);
    assert_eq!(moved.report().rows[0].source_plugin, "Move.esp");
    let deleted = MembershipIndex::build(
        &store(directory.path(), &["Base.esm", "Move.esp", "Delete.esp"]),
        10,
    )
    .unwrap();
    assert_eq!(deleted.report().rows[0].status, Status::DeletedTopic);
    assert!(deleted.topic_infos(&key(0x200)).is_empty());
    assert_eq!(
        deleted.report().rows[0]
            .topic
            .as_ref()
            .unwrap()
            .source_plugin,
        "Delete.esp"
    );
}

#[test]
fn parent_form_ids_use_the_winning_sources_master_table() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"DIAL", 0x100, 0),
            group(0x100, &record(b"INFO", 0x300, 0)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["Other.esm", "Base.esm"]),
            group(0x0100_0100, &record(b"INFO", 0x0100_0300, 0)),
        ]
        .concat(),
    )
    .unwrap();
    let a = MembershipIndex::build(
        &store(directory.path(), &["Other.esm", "Base.esm", "Patch.esp"]),
        10,
    )
    .unwrap();
    let b = MembershipIndex::build(
        &store(directory.path(), &["Base.esm", "Other.esm", "Patch.esp"]),
        10,
    )
    .unwrap();
    assert_eq!(a.report().rows[0].info_key, key(0x300));
    assert_eq!(a.report().rows[0].topic_key, Some(key(0x100)));
    assert_eq!(a.report().rows[0].raw_parent_topic, Some(0x0100_0100));
    assert_eq!(
        serde_json::to_value(a.report()).unwrap(),
        serde_json::to_value(b.report()).unwrap()
    );
}

#[test]
fn absent_null_missing_wrong_kind_and_deleted_info_remain_distinct() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"ACTI", 0x100, 0),
            record(b"INFO", 0x301, 0),
            group(0, &record(b"INFO", 0x302, 0)),
            group(0x200, &record(b"INFO", 0x303, 0)),
            group(0x100, &record(b"INFO", 0x304, 0)),
            record(b"INFO", 0x305, plugin::DELETED),
        ]
        .concat(),
    )
    .unwrap();
    let store = store(directory.path(), &["Base.esm"]);
    let index = MembershipIndex::build(&store, 5).unwrap();
    assert_eq!(
        index
            .report()
            .rows
            .iter()
            .map(|row| row.status)
            .collect::<Vec<_>>(),
        vec![
            Status::MissingParent,
            Status::NullParent,
            Status::MissingTopic,
            Status::WrongTopicKind,
            Status::DeletedInfo
        ]
    );
    assert!(!index.report().payloads_validated);
    assert!(!index.report().retail_selection_order_verified);
    assert!(MembershipIndex::build(&store, 4).is_err());
}

#[test]
fn cache_v2_preserves_topic_labels_and_rejects_old_version_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"DIAL", 0x100, 0),
            group(0x100, &record(b"INFO", 0x300, 0)),
        ]
        .concat(),
    )
    .unwrap();
    let names = vec!["Base.esm".to_owned()];
    let cold = RecordStore::open_nv_headers_cached(
        directory.path(),
        &names,
        plugin::Limits::default(),
        cache.path(),
    )
    .unwrap();
    assert_eq!(
        cold.index_cache_report().unwrap().format,
        "nv-header-index-v2"
    );
    assert!(!cold.index_cache_report().unwrap().plugins[0].reused);
    let expected =
        serde_json::to_value(MembershipIndex::build(&cold, 10).unwrap().report()).unwrap();
    let mut bytes = index_cache::encode(&cold.indices()[0]).unwrap();
    bytes[8..12].copy_from_slice(&1_u32.to_le_bytes());
    assert!(
        index_cache::decode(
            &bytes,
            "Base.esm",
            cold.indices()[0].census.source_bytes,
            plugin::Limits::default()
        )
        .is_err()
    );
    drop(cold);
    let warm = RecordStore::open_nv_headers_cached(
        directory.path(),
        &names,
        plugin::Limits::default(),
        cache.path(),
    )
    .unwrap();
    assert!(warm.index_cache_report().unwrap().plugins[0].reused);
    assert_eq!(
        expected,
        serde_json::to_value(MembershipIndex::build(&warm, 10).unwrap().report()).unwrap()
    );
}
