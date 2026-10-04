use super::*;
use crate::vfs::AssetSource;

fn textured(paths: &[&[u8]], unknown: bool) -> Vec<u8> {
    textured_named(paths, unknown.then_some("UnsupportedTextureCarrier"))
}
fn textured_named(paths: &[&[u8]], unknown: Option<&str>) -> Vec<u8> {
    let mut payload = (paths.len() as u32).to_le_bytes().to_vec();
    for path in paths {
        payload.extend((path.len() as u32).to_le_bytes());
        payload.extend(*path);
    }
    let blocks = if let Some(name) = unknown {
        vec![("BSShaderTextureSet", payload), (name, vec![0; 4])]
    } else {
        vec![("BSShaderTextureSet", payload)]
    };
    // Independent authored NIF20.2.0.7 container, not a second production parser.
    let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    nif.extend(0x14020007u32.to_le_bytes());
    nif.push(1);
    for value in [11, blocks.len() as u32, 34] {
        nif.extend(value.to_le_bytes());
    }
    nif.extend([0; 3]);
    nif.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in &blocks {
        nif.extend((name.len() as u32).to_le_bytes());
        nif.extend(name.as_bytes());
    }
    for i in 0..blocks.len() {
        nif.extend((i as u16).to_le_bytes());
    }
    for (_, bytes) in &blocks {
        nif.extend((bytes.len() as u32).to_le_bytes());
    }
    for value in [1u32, 4, 4] {
        nif.extend(value.to_le_bytes());
    }
    nif.extend(b"name");
    nif.extend(0u32.to_le_bytes());
    for (_, bytes) in blocks {
        nif.extend(bytes);
    }
    nif.extend(0u32.to_le_bytes());
    nif
}

