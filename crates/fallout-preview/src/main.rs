//! A small inspection host for the production decoder, not a gameplay runtime.
mod model;
mod scene;

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
#[command(about = "Inspect archived New Vegas models or a placed interior")]
struct Options {
    #[arg(long)]
    install: PathBuf,
    /// Archive path, for example meshes/furniture/chair01.nif.
    #[arg(long, required_unless_present = "cell", conflicts_with = "cell")]
    model: Option<String>,
    /// Interior CELL editor ID, for example GSDocMitchellHouse.
    #[arg(long, requires = "load_order")]
    cell: Option<String>,
    #[arg(long, requires = "cell")]
    load_order: Option<PathBuf>,
    /// Camera position in original source units (x,y,z).
    #[arg(long, num_args = 3, value_delimiter = ',', allow_negative_numbers = true,
        requires_all = ["camera_look_at", "cell"])]
    camera_position: Option<Vec<f64>>,
    #[arg(long, num_args = 3, value_delimiter = ',', allow_negative_numbers = true,
        requires_all = ["camera_position", "cell"])]
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
    let (prepared, report) = if let Some(name) = &options.model {
        scene::load_model(&options.install, &AssetPath::new(name.as_bytes())?)?
    } else {
        scene::load_cell(
            &options.install,
            options
                .load_order
                .as_deref()
                .expect("clap requires load order"),
            options
                .cell
                .as_deref()
                .expect("clap requires model or cell"),
        )?
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
    }
    eprintln!(
        "Tab: orbit/fly; fly: WASD, Q/E vertical, arrows look, Shift faster; R: reset; Esc: close"
    );
    if let Some(path) = &options.report {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
    }
    let orbit = Orbit {
        center: prepared.center,
        radius: prepared.radius,
        yaw: 2.5,
        pitch: 0.3,
        distance: prepared.radius * 3.,
    };
    let home = match (&options.camera_position, &options.camera_look_at) {
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
    let headless = options.headless;
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: (!headless).then(|| Window {
                title: format!(
                    "Fallout Rust - inspection - {}",
                    options.model.as_ref().or(options.cell.as_ref()).unwrap()
                ),
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
        .insert_resource(prepared)
        .insert_resource(orbit)
        .insert_resource(navigation)
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
    mut prepared: ResMut<scene::Prepared>,
    options: Res<Options>,
    navigation: Res<Navigation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let textures: Vec<_> = prepared
        .images
        .drain(..)
        .map(|image| images.add(image))
        .collect();
    // Upload each model once. Repeated references share mesh/material handles.
    let templates: Vec<Vec<_>> = prepared
        .models
        .iter_mut()
        .map(|model| {
            model
                .parts
                .drain(..)
                .map(|part| {
                    (
                        meshes.add(part.mesh),
                        materials.add(StandardMaterial {
                            base_color: part.color,
                            base_color_texture: part.texture.map(|i| textures[i].clone()),
                            alpha_mode: part.alpha_mode,
                            unlit: true,
                            ..default()
                        }),
                    )
                })
                .collect()
        })
        .collect();
    for instance in prepared.instances.drain(..) {
        let mut parent = commands.spawn((instance.transform, Visibility::default()));
        if let Some(key) = instance.key {
            let view = scene::ReferenceView { key };
            parent.insert((
                Name::new(format!(
                    "{}:{:06X}",
                    view.key.origin_plugin, view.key.local_id
                )),
                view,
            ));
        }
        parent.with_children(|parent| {
            for (mesh, material) in &templates[instance.model] {
                parent.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone())));
            }
        });
    }
    let camera = commands
        .spawn((
            Camera3d::default(),
            Tonemapping::None,
            navigation.home,
            Projection::Perspective(PerspectiveProjection {
                near: (prepared.radius * 0.001).max(0.01),
                far: prepared.radius * 100.,
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
    mut navigation: ResMut<Navigation>,
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
    if keys.just_pressed(KeyCode::Tab) {
        navigation.fly = !navigation.fly;
    }
    if navigation.fly {
        for mut transform in &mut cameras {
            if keys.just_pressed(KeyCode::KeyR) {
                *transform = navigation.home;
            }
            let yaw = direction(KeyCode::ArrowLeft, KeyCode::ArrowRight) * delta;
            let pitch = direction(KeyCode::ArrowUp, KeyCode::ArrowDown) * delta;
            transform.rotate_y(yaw);
            // Keep a small margin from vertical to avoid an ambiguous up direction.
            let current_pitch = transform.forward().y.asin();
            transform.rotate_local_x((current_pitch + pitch).clamp(-1.5, 1.5) - current_pitch);
            let movement = transform.forward().as_vec3() * direction(KeyCode::KeyW, KeyCode::KeyS)
                + transform.right().as_vec3() * direction(KeyCode::KeyD, KeyCode::KeyA)
                + Vec3::Y * direction(KeyCode::KeyE, KeyCode::KeyQ);
            let speed = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
                600.
            } else {
                180.
            };
            transform.translation += movement.normalize_or_zero() * speed * delta;
        }
        return;
    }
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
