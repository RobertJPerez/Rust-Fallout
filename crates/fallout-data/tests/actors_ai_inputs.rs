use fallout_data::{
    actors::{
        self,
        ai_inputs::{self, Limits},
    },
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
    let mut raw = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        raw.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        raw.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &raw)
}
fn config(flags: u16) -> Vec<u8> {
    let mut raw = [0; 24];
    raw[22..24].copy_from_slice(&flags.to_le_bytes());
    field(b"ACBS", &raw)
}
fn ai() -> Vec<u8> {
    vec![
        3, 4, 254, 128, 7, 165, 255, 19, 127, 68, 3, 128, 255, 255, 2, 1, 0, 0, 0, 128,
    ]
}
fn style(id: u32) -> Vec<u8> {
    field(b"ZNAM", &id.to_le_bytes())
}
fn body(creature: bool, flags: u16, raw: Vec<u8>) -> Vec<u8> {
    [
        config(flags),
        field(b"DATA", &vec![0; if creature { 17 } else { 11 }]),
        raw,
    ]
    .concat()
}
fn fixture(path: &Path, variant: u8) {
    fs::create_dir_all(path).unwrap();
    let valid = [field(b"AIDT", &ai()), style(0x300)].concat();
    let mut bytes = header(&[]);
    for (id, creature, version, flags, extra) in [
        (0x100, false, 15, 0, valid.clone()),
        (0x101, true, 13, 0, valid.clone()),
        (
            0x102,
            false,
            15,
            0,
            [
                field(b"AIDT", &ai()),
                field(b"AIDT", &ai()),
                style(0x300),
                style(0x301),
            ]
            .concat(),
        ),
        (0x103, false, 15, 0, Vec::new()),
        (0x104, false, 15, 16, valid.clone()),
        (0x105, false, 15, 1, valid.clone()),
        (
            0x107,
            false,
            15,
            0,
            [field(b"AIDT", &[0; 19]), field(b"ZNAM", &[0; 3])].concat(),
        ),
        (
            0x108,
            false,
            15,
            0,
            [field(b"AIDT", &ai()), style(0)].concat(),
        ),
        (
            0x109,
            false,
            15,
            0,
            [field(b"AIDT", &ai()), style(0x999)].concat(),
        ),
        (
            0x10a,
            false,
            15,
            0,
            [field(b"AIDT", &ai()), style(0x303)].concat(),
        ),
        (
            0x10b,
            false,
            15,
            0,
            [field(b"AIDT", &ai()), style(0x302)].concat(),
        ),
    ] {
        bytes.extend(disk(
            if creature { b"CREA" } else { b"NPC_" },
            id,
            0,
            version,
            &body(creature, flags, extra),
        ));
    }
    bytes.extend(disk(
        b"NPC_",
        0x10c,
        0,
        15,
        &[field(b"DATA", &[0; 11]), valid.clone()].concat(),
    ));
    bytes.extend(disk(
        b"NPC_",
        0x10d,
        0,
        15,
        &[config(0), body(false, 16, valid.clone())].concat(),
    ));
    for (at, index) in [0usize, 1, 4, 12, 14, 15].into_iter().enumerate() {
        let mut unknown = ai();
        unknown[index] = if index == 12 || index == 14 { 128 } else { 255 };
        bytes.extend(disk(
            b"NPC_",
            0x110 + at as u32,
            0,
            15,
            &body(false, 0, [field(b"AIDT", &unknown), style(0x300)].concat()),
        ));
    }
    bytes.extend(disk(b"NPC_", 0x120, plugin::DELETED, 15, &[]));
    // A source-only target header request must not decode this unrelated body.
    bytes.extend(disk(b"CSTY", 0x300, 0, 15, &field(b"UNKN", &[variant])));
    bytes.extend(disk(b"CSTY", 0x301, 0, 15, &[]));
    bytes.extend(disk(b"FACT", 0x302, 0, 15, &[]));
    bytes.extend(disk(b"CSTY", 0x303, plugin::DELETED, 15, &[]));
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
    fs::write(
        path.join("unknown.esp"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"NPC_", 0x100, 0, 99, &body(false, 0, valid.clone())),
        ]
        .concat(),
    )
    .unwrap();
    let mut other = ai();
    other[0] = 1;
    other[12] = 13;
    other[16..20].copy_from_slice(&(-7i32).to_le_bytes());
    fs::write(
        path.join("override.esp"),
        [
            header(&["FalloutNV.esm"]),
            disk(
                b"NPC_",
                0x100,
                0,
                15,
                &body(false, 0, [field(b"AIDT", &other), style(0x301)].concat()),
            ),
            disk(b"CSTY", 0x300, 0, 14, &field(b"UNKN", &[1])),
        ]
        .concat(),
    )
    .unwrap();
}
fn order(overrides: bool) -> Vec<String> {
    if overrides {
        vec!["FalloutNV.esm".into(), "override.esp".into()]
    } else {
        vec!["FalloutNV.esm".into()]
    }
}
fn source(path: &Path, overrides: bool) -> RecordStore {
    RecordStore::open_nv_headers(path, &order(overrides), Default::default()).unwrap()
}
fn with_inputs<T>(
    path: &Path,
    overrides: bool,
    action: impl FnOnce(&mut RecordStore, &actors::Catalogue<'_>) -> T,
) -> T {
    let mut store = source(path, overrides);
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    action(&mut store, &actors)
}

#[test]
fn exact_ai_signed_raw_and_unused_words_are_separate_from_live_behavior() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors| {
        for id in [0x100, 0x101] {
            let m = ai_inputs::request(store, actors, &key(id), Default::default()).unwrap();
            assert!(m.authored_ai_input_admitted);
            assert!(m.record_version_supported);
            assert_eq!(m.ai_data_template_flag, Some(false));
            assert_eq!(m.traits_template_flag, Some(false));
            let f = &m.ai_data[0];
            assert_eq!(f.field_index, 2);
            assert_eq!(f.field_decoded_offset, if id == 0x100 { 47 } else { 53 });
            assert_eq!(f.raw_bytes, ai());
            let v = f.value.as_ref().unwrap();
            assert_eq!(
                (
                    v.aggression,
                    v.confidence,
                    v.energy_level,
                    v.responsibility,
                    v.mood
                ),
                (3, 4, 254, 128, 7)
            );
            assert_eq!(v.mood_unused, [165, 255, 19]);
            assert_eq!(v.services_flags, 0x8003_447f);
            assert_eq!(v.teaches, -1);
            assert_eq!(v.maximum_training_level, 255);
            assert_eq!(v.assistance, 2);
            assert_eq!(v.aggro_radius_behavior, 1);
            assert_eq!(v.aggro_radius, i32::MIN);
            assert!(f.findings.is_empty());
            let s = &m.combat_styles[0];
            assert_eq!(s.field_index, 3);
            assert_eq!(s.binding.as_ref().unwrap().key, Some(key(0x300)));
            assert!(s.binding_admitted);
            assert_eq!(s.winning_header.as_ref().unwrap().kind, *b"CSTY");
            assert_eq!(m.field_visits, 12);
            assert_eq!(m.raw_bytes, 24);
            assert!(!m.live_behavior_evaluated);
            assert!(!m.execution_supported);
        }
    });
}
#[test]
fn repeated_missing_unknown_versions_layouts_and_template_categories_stay_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors| {
        for id in [0x102, 0x103, 0x104, 0x107, 0x10c, 0x10d] {
            let m = ai_inputs::request(store, actors, &key(id), Default::default()).unwrap();
            assert!(!m.authored_ai_input_admitted, "{id:x}");
            if id == 0x102 {
                assert_eq!(m.ai_data.len(), 2);
                assert_eq!(m.combat_styles.len(), 2);
                assert!(m.ai_data.iter().all(|f| f.repeated));
                assert!(m.combat_styles.iter().all(|f| !f.binding_admitted));
            }
            if id == 0x107 {
                assert_eq!(m.ai_data[0].raw_bytes.len(), 19);
                assert!(m.ai_data[0].value.is_none());
                assert!(m.combat_styles[0].binding.is_none());
            }
        }
        let ai_inherited =
            ai_inputs::request(store, actors, &key(0x104), Default::default()).unwrap();
        assert!(ai_inherited.combat_styles[0].binding_admitted);
        let traits_inherited =
            ai_inputs::request(store, actors, &key(0x105), Default::default()).unwrap();
        assert!(traits_inherited.authored_ai_input_admitted);
        assert!(!traits_inherited.combat_styles[0].binding_admitted);
        for id in [0x120, 0x300, 0xffff] {
            assert!(ai_inputs::request(store, actors, &key(id), Default::default()).is_err());
        }
    });
}
#[test]
fn unknown_version_is_refused_by_existing_actor_admission_before_ai_request() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut store = RecordStore::open_nv_headers(
        temp.path(),
        &["FalloutNV.esm".into(), "unknown.esp".into()],
        Default::default(),
    )
    .unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let error = actors::Catalogue::load(&inventory, Default::default())
        .err()
        .expect("unknown actor version must be unavailable");
    assert!(
        error
            .to_string()
            .contains("NPC_ actor record version 99 at unknown.esp")
    );
}

