use fallout_data::{
    actors::{
        self,
        dependencies::{
            LookupStatus, ManifestLimits, Sex, Value,
            equipment::{self, Choice, Limits, Role},
        },
    },
    assets::ArchiveAssets,
    identity::{FormKey, ProfileId},
    inventory, plugin,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};
fn field(kind: &[u8; 4], raw: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(raw.len() as u16).to_le_bytes(), raw].concat()
}
fn disk(kind: &[u8; 4], id: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let body = if flags & plugin::COMPRESSED != 0 && flags & plugin::DELETED == 0 {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), Default::default());
        encoder.write_all(body).unwrap();
        [
            &(body.len() as u32).to_le_bytes()[..],
            &encoder.finish().unwrap(),
        ]
        .concat()
    } else {
        body.to_vec()
    };
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0; 2],
        &body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut bytes = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for name in masters {
        bytes.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        bytes.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &bytes)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn armor(extra: &[u8]) -> Vec<u8> {
    [
        field(
            b"BMDT",
            &[u32::MAX.to_le_bytes().as_slice(), &[255, 0xaa, 0xbb, 0xcc]].concat(),
        ),
        field(b"ETYP", &i32::MIN.to_le_bytes()),
        field(b"MODL", b"armor_male.nif\0"),
        field(b"MOD2", b"armor_male_world.nif\0"),
        field(b"MOD3", b"armor_female.nif\0"),
        field(b"MOD4", b"armor_female_world.nif\0"),
        field(b"MODS", b"opaque texture swaps"),
        extra.to_vec(),
    ]
    .concat()
}
fn fixture(path: &Path, armor_extra: &[u8], weapon_extra: &[u8]) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let actor = [
        field(b"ACBS", &[0; 24]),
        field(b"DATA", &[0; 11]),
        field(
            b"CNTO",
            &[0x200_u32.to_le_bytes().as_slice(), &1_i32.to_le_bytes()].concat(),
        ),
    ]
    .concat();
    let mut weapon = field(b"ETYP", &(-1_i32).to_le_bytes());
    for (index, tag) in [
        *b"MODL", *b"MWD1", *b"MWD2", *b"MWD3", *b"MWD4", *b"MWD5", *b"MWD6", *b"MWD7",
    ]
    .into_iter()
    .enumerate()
    {
        weapon.extend(field(&tag, format!("weapon_{index}.nif\0").as_bytes()));
    }
    for (tag, path) in [
        (b"MOD2", b"shell.nif\0".as_slice()),
        (b"MOD3", b"scope.nif\0".as_slice()),
        (b"MOD4", b"weapon_world.nif\0".as_slice()),
    ] {
        weapon.extend(field(tag, path));
    }
    for (index, tag) in [
        *b"WNAM", *b"WNM1", *b"WNM2", *b"WNM3", *b"WNM4", *b"WNM5", *b"WNM6", *b"WNM7",
    ]
    .into_iter()
    .enumerate()
    {
        weapon.extend(field(&tag, &(0x300_u32 + index as u32).to_le_bytes()));
    }
    weapon.extend(weapon_extra);
    let mut bytes = [
        header(&[]),
        disk(b"NPC_", 0x100, 0, 15, &actor),
        disk(b"ARMO", 0x200, plugin::COMPRESSED, 15, &armor(armor_extra)),
        disk(b"WEAP", 0x210, 0, 15, &weapon),
        disk(b"ARMA", 0x220, 0, 15, &armor(&[])),
        disk(
            b"ARMO",
            0x230,
            plugin::DELETED | plugin::COMPRESSED,
            99,
            b"unread invalid tombstone",
        ),
        disk(b"MISC", 0x400, 0, 15, &[]),
    ]
    .concat();
    for index in 0..8 {
        bytes.extend(disk(
            b"STAT",
            0x300 + index,
            0,
            15,
            &field(b"MODL", format!("first_{index}.nif\0").as_bytes()),
        ));
    }
    fs::write(path.join("Data/FalloutNV.esm"), bytes).unwrap();
    let mut builder = dream_archive::Tes4BsaBuilder::fallout_new_vegas();
    for name in [
        "armor_male",
        "armor_female",
        "armor_male_world",
        "armor_female_world",
        "shell",
        "scope",
        "weapon_world",
    ] {
        builder
            .add_bytes(format!("meshes/{name}.nif"), b"metadata only; not a NIF")
            .unwrap();
    }
    for index in 0..8 {
        for prefix in ["weapon", "first"] {
            builder
                .add_bytes(format!("meshes/{prefix}_{index}.nif"), b"metadata only")
                .unwrap();
        }
    }
    builder.write_path(path.join("Data/A.bsa")).unwrap();
}
fn with_sources(
    path: &Path,
    order: &[&str],
    callback: impl FnOnce(&mut RecordStore, &actors::Catalogue<'_>, &ArchiveAssets),
) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &order.iter().map(|n| (*n).into()).collect::<Vec<_>>(),
        Default::default(),
    )
    .unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let assets = ArchiveAssets::open_nv(path).unwrap();
    callback(&mut store, &actors, &assets);
}
fn selected(
    store: &mut RecordStore,
    actors: &actors::Catalogue<'_>,
    assets: &ArchiveAssets,
    id: u32,
    role: Role,
    limits: Limits,
) -> serde_json::Value {
    serde_json::to_value(
        equipment::request(
            store,
            actors,
            &key(0x100),
            Choice {
                equipment: key(id),
                role,
            },
            assets,
            limits,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn explicit_armor_sex_and_world_roles_keep_slots_without_inferred_equipping_or_fallback() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), &[], &[]);
    with_sources(
        directory.path(),
        &["FalloutNV.esm"],
        |store, actors, assets| {
            let mut results = Vec::new();
            for (role, wanted) in [
                (Role::ArmorBiped { sex: Sex::Male }, "armor_male.nif"),
                (Role::ArmorBiped { sex: Sex::Female }, "armor_female.nif"),
                (Role::ArmorWorld { sex: Sex::Male }, "armor_male_world.nif"),
                (
                    Role::ArmorWorld { sex: Sex::Female },
                    "armor_female_world.nif",
                ),
            ] {
                let report = equipment::request(
                    store,
                    actors,
                    &key(0x100),
                    Choice {
                        equipment: key(0x200),
                        role,
                    },
                    assets,
                    Default::default(),
                )
                .unwrap();
                assert_eq!(report.requests.len(), 1);
                assert_eq!(report.requests[0].path.raw, wanted.as_bytes());
                assert_eq!(
                    report.requests[0].path.lookup_status,
                    LookupStatus::OneArchiveCandidate
                );
                assert!(report.issues.is_empty());
                assert!(
                    !report.equipped_state_verified
                        && !report.effective_model_selection_supported
                        && !report.slot_conflicts_evaluated
                        && !report.texture_swaps_applied
                        && !report.attachment_target_selected
                );
                assert!(matches!(
                    report.source_records[0].fields[0].value,
                    Value::BipedSlots {
                        flags: u32::MAX,
                        general_flags: 255,
                        unused: [0xaa, 0xbb, 0xcc]
                    }
                ));
                assert!(matches!(
                    report.source_records[0].fields[1].value,
                    Value::EquipmentType { raw: i32::MIN }
                ));
                assert!(matches!(
                    report.source_records[0].fields[6].value,
                    Value::Opaque
                ));
                results.push(serde_json::to_value(report).unwrap());
            }
            // Explicit ARMA choice is absent from actor CNTO; it still supplies
            // a declaration request without claiming membership/equipping.
            let outside = selected(
                store,
                actors,
                assets,
                0x220,
                Role::ArmorBiped { sex: Sex::Female },
                Default::default(),
            );
            assert_eq!(
                outside["requests"][0]["path"]["raw"],
                serde_json::json!(b"armor_female.nif".as_slice())
            );
            if let Some(root) = std::env::var_os("FALLOUT_ACTOR_EQUIPMENT_EVIDENCE_DIR") {
                let root = Path::new(&root);
                assert!(root.is_absolute() && root.is_dir());
                let output = root.join("authored-equipment");
                fs::create_dir(&output).unwrap();
                fs::create_dir(output.join("Data")).unwrap();
                for name in ["FalloutNV.esm", "A.bsa"] {
                    fs::copy(
                        directory.path().join("Data").join(name),
                        output.join("Data").join(name),
                    )
                    .unwrap();
                }
                fs::write(output.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
                fs::write(
                    output.join("expected-armor.json"),
                    serde_json::to_vec_pretty(&results).unwrap(),
                )
                .unwrap();
            }
        },
    );
}

#[test]
fn weapon_source_variants_and_singleton_first_person_stat_links_are_explicit() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), &[], &[]);
    with_sources(
        directory.path(),
        &["FalloutNV.esm"],
        |store, actors, assets| {
            for mask in 0..8 {
                for (role, path, sources) in [
                    (
                        Role::WeaponModel { mod_mask: mask },
                        format!("weapon_{mask}.nif"),
                        1,
                    ),
                    (
                        Role::WeaponFirstPerson { mod_mask: mask },
                        format!("first_{mask}.nif"),
                        2,
                    ),
                ] {
                    let report = equipment::request(
                        store,
                        actors,
                        &key(0x100),
                        Choice {
                            equipment: key(0x210),
                            role,
                        },
                        assets,
                        Default::default(),
                    )
                    .unwrap();
                    assert_eq!(report.source_records.len(), sources);
                    assert_eq!(report.requests[0].path.raw, path.as_bytes());
                    assert!(report.issues.is_empty());
                    assert!(!report.equipped_state_verified);
                    assert_eq!(report.requests[0].role, role);
                    if sources == 2 {
                        assert_eq!(report.source_records[1].key, key(0x300 + u32::from(mask)));
                        assert_eq!(report.selected_links[0].target_source_index, Some(1));
                    }
                }
            }
            for (role, path) in [
                (Role::WeaponShell, "shell.nif"),
                (Role::WeaponScope, "scope.nif"),
                (Role::WeaponWorld, "weapon_world.nif"),
            ] {
                let report = selected(store, actors, assets, 0x210, role, Default::default());
                assert_eq!(
                    report["requests"][0]["path"]["raw"],
                    serde_json::json!(path.as_bytes())
                );
            }
            for role in [
                Role::WeaponModel { mod_mask: 8 },
                Role::WeaponFirstPerson { mod_mask: 255 },
            ] {
                assert!(
                    equipment::request(
                        store,
                        actors,
                        &key(0x100),
                        Choice {
                            equipment: key(0x210),
                            role
                        },
                        assets,
                        Default::default()
                    )
                    .is_err()
                );
            }
        },
    );
}

