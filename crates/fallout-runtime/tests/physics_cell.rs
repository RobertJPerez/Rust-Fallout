//! Authored plugin/BSA/NIF inputs reach sealed world residency and cell queries.
use fallout_data::{
    archive::NvArchive,
    coordinates::Affine,
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{
        preparation::CellModelPlan,
        residency::{self, CellResidency, Readiness, Stage, TexturePlan},
    },
};
use fallout_runtime::{
    identity::ReferenceId,
    physics::{cell::*, *},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    num::NonZeroU64,
    thread,
    time::{Duration, Instant},
};

fn record(tag: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
    ]
    .concat()
}
fn field(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
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
fn nif(radius: f32) -> Vec<u8> {
    let mut body = vec![0; 236];
    body[..4].copy_from_slice(&1u32.to_le_bytes());
    body[80..84].copy_from_slice(&1f32.to_le_bytes());
    let shape = [17u32.to_le_bytes(), radius.to_le_bytes()].concat();
    let names = ["bhkRigidBody", "bhkSphereShape"];
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    b.extend(0x14020007u32.to_le_bytes());
    b.push(1);
    for word in [11u32, 2, 34] {
        b.extend(word.to_le_bytes());
    }
    b.extend([0; 3]);
    b.extend(2u16.to_le_bytes());
    for name in names {
        b.extend((name.len() as u32).to_le_bytes());
        b.extend(name.as_bytes());
    }
    b.extend(0u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    for word in [236u32, 8, 0, 0, 0] {
        b.extend(word.to_le_bytes());
    }
    b.extend(body);
    b.extend(shape);
    b.extend(0u32.to_le_bytes());
    b
}
struct Fixture {
    source: tempfile::TempDir,
    cache: tempfile::TempDir,
    plan: CellModelPlan,
    nif: Vec<u8>,
}
impl Fixture {
    fn new(radius: f32) -> Self {
        let source = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let nif = nif(radius);
        // Independent one-folder, one uncompressed-member BSA104.
        let name = b"shape.nif\0";
        let offset = 76 + name.len();
        let mut bsa = vec![0; offset];
        bsa[..4].copy_from_slice(b"BSA\0");
        for (at, word) in [
            (4, 104u32),
            (8, 36),
            (12, 3),
            (16, 1),
            (20, 1),
            (24, 7),
            (28, name.len() as u32),
            (44, 1),
            (48, 52),
            (68, nif.len() as u32),
            (72, offset as u32),
        ] {
            bsa[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        bsa[52] = 7;
        bsa[53..60].copy_from_slice(b"meshes\0");
        bsa[60..68].copy_from_slice(&1u64.to_le_bytes());
        bsa[76..].copy_from_slice(name);
        bsa.extend(&nif);
        let archive = source.path().join("models.bsa");
        fs::write(&archive, bsa).unwrap();
        let mut mounts = MountIndex::default();
        NvArchive::open(&archive)
            .unwrap()
            .census(&mut mounts)
            .unwrap();
        let mut esm = record(
            b"TES4",
            0,
            &field(
                b"HEDR",
                &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
            ),
        );
        esm.extend(record(b"STAT", 0x400, &field(b"MODL", b"shape.nif\0")));
        esm.extend(record(
            b"CELL",
            0x200,
            &[
                field(b"EDID", b"CellCollisionFixture\0"),
                field(b"DATA", &[1]),
            ]
            .concat(),
        ));
        let member = record(
            b"REFR",
            0x300,
            &[
                field(b"NAME", &0x400u32.to_le_bytes()),
                field(b"DATA", &[0; 24]),
            ]
            .concat(),
        );
        esm.extend(group(6, &group(9, &member)));
        fs::write(source.path().join("FalloutNV.esm"), esm).unwrap();
        let mut store = RecordStore::open_nv_headers(
            source.path(),
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap();
        let root = FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x200,
        };
        let plan = CellModelPlan::load(&mut store, &root, &mounts, Default::default()).unwrap();
        Self {
            source,
            cache,
            plan,
            nif,
        }
    }
    fn owner(&self) -> CellResidency {
        CellResidency::new(
            self.source.path(),
            Some(self.cache.path()),
            residency::Limits {
                workers: 1,
                models: 1,
                source_bytes: 1024 * 1024,
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn placement(&self) -> BodyPlacement {
        BodyPlacement {
            reference: ReferenceId(NonZeroU64::new(1).unwrap()),
            source_sha256: Sha256::digest(&self.nif).into(),
            body_block: 0,
            attachment_to_source: Affine {
                rows: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            },
        }
    }
}
fn decoded(world: &mut CellResidency) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while world.poll().unwrap().stage != Stage::Decoded {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
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
        max_distance: 10.,
    }
}

#[test]
fn selected_query_cannot_activate_cell_and_unload_revokes_hits_and_releases_pins() {
    let fixture = Fixture::new(1.);
    let mut world = fixture.owner();
    let ticket = world.request(fixture.plan.clone()).unwrap();
    decoded(&mut world);
    let mut collision = CellCollision::default();
    let scope = collision
        .admit(
            &mut world,
            &ticket,
            0,
            &[fixture.placement()],
            units(),
            QueryLimits::default(),
        )
        .unwrap();
    assert_eq!(scope.generation, ticket.generation());
    assert_eq!(scope.source_identity, ticket.identity());
    assert!(
        world
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    assert_eq!(world.snapshot().dependencies, Readiness::Pending);
    assert_eq!(world.snapshot().stage, Stage::Decoded);
    // This collision-only NIF has no texture requests. Derive that closure from
    // the source lease; its unsupported scene blocks still prevent activation.
    let textures = TexturePlan::load(
        world.sources(&ticket).unwrap(),
        &MountIndex::default(),
        Default::default(),
    )
    .unwrap();
    assert!(textures.receipt().usages.is_empty());
    assert!(textures.receipt().requests.is_empty());
    assert_eq!(textures.receipt().missing_or_ambiguous, 0);
    assert!(!textures.receipt().reference_coverage_verified);
    world.request_textures(&ticket, textures).unwrap();
    world
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap();
    world.report_behavior(&ticket, Readiness::Ready).unwrap();
    assert_eq!(world.snapshot().collision, Readiness::Unsupported);
    assert!(!world.snapshot().simulation_ready);
    let hits = collision
        .ray_cast(&world, ray(), QueryBudget::default())
        .unwrap();
    assert_eq!(hits.hits(&world).unwrap()[0].distance, 2.);
    assert_eq!(hits.scope(&world).unwrap().generation, ticket.generation());
    assert_eq!(
        collision
            .overlap_sphere(&world, [0.; 3], 0., QueryBudget::default())
            .unwrap()
            .hits(&world)
            .unwrap()
            .len(),
        1
    );
    world.unload().unwrap();
    assert_eq!(world.snapshot().pinned_source_bytes, fixture.nif.len());
    assert!(hits.hits(&world).is_err());
    assert!(hits.scope(&world).is_err());
    assert!(collision.invalidate(&world));
    assert_eq!(collision.retained_primitive_count(), 0);
    let after = world.poll().unwrap();
    assert_eq!(after.pinned_source_bytes, 0);
    assert_eq!(after.outstanding, 0);
    assert_eq!(after.retained_plans, 0);
    assert!(
        collision
            .ray_cast(&world, ray(), QueryBudget::default())
            .is_err()
    );
    let fresh = world.request(fixture.plan.clone()).unwrap();
    decoded(&mut world);
    assert_ne!(ticket.generation(), fresh.generation());
    assert!(
        collision
            .admit(
                &mut world,
                &ticket,
                0,
                &[fixture.placement()],
                units(),
                QueryLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot().collision, Readiness::Pending);
    collision
        .admit(
            &mut world,
            &fresh,
            0,
            &[fixture.placement()],
            units(),
            QueryLimits::default(),
        )
        .unwrap();
    assert!(hits.hits(&world).is_err());
    collision.release(&mut world).unwrap();
    assert_eq!(world.snapshot().collision, Readiness::Pending);
}

#[test]
fn equal_root_identity_epoch_in_foreign_owner_never_authorizes_query_or_receipt() {
    let fixture = Fixture::new(1.);
    let mut world = fixture.owner();
    let ticket = world.request(fixture.plan.clone()).unwrap();
    decoded(&mut world);
    let mut collision = CellCollision::default();
    collision
        .admit(
            &mut world,
            &ticket,
            0,
            &[fixture.placement()],
            units(),
            QueryLimits::default(),
        )
        .unwrap();
    let hits = collision
        .ray_cast(&world, ray(), QueryBudget::default())
        .unwrap();
    let mut foreign = fixture.owner();
    let foreign_ticket = foreign.request(fixture.plan.clone()).unwrap();
    decoded(&mut foreign);
    assert_eq!(foreign_ticket.generation(), ticket.generation());
    assert_eq!(foreign_ticket.identity(), ticket.identity());
    assert!(hits.hits(&foreign).is_err());
    assert!(hits.scope(&foreign).is_err());
    assert!(
        collision
            .ray_cast(&foreign, ray(), QueryBudget::default())
            .is_err()
    );
    assert_eq!(collision.retained_primitive_count(), 0);
    assert_eq!(hits.hits(&world).unwrap().len(), 1);
}

#[test]
fn source_digest_unsupported_geometry_and_query_budget_never_publish_partial_scene() {
    for radius in [1., -1.] {
        let fixture = Fixture::new(radius);
        let mut world = fixture.owner();
        let ticket = world.request(fixture.plan.clone()).unwrap();
        decoded(&mut world);
        let mut collision = CellCollision::default();
        let mut wrong = fixture.placement();
        wrong.source_sha256 = [0; 32];
        assert!(matches!(
            collision.admit(
                &mut world,
                &ticket,
                0,
                &[wrong],
                units(),
                QueryLimits::default()
            ),
            Err(CellError::Invalid(_))
        ));
        let built = collision.admit(
            &mut world,
            &ticket,
            0,
            &[fixture.placement()],
            units(),
            QueryLimits::default(),
        );
        if radius < 0. {
            assert!(matches!(
                built,
                Err(CellError::Query(QueryError::Unsupported { .. }))
            ));
            assert_eq!(collision.retained_primitive_count(), 0);
        } else {
            built.unwrap();
            assert!(matches!(
                collision.ray_cast(
                    &world,
                    ray(),
                    QueryBudget {
                        primitive_tests: 0,
                        ..Default::default()
                    }
                ),
                Err(CellError::Query(QueryError::Budget(_)))
            ));
            let mut wrong = fixture.placement();
            wrong.source_sha256 = [0; 32];
            assert!(
                collision
                    .admit(
                        &mut world,
                        &ticket,
                        0,
                        &[wrong],
                        units(),
                        QueryLimits::default()
                    )
                    .is_err()
            );
            assert_eq!(collision.retained_primitive_count(), 0);
            assert!(
                collision
                    .ray_cast(&world, ray(), QueryBudget::default())
                    .is_err()
            );
        }
        assert_eq!(world.snapshot().collision, Readiness::Unsupported);
        assert!(!world.snapshot().simulation_ready);
        collision.release(&mut world).unwrap();
        world.unload().unwrap();
        let after = world.poll().unwrap();
        assert_eq!(after.pinned_source_bytes, 0);
        assert_eq!(after.retained_plans, 0);
    }
}

#[test]
fn stale_query_and_cache_drop_release_the_original_upstream_lease() {
    for explicit_drop in [false, true] {
        let fixture = Fixture::new(1.);
        let mut world = fixture.owner();
        let ticket = world.request(fixture.plan.clone()).unwrap();
        decoded(&mut world);
        let mut collision = CellCollision::default();
        collision
            .admit(
                &mut world,
                &ticket,
                0,
                &[fixture.placement()],
                units(),
                QueryLimits::default(),
            )
            .unwrap();
        world.unload().unwrap();
        assert_eq!(world.snapshot().pinned_source_bytes, fixture.nif.len());
        if explicit_drop {
            drop(collision);
        } else {
            assert!(
                collision
                    .overlap_sphere(&world, [0.; 3], 0., QueryBudget::default())
                    .is_err()
            );
            assert_eq!(collision.retained_primitive_count(), 0);
        }
        let after = world.poll().unwrap();
        assert_eq!(after.pinned_source_bytes, 0);
        assert_eq!(after.retained_plans, 0);
        assert_eq!(after.outstanding, 0);
    }
}
