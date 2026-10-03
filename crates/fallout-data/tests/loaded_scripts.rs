use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Limits, OwnerKind, ReferenceStatus, ScriptKey},
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], form: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 8],
        payload,
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
    schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
    let mut body = field(b"SCHR", &schr);
    // A framed four-byte statement exercises the
    // loader's byte lifetime without claiming command execution.
    body.extend(field(b"SCDA", &[0x1d, 0, 0, 0]));
    for (index, name) in variables {
        let mut slsd = [0; 24];
        slsd[..4].copy_from_slice(&index.to_le_bytes());
        body.extend(field(b"SLSD", &slsd));
        body.extend(field(b"SCVR", &[*name, &[0]].concat()));
    }
    for (kind, value) in references {
        body.extend(field(kind, &value.to_le_bytes()));
    }
    body
}
fn key(origin: &str, form: u32, offset: u32) -> ScriptKey {
    ScriptKey {
        record: FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: origin.into(),
            local_id: form,
        },
        header_decoded_offset: offset,
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
fn load(path: &Path, names: &[&str]) -> Catalogue {
    Catalogue::load(&mut open(path, names), Limits::default(), |_, _| Ok(())).unwrap()
}

#[test]
fn handles_survive_unrelated_reordering_and_reject_changed_source_versions() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [header(&[]), record(b"SCPT", 0x100, 0, &unit(&[], &[]))].concat(),
    )
    .unwrap();
    let patch = directory.path().join("Patch.esp");
    fs::write(
        &patch,
        [
            header(&["Other.esm", "Base.esm"]),
            record(b"SCPT", 0x0100_0100, 0, &unit(&[], &[(42, b"first")])),
        ]
        .concat(),
    )
    .unwrap();
    let a = load(directory.path(), &["Other.esm", "Base.esm", "Patch.esp"]);
    let b = load(directory.path(), &["Base.esm", "Other.esm", "Patch.esp"]);
    let script_key = key("base.esm", 0x100, 0);
    let handle = a.get(&script_key).unwrap().handle().clone();
    assert_eq!(b.get(&script_key).unwrap().handle(), &handle);
    assert!(b.get_handle(&handle).is_some());
    assert_eq!(
        a.get(&script_key).unwrap().version().source_plugin,
        "Patch.esp"
    );
    assert_eq!(a.counts.scripts, 1);
    assert_eq!(a.sources.len(), 3); // Empty plugins still contribute source receipts.
    fs::write(
        &patch,
        [
            header(&["Other.esm", "Base.esm"]),
            record(b"SCPT", 0x0100_0100, 0, &unit(&[], &[(42, b"changed")])),
        ]
        .concat(),
    )
    .unwrap();
    let changed = load(directory.path(), &["Other.esm", "Base.esm", "Patch.esp"]);
    assert!(changed.get(&script_key).is_some());
    assert!(changed.get_handle(&handle).is_none());
    assert_ne!(
        changed.get(&script_key).unwrap().handle().version_sha256,
        handle.version_sha256
    );
}

#[test]
fn references_use_source_namespace_and_preserve_every_resolution_state() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"ACTI", 0x100, 0, &[]),
            record(b"ACTI", 0x200, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let body = unit(
        &[
            (b"SCRO", 0x100),
            (b"SCRO", 0x200),
            (b"SCRO", 0x300),
            (b"SCRO", 0),
            (b"SCRO", 0x14),
            (b"SCRV", 42),
            (b"SCRV", 99),
        ],
        &[(42, b"ref_var")],
    );
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["FalloutNV.esm"]),
            record(b"ACTI", 0x200, plugin::DELETED, &[]),
            record(b"SCPT", 0x0100_0400, 0, &body),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["FalloutNV.esm", "Patch.esp"]);
    let script = catalogue.get(&key("patch.esp", 0x400, 0)).unwrap();
    assert_eq!(
        script
            .references()
            .iter()
            .map(|r| r.status)
            .collect::<Vec<_>>(),
        vec![
            ReferenceStatus::DefinedForm,
            ReferenceStatus::DeletedForm,
            ReferenceStatus::MissingForm,
            ReferenceStatus::NullForm,
            ReferenceStatus::RuntimeDependency,
            ReferenceStatus::DynamicVariable,
            ReferenceStatus::MissingVariableDeclaration
        ]
    );
    assert_eq!(
        script
            .reference(1)
            .unwrap()
            .form_key
            .as_ref()
            .unwrap()
            .origin_plugin,
        "falloutnv.esm"
    );
    assert_eq!(
        script
            .reference(2)
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .source_plugin,
        "Patch.esp"
    );
    assert!(script.reference(5).unwrap().runtime_dependency.is_some());
    assert!(
        script
            .reference(6)
            .unwrap()
            .variable_declaration_offset
            .is_some()
    );
    assert!(script.reference(0).is_none());
    assert!(script.reference(8).is_none());
    assert!(script.issues().is_empty());
}

