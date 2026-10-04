use super::*;
use crate::terrain::preparation::TextureSourcePlan;

fn setup(
    model_textures: usize,
    terrain_textures: usize,
    unresolved: bool,
) -> (Fixture, CellModelPlan, TextureSourcePlan, MountIndex, usize) {
    let names: Vec<_> = (0..model_textures)
        .map(|i| format!("t{i:02}.dds").into_bytes())
        .collect();
    let model_paths: Vec<_> = names.iter().map(Vec::as_slice).collect();
    let nif = textures::textured(&model_paths, false);
    let land = record(
        b"LAND",
        0x600,
        &sub(
            b"BTXT",
            &[0x700u32.to_le_bytes().as_slice(), &[0, 0, 255, 255]].concat(),
        ),
    );
    let mut txst = Vec::new();
    let terrain_names: Vec<_> = (0..terrain_textures)
        .map(|i| format!("l{i:02}.dds").into_bytes())
        .collect();
    for (i, path) in terrain_names.iter().enumerate() {
        let tag = [b'T', b'X', b'0', b'0' + i as u8];
        txst.extend(sub(
            &tag,
            &[
                if unresolved {
                    b"absent.dds"
                } else {
                    path.as_slice()
                },
                &[0],
            ]
            .concat(),
        ));
    }
    let extra = [
        record(b"LTEX", 0x700, &sub(b"TNAM", &0x800u32.to_le_bytes())),
        record(b"TXST", 0x800, &txst),
    ]
    .concat();
    let (fixture, _, _) = setup_payload_extended(1, false, nif.clone(), true, &land, &extra);
    let all_names: Vec<_> = names
        .iter()
        .chain(&terrain_names)
        .map(Vec::as_slice)
        .collect();
    let (mut mounts, _) =
        textures::texture_archive(&fixture, "textures", &all_names, b"terrain source");
    NvArchive::open(&fixture.source.path().join("models.bsa"))
        .unwrap()
        .census(&mut mounts)
        .unwrap();
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
    let models = CellModelPlan::load(&mut store, &root, &mounts, Default::default()).unwrap();
    let terrain = TextureSourcePlan::load(&mut store, &root, &mounts, Default::default()).unwrap();
    (fixture, models, terrain, mounts, nif.len())
}
fn terrain_decoded(owner: &mut CellResidency) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while owner.poll().unwrap().terrain_state == TerrainState::IoPending {
        assert!(Instant::now() < deadline, "terrain jobs did not complete");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn shared_terrain_and_model_textures_have_bounded_fair_polling_and_one_epoch() {
    let (fixture, models, terrain, mounts, nif_bytes) = setup(10, 4, false);
    let mut owner = owner(&fixture, 1, nif_bytes + 14 * 14);
    let ticket = owner.request(models).unwrap();
    decoded(&mut owner);
    let texture_plan =
        TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default()).unwrap();
    owner.request_textures(&ticket, texture_plan).unwrap();
    owner.request_terrain(&ticket, terrain).unwrap();
    let pause = Pause::new(false);
    owner.pause = Some(pause.clone());
    let _release = Release(pause.clone());
    let first = owner.poll().unwrap();
    pause.reached();
    assert_eq!(first.outstanding, 9); // model + eight model textures
    let second = owner.poll().unwrap();
    assert_eq!(second.outstanding, 13); // next bounded poll admits terrain
    let third = owner.poll().unwrap();
    assert_eq!(third.outstanding, 15); // remaining two model textures
    assert!(
        owner
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    pause.release();
    terrain_decoded(&mut owner);
    textures::textures_decoded(&mut owner);
    let lease = owner.terrain_sources(&ticket).unwrap();
    assert_eq!(lease.ticket().generation(), ticket.generation());
    assert_eq!(lease.ticket().identity(), ticket.identity());
    assert_eq!(lease.texture(0).unwrap(), b"terrain source");
    assert_eq!(lease.receipt().unwrap().requests.len(), 4);
    assert!(!lease.terrain().unwrap().runtime_ready);
    assert!(lease.texture(4).is_err());
    let ready = owner.snapshot();
    assert_eq!(ready.completed_terrain_textures, 4);
    assert_eq!(ready.pinned_source_bytes, nif_bytes + 14 * 14);
    assert!(!ready.simulation_ready);
    owner
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap();
    owner.report_collision(&ticket, Readiness::Ready).unwrap();
    assert!(!owner.snapshot().simulation_ready);
    owner.report_behavior(&ticket, Readiness::Ready).unwrap();
    assert!(owner.snapshot().simulation_ready); // explicit authored host reports
}

#[test]
fn terrain_unload_revokes_controlled_running_and_queued_jobs_without_cache_publication() {
    for after_extract in [false, true] {
        let (fixture, models, terrain, _, nif_bytes) = setup(0, 2, false);
        let mut owner = owner(&fixture, 1, nif_bytes + 28);
        let ticket = owner.request(models.clone()).unwrap();
        decoded(&mut owner);
        owner.request_terrain(&ticket, terrain.clone()).unwrap();
        let before_cache = fs::read_dir(fixture.cache.path()).unwrap().count();
        let pause = Pause::new(after_extract);
        owner.pause = Some(pause.clone());
        let _release = Release(pause.clone());
        assert_eq!(owner.poll().unwrap().outstanding, 3);
        pause.reached();
        owner.unload().unwrap();
        assert!(ticket.check().is_err());
        assert!(owner.terrain_sources(&ticket).is_err());
        assert!(owner.request_terrain(&ticket, terrain.clone()).is_err());
        assert!(
            owner
                .publish_render(&ticket, || -> JobResult<()> { panic!("stale publication") })
                .is_err()
        );
        pause.release();
        pause.completed();
        drained(&mut owner);
        assert_eq!(owner.poll().unwrap().stage, Stage::Unrequested);
        assert_eq!(owner.snapshot().plan_metadata_bytes, 0);
        assert_eq!(owner.snapshot().mapped_source_bytes, 0);
        assert_eq!(
            fs::read_dir(fixture.cache.path()).unwrap().count(),
            before_cache
        );
        owner.pause = None;
        let fresh = owner.request(models).unwrap();
        decoded(&mut owner);
        owner.request_terrain(&fresh, terrain).unwrap();
        terrain_decoded(&mut owner);
        assert_eq!(
            owner.terrain_sources(&fresh).unwrap().texture(1).unwrap(),
            b"terrain source"
        );
    }
}

#[test]
fn retained_terrain_keeps_payload_and_plan_charges_until_last_lease_drops() {
    let (fixture, models, terrain, _, nif_bytes) = setup(0, 2, false);
    let mut owner = owner(&fixture, 1, nif_bytes + 28);
    let ticket = owner.request(models.clone()).unwrap();
    decoded(&mut owner);
    owner.request_terrain(&ticket, terrain.clone()).unwrap();
    terrain_decoded(&mut owner);
    let lease = owner.terrain_sources(&ticket).unwrap();
    let before = owner.snapshot();
    owner.unload().unwrap();
    let retired = owner.poll().unwrap();
    assert_eq!(retired.stage, Stage::Unloading);
    assert_eq!(retired.outstanding, 3);
    assert_eq!(retired.pinned_source_bytes, before.pinned_source_bytes);
    assert_eq!(retired.plan_metadata_bytes, before.plan_metadata_bytes);
    assert_eq!(retired.mapped_source_bytes, before.mapped_source_bytes);
    assert!(lease.texture(0).is_err());
    assert!(lease.receipt().is_err());
    assert!(lease.terrain().is_err());
    let fresh = owner.request(models).unwrap();
    assert!(owner.poll().unwrap().backpressured);
    assert_eq!(owner.snapshot().retained_plans, 2);
    drop(lease);
    decoded(&mut owner);
    assert_eq!(owner.snapshot().retained_plans, 1);
    owner.request_terrain(&fresh, terrain).unwrap();
    terrain_decoded(&mut owner);
    assert_eq!(owner.snapshot().pinned_source_bytes, nif_bytes + 28);
}

#[test]
fn unresolved_terrain_blocks_dependency_admission_and_late_scope_is_refused() {
    let (fixture, models, terrain, mounts, nif_bytes) = setup(0, 1, true);
    let mut owner = owner(&fixture, 1, nif_bytes + 14);
    let ticket = owner.request(models.clone()).unwrap();
    decoded(&mut owner);
    let texture_plan =
        TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default()).unwrap();
    owner.request_textures(&ticket, texture_plan).unwrap();
    owner.request_terrain(&ticket, terrain.clone()).unwrap();
    let state = owner.snapshot();
    assert_eq!(state.terrain_state, TerrainState::Unsupported);
    assert_eq!(state.requested_terrain_textures, 0);
    assert!(!state.complete_terrain_coverage);
    assert!(
        owner
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    assert!(owner.request_terrain(&ticket, terrain.clone()).is_err());
    let fresh = owner.retry().unwrap();
    decoded(&mut owner);
    let texture_plan =
        TexturePlan::load(owner.sources(&fresh).unwrap(), &mounts, Default::default()).unwrap();
    owner.request_textures(&fresh, texture_plan).unwrap();
    owner.report_dependencies(&fresh, Readiness::Ready).unwrap();
    assert_eq!(owner.snapshot().terrain_state, TerrainState::Unrequested);
    assert!(owner.request_terrain(&fresh, terrain.clone()).is_err());
    owner
        .report_dependencies(&fresh, Readiness::Pending)
        .unwrap();
    assert!(owner.request_terrain(&fresh, terrain.clone()).is_err());
    owner.report_dependencies(&fresh, Readiness::Ready).unwrap();
    owner.publish_render(&fresh, || Ok(())).unwrap();
    owner
        .report_dependencies(&fresh, Readiness::Pending)
        .unwrap();
    assert!(owner.request_terrain(&fresh, terrain).is_err());
}

#[test]
fn aggregate_payload_count_and_bytes_refuse_without_revoking_existing_batches() {
    for terrain_first in [false, true] {
        for count_limit in [false, true] {
            let (fixture, models, terrain, mounts, nif_bytes) = setup(1, 1, false);
            let mut owner = CellResidency::new(
                fixture.source.path(),
                Some(fixture.cache.path()),
                Limits {
                    workers: 1,
                    models: 1,
                    resources: if count_limit { 2 } else { 3 },
                    source_bytes: if count_limit {
                        nif_bytes + 28
                    } else {
                        nif_bytes + 27
                    },
                    ..Default::default()
                },
            )
            .unwrap();
            let ticket = owner.request(models).unwrap();
            decoded(&mut owner);
            let texture =
                TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default())
                    .unwrap();
            let result = if terrain_first {
                owner.request_terrain(&ticket, terrain).unwrap();
                owner.request_textures(&ticket, texture)
            } else {
                owner.request_textures(&ticket, texture).unwrap();
                let before = owner.snapshot();
                let result = owner.request_terrain(&ticket, terrain);
                assert_eq!(
                    owner.snapshot().plan_metadata_bytes,
                    before.plan_metadata_bytes
                );
                assert_eq!(
                    owner.snapshot().mapped_source_bytes,
                    before.mapped_source_bytes
                );
                result
            };
            if count_limit {
                assert!(matches!(result, Err(JobError::QueueFull)));
            } else {
                assert!(matches!(result, Err(JobError::ByteBudget)));
            }
            ticket.check().unwrap();
            if terrain_first {
                terrain_decoded(&mut owner);
            } else {
                textures::textures_decoded(&mut owner);
            }
            assert_eq!(owner.snapshot().outstanding, 2);
        }
    }
    let (fixture, models, terrain, mounts, nif_bytes) = setup(1, 1, false);
    let mut owner = CellResidency::new(
        fixture.source.path(),
        Some(fixture.cache.path()),
        Limits {
            workers: 1,
            models: 1,
            resources: 3,
            source_bytes: nif_bytes + 28,
            ..Default::default()
        },
    )
    .unwrap();
    let ticket = owner.request(models).unwrap();
    decoded(&mut owner);
    let texture =
        TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default()).unwrap();
    owner.request_textures(&ticket, texture).unwrap();
    owner.request_terrain(&ticket, terrain).unwrap();
    terrain_decoded(&mut owner);
    textures::textures_decoded(&mut owner);
    assert_eq!(owner.snapshot().pinned_source_bytes, nif_bytes + 28);
    assert_eq!(owner.snapshot().outstanding, 3);
}

