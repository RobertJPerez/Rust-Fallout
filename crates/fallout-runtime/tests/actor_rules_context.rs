mod common;
use common::*;
use fallout_data::{
    actors::{self, placements},
    inventory,
    loaded_scripts::Catalogue,
    plugin,
    store::RecordStore,
    world::Transform,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::context::{self, Error, Limits},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    reference_state::{Pose, State},
    snapshot::Snapshot,
};
use std::{fs, num::NonZeroU64, path::Path};

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15_u16.to_le_bytes());
    raw
}
fn actor(kind: &[u8; 4], marker: u8) -> Vec<u8> {
    let mut config = [0; 24];
    config[4] = marker;
    [
        field(b"ACBS", &config),
        field(
            b"DATA",
            &vec![marker; if kind == b"NPC_" { 11 } else { 17 }],
        ),
    ]
    .concat()
}
fn placed(base: u32) -> Vec<u8> {
    let words = [
        0x3f800000_u32,
        0x80000000,
        1,
        0x3f000000,
        0xbf800000,
        0x40000000,
    ];
    [
        field(b"EDID", b"PlacedActor\0"),
        field(b"NAME", &base.to_le_bytes()),
        field(
            b"DATA",
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        field(b"XSCL", &1.25_f32.to_le_bytes()),
    ]
    .concat()
}
fn fixture(path: &Path, override_winners: bool, marker: u8) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let mut raw = [
        header(&[]),
        disk(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
        disk(b"NPC_", 0x100, 0, &actor(b"NPC_", marker)),
        disk(b"NPC_", 0x101, 0, &actor(b"NPC_", 8)),
        disk(b"NPC_", 0x102, plugin::DELETED, b"unread tombstone"),
        disk(b"CREA", 0x200, 0, &actor(b"CREA", 4)),
    ]
    .concat();
    for (kind, id, base, flags) in [
        (b"ACHR", 0x500, 0x100, 0x800),
        (b"ACRE", 0x501, 0x200, 0),
        (b"ACHR", 0x502, 0x100, 0),
        (b"REFR", 0x503, 0x100, 0),
        (b"ACHR", 0x504, 0x100, plugin::DELETED),
        (b"ACHR", 0x505, 0x200, 0),
        (b"ACHR", 0x506, 0x999, 0),
        (b"ACHR", 0x507, 0, 0),
        (b"ACHR", 0x508, 0x102, 0),
    ] {
        raw.extend(disk(kind, id, flags, &placed(base)));
    }
    fs::write(path.join("Data/FalloutNV.esm"), raw).unwrap();
    if override_winners {
        fs::write(
            path.join("Data/ActorPatch.esp"),
            [
                header(&["FalloutNV.esm"]),
                disk(b"NPC_", 0x100, 0, &actor(b"NPC_", 12)),
                disk(b"ACHR", 0x500, 0, &placed(0x101)),
            ]
            .concat(),
        )
        .unwrap();
    }
    let names = if override_winners {
        vec!["FalloutNV.esm", "ActorPatch.esp"]
    } else {
        vec!["FalloutNV.esm"]
    };
    fs::write(path.join("order.json"), serde_json::to_vec(&names).unwrap()).unwrap();
    fs::write(path.join("order.txt"), names.join("\n")).unwrap();
}
fn changed_state() -> State {
    State::new(
        form(0x400),
        Pose::from_source(
            &Transform {
                position: [8192.25, -0.0, -30.5],
                rotation: [0.125, -0.75, 1.5],
            },
            Some(0.75),
        )
        .unwrap(),
        false,
    )
    .unwrap()
}
fn with_sources(
    path: &Path,
    callback: impl FnOnce(&Catalogue, &Content, &placements::Catalogue, &actors::Catalogue<'_>),
) {
    let names: Vec<String> =
        serde_json::from_slice(&fs::read(path.join("order.json")).unwrap()).unwrap();
    let mut store =
        RecordStore::open_nv_headers(&path.join("Data"), &names, Default::default()).unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 1000).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    callback(&scripts, &content, &placements, &actors);
}
fn world(scripts: &Catalogue) -> World<'_> {
    World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x15; 16]).unwrap(),
    )
    .unwrap()
}

