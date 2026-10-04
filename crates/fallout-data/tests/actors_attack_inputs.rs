use fallout_data::{
    actors::attack_inputs::{self, Limits, Role, Value},
    identity::{FormKey, ProfileId},
    plugin,
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
    let mut out = tag.to_vec();
    out.extend((raw.len() as u16).to_le_bytes());
    out.extend(raw);
    out
}
fn record(tag: &[u8; 4], id: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend([19, 22, 31, 43]);
    out.extend(version.to_le_bytes());
    out.extend([55, 66]);
    out.extend(body);
    out
}
fn put(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn weapon_data() -> Vec<u8> {
    let mut d = vec![0; 15];
    put(&mut d, 0, 0x80000000);
    put(&mut d, 4, 0x7fffffff);
    put(&mut d, 8, 0x7fc12345);
    d[12..14].copy_from_slice(&i16::MIN.to_le_bytes());
    d[14] = 255;
    d
}
fn attack(projectile: u32, width: usize) -> Vec<u8> {
    let mut d = vec![0; width];
    for (i, word) in d.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        word.copy_from_slice(&(0x10000000 + i as u32).to_le_bytes());
    }
    put(&mut d, 0, 5);
    put(&mut d, 4, 0);
    d[12..16].copy_from_slice(&[255, 230, 255, 255]);
    put(&mut d, 36, projectile);
    d[40..44].copy_from_slice(&[255, 255, 255, 255]);
    put(&mut d, 56, u32::MAX);
    put(&mut d, 60, 0);
    put(&mut d, 104, 0x80000000);
    if width == 204 {
        put(&mut d, 120, u32::MAX);
        d[172..176].copy_from_slice(&[170, 255, 187, 204]);
    }
    d
}
fn critical() -> Vec<u8> {
    let mut d = vec![255; 16];
    put(&mut d, 4, 0x80000000);
    put(&mut d, 12, 0x500);
    d
}
fn vats(width: usize) -> Vec<u8> {
    let mut d = vec![255; width];
    put(&mut d, 0, 0x500);
    put(&mut d, 4, 0x7fc12345);
    put(&mut d, 8, 0x7f800000);
    put(&mut d, 12, 0x80000000);
    d
}
fn weapon(ammo: u32, projectile: u32, width: usize) -> Vec<u8> {
    [
        field(b"MODL", b"same/model.nif\0"),
        field(b"NAM0", &ammo.to_le_bytes()),
        field(b"DATA", &weapon_data()),
        field(b"DNAM", &attack(projectile, width)),
        field(b"CRDT", &critical()),
        field(b"VATS", &vats(if width == 120 { 16 } else { 20 })),
    ]
    .concat()
}
fn ammo(projectile: u32, width: usize) -> Vec<u8> {
    let mut data = vec![255; 13];
    put(&mut data, 0, 0x7fc12345);
    put(&mut data, 8, 0x80000000);
    let mut second = vec![0; width];
    put(&mut second, 0, u32::MAX);
    put(&mut second, 4, projectile);
    put(&mut second, 8, 0x7f800000);
    if width >= 16 {
        put(&mut second, 12, 0x700);
    }
    if width == 20 {
        put(&mut second, 16, 0x80000000);
    }
    [
        field(b"DATA", &data),
        field(b"DAT2", &second),
        field(b"RCIL", &0x900u32.to_le_bytes()),
        field(b"RCIL", &0x900u32.to_le_bytes()),
    ]
    .concat()
}
fn projectile(kind: u16, width: usize) -> Vec<u8> {
    let mut d = vec![0; width];
    d[..2].copy_from_slice(&u16::MAX.to_le_bytes());
    d[2..4].copy_from_slice(&kind.to_le_bytes());
    for (at, value) in [
        (4, 0x80000000),
        (8, 0x7fc12345),
        (12, 0x7f800000),
        (16, 0x800),
        (20, 0),
        (24, u32::MAX),
        (36, 0x810),
        (40, 0x820),
        (56, 0x999),
        (60, 0x830),
        (64, 0x200),
    ] {
        put(&mut d, at, value);
    }
    if width >= 80 {
        put(&mut d, 68, 0x7fc12345);
        put(&mut d, 72, 0x80000000);
        put(&mut d, 76, 0x7f800000);
    }
    if width == 84 {
        put(&mut d, 80, u32::MAX);
    }
    field(b"DATA", &d)
}
fn fixture(data: &Path, variant: u8) {
    fs::create_dir_all(data).unwrap();
    let mut hedr = [0; 12];
    hedr[..4].copy_from_slice(&1.34f32.to_le_bytes());
    let mut base = record(b"TES4", 0, 0, 15, &field(b"HEDR", &hedr));
    for (id, body, version) in [
        (0x200, weapon(0x300, 0x400, 204), 15),
        (0x201, weapon(0x301, 0x402, 204), 15),
        (0x202, weapon(0x600, 0, 204), 15),
        (
            0x203,
            [
                weapon(0x300, 0x400, 204),
                field(b"NAM0", &0x301u32.to_le_bytes()),
                field(b"DNAM", &attack(0x401, 204)),
            ]
            .concat(),
            15,
        ),
        (0x204, weapon(0x999, 0x998, 204), 15),
        (0x205, weapon(0x308, 0x404, 204), 15),
        (0x206, weapon(0x700, 0x200, 204), 15),
        (0x207, weapon(0, 0, 204), 15),
        (0x208, weapon(0x300, 0x400, 204), 99),
        (
            0x209,
            [
                field(b"NAM0", &[0; 3]),
                field(b"DATA", &[0; 14]),
                field(b"DNAM", &[0; 119]),
                field(b"CRDT", &[0; 15]),
                field(b"VATS", &[0; 19]),
            ]
            .concat(),
            15,
        ),
        (
            0x20a,
            [weapon(0x300, 0x400, 204), field(b"DATA", &weapon_data())].concat(),
            15,
        ),
        (0x20b, weapon(0x303, 0x400, 120), 15),
        (0x20c, weapon(0x304, 0x406, 204), 15),
        (0x20d, weapon(0x300, 0x407, 204), 15),
    ] {
        base.extend(record(b"WEAP", id, 0, version, &body));
    }
    base.extend(record(b"WEAP", 0x220, plugin::DELETED, 15, &[1]));
    for (id, body, version) in [
        (0x300, ammo(0x401, 20), 15),
        (0x301, ammo(0x403, 20), 15),
        (0x303, ammo(0x400, 12), 15),
        (0x304, ammo(0x400, 16), 15),
        (0x305, ammo(0x401, 20), 99),
        (
            0x306,
            [field(b"DATA", &[0; 12]), field(b"DAT2", &[0; 11])].concat(),
            15,
        ),
        (0x307, field(b"EDID", b"empty\0"), 15),
    ] {
        base.extend(record(b"AMMO", id, 0, version, &body));
    }
    base.extend(record(b"AMMO", 0x308, plugin::DELETED, 15, &[1]));
    for (id, body, version) in [
        (0x400, projectile(4, 84), 15),
        (0x401, projectile(1, 68), 15),
        (0x402, projectile(4, 84), 99),
        (0x403, projectile(99, 80), 15),
        (0x406, field(b"DATA", &[0; 67]), 15),
        (0x407, [projectile(2, 68), projectile(2, 68)].concat(), 15),
    ] {
        base.extend(record(b"PROJ", id, 0, version, &body));
    }
    base.extend(record(b"PROJ", 0x404, plugin::DELETED, 15, &[1]));
    for (tag, id, flags, body) in [
        (b"SPEL", 0x500, 0, vec![1]),
        (b"FLST", 0x600, 0, vec![1]),
        (b"MISC", 0x700, 0, vec![1]),
        (b"LIGH", 0x800, 0, vec![1]),
        (b"EXPL", 0x810, 0, vec![1]),
        (b"SOUN", 0x820, 0, vec![1]),
        (b"SOUN", 0x830, plugin::DELETED, vec![1]),
        (b"AMEF", 0x900, 0, vec![variant]),
    ] {
        base.extend(record(tag, id, flags, 15, &body));
    }
    let npc = [
        field(b"ACBS", &[0; 24]),
        field(b"DATA", &[0; 11]),
        field(b"CNTO", &[0, 2, 0, 0, 1, 0, 0, 0]),
    ]
    .concat();
    base.extend(record(b"NPC_", 0x100, 0, 15, &npc));
    fs::write(data.join("FalloutNV.esm"), base).unwrap();
    let mut patch = record(
        b"TES4",
        0,
        0,
        15,
        &[
            field(b"HEDR", &hedr),
            field(b"MAST", b"FalloutNV.esm\0"),
            field(b"DATA", &[0; 8]),
        ]
        .concat(),
    );
    patch.extend(record(b"WEAP", 0x200, 0x80, 15, &weapon(0x301, 0x403, 204)));
    patch.extend(record(b"AMMO", 0x300, 0, 15, &ammo(0x402, 16)));
    patch.extend(record(b"PROJ", 0x401, 0, 15, &projectile(16, 80)));
    fs::write(data.join("override.esp"), patch).unwrap();
}
fn order(overrides: bool) -> Vec<String> {
    if overrides {
        vec!["FalloutNV.esm".into(), "override.esp".into()]
    } else {
        vec!["FalloutNV.esm".into()]
    }
}
fn store(data: &Path, overrides: bool) -> RecordStore {
    RecordStore::open_nv_headers(data, &order(overrides), Default::default()).unwrap()
}
fn json(store: &mut RecordStore, weapon: u32, ammo: Option<u32>) -> serde_json::Value {
    let ammo = ammo.map(key);
    serde_json::to_value(
        attack_inputs::request(store, &key(weapon), ammo.as_ref(), Default::default()).unwrap(),
    )
    .unwrap()
}
#[test]
fn same_model_weapons_keep_both_projectile_sources_and_literal_raw_words() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut s = store(temp.path(), false);
    let a = json(&mut s, 0x200, Some(0x300));
    let b = json(&mut s, 0x201, Some(0x301));
    assert_eq!(
        a["source_nodes"][0]["fields"][0]["raw_bytes"],
        b["source_nodes"][0]["fields"][0]["raw_bytes"]
    );
    assert_eq!(
        a["source_nodes"][0]["links"][0]["binding"]["key"]["local_id"],
        0x300
    );
    assert_eq!(
        b["source_nodes"][0]["links"][0]["binding"]["key"]["local_id"],
        0x301
    );
    assert_eq!(a["source_nodes"][0]["links"][1]["target_node"], 2);
    assert_eq!(a["source_nodes"][1]["links"][0]["target_node"], 3);
    assert_eq!(a["source_nodes"][2]["source"]["key"]["local_id"], 0x400);
    assert_eq!(a["source_nodes"][3]["source"]["key"]["local_id"], 0x401);
    let data = &a["source_nodes"][0]["fields"][2]["value"];
    assert_eq!(data["value"], i32::MIN);
    assert_eq!(data["health"], i32::MAX);
    assert_eq!(data["weight_bits"], 0x7fc12345u32);
    assert_eq!(data["base_damage"], i16::MIN);
    assert_eq!(data["clip_size"], 255);
    let raw = &a["source_nodes"][0]["fields"][3]["value"];
    assert_eq!(raw["flags1"], 255);
    assert_eq!(raw["reload_animation"], 255);
    assert_eq!(raw["ammo_use"], 255);
    assert_eq!(raw["projectile_count"], 255);
    assert_eq!(raw["skill"], i32::MIN);
    assert_eq!(raw["resist_type"], -1);
    assert_eq!(raw["raw_words"][1]["raw"], 0);
    assert_eq!(raw["raw_words"][15]["raw"], 0);
    assert_eq!(raw["raw_words"][14]["raw"], u32::MAX);
    let ammo = &a["source_nodes"][1]["fields"];
    assert_eq!(ammo[0]["value"]["flags"], 255);
    assert_eq!(ammo[0]["value"]["value"], i32::MIN);
    assert_eq!(ammo[1]["value"]["projectiles_per_shot"], u32::MAX);
    assert_eq!(ammo[1]["value"]["weight_bits"], 0x7f800000u32);
    assert_eq!(ammo[1]["value"]["consumed_percentage_bits"], 0x80000000u32);
    assert_eq!(
        a["explicit_ammo"]["declared_relations"][0]["matches_direct_ammo"],
        true
    );
    assert_eq!(a["ammo_choice_verified"], false);
    assert_eq!(a["projectile_priority_selected"], false);
    assert_eq!(a["firing_supported"], false);
}
#[test]
fn caller_ammo_is_independent_and_lists_and_leaf_cycles_remain_header_requests() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut s = store(temp.path(), false);
    let no_ammo = json(&mut s, 0x200, None);
    assert_eq!(no_ammo["source_nodes"].as_array().unwrap().len(), 2);
    assert!(no_ammo["explicit_ammo"].is_null());
    let different = json(&mut s, 0x200, Some(0x301));
    assert_eq!(
        different["explicit_ammo"]["declared_relations"][0]["matches_direct_ammo"],
        false
    );
    let list = json(&mut s, 0x202, Some(0x300));
    assert!(list["explicit_ammo"]["declared_relations"][0]["matches_direct_ammo"].is_null());
    assert_eq!(
        list["explicit_ammo"]["declared_relations"][0]["list_membership_verified"],
        false
    );
    assert_eq!(
        list["source_nodes"][0]["links"][0]["target"]["header"]["kind"],
        serde_json::json!([70, 76, 83, 84])
    );
    let m =
        attack_inputs::request(&mut s, &key(0x200), Some(&key(0x300)), Default::default()).unwrap();
    let projectile = &m.nodes()[2];
    assert_eq!(projectile.links.len(), 7);
    assert_eq!(projectile.links[0].role, Role::Light);
    assert!(projectile.links[0].binding_admitted);
    assert!(projectile.links[0].target_node.is_none());
    assert_eq!(
        projectile.links[1].binding.status,
        fallout_data::inventory::Status::Null
    );
    assert_eq!(
        projectile.links[4].binding.status,
        fallout_data::inventory::Status::Missing
    );
    assert_eq!(
        projectile.links[5].binding.status,
        fallout_data::inventory::Status::Deleted
    );
    assert_eq!(projectile.links[6].role, Role::DefaultWeapon);
    assert_eq!(projectile.links[6].target.as_ref().unwrap().key, key(0x200));
    assert!(projectile.links[6].target_node.is_none());
    assert_eq!(m.nodes()[1].links[2].role, Role::AmmoEffect);
    assert!(m.nodes()[1].links[2].binding_admitted && m.nodes()[1].links[3].binding_admitted);
}
#[test]
fn repeated_missing_deleted_wrong_kind_and_null_inputs_never_select_a_projectile() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut s = store(temp.path(), false);
    let repeated = json(&mut s, 0x203, None);
    assert_eq!(repeated["source_nodes"].as_array().unwrap().len(), 1);
    for link in repeated["source_nodes"][0]["links"].as_array().unwrap() {
        if link["role"] == "weapon_ammo" || link["role"] == "weapon_projectile" {
            assert_eq!(link["binding_admitted"], false);
            assert_eq!(link["repeated"], true);
            assert!(link["target_node"].is_null());
        }
    }
    for (id, status, allowed) in [
        (0x204, "missing", None),
        (0x205, "deleted", Some(true)),
        (0x206, "defined", Some(false)),
        (0x207, "null", None),
    ] {
        let j = json(&mut s, id, None);
        assert_eq!(j["source_nodes"].as_array().unwrap().len(), 1);
        let l = &j["source_nodes"][0]["links"][1];
        assert_eq!(l["binding"]["status"], status);
        assert_eq!(
            l["schema_kind_allowed"],
            serde_json::to_value(allowed).unwrap()
        );
        assert_eq!(l["binding_admitted"], false);
    }
    for (ammo, status, allowed) in [
        (0x999, "missing", None),
        (0x308, "deleted", Some(true)),
        (0x700, "defined", Some(false)),
    ] {
        let j = json(&mut s, 0x207, Some(ammo));
        assert_eq!(j["explicit_ammo"]["status"], status);
        assert_eq!(
            j["explicit_ammo"]["schema_kind_allowed"],
            serde_json::to_value(allowed).unwrap()
        );
        assert!(j["explicit_ammo"]["node_index"].is_null());
    }
    for id in [0x999, 0x220, 0x300] {
        assert!(attack_inputs::request(&mut s, &key(id), None, Default::default()).is_err());
    }
}
#[test]
fn unknown_versions_layouts_and_types_retain_physical_bytes() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut s = store(temp.path(), false);
    for id in [0x208, 0x209] {
        let m = attack_inputs::request(&mut s, &key(id), None, Default::default()).unwrap();
        assert_eq!(m.nodes().len(), 1);
        assert!(
            m.nodes()[0]
                .fields
                .iter()
                .all(|f| matches!(f.value, Value::Opaque))
        );
        assert!(m.nodes()[0].links.is_empty());
        assert!(!m.nodes()[0].issues.is_empty());
    }
    let m =
        attack_inputs::request(&mut s, &key(0x201), Some(&key(0x301)), Default::default()).unwrap();
    assert!(!m.nodes()[2].record_version_supported);
    assert!(
        m.nodes()[2]
            .fields
            .iter()
            .all(|f| matches!(f.value, Value::Opaque))
    );
    match &m.nodes()[3].fields[0].value {
        Value::ProjectileData {
            projectile_type,
            known_projectile_type,
            raw_words,
            ..
        } => {
            assert_eq!(*projectile_type, 99);
            assert!(!known_projectile_type);
            assert_eq!(raw_words[0].raw, 0x80000000);
            assert_eq!(raw_words[1].raw, 0x7fc12345);
        }
        _ => panic!("literal projectile type layout"),
    }
    let malformed = json(&mut s, 0x20c, Some(0x306));
    assert_eq!(
        malformed["source_nodes"][1]["fields"][0]["value"]["kind"],
        "opaque"
    );
    assert_eq!(
        malformed["source_nodes"][2]["fields"][0]["raw_bytes"]
            .as_array()
            .unwrap()
            .len(),
        67
    );
    let repeat = json(&mut s, 0x20d, None);
    assert_eq!(
        repeat["source_nodes"][1]["fields"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        repeat["source_nodes"][1]["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["binding_admitted"] == false)
    );
}
#[test]
fn optional_source_prefixes_and_shared_projectile_depth_remain_explicit() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut s = store(temp.path(), false);
    let j = json(&mut s, 0x20b, Some(0x303));
    assert_eq!(j["source_nodes"].as_array().unwrap().len(), 3);
    assert_eq!(j["counts"]["source_depth"], 2);
    assert!(j["source_nodes"][0]["fields"][3]["value"]["resist_type"].is_null());
    assert!(j["source_nodes"][0]["fields"][5]["value"]["silent"].is_null());
    assert!(j["source_nodes"][1]["fields"][1]["value"]["consumed_percentage_bits"].is_null());
    assert_eq!(j["source_nodes"][1]["links"].as_array().unwrap().len(), 3);
    assert!(
        attack_inputs::request(
            &mut s,
            &key(0x20b),
            Some(&key(0x303)),
            Limits {
                max_depth: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    let j = json(&mut s, 0x200, Some(0x304));
    assert_eq!(j["source_nodes"][1]["links"][1]["role"], "consumed_ammo");
    assert!(j["source_nodes"][1]["fields"][1]["value"]["consumed_percentage_bits"].is_null());
    let unknown = json(&mut s, 0x200, Some(0x305));
    assert!(
        !unknown["source_nodes"][1]["record_version_supported"]
            .as_bool()
            .unwrap()
    );
    assert!(
        unknown["source_nodes"][1]["links"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn overrides_and_changed_deferred_leaf_bodies_change_exact_source_receipts() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut base = store(temp.path(), false);
    let mut overridden = store(temp.path(), true);
    let a = json(&mut base, 0x200, Some(0x300));
    let b = json(&mut overridden, 0x200, Some(0x300));
    assert_eq!(
        a["source_nodes"][0]["source"]["source_name"],
        "FalloutNV.esm"
    );
    assert_eq!(
        b["source_nodes"][0]["source"]["source_name"],
        "override.esp"
    );
    assert_eq!(
        b["source_nodes"][0]["links"][0]["binding"]["key"]["local_id"],
        0x301
    );
    assert_eq!(
        b["source_nodes"][0]["links"][1]["binding"]["key"]["local_id"],
        0x403
    );
    assert_eq!(
        b["source_nodes"][1]["links"][0]["binding"]["key"]["local_id"],
        0x402
    );
    assert_eq!(
        b["explicit_ammo"]["declared_relations"][0]["matches_direct_ammo"],
        false
    );
    let changed = tempfile::tempdir().unwrap();
    fixture(changed.path(), 1);
    let mut fresh = store(changed.path(), false);
    let c = json(&mut fresh, 0x200, Some(0x300));
    assert_eq!(a["winning_content_sha256"], c["winning_content_sha256"]);
    assert_ne!(a["sources"], c["sources"]);
    assert_eq!(
        a["source_nodes"][1]["links"][2]["target"]["header"],
        c["source_nodes"][1]["links"][2]["target"]["header"]
    );
    assert_ne!(
        a["source_nodes"][1]["links"][2]["target"]["source_sha256"],
        c["source_nodes"][1]["links"][2]["target"]["source_sha256"]
    );
}
#[test]
fn every_source_body_header_field_word_and_projection_budget_has_an_exact_edge() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut s = store(temp.path(), false);
    let m =
        attack_inputs::request(&mut s, &key(0x200), Some(&key(0x300)), Default::default()).unwrap();
    let j = serde_json::to_value(&m).unwrap();
    let size = serde_json::to_vec(&m).unwrap().len();
    let c = &j["counts"];
    let exact = Limits {
        max_sources: 1,
        max_nodes: c["nodes"].as_u64().unwrap() as usize,
        max_depth: 2,
        max_headers: c["headers"].as_u64().unwrap() as usize,
        max_record_bytes: m
            .nodes()
            .iter()
            .map(|n| n.source.header.stored_size as usize)
            .max()
            .unwrap(),
        max_decoded_bytes: c["decoded_bytes"].as_u64().unwrap() as usize,
        max_field_visits: c["field_visits"].as_u64().unwrap() as usize,
        max_fields: c["fields"].as_u64().unwrap() as usize,
        max_bindings: c["bindings"].as_u64().unwrap() as usize,
        max_words: c["words"].as_u64().unwrap() as usize,
        max_raw_bytes: c["raw_bytes"].as_u64().unwrap() as usize,
        max_projection_bytes: size,
    };
    drop(m);
    attack_inputs::request(&mut s, &key(0x200), Some(&key(0x300)), exact).unwrap();
    for under in [
        Limits {
            max_sources: 0,
            ..exact
        },
        Limits {
            max_nodes: exact.max_nodes - 1,
            ..exact
        },
        Limits {
            max_depth: 1,
            ..exact
        },
        Limits {
            max_headers: exact.max_headers - 1,
            ..exact
        },
        Limits {
            max_record_bytes: exact.max_record_bytes - 1,
            ..exact
        },
        Limits {
            max_decoded_bytes: exact.max_decoded_bytes - 1,
            ..exact
        },
        Limits {
            max_field_visits: exact.max_field_visits - 1,
            ..exact
        },
        Limits {
            max_fields: exact.max_fields - 1,
            ..exact
        },
        Limits {
            max_bindings: exact.max_bindings - 1,
            ..exact
        },
        Limits {
            max_words: exact.max_words - 1,
            ..exact
        },
        Limits {
            max_raw_bytes: exact.max_raw_bytes - 1,
            ..exact
        },
        Limits {
            max_projection_bytes: size - 1,
            ..exact
        },
    ] {
        assert!(attack_inputs::request(&mut s, &key(0x200), Some(&key(0x300)), under).is_err());
    }
}
#[test]
fn export_authored_weapon_source_requests_for_independent_reader() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_ATTACK_EVIDENCE_DIR") else {
        return;
    };
    let destination = Path::new(&destination);
    assert!(destination.is_absolute() && destination.is_dir());
    let install = destination.join("fixture");
    fixture(&install.join("Data"), 0);
    for overrides in [false, true] {
        let cohort = if overrides { "override" } else { "base" };
        fs::write(
            install.join(format!("{cohort}-order.json")),
            serde_json::to_vec(&order(overrides)).unwrap(),
        )
        .unwrap();
        let mut s = store(&install.join("Data"), overrides);
        let cases = if overrides {
            vec![(0x200, Some(0x300)), (0x200, Some(0x301))]
        } else {
            let mut cases: Vec<_> = (0x200..=0x20d).map(|w| (w, Some(0x300))).collect();
            cases.extend([
                (0x200, None),
                (0x200, Some(0x301)),
                (0x200, Some(0x303)),
                (0x200, Some(0x304)),
                (0x200, Some(0x305)),
                (0x200, Some(0x306)),
                (0x200, Some(0x307)),
                (0x200, Some(0x308)),
                (0x200, Some(0x700)),
                (0x200, Some(0x999)),
                (0x201, Some(0x301)),
                (0x203, None),
                (0x20b, Some(0x303)),
            ]);
            cases
        };
        for (weapon, ammo) in cases {
            let k = ammo.map(key);
            let m = attack_inputs::request(&mut s, &key(weapon), k.as_ref(), Default::default())
                .unwrap();
            fs::write(
                install.join(format!(
                    "{cohort}-{weapon:x}-{}-host.json",
                    ammo.map_or_else(|| "none".into(), |a| format!("{a:x}"))
                )),
                serde_json::to_vec_pretty(&m).unwrap(),
            )
            .unwrap();
        }
    }
}
