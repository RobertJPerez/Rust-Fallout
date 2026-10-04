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

pub mod edit;
#[cfg(test)]
mod render_tests;

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
    panicked: AtomicBool,
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
        let mut panicked = self.0.panicked.load(Ordering::Acquire);
        for task in tasks {
            panicked |= task.join().is_err();
        }
        if panicked {
            self.0.panicked.store(true, Ordering::Release);
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

    /// Called by source preparation, outside render/input updates. Only returned
    /// owners are joined here; outstanding owners remain for final shutdown.
    pub fn start(self, origin: [f64; 3], shutdown: Shutdown) -> model::Result<(Host, Observation)> {
        let provenance = copy_load(&self.load);
        let initial = observe(&self.world, &self.cell, &self.keys, origin, self.load)?;
        let (commands, receiver) = mpsc::sync_channel(1);
        let (results, replies) = mpsc::sync_channel(1);
        let title = restored_title(&initial, "Native");
        let mut tasks = shutdown
            .0
            .tasks
            .lock()
            .map_err(|_| "Native shutdown registry poisoned")?;
        if shutdown.0.closed.load(Ordering::Acquire) {
            return Err("Native host admission closed".into());
        }
        let mut index = 0;
        while index < tasks.len() {
            if tasks[index].is_finished() {
                if tasks.swap_remove(index).join().is_err() {
                    shutdown.0.panicked.store(true, Ordering::Release);
                }
            } else {
                index += 1;
            }
        }
        if shutdown.0.panicked.load(Ordering::Acquire) {
            return Err("Native host panicked before retry".into());
        }
        if tasks.len() >= 8 {
            return Err("Native host eight-outstanding-owner bound exhausted".into());
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
                        provenance,
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
                published: false,
                title,
                revision: initial.report.revision,
                last_intent_sequence: 0,
                last_edit_sequence: 0,
                applied_edit: None,
            },
            initial,
        ))
    }
}

