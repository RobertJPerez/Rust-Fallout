use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Handle, Limits, ScriptKey},
    plugin,
    quest_scripts::{self, Attachments, DeclarationStatus},
    store::RecordStore,
};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], form: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &form.to_le_bytes(),
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
    record(b"TES4", 0, &body)
}
fn unit(name: &[u8], caller: bool) -> Vec<u8> {
    // One source assignment references the quest's declaration. It is inspected,
    // never evaluated, so no numeric or lifecycle rule is implied.
    let compiled = if caller {
        let expression = b"r\x01\0f\x2a\0";
        let operands = [
            b"f\x2a\0".as_slice(),
            &(expression.len() as u16).to_le_bytes(),
            expression,
        ]
        .concat();
        [
            0x15_u16.to_le_bytes().as_slice(),
            &(operands.len() as u16).to_le_bytes(),
            &operands,
        ]
        .concat()
    } else {
        Vec::new()
    };
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&u32::from(caller).to_le_bytes());
    schr[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    let mut body = field(b"SCHR", &schr);
    if caller {
        body.extend(field(b"SCDA", &compiled));
    }
    let mut declaration = [0; 24];
    declaration[..4].copy_from_slice(&42_u32.to_le_bytes());
    body.extend(field(b"SLSD", &declaration));
    body.extend(field(b"SCVR", &[name, &[0]].concat()));
    if caller {
        body.extend(field(b"SCRO", &0x01000100_u32.to_le_bytes()));
    }
    body
}
fn key(plugin: &str, id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: plugin.into(),
        local_id: id,
    }
}
struct Snapshot {
    directory: tempfile::TempDir,
    catalogue: Catalogue,
    attachments: Attachments,
    caller: Handle,
}
fn snapshot(target: u32, unrelated: bool) -> Snapshot {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x200, &unit(b"Alpha", false)),
            record(b"SCPT", 0x201, &unit(b"Beta", false)),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Quest.esm"),
        [
            header(&["Base.esm"]),
            record(b"QUST", 0x01000100, &field(b"SCRI", &target.to_le_bytes())),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("Caller.esp"),
        [
            header(&["Base.esm", "Quest.esm"]),
            record(b"SCPT", 0x02000300, &unit(b"Caller", true)),
        ]
        .concat(),
    )
    .unwrap();
    let mut names = vec!["Base.esm".into(), "Quest.esm".into(), "Caller.esp".into()];
    if unrelated {
        fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
        names.insert(0, "Other.esm".into());
    }
    let mut store =
        RecordStore::open_nv_headers(directory.path(), &names, plugin::Limits::default()).unwrap();
    let catalogue = Catalogue::load(&mut store, Limits::default(), |_, _| Ok(())).unwrap();
    let attachments = Attachments::load(&mut store, &catalogue, 10, |_, _| Ok(())).unwrap();
    let caller = catalogue
        .get(&ScriptKey {
            record: key("caller.esp", 0x300),
            header_decoded_offset: 0,
        })
        .unwrap()
        .handle()
        .clone();
    Snapshot {
        directory,
        catalogue,
        attachments,
        caller,
    }
}
fn retain(name: &str, a: &Snapshot, b: &Snapshot, observation: Value) {
    let Some(root) = std::env::var_os("FALLOUT_QUEST_JOIN_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&root);
    assert!(root.is_absolute() && root.is_dir());
    let case = root.join(name);
    fs::create_dir(&case).unwrap();
    for (label, source) in [("a", a), ("b", b)] {
        let output = case.join(label);
        fs::create_dir(&output).unwrap();
        for entry in fs::read_dir(source.directory.path()).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), output.join(entry.file_name())).unwrap();
        }
    }
    fs::write(
        case.join("observation.json"),
        serde_json::to_vec_pretty(&observation).unwrap(),
    )
    .unwrap();
}
fn cross_case(name: &str, from: u32, to: u32) {
    let a = snapshot(from, false);
    let b = snapshot(to, false);
    assert_eq!(a.caller, b.caller);
    for (key, script) in a.catalogue.iter() {
        assert_eq!(script.handle(), b.catalogue.get(key).unwrap().handle());
    }
    let qa = a.attachments.get(&key("quest.esm", 0x100)).unwrap();
    let qb = b.attachments.get(&key("quest.esm", 0x100)).unwrap();
    assert_eq!(qa.source.plugin, qb.source.plugin);
    assert_eq!(qa.source.record_file_offset, qb.source.record_file_offset);
    assert_eq!(qa.source.record_flags, qb.source.record_flags);
    assert_ne!(qa.source.sha256, qb.source.sha256);
    let own = quest_scripts::declaration(&a.catalogue, &a.attachments, &a.caller, 1, 42);
    assert_eq!(own.status, DeclarationStatus::StaticQuestDeclaration);
    assert_eq!(
        own.target_script.as_ref().unwrap().key.record.local_id,
        from
    );
    let cross = quest_scripts::declaration(&a.catalogue, &b.attachments, &a.caller, 1, 42);
    retain(
        name,
        &a,
        &b,
        json!({"a_sources":a.catalogue.sources,"b_sources":b.catalogue.sources,
        "a_caller":a.caller,"b_caller":b.caller,"a_quest":qa,"b_quest":qb,"fresh":own,"cross":cross}),
    );
    assert_eq!(cross.status, DeclarationStatus::QuestWinnerMismatch);
    assert!(cross.target_script.is_none() && cross.declaration.is_none());
    assert!(!cross.live_value_resolved);
}

#[test]
fn changed_quest_scri_cannot_select_another_still_valid_target_handle() {
    cross_case("alpha-to-beta", 0x200, 0x201);
}
#[test]
fn changed_quest_source_is_rejected_in_the_reverse_join_too() {
    cross_case("beta-to-alpha", 0x201, 0x200);
}
#[test]
fn independent_equal_source_sets_keep_the_static_declaration() {
    let a = snapshot(0x200, false);
    let b = snapshot(0x200, false);
    let row = quest_scripts::declaration(&a.catalogue, &b.attachments, &a.caller, 1, 42);
    assert_eq!(row.status, DeclarationStatus::StaticQuestDeclaration);
    assert_eq!(
        row.target_script.as_ref().unwrap().key.record.local_id,
        0x200
    );
    assert!(!row.live_value_resolved);
    retain("equal-source", &a, &b, json!({"lookup":row}));
}
#[test]
fn unrelated_source_and_order_changes_keep_exact_quest_and_target_sources_usable() {
    let a = snapshot(0x200, false);
    let b = snapshot(0x200, true);
    assert_eq!(a.caller, b.caller);
    let row = quest_scripts::declaration(&a.catalogue, &b.attachments, &a.caller, 1, 42);
    assert_eq!(row.status, DeclarationStatus::StaticQuestDeclaration);
    assert_eq!(
        row.target_script.as_ref().unwrap().key.record.local_id,
        0x200
    );
    retain("unrelated-source", &a, &b, json!({"lookup":row}));
}
