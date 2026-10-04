mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    plugin,
    store::RecordStore,
    world::{self, Transform},
};
use fallout_runtime::{
    Error, Limits, World,
    events::Context,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    reference_state::{Pose, State},
    save::{Captured, Recovery, Repository, SaveStatus, SaveWorker, format},
    snapshot::Snapshot,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

fn fixture(path: &Path) {
    fs::create_dir_all(path).unwrap();
    // Authored exact DATA floats: signed zero and finite subnormal survive.
    let data = [
        0x3f800000_u32,
        0x80000000,
        1,
        0x3f000000,
        0xbf800000,
        0x40000000,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
            record(b"ACTI", 0x100, 0, &[]),
            record(
                b"REFR",
                0x500,
                0,
                &[
                    field(b"NAME", &0x100_u32.to_le_bytes()),
                    field(b"DATA", &data),
                ]
                .concat(),
            ),
            record(b"SCPT", 0x300, 0, &unit(&[(90, 0)], &[(b"SCRV", 90)])),
        ]
        .concat(),
    )
    .unwrap();
}
fn source(path: &Path) -> (Catalogue, Pose) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(
        &mut store,
        fallout_data::loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    let cell = store.winner(&form(0x400)).unwrap();
    world::decode_cell(&store.read(cell).unwrap(), "FalloutNV.esm").unwrap();
    let placed = store.winner(&form(0x500)).unwrap();
    let placement = world::decode_placement(&store.read(placed).unwrap(), "FalloutNV.esm").unwrap();
    let pose = Pose::from_source(
        &placement.transform.value,
        placement.scale.map(|field| field.value),
    )
    .unwrap();
    (catalogue, pose)
}
// Named headless cell consumer: unload drops the observation/source resources,
// while reload resolves the same source key and uses already retained state.
// Enable is explicit caller input; source enable-parent semantics are not guessed.
fn resident(world: &mut World<'_>, pose: Pose, explicit_enable: bool) -> ReferenceId {
    let reference = world
        .authored_reference(&form(0x500))
        .unwrap_or_else(|| world.register_reference(Some(form(0x500))).unwrap());
    let view = world.reference_view(reference).unwrap();
    if view.state().is_none() {
        let proposal = world
            .stage_reference_state(
                &view,
                State::new(form(0x400), pose, explicit_enable).unwrap(),
            )
            .unwrap();
        world.commit_reference_state(proposal).unwrap();
    }
    reference
}
fn changed() -> State {
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
fn exact_world<'a>(catalogue: &'a Catalogue) -> World<'a> {
    World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x76; 16]).unwrap(),
    )
    .unwrap()
}

#[test]
fn source_identity_survives_unload_and_reload_without_resetting_explicit_state() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, pose) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = resident(&mut world, pose.clone(), true);
    let view = world.reference_view(id).unwrap();
    assert_eq!(view.authored(), Some(&form(0x500)));
    assert_eq!(view.state().unwrap().pose(), &pose);
    assert!(view.state().unwrap().enabled());
    assert!(pose.source_scale().is_none());
    assert_eq!(
        pose.source_transform().position.map(f32::to_bits),
        [0x3f800000, 0x80000000, 1]
    );
    let before = world.snapshot();
    let proposal = world.stage_reference_state(&view, changed()).unwrap();
    assert_eq!(world.snapshot(), before); // staging alone has no effects
    let receipt = world.commit_reference_state(proposal).unwrap();
    assert_eq!(receipt.before_revision, before.state_revision);
    assert_eq!(receipt.after_revision, before.state_revision + 1);
    drop(view);
    let changed_snapshot = world.snapshot();
    assert_eq!(resident(&mut world, pose, true), id);
    assert_eq!(world.snapshot(), changed_snapshot);
    assert_eq!(world.reference_view(id).unwrap().state(), Some(&changed()));
    // The existing shared coordinates adapter consumes the explicit source pose.
    let state = changed();
    fallout_data::coordinates::Affine::nv_reference(
        &state.pose().source_transform(),
        state.pose().source_scale().unwrap(),
    )
    .unwrap();
}

