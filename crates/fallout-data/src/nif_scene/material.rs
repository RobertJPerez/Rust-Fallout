//! Source material fields. Interpreting a flag is separate from implementing its shader.
use super::{Scene, cursor::Reader};
use crate::{Result, malformed, nif::NifIndex, vfs::AssetPath};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ObjectHeader {
    pub name: Option<u32>,
    pub extra_data: Vec<Option<u32>>,
    pub controller: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct MaterialBlock {
    pub block: u32,
    pub object: Option<ObjectHeader>,
    pub data: MaterialData,
}

#[derive(Debug, Serialize)]
pub struct Shader {
    pub shade_flags: u16,
    pub shader_type: u32,
    pub flags: u32,
    pub flags2: u32,
    pub environment_scale: f32,
    pub clamp_mode: u32,
}

#[derive(Debug, Serialize)]
pub struct TextureTransform {
    pub translation: [f32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
    pub method: u32,
    pub center: [f32; 2],
}

#[derive(Debug, Serialize)]
pub struct TextureSlot {
    pub source: Option<u32>,
    pub flags: u16,
    pub transform: Option<TextureTransform>,
}

#[derive(Debug, Serialize)]
pub struct ShaderTexture {
    pub texture: TextureSlot,
    pub map_id: u32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MaterialData {
    Material {
        ambient: Option<[f32; 3]>,
        diffuse: Option<[f32; 3]>,
        specular: [f32; 3],
        emissive: [f32; 3],
        glossiness: f32,
        alpha: f32,
        emissive_multiplier: Option<f32>,
    },
    Alpha {
        flags: u16,
        threshold: u8,
    },
    Stencil {
        flags: u16,
        reference: u32,
        mask: u32,
    },
    Shade {
        flags: u16,
    },
    PerPixelLighting {
        shader: Shader,
        texture_set: Option<u32>,
        refraction_strength: Option<f32>,
        refraction_period: Option<i32>,
        parallax_passes: Option<f32>,
        parallax_scale: Option<f32>,
    },
    NoLighting {
        shader: Shader,
        texture: Vec<u8>,
        falloff: Option<[f32; 4]>,
    },
    TextureSet {
        textures: Vec<Vec<u8>>,
    },
    SourceTexture {
        external: u8,
        filename: Option<u32>,
        pixel_data: Option<u32>,
        format_preferences: [u32; 3],
        is_static: u8,
        direct_render: bool,
        persist_render_data: bool,
    },
    Texturing {
        flags: u16,
        texture_count: u32,
        slots: Vec<Option<TextureSlot>>,
        bump_luma: Option<[f32; 2]>,
        bump_matrix: Option<[f32; 4]>,
        parallax_offset: Option<f32>,
        shader_textures: Vec<Option<ShaderTexture>>,
    },
}

pub(super) fn supports(name: &str) -> bool {
    matches!(
        name,
        "NiMaterialProperty"
            | "NiAlphaProperty"
            | "NiStencilProperty"
            | "NiShadeProperty"
            | "BSShaderPPLightingProperty"
            | "BSShaderNoLightingProperty"
            | "BSShaderTextureSet"
            | "NiSourceTexture"
            | "NiTexturingProperty"
    )
}

fn shader(r: &mut Reader<'_>) -> Result<Shader> {
    Ok(Shader {
        shade_flags: r.u16()?,
        shader_type: r.u32()?,
        flags: r.u32()?,
        flags2: r.u32()?,
        environment_scale: r.float()?,
        clamp_mode: r.u32()?,
    })
}

fn texture_slot(r: &mut Reader<'_>) -> Result<Option<TextureSlot>> {
    if !r.boolean()? {
        return Ok(None);
    }
    let source = r.reference()?;
    let flags = r.u16()?;
    let transform = if r.boolean()? {
        Some(TextureTransform {
            translation: r.vector()?,
            scale: r.vector()?,
            rotation: r.float()?,
            method: r.u32()?,
            center: r.vector()?,
        })
    } else {
        None
    };
    Ok(Some(TextureSlot {
        source,
        flags,
        transform,
    }))
}

pub(super) fn read(r: &mut Reader<'_>, block: u32, name: &str) -> Result<MaterialBlock> {
    let stream = r.index.bethesda_version;
    let object = if name == "BSShaderTextureSet" {
        None
    } else {
        Some(ObjectHeader {
            name: r.string()?,
            extra_data: r.references()?,
            controller: r.reference()?,
        })
    };
    let data = match name {
        "NiMaterialProperty" => MaterialData::Material {
            ambient: if stream < 26 { Some(r.vector()?) } else { None },
            diffuse: if stream < 26 { Some(r.vector()?) } else { None },
            specular: r.vector()?,
            emissive: r.vector()?,
            glossiness: r.float()?,
            alpha: r.float()?,
            emissive_multiplier: if stream > 21 { Some(r.float()?) } else { None },
        },
        "NiAlphaProperty" => MaterialData::Alpha {
            flags: r.u16()?,
            threshold: r.u8()?,
        },
        "NiStencilProperty" => MaterialData::Stencil {
            flags: r.u16()?,
            reference: r.u32()?,
            mask: r.u32()?,
        },
        "NiShadeProperty" => MaterialData::Shade { flags: r.u16()? },
        "BSShaderPPLightingProperty" => MaterialData::PerPixelLighting {
            shader: shader(r)?,
            texture_set: r.reference()?,
            refraction_strength: if stream > 14 { Some(r.float()?) } else { None },
            refraction_period: if stream > 14 {
                Some(r.u32()? as i32)
            } else {
                None
            },
            parallax_passes: if stream > 24 { Some(r.float()?) } else { None },
            parallax_scale: if stream > 24 { Some(r.float()?) } else { None },
        },
        "BSShaderNoLightingProperty" => MaterialData::NoLighting {
            shader: shader(r)?,
            texture: r.byte_string()?,
            falloff: if stream > 26 { Some(r.vector()?) } else { None },
        },
        "BSShaderTextureSet" => {
            let count = r.u32()? as usize;
            r.budget(count, 4)?;
            r.reserve::<Vec<u8>>(count)?;
            let mut textures = Vec::with_capacity(count);
            for _ in 0..count {
                textures.push(r.byte_string()?);
            }
            MaterialData::TextureSet { textures }
        }
        "NiSourceTexture" => {
            let external = r.u8()?;
            if external > 1 {
                return Err(r.fail("invalid source texture external flag"));
            }
            MaterialData::SourceTexture {
                external,
                filename: r.string()?,
                pixel_data: r.reference()?,
                format_preferences: [r.u32()?, r.u32()?, r.u32()?],
                is_static: r.u8()?,
                direct_render: r.boolean()?,
                persist_render_data: r.boolean()?,
            }
        }
        "NiTexturingProperty" => {
            let flags = r.u16()?;
            let texture_count = r.u32()?;
            // Five slots are unconditional in this version. Later slots have
            // individual count gates; this is not a count-sized array on disk.
            let slots_count = texture_count.clamp(5, 12) as usize;
            r.reserve::<Option<TextureSlot>>(slots_count)?;
            let mut slots = Vec::with_capacity(slots_count);
            let (mut bump_luma, mut bump_matrix, mut parallax_offset) = (None, None, None);
            for slot in 0..slots_count {
                let texture = texture_slot(r)?;
                if texture.is_some() && slot == 5 {
                    bump_luma = Some(r.vector()?);
                    bump_matrix = Some(r.vector()?);
                }
                if texture.is_some() && slot == 7 {
                    parallax_offset = Some(r.float()?);
                }
                slots.push(texture);
            }
            let count = r.u32()? as usize;
            r.budget(count, 1)?;
            r.reserve::<Option<ShaderTexture>>(count)?;
            let mut shader_textures = Vec::with_capacity(count);
            for _ in 0..count {
                shader_textures.push(match texture_slot(r)? {
                    Some(texture) => Some(ShaderTexture {
                        texture,
                        map_id: r.u32()?,
                    }),
                    None => None,
                });
            }
            MaterialData::Texturing {
                flags,
                texture_count,
                slots,
                bump_luma,
                bump_matrix,
                parallax_offset,
                shader_textures,
            }
        }
        _ => return Err(r.fail("material dispatch has no matching schema")),
    };
    Ok(MaterialBlock {
        block,
        object,
        data,
    })
}

#[derive(Debug, Serialize)]
pub struct TextureReference {
    pub block: u32,
    pub slot: usize,
    pub raw_path: Vec<u8>,
    pub asset_path: Option<AssetPath>,
    pub error: Option<String>,
}

/// Keep the authored bytes alongside lookup normalization. Absolute exporter
/// paths are reported, never opened or silently trimmed into a different asset.
pub fn texture_path(raw: &[u8]) -> Result<AssetPath> {
    if raw.len() > 4096 {
        return Err(crate::Error::Unsupported(
            "texture path exceeds 4096 bytes".into(),
        ));
    }
    let path = AssetPath::new(raw)?;
    if path.bytes().starts_with(b"textures/") {
        return Ok(path);
    }
    let mut rooted = b"textures/".to_vec();
    rooted.extend(path.bytes());
    AssetPath::new(&rooted)
}

pub(super) fn resolve(
    scene: &mut Scene,
    index: &NifIndex,
    source: &str,
    array_bytes_left: &mut usize,
) -> Result<()> {
    let block_type =
        |id: u32| index.block_types[index.blocks[id as usize].type_index as usize].as_str();
    let check = |owner: u32, target: Option<u32>, expected: &str| -> Result<()> {
        if let Some(target) = target
            && block_type(target) != expected
        {
            return Err(malformed(
                source,
                index.blocks[owner as usize].offset as u64,
                format!(
                    "material reference expects {expected}, found {}",
                    block_type(target)
                ),
            ));
        }
        Ok(())
    };
    for material in &scene.materials {
        let block = material.block;
        let mut paths: Vec<&[u8]> = Vec::new();
        match &material.data {
            MaterialData::PerPixelLighting { texture_set, .. } => {
                check(block, *texture_set, "BSShaderTextureSet")?
            }
            MaterialData::Texturing {
                slots,
                shader_textures,
                ..
            } => {
                for slot in slots
                    .iter()
                    .flatten()
                    .chain(shader_textures.iter().flatten().map(|s| &s.texture))
                {
                    check(block, slot.source, "NiSourceTexture")?;
                }
            }
            MaterialData::TextureSet { textures } => {
                paths.extend(textures.iter().map(Vec::as_slice))
            }
            MaterialData::NoLighting { texture, .. } => paths.push(texture),
            MaterialData::SourceTexture {
                external: 1,
                filename: Some(name),
                ..
            } => paths.push(&index.strings[*name as usize]),
            _ => {}
        }
        for (slot, raw) in paths.into_iter().enumerate() {
            if raw.is_empty() {
                continue;
            }
            // A string-table path may be referenced many times. Bound the owned
            // copies and error text too, not just the serialized material arrays.
            let charge = raw
                .len()
                .checked_mul(8)
                .and_then(|n| n.checked_add(std::mem::size_of::<TextureReference>()));
            *array_bytes_left = charge
                .and_then(|n| array_bytes_left.checked_sub(n))
                .ok_or_else(|| {
                    malformed(
                        source,
                        index.blocks[block as usize].offset as u64,
                        "texture reference storage budget exceeded",
                    )
                })?;
            let (asset_path, error) = match texture_path(raw) {
                Ok(path) => (Some(path), None),
                Err(e) => (None, Some(e.to_string())),
            };
            scene.textures.push(TextureReference {
                block,
                slot,
                raw_path: raw.to_vec(),
                asset_path,
                error,
            });
        }
    }
    Ok(())
}
