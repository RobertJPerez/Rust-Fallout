mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, dependencies},
    inventory, leveled, loaded_scripts, plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::stats::{AutomaticCalculation, Error, Limits, Requests},
    snapshot::Snapshot,
};
use std::{fs, path::Path};

fn disk(kind: &[u8; 4], id: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let mut result = record(kind, id, flags, body);
    result[20..22].copy_from_slice(&version.to_le_bytes());
    result
}
fn configuration(flags: u32, mask: u16) -> Vec<u8> {
    let mut bytes = [0; 24];
    bytes[..4].copy_from_slice(&flags.to_le_bytes());
    for (at, word) in [
        (4, 65534_u16),
        (6, 1),
        (8, 65535),
        (10, 1),
        (12, 65535),
        (14, 0),
    ] {
        bytes[at..at + 2].copy_from_slice(&word.to_le_bytes());
    }
    bytes[16..20].copy_from_slice(&0x7fc0_1234_u32.to_le_bytes());
    bytes[20..22].copy_from_slice(&i16::MIN.to_le_bytes());
    bytes[22..24].copy_from_slice(&mask.to_le_bytes());
    field(b"ACBS", &bytes)
}
fn npc(flags: u32, mask: u16, template: Option<u32>, repeats: usize) -> Vec<u8> {
    let mut result = Vec::new();
    for _ in 0..repeats {
        result.extend(configuration(flags, mask));
    }
    let data = [
        i32::MIN.to_le_bytes().as_slice(),
        &[0, 1, 2, 3, 254, 255, 7],
        &[0xaa; 14],
    ]
    .concat();
    let skills: Vec<_> = (0..14).chain((242..=255).rev()).collect();
    for _ in 0..repeats {
        result.extend(field(b"DATA", &data));
        result.extend(field(b"DNAM", &skills));
    }
    if let Some(template) = template {
        result.extend(field(b"TPLT", &template.to_le_bytes()));
    }
    result
}
fn fixture(path: &Path) {
    let creature = [
        configuration(0x10, 0),
        field(
            b"DATA",
            &[
                &[255, 254, 253, 252],
                i16::MIN.to_le_bytes().as_slice(),
                &[0xaa, 0xbb],
                &i16::MAX.to_le_bytes(),
                &[255, 0, 1, 2, 3, 4, 5],
            ]
            .concat(),
        ),
    ]
    .concat();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, 0, 15, &npc(0x80, 0, Some(0x200), 1)),
            disk(b"NPC_", 0x101, 0, 15, &npc(0x10, 0, None, 1)),
            disk(b"NPC_", 0x102, 0, 15, &npc(0, 2, Some(0x200), 1)),
            disk(b"NPC_", 0x103, 0, 15, &[]),
            disk(b"NPC_", 0x104, 0, 15, &npc(0x10, 0, None, 2)),
            disk(b"NPC_", 0x105, 0, 15, &npc(0, 0x11, Some(0x200), 1)),
            disk(
                b"NPC_",
                0x107,
                plugin::DELETED | plugin::COMPRESSED,
                99,
                b"unread tombstone",
            ),
            disk(b"CREA", 0x110, 0, 15, &creature),
            disk(b"NPC_", 0x200, 0, 15, &npc(0, 0, None, 1)),
            disk(b"MISC", 0x400, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn with_sources(
    path: &Path,
    callback: impl FnOnce(&World<'_>, &actors::Catalogue<'_>, &dependencies::Catalogue<'_>),
) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
    let dependencies = dependencies::Catalogue::load(
        &mut store,
        &actors,
        &associations,
        &lists,
        Default::default(),
    )
    .unwrap();
    let mut world = World::new(&scripts, WorldLimits::default()).unwrap();
    world.register_reference(None).unwrap();
    callback(&world, &actors, &dependencies);
}
fn retain(
    path: &Path,
    world: &World<'_>,
    root: u32,
    observation: &fallout_runtime::actor_rules::stats::Observation<'_>,
) {
    let Some(root_dir) = std::env::var_os("FALLOUT_ACTOR_STATS_EVIDENCE_DIR") else {
        return;
    };
    let root_dir = Path::new(&root_dir);
    assert!(root_dir.is_absolute() && root_dir.is_dir());
    let case = root_dir.join(format!("authored-stats-{root:X}"));
    fs::create_dir(&case).unwrap();
    fs::create_dir(case.join("Data")).unwrap();
    fs::copy(path.join("FalloutNV.esm"), case.join("Data/FalloutNV.esm")).unwrap();
    fs::write(case.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        case.join("snapshot.json"),
        world
            .snapshot()
            .encode(WorldLimits::default().max_snapshot_bytes)
            .unwrap(),
    )
    .unwrap();
    fs::write(
        case.join("expected.json"),
        serde_json::to_vec_pretty(observation).unwrap(),
    )
    .unwrap();
}

#[test]
fn source_extremes_and_clear_category_origins_do_not_become_current_values() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    with_sources(dir.path(), |world, actors, sources| {
        let before = world.snapshot();
        let requests =
            Requests::prepare(world, actors, sources, &form(0x100), Limits::default()).unwrap();
        let view = requests.observe(world, Limits::default()).unwrap();
        assert_eq!(view.candidate_scalars.len(), 2);
        assert_eq!(view.fields.len(), 3);
        assert_eq!(view.component_requests, 13);
        assert!(view.missing_fields.is_empty());
        assert!(
            view.current_actor_values.is_none()
                && !view.initialization_supported
                && !view.actor_reference_bound
        );
        assert!(matches!(
            view.fields[0].field.value,
            actors::fields::Value::Configuration {
                fatigue: 65534,
                barter_gold: 1,
                level_word: 65535,
                player_level_multiplier_flag: true,
                calc_min: 1,
                calc_max: 65535,
                speed_multiplier: 0,
                karma_bits: 0x7fc0_1234,
                disposition_base: i16::MIN,
            }
        ));
        assert!(
            matches!(&view.fields[1].field.value,actors::fields::Value::NpcData {
            base_health:i32::MIN, attributes:[0,1,2,3,254,255,7], unused_tail,
        } if unused_tail == &[0xaa;14])
        );
        assert!(std::ptr::eq(
            view.fields[0].field,
            &actors.get(&form(0x100)).unwrap().fields[0]
        ));
        assert!(
            view.fields
                .iter()
                .flat_map(|field| &field.components)
                .all(|component| component.root_declaration_available
                    && component.evaluated_value.is_none())
        );
        assert_eq!(view.fields[0].components[1].template_mask, 0x10);
        assert_eq!(view.fields[0].components[7].template_mask, 1);
        assert_eq!(
            view.fields[1].components[1].automatic_calculation,
            AutomaticCalculation::NpcDeclaration { enabled: false }
        );
        assert_eq!(world.snapshot(), before);
        retain(dir.path(), world, 0x100, &view);
        let bytes = before
            .encode(WorldLimits::default().max_snapshot_bytes)
            .unwrap();
        let snapshot = Snapshot::decode(&bytes, WorldLimits::default()).unwrap();
        let restored = World::restore(world.catalogue(), snapshot, WorldLimits::default()).unwrap();
        assert_eq!(
            serde_json::to_value(requests.observe(&restored, Limits::default()).unwrap()).unwrap(),
            serde_json::to_value(&view).unwrap()
        );
    });
}

