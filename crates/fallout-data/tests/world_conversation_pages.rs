use fallout_data::{
    condition_operands::Signatures,
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    world::conversation::{DialogueSources, Limits, PageLimits},
};
use std::{fs, path::Path};
fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
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
    record(b"TES4", 0, 0, &body)
}
fn group(topic: u32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &topic.to_le_bytes(),
        &7_i32.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
fn patch_key(id: u32) -> FormKey {
    FormKey {
        origin_plugin: "patch.esp".into(),
        ..key(id)
    }
}
fn info(text: &[u8]) -> Vec<u8> {
    let mut trdt = [0; 24];
    trdt[12] = 9;
    [
        field(b"DATA", &[0, 7, 0, 0]),
        field(b"TPIC", &0x100_u32.to_le_bytes()),
        field(b"TRDT", &trdt),
        field(b"NAM1", text),
    ]
    .concat()
}
fn fixture(path: &Path) {
    fs::write(
        path.join("Base.esm"),
        [
            header(&[]),
            record(b"DIAL", 0x100, 0, &field(b"DATA", &[0])),
            record(b"DIAL", 0x200, 0, &[]),
            record(b"NPC_", 0x400, 0, &[]),
            group(
                0x100,
                &[
                    record(b"INFO", 0x303, 0, b"NAM1\x05\0x"),
                    record(b"INFO", 0x302, 0, &info(b"moved\0")),
                    record(b"INFO", 0x301, 0, &info(b"deleted\0")),
                    record(b"INFO", 0x300, 0, &info(b"old\0")),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        path.join("Patch.esp"),
        [
            header(&["Base.esm"]),
            group(
                0x100,
                &[
                    record(b"INFO", 0x01000700, 0, &info(b"own\0")),
                    record(b"INFO", 0x301, plugin::DELETED, &[]),
                    record(b"INFO", 0x300, 0, &info(&[255, 128, 65, 0])),
                ]
                .concat(),
            ),
            group(0x200, &record(b"INFO", 0x302, 0, &[])),
        ]
        .concat(),
    )
    .unwrap();
}
fn open(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn sources(store: &mut RecordStore) -> DialogueSources {
    DialogueSources::build(store, Limits::default()).unwrap()
}
fn one() -> PageLimits {
    PageLimits {
        members: 1,
        ..PageLimits::default()
    }
}
#[test]
fn literal_winners_page_without_reading_bad_sibling_and_prepare_selected_parent() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut store = open(dir.path(), &["Base.esm", "Patch.esp"]);
    let sources = sources(&mut store);
    let before = sources.retained_bytes();
    let first = sources
        .page(&key(0x100), Some(&key(0x400)), None, one())
        .unwrap();
    assert_eq!(first.total_members(), 3);
    assert_eq!(first.start_index(), 0);
    assert_eq!(first.requests()[0].info(), &key(0x300));
    assert_eq!(first.usage().returned_members, 1);
    assert_eq!(first.usage().visited_members, 5);
    assert_eq!(first.usage().copied_bytes, 270);
    let second = sources
        .page(
            &key(0x100),
            Some(&key(0x400)),
            first.cursor(),
            PageLimits {
                members: 2,
                ..PageLimits::default()
            },
        )
        .unwrap();
    assert_eq!(second.start_index(), 1);
    assert_eq!(
        second
            .requests()
            .iter()
            .map(|r| r.info().clone())
            .collect::<Vec<_>>(),
        [key(0x303), patch_key(0x700)]
    );
    assert_eq!(second.usage().visited_members, 12);
    assert_eq!(second.usage().copied_bytes, 271);
    assert!(second.cursor().is_none());
    assert_eq!(sources.retained_bytes(), before);
    assert!(sources.request(key(0x100), key(0x301), None).is_err());
    assert!(sources.request(key(0x100), key(0x302), None).is_err());
    assert!(sources.request(key(0x100), key(0x999), None).is_err());
    let moved = sources.page(&key(0x200), None, None, one()).unwrap();
    assert_eq!(moved.requests()[0].info(), &key(0x302));
    let prepared = sources
        .prepare(
            &mut store,
            &first.requests()[0],
            &Signatures::new(),
            Limits::default(),
        )
        .unwrap();
    assert_eq!(prepared.metadata().info.source_plugin, "Patch.esp");
    assert_eq!(prepared.metadata().info.record_file_offset, 203);
    let winner = store.winner(&key(0x300)).unwrap();
    assert_eq!(store.definition(winner).parent.topic, Some(0x100));
    assert_eq!(store.definition(winner).header.stored_size, 60);
    assert_eq!(
        prepared
            .metadata()
            .info_fields
            .iter()
            .map(|field| field.header_decoded_offset)
            .collect::<Vec<_>>(),
        [0, 10, 20, 50]
    );
    assert_eq!(
        prepared.metadata().speaker.as_ref().unwrap().key,
        key(0x400)
    );
    assert_eq!(
        prepared.subtitle_bytes(0, 0),
        Some([255, 128, 65, 0].as_slice())
    );
    assert!(
        sources
            .prepare(
                &mut store,
                &second.requests()[0],
                &Signatures::new(),
                Limits::default()
            )
            .is_err()
    );
    let projection = serde_json::to_value(&first).unwrap();
    assert!(projection.get("cursor").is_none());
    assert_eq!(projection["payloads_prepared"], false);
    assert_eq!(projection["canonical_structural_order"], true);
    assert_eq!(projection["original_order_verified"], false);
    drop(store);
    drop(dir);
    assert_eq!(
        prepared.subtitle_bytes(0, 0),
        Some([255, 128, 65, 0].as_slice())
    );
}
#[test]
fn cursor_refuses_other_topic_speaker_identical_index_and_changed_cohort() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    fs::write(dir.path().join("Other.esm"), header(&[])).unwrap();
    let mut store = open(dir.path(), &["Base.esm", "Patch.esp"]);
    let index = sources(&mut store);
    let page = index
        .page(&key(0x100), Some(&key(0x400)), None, one())
        .unwrap();
    for (topic, speaker) in [
        (key(0x200), Some(key(0x400))),
        (key(0x100), None),
        (key(0x100), Some(key(0x401))),
    ] {
        assert!(
            index
                .page(&topic, speaker.as_ref(), page.cursor(), one())
                .is_err()
        );
    }
    let other = sources(&mut store);
    assert!(
        other
            .page(&key(0x100), Some(&key(0x400)), page.cursor(), one())
            .is_err()
    );
    let mut changed = open(dir.path(), &["Base.esm", "Patch.esp", "Other.esm"]);
    let changed = sources(&mut changed);
    assert!(
        changed
            .page(&key(0x100), Some(&key(0x400)), page.cursor(), one())
            .is_err()
    );
    drop(index);
    assert!(
        other
            .page(&key(0x100), Some(&key(0x400)), page.cursor(), one())
            .is_err()
    );
}
#[test]
fn all_page_ceilings_and_literal_exact_under_copy_visit_bounds() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut store = open(dir.path(), &["Base.esm", "Patch.esp"]);
    let index = sources(&mut store);
    let exact = PageLimits {
        members: 1,
        visited_members: 5,
        copied_bytes: 270,
    };
    assert!(
        index
            .page(&key(0x100), Some(&key(0x400)), None, exact)
            .is_ok()
    );
    for limit in [
        PageLimits {
            visited_members: 4,
            ..exact
        },
        PageLimits {
            copied_bytes: 269,
            ..exact
        },
        PageLimits {
            members: 0,
            ..exact
        },
        PageLimits {
            members: 1025,
            ..exact
        },
        PageLimits {
            visited_members: 0,
            ..exact
        },
        PageLimits {
            visited_members: 32769,
            ..exact
        },
        PageLimits {
            copied_bytes: 0,
            ..exact
        },
        PageLimits {
            copied_bytes: 1048577,
            ..exact
        },
    ] {
        assert!(
            index
                .page(&key(0x100), Some(&key(0x400)), None, limit)
                .is_err()
        );
    }
    let first = index
        .page(&key(0x100), Some(&key(0x400)), None, exact)
        .unwrap();
    let minimal_visit_error = index
        .page(
            &key(0x100),
            Some(&key(0x400)),
            first.cursor(),
            PageLimits {
                visited_members: 1,
                ..one()
            },
        )
        .err()
        .unwrap()
        .to_string();
    assert!(minimal_visit_error.contains("visited member budget exceeded"));
    let final_limits = PageLimits {
        members: 2,
        visited_members: 12,
        copied_bytes: 271,
    };
    assert!(
        index
            .page(&key(0x100), Some(&key(0x400)), first.cursor(), final_limits)
            .is_ok()
    );
    assert!(
        index
            .page(
                &key(0x100),
                Some(&key(0x400)),
                first.cursor(),
                PageLimits {
                    copied_bytes: 270,
                    ..final_limits
                }
            )
            .is_err()
    );
    assert!(
        index
            .page(
                &key(0x100),
                Some(&key(0x400)),
                first.cursor(),
                PageLimits {
                    visited_members: 11,
                    ..final_limits
                }
            )
            .is_err()
    );
}
#[test]
fn absent_topic_is_empty_structural_source_and_existing_index_ceiling_applies() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let mut store = open(dir.path(), &["Base.esm", "Patch.esp"]);
    assert!(
        DialogueSources::build(
            &mut store,
            Limits {
                infos: 4,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let index = sources(&mut store);
    let absent = index
        .page(
            &key(0x999),
            None,
            None,
            PageLimits {
                members: 1,
                visited_members: 1,
                copied_bytes: 64,
            },
        )
        .unwrap();
    assert_eq!(absent.total_members(), 0);
    assert_eq!(absent.usage().returned_members, 0);
    assert_eq!(absent.usage().visited_members, 0);
    assert_eq!(absent.usage().copied_bytes, 64);
    assert!(absent.requests().is_empty() && absent.cursor().is_none());
    assert!(
        index
            .page(
                &key(0x999),
                None,
                None,
                PageLimits {
                    copied_bytes: 63,
                    ..one()
                }
            )
            .is_err()
    );
    assert!(
        serde_json::to_string(&absent)
            .unwrap()
            .contains("\"payloads_prepared\":false")
    );
}
#[test]
fn deferred_source_change_refuses_selected_request_and_no_original_actor_default() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    fs::write(dir.path().join("Other.esm"), header(&[])).unwrap();
    let mut store = open(dir.path(), &["Base.esm", "Patch.esp", "Other.esm"]);
    let index = sources(&mut store);
    let page = index.page(&key(0x100), None, None, one()).unwrap();
    assert!(page.requests()[0].speaker().is_none());
    drop(store);
    fs::write(
        dir.path().join("Other.esm"),
        [header(&[]), record(b"ACTI", 0x900, 0, &[])].concat(),
    )
    .unwrap();
    let mut changed = open(dir.path(), &["Base.esm", "Patch.esp", "Other.esm"]);
    assert!(
        index
            .prepare(
                &mut changed,
                &page.requests()[0],
                &Signatures::new(),
                Limits::default()
            )
            .is_err()
    );
}