#[test]
fn sparse_duplicate_declarations_keep_first_match_and_outlive_the_store() {
    let directory = tempfile::tempdir().unwrap();
    let payload = unit(
        &[(b"SCRV", 42)],
        &[(42, b"first_\xe9"), (42, b"duplicate"), (0, b"zero")],
    );
    fs::write(
        directory.path().join("Base.esm"),
        [header(&[]), record(b"SCPT", 0x100, 0, &payload)].concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["Base.esm"]);
    // The source file handles and temporary decoded units have already dropped.
    let script = catalogue.get(&key("base.esm", 0x100, 0)).unwrap();
    assert_eq!(script.declarations().len(), 3);
    assert_eq!(
        script.declaration_name(42),
        Some(b"first_\xe9\0".as_slice())
    );
    assert_eq!(script.declaration_name(0), Some(b"zero\0".as_slice()));
    assert!(script.declaration(1).is_none());
    assert_eq!(
        script.reference(1).unwrap().variable_declaration_offset,
        Some(script.declaration(42).unwrap().decoded_offset)
    );
    assert_eq!(script.compiled(), Some([0x1d, 0, 0, 0].as_slice()));
    assert_eq!(script.program().unwrap().unwrap().instructions.len(), 1);
    assert_eq!(catalogue.counts.duplicate_variable_indices, 1);
    assert_eq!(script.owner().kind, OwnerKind::Standalone);
}

#[test]
fn embedded_scripts_keep_distinct_authored_owners_and_unknown_roles() {
    let directory = tempfile::tempdir().unwrap();
    let a = unit(&[], &[]);
    let info = [a.clone(), field(b"NEXT", &[]), a.clone()].concat();
    let quest = [
        field(b"INDX", &7_i16.to_le_bytes()),
        field(b"QSDT", &[0]),
        a.clone(),
    ]
    .concat();
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"INFO", 0x100, 0, &info),
            record(b"QUST", 0x200, 0, &quest),
            record(b"PACK", 0x300, 0, &a),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["Base.esm"]);
    let begin = catalogue.get(&key("base.esm", 0x100, 0)).unwrap();
    let end = catalogue
        .get(&key("base.esm", 0x100, (a.len() + 6) as u32))
        .unwrap();
    assert_eq!(begin.owner().kind, OwnerKind::DialogueBegin);
    assert_eq!(end.owner().kind, OwnerKind::DialogueEnd);
    assert_ne!(begin.handle().key, end.handle().key);
    assert!(begin.owner().schema_ownership_verified);
    let log = catalogue.get(&key("base.esm", 0x200, 15)).unwrap();
    assert_eq!(log.owner().kind, OwnerKind::QuestLogEntry);
    assert_eq!(log.owner().stage_marker, Some(0));
    assert_eq!(log.owner().section_marker, Some(8));
    let unknown = catalogue.get(&key("base.esm", 0x300, 0)).unwrap();
    assert_eq!(unknown.owner().kind, OwnerKind::UnverifiedEmbedded);
    assert!(!unknown.owner().schema_ownership_verified);
}

#[test]
fn deleted_winners_never_resurrect_an_earlier_script() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [header(&[]), record(b"SCPT", 0x100, 0, &unit(&[], &[]))].concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Delete.esp"),
        [
            header(&["Base.esm"]),
            record(b"SCPT", 0x100, plugin::DELETED, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["Base.esm", "Delete.esp"]);
    assert_eq!(catalogue.iter().count(), 0);
    assert_eq!(catalogue.counts.deleted_candidates_skipped, 1);
    assert_eq!(catalogue.counts.candidate_records_read, 0);
}

#[test]
fn resource_limits_and_invalid_compiled_framing_fail_before_publication() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Base.esm");
    fs::write(
        &path,
        [
            header(&[]),
            record(b"SCPT", 0x100, 0, &unit(&[(b"SCRO", 0)], &[(42, b"v")])),
        ]
        .concat(),
    )
    .unwrap();
    for limits in [
        Limits {
            max_candidate_records: 0,
            ..Limits::default()
        },
        Limits {
            max_scripts: 0,
            ..Limits::default()
        },
        Limits {
            max_retained_bytes: 0,
            ..Limits::default()
        },
        Limits {
            max_variables: 0,
            ..Limits::default()
        },
        Limits {
            max_references: 0,
            ..Limits::default()
        },
    ] {
        assert!(
            Catalogue::load(
                &mut open(directory.path(), &["Base.esm"]),
                limits,
                |_, _| Ok(())
            )
            .is_err()
        );
    }
    fs::write(
        &path,
        [
            header(&[]),
            record(
                b"SCPT",
                0x100,
                0,
                &[
                    field(b"SCHR", &[0; 20]),
                    field(b"SCDA", &[0x15, 0, 255, 255]),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
    assert!(
        Catalogue::load(
            &mut open(directory.path(), &["Base.esm"]),
            Limits::default(),
            |_, _| Ok(())
        )
        .is_err()
    );
}