#[test]
fn revisions_epochs_and_missing_identity_refuse_before_any_mutation() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, pose) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = resident(&mut world, pose, true);
    let view = world.reference_view(id).unwrap();
    let winner = world.stage_reference_state(&view, changed()).unwrap();
    let loser = world.stage_reference_state(&view, changed()).unwrap();
    world.commit_reference_state(winner).unwrap();
    let exact = world.snapshot();
    assert!(world.commit_reference_state(loser).is_err());
    assert!(world.stage_reference_state(&view, changed()).is_err());
    assert_eq!(world.snapshot(), exact);
    let current = world.reference_view(id).unwrap();
    let staged = world.stage_reference_state(&current, changed()).unwrap();
    world.replace_from_snapshot(exact.clone()).unwrap();
    assert!(matches!(
        world.commit_reference_state(staged),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), exact);
    let other = exact_world(&catalogue);
    assert!(matches!(
        other.stage_reference_state(&current, changed()),
        Err(Error::StaleHandle)
    ));
    assert!(matches!(
        world.reference_view(ReferenceId(999.try_into().unwrap())),
        Err(Error::MissingReference)
    ));
}

#[test]
fn invalid_pose_scale_and_component_restore_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, pose) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = resident(&mut world, pose, true);
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in 0..6 {
            let mut transform = Transform {
                position: [0.0; 3],
                rotation: [0.0; 3],
            };
            if axis < 3 {
                transform.position[axis] = value
            } else {
                transform.rotation[axis - 3] = value
            }
            assert!(Pose::from_source(&transform, None).is_err());
        }
    }
    for scale in [f32::NAN, f32::INFINITY, -1.0, 0.0, -0.0] {
        assert!(
            Pose::from_source(
                &Transform {
                    position: [0.0; 3],
                    rotation: [0.0; 3]
                },
                Some(scale)
            )
            .is_err()
        );
    }
    let exact = world.snapshot();
    for (field, value) in [
        ("schema_version", json!(2)),
        ("cell", json!(form(0x01000000))),
        (
            "pose",
            json!({"position_bits":[0x7f800000_u32,0,0],"rotation_bits":[0,0,0],"scale_bits":null}),
        ),
    ] {
        let mut dto = serde_json::to_value(&exact).unwrap();
        dto["reference_states"][0]["state"][field] = value;
        let invalid: Snapshot = serde_json::from_value(dto).unwrap();
        assert!(world.replace_from_snapshot(invalid).is_err());
        assert_eq!(world.snapshot(), exact);
    }
    let mut duplicate = exact.clone();
    duplicate
        .reference_states
        .push(duplicate.reference_states[0].clone());
    assert!(world.replace_from_snapshot(duplicate).is_err());
    let mut missing = exact.clone();
    missing.reference_states[0].id = ReferenceId(999.try_into().unwrap());
    assert!(world.replace_from_snapshot(missing).is_err());
    assert_eq!(world.snapshot(), exact);
    assert_eq!(
        world.reference_view(id).unwrap().revision(),
        exact.state_revision
    );
}

#[test]
fn unavailable_state_and_scale_are_not_fabricated_and_admission_precedes_allocation() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, _) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = world.register_reference(Some(form(0x500))).unwrap();
    assert!(world.reference_view(id).unwrap().state().is_none());
    let state = State::new(
        form(0x400),
        Pose::from_source(
            &Transform {
                position: [0.0; 3],
                rotation: [0.0; 3],
            },
            None,
        )
        .unwrap(),
        false,
    )
    .unwrap();
    let stage = world
        .stage_reference_state(&world.reference_view(id).unwrap(), state)
        .unwrap();
    drop(stage);
    assert!(world.snapshot().reference_states.is_empty());
    let bytes = serde_json::to_vec(&json!({"reference_states":[null,null]})).unwrap();
    let limits = Limits {
        max_references: 1,
        ..Limits::default()
    };
    assert!(matches!(
        Snapshot::decode(&bytes, limits),
        Err(Error::Capacity("saved reference states"))
    ));
    let positional = serde_json::to_vec(&json!([
        null,
        null,
        null,
        null,
        null,
        null,
        [],
        null,
        null,
        null,
        null,
        [],
        [],
        [],
        [null, null]
    ]))
    .unwrap();
    assert!(matches!(
        Snapshot::decode(&positional, limits),
        Err(Error::Capacity("saved reference states"))
    ));
    world
        .commit_reference_state(
            world
                .stage_reference_state(&world.reference_view(id).unwrap(), changed())
                .unwrap(),
        )
        .unwrap();
    assert!(Snapshot::decode(&world.snapshot().encode(1 << 20).unwrap(), limits).is_ok());
}

