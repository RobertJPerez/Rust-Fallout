//! Explicit CELL water declarations and one source-bound noise member request.
//! No WRLD fallback, editor defaults, float evaluation or water simulation.
use super::{
    Dependency, SourceField, decode_cell,
    dependencies::{FieldSite, Span, source_cohort},
    dependency,
    preparation::{ArchiveReceipt, RequestReceipt},
    terminated,
};
use crate::{
    Error, Result, baseline,
    identity::{FormKey, ProfileId},
    plugin::{self, Record, RecordHeader, Subrecord},
    resource_jobs::{ArchiveInput, JobHandle, JobToken, ResourceJobs},
    store::{Location, RecordStore, SourceReceipt},
    vfs::{MountIndex, texture_path},
};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub sources: usize,
    pub records: usize,
    pub fields: usize,
    pub record_bytes: usize,
    pub read_bytes: usize,
    pub string_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
    pub dependencies: usize,
    pub archives: usize,
    pub mapped_bytes: u64,
    pub jobs: usize,
    pub noise_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            sources: 256,
            records: 2,
            fields: 65536,
            record_bytes: 4 * 1024 * 1024,
            read_bytes: 8 * 1024 * 1024,
            string_bytes: 4096,
            raw_bytes: 8192,
            metadata_bytes: 16 * 1024 * 1024,
            dependencies: 1,
            archives: 1,
            mapped_bytes: 16 * 1024 * 1024 * 1024,
            jobs: 1,
            noise_bytes: 64 * 1024 * 1024,
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
            (self.record_bytes, max.record_bytes),
            (self.read_bytes, max.read_bytes),
            (self.string_bytes, max.string_bytes),
            (self.raw_bytes, max.raw_bytes),
            (self.metadata_bytes, max.metadata_bytes),
            (self.dependencies, max.dependencies),
            (self.archives, max.archives),
            (self.jobs, max.jobs),
            (self.noise_bytes, max.noise_bytes),
        ] {
            if a > b {
                return Err(failure("limit exceeds ceiling"));
            }
        }
        if self.mapped_bytes > max.mapped_bytes {
            return Err(failure("mapping limit exceeds ceiling"));
        }
        Ok(self)
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub sources: usize,
    pub records: usize,
    pub fields: usize,
    pub read_bytes: usize,
    pub string_bytes: usize,
    pub raw_bytes: usize,
    pub metadata_bytes: usize,
    pub dependencies: usize,
    pub archives: usize,
    pub mapped_bytes: u64,
    pub jobs: usize,
    pub noise_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct RecordSource {
    pub key: FormKey,
    pub source_ordinal: usize,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: RecordHeader,
    pub decoded_sha256: Option<String>,
}
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
pub struct WaterType {
    pub target: Dependency,
    pub source: Option<RecordSource>,
    /// Strict body hashing establishes source integrity; no WATR parameters are decoded.
    pub typed_parameters_included: bool,
}
#[derive(Debug, Serialize)]
pub struct Noise {
    pub status: &'static str,
    pub path: Option<crate::vfs::AssetPath>,
    pub archive: Option<ArchiveReceipt>,
    pub request: Option<RequestReceipt>,
}
#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub identity: String,
    pub source_cohort_sha256: String,
    pub sources: Vec<SourceReceipt>,
    pub cell: RecordSource,
    pub cell_flags: SourceField<u8>,
    pub xclw: Option<Field<u32>>,
    pub xcwt: Option<Field<u32>>,
    pub xnam: Option<Field<Vec<u8>>>,
    pub water_type: Option<WaterType>,
    pub noise: Option<Noise>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
