//! Present one real LAND surface through the existing inspection adapter. This
//! does not substitute a procedural map or claim complete exterior rendering.
use crate::{
    material::Raster,
    model::{self, Model, Part},
    scene::{self, Instance, Prepared},
};
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};
use fallout_data::{
    assets::ArchiveAssets,
    baseline, coordinates, plugin,
    store::RecordStore,
    terrain::{self, Fields},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub terrain: terrain::TerrainReport,
    pub load_order: Vec<String>,
    pub load_order_sha256: String,
    pub geometry_model: &'static str,
    pub vertices: usize,
    pub triangles: usize,
    pub source_origin: [f64; 3],
    pub relative_view_bounds: [[f32; 3]; 2],
    pub land_flags_byte: Option<u8>,
    pub color_mode: &'static str,
    pub coordinates: &'static str,
    pub rendering: &'static str,
    pub retail_parity_accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub textured: Option<crate::terrain_textures::Report>,
}

pub fn load(
    install: &Path,
    order: &Path,
    name: &str,
    repeats: Option<f32>,
) -> model::Result<(Prepared, scene::Report)> {
    let mut order_bytes = Vec::new();
    baseline::open_source(order)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut order_bytes)?;
    if order_bytes.len() > 1024 * 1024 {
        return Err("load-order input exceeds 1 MiB".into());
    }
    let names: Vec<String> = serde_json::from_slice(&order_bytes)?;
    let order_sha = format!("{:x}", Sha256::digest(&order_bytes));
    crate::startup::stage(format!("Indexing {} plugins for {name}...", names.len()));
    let mut store =
        RecordStore::open_nv_headers(&install.join("Data"), &names, plugin::Limits::default())?;
    crate::startup::stage(format!(
        "Reading {name} terrain records and source hashes..."
    ));
    let mut report = terrain::inspect_cell(&mut store, name.as_bytes(), None)?;
    if report.integrity_failures != 0 || report.link_failures != 0 {
        return Err("terrain contains unresolved or corrupt inputs".into());
    }
    if repeats.is_some() {
        crate::startup::stage("Indexing texture archives...");
    }
    let mut archives = repeats
        .map(|_| ArchiveAssets::open_nv(install))
        .transpose()?;
    if let Some(assets) = &mut archives {
        crate::startup::stage("Resolving texture layers and verifying archive/texture bytes...");
        report.texture_dependencies = Some(terrain::textures::inspect(
            &mut store,
            &report,
            assets,
            None,
            None,
            terrain::textures::Limits::default(),
        )?);
    }
    let Fields::Cell(cell) = report.cell.fields.as_ref().ok_or("missing CELL fields")? else {
        return Err("wrong CELL kind".into());
    };
    let flags = cell.land_flags();
    let grid = cell.grid.as_ref().ok_or("missing CELL grid")?.value;
    if report.landscapes.len() != 1 {
        return Err("terrain preview requires exactly one winning LAND".into());
    }
    let entry = &report.landscapes[0];
    let Some(Fields::Land(land)) = &entry.fields else {
        return Err("terrain preview requires a present LAND".into());
    };
    crate::startup::stage("Building the terrain surface...");
    let geometry = terrain::mesh::build(land, flags.unwrap_or(0))?;
    let normal_bits = geometry
        .normal_bits
        .as_ref()
        .ok_or("terrain preview requires authored VNML")?;
    if geometry.indices.is_empty() {
        return Err("all terrain quadrants are hidden".into());
    }
    let origin = [
        f64::from(grid[0]) * 4096. + 2048.,
        f64::from(grid[1]) * 4096. + 2048.,
        geometry.bounds[0][2] * 0.5 + geometry.bounds[1][2] * 0.5,
    ];
    // Local coordinates keep this subtraction exact even for extreme cell IDs.
    let local_origin = [2048., 2048., origin[2]];
    let positions: Vec<_> = geometry
        .local_positions
        .iter()
        .map(|p| coordinates::source_to_view(*p, local_origin).map(|v| v as f32))
        .collect();
    let normals: Vec<_> = normal_bits
        .iter()
        .map(|bits| {
            coordinates::source_to_view(bits.map(|v| f64::from(f32::from_bits(v))), [0.; 3])
                .map(|v| v as f32)
        })
        .collect();
    if positions
        .iter()
        .flatten()
        .chain(normals.iter().flatten())
        .any(|v| !v.is_finite())
    {
        return Err("terrain view exceeds finite f32 coordinates".into());
    }
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in &positions {
        let p = Vec3::from_array(*p);
        min = min.min(p);
        max = max.max(p);
    }
    let vertices = positions.len();
    let triangles = geometry.indices.len() / 3;
    let color_mode = if geometry.colors.is_some() {
        "authored VCLR divided by 255; color-space interpretation unmeasured"
    } else {
        "absent VCLR; white inspection material, no inferred source color"
    };
    let (parts, images, textured) = if let Some(repeats) = repeats {
        crate::startup::stage("Decoding diffuse images and preparing layer draw passes...");
        let (parts, textures, evidence) = crate::terrain_textures::prepare(
            &report,
            land,
            crate::terrain_textures::SurfaceView {
                positions: &positions,
                normals: &normals,
                colors: geometry.colors.as_deref(),
                hidden: flags.unwrap_or(0),
            },
            repeats,
            archives.as_ref().ok_or("missing texture archives")?,
        )?;
        (parts, textures.images, Some(evidence))
    } else {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0f32; 2]; vertices]);
        if let Some(colors) = geometry.colors {
            mesh.insert_attribute(
                Mesh::ATTRIBUTE_COLOR,
                colors
                    .iter()
                    .map(|rgb| {
                        [
                            f32::from(rgb[0]) / 255.,
                            f32::from(rgb[1]) / 255.,
                            f32::from(rgb[2]) / 255.,
                            1.,
                        ]
                    })
                    .collect::<Vec<_>>(),
            );
        }
        mesh.insert_indices(Indices::U32(geometry.indices));
        (
            vec![Part {
                mesh,
                texture: None,
                color: Color::WHITE,
                raster: Raster::default(),
            }],
            vec![],
            None,
        )
    };
    let center = (min + max) * 0.5;
    let radius = (max - min).length() * 0.5;
    if !radius.is_finite() || radius <= 0. {
        return Err("invalid terrain view extent".into());
    }
    let prepared = Prepared {
        models: vec![Model {
            parts,
            center,
            radius,
        }],
        instances: vec![Instance {
            visibility: Visibility::Inherited,
            canonical: None,
            model: 0,
            transform: Transform::IDENTITY,
            key: Some(entry.key.clone()),
        }],
        images,
        center,
        radius,
        origin,
    };
    let report = Report {
        schema_version: 1,
        terrain: report,
        load_order: names,
        load_order_sha256: order_sha,
        geometry_model: terrain::mesh::GEOMETRY_MODEL,
        vertices,
        triangles,
        source_origin: origin,
        relative_view_bounds: [min.to_array(), max.to_array()],
        land_flags_byte: flags,
        color_mode,
        coordinates: "source-local geometry; [x,y,z] -> [x,z,-y]; rebase in f64; measured retail axes/units remain open",
        rendering: if textured.is_some() {
            "unlit authored diffuse-layer inspection; explicit tiling and additive byte-weight passes; retail shaders/blending, defaults, props, water, streaming, collision and gameplay remain open"
        } else {
            "unlit authored terrain height/normal/color inspection; no landscape textures, blending, props, water, retail lighting, streaming, collision or gameplay"
        },
        retail_parity_accepted: false,
        textured,
    };
    Ok((prepared, scene::Report::Terrain(Box::new(report))))
}
