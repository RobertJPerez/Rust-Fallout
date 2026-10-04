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
    ecs::system::SystemParam,
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
use fallout_data::{assets::ArchiveAssets, baseline, vfs::AssetPath};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Parser, Resource, Clone)]
#[command(about = "Inspect New Vegas models, placed interiors or authored terrain")]
#[command(group(ArgGroup::new("mode").required(true).args(["model", "model_file", "cell", "terrain", "material_fixture", "menu", "menu_dependencies", "menu_rectangles", "menu_image", "menu_font"])))]
#[command(group(ArgGroup::new("model_source").args(["model", "model_file"])))]
struct Options {
    #[arg(skip)]
    native_shutdown: native::Shutdown,
    #[arg(skip)]
    rectangle_request: Option<Arc<ui::rectangles::Request>>,
    #[arg(skip)]
    image_request: Option<Arc<ui::images::Request>>,
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
    /// Exact source-span/member/hash bindings for an opt-in menu include closure.
    #[arg(long, requires = "menu")]
    menu_includes: Option<PathBuf>,
    /// Exact selected source value and explicit custom entity environment.
    #[arg(long, requires = "menu", conflicts_with_all = ["menu_includes", "menu_tile"])]
    menu_entities: Option<PathBuf>,
    /// Exact selected tile and explicit literal conversion policies.
    #[arg(long, requires = "menu", conflicts_with_all = ["menu_includes", "menu_entities", "menu_tile"])]
    menu_traits: Option<PathBuf>,
    /// Explicit source-qualified UI dependency session and opaque input changes.
    #[arg(long, requires_all = ["install", "report"], conflicts_with_all = ["capture", "headless"])]
    menu_dependencies: Option<PathBuf>,
    /// Exact literal rectangle subtree under a mandatory caller inspection policy.
    #[arg(long, requires_all = ["install", "report"], conflicts_with_all = ["camera_position", "camera_look_at"])]
    menu_rectangles: Option<PathBuf>,
    /// Exact source image tile and DDS identities with explicit UV/sampling policy.
    #[arg(long, requires_all = ["install", "report"], conflicts_with_all = ["camera_position", "camera_look_at"])]
    menu_image: Option<PathBuf>,
    /// Exact text-font trait, caller-selected profile entry and retained source dependencies.
    #[arg(long, requires_all = ["install", "report"], conflicts_with_all = ["capture", "headless", "camera_position", "camera_look_at"])]
    menu_font: Option<PathBuf>,
    /// Display exactly this source skin geometry in its stored local pose.
    #[arg(long, requires_all = ["model_source", "skin_weight_tolerance"], conflicts_with = "pose_object")]
    skin_geometry: Option<u32>,
    /// Validate raw unit weight sums; never repair or normalize weights.
    #[arg(long, requires = "skin_geometry")]
    skin_weight_tolerance: Option<f64>,
    /// One validated source-linked sample driving the selected skin.
    #[arg(long, requires_all = ["skin_geometry", "skin_sample_controller", "skin_sample_time", "skin_sample_source_sha256", "skin_sample_controller_policy"])]
    skin_sample_object: Option<u32>,
    #[arg(long, requires = "skin_sample_object")]
    skin_sample_controller: Option<u32>,
    /// Direct source-key time; no host clock or original repeat policy.
    #[arg(long, requires = "skin_sample_object", allow_hyphen_values = true)]
    skin_sample_time: Option<f64>,
    #[arg(long, requires = "skin_sample_object", value_parser = parse_source_sha256)]
    skin_sample_source_sha256: Option<[u8; 32]>,
    #[arg(long, requires = "skin_sample_object", value_enum)]
    skin_sample_controller_policy: Option<SkinControllerPolicy>,
    /// Evaluate this exact source object at the caller's explicit source time.
    #[arg(long, requires_all = ["model_source", "pose_controller", "pose_time"], conflicts_with = "skin_geometry")]
    pose_object: Option<u32>,
    #[arg(long, requires = "pose_object")]
    pose_controller: Option<u32>,
    /// Direct source key time; authored clock flags/frequency remain unapplied.
    #[arg(long, requires = "pose_object", allow_hyphen_values = true)]
    pose_time: Option<f64>,
    /// Explicit inspection times: R/Y steps; headless capture applies the finite list.
    #[arg(
        long,
        requires = "pose_object",
        value_delimiter = ',',
        allow_hyphen_values = true
    )]
    pose_times: Vec<f64>,
    /// Fresh final-time receipt written with the successful pose capture.
    #[arg(long, requires_all = ["pose_times", "capture"])]
    pose_receipt: Option<PathBuf>,
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

#[derive(Clone, Copy, clap::ValueEnum)]
enum SkinControllerPolicy {
    RefuseOtherRequired,
}

fn parse_source_sha256(value: &str) -> std::result::Result<[u8; 32], String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("source SHA256 requires exactly 64 hexadecimal ASCII digits".into());
    }
    let mut result = [0; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid source SHA256 digit")?;
    }
    Ok(result)
}

fn selected_pose(options: &Options) -> Option<pose::Request> {
    options
        .skin_geometry
        .map(|geometry| {
            let skin = pose::SkinRequest {
                geometry,
                absolute_weight_tolerance: options
                    .skin_weight_tolerance
                    .expect("clap requires tolerance"),
            };
            match options.skin_sample_object {
                Some(object) => {
                    let policy = match options
                        .skin_sample_controller_policy
                        .expect("clap requires controller policy")
                    {
                        SkinControllerPolicy::RefuseOtherRequired => {
                            fallout_data::nif_skin::pose::ControllerPolicy::RefuseOtherRequired
                        }
                    };
                    pose::Request::SampledSkin(pose::SampledSkinRequest {
                        skin,
                        expected_source_sha256: options
                            .skin_sample_source_sha256
                            .expect("clap requires source SHA256"),
                        animation: pose::ObjectRequest {
                            object,
                            controller: options
                                .skin_sample_controller
                                .expect("clap requires sampled controller"),
                            source_time: options
                                .skin_sample_time
                                .expect("clap requires sampled source time"),
                        },
                        controller_policy: policy,
                    })
                }
                None => pose::Request::Skin(skin),
            }
        })
        .or_else(|| {
            options.pose_object.map(|object| {
                pose::Request::Object(pose::ObjectRequest {
                    object,
                    controller: options.pose_controller.expect("clap requires controller"),
                    source_time: options.pose_time.expect("clap requires source time"),
                })
            })
        })
}

