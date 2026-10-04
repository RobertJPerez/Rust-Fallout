use fallout_data::{
    condition_operands::Signatures,
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Limits as ScriptLimits, OwnerKind},
    plugin,
    store::RecordStore,
    world::conversation::{DialogueSources, Limits, RecordRole},
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], raw: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
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
fn key(local_id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id,
    }
}
fn open(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn script() -> Vec<u8> {
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
    [field(b"SCHR", &schr), field(b"SCDA", &[0x1d, 0, 0, 0])].concat()
}
fn response(number: u8, sound: u32, text: &[u8]) -> Vec<u8> {
    let mut trdt = [0; 24];
    trdt[12] = number;
    trdt[16..20].copy_from_slice(&sound.to_le_bytes());
    [field(b"TRDT", &trdt), field(b"NAM1", text)].concat()
}
fn body() -> Vec<u8> {
    let mut condition = [0; 28];
    condition[8..10].copy_from_slice(&65535_u16.to_le_bytes());
    [
        field(b"DATA", &[0, 7, 0, 0]),
        field(b"QSTI", &0x500_u32.to_le_bytes()),
        field(b"TPIC", &0x100_u32.to_le_bytes()),
        field(b"CTDA", &condition),
        response(9, 0x600, b"first_\xe9\0"),
        field(b"NAM1", b"repeated\0"),
        field(b"SNAM", &0x700_u32.to_le_bytes()),
        response(2, 0, b"second\0"),
        script(),
        field(b"NEXT", &[]),
        script(),
    ]
    .concat()
}
fn fixture(path: &Path) {
    fs::write(
        path.join("Base.esm"),
        [
            header(&[]),
            record(
                b"DIAL",
                0x100,
                0,
                &[
                    field(b"EDID", b"topic\0"),
                    field(b"FULL", b"original_title\0"),
                    field(b"QSTI", &0x500_u32.to_le_bytes()),
                ]
                .concat(),
            ),
            record(b"DIAL", 0x200, 0, &[]),
            record(b"NPC_", 0x400, 0, &[]),
            record(b"QUST", 0x500, 0, &[]),
            record(b"SOUN", 0x600, 0, &[]),
            record(b"IDLE", 0x700, 0, &[]),
            group(0x100, &record(b"INFO", 0x300, 0, &body())),
        ]
        .concat(),
    )
    .unwrap();
}

#[test]
fn actual_subtitle_condition_and_fragment_consumers_retain_original_inputs() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut store = open(directory.path(), &["Base.esm"]);
    let dialogue = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let request = dialogue
        .request(key(0x100), key(0x300), Some(key(0x400)))
        .unwrap();
    let prepared = dialogue
        .prepare(&mut store, &request, &Signatures::new(), Limits::default())
        .unwrap();
    let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(())).unwrap();
    drop(store);
    // Presentation consumes retained raw subtitle bytes after source handles drop.
    assert_eq!(
        prepared.subtitle_bytes(0, 0),
        Some(b"first_\xe9\0".as_slice())
    );
    assert_eq!(
        prepared.subtitle_bytes(0, 1),
        Some(b"repeated\0".as_slice())
    );
    assert_eq!(prepared.subtitle_bytes(1, 0), Some(b"second\0".as_slice()));
    assert_eq!(prepared.subtitle_bytes(1, 1), None);
    assert_eq!(prepared.subtitle_bytes(2, 0), None);
    let metadata = prepared.metadata();
    assert_eq!(
        metadata
            .responses
            .iter()
            .map(|r| r.number)
            .collect::<Vec<_>>(),
        [9, 2]
    );
    assert_eq!(
        metadata.responses[0].sound.target.as_ref().unwrap().key,
        key(0x600)
    );
    assert_eq!(metadata.responses[1].sound.status, "null");
    assert_eq!(metadata.speaker.as_ref().unwrap().key, key(0x400));
    let title = metadata
        .topic_fields
        .iter()
        .position(|field| field.kind == *b"FULL")
        .unwrap();
    assert_eq!(
        prepared.topic_bytes(title),
        Some(b"original_title\0".as_slice())
    );
    assert!(
        metadata
            .links
            .iter()
            .any(|link| link.record == RecordRole::Topic && link.target.key == Some(key(0x500)))
    );
    // Scripts consumes the existing immutable CTDA input; truth stays unknown.
    let condition = &metadata.conditions.conditions().sites()[0];
    assert_eq!(condition.condition().function_id, 65535);
    assert_eq!(condition.raw_bytes().len(), 28);
    assert_eq!(
        metadata.conditions.ownership().sites()[0].owner_section,
        Some(0)
    );
    // Resolve and decode real loaded units rather than an invented fragment body.
    assert_eq!(metadata.fragments.len(), 2);
    for (fragment, role) in metadata
        .fragments
        .iter()
        .zip([OwnerKind::DialogueBegin, OwnerKind::DialogueEnd])
    {
        assert_eq!(fragment.role(), Some(role));
        let loaded = fragment.resolve(&catalogue).unwrap();
        assert_eq!(loaded.compiled(), Some([0x1d, 0, 0, 0].as_slice()));
        assert_eq!(loaded.program().unwrap().unwrap().instructions.len(), 1);
    }
    assert!(
        !metadata.selection_order_verified
            && !metadata.condition_truth_verified
            && !metadata.speaker_assignment_verified
            && !metadata.fragment_timing_verified
            && !metadata.voice_filename_verified
    );
    // A source metadata projection does not accidentally publish story text.
    let json = serde_json::to_string(metadata).unwrap();
    assert!(!json.contains("original_title") && !json.contains("repeated"));
    assert!(prepared.info_bytes(usize::MAX).is_none());
}