#[test]
fn npc_creature_and_shared_base_joins_preserve_source_and_missing_current_state() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), false, 2);
    with_sources(temp.path(), |scripts, content, placements, actors| {
        let mut world = world(scripts);
        for (placement, base, kind) in [
            (0x500, 0x100, *b"NPC_"),
            (0x501, 0x200, *b"CREA"),
            (0x502, 0x100, *b"NPC_"),
        ] {
            let reference = world.register_reference(Some(form(placement))).unwrap();
            let before = world.snapshot();
            let observed = context::observe(
                &world,
                content,
                placements,
                actors,
                reference,
                Default::default(),
            )
            .unwrap();
            assert_eq!(observed.reference.authored(), Some(&form(placement)));
            assert_eq!(observed.actor.key, &form(base));
            assert_eq!(observed.actor.kind, kind);
            assert_eq!(observed.placement.source.plugin, "FalloutNV.esm");
            assert_eq!(observed.actor.source.plugin, "FalloutNV.esm");
            assert_eq!(observed.base_field_index, 1);
            assert_eq!(observed.transform_field_index, 2);
            let core = observed.placement.core.as_ref().unwrap();
            assert_eq!(core.position_bits, [0x3f800000, 0x80000000, 1]);
            assert_eq!(core.scale.as_ref().unwrap().value, 1.25_f32.to_bits());
            assert!(observed.reference.state().is_none());
            assert!(
                !observed.actor_initialization_supported && !observed.authored_enable_evaluated
            );
            assert_eq!(world.snapshot(), before);
        }
    });
}

#[test]
fn override_winners_and_cold_canonical_pose_are_joined_without_resetting_authored_source() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), true, 2);
    with_sources(temp.path(), |scripts, content, placements, actors| {
        let mut live = world(scripts);
        let reference = live.register_reference(Some(form(0x500))).unwrap();
        let view = live.reference_view(reference).unwrap();
        let change = live.stage_reference_state(&view, changed_state()).unwrap();
        live.commit_reference_state(change).unwrap();
        let snapshot = live.snapshot();
        let cold = World::restore(
            scripts,
            Snapshot::decode(
                &snapshot
                    .encode(WorldLimits::default().max_snapshot_bytes)
                    .unwrap(),
                WorldLimits::default(),
            )
            .unwrap(),
            WorldLimits::default(),
        )
        .unwrap();
        let observed = context::observe(
            &cold,
            content,
            placements,
            actors,
            reference,
            Default::default(),
        )
        .unwrap();
        assert_eq!(observed.actor.key, &form(0x101));
        assert_eq!(observed.placement.source.plugin, "ActorPatch.esp");
        assert_eq!(observed.actor.source.plugin, "FalloutNV.esm");
        assert_eq!(observed.reference.state(), Some(&changed_state()));
        assert_ne!(
            observed
                .reference
                .state()
                .unwrap()
                .pose()
                .source_transform()
                .position
                .map(f32::to_bits),
            observed.placement.core.as_ref().unwrap().position_bits
        );
        assert_eq!(cold.snapshot(), snapshot);
        let reference = live.register_reference(Some(form(0x502))).unwrap();
        let observed = context::observe(
            &live,
            content,
            placements,
            actors,
            reference,
            Default::default(),
        )
        .unwrap();
        assert_eq!(observed.actor.key, &form(0x100));
        assert_eq!(observed.actor.source.plugin, "ActorPatch.esp");
        assert_eq!(observed.placement.source.plugin, "FalloutNV.esm");
    });
}