fn texture_archive(
    fixture: &Fixture,
    label: &str,
    names: &[&[u8]],
    payload: &[u8],
) -> (MountIndex, u64) {
    let names_bytes: Vec<_> = names
        .iter()
        .flat_map(|name| name.iter().copied().chain([0]))
        .collect();
    let table = 62;
    let data_offset = table + names.len() * 16 + names_bytes.len();
    let mut bsa = vec![0; data_offset];
    bsa[..4].copy_from_slice(b"BSA\0");
    for (at, word) in [
        (4, 104),
        (8, 36),
        (12, 7),
        (16, 1),
        (20, names.len() as u32),
        (24, 9),
        (28, names_bytes.len() as u32),
        (44, names.len() as u32),
        (48, 52),
    ] {
        bsa[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    bsa[52] = 9;
    bsa[53..62].copy_from_slice(b"textures\0");
    bsa[table + 16 * names.len()..].copy_from_slice(&names_bytes);
    for i in 0..names.len() {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(payload).unwrap();
        let stored = [
            (payload.len() as u32).to_le_bytes().as_slice(),
            &encoder.finish().unwrap(),
        ]
        .concat();
        let offset = bsa.len() as u32;
        let at = table + i * 16;
        bsa[at..at + 8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        bsa[at + 8..at + 12].copy_from_slice(&(stored.len() as u32).to_le_bytes());
        bsa[at + 12..at + 16].copy_from_slice(&offset.to_le_bytes());
        bsa.extend(stored);
    }
    let path = fixture.source.path().join(format!("{label}.bsa"));
    let len = bsa.len() as u64;
    fs::write(&path, bsa).unwrap();
    let mut mounts = MountIndex::default();
    NvArchive::open(&path).unwrap().census(&mut mounts).unwrap();
    (mounts, len)
}

fn textures_decoded(owner: &mut CellResidency) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = owner.poll().unwrap();
        if matches!(
            state.texture_state,
            TextureState::Decoded | TextureState::Unsupported
        ) {
            break;
        }
        assert!(Instant::now() < deadline, "texture jobs did not complete");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn texture_batch_over_eight_reuses_jobs_cache_and_gates_dependencies() {
    let names: Vec<_> = (0..10)
        .map(|i| format!("t{i:02}.dds").into_bytes())
        .collect();
    let paths: Vec<_> = names.iter().map(Vec::as_slice).collect();
    let nif = textured(&paths, false);
    let (fixture, plan, _) = setup_payload(1, false, nif.clone());
    let payload = b"authored texture source bytes";
    let (mounts, mapped) = texture_archive(&fixture, "textures", &paths, payload);
    let mut owner = owner(&fixture, 1, nif.len() + 10 * payload.len());
    let ticket = owner.request(plan).unwrap();
    decoded(&mut owner);
    assert_eq!(owner.snapshot().texture_state, TextureState::Unrequested);
    assert!(
        owner
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    let before = owner.snapshot();
    let texture_plan =
        TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default()).unwrap();
    assert_eq!(texture_plan.receipt().usages.len(), 10);
    assert_eq!(texture_plan.receipt().requests.len(), 10);
    assert_eq!(texture_plan.receipt().mapped_bytes, mapped);
    assert_eq!(
        owner.snapshot().mapped_source_bytes,
        before.mapped_source_bytes + mapped
    );
    owner.request_textures(&ticket, texture_plan).unwrap();
    assert!(
        owner
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    assert!(owner.texture_sources(&ticket).is_err());
    textures_decoded(&mut owner);
    let sources = owner.texture_sources(&ticket).unwrap();
    for i in 0..10 {
        assert_eq!(sources.texture(i).unwrap(), payload);
    }
    let state = owner.snapshot();
    assert_eq!(state.outstanding, 11);
    assert_eq!(state.pinned_source_bytes, nif.len() + 10 * payload.len());
    assert!(state.complete_texture_coverage);
    assert!(state.texture_reference_coverage_verified);
    assert_eq!(sources.receipt().unwrap().identity.len(), 64);
    assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 22);
    owner
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap();
    owner.report_collision(&ticket, Readiness::Ready).unwrap();
    owner.report_behavior(&ticket, Readiness::Ready).unwrap();
    assert!(owner.snapshot().simulation_ready);
    owner.publish_render(&ticket, || Ok(())).unwrap();
    owner.unload().unwrap();
    assert!(matches!(sources.texture(0), Err(JobError::Stale)));
    assert_eq!(owner.snapshot().outstanding, 11);
    drop(sources);
    drained(&mut owner);
    assert_eq!(owner.snapshot().plan_metadata_bytes, 0);
    assert_eq!(owner.snapshot().mapped_source_bytes, 0);
}

#[test]
fn missing_ambiguous_and_absolute_paths_preserve_provenance_and_refuse_ready() {
    let nif = textured(
        &[
            b"ok.dds",
            b"missing.dds",
            b"ambiguous.dds",
            b"C:\\export\\absolute.dds",
            b"ok.dds",
        ],
        false,
    );
    let (fixture, plan, _) = setup_payload(1, false, nif.clone());
    let (mut mounts, _) = texture_archive(
        &fixture,
        "textures",
        &[b"ok.dds", b"ambiguous.dds"],
        b"texture",
    );
    mounts
        .insert(AssetSource {
            container: "second-unopened-archive.bsa".into(),
            entry_index: 0,
            original_path: b"textures/ambiguous.dds".to_vec(),
        })
        .unwrap();
    let mut owner = owner(&fixture, 1, nif.len() + 7);
    let ticket = owner.request(plan).unwrap();
    decoded(&mut owner);
    let texture_plan =
        TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default()).unwrap();
    let receipt = texture_plan.receipt();
    assert_eq!(receipt.usages.len(), 5);
    assert_eq!(receipt.requests.len(), 1);
    assert_eq!(receipt.missing_or_ambiguous, 3);
    assert_eq!(receipt.usages[2].candidates.len(), 2);
    assert_eq!(receipt.usages[3].raw_path, b"C:\\export\\absolute.dds");
    assert!(receipt.usages[3].path.is_none());
    assert!(receipt.usages[3].error.is_some());
    owner.request_textures(&ticket, texture_plan).unwrap();
    textures_decoded(&mut owner);
    assert_eq!(
        owner.texture_sources(&ticket).unwrap().texture(0).unwrap(),
        b"texture"
    );
    assert_eq!(owner.snapshot().texture_state, TextureState::Unsupported);
    assert!(!owner.snapshot().complete_texture_coverage);
    assert!(
        owner
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    owner.report_collision(&ticket, Readiness::Ready).unwrap();
    owner.report_behavior(&ticket, Readiness::Ready).unwrap();
    assert!(!owner.snapshot().simulation_ready);
    assert!(owner.publish_render(&ticket, || Ok(())).is_err());
}

#[test]
fn unknown_nif_blocks_allow_explicit_subset_but_block_full_simulation() {
    let nif = textured(&[], true);
    let (fixture, plan, _) = setup_payload(1, false, nif.clone());
    let mut owner = owner(&fixture, 1, nif.len());
    let ticket = owner.request(plan).unwrap();
    decoded(&mut owner);
    let textures = TexturePlan::load(
        owner.sources(&ticket).unwrap(),
        &MountIndex::default(),
        Default::default(),
    )
    .unwrap();
    assert!(!textures.receipt().reference_coverage_verified);
    assert_eq!(
        textures.receipt().models[0].unsupported_blocks["UnsupportedTextureCarrier"],
        [1]
    );
    owner.request_textures(&ticket, textures).unwrap();
    owner
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap();
    owner.report_collision(&ticket, Readiness::Ready).unwrap();
    owner.report_behavior(&ticket, Readiness::Ready).unwrap();
    assert!(owner.snapshot().complete_texture_coverage);
    assert!(!owner.snapshot().simulation_ready);
    owner.publish_render(&ticket, || Ok(())).unwrap();
}

#[test]
fn retained_texture_plan_and_outputs_charge_across_unload_and_retry() {
    let nif = textured(&[b"t.dds"], false);
    let (fixture, cell, _) = setup_payload(1, false, nif.clone());
    let payload = b"texture";
    let (mounts, _) = texture_archive(&fixture, "textures", &[b"t.dds"], payload);
    let mut owner = owner(&fixture, 1, nif.len() + payload.len());
    let old = owner.request(cell.clone()).unwrap();
    decoded(&mut owner);
    let model_sources = owner.sources(&old).unwrap();
    let plan = TexturePlan::load(model_sources.clone(), &mounts, Default::default()).unwrap();
    let retained = plan.clone();
    let charged = owner.snapshot();
    owner.request_textures(&old, plan).unwrap();
    textures_decoded(&mut owner);
    let sources = owner.texture_sources(&old).unwrap();
    owner.unload().unwrap();
    assert_eq!(
        owner.snapshot().plan_metadata_bytes,
        charged.plan_metadata_bytes
    );
    assert_eq!(
        owner.snapshot().mapped_source_bytes,
        charged.mapped_source_bytes
    );
    let before_stale_load = owner.snapshot();
    assert!(matches!(
        TexturePlan::load(model_sources.clone(), &mounts, Default::default()),
        Err(JobError::Stale)
    ));
    assert_eq!(
        owner.snapshot().plan_metadata_bytes,
        before_stale_load.plan_metadata_bytes
    );
    assert!(matches!(
        owner.request_textures(&old, retained.clone()),
        Err(JobError::Stale)
    ));
    assert!(matches!(sources.texture(0), Err(JobError::Stale)));
    let fresh = owner.request(cell).unwrap();
    assert!(owner.poll().unwrap().backpressured);
    drop(model_sources);
    drop(sources);
    // A plan clone alone still owns the old model source and all texture maps.
    assert_eq!(owner.snapshot().pinned_source_bytes, nif.len());
    assert!(owner.poll().unwrap().backpressured);
    drop(retained);
    decoded(&mut owner);
    let fresh_plan =
        TexturePlan::load(owner.sources(&fresh).unwrap(), &mounts, Default::default()).unwrap();
    owner.request_textures(&fresh, fresh_plan).unwrap();
    textures_decoded(&mut owner);
    assert_eq!(
        owner.texture_sources(&fresh).unwrap().texture(0).unwrap(),
        payload
    );
    owner.unload().unwrap();
    drained(&mut owner);
}

#[test]
fn controlled_texture_cancellation_rejects_running_and_queued_cache_publication() {
    for after_extract in [false, true] {
        let nif = textured(&[b"a.dds", b"b.dds"], false);
        let (fixture, cell, _) = setup_payload(1, false, nif.clone());
        let (mounts, _) = texture_archive(&fixture, "textures", &[b"a.dds", b"b.dds"], b"texture");
        let mut owner = owner(&fixture, 1, nif.len() + 14);
        let old = owner.request(cell.clone()).unwrap();
        decoded(&mut owner);
        let plan =
            TexturePlan::load(owner.sources(&old).unwrap(), &mounts, Default::default()).unwrap();
        owner.request_textures(&old, plan).unwrap();
        let cache_before = fs::read_dir(fixture.cache.path()).unwrap().count();
        let pause = Pause::new(after_extract);
        owner.pause = Some(pause.clone());
        let _release = Release(pause.clone());
        owner.poll().unwrap();
        pause.reached();
        owner.unload().unwrap();
        pause.release();
        pause.completed();
        drained(&mut owner);
        assert_eq!(
            fs::read_dir(fixture.cache.path()).unwrap().count(),
            cache_before
        );
        assert!(old.check().is_err());
        owner.pause = None;
        let fresh = owner.request(cell).unwrap();
        decoded(&mut owner);
        let plan =
            TexturePlan::load(owner.sources(&fresh).unwrap(), &mounts, Default::default()).unwrap();
        owner.request_textures(&fresh, plan).unwrap();
        textures_decoded(&mut owner);
        assert_eq!(
            owner.texture_sources(&fresh).unwrap().texture(1).unwrap(),
            b"texture"
        );
        owner.unload().unwrap();
        drained(&mut owner);
    }
}

#[test]
fn texture_budget_exact_edges_and_foreign_or_duplicate_admission() {
    let nif = textured(&[b"a.dds", b"b.dds"], false);
    let (fixture, cell, _) = setup_payload(1, false, nif.clone());
    let (mounts, texture_bytes) =
        texture_archive(&fixture, "textures", &[b"a.dds", b"b.dds"], b"texture");
    let mut owner = owner(&fixture, 1, nif.len() + 14);
    let ticket = owner.request(cell.clone()).unwrap();
    decoded(&mut owner);
    let before = owner.snapshot();
    for limits in [
        TextureLimits {
            references: 1,
            ..Default::default()
        },
        TextureLimits {
            requests: 1,
            ..Default::default()
        },
        TextureLimits {
            archives: 0,
            ..Default::default()
        },
        TextureLimits {
            metadata_bytes: 1023,
            ..Default::default()
        },
        TextureLimits {
            nif_blocks: 0,
            ..Default::default()
        },
        TextureLimits {
            nif_array_bytes: 0,
            ..Default::default()
        },
    ] {
        assert!(TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, limits).is_err());
        assert_eq!(
            owner.snapshot().plan_metadata_bytes,
            before.plan_metadata_bytes
        );
        assert_eq!(
            owner.snapshot().mapped_source_bytes,
            before.mapped_source_bytes
        );
    }
    let plan =
        TexturePlan::load(owner.sources(&ticket).unwrap(), &mounts, Default::default()).unwrap();
    let exact_metadata = plan.receipt().metadata_bytes;
    drop(plan);
    assert!(matches!(
        TexturePlan::load(
            owner.sources(&ticket).unwrap(),
            &mounts,
            TextureLimits {
                metadata_bytes: exact_metadata - 1,
                ..Default::default()
            }
        ),
        Err(JobError::ByteBudget)
    ));
    let exact = TexturePlan::load(
        owner.sources(&ticket).unwrap(),
        &mounts,
        TextureLimits {
            metadata_bytes: exact_metadata,
            references: 2,
            requests: 2,
            archives: 1,
            nif_blocks: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let mut foreign = super::owner(&fixture, 1, nif.len() + 14);
    let foreign_ticket = foreign.request(cell.clone()).unwrap();
    decoded(&mut foreign);
    assert!(matches!(
        foreign.request_textures(&foreign_ticket, exact.clone()),
        Err(JobError::Invalid(_))
    ));
    owner.request_textures(&ticket, exact.clone()).unwrap();
    assert!(owner.request_textures(&ticket, exact).is_err());
    textures_decoded(&mut owner);
    assert_eq!(owner.snapshot().requested_textures, 2);

    for (resources, source_bytes, maps, metadata, expected) in [
        (
            2,
            nif.len() + 14,
            Limits::default().mapped_source_bytes,
            Limits::default().plan_metadata_bytes,
            "queue",
        ),
        (
            3,
            nif.len() + 13,
            Limits::default().mapped_source_bytes,
            Limits::default().plan_metadata_bytes,
            "bytes",
        ),
        (
            3,
            nif.len() + 14,
            before.mapped_source_bytes + texture_bytes - 1,
            Limits::default().plan_metadata_bytes,
            "bytes",
        ),
        (
            3,
            nif.len() + 14,
            before.mapped_source_bytes + texture_bytes,
            before.plan_metadata_bytes + exact_metadata - 1,
            "bytes",
        ),
        (
            3,
            nif.len() + 14,
            before.mapped_source_bytes + texture_bytes,
            before.plan_metadata_bytes + exact_metadata,
            "ok",
        ),
    ] {
        let mut bounded = CellResidency::new(
            fixture.source.path(),
            None,
            Limits {
                workers: 1,
                models: 1,
                resources,
                source_bytes,
                mapped_source_bytes: maps,
                plan_metadata_bytes: metadata,
                ..Default::default()
            },
        )
        .unwrap();
        let bt = bounded.request(cell.clone()).unwrap();
        decoded(&mut bounded);
        let result = TexturePlan::load(bounded.sources(&bt).unwrap(), &mounts, Default::default());
        match expected {
            "queue" => assert!(matches!(result, Err(JobError::QueueFull))),
            "bytes" => assert!(matches!(result, Err(JobError::ByteBudget))),
            _ => {
                bounded.request_textures(&bt, result.unwrap()).unwrap();
                textures_decoded(&mut bounded);
            }
        }
    }
}

#[test]
fn invalid_model_or_large_unknown_metadata_never_becomes_an_empty_ready_plan() {
    let (fixture, cell, _) = setup_payload(1, false, b"invalid NIF".to_vec());
    let mut owner = owner(&fixture, 1, 11);
    let ticket = owner.request(cell).unwrap();
    decoded(&mut owner);
    let before = owner.snapshot();
    assert!(
        TexturePlan::load(
            owner.sources(&ticket).unwrap(),
            &MountIndex::default(),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(
        owner.snapshot().plan_metadata_bytes,
        before.plan_metadata_bytes
    );
    assert_eq!(owner.snapshot().texture_state, TextureState::Unrequested);
    assert!(
        owner
            .report_dependencies(&ticket, Readiness::Ready)
            .is_err()
    );
    assert_eq!(owner.snapshot().stage, Stage::Decoded);

    let name = "Unknown".repeat(64);
    let nif = textured_named(&[], Some(&name));
    let (fixture, cell, _) = setup_payload(1, false, nif.clone());
    let mut owner = super::owner(&fixture, 1, nif.len());
    let ticket = owner.request(cell).unwrap();
    decoded(&mut owner);
    let before = owner.snapshot();
    assert!(matches!(
        TexturePlan::load(
            owner.sources(&ticket).unwrap(),
            &MountIndex::default(),
            TextureLimits {
                metadata_bytes: 2304,
                ..Default::default()
            }
        ),
        Err(JobError::ByteBudget)
    ));
    assert_eq!(
        owner.snapshot().plan_metadata_bytes,
        before.plan_metadata_bytes
    );
    let plan = TexturePlan::load(
        owner.sources(&ticket).unwrap(),
        &MountIndex::default(),
        Default::default(),
    )
    .unwrap();
    assert!(plan.receipt().metadata_bytes > 2304);
    assert_eq!(plan.receipt().models[0].unsupported_blocks[&name], [1]);
}
