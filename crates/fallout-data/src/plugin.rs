//! FNV record framing; field semantics belong in separate, versioned decoders.
//! Layout evidence: pinned xEdit wbRecordHeader/wbHEDR and esplugin (sources.lock.json).

use crate::{Result, malformed};
use flate2::{Decompress, FlushDecompress, Status};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};

pub const HEADER_SIZE: u64 = 24;
pub const COMPRESSED: u32 = 0x0004_0000;
pub const DELETED: u32 = 0x20;
pub const PERSISTENT: u32 = 0x400;
pub const INITIALLY_DISABLED: u32 = 0x800;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Diagnostic census only. Recovered data is marked untrusted, never accepted.
    pub inspect_checksum_mismatches: bool,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: u64,
    pub max_records: u64,
    pub max_group_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            inspect_checksum_mismatches: false,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 4 * 1024 * 1024 * 1024,
            max_records: 4_000_000,
            max_group_depth: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub offset: u64,
    pub size: u32,
    pub label: [u8; 4],
    pub kind: i32,
    pub trailing_bytes: [u8; 8],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordHeader {
    pub kind: [u8; 4],
    pub offset: u64,
    pub stored_size: u32,
    pub flags: u32,
    pub form_id: u32,
    pub revision: [u8; 4],
    pub version: u16,
    pub trailing_bytes: [u8; 2],
}

#[derive(Debug)]
pub struct Record {
    pub header: RecordHeader,
    /// Exact decoded body, including fields that the runtime does not understand yet.
    pub payload: Vec<u8>,
    pub integrity_issue: Option<ChecksumMismatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumMismatch {
    pub file_offset: u64,
    pub form_id: u32,
    pub stored_adler32: u32,
    pub calculated_adler32: u32,
}

#[derive(Debug)]
pub struct Subrecord<'a> {
    pub kind: [u8; 4],
    /// Offset inside the decoded payload, not an absolute file offset when compressed.
    pub payload_offset: usize,
    pub data: &'a [u8],
}

