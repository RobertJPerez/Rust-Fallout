//! A small inspection host for the production decoder, not a gameplay runtime.
mod fixture;
mod input;
mod material;
mod model;
mod scene;
mod startup;
mod terrain;
mod terrain_textures;

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
    window::ExitCondition,
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

#[derive(Parser, Resource)]
#[command(about = "Inspect New Vegas models, placed interiors or authored terrain")]
#[command(group(ArgGroup::new("mode").required(true).args(["model", "cell", "terrain", "material_fixture"])))]
struct Options {
    #[arg(long)]
    install: Option<PathBuf>,
    /// Archive path, for example meshes/furniture/chair01.nif.
    #[arg(long, requires = "install")]
    model: Option<String>,
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

fn output_path(path: &Path, install: Option<&Path>) -> model::Result<PathBuf> {
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
    if result.try_exists()? {
        return Err(format!("output already exists: {}", result.display()).into());
    }
    Ok(result)
}

fn run() -> model::Result<AppExit> {
    let mut options = Options::parse();
    if let Some(path) = &options.capture {
        options.capture = Some(output_path(path, options.install.as_deref())?);
    }
    if let Some(path) = &options.report {
        options.report = Some(output_path(path, options.install.as_deref())?);
    }
    if options.capture.is_some() && options.capture == options.report {
        return Err("capture and report must have different paths".into());
    }
    let preparation = startup::Progress::start(options.headless);
    let (prepared, report) = if options.material_fixture {
        let (prepared, report) = fixture::prepare()?;
        (prepared, scene::Report::Fixture(report))
    } else if let Some(name) = &options.model {
        scene::load_model(
            options.install.as_deref().expect("clap requires install"),
            &AssetPath::new(name.as_bytes())?,
        )?
    } else if let Some(name) = &options.terrain {
        terrain::load(
            options.install.as_deref().expect("clap requires install"),
            options.load_order.as_deref().expect("clap requires order"),
            name,
            options.terrain_texture_repeat,
        )?
    } else {
        scene::load_cell(
            options.install.as_deref().expect("clap requires install"),
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
    if let Some(path) = &options.report
        && !options.material_fixture
    {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
    }
    preparation.finish();
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
    let headless = options.headless;
    let input_context = if headless || options.material_fixture {
        input::Context::Suspended
    } else if navigation.fly {
        input::Context::Fly
    } else {
        input::Context::Orbit
    };
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: (!headless).then(|| Window {
                title: format!(
                    "Fallout Rust - inspection - {}",
                    options
                        .model
                        .as_deref()
                        .or(options.cell.as_deref())
                        .or(options.terrain.as_deref())
                        .unwrap_or("synthetic material fixture")
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
    if let scene::Report::Fixture(report) = report {
        app.insert_resource(report);
    }
    app.add_plugins(plugins)
        .add_plugins(material::InspectionPlugin)
        .add_plugins(input::InspectionInputPlugin)
        .insert_resource(input_context)
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
    mut materials: ResMut<Assets<material::InspectionMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    startup::stage("Graphics initialized; preparing the terrain/model draw resources.");
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
                        materials.add(material::adapt(
                            StandardMaterial {
                                base_color: part.color,
                                base_color_texture: part.texture.map(|i| textures[i].clone()),
                                ..default()
                            },
                            part.raster,
                        )),
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
                    near: (prepared.radius * 0.001).max(0.01),
                    far: prepared.radius * 100.,
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
