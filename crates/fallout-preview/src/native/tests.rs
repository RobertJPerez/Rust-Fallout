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

fn wait_finished(shutdown: &Shutdown) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if shutdown
            .0
            .tasks
            .lock()
            .unwrap()
            .iter()
            .all(JoinHandle::is_finished)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "disconnected native owner did not return"
        );
        thread::yield_now();
    }
}

#[test]
fn nine_sequential_disconnected_hosts_reuse_completed_owner_slots() {
    let f = fixture();
    let shutdown = Shutdown::default();
    let before = fs::read(f.repository.path().join("current.frsv")).unwrap();
    for index in 0..9 {
        let (host, observation) = f
            .session()
            .start([0.; 3], shutdown.clone())
            .unwrap_or_else(|error| panic!("sequential host {} refused: {error}", index + 1));
        assert_eq!(observation.report.revision, f.world.revision());
        drop(host);
        wait_finished(&shutdown);
    }
    assert_eq!(shutdown.0.tasks.lock().unwrap().len(), 1);
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        before
    );
    shutdown.finish().unwrap();
    assert!(shutdown.0.tasks.lock().unwrap().is_empty());
}

#[test]
fn eight_outstanding_hosts_refuse_ninth_then_reclaim_only_finished_owner() {
    let f = fixture();
    let shutdown = Shutdown::default();
    let before = fs::read(f.repository.path().join("current.frsv")).unwrap();
    let mut hosts = Vec::new();
    for _ in 0..8 {
        hosts.push(f.session().start([0.; 3], shutdown.clone()).unwrap().0);
    }
    assert!(
        shutdown
            .0
            .tasks
            .lock()
            .unwrap()
            .iter()
            .all(|task| !task.is_finished())
    );
    let Err(error) = f.session().start([0.; 3], shutdown.clone()) else {
        panic!("ninth outstanding owner was admitted");
    };
    assert!(error.to_string().contains("eight-outstanding-owner"));
    assert_eq!(shutdown.0.tasks.lock().unwrap().len(), 8);
    drop(hosts.pop());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !shutdown
        .0
        .tasks
        .lock()
        .unwrap()
        .iter()
        .any(JoinHandle::is_finished)
    {
        assert!(
            Instant::now() < deadline,
            "disconnected owner did not return"
        );
        thread::yield_now();
    }
    hosts.push(f.session().start([0.; 3], shutdown.clone()).unwrap().0);
    assert_eq!(shutdown.0.tasks.lock().unwrap().len(), 8);
    assert!(
        shutdown
            .0
            .tasks
            .lock()
            .unwrap()
            .iter()
            .all(|task| !task.is_finished())
    );
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        before
    );
    drop(hosts);
    shutdown.finish().unwrap();
}

#[test]
fn reused_owner_slot_preserves_disconnected_accepted_write_and_real_receipt() {
    let f = fixture();
    let shutdown = Shutdown::default();
    let (mut host, before) = f.session().start([0.; 3], shutdown.clone()).unwrap();
    assert!(host.request(Request::Save));
    drop(host);
    wait_finished(&shutdown); // no final shutdown: retry admission stays open
    let (mut fresh, observation) = f.session().start([0.; 3], shutdown.clone()).unwrap();
    assert_eq!(observation.report.load.metadata.generation, 2);
    assert_eq!(observation.report.revision, before.report.revision);
    assert_eq!(shutdown.0.tasks.lock().unwrap().len(), 1);
    assert!(!fresh.published()); // reclaimed join is not this host's save result
    assert!(fresh.request(Request::Save));
    let Event::Saved(receipt) = event(&mut fresh) else {
        panic!("retry did not return actual publication receipt");
    };
    assert_eq!(receipt.metadata.generation, 3);
    assert_eq!(receipt.metadata.state_revision, before.report.revision);
    assert!(fresh.published());
    drop(fresh);
    shutdown.finish().unwrap();
    assert!(f.session().start([0.; 3], shutdown).is_err());
}

