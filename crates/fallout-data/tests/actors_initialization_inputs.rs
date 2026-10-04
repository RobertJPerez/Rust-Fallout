use fallout_data::{
    actors::{
        self, associations, classes,
        initialization_inputs::{self, Limits},
        races,
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
fn field(kind: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn disk(kind: &[u8; 4], id: u32, flags: u32, version: u16, raw: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
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
    let mut bytes = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        bytes.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        bytes.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &bytes)
}
fn configuration(mask: u16) -> Vec<u8> {
    let mut raw = [0; 24];
    raw[22..24].copy_from_slice(&mask.to_le_bytes());
    field(b"ACBS", &raw)
}
fn links(race: u32, class: u32) -> Vec<u8> {
    [
        field(b"RNAM", &race.to_le_bytes()),
        field(b"CNAM", &class.to_le_bytes()),
    ]
    .concat()
}
fn npc(mask: u16, extra: &[u8]) -> Vec<u8> {
    [
        configuration(mask),
        field(b"DATA", &[0; 11]),
        extra.to_vec(),
    ]
    .concat()
}
fn race_data() -> Vec<u8> {
    let mut raw = vec![
        128, 255, 13, 127, 0, 1, 255, 128, 12, 254, 7, 7, 1, 0, 165, 90,
    ];
    for value in [
        0x7fc0_1234_u32,
        0x8000_0000,
        0x7f80_0000,
        0xff80_0000,
        0xdead_beef,
    ] {
        raw.extend(value.to_le_bytes());
    }
    raw
}
fn race(variant: u8) -> Vec<u8> {
    [
        field(b"DATA", &race_data()),
        field(b"PNAM", &0x7fc0_abcd_u32.to_le_bytes()),
        field(b"UNAM", &0x8000_0000_u32.to_le_bytes()),
        field(b"UNKN", &[variant]),
    ]
    .concat()
}
fn class_data() -> Vec<u8> {
    let mut raw = Vec::new();
    for value in [-1_i32, i32::MIN, 13, i32::MAX] {
        raw.extend(value.to_le_bytes());
    }
    raw.extend(0x8004_0001_u32.to_le_bytes());
    raw.extend(u32::MAX.to_le_bytes());
    raw.extend([128, 255, 170, 187]);
    raw
}
fn class() -> Vec<u8> {
    [
        field(b"DATA", &class_data()),
        field(b"ATTR", &[0, 1, 2, 3, 254, 255, 7]),
    ]
    .concat()
}
fn fixture(path: &Path, variant: u8) {
    fs::create_dir_all(path).unwrap();
    let mut raw = header(&[]);
    for (id, mask, extra) in [
        (0x100, 0, links(0x300, 0x400)),
        (
            0x102,
            0,
            [
                links(0x300, 0x400),
                field(b"RNAM", &0x301_u32.to_le_bytes()),
            ]
            .concat(),
        ),
        (0x103, 0, vec![]),
        (0x104, 1, links(0x300, 0x400)),
        (0x105, 2, links(0x300, 0x400)),
        (0x106, 0, links(0, 0x999)),
        (0x107, 0, links(0x302, 0x402)),
        (0x108, 0, links(0x400, 0x300)),
        (
            0x10b,
            0,
            [
                links(0x300, 0x400),
                field(b"CNAM", &0x401_u32.to_le_bytes()),
            ]
            .concat(),
        ),
        (0x10c, 0, links(0x303, 0x403)),
        (0x10d, 0, links(0x304, 0x404)),
    ] {
        raw.extend(disk(b"NPC_", id, 0, 15, &npc(mask, &extra)));
    }
    raw.extend(disk(
        b"NPC_",
        0x109,
        0,
        15,
        &[field(b"DATA", &[0; 11]), links(0x300, 0x400)].concat(),
    ));
    raw.extend(disk(
        b"NPC_",
        0x10a,
        0,
        15,
        &[configuration(0), npc(1, &links(0x300, 0x400))].concat(),
    ));
    raw.extend(disk(b"NPC_", 0x10f, plugin::DELETED, 15, &[]));
    raw.extend(disk(
        b"CREA",
        0x101,
        0,
        15,
        &[
            configuration(0),
            field(b"DATA", &[0; 17]),
            field(b"RNAM", &[255, 254]),
            field(b"CNAM", &0x300_u32.to_le_bytes()),
        ]
        .concat(),
    ));
    for id in [0x300, 0x301] {
        raw.extend(disk(b"RACE", id, 0, 15, &race(variant)));
    }
    raw.extend(disk(b"RACE", 0x302, plugin::DELETED, 15, &[]));
    raw.extend(disk(
        b"RACE",
        0x303,
        0,
        15,
        &[field(b"PNAM", &[0; 4]), field(b"UNAM", &[0; 4])].concat(),
    ));
    raw.extend(disk(
        b"RACE",
        0x304,
        0,
        15,
        &[field(b"DATA", &race_data()), race(0)].concat(),
    ));
    for id in [0x400, 0x401] {
        raw.extend(disk(b"CLAS", id, 0, 15, &class()));
    }
    raw.extend(disk(b"CLAS", 0x402, plugin::DELETED, 15, &[]));
    raw.extend(disk(b"CLAS", 0x403, 0, 15, &field(b"DATA", &class_data())));
    raw.extend(disk(
        b"CLAS",
        0x404,
        0,
        15,
        &[class(), field(b"ATTR", &[255; 7])].concat(),
    ));
    fs::write(path.join("FalloutNV.esm"), raw).unwrap();
    let mut changed_race = race_data();
    changed_race[16..20].copy_from_slice(&0x3f80_0000_u32.to_le_bytes());
    let mut changed_class = class_data();
    changed_class[..4].copy_from_slice(&7_i32.to_le_bytes());
    fs::write(
        path.join("override.esp"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"NPC_", 0x100, 0, 15, &npc(0, &links(0x301, 0x401))),
            disk(
                b"RACE",
                0x301,
                0,
                15,
                &[
                    field(b"DATA", &changed_race),
                    field(b"PNAM", &[0; 4]),
                    field(b"UNAM", &[0; 4]),
                ]
                .concat(),
            ),
            disk(
                b"CLAS",
                0x401,
                0,
                14,
                &[field(b"DATA", &changed_class), field(b"ATTR", &[1; 7])].concat(),
            ),
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
fn with_inputs<T>(
    path: &Path,
    overrides: bool,
    action: impl FnOnce(
        &mut RecordStore,
        &actors::Catalogue<'_>,
        &associations::Catalogue<'_>,
        &races::Catalogue,
        &classes::Catalogue,
    ) -> T,
) -> T {
    let mut store =
        RecordStore::open_nv_headers(path, &order(overrides), Default::default()).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let races = races::Catalogue::load(&mut store, Default::default()).unwrap();
    let classes = classes::Catalogue::load(&mut store, Default::default()).unwrap();
    action(&mut store, &actors, &associations, &races, &classes)
}
#[test]
fn source_join_retains_signed_float_bits_and_both_sex_arrays() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |s, a, b, r, c| {
        let m =
            initialization_inputs::request(s, a, b, r, c, &key(0x100), Default::default()).unwrap();
        assert_eq!(m.links().len(), 2);
        assert!(m.links().iter().all(|l| l.initialization_inputs_available));
        assert_eq!(m.links()[0].association.field_index, 2);
        assert_eq!(m.links()[0].field.decoded_offset, 47);
        assert_eq!(m.links()[0].raw_bytes, 0x300_u32.to_le_bytes());
        let race = m.links()[0].race_definition.unwrap();
        let races::Value::RaceData {
            skill_boosts,
            height_bits,
            weight_bits,
            unused,
            flags,
        } = &race.fields[0].value
        else {
            panic!()
        };
        assert_eq!((skill_boosts[0].skill, skill_boosts[0].boost), (-128, -1));
        assert_eq!(*height_bits, [0x7fc0_1234, 0x8000_0000]);
        assert_eq!(*weight_bits, [0x7f80_0000, 0xff80_0000]);
        assert_eq!(*unused, [165, 90]);
        assert_eq!(*flags, 0xdead_beef);
        let class = m.links()[1].class_definition.unwrap();
        let classes::Value::ClassData {
            tag_skills,
            flags,
            services,
            teaches,
            maximum_training_level,
            unused,
        } = &class.fields[0].value
        else {
            panic!()
        };
        assert_eq!(*tag_skills, [-1, i32::MIN, 13, i32::MAX]);
        assert_eq!(*flags, 0x8004_0001);
        assert_eq!(*services, u32::MAX);
        assert_eq!(
            (*teaches, *maximum_training_level, *unused),
            (-128, 255, [170, 187])
        );
        let j = serde_json::to_value(&m).unwrap();
        assert_eq!(j["initialization_supported"], false);
        assert_eq!(j["traits_template_flag"], false);
        assert_eq!(j["selected_fields"], 13);
        assert_eq!(j["field_visits"], 22);
        assert_eq!(j["decoded_bytes"], 183);
    });
}
#[test]
fn repeats_and_traits_inheritance_never_choose_a_winner() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |s, a, b, r, c| {
        for id in [0x102, 0x104, 0x10b] {
            let m = initialization_inputs::request(s, a, b, r, c, &key(id), Default::default())
                .unwrap();
            for link in m.links() {
                if link.repeated || id == 0x104 {
                    assert!(!link.direct_binding_available);
                    assert!(!link.initialization_inputs_available);
                }
            }
        }
        let m =
            initialization_inputs::request(s, a, b, r, c, &key(0x105), Default::default()).unwrap();
        assert!(m.links().iter().all(|l| l.initialization_inputs_available));
    });
}
#[test]
fn creature_role_and_unavailable_sources_stay_explicit() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |s, a, b, r, c| {
        for id in [0x101, 0x103] {
            let m = initialization_inputs::request(s, a, b, r, c, &key(id), Default::default())
                .unwrap();
            assert!(m.links().is_empty());
        }
        for id in [0x106, 0x107, 0x108, 0x109, 0x10a] {
            let m = initialization_inputs::request(s, a, b, r, c, &key(id), Default::default())
                .unwrap();
            assert!(m.links().iter().all(|l| !l.direct_binding_available));
        }
        for id in [0x10c, 0x10d] {
            let m = initialization_inputs::request(s, a, b, r, c, &key(id), Default::default())
                .unwrap();
            assert!(
                m.links()
                    .iter()
                    .all(|l| l.direct_binding_available && !l.initialization_inputs_available)
            );
        }
        for id in [0x300, 0x999, 0x10f] {
            assert!(
                initialization_inputs::request(s, a, b, r, c, &key(id), Default::default())
                    .is_err()
            );
        }
    });
}
#[test]
fn actor_race_and_class_overrides_preserve_exact_target_provenance() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), true, |s, a, b, r, c| {
        let m =
            initialization_inputs::request(s, a, b, r, c, &key(0x100), Default::default()).unwrap();
        assert_eq!(m.actor().source.plugin, "override.esp");
        assert_eq!(m.links()[0].association.binding.key, Some(key(0x301)));
        let race = m.links()[0].race_definition.unwrap();
        let class = m.links()[1].class_definition.unwrap();
        assert_eq!(race.source.plugin, "override.esp");
        assert_eq!(class.source.plugin, "override.esp");
        assert_eq!(class.header.version, 14);
        let races::Value::RaceData { height_bits, .. } = race.fields[0].value else {
            panic!()
        };
        assert_eq!(height_bits[0], 0x3f80_0000);
        let classes::Value::ClassData { tag_skills, .. } = class.fields[0].value else {
            panic!()
        };
        assert_eq!(tag_skills[0], 7);
    });
}
#[test]
fn same_header_changed_deferred_body_cannot_relabel_old_producers() {
    let old = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(old.path(), 0);
    fixture(changed.path(), 1);
    with_inputs(old.path(), false, |_, a, b, r, c| {
        let mut store =
            RecordStore::open_nv_headers(changed.path(), &order(false), Default::default())
                .unwrap();
        assert!(
            initialization_inputs::request(&mut store, a, b, r, c, &key(0x100), Default::default())
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
    let races = races::Catalogue::load(&mut fresh, Default::default()).unwrap();
    let classes = classes::Catalogue::load(&mut fresh, Default::default()).unwrap();
    assert!(
        initialization_inputs::request(
            &mut fresh,
            &actors,
            &assoc,
            &races,
            &classes,
            &key(0x100),
            Default::default()
        )
        .unwrap_err()
        .to_string()
        .contains("retained winner differs")
    );
}
#[test]
fn every_source_work_and_output_bound_has_an_exact_edge() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_inputs(temp.path(), false, |s, a, b, r, c| {
        let m =
            initialization_inputs::request(s, a, b, r, c, &key(0x100), Default::default()).unwrap();
        let size = serde_json::to_vec(&m).unwrap().len();
        drop(m);
        let exact = Limits {
            max_sources: 1,
            max_field_visits: 22,
            max_selected_fields: 13,
            max_links: 2,
            max_decoded_bytes: 183,
            max_projection_bytes: size,
        };
        initialization_inputs::request(s, a, b, r, c, &key(0x100), exact).unwrap();
        for limit in [
            Limits {
                max_sources: 0,
                ..exact
            },
            Limits {
                max_field_visits: 21,
                ..exact
            },
            Limits {
                max_selected_fields: 12,
                ..exact
            },
            Limits {
                max_links: 1,
                ..exact
            },
            Limits {
                max_decoded_bytes: 182,
                ..exact
            },
            Limits {
                max_projection_bytes: size - 1,
                ..exact
            },
        ] {
            assert!(initialization_inputs::request(s, a, b, r, c, &key(0x100), limit).is_err());
        }
    });
}
#[test]
fn export_complete_source_requests_for_separate_oracle() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_INIT_EVIDENCE_DIR") else {
        return;
    };
    let destination = Path::new(&destination);
    assert!(destination.is_absolute() && destination.is_dir());
    let path = destination.join("fixture");
    fixture(&path.join("Data"), 0);
    for overrides in [false, true] {
        let cohort = if overrides { "override" } else { "base" };
        fs::write(
            path.join(format!("{cohort}-order.json")),
            serde_json::to_vec(&order(overrides)).unwrap(),
        )
        .unwrap();
        with_inputs(&path.join("Data"), overrides, |s, a, b, r, c| {
            let roots = if overrides {
                vec![0x100, 0x102]
            } else {
                (0x100..=0x10d).collect::<Vec<_>>()
            };
            for id in roots {
                let m = initialization_inputs::request(s, a, b, r, c, &key(id), Default::default())
                    .unwrap();
                fs::write(
                    path.join(format!("{cohort}-{id:x}-host.json")),
                    serde_json::to_vec_pretty(&m).unwrap(),
                )
                .unwrap();
            }
        });
    }
}