#[test]
fn npc_autocalc_and_creature_swims_are_distinct_physical_declarations() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    with_sources(dir.path(), |world, actors, sources| {
        for root in [0x101, 0x110] {
            let requests =
                Requests::prepare(world, actors, sources, &form(root), Limits::default()).unwrap();
            let view = requests.observe(world, Limits::default()).unwrap();
            if root == 0x101 {
                assert!(view.fields[1].components[0].root_declaration_available);
                assert!(!view.fields[1].components[1].root_declaration_available);
                assert!(view.fields[2].components.iter().all(
                    |row| !row.root_declaration_available
                        && row.automatic_calculation
                            == AutomaticCalculation::NpcDeclaration { enabled: true }
                ));
            } else {
                assert_eq!(view.fields[1].components.len(), 7);
                assert!(matches!(
                    view.fields[1].field.value,
                    actors::fields::Value::CreatureData {
                        creature_type: 255,
                        health: i16::MIN,
                        damage: i16::MAX,
                        ..
                    }
                ));
                assert!(
                    view.fields
                        .iter()
                        .flat_map(|row| &row.components)
                        .all(|row| row.root_declaration_available
                            && row.automatic_calculation
                                == AutomaticCalculation::NotDeclaredForComponent)
                );
            }
            retain(dir.path(), world, root, &view);
        }
    });
}