#[test]
fn reaped_owner_panic_stays_failure_for_retry_and_repeated_shutdown() {
    let f = fixture();
    let shutdown = Shutdown::default();
    shutdown
        .0
        .tasks
        .lock()
        .unwrap()
        .push(thread::spawn(|| panic!("controlled native owner panic")));
    wait_finished(&shutdown);
    let Err(error) = f.session().start([0.; 3], shutdown.clone()) else {
        panic!("collected owner panic was hidden by retry");
    };
    assert!(error.to_string().contains("panicked before retry"));
    assert!(shutdown.0.tasks.lock().unwrap().is_empty());
    assert!(
        shutdown
            .finish()
            .unwrap_err()
            .to_string()
            .contains("panicked")
    );
    assert!(
        shutdown
            .finish()
            .unwrap_err()
            .to_string()
            .contains("panicked")
    );
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

fn intent(
    revision: u64,
    sequence: u64,
    action: crate::input::NativeAction,
) -> crate::input::NativeIntent {
    crate::input::NativeIntent {
        action,
        display: crate::input::DisplayIdentity {
            scene_epoch: 7,
            revision,
        },
        sequence,
        context: crate::input::Context::Orbit,
        focused: true,
        window: Entity::PLACEHOLDER,
        device: crate::input::Device::Keyboard,
    }
}

#[test]
fn typed_native_intents_cannot_replay_or_act_on_another_display_boundary() {
    let f = fixture();
    let before_file = fs::read(f.repository.path().join("current.frsv")).unwrap();
    let before_world = f.world.snapshot();
    let (mut host, observation) = f.session().start([0.; 3], Shutdown::default()).unwrap();
    let revision = observation.report.revision;
    assert!(!host.intent(
        intent(revision + 1, 1, crate::input::NativeAction::Save),
        7,
        true
    ));
    assert!(!host.intent(
        intent(revision, 2, crate::input::NativeAction::Save),
        8,
        true
    ));
    assert!(!host.intent(
        intent(revision, 3, crate::input::NativeAction::Save),
        7,
        false
    ));
    let mut loading = intent(revision, 4, crate::input::NativeAction::Save);
    loading.context = crate::input::Context::Loading;
    assert!(!host.intent(loading, 7, true));
    let mut unfocused = intent(revision, 5, crate::input::NativeAction::Save);
    unfocused.focused = false;
    assert!(!host.intent(unfocused, 7, true));
    assert!(!host.pending);
    assert!(host.poll().is_none());
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        before_file
    );
    let current = intent(revision, 6, crate::input::NativeAction::Save);
    assert!(host.intent(current, 7, true));
    assert!(!host.intent(current, 7, true));
    let busy = intent(revision, 7, crate::input::NativeAction::Save);
    assert!(!host.intent(busy, 7, true));
    let Event::Saved(receipt) = event(&mut host) else {
        panic!("fresh typed save failed");
    };
    assert_eq!(receipt.metadata.generation, 2);
    assert_eq!(receipt.metadata.state_revision, revision);
    assert!(!host.intent(busy, 7, true));
    assert!(host.poll().is_none());
    let after_file = fs::read(f.repository.path().join("current.frsv")).unwrap();
    assert!(!host.intent(current, 7, true));
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        after_file
    );
    assert_eq!(f.world.snapshot(), before_world);
    assert!(host.intent(
        intent(revision, 8, crate::input::NativeAction::Save),
        7,
        true
    ));
    let Event::Saved(receipt) = event(&mut host) else {
        panic!("next fresh save failed");
    };
    assert_eq!(receipt.metadata.generation, 3);
    stop(host);
}

