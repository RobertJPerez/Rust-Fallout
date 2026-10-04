//! Bounded NV hardware-partition source fields. The original three-block decoder
//! remains available independently; no weights or authored faces are regenerated.
mod graph;
mod read;
pub mod streams;

use crate::{Result, nif, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub skin: super::Limits,
    pub array_bytes: usize,
    pub index_checks: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            skin: super::Limits::default(),
            array_bytes: 128 * 1024 * 1024,
            index_checks: 16_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Partition {
    pub num_vertices: u16,
    pub num_triangles: u16,
    pub num_bones: u16,
    pub num_strips: u16,
    pub weights_per_vertex: u16,
    pub bone_palette: Vec<u16>,
    pub has_vertex_map: u8,
    pub vertex_map: Vec<u16>,
    pub has_vertex_weights: u8,
    /// Flat vertex-major array with the exact declared width; no normalization.
    pub weight_bits: Vec<u32>,
    pub strip_lengths: Vec<u16>,
    pub has_faces: u8,
    pub strips: Vec<Vec<u16>>,
    pub triangles: Vec<[u16; 3]>,
    pub has_bone_indices: u8,
    pub bone_indices: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: &'static str,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub partitions: Vec<Partition>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Dependency {
    UnownedPartition {
        partition: u32,
    },
    MissingOwner {
        partition: u32,
        instance: u32,
    },
    UnknownGeometry {
        partition: u32,
        geometry: u32,
    },
    AbsentArrays {
        partition: u32,
        ordinal: usize,
        fields: u8,
    },
    BodyPartCountMismatch {
        partition: u32,
        instance: u32,
        body_parts: usize,
        partitions: usize,
    },
}

#[derive(Debug, Serialize)]
pub struct Catalogue {
    pub blocks: Vec<Block>,
    pub dependencies: Vec<Dependency>,
    /// Charged retained records/arrays; temporary count-bounded relation maps
    /// and allocator capacity/overhead are excluded (see docs/nif-skin.md).
    pub retained_bytes: usize,
    pub runtime_ready: bool,
}

#[derive(Debug, Serialize)]
pub struct Source {
    pub skin: super::Skin,
    pub partitions: Catalogue,
}

pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Source)> {
    decode_with_limits(bytes, source, Limits::default())
}

pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source)> {
    let (index, source, _) = decode_with_scene(bytes, source, limits)?;
    Ok((index, source))
}

pub(super) fn decode_with_scene(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source, crate::nif_scene::Scene)> {
    let (index, mut skin, scene) = super::decode_with_scene(bytes, source, limits.skin)?;
    let mut remaining = limits.array_bytes;
    let mut catalogue = Catalogue {
        blocks: Vec::new(),
        dependencies: Vec::new(),
        retained_bytes: 0,
        runtime_ready: false,
    };
    for (id, block) in index.blocks.iter().enumerate() {
        if index.block_types[block.type_index as usize] != "NiSkinPartition" {
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
        reader.reserve::<u8>(64)?;
        let partitions = read::partitions(&mut reader)?;
        reader.finish()?;
        catalogue.blocks.push(Block {
            block: id as u32,
            block_type: "NiSkinPartition",
            offset: block.offset,
            bytes: block.bytes,
            sha256: format!("{:x}", Sha256::digest(payload)),
            partitions,
        });
    }
    graph::resolve(
        &mut catalogue,
        &skin,
        source,
        &mut remaining,
        limits.index_checks,
    )?;
    catalogue.retained_bytes = limits.array_bytes - remaining;
    // This combined entrypoint has decoded all partition payloads. Its source
    // catalogue can retire that first-slice dependency, without changing the
    // original entrypoint or admitting pose evaluation.
    skin.dependencies
        .retain(|dependency| !matches!(dependency, super::Dependency::PartitionPayload { .. }));
    Ok((
        index,
        Source {
            skin,
            partitions: catalogue,
        },
        scene,
    ))
}
