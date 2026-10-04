//! Exact compact-transform/B-spline source fields. Decompression, usable channel
//! ranges, basis evaluation, poses and retail playback remain unverified.
mod read;

use super::{Animation, Dependency, LinkStatus, keyframe};
use crate::{Error, Result, nif, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// The keyframe combined cap also covers this catalogue. Schemas1/2 and
    /// their budgets remain unchanged when this optional decoder is not called.
    pub keyframes: keyframe::Limits,
    pub array_bytes: usize,
    /// One unit per selected block and stored scalar (including both counts).
    /// Work is admitted before allocating control-point arrays.
    pub spline_work: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            keyframes: keyframe::Limits::default(),
            array_bytes: 128 * 1024 * 1024,
            spline_work: 16_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    CompactTransform {
        start_bits: u32,
        stop_bits: u32,
        spline_data: Option<u32>,
        basis_data: Option<u32>,
        translation_bits: [u32; 3],
        rotation_wxyz_bits: [u32; 4],
        scale_bits: u32,
        translation_handle: u32,
        rotation_handle: u32,
        scale_handle: u32,
        translation_offset_bits: u32,
        translation_half_range_bits: u32,
        rotation_offset_bits: u32,
        rotation_half_range_bits: u32,
        scale_offset_bits: u32,
        scale_half_range_bits: u32,
    },
    ControlPoints {
        declared_float_count: u32,
        float_bits: Vec<u32>,
        declared_compact_count: u32,
        /// Exact signed i16 values, including -32768. No scaling/clamping.
        compact: Vec<i16>,
    },
    Basis {
        /// Source scalar only, not a verified allocation/channel cardinality.
        num_control_points: u32,
    },
}
#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: &'static str,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub data: Data,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRole {
    SplineData,
    BasisData,
}
#[derive(Debug, Serialize)]
pub struct UnresolvedLink {
    pub block: u32,
    pub role: LinkRole,
    pub target: u32,
    pub target_type: String,
    pub status: LinkStatus,
}
#[derive(Debug, Serialize)]
pub struct Catalogue {
    pub blocks: Vec<Block>,
    pub dependencies: Vec<UnresolvedLink>,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub runtime_ready: bool,
}
#[derive(Debug, Serialize)]
pub struct Source {
    pub animation: Animation,
    pub keys: keyframe::Catalogue,
    pub splines: Catalogue,
}

pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Source)> {
    decode_with_limits(bytes, source, Limits::default())
}
pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source)> {
    // Reuse one immutable, internally constructed index and the admitted source
    // catalogues. No duplicate importer, caller-provided index or repaired keys.
    let (
        index,
        keyframe::Source {
            mut animation,
            keys,
        },
    ) = keyframe::decode_with_limits(bytes, source, limits.keyframes)?;
    let existing = animation.retained_bytes.checked_add(keys.retained_bytes);
    let admitted = existing
        .and_then(|n| limits.keyframes.max_combined_retained_bytes.checked_sub(n))
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{source}: combined animation/key/spline retained storage budget exceeded"
            ))
        })?
        .min(limits.array_bytes);
    let mut remaining = admitted;
    let mut work = limits.spline_work;
    let count = index
        .block_counts
        .iter()
        .filter(|(name, _)| selected(name).is_some())
        .map(|(_, count)| count)
        .sum();
    read::charge(&mut work, count, source)?;
    super::reserve::<Block>(&mut remaining, count, source)?;
    super::reserve::<u8>(&mut remaining, count * 64, source)?;
    let mut blocks = Vec::with_capacity(count);
    for (id, span) in index.blocks.iter().enumerate() {
        let Some(kind) = selected(&index.block_types[span.type_index as usize]) else {
            continue;
        };
        let payload = &bytes[span.offset..span.offset + span.bytes];
        let reader = Reader {
            data: payload,
            base: span.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut remaining,
        };
        let data = read::decode(reader, kind, &mut work)?;
        blocks.push(Block {
            block: id as u32,
            block_type: kind,
            offset: span.offset,
            bytes: span.bytes,
            sha256: format!("{:x}", Sha256::digest(payload)),
            data,
        });
    }
    let mut dependency_count = 0;
    let mut string_bytes = 0;
    visit(&blocks, &index, source, |_, _, _, target_type| {
        dependency_count += 1;
        string_bytes += target_type.len();
        Ok(())
    })?;
    super::reserve::<UnresolvedLink>(&mut remaining, dependency_count, source)?;
    super::reserve::<u8>(&mut remaining, string_bytes, source)?;
    let mut dependencies = Vec::with_capacity(dependency_count);
    visit(
        &blocks,
        &index,
        source,
        |block, role, target, target_type| {
            dependencies.push(UnresolvedLink {
                block,
                role,
                target,
                target_type: target_type.into(),
                status: LinkStatus::UnknownClass,
            });
            Ok(())
        },
    )?;
    // Only after every payload/link has validated. Retain vector capacity in the
    // charge; release just owned strings for the now-decoded exact target class.
    let mut released_strings = 0;
    animation.dependencies.retain(|dependency| {
        if let Dependency::Link {
            role: super::LinkRole::Interpolator | super::LinkRole::ControlledInterpolator,
            target,
            target_type,
            ..
        } = dependency
            && index.block_types[index.blocks[*target as usize].type_index as usize]
                == "NiBSplineCompTransformInterpolator"
        {
            released_strings += target_type.len();
            false
        } else {
            true
        }
    });
    animation.retained_bytes -= released_strings;
    let splines = Catalogue {
        blocks,
        dependencies,
        retained_bytes: admitted - remaining,
        work_units: limits.spline_work - work,
        runtime_ready: false,
    };
    Ok((
        index,
        Source {
            animation,
            keys,
            splines,
        },
    ))
}
fn selected(name: &str) -> Option<&'static str> {
    match name {
        "NiBSplineCompTransformInterpolator" => Some("NiBSplineCompTransformInterpolator"),
        "NiBSplineData" => Some("NiBSplineData"),
        "NiBSplineBasisData" => Some("NiBSplineBasisData"),
        _ => None,
    }
}
fn visit<'a>(
    blocks: &[Block],
    index: &'a nif::NifIndex,
    source: &str,
    mut emit: impl FnMut(u32, LinkRole, u32, &'a str) -> Result<()>,
) -> Result<()> {
    for block in blocks {
        if let Data::CompactTransform {
            spline_data,
            basis_data,
            ..
        } = &block.data
        {
            for (role, target, expected) in [
                (LinkRole::SplineData, spline_data, "NiBSplineData"),
                (LinkRole::BasisData, basis_data, "NiBSplineBasisData"),
            ] {
                let Some(target) = *target else { continue };
                let target_type =
                    index.block_types[index.blocks[target as usize].type_index as usize].as_str();
                if target_type == expected {
                    continue;
                }
                if super::families::mask(target_type).is_some() {
                    return Err(crate::malformed(
                        source,
                        block.offset as u64,
                        format!("spline {role:?} link has wrong target kind {target_type}"),
                    ));
                }
                emit(block.block, role, target, target_type)?;
            }
        }
    }
    Ok(())
}
