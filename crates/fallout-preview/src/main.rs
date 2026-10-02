//! A small inspection host for the production decoder, not a gameplay runtime.
mod model;

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    asset::RenderAssetUsages,
    camera::RenderTarget,
    core_pipeline::tonemapping::Tonemapping,
    prelude::*,
    render::{
        RenderPlugin,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use clap::Parser;
use fallout_data::vfs::AssetPath;
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Parser, Resource)]
#[command(about = "Inspect one archived New Vegas model with unlit diffuse textures")]
struct Options {
    #[arg(long)]
    install: PathBuf,
    /// Archive path, for example meshes/furniture/chair01.nif.
    #[arg(long)]
    model: String,
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

fn output_path(path: &Path, install: &Path) -> model::Result<PathBuf> {
    let name = path.file_name().ok_or("output needs a file name")?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let result = parent.canonicalize()?.join(name);
    if result.starts_with(install.canonicalize()?) {
        return Err("output must be outside the installation".into());
    }
    if result.try_exists()? {
        return Err(format!("output already exists: {}", result.display()).into());
    }
    Ok(result)
}

fn run() -> model::Result<AppExit> {
    let mut options = Options::parse();
    if let Some(path) = &options.capture {
        options.capture = Some(output_path(path, &options.install)?);
    }
    if let Some(path) = &options.report {
        options.report = Some(output_path(path, &options.install)?);
    }
    if options.capture.is_some() && options.capture == options.report {
        return Err("capture and report must have different paths".into());
    }
    let path = AssetPath::new(options.model.as_bytes())?;
    let (model, report) = model::load(&options.install, &path)?;
    eprintln!(
        "{} meshes, {} vertices, {} triangles, {} diffuse textures; {} recorded limitations",
        report.meshes,
        report.vertices,
        report.triangles,
        report.textures.len(),
        report.warnings.len()
    );
    eprintln!("A/D or arrows: orbit; W/S: tilt; Q/E: zoom; R: reset; Esc: close");
    if let Some(path) = &options.report {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
    }
    let orbit = Orbit {
        center: model.center,
        radius: model.radius,
        yaw: 2.5,
        pitch: 0.3,
        distance: model.radius * 3.,
    };
    let headless = options.headless;
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: (!headless).then(|| Window {
                title: format!("Fallout Rust — model inspection — {}", options.model),
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
        .insert_resource(options)
        .insert_resource(model)
        .insert_resource(orbit)
        .insert_resource(ClearColor(Color::srgb(0.035, 0.045, 0.055)))
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, capture));
    if headless {
        app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(16)));
    }
    Ok(app.run())
}

fn setup(
    mut commands: Commands,
    mut model: ResMut<model::Model>,
    options: Res<Options>,
    orbit: Res<Orbit>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let textures: Vec<_> = model
        .images
        .drain(..)
        .map(|image| images.add(image))
        .collect();
    for part in model.parts.drain(..) {
        commands.spawn((
            Mesh3d(meshes.add(part.mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: part.color,
                base_color_texture: part.texture.map(|i| textures[i].clone()),
                alpha_mode: part.alpha_mode,
                unlit: true,
                ..default()
            })),
        ));
    }
    let camera = commands
        .spawn((
            Camera3d::default(),
            Tonemapping::None,
            orbit.transform(),
            Projection::Perspective(PerspectiveProjection {
                near: (orbit.radius * 0.001).max(0.01),
                far: orbit.radius * 100.,
                ..default()
            }),
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

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut orbit: ResMut<Orbit>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    let delta = time.delta_secs().min(0.1);
    let direction = |positive, negative| {
        f32::from(u8::from(keys.pressed(positive))) - f32::from(u8::from(keys.pressed(negative)))
    };
    orbit.yaw += (direction(KeyCode::KeyD, KeyCode::KeyA)
        + direction(KeyCode::ArrowRight, KeyCode::ArrowLeft))
        * delta;
    orbit.pitch = (orbit.pitch + direction(KeyCode::KeyW, KeyCode::KeyS) * delta).clamp(-1.4, 1.4);
    orbit.distance = (orbit.distance * (direction(KeyCode::KeyE, KeyCode::KeyQ) * delta).exp())
        .clamp(orbit.radius * 0.1, orbit.radius * 20.);
    if keys.just_pressed(KeyCode::KeyR) {
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
    mut state: ResMut<Capture>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = options.capture.clone() else {
        return;
    };
    if state.started.elapsed() > Duration::from_secs(120) {
        error!("GPU capture timed out");
        exit.write(AppExit::error());
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
    commands.spawn(screenshot).observe(
        move |event: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
            let save = || -> model::Result<()> {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                event
                    .image
                    .clone()
                    .try_into_dynamic()?
                    .to_rgb8()
                    .write_to(&mut file, image::ImageFormat::Png)?;
                file.sync_all()?;
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