#[test]
fn terrain_plan_mapping_and_metadata_limits_accept_exact_and_refuse_one_under() {
    let (fixture, models, terrain, _, nif_bytes) = setup(0, 1, false);
    let metadata = models.receipt().usage.metadata_bytes + terrain.receipt().usage.metadata_bytes;
    let mapped: u64 = models
        .receipt()
        .archives
        .iter()
        .chain(&terrain.receipt().archives)
        .map(|a| a.source_bytes)
        .sum();
    for kind in 0..3 {
        let mut owner = CellResidency::new(
            fixture.source.path(),
            Some(fixture.cache.path()),
            Limits {
                workers: 1,
                models: 1,
                source_bytes: nif_bytes + 14,
                plan_metadata_bytes: metadata - usize::from(kind == 1),
                mapped_source_bytes: mapped - u64::from(kind == 2),
                ..Default::default()
            },
        )
        .unwrap();
        let ticket = owner.request(models.clone()).unwrap();
        decoded(&mut owner);
        let before = owner.snapshot();
        let result = owner.request_terrain(&ticket, terrain.clone());
        if kind == 0 {
            result.unwrap();
            terrain_decoded(&mut owner);
            assert_eq!(owner.snapshot().plan_metadata_bytes, metadata);
            assert_eq!(owner.snapshot().mapped_source_bytes, mapped);
        } else {
            assert!(matches!(result, Err(JobError::ByteBudget)));
            assert_eq!(
                owner.snapshot().plan_metadata_bytes,
                before.plan_metadata_bytes
            );
            assert_eq!(
                owner.snapshot().mapped_source_bytes,
                before.mapped_source_bytes
            );
            assert_eq!(owner.snapshot().terrain_state, TerrainState::Unrequested);
            ticket.check().unwrap();
        }
    }
}

