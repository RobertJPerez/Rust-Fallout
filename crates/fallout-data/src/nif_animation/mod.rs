//! Bounded authored animation framing. Source bits/links remain exact; clocks,
//! event delivery, interpolation, external name binding and poses are unverified.
mod families;
mod preflight;
mod read;

use crate::{Error, Result, malformed, nif, nif_scene::cursor::Reader};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub input_bytes: usize,
    pub blocks: usize,
    /// Charged index/source vector elements and string bytes; temporary index
    /// vectors are admitted concurrently, then released from this charge.
    pub array_bytes: usize,
    pub reference_checks: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 64 * 1024 * 1024,
            blocks: 100_000,
            array_bytes: 128 * 1024 * 1024,
            reference_checks: 16_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Controller {
    pub next_controller: Option<u32>,
    pub flags: u16,
    pub frequency_bits: u32,
    pub phase_bits: u32,
    pub start_bits: u32,
    pub stop_bits: u32,
    pub target: Option<u32>,
    pub interpolator: Option<u32>,
}
#[derive(Debug, Serialize)]
pub struct ControlledBlock {
    pub interpolator: Option<u32>,
    pub controller: Option<u32>,
    pub priority: u8,
    pub node_name: Option<u32>,
    pub property_type: Option<u32>,
    pub controller_type: Option<u32>,
    pub controller_id: Option<u32>,
    pub interpolator_id: Option<u32>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "layout", rename_all = "snake_case")]
