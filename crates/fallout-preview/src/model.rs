//! Adapt decoded source data for inspection. Bethesda shader behavior belongs in
//! a separate renderer; this view uses diffuse textures and an unlit material.
use crate::material::Raster;
use bevy::{
    asset::RenderAssetUsages,
    image::{
        CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
    },
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use fallout_data::{
    assets::ArchiveAssets,
    nif_scene::{
        self, ObjectKind,
        material::{MaterialData, texture_path},
    },
    vfs::AssetPath,
    world::residency::ResidentTextures,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Serialize)]
pub struct TextureEvidence {
    pub path: AssetPath,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub format: String,
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub model: AssetPath,
    pub model_sha256: String,
    pub meshes: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub textures: Vec<TextureEvidence>,
    pub warnings: Vec<String>,
    pub bindings: Vec<BindingEvidence>,
    pub bounds: [[f32; 3]; 2],
    pub coordinates: &'static str,
    pub rendering: &'static str,
    pub retail_parity_accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_skin_pose: Option<Box<crate::pose::SkinSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_object_pose: Option<Box<crate::pose::ObjectSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<Box<PathBuf>>,
}

#[derive(Serialize)]
pub struct BindingEvidence {
    pub mesh_block: u32,
    pub diffuse_mode: &'static str,
    pub raster: Raster,
}

pub struct Part {
    pub mesh: Mesh,
    pub texture: Option<usize>,
    pub color: Color,
    pub raster: Raster,
}

#[derive(Resource)]
pub struct Model {
    pub parts: Vec<Part>,
    pub center: Vec3,
    pub radius: f32,
}

/// A texture is shared across every model using the same path and sampler.
/// The scene has an aggregate budget in addition to each decoder's input limits.
#[derive(Default)]
pub struct Textures {
    pub images: Vec<Image>,
    pub evidence: Vec<TextureEvidence>,
    ids: BTreeMap<(AssetPath, u32), usize>,
    bytes: usize,
}

impl Textures {
    pub(super) fn load(
        &mut self,
        assets: &ArchiveAssets,
        path: &AssetPath,
        clamp: u32,
    ) -> Result<usize> {
        let key = (path.clone(), clamp);
        if let Some(id) = self.ids.get(&key) {
            return Ok(*id);
        }
        if self.images.len() >= 4096 {
            return Err("scene texture budget exceeded".into());
        }
        let (_, data) = assets.read_unique_bounded(
            path,
            (256 * 1024 * 1024usize).saturating_sub(self.bytes) as u64,
        )?;
        self.load_bytes(path, clamp, &data)
    }

    /// A cached sampler is usable only if this exact source lease still admits
    /// the path. Check the receipt and payload before consulting the cache.
    fn load_resident(
        &mut self,
        sources: &ResidentTextures,
        path: &AssetPath,
        clamp: u32,
    ) -> Result<usize> {
        let receipt = sources.receipt()?;
        let index = receipt
            .requests
            .binary_search_by(|request| request.path.cmp(path))
            .map_err(|_| {
                format!(
                    "Texture {} is absent from resident plan {}",
                    String::from_utf8_lossy(path.bytes()),
                    receipt.identity
                )
            })?;
        let data = sources.texture(index)?;
        self.load_bytes(path, clamp, data)
    }

    fn load_bytes(&mut self, path: &AssetPath, clamp: u32, data: &[u8]) -> Result<usize> {
        let key = (path.clone(), clamp);
        if let Some(id) = self.ids.get(&key) {
            return Ok(*id);
        }
        if self.images.len() >= 4096 {
            return Err("scene texture budget exceeded".into());
        }
        if data.len() > (256 * 1024 * 1024usize).saturating_sub(self.bytes) {
            return Err("scene texture byte budget exceeded".into());
        }
        let image = decode_diffuse(data, clamp)?;
        let size = image.texture_descriptor.size;
        self.evidence.push(TextureEvidence {
            path: path.clone(),
            sha256: format!("{:x}", Sha256::digest(data)),
            width: size.width,
            height: size.height,
            mip_levels: image.texture_descriptor.mip_level_count,
            format: format!("{:?}", image.texture_descriptor.format),
        });
        let id = self.images.len();
        self.images.push(image);
        self.ids.insert(key, id);
        self.bytes += data.len();
        Ok(id)
    }
}

