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
use std::{collections::BTreeMap, path::Path};

#[derive(Component, Clone)]
pub struct ReferenceView {
    pub key: FormKey,
}

pub struct Instance {
    pub model: usize,
    pub transform: Transform,
    pub key: Option<FormKey>,
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
}

pub fn load_model(install: &Path, path: &AssetPath) -> Result<(Prepared, Report)> {
    let assets = ArchiveAssets::open_nv(install)?;
    let mut textures = Textures::default();
    let (model, report) = model::load(&assets, path, &mut textures)?;
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
        }],
    };
    Ok((prepared, Report::Model(report)))
}

pub fn load_cell(install: &Path, order_path: &Path, editor_id: &str) -> Result<(Prepared, Report)> {
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
        } else if reference.record_flags & plugin::INITIALLY_DISABLED != 0 {
            outcome.status = "initially-disabled".into();
        } else if reference.record_kind != "REFR" {
            outcome.status = "actor-model-selection-unimplemented".into();
        } else if let (Some(placement), Some(base)) = (&reference.placement, &reference.base) {
            if placement.enable_parent.is_some() {
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
        eprintln!("Preparing {}", String::from_utf8_lossy(path.bytes()));
        let mut inspection = ModelInspection {
            path: path.clone(),
            model_index: None,
            report: None,
            error: None,
        };
        match model::load(&assets, &path, &mut textures) {
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
                    outcome.status = "rendered-static-view".into();
                    instances.push(Instance {
                        model: index,
                        transform: Transform::from_matrix(view),
                        key: Some(outcome.key.clone()),
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
    let report = CellReport {
        schema_version: 2,
        cell,
        load_order: names,
        load_order_sha256: baseline::digest_file(order_path)?.1,
        plugin_sha256,
        models: inspections,
        placements,
        unique_render_models: models.len(),
        rendered_references: instances.len(),
        rendered_mesh_instances: mesh_instances,
        shared_geometry_vertices: vertices,
        shared_geometry_triangles: triangles,
        unique_texture_samplers: textures.images.len(),
        source_origin: origin,
        relative_view_bounds: [min.to_array(), max.to_array()],
        coordinates: "source units; clockwise X then Y then Z; [x,y,z] -> [x,z,-y]; subtract source origin in f64",
        rendering: "unlit static views with source alpha, culling and depth states; model failures and omitted references retained; no retail lighting/effects or collision parity",
        runtime_ready: false,
        retail_parity_accepted: false,
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
    ))
}

pub fn source_camera(position: [f64; 3], target: [f64; 3], origin: [f64; 3]) -> Result<Transform> {
    if position.iter().chain(&target).any(|v| !v.is_finite()) {
        return Err("camera coordinates must be finite".into());
    }
    let position =
        Vec3::from_array(coordinates::source_to_view(position, origin).map(|v| v as f32));
    let target = Vec3::from_array(coordinates::source_to_view(target, origin).map(|v| v as f32));
    let direction = target - position;
    if !position.is_finite()
        || !target.is_finite()
        || direction.length() < 0.001
        || direction.normalize().cross(Vec3::Y).length() < 0.001
    {
        return Err("camera position/target must define a finite nonvertical view".into());
    }
    Ok(Transform::from_translation(position).looking_at(target, Vec3::Y))
}
