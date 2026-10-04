use fallout_data::{
    actors::{
        self,
        body_part_inputs::{self, Limits},
    },
    identity::{FormKey, ProfileId},
    inventory,
    store::RecordStore,
};
use serde_json::Value;
use std::{fs, path::Path};

fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn field(tag: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    let mut b = tag.to_vec();
    b.extend((raw.len() as u16).to_le_bytes());
    b.extend(raw);
    b
}
fn record(tag: &[u8; 4], id: u32, flags: u32, version: u16, body: Vec<u8>) -> Vec<u8> {
    let mut b = tag.to_vec();
    b.extend((body.len() as u32).to_le_bytes());
    b.extend(flags.to_le_bytes());
    b.extend(id.to_le_bytes());
    b.extend([0; 4]);
    b.extend(version.to_le_bytes());
    b.extend([0; 2]);
    b.extend(body);
    b
}
fn plugin(master: bool, records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut h = 1.34f32.to_le_bytes().to_vec();
    h.extend([0; 8]);
    let mut b = field(b"HEDR", &h);
    if master {
        b.extend(field(b"MAST", b"FalloutNV.esm\0"));
        b.extend(field(b"DATA", &[0; 8]));
    }
    let mut out = record(b"TES4", 0, 0, 15, b);
    for r in records {
        out.extend(r);
    }
    out
}
fn config(mask: u16) -> Vec<u8> {
    let mut b = vec![0; 24];
    b[22..24].copy_from_slice(&mask.to_le_bytes());
    b
}
fn creature(id: u32, masks: &[u16], links: &[Vec<u8>], version: u16) -> Vec<u8> {
    let mut b = Vec::new();
    for m in masks {
        b.extend(field(b"ACBS", &config(*m)));
    }
    b.extend(field(b"DATA", &[0; 17]));
    for l in links {
        b.extend(field(b"PNAM", l));
    }
    record(b"CREA", id, 0, version, b)
}
fn pnam(id: u32) -> Vec<u8> {
    id.to_le_bytes().to_vec()
}
fn node(part: i8, actor_value: i8, flags: u8) -> Vec<u8> {
    let mut b = vec![0; 84];
    for (at, word) in [
        (0, 0x7fc00001u32),
        (20, 0x7fa00001),
        (24, 0x80000000),
        (28, 0x80000000),
        (40, 0x7f800000),
        (44, 0xff800000),
        (48, 0x7fc0ffff),
        (52, 0xfffffff0),
        (56, 0x3f800000),
        (60, 0),
        (64, 0x80000000),
        (80, 0x7fffffff),
    ] {
        b[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    b[4] = flags;
    b[5] = part as u8;
    b[6] = 255;
    b[7] = actor_value as u8;
    b[8] = 255;
    b[9] = 200;
    b[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
    for (at, word) in [
        (12, 0x400u32),
        (16, 0x401),
        (32, 0x404),
        (36, 0x999),
        (68, 0x402),
        (72, 0x500),
    ] {
        b[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    b[76] = 255;
    b[77] = 254;
    b[78] = 0xaa;
    b[79] = 0xbb;
    b
}
fn parts() -> Vec<u8> {
    let mut b = Vec::new();
    for (tag, raw) in [
        (b"MODL", b"unselected.nif\0".to_vec()),
        (b"BPND", node(5, 76, 255)),
        (b"BPTN", vec![255, 0, 254, 0]),
        (b"BPNN", b"repeated\0".to_vec()),
        (b"BPNT", b"target\0".to_vec()),
        (b"BPNI", b"ik\0".to_vec()),
        (b"NAM1", b"replacement.nif\0".to_vec()),
        (b"NAM4", b"bone\0".to_vec()),
        (b"NAM5", vec![255, 0]),
        (b"BPND", node(5, 76, 127)),
        (b"BPTN", vec![255, 0, 254, 0]),
        (b"BPNN", b"repeated\0".to_vec()),
        (b"BPND", node(-128, -128, 128)),
        (b"BPND", node(-1, -1, 0)),
        (b"RAGA", pnam(0x403)),
        (b"RAGA", pnam(0x404)),
        (b"ZZZZ", vec![255]),
    ] {
        b.extend(field(tag, &raw));
    }
    b
}
fn fixture(path: &Path, variant: u8) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let mut records = vec![
        creature(0x100, &[0], &[pnam(0x200)], 15),
        creature(0x101, &[64], &[pnam(0x200)], 15),
        creature(0x102, &[1], &[pnam(0x200)], 15),
        creature(0x103, &[0], &[pnam(0x200), pnam(0x201)], 15),
        creature(0x104, &[0], &[pnam(0)], 15),
        creature(0x105, &[0], &[pnam(0x999)], 15),
        creature(0x106, &[0], &[pnam(0x202)], 15),
        creature(0x107, &[0], &[pnam(0x500)], 15),
        creature(0x108, &[0], &[], 15),
        creature(0x109, &[0, 0], &[pnam(0x200)], 15),
        creature(0x110, &[], &[pnam(0x200)], 15),
        creature(0x111, &[0], &[vec![1, 2, 3]], 15),
        creature(0x112, &[0], &[pnam(0x200)], 14),
        creature(0x113, &[0], &[pnam(0x203)], 15),
        creature(0x114, &[0], &[pnam(0x204)], 15),
        creature(0x115, &[0], &[pnam(0x205)], 15),
        record(b"CREA", 0x120, 0x20, 15, vec![]),
        record(b"BPTD", 0x200, 0, 15, parts()),
        record(b"BPTD", 0x201, 0, 15, field(b"BPND", &node(14, -1, 0))),
        record(b"BPTD", 0x202, 0x20, 15, vec![]),
        record(b"BPTD", 0x203, 0, 14, parts()),
        record(b"BPTD", 0x204, 0, 15, field(b"BPND", &[0; 83])),
        record(b"BPTD", 0x205, 0, 15, vec![1]),
        record(b"DEBR", 0x400, 0, 15, vec![variant]),
        record(b"EXPL", 0x401, 0, 15, vec![255]),
        record(b"IPDS", 0x402, 0, 15, vec![255]),
        record(b"RGDL", 0x403, 0, 15, vec![255]),
        record(b"DEBR", 0x404, 0x20, 15, vec![]),
        record(b"MISC", 0x500, 0, 15, vec![255]),
    ];
    let mut npc = field(b"ACBS", &config(0));
    npc.extend(field(b"DATA", &[0; 11]));
    records.push(record(b"NPC_", 0x300, 0, 15, npc));
    fs::write(path.join("Data/FalloutNV.esm"), plugin(false, records)).unwrap();
    let mut overridden = field(b"ACBS", &config(0));
    overridden.extend(field(b"DATA", &[0; 17]));
    overridden.extend(field(b"PNAM", &pnam(0x201)));
    fs::write(
        path.join("Data/Override.esp"),
        plugin(
            true,
            vec![
                record(b"CREA", 0x100, 0x80, 15, overridden),
                record(b"BPTD", 0x201, 0x80, 15, field(b"BPND", &node(14, -1, 0))),
                record(b"BPTD", 0x200, 0x80, 15, field(b"BPND", &node(99, 99, 255))),
                record(b"DEBR", 0x400, 0x80, 15, vec![255]),
            ],
        ),
    )
    .unwrap();
}
fn store(path: &Path, over: bool) -> RecordStore {
    let mut names = vec!["FalloutNV.esm".into()];
    if over {
        names.push("Override.esp".into());
    }
    RecordStore::open_nv_headers(&path.join("Data"), &names, Default::default()).unwrap()
}
fn project(path: &Path, root: u32, over: bool, limits: Limits) -> fallout_data::Result<Value> {
    let mut s = store(path, over);
    let inv = inventory::Catalogue::load(&mut s, Default::default())?;
    let a = actors::Catalogue::load(&inv, Default::default())?;
    let m = body_part_inputs::request(&mut s, &a, &key(root), limits)?;
    Ok(serde_json::to_value(m).unwrap())
}
#[test]
fn raw_names_signed_parts_hit_words_unused_vectors_and_all_header_links_stay_physical() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(tmp.path(), 0x100, false, Default::default()).unwrap();
    let d = &m["declaration"];
    let p = &d["parts"][0]["data"];
    assert_eq!(m["declaration_binding_available"], true);
    assert_eq!(
        d["name_field_indices"],
        serde_json::json!([0, 2, 3, 4, 5, 6, 7, 10, 11])
    );
    assert_eq!(d["parts"].as_array().unwrap().len(), 4);
    assert_eq!(d["parts"][0]["field_index"], 1);
    assert_eq!(d["parts"][0]["duplicate_part_type"], true);
    assert_eq!(d["parts"][1]["duplicate_part_type"], true);
    assert_eq!(
        d["fields"][2]["raw_bytes"],
        serde_json::json!([255, 0, 254, 0])
    );
    assert_eq!(d["fields"][2]["raw_bytes"], d["fields"][10]["raw_bytes"]);
    assert_eq!(p["damage_mult_bits"], 0x7fc00001u32);
    assert_eq!(p["flags"], 255);
    assert_eq!(p["flags_known"], false);
    assert_eq!(p["part_type"], 5);
    assert_eq!(p["actor_value"], 76);
    assert_eq!(p["to_hit_chance"], 255);
    assert_eq!(p["explosion_chance"], 200);
    assert_eq!(p["explodable_debris_count"], 65535);
    assert_eq!(p["severable_debris_count"], i32::MIN);
    assert_eq!(
        p["gore_position_bits"],
        serde_json::json!([0xff800000u32, 0x7fc0ffffu32, 0xfffffff0u32])
    );
    assert_eq!(
        p["gore_rotation_bits"],
        serde_json::json!([0x3f800000u32, 0, 0x80000000u32])
    );
    assert_eq!(p["unused"], serde_json::json!([170, 187]));
    assert_eq!(p["limb_replacement_scale_bits"], 0x7fffffffu32);
    assert_eq!(d["links"].as_array().unwrap().len(), 26);
    for (i, offset) in [12, 16, 32, 36, 68, 72].into_iter().enumerate() {
        assert_eq!(d["links"][i]["field_byte_offset"], offset);
        assert_eq!(d["links"][i]["field_index"], 1);
    }
    assert_eq!(d["links"][2]["binding"]["status"], "deleted");
    assert_eq!(d["links"][3]["binding"]["status"], "missing");
    assert_eq!(d["links"][5]["schema_kind_allowed"], false);
    assert_eq!(m["counts"]["field_visits"], 58);
    assert_eq!(m["counts"]["fields"], 20);
    assert_eq!(m["counts"]["bindings"], 27);
    assert_eq!(m["counts"]["headers"], 25);
    for n in [
        "contact_mapped",
        "damage_evaluated",
        "dismemberment_supported",
    ] {
        assert_eq!(m[n], false);
    }
    assert_eq!(d["source_grouping_verified"], false);
}
#[test]
fn only_unique_live_pnam_reads_one_body_and_model_animation_gate_is_distinct() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    assert_eq!(
        project(tmp.path(), 0x101, false, Default::default()).unwrap()["declaration_binding_available"],
        false
    );
    assert_eq!(
        project(tmp.path(), 0x102, false, Default::default()).unwrap()["declaration_binding_available"],
        true
    );
    for id in [0x103, 0x104, 0x105, 0x106, 0x107, 0x108, 0x111, 0x112] {
        let m = project(tmp.path(), id, false, Default::default()).unwrap();
        assert!(m["declaration"].is_null());
        assert_eq!(m["declaration_binding_available"], false);
    }
    let repeated = project(tmp.path(), 0x103, false, Default::default()).unwrap();
    assert_eq!(repeated["pnam_links"].as_array().unwrap().len(), 2);
    assert_eq!(repeated["pnam_repeated"], true);
    for id in [0x109, 0x110] {
        let m = project(tmp.path(), id, false, Default::default()).unwrap();
        assert_eq!(m["model_animation_template_flag"], Value::Null);
        assert_eq!(m["declaration_binding_available"], false);
        assert!(m["declaration"].is_object());
    }
}
#[test]
fn unknown_version_layout_flags_and_signed_enums_keep_source_without_limb_choice() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(tmp.path(), 0x100, false, Default::default()).unwrap();
    assert_eq!(m["declaration"]["parts"][2]["data"]["part_type"], -128);
    assert_eq!(m["declaration"]["parts"][2]["data"]["actor_value"], -128);
    assert_eq!(
        m["declaration"]["parts"][2]["data"]["part_type_known"],
        false
    );
    assert_eq!(
        m["declaration"]["parts"][2]["data"]["actor_value_known"],
        false
    );
    assert_eq!(
        m["declaration"]["parts"][3]["data"]["part_type_known"],
        true
    );
    for id in [0x113, 0x114] {
        let m = project(tmp.path(), id, false, Default::default()).unwrap();
        let d = &m["declaration"];
        assert!(d["parts"][0]["data"].is_null());
        assert_eq!(d["parts"][0]["layout_supported"], false);
        assert_eq!(d["links"].as_array().unwrap().len(), 0);
        assert!(!d["fields"].as_array().unwrap().is_empty());
    }
}
#[test]
fn winning_overrides_and_changed_unread_leaf_source_hash_are_not_interchangeable() {
    let tmp = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    fixture(changed.path(), 1);
    let mut s = store(tmp.path(), false);
    let inv = inventory::Catalogue::load(&mut s, Default::default()).unwrap();
    let a = actors::Catalogue::load(&inv, Default::default()).unwrap();
    let mut other = store(changed.path(), false);
    assert!(body_part_inputs::request(&mut other, &a, &key(0x100), Default::default()).is_err());
    let m = project(tmp.path(), 0x100, true, Default::default()).unwrap();
    assert_eq!(m["actor"]["source"]["plugin"], "Override.esp");
    assert_eq!(m["declaration"]["source"]["source_name"], "Override.esp");
    assert_eq!(m["declaration"]["source"]["header"]["flags"], 128);
    assert_eq!(
        m["declaration"]["links"][0]["source"]["source_name"],
        "Override.esp"
    );
    assert_eq!(m["declaration"]["parts"][0]["data"]["part_type"], 14);
}
#[test]
fn all_twelve_exact_work_and_byte_bounds_accept_then_refuse_one_less() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(tmp.path(), 0x100, false, Default::default()).unwrap();
    let c = &m["counts"];
    let size = |name: &str| c[name].as_u64().unwrap() as usize;
    let exact = Limits {
        max_sources: 1,
        max_record_bytes: parts().len(),
        max_decoded_bytes: size("decoded_bytes"),
        max_field_visits: size("field_visits"),
        max_fields: size("fields"),
        max_parts: size("parts"),
        max_names: size("names"),
        max_name_bytes: size("name_bytes"),
        max_raw_bytes: size("raw_bytes"),
        max_bindings: size("bindings"),
        max_headers: size("headers"),
        max_projection_bytes: serde_json::to_vec(&m).unwrap().len(),
    };
    assert!(project(tmp.path(), 0x100, false, exact).is_ok());
    for selected in 0..12 {
        let mut l = exact;
        let n = match selected {
            0 => &mut l.max_sources,
            1 => &mut l.max_record_bytes,
            2 => &mut l.max_decoded_bytes,
            3 => &mut l.max_field_visits,
            4 => &mut l.max_fields,
            5 => &mut l.max_parts,
            6 => &mut l.max_names,
            7 => &mut l.max_name_bytes,
            8 => &mut l.max_raw_bytes,
            9 => &mut l.max_bindings,
            10 => &mut l.max_headers,
            _ => &mut l.max_projection_bytes,
        };
        *n -= 1;
        assert!(
            project(tmp.path(), 0x100, false, l).is_err(),
            "bound {selected}"
        );
    }
    let mut none = exact;
    none.max_names = 0;
    none.max_name_bytes = 0;
    assert!(project(tmp.path(), 0x100, true, none).is_err()); // exact one-source cohort cap stays active
    let unrestricted = Limits {
        max_names: 0,
        max_name_bytes: 0,
        ..Default::default()
    };
    assert!(project(tmp.path(), 0x100, true, unrestricted).is_ok());
}
#[test]
fn deleted_noncreature_missing_and_selected_malformed_body_refuse_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    for id in [0x120, 0x300, 0x999, 0x115] {
        assert!(project(tmp.path(), id, false, Default::default()).is_err());
    }
    assert!(project(tmp.path(), 0x100, false, Default::default()).is_ok());
}
#[test]
fn export_physical_body_part_requests_for_independent_complete_comparison() {
    let Ok(path) = std::env::var("FALLOUT_ACTOR_BODY_PART_EVIDENCE_DIR") else {
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
    for (cohort, over, ids) in [
        (
            "base",
            false,
            vec![
                0x100, 0x101, 0x102, 0x103, 0x104, 0x105, 0x106, 0x107, 0x108, 0x109, 0x110, 0x111,
                0x112, 0x113, 0x114,
            ],
        ),
        ("override", true, vec![0x100, 0x101]),
    ] {
        for id in ids {
            let m = project(&path, id, over, Default::default()).unwrap();
            fs::write(
                path.join(format!("{cohort}-{id:x}-host.json")),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"actor_body_part_inputs":{"manifest":m}}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
}
