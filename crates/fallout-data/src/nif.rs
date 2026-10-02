//! Bounded NV NIF container inspection. Unknown blocks stay addressable by exact
//! byte range; their names alone do not establish rendering or collision support.
use crate::{Error, Result, malformed};
use serde::Serialize;
use std::collections::BTreeMap;

const MAX_BLOCKS: usize = 1_000_000;
const MAX_STRINGS: usize = 1_000_000;
const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;

struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
    source: &'a str,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if count > self.data.len() - self.position {
            return Err(malformed(
                self.source,
                self.position as u64,
                "NIF field exceeds input",
            ));
        }
        let start = self.position;
        self.position += count;
        Ok(&self.data[start..self.position])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn sized_string(&mut self, max: usize) -> Result<Vec<u8>> {
        let count = self.u32()? as usize;
        if count > max || count > MAX_STRING_BYTES {
            return Err(malformed(
                self.source,
                self.position as u64,
                "NIF string budget exceeded",
            ));
        }
        Ok(self.take(count)?.to_vec())
    }
    fn array_budget(&self, count: usize, width: usize, maximum: usize) -> Result<()> {
        if count > maximum || count > (self.data.len() - self.position) / width {
            return Err(malformed(
                self.source,
                self.position as u64,
                "NIF table count exceeds input or budget",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct Block {
    pub type_index: u16,
    pub offset: usize,
    pub bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct NifIndex {
    pub version: u32,
    pub user_version: u32,
    pub bethesda_version: u32,
    pub export_strings: Vec<Vec<u8>>,
    pub block_types: Vec<String>,
    pub blocks: Vec<Block>,
    pub block_counts: BTreeMap<String, usize>,
    pub strings: Vec<Vec<u8>>,
    pub groups: Vec<u32>,
    pub roots: Vec<Option<u32>>,
    pub payload_start: usize,
    pub footer_offset: usize,
    pub semantics: &'static str,
}

pub fn inspect(bytes: &[u8], source: &str) -> Result<NifIndex> {
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(Error::Unsupported(
            "NIF exceeds 256 MiB inspection budget".into(),
        ));
    }
    let mut input = Cursor {
        data: bytes,
        position: 0,
        source,
    };
    let line_end = bytes
        .iter()
        .take(128)
        .position(|b| *b == b'\n')
        .ok_or_else(|| malformed(source, 0, "missing NIF header line"))?;
    let line = input.take(line_end + 1)?;
    if line != b"Gamebryo File Format, Version 20.2.0.7\n" {
        return Err(Error::Unsupported(format!(
            "{source}: NIF header is outside the NV adapter: {:?}",
            String::from_utf8_lossy(line).trim_end()
        )));
    }
    let version = input.u32()?;
    let endian = input.take(1)?[0];
    let user_version = input.u32()?;
    let count = input.u32()? as usize;
    let bethesda_version = input.u32()?;
    // These stream revisions occur in the NV corpus. Their outer tables share
    // this layout; block payloads still require stream-specific schema dispatch.
    let observed_streams = [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34];
    if (version, endian, user_version) != (0x1402_0007, 1, 11)
        || !observed_streams.contains(&bethesda_version)
    {
        return Err(Error::Unsupported(format!(
            "{source}: NIF tuple version={version:08X}, endian={endian}, user={user_version}, Bethesda={bethesda_version}"
        )));
    }
    input.array_budget(count, 6, MAX_BLOCKS)?;
    let mut export_strings = Vec::new();
    for _ in 0..3 {
        let length = input.take(1)?[0] as usize;
        export_strings.push(input.take(length)?.to_vec());
    }
    let type_count = input.u16()? as usize;
    input.array_budget(type_count, 4, 16_384)?;
    let mut block_types = Vec::with_capacity(type_count);
    for _ in 0..type_count {
        let raw = input.sized_string(1024)?;
        if raw.is_empty() || !raw.iter().all(u8::is_ascii_graphic) {
            return Err(malformed(
                source,
                input.position as u64,
                "invalid NIF block type name",
            ));
        }
        block_types.push(String::from_utf8(raw).expect("ASCII checked"));
    }
    input.array_budget(count, 6, MAX_BLOCKS)?;
    let mut type_indices = Vec::with_capacity(count);
    for _ in 0..count {
        let index = input.u16()?;
        if usize::from(index) >= type_count {
            return Err(malformed(
                source,
                input.position as u64 - 2,
                "NIF block type index out of range",
            ));
        }
        type_indices.push(index);
    }
    let mut sizes = Vec::with_capacity(count);
    for _ in 0..count {
        sizes.push(input.u32()? as usize);
    }
    let string_count = input.u32()? as usize;
    let max_length = input.u32()? as usize;
    input.array_budget(string_count, 4, MAX_STRINGS)?;
    let mut strings = Vec::with_capacity(string_count);
    let mut total_strings = 0;
    for _ in 0..string_count {
        let string = input.sized_string(max_length)?;
        total_strings += string.len();
        if total_strings > MAX_STRING_BYTES {
            return Err(malformed(
                source,
                input.position as u64,
                "NIF string table exceeds budget",
            ));
        }
        strings.push(string);
    }
    let group_count = input.u32()? as usize;
    input.array_budget(group_count, 4, MAX_BLOCKS)?;
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        groups.push(input.u32()?);
    }
    let payload_start = input.position;
    let mut blocks = Vec::with_capacity(count);
    let mut block_counts = BTreeMap::new();
    for (type_index, bytes) in type_indices.into_iter().zip(sizes) {
        let offset = input.position;
        input.take(bytes)?;
        *block_counts
            .entry(block_types[type_index as usize].clone())
            .or_default() += 1;
        blocks.push(Block {
            type_index,
            offset,
            bytes,
        });
    }
    let footer_offset = input.position;
    let root_count = input.u32()? as usize;
    input.array_budget(root_count, 4, MAX_BLOCKS)?;
    let mut roots = Vec::with_capacity(root_count);
    for _ in 0..root_count {
        let index = input.u32()?;
        if index != u32::MAX && index as usize >= count {
            return Err(malformed(
                source,
                input.position as u64 - 4,
                "NIF root reference out of range",
            ));
        }
        roots.push((index != u32::MAX).then_some(index));
    }
    if input.position != bytes.len() {
        return Err(malformed(
            source,
            input.position as u64,
            "unconsumed NIF footer bytes",
        ));
    }
    Ok(NifIndex {
        version,
        user_version,
        bethesda_version,
        export_strings,
        block_types,
        blocks,
        block_counts,
        strings,
        groups,
        roots,
        payload_start,
        footer_offset,
        semantics: "container framing only; block-internal references, meshes, materials, skinning, animation and collision unimplemented",
    })
}