#[test]
fn unregistered_dynamic_missing_deleted_wrong_kind_and_unavailable_base_refuse_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), false, 2);
    with_sources(temp.path(), |scripts, content, placements, actors| {
        let mut world = world(scripts);
        let unknown = ReferenceId(NonZeroU64::new(999).unwrap());
        assert!(matches!(
            context::observe(
                &world,
                content,
                placements,
                actors,
                unknown,
                Default::default()
            ),
            Err(Error::State(fallout_runtime::Error::MissingReference))
        ));
        for origin in [
            None,
            Some(form(0x503)),
            Some(form(0x504)),
            Some(form(0x505)),
            Some(form(0x506)),
            Some(form(0x507)),
            Some(form(0x508)),
            Some(form(0x999)),
        ] {
            let reference = world.register_reference(origin).unwrap();
            let before = world.snapshot();
            assert!(
                context::observe(
                    &world,
                    content,
                    placements,
                    actors,
                    reference,
                    Default::default()
                )
                .is_err()
            );
            assert_eq!(world.snapshot(), before);
        }
    });
}

#[test]
fn changed_source_body_cohort_or_foreign_context_cannot_supply_the_join() {
    let first = tempfile::tempdir().unwrap();
    fixture(first.path(), false, 2);
    let changed = tempfile::tempdir().unwrap();
    fixture(changed.path(), false, 3);
    with_sources(first.path(), |scripts, content, placements, actors| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x500))).unwrap();
        let before = world.snapshot();
        with_sources(
            changed.path(),
            |_, other_content, other_placements, other_actors| {
                assert!(matches!(
                    context::observe(
                        &world,
                        content,
                        other_placements,
                        actors,
                        reference,
                        Default::default()
                    ),
                    Err(Error::ContextChanged)
                ));
                assert!(matches!(
                    context::observe(
                        &world,
                        content,
                        placements,
                        other_actors,
                        reference,
                        Default::default()
                    ),
                    Err(Error::ContextChanged)
                ));
                assert!(matches!(
                    context::observe(
                        &world,
                        other_content,
                        placements,
                        actors,
                        reference,
                        Default::default()
                    ),
                    Err(Error::Content(_))
                ));
            },
        );
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn selected_source_field_visit_and_projection_budgets_have_exact_and_one_less_limits() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), false, 2);
    with_sources(temp.path(), |scripts, content, placements, actors| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x500))).unwrap();
        let before = world.snapshot();
        let observed = context::observe(
            &world,
            content,
            placements,
            actors,
            reference,
            Default::default(),
        )
        .unwrap();
        let exact = Limits {
            max_sources: 1,
            max_fields: observed.fields,
            max_visits: observed.visits,
            max_projection_bytes: serde_json::to_vec(&observed).unwrap().len(),
        };
        assert!(context::observe(&world, content, placements, actors, reference, exact).is_ok());
        for name in ["source", "field", "visit", "projection byte"] {
            let mut under = exact;
            match name {
                "source" => under.max_sources -= 1,
                "field" => under.max_fields -= 1,
                "visit" => under.max_visits -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                _ => unreachable!(),
            }
            assert!(
                matches!(context::observe(&world,content,placements,actors,reference,under),Err(Error::Capacity(found)) if found==name)
            );
        }
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn export_exact_authored_cold_snapshot_inputs_when_requested() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_CONTEXT_EVIDENCE_DIR") else {
        return;
    };
    let destination = std::path::PathBuf::from(destination);
    fs::create_dir_all(&destination).unwrap();
    for case in ["base", "override"] {
        let path = destination.join(case);
        assert!(!path.exists());
        fixture(&path, case == "override", 2);
        with_sources(&path, |scripts, content, placements, actors| {
            let mut world = world(scripts);
            for id in [
                0x500, 0x501, 0x502, 0x503, 0x504, 0x505, 0x506, 0x507, 0x508, 0x999,
            ] {
                world.register_reference(Some(form(id))).unwrap();
            }
            world.register_reference(None).unwrap();
            let view = world
                .reference_view(ReferenceId(NonZeroU64::new(1).unwrap()))
                .unwrap();
            let stage = world.stage_reference_state(&view, changed_state()).unwrap();
            world.commit_reference_state(stage).unwrap();
            let snapshot = world.snapshot();
            fs::write(
                path.join("snapshot.json"),
                snapshot
                    .encode(WorldLimits::default().max_snapshot_bytes)
                    .unwrap(),
            )
            .unwrap();
            let cold = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
            for id in [1, 2, 3] {
                let observation = context::observe(
                    &cold,
                    content,
                    placements,
                    actors,
                    ReferenceId(NonZeroU64::new(id).unwrap()),
                    Default::default(),
                )
                .unwrap();
                fs::write(
                    path.join(format!("host-context-{id}.json")),
                    serde_json::to_vec(&observation).unwrap(),
                )
                .unwrap();
            }
            assert_eq!(cold.snapshot(), snapshot);
        });
    }
}

