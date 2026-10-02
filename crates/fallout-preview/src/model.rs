//! Adapt decoded source data for inspection. Bethesda shader behavior belongs in
//! a separate renderer; this view uses diffuse textures and an unlit material.
use crate::material::Raster;
use bevy::{
    asset::RenderAssetUsages,
    image::{
        CompressedImageFormats, ImageAddressMode, ImageSampler, ImageSamplerDescriptor, ImageType,
    },
    mesh::Indices,
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
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

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
    fn load(&mut self, assets: &ArchiveAssets, path: &AssetPath, clamp: u32) -> Result<usize> {
        let key = (path.clone(), clamp);
        if let Some(id) = self.ids.get(&key) {
            return Ok(*id);
        }
        let (_, data) = assets.read_unique(path)?;
        if self.images.len() >= 4096
            || data.len() > (256 * 1024 * 1024usize).saturating_sub(self.bytes)
        {
            return Err("scene texture budget exceeded".into());
        }
        let image = decode_diffuse(&data, clamp)?;
        let size = image.texture_descriptor.size;
        self.evidence.push(TextureEvidence {
            path: path.clone(),
            sha256: format!("{:x}", Sha256::digest(&data)),
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
) -> Result<(Model, Report)> {
    let (_, bytes) = assets.read_unique(path)?;
    let (index, scene) = nif_scene::decode(&bytes, &String::from_utf8_lossy(path.bytes()))?;
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
        model_sha256: format!("{:x}", Sha256::digest(&bytes)),
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
    };
    for (kind, blocks) in &scene.unsupported_blocks {
        report
            .warnings
            .push(format!("{} unsupported {kind} blocks", blocks.len()));
    }
    let mut parts = Vec::new();
    let mut used_textures = BTreeSet::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for object in &scene.objects {
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
        if skin.is_some() {
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
        let matrix = affine(world.matrix);
        if !matrix.is_finite() || matrix.determinant().abs() < 1e-12 {
            return Err(format!(
                "mesh {} has a singular or unrepresentable transform",
                object.block
            )
            .into());
        }
        let positions: Vec<[f32; 3]> = source
            .vertices
            .iter()
            .map(|v| {
                let p = basis(matrix.transform_point3(Vec3::from_array(*v)));
                min = min.min(p);
                max = max.max(p);
                p.to_array()
            })
            .collect();
        if positions.iter().flatten().any(|v| !v.is_finite()) {
            return Err("preview positions overflowed f32".into());
        }
        let mut indices = Vec::with_capacity(source.triangles.len() * 3);
        for triangle in &source.triangles {
            let [a, b, c] = triangle.map(u32::from);
            indices.extend(if matrix.determinant() < 0. {
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
        if !source.normals.is_empty() {
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
            let id = textures.load(assets, &path, clamp)?;
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
        return Err("model has no supported visible, unskinned triangle meshes".into());
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
    {
        return Err("DDS exceeds preview dimensions or byte budget".into());
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
    if levels == 0 || levels > width.max(height).ilog2() + 1 {
        return Err("invalid DDS mip count".into());
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