struct Inner {
    receipt: Receipt,
    input: Option<Arc<ArchiveInput>>,
}
/// Only protected source reads and the existing archive importer construct this
/// request. Worker artifacts retain its immutable metadata and mapped source scope.
pub struct CellWaterSources(Arc<Inner>);
impl CellWaterSources {
    pub fn receipt(&self) -> &Receipt {
        &self.0.receipt
    }
    pub fn root(&self) -> &FormKey {
        &self.receipt().cell.key
    }
    pub fn identity(&self) -> &str {
        &self.receipt().identity
    }
    pub fn validate_sources(&self, store: &mut RecordStore) -> Result<()> {
        if store.indices().len() != self.receipt().sources.len()
            || store
                .indices()
                .iter()
                .zip(&self.receipt().sources)
                .any(|(index, source)| index.census.name != source.source_name)
        {
            return Err(failure("ordered source names/count changed"));
        }
        let current = store.source_receipts()?;
        if current
            .iter()
            .zip(&self.receipt().sources)
            .any(|(a, b)| a.source_bytes != b.source_bytes || a.source_sha256 != b.source_sha256)
        {
            return Err(failure("ordered source bytes changed"));
        }
        Ok(())
    }
    /// The caller owns a bounded existing pool/generation. No job is made for an
    /// absent/empty/missing noise declaration; inspect the explicit receipt status.
    pub fn submit_noise(
        &self,
        store: &mut RecordStore,
        jobs: &ResourceJobs,
        token: JobToken,
        cache: Option<(PathBuf, PathBuf)>,
    ) -> Result<Option<JobHandle>> {
        self.validate_sources(store)?;
        token.check().map_err(job_error)?;
        if token.source_identity() != self.identity() {
            return Err(failure("noise token source mismatch"));
        }
        let Some(input) = &self.0.input else {
            return Ok(None);
        };
        let request = self
            .receipt()
            .noise
            .as_ref()
            .and_then(|noise| noise.request.as_ref())
            .expect("private admitted noise input always has exact source request");
        let member = input
            .member(&request.path, &request.source)
            .map_err(job_error)?;
        let handle = jobs
            .submit_scoped(
                member,
                token,
                cache,
                self.0.clone(),
                #[cfg(test)]
                None,
            )
            .map_err(job_error)?;
        Ok(Some(handle))
    }
    pub fn load(
        store: &mut RecordStore,
        root: &FormKey,
        mounts: &MountIndex,
        limits: Limits,
    ) -> Result<Self> {
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
        budget.metadata(record.payload.len())?;
        let cell_flags = decode_cell(&record, &cell.source_plugin)?.flags;
        let (xclw, xcwt, xnam) = fields(&record, &cell.source_plugin, &mut budget)?;
        let water_type = if let Some(link) = &xcwt {
            let census = &store.indices()[location.plugin].census;
            let longest = census
                .masters
                .iter()
                .map(String::len)
                .chain([census.name.len()])
                .max()
                .unwrap_or(0);
            budget.metadata(512 + 4 * longest)?;
            let target = dependency(store, location, link.value, &[*b"WATR"])?;
            let source = if let Some(key) = &target.key
                && let Some(winner) = store.winner(key)
            {
                let mut source = identity(store, key, winner, &mut budget)?;
                if target.status == "resolved" {
                    let body = read(store, winner, &mut budget)?;
                    plugin::visit_subrecords(&body, &source.source_plugin, |_| {
                        add(&mut budget.usage.fields, 1, budget.limits.fields, "fields")
                    })?;
                    source.decoded_sha256 = Some(format!("{:x}", Sha256::digest(&body.payload)));
                }
                Some(source)
            } else {
                None
            };
            Some(WaterType {
                target,
                source,
                typed_parameters_included: false,
            })
        } else {
            None
        };
        let (noise, input) = noise(&xnam, mounts, &mut budget)?;
        let mut receipt = Receipt {
            schema_version: 1,
            identity: String::new(),
            source_cohort_sha256: cohort,
            sources,
            cell,
            cell_flags,
            xclw,
            xcwt,
            xnam,
            water_type,
            noise,
            usage: budget.usage,
            limits: budget.limits,
            runtime_ready: false,
        };
        let mut writer = HashWriter(Sha256::new());
        writer.0.update(b"nv-cell-water-source-v1\0");
        serde_json::to_writer(
            &mut writer,
            &(
                &receipt.source_cohort_sha256,
                &receipt.cell,
                &receipt.cell_flags,
                &receipt.xclw,
                &receipt.xcwt,
                &receipt.xnam,
                &receipt.water_type,
                &receipt.noise,
            ),
        )
        .map_err(|error| failure(&error.to_string()))?;
        receipt.identity = format!("{:x}", writer.0.finalize());
        Ok(Self(Arc::new(Inner { receipt, input })))
    }
}
impl Serialize for CellWaterSources {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.receipt().serialize(serializer)
    }
}
fn job_error(error: crate::resource_jobs::JobError) -> Error {
    failure(&error.to_string())
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
type WaterFields = (
    Option<Field<u32>>,
    Option<Field<u32>>,
    Option<Field<Vec<u8>>>,
);
fn fields(record: &Record, name: &str, budget: &mut Budget) -> Result<WaterFields> {
    let (mut xclw, mut xcwt, mut xnam) = (None, None, None);
    let mut start = 0;
    plugin::visit_subrecords(record, name, |sub| {
        add(&mut budget.usage.fields, 1, budget.limits.fields, "fields")?;
        let frame_start = start;
        start = sub.payload_offset + 6 + sub.data.len();
        match &sub.kind {
            b"XCLW" => {
                unique(&xclw, "XCLW")?;
                let value = scalar(sub.data)?;
                xclw = Some(field(record, &sub, frame_start, value, budget)?);
            }
            b"XCWT" => {
                unique(&xcwt, "XCWT")?;
                add(
                    &mut budget.usage.dependencies,
                    1,
                    budget.limits.dependencies,
                    "dependencies",
                )?;
                let value = scalar(sub.data)?;
                xcwt = Some(field(record, &sub, frame_start, value, budget)?);
            }
            b"XNAM" => {
                unique(&xnam, "XNAM")?;
                let raw = terminated(sub.data, name, record.header.offset)?;
                add(
                    &mut budget.usage.string_bytes,
                    sub.data.len(),
                    budget.limits.string_bytes,
                    "string bytes",
                )?;
                budget.metadata(sub.data.len())?;
                xnam = Some(field(record, &sub, frame_start, raw.to_vec(), budget)?);
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok((xclw, xcwt, xnam))
}
fn noise(
    xnam: &Option<Field<Vec<u8>>>,
    mounts: &MountIndex,
    budget: &mut Budget,
) -> Result<(Option<Noise>, Option<Arc<ArchiveInput>>)> {
    let Some(field) = xnam else {
        return Ok((None, None));
    };
    if field.value.is_empty() {
        return Ok((
            Some(Noise {
                status: "empty-declaration",
                path: None,
                archive: None,
                request: None,
            }),
            None,
        ));
    }
    budget.metadata(1024 + 4 * field.value.len())?;
    let path = texture_path(&field.value)?;
    let candidates = mounts.candidates(path.bytes())?;
    match candidates {
        [] => Ok((
            Some(Noise {
                status: "missing",
                path: Some(path),
                archive: None,
                request: None,
            }),
            None,
        )),
        [source] => {
            if source.original_path.len() > 4096 {
                return Err(failure("noise member path exceeds ceiling"));
            }
            add(
                &mut budget.usage.archives,
                1,
                budget.limits.archives,
                "archives",
            )?;
            add(
                &mut budget.usage.jobs,
                1,
                budget.limits.jobs,
                "planned jobs",
            )?;
            budget.metadata(
                4096 + 4 * source.container.len()
                    + 4 * source.original_path.len()
                    + 4 * path.bytes().len(),
            )?;
            // Keep this write-denying handle alive through extent admission/mapping.
            let protected = baseline::open_source(Path::new(&source.container))?;
            let bytes = protected
                .metadata()
                .map_err(|error| crate::io(&source.container, error))?
                .len();
            budget.usage.mapped_bytes = bytes;
            if bytes > budget.limits.mapped_bytes {
                return Err(failure("mapped bytes budget exceeded"));
            }
            let input = ArchiveInput::open(Path::new(&source.container)).map_err(job_error)?;
            if input.source_bytes() != bytes {
                return Err(failure("mapped source extent changed"));
            }
            let member = input.member(&path, source).map_err(job_error)?;
            add(
                &mut budget.usage.noise_bytes,
                member.bytes,
                budget.limits.noise_bytes,
                "noise bytes",
            )?;
            let request = RequestReceipt {
                path: path.clone(),
                source: source.clone(),
                archive_sha256: input.source_sha256().to_owned(),
                decoded_bytes: member.bytes,
            };
            let archive = ArchiveReceipt {
                container: source.container.clone(),
                source_bytes: bytes,
                source_sha256: input.source_sha256().to_owned(),
            };
            Ok((
                Some(Noise {
                    status: "one-archive-source; retail-precedence-unverified",
                    path: Some(path),
                    archive: Some(archive),
                    request: Some(request),
                }),
                Some(input),
            ))
        }
        _ => Err(failure(
            "ambiguous noise asset; profile precedence is not verified",
        )),
    }
}
fn unique<T>(field: &Option<T>, name: &str) -> Result<()> {
    if field.is_some() {
        return Err(failure(&format!("duplicate {name}")));
    }
    Ok(())
}
fn scalar(bytes: &[u8]) -> Result<u32> {
    if bytes.len() != 4 {
        return Err(failure("scalar field must have four bytes"));
    }
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("validated scalar"),
    ))
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
    Error::Resolution(format!("CELL water sources: {message}"))
}
fn add(used: &mut usize, bytes: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| failure(&format!("{name} budget exceeded")))?;
    Ok(())
}
