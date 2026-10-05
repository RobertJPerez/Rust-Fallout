use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::JobError,
    store::RecordStore,
    vfs::MountIndex,
    world::{preparation::CellModelPlan, residency::*},
};
use fallout_preview::engine_bridge::{self, Phase, SceneLifetime};
use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const EPOCH: u64 = 17;

// Original authored framing exercises production source plans and ResourceJobs.
// This is a lifetime fixture, not evidence of retail archive lookup or rendering.
struct Fixture {
    root: PathBuf,
    nif: Vec<u8>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Delete only our two known leaves, after their protected leases close.
        fs::remove_file(self.root.join("models.bsa")).unwrap();
        fs::remove_file(self.root.join("FalloutNV.esm")).unwrap();
        fs::remove_dir(&self.root).unwrap();
    }
}
fn sub(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(tag: &[u8; 4], id: u32, bytes: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        bytes,
    ]
    .concat()
}
fn group(kind: i32, bytes: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(bytes.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}

fn setup() -> (Fixture, CellResidency, Ticket) {
    let root = std::env::temp_dir().join(format!(
        "fallout-engine-lifetime-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    nif.extend(0x14020007u32.to_le_bytes());
    nif.push(1);
    for value in [11u32, 0, 34] {
        nif.extend(value.to_le_bytes());
    }
    nif.extend([0; 21]);
    let fixture = Fixture { root, nif };
    let data_offset = 82;
    let mut bsa = vec![0; data_offset];
    bsa[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104u32),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 1),
        (24, 7),
        (28, 6),
        (44, 1),
        (48, 52),
        (68, fixture.nif.len() as u32),
        (72, data_offset as u32),
    ] {
        bsa[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bsa[52] = 7;
    bsa[53..60].copy_from_slice(b"meshes\0");
    bsa[60..68].copy_from_slice(&1u64.to_le_bytes());
    bsa[76..82].copy_from_slice(b"m.nif\0");
    bsa.extend(&fixture.nif);
    fs::write(fixture.root.join("models.bsa"), bsa).unwrap();
    let reference = record(
        b"REFR",
        0x300,
        &[
            sub(b"NAME", &0x400u32.to_le_bytes()),
            sub(b"DATA", &[0; 24]),
        ]
        .concat(),
    );
    let esm = [
        record(
            b"TES4",
            0,
            &sub(
                b"HEDR",
                &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
            ),
        ),
        record(b"STAT", 0x400, &sub(b"MODL", b"m.nif\0")),
        record(
            b"CELL",
            0x200,
            &[sub(b"EDID", b"LifetimeRoom\0"), sub(b"DATA", &[1])].concat(),
        ),
        group(6, &group(9, &reference)),
    ]
    .concat();
    fs::write(fixture.root.join("FalloutNV.esm"), esm).unwrap();
    let mut mounts = MountIndex::default();
    NvArchive::open(&fixture.root.join("models.bsa"))
        .unwrap()
        .census(&mut mounts)
        .unwrap();
    let mut store = RecordStore::open_nv_headers(
        &fixture.root,
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let key = FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: 0x200,
    };
    let plan = CellModelPlan::load(&mut store, &key, &mounts, Default::default()).unwrap();
    let mut owner = CellResidency::new(
        &fixture.root,
        None,
        Limits {
            workers: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let ticket = owner.request(plan).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while owner.poll().unwrap().stage != Stage::Decoded {
        assert!(Instant::now() < deadline, "source jobs did not complete");
        thread::sleep(Duration::from_millis(1));
    }
    let sources = owner.sources(&ticket).unwrap();
    let textures = TexturePlan::load(sources, &mounts, Default::default()).unwrap();
    owner.request_textures(&ticket, textures).unwrap();
    while owner.poll().unwrap().texture_state != TextureState::Decoded {
        assert!(Instant::now() < deadline, "texture jobs did not complete");
        thread::sleep(Duration::from_millis(1));
    }
    owner
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap();
    (fixture, owner, ticket)
}

mod upload_admission {
    use super::*;
    use bevy::{
        app::SubApp,
        asset::{Assets, RenderAssetUsages},
        pbr::StandardMaterial,
        prelude::{App, Handle, Image, Mesh},
        render::{Render, RenderApp, RenderStartup, render_resource::PrimitiveTopology},
    };
    use engine_bridge::upload::{UploadAssets, UploadMonitor, UploadPlugin};

    fn app(render_schedule: bool) -> App {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>();
        if render_schedule {
            app.insert_sub_app(RenderApp, SubApp::new());
        }
        app.add_plugins(UploadPlugin::<StandardMaterial>::default());
        if render_schedule {
            app.sub_app_mut(RenderApp)
                .world_mut()
                .run_schedule(RenderStartup);
        }
        app
    }

    fn draws(app: &mut App) -> UploadAssets<StandardMaterial> {
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
        );
        UploadAssets {
            meshes: vec![app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh)],
            materials: vec![
                app.world_mut()
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(StandardMaterial::default()),
            ],
            images: vec![
                app.world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add(Image::default()),
            ],
        }
    }

    #[test]
    fn no_renderer_never_admits_an_upload_after_elapsed_updates() {
        let (_fixture, owner, ticket) = setup();
        let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        scene.submission_complete(EPOCH).unwrap();
        let mut app = app(false);
        let monitor = app
            .world()
            .resource::<UploadMonitor<StandardMaterial>>()
            .clone();
        for _ in 0..3 {
            app.update();
            assert!(matches!(
                monitor.watch(&scene, EPOCH, draws(&mut app)),
                Err(JobError::Invalid(message)) if message.contains("not initialized")
            ));
        }
        assert_eq!(scene.phase(), Phase::Submitted);
        assert!(!scene.snapshot().source.render_published);
    }

    #[test]
    fn weak_and_duplicate_handles_are_refused_for_each_asset_kind() {
        let (_fixture, owner, ticket) = setup();
        let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        scene.submission_complete(EPOCH).unwrap();
        let mut app = app(true);
        let monitor = app
            .world()
            .resource::<UploadMonitor<StandardMaterial>>()
            .clone();
        for kind in 0..3 {
            for duplicate in [false, true] {
                let mut assets = draws(&mut app);
                match (kind, duplicate) {
                    (0, false) => assets.meshes[0] = Handle::default(),
                    (1, false) => assets.materials[0] = Handle::default(),
                    (2, false) => assets.images[0] = Handle::default(),
                    (0, true) => assets.meshes.push(assets.meshes[0].clone()),
                    (1, true) => assets.materials.push(assets.materials[0].clone()),
                    (2, true) => assets.images.push(assets.images[0].clone()),
                    _ => unreachable!(),
                }
                assert!(matches!(
                    monitor.watch(&scene, EPOCH, assets),
                    Err(JobError::Invalid(message)) if message.contains("unique strong")
                ));
            }
        }
        assert!(!scene.snapshot().source.render_published);
    }

    #[test]
    fn upload_requires_submission_and_nonempty_draw_inventory() {
        let (_fixture, owner, ticket) = setup();
        let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        let mut app = app(true);
        let monitor = app
            .world()
            .resource::<UploadMonitor<StandardMaterial>>()
            .clone();
        assert!(monitor.watch(&scene, EPOCH, draws(&mut app)).is_err());
        assert_eq!(scene.phase(), Phase::Prepared);
        scene.submission_complete(EPOCH).unwrap();
        for empty_meshes in [false, true] {
            let mut assets = draws(&mut app);
            if empty_meshes {
                assets.meshes.clear();
            } else {
                assets.materials.clear();
            }
            assert!(monitor.watch(&scene, EPOCH, assets).is_err());
        }
        assert!(!scene.snapshot().source.render_published);
    }

    #[test]
    fn actual_render_schedule_without_gpu_resources_keeps_publication_pending() {
        let (_fixture, owner, ticket) = setup();
        let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        scene.submission_complete(EPOCH).unwrap();
        let mut app = app(true);
        let monitor = app
            .world()
            .resource::<UploadMonitor<StandardMaterial>>()
            .clone();
        let upload = monitor.watch(&scene, EPOCH, draws(&mut app)).unwrap();
        let publications = Cell::new(0);
        // The real Bevy schedules run, but no GPU objects or queue are installed.
        // This verifies refusal/pending behavior, never a successful frame upload.
        for _ in 0..3 {
            app.sub_app_mut(RenderApp).world_mut().run_schedule(Render);
            assert!(!upload.ready_for(&scene, EPOCH).unwrap());
            assert_eq!(
                upload
                    .publish_uploaded(&mut scene, EPOCH, || {
                        publications.set(publications.get() + 1);
                        Ok(())
                    })
                    .unwrap(),
                None
            );
        }
        assert_eq!(publications.get(), 0);
        assert_eq!(scene.phase(), Phase::Submitted);
        upload.cancel();
        assert!(upload.ready_for(&scene, EPOCH).is_err());
        assert!(
            upload
                .publish_uploaded(&mut scene, EPOCH, || Ok(()))
                .is_err()
        );
        let retry = monitor.watch(&scene, EPOCH, draws(&mut app)).unwrap();
        assert!(!retry.ready_for(&scene, EPOCH).unwrap());
        scene.begin_retirement(EPOCH).unwrap();
        assert!(retry.ready_for(&scene, EPOCH).is_err());
        assert!(!scene.snapshot().source.render_published);
    }

    #[test]
    fn upload_binds_the_exact_scene_owner_epoch_and_render_startup() {
        let (_fixture, owner, ticket) = setup();
        let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        scene.submission_complete(EPOCH).unwrap();
        let (_foreign_fixture, foreign_owner, foreign_ticket) = setup();
        let mut foreign = SceneLifetime::prepared(foreign_owner, foreign_ticket, EPOCH).unwrap();
        foreign.submission_complete(EPOCH).unwrap();
        let mut app = app(true);
        let monitor = app
            .world()
            .resource::<UploadMonitor<StandardMaterial>>()
            .clone();
        let upload = monitor.watch(&scene, EPOCH, draws(&mut app)).unwrap();
        assert!(matches!(
            upload.ready_for(&scene, EPOCH + 1),
            Err(JobError::Stale)
        ));
        assert!(matches!(
            upload.ready_for(&foreign, EPOCH),
            Err(JobError::Invalid(message)) if message.contains("another source owner")
        ));
        assert!(monitor.watch(&scene, EPOCH, draws(&mut app)).is_err());
        app.sub_app_mut(RenderApp)
            .world_mut()
            .run_schedule(RenderStartup);
        assert!(matches!(
            upload.ready_for(&scene, EPOCH),
            Err(JobError::Stale)
        ));
        let replacement = monitor.watch(&scene, EPOCH, draws(&mut app)).unwrap();
        assert!(!replacement.ready_for(&scene, EPOCH).unwrap());
        drop(replacement);
        app.sub_app_mut(RenderApp).world_mut().run_schedule(Render);
        assert!(monitor.watch(&scene, EPOCH, draws(&mut app)).is_ok());
        assert!(!scene.snapshot().source.render_published);
        assert!(!foreign.snapshot().source.render_published);
    }
}

#[test]
fn real_source_submission_upload_and_simulation_readiness_stay_separate() {
    let (fixture, owner, ticket) = setup();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    assert_eq!(scene.sources(EPOCH).unwrap().model(0).unwrap(), fixture.nif);
    assert!(
        scene
            .textures(EPOCH)
            .unwrap()
            .receipt()
            .unwrap()
            .reference_coverage_verified
    );
    assert_eq!(scene.phase(), Phase::Prepared);
    assert!(!scene.snapshot().simulation_ready);
    scene.report_collision(EPOCH, Readiness::Ready).unwrap();
    scene.report_behavior(EPOCH, Readiness::Ready).unwrap();
    assert!(scene.snapshot().source.simulation_ready);
    assert!(!scene.snapshot().simulation_ready);
    scene.submission_complete(EPOCH).unwrap();
    let calls = Cell::new(0);
    assert_eq!(
        scene
            .publish_uploaded(
                EPOCH,
                || Ok(false),
                || {
                    calls.set(calls.get() + 1);
                    Ok(3)
                }
            )
            .unwrap(),
        None
    );
    assert_eq!(scene.phase(), Phase::Submitted);
    assert!(!scene.snapshot().source.render_published);
    assert_eq!(calls.get(), 0);
    assert_eq!(
        scene
            .publish_uploaded(
                EPOCH,
                || Ok(true),
                || {
                    calls.set(calls.get() + 1);
                    Ok(3)
                }
            )
            .unwrap(),
        Some(3)
    );
    assert_eq!(calls.get(), 1);
    assert!(scene.snapshot().simulation_ready);
    assert!(
        scene
            .publish_uploaded(
                EPOCH,
                || Ok(true),
                || {
                    calls.set(99);
                    Ok(3)
                }
            )
            .is_err()
    );
    assert_eq!(calls.get(), 1);
    scene
        .report_collision(EPOCH, Readiness::Unsupported)
        .unwrap();
    assert!(!scene.snapshot().simulation_ready);
}

#[test]
fn stale_epoch_and_failed_gpu_or_publication_checks_preserve_admission() {
    let (_fixture, owner, ticket) = setup();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    let before = serde_json::to_value(scene.snapshot()).unwrap();
    assert!(scene.sources(EPOCH + 1).is_err());
    assert!(scene.textures(EPOCH + 1).is_err());
    assert!(scene.submission_complete(EPOCH + 1).is_err());
    assert!(scene.begin_retirement(EPOCH + 1).is_err());
    assert!(scene.draw_disposal_complete(EPOCH).is_err());
    assert_eq!(serde_json::to_value(scene.snapshot()).unwrap(), before);
    scene.submission_complete(EPOCH).unwrap();
    let before = serde_json::to_value(scene.snapshot()).unwrap();
    let calls = Cell::new(0);
    let never_publish = || {
        calls.set(calls.get() + 1);
        Ok(())
    };
    assert!(
        scene
            .publish_uploaded(EPOCH + 1, || Ok(true), never_publish)
            .is_err()
    );
    assert!(
        scene
            .publish_uploaded(EPOCH, || Err(JobError::Closed), never_publish)
            .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert!(
        scene
            .publish_uploaded::<()>(
                EPOCH,
                || Ok(true),
                || Err(JobError::Invalid("injected visibility refusal".into()))
            )
            .is_err()
    );
    assert_eq!(serde_json::to_value(scene.snapshot()).unwrap(), before);
    assert_eq!(
        scene
            .publish_uploaded(EPOCH, || Ok(true), || Ok(()))
            .unwrap(),
        Some(())
    );
}

#[test]
fn foreign_owner_ticket_cannot_join_identical_source_generation() {
    let (_fixture, owner, ticket) = setup();
    let plan = owner.sources(&ticket).unwrap();
    // A second owner of the very same plan still has a distinct cancellation gate.
    let mut other = CellResidency::new(&_fixture.root, None, Default::default()).unwrap();
    let mut store = RecordStore::open_nv_headers(
        &_fixture.root,
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let mut mounts = MountIndex::default();
    NvArchive::open(&_fixture.root.join("models.bsa"))
        .unwrap()
        .census(&mut mounts)
        .unwrap();
    let same_plan =
        CellModelPlan::load(&mut store, ticket.root(), &mounts, Default::default()).unwrap();
    let foreign = other.request(same_plan).unwrap();
    assert_eq!(foreign.generation(), ticket.generation());
    assert_eq!(foreign.identity(), ticket.identity());
    assert!(SceneLifetime::prepared(owner, foreign, EPOCH).is_err());
    assert!(plan.ticket().check().is_err());
}

#[test]
fn retirement_keeps_leases_until_draw_disposal_and_final_external_user() {
    let (_fixture, owner, ticket) = setup();
    let external = owner.sources(&ticket).unwrap();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    let bytes = scene.snapshot().source.pinned_source_bytes;
    assert!(bytes > 0);
    scene.begin_retirement(EPOCH).unwrap();
    assert!(scene.ticket().check().is_err());
    assert!(scene.submission_complete(EPOCH).is_err());
    assert!(scene.sources(EPOCH).is_err());
    let retained = scene.poll_retirement(EPOCH).unwrap();
    assert_eq!(retained.phase, Phase::Retiring);
    assert_eq!(retained.source.pinned_source_bytes, bytes);
    assert!(retained.source.retained_plans > 0);
    let external_retained = scene.draw_disposal_complete(EPOCH).unwrap();
    assert_eq!(external_retained.phase, Phase::Retiring);
    assert_eq!(external_retained.source.pinned_source_bytes, bytes);
    drop(external);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = scene.poll_retirement(EPOCH).unwrap();
        if snapshot.phase == Phase::Released {
            assert_eq!(snapshot.source.pinned_source_bytes, 0);
            assert_eq!(snapshot.source.retained_plans, 0);
            assert_eq!(snapshot.source.plan_metadata_bytes, 0);
            assert_eq!(snapshot.source.mapped_source_bytes, 0);
            assert_eq!(snapshot.source.outstanding, 0);
            assert!(!snapshot.simulation_ready);
            break;
        }
        assert!(Instant::now() < deadline, "retained jobs did not drain");
        thread::sleep(Duration::from_millis(1));
    }
    scene.begin_retirement(EPOCH).unwrap();
    assert_eq!(
        scene.draw_disposal_complete(EPOCH).unwrap().phase,
        Phase::Released
    );
}

fn reference_key(local_id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id,
    }
}

fn uploaded(scene: &mut SceneLifetime) {
    // Injected acknowledgement tests the admission mechanism only, not a GPU.
    scene.submission_complete(EPOCH).unwrap();
    scene
        .publish_uploaded(EPOCH, || Ok(true), || Ok(()))
        .unwrap();
}

#[test]
fn prepared_reference_admission_needs_upload_and_complete_protected_membership() {
    let (_fixture, owner, ticket) = setup();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    let active = [reference_key(0x300)];
    assert!(
        scene
            .admit_references(EPOCH, 4, 4, &active, &active)
            .is_err()
    );
    uploaded(&mut scene);
    let before = serde_json::to_value(scene.snapshot()).unwrap();
    assert!(scene.admit_references(EPOCH, 4, 4, &active, &[]).is_err());
    // A base in the same protected graph is not a member of this CELL.
    assert!(
        scene
            .admit_references(EPOCH, 4, 4, &[], &[reference_key(0x400)])
            .is_err()
    );
    let mut foreign = active[0].clone();
    foreign.profile = ProfileId::Fo3Original;
    assert!(
        scene
            .admit_references(EPOCH, 4, 4, &[], &[foreign])
            .is_err()
    );
    assert!(
        scene
            .admit_references(EPOCH, 4, 4, &[], &[reference_key(0x900)])
            .is_err()
    );
    assert_eq!(serde_json::to_value(scene.snapshot()).unwrap(), before);
}

#[test]
fn explicit_unavailable_pose_is_a_binding_and_admission_publishes_once() {
    let (_fixture, owner, ticket) = setup();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    uploaded(&mut scene);
    let active = [reference_key(0x300)];
    let projected: std::collections::BTreeMap<FormKey, Option<[f32; 3]>> =
        [(active[0].clone(), None)].into_iter().collect();
    let observed: Vec<_> = projected.keys().cloned().collect();
    let admission: engine_bridge::ReferenceAdmission<'_> = scene
        .admit_references(EPOCH, 4, 4, &active, &observed)
        .unwrap();
    assert_eq!(admission.scene_epoch(), EPOCH);
    assert_eq!(admission.source_generation(), scene.ticket().generation());
    assert_eq!(admission.source_identity(), scene.ticket().identity());
    assert_eq!(admission.observed_keys(), observed);
    let calls = Cell::new(0);
    let mut shown = true;
    admission
        .publish(EPOCH, 4, |keys| {
            assert_eq!(keys, active);
            shown = projected[&keys[0]].is_some();
            calls.set(calls.get() + 1);
        })
        .unwrap();
    assert!(!shown);
    assert_eq!(calls.get(), 1);
    assert!(!scene.snapshot().simulation_ready);
}

#[test]
fn changed_display_revision_or_scene_epoch_refuses_before_reference_commit() {
    let (_fixture, owner, ticket) = setup();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    uploaded(&mut scene);
    let active = [reference_key(0x300)];
    let before = serde_json::to_value(scene.snapshot()).unwrap();
    let calls = Cell::new(0);
    assert!(matches!(
        scene.admit_references(EPOCH, 4, 5, &active, &active),
        Err(JobError::Stale)
    ));
    for (epoch, revision) in [(EPOCH + 1, 4), (EPOCH, 5)] {
        let admission = scene
            .admit_references(EPOCH, 4, 4, &active, &active)
            .unwrap();
        assert!(matches!(
            admission.publish(epoch, revision, |_| calls.set(1)),
            Err(JobError::Stale)
        ));
    }
    assert_eq!(calls.get(), 0);
    assert_eq!(serde_json::to_value(scene.snapshot()).unwrap(), before);
    scene.begin_retirement(EPOCH).unwrap();
    assert!(
        scene
            .admit_references(EPOCH, 4, 4, &active, &active)
            .is_err()
    );
}

#[test]
fn ambiguous_or_unbounded_reference_sets_never_reach_publication() {
    let (_fixture, owner, ticket) = setup();
    let mut scene = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
    uploaded(&mut scene);
    let key = reference_key(0x300);
    let repeated = [key.clone(), key.clone()];
    assert!(
        scene
            .admit_references(EPOCH, 4, 4, &repeated, &repeated)
            .is_err()
    );
    let reversed = [reference_key(0x301), key.clone()];
    assert!(scene.admit_references(EPOCH, 4, 4, &[], &reversed).is_err());
    let excess = vec![key; 10_001];
    assert!(scene.admit_references(EPOCH, 4, 4, &[], &excess).is_err());
}

mod continue_tests {
    use super::*;
    use engine_bridge::{ContinueDisplay, DisplayStamp};
    use fallout_data::loaded_scripts::Catalogue;
    use fallout_runtime::{
        Limits as RuntimeLimits, World,
        application::{self, ContinueBoundary, Host, HostLimits},
        foreign::Content,
        identity::CampaignId,
        reference_state::{Pose, State},
        save::{Captured, Recovery, Repository, RestorePoll, RestoreTask, SaveWorker},
        source_items::{Policy, Role},
    };
    use std::{num::NonZeroU64, sync::Arc};

    // This joins the real native machinery and application with real residency.
    // The final display callback remains a fixture, not the legacy Bevy host.
    struct Native {
        repository: Repository,
    }
    impl Native {
        fn create(world: &World<'_>, protected: &PathBuf) -> Self {
            let root = std::env::temp_dir().join(format!(
                "fallout-engine-continue-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let repository =
                Repository::create(&root, std::slice::from_ref(protected), world.campaign())
                    .unwrap();
            save(&repository, world);
            Self { repository }
        }
    }
    impl Drop for Native {
        fn drop(&mut self) {
            // Only the four known leaves in this exclusively created directory.
            for name in [
                "current.frsv",
                "previous.frsv",
                "writer.lock",
                ".rust-fallout-saves",
            ] {
                let path = self.repository.path().join(name);
                if path.try_exists().unwrap() {
                    fs::remove_file(path).unwrap();
                }
            }
            fs::remove_dir(self.repository.path()).unwrap();
        }
    }
    fn id(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }
    fn save(repository: &Repository, world: &World<'_>) {
        let mut writer = SaveWorker::start(repository.clone(), 1).unwrap();
        let ticket = writer.try_submit(Captured::at_boundary(world)).unwrap();
        writer.finish().unwrap();
        assert_eq!(
            ticket.wait().unwrap().metadata.state_revision,
            world.revision()
        );
    }
    fn set_pose(world: &mut World<'_>, position: [f32; 3]) {
        let view = world
            .reference_view(world.authored_reference(&reference_key(0x300)).unwrap())
            .unwrap();
        let pose = Pose::from_source(
            &fallout_data::world::Transform {
                position,
                rotation: [0.; 3],
            },
            Some(1.),
        )
        .unwrap();
        let stage = world
            .stage_reference_state(&view, State::new(reference_key(0x200), pose, true).unwrap())
            .unwrap();
        world.commit_reference_state(stage).unwrap();
    }
    fn host(fixture: &Fixture) -> (Host<'static>, Arc<Catalogue>, Native) {
        let mut store = RecordStore::open_nv_headers(
            &fixture.root,
            &["FalloutNV.esm".into()],
            Default::default(),
        )
        .unwrap();
        let catalogue =
            Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
        let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
        let mut world = World::with_campaign(
            Arc::clone(&catalogue),
            RuntimeLimits::default(),
            CampaignId::from_bytes([0x39; 16]).unwrap(),
        )
        .unwrap();
        world
            .register_reference(Some(reference_key(0x300)))
            .unwrap();
        set_pose(&mut world, [16., 8., 4.]);
        let native = Native::create(&world, &fixture.root);
        set_pose(&mut world, [80., 40., 20.]);
        let host = Host::new(
            world,
            content,
            Policy::new(&[(Role::Base, &[*b"MISC"])]).unwrap(),
            id(EPOCH),
            HostLimits::default(),
        )
        .unwrap();
        (host, catalogue, native)
    }
    fn prepare(
        host: &mut Host<'static>,
        catalogue: &Arc<Catalogue>,
        native: &Native,
        request_id: u64,
    ) -> (RestoreTask, application::PreparedContinue) {
        let request = host.begin_continue(id(request_id)).unwrap();
        let mut task = RestoreTask::start(
            native.repository.clone(),
            Arc::clone(catalogue),
            RuntimeLimits::default(),
            Recovery::Strict,
            request.identity().clone(),
        )
        .unwrap();
        task.finish().unwrap();
        let RestorePoll::Ready(candidate) = task.try_poll() else {
            panic!("missing native candidate")
        };
        let prepared = host.prepare_continue(request, *candidate).unwrap();
        (task, prepared)
    }
    fn projected(
        candidate: &World<'_>,
        _: &ContinueBoundary,
    ) -> application::Result<(Vec<FormKey>, Option<[f32; 3]>)> {
        let key = reference_key(0x300);
        let view = candidate
            .authored_reference(&key)
            .map(|id| candidate.reference_view(id))
            .transpose()?;
        let position = view
            .as_ref()
            .and_then(|v| v.state())
            .map(|state| state.pose().source_transform().position);
        Ok((vec![key], position))
    }

    #[test]
    fn late_missing_scene_member_preserves_canonical_display_and_next_native_save() {
        let (fixture, owner, ticket) = setup();
        let mut source = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        uploaded(&mut source);
        let (mut host, catalogue, native) = host(&fixture);
        let before = host.world().snapshot();
        let mut stamp = DisplayStamp::from_host(&host);
        let stamp_before = stamp;
        let active = [reference_key(0x300)];
        let calls = Cell::new(0);
        let (_task, candidate) = prepare(&mut host, &catalogue, &native, 1);
        assert_ne!(candidate.world().snapshot(), before);
        let mut display = ContinueDisplay::new(
            &source,
            &mut stamp,
            &active,
            |world: &World<'_>, boundary: &ContinueBoundary| {
                let (_, stage) = projected(world, boundary)?;
                Ok((Vec::new(), stage))
            },
            |_, _: &ContinueBoundary| calls.set(1),
        );
        assert!(host.publish_continue(candidate, &mut display).is_err());
        assert_eq!(host.world().snapshot(), before);
        assert_eq!(stamp, stamp_before);
        assert_eq!(calls.get(), 0);
        save(&native.repository, host.world());
        let (cold, _) = native
            .repository
            .load(catalogue, RuntimeLimits::default(), Recovery::Strict)
            .unwrap();
        assert_eq!(cold.snapshot(), before);
    }

    #[test]
    fn renderer_preflight_failure_never_replaces_world_or_releases_source_leases() {
        let (fixture, owner, ticket) = setup();
        let mut source = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        uploaded(&mut source);
        let (mut host, catalogue, native) = host(&fixture);
        let before = host.world().snapshot();
        let resources = serde_json::to_value(source.snapshot()).unwrap();
        let mut stamp = DisplayStamp::from_host(&host);
        let stamp_before = stamp;
        let active = [reference_key(0x300)];
        let calls = Cell::new(0);
        let (_task, candidate) = prepare(&mut host, &catalogue, &native, 1);
        let mut display = ContinueDisplay::new(
            &source,
            &mut stamp,
            &active,
            |_: &World<'_>, _: &ContinueBoundary| -> application::Result<(Vec<FormKey>, ())> {
                Err(application::Failure::Refused(
                    "injected render resource refusal",
                ))
            },
            |(), _: &ContinueBoundary| calls.set(1),
        );
        assert!(host.publish_continue(candidate, &mut display).is_err());
        assert_eq!(host.world().snapshot(), before);
        assert_eq!(stamp, stamp_before);
        assert_eq!(serde_json::to_value(source.snapshot()).unwrap(), resources);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn admitted_native_candidate_and_owned_display_stage_publish_one_boundary() {
        let (fixture, owner, ticket) = setup();
        let mut source = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        uploaded(&mut source);
        let (mut host, catalogue, native) = host(&fixture);
        let prior_revision = host.world().revision();
        let mut stamp = DisplayStamp::from_host(&host);
        let prior_host = stamp.host_identity();
        let active = [reference_key(0x300)];
        let calls = Cell::new(0);
        let mut shown = Some([80., 40., 20.]);
        let (_task, candidate) = prepare(&mut host, &catalogue, &native, 1);
        let expected = candidate.world().snapshot();
        let mut display = ContinueDisplay::new(
            &source,
            &mut stamp,
            &active,
            projected,
            |stage, _: &ContinueBoundary| {
                shown = stage;
                calls.set(calls.get() + 1);
            },
        );
        let receipt = host.publish_continue(candidate, &mut display).unwrap();
        assert_eq!(host.world().snapshot(), expected);
        assert_eq!(shown, Some([16., 8., 4.]));
        assert_eq!(calls.get(), 1);
        assert_eq!(stamp.revision(), host.world().revision());
        assert_eq!(stamp.scene_epoch(), EPOCH);
        assert_eq!(stamp.host_identity(), host.identity());
        assert_ne!(stamp.host_identity(), prior_host);
        assert_eq!(receipt.boundary.prior_revision(), prior_revision);
        assert!(!source.snapshot().simulation_ready);
    }

    #[test]
    fn same_revision_restore_cannot_admit_a_previous_host_display_stamp() {
        let (fixture, owner, ticket) = setup();
        let mut source = SceneLifetime::prepared(owner, ticket, EPOCH).unwrap();
        uploaded(&mut source);
        let (mut host, catalogue, native) = host(&fixture);
        save(&native.repository, host.world());
        let mut stamp = DisplayStamp::from_host(&host);
        let stale = stamp;
        let active = [reference_key(0x300)];
        let (_task, candidate) = prepare(&mut host, &catalogue, &native, 1);
        let mut display = ContinueDisplay::new(
            &source,
            &mut stamp,
            &active,
            projected,
            |_, _: &ContinueBoundary| {},
        );
        host.publish_continue(candidate, &mut display).unwrap();
        assert_eq!(stamp.revision(), stale.revision());
        assert_ne!(stamp.host_identity(), stale.host_identity());
        let before = host.world().snapshot();
        stamp = stale;
        let calls = Cell::new(0);
        let (_task, candidate) = prepare(&mut host, &catalogue, &native, 2);
        let mut display = ContinueDisplay::new(
            &source,
            &mut stamp,
            &active,
            |world: &World<'_>, boundary: &ContinueBoundary| {
                calls.set(1);
                projected(world, boundary)
            },
            |_, _: &ContinueBoundary| calls.set(2),
        );
        assert!(host.publish_continue(candidate, &mut display).is_err());
        assert_eq!(calls.get(), 0);
        assert_eq!(host.world().snapshot(), before);
        assert_eq!(stamp, stale);
    }
}
