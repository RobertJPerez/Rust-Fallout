//! Read-only canonical observations and an off-frame native save/Continue host.
//! The existing runtime owns restoration, captures, publication and identity.
use crate::model;
use bevy::prelude::*;
use fallout_data::{coordinates::Affine, identity::FormKey, loaded_scripts::Catalogue};
use fallout_runtime::{
    Limits, World,
    reference_state::View,
    save::{Captured, LoadReceipt, Recovery, Repository, SaveState, SaveStatus, SaveWorker},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug, Clone, Serialize)]
pub struct Binding {
    pub key: FormKey,
    pub canonical: Option<View>,
    pub source_affine: Option<Affine>,
    pub display: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub load: LoadReceipt,
    pub campaign: fallout_runtime::identity::CampaignId,
    pub catalogue_sha256: String,
    pub revision: u64,
    pub cell: FormKey,
    pub bindings: Vec<Binding>,
    pub scope: &'static str,
    pub original_save_compatibility: bool,
}

pub struct Observation {
    pub report: Report,
    pub draws: BTreeMap<FormKey, Option<Transform>>,
    index: BTreeMap<FormKey, usize>,
}
impl Observation {
    pub fn binding(&self, key: &FormKey) -> Option<&Binding> {
        self.index
            .get(key)
            .map(|index| &self.report.bindings[*index])
    }
}

pub struct Session {
    world: World<'static>,
    catalogue: Arc<Catalogue>,
    repository: Repository,
    cell: FormKey,
    keys: Vec<FormKey>,
    load: LoadReceipt,
}

#[derive(Clone, Default)]
pub struct Shutdown(Arc<ShutdownState>);
#[derive(Default)]
struct ShutdownState {
    closed: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}
impl Shutdown {
    /// Drain after App::run, never from a render/input frame. A late cancelled
    /// loader cannot admit another owner once shutdown has closed admission.
    pub fn finish(&self) -> model::Result<()> {
        let tasks = {
            let mut tasks = self
                .0
                .tasks
                .lock()
                .map_err(|_| "Native shutdown registry poisoned")?;
            self.0.closed.store(true, Ordering::Release);
            std::mem::take(&mut *tasks)
        };
        let mut panicked = false;
        for task in tasks {
            panicked |= task.join().is_err();
        }
        if panicked {
            return Err("Native host panicked during shutdown".into());
        }
        Ok(())
    }
}

impl Session {
    pub fn load(
        path: &Path,
        protected: &[PathBuf],
        catalogue: Arc<Catalogue>,
        cell: FormKey,
        keys: Vec<FormKey>,
    ) -> model::Result<Self> {
        if keys.len() > 10_000 || keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(
                "Canonical display requires at most 10000 unique sorted source keys".into(),
            );
        }
        let repository = Repository::open(path, protected)?;
        let (world, load) =
            repository.load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict)?;
        Ok(Self {
            world,
            catalogue,
            repository,
            cell,
            keys,
            load,
        })
    }

    /// No registration, initialization or state commit is performed by display.
    pub fn bindings(&self) -> model::Result<Vec<Binding>> {
        bindings(&self.world, &self.cell, &self.keys)
    }

    pub fn start(self, origin: [f64; 3], shutdown: Shutdown) -> model::Result<(Host, Observation)> {
        let initial = observe(&self.world, &self.cell, &self.keys, origin, self.load)?;
        let (commands, receiver) = mpsc::sync_channel(1);
        let (results, replies) = mpsc::sync_channel(1);
        let title = format!(
            "Native revision {} restored; F5 save; F9 Continue",
            initial.report.revision
        );
        let mut tasks = shutdown
            .0
            .tasks
            .lock()
            .map_err(|_| "Native shutdown registry poisoned")?;
        if shutdown.0.closed.load(Ordering::Acquire) || tasks.len() >= 8 {
            return Err("Native host admission closed or its eight-owner bound exhausted".into());
        }
        let closing = Arc::clone(&shutdown.0);
        tasks.push(
            thread::Builder::new()
                .name("fallout-preview-native".into())
                .spawn(move || {
                    run(
                        self.world,
                        self.catalogue,
                        self.repository,
                        self.cell,
                        self.keys,
                        origin,
                        receiver,
                        results,
                        closing,
                    );
                })?,
        );
        drop(tasks);
        Ok((
            Host {
                commands: Some(commands),
                replies: Mutex::new(replies),
                shutdown,
                pending: false,
                title,
            },
            initial,
        ))
    }
}