#[test]
fn continued_world_rejects_input_sampled_from_previous_revision() {
    let mut f = fixture();
    let (mut host, before) = f.session().start([0.; 3], Shutdown::default()).unwrap();
    set(
        &mut f.world,
        &f.cell,
        &f.keys[0],
        [100., 20., 30.],
        Some(1.),
        true,
    );
    f.repository
        .commit(&Captured::at_boundary(&f.world))
        .unwrap();
    assert!(host.intent(
        intent(
            before.report.revision,
            1,
            crate::input::NativeAction::Continue
        ),
        7,
        true
    ));
    let Event::Continued(after) = event(&mut host) else {
        panic!("typed Continue failed");
    };
    assert!(after.report.revision > before.report.revision);
    let bytes = fs::read(f.repository.path().join("current.frsv")).unwrap();
    assert!(!host.intent(
        intent(before.report.revision, 2, crate::input::NativeAction::Save),
        7,
        true
    ));
    assert!(!host.pending);
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        bytes
    );
    assert!(host.intent(
        intent(after.report.revision, 3, crate::input::NativeAction::Save),
        7,
        true
    ));
    let Event::Saved(receipt) = event(&mut host) else {
        panic!("fresh restored-boundary save failed");
    };
    assert_eq!(receipt.metadata.state_revision, after.report.revision);
    stop(host);
}

#[test]
fn actual_canonical_owner_refuses_bad_revision_before_any_publication() {
    let f = fixture();
    let before = fs::read(f.repository.path().join("current.frsv")).unwrap();
    let (mut host, observation) = f.session().start([0.; 3], Shutdown::default()).unwrap();
    host.commands
        .as_ref()
        .unwrap()
        .try_send(Command {
            request: Request::Save,
            expected_revision: observation.report.revision + 1,
        })
        .unwrap();
    host.pending = true;
    assert!(
        matches!(event(&mut host), Event::Failed(error) if error.contains("command revision differs"))
    );
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        before
    );
    assert!(!host.published());
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

fn edit_request(world: &World<'_>, cell: &FormKey, selected: &FormKey) -> edit::Request {
    let view = world
        .reference_view(world.authored_reference(selected).unwrap())
        .unwrap();
    edit::Request {
        schema_version: 1,
        scene_epoch: 7,
        intent_sequence: 1,
        expected_campaign: view.campaign(),
        expected_catalogue_sha256: view.catalogue_fingerprint().into(),
        expected_revision: view.revision(),
        key: selected.clone(),
        reference: view.reference(),
        state: State::new(
            cell.clone(),
            Pose::from_source(
                &SourceTransform {
                    position: [8193.25, -0.0, 32.5],
                    rotation: [0., 0., std::f32::consts::FRAC_PI_2],
                },
                Some(2.),
            )
            .unwrap(),
            true,
        )
        .unwrap(),
    }
}

#[test]
fn explicit_edit_ingress_preserves_all_source_words_and_enforces_exact_byte_bounds() {
    let f = fixture();
    let request = edit_request(&f.world, &f.cell, &f.keys[0]);
    let path = f.root.join("edit.json");
    let mut raw = serde_json::to_vec(&request).unwrap();
    let length = raw.len();
    fs::write(&path, &raw).unwrap();
    assert!(edit::read_request(&path, length - 1).is_err());
    let parsed = edit::read_request(&path, length).unwrap();
    assert_eq!(parsed.state, request.state);
    assert_eq!(
        parsed
            .state
            .pose()
            .source_transform()
            .position
            .map(f32::to_bits),
        [8193.25f32.to_bits(), 0x80000000, 32.5f32.to_bits()]
    );
    raw.resize(edit::REQUEST_BYTES, b' ');
    fs::write(&path, &raw).unwrap();
    assert_eq!(
        edit::read_request(&path, edit::REQUEST_BYTES)
            .unwrap()
            .state,
        request.state
    );
    raw.push(b' ');
    fs::write(&path, &raw).unwrap();
    assert!(edit::read_request(&path, edit::REQUEST_BYTES).is_err());
    assert!(edit::read_request(&path, edit::REQUEST_BYTES + 1).is_err());
    assert_eq!(fs::read(&path).unwrap(), raw);
}