pub enum NoteLinks {
    Absent,
    Single {
        target: Option<u32>,
    },
    Array {
        declared_count: u16,
        targets: Vec<Option<u32>>,
    },
}
#[derive(Debug, Serialize)]
pub struct Sequence {
    pub name: Option<u32>,
    pub declared_controlled_blocks: u32,
    pub array_grow_by: u32,
    pub controlled_blocks: Vec<ControlledBlock>,
    pub weight_bits: u32,
    pub text_keys: Option<u32>,
    pub cycle_type: u32,
    pub frequency_bits: u32,
    pub start_bits: u32,
    pub stop_bits: u32,
    pub manager: Option<u32>,
    pub accum_root_name: Option<u32>,
    pub notes: NoteLinks,
}
#[derive(Debug, Serialize)]
pub struct TransformInterpolator {
    pub translation_bits: [u32; 3],
    /// Stored quaternion order is W,X,Y,Z. No normalization is performed.
    pub rotation_wxyz_bits: [u32; 4],
    pub scale_bits: u32,
    pub data: Option<u32>,
}
#[derive(Debug, Serialize)]
pub struct TextKey {
    pub time_bits: u32,
    pub value: Option<u32>,
}
#[derive(Debug, Serialize)]
pub struct TextKeys {
    pub name: Option<u32>,
    pub declared_keys: u32,
    pub keys: Vec<TextKey>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Data {
    TransformController { controller: Controller },
    ControllerSequence { sequence: Sequence },
    TransformInterpolator { interpolator: TransformInterpolator },
    TextKeyExtraData { text_keys: TextKeys },
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
    NextController,
    Target,
    Interpolator,
    ControlledInterpolator,
    ControlledController,
    TextKeys,
    Manager,
    TransformData,
    Notes,
}
impl LinkRole {
    fn family(self) -> u8 {
        match self {
            Self::NextController | Self::ControlledController => 1,
            Self::Interpolator | Self::ControlledInterpolator => 2,
            Self::Target => 4,
            Self::TextKeys => 8,
            Self::Manager => 16,
            Self::TransformData => 32,
            Self::Notes => 64,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkStatus {
    UndecodedPayload,
    UnknownClass,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Dependency {
    Link {
        block: u32,
        role: LinkRole,
        ordinal: Option<usize>,
        target: u32,
        target_type: String,
        status: LinkStatus,
    },
    ExternalBinding {
        sequence: u32,
        ordinal: usize,
    },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Diagnostic {
    UnverifiedCycleType { sequence: u32, value: u32 },
}
#[derive(Debug, Serialize)]
pub struct Animation {
    pub blocks: Vec<Block>,
    pub dependencies: Vec<Dependency>,
    pub diagnostics: Vec<Diagnostic>,
    /// Conservative logical retained storage for the returned index/catalogue.
    /// Released table scratch and allocator overhead are not retained bytes.
    pub retained_bytes: usize,
    pub runtime_ready: bool,
}

pub fn decode(bytes: &[u8], source: &str) -> Result<(nif::NifIndex, Animation)> {
    decode_with_limits(bytes, source, Limits::default())
}
pub fn decode_with_limits(
    bytes: &[u8],
    source: &str,
    limits: Limits,
) -> Result<(nif::NifIndex, Animation)> {
    let index_storage = preflight::storage(bytes, source, limits)?;
    let index = nif::inspect(bytes, source)?;
    let mut remaining = limits.array_bytes - index_storage;
    let selected = index
        .block_counts
        .iter()
        .filter(|(name, _)| selected_kind(name).is_some())
        .map(|(_, count)| count)
        .sum();
    reserve::<Block>(&mut remaining, selected, source)?;
    reserve::<u8>(&mut remaining, selected * 64, source)?;
    let mut blocks = Vec::with_capacity(selected);
    let mut checks = limits.reference_checks;
    for (id, block) in index.blocks.iter().enumerate() {
        let Some(kind) = selected_kind(&index.block_types[block.type_index as usize]) else {
            continue;
        };
        charge(&mut checks, source)?; // Empty source blocks/arrays still cost work.
        let payload = &bytes[block.offset..block.offset + block.bytes];
        let reader = Reader {
            data: payload,
            base: block.offset,
            position: 0,
            source,
            index: &index,
            array_bytes_left: &mut remaining,
        };
        let data = read::decode(reader, kind, &mut checks)?;
        blocks.push(Block {
            block: id as u32,
            block_type: kind,
            offset: block.offset,
            bytes: block.bytes,
            sha256: format!("{:x}", Sha256::digest(payload)),
            data,
        });
    }
    // Two bounded visits avoid speculative/doubling dependency-vector allocation.
    // The first visit validates families and measures exact logical storage; the
    // second fills the preallocated vectors. Source relation work is charged in
    // the reader, including null links/string indices and empty packet products.
    let mut dependency_count = 0;
    let mut diagnostic_count = 0;
    let mut target_type_bytes = 0;
    visit(&blocks, &index, source, |item| {
        match item {
            Finding::Link { target_type, .. } => {
                dependency_count += 1;
                target_type_bytes += target_type.len();
            }
            Finding::External { .. } => dependency_count += 1,
            Finding::Cycle { .. } => diagnostic_count += 1,
        }
        Ok(())
    })?;
    reserve::<Dependency>(&mut remaining, dependency_count, source)?;
    reserve::<Diagnostic>(&mut remaining, diagnostic_count, source)?;
    reserve::<u8>(&mut remaining, target_type_bytes, source)?;
    let mut dependencies = Vec::with_capacity(dependency_count);
    let mut diagnostics = Vec::with_capacity(diagnostic_count);
    visit(&blocks, &index, source, |item| {
        match item {
            Finding::Link {
                block,
                role,
                ordinal,
                target,
                target_type,
                status,
            } => dependencies.push(Dependency::Link {
                block,
                role,
                ordinal,
                target,
                target_type: target_type.into(),
                status,
            }),
            Finding::External { sequence, ordinal } => {
                dependencies.push(Dependency::ExternalBinding { sequence, ordinal })
            }
            Finding::Cycle { sequence, value } => {
                diagnostics.push(Diagnostic::UnverifiedCycleType { sequence, value })
            }
        }
        Ok(())
    })?;
    Ok((
        index,
        Animation {
            blocks,
            dependencies,
            diagnostics,
            retained_bytes: limits.array_bytes - remaining,
            runtime_ready: false,
        },
    ))
}

fn selected_kind(name: &str) -> Option<&'static str> {
    match name {
        "NiTransformController" => Some("NiTransformController"),
        "NiControllerSequence" => Some("NiControllerSequence"),
        "NiTransformInterpolator" => Some("NiTransformInterpolator"),
        "NiTextKeyExtraData" => Some("NiTextKeyExtraData"),
        _ => None,
    }
}
fn reserve<T>(remaining: &mut usize, count: usize, source: &str) -> Result<()> {
    *remaining = count
        .checked_mul(std::mem::size_of::<T>())
        .and_then(|n| remaining.checked_sub(n))
        .ok_or_else(|| {
            Error::Unsupported(format!(
                "{source}: animation retained storage budget exceeded"
            ))
        })?;
    Ok(())
}
fn charge(checks: &mut usize, source: &str) -> Result<()> {
    *checks = checks.checked_sub(1).ok_or_else(|| {
        Error::Unsupported(format!(
            "{source}: animation reference-check budget exceeded"
        ))
    })?;
    Ok(())
}

enum Finding<'a> {
    Link {
        block: u32,
        role: LinkRole,
        ordinal: Option<usize>,
        target: u32,
        target_type: &'a str,
        status: LinkStatus,
    },
    External {
        sequence: u32,
        ordinal: usize,
    },
    Cycle {
        sequence: u32,
        value: u32,
    },
}
fn visit<'a>(
    blocks: &[Block],
    index: &'a nif::NifIndex,
    source: &str,
    mut emit: impl FnMut(Finding<'a>) -> Result<()>,
) -> Result<()> {
    for block in blocks {
        let mut link = |role: LinkRole, ordinal, target| {
            let Some(target) = target else { return Ok(()) };
            let span = &index.blocks[target as usize];
            let target_type = index.block_types[span.type_index as usize].as_str();
            let status = match families::mask(target_type) {
                Some(mask) if mask & role.family() == 0 => {
                    return Err(malformed(
                        source,
                        block.offset as u64,
                        format!("animation {role:?} link has wrong target kind {target_type}"),
                    ));
                }
                Some(_) if selected_kind(target_type).is_some() => return Ok(()),
                Some(_) => LinkStatus::UndecodedPayload,
                None => LinkStatus::UnknownClass,
            };
            emit(Finding::Link {
                block: block.block,
                role,
                ordinal,
                target,
                target_type,
                status,
            })
        };
        match &block.data {
            Data::TransformController { controller: c } => {
                link(LinkRole::NextController, None, c.next_controller)?;
                link(LinkRole::Target, None, c.target)?;
                link(LinkRole::Interpolator, None, c.interpolator)?;
            }
            Data::TransformInterpolator { interpolator: t } => {
                link(LinkRole::TransformData, None, t.data)?
            }
            Data::TextKeyExtraData { .. } => {}
            Data::ControllerSequence { sequence: s } => {
                for (ordinal, packet) in s.controlled_blocks.iter().enumerate() {
                    link(
                        LinkRole::ControlledInterpolator,
                        Some(ordinal),
                        packet.interpolator,
                    )?;
                    link(
                        LinkRole::ControlledController,
                        Some(ordinal),
                        packet.controller,
                    )?;
                }
                link(LinkRole::TextKeys, None, s.text_keys)?;
                link(LinkRole::Manager, None, s.manager)?;
                match &s.notes {
                    NoteLinks::Absent => {}
                    NoteLinks::Single { target } => link(LinkRole::Notes, None, *target)?,
                    NoteLinks::Array { targets, .. } => {
                        for (ordinal, target) in targets.iter().enumerate() {
                            link(LinkRole::Notes, Some(ordinal), *target)?;
                        }
                    }
                }
                for ordinal in 0..s.controlled_blocks.len() {
                    emit(Finding::External {
                        sequence: block.block,
                        ordinal,
                    })?;
                }
                if s.cycle_type > 2 {
                    emit(Finding::Cycle {
                        sequence: block.block,
                        value: s.cycle_type,
                    })?;
                }
            }
        }
    }
    Ok(())
}