#[test]
fn foreign_owner_and_changed_full_source_cohort_cannot_retain_terrain() {
    let (fixture, models, terrain, mounts, nif_bytes) = setup(0, 1, false);
    let mut owner = owner(&fixture, 1, nif_bytes + 14);
    let mut foreign = super::owner(&fixture, 1, nif_bytes + 14);
    let ticket = owner.request(models.clone()).unwrap();
    let other = foreign.request(models).unwrap();
    decoded(&mut owner);
    decoded(&mut foreign);
    let before = owner.snapshot();
    assert!(matches!(
        owner.request_terrain(&other, terrain.clone()),
        Err(JobError::Invalid(_))
    ));
    assert_eq!(
        owner.snapshot().plan_metadata_bytes,
        before.plan_metadata_bytes
    );

    // Same CELL identity and terrain fields, changed unreferenced source record.
    let (changed, _, _, _, _) = setup(0, 1, false);
    let path = changed.source.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"TXST", 0x900, &sub(b"ZZZZ", &[1])));
    fs::write(&path, bytes).unwrap();
    let mut store = RecordStore::open_nv_headers(
        changed.source.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let changed_plan =
        TextureSourcePlan::load(&mut store, ticket.root(), &mounts, Default::default()).unwrap();
    assert!(matches!(
        owner.request_terrain(&ticket, changed_plan),
        Err(JobError::Invalid(_))
    ));
    assert_eq!(
        owner.snapshot().plan_metadata_bytes,
        before.plan_metadata_bytes
    );
    assert_eq!(
        owner.snapshot().mapped_source_bytes,
        before.mapped_source_bytes
    );
    owner.request_terrain(&ticket, terrain).unwrap();
    terrain_decoded(&mut owner);
}

