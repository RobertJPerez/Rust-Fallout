//! Bounded Skyrim NIF header evidence from an existing TES4 BSA archive.
//!
//! This intentionally does not decode NIF blocks or certify runtime support.
use crate::{
    Result,
    archive::{NifArchiveIndex, SkyrimArchive},
    bad,
};
use fallout_data::baseline::digest_reader;
use fallout_data::vfs::AssetPath;
use serde::Serialize;
use std::{io::Cursor, path::Path};

const HEADER_PREFIX: &[u8] = b"Gamebryo File Format, Version ";
const SSE_HEADER_LINE: &str = "Gamebryo File Format, Version 20.2.0.7";
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub archive: String,
    pub archive_version: u32,
    pub member_path: Vec<u8>,
    pub asset_bytes: u64,
    pub asset_sha256: String,
    pub header: HeaderEvidence,
    pub scope: &'static str,
}

#[derive(Debug, Serialize)]
pub struct HeaderEvidence {
    pub text_line: String,
    pub textual_version: Option<String>,
    pub binary_version: Option<u32>,
    pub endian_marker: Option<u8>,
    pub user_version: Option<u32>,
    pub block_count: Option<u32>,
    pub user_version_2: Option<u32>,
    pub tuple_bytes_hex: String,
    pub classification: &'static str,
    pub skyrim_se_header_tuple: bool,
}

fn u32_at(bytes: &[u8], start: usize) -> Option<u32> {
    let end = start.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(start..end)?.try_into().ok()?))
}

/// Read only the common text line and fixed header tuple; leave every block opaque.
pub fn parse_header(source: &str, bytes: &[u8]) -> Result<HeaderEvidence> {
    let newline = bytes
        .iter()
        .take(129)
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| bad(source, 0, "NIF header line is missing or exceeds 128 bytes"))?;
    let line = bytes
        .get(..newline)
        .ok_or_else(|| bad(source, 0, "invalid NIF header line bounds"))?;
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let text_line = std::str::from_utf8(line)
        .map_err(|_| bad(source, 0, "NIF header line is not UTF-8/ASCII"))?
        .to_owned();
    let textual_version = line
        .strip_prefix(HEADER_PREFIX)
        .and_then(|version| std::str::from_utf8(version).ok())
        .map(str::to_owned);
    let tail = bytes.get(newline + 1..).unwrap_or_default();
    let tuple = tail.get(..tail.len().min(17)).unwrap_or_default();
    let tuple_bytes_hex = tuple
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let binary_version = u32_at(tail, 0);
    let endian_marker = tail.get(4).copied();
    let user_version = u32_at(tail, 5);
    let block_count = u32_at(tail, 9);
    let user_version_2 = u32_at(tail, 13);

    let skyrim_se_header_tuple = text_line == SSE_HEADER_LINE
        && binary_version == Some(0x1402_0007)
        && endian_marker == Some(1)
        && user_version == Some(12)
        && user_version_2 == Some(100);
    let classification = if skyrim_se_header_tuple {
        "Skyrim SE stream header tuple; format evidence only"
    } else if text_line == SSE_HEADER_LINE
        && binary_version == Some(0x1402_0007)
        && endian_marker == Some(1)
        && user_version == Some(12)
        && user_version_2 == Some(83)
    {
        "Skyrim legacy stream header tuple; distinct from stream-100 profile; runtime use unverified"
    } else {
        "unclassified NIF header; block layout unsupported by this probe"
    };

    Ok(HeaderEvidence {
        text_line,
        textual_version,
        binary_version,
        endian_marker,
        user_version,
        block_count,
        user_version_2,
        tuple_bytes_hex,
        classification,
        skyrim_se_header_tuple,
    })
}

