//! Bounded diffuse inspection passes over authored quadrant weights. These draw
//! states are preview choices, not reconstructed Bethesda material semantics.
use crate::{
    material::Raster,
    model::{self, Part, Textures},
};
use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};
use fallout_data::{
    assets::ArchiveAssets,
    terrain::{self, Landscape, TerrainReport},
    vfs::AssetPath,
};
use serde::Serialize;
use std::collections::BTreeMap;

pub struct SurfaceView<'a> {
    pub positions: &'a [[f32; 3]],
    pub normals: &'a [[f32; 3]],
    pub colors: Option<&'a [[u8; 3]]>,
    pub hidden: u8,
}

#[derive(Serialize)]
pub struct Report {
    pub blend_maps: terrain::blends::BlendMaps,
    pub diffuse_textures: Vec<model::TextureEvidence>,
    pub layer_draws: usize,
    pub drawn_vertices: usize,
    pub drawn_triangles: usize,
    pub texture_repeats_per_quadrant: f32,
    pub uv_convention: &'static str,
    pub rendering: &'static str,
}

pub fn prepare(
    report: &TerrainReport,
    land: &Landscape,
    surface: SurfaceView<'_>,
    repeats: f32,
    assets: &ArchiveAssets,
) -> model::Result<(Vec<Part>, Textures, Report)> {
    if !repeats.is_finite() || repeats <= 0. || repeats > 64. {
        return Err("terrain inspection texture repetition must be finite and in (0,64]".into());
    }
    let maps = terrain::blends::build(land)?;
    if !maps.missing_base_quadrants.is_empty() {
        return Err(format!("textured terrain has missing base layers in quadrants {:?}; default/inheritance behavior is unverified", maps.missing_base_quadrants).into());
    }
    if maps.unapplied_default_layers != 0 {
        return Err("textured terrain contains unapplied NULL default layers; select the vertex-color inspector until default behavior is verified".into());
    }
    if maps.clamped_samples != 0 {
        return Err("textured terrain preview refuses out-of-range authored opacity".into());
    }
    if surface.positions.len() != 1089
        || surface.normals.len() != 1089
        || surface.colors.is_some_and(|v| v.len() != 1089)
    {
        return Err("terrain view does not contain the complete source grid".into());
    }
    let closure = report
        .texture_dependencies
        .as_ref()
        .ok_or("missing terrain texture inspection")?;
    if closure.failures != 0 {
        return Err("terrain texture dependencies are unresolved".into());
    }
    let bindings: BTreeMap<_, _> = closure
        .bindings
        .iter()
        .map(|b| (b.layer_index, b))
        .collect();
    let paths: BTreeMap<_, _> = closure
        .path_usages
        .iter()
        .filter(|p| p.slot == 0)
        .filter_map(|p| p.path.as_ref().map(|path| (&p.texture_set, path)))
        .collect();
    let mut textures = Textures::default();
    let mut parts = Vec::new();
    for quadrant in &maps.quadrants {
        if surface.hidden & (1 << quadrant.quadrant) != 0 {
            continue;
        }
        let base = quadrant.base.as_ref().ok_or("missing base layer")?;
        for (pass, layer) in std::iter::once(base).chain(&quadrant.overlays).enumerate() {
            let binding = bindings
                .get(&layer.source_layer)
                .ok_or("missing layer texture binding")?;
            let set = binding
                .texture_set
                .as_ref()
                .and_then(|d| d.key.as_ref())
                .ok_or("missing texture set identity")?;
            let path: &AssetPath = paths.get(set).ok_or("missing resolved diffuse path")?;
            let texture = textures.load(assets, path, 3)?;
            let mut positions = Vec::with_capacity(289);
            let mut normals = Vec::with_capacity(289);
            let mut uv = Vec::with_capacity(289);
            let mut colors = Vec::with_capacity(289);
            for y in 0..17 {
                for x in 0..17 {
                    let source = terrain::blends::source_vertex(quadrant.quadrant, x, y)
                        .ok_or("invalid quadrant vertex")?;
                    positions.push(surface.positions[source]);
                    normals.push(surface.normals[source]);
                    let start_x = usize::from(quadrant.quadrant & 1) * 16;
                    let start_y = usize::from(quadrant.quadrant >> 1) * 16;
                    uv.push([
                        (start_x + x) as f32 / 16. * repeats,
                        (start_y + y) as f32 / 16. * repeats,
                    ]);
                    let weight = f32::from(layer.weights[y * 17 + x]) / 255.;
                    let rgb = surface.colors.map_or([255; 3], |v| v[source]);
                    colors.push([
                        f32::from(rgb[0]) / 255. * weight,
                        f32::from(rgb[1]) / 255. * weight,
                        f32::from(rgb[2]) / 255. * weight,
                        1.,
                    ]);
                }
            }
            let mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(terrain::blends::indices()));
            parts.push(Part {
                mesh,
                texture: Some(texture),
                color: Color::WHITE,
                raster: layer_raster(pass != 0),
            });
        }
    }
    if parts.is_empty() {
        return Err("all textured terrain quadrants are hidden".into());
    }
    let evidence = Report {
        layer_draws: parts.len(),
        drawn_vertices: parts.len() * 289,
        drawn_triangles: parts.len() * 512,
        blend_maps: maps,
        diffuse_textures: textures.evidence.clone(),
        texture_repeats_per_quadrant: repeats,
        uv_convention: "source local X/Y; periodic phase across quadrant boundaries; repetition supplied explicitly for inspection, retail orientation/scale unmeasured",
        rendering: "unlit diffuse RGB times authored VCLR and interpolated byte weights; opaque residual base plus additive depth-tested overlays; weights not renormalized; no normal/specular shader, measured retail blend or lighting",
    };
    Ok((parts, textures, evidence))
}

/// An opaque base establishes depth even where its residual color weight is zero.
/// Overlay passes add linear RGB at that depth without changing the depth buffer.
pub fn layer_raster(overlay: bool) -> Raster {
    if overlay {
        Raster {
            alpha_flags: 1,
            depth_write: false,
            ..Raster::default()
        }
    } else {
        Raster::default()
    }
}