fn legacy_envelope(snapshot: &Snapshot, body: &[u8], schema: u32) -> Vec<u8> {
    let mut meta = Vec::new();
    meta.extend(1_u32.to_le_bytes());
    meta.extend(schema.to_le_bytes());
    meta.extend(7_u64.to_le_bytes());
    meta.extend(snapshot.clocks.tick.to_le_bytes());
    for i in (0..64).step_by(2) {
        meta.push(u8::from_str_radix(&snapshot.catalogue_sha256[i..i + 2], 16).unwrap());
    }
    meta.extend((body.len() as u64).to_le_bytes());
    meta.extend(snapshot.campaign.bytes());
    meta.extend(snapshot.state_revision.to_le_bytes());
    let mut out = b"FRSAVE01".to_vec();
    out.extend(1_u16.to_le_bytes());
    out.extend(0_u16.to_le_bytes());
    out.extend(2_u32.to_le_bytes());
    for (tag, payload) in [(b"META", meta.as_slice()), (b"STAT", body)] {
        out.extend(tag);
        out.extend(1_u32.to_le_bytes());
        out.extend((payload.len() as u64).to_le_bytes());
        out.extend(Sha256::digest(payload));
        out.extend(payload);
    }
    out.extend(Sha256::digest(&out));
    out
}

#[test]
fn rehashed_invalid_component_cannot_rotate_valid_previous() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, pose) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = resident(&mut world, pose, true);
    let repository =
        Repository::create(&dir.path().join("rotation"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    world
        .commit_reference_state(
            world
                .stage_reference_state(&world.reference_view(id).unwrap(), changed())
                .unwrap(),
        )
        .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    let snapshot = world.snapshot();
    for case in 0..3 {
        let mut forged = serde_json::to_value(&snapshot).unwrap();
        match case {
            0 => {
                forged["reference_states"][0]["state"]["pose"]["position_bits"][1] =
                    json!(0x7f800000_u32)
            }
            1 => {
                let row = forged["reference_states"][0].clone();
                forged["reference_states"].as_array_mut().unwrap().push(row);
            }
            _ => forged["reference_states"][0]["id"] = json!(999),
        }
        let forged = legacy_envelope(&snapshot, &serde_json::to_vec(&forged).unwrap(), 4);
        // Integrity and shape pass, so refusal proves semantic validation.
        assert!(format::decode(&forged, Limits::default()).is_ok());
        fs::write(repository.path().join("current.frsv"), &forged).unwrap();
        let mut stages = 0;
        assert!(
            repository
                .commit_observing(&Captured::at_boundary(&world), |_| {
                    stages += 1;
                })
                .is_err()
        );
        assert_eq!(stages, 0);
        assert_eq!(
            fs::read(repository.path().join("current.frsv")).unwrap(),
            forged
        );
        assert_eq!(
            fs::read(repository.path().join("previous.frsv")).unwrap(),
            previous
        );
    }
}

#[test]
fn exhausted_revision_cannot_partly_commit_reference_state() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, pose) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = resident(&mut world, pose, true);
    let mut exhausted = world.snapshot();
    exhausted.state_revision = u64::MAX;
    world.replace_from_snapshot(exhausted.clone()).unwrap();
    let proposal = world
        .stage_reference_state(&world.reference_view(id).unwrap(), changed())
        .unwrap();
    assert!(matches!(
        world.commit_reference_state(proposal),
        Err(Error::Capacity("state revisions"))
    ));
    assert_eq!(world.snapshot(), exhausted);
}

