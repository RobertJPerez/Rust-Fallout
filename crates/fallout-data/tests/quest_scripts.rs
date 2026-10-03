use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Limits, ScriptKey},
    plugin,
    quest_scripts::{self, Attachments, DeclarationStatus, Status},
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
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
fn unit(references: &[(&[u8; 4], u32)], variables: &[(u32, &[u8])]) -> Vec<u8> {
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&(references.len() as u32).to_le_bytes());
    let mut body = field(b"SCHR", &schr);
    for (index, name) in variables {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        body.extend(field(b"SLSD", &declaration));
        body.extend(field(b"SCVR", &[*name, &[0]].concat()));
    }
    for (kind, raw) in references {
        body.extend(field(kind, &raw.to_le_bytes()));
    }
    body
}
fn key(raw: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: raw,
    }
}
fn open(path: &Path, names: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &names.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn load(store: &mut RecordStore) -> Catalogue {
    Catalogue::load(store, Limits::default(), |_, _| Ok(())).unwrap()
}
fn fixture(path: &Path) {
    let mut bytes = header(&[]);
    for (raw, target) in [
        (0x100, 0x200_u32),
        (0x102, 0),
        (0x104, 0x201),
        (0x105, 0x202),
        (0x106, 0x203),
        (0x107, 0x204),
        (0x108, 0x205),
    ] {
        bytes.extend(record(
            b"QUST",
            raw,
            0,
            &field(b"SCRI", &target.to_le_bytes()),
        ));
    }
    bytes.extend(record(b"QUST", 0x101, 0, &[]));
    bytes.extend(record(
        b"QUST",
        0x103,
        0,
        &[
            field(b"SCRI", &0x200_u32.to_le_bytes()),
            field(b"SCRI", &0x200_u32.to_le_bytes()),
        ]
        .concat(),
    ));
    bytes.extend(record(b"QUST", 0x109, plugin::DELETED, &[]));
    bytes.extend(record(b"QUST", 0x110, 0, &field(b"DATA", &[5, 0])));
    bytes.extend(record(
        b"SCPT",
        0x200,
        0,
        &unit(&[], &[(42, b"target_first"), (42, b"target_second")]),
    ));
    bytes.extend(record(b"SCPT", 0x202, plugin::DELETED, &[]));
    bytes.extend(record(b"ACTI", 0x203, 0, &[]));
    bytes.extend(record(b"SCPT", 0x204, 0, &[]));
    bytes.extend(record(
        b"SCPT",
        0x205,
        0,
        &[unit(&[], &[]), unit(&[], &[])].concat(),
    ));
    bytes.extend(record(b"REFR", 0x210, 0, &[]));
    bytes.extend(record(
        b"SCPT",
        0x300,
        0,
        &unit(
            &[
                (b"SCRO", 0x100),
                (b"SCRO", 0x210),
                (b"SCRO", 0x203),
                (b"SCRO", 0),
                (b"SCRO", 0x14),
                (b"SCRO", 0x999),
                (b"SCRO", 0x202),
                (b"SCRV", 42),
                (b"SCRV", 99),
                (b"SCRO", 0x101),
            ],
            &[(42, b"current_wrong")],
        ),
    ));
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
}

#[test]
fn quest_attachment_states_preserve_absence_null_deletion_and_ambiguity() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut store = open(directory.path(), &["FalloutNV.esm"]);
    let catalogue = load(&mut store);
    let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
    for (raw, status) in [
        (0x100, Status::LoadedDefinition),
        (0x101, Status::NoScriptField),
        (0x102, Status::NullScript),
        (0x103, Status::MultipleScriptFields),
        (0x104, Status::MissingScript),
        (0x105, Status::DeletedScript),
        (0x106, Status::WrongScriptKind),
        (0x107, Status::MissingLoadedDefinition),
        (0x108, Status::MultipleStandaloneUnits),
        (0x109, Status::DeletedQuest),
    ] {
        assert_eq!(attachments.get(&key(raw)).unwrap().status, status);
    }
    assert_eq!(attachments.counts.quests, 11);
    assert_eq!(attachments.counts.source_findings, 1);
    assert_eq!(attachments.get(&key(0x103)).unwrap().fields.len(), 2);
    assert!(
        attachments
            .get(&key(0x109))
            .unwrap()
            .source
            .decoded_record_sha256
            .is_none()
    );
    let target = attachments
        .get(&key(0x100))
        .unwrap()
        .script
        .as_ref()
        .unwrap();
    assert_eq!(target.key.record, key(0x200));
    assert!(catalogue.get_handle(target).is_some());
    assert_eq!(catalogue.record_scripts(&key(0x205)).count(), 2);
}