pub enum Event<'a> {
    Group(&'a Group),
    Record(&'a Record),
}

/// A bounded index can retain a header without decoding its body. Such a header
/// establishes an extent and identity, never payload integrity or field semantics.
pub enum SelectedEvent<'a> {
    Group(&'a Group),
    Record(&'a Record),
    Deferred(&'a RecordHeader),
}

fn record_header(h: &[u8; 24], offset: u64) -> RecordHeader {
    RecordHeader {
        kind: h[..4].try_into().expect("fixed header"),
        offset,
        stored_size: u32_at(h, 4),
        flags: u32_at(h, 8),
        form_id: u32_at(h, 12),
        revision: h[16..20].try_into().expect("fixed header"),
        version: u16::from_le_bytes([h[20], h[21]]),
        trailing_bytes: [h[22], h[23]],
    }
}

fn u32_at(bytes: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
}

/// Walk iteratively so a malicious group tree cannot exhaust the call stack.
pub fn visit(
    reader: &mut impl Read,
    length: u64,
    name: &str,
    limits: Limits,
    mut visitor: impl FnMut(Event<'_>) -> Result<()>,
) -> Result<()> {
    visit_selected(
        reader,
        length,
        name,
        limits,
        |_| true,
        |event| match event {
            SelectedEvent::Group(group) => visitor(Event::Group(group)),
            SelectedEvent::Record(record) => visitor(Event::Record(record)),
            SelectedEvent::Deferred(_) => unreachable!("full visitor selects every body"),
        },
    )
}

/// Walk every record/group boundary, decoding only selected bodies. TES4 is always
/// selected. Deferred bytes are consumed without allocation, even on a plain Read.
pub fn visit_selected(
    reader: &mut impl Read,
    length: u64,
    name: &str,
    limits: Limits,
    mut select: impl FnMut(&RecordHeader) -> bool,
    mut visitor: impl FnMut(SelectedEvent<'_>) -> Result<()>,
) -> Result<()> {
    if length < HEADER_SIZE {
        return Err(malformed(name, 0, "missing TES4 header"));
    }
    let mut offset = 0u64;
    let mut ends = vec![length];
    let mut records = 0u64;
    let mut decoded = 0u64;
    while offset < length {
        while ends.len() > 1 && ends.last() == Some(&offset) {
            ends.pop();
        }
        let boundary = ends[ends.len() - 1];
        if boundary.saturating_sub(offset) < HEADER_SIZE {
            return Err(malformed(
                name,
                offset,
                "truncated header at group boundary",
            ));
        }
        let mut h = [0u8; 24];
        reader
            .read_exact(&mut h)
            .map_err(|e| malformed(name, offset, e.to_string()))?;
        let kind: [u8; 4] = h[..4].try_into().expect("fixed header slice");
        if offset == 0 && kind != *b"TES4" {
            return Err(malformed(name, 0, "expected TES4"));
        }
        let size = u32_at(&h, 4);
        if kind == *b"GRUP" {
            if size < 24 || offset + u64::from(size) > boundary {
                return Err(malformed(name, offset, "group extent exceeds its parent"));
            }
            if ends.len() > limits.max_group_depth {
                return Err(malformed(name, offset, "group nesting limit exceeded"));
            }
            let group = Group {
                offset,
                size,
                label: h[8..12].try_into().expect("fixed slice"),
                kind: i32::from_le_bytes(h[12..16].try_into().expect("fixed slice")),
                trailing_bytes: h[16..24].try_into().expect("fixed slice"),
            };
            visitor(SelectedEvent::Group(&group))?;
            ends.push(offset + u64::from(size));
            offset += HEADER_SIZE;
            continue;
        }
        records += 1;
        if records > limits.max_records {
            return Err(malformed(name, offset, "record count limit exceeded"));
        }
        if size as usize > limits.max_record_bytes
            || u64::from(size) > boundary - offset - HEADER_SIZE
        {
            return Err(malformed(
                name,
                offset,
                "record exceeds size limit or group boundary",
            ));
        }
        if offset != 0 && kind == *b"TES4" {
            return Err(malformed(name, offset, "duplicate TES4 header"));
        }
        let header = record_header(&h, offset);
        if offset == 0 || select(&header) {
            let record = read_body(reader, &h, offset, name, limits, decoded)?;
            decoded += record.payload.len() as u64;
            visitor(SelectedEvent::Record(&record))?;
        } else {
            let copied = std::io::copy(
                &mut (&mut *reader).take(u64::from(size)),
                &mut std::io::sink(),
            )
            .map_err(|e| malformed(name, offset, e.to_string()))?;
            if copied != u64::from(size) {
                return Err(malformed(name, offset, "truncated deferred record body"));
            }
            visitor(SelectedEvent::Deferred(&header))?;
        }
        offset += HEADER_SIZE + u64::from(size);
    }
    Ok(())
}

/// Read a record previously located by a complete bounded scan. The stored header
/// must still match, and the caller must keep the source immutable while indexed.
pub fn read_indexed(
    reader: &mut (impl Read + Seek),
    length: u64,
    expected: &RecordHeader,
    name: &str,
    limits: Limits,
) -> Result<Record> {
    let end = expected
        .offset
        .checked_add(HEADER_SIZE)
        .and_then(|n| n.checked_add(u64::from(expected.stored_size)))
        .ok_or_else(|| malformed(name, expected.offset, "indexed extent overflow"))?;
    if end > length || expected.kind == *b"GRUP" {
        return Err(malformed(
            name,
            expected.offset,
            "indexed record exceeds source",
        ));
    }
    reader
        .seek(SeekFrom::Start(expected.offset))
        .map_err(|e| malformed(name, expected.offset, e.to_string()))?;
    let mut h = [0u8; 24];
    reader
        .read_exact(&mut h)
        .map_err(|e| malformed(name, expected.offset, e.to_string()))?;
    let actual = record_header(&h, expected.offset);
    if &actual != expected {
        return Err(malformed(
            name,
            expected.offset,
            "source header changed since indexing",
        ));
    }
    read_body(reader, &h, expected.offset, name, limits, 0)
}

fn read_body(
    reader: &mut impl Read,
    h: &[u8; 24],
    offset: u64,
    name: &str,
    limits: Limits,
    decoded: u64,
) -> Result<Record> {
    let size = u32_at(h, 4);
    let kind = h[..4].try_into().expect("fixed header");
    if size as usize > limits.max_record_bytes {
        return Err(malformed(name, offset, "record size budget exceeded"));
    }
    let flags = u32_at(h, 8);
    let mut integrity_issue = None;
    let mut payload = vec![0u8; size as usize];
    reader
        .read_exact(&mut payload)
        .map_err(|e| malformed(name, offset, e.to_string()))?;
    if flags & COMPRESSED != 0 {
        if payload.len() < 4 {
            return Err(malformed(
                name,
                offset,
                "compressed body lacks decoded size",
            ));
        }
        let expected = u32_at(&payload, 0) as usize;
        if expected > limits.max_record_bytes
            || expected as u64 > limits.max_decoded_bytes.saturating_sub(decoded)
        {
            return Err(malformed(name, offset, "decompression budget exceeded"));
        }
        // The extra byte lets us detect an understated length without an unbounded read.
        let mut body = vec![0; expected + 1];
        let mut decoder = Decompress::new(true);
        let decoded_result = decoder.decompress(&payload[4..], &mut body, FlushDecompress::Finish);
        let status = match decoded_result {
            Ok(status) => status,
            Err(error) if limits.inspect_checksum_mismatches => {
                // Recover only a well-framed deflate stream with the exact expected
                // length and a demonstrably wrong Adler checksum. All other faults fail.
                let (recovered, stored, calculated) = inspect_checksum(&payload, expected)
                    .ok_or_else(|| {
                        malformed(
                            name,
                            offset,
                            format!("zlib: {error}; not a checksum-only mismatch"),
                        )
                    })?;
                body = recovered;
                integrity_issue = Some(ChecksumMismatch {
                    file_offset: offset,
                    form_id: u32_at(h, 12),
                    stored_adler32: stored,
                    calculated_adler32: calculated,
                });
                Status::StreamEnd
            }
            Err(error) => {
                return Err(malformed(
                    name,
                    offset,
                    format!("zlib: {error}; strict integrity check failed"),
                ));
            }
        };
        if integrity_issue.is_none()
            && (status != Status::StreamEnd
                || decoder.total_out() != expected as u64
                || decoder.total_in() != (payload.len() - 4) as u64)
        {
            return Err(malformed(
                name,
                offset,
                "zlib length, checksum, or trailing data mismatch",
            ));
        }
        body.truncate(expected);
        payload = body;
    }
    if payload.len() as u64 > limits.max_decoded_bytes.saturating_sub(decoded) {
        return Err(malformed(name, offset, "decoded byte budget exceeded"));
    }
    Ok(Record {
        header: RecordHeader {
            kind,
            offset,
            stored_size: size,
            flags,
            form_id: u32_at(h, 12),
            revision: h[16..20].try_into().expect("fixed slice"),
            version: u16::from_le_bytes([h[20], h[21]]),
            trailing_bytes: [h[22], h[23]],
        },
        payload,
        integrity_issue,
    })
}

fn inspect_checksum(payload: &[u8], expected: usize) -> Option<(Vec<u8>, u32, u32)> {
    let stream = payload.get(4..)?;
    if stream.len() < 6
        || stream[0] & 0x0f != 8
        || stream[0] >> 4 > 7
        || stream[1] & 0x20 != 0
        || u16::from_be_bytes([stream[0], stream[1]]) % 31 != 0
    {
        return None;
    }
    let deflate = &stream[2..stream.len() - 4];
    let mut decoder = Decompress::new(false);
    let mut output = vec![0; expected + 1];
    if decoder
        .decompress(deflate, &mut output, FlushDecompress::Finish)
        .ok()?
        != Status::StreamEnd
        || decoder.total_in() != deflate.len() as u64
        || decoder.total_out() != expected as u64
    {
        return None;
    }
    output.truncate(expected);
    let stored = u32::from_be_bytes(stream[stream.len() - 4..].try_into().ok()?);
    let calculated = adler2::adler32_slice(&output);
    (stored != calculated).then_some((output, stored, calculated))
}

pub fn visit_subrecords(
    record: &Record,
    name: &str,
    mut visitor: impl FnMut(Subrecord<'_>) -> Result<()>,
) -> Result<()> {
    let bytes = &record.payload;
    let mut pos = 0usize;
    let mut extended = None;
    while pos < bytes.len() {
        if bytes.len() - pos < 6 {
            return Err(malformed(
                name,
                record.header.offset,
                format!("short subrecord at decoded +0x{pos:X}"),
            ));
        }
        let kind: [u8; 4] = bytes[pos..pos + 4].try_into().expect("checked slice");
        let short = u16::from_le_bytes([bytes[pos + 4], bytes[pos + 5]]) as usize;
        let start = pos;
        pos += 6;
        if kind == *b"XXXX" {
            if short != 4 || extended.is_some() || bytes.len() - pos < 4 {
                return Err(malformed(
                    name,
                    record.header.offset,
                    "malformed extended subrecord size",
                ));
            }
            extended = Some(u32_at(bytes, pos) as usize);
            pos += 4;
            continue;
        }
        let size = extended.take().unwrap_or(short);
        if size > bytes.len() - pos {
            return Err(malformed(
                name,
                record.header.offset,
                format!("subrecord at decoded +0x{start:X} exceeds body"),
            ));
        }
        visitor(Subrecord {
            kind,
            payload_offset: start,
            data: &bytes[pos..pos + size],
        })?;
        pos += size;
    }
    if extended.is_some() {
        return Err(malformed(name, record.header.offset, "orphan XXXX prefix"));
    }
    Ok(())
}

pub fn signature(kind: [u8; 4]) -> String {
    kind.iter()
        .map(|b| {
            if b.is_ascii_graphic() {
                char::from(*b).to_string()
            } else {
                format!("\\x{b:02X}")
            }
        })
        .collect()
}
