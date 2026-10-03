//! Rebuildable plugin metadata, never record bodies or saved game state.
//! Raw FormIDs and parent labels stay local to their source; winners are rebuilt
//! for each supplied order, so a cached plugin cannot carry an old override winner.
use crate::{
    Error, Result, cache,
    content::{Definition, ParentContext, PayloadScope, PluginCensus, PluginIndex},
    identity::{ProfileId, plugin_name},
    plugin::{self, Limits, RecordHeader},
    script_inventory::ScriptReference,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, mem::size_of, path::Path};

const MAGIC: &[u8; 8] = b"FNVHIDX\0";
const VERSION: u32 = 1;
pub const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_SUMMARY: usize = 1024 * 1024;
const MAX_EDITOR_ID: usize = 4096;
const MAX_OWNED_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct PluginReceipt {
    pub plugin: String,
    pub source_sha256: String,
    pub key: String,
    pub reused: bool,
    pub index_bytes: u64,
    pub index_sha256: String,
    pub records: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub format: &'static str,
    pub source_bytes_hashed: u64,
    pub ordered_source_sha256: String,
    pub plugins: Vec<PluginReceipt>,
    pub scope: &'static str,
}

pub fn identity(name: &str, digest: String, limits: Limits) -> cache::ArtifactIdentity {
    cache::ArtifactIdentity {
        profile: ProfileId::NvOriginal,
        source_sha256: digest,
        path_bytes: name.as_bytes().to_vec(),
        transform_version: format!(
            "nv-header-index-v1;record={};decoded={};records={};depth={}",
            limits.max_record_bytes,
            limits.max_decoded_bytes,
            limits.max_records,
            limits.max_group_depth
        ),
    }
}

#[derive(Serialize, Deserialize)]
struct Summary {
    census: PluginCensus,
    script_references: Vec<ScriptReference>,
}
#[derive(Serialize)]
struct SummaryRef<'a> {
    census: &'a PluginCensus,
    script_references: &'a [ScriptReference],
}

fn fail(reason: &str) -> Error {
    Error::Resolution(format!("plugin index cache: {reason}"))
}

pub fn encode(index: &PluginIndex) -> Result<Vec<u8>> {
    if index.census.payload_scope != PayloadScope::CellMetadata
        || !index.census.integrity_issues.is_empty()
    {
        return Err(fail("only strict header indices may be cached"));
    }
    let summary = serde_json::to_vec(&SummaryRef {
        census: &index.census,
        script_references: &index.script_references,
    })
    .map_err(|e| fail(&e.to_string()))?;
    let records = u32::try_from(index.records.len()).map_err(|_| fail("record count overflow"))?;
    if summary.len() > MAX_SUMMARY {
        return Err(fail("summary exceeds input budget"));
    }
    let required = index
        .records
        .iter()
        .try_fold(20 + summary.len(), |bytes, record| {
            let name = match &record.editor_id {
                Some(name) if name.len() <= MAX_EDITOR_ID => 4 + name.len(),
                Some(_) => return Err(fail("editor ID exceeds input budget")),
                None => 0,
            };
            bytes
                .checked_add(48 + name)
                .ok_or_else(|| fail("byte size overflow"))
        })?;
    if required > MAX_BYTES {
        return Err(fail("encoded index exceeds input budget"));
    }
    let mut bytes = Vec::with_capacity(required);
    bytes.extend(MAGIC);
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend(records.to_le_bytes());
    bytes.extend((summary.len() as u32).to_le_bytes());
    bytes.extend(summary);
    for record in &index.records {
        let header = &record.header;
        bytes.extend(header.kind);
        bytes.extend(header.offset.to_le_bytes());
        bytes.extend(header.stored_size.to_le_bytes());
        bytes.extend(header.flags.to_le_bytes());
        bytes.extend(header.form_id.to_le_bytes());
        bytes.extend(header.revision);
        bytes.extend(header.version.to_le_bytes());
        bytes.extend(header.trailing_bytes);
        option(&mut bytes, record.parent.world);
        option(&mut bytes, record.parent.cell);
        option(&mut bytes, record.parent.child_group.map(|v| v as u32));
        bytes.push(u8::from(record.editor_id.is_some()));
        if let Some(name) = &record.editor_id {
            bytes.extend((name.len() as u32).to_le_bytes());
            bytes.extend(name);
        }
    }
    debug_assert_eq!(bytes.len(), required);
    Ok(bytes)
}

fn option(bytes: &mut Vec<u8>, value: Option<u32>) {
    bytes.push(u8::from(value.is_some()));
    bytes.extend(value.unwrap_or(0).to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.span(N)?.try_into().expect("fixed span"))
    }
    fn span(&mut self, size: usize) -> Result<&'a [u8]> {
        if size > self.bytes.len() - self.position {
            return Err(fail("truncated metadata"));
        }
        let start = self.position;
        self.position += size;
        Ok(&self.bytes[start..self.position])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn option(&mut self) -> Result<Option<u32>> {
        let flag = self.take::<1>()?[0];
        let value = self.u32()?;
        match flag {
            0 if value == 0 => Ok(None),
            1 => Ok(Some(value)),
            _ => Err(fail("noncanonical optional parent")),
        }
    }
}

