use fallout_data::{
    actors::{
        self,
        dependencies::{
            Sex,
            equipment::{self, Choice, Role},
            material_overrides::{self, Limits},
        },
    },
    assets::ArchiveAssets,
    identity::{FormKey, ProfileId},
    inventory, plugin,
    store::RecordStore,
};
use std::{fs, path::Path};
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn field(tag: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn disk(tag: &[u8; 4], id: u32, flags: u32, version: u16, raw: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(raw.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0; 2],
        raw,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut r = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for m in masters {
        r.extend(field(b"MAST", &[m.as_bytes(), &[0]].concat()));
        r.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &r)
}
fn array(entries: &[(&[u8], u32, i32)]) -> Vec<u8> {
    let mut r = (entries.len() as u32).to_le_bytes().to_vec();
    for (name, id, index) in entries {
        r.extend((name.len() as u32).to_le_bytes());
        r.extend(*name);
        r.extend(id.to_le_bytes());
        r.extend(index.to_le_bytes());
    }
    r
}
fn alts() -> Vec<u8> {
    array(&[
        (b"\xff\0\xfe", 0x400, i32::MIN),
        (b"\xff\0\xfe", 0x401, i32::MIN),
        (b"\xff\0\xfe", 0x402, i32::MAX),
        (b"", 0, -1),
        (b"missing", 0x999, 0),
        (b"wrong", 0x500, 1),
        (b"unique", 0x400, 0),
        (b"node\0tail", 0x401, 2),
    ])
}
fn slots() -> Vec<u8> {
    [
        field(b"BMDT", &[0xff, 0xff, 0xff, 0xff, 255, 170, 187, 204]),
        field(b"ETYP", &i32::MIN.to_le_bytes()),
    ]
    .concat()
}
fn armor() -> Vec<u8> {
    [
        slots(),
        field(b"MODS", &alts()),
        field(b"MOD3", b"female.nif\0"),
        field(b"MO3S", &array(&[(b"female", 0x401, i32::MAX)])),
        field(b"MODL", b"male.nif\0"),
        field(b"MOD2", b"male_world.nif\0"),
        field(b"MO2S", &array(&[(b"male_world", 0x400, -1)])),
        field(b"MOD4", b"female_world.nif\0"),
        field(b"MO4S", &array(&[])),
    ]
    .concat()
}
fn weapon(first: u32) -> Vec<u8> {
    [
        field(b"ETYP", &(-1_i32).to_le_bytes()),
        field(b"MODL", b"weapon.nif\0"),
        field(b"MODS", &array(&[(b"weapon", 0x400, 3)])),
        field(b"MWD1", b"modded.nif\0"),
        field(b"MOD2", b"shell.nif\0"),
        field(b"MO2S", &array(&[(b"shell", 0x401, -2)])),
        field(b"MOD3", b"scope.nif\0"),
        field(b"MO3S", &array(&[(b"scope", 0x400, -3)])),
        field(b"MOD4", b"world.nif\0"),
        field(b"MO4S", &array(&[(b"world", 0x400, -4)])),
        field(b"WNAM", &first.to_le_bytes()),
        field(b"WNM1", &0x301_u32.to_le_bytes()),
    ]
    .concat()
}
fn fixture(path: &Path, variant: u8) {
    let data = path.join("Data");
    fs::create_dir_all(&data).unwrap();
    let mut r = header(&[]);
    r.extend(disk(
        b"NPC_",
        0x100,
        0,
        15,
        &[field(b"ACBS", &[0; 24]), field(b"DATA", &[0; 11])].concat(),
    ));
    r.extend(disk(b"NPC_", 0x120, plugin::DELETED, 15, &[]));
    r.extend(disk(b"ARMO", 0x200, 0, 15, &armor()));
    r.extend(disk(
        b"ARMO",
        0x201,
        0,
        15,
        &[
            armor(),
            field(b"MODL", b"duplicate.nif\0"),
            field(b"MODS", &array(&[(b"other", 0x400, 0)])),
        ]
        .concat(),
    ));
    r.extend(disk(
        b"ARMO",
        0x202,
        0,
        15,
        &[
            slots(),
            field(b"MODS", &array(&[(b"without_model", 0x400, 0)])),
        ]
        .concat(),
    ));
    r.extend(disk(
        b"ARMO",
        0x203,
        0,
        15,
        &[slots(), field(b"MODL", b"no_array.nif\0")].concat(),
    ));
    for (id, raw) in [
        (0x204, u32::MAX.to_le_bytes().to_vec()),
        (
            0x205,
            [
                1_u32.to_le_bytes().as_slice(),
                &4_u32.to_le_bytes(),
                &[0; 3],
            ]
            .concat(),
        ),
        (0x206, [array(&[]), vec![0]].concat()),
        (0x207, vec![0; 3]),
    ] {
        r.extend(disk(
            b"ARMO",
            id,
            0,
            15,
            &[slots(), field(b"MODL", b"bad.nif\0"), field(b"MODS", &raw)].concat(),
        ));
    }
    r.extend(disk(b"ARMO", 0x208, plugin::DELETED, 15, &[]));
    r.extend(disk(b"GLOB", 0x209, 0, 15, &[variant]));
    r.extend(disk(b"ARMA", 0x210, 0, 15, &armor()));
    r.extend(disk(b"ARMO", 0x211, 0, 99, &armor()));
    r.extend(disk(b"WEAP", 0x220, 0, 15, &weapon(0x300)));
    r.extend(disk(
        b"WEAP",
        0x221,
        0,
        15,
        &[weapon(0x300), field(b"WNAM", &0x301_u32.to_le_bytes())].concat(),
    ));
    r.extend(disk(b"WEAP", 0x222, 0, 15, &weapon(0x500)));
    r.extend(disk(b"WEAP", 0x223, 0, 15, &weapon(0x302)));
    r.extend(disk(
        b"STAT",
        0x300,
        0,
        15,
        &[
            field(b"MODS", &array(&[(b"first_person", 0x401, 15)])),
            field(b"MODL", b"first.nif\0"),
        ]
        .concat(),
    ));
    r.extend(disk(
        b"STAT",
        0x301,
        0,
        15,
        &[
            field(b"MODL", b"one.nif\0"),
            field(b"MODS", &array(&[(b"ambiguous", 0x401, -15)])),
            field(b"MODL", b"two.nif\0"),
        ]
        .concat(),
    ));
    r.extend(disk(b"STAT", 0x302, plugin::DELETED, 15, &[]));
    // All leaf bodies are deliberately invalid subrecord streams.
    for (tag, id, flags) in [
        (b"TXST", 0x400, 0),
        (b"TXST", 0x401, 0),
        (b"TXST", 0x402, plugin::DELETED),
        (b"GLOB", 0x500, 0),
    ] {
        r.extend(disk(tag, id, flags, 15, &[variant]));
    }
    fs::write(data.join("FalloutNV.esm"), r).unwrap();
    let mut r = header(&["FalloutNV.esm"]);
    r.extend(disk(
        b"ARMO",
        0x200,
        0x80,
        15,
        &[
            slots(),
            field(b"MODL", b"override.nif\0"),
            field(b"MODS", &array(&[(b"override", 0x400, i32::MAX)])),
        ]
        .concat(),
    ));
    r.extend(disk(b"TXST", 0x400, 0x80, 15, &[variant]));
    r.extend(disk(
        b"STAT",
        0x300,
        0,
        15,
        &[
            field(b"MODL", b"override_first.nif\0"),
            field(b"MODS", &array(&[(b"override_first", 0x400, -123)])),
        ]
        .concat(),
    ));
    fs::write(data.join("Override.esp"), r).unwrap();
}
fn role(label: &str) -> Role {
    match label {
        "armor-male-biped" => Role::ArmorBiped { sex: Sex::Male },
        "armor-female-biped" => Role::ArmorBiped { sex: Sex::Female },
        "armor-male-world" => Role::ArmorWorld { sex: Sex::Male },
        "armor-female-world" => Role::ArmorWorld { sex: Sex::Female },
        "weapon-model:0" => Role::WeaponModel { mod_mask: 0 },
        "weapon-model:1" => Role::WeaponModel { mod_mask: 1 },
        "weapon-first-person:0" => Role::WeaponFirstPerson { mod_mask: 0 },
        "weapon-first-person:1" => Role::WeaponFirstPerson { mod_mask: 1 },
        "weapon-shell" => Role::WeaponShell,
        "weapon-scope" => Role::WeaponScope,
        "weapon-world" => Role::WeaponWorld,
        _ => panic!("literal role"),
    }
}
fn store(path: &Path, overridden: bool) -> RecordStore {
    let names = if overridden {
        vec!["FalloutNV.esm".into(), "Override.esp".into()]
    } else {
        vec!["FalloutNV.esm".into()]
    };
    RecordStore::open_nv_headers(&path.join("Data"), &names, Default::default()).unwrap()
}
fn project(
    path: &Path,
    id: u32,
    label: &str,
    overridden: bool,
    limits: Limits,
) -> fallout_data::Result<serde_json::Value> {
    let mut s = store(path, overridden);
    let inv = inventory::Catalogue::load(&mut s, Default::default())?;
    let actors = actors::Catalogue::load(&inv, Default::default())?;
    let assets = ArchiveAssets::open_nv(path)?;
    Ok(serde_json::to_value(material_overrides::request(
        &mut s,
        &actors,
        &key(0x100),
        Choice {
            equipment: key(id),
            role: role(label),
        },
        &assets,
        limits,
    )?)
    .unwrap())
}
#[test]
fn exact_raw_names_signed_indices_duplicates_bindings_and_unordered_model_roles() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(
        tmp.path(),
        0x200,
        "armor-male-biped",
        false,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        m["selected_model_kind"],
        serde_json::json!([77, 79, 68, 76])
    );
    assert_eq!(m["model_field_indices"], serde_json::json!([5]));
    assert_eq!(m["arrays"][0]["field_index"], 2);
    let e = &m["arrays"][0]["entries"];
    assert_eq!(e.as_array().unwrap().len(), 8);
    assert_eq!(e[0]["name_bytes"], serde_json::json!([255, 0, 254]));
    assert_eq!(e[0]["field_byte_offset"], 4);
    assert_eq!(e[0]["name_byte_offset"], 8);
    assert_eq!(e[0]["texture_byte_offset"], 11);
    assert_eq!(e[0]["index_byte_offset"], 15);
    assert_eq!(e[0]["mesh_index"], i32::MIN);
    assert_eq!(e[0]["index_word"], 0x80000000_u32);
    for i in [0, 1] {
        assert_eq!(e[i]["duplicate_mesh_declaration"], true);
        assert_eq!(e[i]["source_binding_available"], false);
    }
    assert_eq!(e[2]["mesh_index"], i32::MAX);
    assert_eq!(e[2]["texture"]["status"], "deleted");
    assert!(e[2]["texture_source"].is_object());
    assert_eq!(e[3]["name_bytes"], serde_json::json!([]));
    assert_eq!(e[3]["texture"]["status"], "null");
    assert_eq!(e[4]["texture"]["status"], "missing");
    assert_eq!(e[5]["schema_kind_allowed"], false);
    assert_eq!(e[6]["source_binding_available"], true);
    assert_eq!(
        e[7]["name_bytes"],
        serde_json::json!([110, 111, 100, 101, 0, 116, 97, 105, 108])
    );
    let female = project(
        tmp.path(),
        0x200,
        "armor-female-biped",
        false,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(female["arrays"][0]["field_index"], 4);
    assert_eq!(
        female["arrays"][0]["entries"][0]["name_bytes"],
        serde_json::json!([102, 101, 109, 97, 108, 101])
    );
    assert_eq!(m["texture_swaps_applied"], false);
    assert_eq!(m["mesh_target_selected"], false);
    assert_eq!(m["render_material_supported"], false);
}
#[test]
fn repeated_or_missing_models_and_arrays_never_select_first_or_another_role() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(
        tmp.path(),
        0x201,
        "armor-male-biped",
        false,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(m["selected_model_unique"], false);
    assert_eq!(m["alternate_array_repeated"], true);
    assert_eq!(m["arrays"].as_array().unwrap().len(), 2);
    assert!(
        m["arrays"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|a| a["entries"].as_array().unwrap())
            .all(|e| e["source_binding_available"] == false)
    );
    let m = project(
        tmp.path(),
        0x202,
        "armor-male-biped",
        false,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(m["selected_model_unique"], false);
    assert_eq!(m["arrays"][0]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        m["arrays"][0]["entries"][0]["source_binding_available"],
        false
    );
    assert!(
        project(
            tmp.path(),
            0x203,
            "armor-male-biped",
            false,
            Limits::default()
        )
        .unwrap()["arrays"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        project(
            tmp.path(),
            0x200,
            "armor-female-world",
            false,
            Limits::default()
        )
        .unwrap()["arrays"][0]["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn first_person_stat_and_weapon_roles_use_existing_explicit_selection_without_palette_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    for label in [
        "weapon-model:0",
        "weapon-shell",
        "weapon-scope",
        "weapon-world",
    ] {
        let m = project(tmp.path(), 0x220, label, false, Limits::default()).unwrap();
        assert_eq!(m["arrays"].as_array().unwrap().len(), 1);
        assert_eq!(m["arrays"][0]["source_index"], 0);
    }
    let m = project(
        tmp.path(),
        0x220,
        "weapon-first-person:0",
        false,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(m["selected_source_index"], 1);
    assert_eq!(m["arrays"][0]["source_index"], 1);
    assert_eq!(m["arrays"][0]["entries"][0]["mesh_index"], 15);
    let m = project(
        tmp.path(),
        0x220,
        "weapon-model:1",
        false,
        Limits::default(),
    )
    .unwrap();
    assert!(m["alternate_kind"].is_null());
    assert!(m["arrays"].as_array().unwrap().is_empty());
    let m = project(
        tmp.path(),
        0x220,
        "weapon-first-person:1",
        false,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(m["selected_model_unique"], false);
    assert_eq!(
        m["arrays"][0]["entries"][0]["source_binding_available"],
        false
    );
    for id in [0x221, 0x222, 0x223] {
        let m = project(
            tmp.path(),
            id,
            "weapon-first-person:0",
            false,
            Limits::default(),
        )
        .unwrap();
        assert!(m["selected_source_index"].is_null());
        assert!(m["arrays"].as_array().unwrap().is_empty());
    }
    for id in [0x208, 0x209, 0x999] {
        let m = project(tmp.path(), id, "armor-male-biped", false, Limits::default()).unwrap();
        assert!(m["selected_source_index"].is_null());
    }
    assert!(
        project(
            tmp.path(),
            0x210,
            "armor-male-biped",
            false,
            Limits::default()
        )
        .is_ok()
    );
    assert!(
        project(
            tmp.path(),
            0x211,
            "armor-male-biped",
            false,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn malformed_count_name_extent_words_and_trailing_data_refuse_without_a_partial_request() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    for id in [0x204, 0x205, 0x206, 0x207] {
        assert!(project(tmp.path(), id, "armor-male-biped", false, Limits::default()).is_err());
    }
}
#[test]
fn all_exact_projection_and_work_budget_edges_are_checked() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(
        tmp.path(),
        0x200,
        "armor-male-biped",
        false,
        Limits::default(),
    )
    .unwrap();
    let c = &m["counts"];
    let largest = m["equipment"]["source_records"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["bytes"].as_u64().unwrap() as usize + 6)
        .sum();
    let mut l = Limits {
        max_sources: 1,
        max_record_bytes: largest,
        max_decoded_bytes: c["decoded_bytes"].as_u64().unwrap() as usize,
        max_field_visits: c["field_visits"].as_u64().unwrap() as usize,
        max_fields: c["fields"].as_u64().unwrap() as usize,
        max_arrays: c["arrays"].as_u64().unwrap() as usize,
        max_entries: c["entries"].as_u64().unwrap() as usize,
        max_name_bytes: c["name_bytes"].as_u64().unwrap() as usize,
        max_raw_bytes: c["raw_bytes"].as_u64().unwrap() as usize,
        max_bindings: c["bindings"].as_u64().unwrap() as usize,
        max_headers: c["headers"].as_u64().unwrap() as usize,
        max_projection_bytes: serde_json::to_vec(&m).unwrap().len(),
        ..Limits::default()
    };
    assert!(project(tmp.path(), 0x200, "armor-male-biped", false, l).is_ok());
    for i in 0..12 {
        let mut low = l;
        match i {
            0 => low.max_sources -= 1,
            1 => low.max_record_bytes -= 1,
            2 => low.max_decoded_bytes -= 1,
            3 => low.max_field_visits -= 1,
            4 => low.max_fields -= 1,
            5 => low.max_arrays -= 1,
            6 => low.max_entries -= 1,
            7 => low.max_name_bytes -= 1,
            8 => low.max_raw_bytes -= 1,
            9 => low.max_bindings -= 1,
            10 => low.max_headers -= 1,
            _ => low.max_projection_bytes -= 1,
        };
        assert!(
            project(tmp.path(), 0x200, "armor-male-biped", false, low).is_err(),
            "limit{i}"
        );
    }
    l.max_entries = 0;
    assert!(project(tmp.path(), 0x200, "armor-female-world", false, l).is_ok());
}
#[test]
fn freshly_constructed_wrapper_preserves_old_equipment_projection_and_rejects_stale_actor_cohorts()
{
    let tmp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    fixture(other.path(), 1);
    let mut s = store(tmp.path(), false);
    let inv = inventory::Catalogue::load(&mut s, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inv, Default::default()).unwrap();
    let assets = ArchiveAssets::open_nv(tmp.path()).unwrap();
    let choice = Choice {
        equipment: key(0x200),
        role: role("armor-male-biped"),
    };
    let before = equipment::request(
        &mut s,
        &actors,
        &key(0x100),
        choice.clone(),
        &assets,
        Default::default(),
    )
    .unwrap();
    let after = material_overrides::request(
        &mut s,
        &actors,
        &key(0x100),
        choice.clone(),
        &assets,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(after.equipment()).unwrap()
    );
    let mut changed = store(other.path(), false);
    assert!(
        material_overrides::request(
            &mut changed,
            &actors,
            &key(0x100),
            choice,
            &assets,
            Default::default()
        )
        .is_err()
    );
    let over = project(
        tmp.path(),
        0x200,
        "armor-male-biped",
        true,
        Limits::default(),
    )
    .unwrap();
    assert_eq!(
        over["equipment"]["source_records"][0]["source"]["plugin"],
        "Override.esp"
    );
    assert_eq!(
        over["arrays"][0]["entries"][0]["texture_source"]["source_name"],
        "Override.esp"
    );
    assert_eq!(
        over["arrays"][0]["entries"][0]["texture_source"]["header"]["flags"],
        128
    );
}
#[test]
fn export_literal_role_requests_for_independent_comparison() {
    let Ok(path) = std::env::var("FALLOUT_ACTOR_MATERIAL_EVIDENCE_DIR") else {
        return;
    };
    let path = Path::new(&path).join("fixture");
    fixture(&path, 0);
    fs::write(path.join("base-order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        path.join("override-order.json"),
        b"[\"FalloutNV.esm\",\"Override.esp\"]",
    )
    .unwrap();
    for (cohort, overridden, cases) in [
        (
            "base",
            false,
            vec![
                (0x200, "armor-male-biped"),
                (0x200, "armor-female-biped"),
                (0x200, "armor-male-world"),
                (0x200, "armor-female-world"),
                (0x201, "armor-male-biped"),
                (0x202, "armor-male-biped"),
                (0x203, "armor-male-biped"),
                (0x208, "armor-male-biped"),
                (0x209, "armor-male-biped"),
                (0x210, "armor-male-biped"),
                (0x220, "weapon-model:0"),
                (0x220, "weapon-model:1"),
                (0x220, "weapon-shell"),
                (0x220, "weapon-scope"),
                (0x220, "weapon-world"),
                (0x220, "weapon-first-person:0"),
                (0x220, "weapon-first-person:1"),
                (0x221, "weapon-first-person:0"),
                (0x222, "weapon-first-person:0"),
                (0x223, "weapon-first-person:0"),
                (0x999, "armor-male-biped"),
            ],
        ),
        (
            "override",
            true,
            vec![
                (0x200, "armor-male-biped"),
                (0x220, "weapon-first-person:0"),
            ],
        ),
    ] {
        for (id, label) in cases {
            let m = project(&path, id, label, overridden, Limits::default()).unwrap();
            let filename = format!("{cohort}-{id:x}-{}-host.json", label.replace(':', "_"));
            fs::write(
                path.join(filename),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"actor_material_overrides":{"manifest":m}}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
}
