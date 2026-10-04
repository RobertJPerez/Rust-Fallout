mod common;
use common::*;
use fallout_data::{
    actors::{
        self,
        dependencies::{self, RenderRole},
        placements,
    },
    assets::ArchiveAssets,
    inventory, leveled,
    loaded_scripts::Catalogue,
    store::RecordStore,
    world::Transform,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::render_context::{self, Error, Limits, Occurrence, Outcome, Sources},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    reference_state::{Pose, State},
    snapshot::Snapshot,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15_u16.to_le_bytes());
    raw
}
fn actor(kind: &[u8; 4], duplicate: bool, marker: u8) -> Vec<u8> {
    let mut raw = [
        field(b"ACBS", &[0; 24]),
        field(
            b"DATA",
            &vec![marker; if kind == b"CREA" { 17 } else { 11 }],
        ),
        field(b"MODL", b"armor_male.nif\0"),
    ]
    .concat();
    if duplicate {
        raw.extend(field(b"MODL", b"armor_female.nif\0"));
    }
    if kind == b"NPC_" {
        raw.extend(field(b"RNAM", &0x300_u32.to_le_bytes()));
        raw.extend(field(b"PNAM", &0x310_u32.to_le_bytes()));
    }
    raw
}
fn placed(base: u32) -> Vec<u8> {
    [
        field(b"NAME", &base.to_le_bytes()),
        field(
            b"DATA",
            &[
                0x3f80_0000_u32,
                0x8000_0000,
                1,
                0x3f00_0000,
                0xbf80_0000,
                0x4000_0000,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
        ),
        field(b"XSCL", &2_f32.to_le_bytes()),
    ]
    .concat()
}
fn fixture(path: &Path, marker: u8) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let mut raw = [
        header(&[]),
        disk(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
        disk(b"CREA", 0x200, 0, &actor(b"CREA", false, marker)),
        disk(b"CREA", 0x201, 0, &actor(b"CREA", true, 0)),
        disk(b"NPC_", 0x100, 0, &actor(b"NPC_", false, 0)),
        disk(
            b"RACE",
            0x300,
            0,
            &[
                field(b"NAM1", &[]),
                field(b"MNAM", &[]),
                field(b"INDX", &0_u32.to_le_bytes()),
                field(b"MODL", b"armor_male_world.nif\0"),
            ]
            .concat(),
        ),
        disk(
            b"HDPT",
            0x310,
            0,
            &[
                field(b"MODL", b"armor_female.nif\0"),
                field(b"HNAM", &0x311_u32.to_le_bytes()),
            ]
            .concat(),
        ),
        disk(
            b"HDPT",
            0x311,
            0,
            &[
                field(b"MODL", b"armor_female_world.nif\0"),
                field(b"HNAM", &0x310_u32.to_le_bytes()),
            ]
            .concat(),
        ),
    ]
    .concat();
    for (kind, id, base) in [
        (b"ACRE", 0x500, 0x200),
        (b"ACRE", 0x501, 0x200),
        (b"ACRE", 0x502, 0x201),
        (b"ACHR", 0x503, 0x100),
        (b"ACRE", 0x504, 0x100),
        (b"ACRE", 0x505, 0x200),
        (b"ACRE", 0x506, 0x200),
    ] {
        raw.extend(disk(kind, id, 0x800, &placed(base)));
    }
    fs::write(path.join("Data/FalloutNV.esm"), raw).unwrap();
    fs::write(
        path.join("Data/A.bsa"),
        include_bytes!("../../../tools/actor-oracle/fixtures/render-context-models.bsa"),
    )
    .unwrap();
    fs::write(path.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
}
fn with_sources(path: &Path, callback: impl FnOnce(&Catalogue, &Content, Sources<'_, '_>)) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        actors::associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
    let dependencies = dependencies::Catalogue::load(
        &mut store,
        &actors,
        &associations,
        &lists,
        Default::default(),
    )
    .unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    let assets = ArchiveAssets::open_nv(path).unwrap();
    callback(
        &scripts,
        &content,
        Sources {
            placements: &placements,
            actors: &actors,
            dependencies: &dependencies,
            assets: &assets,
        },
    );
}
fn world(scripts: &Catalogue) -> World<'_> {
    World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x29; 16]).unwrap(),
    )
    .unwrap()
}
fn explicit_state(index: u8, enabled: bool, scale: Option<f32>) -> State {
    State::new(
        form(0x400),
        Pose::from_source(
            &Transform {
                position: [8192.25 + f32::from(index), -0.0, -30.5],
                rotation: [0.125, -0.75, 1.5],
            },
            scale,
        )
        .unwrap(),
        enabled,
    )
    .unwrap()
}
fn set(world: &mut World<'_>, reference: ReferenceId, state: State) {
    let view = world.reference_view(reference).unwrap();
    let stage = world.stage_reference_state(&view, state).unwrap();
    world.commit_reference_state(stage).unwrap();
}
fn occurrences(path: &Path) -> Vec<Occurrence> {
    // Actor0x200 starts after42-byte TES4 and31-byte CELL; its MODL is field2
    // after30-byte ACBS and23-byte DATA. These identities come from fixture bytes.
    vec![Occurrence {
        source: form(0x200),
        source_sha256: format!(
            "{:x}",
            Sha256::digest(fs::read(path.join("Data/FalloutNV.esm")).unwrap())
        ),
        record_file_offset: 73,
        field_index: 2,
        field_decoded_offset: 53,
        field_byte_offset: 0,
        role: RenderRole::ActorModel,
    }]
}
fn copy_sources<'a, 'b>(s: &Sources<'a, 'b>) -> Sources<'a, 'b> {
    Sources {
        placements: s.placements,
        actors: s.actors,
        dependencies: s.dependencies,
        assets: s.assets,
    }
}