pub fn decode(bytes: &[u8], name: &str, source_bytes: u64, limits: Limits) -> Result<PluginIndex> {
    if limits.inspect_checksum_mismatches || bytes.len() > MAX_BYTES {
        return Err(fail("strict mode or input budget violated"));
    }
    let mut reader = Reader { bytes, position: 0 };
    if &reader.take::<8>()? != MAGIC || reader.u32()? != VERSION {
        return Err(fail("unsupported metadata format"));
    }
    let count = reader.u32()? as usize;
    let summary_size = reader.u32()? as usize;
    if summary_size > MAX_SUMMARY {
        return Err(fail("summary exceeds input budget"));
    }
    let summary: Summary = serde_json::from_slice(reader.span(summary_size)?)
        .map_err(|e| fail(&format!("summary: {e}")))?;
    if count as u64 >= limits.max_records || count > (bytes.len() - reader.position) / 48 {
        return Err(fail("record count exceeds input or configured budget"));
    }
    let mut owned_left = MAX_OWNED_BYTES
        .checked_sub(
            count
                .checked_mul(size_of::<Definition>())
                .ok_or_else(|| fail("storage overflow"))?,
        )
        .ok_or_else(|| fail("metadata storage budget exceeded"))?;
    validate_census(&summary.census, name, source_bytes, count, limits)?;
    let mut records = Vec::with_capacity(count);
    let mut end = plugin::HEADER_SIZE;
    let mut ids = BTreeSet::new();
    for _ in 0..count {
        let header = RecordHeader {
            kind: reader.take()?,
            offset: u64::from_le_bytes(reader.take()?),
            stored_size: reader.u32()?,
            flags: reader.u32()?,
            form_id: reader.u32()?,
            revision: reader.take()?,
            version: u16::from_le_bytes(reader.take()?),
            trailing_bytes: reader.take()?,
        };
        let next = header
            .offset
            .checked_add(plugin::HEADER_SIZE)
            .and_then(|v| v.checked_add(u64::from(header.stored_size)))
            .ok_or_else(|| fail("record extent overflow"))?;
        if header.offset < end
            || next > source_bytes
            || header.stored_size as usize > limits.max_record_bytes
            || header.form_id == 0
            || !ids.insert(header.form_id)
            || header.kind == *b"GRUP"
            || header.kind == *b"TES4"
        {
            return Err(fail("invalid record extent, identity or order"));
        }
        end = next;
        let parent = ParentContext {
            world: reader.option()?,
            cell: reader.option()?,
            child_group: reader.option()?.map(|v| v as i32),
        };
        if parent
            .child_group
            .is_some_and(|kind| !(8..=10).contains(&kind) || parent.cell.is_none())
        {
            return Err(fail("invalid cell child group"));
        }
        let editor_id = match reader.take::<1>()?[0] {
            0 => None,
            1 => {
                let size = reader.u32()? as usize;
                if size > MAX_EDITOR_ID {
                    return Err(fail("editor ID exceeds input budget"));
                }
                owned_left = owned_left
                    .checked_sub(size)
                    .ok_or_else(|| fail("metadata storage budget exceeded"))?;
                let name = reader.span(size)?;
                if name.contains(&0) {
                    return Err(fail("editor ID contains NUL"));
                }
                Some(name.to_vec())
            }
            _ => return Err(fail("noncanonical optional editor ID")),
        };
        records.push(Definition {
            header,
            parent,
            editor_id,
        });
    }
    if reader.position != bytes.len() {
        return Err(fail("surplus metadata bytes"));
    }
    Ok(PluginIndex {
        census: summary.census,
        records,
        script_references: summary.script_references,
    })
}

fn validate_census(
    census: &PluginCensus,
    name: &str,
    bytes: u64,
    count: usize,
    limits: Limits,
) -> Result<()> {
    if census.name != name
        || census.source_bytes != bytes
        || census.payload_scope != PayloadScope::CellMetadata
        || !census.integrity_issues.is_empty()
        || census.records_excluding_header != count as u64
        || census
            .record_payloads_decoded
            .checked_add(census.record_payloads_deferred)
            != Some(count as u64 + 1)
        || census.record_payloads_decoded == 0
        || census
            .record_kinds
            .values()
            .try_fold(0u64, |a, b| a.checked_add(b.occurrences))
            != Some(count as u64 + 1)
        || census
            .record_kinds
            .values()
            .try_fold(0u64, |a, b| a.checked_add(b.decoded_bytes))
            .is_none_or(|v| v > limits.max_decoded_bytes)
        || ![1.32f32.to_bits(), 1.33f32.to_bits(), 1.34f32.to_bits()]
            .contains(&census.header_version.to_bits())
        || census.masters.len() > 254
    {
        return Err(fail("summary does not match this strict source index"));
    }
    let mut masters = BTreeSet::new();
    for master in &census.masters {
        if !masters.insert(plugin_name(master)?) {
            return Err(fail("duplicate master"));
        }
    }
    Ok(())
}

pub(crate) fn load_or_build(
    root: &Path,
    source_tree: &Path,
    path: &Path,
    identity: cache::ArtifactIdentity,
    source_bytes: u64,
    limits: Limits,
) -> Result<(PluginIndex, PluginReceipt)> {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| fail("plugin filename encoding"))?;
    let (index, cached) = if let Some((receipt, bytes)) =
        cache::read_verified(root, source_tree, &identity, MAX_BYTES)?
    {
        (decode(&bytes, name, source_bytes, limits)?, receipt)
    } else {
        let index = crate::content::index_plugin_headers(path, limits)?;
        let bytes = encode(&index)?;
        let cached = cache::publish(root, source_tree, identity.clone(), &bytes)?;
        (index, cached)
    };
    let receipt = PluginReceipt {
        plugin: name.into(),
        source_sha256: identity.source_sha256,
        key: cached.key,
        reused: cached.reused,
        index_bytes: cached.manifest.bytes,
        index_sha256: cached.manifest.sha256,
        records: index.records.len(),
    };
    Ok((index, receipt))
}