#[test]
fn unavailable_equipment_duplicate_paths_and_duplicate_first_person_links_do_not_choose_winners() {
    let directory = tempfile::tempdir().unwrap();
    fixture(
        directory.path(),
        &field(b"MOD3", b"\0"),
        &field(b"WNAM", &0x301_u32.to_le_bytes()),
    );
    with_sources(
        directory.path(),
        &["FalloutNV.esm"],
        |store, actors, assets| {
            for (id, issue) in [
                (0x777, "selected_equipment_missing"),
                (0x230, "selected_equipment_deleted"),
                (0x400, "selected_equipment_wrong_kind"),
            ] {
                let report = equipment::request(
                    store,
                    actors,
                    &key(0x100),
                    Choice {
                        equipment: key(id),
                        role: Role::ArmorBiped { sex: Sex::Male },
                    },
                    assets,
                    Default::default(),
                )
                .unwrap();
                assert_eq!(report.issues[0].code, issue);
                assert!(report.requests.is_empty());
                assert!(
                    report
                        .source_records
                        .iter()
                        .all(|source| source.record().is_none())
                );
            }
            let report = equipment::request(
                store,
                actors,
                &key(0x100),
                Choice {
                    equipment: key(0x200),
                    role: Role::ArmorBiped { sex: Sex::Female },
                },
                assets,
                Default::default(),
            )
            .unwrap();
            assert_eq!(report.requests.len(), 2);
            assert!(report.requests.iter().all(|r| r.ambiguous_source));
            assert_eq!(
                report.requests[1].path.lookup_status,
                LookupStatus::EmptySourcePath
            );
            let first = equipment::request(
                store,
                actors,
                &key(0x100),
                Choice {
                    equipment: key(0x210),
                    role: Role::WeaponFirstPerson { mod_mask: 0 },
                },
                assets,
                Default::default(),
            )
            .unwrap();
            assert_eq!(first.selected_links.len(), 2);
            assert_eq!(first.source_records.len(), 1);
            assert!(first.requests.is_empty());
            assert!(
                first
                    .selected_links
                    .iter()
                    .all(|link| link.ambiguous_source && link.target_source_index.is_none())
            );
        },
    );
}

