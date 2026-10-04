use fallout_data::{
    actors::{
        self, associations,
        dependencies::{self, Catalogue, Limits, LookupStatus, ManifestLimits, Value},
    },
    assets::ArchiveAssets,
    identity::FormKey,
    inventory, leveled, plugin,
    store::RecordStore,
};
use std::{fs, io::Write, path::Path};

fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn word(kind: &[u8; 4], raw: u32) -> Vec<u8> {
    field(kind, &raw.to_le_bytes())
}
fn disk(kind: &[u8; 4], raw: u32, flags: u32, version: u16, data: &[u8]) -> Vec<u8> {
    let body = if flags & plugin::COMPRESSED != 0 && flags & plugin::DELETED == 0 {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        [
            &(data.len() as u32).to_le_bytes()[..],
            &encoder.finish().unwrap(),
        ]
        .concat()
    } else {
        data.to_vec()
    };
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
        &[0; 4],
        &version.to_le_bytes(),
        &[0xA5, 0xCD],
        &body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    disk(b"TES4", 0, 0, 15, &body)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: fallout_data::identity::ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}
fn store(path: &Path, order: &[&str]) -> RecordStore {
    RecordStore::open_nv_headers(
        path,
        &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap()
}
fn with_catalogue<T>(
    store: &mut RecordStore,
    limits: Limits,
    run: impl FnOnce(&actors::Catalogue<'_>, &Catalogue<'_>) -> T,
) -> T {
    let inventory = inventory::Catalogue::load(store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations = associations::Catalogue::load(store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(store, Default::default()).unwrap();
    let catalogue = Catalogue::load(store, &actors, &associations, &lists, limits).unwrap();
    run(&actors, &catalogue)
}
fn dependency_error(store: &mut RecordStore, limits: Limits) -> String {
    let inventory = inventory::Catalogue::load(store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations = associations::Catalogue::load(store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(store, Default::default()).unwrap();
    Catalogue::load(store, &actors, &associations, &lists, limits)
        .err()
        .unwrap()
        .to_string()
}
fn write_single(path: &Path, kind: &[u8; 4], version: u16, data: &[u8]) {
    fs::write(
        path.join("FalloutNV.esm"),
        [header(&[]), disk(kind, 0x100, 0, version, data)].concat(),
    )
    .unwrap();
}
fn fixture(path: &Path) {
    let npc = [
        field(b"MODL", b"Actors\\Root.NIF\0"),
        word(b"TPLT", 0x101),
        field(
            b"CNTO",
            &[0x102u32.to_le_bytes().as_slice(), &(-2i32).to_le_bytes()].concat(),
        ),
        word(b"PNAM", 0x110),
        word(b"PNAM", 0x110),
        word(b"PNAM", 0x113),
        word(b"PNAM", 0x200),
        word(b"PNAM", 0x999),
        word(b"PNAM", 0),
        word(b"HNAM", 0x120),
        word(b"ENAM", 0x130),
        word(b"RNAM", 0x140),
        field(b"ZZZZ", &[0xFF, 0xCD]),
    ]
    .concat();
    let creature = [
        field(b"NIFZ", b"a.nif\0\0"),
        field(b"KFFZ", b"idle.kf\0walk.kf\0\0"),
    ]
    .concat();
    let entry = |target: u32| {
        field(
            b"LVLO",
            &[&[1, 0, 0xCD, 0xA5][..], &target.to_le_bytes()].concat(),
        )
    };
    let race = [
        field(b"NAM0", &[]),
        field(b"MNAM", &[]),
        word(b"INDX", u32::MAX),
        field(b"MODL", b"Heads\\\xE9.NIF\0"),
        field(b"ICON", b"Textures\\Head.DDS\0"),
        field(b"NAM1", &[]),
        field(b"FNAM", &[]),
        word(b"INDX", 9),
        field(b"MODL", b"Body.NIF\0"),
        field(
            b"HNAM",
            &[0x120u32.to_le_bytes().as_slice(), &0x120u32.to_le_bytes()].concat(),
        ),
        word(b"ENAM", 0x130),
        field(b"MNAM", &[]),
        word(b"INDX", 5),
        field(b"MODL", b"Unscoped.NIF\0"),
    ]
    .concat();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, plugin::COMPRESSED, 15, &npc),
            disk(b"CREA", 0x101, 0, 9, &creature),
            disk(
                b"LVLC",
                0x102,
                0,
                15,
                &[entry(0x103), entry(0x101)].concat(),
            ),
            disk(b"LVLN", 0x103, 0, 15, &entry(0x100)),
            disk(b"HDPT", 0x110, 0, 15, &word(b"HNAM", 0x111)),
            disk(b"HDPT", 0x111, 0, 15, &word(b"HNAM", 0x110)),
            disk(
                b"HDPT",
                0x113,
                plugin::DELETED | plugin::COMPRESSED,
                99,
                b"hostile unread body",
            ),
            disk(
                b"HAIR",
                0x120,
                0,
                15,
                &[field(b"MODL", b"Hair.NIF\0"), field(b"ICON", b"Hair.DDS\0")].concat(),
            ),
            disk(b"EYES", 0x130, 0, 3, &field(b"ICON", b"Eyes.DDS\0")),
            disk(b"RACE", 0x140, 0, 15, &race),
            disk(b"WEAP", 0x200, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn empty_assets(path: &Path) -> ArchiveAssets {
    fs::create_dir_all(path.join("Data")).unwrap();
    ArchiveAssets::open_nv(path).unwrap()
}

#[test]
fn physical_fields_context_and_frames_preserve_absence_legacy_bytes_and_borrowed_bodies() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    with_catalogue(&mut source, Default::default(), |actors, catalogue| {
        assert_eq!(catalogue.counts().records, 8);
        assert_eq!(catalogue.counts().deleted_records, 1);
        let npc = catalogue.get(&key(0x100)).unwrap();
        assert!(std::ptr::eq(
            npc.record().unwrap(),
            actors.get(&key(0x100)).unwrap().record().unwrap()
        ));
        assert_eq!(npc.header.trailing_bytes, [0xA5, 0xCD]);
        assert_eq!(npc.race_links[0].field_index, 11);
        assert_eq!(npc.race_links[0].binding.key, Some(key(0x140)));
        assert!(matches!(npc.fields.last().unwrap().value, Value::Opaque));
        assert_eq!(npc.fields.last().unwrap().bytes, 2);
        let creature = catalogue.get(&key(0x101)).unwrap();
        let Value::Paths { strings, .. } = &creature.fields[1].value else {
            panic!("animation strings absent")
        };
        assert_eq!(
            strings
                .iter()
                .map(|s| (s.field_byte_offset, s.raw.as_slice()))
                .collect::<Vec<_>>(),
            vec![(0, b"idle.kf".as_slice()), (8, b"walk.kf"), (16, b"")]
        );
        let race = catalogue.get(&key(0x140)).unwrap();
        let Value::Paths {
            strings, context, ..
        } = &race.fields[3].value
        else {
            panic!("race model absent")
        };
        assert_eq!(strings[0].raw, b"Heads\\\xE9.NIF");
        assert_eq!(context.region.unwrap().kind, *b"NAM0");
        assert_eq!(context.sex.unwrap().kind, *b"MNAM");
        assert_eq!(context.part.unwrap().raw_index, Some(u32::MAX));
        let Value::Paths { context, .. } = &race.fields[13].value else {
            panic!("unscoped model absent")
        };
        assert_eq!(*context, dependencies::Context::default());
        assert_eq!(race.findings.len(), 1);
        assert_eq!(race.findings[0].code, "race_path_without_part_context");
        let deleted = catalogue.get(&key(0x113)).unwrap();
        assert_eq!(deleted.header.version, 99);
        assert!(
            deleted.record().is_none()
                && deleted.fields.is_empty()
                && deleted.source.decoded_record_sha256.is_none()
        );
    });
}

#[test]
fn combined_closure_retains_occurrences_unresolved_targets_cycles_and_unbased_lists() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let assets = empty_assets(directory.path());
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    with_catalogue(&mut source, Default::default(), |_, catalogue| {
        let manifest = catalogue
            .manifest(&key(0x100), &assets, Default::default())
            .unwrap();
        assert_eq!(manifest.counts.nodes, 9);
        assert_eq!(manifest.counts.inventory_edges, 5);
        assert_eq!(manifest.counts.model_edges, 14);
        assert_eq!(
            manifest
                .cyclic_components
                .iter()
                .map(|c| c
                    .iter()
                    .map(|&i| manifest.nodes[i].local_id)
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![0x100, 0x102, 0x103], vec![0x110, 0x111]]
        );
        assert_eq!(
            manifest
                .model_edges
                .iter()
                .filter(|e| e.source == key(0x100) && e.binding.key == Some(key(0x110)))
                .count(),
            2
        );
        assert!(
            manifest
                .model_edges
                .iter()
                .any(|e| e.binding.status == inventory::Status::Missing)
        );
        assert!(
            manifest
                .model_edges
                .iter()
                .any(|e| e.binding.status == inventory::Status::Deleted)
        );
        assert!(
            manifest
                .model_edges
                .iter()
                .any(|e| e.binding.status == inventory::Status::Null)
        );
        assert!(
            manifest
                .model_edges
                .iter()
                .any(|e| e.binding.key == Some(key(0x200)) && e.schema_kind_allowed == Some(false))
        );
        assert!(
            !manifest.nodes.contains(&key(0x200))
                && !manifest.nodes.contains(&key(0x999))
                && !manifest.nodes.contains(&key(0x113))
        );
        assert_eq!(
            manifest
                .paths
                .iter()
                .filter(|p| p.lookup_status == LookupStatus::RelativeBaseUnresolved)
                .count(),
            3
        );
        assert_eq!(
            manifest
                .paths
                .iter()
                .filter(|p| p.lookup_status == LookupStatus::EmptySourcePath)
                .count(),
            2
        );
        assert!(
            manifest
                .paths
                .iter()
                .filter(|p| p.lookup_status == LookupStatus::RelativeBaseUnresolved)
                .all(|p| p.asset_path.is_none() && p.candidates.is_empty())
        );
        assert_eq!(
            manifest.paths[0].asset_path.as_ref().unwrap().bytes(),
            b"meshes/actors/root.nif"
        );
        for invalid in [key(0x999), key(0x113), key(0x120)] {
            assert!(
                catalogue
                    .manifest(&invalid, &assets, Default::default())
                    .unwrap_err()
                    .to_string()
                    .contains("root")
            );
        }
    });
}

#[test]
fn every_source_and_manifest_budget_accepts_exact_extent_then_rejects_one_less() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let assets = empty_assets(directory.path());
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    let exact = with_catalogue(&mut source, Default::default(), |_, catalogue| {
        let c = catalogue.counts();
        let g = &catalogue.inventory_graph().counts;
        Limits {
            max_records: c.records,
            max_record_bytes: catalogue
                .iter()
                .filter_map(|(_, d)| {
                    d.record()
                        .map(|r| r.payload.len().max(r.header.stored_size as usize))
                })
                .max()
                .unwrap(),
            max_decoded_bytes: c.decoded_bytes,
            max_fields: c.fields,
            max_strings: c.strings,
            max_path_bytes: c.path_bytes,
            max_bindings: c.bindings,
            max_graph_nodes: g.nodes,
            max_graph_edges: g.edges,
        }
    });
    with_catalogue(&mut source, exact, |_, catalogue| {
        let manifest = catalogue
            .manifest(&key(0x100), &assets, Default::default())
            .unwrap();
        let c = manifest.counts;
        let exact = ManifestLimits {
            max_nodes: c.nodes,
            max_edges: c.inventory_edges + c.model_edges,
            max_field_visits: c.field_visits,
            max_paths: c.paths,
            max_path_bytes: c.path_bytes,
            ..Default::default()
        };
        catalogue.manifest(&key(0x100), &assets, exact).unwrap();
        for (label, limits) in [
            (
                "node",
                ManifestLimits {
                    max_nodes: exact.max_nodes - 1,
                    ..exact
                },
            ),
            (
                "edge",
                ManifestLimits {
                    max_edges: exact.max_edges - 1,
                    ..exact
                },
            ),
            (
                "field visit",
                ManifestLimits {
                    max_field_visits: exact.max_field_visits - 1,
                    ..exact
                },
            ),
            (
                "path budget",
                ManifestLimits {
                    max_paths: exact.max_paths - 1,
                    ..exact
                },
            ),
            (
                "path byte",
                ManifestLimits {
                    max_path_bytes: exact.max_path_bytes - 1,
                    ..exact
                },
            ),
        ] {
            let error = catalogue
                .manifest(&key(0x100), &assets, limits)
                .unwrap_err()
                .to_string();
            assert!(error.contains(label), "{label}: {error}");
        }
    });
    for (label, limits) in [
        (
            "record budget",
            Limits {
                max_records: exact.max_records - 1,
                ..exact
            },
        ),
        (
            "budget",
            Limits {
                max_record_bytes: exact.max_record_bytes - 1,
                ..exact
            },
        ),
        (
            "budget",
            Limits {
                max_decoded_bytes: exact.max_decoded_bytes - 1,
                ..exact
            },
        ),
        (
            "field budget",
            Limits {
                max_fields: exact.max_fields - 1,
                ..exact
            },
        ),
        (
            "string budget",
            Limits {
                max_strings: exact.max_strings - 1,
                ..exact
            },
        ),
        (
            "path byte",
            Limits {
                max_path_bytes: exact.max_path_bytes - 1,
                ..exact
            },
        ),
        (
            "binding budget",
            Limits {
                max_bindings: exact.max_bindings - 1,
                ..exact
            },
        ),
        (
            "graph node",
            Limits {
                max_graph_nodes: exact.max_graph_nodes - 1,
                ..exact
            },
        ),
        (
            "graph edge",
            Limits {
                max_graph_edges: exact.max_graph_edges - 1,
                ..exact
            },
        ),
    ] {
        let error = dependency_error(&mut source, limits);
        assert!(error.contains(label), "{label}: {error}");
    }
}

#[test]
fn malformed_fields_and_unsupported_versions_fail_for_the_intended_source_reason() {
    let directory = tempfile::tempdir().unwrap();
    for (kind, tag, data, reason) in [
        (b"HDPT", b"MODL", b"no-nul".as_slice(), "NUL terminated"),
        (b"HDPT", b"MODL", b"a\0b\0", "embedded"),
        (b"CREA", b"NIFZ", b"a\0b", "unterminated string frame"),
        (b"RACE", b"NAM0", &[1], "needs 0 bytes"),
        (b"RACE", b"INDX", &[1, 2], "needs 4 bytes"),
        (b"RACE", b"HNAM", &[1, 2, 3], "complete 4-byte words"),
        (b"HDPT", b"HNAM", &[1, 2, 3], "needs 4 bytes"),
        (b"NPC_", b"PNAM", &[0; 8], "needs 4 bytes"),
    ] {
        write_single(directory.path(), kind, 15, &field(tag, data));
        let mut source = store(directory.path(), &["FalloutNV.esm"]);
        let error = dependency_error(&mut source, Default::default());
        assert!(error.contains(reason), "{kind:?}/{tag:?}: {error}");
    }
    for (kind, version) in [(b"HDPT", 14), (b"HAIR", 16), (b"EYES", 4)] {
        write_single(directory.path(), kind, version, &[]);
        let mut source = store(directory.path(), &["FalloutNV.esm"]);
        assert!(
            dependency_error(&mut source, Default::default()).contains("dependency record version")
        );
    }
}

#[test]
fn empty_arrays_and_duplicate_singletons_are_retained_without_synthetic_defaults() {
    let directory = tempfile::tempdir().unwrap();
    let body = [
        field(b"MODL", b"\0"),
        field(b"MODL", b"../bad\0"),
        field(b"NIFZ", &[]),
        field(b"NIFZ", b"\0\0"),
        field(b"KFFZ", &[]),
        field(b"KFFZ", b"\0"),
    ]
    .concat();
    write_single(directory.path(), b"CREA", 11, &body);
    let assets = empty_assets(directory.path());
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    with_catalogue(&mut source, Default::default(), |_, catalogue| {
        let definition = catalogue.get(&key(0x100)).unwrap();
        assert_eq!(
            definition
                .findings
                .iter()
                .map(|f| f.code)
                .collect::<Vec<_>>(),
            vec![
                "multiple_actor_model_fields",
                "multiple_actor_model_list_fields",
                "multiple_actor_animation_list_fields"
            ]
        );
        let manifest = catalogue
            .manifest(&key(0x100), &assets, Default::default())
            .unwrap();
        assert_eq!(manifest.paths.len(), 5);
        assert_eq!(
            manifest
                .paths
                .iter()
                .filter(|p| p.lookup_status == LookupStatus::EmptySourcePath)
                .count(),
            4
        );
        assert_eq!(
            manifest.paths[1].lookup_status,
            LookupStatus::UnsafeAssetPath
        );
    });
}

#[test]
fn archive_collisions_and_candidate_budgets_preserve_all_physical_mounts() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("Data")).unwrap();
    for name in ["A.bsa", "B.bsa"] {
        let mut builder = dream_archive::Tes4BsaBuilder::fallout_new_vegas();
        builder
            .add_bytes("meshes/actor.nif", b"metadata only")
            .unwrap();
        builder
            .write_path(directory.path().join("Data").join(name))
            .unwrap();
    }
    write_single(
        directory.path(),
        b"NPC_",
        15,
        &field(b"MODL", b"ACTOR.NIF\0"),
    );
    let assets = ArchiveAssets::open_nv(directory.path()).unwrap();
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    with_catalogue(&mut source, Default::default(), |_, catalogue| {
        let manifest = catalogue
            .manifest(&key(0x100), &assets, Default::default())
            .unwrap();
        assert_eq!(
            manifest.paths[0].lookup_status,
            LookupStatus::ArchiveCollision
        );
        assert_eq!(manifest.paths[0].candidates.len(), 2);
        assert!(manifest.paths[0].candidates[0].container.ends_with("A.bsa"));
        assert!(manifest.paths[0].candidates[1].container.ends_with("B.bsa"));
        let exact = ManifestLimits {
            max_candidates: 2,
            max_candidate_bytes: manifest.counts.candidate_bytes,
            ..Default::default()
        };
        catalogue.manifest(&key(0x100), &assets, exact).unwrap();
        for limits in [
            ManifestLimits {
                max_candidates: 1,
                ..exact
            },
            ManifestLimits {
                max_candidate_bytes: exact.max_candidate_bytes - 1,
                ..exact
            },
        ] {
            assert!(
                catalogue
                    .manifest(&key(0x100), &assets, limits)
                    .unwrap_err()
                    .to_string()
                    .contains("candidate")
            );
        }
    });
}

#[test]
fn a_changed_source_cohort_cannot_be_joined_even_when_counts_and_identities_match() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    write_single(left.path(), b"NPC_", 15, &field(b"MODL", b"a.nif\0"));
    write_single(right.path(), b"NPC_", 15, &field(b"MODL", b"b.nif\0"));
    let mut old = store(left.path(), &["FalloutNV.esm"]);
    let mut new = store(right.path(), &["FalloutNV.esm"]);
    let inventory = inventory::Catalogue::load(&mut old, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut old, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut old, Default::default()).unwrap();
    assert!(
        Catalogue::load(&mut new, &actors, &associations, &lists, Default::default())
            .err()
            .unwrap()
            .to_string()
            .contains("cohorts or winners differ")
    );
}

