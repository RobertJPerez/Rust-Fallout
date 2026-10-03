//! Exact NiTransformData source fields; interpolation/angles/poses unverified.
mod read;
use super::{Animation, Dependency, LinkRole};
use crate::{Result, nif, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub animation: super::Limits,
    /// Additional key catalogue storage, separate from the original index and
    /// four-class animation construction budget.
    pub array_bytes: usize,
    /// Logical animation/index plus key retention admission. Animation scratch
    /// is also admitted under this cap; allocator overhead is not measured.
    pub max_combined_retained_bytes: usize,
    /// One unit per selected block, rotation/group product and stored float word.
    /// Array float-word work is admitted before its storage allocation.
    pub key_work: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            animation: super::Limits::default(),
            array_bytes: 128 * 1024 * 1024,
            max_combined_retained_bytes: 256 * 1024 * 1024,
            key_work: 16_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(bound(serialize = "[u32; N]: Serialize"))]
pub struct Key<const N: usize> {
    pub time_bits: u32,
    pub value_bits: [u32; N],
    pub forward_bits: Option<[u32; N]>,
    pub backward_bits: Option<[u32; N]>,
    pub tbc_bits: Option<[u32; 3]>,
}
#[derive(Debug, Serialize)]
#[serde(bound(serialize = "[u32; N]: Serialize"))]
pub struct Group<const N: usize> {
    pub declared_keys: u32,
    /// None means the tag is absent in the source's zero-count layout.
    pub key_type: Option<u32>,
    pub keys: Vec<Key<N>>,
}
#[derive(Debug, Serialize)]
pub struct QuaternionKey {
    pub time_bits: u32,
    pub value_wxyz_bits: [u32; 4],
    pub tbc_bits: Option<[u32; 3]>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "layout", rename_all = "snake_case")]
pub enum Rotation {
    Absent,
    Quaternion {
        key_type: u32,
        keys: Vec<QuaternionKey>,
    },
    /// Source X,Y,Z order; no angle convention is assigned.
    Xyz {
        axes: [Group<1>; 3],
    },
}
#[derive(Debug, Serialize)]
pub struct TransformKeys {
    pub declared_rotation_keys: u32,
    pub rotation: Rotation,
    pub translations: Group<3>,
    pub scales: Group<1>,
}
#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: &'static str,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub data: TransformKeys,
}
#[derive(Debug, Serialize)]
pub struct Catalogue {
    pub blocks: Vec<Block>,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub runtime_ready: bool,
}
#[derive(Debug, Serialize)]
pub struct Source {
    pub animation: Animation,
    pub keys: Catalogue,
}
pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Source)> {
    decode_with_limits(bytes, source, Limits::default())
}
pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source)> {
    // One immutable index construction, supplied by the existing approved
    // animation decoder. No public caller can pass a forged/stale index.
    let animation_limits = super::Limits {
        array_bytes: limits
            .animation
            .array_bytes
            .min(limits.max_combined_retained_bytes),
        ..limits.animation
    };
    let (index, mut animation) = super::decode_with_limits(bytes, source, animation_limits)?;
    let count = index
        .block_counts
        .get("NiTransformData")
        .copied()
        .unwrap_or(0);
    let admitted_keys = limits
        .max_combined_retained_bytes
        .checked_sub(animation.retained_bytes)
        .ok_or_else(|| {
            crate::Error::Unsupported(format!(
                "{source}: combined animation/key retained storage budget exceeded"
            ))
        })?
        .min(limits.array_bytes);
    let mut remaining = admitted_keys;
    let mut work = limits.key_work;
    read::charge_many(&mut work, count, source)?;
    super::reserve::<Block>(&mut remaining, count, source)?;
    super::reserve::<u8>(&mut remaining, count * 64, source)?;
    let mut blocks = Vec::with_capacity(count);
    for (id, span) in index.blocks.iter().enumerate() {
        if index.block_types[span.type_index as usize] != "NiTransformData" {
            continue;
        }
        let payload = &bytes[span.offset..span.offset + span.bytes];
        let reader = Reader {
            data: payload,
            base: span.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut remaining,
        };
        let data = read::decode(reader, &mut work)?;
        blocks.push(Block {
            block: id as u32,
            block_type: "NiTransformData",
            offset: span.offset,
            bytes: span.bytes,
            sha256: format!("{:x}", Sha256::digest(payload)),
            data,
        });
    }
    // Retirement happens after ALL admitted key payloads have parsed. Vector
    // capacity remains retained; the removed owned type strings are released.
    let mut released_strings = 0;
    animation.dependencies.retain(|dep| {
        if let Dependency::Link {
            role: LinkRole::TransformData,
            target,
            target_type,
            ..
        } = dep
            && index.block_types[index.blocks[*target as usize].type_index as usize]
                == "NiTransformData"
        {
            released_strings += target_type.len();
            false
        } else {
            true
        }
    });
    animation.retained_bytes -= released_strings;
    let keys = Catalogue {
        blocks,
        retained_bytes: admitted_keys - remaining,
        work_units: limits.key_work - work,
        runtime_ready: false,
    };
    Ok((index, Source { animation, keys }))
}
