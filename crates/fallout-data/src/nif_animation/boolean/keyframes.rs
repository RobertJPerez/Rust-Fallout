//! Exact zero-count/constant NiBoolData source groups, without truth or events.
use crate::{Error, Result, nif, nif_animation, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub booleans: super::Limits,
    pub array_bytes: usize,
    /// One unit per selected block and stored count/tag/time/value primitive.
    pub key_work: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            booleans: super::Limits::default(),
            array_bytes: 128 * 1024 * 1024,
            key_work: 16_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Key {
    pub time_bits: u32,
    pub raw_value: u8,
}
#[derive(Debug, Serialize)]
pub struct Group {
    pub declared_keys: u32,
    /// Absent in the physical zero-count source layout.
    pub key_type: Option<u32>,
    pub keys: Vec<Key>,
}
#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: &'static str,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub data: Group,
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
    pub source: super::Source,
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
    let (index, mut decoded) = super::decode_with_limits(bytes, source, limits.booleans)?;
    let prior = &decoded.source.source;
    let existing = prior
        .animation
        .retained_bytes
        .checked_add(prior.keys.retained_bytes)
        .and_then(|n| n.checked_add(prior.splines.retained_bytes))
        .and_then(|n| n.checked_add(decoded.source.components.retained_bytes))
        .and_then(|n| n.checked_add(decoded.booleans.retained_bytes));
    let admitted = existing
        .and_then(|n| {
            limits
                .booleans
                .components
                .splines
                .keyframes
                .max_combined_retained_bytes
                .checked_sub(n)
        })
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{source}: combined Boolean-key retained storage budget exceeded"
            ))
        })?
        .min(limits.array_bytes);
    let mut remaining = admitted;
    let mut work = limits.key_work;
    let count: usize = index.block_counts.get("NiBoolData").copied().unwrap_or(0);
    charge(&mut work, count, source)?;
    nif_animation::reserve::<Block>(&mut remaining, count, source)?;
    nif_animation::reserve::<u8>(&mut remaining, count * 64, source)?;
    let mut blocks = Vec::with_capacity(count);
    for (id, span) in index.blocks.iter().enumerate() {
        if index.block_types[span.type_index as usize] != "NiBoolData" {
            continue;
        }
        let payload = &bytes[span.offset..span.offset + span.bytes];
        let mut reader = Reader {
            data: payload,
            base: span.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut remaining,
        };
        charge(&mut work, 1, source)?;
        let declared_keys = reader.u32()?;
        let key_type = if declared_keys == 0 {
            None
        } else {
            charge(&mut work, 1, source)?;
            let tag = reader.u32()?;
            if tag != 5 {
                return Err(Error::Unsupported(format!(
                    "{source}: NiBoolData key type {tag} unsupported in constant source branch"
                )));
            }
            Some(tag)
        };
        let count = declared_keys as usize;
        reader.budget(count, 5)?;
        if reader.data.len() - reader.position != count * 5 {
            return Err(reader.fail("unconsumed bytes in supported NiBoolData source group"));
        }
        charge(&mut work, count * 2, source)?;
        reader.reserve::<Key>(count)?;
        let mut keys = Vec::with_capacity(count);
        for _ in 0..count {
            keys.push(Key {
                time_bits: reader.float()?.to_bits(),
                raw_value: reader.u8()?,
            });
        }
        reader.finish()?;
        blocks.push(Block {
            block: id as u32,
            block_type: "NiBoolData",
            offset: span.offset,
            bytes: span.bytes,
            sha256: format!("{:x}", Sha256::digest(payload)),
            data: Group {
                declared_keys,
                key_type,
                keys,
            },
        });
    }
    // Only complete source groups retire exact data-payload dependencies.
    // Unknown classes and existing dependency vector capacity remain retained.
    let mut released = 0;
    decoded.booleans.dependencies.retain(|dependency| {
        if index.block_types[index.blocks[dependency.target as usize].type_index as usize]
            == "NiBoolData"
        {
            released += dependency.target_type.len();
            false
        } else {
            true
        }
    });
    decoded.booleans.retained_bytes -= released;
    Ok((
        index,
        Source {
            source: decoded,
            keys: Catalogue {
                blocks,
                retained_bytes: admitted - remaining,
                work_units: limits.key_work - work,
                runtime_ready: false,
            },
        },
    ))
}
fn charge(work: &mut usize, count: usize, source: &str) -> Result<()> {
    *work = work
        .checked_sub(count)
        .ok_or_else(|| Error::Unsupported(format!("{source}: Boolean-key work budget exceeded")))?;
    Ok(())
}