impl Options {
    fn tile_viewport(&self) -> Option<&ui::rectangles::Viewport> {
        self.rectangle_request
            .as_ref()
            .map(|r| &r.viewport)
            .or_else(|| self.image_request.as_ref().map(|r| &r.viewport))
    }
    fn tile_camera(&self) -> Option<(Transform, Projection)> {
        self.rectangle_request
            .as_ref()
            .map(|r| ui::rectangles::camera(r))
            .or_else(|| self.image_request.as_ref().map(|r| ui::images::camera(r)))
    }
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
    projection: Option<Projection>,
    pose: Option<LivePose>,
}

#[derive(Resource)]
struct LivePose {
    source: Arc<model::ObjectSource>,
    epoch: u64,
    next: usize,
    sequence: u64,
    expected_time: u64,
    pending: Option<loading::Job<model::ObjectFrame>>,
    receipt: Option<model::ObjectReceipt>,
    settled: usize,
    error: Option<String>,
}

fn read_object_source(options: &Options) -> model::Result<Vec<u8>> {
    if let Some(path) = &options.model_file {
        let mut bytes = Vec::new();
        baseline::open_source(path)?
            .take(model::OBJECT_SOURCE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > model::OBJECT_SOURCE_BYTES {
            return Err("Live object source exceeds 4 MiB".into());
        }
        Ok(bytes)
    } else {
        let assets =
            ArchiveAssets::open_nv(options.install.as_deref().ok_or("Missing installation")?)?;
        let (_, bytes) = assets.read_unique_bounded(
            &AssetPath::new(
                options
                    .model
                    .as_deref()
                    .ok_or("Missing model source")?
                    .as_bytes(),
            )?,
            model::OBJECT_SOURCE_BYTES as u64,
        )?;
        Ok(bytes)
    }
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
    for path in [
        options.report.as_ref(),
        options.capture.as_ref(),
        options.pose_receipt.as_ref(),
    ]
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

fn validate_pose_times(options: &Options) -> model::Result<()> {
    if options.pose_times.len() > 32 || options.pose_times.iter().any(|time| !time.is_finite()) {
        return Err("Explicit live pose times require 1..32 finite requested values".into());
    }
    if !options.pose_times.is_empty() && options.capture.is_some() && options.pose_receipt.is_none()
    {
        return Err("Live pose capture requires a fresh --pose-receipt path".into());
    }
    Ok(())
}

fn run() -> model::Result<AppExit> {
    let mut options = Options::parse();
    validate_pose_times(&options)?;
    if let Some(path) = &options.pose_receipt {
        options.pose_receipt = Some(output_path(
            path,
            options.install.as_deref(),
            options.model_file.as_deref(),
        )?);
    }
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
    if options.pose_receipt.is_some()
        && (options.pose_receipt == options.report || options.pose_receipt == options.capture)
    {
        return Err("pose receipt, capture and report require different fresh paths".into());
    }
    if let Some(path) = &options.menu_font {
        let limits = ui::fonts::Limits::default();
        let request = ui::fonts::read_request(path, limits)?;
        let mut report_path = options.report.clone().expect("font requires report");
        for root in [&request.ini.documents, &request.ini.local_appdata] {
            report_path = output_path(&report_path, Some(root), None)?;
        }
        let (report, lease) = ui::fonts::bind(
            options
                .install
                .as_deref()
                .expect("font requires installation"),
            &request,
            limits,
        )?;
        let mut bytes = Vec::new();
        ui::fonts::write_report(&mut bytes, &report, limits.literal.output_bytes)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(report_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        if let Some(lease) = &lease {
            eprintln!(
                "Menu font slot {}: {} retained bytes, {} texture dependencies, {} pinned profile sources; codec/layout unavailable",
                lease.text.slot,
                lease.font.bytes.len()
                    + lease.textures.iter().map(|p| p.bytes.len()).sum::<usize>(),
                lease.textures.len(),
                lease.profile.sources.len()
            );
        } else {
            eprintln!("Menu font dependencies unavailable; no font payload lease or substitute");
        }
        return Ok(AppExit::Success);
    }
    if let Some(path) = &options.menu_rectangles {
        options.rectangle_request = Some(Arc::new(ui::rectangles::read_request(
            path,
            ui::rectangles::Limits::default(),
        )?));
    }
    if let Some(path) = &options.menu_image {
        options.image_request = Some(Arc::new(ui::images::read_request(
            path,
            ui::images::Limits::default(),
        )?));
    }
    if let Some(path) = &options.menu_dependencies {
        let limits = ui::dependencies::Limits::default();
        let request = ui::dependencies::read_request(path, limits)?;
        let report = ui::dependencies::inspect(
            options
                .install
                .as_deref()
                .expect("menu dependencies require installation"),
            &request,
            limits,
        )?;
        let file = OpenOptions::new().write(true).create_new(true).open(
            options
                .report
                .as_ref()
                .expect("menu dependencies require report"),
        )?;
        ui::dependencies::write_report(file, &report, limits.output_bytes)?;
        eprintln!(
            "Menu dependency session: {} nodes, {} edges, revision {}; values remain unevaluated",
            report.graph.graph_nodes, report.graph.unique_edges, report.final_revision
        );
        return Ok(AppExit::Success);
    }
    if let Some(menu) = &options.menu {
        if let Some(request) = &options.menu_traits {
            let limits = ui::traits::Limits::default();
            let request = ui::traits::read_request(request, limits)?;
            let report = ui::traits::inspect(
                options
                    .install
                    .as_deref()
                    .expect("menu requires installation"),
                &AssetPath::new(menu.as_bytes())?,
                &request,
                limits,
            )?;
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(options.report.as_ref().expect("menu requires report"))?;
            ui::traits::write_report(file, &report, limits.output_bytes)?;
            eprintln!(
                "Menu literal projection: {} rows; original tile display remains unavailable",
                report.projection.rows.len()
            );
            return Ok(AppExit::Success);
        }
        if let Some(request) = &options.menu_entities {
            let limits = ui::entities::Limits::default();
            let request = ui::entities::read_request(request, limits)?;
            let report = ui::entities::inspect(
                options
                    .install
                    .as_deref()
                    .expect("menu requires installation"),
                &AssetPath::new(menu.as_bytes())?,
                &request,
                limits,
            )?;
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(options.report.as_ref().expect("menu requires report"))?;
            ui::entities::write_report(file, &report, limits.output_bytes)?;
            eprintln!(
                "Menu selected value {}; {} unsupplied entities; tile display remains unavailable",
                if report.resolution.value.is_some() {
                    "resolved"
                } else {
                    "unavailable"
                },
                report.resolution.unresolved.len()
            );
            return Ok(AppExit::Success);
        }
        if let Some(request) = &options.menu_includes {
            let limits = ui::includes::Limits::default();
            let request = ui::includes::read_request(request, limits)?;
            let report = ui::includes::inspect(
                options
                    .install
                    .as_deref()
                    .expect("menu requires installation"),
                &AssetPath::new(menu.as_bytes())?,
                options.menu_tile.as_deref(),
                request,
                limits,
            )?;
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(options.report.as_ref().expect("menu requires report"))?;
            ui::includes::write_report(file, &report, limits.output_bytes)?;
            eprintln!(
                "Menu include sources retained: {} files, {} exact edges; tile evaluation/display remains unavailable",
                report.files.len(),
                report.edges.len()
            );
            return Ok(AppExit::Success);
        }
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
        } else if let Some((camera, _)) = options.tile_camera() {
            camera
        } else {
            orbit.transform()
        },
    };
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: (!headless).then(|| Window {
                title: "Fallout Rust - Loading source data".into(),
                resolution: options
                    .tile_viewport()
                    .map_or((1280, 900), |r| (r.width, r.height))
                    .into(),
                resizable: options.tile_viewport().is_none(),
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
    let background = options
        .tile_viewport()
        .map_or(Color::srgb(0.035, 0.045, 0.055), |r| {
            let [red, green, blue, alpha] = r.background;
            Color::srgba(red, green, blue, alpha)
        });
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
        .insert_resource(ClearColor(background))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (controls, drive_loading, drive_pose, capture).chain(),
        );
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
    if let Some(request) = &options.image_request {
        let limits = ui::images::Limits::default();
        let (prepared, report, view) = ui::images::load(
            options
                .install
                .as_deref()
                .expect("image requires installation"),
            request,
            limits,
            context,
            epoch,
        )?;
        context.stage("Preparing bounded source image upload")?;
        let orbit = Orbit {
            center: prepared.center,
            radius: prepared.radius,
            yaw: 0.,
            pitch: 0.,
            distance: prepared.radius * 3.,
        };
        let (home, projection) = ui::images::camera(request);
        let queue = upload::Queue::new_image(epoch, prepared, view)?;
        write_tile_report(
            options.report.as_deref().expect("image requires report"),
            context,
            |writer| ui::images::write_report(writer, &report, limits.literal.output_bytes),
        )?;
        eprintln!(
            "Exact source image: {}x{} DDS, {} retained bytes; explicit UV/sampling policy",
            report.texture.width, report.texture.height, report.texture.retained_bytes
        );
        return Ok(ReadyScene {
            upload: DrawScene { queue, cell: None },
            orbit,
            navigation: Navigation { fly: false, home },
            fixture: None,
            projection: Some(projection),
            pose: None,
        });
    }
    if let Some(request) = &options.rectangle_request {
        let limits = ui::rectangles::Limits::default();
        let (prepared, report, views) = ui::rectangles::load(
            options
                .install
                .as_deref()
                .expect("rectangles require installation"),
            request,
            limits,
            context,
            epoch,
        )?;
        context.stage("Preparing bounded source rectangle uploads")?;
        let orbit = Orbit {
            center: prepared.center,
            radius: prepared.radius,
            yaw: 0.,
            pitch: 0.,
            distance: prepared.radius * 3.,
        };
        let (home, projection) = ui::rectangles::camera(request);
        let queue = upload::Queue::new_tiles(epoch, prepared, views)?;
        context.check()?;
        write_tile_report(
            options
                .report
                .as_deref()
                .expect("rectangles require report"),
            context,
            |writer| ui::rectangles::write_report(writer, &report, limits.output_bytes),
        )?;
        eprintln!(
            "{} exact literal source rectangles; fixed caller inspection viewport",
            report.plan.rectangles.len()
        );
        return Ok(ReadyScene {
            upload: DrawScene { queue, cell: None },
            orbit,
            navigation: Navigation { fly: false, home },
            fixture: None,
            projection: Some(projection),
            pose: None,
        });
    }
    let pose = selected_pose(options);
    let mut cell_sources = None;
    let (mut prepared, report) = if options.material_fixture {
        let (prepared, report) = fixture::prepare()?;
        (prepared, scene::Report::Fixture(report))
    } else if !options.pose_times.is_empty() {
        // Bound the opted-in source before its initial decode and mesh copy.
        let bytes = read_object_source(options)?;
        let label = if let Some(path) = &options.model_file {
            let filename = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Explicit model source requires a Unicode filename")?;
            AssetPath::new(format!("local-source/{filename}").as_bytes())?
        } else {
            AssetPath::new(
                options
                    .model
                    .as_deref()
                    .ok_or("Missing source model")?
                    .as_bytes(),
            )?
        };
        let assets =
            ArchiveAssets::open_nv(options.install.as_deref().ok_or("Missing installation")?)?;
        let mut textures = model::Textures::default();
        let (model, mut report) =
            model::from_bytes_with_pose(&assets, &label, &bytes, &mut textures, pose)?;
        if let Some(path) = &options.model_file {
            report.schema_version = 3;
            report.source_file = Some(Box::new(path.clone()));
        }
        let prepared = scene::Prepared {
            center: model.center,
            radius: model.radius,
            origin: [0.; 3],
            models: vec![model],
            images: textures.images,
            instances: vec![scene::Instance {
                model: 0,
                transform: Transform::IDENTITY,
                key: None,
                visibility: Visibility::Inherited,
                canonical: None,
            }],
        };
        (prepared, scene::Report::Model(report))
    } else if let Some(name) = &options.model {
        scene::load_model(
            options.install.as_deref().expect("clap requires install"),
            &AssetPath::new(name.as_bytes())?,
            pose,
        )?
    } else if let Some(path) = &options.model_file {
        let (prepared, mut report) = scene::load_model_file(
            options.install.as_deref().expect("clap requires install"),
            path,
            pose,
        )?;
        // The existing file loader tags its file metadata as schema3. The
        // opt-in sampled receipt requires schema4 on both source entry routes.
        if let scene::Report::Model(model) = &mut report
            && model.source_sampled_skin_pose.is_some()
        {
            model.schema_version = 4;
        }
        (prepared, report)
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
    let live_pose = if options.pose_times.is_empty() {
        None
    } else {
        context.stage("Sealing retained source object for explicit live times")?;
        let scene::Report::Model(report) = &report else {
            return Err("Live object updates require one selected source model".into());
        };
        let Some(pose::Request::Object(request)) = selected_pose(options) else {
            return Err("Live object updates require the exact selected object request".into());
        };
        if prepared.models.len() != 1 || prepared.instances.len() != 1 {
            return Err("Live object updates require one source model/instance".into());
        }
        let bytes = read_object_source(options)?;
        let source = model::ObjectSource::new(
            &bytes,
            &String::from_utf8_lossy(report.model.bytes()),
            request,
            report,
            &mut prepared.models[0],
        )?;
        Some(LivePose {
            source: Arc::new(source),
            epoch,
            next: 0,
            sequence: 0,
            expected_time: request.source_time.to_bits(),
            pending: None,
            receipt: None,
            settled: 0,
            error: None,
        })
    };
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
            queue: if let Some(live) = &live_pose {
                upload::Queue::new_object(epoch, prepared, live.source.binding.clone())?
            } else {
                upload::Queue::new(epoch, prepared)?
            },
            cell: cell_sources,
        },
        orbit,
        navigation,
        projection: None,
        pose: live_pose,
        fixture: if let scene::Report::Fixture(report) = report {
            Some(report)
        } else {
            None
        },
    })
}

fn write_tile_report(
    path: &Path,
    context: &loading::Context,
    write: impl FnOnce(&mut Vec<u8>) -> model::Result<()>,
) -> model::Result<()> {
    context.check()?;
    // Stream into bounded memory first. Semantic output admission finishes
    // off-thread before create_new; an error can leave a bounded memory prefix.
    let mut bytes = Vec::new();
    write(&mut bytes)?;
    context.check()?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    context.check()?;
    Ok(())
}

#[derive(SystemParam)]
struct LoadingOptions<'w> {
    options: Res<'w, Options>,
    live: Option<Res<'w, LivePose>>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "Loading coordinates separate host resource owners"
)]
fn drive_loading(
    mut commands: Commands,
    request: LoadingOptions,
    actions: Res<input::Actions>,
    mut state: ResMut<Loading>,
    mut context: ResMut<input::Context>,
    mut display: ResMut<input::NativeDisplay>,
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
    let options = &*request.options;
    let live = &request.live;
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
        display.0 = None;
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
            Phase::WaitingForWindow => {
                display.0 = None;
                Phase::Cancelled
            }
            Phase::Preparing(mut job) => {
                display.0 = None;
                job.cancel();
                state.epoch = state.epoch.checked_add(1).unwrap_or(state.epoch);
                Phase::Draining(job)
            }
            Phase::Uploading(queue) => {
                display.0 = None;
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
            match start_preparation(options, epoch) {
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
            if actions.retry_loading
                && !actions.cancel_loading
                && !options.headless
                && live.as_ref().is_none_or(|live| live.pending.is_none()) =>
        {
            let request = retry_outputs(options).and_then(|()| {
                let next = state
                    .epoch
                    .checked_add(1)
                    .ok_or("Source retry epoch exhausted")?;
                state.epoch = next;
                start_preparation(options, next)
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
                        if let Some(live) = ready.pose {
                            commands.insert_resource(live);
                        } else {
                            commands.remove_resource::<LivePose>();
                        }
                        *orbit = ready.orbit;
                        *navigation = ready.navigation;
                        for (mut transform, mut projection) in &mut cameras {
                            *transform = navigation.home;
                            if let Some(prepared) = &ready.projection {
                                *projection = prepared.clone();
                            } else if let Projection::Perspective(perspective) = &mut *projection {
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
                display.0 = queue
                    .cell
                    .as_ref()
                    .and_then(|cell| cell.native.as_ref())
                    .map(|host| input::DisplayIdentity {
                        scene_epoch: epoch,
                        revision: host.revision(),
                    });
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
                                display.0 = Some(input::DisplayIdentity {
                                    scene_epoch: epoch,
                                    revision: observation.report.revision,
                                });
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
                if let Some(intent) = actions.native {
                    let admitted = *context == intent.context
                        && windows
                            .iter()
                            .any(|(id, window)| id == intent.window && window.focused);
                    host.intent(intent, epoch, admitted);
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
                "Loading: {message} ({}s) â€” Backspace cancels",
                elapsed.as_secs()
            )
        }
        Phase::Draining(job) => format!(
            "Cancelling: waiting for source worker return ({}s) â€” Escape closes",
            job.status().1.as_secs()
        ),
        Phase::Uploading(queue) => format!("{} â€” Backspace cancels", queue.status()),
        Phase::Disposing(queue, _) => queue.queue.disposal_status(),
        Phase::Ready(queue) => queue
            .cell
            .as_ref()
            .and_then(|cell| cell.native.as_ref())
            .map_or_else(|| "Ready".into(), |host| host.title().into()),
        Phase::Failed(error) => format!(
            "Failed: {} â€” Enter retries; Escape closes",
            error.chars().take(180).collect::<String>()
        ),
        Phase::Cancelled => "Cancelled â€” Enter retries; Escape closes".into(),
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
            if let Some((_, projection)) = options.tile_camera() {
                projection
            } else if options.material_fixture {
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
            if options.material_fixture || options.tile_viewport().is_some() {
                Msaa::Off
            } else {
                Msaa::default()
            },
        ))
        .id();
    let target = if options.headless {
        let mut image = Image::new_uninit(
            Extent3d {
                width: options.tile_viewport().map_or(1280, |r| r.width),
                height: options.tile_viewport().map_or(900, |r| r.height),
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
    if options.tile_viewport().is_some() {
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
            if actions.reset && options.pose_times.is_empty() {
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
    if actions.reset && options.pose_times.is_empty() {
        orbit.yaw = 2.5;
        orbit.pitch = 0.3;
        orbit.distance = orbit.radius * 3.;
    }
    for mut transform in &mut cameras {
        *transform = orbit.transform();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "Live pose coordinates existing scene, assets and input owners"
)]
fn drive_pose(
    mut commands: Commands,
    options: Res<Options>,
    actions: Res<input::Actions>,
    context: Res<input::Context>,
    mut loading: ResMut<Loading>,
    live: Option<ResMut<LivePose>>,
    mut assets: upload::Resources,
    mut capture: ResMut<Capture>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut live) = live else {
        return;
    };
    let epoch = loading.epoch;
    if epoch != live.epoch || !matches!(loading.phase, Phase::Ready(_)) {
        if let Some(job) = &mut live.pending
            && matches!(job.retire(), loading::Retirement::Done(_))
        {
            live.pending = None;
        }
        return;
    }
    let mut applied = false;
    if let Some(job) = &mut live.pending {
        match job.poll(epoch) {
            loading::Poll::Pending => {}
            loading::Poll::Ready(frame) => {
                live.pending = None;
                let result =
                    if frame.summary.evaluation.requested_time_f64_bits != live.expected_time {
                        Err("Live pose completion differs from the explicit requested time".into())
                    } else if let Phase::Ready(draw) = &mut loading.phase {
                        draw.queue.update_object(
                            &mut commands,
                            &mut assets,
                            epoch,
                            live.sequence,
                            frame,
                        )
                    } else {
                        Err("Live pose lost its current visible scene".into())
                    };
                match result {
                    Ok(receipt) => {
                        eprintln!(
                            "Live object pose receipt: {}",
                            serde_json::to_string(&receipt).expect("serializable pose receipt")
                        );
                        live.receipt = Some(receipt);
                        live.next += 1;
                        live.settled = 0;
                        live.error = None;
                        capture.frame = 0;
                        capture.started = Instant::now();
                        applied = true;
                    }
                    Err(error) => live.error = Some(error),
                }
            }
            loading::Poll::Failed(error) => {
                live.pending = None;
                live.error = Some(error);
            }
            loading::Poll::Cancelled | loading::Poll::Finished => {
                live.pending = None;
                live.error = Some("Live pose request cancelled; visible geometry preserved".into());
            }
        }
    }
    if live.pending.is_none() && !applied {
        live.settled = live.settled.saturating_add(1);
        let explicit_step =
            actions.reset && matches!(*context, input::Context::Orbit | input::Context::Fly);
        let capture_step = options.headless && live.settled >= 64 && live.error.is_none();
        if live.next < options.pose_times.len() && (explicit_step || capture_step) {
            let time = options.pose_times[live.next];
            if let Some(sequence) = live.sequence.checked_add(1) {
                let source = Arc::clone(&live.source);
                let request = options.clone();
                match loading::Job::start(epoch, move |context| {
                    context.stage("Checking current pose source")?;
                    let bytes = read_object_source(&request).map_err(|error| error.to_string())?;
                    context.stage("Evaluating explicit selected object time")?;
                    let frame = source
                        .frame(&bytes, time, sequence)
                        .map_err(|error| error.to_string())?;
                    context.check()?;
                    Ok(frame)
                }) {
                    Ok(job) => {
                        live.sequence = sequence;
                        live.expected_time = time.to_bits();
                        live.pending = Some(job);
                        live.error = None;
                    }
                    Err(error) => live.error = Some(error.to_string()),
                }
            } else {
                live.error = Some("Live pose request sequence exhausted".into());
            }
        }
    }
    if let Some(error) = &live.error
        && options.headless
    {
        error!("Live object update failed; visible pose preserved: {error}");
        exit.write(AppExit::error());
    }
    let status = if let Some(error) = &live.error {
        format!(
            "Pose update failed: {} - previous pose retained",
            error.chars().take(160).collect::<String>()
        )
    } else if let Some(job) = &live.pending {
        format!(
            "Pose pending: {} - camera remains available",
            job.status().0
        )
    } else if let Some(receipt) = &live.receipt {
        format!(
            "Ready - source pose time {} - R/Y requests next explicit time",
            f64::from_bits(receipt.pose.evaluation.requested_time_f64_bits)
        )
    } else {
        "Ready - R/Y requests next explicit pose time".into()
    };
    for mut window in &mut windows {
        window.title = format!("Fallout Rust - {status}");
    }
}

fn pose_capture_receipt(bytes: &[u8], path: &Path, dimensions: [u32; 2]) -> model::Result<Vec<u8>> {
    const MAX_PNG: usize = 8 * 1024 * 1024;
    const MAX_JSON: usize = 1024 * 1024;
    if bytes.len().saturating_add(1) > MAX_JSON {
        return Err("Live pose capture receipt exceeds 1 MiB".into());
    }
    let mut file = std::fs::File::open(path)?.take(MAX_PNG as u64 + 1);
    let mut header = [0u8; 24];
    file.read_exact(&mut header)?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n"
        || &header[12..16] != b"IHDR"
        || u32::from_be_bytes(header[16..20].try_into()?) != dimensions[0]
        || u32::from_be_bytes(header[20..24].try_into()?) != dimensions[1]
    {
        return Err("Live pose capture PNG dimensions/header differ".into());
    }
    let mut hasher = Sha256::new();
    hasher.update(header);
    let mut total = header.len();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count)
            .ok_or("Capture PNG byte overflow")?;
        if total > MAX_PNG {
            return Err("Live pose capture PNG exceeds 8 MiB".into());
        }
        hasher.update(&buffer[..count]);
    }
    let mut receipt: serde_json::Value = serde_json::from_slice(bytes)?;
    receipt
        .as_object_mut()
        .ok_or("Live pose capture receipt must be an object")?
        .insert(
            "capture".into(),
            serde_json::json!({
                "path": path, "sha256": format!("{:x}", hasher.finalize()),
                "encoded_bytes": total, "width": dimensions[0], "height": dimensions[1],
            }),
        );
    let encoded = serde_json::to_vec_pretty(&receipt)?;
    if encoded.len().saturating_add(1) > MAX_JSON {
        return Err("Live pose capture receipt exceeds 1 MiB".into());
    }
    Ok(encoded)
}

fn capture(
    mut commands: Commands,
    options: Res<Options>,
    loading: Res<Loading>,
    mut state: ResMut<Capture>,
    mut exit: MessageWriter<AppExit>,
    live: Option<Res<LivePose>>,
) {
    if !matches!(loading.phase, Phase::Ready(_)) {
        return;
    }
    if !options.pose_times.is_empty()
        && live.as_ref().is_none_or(|live| {
            live.epoch != loading.epoch
                || live.pending.is_some()
                || live.error.is_some()
                || live.next != options.pose_times.len()
                || live.receipt.is_none()
        })
    {
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
    let pose_receipt = options.pose_receipt.clone();
    let pose_identity = live
        .as_ref()
        .and_then(|live| live.receipt.as_ref())
        .map(|receipt| {
            (
                receipt.scene_epoch,
                receipt.request_sequence,
                receipt.pose.evaluation.requested_time_f64_bits,
            )
        });
    let pose_bytes = live
        .as_ref()
        .and_then(|live| live.receipt.as_ref())
        .map(|receipt| {
            serde_json::to_vec_pretty(receipt).expect("serializable current pose receipt")
        });
    if pose_bytes
        .as_ref()
        .is_some_and(|bytes| bytes.len().saturating_add(1) > 1024 * 1024)
    {
        error!("Live pose capture receipt exceeds 1 MiB");
        exit.write(AppExit::error());
        return;
    }
    commands.spawn(screenshot).observe(
        move |event: On<ScreenshotCaptured>,
              mut exit: MessageWriter<AppExit>,
              mut fixture: Option<ResMut<fixture::Report>>,
              loading: Res<Loading>,
              live: Option<Res<LivePose>>| {
            let mut save = || -> model::Result<()> {
                if let Some((epoch, sequence, time)) = pose_identity {
                    let current = live.as_ref().and_then(|live| live.receipt.as_ref());
                    if loading.epoch != epoch
                        || !matches!(loading.phase, Phase::Ready(_))
                        || live
                            .as_ref()
                            .is_none_or(|live| live.pending.is_some() || live.error.is_some())
                        || current.is_none_or(|receipt| {
                            receipt.request_sequence != sequence
                                || receipt.pose.evaluation.requested_time_f64_bits != time
                        })
                    {
                        return Err("Discarded stale live pose capture readback".into());
                    }
                }
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                let image = event.image.clone().try_into_dynamic()?.to_rgb8();
                image.write_to(&mut file, image::ImageFormat::Png)?;
                file.sync_all()?;
                if let Some(receipt_path) = &pose_receipt {
                    let bytes = pose_bytes
                        .as_ref()
                        .ok_or("Current pose capture receipt unavailable")?;
                    let bytes =
                        pose_capture_receipt(bytes, &path, [image.width(), image.height()])?;
                    let mut receipt_file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(receipt_path)?;
                    receipt_file.write_all(&bytes)?;
                    receipt_file.write_all(b"\n")?;
                    receipt_file.sync_all()?;
                }
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

    #[test]
    fn actual_source_file_preparation_keeps_stored_schema3_and_sampled_schema4() {
        use sha2::Digest;
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/preview-sample-file-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let install = root.join("install");
        std::fs::create_dir_all(install.join("Data")).unwrap();
        let inputs = root.join("inputs");
        std::fs::create_dir_all(&inputs).unwrap();
        let bytes = pose::tests::packet(&pose::tests::sampled_blocks());
        let source = inputs.join("sampled.nif");
        std::fs::write(&source, &bytes).unwrap();
        let hash = format!("{:x}", sha2::Sha256::digest(&bytes));
        for sampled in [false, true] {
            let report = root.join(if sampled {
                "sampled.json"
            } else {
                "stored.json"
            });
            let mut arguments = vec![
                "fallout-preview".to_owned(),
                "--install".into(),
                install.to_string_lossy().into_owned(),
                "--model-file".into(),
                source.to_string_lossy().into_owned(),
                "--skin-geometry".into(),
                "3".into(),
                "--skin-weight-tolerance".into(),
                "0".into(),
                "--report".into(),
                report.to_string_lossy().into_owned(),
            ];
            if sampled {
                arguments.extend(
                    [
                        "--skin-sample-object",
                        "1",
                        "--skin-sample-controller",
                        "7",
                        "--skin-sample-time",
                        "0",
                        "--skin-sample-source-sha256",
                        &hash,
                        "--skin-sample-controller-policy",
                        "refuse-other-required",
                    ]
                    .into_iter()
                    .map(str::to_owned),
                );
            }
            let options = Options::try_parse_from(arguments).unwrap();
            let mut job = loading::Job::start(7, move |context| {
                prepare_scene(&options, &context, 7).map_err(|e| e.to_string())
            })
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match job.poll(7) {
                    loading::Poll::Ready(_) => break,
                    loading::Poll::Pending => {
                        assert!(Instant::now() < deadline);
                        std::thread::yield_now();
                    }
                    loading::Poll::Failed(e) => {
                        panic!("Actual sampled source-file preparation failed: {e}")
                    }
                    _ => panic!("Actual preparation ended without a result"),
                }
            }
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
            assert_eq!(value["schema_version"], if sampled { 4 } else { 3 });
            assert_eq!(value["source_file"], source.to_string_lossy().as_ref());
            assert_eq!(value.get("source_sampled_skin_pose").is_some(), sampled);
        }
    }

    #[test]
    fn sampled_skin_cli_requires_all_explicit_inputs_and_preserves_exact_time_identity() {
        let hash = "aB".repeat(32);
        let mut complete = vec![
            "fallout-preview",
            "--install",
            "authored-empty-install",
            "--model-file",
            "authored.packet",
            "--skin-geometry",
            "3",
            "--skin-weight-tolerance",
            "0",
            "--skin-sample-object",
            "1",
            "--skin-sample-controller",
            "7",
            "--skin-sample-time",
            "-0",
            "--skin-sample-source-sha256",
            &hash,
            "--skin-sample-controller-policy",
            "refuse-other-required",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        let parsed = Options::try_parse_from(&complete).unwrap();
        let Some(pose::Request::SampledSkin(request)) = selected_pose(&parsed) else {
            panic!("Explicit sampled mode missing")
        };
        assert_eq!(request.expected_source_sha256, [0xab; 32]);
        assert_eq!(request.animation.source_time.to_bits(), (-0f64).to_bits());
        assert_eq!(
            (
                request.skin.geometry,
                request.animation.object,
                request.animation.controller
            ),
            (3, 1, 7)
        );
        for flag in [
            "--skin-geometry",
            "--skin-weight-tolerance",
            "--skin-sample-object",
            "--skin-sample-controller",
            "--skin-sample-time",
            "--skin-sample-source-sha256",
            "--skin-sample-controller-policy",
        ] {
            let mut partial = complete.clone();
            let i = partial.iter().position(|v| v == flag).unwrap();
            partial.drain(i..i + 2);
            assert!(
                Options::try_parse_from(partial).is_err(),
                "Missing{flag} must refuse"
            );
        }
        complete.extend(
            [
                "--pose-object",
                "1",
                "--pose-controller",
                "7",
                "--pose-time",
                "0",
            ]
            .into_iter()
            .map(str::to_owned),
        );
        assert!(Options::try_parse_from(complete).is_err());
        let stored = Options::try_parse_from([
            "fallout-preview",
            "--install",
            ".",
            "--model-file",
            "authored.packet",
            "--skin-geometry",
            "3",
            "--skin-weight-tolerance",
            "0",
        ])
        .unwrap();
        assert!(matches!(
            selected_pose(&stored),
            Some(pose::Request::Skin(_))
        ));
        for invalid in [
            "gg".repeat(32),
            "ff".repeat(31),
            "ff".repeat(33),
            "?".repeat(32),
        ] {
            assert!(parse_source_sha256(&invalid).is_err());
        }
        assert_eq!(parse_source_sha256(&"AB".repeat(32)).unwrap(), [0xab; 32]);
        let mut policy = vec![
            "fallout-preview",
            "--install",
            ".",
            "--model-file",
            "authored.packet",
            "--skin-geometry",
            "3",
            "--skin-weight-tolerance",
            "0",
            "--skin-sample-object",
            "1",
            "--skin-sample-controller",
            "7",
            "--skin-sample-time",
            "0",
            "--skin-sample-source-sha256",
            &hash,
            "--skin-sample-controller-policy",
            "stored-fallback",
        ];
        assert!(Options::try_parse_from(&policy).is_err());
        policy.pop();
        policy.push("refuse-other-required");
        assert!(Options::try_parse_from(&policy).is_ok());
    }
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
            .init_resource::<input::NativeDisplay>()
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
            .add_systems(Update, (drive_loading, drive_pose, capture).chain());
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
            projection: None,
            pose: None,
        }
    }

    fn live_pose_app() -> App {
        let (source, model, bytes) = model::tests::live_object_fixture(false);
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local")
            .join(format!(
                "v3-view-24-unit-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("source.packet");
        std::fs::write(&path, bytes).unwrap();
        let prepared = scene::Prepared {
            center: model.center,
            radius: model.radius,
            origin: [0.; 3],
            models: vec![model],
            images: vec![],
            instances: vec![scene::Instance {
                model: 0,
                transform: Transform::IDENTITY,
                key: None,
                visibility: Visibility::Inherited,
                canonical: None,
            }],
        };
        let queue = upload::Queue::new_object(7, prepared, source.binding.clone()).unwrap();
        let mut app = loading_app(Phase::Uploading(DrawScene { queue, cell: None }));
        app.world_mut().insert_resource(LivePose {
            source: Arc::new(source),
            epoch: 7,
            next: 0,
            sequence: 0,
            expected_time: 0f64.to_bits(),
            pending: None,
            receipt: None,
            settled: 0,
            error: None,
        });
        {
            let mut options = app.world_mut().resource_mut::<Options>();
            options.model = None;
            options.model_file = Some(path);
            options.pose_times = vec![3., 0.];
            options.capture = Some(directory.join("forbidden-pending-capture.png"));
            options.pose_receipt = Some(directory.join("forbidden-pending-receipt.json"));
        }
        for _ in 0..20 {
            app.update();
            if matches!(app.world().resource::<Loading>().phase, Phase::Ready(_)) {
                break;
            }
        }
        assert!(matches!(
            app.world().resource::<Loading>().phase,
            Phase::Ready(_)
        ));
        app.world_mut().resource_mut::<Capture>().frame = 63;
        app
    }

    fn live_positions(app: &mut App) -> Vec<Vec<[f32; 3]>> {
        let mut draws = app.world_mut().query::<&Mesh3d>();
        let assets = app.world().resource::<Assets<Mesh>>();
        draws
            .iter(app.world())
            .map(|draw| {
                let Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) = assets
                    .get(&draw.0)
                    .unwrap()
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("Missing actual draw positions")
                };
                values.clone()
            })
            .collect()
    }

    fn until_pose(app: &mut App, done: impl Fn(&LivePose) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(app.world().resource::<LivePose>()) {
            assert!(
                Instant::now() < deadline,
                "Controlled live pose worker did not complete"
            );
            app.update();
            std::thread::yield_now();
        }
    }

    #[test]
    fn actual_live_host_uses_one_explicit_request_and_blocks_capture_until_final_applied_time() {
        let mut app = live_pose_app();
        *app.world_mut().resource_mut::<input::Context>() = input::Context::Suspended;
        app.world_mut().resource_mut::<input::Actions>().reset = true;
        app.update();
        assert!(app.world().resource::<LivePose>().pending.is_none());
        assert_eq!(app.world().resource::<LivePose>().sequence, 0);
        *app.world_mut().resource_mut::<input::Context>() = input::Context::Orbit;
        app.update();
        assert!(app.world().resource::<LivePose>().pending.is_some());
        assert_eq!(app.world().resource::<LivePose>().sequence, 1);
        assert_eq!(app.world().resource::<LivePose>().next, 0);
        assert_eq!(app.world().resource::<Capture>().frame, 63);
        app.world_mut().resource_mut::<input::Actions>().reset = false;
        until_pose(&mut app, |live| live.receipt.is_some());
        let live = app.world().resource::<LivePose>();
        assert!(live.pending.is_none());
        assert_eq!(live.next, 1);
        assert_eq!(live.sequence, 1);
        assert_eq!(
            live.receipt
                .as_ref()
                .unwrap()
                .pose
                .evaluation
                .requested_time_f64_bits,
            3f64.to_bits()
        );
        assert_eq!(app.world().resource::<Capture>().frame, 0);
        assert_eq!(
            live_positions(&mut app),
            [vec![[-6., 29., 7.], [-6., 29., 11.], [-6., 33., 7.]]]
        );
        let options = app.world().resource::<Options>();
        assert!(!options.capture.as_ref().unwrap().exists());
        assert!(!options.pose_receipt.as_ref().unwrap().exists());
    }

    #[test]
    fn changed_current_source_fails_live_host_without_replacing_its_visible_mesh() {
        let mut app = live_pose_app();
        let original = live_positions(&mut app);
        let path = app
            .world()
            .resource::<Options>()
            .model_file
            .clone()
            .unwrap();
        let mut changed = std::fs::read(&path).unwrap();
        *changed.last_mut().unwrap() ^= 1;
        std::fs::write(path, changed).unwrap();
        *app.world_mut().resource_mut::<input::Context>() = input::Context::Orbit;
        app.world_mut().resource_mut::<input::Actions>().reset = true;
        app.update();
        app.world_mut().resource_mut::<input::Actions>().reset = false;
        until_pose(&mut app, |live| live.error.is_some());
        assert!(
            app.world()
                .resource::<LivePose>()
                .error
                .as_ref()
                .unwrap()
                .contains("source changed")
        );
        assert!(app.world().resource::<LivePose>().receipt.is_none());
        assert_eq!(app.world().resource::<LivePose>().next, 0);
        assert_eq!(live_positions(&mut app), original);
        assert_eq!(app.world().resource::<Capture>().frame, 63);
    }

    #[test]
    fn completed_old_pose_worker_is_drained_after_scene_epoch_changes_and_cannot_publish() {
        let mut app = live_pose_app();
        let original = live_positions(&mut app);
        let source = Arc::clone(&app.world().resource::<LivePose>().source);
        let bytes = read_object_source(app.world().resource::<Options>()).unwrap();
        let (entered, observer) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let job = loading::Job::start(7, move |_| {
            let frame = source
                .frame(&bytes, 3., 1)
                .map_err(|error| error.to_string())?;
            entered.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(frame)
        })
        .unwrap();
        observer.recv_timeout(Duration::from_secs(5)).unwrap();
        {
            let mut live = app.world_mut().resource_mut::<LivePose>();
            live.sequence = 1;
            live.expected_time = 3f64.to_bits();
            live.pending = Some(job);
        }
        *app.world_mut().resource_mut::<input::Context>() = input::Context::Orbit;
        app.world_mut().resource_mut::<input::Actions>().reset = true;
        for _ in 0..10 {
            app.update();
            assert_eq!(app.world().resource::<LivePose>().sequence, 1);
            assert_eq!(app.world().resource::<LivePose>().next, 0);
            assert_eq!(app.world().resource::<Capture>().frame, 63);
            assert_eq!(live_positions(&mut app), original);
        }
        app.world_mut().resource_mut::<input::Actions>().reset = false;
        app.world_mut().resource_mut::<Loading>().epoch = 8;
        app.update();
        assert!(app.world().resource::<LivePose>().pending.is_some());
        assert_eq!(live_positions(&mut app), original);
        release.send(()).unwrap();
        until_pose(&mut app, |live| live.pending.is_none());
        assert!(app.world().resource::<LivePose>().receipt.is_none());
        assert_eq!(app.world().resource::<LivePose>().next, 0);
        assert_eq!(live_positions(&mut app), original);
        assert_eq!(app.world().resource::<Capture>().frame, 63);
    }

    #[test]
    fn explicit_time_count_nonfinite_values_and_current_source_read_caps_are_exact() {
        let mut app = live_pose_app();
        {
            let mut options = app.world_mut().resource_mut::<Options>();
            options.pose_times = vec![-0.; 32];
            assert!(validate_pose_times(&options).is_ok());
            assert_eq!(options.pose_times[0].to_bits(), (-0f64).to_bits());
            options.pose_times.push(0.);
            assert!(validate_pose_times(&options).is_err());
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                options.pose_times = vec![0., value];
                assert!(validate_pose_times(&options).is_err());
            }
            options.pose_times = vec![0.];
            options.pose_receipt = None;
            assert!(validate_pose_times(&options).is_err());
        }
        let options = app.world().resource::<Options>();
        let path = options.model_file.as_ref().unwrap();
        std::fs::write(path, vec![0u8; model::OBJECT_SOURCE_BYTES]).unwrap();
        assert_eq!(
            read_object_source(options).unwrap().len(),
            model::OBJECT_SOURCE_BYTES
        );
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(&[0])
            .unwrap();
        assert!(
            read_object_source(options)
                .err()
                .unwrap()
                .to_string()
                .contains("exceeds 4 MiB")
        );
    }

    #[test]
    fn final_pose_receipt_binds_actual_png_hash_dimensions_and_exact_output_caps() {
        let app = live_pose_app();
        let path = app.world().resource::<Options>().capture.as_ref().unwrap();
        image::RgbImage::from_pixel(3, 2, image::Rgb([7, 19, 201]))
            .save(path)
            .unwrap();
        let png = std::fs::read(path).unwrap();
        let encoded = pose_capture_receipt(br#"{"request_sequence":7}"#, path, [3, 2]).unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(receipt["request_sequence"], 7);
        assert_eq!(
            receipt["capture"]["path"],
            serde_json::to_value(path).unwrap()
        );
        assert_eq!(
            receipt["capture"]["sha256"],
            format!("{:x}", Sha256::digest(&png))
        );
        assert_eq!(receipt["capture"]["width"], 3);
        assert_eq!(receipt["capture"]["height"], 2);
        assert_eq!(receipt["capture"]["encoded_bytes"], png.len());
        assert!(pose_capture_receipt(b"{}", path, [4, 2]).is_err());
        assert!(pose_capture_receipt(b"[]", path, [3, 2]).is_err());
        assert!(pose_capture_receipt(&vec![b' '; 1024 * 1024], path, [3, 2]).is_err());
        let huge =
            serde_json::to_vec(&serde_json::json!({"pad":"x".repeat(1024*1024-32)})).unwrap();
        assert!(huge.len() < 1024 * 1024);
        assert!(pose_capture_receipt(&huge, path, [3, 2]).is_err());
        let file = OpenOptions::new().write(true).open(path).unwrap();
        file.set_len(8 * 1024 * 1024).unwrap();
        let exact = pose_capture_receipt(b"{}", path, [3, 2]).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&exact).unwrap();
        assert_eq!(value["capture"]["encoded_bytes"], 8 * 1024 * 1024);
        assert_eq!(
            value["capture"]["sha256"],
            format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
        );
        file.set_len(8 * 1024 * 1024 + 1).unwrap();
        assert!(
            pose_capture_receipt(b"{}", path, [3, 2])
                .err()
                .unwrap()
                .to_string()
                .contains("exceeds 8 MiB")
        );
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