pub fn inspect_archive_member(archive_path: &Path, member_path: &[u8]) -> Result<Report> {
    let (archive_name, archive_version, actual_path, data) =
        read_archive_member(archive_path, member_path)?;
    let header = parse_header(
        &format!("{archive_name}:{}", String::from_utf8_lossy(&actual_path)),
        &data,
    )?;
    let (asset_bytes, asset_sha256) = digest_bytes(&data)?;
    Ok(Report {
        schema_version: 1,
        archive: archive_name,
        archive_version,
        member_path: actual_path,
        asset_bytes,
        asset_sha256,
        header,
        scope: "one bounded archive member; NIF text header and version tuple only; no block, mesh, collision, animation, or runtime compatibility claim",
    })
}

pub(crate) fn read_archive_member(
    archive_path: &Path,
    member_path: &[u8],
) -> Result<(String, u32, Vec<u8>, Vec<u8>)> {
    let normalized = AssetPath::new(member_path)?;
    if !normalized.bytes().ends_with(b".nif") {
        return Err(crate::Error::Unsupported(
            "NIF archive inspection requires an archive member ending in .nif".into(),
        ));
    }
    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::Unsupported("archive path needs a Unicode filename".into()))?
        .to_owned();
    let archive = SkyrimArchive::open(archive_path)?;
    let (actual_path, data) = archive.read_path_bounded(member_path, MAX_ASSET_BYTES)?;
    Ok((archive_name, 105, actual_path, data))
}

pub(crate) fn digest_bytes(bytes: &[u8]) -> Result<(u64, String)> {
    Ok(digest_reader(&mut Cursor::new(bytes))?)
}

pub fn list_archive_nifs(
    archive_path: &Path,
    prefix: Option<&[u8]>,
    offset: usize,
    limit: usize,
) -> Result<NifArchiveIndex> {
    SkyrimArchive::open(archive_path)?.list_nif_paths(prefix, offset, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(stream_version: u32) -> Vec<u8> {
        let mut bytes = SSE_HEADER_LINE.as_bytes().to_vec();
        bytes.push(b'\n');
        bytes.extend_from_slice(&0x1402_0007u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&stream_version.to_le_bytes());
        bytes
    }

    #[test]
    fn identifies_only_the_se_stream_tuple_and_keeps_block_count_as_evidence() {
        let parsed = parse_header("synthetic.nif", &header(100)).unwrap();
        assert!(parsed.skyrim_se_header_tuple);
        assert_eq!(parsed.binary_version, Some(0x1402_0007));
        assert_eq!(parsed.user_version, Some(12));
        assert_eq!(parsed.block_count, Some(3));
        assert_eq!(parsed.user_version_2, Some(100));
        assert!(parsed.classification.contains("format evidence only"));
    }

    #[test]
    fn classifies_legacy_and_unknown_tuples_without_claiming_support() {
        let legacy = parse_header("legacy.nif", &header(83)).unwrap();
        assert!(!legacy.skyrim_se_header_tuple);
        assert!(legacy.classification.contains("distinct from stream-100"));

        let unknown = parse_header("unknown.nif", &header(999)).unwrap();
        assert!(!unknown.skyrim_se_header_tuple);
        assert!(unknown.classification.starts_with("unclassified"));
    }

    #[test]
    fn preserves_a_partial_binary_tuple_as_unclassified_evidence() {
        let mut bytes = SSE_HEADER_LINE.as_bytes().to_vec();
        bytes.extend_from_slice(b"\n\x07\x00\x02");
        let partial = parse_header("partial.nif", &bytes).unwrap();
        assert_eq!(partial.textual_version.as_deref(), Some("20.2.0.7"));
        assert_eq!(partial.binary_version, None);
        assert_eq!(partial.tuple_bytes_hex, "070002");
        assert!(!partial.skyrim_se_header_tuple);
        assert!(partial.classification.starts_with("unclassified"));
    }

    #[test]
    fn rejects_truncated_or_unbounded_text_headers() {
        assert!(parse_header("truncated.nif", b"Gamebryo File Format").is_err());
        let too_long = vec![b'a'; 130];
        assert!(parse_header("long.nif", &too_long).is_err());
    }
}