#[test]
fn exact_same_base_references_keep_distinct_current_poses_enable_and_authored_data() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut world = world(scripts);
        let first = world.register_reference(Some(form(0x500))).unwrap();
        let second = world.register_reference(Some(form(0x501))).unwrap();
        set(&mut world, first, explicit_state(0, true, Some(0.75)));
        set(&mut world, second, explicit_state(8, false, Some(1.25)));
        let before = world.snapshot();
        let chosen = occurrences(temp.path());
        for (reference, expected, index) in [
            (first, Outcome::Admitted, 0),
            (second, Outcome::Disabled, 8),
        ] {
            let view = world.reference_view(reference).unwrap();
            let joined = render_context::observe(
                &world,
                content,
                copy_sources(&sources),
                &view,
                &chosen,
                Default::default(),
            )
            .unwrap();
            assert_eq!(joined.outcome(), expected);
            assert_eq!(joined.context().actor.key, &form(0x200));
            assert_eq!(
                joined.context().reference.state(),
                Some(&explicit_state(
                    index,
                    index == 0,
                    Some(if index == 0 { 0.75 } else { 1.25 })
                ))
            );
            assert_eq!(
                joined
                    .context()
                    .placement
                    .core
                    .as_ref()
                    .unwrap()
                    .position_bits,
                [0x3f80_0000, 0x8000_0000, 1]
            );
            assert_eq!(joined.selected()[0].manifest_path_index, 0);
            assert_eq!(joined.admitted_paths().count(), usize::from(index == 0));
            if index == 0 {
                assert_eq!(
                    joined.admitted_paths().next().unwrap().raw,
                    b"armor_male.nif"
                );
            }
            assert_ne!(
                joined
                    .context()
                    .reference
                    .state()
                    .unwrap()
                    .pose()
                    .source_transform()
                    .position
                    .map(f32::to_bits),
                joined
                    .context()
                    .placement
                    .core
                    .as_ref()
                    .unwrap()
                    .position_bits
            );
            let json = serde_json::to_value(&joined).unwrap();
            assert_eq!(json["gpu_ready"], false);
            assert_eq!(json["state_changed"], false);
        }
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn unavailable_component_scale_and_no_selection_have_distinct_no_mesh_outcomes() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x500))).unwrap();
        let chosen = occurrences(temp.path());
        for (state, expected) in [
            (None, Outcome::ComponentUnavailable),
            (
                Some(explicit_state(0, true, None)),
                Outcome::ScaleUnavailable,
            ),
            (Some(explicit_state(0, false, None)), Outcome::Disabled),
        ] {
            if let Some(state) = state {
                set(&mut world, reference, state);
            }
            let before = world.snapshot();
            let view = world.reference_view(reference).unwrap();
            let joined = render_context::observe(
                &world,
                content,
                copy_sources(&sources),
                &view,
                &chosen,
                Default::default(),
            )
            .unwrap();
            assert_eq!(joined.outcome(), expected);
            assert_eq!(joined.admitted_paths().count(), 0);
            assert_eq!(world.snapshot(), before);
        }
        set(&mut world, reference, explicit_state(0, true, Some(1.0)));
        let view = world.reference_view(reference).unwrap();
        let joined = render_context::observe(
            &world,
            content,
            copy_sources(&sources),
            &view,
            &[],
            Default::default(),
        )
        .unwrap();
        assert_eq!(joined.outcome(), Outcome::NoSelection);
        assert_eq!(joined.admitted_paths().count(), 0);
    });
}

