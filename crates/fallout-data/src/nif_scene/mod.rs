//! NV scene and triangle payloads in source coordinates. This is a decoder, not
//! a renderer: controller evaluation, skinning, shaders and collision stay explicit.
pub(crate) mod cursor;
mod graph;
pub mod material;
mod mesh;

use crate::{Error, Result, nif};
use cursor::Reader;
pub use graph::WorldTransform;
pub use mesh::{MeshData, Topology};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input_bytes: usize,
    pub blocks: usize,
    /// Aggregate element storage for payload arrays, including expanded topology.
    /// Object tables and traversal storage are bounded separately by `blocks`.
    pub array_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 64 * 1024 * 1024,
            blocks: 100_000,
            array_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Transform {
    pub translation: [f32; 3],
    /// Stored triples, interpreted as rows by the independent nifly transform API.
    /// Keep source coordinates here; presentation-axis conversion belongs elsewhere.
    pub rotation: [[f32; 3]; 3],
    pub scale: f32,
}

#[derive(Debug, Serialize)]
pub struct Object {
    pub block: u32,
    pub name: Option<u32>,
    pub extra_data: Vec<Option<u32>>,
    pub controller: Option<u32>,
    pub flags: u32,
    pub transform: Transform,
    pub properties: Vec<Option<u32>>,
    pub collision: Option<u32>,
    pub kind: ObjectKind,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectKind {
    Node {
        children: Vec<Option<u32>>,
        effects: Vec<Option<u32>>,
    },
    Mesh {
        data: Option<u32>,
        skin: Option<u32>,
        material_names: Vec<Option<u32>>,
        material_extra: Vec<i32>,
        active_material: i32,
        material_needs_update: bool,
    },
}

#[derive(Debug, Serialize)]
pub struct UnsupportedEdge {
    pub parent: Option<u32>,
    pub target: u32,
    pub block_type: String,
}

#[derive(Debug, Serialize)]
pub struct ExtraFlags {
    pub block: u32,
    pub name: Option<u32>,
    /// Preserve every bit; interpreting animation/collision flags is separate work.
    pub value: u32,
}

#[derive(Debug, Serialize)]
pub struct Scene {
    pub objects: Vec<Object>,
    pub meshes: Vec<MeshData>,
    pub materials: Vec<material::MaterialBlock>,
    pub textures: Vec<material::TextureReference>,
    pub extra_flags: Vec<ExtraFlags>,
    pub world_transforms: Vec<WorldTransform>,
    pub unsupported_blocks: BTreeMap<String, Vec<u32>>,
    pub unsupported_scene_edges: Vec<UnsupportedEdge>,
    pub runtime_ready: bool,
}

fn object(r: &mut Reader<'_>, block: u32, node: bool) -> Result<Object> {
    let name = r.string()?;
    let extra_data = r.references()?;
    let controller = r.reference()?;
    let flags = if r.index.bethesda_version <= 26 {
        u32::from(r.u16()?)
    } else {
        r.u32()?
    };
    let translation = r.vector()?;
    let rotation = [r.vector()?, r.vector()?, r.vector()?];
    let scale = r.float()?;
    let transform = Transform {
        translation,
        rotation,
        scale,
    };
    let properties = r.references()?;
    let collision = r.reference()?;
    let kind = if node {
        ObjectKind::Node {
            children: r.references()?,
            effects: r.references()?,
        }
    } else {
        let data = r.reference()?;
        let skin = r.reference()?;
        let count = r.u32()? as usize;
        r.budget(count, 8)?;
        r.reserve::<Option<u32>>(count)?;
        r.reserve::<i32>(count)?;
        let material_names = (0..count).map(|_| r.string()).collect::<Result<Vec<_>>>()?;
        let material_extra = (0..count)
            .map(|_| r.u32().map(|n| n as i32))
            .collect::<Result<Vec<_>>>()?;
        ObjectKind::Mesh {
            data,
            skin,
            material_names,
            material_extra,
            active_material: r.u32()? as i32,
            material_needs_update: r.boolean()?,
        }
    };
    Ok(Object {
        block,
        name,
        extra_data,
        controller,
        flags,
        transform,
        properties,
        collision,
        kind,
    })
}

/// Parse the container and the supported payloads together so a stale or invented
/// index cannot direct a public caller outside the input bytes.
pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Scene)> {
    decode_with_limits(bytes, source, Limits::default())
}

pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Scene)> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Unsupported(
            "NIF scene exceeds its input byte budget".into(),
        ));
    }
    let index = nif::inspect(bytes, source)?;
    if index.blocks.len() > limits.blocks {
        return Err(Error::Unsupported(
            "NIF scene exceeds its block count budget".into(),
        ));
    }
    let mut scene = Scene {
        objects: Vec::new(),
        meshes: Vec::new(),
        materials: Vec::new(),
        textures: Vec::new(),
        extra_flags: Vec::new(),
        world_transforms: Vec::new(),
        unsupported_blocks: BTreeMap::new(),
        unsupported_scene_edges: Vec::new(),
        runtime_ready: false,
    };
    let mut array_bytes_left = limits.array_bytes;
    for (id, block) in index.blocks.iter().enumerate() {
        let mut r = Reader {
            data: &bytes[block.offset..block.offset + block.bytes],
            base: block.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut array_bytes_left,
        };
        let name = index.block_types[block.type_index as usize].as_str();
        match name {
            "BSXFlags" => scene.extra_flags.push(ExtraFlags {
                block: id as u32,
                name: r.string()?,
                value: r.u32()?,
            }),
            "NiNode" | "BSFadeNode" => scene.objects.push(object(&mut r, id as u32, true)?),
            "NiTriShape" | "NiTriStrips" => scene.objects.push(object(&mut r, id as u32, false)?),
            "NiTriShapeData" | "NiTriStripsData" => {
                scene
                    .meshes
                    .push(mesh::read(&mut r, id as u32, name == "NiTriStripsData")?)
            }
            name if material::supports(name) => scene
                .materials
                .push(material::read(&mut r, id as u32, name)?),
            _ => {
                scene
                    .unsupported_blocks
                    .entry(name.into())
                    .or_default()
                    .push(id as u32);
                continue;
            }
        }
        r.finish()?;
    }
    graph::resolve(&mut scene, &index, source)?;
    material::resolve(&mut scene, &index, source, &mut array_bytes_left)?;
    Ok((index, scene))
}