#[test]
fn explicit_edit_ingress_rejects_missing_duplicate_unknown_and_invalid_component_words() {
    let f = fixture();
    let request = edit_request(&f.world, &f.cell, &f.keys[0]);
    let value = serde_json::to_value(&request).unwrap();
    let before = f.world.snapshot();
    let mut refused = Vec::new();
    for pointer in [
        "/state/pose/scale_bits",
        "/state/pose/rotation_bits",
        "/state/pose/position_bits",
        "/state/enabled",
        "/scene_epoch",
    ] {
        let mut v = value.clone();
        let (parent, name) = pointer.rsplit_once('/').unwrap();
        v.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(name);
        refused.push(serde_json::to_vec(&v).unwrap());
    }
    for pointer in ["", "/key", "/state", "/state/cell", "/state/pose"] {
        let mut v = value.clone();
        v.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        refused.push(serde_json::to_vec(&v).unwrap());
    }
    for pointer in ["/state/pose/position_bits/0", "/state/pose/rotation_bits/2"] {
        for bits in [0x7f800000u32, 0xff800000, 0x7fc01234] {
            let mut v = value.clone();
            *v.pointer_mut(pointer).unwrap() = bits.into();
            refused.push(serde_json::to_vec(&v).unwrap());
        }
    }
    for bits in [0u32, 0x80000000, 0xbf800000, 0x7f800000] {
        let mut v = value.clone();
        v["state"]["pose"]["scale_bits"] = bits.into();
        refused.push(serde_json::to_vec(&v).unwrap());
    }
    let raw = String::from_utf8(serde_json::to_vec(&request).unwrap()).unwrap();
    refused.push(
        raw.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        )
        .into_bytes(),
    );
    refused.push(
        raw.replacen(
            "\"scale_bits\":1073741824",
            "\"scale_bits\":1073741824,\"scale_bits\":1073741824",
            1,
        )
        .into_bytes(),
    );
    let path = f.root.join("malformed.json");
    for (index, raw) in refused.iter().enumerate() {
        fs::write(&path, raw).unwrap();
        assert!(
            edit::read_request(&path, edit::REQUEST_BYTES).is_err(),
            "case{index}"
        );
        assert_eq!(fs::read(&path).unwrap(), *raw);
        assert_eq!(f.world.snapshot(), before);
    }
    let mut absent = value.clone();
    absent["state"]["enabled"] = false.into();
    absent["state"]["pose"]["scale_bits"] = serde_json::Value::Null;
    fs::write(&path, serde_json::to_vec(&absent).unwrap()).unwrap();
    assert!(
        edit::read_request(&path, edit::REQUEST_BYTES)
            .unwrap()
            .state
            .pose()
            .source_scale()
            .is_none()
    );
}

#[test]
fn sole_owner_edit_changes_only_one_complete_component_and_one_revision_with_literal_basis() {
    let mut f = fixture();
    let before = f.world.snapshot();
    let request = edit_request(&f.world, &f.cell, &f.keys[0]);
    let current = f.world.reference_view(request.reference).unwrap();
    let load = f.session().load;
    let applied = edit::apply(
        &mut f.world,
        &f.cell,
        &f.keys,
        [8192., -5., 30.],
        load,
        edit::Command {
            request: request.clone(),
            displayed: current,
        },
    )
    .unwrap();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected
        .reference_states
        .iter_mut()
        .find(|entry| entry.id == request.reference)
        .unwrap()
        .state = request.state.clone();
    assert_eq!(f.world.snapshot(), expected);
    assert_eq!(applied.receipt.before.revision(), before.state_revision);
    assert_eq!(applied.receipt.after.state(), Some(&request.state));
    assert_eq!(
        applied.receipt.canonical_commit.before_revision,
        before.state_revision
    );
    assert_eq!(
        applied.receipt.canonical_commit.after_revision,
        expected.state_revision
    );
    assert!(!applied.receipt.durable_publication);
    let draw = applied.observation.draws[&f.keys[0]].unwrap();
    assert_eq!(draw.translation, Vec3::new(1.25, 2.5, -5.));
    for (point, expected) in [
        (Vec3::X, Vec3::new(1.25, 2.5, -3.)),
        (Vec3::Y, Vec3::new(1.25, 4.5, -5.)),
        (Vec3::Z, Vec3::new(-0.75, 2.5, -5.)),
    ] {
        assert!((draw.transform_point(point) - expected).length() < 1e-5);
    }
    assert!(
        applied
            .observation
            .report
            .bindings
            .iter()
            .filter_map(|binding| binding.canonical.as_ref())
            .all(|view| view.revision() == expected.state_revision)
    );
    let bytes = edit::receipt_bytes(&applied.receipt, edit::RECEIPT_BYTES).unwrap();
    assert!(edit::receipt_bytes(&applied.receipt, bytes.len()).is_err());
    assert_eq!(
        edit::receipt_bytes(&applied.receipt, bytes.len() + 1).unwrap(),
        bytes
    );
    assert!(edit::receipt_bytes(&applied.receipt, edit::RECEIPT_BYTES + 1).is_err());
    assert_eq!(f.world.snapshot(), expected);
}

