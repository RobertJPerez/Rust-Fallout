//! Explicit CELL environment links and winning target header provenance.
//! Target behavior and parent/default selection remain unevaluated.
use super::{
    Dependency, SourceField, decode_cell,
    dependencies::{FieldSite, Span, source_cohort},
    dependency,
};
use crate::{
    Error, Result,
    identity::{FormKey, ProfileId},
    plugin::{self, RecordHeader},
    store::{Location, RecordStore, SourceReceipt},
};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{io::Write, sync::Arc};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub sources: usize,
    pub records: usize,
    pub fields: usize,
    pub links: usize,
    pub record_bytes: usize,
    pub read_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            sources: 256,
            records: 6,
            fields: 65536,
            links: 5,
            record_bytes: 4 * 1024 * 1024,
            read_bytes: 4 * 1024 * 1024,
            raw_bytes: 256,
            metadata_bytes: 16 * 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let max = Self::default();
        for (a, b) in [
            (self.sources, max.sources),
            (self.records, max.records),
            (self.fields, max.fields),
            (self.links, max.links),
            (self.record_bytes, max.record_bytes),
            (self.read_bytes, max.read_bytes),
            (self.raw_bytes, max.raw_bytes),
            (self.metadata_bytes, max.metadata_bytes),
        ] {
            if a > b {
                return Err(failure("limit exceeds ceiling"));
            }
        }
        Ok(self)
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub sources: usize,
    pub records: usize,
    pub fields: usize,
    pub links: usize,
    pub read_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct RecordSource {
    pub key: FormKey,
    pub source_ordinal: usize,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: RecordHeader,
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub kind: [u8; 4],
    /// Zero-based actual subrecord header ordinal, counting an XXXX prefix.
    pub physical_field_ordinal: usize,
    /// Existing visitor callback ordinal, excluding an XXXX prefix.
    pub logical_field_ordinal: usize,
    pub site: FieldSite,
    pub decoded_framing_offset: usize,
    pub framing: Vec<u8>,
    pub stored_body_offset: u64,
    pub stored_body_bytes: u32,
    pub physical_framing_offset: Option<u64>,
    pub raw: u32,
    pub target: Dependency,
    pub source: Option<RecordSource>,
    pub header_source_available: bool,
    pub behavior_status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub identity: String,
    pub source_cohort_sha256: String,
    pub sources: Vec<SourceReceipt>,
    pub cell: RecordSource,
    pub cell_decoded_sha256: String,
    pub cell_flags: SourceField<u8>,
    pub links: Vec<Link>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
/// Immutable source closure; serialized reports cannot construct a request.
pub struct CellEnvironmentSources(Arc<Receipt>);
impl CellEnvironmentSources {
    pub fn receipt(&self) -> &Receipt {
        &self.0
    }
    pub fn root(&self) -> &FormKey {
        &self.0.cell.key
    }
    pub fn identity(&self) -> &str {
        &self.0.identity
    }
    pub fn validate_sources(&self, store: &mut RecordStore) -> Result<()> {
        if store.indices().len() != self.0.sources.len()
            || store
                .indices()
                .iter()
                .zip(&self.0.sources)
                .any(|(index, source)| index.census.name != source.source_name)
        {
            return Err(failure("ordered source names/count changed"));
        }
        if store
            .source_receipts()?
            .iter()
            .zip(&self.0.sources)
            .any(|(a, b)| a.source_bytes != b.source_bytes || a.source_sha256 != b.source_sha256)
        {
            return Err(failure("ordered source bytes changed"));
        }
        Ok(())
    }
    pub fn load(store: &mut RecordStore, root: &FormKey, limits: Limits) -> Result<Self> {
        let mut budget = Budget {
            limits: limits.validate()?,
            usage: Usage::default(),
        };
        if root.profile != ProfileId::NvOriginal {
            return Err(failure("requires nv-original CELL"));
        }
        let location = store.winner(root).ok_or_else(|| failure("CELL missing"))?;
        let header = &store.definition(location).header;
        if header.kind != *b"CELL" || header.flags & plugin::DELETED != 0 {
            return Err(failure("CELL deleted or wrong kind"));
        }
        budget.metadata(4096)?;
        add(
            &mut budget.usage.sources,
            store.indices().len(),
            budget.limits.sources,
            "sources",
        )?;
        for source in store.indices() {
            budget.metadata(512 + 4 * source.census.name.len())?;
        }
        let sources = store.source_receipts()?;
        let cohort = source_cohort(&sources);
        let cell = identity(store, root, location, &mut budget)?;
        let record = store.read_bounded(
            location,
            budget.limits.record_bytes.min(budget.limits.read_bytes),
        )?;
        add(
            &mut budget.usage.read_bytes,
            record.payload.len().max(record.header.stored_size as usize),
            budget.limits.read_bytes,
            "read bytes",
        )?;
        if record.integrity_issue.is_some() {
            return Err(failure("tainted requested CELL body"));
        }
        budget.metadata(record.payload.len())?;
        let cell_flags = decode_cell(&record, &cell.source_plugin)?.flags;
        let mut links = Vec::new();
        let mut frame_start = 0;
        let mut physical_ordinal = 0;
        let mut logical_ordinal = 0;
        plugin::visit_subrecords(&record, &cell.source_plugin, |sub| {
            // The existing visitor permits one validated ten-byte XXXX prefix.
            let prefix = sub
                .payload_offset
                .checked_sub(frame_start)
                .ok_or_else(|| failure("visitor offset order"))?;
            if prefix != 0 && prefix != 10 {
                return Err(failure("visitor framing extent changed"));
            }
            let extended = usize::from(prefix == 10);
            add(
                &mut budget.usage.fields,
                1 + extended,
                budget.limits.fields,
                "physical fields",
            )?;
            let field_ordinal = physical_ordinal + extended;
            physical_ordinal = field_ordinal + 1;
            let callback_ordinal = logical_ordinal;
            logical_ordinal += 1;
            let start = frame_start;
            let end = sub.payload_offset + 6 + sub.data.len();
            frame_start = end;
            let Some(expected) = expected(sub.kind) else {
                return Ok(());
            };
            if links.iter().any(|link: &Link| link.kind == sub.kind) {
                return Err(failure("duplicate environment field"));
            }
            if sub.data.len() != 4 {
                return Err(failure("environment field must have four bytes"));
            }
            add(&mut budget.usage.links, 1, budget.limits.links, "link rows")?;
            add(
                &mut budget.usage.raw_bytes,
                end - start,
                budget.limits.raw_bytes,
                "raw frames",
            )?;
            let census = &store.indices()[location.plugin].census;
            let longest = census
                .masters
                .iter()
                .map(String::len)
                .chain([census.name.len()])
                .max()
                .unwrap_or(0);
            budget.metadata(2048 + 4 * longest + end - start)?;
            let raw = u32::from_le_bytes(sub.data.try_into().expect("validated scalar"));
            let target = dependency(store, location, raw, &[expected])?;
            let source = if let Some(key) = &target.key
                && let Some(winner) = store.winner(key)
            {
                Some(identity(store, key, winner, &mut budget)?)
            } else {
                None
            };
            let header_source_available = target.status == "resolved";
            let stored_body_offset = record
                .header
                .offset
                .checked_add(24)
                .ok_or_else(|| failure("source offset overflow"))?;
            let physical_framing_offset = if record.header.flags & plugin::COMPRESSED == 0 {
                Some(
                    stored_body_offset
                        .checked_add(start as u64)
                        .ok_or_else(|| failure("source offset overflow"))?,
                )
            } else {
                None
            };
            links.push(Link {
                kind: sub.kind,
                physical_field_ordinal: field_ordinal,
                logical_field_ordinal: callback_ordinal,
                site: FieldSite {
                    kind: sub.kind,
                    decoded_header_offset: sub.payload_offset,
                    span: Span {
                        decoded_offset: sub.payload_offset + 6,
                        bytes: 4,
                    },
                },
                decoded_framing_offset: start,
                framing: record.payload[start..end].to_vec(),
                stored_body_offset,
                stored_body_bytes: record.header.stored_size,
                physical_framing_offset,
                raw,
                target,
                source,
                header_source_available,
                behavior_status: "unknown; target body and behavior not decoded",
            });
            Ok(())
        })?;
        let mut receipt = Receipt {
            schema_version: 1,
            identity: String::new(),
            source_cohort_sha256: cohort,
            sources,
            cell,
            cell_decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
            cell_flags,
            links,
            usage: budget.usage,
            limits: budget.limits,
            runtime_ready: false,
        };
        let mut writer = HashWriter(Sha256::new());
        writer.0.update(b"nv-cell-environment-source-v1\0");
        serde_json::to_writer(
            &mut writer,
            &(
                &receipt.source_cohort_sha256,
                &receipt.cell,
                &receipt.cell_decoded_sha256,
                &receipt.cell_flags,
                &receipt.links,
            ),
        )
        .map_err(|e| failure(&e.to_string()))?;
        receipt.identity = format!("{:x}", writer.0.finalize());
        Ok(Self(Arc::new(receipt)))
    }
}
impl Serialize for CellEnvironmentSources {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.receipt().serialize(serializer)
    }
}
fn expected(kind: [u8; 4]) -> Option<[u8; 4]> {
    Some(match &kind {
        b"XCCM" => *b"CLMT",
        b"XCIM" => *b"IMGS",
        b"XEZN" => *b"ECZN",
        b"XCAS" => *b"ASPC",
        b"XCMO" => *b"MUSC",
        _ => return None,
    })
}
struct Budget {
    limits: Limits,
    usage: Usage,
}
impl Budget {
    fn metadata(&mut self, bytes: usize) -> Result<()> {
        add(
            &mut self.usage.metadata_bytes,
            bytes,
            self.limits.metadata_bytes,
            "metadata",
        )
    }
}
fn identity(
    store: &mut RecordStore,
    key: &FormKey,
    location: Location,
    budget: &mut Budget,
) -> Result<RecordSource> {
    add(
        &mut budget.usage.records,
        1,
        budget.limits.records,
        "records",
    )?;
    budget.metadata(1024 + 4 * key.origin_plugin.len() + 4 * store.source_name(location).len())?;
    Ok(RecordSource {
        key: key.clone(),
        source_ordinal: location.plugin,
        source_plugin: store.source_name(location).to_owned(),
        source_sha256: store.source_digest(location)?,
        header: store.definition(location).header.clone(),
    })
}
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn failure(message: &str) -> Error {
    Error::Resolution(format!("CELL environment sources: {message}"))
}
fn add(used: &mut usize, bytes: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| failure(&format!("{name} budget exceeded")))?;
    Ok(())
}