#[test]
fn each_unknown_enum_is_retained_and_reported_without_invented_truth() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors| {
        for (at, code) in [
            "unknown_aggression_enum",
            "unknown_confidence_enum",
            "unknown_mood_enum",
            "unknown_teaches_enum",
            "unknown_assistance_enum",
            "unknown_aggro_radius_behavior_enum",
        ]
        .into_iter()
        .enumerate()
        {
            let m = ai_inputs::request(store, actors, &key(0x110 + at as u32), Default::default())
                .unwrap();
            assert!(!m.authored_ai_input_admitted);
            assert_eq!(m.ai_data[0].findings, vec![code]);
            assert!(m.ai_data[0].value.is_some());
            assert!(m.combat_styles[0].binding_admitted);
            assert!(!m.live_behavior_evaluated);
        }
    });
}
#[test]
fn source_target_refusals_and_winning_overrides_preserve_exact_header_identity() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors| {
        for (id, code) in [
            (0x108, "null_combat_style"),
            (0x109, "missing_combat_style"),
            (0x10a, "deleted_combat_style"),
            (0x10b, "combat_style_kind_not_allowed"),
        ] {
            let m = ai_inputs::request(store, actors, &key(id), Default::default()).unwrap();
            assert!(m.authored_ai_input_admitted);
            assert!(!m.combat_styles[0].binding_admitted);
            assert_eq!(m.combat_styles[0].findings, vec![code]);
        }
    });
    with_inputs(temp.path(), true, |store, actors| {
        let m = ai_inputs::request(store, actors, &key(0x100), Default::default()).unwrap();
        assert_eq!(m.actor.source.plugin, "override.esp");
        let v = m.ai_data[0].value.as_ref().unwrap();
        assert_eq!((v.aggression, v.teaches, v.aggro_radius), (1, 13, -7));
        assert_eq!(
            m.combat_styles[0].binding.as_ref().unwrap().key,
            Some(key(0x301))
        );
        let creature = ai_inputs::request(store, actors, &key(0x101), Default::default()).unwrap();
        let s = &creature.combat_styles[0];
        assert_eq!(
            s.binding
                .as_ref()
                .unwrap()
                .target
                .as_ref()
                .unwrap()
                .source_plugin,
            "override.esp"
        );
        assert_eq!(s.winning_header.as_ref().unwrap().version, 14);
    });
}
#[test]
fn changed_deferred_target_payload_and_forged_receipts_cannot_join_old_actor_sources() {
    let original = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(original.path(), 0);
    fixture(changed.path(), 1);
    with_inputs(original.path(), false, |_, actors| {
        let mut other = source(changed.path(), false);
        assert!(
            ai_inputs::request(&mut other, actors, &key(0x100), Default::default())
                .unwrap_err()
                .to_string()
                .contains("cohort")
        );
    });
    let mut old_store = source(original.path(), false);
    let mut inventory = inventory::Catalogue::load(&mut old_store, Default::default()).unwrap();
    let mut new_store = source(changed.path(), false);
    inventory.sources = new_store.source_receipts().unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    assert!(
        ai_inputs::request(&mut new_store, &actors, &key(0x100), Default::default())
            .unwrap_err()
            .to_string()
            .contains("winning actor")
    );
}
#[test]
fn selected_limits_charge_exact_source_work_and_all_output() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |store, actors| {
        let m = ai_inputs::request(store, actors, &key(0x100), Default::default()).unwrap();
        let bytes = serde_json::to_vec(&m).unwrap().len();
        let exact = Limits {
            max_sources: 1,
            max_field_visits: 12,
            max_selected_fields: 2,
            max_raw_bytes: 24,
            max_projection_bytes: bytes,
        };
        assert!(ai_inputs::request(store, actors, &key(0x100), exact).is_ok());
        for smaller in [
            Limits {
                max_sources: 0,
                ..exact
            },
            Limits {
                max_field_visits: 11,
                ..exact
            },
            Limits {
                max_selected_fields: 1,
                ..exact
            },
            Limits {
                max_raw_bytes: 23,
                ..exact
            },
            Limits {
                max_projection_bytes: bytes - 1,
                ..exact
            },
        ] {
            assert!(ai_inputs::request(store, actors, &key(0x100), smaller).is_err());
        }
    });
}
#[test]
fn export_optional_source_fixture_and_host_ai_inputs() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_AI_EVIDENCE_DIR") else {
        return;
    };
    let destination = Path::new(&destination);
    let install = destination.join("fixture");
    let data = install.join("Data");
    fixture(&data, 0);
    for overrides in [false, true] {
        let name = if overrides { "override" } else { "base" };
        fs::write(
            install.join(format!("{name}-order.json")),
            serde_json::to_vec(&order(overrides)).unwrap(),
        )
        .unwrap();
        with_inputs(&data, overrides, |store, actors| {
            for id in if overrides {
                vec![0x100, 0x101]
            } else {
                (0x100..=0x10d)
                    .filter(|id| *id != 0x106)
                    .chain(0x110..=0x115)
                    .collect()
            } {
                let m = ai_inputs::request(store, actors, &key(id), Default::default()).unwrap();
                fs::write(
                    install.join(format!("{name}-{id:x}-host.json")),
                    serde_json::to_vec(&m).unwrap(),
                )
                .unwrap();
            }
        });
    }
}
