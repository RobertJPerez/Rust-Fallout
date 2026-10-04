//! A small inspection host for the production decoder, not a gameplay runtime.
mod fixture;
mod input;
mod loading;
#[cfg(test)]
mod loading_tests;
mod material;
mod model;
mod native;
mod pose;
mod scene;
mod startup;
mod terrain;
mod terrain_textures;
mod ui;
mod upload;

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    asset::RenderAssetUsages,
    camera::{RenderTarget, ScalingMode},
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::{ExitCondition, PrimaryWindow, WindowCloseRequested, WindowCreated},
    winit::WinitPlugin,
};
use clap::{ArgGroup, Parser};
use fallout_data::vfs::AssetPath;
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Parser, Resource, Clone)]
#[command(about = "Inspect New Vegas models, placed interiors or authored terrain")]
#[command(group(ArgGroup::new("mode").required(true).args(["model", "model_file", "cell", "terrain", "material_fixture", "menu"])))]
#[command(group(ArgGroup::new("model_source").args(["model", "model_file"])))]
struct Options {
    #[arg(skip)]
    native_shutdown: native::Shutdown,
    #[arg(long)]
    install: Option<PathBuf>,
    /// Archive path, for example meshes/furniture/chair01.nif.
    #[arg(long, requires = "install")]
    model: Option<String>,
    /// Exact bounded local NIF input; diffuse paths use the same archive lookup.
    #[arg(long, requires = "install")]
    model_file: Option<PathBuf>,
    /// Retain exact source menu XML in a local inspection report, without evaluating tiles.
    #[arg(long, requires_all = ["install", "report"], conflicts_with_all = ["capture", "headless"])]
    menu: Option<String>,
    /// Select exactly one authored name attribute; ambiguous names are refused.
    #[arg(long, requires = "menu")]
    menu_tile: Option<String>,
    /// Display exactly this source skin geometry in its stored local pose.
    #[arg(long, requires_all = ["model_source", "skin_weight_tolerance"], conflicts_with = "pose_object")]
    skin_geometry: Option<u32>,
    /// Validate raw unit weight sums; never repair or normalize weights.
    #[arg(long, requires = "skin_geometry")]
    skin_weight_tolerance: Option<f64>,
    /// Evaluate this exact source object at the caller's explicit source time.
    #[arg(long, requires_all = ["model_source", "pose_controller", "pose_time"], conflicts_with = "skin_geometry")]
    pose_object: Option<u32>,
    #[arg(long, requires = "pose_object")]
    pose_controller: Option<u32>,
    /// Direct source key time; authored clock flags/frequency remain unapplied.
    #[arg(long, requires = "pose_object", allow_hyphen_values = true)]
    pose_time: Option<f64>,
    /// Interior CELL editor ID, for example GSDocMitchellHouse.
    #[arg(long, requires_all = ["load_order", "install"])]
    cell: Option<String>,
    /// Exterior terrain CELL editor ID, for example Goodsprings. No gameplay is simulated.
    #[arg(long, requires_all = ["load_order", "install"])]
    terrain: Option<String>,
    /// Draw authored diffuse layers; missing bases and NULL defaults fail explicitly.
    #[arg(long, requires_all = ["terrain", "terrain_texture_repeat"])]
    terrain_textures: bool,
    /// Explicit inspection tiling per quadrant, with no claimed retail scale.
    #[arg(long, requires = "terrain_textures")]
    terrain_texture_repeat: Option<f32>,
    /// Check synthetic material states on the GPU without reading game assets.
    #[arg(long, requires_all = ["headless", "report"])]
    material_fixture: bool,
    #[arg(long, requires = "install")]
    load_order: Option<PathBuf>,
    /// Source-bound current project-native save repository; F5 save/F9 Continue.
    #[arg(long, requires = "cell")]
    native_save: Option<PathBuf>,
    /// Engineering capture: submit one real save after complete draw admission.
    #[arg(long, requires_all = ["native_save", "headless"])]
    native_save_after_ready: bool,
    /// Camera position in original source units (x,y,z).
    #[arg(long, num_args = 3, value_delimiter = ',', allow_negative_numbers = true,
        requires_all = ["camera_look_at", "load_order"])]
    camera_position: Option<Vec<f64>>,
    #[arg(long, num_args = 3, value_delimiter = ',', allow_negative_numbers = true,
        requires_all = ["camera_position", "load_order"])]
    camera_look_at: Option<Vec<f64>>,
    /// Write a PNG and exit after the GPU capture completes. Existing files are refused.
    #[arg(long)]
    capture: Option<PathBuf>,
    /// Render to an image without opening a desktop window.
    #[arg(long, requires = "capture")]
    headless: bool,
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Resource)]
struct Orbit {
    center: Vec3,
    radius: f32,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Orbit {
    fn transform(&self) -> Transform {
        let direction = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        Transform::from_translation(self.center + direction * self.distance)
            .looking_at(self.center, Vec3::Y)
    }
}

#[derive(Resource)]
struct Capture {
    target: Option<Handle<Image>>,
    frame: usize,
    started: Instant,
}

#[derive(Resource)]
struct Navigation {
    fly: bool,
    home: Transform,
}

struct ReadyScene {
    upload: DrawScene,
    orbit: Orbit,
    navigation: Navigation,
    fixture: Option<fixture::Report>,
}

struct DrawScene {
    queue: upload::Queue,
    cell: Option<scene::CellSources>,
}

impl DrawScene {
    fn status(&self) -> String {
        self.queue.status()
    }