#[test]
fn owner_edit_refusals_preserve_full_snapshot_for_identity_cell_invalid_pose_and_runtime_epoch() {
    let mut f = fixture();
    let before = f.world.snapshot();
    let request = edit_request(&f.world, &f.cell, &f.keys[0]);
    let displayed = f.world.reference_view(request.reference).unwrap();
    for index in 0..12 {
        let mut r = request.clone();
        match index {
            0 => r.expected_revision += 1,
            1 => r.expected_campaign = CampaignId::from_bytes([3; 16]).unwrap(),
            2 => r.expected_catalogue_sha256 = "a".repeat(64),
            3 => r.key = f.keys[1].clone(),
            4 => r.reference = fallout_runtime::identity::ReferenceId(999.try_into().unwrap()),
            5 => r.state = State::new(key(0x401), r.state.pose().clone(), true).unwrap(),
            6 => r.schema_version = 2,
            7 => r.scene_epoch = 0,
            8 => r.intent_sequence = 0,
            9 => {
                r.state = State::new(
                    f.cell.clone(),
                    Pose::from_source(
                        &SourceTransform {
                            position: [0.; 3],
                            rotation: [0.; 3],
                        },
                        None,
                    )
                    .unwrap(),
                    true,
                )
                .unwrap()
            }
            10 => {
                let mut v = serde_json::to_value(&r.state).unwrap();
                v["pose"]["position_bits"][0] = 0x7f800000u32.into();
                r.state = serde_json::from_value(v).unwrap();
            }
            11 => {
                r.state = State::new(
                    f.cell.clone(),
                    Pose::from_source(
                        &SourceTransform {
                            position: [0.; 3],
                            rotation: [0.; 3],
                        },
                        Some(f32::MAX),
                    )
                    .unwrap(),
                    true,
                )
                .unwrap()
            }
            _ => unreachable!(),
        }
        let origin = [0.; 3];
        let load = f.session().load;
        assert!(
            edit::apply(
                &mut f.world,
                &f.cell,
                &f.keys,
                origin,
                load,
                edit::Command {
                    request: r,
                    displayed: displayed.clone()
                }
            )
            .is_err(),
            "case{index}"
        );
        assert_eq!(f.world.snapshot(), before);
    }
    // Same bytes/revision/campaign after cold replacement still have a new authority epoch.
    f.world.replace_from_snapshot(before.clone()).unwrap();
    let load = f.session().load;
    assert!(
        edit::apply(
            &mut f.world,
            &f.cell,
            &f.keys,
            [0.; 3],
            load,
            edit::Command { request, displayed }
        )
        .is_err()
    );
    assert_eq!(f.world.snapshot(), before);
}