/// Rotate source Z-up into Bevy Y-up without changing handedness or source units.
fn basis(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

pub(super) fn affine(rows: [[f64; 4]; 3]) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(rows[0][0] as f32, rows[1][0] as f32, rows[2][0] as f32, 0.),
        Vec4::new(rows[0][1] as f32, rows[1][1] as f32, rows[2][1] as f32, 0.),
        Vec4::new(rows[0][2] as f32, rows[1][2] as f32, rows[2][2] as f32, 0.),
        Vec4::new(rows[0][3] as f32, rows[1][3] as f32, rows[2][3] as f32, 1.),
    )
}

pub fn load(
    assets: &ArchiveAssets,
    path: &AssetPath,
    textures: &mut Textures,
    pose: Option<crate::pose::Request>,
) -> Result<(Model, Report)> {
    let (_, bytes) = assets.read_unique(path)?;
    if let Some(request) = pose {
        from_bytes_with_pose(assets, path, &bytes, textures, Some(request))
    } else {
        from_bytes(assets, path, &bytes, textures)
    }
}

/// Adapt already-read static model bytes with archived diffuse textures.
/// CELL adaptation uses its retained texture lease through from_resident_bytes.
pub fn from_bytes(
    assets: &ArchiveAssets,
    path: &AssetPath,
    bytes: &[u8],
    textures: &mut Textures,
) -> Result<(Model, Report)> {
    from_texture_source(TextureInput::Archive(assets), path, bytes, textures, None)
}

/// An explicit standalone pose shares the same model and diffuse adapters.
pub fn from_bytes_with_pose(
    assets: &ArchiveAssets,
    path: &AssetPath,
    bytes: &[u8],
    textures: &mut Textures,
    pose: Option<crate::pose::Request>,
) -> Result<(Model, Report)> {
    from_texture_source(TextureInput::Archive(assets), path, bytes, textures, pose)
}

/// CELL adaptation borrows both payloads from this generation's sealed leases.
/// A missing resident texture is an error; it cannot trigger another archive read.
pub fn from_resident_bytes(
    sources: &ResidentTextures,
    path: &AssetPath,
    bytes: &[u8],
    textures: &mut Textures,
) -> Result<(Model, Report)> {
    sources.ticket().check()?;
    from_texture_source(TextureInput::Resident(sources), path, bytes, textures, None)
}

#[derive(Clone, Copy)]
enum TextureInput<'a> {
    Archive(&'a ArchiveAssets),
    Resident(&'a ResidentTextures),
}
impl TextureInput<'_> {
    fn load(self, textures: &mut Textures, path: &AssetPath, clamp: u32) -> Result<usize> {
        match self {
            Self::Archive(assets) => textures.load(assets, path, clamp),
            Self::Resident(sources) => textures.load_resident(sources, path, clamp),
        }
    }
}