#[test]
fn inherited_categories_and_absent_repeated_fields_remain_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    with_sources(dir.path(), |world, actors, sources| {
        for root in [0x102, 0x103, 0x104, 0x105] {
            let requests =
                Requests::prepare(world, actors, sources, &form(root), Limits::default()).unwrap();
            let view = requests.observe(world, Limits::default()).unwrap();
            if root == 0x102 {
                assert!(
                    view.fields[1]
                        .components
                        .iter()
                        .all(|row| !row.root_declaration_available)
                );
                assert!(view.fields[0].components[1].root_declaration_available);
                assert_eq!(view.candidate_scalars[1].definition.key, &form(0x200));
            } else if root == 0x103 {
                assert!(view.fields.is_empty());
                assert_eq!(view.missing_fields.len(), 3);
                assert!(
                    view.template_request.candidate_sources[0]
                        .configuration
                        .is_none()
                );
            } else if root == 0x104 {
                assert_eq!(view.fields.len(), 6);
                assert!(view.fields.iter().all(|row| {
                    row.ambiguous_source
                        && row
                            .components
                            .iter()
                            .all(|component| !component.root_declaration_available)
                }));
                assert_eq!(
                    view.fields[3].components[1].automatic_calculation,
                    AutomaticCalculation::ConfigurationUnavailable
                );
            } else {
                assert!(view.fields[1].components[0].root_declaration_available);
                assert!(!view.fields[0].components[1].root_declaration_available);
                assert!(!view.fields[0].components[7].root_declaration_available);
            }
            assert!(
                view.fields
                    .iter()
                    .flat_map(|row| &row.components)
                    .all(|row| row.evaluated_value.is_none())
            );
            retain(dir.path(), world, root, &view);
        }
        for root in [0x107, 0x400, 0x777] {
            assert!(matches!(
                Requests::prepare(world, actors, sources, &form(root), Limits::default()),
                Err(Error::Source(_))
            ));
        }
    });
}

#[test]
fn exact_and_one_less_limits_apply_at_prepare_and_observation() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    with_sources(dir.path(), |world, actors, sources| {
        let requests =
            Requests::prepare(world, actors, sources, &form(0x100), Limits::default()).unwrap();
        let view = requests.observe(world, Limits::default()).unwrap();
        let mut exact = Limits {
            max_scalar_fields: view.scalar_fields,
            max_decoded_bytes: view.decoded_bytes,
            max_components: view.component_requests,
            max_visits: view.preparation_visits,
            max_projection_bytes: serde_json::to_vec(&view).unwrap().len(),
            ..Limits::default()
        };
        exact.template.max_sources = view.template_request.candidate_sources.len();
        exact.template.max_links = view.template_request.links.len();
        exact.template.max_issues = view.template_request.issues.len();
        exact.template.max_field_visits = view.template_request.field_visits;
        exact.template.closure.max_nodes = view.template_request.structural_closure.nodes.len();
        exact.template.closure.max_edges =
            view.template_request.structural_closure.edge_indices.len();
        let exact_requests =
            Requests::prepare(world, actors, sources, &form(0x100), exact).unwrap();
        exact_requests.observe(world, exact).unwrap();
        for index in 0..9 {
            let mut short = exact;
            match index {
                0 => short.max_scalar_fields -= 1,
                1 => short.max_decoded_bytes -= 1,
                2 => short.max_components -= 1,
                3 => short.max_visits -= 1,
                4 => short.template.max_sources -= 1,
                5 => short.template.max_links -= 1,
                6 => short.template.max_field_visits -= 1,
                7 => short.template.closure.max_nodes -= 1,
                _ => short.template.closure.max_edges -= 1,
            }
            assert!(
                Requests::prepare(world, actors, sources, &form(0x100), short).is_err(),
                "prepare bound {index}"
            );
            assert!(
                requests.observe(world, short).is_err(),
                "observe bound {index}"
            );
        }
        exact.max_projection_bytes -= 1;
        assert!(requests.observe(world, exact).is_err());
    });
}

