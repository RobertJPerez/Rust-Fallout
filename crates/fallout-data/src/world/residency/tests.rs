use super::*;
use crate::{
    archive::NvArchive,
    identity::ProfileId,
    plugin,
    resource_jobs::tests::{Fixture, Pause},
    store::RecordStore,
    vfs::MountIndex,
};
use std::{
    fs,
    io::Write,
    thread,
    time::{Duration, Instant},
};

mod terrain;
mod textures;

struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn sub(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
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
fn setup(count: usize, missing: bool) -> (Fixture, CellModelPlan, Vec<u8>) {
    let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    nif.extend(0x14020007u32.to_le_bytes());
    nif.push(1);
    for value in [11u32, 0, 34] {
        nif.extend(value.to_le_bytes());
    }
    nif.extend([0; 21]);
    setup_payload(count, missing, nif)
}
fn setup_payload(count: usize, missing: bool, nif: Vec<u8>) -> (Fixture, CellModelPlan, Vec<u8>) {
    setup_payload_extended(count, missing, nif, false, &[], &[])
}
fn setup_payload_extended(
    count: usize,
    missing: bool,
    nif: Vec<u8>,
    exterior: bool,
    extra_members: &[u8],
    extra_records: &[u8],
) -> (Fixture, CellModelPlan, Vec<u8>) {
    let fixture = Fixture::new(&nif, true);
    let mut esm = record(
        b"TES4",
        0,
        &sub(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    let mut members = Vec::new();
    let names: Vec<_> = (0..count).map(|i| format!("m{i:02}.nif")).collect();
    // Authored BSA104 physical table, independent of the production importer.
    let name_bytes: Vec<_> = names.iter().flat_map(|s| s.bytes().chain([0])).collect();
    let data_offset = 60 + count * 16 + name_bytes.len();
    let mut bsa = vec![0; data_offset];
    bsa[..4].copy_from_slice(b"BSA\0");
    for (at, word) in [
        (4, 104),
        (8, 36),
        (12, 7),
        (16, 1),
        (20, count as u32),
        (24, 7),
        (28, name_bytes.len() as u32),
        (44, count as u32),
        (48, 52),
    ] {
        bsa[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    bsa[52] = 7;
    bsa[53..60].copy_from_slice(b"meshes\0");
    bsa[60 + 16 * count..].copy_from_slice(&name_bytes);
    for (i, name) in names.iter().enumerate() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&nif).unwrap();
        let stored = [
            (nif.len() as u32).to_le_bytes().as_slice(),
            &encoder.finish().unwrap(),
        ]
        .concat();
        let at = 60 + 16 * i;
        let offset = bsa.len() as u32;
        bsa[at..at + 8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        bsa[at + 8..at + 12].copy_from_slice(&(stored.len() as u32).to_le_bytes());
        bsa[at + 12..at + 16].copy_from_slice(&offset.to_le_bytes());
        bsa.extend(stored);
        esm.extend(record(
            b"STAT",
            0x400 + i as u32,
            &sub(
                b"MODL",
                &[
                    if missing && i == 0 {
                        b"missing.nif"
                    } else {
                        name.as_bytes()
                    },
                    &[0],
                ]
                .concat(),
            ),
        ));
        members.extend(record(
            b"REFR",
            0x300 + i as u32,
            &[
                sub(b"NAME", &(0x400 + i as u32).to_le_bytes()),
                sub(b"DATA", &[0; 24]),
            ]
            .concat(),
        ));
    }
    let path = fixture.source.path().join("models.bsa");
    fs::write(&path, bsa).unwrap();
    let mut mounts = MountIndex::default();
    NvArchive::open(&path).unwrap().census(&mut mounts).unwrap();
    let cell = record(
        b"CELL",
        0x200,
        &[
            sub(b"EDID", b"ResidentClinic\0"),
            sub(b"DATA", &[u8::from(!exterior)]),
            if exterior {
                sub(b"XCLC", &[0; 12])
            } else {
                Vec::new()
            },
        ]
        .concat(),
    );
    members.extend(extra_members);
    let children = group(6, &group(9, &members));
    if exterior {
        esm.extend(record(b"WRLD", 0x100, &sub(b"DATA", &[0])));
        let mut world = group(1, &[cell, children].concat());
        world[8..12].copy_from_slice(&0x100u32.to_le_bytes());
        esm.extend(world);
    } else {
        esm.extend(cell);
        esm.extend(children);
    }
    esm.extend(extra_records);
    fs::write(fixture.source.path().join("FalloutNV.esm"), esm).unwrap();
    let mut store = RecordStore::open_nv_headers(
        fixture.source.path(),
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
    (fixture, plan, nif)
}
fn owner(fixture: &Fixture, models: usize, source_bytes: usize) -> CellResidency {
    CellResidency::new(
        fixture.source.path(),
        Some(fixture.cache.path()),
        Limits {
            workers: 1,
            models,
            source_bytes,
            ..Default::default()
        },
    )
    .unwrap()
}
fn decoded(owner: &mut CellResidency) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = owner.poll().unwrap();
        if state.stage == Stage::Decoded {
            break;
        }
        assert!(Instant::now() < deadline, "source jobs did not complete");
        thread::sleep(Duration::from_millis(1));
    }
}
fn drained(owner: &mut CellResidency) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while owner.poll().unwrap().outstanding != 0 {
        assert!(Instant::now() < deadline, "source pins did not drain");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(owner.snapshot().pinned_source_bytes, 0);
}

fn ready(owner: &mut CellResidency, ticket: &Ticket) {
    if owner.textures.is_none() {
        let plan = TexturePlan::load(
            owner.sources(ticket).unwrap(),
            &MountIndex::default(),
            Default::default(),
        )
        .unwrap();
        owner.request_textures(ticket, plan).unwrap();
    }
    owner.report_dependencies(ticket, Readiness::Ready).unwrap();
}

#[test]
fn source_batch_over_eight_feeds_existing_decoder_then_separate_readiness() {
    let (fixture, plan, nif) = setup(12, false);
    let mut owner = owner(&fixture, 12, 12 * nif.len());
    let ticket = owner.request(plan).unwrap();
    assert_eq!(owner.snapshot().stage, Stage::IoPending);
    assert!(owner.sources(&ticket).is_err());
    decoded(&mut owner);
    let sources = owner.sources(&ticket).unwrap();
    assert_eq!(sources.plan().unwrap().receipt().requests.len(), 12);
    for i in 0..12 {
        assert_eq!(sources.model(i).unwrap(), nif);
        let (decoded, scene) =
            crate::nif_scene::decode(sources.model(i).unwrap(), "authored-resident").unwrap();
        assert_eq!(decoded.bethesda_version, 34);
        assert!(scene.objects.is_empty());
    }
    assert!(!owner.snapshot().simulation_ready);
    let mut calls = 0;
    assert!(
        owner
            .publish_render(&ticket, || {
                calls += 1;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(calls, 0);
    ready(&mut owner, &ticket);
    assert_eq!(owner.snapshot().stage, Stage::DependenciesReady);
    owner
        .publish_render(&ticket, || {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(owner.snapshot().stage, Stage::RenderResident);
    owner.report_behavior(&ticket, Readiness::Ready).unwrap();
    assert!(!owner.snapshot().simulation_ready);
    owner
        .report_collision(&ticket, Readiness::Unsupported)
        .unwrap();
    assert!(!owner.snapshot().simulation_ready);
    owner.report_collision(&ticket, Readiness::Ready).unwrap();
    assert!(owner.snapshot().simulation_ready);
    owner
        .report_dependencies(&ticket, Readiness::Pending)
        .unwrap();
    assert!(!owner.snapshot().simulation_ready);
    assert_eq!(owner.snapshot().stage, Stage::Decoded);
    owner.unload().unwrap();
    assert_eq!(owner.snapshot().pinned_source_bytes, 12 * nif.len());
    assert!(matches!(sources.model(0), Err(JobError::Stale)));
    drop(sources);
    drained(&mut owner);
    assert_eq!(owner.snapshot().stage, Stage::Unrequested);
}

#[test]
fn unload_keeps_borrowed_payload_charged_and_retry_cannot_bypass_it() {
    let (fixture, plan, nif) = setup(1, false);
    let mut owner = owner(&fixture, 1, nif.len());
    let old = owner.request(plan.clone()).unwrap();
    decoded(&mut owner);
    let sources = owner.sources(&old).unwrap();
    owner.unload().unwrap();
    let fresh = owner.request(plan).unwrap();
    assert!(fresh.generation() > old.generation());
    let stalled = owner.poll().unwrap();
    assert!(stalled.backpressured);
    assert_eq!(stalled.pinned_source_bytes, nif.len());
    assert_eq!(stalled.completed_models, 0);
    let mut spawned = false;
    assert!(matches!(
        owner.publish_render(&old, || {
            spawned = true;
            Ok(())
        }),
        Err(JobError::Stale)
    ));
    assert!(!spawned);
    assert!(matches!(
        owner.report_collision(&old, Readiness::Ready),
        Err(JobError::Stale)
    ));
    drop(sources);
    decoded(&mut owner);
    assert_eq!(owner.sources(&fresh).unwrap().model(0).unwrap(), nif);
    owner.unload().unwrap();
    drained(&mut owner);
}

#[test]
fn controlled_unload_rejects_running_and_queued_source_outputs_before_retry() {
    for after_extract in [false, true] {
        let (fixture, plan, nif) = setup(2, false);
        let mut owner = owner(&fixture, 2, 2 * nif.len());
        let old = owner.request(plan.clone()).unwrap();
        let pause = Pause::new(after_extract);
        owner.pause = Some(pause.clone());
        let _release = Release(pause.clone());
        owner.poll().unwrap();
        pause.reached();
        owner.unload().unwrap();
        pause.release();
        pause.completed();
        drained(&mut owner);
        assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 0);
        assert!(matches!(old.check(), Err(JobError::Stale)));
        owner.pause = None;
        let fresh = owner.request(plan).unwrap();
        decoded(&mut owner);
        assert_eq!(owner.sources(&fresh).unwrap().model(1).unwrap(), nif);
        owner.unload().unwrap();
        drained(&mut owner);
    }
}

#[test]
fn admission_foreign_owner_and_missing_coverage_do_not_activate() {
    let (fixture, plan, nif) = setup(2, false);
    let mut small = owner(&fixture, 1, 2 * nif.len());
    assert!(matches!(
        small.request(plan.clone()),
        Err(JobError::QueueFull)
    ));
    assert_eq!(small.snapshot().stage, Stage::Unrequested);
    let mut bytes = owner(&fixture, 2, nif.len());
    assert!(matches!(
        bytes.request(plan.clone()),
        Err(JobError::ByteBudget)
    ));
    let mut a = owner(&fixture, 2, 2 * nif.len());
    let at = a.request(plan.clone()).unwrap();
    decoded(&mut a);
    let mut b = owner(&fixture, 2, 2 * nif.len());
    b.request(plan).unwrap();
    decoded(&mut b);
    assert!(matches!(
        b.report_collision(&at, Readiness::Ready),
        Err(JobError::Invalid(_))
    ));
    ready(&mut a, &at);
    assert!(
        a.publish_render::<()>(&at, || Err(JobError::Invalid(
            "GPU admission failed".into()
        )))
        .is_err()
    );
    assert_eq!(a.snapshot().stage, Stage::DependenciesReady);
    let (missing_fixture, missing_plan, _) = setup(1, true);
    let mut missing = owner(&missing_fixture, 1, nif.len());
    let mt = missing.request(missing_plan).unwrap();
    decoded(&mut missing);
    ready(&mut missing, &mt);
    missing.report_behavior(&mt, Readiness::Ready).unwrap();
    missing.report_collision(&mt, Readiness::Ready).unwrap();
    assert!(!missing.snapshot().complete_model_coverage);
    assert!(!missing.snapshot().simulation_ready);
}

#[test]
fn corrupt_cache_fails_without_publication_and_owner_drop_revokes_retained_sources() {
    let (fixture, plan, nif) = setup(1, false);
    let mut owner = owner(&fixture, 1, nif.len());
    let old = owner.request(plan).unwrap();
    decoded(&mut owner);
    let blob = fs::read_dir(fixture.cache.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "blob"))
        .unwrap();
    fs::write(blob, b"corrupt").unwrap();
    let fresh = owner.retry().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Err(error) = owner.poll() {
            assert!(error.to_string().contains("cache"));
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(owner.snapshot().stage, Stage::Failed);
    assert!(owner.sources(&fresh).is_err());
    assert!(old.check().is_err());
    let (fixture, plan, nif) = setup(1, false);
    let mut active = CellResidency::new(fixture.source.path(), None, Limits::default()).unwrap();
    let ticket = active.request(plan).unwrap();
    decoded(&mut active);
    let sources = active.sources(&ticket).unwrap();
    assert_eq!(sources.model(0).unwrap(), nif);
    drop(active);
    assert!(matches!(ticket.check(), Err(JobError::Closed)));
    assert!(sources.model(0).is_err());
}

#[test]
fn empty_selected_batches_still_charge_retired_plan_metadata_and_pin_count() {
    let (fixture, plan, nif) = setup(1, true);
    let mut owner = owner(&fixture, 1, nif.len());
    let first = owner.request(plan.clone()).unwrap();
    decoded(&mut owner);
    let first_sources = owner.sources(&first).unwrap();
    owner.unload().unwrap();
    assert_eq!(owner.snapshot().pinned_source_bytes, 0);
    assert_eq!(owner.poll().unwrap().stage, Stage::Unloading);
    let second = owner.request(plan.clone()).unwrap();
    decoded(&mut owner);
    let second_sources = owner.sources(&second).unwrap();
    owner.unload().unwrap();
    assert_eq!(owner.snapshot().retained_plans, 2);
    assert!(owner.snapshot().plan_metadata_bytes > 0);
    assert!(matches!(owner.request(plan), Err(JobError::QueueFull)));
    drop(first_sources);
    drop(second_sources);
    assert_eq!(owner.poll().unwrap().stage, Stage::Unrequested);
    assert_eq!(owner.snapshot().retained_plans, 0);
    assert_eq!(owner.snapshot().plan_metadata_bytes, 0);
}

#[test]
fn worker_ceiling_is_enforced_before_pool_creation() {
    let (fixture, _, _) = setup(0, false);
    for workers in [0, 3, 4, usize::MAX] {
        assert!(matches!(
            CellResidency::new(
                fixture.source.path(),
                None,
                Limits {
                    workers,
                    ..Limits::default()
                }
            ),
            Err(JobError::Invalid(_))
        ));
    }
    for workers in [1, 2] {
        assert!(
            CellResidency::new(
                fixture.source.path(),
                None,
                Limits {
                    workers,
                    ..Limits::default()
                }
            )
            .is_ok()
        );
    }
}

#[test]
fn publication_is_once_per_epoch_even_after_dependency_downgrade() {
    let (fixture, plan, nif) = setup(1, false);
    let mut owner = owner(&fixture, 1, nif.len());
    let ticket = owner.request(plan).unwrap();
    decoded(&mut owner);
    ready(&mut owner, &ticket);
    let mut calls = 0;
    assert!(
        owner
            .publish_render::<()>(&ticket, || {
                calls += 1;
                Err(JobError::Invalid("failed GPU admission".into()))
            })
            .is_err()
    );
    assert!(!owner.snapshot().render_published);
    owner
        .publish_render(&ticket, || {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 2);
    assert!(owner.snapshot().render_published);
    assert!(
        owner
            .publish_render(&ticket, || {
                calls += 1;
                Ok(())
            })
            .is_err()
    );
    owner
        .report_dependencies(&ticket, Readiness::Pending)
        .unwrap();
    owner
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap();
    assert!(
        owner
            .publish_render(&ticket, || {
                calls += 1;
                Ok(())
            })
            .is_err()
    );
    assert_eq!(calls, 2);
    assert!(owner.snapshot().render_published);
    assert_eq!(owner.snapshot().stage, Stage::RenderResident);
    let fresh = owner.retry().unwrap();
    assert!(!owner.snapshot().render_published);
    decoded(&mut owner);
    ready(&mut owner, &fresh);
    assert!(
        owner
            .publish_render(&ticket, || {
                calls += 1;
                Ok(())
            })
            .is_err()
    );
    owner
        .publish_render(&fresh, || {
            calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(calls, 3);
    owner.unload().unwrap();
    assert!(!owner.snapshot().render_published);
}