#[test]
fn source_and_lookup_aggregate_limits_accept_exact_then_reject_one_less() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), &[], &[]);
    with_sources(
        directory.path(),
        &["FalloutNV.esm"],
        |store, actors, assets| {
            let choice = Choice {
                equipment: key(0x210),
                role: Role::WeaponFirstPerson { mod_mask: 7 },
            };
            let report = equipment::request(
                store,
                actors,
                &key(0x100),
                choice.clone(),
                assets,
                Default::default(),
            )
            .unwrap();
            let exact = Limits {
                max_sources: 2,
                max_record_bytes: report
                    .source_records
                    .iter()
                    .map(|s| {
                        s.record()
                            .unwrap()
                            .payload
                            .len()
                            .max(s.header.stored_size as usize)
                    })
                    .max()
                    .unwrap(),
                max_decoded_bytes: report.counts.decoded_bytes,
                max_fields: report.counts.fields,
                max_strings: report.counts.strings,
                max_path_bytes: report.counts.path_bytes,
                max_bindings: report.counts.bindings,
                max_requests: 1,
                max_selected_links: 1,
                max_issues: 0,
                max_visits: report.counts.visits,
                lookup: ManifestLimits {
                    max_paths: 1,
                    max_path_bytes: report.counts.lookup.path_bytes,
                    max_candidates: 1,
                    max_candidate_bytes: report.counts.lookup.candidate_bytes,
                    ..Default::default()
                },
            };
            equipment::request(store, actors, &key(0x100), choice.clone(), assets, exact).unwrap();
            for limits in [
                Limits {
                    max_sources: 1,
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
                    max_fields: exact.max_fields - 1,
                    ..exact
                },
                Limits {
                    max_strings: exact.max_strings - 1,
                    ..exact
                },
                Limits {
                    max_path_bytes: exact.max_path_bytes - 1,
                    ..exact
                },
                Limits {
                    max_bindings: exact.max_bindings - 1,
                    ..exact
                },
                Limits {
                    max_requests: 0,
                    ..exact
                },
                Limits {
                    max_selected_links: 0,
                    ..exact
                },
                Limits {
                    max_visits: exact.max_visits - 1,
                    ..exact
                },
                Limits {
                    lookup: ManifestLimits {
                        max_paths: 0,
                        ..exact.lookup
                    },
                    ..exact
                },
                Limits {
                    lookup: ManifestLimits {
                        max_path_bytes: exact.lookup.max_path_bytes - 1,
                        ..exact.lookup
                    },
                    ..exact
                },
                Limits {
                    lookup: ManifestLimits {
                        max_candidates: 0,
                        ..exact.lookup
                    },
                    ..exact
                },
                Limits {
                    lookup: ManifestLimits {
                        max_candidate_bytes: exact.lookup.max_candidate_bytes - 1,
                        ..exact.lookup
                    },
                    ..exact
                },
            ] {
                assert!(
                    equipment::request(store, actors, &key(0x100), choice.clone(), assets, limits)
                        .is_err()
                );
            }
        },
    );
}

