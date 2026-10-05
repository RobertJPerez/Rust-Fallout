//! Skyrim SE NIF outer-table indexing for the verified 20.2.0.7/user-12/100 tuple.
//!
//! This is deliberately an envelope reader only: block bytes stay opaque and
//! no NV/FO4 block interpretation is imported.
use crate::{Result, bad, nif_header};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

const MAX_ASSET_BYTES: usize = 64 * 1024 * 1024;
const MAX_BLOCKS: usize = 1_000_000;
const MAX_BLOCK_TYPES: usize = 16_384;
const MAX_STRINGS: usize = 1_000_000;
const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub archive: String,
    pub archive_version: u32,
    pub member_path: Vec<u8>,
    pub asset_bytes: u64,
    pub asset_sha256: String,
    pub index: NifIndex,
    pub scope: &'static str,
}

#[derive(Debug, Serialize)]
pub struct NifIndex {
    pub version: u32,
    pub user_version: u32,
    pub stream_version: u32,
    pub block_types: Vec<String>,
    pub blocks: Vec<Block>,
    pub block_counts: BTreeMap<String, usize>,
    pub strings: Vec<Vec<u8>>,
    pub groups: Vec<u32>,
    pub roots: Vec<Option<u32>>,
    pub payload_start: usize,
    pub footer_offset: usize,
    pub end_offset: usize,
    pub semantics: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Block {
    pub raw_type_index: u16,
    pub type_index: u16,
    /// Preserved as a bit only; the NIF reference describes its PhysX use as
    /// apparent rather than specifying a Skyrim semantic here.
    pub upper_bit_set: bool,
    pub type_name: String,
    pub offset: usize,
    pub bytes: usize,
}

struct Cursor<'a> {
    data: &'a [u8],
    position: usize,
    source: &'a str,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| bad(self.source, self.position, "NIF offset overflow"))?;
        let bytes = self
            .data
            .get(self.position..end)
            .ok_or_else(|| bad(self.source, self.position, "NIF field exceeds input"))?;
        self.position = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn count(&mut self, width: usize, maximum: usize, label: &str) -> Result<usize> {
        let offset = self.position;
        let count = usize::try_from(self.u32()?).map_err(|_| {
            bad(
                self.source,
                offset,
                format!("{label} count exceeds platform"),
            )
        })?;
        self.array_budget(count, width, maximum, label)?;
        Ok(count)
    }

    fn array_budget(&self, count: usize, width: usize, maximum: usize, label: &str) -> Result<()> {
        let minimum_bytes = count
            .checked_mul(width)
            .ok_or_else(|| bad(self.source, self.position, format!("{label} size overflow")))?;
        if count > maximum || minimum_bytes > self.data.len().saturating_sub(self.position) {
            return Err(bad(
                self.source,
                self.position,
                format!("{label} count exceeds input or budget"),
            ));
        }
        Ok(())
    }

    fn sized_string(&mut self, maximum: usize, label: &str) -> Result<Vec<u8>> {
        let offset = self.position;
        let length = usize::try_from(self.u32()?).map_err(|_| {
            bad(
                self.source,
                offset,
                format!("{label} length exceeds platform"),
            )
        })?;
        if length > maximum {
            return Err(bad(
                self.source,
                offset,
                format!("{label} length exceeds budget"),
            ));
        }
        Ok(self.take(length)?.to_vec())
    }
}

