use super::*;
use fallout_data::{
    identity::ProfileId, plugin, store::RecordStore, world::Transform as SourceTransform,
};
use fallout_runtime::{
    identity::CampaignId,
    reference_state::{Pose, State},
};
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture {
    root: PathBuf,
    install: PathBuf,
    catalogue: Arc<Catalogue>,
    repository: Repository,
    world: World<'static>,
    cell: FormKey,
    keys: Vec<FormKey>,
}
fn key(local_id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id,
    }
}
fn record(kind: &[u8; 4], id: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}
fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "fallout-preview-native-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let install = root.join("install");
    let data = install.join("Data");
    fs::create_dir_all(&data).unwrap();
    let header = [
        b"HEDR".as_slice(),
        &12_u16.to_le_bytes(),
        &1.34_f32.to_le_bytes(),
        &[0; 8],
    ]
    .concat();
    fs::write(
        data.join("FalloutNV.esm"),
        [
            record(b"TES4", 0, &header),
            record(b"CELL", 0x400, &[]),
            record(b"REFR", 0x500, &[]),
            record(b"REFR", 0x501, &[]),
            record(b"REFR", 0x502, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let mut store =
        RecordStore::open_nv_headers(&data, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let cell = key(0x400);
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes([0xA7; 16]).unwrap(),
    )
    .unwrap();
    let keys = vec![key(0x500), key(0x501), key(0x502)];
    for key in &keys[..2] {
        world.register_reference(Some(key.clone())).unwrap();
    }
    set(
        &mut world,
        &cell,
        &keys[0],
        [8192.25, -5.5, 30.5],
        Some(2.),
        true,
    );
    let repository = Repository::create(
        &root.join("native"),
        std::slice::from_ref(&install),
        world.campaign(),
    )
    .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    Fixture {
        root,
        install,
        catalogue,
        repository,
        world,
        cell,
        keys,
    }
}
fn set(
    world: &mut World<'_>,
    cell: &FormKey,
    key: &FormKey,
    position: [f32; 3],
    scale: Option<f32>,
    enabled: bool,
) {
    let id = world.authored_reference(key).unwrap();
    let view = world.reference_view(id).unwrap();
    let pose = Pose::from_source(
        &SourceTransform {
            position,
            rotation: [0.; 3],
        },
        scale,
    )
    .unwrap();
    let staged = world
        .stage_reference_state(&view, State::new(cell.clone(), pose, enabled).unwrap())
        .unwrap();
    world.commit_reference_state(staged).unwrap();
}
impl Fixture {
    fn session(&self) -> Session {
        Session::load(
            self.repository.path(),
            std::slice::from_ref(&self.install),
            Arc::clone(&self.catalogue),
            self.cell.clone(),
            self.keys.clone(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn event(host: &mut Host) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(event) = host.poll() {
            return event;
        }
        assert!(Instant::now() < deadline, "native host did not complete");
        thread::yield_now();
    }
}
fn stop(mut host: Host) {
    host.commands.take();
    host.shutdown.finish().unwrap();
}

#[test]
fn shutdown_drains_an_admitted_request_without_claiming_join_as_success() {
    let f = fixture();
    let shutdown = Shutdown::default();
    let (mut host, _) = f.session().start([0.; 3], shutdown.clone()).unwrap();
    assert!(host.request(Request::Save));
    shutdown.finish().unwrap(); // outside a host frame
    let Some(Event::Saved(receipt)) = host.poll() else {
        panic!("joined owner did not return actual save receipt");
    };
    assert_eq!(receipt.metadata.generation, 2);
    assert!(!host.request(Request::Continue));
    assert!(host.title().contains("admission closed"));
    assert!(f.session().start([0.; 3], shutdown).is_err());
}

#[test]
fn source_bound_restore_maps_once_and_preserves_unavailable_identity_and_state() {
    let f = fixture();
    let before = f.world.snapshot();
    let (host, observation) = f
        .session()
        .start([8192., -5., 30.], Shutdown::default())
        .unwrap();
    let draw = observation.draws[&f.keys[0]].unwrap();
    assert_eq!(draw.translation, Vec3::new(0.25, 0.5, 0.5));
    assert_eq!(draw.scale, Vec3::splat(2.));
    assert_eq!(draw.rotation, Quat::IDENTITY);
    assert!(observation.draws[&f.keys[1]].is_none());
    assert!(observation.draws[&f.keys[2]].is_none());
    assert_eq!(
        observation.report.bindings[1].display,
        "canonical-state-unavailable"
    );
    assert_eq!(
        observation.report.bindings[2].display,
        "canonical-identity-unavailable"
    );
    assert_eq!(
        observation.report.bindings[0]
            .canonical
            .as_ref()
            .unwrap()
            .reference(),
        f.world.authored_reference(&f.keys[0]).unwrap()
    );
    assert_eq!(observation.report.revision, before.state_revision);
    stop(host);
    let (cold, _) = f
        .repository
        .load(
            Arc::clone(&f.catalogue),
            Limits::default(),
            Recovery::Strict,
        )
        .unwrap();
    assert_eq!(cold.snapshot(), before);
    assert_eq!(f.world.snapshot(), before);
}

#[test]
fn actual_host_continue_retains_identity_and_failed_continue_preserves_active_boundary() {
    let mut f = fixture();
    let (mut host, before) = f
        .session()
        .start([8192., -5., 30.], Shutdown::default())
        .unwrap();
    let id = before.report.bindings[0]
        .canonical
        .as_ref()
        .unwrap()
        .reference();
    set(
        &mut f.world,
        &f.cell,
        &f.keys[0],
        [9000., 1., 40.],
        Some(0.5),
        true,
    );
    f.repository
        .commit(&Captured::at_boundary(&f.world))
        .unwrap();
    assert!(host.request(Request::Continue));
    assert!(!host.request(Request::Save)); // bounded one-request admission
    let Event::Continued(after) = event(&mut host) else {
        panic!("Continue failed");
    };
    assert_eq!(
        after.report.bindings[0]
            .canonical
            .as_ref()
            .unwrap()
            .reference(),
        id
    );
    assert_eq!(after.report.revision, f.world.revision());
    assert_eq!(
        after.draws[&f.keys[0]].unwrap().translation,
        Vec3::new(808., 10., -6.)
    );
    let exact = fs::read(f.repository.path().join("current.frsv")).unwrap();
    fs::write(
        f.repository.path().join("current.frsv"),
        b"corrupt authored current",
    )
    .unwrap();
    assert!(host.request(Request::Continue));
    assert!(matches!(event(&mut host), Event::Failed(_)));
    assert!(host.title().starts_with("Native failed:"));
    assert!(host.poll().is_none());
    // The failed restore did not replace the already active boundary.
    fs::write(f.repository.path().join("current.frsv"), exact).unwrap();
    assert!(host.request(Request::Save));
    let Event::Saved(receipt) = event(&mut host) else {
        panic!("save after rejected Continue failed");
    };
    assert_eq!(receipt.metadata.state_revision, after.report.revision);
    stop(host);
}

#[test]
fn actual_save_success_requires_publication_and_writer_failure_stays_failure() {
    let f = fixture();
    let (mut host, before) = f.session().start([0.; 3], Shutdown::default()).unwrap();
    assert!(host.request(Request::Save));
    assert_eq!(host.title(), "Save pending publication");
    let Event::Saved(receipt) = event(&mut host) else {
        panic!("native save failed");
    };
    assert_eq!(receipt.metadata.generation, 2);
    assert_eq!(receipt.metadata.state_revision, before.report.revision);
    assert!(host.title().starts_with("Save published generation 2"));
    assert!(host.poll().is_none());
    let (cold, load) = f
        .repository
        .load(
            Arc::clone(&f.catalogue),
            Limits::default(),
            Recovery::Strict,
        )
        .unwrap();
    assert_eq!(load.metadata, receipt.metadata);
    assert_eq!(cold.snapshot(), f.world.snapshot());
    let current = fs::read(f.repository.path().join("current.frsv")).unwrap();
    fs::remove_file(f.repository.path().join("previous.frsv")).unwrap();
    fs::create_dir(f.repository.path().join("previous.frsv")).unwrap();
    assert!(host.request(Request::Save));
    assert!(matches!(event(&mut host), Event::Failed(_)));
    assert!(host.title().starts_with("Native failed:"));
    assert!(!host.title().contains("published"));
    assert!(host.poll().is_none());
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        current
    );
    stop(host);
}

#[test]
fn missing_scale_disabled_outside_cell_and_changed_sources_never_invent_live_values() {
    let mut f = fixture();
    for (scale, enabled, cell, status) in [
        (None, true, f.cell.clone(), "canonical-scale-unavailable"),
        (None, false, f.cell.clone(), "canonical-disabled"),
        (
            Some(1.),
            true,
            key(0x401),
            "canonical-reference-outside-selected-cell",
        ),
    ] {
        set(
            &mut f.world,
            &cell,
            &f.keys[0],
            [1., -0., 3.],
            scale,
            enabled,
        );
        f.repository
            .commit(&Captured::at_boundary(&f.world))
            .unwrap();
        let (host, observation) = f.session().start([0.; 3], Shutdown::default()).unwrap();
        assert_eq!(observation.report.bindings[0].display, status);
        assert!(observation.draws[&f.keys[0]].is_none());
        stop(host);
    }
    let data = f.install.join("Data");
    let mut bytes = fs::read(data.join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"ACTI", 0x600, &[]));
    fs::write(data.join("FalloutNV.esm"), bytes).unwrap();
    let mut store =
        RecordStore::open_nv_headers(&data, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let changed = Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    assert!(
        Session::load(
            f.repository.path(),
            std::slice::from_ref(&f.install),
            changed,
            f.cell.clone(),
            f.keys.clone()
        )
        .is_err()
    );
}