fn bindings(world: &World<'_>, cell: &FormKey, keys: &[FormKey]) -> model::Result<Vec<Binding>> {
    keys.iter()
        .map(|key| {
            let canonical = world
                .authored_reference(key)
                .map(|id| world.reference_view(id))
                .transpose()?;
            let mut binding = Binding {
                key: key.clone(),
                canonical,
                source_affine: None,
                display: "canonical-identity-unavailable",
            };
            if let Some(view) = &binding.canonical {
                if view.authored() != Some(key)
                    || view.campaign() != world.campaign()
                    || view.revision() != world.revision()
                    || view.catalogue_fingerprint() != world.catalogue_fingerprint()
                {
                    return Err("Canonical observation identity differs from restored world".into());
                }
                binding.display = match view.state() {
                    None => "canonical-state-unavailable",
                    Some(state) if state.cell() != cell => {
                        "canonical-reference-outside-selected-cell"
                    }
                    Some(state) if !state.enabled() => "canonical-disabled",
                    Some(state) => match state.pose().source_scale() {
                        None => "canonical-scale-unavailable",
                        Some(scale) => {
                            binding.source_affine = Some(Affine::nv_reference(
                                &state.pose().source_transform(),
                                scale,
                            )?);
                            "canonical-enabled"
                        }
                    },
                };
            }
            Ok(binding)
        })
        .collect()
}

fn observe(
    world: &World<'_>,
    cell: &FormKey,
    keys: &[FormKey],
    origin: [f64; 3],
    load: LoadReceipt,
) -> model::Result<Observation> {
    if origin.iter().any(|v| !v.is_finite()) {
        return Err("Canonical display origin must be finite".into());
    }
    let bindings = bindings(world, cell, keys)?;
    let index = bindings
        .iter()
        .enumerate()
        .map(|(index, binding)| (binding.key.clone(), index))
        .collect();
    let mut draws = BTreeMap::new();
    for binding in &bindings {
        let transform = binding
            .source_affine
            .map(|source| -> model::Result<Transform> {
                let matrix = model::affine(source.relative_view(origin).rows);
                let transform = Transform::from_matrix(matrix);
                if !matrix.is_finite() || !transform.is_finite() {
                    return Err(
                        "Canonical pose overflows the renderer's finite f32 transform".into(),
                    );
                }
                Ok(transform)
            })
            .transpose()?;
        draws.insert(binding.key.clone(), transform);
    }
    Ok(Observation {
        draws,
        index,
        report: Report {
            schema_version: 1,
            load,
            campaign: world.campaign(),
            catalogue_sha256: world.catalogue_fingerprint().into(),
            revision: world.revision(),
            cell: cell.clone(),
            bindings,
            scope: "Source-bound project-native canonical pose/enable display; immutable observations; no source initialization or gameplay simulation",
            original_save_compatibility: false,
        },
    })
}

#[derive(Clone, Copy)]
pub enum Request {
    Save,
    Continue,
}

pub enum Event {
    Saved(fallout_runtime::save::WriteReceipt),
    Continued(Box<Observation>),
    Failed(String),
}

pub struct Host {
    commands: Option<SyncSender<Request>>,
    replies: Mutex<Receiver<Event>>,
    shutdown: Shutdown,
    pending: bool,
    title: String,
}

impl Host {
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn pending(&self) -> bool {
        self.pending
    }

    pub fn request(&mut self, request: Request) -> bool {
        if self.pending {
            return false;
        }
        if self.shutdown.0.closed.load(Ordering::Acquire) {
            self.failure("Native host admission closed".into());
            return false;
        }
        match self
            .commands
            .as_ref()
            .ok_or("Native host stopped")
            .and_then(|sender| {
                sender
                    .try_send(request)
                    .map_err(|_| "Native host unavailable")
            }) {
            Ok(()) => {
                self.pending = true;
                self.title = match request {
                    Request::Save => "Save pending publication",
                    Request::Continue => "Continue validating current native save",
                }
                .into();
                true
            }
            Err(error) => {
                self.failure(error.into());
                false
            }
        }
    }