#[test]
fn hostile_declared_lengths_are_rejected_before_inflation_and_tombstones_stay_unread() {
    let directory = tempfile::tempdir().unwrap();
    let hostile = [
        &(64u32 * 1024 * 1024 + 1).to_le_bytes()[..],
        &[0x78, 0x9c, 3, 0, 0, 0, 0, 1],
    ]
    .concat();
    let mut record = disk(b"HDPT", 0x100, 0, 15, &hostile);
    record[8..12].copy_from_slice(&plugin::COMPRESSED.to_le_bytes());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), record].concat(),
    )
    .unwrap();
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    assert!(dependency_error(&mut source, Default::default()).contains("decompression budget"));
    assert!(
        dependency_error(
            &mut source,
            Limits {
                max_records: 0,
                ..Default::default()
            }
        )
        .contains("record budget")
    );
    drop(source);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(
                b"HDPT",
                0x100,
                plugin::COMPRESSED | plugin::DELETED,
                99,
                &hostile,
            ),
        ]
        .concat(),
    )
    .unwrap();
    let mut source = store(directory.path(), &["FalloutNV.esm"]);
    with_catalogue(
        &mut source,
        Limits {
            max_records: 1,
            max_record_bytes: 0,
            max_decoded_bytes: 0,
            max_fields: 0,
            max_strings: 0,
            max_path_bytes: 0,
            max_bindings: 0,
            max_graph_nodes: 0,
            max_graph_edges: 0,
        },
        |_, catalogue| {
            assert_eq!(catalogue.counts().decoded_bytes, 0);
            assert!(catalogue.get(&key(0x100)).unwrap().record().is_none());
        },
    );
}