#[test]
fn campaign_and_changed_source_cohorts_reject_before_observation() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let changed = tempfile::tempdir().unwrap();
    fixture(changed.path());
    fs::write(changed.path().join("Other.esm"), header(&[])).unwrap();
    let mut store = RecordStore::open_nv_headers(
        changed.path(),
        &["FalloutNV.esm".into(), "Other.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let changed_scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let changed_world = World::new(&changed_scripts, WorldLimits::default()).unwrap();
    with_sources(dir.path(), |world, actors, sources| {
        let requests =
            Requests::prepare(world, actors, sources, &form(0x100), Limits::default()).unwrap();
        let other_campaign = World::new(world.catalogue(), WorldLimits::default()).unwrap();
        assert!(matches!(
            requests.observe(&other_campaign, Limits::default()),
            Err(Error::ContextChanged)
        ));
        assert!(matches!(
            requests.observe(&changed_world, Limits::default()),
            Err(Error::ContextChanged)
        ));
        assert!(matches!(
            Requests::prepare(
                &changed_world,
                actors,
                sources,
                &form(0x100),
                Limits::default()
            ),
            Err(Error::ContextChanged)
        ));
    });
}

#[test]
fn winning_override_controls_scalars_and_cycle_candidates_never_initialize() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    fs::write(
        dir.path().join("Override.esp"),
        [
            header(&["FalloutNV.esm"]),
            disk(b"NPC_", 0x100, 0, 15, &npc(0x10, 2, Some(0x200), 1)),
            disk(b"NPC_", 0x200, 0, 15, &npc(0, 2, Some(0x100), 1)),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into(), "Override.esp".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
    let sources = dependencies::Catalogue::load(
        &mut store,
        &actors,
        &associations,
        &lists,
        Default::default(),
    )
    .unwrap();
    let world = World::new(&scripts, WorldLimits::default()).unwrap();
    let requests =
        Requests::prepare(&world, &actors, &sources, &form(0x100), Limits::default()).unwrap();
    let view = requests.observe(&world, Limits::default()).unwrap();
    assert_eq!(view.actor_source.plugin, "Override.esp");
    assert_eq!(view.template_request.template_cycles, vec![vec![0, 1]]);
    assert!(
        view.template_request
            .issues
            .iter()
            .any(|issue| issue.code == "cyclic_template_dependency")
    );
    assert!(
        view.fields[1]
            .components
            .iter()
            .all(|row| !row.root_declaration_available)
    );
    assert!(!view.initialization_supported && view.current_actor_values.is_none());
    let mut short = Limits::default();
    short.template.max_issues = view.template_request.issues.len() - 1;
    assert!(Requests::prepare(&world, &actors, &sources, &form(0x100), short).is_err());
    assert!(requests.observe(&world, short).is_err());
}

#[test]
#[ignore = "Requires explicit local installed data, order and private evidence directories"]
fn installed_selected_stat_request_preserves_source_and_cold_canonical_snapshot() {
    let data = std::env::var_os("FALLOUT_NV_ACTOR_STATS_INSTALLED_DATA").unwrap();
    let order = std::env::var_os("FALLOUT_NV_ACTOR_STATS_INSTALLED_ORDER").unwrap();
    let evidence = std::env::var_os("FALLOUT_ACTOR_STATS_INSTALLED_EVIDENCE_DIR").unwrap();
    let data = Path::new(&data);
    let evidence = Path::new(&evidence);
    assert!(data.is_absolute() && evidence.is_absolute() && evidence.is_dir());
    let names: Vec<String> = serde_json::from_slice(&fs::read(order).unwrap()).unwrap();
    let mut store = RecordStore::open_nv_headers(data, &names, plugin::Limits::default()).unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
    let sources = dependencies::Catalogue::load(
        &mut store,
        &actors,
        &associations,
        &lists,
        Default::default(),
    )
    .unwrap();
    let world = World::new(&scripts, WorldLimits::default()).unwrap();
    let before = world.snapshot();
    let requests = Requests::prepare(
        &world,
        &actors,
        &sources,
        &form(0x104c0c),
        Limits::default(),
    )
    .unwrap();
    let view = requests.observe(&world, Limits::default()).unwrap();
    assert_eq!(view.fields.len(), 3);
    assert_eq!(view.component_requests, 13);
    assert!(view.current_actor_values.is_none() && !view.initialization_supported);
    let snapshot = before
        .encode(WorldLimits::default().max_snapshot_bytes)
        .unwrap();
    let restored = World::restore(
        &scripts,
        Snapshot::decode(&snapshot, WorldLimits::default()).unwrap(),
        WorldLimits::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(requests.observe(&restored, Limits::default()).unwrap()).unwrap(),
        serde_json::to_value(&view).unwrap()
    );
    assert_eq!(world.snapshot(), before);
    for name in [
        "snapshot.json",
        "expected.json",
        "order.json",
        "source-receipts.json",
    ] {
        assert!(!evidence.join(name).exists());
    }
    fs::write(evidence.join("snapshot.json"), snapshot).unwrap();
    fs::write(
        evidence.join("expected.json"),
        serde_json::to_vec_pretty(&view).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("order.json"),
        serde_json::to_vec(&names).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("source-receipts.json"),
        serde_json::to_vec_pretty(&store.source_receipts().unwrap()).unwrap(),
    )
    .unwrap();
}