fn from_texture_source(
    input: TextureInput<'_>,
    path: &AssetPath,
    bytes: &[u8],
    textures: &mut Textures,
    pose: Option<crate::pose::Request>,
) -> Result<(Model, Report)> {
    let selected_skin = match pose {
        Some(crate::pose::Request::Skin(request)) => Some(crate::pose::skin(
            bytes,
            &String::from_utf8_lossy(path.bytes()),
            request,
        )?),
        _ => None,
    };
    let (index, scene) = nif_scene::decode(bytes, &String::from_utf8_lossy(path.bytes()))?;
    let mut selected_object = match pose {
        Some(crate::pose::Request::Object(request)) => Some(crate::pose::object(
            bytes,
            &String::from_utf8_lossy(path.bytes()),
            request,
            &scene,
        )?),
        _ => None,
    };
    let objects: BTreeMap<_, _> = scene.objects.iter().map(|v| (v.block, v)).collect();
    let worlds: BTreeMap<_, _> = scene
        .world_transforms
        .iter()
        .map(|v| (v.block, v))
        .collect();
    let materials: BTreeMap<_, _> = scene.materials.iter().map(|v| (v.block, &v.data)).collect();
    let meshes: BTreeMap<_, _> = scene.meshes.iter().map(|v| (v.block, v)).collect();
    let mut report = Report {
        schema_version: 2,
        model: path.clone(),
        model_sha256: format!("{:x}", Sha256::digest(bytes)),
        meshes: 0,
        vertices: 0,
        triangles: 0,
        textures: vec![],
        warnings: vec![],
        bindings: vec![],
        bounds: [[0.; 3]; 2],
        coordinates: "source units; [x, y, z] -> [x, z, -y]; source UVs unchanged",
        rendering: "unlit diffuse/vertex-color inspection with source alpha, culling and depth states; no retail lighting, effects, animation or collision parity",
        retail_parity_accepted: false,
        source_skin_pose: None,
        source_object_pose: None,
        source_file: None,
    };
    for (kind, blocks) in &scene.unsupported_blocks {
        report.warnings.push(if pose.is_some() {
            format!(
                "{} {kind} blocks outside static scene projection; selected pose capabilities recorded separately",
                blocks.len()
            )
        } else {
            format!("{} unsupported {kind} blocks", blocks.len())
        });
    }
    let mut parts = Vec::new();
    let mut used_textures = BTreeSet::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for object in &scene.objects {
        let object_world = selected_object
            .as_ref()
            .and_then(|pose| pose.worlds.get(&object.block))
            .copied();
        if selected_object.is_some() && object_world.is_none() {
            continue;
        }
        if selected_skin
            .as_ref()
            .is_some_and(|skin| skin.summary.geometry != object.block)
        {
            continue;
        }
        let ObjectKind::Mesh {
            data: Some(data),
            skin,
            ..
        } = object.kind
        else {
            continue;
        };
        let world = worlds[&object.block];
        if !world.reachable_from_footer {
            continue;
        }
        if skin.is_some() && selected_skin.is_none() {
            report.warnings.push(format!(
                "Skipped skinned mesh {}: skinning is not implemented",
                object.block
            ));
            continue;
        }
        let mut ancestor = Some(object.block);
        let mut hidden = false;
        let mut root = object.block;
        while let Some(id) = ancestor {
            root = id;
            hidden |= objects[&id].flags & 1 != 0;
            if id != object.block && !objects[&id].properties.is_empty() {
                report.warnings.push(format!(
                    "Mesh {} has ancestor properties; inheritance is not evaluated",
                    object.block
                ));
            }
            ancestor = worlds[&id].parent;
        }
        if hidden {
            continue;
        }
        let has_markers = objects[&root].extra_data.iter().flatten().any(|id| {
            scene
                .extra_flags
                .iter()
                .any(|extra| extra.block == *id && extra.value & 32 != 0)
        });
        let name = object
            .name
            .map(|id| index.strings[id as usize].as_slice())
            .unwrap_or_default();
        if has_markers
            && [
                b"EditorMarker".as_slice(),
                b"VisibilityEditorMarker".as_slice(),
            ]
            .iter()
            .any(|prefix| {
                name.get(..prefix.len())
                    .is_some_and(|v| v.eq_ignore_ascii_case(prefix))
            })
        {
            report.warnings.push(format!(
                "Omitted declared editor marker mesh {} ({})",
                object.block,
                String::from_utf8_lossy(name)
            ));
            continue;
        }
        let Some(source) = meshes.get(&data) else {
            continue;
        };
        if source.vertices.is_empty() || source.triangles.is_empty() {
            continue;
        }
        if let Some(skin) = &selected_skin
            && (skin.summary.geometry_data != data
                || skin.positions.len() != source.vertices.len()
                || skin.normals.len() != source.normals.len())
        {
            return Err("Selected skin arrays differ from the exact source geometry".into());
        }
        // The skin adapter already applied its world map and view basis.
        // Object descendants carry their freshly composed source world below;
        // neither path reapplies the geometry owner's stored world matrix.
        let matrix = if let Some(posed_world) = object_world {
            affine(posed_world)
        } else {
            selected_skin
                .as_ref()
                .map_or_else(|| affine(world.matrix), |skin| skin.world)
        };
        let determinant = matrix.determinant();
        if !matrix.is_finite() || !determinant.is_finite() || determinant.abs() < 1e-12 {
            return Err(format!(
                "mesh {} has a singular or unrepresentable transform",
                object.block
            )
            .into());
        }
        let positions: Vec<[f32; 3]> = if let Some(posed_world) = object_world {
            crate::pose::object_vectors(posed_world, &source.vertices, false)?
        } else if let Some(skin) = &selected_skin {
            skin.positions.clone()
        } else {
            source
                .vertices
                .iter()
                .map(|v| {
                    let p = basis(matrix.transform_point3(Vec3::from_array(*v)));
                    p.to_array()
                })
                .collect()
        };
        if positions.iter().flatten().any(|v| !v.is_finite()) {
            return Err("preview positions overflowed f32".into());
        }
        for &point in &positions {
            min = min.min(Vec3::from_array(point));
            max = max.max(Vec3::from_array(point));
        }
        let mut indices = Vec::with_capacity(source.triangles.len() * 3);
        for triangle in &source.triangles {
            let [a, b, c] = triangle.map(u32::from);
            indices.extend(if determinant < 0. {
                [a, c, b]
            } else {
                [a, b, c]
            });
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices));
        if let Some(posed_world) = object_world {
            if !source.normals.is_empty() {
                mesh.insert_attribute(
                    Mesh::ATTRIBUTE_NORMAL,
                    crate::pose::object_vectors(posed_world, &source.normals, true)?,
                );
            }
        } else if let Some(skin) = &selected_skin {
            if !skin.normals.is_empty() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, skin.normals.clone());
            }
        } else if !source.normals.is_empty() {
            let normal_matrix = matrix.inverse().transpose();
            let normals: Vec<[f32; 3]> = source
                .normals
                .iter()
                .map(|v| {
                    basis(normal_matrix.transform_vector3(Vec3::from_array(*v)))
                        .normalize_or_zero()
                        .to_array()
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        } else {
            mesh.compute_smooth_normals();
        }
        if let Some(skin) = &selected_skin {
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                return Err("Selected skin draw position attribute is unavailable".into());
            };
            let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
                Some(VertexAttributeValues::Float32x3(normals)) => normals.as_slice(),
                None if skin.normals.is_empty() => &[],
                _ => return Err("Selected skin draw normal attribute is unavailable".into()),
            };
            if crate::pose::draw_hash(positions) != skin.summary.draw_positions_sha256
                || crate::pose::draw_hash(normals) != skin.summary.draw_normals_sha256
            {
                return Err("Selected skin draw attributes differ from the source pose".into());
            }
        }
        if let Some(posed_world) = object_world {
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                return Err("Selected object draw positions are unavailable".into());
            };
            let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
                Some(VertexAttributeValues::Float32x3(normals)) => normals.as_slice(),
                None if source.normals.is_empty() => &[],
                _ => return Err("Selected object draw normals are unavailable".into()),
            };
            selected_object
                .as_mut()
                .expect("object world came from selected pose")
                .summary
                .draw_meshes
                .push(crate::pose::ObjectMesh {
                    geometry: object.block,
                    geometry_data: data,
                    source_world: posed_world,
                    positions_sha256: crate::pose::draw_hash(positions),
                    normals_sha256: crate::pose::draw_hash(normals),
                });
        }
        if let Some(uvs) = source.uv_sets.first() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone());
        }
        if !source.colors.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, source.colors.clone());
        }
        let mut diffuse = None;
        let mut clamp = 3;
        let mut alpha = 1.;
        let mut raster = Raster::default();
        let mut untextured_shader = false;
        let mut has_material = false;
        let mut has_texture_property = false;
        let mut unknown_property = false;
        for property in object.properties.iter().flatten() {
            match materials.get(property) {
                Some(MaterialData::PerPixelLighting {
                    texture_set: Some(set),
                    shader,
                    ..
                }) => {
                    has_texture_property = true;
                    clamp = shader.clamp_mode;
                    raster.depth_test = shader.flags & 0x8000_0000 != 0;
                    raster.depth_write = shader.flags2 & 1 != 0;
                    if let Some(MaterialData::TextureSet { textures }) = materials.get(set) {
                        diffuse = textures
                            .first()
                            .filter(|p| !p.is_empty())
                            .map(|p| texture_path(p))
                            .transpose()?;
                    }
                }
                Some(MaterialData::NoLighting {
                    texture, shader, ..
                }) => {
                    has_texture_property = true;
                    clamp = shader.clamp_mode;
                    raster.depth_test = shader.flags & 0x8000_0000 != 0;
                    raster.depth_write = shader.flags2 & 1 != 0;
                    if texture.is_empty() && shader.shader_type == 33 {
                        untextured_shader = true;
                    } else if !texture.is_empty() {
                        diffuse = Some(texture_path(texture)?);
                    }
                    report.warnings.push(format!(
                        "Mesh {}: NoLighting falloff/controllers are not evaluated",
                        object.block
                    ));
                }
                Some(MaterialData::Texturing { slots, .. }) => {
                    has_texture_property = true;
                    if let Some(Some(slot)) = slots.first() {
                        diffuse = scene
                            .textures
                            .iter()
                            .find(|t| Some(t.block) == slot.source)
                            .and_then(|t| t.asset_path.clone());
                        report.warnings.push(format!(
                            "Mesh {}: legacy texture transform/sampler approximated",
                            object.block
                        ));
                    }
                }
                Some(MaterialData::Material { alpha: value, .. }) => {
                    alpha = *value;
                    has_material = true;
                }
                Some(MaterialData::Alpha { flags, threshold }) => {
                    raster.alpha_flags = *flags;
                    raster.alpha_threshold = *threshold;
                }
                Some(MaterialData::Stencil { flags, .. }) => {
                    raster.draw_mode = ((flags >> 10) & 3) as u8;
                    if flags & 1 != 0 {
                        report.warnings.push(format!(
                            "Mesh {}: stencil buffer operations are not evaluated",
                            object.block
                        ));
                    }
                }
                Some(MaterialData::Shade { .. }) => {}
                _ => {
                    unknown_property = true;
                }
            }
        }
        raster.validate()?;
        let untextured =
            untextured_shader || (has_material && !has_texture_property && !unknown_property);
        let texture = if let Some(path) = diffuse {
            if source.uv_sets.is_empty() {
                return Err(format!("textured mesh {} has no UVs", object.block).into());
            }
            let id = input.load(textures, &path, clamp)?;
            used_textures.insert(id);
            Some(id)
        } else if untextured {
            None
        } else {
            report.warnings.push(format!(
                "Mesh {} has no supported diffuse texture; shown magenta",
                object.block
            ));
            None
        };
        let color = if texture.is_some() || untextured {
            Color::linear_rgba(1., 1., 1., alpha)
        } else {
            Color::srgba(1., 0., 1., alpha)
        };
        parts.push(Part {
            mesh,
            texture,
            color,
            raster,
        });
        report.bindings.push(BindingEvidence {
            mesh_block: object.block,
            diffuse_mode: if texture.is_some() {
                "archived-texture"
            } else if untextured {
                "authored-untextured"
            } else {
                "unsupported-magenta"
            },
            raster,
        });
        report.meshes += 1;
        report.vertices += source.vertices.len();
        report.triangles += source.triangles.len();
    }
    if parts.is_empty() {
        if selected_object.is_some() {
            return Err(
                "selected source object pose has no supported visible triangle mesh".into(),
            );
        }
        if selected_skin.is_some() {
            return Err("selected source skin has no supported visible triangle mesh".into());
        }
        return Err("model has no supported visible, unskinned triangle meshes".into());
    }
    if let Some(skin) = selected_skin {
        report.schema_version = 3;
        report.source_skin_pose = Some(Box::new(skin.summary));
        report.rendering = "unlit exact selected source-local skin; source-world map once; raw weights/linear normals, controllers and original playback unapplied; no retail parity";
    }
    if let Some(pose) = selected_object {
        report.schema_version = 3;
        report.source_object_pose = Some(Box::new(pose.summary));
        report.rendering = "unlit exact source-time translation/scale pose of selected object/static descendants; source world and basis once; raw linear normal directions; clock/rotation-key/playback fields unapplied, no retail parity";
    }
    report.bounds = [min.to_array(), max.to_array()];
    report.textures = used_textures
        .into_iter()
        .map(|id| textures.evidence[id].clone())
        .collect();
    let center = (min + max) * 0.5;
    let radius = (max - min).length().max(1.) * 0.5;
    Ok((
        Model {
            parts,
            center,
            radius,
        },
        report,
    ))
}

