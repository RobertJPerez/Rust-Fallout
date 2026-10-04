//! Exact winning CELL lighting declarations. No editor migration, inheritance,
//! float interpretation, shader convention or runtime admission is performed.
use super::{
    Dependency, SourceField, decode_cell,
    dependencies::{FieldSite, Span, source_cohort},
    dependency,
};
use crate::{
    Error, Result,
    identity::{FormKey, ProfileId},
    plugin::{self, Record, RecordHeader, Subrecord},
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
    pub record_bytes: usize,
    pub read_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
    pub template_links: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            sources: 256,
            records: 2,
            fields: 65536,
            record_bytes: 4 * 1024 * 1024,
            read_bytes: 8 * 1024 * 1024,
            raw_bytes: 256,
            metadata_bytes: 16 * 1024 * 1024,
            template_links: 1,
        }
    }
}
impl Limits {
    pub(in crate::world) fn validate(self) -> Result<Self> {
        let max = Self::default();
        for (a, b) in [
            (self.sources, max.sources),
            (self.records, max.records),
            (self.fields, max.fields),
            (self.record_bytes, max.record_bytes),
            (self.read_bytes, max.read_bytes),
            (self.raw_bytes, max.raw_bytes),
            (self.metadata_bytes, max.metadata_bytes),
            (self.template_links, max.template_links),
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
    /// Retained source record identities, including header-only unavailable targets.
    pub records: usize,
    pub fields: usize,
    pub read_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
    pub template_links: usize,
}
#[derive(Debug, Serialize)]
pub struct RecordIdentity {
    pub key: FormKey,
    pub source_ordinal: usize,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: RecordHeader,
    pub decoded_sha256: Option<String>,
}
/// The fourth byte is unused source padding, not an inferred alpha channel.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ByteColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub unused: u8,
}
#[derive(Debug, Serialize)]
pub struct Lighting {
    pub ambient: ByteColor,
    pub directional: ByteColor,
    pub fog: ByteColor,
    pub fog_near_word: u32,
    pub fog_far_word: u32,
    pub rotation_xy_word: u32,
    pub rotation_z_word: u32,
    pub rotation_xy: i32,
    pub rotation_z: i32,
    pub directional_fade_word: Option<u32>,
    pub fog_clip_distance_word: Option<u32>,
    pub fog_power_word: Option<u32>,
}
/// Complete framing is copied from the existing visitor's boundaries, including
/// any validated XXXX prefix. Compressed decoded offsets are never file offsets.
#[derive(Debug, Serialize)]
pub struct Field<T> {
    pub site: FieldSite,
    pub decoded_framing_offset: usize,
    pub framing: Vec<u8>,
    pub stored_body_offset: u64,
    pub stored_body_bytes: u32,
    pub physical_framing_offset: Option<u64>,
    pub value: T,
}
#[derive(Debug, Serialize)]
pub struct Template {
    pub target: Dependency,
    pub source: Option<RecordIdentity>,
    pub lighting: Option<Field<Lighting>>,
    /// Resolution status, or missing-data for a live LGTM lacking required DATA.
    pub input_status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub identity: String,
    pub source_cohort_sha256: String,
    pub sources: Vec<SourceReceipt>,
    pub cell: RecordIdentity,
    pub cell_flags: SourceField<u8>,
    pub xcll: Option<Field<Lighting>>,
    pub ltmp: Option<Field<u32>>,
    pub lnam: Option<Field<u32>>,
    pub template: Option<Template>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
/// An immutable source request reusable by presentation. Mutable report JSON
/// cannot manufacture this authority, and stored bytes never become defaults.
pub struct CellLightingSources(Arc<Receipt>);
impl CellLightingSources {
    pub fn receipt(&self) -> &Receipt {
        &self.0
    }
    pub fn root(&self) -> &FormKey {
        &self.0.cell.key
    }
    pub fn identity(&self) -> &str {
        &self.0.identity
    }
    /// Only source input presence, never an inherited/computed lighting result.
    pub fn source_inputs_available(&self) -> bool {
        let template = self.0.template.as_ref();
        let any = self.0.xcll.is_some() || template.is_some_and(|v| v.lighting.is_some());
        any && template.is_none_or(|v| matches!(v.input_status, "resolved" | "null"))
    }
    pub fn validate_sources(&self, store: &mut RecordStore) -> Result<()> {
        // Constructor already admitted this bounded receipt/scratch cohort.
        if store.indices().len() != self.0.sources.len() {
            return Err(failure("source count changed"));
        }
        let current = store.source_receipts()?;
        if current.iter().zip(&self.0.sources).any(|(a, b)| {
            a.source_name != b.source_name
                || a.source_bytes != b.source_bytes
                || a.source_sha256 != b.source_sha256
        }) {
            return Err(failure("ordered source cohort changed"));
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
        let mut cell = identity(store, root, location, &mut budget)?;
        let record = read(store, location, &mut budget)?;
        cell.decoded_sha256 = Some(format!("{:x}", Sha256::digest(&record.payload)));
        // Existing CELL decoder may copy a full name; bound that temporary first.
        budget.metadata(record.payload.len())?;
        let cell_flags = decode_cell(&record, &cell.source_plugin)?.flags;
        let (xcll, ltmp, lnam) = cell_fields(&record, &cell.source_plugin, &mut budget)?;
        let template = if let Some(link) = &ltmp {
            // Resolve the raw LTMP through this winning CELL's own master table.
            let census = &store.indices()[location.plugin].census;
            let temporary = census
                .masters
                .iter()
                .map(String::len)
                .chain([census.name.len()])
                .max()
                .unwrap_or(0);
            budget.metadata(512 + 4 * temporary)?;
            let target = dependency(store, location, link.value, &[*b"LGTM"])?;
            let mut source = None;
            let mut lighting = None;
            if let Some(key) = &target.key
                && let Some(target_location) = store.winner(key)
            {
                let mut retained = identity(store, key, target_location, &mut budget)?;
                if target.status == "resolved" {
                    let template_record = read(store, target_location, &mut budget)?;
                    retained.decoded_sha256 =
                        Some(format!("{:x}", Sha256::digest(&template_record.payload)));
                    lighting =
                        template_fields(&template_record, &retained.source_plugin, &mut budget)?;
                }
                source = Some(retained);
            }
            let input_status = match target.status {
                "resolved" if lighting.is_none() => "missing-data",
                "resolved" => "resolved",
                "null" => "null",
                "missing" => "missing",
                "deleted" => "deleted",
                _ => "wrong-record-kind",
            };
            Some(Template {
                target,
                source,
                lighting,
                input_status,
            })
        } else {
            None
        };
        let mut receipt = Receipt {
            schema_version: 1,
            identity: String::new(),
            source_cohort_sha256: cohort,
            sources,
            cell,
            cell_flags,
            xcll,
            ltmp,
            lnam,
            template,
            usage: budget.usage,
            limits: budget.limits,
            runtime_ready: false,
        };
        let mut writer = HashWriter(Sha256::new());
        writer.0.update(b"nv-cell-lighting-source-v1\0");
        serde_json::to_writer(
            &mut writer,
            &(
                &receipt.source_cohort_sha256,
                &receipt.cell,
                &receipt.cell_flags,
                &receipt.xcll,
                &receipt.ltmp,
                &receipt.lnam,
                &receipt.template,
            ),
        )
        .map_err(|e| failure(&e.to_string()))?;
        receipt.identity = format!("{:x}", writer.0.finalize());
        Ok(Self(Arc::new(receipt)))
    }
}
impl Serialize for CellLightingSources {
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
) -> Result<RecordIdentity> {
    add(
        &mut budget.usage.records,
        1,
        budget.limits.records,
        "records",
    )?;
    budget.metadata(1024 + 4 * key.origin_plugin.len() + 4 * store.source_name(location).len())?;
    Ok(RecordIdentity {
        key: key.clone(),
        source_ordinal: location.plugin,
        source_plugin: store.source_name(location).to_owned(),
        source_sha256: store.source_digest(location)?,
        header: store.definition(location).header.clone(),
        decoded_sha256: None,
    })
}
fn read(store: &mut RecordStore, location: Location, budget: &mut Budget) -> Result<Record> {
    let maximum = budget
        .limits
        .record_bytes
        .min(budget.limits.read_bytes - budget.usage.read_bytes);
    let record = store.read_bounded(location, maximum)?;
    add(
        &mut budget.usage.read_bytes,
        record.payload.len().max(record.header.stored_size as usize),
        budget.limits.read_bytes,
        "read bytes",
    )?;
    if record.integrity_issue.is_some() {
        return Err(failure("tainted requested body"));
    }
    Ok(record)
}
type CellFields = (
    Option<Field<Lighting>>,
    Option<Field<u32>>,
    Option<Field<u32>>,
);
fn cell_fields(record: &Record, name: &str, budget: &mut Budget) -> Result<CellFields> {
    let (mut xcll, mut ltmp, mut lnam) = (None, None, None);
    let mut start = 0;
    plugin::visit_subrecords(record, name, |sub| {
        add(&mut budget.usage.fields, 1, budget.limits.fields, "fields")?;
        let framing_start = start;
        start = sub.payload_offset + 6 + sub.data.len();
        match &sub.kind {
            b"XCLL" => {
                unique(&xcll, "XCLL")?;
                let value = lighting(sub.data, false)?;
                xcll = Some(field(record, &sub, framing_start, value, budget)?);
            }
            b"LTMP" => {
                unique(&ltmp, "LTMP")?;
                add(
                    &mut budget.usage.template_links,
                    1,
                    budget.limits.template_links,
                    "template links",
                )?;
                let value = scalar(sub.data)?;
                ltmp = Some(field(record, &sub, framing_start, value, budget)?);
            }
            b"LNAM" => {
                unique(&lnam, "LNAM")?;
                let value = scalar(sub.data)?;
                lnam = Some(field(record, &sub, framing_start, value, budget)?);
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok((xcll, ltmp, lnam))
}
fn template_fields(
    record: &Record,
    name: &str,
    budget: &mut Budget,
) -> Result<Option<Field<Lighting>>> {
    let mut data = None;
    let mut start = 0;
    plugin::visit_subrecords(record, name, |sub| {
        add(&mut budget.usage.fields, 1, budget.limits.fields, "fields")?;
        let framing_start = start;
        start = sub.payload_offset + 6 + sub.data.len();
        if sub.kind == *b"DATA" {
            unique(&data, "LGTM DATA")?;
            let value = lighting(sub.data, true)?;
            data = Some(field(record, &sub, framing_start, value, budget)?);
        }
        Ok(())
    })?;
    Ok(data)
}
fn unique<T>(value: &Option<T>, name: &str) -> Result<()> {
    if value.is_some() {
        return Err(failure(&format!("duplicate {name}")));
    }
    Ok(())
}
fn scalar(bytes: &[u8]) -> Result<u32> {
    if bytes.len() != 4 {
        return Err(failure("scalar field must have four bytes"));
    }
    Ok(word(bytes, 0))
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("validated field layout"),
    )
}
fn lighting(bytes: &[u8], template: bool) -> Result<Lighting> {
    if (template && bytes.len() != 40) || (!template && ![28, 32, 36, 40].contains(&bytes.len())) {
        return Err(failure("unsupported lighting layout"));
    }
    let color = |at| ByteColor {
        red: bytes[at],
        green: bytes[at + 1],
        blue: bytes[at + 2],
        unused: bytes[at + 3],
    };
    let optional = |at| (bytes.len() >= at + 4).then(|| word(bytes, at));
    let rotation_xy_word = word(bytes, 20);
    let rotation_z_word = word(bytes, 24);
    Ok(Lighting {
        ambient: color(0),
        directional: color(4),
        fog: color(8),
        fog_near_word: word(bytes, 12),
        fog_far_word: word(bytes, 16),
        rotation_xy_word,
        rotation_z_word,
        rotation_xy: rotation_xy_word as i32,
        rotation_z: rotation_z_word as i32,
        directional_fade_word: optional(28),
        fog_clip_distance_word: optional(32),
        fog_power_word: optional(36),
    })
}
fn field<T>(
    record: &Record,
    sub: &Subrecord<'_>,
    start: usize,
    value: T,
    budget: &mut Budget,
) -> Result<Field<T>> {
    let end = sub.payload_offset + 6 + sub.data.len();
    add(
        &mut budget.usage.raw_bytes,
        end - start,
        budget.limits.raw_bytes,
        "raw frames",
    )?;
    budget.metadata(512 + end - start)?;
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
    Ok(Field {
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
        value,
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
    Error::Resolution(format!("CELL lighting sources: {message}"))
}
fn add(used: &mut usize, amount: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(amount)
        .filter(|v| *v <= maximum)
        .ok_or_else(|| failure(&format!("{name} budget exceeded")))?;
    Ok(())
}
