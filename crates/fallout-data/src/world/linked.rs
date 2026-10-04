//! Exact placed linked-reference declarations and header-only target witnesses.
//! No target placement decoding, GetLinkedRef execution or actor package selection.
use super::{
    Dependency, Placement, decode_placement,
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
            records: 2,
            fields: 65536,
            links: 1,
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
pub struct FieldFrame {
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
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub field: FieldFrame,
    pub raw: u32,
    pub target: Dependency,
    pub source: Option<RecordSource>,
    pub header_source_available: bool,
    pub behavior_status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Color {
    pub field: FieldFrame,
    /// Two source ByteColors: RGB plus unused fourth byte each; never alpha.
    pub raw: [u8; 8],
    pub behavior_status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub identity: String,
    pub source_cohort_sha256: String,
    pub sources: Vec<SourceReceipt>,
    pub placed: RecordSource,
    pub placed_decoded_sha256: String,
    pub placement: Placement,
    pub link: Option<Link>,
    pub color: Option<Color>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
/// Immutable source closure; serialized reports cannot construct a request.
pub struct PlacedLinkedSources(Arc<Receipt>);
impl PlacedLinkedSources {
    pub fn receipt(&self) -> &Receipt {
        &self.0
    }
    pub fn root(&self) -> &FormKey {
        &self.0.placed.key
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
            return Err(failure("requires nv-original placement"));
        }
        let location = store
            .winner(root)
            .ok_or_else(|| failure("placed root missing"))?;
        let header = &store.definition(location).header;
        if ![*b"REFR", *b"ACHR", *b"ACRE"].contains(&header.kind)
            || header.flags & plugin::DELETED != 0
        {
            return Err(failure("placed root deleted or wrong kind"));
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
        let placed = identity(store, root, location, &mut budget)?;
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
            return Err(failure("tainted requested placed body"));
        }
        budget.metadata(record.payload.len())?;
        // Charge all physical fields and core decoder map entries before it allocates.
        let mut previous_end = 0;
        plugin::visit_subrecords(&record, &placed.source_plugin, |sub| {
            let prefix = sub
                .payload_offset
                .checked_sub(previous_end)
                .ok_or_else(|| failure("visitor offset order"))?;
            if prefix != 0 && prefix != 10 {
                return Err(failure("visitor framing extent changed"));
            }
            add(
                &mut budget.usage.fields,
                1 + usize::from(prefix == 10),
                budget.limits.fields,
                "physical fields",
            )?;
            budget.metadata(256)?; // One possible unhandled signature/BTreeMap entry.
            previous_end = sub.payload_offset + 6 + sub.data.len();
            Ok(())
        })?;
        let placement = decode_placement(&record, &placed.source_plugin)?;
        let mut link = None;
        let mut color = None;
        let mut frame_start = 0;
        let mut physical_ordinal = 0;
        let mut logical_ordinal = 0;
        plugin::visit_subrecords(&record, &placed.source_plugin, |sub| {
            // The existing visitor permits one validated ten-byte XXXX prefix.
            let prefix = sub
                .payload_offset
                .checked_sub(frame_start)
                .ok_or_else(|| failure("visitor offset order"))?;
            if prefix != 0 && prefix != 10 {
                return Err(failure("visitor framing extent changed"));
            }
            let extended = usize::from(prefix == 10);
            let field_ordinal = physical_ordinal + extended;
            physical_ordinal = field_ordinal + 1;
            let callback_ordinal = logical_ordinal;
            logical_ordinal += 1;
            let start = frame_start;
            let end = sub.payload_offset + 6 + sub.data.len();
            frame_start = end;
            if !matches!(&sub.kind, b"XLKR" | b"XCLP") {
                return Ok(());
            }
            if (sub.kind == *b"XLKR" && link.is_some()) || (sub.kind == *b"XCLP" && color.is_some())
            {
                return Err(failure("duplicate linked-reference field"));
            }
            let width = if sub.kind == *b"XLKR" { 4 } else { 8 };
            if sub.data.len() != width {
                return Err(failure("linked-reference field has unsupported width"));
            }
            if sub.kind == *b"XLKR" {
                add(
                    &mut budget.usage.links,
                    1,
                    budget.limits.links,
                    "source link",
                )?;
            }
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
            let field = FieldFrame {
                physical_field_ordinal: field_ordinal,
                logical_field_ordinal: callback_ordinal,
                site: FieldSite {
                    kind: sub.kind,
                    decoded_header_offset: sub.payload_offset,
                    span: Span {
                        decoded_offset: sub.payload_offset + 6,
                        bytes: width,
                    },
                },
                decoded_framing_offset: start,
                framing: record.payload[start..end].to_vec(),
                stored_body_offset,
                stored_body_bytes: record.header.stored_size,
                physical_framing_offset,
            };
            if sub.kind == *b"XCLP" {
                color = Some(Color {
                    field,
                    raw: sub.data.try_into().expect("validated color bytes"),
                    behavior_status: "unknown; source colors not applied",
                });
            } else {
                let raw = u32::from_le_bytes(sub.data.try_into().expect("validated scalar"));
                let target = dependency(
                    store,
                    location,
                    raw,
                    &[
                        *b"REFR", *b"ACRE", *b"ACHR", *b"PGRE", *b"PMIS", *b"PBEA", *b"PLYR",
                    ],
                )?;
                let source = if let Some(key) = &target.key
                    && let Some(winner) = store.winner(key)
                {
                    Some(identity(store, key, winner, &mut budget)?)
                } else {
                    None
                };
                let header_source_available = target.status == "resolved";
                link = Some(Link {
                    field,
                    raw,
                    target,
                    source,
                    header_source_available,
                    behavior_status: "unknown; target body, placement and binding not evaluated",
                });
            }
            Ok(())
        })?;
        let mut receipt = Receipt {
            schema_version: 1,
            identity: String::new(),
            source_cohort_sha256: cohort,
            sources,
            placed,
            placed_decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
            placement,
            link,
            color,
            usage: budget.usage,
            limits: budget.limits,
            runtime_ready: false,
        };
        let mut writer = HashWriter(Sha256::new());
        writer.0.update(b"nv-placed-linked-source-v1\0");
        serde_json::to_writer(
            &mut writer,
            &(
                &receipt.source_cohort_sha256,
                &receipt.placed,
                &receipt.placed_decoded_sha256,
                &receipt.placement,
                &receipt.link,
                &receipt.color,
            ),
        )
        .map_err(|e| failure(&e.to_string()))?;
        receipt.identity = format!("{:x}", writer.0.finalize());
        Ok(Self(Arc::new(receipt)))
    }
}
impl Serialize for PlacedLinkedSources {
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
    Error::Resolution(format!("placed linked sources: {message}"))
}
fn add(used: &mut usize, bytes: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| failure(&format!("{name} budget exceeded")))?;
    Ok(())
}
