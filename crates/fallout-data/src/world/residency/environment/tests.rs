use super::super::test_sources::{Fixture, LIGHT_WORDS, field, key, record, until};
use super::*;
use crate::resource_jobs::tests::Pause;
use sha2::{Digest, Sha256};
fn model_owner(f: &Fixture, limits: Limits) -> (CellResidency, Ticket) {
    let plan =
        CellModelPlan::load(&mut f.store(), &key(0x200), &f.mounts, Default::default()).unwrap();
    let mut owner = CellResidency::new(f.root.path(), Some(f.cache.path()), limits).unwrap();
    let ticket = owner.request(plan).unwrap();
    until(|| owner.poll().unwrap().stage == Stage::Decoded);
    (owner, ticket)
}
fn plan(
    f: &Fixture,
    owner: &CellResidency,
    t: &Ticket,
    limits: EnvironmentLimits,
) -> EnvironmentPlan {
    EnvironmentPlan::load(
        owner.sources(t).unwrap(),
        &mut f.store(),
        &f.mounts,
        limits,
        Default::default(),
        Default::default(),
    )
    .unwrap()
}
fn complete(owner: &mut CellResidency, t: &Ticket) {
    until(|| {
        owner.poll().unwrap();
        owner.environment_sources(t).is_ok()
    });
}
#[test]
fn literal_lighting_water_and_noise_use_one_parent_pool_then_revoke_and_drain() {
    let f = Fixture::scene();
    let (mut owner, t) = model_owner(&f, Default::default());
    let p = plan(&f, &owner, &t, Default::default());
    let identity = p.identity().to_owned();
    owner.request_environment(&t, p, &mut f.store()).unwrap();
    complete(&mut owner, &t);
    let sources = owner.environment_sources(&t).unwrap();
    let lighting = sources.lighting().unwrap();
    assert_eq!(lighting.cell.key, key(0x200));
    assert_eq!(
        lighting.xcll.as_ref().unwrap().framing,
        field(
            b"XCLL",
            &LIGHT_WORDS
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        lighting.xcll.as_ref().unwrap().value.fog_near_word,
        0x80000000
    );
    assert_eq!(
        lighting.xcll.as_ref().unwrap().value.fog_far_word,
        0x7fc00001
    );
    assert_eq!(lighting.ltmp.as_ref().unwrap().value, 0x500);
    assert_eq!(lighting.lnam.as_ref().unwrap().value, 0x2c);
    assert_eq!(
        lighting
            .template
            .as_ref()
            .unwrap()
            .source
            .as_ref()
            .unwrap()
            .key,
        key(0x500)
    );
    let water = sources.water().unwrap();
    assert_eq!(water.xclw.as_ref().unwrap().value, 0x7fc00001);
    assert_eq!(water.xcwt.as_ref().unwrap().value, 0x501);
    assert_eq!(water.xnam.as_ref().unwrap().value, b"n.dds");
    assert_eq!(water.water_type.as_ref().unwrap().target.status, "resolved");
    assert!(!water.water_type.as_ref().unwrap().typed_parameters_included);
    assert_eq!(sources.noise().unwrap().unwrap(), f.noise);
    assert_eq!(
        format!("{:x}", Sha256::digest(sources.noise().unwrap().unwrap())),
        format!("{:x}", Sha256::digest(&f.noise))
    );
    assert_eq!(sources.identity(), identity);
    assert_ne!(lighting.identity, water.identity);
    let before = owner.snapshot();
    let environment = owner.environment_snapshot();
    assert_eq!(before.outstanding, 2);
    assert_eq!(
        before.pinned_source_bytes,
        f.models[0].len() + f.noise.len()
    );
    assert_eq!(environment.retained_scopes, 1);
    assert_eq!(environment.noise_completed, 1);
    assert!(environment.metadata_bytes > 0);
    assert!(environment.mapped_bytes > 0);
    assert_eq!(before.dependencies, Readiness::Pending);
    assert!(!before.simulation_ready);
    owner.unload().unwrap();
    assert!(sources.lighting().is_err());
    assert!(sources.water().is_err());
    assert!(sources.noise().is_err());
    let retired = owner.poll().unwrap();
    assert_eq!(retired.outstanding, before.outstanding);
    assert_eq!(retired.plan_metadata_bytes, before.plan_metadata_bytes);
    assert_eq!(retired.mapped_source_bytes, before.mapped_source_bytes);
    assert_eq!(
        owner.environment_snapshot().metadata_bytes,
        environment.metadata_bytes
    );
    drop(sources);
    until(|| owner.poll().unwrap().stage == Stage::Unrequested);
    assert_eq!(owner.environment_snapshot().retained_scopes, 0);
    assert_eq!(owner.environment_snapshot().metadata_bytes, 0);
    assert_eq!(owner.environment_snapshot().mapped_bytes, 0);
    assert_eq!(owner.snapshot().outstanding, 0);
}
#[test]
fn foreign_equal_epochs_changed_source_and_replaced_parent_cannot_attach_environment() {
    let mut f = Fixture::scene();
    let (mut a, ta) = model_owner(&f, Default::default());
    let (b, tb) = model_owner(&f, Default::default());
    assert_eq!(ta.generation(), tb.generation());
    assert_eq!(ta.identity(), tb.identity());
    let foreign = plan(&f, &b, &tb, Default::default());
    assert!(a.request_environment(&ta, foreign, &mut f.store()).is_err());
    assert_eq!(a.environment_snapshot().retained_scopes, 0);
    assert!(b.sources(&tb).is_ok());
    let own = plan(&f, &a, &ta, Default::default());
    assert!(
        EnvironmentPlan::load(
            a.sources(&ta).unwrap(),
            &mut f.store(),
            &f.mounts,
            Default::default(),
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    let old = own.clone();
    a.unload().unwrap();
    assert!(a.request_environment(&ta, own, &mut f.store()).is_err());
    assert!(old.ticket().check().is_err());
    drop(old);
    ta.check().unwrap_err();
    tb.check().unwrap();
    let (mut current, t) = model_owner(&f, Default::default());
    let p = plan(&f, &current, &t, Default::default());
    f.patch(&record(b"STAT", 0x599, 0, &[]));
    assert!(current.request_environment(&t, p, &mut f.store()).is_err());
    assert_eq!(current.environment_snapshot().retained_scopes, 0);
    assert!(current.sources(&t).is_ok());
    let water =
        CellWaterSources::load(&mut f.store(), &key(0x201), &f.mounts, Default::default()).unwrap();
    assert!(
        water
            .resident_member(&current.sources(&t).unwrap())
            .is_err()
    );
}
#[test]
fn exact_retention_payload_and_queue_limits_and_last_refusal_release_unattached_scope() {
    let f = Fixture::scene();
    let (mut baseline, t) = model_owner(&f, Default::default());
    let before = baseline.snapshot();
    let p = plan(&f, &baseline, &t, Default::default());
    let bytes = baseline.environment_snapshot().metadata_bytes;
    let mapped = baseline.environment_snapshot().mapped_bytes;
    drop(p);
    assert_eq!(baseline.environment_snapshot().retained_scopes, 0);
    let limits = EnvironmentLimits {
        metadata_bytes: bytes,
        mapped_bytes: mapped,
        noise_bytes: f.noise.len(),
    };
    let p = plan(&f, &baseline, &t, limits);
    baseline.request_environment(&t, p, &mut f.store()).unwrap();
    complete(&mut baseline, &t);
    for mode in 0..3 {
        let (mut owner, t) = model_owner(&f, Default::default());
        let mut lower = limits;
        match mode {
            0 => lower.metadata_bytes -= 1,
            1 => lower.mapped_bytes -= 1,
            _ => lower.noise_bytes -= 1,
        }
        assert!(
            EnvironmentPlan::load(
                owner.sources(&t).unwrap(),
                &mut f.store(),
                &f.mounts,
                lower,
                Default::default(),
                Default::default()
            )
            .is_err()
        );
        assert_eq!(owner.environment_snapshot().retained_scopes, 0);
        assert_eq!(
            owner.snapshot().plan_metadata_bytes,
            before.plan_metadata_bytes
        );
        assert_eq!(
            owner.snapshot().mapped_source_bytes,
            before.mapped_source_bytes
        );
        owner.unload().unwrap();
    }
    let (mut queue, t) = model_owner(
        &f,
        Limits {
            workers: 1,
            models: 1,
            resources: 1,
            ..Default::default()
        },
    );
    let p = plan(&f, &queue, &t, Default::default());
    assert!(matches!(
        queue.request_environment(&t, p, &mut f.store()),
        Err(JobError::QueueFull)
    ));
    assert_eq!(queue.environment_snapshot().retained_scopes, 0);
    assert!(queue.sources(&t).is_ok());
    let (mut exact, t) = model_owner(
        &f,
        Limits {
            workers: 1,
            models: 1,
            resources: 2,
            source_bytes: f.models[0].len() + f.noise.len(),
            ..Default::default()
        },
    );
    let p = plan(&f, &exact, &t, limits);
    exact.request_environment(&t, p, &mut f.store()).unwrap();
    complete(&mut exact, &t);
    assert_eq!(
        exact.snapshot().pinned_source_bytes,
        f.models[0].len() + f.noise.len()
    );
    let (mut host, t) = model_owner(
        &f,
        Limits {
            plan_metadata_bytes: before.plan_metadata_bytes + bytes - 1,
            ..Default::default()
        },
    );
    assert!(
        EnvironmentPlan::load(
            host.sources(&t).unwrap(),
            &mut f.store(),
            &f.mounts,
            Default::default(),
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(host.environment_snapshot().retained_scopes, 0);
    host.unload().unwrap();
}
#[test]
fn absent_null_and_missing_declarations_are_explicit_source_statuses_without_defaults() {
    for mode in 0..5 {
        let mut f = Fixture::new();
        if mode != 0 {
            let extra = match mode {
                1 => field(b"LTMP", &0u32.to_le_bytes()),
                2 => field(b"XNAM", b"missing.dds\0"),
                3 => field(b"LTMP", &0xdeadu32.to_le_bytes()),
                _ => field(b"XCWT", &0xdeadu32.to_le_bytes()),
            };
            f.patch(&record(
                b"CELL",
                0x200,
                0,
                &[field(b"DATA", &[0]), extra].concat(),
            ));
        }
        let (mut owner, t) = model_owner(&f, Default::default());
        let p = plan(&f, &owner, &t, Default::default());
        owner.request_environment(&t, p, &mut f.store()).unwrap();
        complete(&mut owner, &t);
        let sources = owner.environment_sources(&t).unwrap();
        assert!(sources.noise().unwrap().is_none());
        assert!(sources.lighting().unwrap().xcll.is_none());
        assert!(sources.water().unwrap().xclw.is_none());
        if mode == 1 {
            assert_eq!(
                sources
                    .lighting()
                    .unwrap()
                    .template
                    .as_ref()
                    .unwrap()
                    .input_status,
                "null"
            );
        }
        if mode == 2 {
            assert_eq!(
                sources.water().unwrap().noise.as_ref().unwrap().status,
                "missing"
            );
        }
        assert!(!owner.snapshot().simulation_ready);
        assert_eq!(owner.snapshot().dependencies, Readiness::Pending);
        assert_eq!(sources.source_declarations_available().unwrap(), mode < 2);
        if mode >= 2 {
            let textures =
                TexturePlan::load(owner.sources(&t).unwrap(), &f.mounts, Default::default())
                    .unwrap();
            owner.request_textures(&t, textures).unwrap();
            until(|| owner.poll().unwrap().texture_state == TextureState::Decoded);
            let error = owner.report_dependencies(&t, Readiness::Ready).unwrap_err();
            assert!(error.to_string().contains("environment"));
            assert_eq!(owner.snapshot().dependencies, Readiness::Pending);
        }
    }
}
struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
#[test]
fn parent_cancel_before_and_after_noise_extraction_blocks_marker_completion_and_keeps_scope_charged()
 {
    for after in [false, true] {
        let f = Fixture::scene();
        let (mut owner, t) = model_owner(&f, Default::default());
        let p = plan(&f, &owner, &t, Default::default());
        owner.request_environment(&t, p, &mut f.store()).unwrap();
        let before = std::fs::read_dir(f.cache.path()).unwrap().count();
        let pause = Pause::new(after);
        owner.pause = Some(pause.clone());
        let _release = Release(pause.clone());
        owner.poll().unwrap();
        pause.reached();
        owner.unload().unwrap();
        assert!(owner.environment_sources(&t).is_err());
        assert_eq!(owner.environment_snapshot().retained_scopes, 1);
        assert!(owner.environment_snapshot().metadata_bytes > 0);
        assert!(owner.environment_snapshot().mapped_bytes > 0);
        assert_eq!(owner.snapshot().outstanding, 2);
        pause.release();
        pause.completed();
        until(|| owner.poll().unwrap().stage == Stage::Unrequested);
        assert_eq!(std::fs::read_dir(f.cache.path()).unwrap().count(), before);
        assert_eq!(owner.environment_snapshot().retained_scopes, 0);
        assert_eq!(owner.snapshot().mapped_source_bytes, 0);
        owner.pause = None;
        let p = CellModelPlan::load(&mut f.store(), &key(0x200), &f.mounts, Default::default())
            .unwrap();
        let next = owner.request(p).unwrap();
        until(|| owner.poll().unwrap().stage == Stage::Decoded);
        let p = plan(&f, &owner, &next, Default::default());
        owner.request_environment(&next, p, &mut f.store()).unwrap();
        complete(&mut owner, &next);
        assert_eq!(
            owner
                .environment_sources(&next)
                .unwrap()
                .noise()
                .unwrap()
                .unwrap(),
            f.noise
        );
    }
}