/// Index the SSE container tables after requiring the exact Skyrim SE tuple.
/// No field inside any block payload is decoded.
pub fn index_sse(bytes: &[u8], source: &str) -> Result<NifIndex> {
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(crate::Error::Unsupported(format!(
            "{source}: NIF exceeds the 64 MiB Skyrim index budget"
        )));
    }
    let header = nif_header::parse_header(source, bytes)?;
    if !header.skyrim_se_header_tuple {
        return Err(crate::Error::Unsupported(format!(
            "{source}: Skyrim NIF indexing requires 20.2.0.7/user-12/stream-100; observed {}",
            header.classification
        )));
    }
    let newline = bytes
        .iter()
        .take(129)
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| bad(source, 0, "NIF header line is missing"))?;
    let mut input = Cursor {
        data: bytes,
        position: newline + 1,
        source,
    };

    let version = input.u32()?;
    let endian = input.u8()?;
    let user_version = input.u32()?;
    let block_count = usize::try_from(input.u32()?)
        .map_err(|_| bad(source, input.position, "NIF block count exceeds platform"))?;
    let stream_version = input.u32()?;
    if (version, endian, user_version, stream_version) != (0x1402_0007, 1, 12, 100) {
        return Err(crate::Error::Unsupported(format!(
            "{source}: NIF tuple changed while indexing"
        )));
    }
    input.array_budget(block_count, 6, MAX_BLOCKS, "NIF block")?;

    // BSStreamHeader for this tuple is BS Version + Author, Process Script,
    // and Export Script. The conditional Max Filepath field begins at 103.
    for field in [
        "author export string",
        "process export string",
        "export string",
    ] {
        let length = usize::from(input.u8()?);
        input
            .take(length)
            .map_err(|_| bad(source, input.position, format!("truncated NIF {field}")))?;
    }

    let type_count = usize::from(input.u16()?);
    input.array_budget(type_count, 4, MAX_BLOCK_TYPES, "NIF block type")?;
    let mut block_types = Vec::with_capacity(type_count);
    for _ in 0..type_count {
        let offset = input.position;
        let raw = input.sized_string(1024, "NIF block type")?;
        if raw.is_empty() || !raw.iter().all(u8::is_ascii_graphic) {
            return Err(bad(source, offset, "invalid NIF block type name"));
        }
        block_types.push(String::from_utf8(raw).expect("ASCII checked"));
    }

    input.array_budget(block_count, 2, MAX_BLOCKS, "NIF block type index")?;
    let mut raw_type_indices = Vec::with_capacity(block_count);
    for _ in 0..block_count {
        let offset = input.position;
        let raw = input.u16()?;
        let index = raw & 0x7fff;
        if usize::from(index) >= type_count {
            return Err(bad(source, offset, "NIF block type index out of range"));
        }
        raw_type_indices.push(raw);
    }

    input.array_budget(block_count, 4, MAX_BLOCKS, "NIF block size")?;
    let mut block_sizes = Vec::with_capacity(block_count);
    for _ in 0..block_count {
        let offset = input.position;
        let size = usize::try_from(input.u32()?)
            .map_err(|_| bad(source, offset, "NIF block size exceeds platform"))?;
        block_sizes.push(size);
    }

    let string_count = input.count(4, MAX_STRINGS, "NIF string")?;
    let maximum_string_length = usize::try_from(input.u32()?).map_err(|_| {
        bad(
            source,
            input.position,
            "NIF maximum string length exceeds platform",
        )
    })?;
    if maximum_string_length > MAX_STRING_BYTES {
        return Err(bad(
            source,
            input.position - 4,
            "NIF maximum string length exceeds budget",
        ));
    }
    let mut strings = Vec::with_capacity(string_count);
    let mut total_string_bytes = 0usize;
    for _ in 0..string_count {
        let string = input.sized_string(maximum_string_length, "NIF string")?;
        total_string_bytes = total_string_bytes
            .checked_add(string.len())
            .ok_or_else(|| bad(source, input.position, "NIF string table size overflow"))?;
        if total_string_bytes > MAX_STRING_BYTES {
            return Err(bad(
                source,
                input.position,
                "NIF string table exceeds budget",
            ));
        }
        strings.push(string);
    }

    let group_count = input.count(4, MAX_BLOCKS, "NIF group")?;
    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        groups.push(input.u32()?);
    }

    let payload_start = input.position;
    let mut block_counts = BTreeMap::new();
    let mut blocks = Vec::with_capacity(block_count);
    for (raw_type_index, bytes) in raw_type_indices.into_iter().zip(block_sizes) {
        let type_index = raw_type_index & 0x7fff;
        let type_name = block_types[usize::from(type_index)].clone();
        let offset = input.position;
        input.take(bytes)?;
        *block_counts.entry(type_name.clone()).or_default() += 1;
        blocks.push(Block {
            raw_type_index,
            type_index,
            upper_bit_set: raw_type_index & 0x8000 != 0,
            type_name,
            offset,
            bytes,
        });
    }

    let footer_offset = input.position;
    let root_count = input.count(4, MAX_BLOCKS, "NIF root")?;
    let mut roots = Vec::with_capacity(root_count);
    for _ in 0..root_count {
        let offset = input.position;
        let root = input.u32()?;
        if root != u32::MAX && usize::try_from(root).unwrap_or(usize::MAX) >= block_count {
            return Err(bad(source, offset, "NIF root block index out of range"));
        }
        roots.push((root != u32::MAX).then_some(root));
    }
    if input.position != bytes.len() {
        return Err(bad(
            source,
            input.position,
            "unconsumed NIF bytes after footer",
        ));
    }

    Ok(NifIndex {
        version,
        user_version,
        stream_version,
        block_types,
        blocks,
        block_counts,
        strings,
        groups,
        roots,
        payload_start,
        footer_offset,
        end_offset: input.position,
        semantics: "Skyrim SE outer table and block byte spans only; block payloads, references, geometry, materials, skinning, animation and collision remain opaque",
    })
}