#[test]
fn owner_drop_closes_a_retained_terrain_lease() {
    let (fixture, models, terrain, _, nif_bytes) = setup(0, 1, false);
    let mut owner = owner(&fixture, 1, nif_bytes + 14);
    let ticket = owner.request(models).unwrap();
    decoded(&mut owner);
    owner.request_terrain(&ticket, terrain).unwrap();
    terrain_decoded(&mut owner);
    let lease = owner.terrain_sources(&ticket).unwrap();
    assert_eq!(lease.texture(0).unwrap(), b"terrain source");
    drop(owner);
    assert!(ticket.check().is_err());
    assert!(lease.texture(0).is_err());
    assert!(lease.receipt().is_err());
}

#[test]
fn cancellation_of_both_pending_texture_batches_drains_shared_sidecars_before_retry() {
    for after_extract in [false, true] {
        for replace in [false, true] {
            let (fixture, models, terrain, mounts, nif_bytes) = setup(3, 2, false);
            let mut owner = owner(&fixture, 1, nif_bytes + 70);
            let old = owner.request(models.clone()).unwrap();
            decoded(&mut owner);
            let textures =
                TexturePlan::load(owner.sources(&old).unwrap(), &mounts, Default::default())
                    .unwrap();
            owner.request_textures(&old, textures).unwrap();
            owner.request_terrain(&old, terrain.clone()).unwrap();
            let cached_models = fs::read_dir(fixture.cache.path()).unwrap().count();
            let pause = Pause::new(after_extract);
            owner.pause = Some(pause.clone());
            let _release = Release(pause.clone());
            assert_eq!(owner.poll().unwrap().outstanding, 4);
            pause.reached();
            let pending = owner.poll().unwrap();
            assert_eq!(pending.outstanding, 6);
            assert_eq!(pending.texture_state, TextureState::IoPending);
            assert_eq!(pending.terrain_state, TerrainState::IoPending);
            assert_eq!(pending.pinned_source_bytes, nif_bytes + 70);
            let fresh = if replace {
                Some(owner.request(models.clone()).unwrap())
            } else {
                owner.unload().unwrap();
                None
            };
            assert!(old.check().is_err());
            assert!(owner.texture_sources(&old).is_err());
            assert!(owner.terrain_sources(&old).is_err());
            assert!(owner.request_terrain(&old, terrain.clone()).is_err());
            pause.release();
            pause.completed();
            owner.pause = None;
            let fresh = match fresh {
                Some(ticket) => {
                    decoded(&mut owner);
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while owner.poll().unwrap().retained_plans != 1
                        || owner.snapshot().outstanding != 1
                    {
                        assert!(
                            Instant::now() < deadline,
                            "retired shared batches did not drain"
                        );
                        thread::sleep(Duration::from_millis(1));
                    }
                    assert_eq!(owner.snapshot().pinned_source_bytes, nif_bytes);
                    ticket
                }
                None => {
                    drained(&mut owner);
                    assert_eq!(owner.poll().unwrap().stage, Stage::Unrequested);
                    assert_eq!(owner.snapshot().plan_metadata_bytes, 0);
                    assert_eq!(owner.snapshot().mapped_source_bytes, 0);
                    let ticket = owner.request(models).unwrap();
                    decoded(&mut owner);
                    ticket
                }
            };
            assert_ne!(fresh.generation(), old.generation());
            // Neither the running member nor either queued batch published a
            // cache artifact after revocation; the fresh model reuses its pin.
            assert_eq!(
                fs::read_dir(fixture.cache.path()).unwrap().count(),
                cached_models
            );
            let textures =
                TexturePlan::load(owner.sources(&fresh).unwrap(), &mounts, Default::default())
                    .unwrap();
            owner.request_textures(&fresh, textures).unwrap();
            owner.request_terrain(&fresh, terrain).unwrap();
            terrain_decoded(&mut owner);
            super::textures::textures_decoded(&mut owner);
            assert_eq!(owner.snapshot().outstanding, 6);
            assert_eq!(owner.snapshot().pinned_source_bytes, nif_bytes + 70);
            assert_eq!(
                owner.texture_sources(&fresh).unwrap().texture(0).unwrap(),
                b"terrain source"
            );
            assert_eq!(
                owner.terrain_sources(&fresh).unwrap().texture(0).unwrap(),
                b"terrain source"
            );
            assert!(!owner.snapshot().simulation_ready);
        }
    }
}