#[test]
fn explicit_schema_three_migration_keeps_inventory_and_absent_reference_state() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path());
    let (catalogue, _) = source(dir.path());
    let mut world = exact_world(&catalogue);
    let id = world.register_reference(Some(form(0x500))).unwrap();
    world.initialize_inventory(id).unwrap();
    world
        .add_item(
            id,
            fallout_runtime::inventory::Facts::unknown(form(0x100)),
            3.try_into().unwrap(),
        )
        .unwrap();
    let expected = world.snapshot();
    let mut legacy = serde_json::to_value(&expected).unwrap();
    legacy.as_object_mut().unwrap().remove("reference_states");
    legacy["schema_version"] = 3.into();
    let body = serde_json::to_vec(&legacy).unwrap();
    let container = legacy_envelope(&expected, &body, 3);
    assert!(format::decode(&container, Limits::default()).is_err());
    let imported = format::migrate_v3(&container, Limits::default()).unwrap();
    assert_eq!(imported.source_state_schema, 3);
    assert_eq!(imported.source_metadata.generation, 7);
    assert_eq!(
        imported.source_metadata.container_sha256,
        format!("{:x}", Sha256::digest(&container))
    );
    assert_eq!(imported.snapshot, expected);
    let restored = World::restore(&catalogue, imported.snapshot, Limits::default()).unwrap();
    assert!(restored.reference_view(id).unwrap().state().is_none());
    assert_eq!(restored.inventory_count(id, &form(0x100)).unwrap(), 3);
    let repository =
        Repository::create(&dir.path().join("migrated"), &[], world.campaign()).unwrap();
    repository
        .commit(&Captured::at_boundary(&restored))
        .unwrap();
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected
    );
    legacy["reference_states"] = json!([]);
    assert!(
        Snapshot::migrate_v3(&serde_json::to_vec(&legacy).unwrap(), Limits::default()).is_err()
    );
    legacy.as_object_mut().unwrap().remove("reference_states");
    let body = serde_json::to_string(&legacy).unwrap();
    let duplicate = format!("{{\"schema_version\":3,{}", &body[1..]);
    assert!(Snapshot::migrate_v3(duplicate.as_bytes(), Limits::default()).is_err());
    let mut positional = serde_json::to_value(&expected).unwrap();
    let mut map = positional.as_object_mut().unwrap().clone();
    map.remove("reference_states");
    map.insert("schema_version".into(), 3.into());
    positional = json!(
        [
            "schema_version",
            "campaign",
            "state_revision",
            "profile",
            "catalogue_sha256",
            "next_item",
            "inventory_banks",
            "next_instance",
            "next_reference",
            "next_event_sequence",
            "clocks",
            "references",
            "instances",
            "pending_events"
        ]
        .map(|field| map.remove(field).unwrap())
    );
    assert_eq!(
        Snapshot::migrate_v3(&serde_json::to_vec(&positional).unwrap(), Limits::default()).unwrap(),
        expected
    );
}

#[test]
fn worker_capture_and_fresh_process_restore_keep_source_pose_enable_and_shared_links() {
    let temp = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("FALLOUT_REFERENCE_PROOF_ROOT").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fixture(root);
    let (catalogue, pose) = source(root);
    let source_bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    let mut world = exact_world(&catalogue);
    let id = resident(&mut world, pose.clone(), true);
    let context = Context {
        calling_reference: Some(id),
        ..Context::default()
    };
    let h = world
        .create_instance(
            &definition(&catalogue),
            Owner::Placed { reference: id },
            context,
        )
        .unwrap();
    world
        .assign(
            h,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id },
                },
            )],
        )
        .unwrap();
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = world.snapshot();
    let before = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    world
        .commit_reference_state(
            world
                .stage_reference_state(&world.reference_view(id).unwrap(), changed())
                .unwrap(),
        )
        .unwrap();
    assert_eq!(before.wait().unwrap().metadata.generation, 1);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        first
    );
    let second = world.snapshot();
    let done = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    worker.finish().unwrap();
    assert_eq!(done.wait().unwrap().metadata.generation, 2);
    assert_eq!(
        format::decode(
            &fs::read(repository.path().join("previous.frsv")).unwrap(),
            Limits::default()
        )
        .unwrap()
        .snapshot,
        first
    );
    fs::write(root.join("expected.json"), second.encode(1 << 20).unwrap()).unwrap();
    drop(world);
    drop(catalogue);
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_reference_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_REFERENCE_COLD_ROOT", root)
        .output()
        .unwrap();
    fs::write(root.join("cold.stdout.txt"), &result.stdout).unwrap();
    fs::write(root.join("cold.stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source_bytes);
}

#[test]
#[ignore = "fresh child process helper selected explicitly by the parent"]
fn cold_reference_helper() {
    let root = std::path::PathBuf::from(std::env::var_os("FALLOUT_REFERENCE_COLD_ROOT").unwrap());
    let (catalogue, source_pose) = source(&root);
    let repository = Repository::open(&root.join("saved"), &[]).unwrap();
    let (mut world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join("expected.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    assert_eq!(receipt.metadata.generation, 2);
    let id = resident(&mut world, source_pose, true);
    assert_eq!(world.snapshot(), expected);
    assert_eq!(world.reference_view(id).unwrap().state(), Some(&changed()));
    let instance = &expected.instances[0];
    let live = world.instance(world.handle(instance.id).unwrap()).unwrap();
    assert_eq!(live.context().calling_reference, Some(id));
    assert_eq!(
        live.local(90).unwrap(),
        &Value::Reference {
            value: ReferenceValue::Live { id }
        }
    );
    println!(
        "cold source-selected identity, pose, enable, exact snapshot and links restored before consumer; no callback or retail parity claim"
    );
}
