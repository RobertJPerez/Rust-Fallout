mod common;
use common::{field, form, header, record};
use fallout_data::{
    archive::NvArchive,
    coordinates::Affine,
    loaded_scripts::Catalogue,
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{
        preparation::CellModelPlan,
        residency::{self, CellResidency, Stage},
    },
};
use fallout_runtime::{
    World,
    events::Clocks,
    identity::CampaignId,
    physics::{
        reference::{self, ReferenceCollision, ReferencePlacement},
        *,
    },
    reference_state::{Pose, State},
    save::{Captured, Recovery, Repository},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

fn group(kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn nif() -> Vec<u8> {
    let mut body = vec![0; 236];
    body[..4].copy_from_slice(&1u32.to_le_bytes());
    body[80..84].copy_from_slice(&1f32.to_le_bytes());
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    b.extend(0x14020007u32.to_le_bytes());
    b.push(1);
    for value in [11u32, 2, 34] {
        b.extend(value.to_le_bytes());
    }
    b.extend([0; 3]);
    b.extend(2u16.to_le_bytes());
    for name in ["bhkRigidBody", "bhkSphereShape"] {
        b.extend((name.len() as u32).to_le_bytes());
        b.extend(name.as_bytes());
    }
    b.extend(0u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    for value in [236u32, 8, 0, 0, 0] {
        b.extend(value.to_le_bytes());
    }
    b.extend(body);
    b.extend(17u32.to_le_bytes());
    b.extend(1f32.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b
}
struct Fixture {
    plan: CellModelPlan,
    catalogue: Arc<Catalogue>,
    source: tempfile::TempDir,
    cache: tempfile::TempDir,
    save: tempfile::TempDir,
    model: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::try_new_mode(0).unwrap()
    }
    fn try_new_mode(mode: usize) -> fallout_data::Result<Self> {
        let source = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let save = tempfile::tempdir().unwrap();
        let model = nif();
        let name = b"shape.nif\0";
        let offset = 76 + name.len();
        let mut bsa = vec![0; offset];
        bsa[..4].copy_from_slice(b"BSA\0");
        for (at, value) in [
            (4, 104u32),
            (8, 36),
            (12, 3),
            (16, 1),
            (20, 1),
            (24, 7),
            (28, name.len() as u32),
            (44, 1),
            (48, 52),
            (68, model.len() as u32),
            (72, offset as u32),
        ] {
            bsa[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bsa[52] = 7;
        bsa[53..60].copy_from_slice(b"meshes\0");
        bsa[60..68].copy_from_slice(&1u64.to_le_bytes());
        bsa[76..].copy_from_slice(name);
        bsa.extend(&model);
        let archive = source.path().join("models.bsa");
        fs::write(&archive, bsa).unwrap();
        let mut mounts = MountIndex::default();
        NvArchive::open(&archive)
            .unwrap()
            .census(&mut mounts)
            .unwrap();
        let mut members = Vec::new();
        for (id, x) in [(0x300, 33f32), (0x301, 44f32)] {
            let data = [x, 0., 0., 0., 0., 0.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            let kind = if id == 0x300 && mode == 2 {
                b"ACHR"
            } else if id == 0x300 && mode == 5 {
                b"STAT"
            } else {
                b"REFR"
            };
            let base = if id == 0x300 && mode == 3 {
                0x999u32
            } else {
                0x400u32
            };
            let mut payload = [
                field(b"NAME", &base.to_le_bytes()),
                field(b"DATA", &data),
                field(b"XSCL", &1f32.to_le_bytes()),
            ]
            .concat();
            if id == 0x300 && mode == 6 {
                payload.extend(field(b"NAME", &base.to_le_bytes()));
            }
            members.extend(record(
                kind,
                id,
                if id == 0x300 && mode == 1 {
                    plugin::DELETED
                } else {
                    0
                },
                &payload,
            ));
        }
        let esm = [
            header(&[]),
            record(
                b"STAT",
                0x400,
                0,
                &if mode == 4 {
                    Vec::new()
                } else {
                    field(b"MODL", b"shape.nif\0")
                },
            ),
            record(
                b"CELL",
                0x200,
                0,
                &[
                    field(b"EDID", b"ReferenceCollisionFixture\0"),
                    field(b"DATA", &[1]),
                ]
                .concat(),
            ),
            group(6, &group(9, &members)),
        ]
        .concat();
        fs::write(source.path().join("FalloutNV.esm"), esm).unwrap();
        let mut store = RecordStore::open_nv_headers(
            source.path(),
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap();
        let catalogue =
            Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
        let plan = CellModelPlan::load(&mut store, &form(0x200), &mounts, Default::default())?;
        Ok(Self {
            plan,
            catalogue,
            source,
            cache,
            save,
            model,
        })
    }
    fn store(&self) -> RecordStore {
        RecordStore::open_nv_headers(
            self.source.path(),
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap()
    }
    fn owner(&self) -> CellResidency {
        CellResidency::new(
            self.source.path(),
            Some(self.cache.path()),
            residency::Limits {
                workers: 1,
                models: 1,
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn canonical(&self) -> World<'static> {
        let mut world = World::with_campaign(
            Arc::clone(&self.catalogue),
            fallout_runtime::Limits::default(),
            CampaignId::from_bytes([8; 16]).unwrap(),
        )
        .unwrap();
        let mut snapshot = world.snapshot();
        snapshot.next_reference = 0xf000_0000_0000_0010;
        world = World::restore(
            Arc::clone(&self.catalogue),
            snapshot,
            fallout_runtime::Limits::default(),
        )
        .unwrap();
        for (key, x) in [(form(0x300), 100f32), (form(0x301), 200f32)] {
            let id = world.register_reference(Some(key)).unwrap();
            let pose = Pose::from_source(
                &fallout_data::world::Transform {
                    position: [x, 0., 0.],
                    rotation: [0.; 3],
                },
                Some(1.),
            )
            .unwrap();
            let stage = world
                .stage_reference_state(
                    &world.reference_view(id).unwrap(),
                    State::new(form(0x200), pose, true).unwrap(),
                )
                .unwrap();
            world.commit_reference_state(stage).unwrap();
        }
        world
    }
    fn placements(&self, world: &World<'_>) -> Vec<ReferencePlacement> {
        [(form(0x300), 0.), (form(0x301), 10.)]
            .into_iter()
            .map(|(authored, x)| ReferencePlacement {
                body: BodyPlacement {
                    reference: world.authored_reference(&authored).unwrap(),
                    source_sha256: Sha256::digest(&self.model).into(),
                    body_block: 0,
                    attachment_to_source: Affine {
                        rows: [[1., 0., 0., x], [0., 1., 0., 0.], [0., 0., 1., 0.]],
                    },
                },
                authored,
            })
            .collect()
    }
}
fn decoded(owner: &mut CellResidency) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = owner.poll().unwrap();
        if snapshot.stage == Stage::Decoded {
            break;
        }
        assert_ne!(snapshot.stage, Stage::Failed);
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn units() -> EngineeringUnits {
    EngineeringUnits {
        havok_to_source: 1.,
        source_to_query: 1.,
        transform_tolerance: 1e-6,
    }
}
fn ray() -> Ray {
    Ray {
        origin: [-3., 0., 0.],
        direction: [1., 0., 0.],
        max_distance: 20.,
    }
}

#[test]
fn strict_saved_world_joins_two_high_ids_without_converting_source_or_saved_pose() {
    let fixture = Fixture::new();
    let original = fixture.canonical();
    let expected = original.snapshot();
    let root = fixture.save.path().join("native");
    let repository =
        Repository::create(&root, &[fixture.source.path().into()], original.campaign()).unwrap();
    repository
        .commit(&Captured::at_boundary(&original))
        .unwrap();
    drop(original);
    let (world, receipt) = Repository::open(&root, &[fixture.source.path().into()])
        .unwrap()
        .load(
            Arc::clone(&fixture.catalogue),
            fallout_runtime::Limits::default(),
            Recovery::Strict,
        )
        .unwrap();
    assert!(!receipt.current_repaired);
    assert_eq!(world.snapshot(), expected);
    let mut store = fixture.store();
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut collision = ReferenceCollision::default();
    let scope = collision
        .admit(
            &world,
            &mut owner,
            &ticket,
            &mut store,
            0,
            &fixture.placements(&world),
            units(),
            reference::Limits::default(),
        )
        .unwrap();
    assert_eq!(scope.campaign, world.campaign());
    assert_eq!(scope.revision, world.revision());
    assert_eq!(scope.catalogue_sha256, world.catalogue_fingerprint());
    assert_eq!(scope.bindings.len(), 2);
    #[cfg(windows)]
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.source.path().join("FalloutNV.esm"))
            .is_err(),
        "retained protected plugin handles must deny writes while queries remain usable"
    );
    assert_eq!(
        scope.usage.source_bytes,
        fs::metadata(fixture.source.path().join("FalloutNV.esm"))
            .unwrap()
            .len()
    );
    assert_eq!(
        scope.usage.decoded_bytes,
        fixture.plan.receipt().usage.record_decoded_bytes + fixture.model.len() + 132
    );
    assert_eq!(
        scope.usage.field_sites,
        fixture.plan.receipt().usage.field_sites + 8
    );
    let result = collision
        .ray_cast(&world, &owner, ray(), reference::Budget::default())
        .unwrap();
    let hits = result.hits(&world, &owner).unwrap();
    assert_eq!(
        hits.iter().map(|h| h.distance).collect::<Vec<_>>(),
        vec![2., 12.]
    );
    assert_eq!(
        hits.iter()
            .map(|h| h.source.reference.0.get())
            .collect::<Vec<_>>(),
        vec![0xf000_0000_0000_0010, 0xf000_0000_0000_0011]
    );
    for binding in result.scope(&world, &owner).unwrap().bindings.iter() {
        assert_eq!(binding.base.key, form(0x400));
        assert_eq!(binding.model_path.bytes(), b"meshes/shape.nif");
        assert_eq!(binding.name_decoded_offset, 0);
        assert_eq!(
            binding.placed.source_sha256,
            format!(
                "{:x}",
                Sha256::digest(fs::read(fixture.source.path().join("FalloutNV.esm")).unwrap())
            )
        );
        let saved = world.reference_view(binding.reference).unwrap();
        assert_ne!(
            f64::from(saved.state().unwrap().pose().source_transform().position[0]),
            binding.attachment_rows[0][3]
        );
    }
    let overlaps = collision
        .overlap_sphere(&world, &owner, [0.; 3], 0., reference::Budget::default())
        .unwrap();
    assert_eq!(overlaps.hits(&world, &owner).unwrap().len(), 1);
    assert_eq!(world.snapshot(), expected);
    assert_eq!(
        owner.snapshot().collision,
        residency::Readiness::Unsupported
    );
    assert!(!owner.snapshot().simulation_ready);
}

#[test]
fn revision_equal_state_restore_and_foreign_owner_revoke_opaque_hits() {
    let fixture = Fixture::new();
    let mut world = fixture.canonical();
    let mut store = fixture.store();
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut collision = ReferenceCollision::default();
    collision
        .admit(
            &world,
            &mut owner,
            &ticket,
            &mut store,
            0,
            &fixture.placements(&world),
            units(),
            reference::Limits::default(),
        )
        .unwrap();
    let result = collision
        .ray_cast(&world, &owner, ray(), reference::Budget::default())
        .unwrap();
    let restored = World::restore(
        Arc::clone(&fixture.catalogue),
        world.snapshot(),
        fallout_runtime::Limits::default(),
    )
    .unwrap();
    assert_eq!(restored.snapshot(), world.snapshot());
    assert!(result.hits(&restored, &owner).is_err());
    assert!(result.scope(&restored, &owner).is_err());
    let mut other_campaign_snapshot = world.snapshot();
    other_campaign_snapshot.campaign = CampaignId::from_bytes([9; 16]).unwrap();
    let other_campaign = World::restore(
        Arc::clone(&fixture.catalogue),
        other_campaign_snapshot,
        fallout_runtime::Limits::default(),
    )
    .unwrap();
    assert_ne!(other_campaign.campaign(), world.campaign());
    assert!(result.hits(&other_campaign, &owner).is_err());
    let other_fixture = Fixture::try_new_mode(4).unwrap();
    let other_cohort = other_fixture.canonical();
    assert_ne!(
        other_cohort.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    assert!(result.hits(&other_cohort, &owner).is_err());
    let mut foreign = fixture.owner();
    foreign.request(fixture.plan.clone()).unwrap();
    decoded(&mut foreign);
    assert!(result.hits(&world, &foreign).is_err());
    world
        .advance_clocks(Clocks {
            tick: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(result.hits(&world, &owner).is_err());
    assert!(
        collision
            .ray_cast(&world, &owner, ray(), reference::Budget::default())
            .is_err()
    );
    assert_eq!(collision.retained_primitive_count(), 0);
    owner.unload().unwrap();
    let after = owner.poll().unwrap();
    assert_eq!(after.pinned_source_bytes, 0);
    assert_eq!(after.retained_plans, 0);
}

#[test]
fn unload_invalidates_reference_selection_and_releases_original_model_lease() {
    let fixture = Fixture::new();
    let world = fixture.canonical();
    let mut store = fixture.store();
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut collision = ReferenceCollision::default();
    collision
        .admit(
            &world,
            &mut owner,
            &ticket,
            &mut store,
            0,
            &fixture.placements(&world),
            units(),
            reference::Limits::default(),
        )
        .unwrap();
    let result = collision
        .ray_cast(&world, &owner, ray(), reference::Budget::default())
        .unwrap();
    owner.unload().unwrap();
    assert!(result.hits(&world, &owner).is_err());
    assert!(collision.invalidate(&world, &owner));
    let after = owner.poll().unwrap();
    assert_eq!(after.pinned_source_bytes, 0);
    assert_eq!(after.retained_plans, 0);
    assert_eq!(after.outstanding, 0);
    assert_eq!(collision.retained_primitive_count(), 0);
}

#[test]
fn forged_wrong_absent_source_and_duplicate_reference_bodies_never_publish_geometry() {
    let fixture = Fixture::new();
    let mut world = fixture.canonical();
    let wrong = world.register_reference(Some(form(0x400))).unwrap();
    let dynamic = world.register_reference(None).unwrap();
    let expected = world.snapshot();
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    for case in 0..7 {
        let mut store = fixture.store();
        let mut placements = fixture.placements(&world);
        match case {
            0 => placements[0].body.reference = placements[1].body.reference,
            1 => placements[0].body.reference = wrong,
            2 => placements[0].body.reference = dynamic,
            3 => placements[0].body.source_sha256 = [0; 32],
            4 => placements.push(placements[0].clone()),
            5 => placements[0].body.body_block = 1,
            6 => {
                placements[0].body.reference =
                    fallout_runtime::identity::ReferenceId(std::num::NonZeroU64::new(77).unwrap())
            }
            _ => unreachable!(),
        }
        let mut collision = ReferenceCollision::default();
        assert!(
            collision
                .admit(
                    &world,
                    &mut owner,
                    &ticket,
                    &mut store,
                    0,
                    &placements,
                    units(),
                    reference::Limits::default()
                )
                .is_err(),
            "case{case}"
        );
        assert_eq!(collision.retained_primitive_count(), 0);
        assert!(
            collision
                .ray_cast(&world, &owner, ray(), reference::Budget::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), expected);
    }
}

#[test]
fn admission_and_reference_work_have_exact_and_one_under_boundaries() {
    let fixture = Fixture::new();
    let world = fixture.canonical();
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let source_bytes = fs::metadata(fixture.source.path().join("FalloutNV.esm"))
        .unwrap()
        .len();
    for case in 0..8 {
        let mut store = fixture.store();
        let mut limits = reference::Limits {
            source_bytes,
            decoded_bytes: fixture.plan.receipt().usage.record_decoded_bytes
                + fixture.model.len()
                + 132,
            field_sites: fixture.plan.receipt().usage.field_sites + 8,
            model_bytes: fixture.model.len(),
            ..Default::default()
        };
        match case {
            1 => limits.source_bytes -= 1,
            2 => limits.decoded_bytes -= 1,
            3 => limits.field_sites -= 1,
            4 => limits.model_bytes -= 1,
            5 => limits.placements = 1,
            6 => limits.scene.primitives = 1,
            7 => limits.metadata_bytes = 0,
            _ => {}
        }
        let mut collision = ReferenceCollision::default();
        let outcome = collision.admit(
            &world,
            &mut owner,
            &ticket,
            &mut store,
            0,
            &fixture.placements(&world),
            units(),
            limits,
        );
        if case == 0 {
            outcome.unwrap();
            let exact = reference::Budget {
                reference_checks: 4,
                geometry: QueryBudget::default(),
            };
            assert_eq!(
                collision
                    .ray_cast(&world, &owner, ray(), exact)
                    .unwrap()
                    .hits(&world, &owner)
                    .unwrap()
                    .len(),
                2
            );
            assert!(matches!(
                collision.ray_cast(
                    &world,
                    &owner,
                    ray(),
                    reference::Budget {
                        reference_checks: 3,
                        ..exact
                    }
                ),
                Err(reference::Error::Budget("reference checks"))
            ));
            assert_eq!(
                collision
                    .ray_cast(&world, &owner, ray(), exact)
                    .unwrap()
                    .hits(&world, &owner)
                    .unwrap()
                    .len(),
                2
            );
        } else {
            assert!(outcome.is_err(), "case{case}");
            assert_eq!(collision.retained_primitive_count(), 0);
        }
    }
}

#[test]
fn source_deleted_wrong_kind_missing_name_and_missing_model_refuse() {
    for mode in 1..=5 {
        let fixture = Fixture::try_new_mode(mode).unwrap();
        let world = fixture.canonical();
        let before = world.snapshot();
        let mut store = fixture.store();
        let mut owner = fixture.owner();
        let ticket = owner.request(fixture.plan.clone()).unwrap();
        decoded(&mut owner);
        let mut collision = ReferenceCollision::default();
        assert!(
            collision
                .admit(
                    &world,
                    &mut owner,
                    &ticket,
                    &mut store,
                    0,
                    &fixture.placements(&world),
                    units(),
                    reference::Limits::default()
                )
                .is_err(),
            "mode {mode}"
        );
        assert_eq!(collision.retained_primitive_count(), 0);
        assert!(
            collision
                .ray_cast(&world, &owner, ray(), reference::Budget::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(
        Fixture::try_new_mode(6).is_err(),
        "duplicate source NAME must refuse during source planning"
    );
}

#[test]
fn release_drop_and_failed_replacement_revoke_existing_receipts() {
    for mode in 0..3 {
        let fixture = Fixture::new();
        let world = fixture.canonical();
        let before = world.snapshot();
        let mut store = fixture.store();
        let mut replacement_store = fixture.store();
        let mut owner = fixture.owner();
        let ticket = owner.request(fixture.plan.clone()).unwrap();
        decoded(&mut owner);
        let mut collision = ReferenceCollision::default();
        let placements = fixture.placements(&world);
        collision
            .admit(
                &world,
                &mut owner,
                &ticket,
                &mut store,
                0,
                &placements,
                units(),
                reference::Limits::default(),
            )
            .unwrap();
        let hits = collision
            .ray_cast(&world, &owner, ray(), reference::Budget::default())
            .unwrap();
        match mode {
            0 => collision.release(&mut owner).unwrap(),
            1 => drop(collision),
            2 => {
                let mut forged = placements;
                forged[0].body.source_sha256 = [0; 32];
                assert!(
                    collision
                        .admit(
                            &world,
                            &mut owner,
                            &ticket,
                            &mut replacement_store,
                            0,
                            &forged,
                            units(),
                            reference::Limits::default()
                        )
                        .is_err()
                );
                assert_eq!(collision.retained_primitive_count(), 0);
            }
            _ => unreachable!(),
        }
        assert!(hits.hits(&world, &owner).is_err(), "mode {mode}");
        assert!(hits.scope(&world, &owner).is_err(), "mode {mode}");
        assert_eq!(world.snapshot(), before);
        owner.unload().unwrap();
        let after = owner.poll().unwrap();
        assert_eq!(after.pinned_source_bytes, 0);
        assert_eq!(after.retained_plans, 0);
        assert_eq!(after.outstanding, 0);
    }
}

#[test]
fn changed_protected_cohort_cannot_join_an_old_catalogue_and_plan() {
    let fixture = Fixture::new();
    let world = fixture.canonical();
    let before = world.snapshot();
    let source_path = fixture.source.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&source_path).unwrap();
    bytes.extend(record(b"ACTI", 0x900, 0, &[]));
    fs::write(&source_path, bytes).unwrap();
    let mut changed = fixture.store();
    let mut owner = fixture.owner();
    let ticket = owner.request(fixture.plan.clone()).unwrap();
    decoded(&mut owner);
    let mut collision = ReferenceCollision::default();
    assert!(
        collision
            .admit(
                &world,
                &mut owner,
                &ticket,
                &mut changed,
                0,
                &fixture.placements(&world),
                units(),
                reference::Limits::default()
            )
            .is_err()
    );
    assert_eq!(collision.retained_primitive_count(), 0);
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "explicit private cold-process CLI fixture export"]
fn reference_cli_fixture_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_REFERENCE_FIXTURE").expect("explicit export root"),
    );
    fs::create_dir(&root).unwrap();
    let fixture = Fixture::new();
    let world = fixture.canonical();
    let install = root.join("install");
    let data = install.join("Data");
    fs::create_dir_all(&data).unwrap();
    for name in ["FalloutNV.esm", "models.bsa"] {
        fs::copy(fixture.source.path().join(name), data.join(name)).unwrap();
    }
    let native = root.join("native");
    Repository::create(&native, &[install], world.campaign())
        .unwrap()
        .commit(&Captured::at_boundary(&world))
        .unwrap();
    let placements = fixture.placements(&world).into_iter().map(|p| serde_json::json!({
        "reference":p.body.reference.0.get(),"authored":p.authored,"body_blocks":[p.body.body_block],
        "attachment_rows":p.body.attachment_to_source.rows
    })).collect::<Vec<_>>();
    let request = serde_json::json!({"model_index":0,"source_sha256":format!("{:x}",Sha256::digest(&fixture.model)),
        "placements":placements,"units":units(),"ray":{"origin":[-3.,0.,0.],"direction":[1.,0.,0.],"max_distance":20.},
        "overlap":{"center":[0.,0.,0.],"radius":0.},"io_deadline_ms":10000,"verify_unload":true});
    fs::write(
        root.join("request.json"),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
    fs::write(root.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        root.join("snapshot.json"),
        world
            .snapshot()
            .encode(fallout_runtime::Limits::default().max_snapshot_bytes)
            .unwrap(),
    )
    .unwrap();
}
