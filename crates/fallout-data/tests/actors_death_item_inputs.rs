use fallout_data::{
    actors::{
        self, associations,
        death_item_inputs::{self, Limits},
    },
    identity::{FormKey, ProfileId},
    inventory, leveled, plugin,
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
    let mut raw = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for m in masters {
        raw.extend(field(b"MAST", &[m.as_bytes(), &[0]].concat()));
        raw.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &raw)
}
fn config(mask: u16) -> Vec<u8> {
    let mut raw = [0; 24];
    raw[22..24].copy_from_slice(&mask.to_le_bytes());
    field(b"ACBS", &raw)
}
fn link(tag: &[u8; 4], id: u32) -> Vec<u8> {
    field(tag, &id.to_le_bytes())
}
fn actor(creature: bool, mask: u16, extra: Vec<u8>) -> Vec<u8> {
    [
        config(mask),
        field(b"DATA", &vec![0; if creature { 17 } else { 11 }]),
        extra,
    ]
    .concat()
}
fn meta(chance: u8, flags: u8) -> Vec<u8> {
    [field(b"LVLD", &[chance]), field(b"LVLF", &[flags])].concat()
}
fn entry(level: u16, id: u32, count: Option<u16>, padding: bool) -> Vec<u8> {
    let mut raw = [
        level.to_le_bytes().as_slice(),
        &0xbbaau16.to_le_bytes(),
        &id.to_le_bytes(),
    ]
    .concat();
    if let Some(count) = count {
        raw.extend(count.to_le_bytes());
        if padding {
            raw.extend(0xddccu16.to_le_bytes());
        }
    }
    field(b"LVLO", &raw)
}
fn extra(owner: u32, word: u32, bits: u32) -> Vec<u8> {
    field(
        b"COED",
        &[owner.to_le_bytes(), word.to_le_bytes(), bits.to_le_bytes()].concat(),
    )
}
fn list() -> Vec<u8> {
    [
        field(b"EDID", b"literal_death_list\0"),
        meta(255, 255),
        link(b"LVLG", 0x800),
        entry(0x8000, 0x700, Some(u16::MAX), true),
        extra(0x900, 0x80000000, 0x7fc12345),
        extra(0, u32::MAX, 0x80000000),
        entry(u16::MAX, 0x700, Some(0), false),
        entry(1, 0x301, None, false),
        entry(2, 0x301, Some(1), true),
        entry(0, 0, None, false),
        entry(0, 0x999, Some(1), true),
        entry(0, 0x701, Some(1), true),
        entry(0, 0x101, Some(1), true),
        entry(0, 0x400, Some(1), true),
        entry(0, 0x702, Some(1), true),
        field(b"ZZZZ", &[170, 187]),
        extra(0x999, 123, 0x7f800000),
    ]
    .concat()
}
fn fixture(path: &Path, variant: u8) {
    fs::create_dir_all(path).unwrap();
    let mut bytes = header(&[]);
    for (id, creature, mask, extra) in [
        (0x100, false, 0, link(b"INAM", 0x300)),
        (0x101, true, 0, link(b"INAM", 0x300)),
        (0x102, false, 1, link(b"INAM", 0x300)),
        (0x103, false, 8, link(b"INAM", 0x300)),
        (0x104, false, 256, link(b"INAM", 0x300)),
        (
            0x105,
            false,
            0,
            [link(b"INAM", 0x300), link(b"INAM", 0x301)].concat(),
        ),
        (0x106, false, 0, link(b"INAM", 0)),
        (0x107, false, 0, link(b"INAM", 0x999)),
        (0x108, false, 0, link(b"INAM", 0x305)),
        (0x109, false, 0, link(b"INAM", 0x700)),
        (0x10a, false, 0, link(b"INAM", 0x304)),
        (0x10b, false, 0, link(b"INAM", 0x306)),
        (0x10e, false, 0, link(b"INAM", 0x307)),
        (0x10f, false, 0, link(b"INAM", 0x308)),
    ] {
        bytes.extend(disk(
            if creature { b"CREA" } else { b"NPC_" },
            id,
            0,
            15,
            &actor(creature, mask, extra),
        ));
    }
    bytes.extend(disk(
        b"NPC_",
        0x10c,
        0,
        15,
        &[field(b"DATA", &[0; 11]), link(b"INAM", 0x300)].concat(),
    ));
    bytes.extend(disk(
        b"NPC_",
        0x10d,
        0,
        15,
        &[config(0), actor(false, 1, link(b"INAM", 0x300))].concat(),
    ));
    bytes.extend(disk(b"NPC_", 0x120, plugin::DELETED, 15, &[]));
    for (id, version, body) in [
        (0x300, 15, list()),
        (
            0x301,
            15,
            [
                meta(0, 0),
                entry(0x8000, 0x703, Some(0x8000), false),
                extra(0x102, 0x800, 0xff800000),
            ]
            .concat(),
        ),
        (
            0x304,
            99,
            [meta(255, 255), entry(1, 0x307, Some(u16::MAX), true)].concat(),
        ),
        (
            0x306,
            15,
            [
                meta(0, 0),
                meta(255, 255),
                link(b"LVLG", 0x800),
                link(b"LVLG", 0x800),
                entry(0, 0x700, Some(1), true),
            ]
            .concat(),
        ),
        (
            0x307,
            15,
            [meta(0, 0), entry(0, 0x307, Some(1), true)].concat(),
        ),
        (
            0x308,
            15,
            [
                meta(0, 0),
                entry(0, 0x301, Some(1), true),
                entry(0, 0x310, Some(1), true),
            ]
            .concat(),
        ),
        (
            0x309,
            15,
            [meta(0, 0), entry(0, 0x309, Some(1), true)].concat(),
        ),
        (
            0x310,
            15,
            [meta(0, 0), entry(0, 0x301, Some(1), true)].concat(),
        ),
    ] {
        bytes.extend(disk(b"LVLI", id, 0, version, &body));
    }
    bytes.extend(disk(b"LVLI", 0x305, plugin::DELETED, 15, &[]));
    bytes.extend(disk(b"LVLC", 0x400, 0, 15, &meta(0, 0)));
    for (tag, id, flags) in [
        (b"WEAP", 0x700, 0),
        (b"WEAP", 0x701, plugin::DELETED),
        (b"WEAP", 0x702, 0),
        (b"ARMO", 0x703, 0),
        (b"GLOB", 0x800, 0),
        (b"FACT", 0x900, 0),
    ] {
        // Deliberately not a valid subrecord body; leaf requests must not read it.
        bytes.extend(disk(tag, id, flags, 15, &[variant]));
    }
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
    let mut overrides = header(&["FalloutNV.esm"]);
    overrides.extend(disk(
        b"LVLI",
        0x300,
        0x80,
        15,
        &[
            meta(17, 4),
            entry(65535, 0x301, Some(65535), true),
            entry(0, 0x701, Some(0), true),
        ]
        .concat(),
    ));
    overrides.extend(disk(
        b"LVLI",
        0x301,
        0,
        15,
        &[meta(4, 3), entry(65535, 0x703, None, false)].concat(),
    ));
    overrides.extend(disk(b"WEAP", 0x701, 0, 15, &[variant]));
    fs::write(path.join("Override.esp"), overrides).unwrap();
}
fn store(path: &Path, overridden: bool) -> RecordStore {
    let names = if overridden {
        vec!["FalloutNV.esm".into(), "Override.esp".into()]
    } else {
        vec!["FalloutNV.esm".into()]
    };
    RecordStore::open_nv_headers(path, &names, plugin::Limits::default()).unwrap()
}
fn project(
    path: &Path,
    id: u32,
    index: usize,
    overridden: bool,
    limits: Limits,
) -> fallout_data::Result<serde_json::Value> {
    let mut s = store(path, overridden);
    let inv = inventory::Catalogue::load(&mut s, Default::default())?;
    let actors = actors::Catalogue::load(&inv, Default::default())?;
    let assoc = associations::Catalogue::load(&mut s, &actors, Default::default())?;
    let lists = leveled::Catalogue::load(&mut s, Default::default())?;
    Ok(serde_json::to_value(death_item_inputs::request(
        &mut s,
        &actors,
        &assoc,
        &lists,
        &key(id),
        index,
        limits,
    )?)
    .unwrap())
}
#[test]
fn exact_order_duplicates_unsigned_optional_words_and_header_only_leaf_requests() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(tmp.path(), 0x100, 2, false, Limits::default()).unwrap();
    assert_eq!(m["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(m["root_node"], 0);
    assert_eq!(m["nodes"][0]["entries"].as_array().unwrap().len(), 10);
    let n = &m["nodes"][0];
    assert_eq!(n["fields"][4]["value"]["level_bits"], 32768);
    assert_eq!(n["fields"][4]["value"]["count_bits"], 65535);
    assert_eq!(n["fields"][4]["value"]["level_padding"], 0xbbaa);
    assert_eq!(n["fields"][4]["value"]["count_padding"], 0xddcc);
    assert_eq!(n["fields"][5]["value"]["union_word"]["value"], i32::MIN);
    assert_eq!(n["fields"][5]["value"]["condition_bits"], 0x7fc12345_u32);
    assert_eq!(n["fields"][7]["value"]["count_bits"], 0);
    assert!(n["fields"][8]["value"]["count_bits"].is_null());
    assert_eq!(n["fields"][2]["value"]["raw"], 255);
    assert_eq!(n["fields"][1]["value"]["raw"], 255);
    assert_eq!(n["entries"][0]["coed_fields"], serde_json::json!([5, 6]));
    assert_eq!(n["entries"][0]["unique_extra_available"], false);
    let links = n["links"].as_array().unwrap();
    assert_eq!(links[5]["nested_node"], 1);
    assert_eq!(links[6]["nested_node"], 1);
    assert_eq!(links[1]["binding"]["key"], links[4]["binding"]["key"]);
    assert!(links.iter().any(|l| l["binding"]["status"] == "null"));
    assert!(links.iter().any(|l| l["binding"]["status"] == "missing"));
    assert!(
        links
            .iter()
            .any(|l| l["binding"]["status"] == "deleted" && l["source"].is_object())
    );
    assert!(
        links
            .iter()
            .any(|l| l["schema_kind_allowed"] == false && l["source"].is_object())
    );
    assert_eq!(m["counts"]["source_depth"], 3);
    assert_eq!(m["declaration_binding_available"], true);
    assert_eq!(m["items_created"], false);
    assert_eq!(m["roll_supported"], false);
    assert_eq!(m["death_event_verified"], false);
}
#[test]
fn traits_gate_is_separate_from_actor_effect_and_inventory_bits_and_repeats() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    for id in [0x100, 0x101, 0x103, 0x104] {
        let m = project(tmp.path(), id, 2, false, Limits::default()).unwrap();
        assert_eq!(m["declaration_binding_available"], true);
    }
    for id in [0x102, 0x105, 0x10d] {
        let m = project(
            tmp.path(),
            id,
            if id == 0x10d { 3 } else { 2 },
            false,
            Limits::default(),
        )
        .unwrap();
        assert_eq!(m["declaration_binding_available"], false);
        assert_eq!(m["nodes"].as_array().unwrap().len(), 2);
    }
    let absent = project(tmp.path(), 0x10c, 1, false, Limits::default()).unwrap();
    assert!(absent["traits_template_flag"].is_null());
    assert_eq!(absent["declaration_binding_available"], false);
    let repeated = project(tmp.path(), 0x105, 3, false, Limits::default()).unwrap();
    assert_eq!(repeated["singleton_repeated"], true);
    assert_eq!(repeated["nodes"][0]["source"]["key"]["local_id"], 0x301);
}
#[test]
fn unavailable_root_links_versions_and_metadata_are_explicit() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    for id in [0x106, 0x107, 0x108, 0x109] {
        let m = project(tmp.path(), id, 2, false, Limits::default()).unwrap();
        assert!(m["root_node"].is_null());
        assert!(m["nodes"].as_array().unwrap().is_empty());
        assert_eq!(m["declaration_binding_available"], false);
    }
    let m = project(tmp.path(), 0x10a, 2, false, Limits::default()).unwrap();
    assert_eq!(m["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(m["nodes"][0]["record_version_supported"], false);
    assert!(
        m["nodes"][0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["value"].is_null())
    );
    assert!(m["nodes"][0]["links"].as_array().unwrap().is_empty());
    let m = project(tmp.path(), 0x10b, 2, false, Limits::default()).unwrap();
    assert_eq!(m["nodes"][0]["metadata_singletons_unambiguous"], false);
    assert_eq!(
        m["nodes"][0]["chance_field_indices"],
        serde_json::json!([0, 2])
    );
    assert_eq!(
        m["nodes"][0]["flag_field_indices"],
        serde_json::json!([1, 3])
    );
    assert_eq!(
        m["nodes"][0]["global_field_indices"],
        serde_json::json!([4, 5])
    );
    for (id, index) in [
        (0x100, 1),
        (0x100, usize::MAX),
        (0x120, 0),
        (0x999, 0),
        (0x700, 0),
    ] {
        assert!(project(tmp.path(), id, index, false, Limits::default()).is_err());
    }
}
#[test]
fn selected_cycles_and_longest_shared_paths_refuse_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    assert!(
        project(tmp.path(), 0x10e, 2, false, Limits::default())
            .unwrap_err()
            .to_string()
            .contains("selected list cycle")
    );
    let m = project(tmp.path(), 0x10f, 2, false, Limits::default()).unwrap();
    assert_eq!(m["counts"]["source_depth"], 4);
    assert_eq!(m["nodes"].as_array().unwrap().len(), 3);
    let limits = Limits {
        max_depth: 3,
        ..Limits::default()
    };
    assert!(
        project(tmp.path(), 0x10f, 2, false, limits)
            .unwrap_err()
            .to_string()
            .contains("depth budget")
    );
    assert!(project(tmp.path(), 0x100, 2, false, Limits::default()).is_ok()); // Unselected self-cycle309 does not gate root300.
}
#[test]
fn all_exact_budget_edges_and_zeroes_refuse_without_a_partial_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let m = project(tmp.path(), 0x100, 2, false, Limits::default()).unwrap();
    let c = &m["counts"];
    let limits = Limits {
        max_sources: 1,
        max_depth: c["source_depth"].as_u64().unwrap() as usize,
        max_lists: c["lists"].as_u64().unwrap() as usize,
        max_record_bytes: 256,
        max_decoded_bytes: c["decoded_bytes"].as_u64().unwrap() as usize,
        max_field_visits: c["field_visits"].as_u64().unwrap() as usize,
        max_fields: c["fields"].as_u64().unwrap() as usize,
        max_entries: c["entries"].as_u64().unwrap() as usize,
        max_bindings: c["bindings"].as_u64().unwrap() as usize,
        max_headers: c["headers"].as_u64().unwrap() as usize,
        max_raw_bytes: c["raw_bytes"].as_u64().unwrap() as usize,
        max_projection_bytes: serde_json::to_vec(&m).unwrap().len(),
    };
    let largest = m["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            n["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| 6 + f["raw_bytes"].as_array().unwrap().len())
                .sum::<usize>()
        })
        .max()
        .unwrap();
    let limits = Limits {
        max_record_bytes: largest,
        ..limits
    };
    assert!(project(tmp.path(), 0x100, 2, false, limits).is_ok());
    for which in 0..12 {
        let mut low = limits;
        match which {
            0 => low.max_sources -= 1,
            1 => low.max_depth -= 1,
            2 => low.max_lists -= 1,
            3 => low.max_record_bytes -= 1,
            4 => low.max_decoded_bytes -= 1,
            5 => low.max_field_visits -= 1,
            6 => low.max_fields -= 1,
            7 => low.max_entries -= 1,
            8 => low.max_bindings -= 1,
            9 => low.max_headers -= 1,
            10 => low.max_raw_bytes -= 1,
            _ => low.max_projection_bytes -= 1,
        };
        assert!(
            project(tmp.path(), 0x100, 2, false, low).is_err(),
            "limit{which}"
        );
    }
}
#[test]
fn overrides_and_changed_unread_body_receipts_cannot_lend_stale_source_authority() {
    let tmp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    fixture(other.path(), 1);
    let m = project(tmp.path(), 0x100, 2, true, Limits::default()).unwrap();
    assert_eq!(m["nodes"][0]["source"]["source_name"], "Override.esp");
    assert_eq!(m["nodes"][0]["source"]["header"]["flags"], 128);
    assert_eq!(m["nodes"][0]["fields"][2]["value"]["count_bits"], 65535);
    let mut old = store(tmp.path(), false);
    let inv = inventory::Catalogue::load(&mut old, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inv, Default::default()).unwrap();
    let assoc = associations::Catalogue::load(&mut old, &actors, Default::default()).unwrap();
    let mut lists = leveled::Catalogue::load(&mut old, Default::default()).unwrap();
    let mut new = store(other.path(), false);
    assert!(
        death_item_inputs::request(
            &mut new,
            &actors,
            &assoc,
            &lists,
            &key(0x100),
            2,
            Limits::default()
        )
        .unwrap_err()
        .to_string()
        .contains("source cohorts differ")
    );
    lists.sources[0].source_sha256 = "00".repeat(32);
    assert!(
        death_item_inputs::request(
            &mut old,
            &actors,
            &assoc,
            &lists,
            &key(0x100),
            2,
            Limits::default()
        )
        .is_err()
    );
}
#[test]
fn malformed_known_list_layout_refuses_through_existing_producer() {
    let tmp = tempfile::tempdir().unwrap();
    fixture(tmp.path(), 0);
    let p = tmp.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&p).unwrap();
    bytes.extend(disk(b"LVLI", 0x320, 0, 15, &field(b"LVLO", &[0; 9])));
    fs::write(p, bytes).unwrap();
    let mut s = store(tmp.path(), false);
    assert!(
        leveled::Catalogue::load(&mut s, Default::default())
            .err()
            .unwrap()
            .to_string()
            .contains("LVLO needs")
    );
}
#[test]
fn export_literal_authored_source_requests_when_requested() {
    let Ok(path) = std::env::var("FALLOUT_ACTOR_DEATH_EVIDENCE_DIR") else {
        return;
    };
    let path = Path::new(&path).join("fixture");
    let data = path.join("Data");
    fixture(&data, 0);
    fs::write(path.join("base-order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        path.join("override-order.json"),
        b"[\"FalloutNV.esm\",\"Override.esp\"]",
    )
    .unwrap();
    for (cohort, overridden) in [("base", false), ("override", true)] {
        for (id, index) in if overridden {
            vec![(0x100, 2), (0x105, 3)]
        } else {
            vec![
                (0x100, 2),
                (0x101, 2),
                (0x102, 2),
                (0x103, 2),
                (0x104, 2),
                (0x105, 2),
                (0x105, 3),
                (0x106, 2),
                (0x107, 2),
                (0x108, 2),
                (0x109, 2),
                (0x10a, 2),
                (0x10b, 2),
                (0x10c, 1),
                (0x10d, 3),
                (0x10f, 2),
            ]
        } {
            let manifest = project(&data, id, index, overridden, Limits::default()).unwrap();
            fs::write(
                path.join(format!("{cohort}-{id:x}-{index}-host.json")),
                serde_json::to_vec_pretty(
                    &serde_json::json!({"actor_death_item_inputs":{"manifest":manifest}}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
}