#[test]
fn old_request_refuses_changed_empty_plugin_order_and_wrong_fragment_catalogue() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let mut original = open(directory.path(), &["Base.esm", "Other.esm"]);
    let sources = DialogueSources::build(&mut original, Limits::default()).unwrap();
    let request = sources.request(key(0x100), key(0x300), None).unwrap();
    let prepared = sources
        .prepare(
            &mut original,
            &request,
            &Signatures::new(),
            Limits::default(),
        )
        .unwrap();
    drop(original);
    let mut reordered = open(directory.path(), &["Other.esm", "Base.esm"]);
    assert!(
        sources
            .prepare(
                &mut reordered,
                &request,
                &Signatures::new(),
                Limits::default()
            )
            .is_err()
    );
    let reordered_sources = DialogueSources::build(&mut reordered, Limits::default()).unwrap();
    assert!(
        reordered_sources
            .prepare(
                &mut reordered,
                &request,
                &Signatures::new(),
                Limits::default()
            )
            .is_err()
    );
    drop(reordered);
    fs::write(
        directory.path().join("Other.esm"),
        [header(&[]), record(b"ACTI", 0x900, 0, &[])].concat(),
    )
    .unwrap();
    let mut changed = open(directory.path(), &["Base.esm", "Other.esm"]);
    assert!(
        sources
            .prepare(
                &mut changed,
                &request,
                &Signatures::new(),
                Limits::default()
            )
            .is_err()
    );
    drop(changed);
    // Source body edits cannot be hidden by copying the public catalogue report.
    let mut altered = body();
    altered.extend(field(b"RNAM", b"edited\0"));
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["Base.esm"]),
            group(0x100, &record(b"INFO", 0x300, 0, &altered)),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = open(directory.path(), &["Base.esm", "Patch.esp"]);
    let mut catalogue =
        Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(())).unwrap();
    catalogue.sources.clear();
    assert!(
        prepared.metadata().fragments[0]
            .resolve(&catalogue)
            .is_err()
    );
}

#[test]
fn moved_deleted_and_wrong_speaker_sources_never_resurrect_predecessors() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    fs::write(
        directory.path().join("Move.esp"),
        [
            header(&["Base.esm"]),
            group(0x200, &record(b"INFO", 0x300, 0, &body())),
        ]
        .concat(),
    )
    .unwrap();
    let mut moved = open(directory.path(), &["Base.esm", "Move.esp"]);
    let sources = DialogueSources::build(&mut moved, Limits::default()).unwrap();
    assert!(sources.request(key(0x100), key(0x300), None).is_err());
    let request = sources
        .request(key(0x200), key(0x300), Some(key(0x500)))
        .unwrap();
    assert!(
        sources
            .prepare(&mut moved, &request, &Signatures::new(), Limits::default())
            .is_err()
    );
    let request = sources.request(key(0x200), key(0x300), None).unwrap();
    assert_eq!(
        sources
            .prepare(&mut moved, &request, &Signatures::new(), Limits::default())
            .unwrap()
            .metadata()
            .info
            .source_plugin,
        "Move.esp"
    );
    fs::write(
        directory.path().join("Delete.esp"),
        [
            header(&["Base.esm"]),
            group(0x200, &record(b"INFO", 0x300, plugin::DELETED, &[])),
        ]
        .concat(),
    )
    .unwrap();
    let mut deleted = open(directory.path(), &["Base.esm", "Move.esp", "Delete.esp"]);
    let sources = DialogueSources::build(&mut deleted, Limits::default()).unwrap();
    assert!(sources.request(key(0x100), key(0x300), None).is_err());
    assert!(sources.request(key(0x200), key(0x300), None).is_err());
}