pub fn inspect_archive_member(archive_path: &Path, member_path: &[u8]) -> Result<Report> {
    let (archive, archive_version, actual_path, bytes) =
        nif_header::read_archive_member(archive_path, member_path)?;
    let index = index_sse(
        &bytes,
        &format!("{archive}:{}", String::from_utf8_lossy(&actual_path)),
    )?;
    let (asset_bytes, asset_sha256) = crate::nif_header::digest_bytes(&bytes)?;
    Ok(Report {
        schema_version: 1,
        archive,
        archive_version,
        member_path: actual_path,
        asset_bytes,
        asset_sha256,
        index,
        scope: "one bounded BSA member; Skyrim SSE NIF outer tables and exact block byte spans only; no block payloads decoded or runtime compatibility claimed",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sized(bytes: &[u8], output: &mut Vec<u8>) {
        output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        output.extend_from_slice(bytes);
    }

    fn sse_fixture() -> Vec<u8> {
        let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        bytes.extend_from_slice(&0x1402_0007u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&100u32.to_le_bytes());
        for value in [b"author\0".as_slice(), b"process\0", b"export\0"] {
            bytes.push(value.len() as u8);
            bytes.extend_from_slice(value);
        }
        bytes.extend_from_slice(&2u16.to_le_bytes());
        sized(b"NiNode", &mut bytes);
        sized(b"BSTriShape", &mut bytes);
        for index in [0u16, 0x8001, 1] {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        for size in [1u32, 2, 0] {
            bytes.extend_from_slice(&size.to_le_bytes());
        }
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        sized(b"Root", &mut bytes);
        sized(b"child", &mut bytes);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&42u32.to_le_bytes());
        bytes.extend_from_slice(b"abc");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes
    }

    #[test]
    fn indexes_sse_outer_tables_without_interpreting_block_payloads() {
        let bytes = sse_fixture();
        let index = index_sse(&bytes, "synthetic-sse.nif").unwrap();
        assert_eq!(index.version, 0x1402_0007);
        assert_eq!(index.user_version, 12);
        assert_eq!(index.stream_version, 100);
        assert_eq!(index.block_types, ["NiNode", "BSTriShape"]);
        assert_eq!(index.blocks.len(), 3);
        assert_eq!(index.blocks[0].bytes, 1);
        assert_eq!(index.blocks[0].offset, index.payload_start);
        assert_eq!(index.blocks[1].raw_type_index, 0x8001);
        assert!(index.blocks[1].upper_bit_set);
        assert_eq!(index.blocks[1].type_name, "BSTriShape");
        assert_eq!(index.strings, [b"Root".to_vec(), b"child".to_vec()]);
        assert_eq!(index.groups, [42]);
        assert_eq!(index.roots, [Some(0), Some(2)]);
        assert_eq!(index.end_offset, bytes.len());
        assert_eq!(index.block_counts["BSTriShape"], 2);
    }

    #[test]
    fn rejects_every_truncation_and_unknown_tuple() {
        let bytes = sse_fixture();
        for end in 0..bytes.len() {
            assert!(
                index_sse(&bytes[..end], "truncated-sse.nif").is_err(),
                "end={end}"
            );
        }
        let mut unknown = bytes;
        let newline = unknown.iter().position(|byte| *byte == b'\n').unwrap();
        unknown[newline + 14..newline + 18].copy_from_slice(&83u32.to_le_bytes());
        assert!(index_sse(&unknown, "legacy-skyrim.nif").is_err());
    }

    #[test]
    fn rejects_bad_type_indices_roots_and_trailing_bytes() {
        let base = sse_fixture();
        let mut trailing = base.clone();
        trailing.push(0);
        assert!(index_sse(&trailing, "trailing.nif").is_err());

        let mut bad_root = base.clone();
        let root_offset = bad_root.len() - 8;
        bad_root[root_offset..root_offset + 4].copy_from_slice(&3u32.to_le_bytes());
        assert!(index_sse(&bad_root, "bad-root.nif").is_err());

        let mut bad_type = base;
        let type_table = bad_type
            .windows(b"NiNode".len())
            .position(|window| window == b"NiNode")
            .unwrap();
        let first_index = type_table + b"NiNode".len() + 4 + 10;
        bad_type[first_index..first_index + 2].copy_from_slice(&9u16.to_le_bytes());
        assert!(index_sse(&bad_type, "bad-type.nif").is_err());
    }
}