#[test]
fn stale_revision_cold_private_epoch_foreign_campaign_and_changed_source_cohort_refuse() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let altered = tempfile::tempdir().unwrap();
    fixture(altered.path(), 1);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut live = world(scripts);
        let reference = live.register_reference(Some(form(0x500))).unwrap();
        set(&mut live, reference, explicit_state(0, true, Some(1.0)));
        let view = live.reference_view(reference).unwrap();
        let chosen = occurrences(temp.path());
        let before = live.snapshot();
        let cold = World::restore(
            scripts,
            Snapshot::decode(
                &before
                    .encode(WorldLimits::default().max_snapshot_bytes)
                    .unwrap(),
                WorldLimits::default(),
            )
            .unwrap(),
            WorldLimits::default(),
        )
        .unwrap();
        assert!(matches!(
            render_context::observe(
                &cold,
                content,
                copy_sources(&sources),
                &view,
                &chosen,
                Default::default()
            ),
            Err(Error::State(fallout_runtime::Error::StaleHandle))
        ));
        let fresh = cold.reference_view(reference).unwrap();
        assert_eq!(
            render_context::observe(
                &cold,
                content,
                copy_sources(&sources),
                &fresh,
                &chosen,
                Default::default()
            )
            .unwrap()
            .outcome(),
            Outcome::Admitted
        );
        let mut foreign = World::with_campaign(
            scripts,
            WorldLimits::default(),
            CampaignId::from_bytes([0x30; 16]).unwrap(),
        )
        .unwrap();
        let other = foreign.register_reference(Some(form(0x500))).unwrap();
        set(&mut foreign, other, explicit_state(0, true, Some(1.0)));
        assert_eq!(other, reference);
        assert!(matches!(
            render_context::observe(
                &foreign,
                content,
                copy_sources(&sources),
                &view,
                &chosen,
                Default::default()
            ),
            Err(Error::ContextChanged)
        ));
        with_sources(altered.path(), |_, changed_content, changed_sources| {
            let mixed = Sources {
                dependencies: changed_sources.dependencies,
                ..copy_sources(&sources)
            };
            assert!(matches!(
                render_context::observe(&live, content, mixed, &view, &chosen, Default::default()),
                Err(Error::ContextChanged)
            ));
            assert!(matches!(
                render_context::observe(
                    &live,
                    changed_content,
                    copy_sources(&sources),
                    &view,
                    &chosen,
                    Default::default()
                ),
                Err(Error::Content(_))
            ));
        });
        set(&mut live, reference, explicit_state(4, true, Some(0.5)));
        assert!(matches!(
            render_context::observe(
                &live,
                content,
                copy_sources(&sources),
                &view,
                &chosen,
                Default::default()
            ),
            Err(Error::ContextChanged)
        ));
        assert_eq!(cold.snapshot(), before);
    });
}

