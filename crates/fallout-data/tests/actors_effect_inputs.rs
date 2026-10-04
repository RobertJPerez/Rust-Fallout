use fallout_data::{
    actors::{
        self, associations,
        effect_inputs::{self, Limits, Value},
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
fn actor(creature: bool, mask: u16, extra: Vec<u8>) -> Vec<u8> {
    [
        config(mask),
        field(b"DATA", &vec![0; if creature { 17 } else { 11 }]),
        extra,
    ]
    .concat()
}
fn link(tag: &[u8; 4], id: u32) -> Vec<u8> {
    field(tag, &id.to_le_bytes())
}
fn metadata(enchantment: bool) -> Vec<u8> {
    let words = if enchantment {
        [3_u32, 0x8000_0000, u32::MAX]
    } else {
        [10_u32, u32::MAX, 0x8000_0000]
    };
    let mut raw = Vec::new();
    for word in words {
        raw.extend(word.to_le_bytes());
    }
    raw.extend([255, 170, 187, 204]);
    field(if enchantment { b"ENIT" } else { b"SPIT" }, &raw)
}
fn effect_data(kind: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for word in [u32::MAX, 0x8000_0000, 65535, kind, 0x8000_0000] {
        raw.extend(word.to_le_bytes());
    }
    raw
}
fn group(id: u32, kind: u32) -> Vec<u8> {
    [link(b"EFID", id), field(b"EFIT", &effect_data(kind))].concat()
}
fn spell() -> Vec<u8> {
    [
        field(b"EDID", b"literal_effect\0"),
        metadata(false),
        group(0x700, 2),
        field(b"CTDA", &[187; 32]),
        group(0x700, 0),
        group(0x701, 1),
        group(0, 1),
        group(0x999, 1),
        group(0x702, 1),
    ]
    .concat()
}
fn fixture(path: &Path, variant: u8) {
    fs::create_dir_all(path).unwrap();
    let mut bytes = header(&[]);
    for (id, creature, mask, extra) in [
        (
            0x100,
            false,
            0,
            [
                link(b"SPLO", 0x300),
                link(b"SPLO", 0x300),
                link(b"EITM", 0x400),
            ]
            .concat(),
        ),
        (0x101, true, 0, link(b"EITM", 0x400)),
        (0x102, false, 8, link(b"SPLO", 0x300)),
        (0x103, false, 1, link(b"SPLO", 0x300)),
        (
            0x104,
            false,
            0,
            [link(b"EITM", 0x400), link(b"EITM", 0x401)].concat(),
        ),
        (0x105, false, 0, link(b"SPLO", 0)),
        (0x106, false, 0, link(b"SPLO", 0x999)),
        (0x107, false, 0, link(b"SPLO", 0x305)),
        (0x108, false, 0, link(b"SPLO", 0x702)),
        (0x109, false, 0, link(b"SPLO", 0x301)),
        (0x10a, false, 0, link(b"SPLO", 0x302)),
        (0x10b, false, 0, link(b"SPLO", 0x303)),
        (0x10c, false, 0, link(b"SPLO", 0x304)),
        (0x10d, false, 0, link(b"EITM", 0x401)),
        (0x110, false, 0, link(b"SPLO", 0x306)),
        (0x111, false, 0, link(b"SPLO", 0x307)),
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
        0x10e,
        0,
        15,
        &[field(b"DATA", &[0; 11]), link(b"SPLO", 0x300)].concat(),
    ));
    bytes.extend(disk(
        b"NPC_",
        0x10f,
        0,
        15,
        &[config(0), actor(false, 8, link(b"SPLO", 0x300))].concat(),
    ));
    bytes.extend(disk(b"NPC_", 0x120, plugin::DELETED, 15, &[]));
    bytes.extend(disk(b"SPEL", 0x300, 0, 15, &spell()));
    bytes.extend(disk(
        b"SPEL",
        0x301,
        0,
        15,
        &[
            metadata(false),
            group(0x700, 1),
            field(b"EFIT", &effect_data(1)),
        ]
        .concat(),
    ));
    bytes.extend(disk(
        b"SPEL",
        0x302,
        0,
        15,
        &[
            field(b"EFIT", &effect_data(1)),
            metadata(false),
            link(b"EFID", 0x700),
            field(b"CTDA", &[0; 32]),
            field(b"EFIT", &effect_data(1)),
        ]
        .concat(),
    ));
    bytes.extend(disk(
        b"SPEL",
        0x303,
        0,
        15,
        &[
            field(b"SPIT", &[0; 15]),
            field(b"EFID", &[0; 3]),
            field(b"EFIT", &[0; 19]),
        ]
        .concat(),
    ));
    bytes.extend(disk(
        b"SPEL",
        0x304,
        0,
        99,
        &[metadata(false), group(0x700, 1)].concat(),
    ));
    bytes.extend(disk(b"SPEL", 0x305, plugin::DELETED, 15, &[]));
    bytes.extend(disk(
        b"SPEL",
        0x306,
        0,
        15,
        &[metadata(false), group(0x700, 99)].concat(),
    ));
    bytes.extend(disk(
        b"SPEL",
        0x307,
        0,
        15,
        &[metadata(false), group(0x700, 1), field(b"UNKN", &[255])].concat(),
    ));
    bytes.extend(disk(
        b"ENCH",
        0x400,
        0,
        15,
        &[metadata(true), group(0x700, 1)].concat(),
    ));
    bytes.extend(disk(b"ENCH", 0x401, 0, 15, &link(b"EFID", 0x700)));
    // Deliberately malformed MGEF body: the producer requests headers, not bodies.
    bytes.extend(disk(b"MGEF", 0x700, 0, 15, &[variant]));
    bytes.extend(disk(b"MGEF", 0x701, plugin::DELETED, 15, &[]));
    bytes.extend(disk(b"FACT", 0x702, 0, 15, &[]));
    fs::write(path.join("FalloutNV.esm"), bytes).unwrap();
    fs::write(
        path.join("override.esp"),
        [
            header(&["FalloutNV.esm"]),
            disk(
                b"SPEL",
                0x300,
                0,
                15,
                &[metadata(false), group(0x703, 0)].concat(),
            ),
            disk(b"MGEF", 0x703, 0, 15, &[255]),
            disk(b"MGEF", 0x700, 0x80, 14, &[255]),
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
fn with_sources<T>(
    path: &Path,
    overrides: bool,
    action: impl FnOnce(&mut RecordStore, &actors::Catalogue<'_>, &associations::Catalogue<'_>) -> T,
) -> T {
    let mut store =
        RecordStore::open_nv_headers(path, &order(overrides), Default::default()).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let assoc = associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    action(&mut store, &actors, &assoc)
}
#[test]
fn exact_repeated_effect_groups_keep_raw_words_and_header_requests() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), false, |s, a, b| {
        for index in [2, 3] {
            let m =
                effect_inputs::request(s, a, b, &key(0x100), index, Default::default()).unwrap();
            let d = m.declaration().unwrap();
            assert_eq!(d.groups.len(), 6);
            assert!(d.groups[0].binding_admitted && d.groups[1].binding_admitted);
            assert_eq!(d.groups[0].base_effect.as_ref().unwrap().key, key(0x700));
            assert_eq!(d.groups[1].base_effect.as_ref().unwrap().key, key(0x700));
            assert_eq!(d.groups[0].condition_field_indices, vec![4]);
            assert_eq!(d.fields[4].raw_bytes, [187; 32]);
            let Value::EffectData {
                magnitude,
                area,
                duration,
                effect_type,
                actor_value,
                known_effect_type,
            } = d.fields[3].value
            else {
                panic!()
            };
            assert_eq!(
                (
                    magnitude,
                    area,
                    duration,
                    effect_type,
                    actor_value,
                    known_effect_type
                ),
                (u32::MAX, 0x8000_0000, 65535, 2, i32::MIN, true)
            );
            let Value::SpellMetadata {
                effect_type,
                cost_unused,
                level_unused,
                flags,
                unused,
            } = d.fields[1].value
            else {
                panic!()
            };
            assert_eq!(
                (effect_type, cost_unused, level_unused, flags, unused),
                (10, u32::MAX, 0x8000_0000, 255, [170, 187, 204])
            );
            assert_eq!(
                d.groups[2].binding.as_ref().unwrap().status,
                inventory::Status::Deleted
            );
            assert!(d.groups[2].base_effect.is_some());
            assert_eq!(
                d.groups[3].binding.as_ref().unwrap().status,
                inventory::Status::Null
            );
            assert_eq!(
                d.groups[4].binding.as_ref().unwrap().status,
                inventory::Status::Missing
            );
            assert_eq!(d.groups[5].schema_kind_allowed, Some(false));
            assert!(d.groups[2..].iter().all(|g| !g.binding_admitted));
            let j = serde_json::to_value(&m).unwrap();
            assert_eq!(j["execution_supported"], false);
            assert_eq!(j["active_effects_created"], false);
        }
    });
}
#[test]
fn enchantment_and_creature_links_use_the_exact_existing_association() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), false, |s, a, b| {
        for (id, index) in [(0x100, 4), (0x101, 2)] {
            let m = effect_inputs::request(s, a, b, &key(id), index, Default::default()).unwrap();
            let d = m.declaration().unwrap();
            assert_eq!(d.source.header.kind, *b"ENCH");
            assert_eq!(d.groups.len(), 1);
            assert!(d.groups[0].data_admitted);
            let Value::EnchantmentMetadata {
                effect_type,
                unused_words,
                flags,
                unused,
            } = d.fields[0].value
            else {
                panic!()
            };
            assert_eq!(
                (effect_type, unused_words, flags, unused),
                (3, [0x8000_0000, u32::MAX], 255, [170, 187, 204])
            );
        }
    });
}
#[test]
fn inheritance_singletons_and_missing_configuration_never_admit_a_live_effect() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), false, |s, a, b| {
        for id in [0x102, 0x104, 0x10e, 0x10f] {
            let m = effect_inputs::request(
                s,
                a,
                b,
                &key(id),
                if id == 0x10e {
                    1
                } else if id == 0x10f {
                    3
                } else {
                    2
                },
                Default::default(),
            )
            .unwrap();
            assert!(
                !serde_json::to_value(&m).unwrap()["declaration_binding_available"]
                    .as_bool()
                    .unwrap()
            );
            assert!(
                m.declaration()
                    .unwrap()
                    .groups
                    .iter()
                    .all(|g| !g.binding_admitted)
            );
        }
        let m = effect_inputs::request(s, a, b, &key(0x103), 2, Default::default()).unwrap();
        assert!(m.declaration().unwrap().groups[0].binding_admitted);
        for id in [0x105, 0x106, 0x107, 0x108] {
            let m = effect_inputs::request(s, a, b, &key(id), 2, Default::default()).unwrap();
            assert!(m.declaration().is_none());
        }
        for (id, index) in [(0x999, 2), (0x120, 2), (0x300, 2), (0x100, 0), (0x100, 99)] {
            assert!(effect_inputs::request(s, a, b, &key(id), index, Default::default()).is_err());
        }
    });
}
#[test]
fn unsupported_grouping_layout_and_version_keep_all_physical_fields() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), false, |s, a, b| {
        for id in [0x109, 0x10a, 0x10b, 0x10c, 0x10d, 0x110, 0x111] {
            let m = effect_inputs::request(s, a, b, &key(id), 2, Default::default()).unwrap();
            let d = m.declaration().unwrap();
            assert!(d.groups.iter().all(|g| !g.data_admitted));
            assert!(!d.fields.is_empty());
        }
        let m = effect_inputs::request(s, a, b, &key(0x10c), 2, Default::default()).unwrap();
        let d = m.declaration().unwrap();
        assert!(!d.record_version_supported);
        assert!(d.fields.iter().all(|f| matches!(f.value, Value::Opaque)));
        assert!(d.groups[0].binding.is_none());
    });
}
#[test]
fn winning_override_changes_only_exact_selected_source_inputs() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), true, |s, a, b| {
        let m = effect_inputs::request(s, a, b, &key(0x100), 2, Default::default()).unwrap();
        let d = m.declaration().unwrap();
        assert_eq!(d.source.source_name, "override.esp");
        assert_eq!(d.groups.len(), 1);
        assert_eq!(d.groups[0].base_effect.as_ref().unwrap().key, key(0x703));
        let m = effect_inputs::request(s, a, b, &key(0x101), 2, Default::default()).unwrap();
        let target = m.declaration().unwrap().groups[0]
            .base_effect
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                target.source_name.as_str(),
                target.header.version,
                target.header.flags
            ),
            ("override.esp", 14, 0x80)
        );
    });
}
#[test]
fn source_hashes_refuse_same_header_changed_deferred_bodies_and_receipt_relabels() {
    let old = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(old.path(), 0);
    fixture(changed.path(), 1);
    with_sources(old.path(), false, |_, a, b| {
        let mut fresh =
            RecordStore::open_nv_headers(changed.path(), &order(false), Default::default())
                .unwrap();
        assert!(
            effect_inputs::request(&mut fresh, a, b, &key(0x100), 2, Default::default())
                .unwrap_err()
                .to_string()
                .contains("source cohorts differ")
        );
    });
    let mut old_store =
        RecordStore::open_nv_headers(old.path(), &order(false), Default::default()).unwrap();
    let mut inv = inventory::Catalogue::load(&mut old_store, Default::default()).unwrap();
    let mut fresh =
        RecordStore::open_nv_headers(changed.path(), &order(false), Default::default()).unwrap();
    inv.sources = fresh.source_receipts().unwrap();
    let actors = actors::Catalogue::load(&inv, Default::default()).unwrap();
    let assoc = associations::Catalogue::load(&mut fresh, &actors, Default::default()).unwrap();
    assert!(
        effect_inputs::request(
            &mut fresh,
            &actors,
            &assoc,
            &key(0x100),
            2,
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("retained actor differs")
    );
}
#[test]
fn every_selected_source_and_output_budget_has_a_precise_edge() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), false, |s, a, b| {
        let m = effect_inputs::request(s, a, b, &key(0x101), 2, Default::default()).unwrap();
        let j = serde_json::to_value(&m).unwrap();
        let d = m.declaration().unwrap();
        let size = serde_json::to_vec(&m).unwrap().len();
        let exact = Limits {
            max_sources: 1,
            max_depth: 2,
            max_selected_records: j["selected_records"].as_u64().unwrap() as usize,
            max_record_bytes: d.source.header.stored_size as usize,
            max_decoded_bytes: j["decoded_bytes"].as_u64().unwrap() as usize,
            max_field_visits: j["field_visits"].as_u64().unwrap() as usize,
            max_fields: j["retained_fields"].as_u64().unwrap() as usize,
            max_bindings: 1,
            max_groups: 1,
            max_raw_bytes: j["raw_bytes"].as_u64().unwrap() as usize,
            max_projection_bytes: size,
        };
        drop(m);
        effect_inputs::request(s, a, b, &key(0x101), 2, exact).unwrap();
        for limit in [
            Limits {
                max_sources: 0,
                ..exact
            },
            Limits {
                max_depth: 1,
                ..exact
            },
            Limits {
                max_selected_records: exact.max_selected_records - 1,
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
                max_bindings: 0,
                ..exact
            },
            Limits {
                max_groups: 0,
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
            assert!(effect_inputs::request(s, a, b, &key(0x101), 2, limit).is_err());
        }
    });
}
#[test]
fn retain_authored_effect_requests_for_independent_source_comparison() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_EFFECT_EVIDENCE_DIR") else {
        return;
    };
    let destination = Path::new(&destination);
    assert!(destination.is_absolute() && destination.is_dir());
    let case = destination.join("fixture");
    fixture(&case.join("Data"), 0);
    for overrides in [false, true] {
        let cohort = if overrides { "override" } else { "base" };
        fs::write(
            case.join(format!("{cohort}-order.json")),
            serde_json::to_vec(&order(overrides)).unwrap(),
        )
        .unwrap();
        with_sources(&case.join("Data"), overrides, |s, a, b| {
            let cases = if overrides {
                vec![(0x100, 2), (0x101, 2)]
            } else {
                (0x100..=0x111)
                    .flat_map(|id| match id {
                        0x100 => vec![(id, 2), (id, 3), (id, 4)],
                        0x10e => vec![(id, 1)],
                        0x10f => vec![(id, 3)],
                        _ => vec![(id, 2)],
                    })
                    .collect()
            };
            for (id, index) in cases {
                let m =
                    effect_inputs::request(s, a, b, &key(id), index, Default::default()).unwrap();
                fs::write(
                    case.join(format!("{cohort}-{id:x}-{index}-host.json")),
                    serde_json::to_vec_pretty(&m).unwrap(),
                )
                .unwrap();
            }
        });
    }
}
