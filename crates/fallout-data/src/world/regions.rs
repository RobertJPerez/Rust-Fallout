//! Exact on-disk CELL region array occurrences and header-only REGN witnesses.
//! No editor sorting/filtering, inherited membership or region behavior is applied.
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
    pub occurrences: usize,
    pub elements: usize,
    pub record_bytes: usize,
    pub read_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            sources: 256,
            records: 4097,
            fields: 65536,
            occurrences: 256,
            elements: 4096,
            record_bytes: 4 * 1024 * 1024,
            read_bytes: 4 * 1024 * 1024,
            raw_bytes: 1024 * 1024,
            metadata_bytes: 32 * 1024 * 1024,
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
            (self.occurrences, max.occurrences),
            (self.elements, max.elements),
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
    pub occurrences: usize,
    pub elements: usize,
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
pub struct Element {
    /// Zero-based index within this physical array, preserving repeats.
    pub element_ordinal: usize,
    pub span: Span,
    pub physical_payload_offset: Option<u64>,
    pub raw: u32,
    pub target: Dependency,
    pub source: Option<RecordSource>,
    pub header_source_available: bool,
    pub behavior_status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Occurrence {
    /// Zero-based index among XCLR occurrences in this CELL.
    pub occurrence_ordinal: usize,
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
    pub elements: Vec<Element>,
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
    pub occurrences: Vec<Occurrence>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
/// Immutable source closure; serialized reports cannot construct a request.
pub struct CellRegionSources(Arc<Receipt>);
impl CellRegionSources {
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
        let mut occurrences = Vec::new();
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
            if sub.kind != *b"XCLR" {
                return Ok(());
            }
            if sub.data.len() % 4 != 0 {
                return Err(failure("XCLR has a non-word tail"));
            }
            let count = sub.data.len() / 4;
            // Charge the entire array before allocation or dependency resolution.
            add(
                &mut budget.usage.occurrences,
                1,
                budget.limits.occurrences,
                "occurrences",
            )?;
            add(
                &mut budget.usage.elements,
                count,
                budget.limits.elements,
                "array elements/source links",
            )?;
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
            let entry_bytes = 2048_usize
                .checked_add(
                    4_usize
                        .checked_mul(longest)
                        .ok_or_else(|| failure("key byte overflow"))?,
                )
                .ok_or_else(|| failure("metadata overflow"))?;
            budget.metadata(1024 + end - start)?;
            budget.metadata(
                count
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| failure("array metadata overflow"))?,
            )?;
            let stored_body_offset = record
                .header
                .offset
                .checked_add(24)
                .ok_or_else(|| failure("source offset overflow"))?;
            let physical_base = if record.header.flags & plugin::COMPRESSED == 0 {
                Some(stored_body_offset)
            } else {
                None
            };
            let physical_framing_offset = physical_base
                .map(|base| {
                    base.checked_add(start as u64)
                        .ok_or_else(|| failure("source offset overflow"))
                })
                .transpose()?;
            let mut elements = Vec::with_capacity(count);
            for (element_ordinal, word) in sub.data.as_chunks::<4>().0.iter().enumerate() {
                let decoded_offset = sub.payload_offset + 6 + element_ordinal * 4;
                let raw = u32::from_le_bytes(*word);
                let target = dependency(store, location, raw, &[*b"REGN"])?;
                let source = if let Some(key) = &target.key
                    && let Some(winner) = store.winner(key)
                {
                    Some(identity(store, key, winner, &mut budget)?)
                } else {
                    None
                };
                let header_source_available = target.status == "resolved";
                let physical_payload_offset = physical_base
                    .map(|base| {
                        base.checked_add(decoded_offset as u64)
                            .ok_or_else(|| failure("source offset overflow"))
                    })
                    .transpose()?;
                elements.push(Element {
                    element_ordinal,
                    span: Span {
                        decoded_offset,
                        bytes: 4,
                    },
                    physical_payload_offset,
                    raw,
                    target,
                    source,
                    header_source_available,
                    behavior_status: "unknown; REGN body and behavior not decoded",
                });
            }
            occurrences.push(Occurrence {
                occurrence_ordinal: occurrences.len(),
                physical_field_ordinal: field_ordinal,
                logical_field_ordinal: callback_ordinal,
                site: FieldSite {
                    kind: sub.kind,
                    decoded_header_offset: sub.payload_offset,
                    span: Span {
                        decoded_offset: sub.payload_offset + 6,
                        bytes: sub.data.len(),
                    },
                },
                decoded_framing_offset: start,
                framing: record.payload[start..end].to_vec(),
                stored_body_offset,
                stored_body_bytes: record.header.stored_size,
                physical_framing_offset,
                elements,
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
            occurrences,
            usage: budget.usage,
            limits: budget.limits,
            runtime_ready: false,
        };
        let mut writer = HashWriter(Sha256::new());
        writer.0.update(b"nv-cell-region-source-v1\0");
        serde_json::to_writer(
            &mut writer,
            &(
                &receipt.source_cohort_sha256,
                &receipt.cell,
                &receipt.cell_decoded_sha256,
                &receipt.cell_flags,
                &receipt.occurrences,
            ),
        )
        .map_err(|e| failure(&e.to_string()))?;
        receipt.identity = format!("{:x}", writer.0.finalize());
        Ok(Self(Arc::new(receipt)))
    }
}
impl Serialize for CellRegionSources {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.receipt().serialize(serializer)
    }
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
    Error::Resolution(format!("CELL region sources: {message}"))
}
fn add(used: &mut usize, bytes: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| failure(&format!("{name} budget exceeded")))?;
    Ok(())
}