#[test]
fn exact_and_one_over_source_and_conversation_budgets_are_enforced() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut store = open(directory.path(), &["Base.esm"]);
    assert!(
        DialogueSources::build(
            &mut store,
            Limits {
                infos: 0,
                ..Limits::default()
            }
        )
        .is_err()
    );
    let sources = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let request = sources.request(key(0x100), key(0x300), None).unwrap();
    let exact_source = Limits {
        retained_bytes: sources.retained_bytes(),
        ..Limits::default()
    };
    assert!(DialogueSources::build(&mut store, exact_source).is_ok());
    assert!(
        DialogueSources::build(
            &mut store,
            Limits {
                retained_bytes: exact_source.retained_bytes - 1,
                ..exact_source
            }
        )
        .is_err()
    );
    let prepared = sources
        .prepare(&mut store, &request, &Signatures::new(), Limits::default())
        .unwrap();
    let exact = Limits {
        retained_bytes: prepared.retained_bytes(),
        ..Limits::default()
    };
    assert!(
        sources
            .prepare(&mut store, &request, &Signatures::new(), exact)
            .is_ok()
    );
    assert!(
        sources
            .prepare(
                &mut store,
                &request,
                &Signatures::new(),
                Limits {
                    retained_bytes: exact.retained_bytes - 1,
                    ..exact
                }
            )
            .is_err()
    );
    let topic = store.read(store.winner(&key(0x100)).unwrap()).unwrap();
    let info = store.read(store.winner(&key(0x300)).unwrap()).unwrap();
    let read_cost = topic.payload.len() + 2 * info.payload.len();
    assert!(
        sources
            .prepare(
                &mut store,
                &request,
                &Signatures::new(),
                Limits {
                    read_bytes: read_cost,
                    ..Limits::default()
                }
            )
            .is_ok()
    );
    for limits in [
        Limits {
            read_bytes: read_cost - 1,
            ..Limits::default()
        },
        Limits {
            conditions: 0,
            ..Limits::default()
        },
        Limits {
            sections: 1,
            ..Limits::default()
        },
        Limits {
            fields: 1,
            ..Limits::default()
        },
        Limits {
            record_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(
            sources
                .prepare(&mut store, &request, &Signatures::new(), limits)
                .is_err()
        );
    }
}

#[test]
fn repeated_script_roles_and_orphan_subtitles_stay_explicit() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let payload = [
        field(b"NAM1", b"orphan\0"),
        response(0, 0, b"kept\0"),
        script(),
        script(),
    ]
    .concat();
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["Base.esm"]),
            group(0x100, &record(b"INFO", 0x300, 0, &payload)),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = open(directory.path(), &["Base.esm", "Patch.esp"]);
    let sources = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let request = sources.request(key(0x100), key(0x300), None).unwrap();
    let prepared = sources
        .prepare(&mut store, &request, &Signatures::new(), Limits::default())
        .unwrap();
    let catalogue = Catalogue::load(&mut store, ScriptLimits::default(), |_, _| Ok(())).unwrap();
    assert_eq!(prepared.subtitle_bytes(0, 0), Some(b"kept\0".as_slice()));
    assert_eq!(prepared.info_bytes(0), Some(b"orphan\0".as_slice()));
    assert!(
        prepared
            .metadata()
            .info_findings
            .iter()
            .any(|finding| finding.reason == "repeated script role")
    );
    assert!(
        prepared
            .metadata()
            .fragments
            .iter()
            .all(|fragment| fragment.resolve(&catalogue).is_err())
    );
}

#[test]
fn compressed_info_charges_decoded_work_before_admission() {
    use std::io::Write;
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut text = vec![b'a'; 32_768];
    *text.last_mut().unwrap() = 0;
    let payload = response(1, 0, &text);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&payload).unwrap();
    let stored = [
        (payload.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat();
    assert!(stored.len() < payload.len());
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["Base.esm"]),
            group(0x100, &record(b"INFO", 0x300, plugin::COMPRESSED, &stored)),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = open(directory.path(), &["Base.esm", "Patch.esp"]);
    let sources = DialogueSources::build(&mut store, Limits::default()).unwrap();
    let request = sources.request(key(0x100), key(0x300), None).unwrap();
    let topic = store.read(store.winner(&key(0x100)).unwrap()).unwrap();
    let exact_cost = topic.payload.len() + 2 * payload.len();
    assert!(
        sources
            .prepare(
                &mut store,
                &request,
                &Signatures::new(),
                Limits {
                    read_bytes: exact_cost - 1,
                    ..Limits::default()
                }
            )
            .is_err()
    );
    assert!(
        sources
            .prepare(
                &mut store,
                &request,
                &Signatures::new(),
                Limits {
                    record_bytes: stored.len(),
                    ..Limits::default()
                }
            )
            .is_err()
    );
    let prepared = sources
        .prepare(
            &mut store,
            &request,
            &Signatures::new(),
            Limits {
                read_bytes: exact_cost,
                ..Limits::default()
            },
        )
        .unwrap();
    assert_eq!(prepared.subtitle_bytes(0, 0), Some(text.as_slice()));
}
