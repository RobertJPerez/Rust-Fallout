//! xEdit 9fb0168 TES5 VMADFragmentedINFO/PACK/PERK/QUST/SCEN layouts.
use super::{Object, Reader, Script};
use crate::{Result, bad};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Extension<'a> {
    pub offset: usize,
    pub end: usize,
    pub extra_bind_version: u8,
    pub flags: Option<u8>,
    pub file_name: &'a [u8],
    /// Source order is meaningful; do not sort by selector or function name.
    pub fragments: Vec<Fragment<'a>>,
    pub aliases: Vec<Alias<'a>>,
}
#[derive(Debug, Serialize)]
pub struct Fragment<'a> {
    pub offset: usize,
    pub selector: Selector,
    pub unknown: u8,
    pub script_name: &'a [u8],
    pub function_name: &'a [u8],
}
#[derive(Debug, PartialEq, Serialize)]
pub enum Selector {
    FlagBit(u8),
    PerkIndex(u32),
    Quest { stage: u32, log_entry: u32 },
    ScenePhase { flags: u8, index: u32 },
}
#[derive(Debug, Serialize)]
pub struct Alias<'a> {
    pub offset: usize,
    pub object: Object,
    pub version: u16,
    pub object_format: u16,
    pub scripts: Vec<Script<'a>>,
}
fn fragment<'a>(r: &mut Reader<'a>, offset: usize, selector: Selector) -> Result<Fragment<'a>> {
    Ok(Fragment {
        offset,
        selector,
        unknown: r.u8()?,
        script_name: r.string()?,
        function_name: r.string()?,
    })
}
pub(super) fn read<'a>(
    r: &mut Reader<'a>,
    kind: [u8; 4],
    object_format: u16,
) -> Result<Extension<'a>> {
    if !matches!(&kind, b"INFO" | b"PACK" | b"PERK" | b"QUST" | b"SCEN") {
        return Err(bad(r.name, r.at, "VMAD tail on an unsupported record type"));
    }
    let offset = r.at;
    let extra_bind_version = r.u8()?;
    if extra_bind_version != 2 {
        return Err(bad(
            r.name,
            offset,
            format!("unsupported extra-bind version {extra_bind_version}"),
        ));
    }
    let mut flags = None;
    let mut fragments = Vec::new();
    let mut aliases = Vec::new();
    let file_name;
    match &kind {
        b"INFO" | b"PACK" | b"SCEN" => {
            let value = r.u8()?;
            let width = if kind == *b"PACK" { 3 } else { 2 };
            if value >> width != 0 {
                return Err(bad(r.name, r.at - 1, "unknown fragment flag bits"));
            }
            flags = Some(value);
            file_name = r.string()?;
            r.admit(value.count_ones() as usize, 5)?;
            for bit in 0..width {
                if value & (1 << bit) != 0 {
                    fragments.push(fragment(r, r.at, Selector::FlagBit(bit))?);
                }
            }
            if kind == *b"SCEN" {
                let count = r.u16()? as usize;
                r.admit(count, 10)?;
                for _ in 0..count {
                    let at = r.at;
                    let flags = r.u8()?;
                    let index = r.u32()?;
                    fragments.push(fragment(r, at, Selector::ScenePhase { flags, index })?);
                }
            }
        }
        b"PERK" => {
            file_name = r.string()?;
            let count = r.u16()? as usize;
            r.admit(count, 9)?;
            for _ in 0..count {
                let at = r.at;
                let index = r.u32()?;
                fragments.push(fragment(r, at, Selector::PerkIndex(index))?);
            }
        }
        b"QUST" => {
            let count = r.u16()? as usize;
            file_name = r.string()?;
            r.admit(count, 13)?;
            for _ in 0..count {
                let at = r.at;
                let stage = r.u32()?;
                let log_entry = r.u32()?;
                fragments.push(fragment(r, at, Selector::Quest { stage, log_entry })?);
            }
            let count = r.u16()? as usize;
            r.admit(count, 14)?;
            for _ in 0..count {
                let offset = r.at;
                // xEdit selects this object's layout from the outer VMAD header.
                let object = r.object(object_format)?;
                let version = r.u16()?;
                let alias_format = r.u16()?;
                if !matches!(version, 4 | 5) || alias_format != object_format {
                    return Err(bad(
                        r.name,
                        offset + 8,
                        "unsupported alias version or mixed object formats",
                    ));
                }
                let scripts = r.scripts(version, alias_format)?;
                aliases.push(Alias {
                    offset,
                    object,
                    version,
                    object_format: alias_format,
                    scripts,
                });
            }
        }
        _ => unreachable!("kind validated before reading"),
    }
    Ok(Extension {
        offset,
        end: r.at,
        extra_bind_version,
        flags,
        file_name,
        fragments,
        aliases,
    })
}