pub fn decode_diffuse(bytes: &[u8], clamp: u32) -> Result<Image> {
    decode_diffuse_bounded(bytes, clamp, u64::MAX, 128 * 1024 * 1024)
}

/// Bevy pads the base BC extent, then wgpu pads each mip to whole 4x4 blocks.
/// Count that physical footprint, including the final sub-block mip levels.
pub(crate) fn diffuse_physical_mip_pixels(width: u32, height: u32, levels: u32) -> Result<u64> {
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err("DDS exceeds preview dimensions".into());
    }
    if levels == 0 || levels > width.max(height).ilog2() + 1 {
        return Err("invalid DDS mip count".into());
    }
    let physical = |axis: u32| {
        axis.checked_add(3)
            .and_then(|value| (value / 4).checked_mul(4))
            .ok_or("DDS physical extent overflow")
    };
    let (width, height) = (physical(width)?, physical(height)?);
    (0..levels)
        .try_fold(0u64, |total, mip| {
            let pixels = u64::from(physical((width >> mip).max(1))?)
                .checked_mul(u64::from(physical((height >> mip).max(1))?))
                .ok_or("DDS physical mip texel count overflow")?;
            total
                .checked_add(pixels)
                .ok_or("DDS physical mip texel count overflow")
        })
        .map_err(Into::into)
}