#[test]
fn overrides_self_selectors_and_deleted_winners_survive_cold_warm_reordered_sources() {
    let directory = tempfile::tempdir().unwrap();
    let assets = empty_assets(directory.path());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            disk(
                b"NPC_",
                0x100,
                0,
                15,
                &[word(b"PNAM", 0x110), word(b"PNAM", 0x111)].concat(),
            ),
            disk(b"HDPT", 0x110, 0, 15, &field(b"MODL", b"base.nif\0")),
            disk(b"HDPT", 0x111, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("A.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(
                b"HDPT",
                0x110,
                plugin::COMPRESSED,
                15,
                &field(b"MODL", b"override.nif\0"),
            ),
            disk(
                b"HDPT",
                0x111,
                plugin::DELETED | plugin::COMPRESSED,
                99,
                b"unread bytes",
            ),
            disk(b"HDPT", 0xFF00_0112, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(
        directory.path().join("B.esm"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"HDPT", 0x0100_0112, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut observed = Vec::new();
    for (phase, order) in [
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "A.esm", "B.esm"],
        ["FalloutNV.esm", "B.esm", "A.esm"],
    ]
    .into_iter()
    .enumerate()
    {
        let mut source = RecordStore::open_nv_headers_cached(
            directory.path(),
            &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
            plugin::Limits::default(),
            cache.path(),
        )
        .unwrap();
        assert!(
            source
                .index_cache_report()
                .unwrap()
                .plugins
                .iter()
                .all(|receipt| receipt.reused == (phase != 0))
        );
        with_catalogue(&mut source, Default::default(), |_, catalogue| {
            assert_eq!(catalogue.counts().records, 5);
            assert_eq!(catalogue.get(&key(0x110)).unwrap().source.plugin, "A.esm");
            let manifest = catalogue
                .manifest(&key(0x100), &assets, Default::default())
                .unwrap();
            assert_eq!(manifest.paths[0].raw, b"override.nif");
            assert_eq!(
                manifest.model_edges[1].binding.status,
                inventory::Status::Deleted
            );
            for name in ["a.esm", "b.esm"] {
                assert!(
                    catalogue
                        .get(&FormKey {
                            origin_plugin: name.into(),
                            local_id: 0x112,
                            ..key(0x112)
                        })
                        .is_some()
                );
            }
            observed.push(serde_json::json!({"definitions":catalogue.iter().map(|(_, d)|d).collect::<Vec<_>>(), "graph":catalogue.inventory_graph(), "manifest":manifest, "digest":catalogue.winning_content_sha256()}));
        });
    }
    assert_eq!(observed[0], observed[1]);
    assert_eq!(observed[0], observed[2]);
}

#[test]
fn forensic_checksum_recovery_cannot_supply_typed_dependency_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let body = field(b"MODL", b"actor.nif\0");
    let mut compressed = disk(b"HDPT", 0x100, plugin::COMPRESSED, 15, &body);
    *compressed.last_mut().unwrap() ^= 1;
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), compressed].concat(),
    )
    .unwrap();
    let mut source = RecordStore::open_nv(
        directory.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits {
            inspect_checksum_mismatches: true,
            ..Default::default()
        },
    )
    .unwrap();
    let location = source.winner(&key(0x100)).unwrap();
    let recovered = source.read(location).unwrap();
    assert_eq!(recovered.payload, body);
    assert!(recovered.integrity_issue.is_some());
    let inventory = inventory::Catalogue::load(&mut source, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut source, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut source, Default::default()).unwrap();
    let result = Catalogue::load(
        &mut source,
        &actors,
        &associations,
        &lists,
        Default::default(),
    );
    assert!(
        result.is_err(),
        "forensic recovered body was admitted as trusted dependency inputs"
    );
    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("tainted record cannot supply actor dependency inputs")
    );
}
