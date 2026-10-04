#[path = "../src/engine_bridge/mod.rs"]
mod engine_bridge;

use engine_bridge::{Phase, SceneLifetime};
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::JobError,
    store::RecordStore,
    vfs::MountIndex,
    world::{preparation::CellModelPlan, residency::*},
};
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