#[test]
fn changed_actor_source_cohort_is_rejected_even_for_the_same_equipment_identity() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    fixture(left.path(), &[], &[]);
    fixture(right.path(), &field(b"UNKN", &[1]), &[]);
    with_sources(left.path(), &["FalloutNV.esm"], |_, actors, _| {
        with_sources(right.path(), &["FalloutNV.esm"], |store, _, assets| {
            assert!(
                equipment::request(
                    store,
                    actors,
                    &key(0x100),
                    Choice {
                        equipment: key(0x200),
                        role: Role::ArmorBiped { sex: Sex::Male }
                    },
                    assets,
                    Default::default()
                )
                .err()
                .unwrap()
                .to_string()
                .contains("cohort")
            );
        });
    });
}

#[test]
fn equipment_and_first_person_overrides_follow_current_winners() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), &[], &[]);
    fs::write(
        directory.path().join("Data/A.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(
                b"ARMO",
                0x200,
                0,
                15,
                &[
                    field(b"BMDT", &[0; 8]),
                    field(b"ETYP", &[0; 4]),
                    field(b"MOD3", b"override.nif\0"),
                ]
                .concat(),
            ),
            disk(
                b"STAT",
                0x300,
                plugin::DELETED | plugin::COMPRESSED,
                99,
                b"unread",
            ),
        ]
        .concat(),
    )
    .unwrap();
    with_sources(
        directory.path(),
        &["FalloutNV.esm", "A.esm"],
        |store, actors, assets| {
            let armor = equipment::request(
                store,
                actors,
                &key(0x100),
                Choice {
                    equipment: key(0x200),
                    role: Role::ArmorBiped { sex: Sex::Female },
                },
                assets,
                Default::default(),
            )
            .unwrap();
            assert_eq!(armor.source_records[0].source.plugin, "A.esm");
            assert_eq!(armor.requests[0].path.raw, b"override.nif");
            assert_eq!(
                armor.requests[0].path.lookup_status,
                LookupStatus::MissingArchiveCandidate
            );
            let male = selected(
                store,
                actors,
                assets,
                0x200,
                Role::ArmorBiped { sex: Sex::Male },
                Default::default(),
            );
            assert_eq!(male["requests"], serde_json::json!([]));
            assert_eq!(male["issues"][0]["code"], "missing_selected_model_field");
            let first = equipment::request(
                store,
                actors,
                &key(0x100),
                Choice {
                    equipment: key(0x210),
                    role: Role::WeaponFirstPerson { mod_mask: 0 },
                },
                assets,
                Default::default(),
            )
            .unwrap();
            assert_eq!(first.source_records.len(), 1);
            assert!(first.requests.is_empty());
            assert_eq!(
                first.issues[0].code,
                "selected_first_person_target_unavailable"
            );
        },
    );
}