/// The selected tile consumer has tighter input/mip-texel limits. Header,
/// format and payload interpretation remain in this single existing adapter.
pub fn decode_diffuse_bounded(
    bytes: &[u8],
    clamp: u32,
    max_pixels: u64,
    max_bytes: usize,
) -> Result<Image> {
    // Avoid trusting a DDS header with unbounded dimensions before it reaches
    // the image library or GPU. No custom pixel decompressor lives here.
    if bytes.len() < 128 || &bytes[..4] != b"DDS " {
        return Err("expected a DDS texture".into());
    }
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let (height, width) = (word(12), word(16));
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || bytes.len() > 128 * 1024 * 1024
        || bytes.len() > max_bytes
        || u64::from(width) * u64::from(height) > max_pixels
    {
        return Err("DDS exceeds preview dimensions or byte budget".into());
    }
    // Match pinned ddsfile's optional MIPMAPCOUNT field and Bevy's zero-to-one
    // rule. All accepted BC1/BC2/BC3 formats have 4x4 blocks; the existing image
    // decoder still owns format interpretation. Refuse before it retains data.
    let header_levels = if word(8) & 0x20000 != 0 {
        word(28).max(1)
    } else {
        1
    };
    if diffuse_physical_mip_pixels(width, height, header_levels)? > max_pixels {
        return Err("DDS exceeds selected image physical mip texel budget".into());
    }
    let wrap = ImageAddressMode::Repeat;
    let edge = ImageAddressMode::ClampToEdge;
    let (u, v) = match clamp {
        0 => (edge, edge),
        1 => (edge, wrap),
        2 => (wrap, edge),
        3 => (wrap, wrap),
        _ => return Err("unknown texture clamp mode".into()),
    };
    let image = Image::from_buffer(
        bytes,
        ImageType::Extension("dds"),
        CompressedImageFormats::BC,
        true,
        ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: u,
            address_mode_v: v,
            ..ImageSamplerDescriptor::linear()
        }),
        RenderAssetUsages::RENDER_WORLD,
    )?;
    if image.texture_descriptor.size.depth_or_array_layers != 1
        || image.texture_view_descriptor.is_some()
    {
        return Err("diffuse slot is not a plain 2D texture".into());
    }
    use bevy::render::render_resource::TextureFormat;
    let block_bytes = match image.texture_descriptor.format {
        TextureFormat::Bc1RgbaUnormSrgb => 8,
        TextureFormat::Bc2RgbaUnormSrgb | TextureFormat::Bc3RgbaUnormSrgb => 16,
        _ => return Err("preview currently supports BC1/BC2/BC3 diffuse textures".into()),
    };
    let levels = image.texture_descriptor.mip_level_count;
    if levels != header_levels
        || image.texture_descriptor.size.width != width.div_ceil(4) * 4
        || image.texture_descriptor.size.height != height.div_ceil(4) * 4
    {
        return Err("DDS decoded extent/mip count differs from bounded header".into());
    }
    if diffuse_physical_mip_pixels(
        image.texture_descriptor.size.width,
        image.texture_descriptor.size.height,
        levels,
    )? > max_pixels
    {
        return Err("DDS exceeds selected image physical mip texel budget".into());
    }
    let expected: usize = (0..levels)
        .map(|mip| {
            ((width >> mip).max(1).div_ceil(4) * (height >> mip).max(1).div_ceil(4)) as usize
                * block_bytes
        })
        .sum();
    if image.data.as_ref().map(Vec::len) != Some(expected) {
        return Err("DDS data length does not match its dimensions and mip count".into());
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authored_sources() -> ArchiveAssets {
        let install = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/preview-pose-test-install");
        std::fs::create_dir_all(install.join("Data")).unwrap();
        ArchiveAssets::open_nv(&install).unwrap()
    }

    #[test]
    fn exact_source_time_reaches_actual_mesh_without_reapplying_owner_world() {
        let bytes = include_bytes!("testdata/source-pose-triangle.packet");
        let assets = authored_sources();
        let path = AssetPath::new(b"authored/source-pose-triangle.nif").unwrap();
        for (time, positions, normals, world, indices) in [
            (
                0.,
                [[-6., -1., -5.], [-6., -1., -7.], [-6., -3., -5.]],
                [[2., 0., 0.]; 3],
                [[0., 0., 1., -6.], [1., 0., 0., 5.], [0., -1., 0., -1.]],
                [0, 2, 1],
            ),
            (
                3.,
                [[-6., 29., 7.], [-6., 29., 11.], [-6., 33., 7.]],
                [[-4., 0., 0.]; 3],
                [[0., 0., -2., -6.], [-2., 0., 0., -7.], [0., 2., 0., 29.]],
                [0, 1, 2],
            ),
        ] {
            let (model, report) = from_bytes_with_pose(
                &assets,
                &path,
                bytes,
                &mut Textures::default(),
                Some(crate::pose::Request::Object(crate::pose::ObjectRequest {
                    object: 1,
                    controller: 2,
                    source_time: time,
                })),
            )
            .unwrap();
            assert_eq!(model.parts.len(), 1);
            let mesh = &model.parts[0].mesh;
            assert!(matches!(mesh.attribute(Mesh::ATTRIBUTE_POSITION),
                Some(VertexAttributeValues::Float32x3(values)) if values == &positions));
            assert!(matches!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
                Some(VertexAttributeValues::Float32x3(values)) if values == &normals));
            assert!(matches!(mesh.indices(), Some(Indices::U32(values)) if values == &indices));
            let pose = report.source_object_pose.unwrap();
            assert_eq!(pose.draw_meshes[0].source_world, world);
            assert_eq!(pose.evaluation.requested_time_f64_bits, time.to_bits());
            assert_eq!(pose.evaluation.unapplied_controller_fields.flags, 0xffff);
            assert!(!pose.evaluation.retail_behavior_verified);
            assert_eq!(report.bindings[0].diffuse_mode, "authored-untextured");
            assert_eq!(report.bindings[0].raster.draw_mode, 3);
            assert_eq!(report.schema_version, 3);
        }
    }

    #[test]
    fn source_pose_draw_refuses_unapplied_descendant_and_singular_sample() {
        let assets = authored_sources();
        let path = AssetPath::new(b"authored/source-pose-triangle.nif").unwrap();
        for (bytes, time, expected) in [
            (
                include_bytes!("testdata/source-pose-controlled-descendant.packet").as_slice(),
                0.,
                "descendant 5 has an unapplied controller",
            ),
            (
                include_bytes!("testdata/source-pose-triangle.packet").as_slice(),
                1.,
                "singular or unrepresentable transform",
            ),
        ] {
            let error = from_bytes_with_pose(
                &assets,
                &path,
                bytes,
                &mut Textures::default(),
                Some(crate::pose::Request::Object(crate::pose::ObjectRequest {
                    object: 1,
                    controller: 2,
                    source_time: time,
                })),
            )
            .err()
            .expect("Unsupported source pose must not yield a model");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    fn dds(four_cc: &[u8; 4], payload_bytes: usize) -> Vec<u8> {
        let mut bytes = vec![0; 128 + payload_bytes];
        bytes[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [
            (4, 124u32),
            (8, 0xa1007),
            (12, 4),
            (16, 4),
            (20, payload_bytes as u32),
            (28, 1),
            (76, 32),
            (80, 4),
            (108, 0x401008),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[84..88].copy_from_slice(four_cc);
        bytes
    }

    #[test]
    fn dds_upload_rejects_missing_mips_and_oversized_headers() {
        for (format, size) in [(b"DXT1", 8), (b"DXT3", 16), (b"DXT5", 16)] {
            let mut bytes = dds(format, size);
            assert!(decode_diffuse(&bytes, 3).is_ok());
            bytes.pop();
            assert!(decode_diffuse(&bytes, 3).is_err());
        }
        let mut bytes = dds(b"DXT1", 8);
        bytes[28..32].copy_from_slice(&4u32.to_le_bytes());
        assert!(decode_diffuse(&bytes, 3).is_err());
        bytes[16..20].copy_from_slice(&32768u32.to_le_bytes());
        assert!(decode_diffuse(&bytes, 3).is_err());
        assert!(decode_diffuse(&[], 3).is_err());
    }

    #[test]
    fn presentation_rotation_preserves_triangle_front_and_source_units() {
        let points = [
            Vec3::new(2., 3., 4.),
            Vec3::new(5., 3., 4.),
            Vec3::new(2., 8., 4.),
        ];
        let converted = points.map(basis);
        assert_eq!(converted[0], Vec3::new(2., 4., -3.));
        assert_eq!((converted[1] - converted[0]).length(), 3.);
        let front = (converted[1] - converted[0])
            .cross(converted[2] - converted[0])
            .normalize();
        assert_eq!(front, Vec3::Y);
    }
}

#[cfg(test)]
mod resident_tests;
