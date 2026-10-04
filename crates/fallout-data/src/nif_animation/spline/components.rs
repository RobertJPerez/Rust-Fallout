//! Additive compact float/point3 source fields. Separate types keep the earlier
//! compact-transform catalogue, entrypoints and logical receipts unchanged.
use super::{LinkRole, UnresolvedLink};
use crate::{Error, Result, nif, nif_animation, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub splines: super::Limits,
    pub array_bytes: usize,
    /// One unit per selected block and stored primitive, before construction.
    pub component_work: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            splines: super::Limits::default(),
            array_bytes: 128 * 1024 * 1024,
            component_work: 16_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    CompactFloat {
        start_bits: u32,
        stop_bits: u32,
        spline_data: Option<u32>,
        basis_data: Option<u32>,
        value_bits: u32,
        handle: u32,
        float_offset_bits: u32,
        float_half_range_bits: u32,
    },
    CompactPoint3 {
        start_bits: u32,
        stop_bits: u32,
        spline_data: Option<u32>,
        basis_data: Option<u32>,
        value_bits: [u32; 3],
        handle: u32,
        position_offset_bits: u32,
        position_half_range_bits: u32,
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
    pub source: super::Source,
    pub components: Catalogue,
}
pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Source)> {
    decode_with_limits(bytes, source, Limits::default())
}
pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source)> {
    let (index, mut decoded) = super::decode_with_limits(bytes, source, limits.splines)?;
    let existing = decoded
        .animation
        .retained_bytes
        .checked_add(decoded.keys.retained_bytes)
        .and_then(|n| n.checked_add(decoded.splines.retained_bytes));
    let admitted = existing
        .and_then(|n| {
            limits
                .splines
                .keyframes
                .max_combined_retained_bytes
                .checked_sub(n)
        })
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{source}: combined spline/component retained storage budget exceeded"
            ))
        })?
        .min(limits.array_bytes);
    let mut remaining = admitted;
    let mut work = limits.component_work;
    let count = index
        .block_counts
        .iter()
        .filter(|(name, _)| selected(name).is_some())
        .map(|(_, count)| count)
        .sum();
    charge(&mut work, count, source)?;
    nif_animation::reserve::<Block>(&mut remaining, count, source)?;
    nif_animation::reserve::<u8>(&mut remaining, count * 64, source)?;
    let mut blocks = Vec::with_capacity(count);
    for (id, span) in index.blocks.iter().enumerate() {
        let Some(kind) = selected(&index.block_types[span.type_index as usize]) else {
            continue;
        };
        let payload = &bytes[span.offset..span.offset + span.bytes];
        let mut reader = Reader {
            data: payload,
            base: span.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut remaining,
        };
        charge(
            &mut work,
            if kind == "NiBSplineCompFloatInterpolator" {
                8
            } else {
                10
            },
            source,
        )?;
        let start_bits = reader.float()?.to_bits();
        let stop_bits = reader.float()?.to_bits();
        let spline_data = reader.reference()?;
        let basis_data = reader.reference()?;
        let data = if kind == "NiBSplineCompFloatInterpolator" {
            Data::CompactFloat {
                start_bits,
                stop_bits,
                spline_data,
                basis_data,
                value_bits: reader.float()?.to_bits(),
                handle: reader.u32()?,
                float_offset_bits: reader.float()?.to_bits(),
                float_half_range_bits: reader.float()?.to_bits(),
            }
        } else {
            Data::CompactPoint3 {
                start_bits,
                stop_bits,
                spline_data,
                basis_data,
                value_bits: [
                    reader.float()?.to_bits(),
                    reader.float()?.to_bits(),
                    reader.float()?.to_bits(),
                ],
                handle: reader.u32()?,
                position_offset_bits: reader.float()?.to_bits(),
                position_half_range_bits: reader.float()?.to_bits(),
            }
        };
        reader.finish()?;
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
    visit(&blocks, &index, source, |_, _, _, name| {
        dependency_count += 1;
        string_bytes += name.len();
        Ok(())
    })?;
    nif_animation::reserve::<UnresolvedLink>(&mut remaining, dependency_count, source)?;
    nif_animation::reserve::<u8>(&mut remaining, string_bytes, source)?;
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
                status: nif_animation::LinkStatus::UnknownClass,
            });
            Ok(())
        },
    )?;
    // Retire only exact fully decoded targets after every payload/link succeeds.
    // Original vector capacity remains charged; only released strings are freed.
    let mut released = 0;
    decoded.animation.dependencies.retain(|dependency| {
        if let nif_animation::Dependency::Link { role: nif_animation::LinkRole::Interpolator | nif_animation::LinkRole::ControlledInterpolator, target, target_type, .. } = dependency
            && selected(&index.block_types[index.blocks[*target as usize].type_index as usize]).is_some() {
            released += target_type.len(); false
        } else { true }
    });
    decoded.animation.retained_bytes -= released;
    Ok((
        index,
        Source {
            source: decoded,
            components: Catalogue {
                blocks,
                dependencies,
                retained_bytes: admitted - remaining,
                work_units: limits.component_work - work,
                runtime_ready: false,
            },
        },
    ))
}
fn selected(name: &str) -> Option<&'static str> {
    match name {
        "NiBSplineCompFloatInterpolator" => Some("NiBSplineCompFloatInterpolator"),
        "NiBSplineCompPoint3Interpolator" => Some("NiBSplineCompPoint3Interpolator"),
        _ => None,
    }
}
fn charge(work: &mut usize, count: usize, source: &str) -> Result<()> {
    *work = work.checked_sub(count).ok_or_else(|| {
        Error::Unsupported(format!("{source}: spline-component work budget exceeded"))
    })?;
    Ok(())
}
fn visit<'a>(
    blocks: &[Block],
    index: &'a nif::NifIndex,
    source: &str,
    mut emit: impl FnMut(u32, LinkRole, u32, &'a str) -> Result<()>,
) -> Result<()> {
    for block in blocks {
        let (data, basis) = match &block.data {
            Data::CompactFloat {
                spline_data,
                basis_data,
                ..
            }
            | Data::CompactPoint3 {
                spline_data,
                basis_data,
                ..
            } => (*spline_data, *basis_data),
        };
        for (role, target, expected) in [
            (LinkRole::SplineData, data, "NiBSplineData"),
            (LinkRole::BasisData, basis, "NiBSplineBasisData"),
        ] {
            let Some(target) = target else { continue };
            let target_type =
                index.block_types[index.blocks[target as usize].type_index as usize].as_str();
            if target_type == expected {
                continue;
            }
            if nif_animation::families::mask(target_type).is_some() {
                return Err(crate::malformed(
                    source,
                    block.offset as u64,
                    format!("spline component {role:?} link has wrong target kind {target_type}"),
                ));
            }
            emit(block.block, role, target, target_type)?;
        }
    }
    Ok(())
}