fn copy_load(load: &LoadReceipt) -> LoadReceipt {
    LoadReceipt {
        slot: load.slot,
        metadata: load.metadata.clone(),
        current_failure: load.current_failure.clone(),
        current_repaired: load.current_repaired,
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

#[derive(Clone)]
pub enum Request {
    Save,
    Continue,
    Edit(Box<edit::Command>),
}

struct Command {
    request: Request,
    expected_revision: u64,
}

pub enum Event {
    Saved(fallout_runtime::save::WriteReceipt),
    Continued(Box<Observation>),
    Edited(Box<edit::Applied>),
    Failed(String),
}

pub struct Host {
    commands: Option<SyncSender<Command>>,
    replies: Mutex<Receiver<Event>>,
    shutdown: Shutdown,
    pending: bool,
    published: bool,
    title: String,
    revision: u64,
    last_intent_sequence: u64,
    last_edit_sequence: u64,
    applied_edit: Option<edit::Receipt>,
}

impl Host {
    pub fn title(&self) -> &str {
        &self.title
    }
    pub fn published(&self) -> bool {
        self.published
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn applied_edit(&self) -> Option<&edit::Receipt> {
        self.applied_edit.as_ref()
    }

    /// The adapter supplies the actual current ECS observation, never a forged
    /// runtime View. The owner repeats authority checks before stage/commit.
    pub fn edit(
        &mut self,
        request: edit::Request,
        displayed: &View,
        scene_epoch: u64,
        admitted: bool,
    ) -> bool {
        if request.intent_sequence <= self.last_edit_sequence {
            return false;
        }
        self.last_edit_sequence = request.intent_sequence;
        if self.pending {
            return false;
        }
        if !admitted
            || request.scene_epoch != scene_epoch
            || request.expected_revision != self.revision
            || edit::validate_display(&request, displayed).is_err()
        {
            self.failure("Reference edit belongs to a different displayed identity".into());
            return false;
        }
        self.request(Request::Edit(Box::new(edit::Command {
            request,
            displayed: displayed.clone(),
        })))
    }

    pub fn intent(
        &mut self,
        intent: crate::input::NativeIntent,
        scene_epoch: u64,
        admitted: bool,
    ) -> bool {
        if intent.sequence <= self.last_intent_sequence {
            return false;
        }
        // Consume once even if this single-slot owner is busy; no replay queue.
        self.last_intent_sequence = intent.sequence;
        if self.pending {
            return false;
        }
        if !admitted
            || !intent.focused
            || !matches!(
                intent.context,
                crate::input::Context::Orbit | crate::input::Context::Fly
            )
            || intent.device != crate::input::Device::Keyboard
            || intent.display.scene_epoch != scene_epoch
            || intent.display.revision != self.revision
        {
            self.failure(
                "Native input belongs to a different display/window/focus/context boundary".into(),
            );
            return false;
        }
        self.request(match intent.action {
            crate::input::NativeAction::Save => Request::Save,
            crate::input::NativeAction::Continue => Request::Continue,
        })
    }

    pub fn request(&mut self, request: Request) -> bool {
        if self.pending {
            return false;
        }
        if self.shutdown.0.closed.load(Ordering::Acquire) {
            self.failure("Native host admission closed".into());
            return false;
        }
        let saving = matches!(request, Request::Save);
        let title = match &request {
            Request::Save => "Save pending publication",
            Request::Continue => "Continue validating current native save",
            Request::Edit(_) => "Engineering reference edit pending canonical commit",
        };
        match self
            .commands
            .as_ref()
            .ok_or("Native host stopped")
            .and_then(|sender| {
                sender
                    .try_send(Command {
                        request,
                        expected_revision: self.revision,
                    })
                    .map_err(|_| "Native host unavailable")
            }) {
            Ok(()) => {
                self.pending = true;
                if saving {
                    self.published = false;
                }
                self.title = title.into();
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
                self.published = true;
                self.title = format!(
                    "Save published generation {} revision {}; F5 save; F9 Continue",
                    receipt.metadata.generation, receipt.metadata.state_revision
                )
            }
            Event::Continued(observation) => {
                self.revision = observation.report.revision;
                self.applied_edit = None;
                self.title = restored_title(observation, "Continue");
            }
            Event::Edited(applied) => {
                self.revision = applied.observation.report.revision;
                self.published = false;
                self.applied_edit = Some(applied.receipt.clone());
                self.title = format!(
                    "Engineering edit committed revision {}; unsaved; F5 save; F9 Continue",
                    self.revision
                );
            }
            Event::Failed(error) => self.failure(error.clone()),
        }
        Some(event)
    }
}

fn restored_title(observation: &Observation, action: &str) -> String {
    let available = observation
        .report
        .bindings
        .iter()
        .filter(|binding| binding.source_affine.is_some())
        .count();
    format!(
        "{action} restored revision {}; {available}/{} poses available; F5 save; F9 Continue",
        observation.report.revision,
        observation.report.bindings.len()
    )
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
    mut provenance: LoadReceipt,
    commands: Receiver<Command>,
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
        if request.expected_revision != world.revision() {
            if results
                .send(Event::Failed(
                    "Native command revision differs from the owned canonical boundary".into(),
                ))
                .is_err()
            {
                break;
            }
            continue;
        }
        let event = match request.request {
            Request::Edit(command) => match edit::apply(
                &mut world,
                &cell,
                &keys,
                origin,
                copy_load(&provenance),
                *command,
            ) {
                Ok(applied) => Event::Edited(Box::new(applied)),
                Err(error) => Event::Failed(error.to_string()),
            },
            Request::Continue => {
                let result =
                    repository.load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict);
                match result {
                    Ok((fresh, load)) if fresh.campaign() == world.campaign() => {
                        match observe(&fresh, &cell, &keys, origin, copy_load(&load)) {
                            Ok(observation) => {
                                world = fresh;
                                provenance = load;
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

#[cfg(test)]
mod tests;