#[test]
fn host_edit_has_one_pending_consumed_sequence_and_real_save_cold_continue() {
    let f = fixture();
    let before = f.world.snapshot();
    let request = edit_request(&f.world, &f.cell, &f.keys[0]);
    let file_before = fs::read(f.repository.path().join("current.frsv")).unwrap();
    let (mut host, initial) = f
        .session()
        .start([8192., -5., 30.], Shutdown::default())
        .unwrap();
    let displayed = initial
        .binding(&f.keys[0])
        .unwrap()
        .canonical
        .as_ref()
        .unwrap();
    assert!(host.edit(request.clone(), displayed, 7, true));
    assert!(!host.edit(request.clone(), displayed, 7, true));
    let mut busy = request.clone();
    busy.intent_sequence = 2;
    assert!(!host.edit(busy.clone(), displayed, 7, true));
    let Event::Edited(applied) = event(&mut host) else {
        panic!("edit did not commit")
    };
    assert_eq!(host.revision(), before.state_revision + 1);
    assert!(!host.published());
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        file_before
    );
    assert!(!host.edit(busy, displayed, 7, true));
    assert!(!host.intent(
        intent(before.state_revision, 1, crate::input::NativeAction::Save),
        7,
        true
    ));
    assert!(host.request(Request::Save));
    let Event::Saved(saved) = event(&mut host) else {
        panic!("save did not publish")
    };
    assert_eq!(
        saved.metadata.state_revision,
        applied.receipt.after.revision()
    );
    assert!(host.published());
    let (cold, _) = f
        .repository
        .load(
            Arc::clone(&f.catalogue),
            Limits::default(),
            Recovery::Strict,
        )
        .unwrap();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected
        .reference_states
        .iter_mut()
        .find(|entry| entry.id == request.reference)
        .unwrap()
        .state = request.state.clone();
    assert_eq!(cold.snapshot(), expected);
    assert!(host.request(Request::Continue));
    let Event::Continued(observed) = event(&mut host) else {
        panic!("Continue failed")
    };
    assert_eq!(
        observed
            .binding(&f.keys[0])
            .unwrap()
            .canonical
            .as_ref()
            .unwrap()
            .state(),
        Some(&request.state)
    );
    assert_eq!(observed.draws, applied.observation.draws);
    // A View sampled before that byte-identical Continue cannot stage another edit.
    let mut stale = request;
    stale.expected_revision = host.revision();
    stale.intent_sequence = 3;
    assert!(host.edit(stale, &applied.receipt.after, 7, true));
    assert!(matches!(event(&mut host), Event::Failed(_)));
    assert_eq!(host.revision(), expected.state_revision);
    stop(host);
}

#[test]
fn edit_save_failure_keeps_unsaved_commit_distinct_from_durable_repository() {
    let f = fixture();
    let request = edit_request(&f.world, &f.cell, &f.keys[0]);
    let (mut host, initial) = f.session().start([0.; 3], Shutdown::default()).unwrap();
    let view = initial
        .binding(&f.keys[0])
        .unwrap()
        .canonical
        .as_ref()
        .unwrap();
    assert!(!host.edit(request.clone(), view, 8, true));
    let mut accepted = request.clone();
    accepted.intent_sequence = 2;
    assert!(host.edit(accepted, view, 7, true));
    let Event::Edited(edited) = event(&mut host) else {
        panic!("edit failed")
    };
    fs::create_dir(f.repository.path().join("previous.frsv")).unwrap();
    let exact = fs::read(f.repository.path().join("current.frsv")).unwrap();
    assert!(host.request(Request::Save));
    assert!(matches!(event(&mut host), Event::Failed(_)));
    assert!(!host.published());
    assert!(host.title().starts_with("Native failed:"));
    assert_eq!(host.revision(), edited.receipt.after.revision());
    assert_eq!(
        fs::read(f.repository.path().join("current.frsv")).unwrap(),
        exact
    );
    fs::remove_dir(f.repository.path().join("previous.frsv")).unwrap();
    assert!(host.request(Request::Save));
    let Event::Saved(saved) = event(&mut host) else {
        panic!("save retry failed")
    };
    assert_eq!(
        saved.metadata.state_revision,
        edited.receipt.after.revision()
    );
    stop(host);
}
