//! Prepare immutable models once, then place views of them by reference identity.
use crate::model::{self, Model, Result, Textures};
use bevy::prelude::*;
use fallout_data::{
    assets::ArchiveAssets,
    baseline,
    coordinates::{self, Affine},
    identity::FormKey,
    plugin,
    store::RecordStore,
    vfs::AssetPath,
    world,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

pub struct CellSources {
    pub native: Option<crate::native::Host>,
    pub owner: Mutex<world::residency::CellResidency>,
    pub ticket: world::residency::Ticket,
    // Keep the complete resource-bearing plan/payload lease through decode,
    // GPU staging and admission. Never detach a clone of sources.plan().
    pub sources: Arc<world::residency::ResidentSources>,
    pub textures: Arc<world::residency::ResidentTextures>,
}

#[derive(Component, Clone)]
pub struct ReferenceView {
    pub key: FormKey,
    pub canonical: Option<fallout_runtime::reference_state::View>,
}

impl ReferenceView {
    pub fn label(&self) -> String {
        format!("{}:{:06X}", self.key.origin_plugin, self.key.local_id)
    }
}

pub struct Instance {
    pub model: usize,
    pub transform: Transform,
    pub key: Option<FormKey>,
    pub visibility: Visibility,
    pub canonical: Option<fallout_runtime::reference_state::View>,
}

#[derive(Resource)]
pub struct Prepared {
    pub models: Vec<Model>,
    pub instances: Vec<Instance>,
    pub images: Vec<Image>,
    pub center: Vec3,
    pub radius: f32,
    pub origin: [f64; 3],
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum Report {
    Model(model::Report),
    Cell(Box<CellReport>),
    Fixture(crate::fixture::Report),
    Terrain(Box<crate::terrain::Report>),
}

#[derive(Serialize)]
pub struct ModelInspection {
    pub path: AssetPath,
    pub model_index: Option<usize>,
    pub report: Option<model::Report>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct PlacementOutcome {
    pub key: FormKey,
    pub model: Option<usize>,
    pub source_affine: Option<Affine>,
    pub status: String,
}

#[derive(Serialize)]
pub struct CellReport {
    pub schema_version: u32,
    pub cell: world::CellReport,
    pub load_order: Vec<String>,
    pub load_order_sha256: String,
    pub plugin_sha256: BTreeMap<String, String>,
    pub models: Vec<ModelInspection>,
    pub placements: Vec<PlacementOutcome>,
    pub unique_render_models: usize,
    pub rendered_references: usize,
    pub rendered_mesh_instances: usize,
    pub shared_geometry_vertices: usize,
    pub shared_geometry_triangles: usize,
    pub unique_texture_samplers: usize,
    pub source_origin: [f64; 3],
    pub relative_view_bounds: [[f32; 3]; 2],
    pub coordinates: &'static str,
    pub rendering: &'static str,
    pub runtime_ready: bool,
    pub retail_parity_accepted: bool,
    pub source_residency: world::residency::Snapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_state: Option<crate::native::Report>,
}

pub fn load_model(
    install: &Path,
    path: &AssetPath,
    pose: Option<crate::pose::Request>,
) -> Result<(Prepared, Report)> {
    let assets = ArchiveAssets::open_nv(install)?;
    let mut textures = Textures::default();
    let (model, report) = model::load(&assets, path, &mut textures, pose)?;
    Ok(single_model(model, report, textures))
}

pub fn load_model_file(
    install: &Path,
    path: &Path,
    pose: Option<crate::pose::Request>,
) -> Result<(Prepared, Report)> {
    let mut bytes = Vec::new();
    baseline::open_source(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Explicit model source file exceeds 64 MiB".into());
    }
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Explicit model source requires a Unicode filename")?;
    let label = AssetPath::new(format!("local-source/{filename}").as_bytes())?;
    let assets = ArchiveAssets::open_nv(install)?;
    let mut textures = Textures::default();
    let (model, mut report) =
        model::from_bytes_with_pose(&assets, &label, &bytes, &mut textures, pose)?;
    report.schema_version = 3;
    report.source_file = Some(Box::new(path.to_path_buf()));
    Ok(single_model(model, report, textures))
}

fn single_model(model: Model, report: model::Report, textures: Textures) -> (Prepared, Report) {
    let prepared = Prepared {
        center: model.center,
        radius: model.radius,
        origin: [0.; 3],
        models: vec![model],
        images: textures.images,
        instances: vec![Instance {
            model: 0,
            transform: Transform::IDENTITY,
            key: None,
            visibility: Visibility::Inherited,
            canonical: None,
        }],
    };
    (prepared, Report::Model(report))
}

pub fn load_cell(
    install: &Path,
    order_path: &Path,
    editor_id: &str,
    context: &crate::loading::Context,
    native_save: Option<&Path>,
    shutdown: crate::native::Shutdown,
) -> Result<(Prepared, Report, CellSources)> {
    context.stage("Opening original plugin headers and archive indices")?;
    let names: Vec<String> = serde_json::from_reader(baseline::open_source(order_path)?)?;
    let mut store =
        RecordStore::open_nv_headers(&install.join("Data"), &names, plugin::Limits::default())?;
    let assets = ArchiveAssets::open_nv(install)?;
    let cell = world::inspect_cell(&mut store, editor_id.as_bytes(), assets.mounts())?;
    if cell.cell.flags.value & 1 == 0 {
        return Err("cell preview currently requires an interior CELL".into());
    }
    if cell.link_failures != 0 {
        return Err("cell contains unresolved or wrong-kind reference links".into());
    }
    if cell.references.len() > 10_000 {
        return Err("cell reference budget exceeded".into());
    }
    let native_session = if let Some(path) = native_save {
        context.stage("Binding canonical save to the same source catalogue")?;
        let catalogue = Arc::new(fallout_data::loaded_scripts::Catalogue::load(
            &mut store,
            Default::default(),
            |_, _| {
                context
                    .check()
                    .map_err(|error| fallout_data::Error::Resolution(error.to_string()))
            },
        )?);
        let mut keys = cell
            .references
            .iter()
            .map(|reference| reference.key.clone())
            .collect::<Vec<_>>();
        keys.sort();
        Some(crate::native::Session::load(
            path,
            &[install.to_path_buf()],
            catalogue,
            cell.key.clone(),
            keys,
        )?)
    } else {
        None
    };
    let canonical = native_session
        .as_ref()
        .map(crate::native::Session::bindings)
        .transpose()?
        .map(|bindings| {
            bindings
                .into_iter()
                .map(|binding| (binding.key.clone(), binding))
                .collect::<BTreeMap<_, _>>()
        });
    context.stage("Sealing source CELL model requests")?;
    let plan = world::preparation::CellModelPlan::load(
        &mut store,
        &cell.key,
        assets.mounts(),
        Default::default(),
    )?;
    context.check()?;
    let mut owner = world::residency::CellResidency::new(
        install,
        None,
        world::residency::Limits {
            workers: 1,
            ..Default::default()
        },
    )?;
    let ticket = owner.request(plan)?;
    loop {
        context.check()?;
        let snapshot = owner.poll()?;
        context.stage(format!(
            "Loading CELL models: {}/{}",
            snapshot.completed_models, snapshot.requested_models
        ))?;
        if snapshot.stage == world::residency::Stage::Decoded {
            break;
        }
        // This wait is confined to the single host worker. Window polls remain
        // nonblocking; ResourceJobs owns extraction and cancellation boundaries.
        thread::sleep(Duration::from_millis(5));
    }
    let sources = owner.sources(&ticket)?;
    context.stage("Sealing captured CELL texture requests")?;
    let texture_plan =
        world::residency::TexturePlan::load(sources.clone(), assets.mounts(), Default::default())?;
    context.check()?;
    owner.request_textures(&ticket, texture_plan)?;
    let resident_textures = loop {
        context.check()?;
        ticket.check()?;
        let snapshot = owner.poll()?;
        context.stage(format!(
            "Loading CELL textures: {}/{}",
            snapshot.completed_textures, snapshot.requested_textures
        ))?;
        match snapshot.texture_state {
            world::residency::TextureState::Decoded => {
                break owner.texture_sources(&ticket)?;
            }
            world::residency::TextureState::Unsupported => {
                let captured = owner.texture_sources(&ticket)?;
                let receipt = captured.receipt()?;
                let failures: Vec<_> = receipt
                    .usages
                    .iter()
                    .filter(|usage| usage.error.is_some() || usage.candidates.len() != 1)
                    .take(3)
                    .map(|usage| {
                        let path: String = String::from_utf8_lossy(&usage.raw_path)
                            .chars()
                            .take(160)
                            .collect();
                        format!(
                            "{path}: {}",
                            usage.error.clone().unwrap_or_else(|| {
                                format!("{} source candidates", usage.candidates.len())
                            })
                        )
                    })
                    .collect();
                return Err(format!(
                    "Captured CELL texture dependencies are unresolved ({} usages; plan {}): {}",
                    receipt.missing_or_ambiguous,
                    receipt.identity,
                    failures.join("; ")
                )
                .into());
            }
            _ => thread::sleep(Duration::from_millis(5)),
        }
    };
    let mut plugin_sha256 = BTreeMap::new();
    for name in &names {
        plugin_sha256.insert(
            name.clone(),
            baseline::digest_file(&install.join("Data").join(name))?.1,
        );
    }
    let by_base: BTreeMap<_, _> = cell.models.iter().map(|m| (&m.base_key, m)).collect();
    let mut selected = BTreeMap::<AssetPath, Vec<usize>>::new();
    let mut placements = Vec::new();
    for reference in &cell.references {
        let mut outcome = PlacementOutcome {
            key: reference.key.clone(),
            model: None,
            source_affine: None,
            status: String::new(),
        };
        if reference.record_flags & plugin::DELETED != 0 {
            outcome.status = "deleted".into();
        } else if canonical.is_none() && reference.record_flags & plugin::INITIALLY_DISABLED != 0 {
            outcome.status = "initially-disabled".into();
        } else if reference.record_kind != "REFR" {
            outcome.status = "actor-model-selection-unimplemented".into();
        } else if let (Some(placement), Some(base)) = (&reference.placement, &reference.base) {
            if canonical.is_none() && placement.enable_parent.is_some() {
                outcome.status = "enable-parent-evaluation-unimplemented".into();
            } else if let Some(model) = base.key.as_ref().and_then(|key| by_base.get(key)) {
                if let Some(path) = &model.asset_path {
                    if model.candidates.len() != 1 {
                        outcome.status = "missing-or-ambiguous-model".into();
                    } else {
                        outcome.source_affine = Some(Affine::nv_reference(
                            &placement.transform.value,
                            placement.scale.as_ref().map_or(1., |v| v.value),
                        )?);
                        if let Some(source) = canonical
                            .as_ref()
                            .and_then(|bindings| bindings[&reference.key].source_affine)
                        {
                            outcome.source_affine = Some(source);
                        }
                        selected
                            .entry(path.clone())
                            .or_default()
                            .push(placements.len());
                        outcome.status = "selected".into();
                    }
                } else {
                    outcome.status = "no-supported-modl".into();
                }
            } else {
                outcome.status = "base-model-not-indexed".into();
            }
        } else {
            outcome.status = "missing-placement-or-base".into();
        }
        placements.push(outcome);
    }
    if selected.len() > 2048 {
        return Err("cell model budget exceeded".into());
    }
    let origin = placements
        .iter()
        .find_map(|p| p.source_affine.map(|a| a.rows.map(|r| r[3])))
        .ok_or("cell has no supported model references")?;
    let mut models = Vec::new();
    let mut inspections = Vec::new();
    let mut textures = Textures::default();
    let mut instances = Vec::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut vertices = 0usize;
    let mut triangles = 0usize;
    let mut mesh_instances = 0usize;
    for (path, uses) in selected {
        context.stage(format!(
            "Decoding {}",
            String::from_utf8_lossy(path.bytes())
        ))?;
        ticket.check()?;
        eprintln!("Preparing {}", String::from_utf8_lossy(path.bytes()));
        let mut inspection = ModelInspection {
            path: path.clone(),
            model_index: None,
            report: None,
            error: None,
        };
        let request = sources
            .plan()?
            .receipt()
            .requests
            .iter()
            .position(|request| request.path == path)
            .ok_or("Selected render model is absent from sealed residency requests")?;
        match model::from_resident_bytes(
            &resident_textures,
            &path,
            sources.model(request)?,
            &mut textures,
        ) {
            Ok((model, report)) => {
                let geometry_bytes =
                    (vertices + report.vertices) * 64 + (triangles + report.triangles) * 12;
                if geometry_bytes > 256 * 1024 * 1024 {
                    return Err("cell geometry budget exceeded".into());
                }
                vertices += report.vertices;
                triangles += report.triangles;
                let index = models.len();
                let bounds = report.bounds;
                for use_index in uses {
                    let outcome = &mut placements[use_index];
                    let source = outcome
                        .source_affine
                        .expect("selected reference has a transform");
                    let view = model::affine(source.relative_view(origin).rows);
                    if !view.is_finite() {
                        return Err("relative reference transform overflows f32".into());
                    }
                    for corner in 0..8 {
                        let p = Vec3::new(
                            bounds[(corner & 1) as usize][0],
                            bounds[((corner >> 1) & 1) as usize][1],
                            bounds[((corner >> 2) & 1) as usize][2],
                        );
                        let p = view.transform_point3(p);
                        if !p.is_finite() {
                            return Err("relative model bounds overflow f32".into());
                        }
                        min = min.min(p);
                        max = max.max(p);
                    }
                    outcome.model = Some(index);
                    let canonical_binding =
                        canonical.as_ref().map(|bindings| &bindings[&outcome.key]);
                    outcome.status = canonical_binding
                        .map_or("rendered-static-view", |binding| binding.display)
                        .into();
                    instances.push(Instance {
                        model: index,
                        transform: Transform::from_matrix(view),
                        key: Some(outcome.key.clone()),
                        visibility: if canonical_binding
                            .is_some_and(|binding| binding.source_affine.is_none())
                        {
                            Visibility::Hidden
                        } else {
                            Visibility::Inherited
                        },
                        canonical: canonical_binding.and_then(|binding| binding.canonical.clone()),
                    });
                    mesh_instances += model.parts.len();
                }
                inspection.model_index = Some(index);
                inspection.report = Some(report);
                models.push(model);
            }
            Err(error) => {
                let error = error.to_string();
                for use_index in uses {
                    placements[use_index].status = format!("model-unavailable: {error}");
                }
                inspection.error = Some(error);
            }
        }
        inspections.push(inspection);
    }
    if instances.is_empty() {
        return Err("cell has no renderable static references".into());
    }
    let center = (min + max) * 0.5;
    let radius = (max - min).length().max(1.) * 0.5;
    context.check()?;
    ticket.check()?;
    let (native, canonical_state) = if let Some(session) = native_session {
        context.stage("Starting read-only canonical presentation host")?;
        let (host, observation) = session.start(origin, shutdown)?;
        (Some(host), Some(observation.report))
    } else {
        (None, None)
    };
    // Captured texture closure is complete; draw readiness still describes the
    // displayed static subset. The report retains every omission; actor models,
    // original shaders, collision and behavior do not become simulation-ready.
    owner.report_dependencies(&ticket, world::residency::Readiness::Ready)?;
    owner.report_collision(&ticket, world::residency::Readiness::Unsupported)?;
    owner.report_behavior(&ticket, world::residency::Readiness::Unsupported)?;
    let report = CellReport {
        schema_version: if canonical_state.is_some() { 4 } else { 3 },
        cell,
        load_order: names,
        load_order_sha256: baseline::digest_file(order_path)?.1,
        plugin_sha256,
        models: inspections,
        placements,
        unique_render_models: models.len(),
        rendered_references: instances
            .iter()
            .filter(|instance| instance.visibility != Visibility::Hidden)
            .count(),
        rendered_mesh_instances: if canonical_state.is_some() {
            instances
                .iter()
                .filter(|instance| instance.visibility != Visibility::Hidden)
                .map(|instance| models[instance.model].parts.len())
                .sum()
        } else {
            mesh_instances
        },
        shared_geometry_vertices: vertices,
        shared_geometry_triangles: triangles,
        unique_texture_samplers: textures.images.len(),
        source_origin: origin,
        relative_view_bounds: [min.to_array(), max.to_array()],
        coordinates: "source units; clockwise X then Y then Z; [x,y,z] -> [x,z,-y]; subtract source origin in f64",
        rendering: "unlit static views with source alpha, culling and depth states; model failures and omitted references retained; no retail lighting/effects or collision parity",
        runtime_ready: false,
        retail_parity_accepted: false,
        source_residency: owner.snapshot(),
        canonical_state,
    };
    Ok((
        Prepared {
            models,
            instances,
            images: textures.images,
            center,
            radius,
            origin,
        },
        Report::Cell(Box::new(report)),
        CellSources {
            native,
            owner: Mutex::new(owner),
            ticket,
            sources,
            textures: resident_textures,
        },
    ))
}

pub fn source_camera(position: [f64; 3], target: [f64; 3], origin: [f64; 3]) -> Result<Transform> {
    if position
        .iter()
        .chain(&target)
        .chain(&origin)
        .any(|v| !v.is_finite())
    {
        return Err("camera coordinates must be finite".into());
    }
    let position =
        Vec3::from_array(coordinates::source_to_view(position, origin).map(|v| v as f32));
    let target = Vec3::from_array(coordinates::source_to_view(target, origin).map(|v| v as f32));
    let direction = target - position;
    // Finite endpoints can subtract to infinity, and a finite direction can
    // overflow while computing its length. Bevy look_to substitutes NEG_Z when
    // Dir3 conversion fails; a source camera must refuse that changed facing.
    let (facing, length) = Dir3::new_and_length(direction)
        .map_err(|_| "camera direction must have a finite nonzero length")?;
    if !position.is_finite()
        || !target.is_finite()
        || length < 0.001
        || facing.cross(Vec3::Y).length() < 0.001
    {
        return Err("camera position/target must define a finite nonvertical view".into());
    }
    Ok(Transform::from_translation(position).looking_to(facing, Dir3::Y))
}

/// An inspection record retains the actual renderer words. Source coordinates
/// are redundant evidence and must round-trip without losing a view component.
pub mod camera {
    use super::*;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::io::{self, Write};

    pub const RECORD_BYTES: usize = 4096;
    const SOURCE_REPORT_BYTES: usize = 64 * 1024 * 1024;
    const MAX_VIEW_COMPONENT: f32 = 1e12;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Binding {
        pub scene_epoch: u64,
        pub source_scene_sha256: String,
        pub source_origin_f64_bits: [u64; 3],
        #[serde(deserialize_with = "required_option")]
        pub canonical_revision: Option<u64>,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Initial {
        pub position_f64_bits: [u64; 3],
        pub target_f64_bits: [u64; 3],
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Viewport {
        pub physical_pixels: [u32; 2],
        pub logical_f32_bits: [u32; 2],
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Perspective {
        pub fov_f32_bits: u32,
        pub aspect_f32_bits: u32,
        pub near_f32_bits: u32,
        pub far_f32_bits: u32,
        pub near_clip_plane_f32_bits: [u32; 4],
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Record {
        pub schema_version: u32,
        pub binding: Binding,
        pub translation_f32_bits: [u32; 3],
        pub rotation_f32_bits: [u32; 4],
        pub source_position_f64_bits: [u64; 3],
        pub source_direction_f64_bits: [u64; 3],
        pub perspective: Perspective,
        pub viewport: Viewport,
        #[serde(deserialize_with = "required_option")]
        pub initial_source: Option<Initial>,
        pub original_gameplay_accepted: bool,
    }

    fn required_option<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Option<T>, D::Error> {
        Option::deserialize(deserializer)
    }

    struct HashWriter {
        hash: Sha256,
        bytes: usize,
    }

    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let next = self
                .bytes
                .checked_add(bytes.len())
                .filter(|n| *n <= SOURCE_REPORT_BYTES)
                .ok_or_else(|| io::Error::other("Camera source report exceeds 64 MiB"))?;
            self.hash.update(bytes);
            self.bytes = next;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    pub fn bind(report: &Report, origin: [f64; 3], epoch: u64) -> Result<Binding> {
        if epoch == 0 || origin.iter().any(|v| !v.is_finite()) {
            return Err("Camera scene needs a nonzero epoch and finite source origin".into());
        }
        let mut writer = HashWriter {
            hash: Sha256::new(),
            bytes: 0,
        };
        // Save-slot generations and residency progress are transaction/status
        // evidence. A new Save of an unchanged scene must not change its camera
        // identity. Keep every source placement, omission and native view.
        match report {
            Report::Cell(cell) => serde_json::to_writer(
                &mut writer,
                &(
                    "cell-source-camera-v1",
                    &cell.cell,
                    &cell.load_order,
                    &cell.load_order_sha256,
                    &cell.plugin_sha256,
                    &cell.models,
                    &cell.placements,
                    cell.unique_render_models,
                    cell.rendered_references,
                    cell.rendered_mesh_instances,
                    cell.shared_geometry_vertices,
                    cell.shared_geometry_triangles,
                    cell.unique_texture_samplers,
                    cell.source_origin,
                    cell.relative_view_bounds,
                    cell.canonical_state.as_ref().map(|state| {
                        (
                            state.schema_version,
                            state.campaign,
                            &state.catalogue_sha256,
                            state.revision,
                            &state.cell,
                            &state.bindings,
                        )
                    }),
                ),
            )?,
            _ => serde_json::to_writer(&mut writer, report)?,
        }
        Ok(Binding {
            scene_epoch: epoch,
            source_scene_sha256: format!("{:x}", writer.hash.finalize()),
            source_origin_f64_bits: origin.map(f64::to_bits),
            canonical_revision: match report {
                Report::Cell(cell) => cell.canonical_state.as_ref().map(|state| state.revision),
                _ => None,
            },
        })
    }

    pub fn viewport(camera: &Camera) -> Result<Viewport> {
        if !camera.is_active || camera.viewport.is_some() || camera.sub_camera_view.is_some() {
            return Err("Camera record requires one active full inspection viewport".into());
        }
        let physical = camera
            .physical_viewport_size()
            .ok_or("Camera viewport not ready")?;
        let logical = camera
            .logical_viewport_size()
            .ok_or("Camera logical viewport not ready")?;
        let result = Viewport {
            physical_pixels: physical.to_array(),
            logical_f32_bits: logical.to_array().map(f32::to_bits),
        };
        result.validate()?;
        Ok(result)
    }

    impl Viewport {
        fn validate(&self) -> Result<()> {
            let [width, height] = self.physical_pixels;
            let logical = self.logical_f32_bits.map(f32::from_bits);
            if width == 0
                || height == 0
                || width > 8192
                || height > 8192
                || u64::from(width) * u64::from(height) > 16 * 1024 * 1024
                || logical
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0. || *v > 8192.)
            {
                return Err("Camera viewport exceeds finite dimension/pixel limits".into());
            }
            Ok(())
        }
    }

    impl Perspective {
        fn from_projection(projection: &Projection) -> Result<Self> {
            let Projection::Perspective(p) = projection else {
                return Err(
                    "Camera record supports the existing perspective inspector only".into(),
                );
            };
            Ok(Self {
                fov_f32_bits: p.fov.to_bits(),
                aspect_f32_bits: p.aspect_ratio.to_bits(),
                near_f32_bits: p.near.to_bits(),
                far_f32_bits: p.far.to_bits(),
                near_clip_plane_f32_bits: p.near_clip_plane.to_array().map(f32::to_bits),
            })
        }

        fn projection(&self, viewport: &Viewport) -> Result<Projection> {
            viewport.validate()?;
            let fov = f32::from_bits(self.fov_f32_bits);
            let aspect_ratio = f32::from_bits(self.aspect_f32_bits);
            let near = f32::from_bits(self.near_f32_bits);
            let far = f32::from_bits(self.far_f32_bits);
            let plane = self.near_clip_plane_f32_bits.map(f32::from_bits);
            let [width, height] = viewport.logical_f32_bits.map(f32::from_bits);
            if !fov.is_finite()
                || !(0.001..3.13).contains(&fov)
                || !aspect_ratio.is_finite()
                || aspect_ratio <= 0.
                || aspect_ratio.to_bits() != (width / height).to_bits()
                || !near.is_finite()
                || near <= 0.
                || !far.is_finite()
                || far <= near
                || far > MAX_VIEW_COMPONENT
                || plane.iter().any(|v| !v.is_finite())
                || plane[0] != 0.
                || plane[1] != 0.
                || plane[2] != -1.
                || plane[3] >= 0.
            {
                return Err("Camera perspective/viewport words are invalid or oblique".into());
            }
            let projection = Projection::Perspective(PerspectiveProjection {
                fov,
                aspect_ratio,
                near,
                far,
                near_clip_plane: Vec4::from_array(plane),
            });
            let matrix = projection.get_clip_from_view();
            if !matrix.is_finite()
                || matrix.determinant() == 0.
                || !matrix.inverse().is_finite()
                || projection
                    .get_frustum_corners(-near, -far)
                    .iter()
                    .any(|corner| !corner.is_finite())
            {
                return Err("Camera projection must have a finite nonsingular matrix".into());
            }
            Ok(projection)
        }
    }

    fn source_position(transform: &Transform, origin: [f64; 3]) -> Result<[f64; 3]> {
        let [x, y, z] = transform.translation.to_array().map(f64::from);
        let result = [origin[0] + x, origin[1] - z, origin[2] + y];
        let roundtrip = coordinates::source_to_view(result, origin).map(|v| v as f32);
        if result.iter().any(|v| !v.is_finite())
            || roundtrip
                .iter()
                .zip(transform.translation.to_array())
                .any(|(a, b)| a.to_bits() != b.to_bits() && !(*a == 0. && b == 0.))
        {
            return Err("Camera source origin loses actual view position precision".into());
        }
        Ok(result)
    }

    fn source_direction(transform: &Transform) -> [f64; 3] {
        let forward = transform.forward().to_array();
        [
            f64::from(forward[0]),
            -f64::from(forward[2]),
            f64::from(forward[1]),
        ]
    }

    fn validate_view(transform: &Transform, origin: [f64; 3]) -> Result<()> {
        let length = transform.rotation.length_squared();
        if origin.iter().any(|v| !v.is_finite())
            || !transform.translation.is_finite()
            || transform.translation.abs().max_element() > MAX_VIEW_COMPONENT
            || !transform.rotation.is_finite()
            || !length.is_finite()
            || (length - 1.).abs() > 0.00002
            || transform.scale != Vec3::ONE
        {
            return Err("Camera view words must be finite with unit rotation and scale".into());
        }
        // forward() constructs an unchecked Dir3. Check rotation before calling it.
        let direction = Dir3::new(transform.forward().as_vec3())
            .map_err(|_| "Camera look direction is singular")?;
        if direction.cross(Vec3::Y).length() < 0.001 {
            return Err("Camera look direction is vertical".into());
        }
        Ok(())
    }

    impl Record {
        fn validate(&self) -> Result<(Transform, Projection)> {
            if self.schema_version != 1
                || self.original_gameplay_accepted
                || self.binding.scene_epoch == 0
                || self.binding.source_scene_sha256.len() != 64
                || !self
                    .binding
                    .source_scene_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
            {
                return Err("Camera record schema/source/epoch is invalid".into());
            }
            let origin = self.binding.source_origin_f64_bits.map(f64::from_bits);
            let transform = Transform {
                translation: Vec3::from_array(self.translation_f32_bits.map(f32::from_bits)),
                rotation: Quat::from_array(self.rotation_f32_bits.map(f32::from_bits)),
                scale: Vec3::ONE,
            };
            validate_view(&transform, origin)?;
            if source_position(&transform, origin)?.map(f64::to_bits)
                != self.source_position_f64_bits
                || source_direction(&transform).map(f64::to_bits) != self.source_direction_f64_bits
            {
                return Err("Camera source and actual view words disagree".into());
            }
            if let Some(initial) = &self.initial_source {
                super::source_camera(
                    initial.position_f64_bits.map(f64::from_bits),
                    initial.target_f64_bits.map(f64::from_bits),
                    origin,
                )?;
            }
            Ok((transform, self.perspective.projection(&self.viewport)?))
        }
    }

    pub fn record(
        binding: &Binding,
        transform: &Transform,
        projection: &Projection,
        viewport: Viewport,
        initial_source: Option<Initial>,
        max_bytes: usize,
    ) -> Result<Record> {
        validate_view(
            transform,
            binding.source_origin_f64_bits.map(f64::from_bits),
        )?;
        let value = Record {
            schema_version: 1,
            binding: binding.clone(),
            translation_f32_bits: transform.translation.to_array().map(f32::to_bits),
            rotation_f32_bits: transform.rotation.to_array().map(f32::to_bits),
            source_position_f64_bits: source_position(
                transform,
                binding.source_origin_f64_bits.map(f64::from_bits),
            )?
            .map(f64::to_bits),
            source_direction_f64_bits: source_direction(transform).map(f64::to_bits),
            perspective: Perspective::from_projection(projection)?,
            viewport,
            initial_source,
            original_gameplay_accepted: false,
        };
        value.validate()?;
        encode(&value, max_bytes)?;
        Ok(value)
    }

    pub fn restore(
        record: &Record,
        current: &Binding,
        viewport: &Viewport,
    ) -> Result<(Transform, Projection)> {
        if record.binding != *current || record.viewport != *viewport {
            return Err("Camera restore source/origin/epoch/revision/viewport is stale".into());
        }
        record.validate()
    }

    struct BoundedWriter {
        bytes: Vec<u8>,
        max: usize,
    }
    impl Write for BoundedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self
                .bytes
                .len()
                .checked_add(bytes.len())
                .is_none_or(|n| n > self.max)
            {
                return Err(io::Error::other(
                    "Camera record exceeds admitted output bytes",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    pub fn encode(record: &Record, max_bytes: usize) -> Result<Vec<u8>> {
        if max_bytes == 0 || max_bytes > RECORD_BYTES {
            return Err("Camera record bound must be 1..4096 bytes".into());
        }
        let mut writer = BoundedWriter {
            bytes: Vec::new(),
            max: max_bytes,
        };
        serde_json::to_writer_pretty(&mut writer, record)?;
        writer.write_all(b"\n")?;
        Ok(writer.bytes)
    }

    pub fn decode(bytes: &[u8], max_bytes: usize) -> Result<Record> {
        if max_bytes == 0 || max_bytes > RECORD_BYTES || bytes.len() > max_bytes {
            return Err("Camera request exceeds admitted 4 KiB input".into());
        }
        let record: Record = serde_json::from_slice(bytes)?;
        record.validate()?;
        Ok(record)
    }

    pub fn read_request(path: &Path, max_bytes: usize) -> Result<Record> {
        if max_bytes == 0 || max_bytes > RECORD_BYTES {
            return Err("Camera request bound must be 1..4096 bytes".into());
        }
        let mut bytes = Vec::new();
        baseline::open_source(path)?
            .take(max_bytes as u64 + 1)
            .read_to_end(&mut bytes)?;
        decode(&bytes, max_bytes)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bevy::camera::CameraProjection;

        fn fixture() -> (Binding, Transform, Projection, Viewport) {
            let binding = Binding {
                scene_epoch: 7,
                source_scene_sha256: "ab".repeat(32),
                source_origin_f64_bits: [1000., 2000., 3000.].map(f64::to_bits),
                canonical_revision: None,
            };
            let transform = Transform::from_xyz(1., 3., -2.);
            let projection = Projection::Perspective(PerspectiveProjection {
                fov: 0.8,
                aspect_ratio: 1280. / 900.,
                near: 0.25,
                far: 5000.,
                ..default()
            });
            let viewport = Viewport {
                physical_pixels: [1280, 900],
                logical_f32_bits: [1280., 900.].map(f32::to_bits),
            };
            (binding, transform, projection, viewport)
        }

        #[test]
        fn literal_source_basis_exact_actual_words_and_camera_roundtrip() {
            let (binding, transform, projection, viewport) = fixture();
            let value = record(
                &binding,
                &transform,
                &projection,
                viewport.clone(),
                None,
                RECORD_BYTES,
            )
            .unwrap();
            assert_eq!(
                value.source_position_f64_bits,
                [1001., 2002., 3003.].map(f64::to_bits)
            );
            assert_eq!(
                value.source_direction_f64_bits.map(f64::from_bits),
                [0., 1., 0.]
            );
            assert_eq!(value.translation_f32_bits, [1., 3., -2.].map(f32::to_bits));
            assert_eq!(value.rotation_f32_bits, [0., 0., 0., 1.].map(f32::to_bits));
            let encoded = encode(&value, RECORD_BYTES).unwrap();
            assert_eq!(decode(&encoded, RECORD_BYTES).unwrap(), value);
            let (restored, restored_projection) = restore(&value, &binding, &viewport).unwrap();
            assert_eq!(restored, transform);
            assert_eq!(
                Perspective::from_projection(&restored_projection).unwrap(),
                value.perspective
            );
            let exact = encoded.len();
            assert_eq!(encode(&value, exact).unwrap(), encoded);
            assert!(encode(&value, exact - 1).is_err());
            assert!(decode(&encoded, exact - 1).is_err());
            let mut padded = encoded.clone();
            padded.resize(RECORD_BYTES, b' ');
            assert_eq!(decode(&padded, RECORD_BYTES).unwrap(), value);
            padded.push(b' ');
            assert!(decode(&padded, RECORD_BYTES).is_err());

            let mut shifted = binding.clone();
            shifted.source_origin_f64_bits[0] = 1e20f64.to_bits();
            assert!(
                record(
                    &shifted,
                    &transform,
                    &projection,
                    viewport,
                    None,
                    RECORD_BYTES
                )
                .is_err()
            );
            assert_eq!(transform.translation, Vec3::new(1., 3., -2.));
            let mut signed = binding;
            signed.source_origin_f64_bits = [0.; 3].map(f64::to_bits);
            let signed_view = Transform::from_xyz(-0., 1., -0.);
            let signed_record = record(
                &signed,
                &signed_view,
                &projection,
                fixture().3,
                None,
                RECORD_BYTES,
            )
            .unwrap();
            assert_eq!(
                restore(&signed_record, &signed, &signed_record.viewport)
                    .unwrap()
                    .0
                    .translation
                    .to_array()
                    .map(f32::to_bits),
                [-0., 1., -0.].map(f32::to_bits)
            );
        }

        #[test]
        fn complete_strict_camera_input_refuses_unknown_missing_duplicate_and_invalid_words() {
            let (binding, transform, projection, viewport) = fixture();
            let valid = record(
                &binding,
                &transform,
                &projection,
                viewport,
                None,
                RECORD_BYTES,
            )
            .unwrap();
            let json = serde_json::to_value(&valid).unwrap();
            for field in [
                "schema_version",
                "binding",
                "translation_f32_bits",
                "rotation_f32_bits",
                "source_position_f64_bits",
                "source_direction_f64_bits",
                "perspective",
                "viewport",
                "initial_source",
                "original_gameplay_accepted",
            ] {
                let mut bad = json.clone();
                bad.as_object_mut().unwrap().remove(field);
                assert!(
                    decode(&serde_json::to_vec(&bad).unwrap(), RECORD_BYTES).is_err(),
                    "missing {field}"
                );
            }
            for nested in ["binding", "perspective", "viewport"] {
                let fields: Vec<_> = json[nested].as_object().unwrap().keys().cloned().collect();
                for field in fields {
                    let mut bad = json.clone();
                    bad[nested].as_object_mut().unwrap().remove(&field);
                    assert!(
                        decode(&serde_json::to_vec(&bad).unwrap(), RECORD_BYTES).is_err(),
                        "missing {nested}.{field}"
                    );
                }
                let mut bad = json.clone();
                bad[nested]["guessed"] = serde_json::json!(1);
                assert!(decode(&serde_json::to_vec(&bad).unwrap(), RECORD_BYTES).is_err());
            }
            let mut bad = json.clone();
            bad["extra"] = serde_json::json!(true);
            assert!(decode(&serde_json::to_vec(&bad).unwrap(), RECORD_BYTES).is_err());
            let duplicate = String::from_utf8(encode(&valid, RECORD_BYTES).unwrap())
                .unwrap()
                .replacen("{", "{\"schema_version\":1,", 1);
            assert!(decode(duplicate.as_bytes(), RECORD_BYTES).is_err());
            for index in 0..11 {
                let mut bad = valid.clone();
                match index {
                    0 => bad.rotation_f32_bits = [0; 4],
                    1 => bad.translation_f32_bits[0] = f32::INFINITY.to_bits(),
                    2 => bad.perspective.fov_f32_bits = 0,
                    3 => bad.perspective.aspect_f32_bits = 2f32.to_bits(),
                    4 => bad.perspective.near_f32_bits = (-1f32).to_bits(),
                    5 => bad.perspective.far_f32_bits = bad.perspective.near_f32_bits,
                    6 => bad.source_position_f64_bits[0] = 1002f64.to_bits(),
                    7 => bad.source_direction_f64_bits[1] = 0,
                    8 => bad.original_gameplay_accepted = true,
                    9 => bad.perspective.near_clip_plane_f32_bits[0] = 1f32.to_bits(),
                    _ => {
                        // Infinite-reverse perspective matrices do not contain
                        // far. Matrix/inverse checks alone admit these words,
                        // while actual frustum corner products overflow.
                        bad.perspective.fov_f32_bits = 3f32.to_bits();
                        bad.perspective.far_f32_bits = f32::MAX.to_bits();
                        let raw = PerspectiveProjection {
                            fov: 3.,
                            far: f32::MAX,
                            near: 0.25,
                            aspect_ratio: 1280. / 900.,
                            ..default()
                        };
                        assert!(raw.get_clip_from_view().is_finite());
                        assert!(
                            raw.get_frustum_corners(-raw.near, -raw.far)
                                .iter()
                                .any(|corner| !corner.is_finite())
                        );
                    }
                }
                assert!(
                    decode(&encode(&bad, RECORD_BYTES).unwrap(), RECORD_BYTES).is_err(),
                    "invalid {index}"
                );
            }
        }

        #[test]
        fn stale_source_origin_epoch_revision_and_viewport_do_not_restore() {
            let (binding, transform, projection, viewport) = fixture();
            let value = record(
                &binding,
                &transform,
                &projection,
                viewport.clone(),
                None,
                RECORD_BYTES,
            )
            .unwrap();
            for index in 0..4 {
                let mut other = binding.clone();
                match index {
                    0 => other.scene_epoch += 1,
                    1 => other.source_scene_sha256 = "cd".repeat(32),
                    2 => other.source_origin_f64_bits[1] = 0,
                    _ => other.canonical_revision = Some(1),
                }
                assert!(restore(&value, &other, &viewport).is_err());
            }
            let mut other = viewport;
            other.physical_pixels = [640, 450];
            assert!(restore(&value, &binding, &other).is_err());
            for rotation in [
                Quat::from_array([0.; 4]),
                Quat::from_array([f32::NAN, 0., 0., 1.]),
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            ] {
                let invalid = Transform {
                    rotation,
                    ..transform
                };
                assert!(
                    record(
                        &binding,
                        &invalid,
                        &projection,
                        fixture().3,
                        None,
                        RECORD_BYTES
                    )
                    .is_err()
                );
            }
            let orthographic = Projection::Orthographic(OrthographicProjection::default_3d());
            assert!(
                record(
                    &binding,
                    &transform,
                    &orthographic,
                    fixture().3,
                    None,
                    RECORD_BYTES
                )
                .is_err()
            );
            let mut writer = HashWriter {
                hash: Sha256::new(),
                bytes: SOURCE_REPORT_BYTES - 1,
            };
            writer.write_all(b"x").unwrap();
            let prior = writer.hash.clone().finalize();
            assert!(writer.write_all(b"y").is_err());
            assert_eq!(writer.hash.finalize(), prior);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_camera_rejects_finite_endpoint_subtraction_and_length_overflow() {
        let max = f64::from(f32::MAX);
        assert!(source_camera([-max, 0., 0.], [max, 0., 0.], [0.; 3]).is_err());
        assert!(source_camera([0.; 3], [1e30, 0., 0.], [0.; 3]).is_err());
    }

    #[test]
    fn source_camera_rejects_degenerate_nonfinite_and_narrowing_failures() {
        for (position, target, origin) in [
            ([0.; 3], [0.; 3], [0.; 3]),
            ([0.; 3], [0., 0., 10.], [0.; 3]),
            ([0.; 3], [0.0001, 0., 0.], [0.; 3]),
            ([f64::NAN, 0., 0.], [1.; 3], [0.; 3]),
            ([0.; 3], [f64::INFINITY, 0., 0.], [0.; 3]),
            ([0.; 3], [1.; 3], [f64::NAN, 0., 0.]),
            ([f64::MAX, 0., 0.], [0.; 3], [-f64::MAX, 0., 0.]),
            ([1e20, 0., 0.], [1e20 + 1., 0., 0.], [0.; 3]),
        ] {
            assert!(source_camera(position, target, origin).is_err());
        }
    }

    #[test]
    fn source_camera_keeps_declared_facing_and_rebased_position() {
        // The source +Y direction is view -Z; +X remains +X. These expectations
        // are basis vectors, independent of the production camera calculation.
        for (target, facing) in [([10., 25., 30.], Vec3::NEG_Z), ([15., 20., 30.], Vec3::X)] {
            let camera = source_camera([10., 20., 30.], target, [9., 18., 27.]).unwrap();
            assert_eq!(camera.translation, Vec3::new(1., 3., -2.));
            assert!((camera.forward().as_vec3() - facing).length() < 1e-6);
            assert!(camera.rotation.is_finite());
        }
    }
}