#[test]
fn wrong_base_offset_field_frame_hash_role_and_unknown_wire_input_refuse() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x500))).unwrap();
        set(&mut world, reference, explicit_state(0, true, Some(1.0)));
        let view = world.reference_view(reference).unwrap();
        let before = world.snapshot();
        for name in ["base", "record", "field", "offset", "frame", "hash", "role"] {
            let mut selected = occurrences(temp.path());
            match name {
                "base" => selected[0].source = form(0x201),
                "record" => selected[0].record_file_offset += 1,
                "field" => selected[0].field_index += 1,
                "offset" => selected[0].field_decoded_offset += 1,
                "frame" => selected[0].field_byte_offset += 1,
                "hash" => selected[0].source_sha256 = "0".repeat(64),
                "role" => selected[0].role = RenderRole::HeadPart,
                _ => unreachable!(),
            }
            assert!(
                render_context::observe(
                    &world,
                    content,
                    copy_sources(&sources),
                    &view,
                    &selected,
                    Default::default()
                )
                .is_err(),
                "{name}"
            );
        }
        let wrong = world.register_reference(Some(form(0x504))).unwrap();
        let view = world.reference_view(wrong).unwrap();
        assert!(
            render_context::observe(
                &world,
                content,
                copy_sources(&sources),
                &view,
                &[],
                Default::default()
            )
            .is_err()
        );
        let encoded = serde_json::to_value(&occurrences(temp.path())[0]).unwrap();
        for name in ["outer", "role"] {
            let mut bad = encoded.clone();
            if name == "outer" {
                bad["unknown"] = 1.into();
            } else {
                bad["role"]["unknown"] = 1.into();
            }
            assert!(serde_json::from_value::<Occurrence>(bad).is_err());
        }
        assert_eq!(before.reference_states, world.snapshot().reference_states);
    });
}

#[test]
fn repeated_explicit_selection_preserves_order_without_implicit_requests_or_path_copies() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x500))).unwrap();
        set(&mut world, reference, explicit_state(0, true, Some(1.0)));
        let view = world.reference_view(reference).unwrap();
        let one = occurrences(temp.path());
        let repeated = vec![one[0].clone(); 3];
        let joined = render_context::observe(
            &world,
            content,
            copy_sources(&sources),
            &view,
            &repeated,
            Default::default(),
        )
        .unwrap();
        assert_eq!(joined.outcome(), Outcome::Admitted);
        assert_eq!(joined.render().manifest.paths.len(), 1);
        assert_eq!(joined.admitted_paths().count(), 3);
        assert!(
            joined
                .selected()
                .iter()
                .all(|s| s.manifest_path_index == 0 && s.render_request_index == 0)
        );
    });
}

#[test]
fn physical_model_role_collision_and_selected_head_part_cycle_withhold_all_meshes() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut world = world(scripts);
        for (origin, base, cycle) in [(0x502, 0x201, false), (0x503, 0x100, true)] {
            let reference = world.register_reference(Some(form(origin))).unwrap();
            set(&mut world, reference, explicit_state(0, true, Some(1.0)));
            let view = world.reference_view(reference).unwrap();
            let definition = sources.dependencies.get(&form(base)).unwrap();
            let mut selected = occurrences(temp.path());
            selected[0].source = form(base);
            selected[0].record_file_offset = definition.header.offset;
            selected[0].field_decoded_offset = if cycle { 47 } else { 53 };
            let before = world.snapshot();
            let joined = render_context::observe(
                &world,
                content,
                copy_sources(&sources),
                &view,
                &selected,
                Default::default(),
            )
            .unwrap();
            assert_eq!(joined.outcome(), Outcome::SourceUnavailable);
            assert_eq!(joined.admitted_paths().count(), 0);
            if cycle {
                assert!(!joined.render().selected_source_cycles.is_empty());
            } else {
                assert!(joined.render().requests.iter().any(|r| r.ambiguous_source));
            }
            assert_eq!(world.snapshot(), before);
        }
    });
}

