//! FRSAVE01: two versioned chunks and a whole-container SHA-256. Every extent is
//! checked before slicing. Unknown versions/chunks are rejected, never dropped.
use super::{Captured, Error, Result};
use crate::{
    Limits,
    identity::CampaignId,
    snapshot::{self, Snapshot},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MAGIC: &[u8; 8] = b"FRSAVE01";
pub const CONTAINER_VERSION: u16 = 1;
pub const OVERHEAD: usize = 232;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Metadata {
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub generation: u64,
    pub boundary_tick: u64,
    pub catalogue_sha256: String,
    pub snapshot_bytes: usize,
    pub snapshot_sha256: String,
    pub container_bytes: usize,
    pub container_sha256: String,
}
pub struct Decoded {
    pub metadata: Metadata,
    pub snapshot: Snapshot,
}

fn fail(reason: &str) -> Error {
    Error::Format(reason.into())
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn from_hex(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(fail("noncanonical catalogue digest"));
    }
    let mut result = [0; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
        result[index] = (digit(pair[0]) << 4) | digit(pair[1]);
    }
    Ok(result)
}
fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], bytes: &[u8]) {
    out.extend(tag);
    out.extend(1_u32.to_le_bytes());
    out.extend((bytes.len() as u64).to_le_bytes());
    out.extend(Sha256::digest(bytes));
    out.extend(bytes);
}
pub fn encode(capture: &Captured, generation: u64) -> Result<Vec<u8>> {
    if generation == 0 {
        return Err(fail("zero save generation"));
    }
    let snapshot = capture.snapshot.encode(capture.limits.max_snapshot_bytes)?;
    let mut meta = Vec::with_capacity(88);
    meta.extend(1_u32.to_le_bytes()); // Profile adapter 1 is original NV only.
    meta.extend(snapshot::SCHEMA_VERSION.to_le_bytes());
    meta.extend(generation.to_le_bytes());
    meta.extend(capture.snapshot.clocks.tick.to_le_bytes());
    meta.extend(from_hex(&capture.snapshot.catalogue_sha256)?);
    meta.extend((snapshot.len() as u64).to_le_bytes());
    meta.extend(capture.snapshot.campaign.bytes());
    meta.extend(capture.snapshot.state_revision.to_le_bytes());
    let size = snapshot
        .len()
        .checked_add(OVERHEAD)
        .ok_or_else(|| fail("container size overflow"))?;
    let mut out = Vec::with_capacity(size);
    out.extend(MAGIC);
    out.extend(CONTAINER_VERSION.to_le_bytes());
    out.extend(0_u16.to_le_bytes());
    out.extend(2_u32.to_le_bytes());
    chunk(&mut out, b"META", &meta);
    chunk(&mut out, b"STAT", &snapshot);
    let checksum = Sha256::digest(&out);
    out.extend(checksum);
    Ok(out)
}
struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        if count > self.bytes.len().saturating_sub(self.cursor) {
            return Err(fail("truncated container extent"));
        }
        let start = self.cursor;
        self.cursor += count;
        Ok(&self.bytes[start..self.cursor])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("checked extent"),
        ))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("checked extent"),
        ))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("checked extent"),
        ))
    }
    fn chunk(&mut self, tag: &[u8; 4], maximum: usize) -> Result<&'a [u8]> {
        if self.take(4)? != tag || self.u32()? != 1 {
            return Err(fail("unknown chunk or chunk version"));
        }
        let length = usize::try_from(self.u64()?).map_err(|_| fail("chunk size overflow"))?;
        if length > maximum {
            return Err(fail("chunk exceeds byte budget"));
        }
        let checksum = self.take(32)?;
        let payload = self.take(length)?;
        if Sha256::digest(payload).as_slice() != checksum {
            return Err(fail("chunk integrity mismatch"));
        }
        Ok(payload)
    }
}
/// Migration receipts identify the original bytes separately from the new state.
/// The migrated snapshot still requires full source-bound World::restore.
pub struct Migration {
    pub source_state_schema: u32,
    pub source_metadata: Metadata,
    pub snapshot: Snapshot,
}
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Decoded> {
    decode_schema(bytes, limits, snapshot::SCHEMA_VERSION)
}
/// Explicitly import our schema-2 native container. Normal decode stays strict.
/// This never reads Bethesda saves and never initializes absent inventory state.
pub fn migrate_v2(bytes: &[u8], limits: Limits) -> Result<Migration> {
    let decoded = decode_schema(bytes, limits, 2)?;
    Ok(Migration {
        source_state_schema: 2,
        source_metadata: decoded.metadata,
        snapshot: decoded.snapshot,
    })
}
fn decode_schema(bytes: &[u8], limits: Limits, source_schema: u32) -> Result<Decoded> {
    let maximum = limits
        .max_snapshot_bytes
        .checked_add(OVERHEAD)
        .ok_or_else(|| fail("file budget overflow"))?;
    if bytes.len() < OVERHEAD || bytes.len() > maximum {
        return Err(fail("container byte budget/extent"));
    }
    let unsigned = &bytes[..bytes.len() - 32];
    if Sha256::digest(unsigned).as_slice() != &bytes[bytes.len() - 32..] {
        return Err(fail("whole-container integrity mismatch"));
    }
    let mut reader = Reader {
        bytes: unsigned,
        cursor: 0,
    };
    if reader.take(8)? != MAGIC
        || reader.u16()? != CONTAINER_VERSION
        || reader.u16()? != 0
        || reader.u32()? != 2
    {
        return Err(fail("unsupported header, version, flags or chunk count"));
    }
    let meta = reader.chunk(b"META", 88)?;
    if meta.len() != 88 {
        return Err(fail("metadata extent"));
    }
    let mut meta = Reader {
        bytes: meta,
        cursor: 0,
    };
    if meta.u32()? != 1 || meta.u32()? != source_schema {
        return Err(fail("unsupported profile or state schema"));
    }
    let generation = meta.u64()?;
    if generation == 0 {
        return Err(fail("zero save generation"));
    }
    let boundary_tick = meta.u64()?;
    let cohort = meta.take(32)?;
    let snapshot_bytes =
        usize::try_from(meta.u64()?).map_err(|_| fail("snapshot extent overflow"))?;
    let campaign =
        CampaignId::from_bytes(meta.take(16)?.try_into().expect("checked campaign extent"))?;
    let state_revision = meta.u64()?;
    let body = reader.chunk(b"STAT", limits.max_snapshot_bytes)?;
    if body.len() != snapshot_bytes || reader.cursor != unsigned.len() {
        return Err(fail("snapshot extent mismatch or trailing container bytes"));
    }
    let snapshot = if source_schema == 2 {
        Snapshot::migrate_v2(body, limits)?
    } else {
        Snapshot::decode(body, limits)?
    };
    if snapshot.campaign != campaign
        || snapshot.state_revision != state_revision
        || snapshot.clocks.tick != boundary_tick
        || from_hex(&snapshot.catalogue_sha256)?.as_slice() != cohort
        || snapshot.schema_version != snapshot::SCHEMA_VERSION
        || snapshot.profile != fallout_data::identity::ProfileId::NvOriginal
    {
        return Err(fail("metadata and canonical state disagree"));
    }
    Ok(Decoded {
        metadata: Metadata {
            campaign,
            state_revision,
            generation,
            boundary_tick,
            catalogue_sha256: snapshot.catalogue_sha256.clone(),
            snapshot_bytes,
            snapshot_sha256: digest(body),
            container_bytes: bytes.len(),
            container_sha256: digest(bytes),
        },
        snapshot,
    })
}