    fn advance(
        &mut self,
        commands: &mut Commands,
        assets: &mut upload::Resources,
        epoch: u64,
    ) -> Result<bool, String> {
        if let Some(cell) = &self.cell {
            cell.sources
                .ticket()
                .check()
                .map_err(|error| error.to_string())?;
            cell.textures
                .ticket()
                .check()
                .map_err(|error| error.to_string())?;
        }
        if !self.queue.advance(commands, assets, epoch)? {
            return Ok(false);
        }
        let queue = &mut self.queue;
        if let Some(cell) = &mut self.cell {
            cell.owner
                .get_mut()
                .map_err(|_| "Source residency owner poisoned".to_string())?
                .publish_render(&cell.ticket, || {
                    queue
                        .publish(commands, epoch)
                        .map_err(fallout_data::resource_jobs::JobError::Invalid)
                })
                .map_err(|error| error.to_string())?;
        } else {
            queue.publish(commands, epoch)?;
        }
        Ok(true)
    }

    fn dispose(&mut self, commands: &mut Commands, assets: &mut upload::Resources) -> bool {
        if !self.queue.retiring()
            && let Some(cell) = &mut self.cell
        {
            cell.native.take();
            match cell.owner.get_mut() {
                Ok(owner) => {
                    if let Err(error) = owner.unload() {
                        error!("Source residency unload failed: {error}");
                    }
                }
                Err(error) => error!("Source residency owner poisoned: {error}"),
            }
        }
        self.queue.dispose(commands, assets)
    }
}

enum Phase {
    WaitingForWindow,
    Preparing(loading::Job<ReadyScene>),
    Draining(loading::Job<ReadyScene>),
    Uploading(DrawScene),
    Ready(DrawScene),
    Disposing(DrawScene, Option<String>),
    Failed(String),
    Cancelled,
}

#[derive(Resource)]
struct Loading {
    epoch: u64,
    phase: Phase,
}

type InspectionCameraFilter = (With<Camera3d>, Without<scene::ReferenceView>);

fn start_preparation(options: &Options, epoch: u64) -> Result<loading::Job<ReadyScene>, String> {
    let request = options.clone();
    loading::Job::start(epoch, move |context| {
        prepare_scene(&request, &context, epoch).map_err(|error| error.to_string())
    })
    .map_err(|error| error.to_string())
}

fn retry_outputs(options: &Options) -> Result<(), String> {
    for path in [options.report.as_ref(), options.capture.as_ref()]
        .into_iter()
        .flatten()
    {
        if path
            .try_exists()
            .map_err(|error| format!("Retry output check failed: {error}"))?
        {
            return Err(format!(
                "Retry refused: output {} already exists; start a new run with fresh output paths",
                path.display()
            ));
        }
    }
    Ok(())
}

fn output_path(
    path: &Path,
    install: Option<&Path>,
    source_file: Option<&Path>,
) -> model::Result<PathBuf> {
    let name = path.file_name().ok_or("output needs a file name")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let result = parent.canonicalize()?.join(name);
    if let Some(install) = install
        && result.starts_with(install.canonicalize()?)
    {
        return Err("output must be outside the installation".into());
    }
    if let Some(source) = source_file {
        let parent = source
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if result.starts_with(parent.canonicalize()?) {
            return Err("output must be outside the explicit model source directory".into());
        }
    }
    if result.try_exists()? {
        return Err(format!("output already exists: {}", result.display()).into());
    }
    Ok(result)
}

fn run() -> model::Result<AppExit> {
    let mut options = Options::parse();
    if let Some(path) = &options.capture {
        options.capture = Some(output_path(
            path,
            options.install.as_deref(),
            options.model_file.as_deref(),
        )?);
    }
    if let Some(path) = &options.report {
        options.report = Some(output_path(
            path,
            options.install.as_deref(),
            options.model_file.as_deref(),
        )?);
    }
    if options.capture.is_some() && options.capture == options.report {
        return Err("capture and report must have different paths".into());
    }
    if let Some(menu) = &options.menu {
        let report = ui::inspect(
            options
                .install
                .as_deref()
                .expect("menu requires installation"),
            &AssetPath::new(menu.as_bytes())?,
            options.menu_tile.as_deref(),
            ui::Limits::default(),
        )?;
        let path = options.report.as_ref().expect("menu requires report");
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        ui::write_report(file, &report, ui::Limits::default().output_bytes)?;
        eprintln!(
            "Menu source tree retained: {} nodes; tile evaluation/display remains unavailable",
            report.document.nodes.len()
        );
        return Ok(AppExit::Success);
    }
    let headless = options.headless;
    let native_shutdown = options.native_shutdown.clone();
    let orbit = Orbit {
        center: Vec3::ZERO,
        radius: 1.,
        yaw: 2.5,
        pitch: 0.3,
        distance: 3.,
    };
    let navigation = Navigation {
        fly: options.camera_position.is_some(),
        home: if options.material_fixture {
            Transform::from_xyz(0., 0., 1000.).looking_at(Vec3::ZERO, Vec3::Y)
        } else {
            orbit.transform()
        },
    };
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: (!headless).then(|| Window {
                title: "Fallout Rust - Loading source data".into(),
                resolution: (1280, 900).into(),
                ..default()
            }),
            exit_condition: if headless {
                ExitCondition::DontExit
            } else {
                ExitCondition::OnAllClosed
            },
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        });
    if headless {
        plugins = plugins.disable::<WinitPlugin>();
    }
    let mut app = App::new();
    app.add_plugins(plugins)
        .add_plugins(material::InspectionPlugin)
        .add_plugins(input::InspectionInputPlugin)
        .insert_resource(if headless {
            input::Context::Suspended
        } else {
            input::Context::Loading
        })
        .insert_resource(options)
        .insert_resource(orbit)
        .insert_resource(navigation)
        .insert_resource(Loading {
            epoch: 1,
            phase: Phase::WaitingForWindow,
        })
        .insert_resource(ClearColor(Color::srgb(0.035, 0.045, 0.055)))
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, drive_loading, capture).chain());
    if headless {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)));
    }
    let result = app.run();
    native_shutdown.finish()?;
    Ok(result)
}

