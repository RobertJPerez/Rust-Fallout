//! Authored NV skin sources. Exact bits and unresolved dependencies are retained;
//! source decoding does not establish pose evaluation or gameplay skinning.
pub mod binding;
pub mod bounds;
pub mod external;
mod graph;
pub mod influences;
pub mod partition;
pub mod pose;
mod read;
pub(crate) mod storage;
pub mod streams;

use crate::{Error, Result, nif, nif_scene, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub scene: nif_scene::Limits,
    /// Separate from the existing scene decoder's array budget.
    pub skin_array_bytes: usize,
    /// Bound repeated source-index checks when several meshes share skin data.
    pub weight_index_checks: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            scene: nif_scene::Limits::default(),
            skin_array_bytes: 128 * 1024 * 1024,
            weight_index_checks: 16_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Transform {
    /// Nine source floats, grouped into the stored triples without transposition.
    pub rotation_bits: [[u32; 3]; 3],
    pub translation_bits: [u32; 3],
    pub scale_bits: u32,
}

#[derive(Debug, Serialize)]
pub struct Weight {
    pub vertex: u16,
    pub weight_bits: u32,
}

#[derive(Debug, Serialize)]
pub struct Bone {
    pub transform: Transform,
    pub center_bits: [u32; 3],
    pub radius_bits: u32,
    /// Retained even if weights are absent; nifly zeroes this in that branch.
    pub declared_vertices: u16,
    pub weights: Vec<Weight>,
}

#[derive(Debug, Serialize)]
pub struct BodyPart {
    pub flags: u16,
    pub body_part: u16,
}

#[derive(Debug, Serialize)]
pub struct Instance {
    pub data: Option<u32>,
    pub partition: Option<u32>,
    pub skeleton_root: Option<u32>,
    pub bones: Vec<Option<u32>>,
    /// None for NiSkinInstance, Some (including empty) for the dismember subtype.
    pub body_parts: Option<Vec<BodyPart>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    SkinData {
        transform: Transform,
        /// Interpret zero/nonzero for layout only. Never canonicalize the byte.
        has_vertex_weights: u8,
        bones: Vec<Bone>,
    },
    Instance {
        instance: Instance,
    },
}

#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: String,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub data: Data,
}

#[derive(Debug, Serialize)]
pub struct Owner {
    pub geometry: u32,
    pub instance: u32,
    pub geometry_data: Option<u32>,
    pub vertex_count: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Dependency {
    MissingData { instance: u32 },
    MissingSkeletonRoot { instance: u32 },
    MissingBone { instance: u32, ordinal: usize },
    UndecodedNode { instance: u32, target: u32 },
    PartitionPayload { instance: u32, target: u32 },
    UnownedInstance { instance: u32 },
    UnownedData { data: u32 },
    MissingGeometryData { geometry: u32 },
}

#[derive(Debug, Serialize)]
pub struct Skin {
    pub blocks: Vec<Block>,
    pub owners: Vec<Owner>,
    pub dependencies: Vec<Dependency>,
    /// Charged vector element storage; block tables/maps are also count-bounded.
    pub retained_bytes: usize,
    pub runtime_ready: bool,
}

pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Skin)> {
    decode_with_limits(bytes, source, Limits::default())
}

/// Always derive both the index and geometry owners from this input. Public
/// callers cannot pass an invented block table or stale owner vertex counts.
pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Skin)> {
    let (index, skin, _) = decode_with_scene(bytes, source, limits)?;
    Ok((index, skin))
}

// Private source access for binding; public callers still supply only bytes and
// cannot combine unrelated scene/index/skin catalogues.
fn decode_with_scene(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Skin, nif_scene::Scene)> {
    let (index, scene) = nif_scene::decode_with_limits(bytes, source, limits.scene)?;
    let mut remaining = limits.skin_array_bytes;
    let mut skin = Skin {
        blocks: Vec::new(),
        owners: Vec::new(),
        dependencies: Vec::new(),
        retained_bytes: 0,
        runtime_ready: false,
    };
    for (id, block) in index.blocks.iter().enumerate() {
        let name = index.block_types[block.type_index as usize].as_str();
        if !matches!(
            name,
            "NiSkinData" | "NiSkinInstance" | "BSDismemberSkinInstance"
        ) {
            continue;
        }
        let payload = &bytes[block.offset..block.offset + block.bytes];
        let mut reader = Reader {
            data: payload,
            base: block.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut remaining,
        };
        reader.reserve::<Block>(1)?;
        reader.reserve::<u8>(name.len() + 64)?;
        let data = if name == "NiSkinData" {
            read::skin_data(&mut reader)?
        } else {
            Data::Instance {
                instance: read::instance(&mut reader, name == "BSDismemberSkinInstance")?,
            }
        };
        reader.finish()?;
        skin.blocks.push(Block {
            block: id as u32,
            block_type: name.into(),
            offset: block.offset,
            bytes: block.bytes,
            sha256: format!("{:x}", Sha256::digest(payload)),
            data,
        });
    }
    graph::resolve(
        &mut skin,
        &index,
        &scene,
        source,
        &mut remaining,
        limits.weight_index_checks,
    )?;
    skin.retained_bytes = limits.skin_array_bytes - remaining;
    Ok((index, skin, scene))
}

fn reserve<T>(remaining: &mut usize, count: usize, source: &str) -> Result<()> {
    *remaining = count
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|bytes| remaining.checked_sub(bytes))
        .ok_or_else(|| {
            Error::Unsupported(format!("{source}: skin catalogue storage budget exceeded"))
        })?;
    Ok(())
}

// Shared raw policy checks. They return details so each existing consumer keeps
// its own exact source/error context, without accepting public decoded weights.
fn raw_weight(bits: u32) -> std::result::Result<f64, &'static str> {
    let value = f64::from(f32::from_bits(bits));
    if !value.is_finite() || value < 0. {
        Err("negative or nonfinite raw weight")
    } else {
        Ok(value)
    }
}

fn weight_sum_error(vertex: usize, sum: f64, policy: pose::WeightPolicy) -> Option<String> {
    if !sum.is_finite() || sum <= 0. {
        return Some(format!("vertex {vertex} has no positive finite weight sum"));
    }
    if let pose::WeightPolicy::RequireUnitSum { absolute_tolerance } = policy
        && (sum - 1.).abs() > absolute_tolerance
    {
        return Some(format!(
            "vertex {vertex} raw weight sum {sum} exceeds declared tolerance {absolute_tolerance}"
        ));
    }
    None
}