    pub fn failure(&mut self, error: String) {
        self.title = format!(
            "Native failed: {}",
            error.chars().take(160).collect::<String>()
        );
    }

    pub fn poll(&mut self) -> Option<Event> {
        if !self.pending {
            return None;
        }
        let result = self
            .replies
            .get_mut()
            .expect("exclusive native host receiver")
            .try_recv();
        let event = match result {
            Ok(event) => event,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                Event::Failed("Native host stopped before returning this request".into())
            }
        };
        self.pending = false;
        match &event {
            Event::Saved(receipt) => {
                self.title = format!(
                    "Save published generation {} revision {}; F5 save; F9 Continue",
                    receipt.metadata.generation, receipt.metadata.state_revision
                )
            }
            Event::Continued(observation) => {
                self.title = format!(
                    "Continue restored revision {}; F5 save; F9 Continue",
                    observation.report.revision
                )
            }
            Event::Failed(error) => self.failure(error.clone()),
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
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
        let mut store = RecordStore::open_nv_headers(
            &data,
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
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
        let mut store = RecordStore::open_nv_headers(
            &data,
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap();
        let changed =
            Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
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
}

impl Drop for Host {
    fn drop(&mut self) {
        // Disconnect admission. The owner drains accepted publication on its
        // own thread; a slow disk cannot turn window close into a frame join.
        self.commands.take();
        // Join ownership remains in Shutdown for collection outside App::run.
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "The runtime thread owns source, authority, request and result channels separately"
)]
fn run(
    mut world: World<'static>,
    catalogue: Arc<Catalogue>,
    repository: Repository,
    cell: FormKey,
    keys: Vec<FormKey>,
    origin: [f64; 3],
    commands: Receiver<Request>,
    results: SyncSender<Event>,
    closing: Arc<ShutdownState>,
) {
    // This is the only writer handle, and it is dropped outside the frame loop.
    let mut writer: Option<SaveWorker> = None;
    loop {
        let request = if closing.closed.load(Ordering::Acquire) {
            // Drain an already admitted host command before exiting. No new
            // request can pass Host::request after admission is closed.
            match commands.try_recv() {
                Ok(request) => request,
                Err(_) => break,
            }
        } else {
            match commands.recv_timeout(Duration::from_millis(5)) {
                Ok(request) => request,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        let event = match request {
            Request::Continue => {
                let result =
                    repository.load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict);
                match result {
                    Ok((fresh, load)) if fresh.campaign() == world.campaign() => {
                        match observe(&fresh, &cell, &keys, origin, load) {
                            Ok(observation) => {
                                world = fresh;
                                Event::Continued(Box::new(observation))
                            }
                            Err(error) => Event::Failed(error.to_string()),
                        }
                    }
                    Ok(_) => Event::Failed(
                        "Continue campaign differs from the active canonical world".into(),
                    ),
                    Err(error) => Event::Failed(error.to_string()),
                }
            }
            Request::Save => {
                if writer.is_none() {
                    match SaveWorker::start(repository.clone(), 1) {
                        Ok(worker) => writer = Some(worker),
                        Err(error) => {
                            if results.send(Event::Failed(error.to_string())).is_err() {
                                break;
                            }
                            continue;
                        }
                    }
                }
                match writer
                    .as_mut()
                    .expect("started writer")
                    .try_submit(Captured::at_boundary(&world))
                {
                    Err(error) => Event::Failed(error.to_string()),
                    Ok(ticket) => {
                        let mut status = SaveStatus::new(ticket);
                        while matches!(status.poll(), SaveState::Pending) {
                            thread::sleep(Duration::from_millis(2));
                        }
                        match status.wait() {
                            Ok(receipt) => Event::Saved(receipt),
                            Err(error) => Event::Failed(error.to_string()),
                        }
                    }
                }
            }
        };
        if results.send(event).is_err() {
            break;
        }
    }
    if let Some(writer) = writer
        && let Err(error) = writer.finish()
    {
        eprintln!("Native host writer shutdown: {error}");
    }
}