#[test]
fn foreign_declarations_select_the_quest_script_and_never_current_locals() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut store = open(directory.path(), &["FalloutNV.esm"]);
    let catalogue = load(&mut store);
    let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
    let source = catalogue
        .get(&ScriptKey {
            record: key(0x300),
            header_decoded_offset: 0,
        })
        .unwrap();
    let row = quest_scripts::declaration(&catalogue, &attachments, source.handle(), 1, 42);
    assert_eq!(row.status, DeclarationStatus::StaticQuestDeclaration);
    assert_eq!(row.target_script.as_ref().unwrap().key.record, key(0x200));
    assert_eq!(
        row.declaration.as_ref().unwrap().name_sha256,
        catalogue
            .get_handle(row.target_script.as_ref().unwrap())
            .unwrap()
            .declaration(42)
            .unwrap()
            .name_sha256
    );
    assert_ne!(
        row.declaration.as_ref().unwrap().name_sha256,
        source.declaration(42).unwrap().name_sha256
    );
    assert!(!row.live_value_resolved);
    assert_eq!(
        quest_scripts::declaration(&catalogue, &attachments, source.handle(), 1, 99).status,
        DeclarationStatus::MissingForeignDeclaration
    );
    for (context, status) in [
        (0, DeclarationStatus::MissingContextEntry),
        (2, DeclarationStatus::PlacedReferenceNeedsEventList),
        (3, DeclarationStatus::UnsupportedContextKind),
        (4, DeclarationStatus::NullContext),
        (5, DeclarationStatus::RuntimeContext),
        (6, DeclarationStatus::MissingContextForm),
        (7, DeclarationStatus::DeletedContextForm),
        (8, DeclarationStatus::DynamicContext),
        (9, DeclarationStatus::MissingContextVariable),
        (10, DeclarationStatus::QuestScriptUnavailable),
    ] {
        let row =
            quest_scripts::declaration(&catalogue, &attachments, source.handle(), context, 42);
        assert_eq!(row.status, status);
        assert!(!row.live_value_resolved);
    }
    let mut stale = source.handle().clone();
    stale.version_sha256 = "stale".into();
    assert_eq!(
        quest_scripts::declaration(&catalogue, &attachments, &stale, 1, 42).status,
        DeclarationStatus::StaleSourceHandle
    );
}

#[test]
fn source_rebasing_reordering_and_deleted_script_override_keep_canonical_keys() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x200, 0, &unit(&[], &[])),
            record(b"QUST", 0x100, 0, &field(b"SCRI", &0x200_u32.to_le_bytes())),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["Other.esm", "FalloutNV.esm"]),
            record(
                b"QUST",
                0x0100_0100,
                0,
                &field(b"SCRI", &0x0100_0200_u32.to_le_bytes()),
            ),
        ]
        .concat(),
    )
    .unwrap();
    let mut results = Vec::new();
    for names in [
        &["Other.esm", "FalloutNV.esm", "Patch.esp"][..],
        &["FalloutNV.esm", "Other.esm", "Patch.esp"][..],
    ] {
        let mut store = open(directory.path(), names);
        let catalogue = load(&mut store);
        let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
        let quest = attachments.get(&key(0x100)).unwrap();
        assert_eq!(quest.fields[0].key, Some(key(0x200)));
        results.push(serde_json::to_value(quest).unwrap());
    }
    assert_eq!(results[0], results[1]);
    fs::write(
        directory.path().join("Delete.esp"),
        [
            header(&["FalloutNV.esm"]),
            record(b"SCPT", 0x200, plugin::DELETED, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = open(directory.path(), &["FalloutNV.esm", "Delete.esp"]);
    let catalogue = load(&mut store);
    let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
    assert_eq!(
        attachments.get(&key(0x100)).unwrap().status,
        Status::DeletedScript
    );
    assert!(attachments.get(&key(0x100)).unwrap().script.is_none());
}

#[test]
fn attachment_budget_bad_scri_and_cross_source_catalogues_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut store = open(directory.path(), &["FalloutNV.esm"]);
    let catalogue = load(&mut store);
    assert!(Attachments::load(&mut store, &catalogue, 0, |_, _| Ok(())).is_err());
    drop(store);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"QUST", 0x100, 0, &field(b"SCRI", &[1, 2, 3])),
        ]
        .concat(),
    )
    .unwrap();
    let mut changed = open(directory.path(), &["FalloutNV.esm"]);
    assert!(Attachments::load(&mut changed, &catalogue, 100, |_, _| Ok(())).is_err());
    let fresh = load(&mut changed);
    assert!(Attachments::load(&mut changed, &fresh, 100, |_, _| Ok(())).is_err());
}

#[test]
fn mismatched_quest_winners_are_not_silently_combined() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["FalloutNV.esm"]),
            record(b"QUST", 0x100, 0, &field(b"SCRI", &0x200_u32.to_le_bytes())),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Later.esp"),
        [
            header(&["FalloutNV.esm"]),
            record(b"QUST", 0x100, 0, &field(b"SCRI", &0x200_u32.to_le_bytes())),
        ]
        .concat(),
    )
    .unwrap();
    let mut a = open(
        directory.path(),
        &["FalloutNV.esm", "Patch.esp", "Later.esp"],
    );
    let catalogue = load(&mut a);
    let mut b = open(
        directory.path(),
        &["FalloutNV.esm", "Later.esp", "Patch.esp"],
    );
    let attachments = Attachments::load(&mut b, &catalogue, 100, |_, _| Ok(())).unwrap();
    let source = catalogue
        .get(&ScriptKey {
            record: key(0x300),
            header_decoded_offset: 0,
        })
        .unwrap();
    assert_eq!(
        quest_scripts::declaration(&catalogue, &attachments, source.handle(), 1, 42).status,
        DeclarationStatus::QuestWinnerMismatch
    );
}