fn prepare_scene(
    options: &Options,
    context: &loading::Context,
    epoch: u64,
) -> model::Result<ReadyScene> {
    context.stage("Reading source data")?;
    let pose = options
        .skin_geometry
        .map(|geometry| {
            pose::Request::Skin(pose::SkinRequest {
                geometry,
                absolute_weight_tolerance: options
                    .skin_weight_tolerance
                    .expect("clap requires tolerance"),
            })
        })
        .or_else(|| {
            options.pose_object.map(|object| {
                pose::Request::Object(pose::ObjectRequest {
                    object,
                    controller: options.pose_controller.expect("clap requires controller"),
                    source_time: options.pose_time.expect("clap requires source time"),
                })
            })
        });
    let mut cell_sources = None;
    let (prepared, report) = if options.material_fixture {
        let (prepared, report) = fixture::prepare()?;
        (prepared, scene::Report::Fixture(report))
    } else if let Some(name) = &options.model {
        scene::load_model(
            options.install.as_deref().expect("clap requires install"),
            &AssetPath::new(name.as_bytes())?,
            pose,
        )?
    } else if let Some(path) = &options.model_file {
        scene::load_model_file(
            options.install.as_deref().expect("clap requires install"),
            path,
            pose,
        )?
    } else if let Some(name) = &options.terrain {
        terrain::load(
            options.install.as_deref().expect("clap requires install"),
            options.load_order.as_deref().expect("clap requires order"),
            name,
            options.terrain_texture_repeat,
        )?
    } else {
        let (prepared, report, sources) = scene::load_cell(
            options.install.as_deref().expect("clap requires install"),
            options
                .load_order
                .as_deref()
                .expect("clap requires load order"),
            options
                .cell
                .as_deref()
                .expect("clap requires model or cell"),
            context,
            options.native_save.as_deref(),
            options.native_shutdown.clone(),
        )?;
        cell_sources = Some(sources);
        (prepared, report)
    };
    match &report {
        scene::Report::Model(report) => eprintln!(
            "{} meshes, {} vertices, {} triangles, {} diffuse textures; {} recorded limitations",
            report.meshes,
            report.vertices,
            report.triangles,
            report.textures.len(),
            report.warnings.len()
        ),
        scene::Report::Cell(report) => eprintln!(
            "{} placed references, {} shared models, {} mesh instances, {} texture samplers; {} omitted references",
            report.rendered_references,
            report.unique_render_models,
            report.rendered_mesh_instances,
            report.unique_texture_samplers,
            report.placements.len() - report.rendered_references
        ),
        scene::Report::Fixture(report) => {
            eprintln!("{} synthetic material GPU cases", report.cases.len())
        }
        scene::Report::Terrain(report) => eprintln!(
            "{} terrain vertices, {} triangles; {}",
            report.vertices, report.triangles, report.rendering
        ),
    }
    eprintln!(
        "Tab/Select: orbit/fly; WASD/left stick: move; Q/E or shoulders: vertical/zoom; arrows/right stick or right-drag: look; wheel: orbit zoom; Shift/left stick click: faster; R/Y: reset; Esc/Start: close"
    );
    context.check()?;
    if let Some(path) = &options.report
        && !options.material_fixture
    {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
    }
    context.check()?;
    if !prepared.center.is_finite()
        || !prepared.radius.is_finite()
        || prepared.radius <= 0.
        || !(prepared.radius * 100.).is_finite()
    {
        return Err("Prepared camera bounds must be finite and positive".into());
    }
    let orbit = Orbit {
        center: prepared.center,
        radius: prepared.radius,
        yaw: 2.5,
        pitch: 0.3,
        distance: prepared.radius * 3.,
    };
    let home = match (&options.camera_position, &options.camera_look_at) {
        _ if options.material_fixture => {
            Transform::from_xyz(0., 0., 1000.).looking_at(Vec3::ZERO, Vec3::Y)
        }
        (Some(position), Some(target)) => scene::source_camera(
            position.as_slice().try_into()?,
            target.as_slice().try_into()?,
            prepared.origin,
        )?,
        _ => orbit.transform(),
    };
    let navigation = Navigation {
        fly: options.camera_position.is_some(),
        home,
    };
    context.stage("Preparing bounded draw uploads")?;
    Ok(ReadyScene {
        upload: DrawScene {
            queue: upload::Queue::new(epoch, prepared)?,
            cell: cell_sources,
        },
        orbit,
        navigation,
        fixture: if let scene::Report::Fixture(report) = report {
            Some(report)
        } else {
            None
        },
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Loading coordinates separate host resource owners"
)]
fn drive_loading(
    mut commands: Commands,
    options: Res<Options>,
    actions: Res<input::Actions>,
    mut state: ResMut<Loading>,
    mut context: ResMut<input::Context>,
    mut orbit: ResMut<Orbit>,
    mut navigation: ResMut<Navigation>,
    mut cameras: Query<(&mut Transform, &mut Projection), InspectionCameraFilter>,
    mut references: Query<
        (&mut scene::ReferenceView, &mut Transform, &mut Visibility),
        Without<Camera3d>,
    >,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut created: MessageReader<WindowCreated>,
    mut closed: MessageReader<WindowCloseRequested>,
    mut assets: upload::Resources,
    mut capture: ResMut<Capture>,
    mut exit: MessageWriter<AppExit>,
) {
    let window_ready = created
        .read()
        .any(|event| windows.iter().any(|(id, _)| id == event.window));
    let closing = closed
        .read()
        .any(|event| windows.iter().any(|(id, _)| id == event.window))
        || actions.close;
    let epoch = state.epoch;
    let mut phase = std::mem::replace(&mut state.phase, Phase::Cancelled);
    if closing {
        state.epoch = state.epoch.saturating_add(1);
        match phase {
            Phase::Preparing(mut job) | Phase::Draining(mut job) => {
                job.cancel();
                // Retain a result queued before close. Its potentially large
                // draw payload belongs to bounded retirement or app teardown,
                // never an implicit receiver drop inside this window update.
                state.phase = Phase::Draining(job);
            }
            Phase::Uploading(mut queue)
            | Phase::Ready(mut queue)
            | Phase::Disposing(mut queue, _) => {
                if !queue.dispose(&mut commands, &mut assets) {
                    state.phase = Phase::Disposing(queue, None);
                }
            }
            _ => {}
        }
        *context = input::Context::Suspended;
        return;
    }
    if actions.cancel_loading && !options.headless {
        phase = match phase {
            Phase::WaitingForWindow => Phase::Cancelled,
            Phase::Preparing(mut job) => {
                job.cancel();
                state.epoch = state.epoch.checked_add(1).unwrap_or(state.epoch);
                Phase::Draining(job)
            }
            Phase::Uploading(queue) => {
                state.epoch = state.epoch.checked_add(1).unwrap_or(state.epoch);
                Phase::Disposing(queue, None)
            }
            phase => phase,
        };
    }
    let failure = |error: String, exit: &mut MessageWriter<AppExit>| {
        error!("Source scene failed: {error}");
        if options.headless {
            exit.write(AppExit::error());
        }
        Phase::Failed(error)
    };
    state.phase = match phase {
        Phase::WaitingForWindow if options.headless || window_ready => {
            match start_preparation(&options, epoch) {
                Ok(job) => Phase::Preparing(job),
                Err(error) => failure(error, &mut exit),
            }
        }
        Phase::Draining(mut job) => match job.retire() {
            loading::Retirement::Pending => Phase::Draining(job),
            loading::Retirement::Done(Some(mut ready)) => {
                if ready.upload.dispose(&mut commands, &mut assets) {
                    Phase::Cancelled
                } else {
                    Phase::Disposing(ready.upload, None)
                }
            }
            loading::Retirement::Done(None) => Phase::Cancelled,
        },
        Phase::Failed(_) | Phase::Cancelled
            if actions.retry_loading && !actions.cancel_loading && !options.headless =>
        {
            let request = retry_outputs(&options).and_then(|()| {
                let next = state
                    .epoch
                    .checked_add(1)
                    .ok_or("Source retry epoch exhausted")?;
                state.epoch = next;
                start_preparation(&options, next)
            });
            match request {
                Ok(job) => Phase::Preparing(job),
                Err(error) => failure(error, &mut exit),
            }
        }
        Phase::Preparing(mut job) => {
            if options.headless && job.status().1 > Duration::from_secs(600) {
                job.cancel();
                failure("Source preparation timed out".into(), &mut exit)
            } else {
                match job.poll(epoch) {
                    loading::Poll::Pending => Phase::Preparing(job),
                    loading::Poll::Ready(ready) => {
                        *orbit = ready.orbit;
                        *navigation = ready.navigation;
                        for (mut transform, mut projection) in &mut cameras {
                            *transform = navigation.home;
                            if let Projection::Perspective(perspective) = &mut *projection {
                                perspective.near = (orbit.radius * 0.001).max(0.01);
                                perspective.far = orbit.radius * 100.;
                            }
                        }
                        if let Some(report) = ready.fixture {
                            commands.insert_resource(report);
                        }
                        Phase::Uploading(ready.upload)
                    }
                    loading::Poll::Failed(error) => failure(error, &mut exit),
                    loading::Poll::Cancelled => Phase::Cancelled,
                    loading::Poll::Finished => {
                        failure("Source result was already consumed".into(), &mut exit)
                    }
                }
            }
        }
        Phase::Uploading(mut queue) => match queue.advance(&mut commands, &mut assets, epoch) {
            Ok(false) => Phase::Uploading(queue),
            Ok(true) => {
                *context = if options.headless || options.material_fixture {
                    input::Context::Suspended
                } else if navigation.fly {
                    input::Context::Fly
                } else {
                    input::Context::Orbit
                };
                capture.frame = 0;
                capture.started = Instant::now();
                startup::stage("Source scene admitted; graphics settling before capture.");
                if options.native_save_after_ready
                    && let Some(host) = queue.cell.as_mut().and_then(|cell| cell.native.as_mut())
                {
                    host.request(native::Request::Save);
                }
                Phase::Ready(queue)
            }
            Err(error) => {
                if queue.dispose(&mut commands, &mut assets) {
                    failure(error, &mut exit)
                } else {
                    Phase::Disposing(queue, Some(error))
                }
            }
        },
        Phase::Disposing(mut queue, error) => {
            if queue.dispose(&mut commands, &mut assets) {
                if let Some(error) = error {
                    failure(error, &mut exit)
                } else {
                    Phase::Cancelled
                }
            } else {
                Phase::Disposing(queue, error)
            }
        }
        Phase::Ready(mut queue) => {
            if let Some(host) = queue.cell.as_mut().and_then(|cell| cell.native.as_mut()) {
                if let Some(event) = host.poll() {
                    match event {
                        native::Event::Continued(observation) => {
                            // Validate the complete destination set before changing any entity.
                            let valid = references
                                .iter()
                                .all(|(view, _, _)| observation.draws.contains_key(&view.key));
                            if valid {
                                for (mut view, mut transform, mut visibility) in &mut references {
                                    view.canonical = observation
                                        .binding(&view.key)
                                        .and_then(|binding| binding.canonical.clone());
                                    if let Some(next) = observation.draws[&view.key] {
                                        *transform = next;
                                        *visibility = Visibility::Inherited;
                                    } else {
                                        *visibility = Visibility::Hidden;
                                    }
                                }
                                eprintln!(
                                    "Continue source-bound revision {}",
                                    observation.report.revision
                                );
                            } else {
                                host.failure(
                                    "Continue does not bind every active source view".into(),
                                );
                            }
                        }
                        native::Event::Saved(receipt) => {
                            eprintln!(
                                "Native save publication receipt: {}",
                                serde_json::to_string(&receipt).expect("serializable save receipt")
                            );
                        }
                        native::Event::Failed(error) => {
                            error!("Native request failed: {error}");
                            if options.native_save_after_ready {
                                exit.write(AppExit::error());
                            }
                        }
                    }
                }
                if actions.continue_saved {
                    host.request(native::Request::Continue);
                } else if actions.save {
                    host.request(native::Request::Save);
                }
            }
            Phase::Ready(queue)
        }
        phase => phase,
    };
    let status = match &state.phase {
        Phase::WaitingForWindow => "Opening inspection window".into(),
        Phase::Preparing(job) => {
            let (message, elapsed) = job.status();
            format!(
                "Loading: {message} ({}s) — Backspace cancels",
                elapsed.as_secs()
            )
        }
        Phase::Draining(job) => format!(
            "Cancelling: waiting for source worker return ({}s) — Escape closes",
            job.status().1.as_secs()
        ),
        Phase::Uploading(queue) => format!("{} — Backspace cancels", queue.status()),
        Phase::Disposing(queue, _) => queue.queue.disposal_status(),
        Phase::Ready(queue) => queue
            .cell
            .as_ref()
            .and_then(|cell| cell.native.as_ref())
            .map_or_else(|| "Ready".into(), |host| host.title().into()),
        Phase::Failed(error) => format!(
            "Failed: {} — Enter retries; Escape closes",
            error.chars().take(180).collect::<String>()
        ),
        Phase::Cancelled => "Cancelled — Enter retries; Escape closes".into(),
    };
    for (_, mut window) in &mut windows {
        let title = format!("Fallout Rust - {status}");
        if window.title != title {
            window.title = title;
        }
    }
}

fn setup(
    mut commands: Commands,
    options: Res<Options>,
    navigation: Res<Navigation>,
    mut images: ResMut<Assets<Image>>,
) {
    startup::stage("Graphics initialized; source preparation follows window creation.");
    let camera = commands
        .spawn((
            Camera3d::default(),
            Tonemapping::None,
            navigation.home,
            if options.material_fixture {
                Projection::Orthographic(OrthographicProjection {
                    scaling_mode: ScalingMode::Fixed {
                        width: 1280.,
                        height: 900.,
                    },
                    near: 0.1,
                    far: 2000.,
                    ..OrthographicProjection::default_3d()
                })
            } else {
                Projection::Perspective(PerspectiveProjection {
                    near: 0.01,
                    far: 10_000.,
                    ..default()
                })
            },
            if options.material_fixture {
                Msaa::Off
            } else {
                Msaa::default()
            },
        ))
        .id();
    let target = if options.headless {
        let mut image = Image::new_uninit(
            Extent3d {
                width: 1280,
                height: 900,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
        let handle = images.add(image);
        commands
            .entity(camera)
            .insert(RenderTarget::Image(handle.clone().into()));
        Some(handle)
    } else {
        None
    };
    commands.insert_resource(Capture {
        target,
        frame: 0,
        started: Instant::now(),
    });
}

#[expect(
    clippy::too_many_arguments,
    reason = "Bevy separates camera, input, time and application exit owners"
)]
fn controls(
    options: Res<Options>,
    actions: Res<input::Actions>,
    mut context: ResMut<input::Context>,
    time: Res<Time>,
    mut orbit: ResMut<Orbit>,
    mut navigation: ResMut<Navigation>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if options.material_fixture {
        return;
    }
    if actions.close {
        exit.write(AppExit::Success);
        return;
    }
    let delta = time.delta_secs().min(0.1);
    if actions.toggle {
        navigation.fly = !navigation.fly;
        *context = if navigation.fly {
            input::Context::Fly
        } else {
            input::Context::Orbit
        };
        // The current sample belongs to the old camera mode. The input owner
        // quarantines its held buttons/sticks on the next context sample.
        return;
    }
    if navigation.fly {
        for mut transform in &mut cameras {
            if actions.reset {
                *transform = navigation.home;
                continue;
            }
            let yaw = -actions.look.x * delta - actions.pointer_look.x * 0.003;
            let pitch = actions.look.y * delta - actions.pointer_look.y * 0.003;
            transform.rotate_y(yaw);
            // Keep a small margin from vertical to avoid an ambiguous up direction.
            let current_pitch = transform.forward().y.asin();
            transform.rotate_local_x((current_pitch + pitch).clamp(-1.5, 1.5) - current_pitch);
            let movement = transform.forward().as_vec3() * actions.movement.y
                + transform.right().as_vec3() * actions.movement.x
                + Vec3::Y * actions.movement.z;
            let speed = if actions.fast { 600. } else { 180. };
            transform.translation += movement.clamp_length_max(1.) * speed * delta;
        }
        return;
    }
    orbit.yaw += (actions.movement.x + actions.look.x) * delta + actions.pointer_look.x * 0.003;
    orbit.pitch = (orbit.pitch + (actions.movement.y + actions.look.y) * delta
        - actions.pointer_look.y * 0.003)
        .clamp(-1.4, 1.4);
    orbit.distance = (orbit.distance * (actions.movement.z * delta - actions.scroll * 0.1).exp())
        .clamp(orbit.radius * 0.1, orbit.radius * 20.);
    if actions.reset {
        orbit.yaw = 2.5;
        orbit.pitch = 0.3;
        orbit.distance = orbit.radius * 3.;
    }
    for mut transform in &mut cameras {
        *transform = orbit.transform();
    }
}

fn capture(
    mut commands: Commands,
    options: Res<Options>,
    loading: Res<Loading>,
    mut state: ResMut<Capture>,
    mut exit: MessageWriter<AppExit>,
) {
    if !matches!(loading.phase, Phase::Ready(_)) {
        return;
    }
    let Some(path) = options.capture.clone() else {
        return;
    };
    if state.started.elapsed() > Duration::from_secs(120) {
        error!("GPU capture timed out");
        exit.write(AppExit::error());
        return;
    }
    if options.native_save_after_ready
        && let Phase::Ready(queue) = &loading.phase
        && queue
            .cell
            .as_ref()
            .and_then(|cell| cell.native.as_ref())
            .is_some_and(|host| !host.published())
    {
        return;
    }
    state.frame += 1;
    // Asset extraction takes several frames. Synchronous pipeline compilation
    // above ensures a queued shader does not turn the smoke capture into a blank.
    if state.frame != 64 {
        return;
    }
    let screenshot = state
        .target
        .clone()
        .map(Screenshot::image)
        .unwrap_or_else(Screenshot::primary_window);
    let report_path = options.report.clone();
    commands.spawn(screenshot).observe(
        move |event: On<ScreenshotCaptured>,
              mut exit: MessageWriter<AppExit>,
              mut fixture: Option<ResMut<fixture::Report>>| {
            let mut save = || -> model::Result<()> {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                let image = event.image.clone().try_into_dynamic()?.to_rgb8();
                image.write_to(&mut file, image::ImageFormat::Png)?;
                file.sync_all()?;
                if let Some(report) = fixture.as_mut() {
                    let verification = report.verify(&image);
                    // Keep failed measurements too, so a shader failure is reviewable.
                    let mut report_file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(report_path.as_ref().expect("fixture requires report"))?;
                    serde_json::to_writer_pretty(&mut report_file, &**report)?;
                    report_file.write_all(b"\n")?;
                    report_file.sync_all()?;
                    verification?;
                }
                Ok(())
            };
            match save() {
                Ok(()) => {
                    info!("Captured {}", path.display());
                    exit.write(AppExit::Success);
                }
                Err(error) => {
                    error!("Capture failed: {error}");
                    exit.write(AppExit::error());
                }
            }
        },
    );
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(AppExit::Success) => std::process::ExitCode::SUCCESS,
        Ok(_) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn explicit_model_source_directory_remains_read_only_for_outputs() {
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/preview-source-output-test");
        let inputs = root.join("inputs");
        std::fs::create_dir_all(&inputs).unwrap();
        let source = inputs.join("authored.packet");
        let error = output_path(&inputs.join("forbidden.png"), None, Some(&source)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("outside the explicit model source directory")
        );
        assert!(output_path(&root.join("permitted.png"), None, Some(&source)).is_ok());
    }

    #[test]
    fn pose_request_requires_exact_controller_time_and_one_model_source() {
        let base = [
            "fallout-preview",
            "--install",
            "authored-empty-install",
            "--model-file",
            "authored.packet",
            "--pose-object",
            "1",
        ];
        assert!(Options::try_parse_from(base).is_err());
        let mut arguments = base.to_vec();
        arguments.extend(["--pose-controller", "2", "--pose-time", "-1.5"]);
        let parsed = Options::try_parse_from(&arguments).unwrap();
        assert_eq!(parsed.pose_time, Some(-1.5));
        assert!(parsed.model.is_none() && parsed.model_file.is_some());
        arguments.extend(["--skin-geometry", "5", "--skin-weight-tolerance", "0"]);
        assert!(Options::try_parse_from(&arguments).is_err());
        assert!(
            Options::try_parse_from([
                "fallout-preview",
                "--install",
                "authored-empty-install",
                "--cell",
                "TestCell",
                "--load-order",
                "order.json",
                "--pose-object",
                "1",
                "--pose-controller",
                "2",
                "--pose-time",
                "0",
            ])
            .is_err()
        );
    }

    pub(crate) fn loading_app(phase: Phase) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, WindowPlugin::default()))
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<material::InspectionMaterial>>()
            .init_resource::<Assets<Image>>()
            .insert_resource(
                Options::try_parse_from([
                    "fallout-preview",
                    "--model",
                    "fixture.nif",
                    "--install",
                    "missing-fixture-installation",
                ])
                .unwrap(),
            )
            .insert_resource(input::Context::Loading)
            .insert_resource(input::Actions::default())
            .insert_resource(Loading { epoch: 7, phase })
            .insert_resource(Orbit {
                center: Vec3::ZERO,
                radius: 1.,
                yaw: 0.,
                pitch: 0.,
                distance: 3.,
            })
            .insert_resource(Navigation {
                fly: false,
                home: Transform::IDENTITY,
            })
            .insert_resource(Capture {
                target: None,
                frame: 63,
                started: Instant::now(),
            })
            .add_message::<AppExit>()
            .add_systems(Update, (drive_loading, capture).chain());
        app.world_mut().spawn((
            Camera3d::default(),
            Transform::IDENTITY,
            Projection::default(),
        ));
        app
    }

    pub(crate) fn ready_fixture(epoch: u64) -> ReadyScene {
        let (prepared, _) = fixture::prepare().unwrap();
        ReadyScene {
            upload: DrawScene {
                queue: upload::Queue::new(epoch, prepared).unwrap(),
                cell: None,
            },
            orbit: Orbit {
                center: Vec3::ZERO,
                radius: 1.,
                yaw: 0.,
                pitch: 0.,
                distance: 3.,
            },
            navigation: Navigation {
                fly: true,
                home: Transform::from_xyz(100., 20., 30.),
            },
            fixture: None,
        }
    }

    #[test]
    fn native_window_event_precedes_source_preparation_and_loading_close_cancels() {
        let mut app = loading_app(Phase::WaitingForWindow);
        app.update();
        assert!(matches!(
            app.world().resource::<Loading>().phase,
            Phase::WaitingForWindow
        ));
        assert_eq!(app.world().resource::<Capture>().frame, 63);
        let mut windows = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        let window = windows.single(app.world()).unwrap();
        app.world_mut().write_message(WindowCreated { window });
        app.update();
        assert!(matches!(
            app.world().resource::<Loading>().phase,
            Phase::Preparing(_)
        ));
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        app.update();
        assert!(matches!(
            app.world().resource::<Loading>().phase,
            Phase::Draining(_)
        ));
        assert_eq!(app.world().resource::<Loading>().epoch, 8);
        assert_eq!(
            *app.world().resource::<input::Context>(),
            input::Context::Suspended
        );
        assert_eq!(app.world().resource::<Capture>().frame, 63);
    }

    #[test]
    fn actual_host_poll_remains_pending_and_capture_waits_for_complete_upload() {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let job = loading::Job::start(7, move |context| {
            context.stage("Gated source fixture")?;
            entered.send(()).unwrap();
            gate.recv().unwrap();
            context.check()?;
            Ok(ready_fixture(7))
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut app = loading_app(Phase::Preparing(job));
        for _ in 0..3 {
            app.update();
            assert!(matches!(
                app.world().resource::<Loading>().phase,
                Phase::Preparing(_)
            ));
            assert_eq!(app.world().resource::<Capture>().frame, 63);
            assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        }
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !matches!(app.world().resource::<Loading>().phase, Phase::Ready(_)) {
            assert!(
                Instant::now() < deadline,
                "controlled worker did not complete"
            );
            app.update();
            std::thread::yield_now();
        }
        assert_eq!(app.world().resource::<Capture>().frame, 0);
        assert_eq!(
            *app.world().resource::<input::Context>(),
            input::Context::Fly
        );
        let mut cameras = app
            .world_mut()
            .query_filtered::<&Transform, With<Camera3d>>();
        assert_eq!(
            cameras.single(app.world()).unwrap().translation,
            Vec3::new(100., 20., 30.)
        );
    }

    #[test]
    fn actual_source_failure_is_visible_and_never_becomes_capture_ready() {
        let mut job =
            loading::Job::<ReadyScene>::start(7, |_| Err("Selected source is unavailable".into()))
                .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match job.poll(7) {
                loading::Poll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                loading::Poll::Failed(_) => break,
                _ => panic!("fixture must fail"),
            }
        }
        let mut app = loading_app(Phase::Preparing(job));
        app.update();
        assert!(matches!(
            app.world().resource::<Loading>().phase,
            Phase::Failed(_)
        ));
        let mut windows = app
            .world_mut()
            .query_filtered::<&Window, With<PrimaryWindow>>();
        assert!(
            windows
                .single(app.world())
                .unwrap()
                .title
                .contains("Selected source is unavailable")
        );
        assert_eq!(app.world().resource::<Capture>().frame, 63);
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    }

    #[test]
    fn camera_consumer_retains_analog_speed_and_does_not_apply_old_mode_actions() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(50));
        app.insert_resource(time)
            .insert_resource(
                Options::try_parse_from([
                    "fallout-preview",
                    "--model",
                    "fixture.nif",
                    "--install",
                    ".",
                ])
                .unwrap(),
            )
            .insert_resource(Orbit {
                center: Vec3::ZERO,
                radius: 1.,
                yaw: 0.,
                pitch: 0.,
                distance: 3.,
            })
            .insert_resource(Navigation {
                fly: true,
                home: Transform::IDENTITY,
            })
            .insert_resource(input::Context::Fly)
            .insert_resource(input::Actions {
                movement: Vec3::Y * 0.5,
                ..default()
            })
            .add_message::<AppExit>()
            .add_systems(Update, controls);
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), Transform::IDENTITY))
            .id();
        app.update();
        let position = app.world().get::<Transform>(camera).unwrap().translation;
        // Half stick deflection, 180 source units/second, 50 ms: 4.5 units.
        assert!((position - Vec3::new(0., 0., -4.5)).length() < 1e-5);
        *app.world_mut().resource_mut::<input::Actions>() = input::Actions {
            movement: Vec3::Y,
            toggle: true,
            ..default()
        };
        app.update();
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            position
        );
        assert_eq!(
            *app.world().resource::<input::Context>(),
            input::Context::Orbit
        );
        assert!(!app.world().resource::<Navigation>().fly);
    }
}
