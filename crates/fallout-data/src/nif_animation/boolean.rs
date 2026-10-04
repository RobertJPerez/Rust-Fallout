//! Exact Boolean interpolator source bytes. The raw byte has no admitted truth
//! meaning here; Boolean keys and timeline/event evaluation remain unsupported.
use super::{LinkStatus, spline};
use crate::{Error, Result, nif, nif_animation, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub components: spline::components::Limits,
    pub array_bytes: usize,
    /// Five units per selected block: block, two primitives and two link visits.
    pub boolean_work: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            components: spline::components::Limits::default(),
            array_bytes: 128 * 1024 * 1024,
            boolean_work: 16_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Interpolator {
    pub raw_value: u8,
    pub data: Option<u32>,
}
#[derive(Debug, Serialize)]
pub struct Block {
    pub block: u32,
    pub block_type: &'static str,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub data: Interpolator,
}
#[derive(Debug, Serialize)]
pub struct UnresolvedLink {
    pub block: u32,
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
    pub source: spline::components::Source,
    pub booleans: Catalogue,
}
pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Source)> {
    decode_with_limits(bytes, source, Limits::default())
}
pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Source)> {
    let (index, mut decoded) =
        spline::components::decode_with_limits(bytes, source, limits.components)?;
    let prior = &decoded.source;
    let existing = prior
        .animation
        .retained_bytes
        .checked_add(prior.keys.retained_bytes)
        .and_then(|n| n.checked_add(prior.splines.retained_bytes))
        .and_then(|n| n.checked_add(decoded.components.retained_bytes));
    let admitted = existing
        .and_then(|n| {
            limits
                .components
                .splines
                .keyframes
                .max_combined_retained_bytes
                .checked_sub(n)
        })
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{source}: combined Boolean retained storage budget exceeded"
            ))
        })?
        .min(limits.array_bytes);
    let count: usize = index
        .block_counts
        .iter()
        .filter(|(kind, _)| selected(kind).is_some())
        .map(|(_, count)| count)
        .sum();
    let work_units = count
        .checked_mul(5)
        .filter(|n| *n <= limits.boolean_work)
        .ok_or_else(|| {
            Error::Unsupported(format!("{source}: Boolean source work budget exceeded"))
        })?;
    let mut remaining = admitted;
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
        // u8 is deliberate: XML's Boolean storage includes raw pose sentinel 2.
        let data = Interpolator {
            raw_value: reader.u8()?,
            data: reader.reference()?,
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
    visit(&blocks, &index, source, |_, _, name, _| {
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
        |block, target, target_type, status| {
            dependencies.push(UnresolvedLink {
                block,
                target,
                target_type: target_type.into(),
                status,
            });
            Ok(())
        },
    )?;
    // Only now retire the exact interpolator payload dependencies. NiBoolData
    // remains unparsed, and original dependency vector capacity stays charged.
    let mut released = 0;
    decoded.source.animation.dependencies.retain(|dependency| {
        if let nif_animation::Dependency::Link { role: nif_animation::LinkRole::Interpolator | nif_animation::LinkRole::ControlledInterpolator, target, target_type, .. } = dependency
            && selected(&index.block_types[index.blocks[*target as usize].type_index as usize]).is_some() {
            released += target_type.len(); false
        } else { true }
    });
    decoded.source.animation.retained_bytes -= released;
    Ok((
        index,
        Source {
            source: decoded,
            booleans: Catalogue {
                blocks,
                dependencies,
                retained_bytes: admitted - remaining,
                work_units,
                runtime_ready: false,
            },
        },
    ))
}
fn selected(name: &str) -> Option<&'static str> {
    match name {
        "NiBoolInterpolator" => Some("NiBoolInterpolator"),
        "NiBoolTimelineInterpolator" => Some("NiBoolTimelineInterpolator"),
        _ => None,
    }
}
fn visit<'a>(
    blocks: &[Block],
    index: &'a nif::NifIndex,
    source: &str,
    mut emit: impl FnMut(u32, u32, &'a str, LinkStatus) -> Result<()>,
) -> Result<()> {
    for block in blocks {
        let Some(target) = block.data.data else {
            continue;
        };
        let kind = index.block_types[index.blocks[target as usize].type_index as usize].as_str();
        let status = if kind == "NiBoolData" {
            LinkStatus::UndecodedPayload
        } else if nif_animation::families::mask(kind).is_some() {
            return Err(crate::malformed(
                source,
                block.offset as u64,
                format!("Boolean data link has wrong target kind {kind}"),
            ));
        } else {
            LinkStatus::UnknownClass
        };
        emit(block.block, target, kind, status)?;
    }
    Ok(())
}