#[test]
#[ignore = "requires explicit private original source/order/origin and legacy snapshot; run in heavy slot"]
fn export_selected_installed_canonical_context_through_reviewed_apis() {
    let request_path = std::env::var_os("FALLOUT_ACTOR_CONTEXT_INSTALLED_REQUEST")
        .expect("explicit source request required");
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(request_path).unwrap()).unwrap();
    let data = std::path::PathBuf::from(request["data"].as_str().unwrap());
    let names: Vec<String> = serde_json::from_value(request["load_order"].clone()).unwrap();
    let origin: fallout_data::identity::FormKey =
        serde_json::from_value(request["origin"].clone()).unwrap();
    let destination = std::path::PathBuf::from(request["destination"].as_str().unwrap());
    assert!(!destination.exists());
    fs::create_dir_all(&destination).unwrap();
    let mut store = RecordStore::open_nv_headers(&data, &names, Default::default()).unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 2_000_000).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    // The caller explicitly requests the already reviewed legacy importer.
    // Never rewrite the old schema or implicitly migrate a CLI input.
    let legacy = fs::read(request["legacy_snapshot"].as_str().unwrap()).unwrap();
    let migrated = Snapshot::migrate_v3(&legacy, WorldLimits::default()).unwrap();
    let mut live = World::restore(&scripts, migrated, WorldLimits::default()).unwrap();
    let reference = live.register_reference(Some(origin)).unwrap();
    let cell: fallout_data::identity::FormKey =
        serde_json::from_value(request["cell"].clone()).unwrap();
    assert_eq!(content.source_form(&live, &cell).unwrap().kind, *b"CELL");
    for (name, with_state) in [("missing-state", false), ("explicit-state", true)] {
        if with_state {
            let view = live.reference_view(reference).unwrap();
            let explicit = State::new(cell.clone(), changed_state().pose().clone(), false).unwrap();
            let stage = live.stage_reference_state(&view, explicit).unwrap();
            live.commit_reference_state(stage).unwrap();
        }
        let snapshot = live.snapshot();
        fs::write(
            destination.join(format!("{name}-snapshot.json")),
            snapshot
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        let cold = World::restore(&scripts, snapshot.clone(), WorldLimits::default()).unwrap();
        let observed = context::observe(
            &cold,
            &content,
            &placements,
            &actors,
            reference,
            Default::default(),
        )
        .unwrap();
        assert_eq!(observed.reference.state().is_some(), with_state);
        fs::write(
            destination.join(format!("{name}-host.json")),
            serde_json::to_vec(&observed).unwrap(),
        )
        .unwrap();
        assert_eq!(cold.snapshot(), snapshot);
    }
    fs::write(
        destination.join("reference.json"),
        serde_json::to_vec(&reference).unwrap(),
    )
    .unwrap();
}