#[test]
fn exact_selection_identity_view_source_work_and_projection_budgets_refuse_one_less() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, sources| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x500))).unwrap();
        set(&mut world, reference, explicit_state(0, true, Some(1.0)));
        let view = world.reference_view(reference).unwrap();
        let selected = occurrences(temp.path());
        let baseline = render_context::observe(
            &world,
            content,
            copy_sources(&sources),
            &view,
            &selected,
            Default::default(),
        )
        .unwrap();
        let json = serde_json::to_value(&baseline).unwrap();
        let exact = Limits {
            max_sources: 1,
            max_selected: 1,
            max_identity_bytes: 77,
            max_visits: json["visits"].as_u64().unwrap() as usize,
            max_view_bytes: serde_json::to_vec(&view).unwrap().len(),
            max_projection_bytes: serde_json::to_vec(&baseline).unwrap().len(),
            ..Default::default()
        };
        assert!(
            render_context::observe(
                &world,
                content,
                copy_sources(&sources),
                &view,
                &selected,
                exact
            )
            .is_ok()
        );
        let before = world.snapshot();
        for name in [
            "source",
            "selection",
            "identity byte",
            "visit",
            "view byte",
            "projection byte",
        ] {
            let mut under = exact;
            match name {
                "source" => under.max_sources -= 1,
                "selection" => under.max_selected -= 1,
                "identity byte" => under.max_identity_bytes -= 1,
                "visit" => under.max_visits -= 1,
                "view byte" => under.max_view_bytes -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                _ => unreachable!(),
            }
            assert!(
                render_context::observe(
                    &world,
                    content,
                    copy_sources(&sources),
                    &view,
                    &selected,
                    under
                )
                .is_err(),
                "{name}"
            );
        }
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn export_authored_current_render_context_for_independent_actual_cli_when_requested() {
    let Some(destination) = std::env::var_os("FALLOUT_ACTOR_RENDER_CONTEXT_EVIDENCE_DIR") else {
        return;
    };
    let path = std::path::PathBuf::from(destination).join("fixture");
    assert!(!path.exists());
    fixture(&path, 0);
    with_sources(&path, |scripts, content, sources| {
        let mut live = world(scripts);
        let first = live.register_reference(Some(form(0x500))).unwrap();
        let second = live.register_reference(Some(form(0x501))).unwrap();
        let missing = live.register_reference(Some(form(0x505))).unwrap();
        let scale = live.register_reference(Some(form(0x506))).unwrap();
        let duplicate = live.register_reference(Some(form(0x502))).unwrap();
        let cycle = live.register_reference(Some(form(0x503))).unwrap();
        set(&mut live, first, explicit_state(0, true, Some(0.75)));
        set(&mut live, second, explicit_state(8, false, Some(1.25)));
        for reference in [duplicate, cycle] {
            set(&mut live, reference, explicit_state(0, true, Some(1.0)));
        }
        set(&mut live, scale, explicit_state(0, true, None));
        let snapshot = live.snapshot();
        fs::write(
            path.join("snapshot.json"),
            snapshot
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        let cold = World::restore(scripts, snapshot.clone(), WorldLimits::default()).unwrap();
        let selected = occurrences(&path);
        fs::write(
            path.join("selection.json"),
            serde_json::to_vec(&selected).unwrap(),
        )
        .unwrap();
        let mut requests = Vec::new();
        for (name, reference, base, count) in [
            ("enabled", first, 0x200, 1),
            ("disabled", second, 0x200, 1),
            ("component-unavailable", missing, 0x200, 1),
            ("scale-unavailable", scale, 0x200, 1),
            ("collision", duplicate, 0x201, 1),
            ("cycle", cycle, 0x100, 1),
            ("repeated", first, 0x200, 3),
            ("empty", first, 0x200, 0),
        ] {
            let mut choice = selected[0].clone();
            choice.source = form(base);
            choice.record_file_offset =
                sources.dependencies.get(&form(base)).unwrap().header.offset;
            choice.field_decoded_offset = if base == 0x100 { 47 } else { 53 };
            let choices = vec![choice; count];
            let selection_file = format!("{name}-selection.json");
            fs::write(
                path.join(&selection_file),
                serde_json::to_vec(&choices).unwrap(),
            )
            .unwrap();
            let view = cold.reference_view(reference).unwrap();
            let joined = render_context::observe(
                &cold,
                content,
                copy_sources(&sources),
                &view,
                &choices,
                Default::default(),
            )
            .unwrap();
            fs::write(
                path.join(format!("host-{name}.json")),
                serde_json::to_vec(&joined).unwrap(),
            )
            .unwrap();
            requests.push(serde_json::json!({"name":name,"reference":reference,"actor":format!("FalloutNV.esm:{base:X}"),"selection":selection_file,"host":format!("host-{name}.json")}));
        }
        fs::write(
            path.join("requests.json"),
            serde_json::to_vec(&serde_json::json!({"cases":requests})).unwrap(),
        )
        .unwrap();
        assert_eq!(cold.snapshot(), snapshot);
    });
}
