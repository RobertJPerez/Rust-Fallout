//! Source-bound compressed record extraction. Checksum inspection keeps taint
//! visible; runtime callers still use the strict default plugin decoder.
use crate::{Result, baseline, io, plugin};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub plugin: plugin::Limits,
    pub max_rows: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            plugin: plugin::Limits::default(),
            max_rows: 262_144,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Row {
    pub record_kind: String,
    pub form_id: u32,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub stored_bytes: u32,
    pub decoded_bytes: usize,
    pub stored_sha256: String,
    pub decoded_sha256: String,
    pub integrity_issue: Option<plugin::ChecksumMismatch>,
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: u64,
    pub stored_bytes: u64,
    pub zlib_bytes: u64,
    pub decoded_bytes: u64,
    pub checksum_mismatches: u64,
    pub record_kinds: BTreeMap<String, u64>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub record_payloads_decoded: u64,
    pub record_payloads_deferred: u64,
    pub groups: u64,
    pub counts: Counts,
    pub rows: Vec<Row>,
}
pub fn inspect(path: &Path, limits: Limits) -> Result<Report> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            crate::Error::Unsupported("compressed record source filename encoding".into())
        })?;
    crate::identity::plugin_name(name)?;
    let mut file = baseline::open_source(path)?;
    let (source_bytes, source_sha256) =
        baseline::digest_reader(&mut file).map_err(|error| io(path, error))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io(path, error))?;
    let mut stored = baseline::open_source(path)?;
    let mut report = Report {
        source_name: name.into(),
        source_bytes,
        source_sha256,
        record_payloads_decoded: 0,
        record_payloads_deferred: 0,
        groups: 0,
        counts: Counts::default(),
        rows: Vec::new(),
    };
    plugin::visit_selected(
        &mut BufReader::new(file),
        source_bytes,
        name,
        limits.plugin,
        |header| header.flags & plugin::COMPRESSED != 0,
        |event| {
            match event {
                plugin::SelectedEvent::Group(_) => report.groups += 1,
                plugin::SelectedEvent::Deferred(_) => report.record_payloads_deferred += 1,
                plugin::SelectedEvent::Record(record) => {
                    report.record_payloads_decoded += 1;
                    if record.header.flags & plugin::COMPRESSED == 0 {
                        return Ok(());
                    }
                    if report.rows.len() >= limits.max_rows {
                        return Err(crate::Error::Unsupported(
                            "compressed record row budget exceeded".into(),
                        ));
                    }
                    stored
                        .seek(SeekFrom::Start(record.header.offset + plugin::HEADER_SIZE))
                        .map_err(|error| io(path, error))?;
                    let mut hash = Sha256::new();
                    let mut remaining = record.header.stored_size as usize;
                    let mut buffer = [0; 64 * 1024];
                    while remaining > 0 {
                        let count = remaining.min(buffer.len());
                        stored
                            .read_exact(&mut buffer[..count])
                            .map_err(|error| io(path, error))?;
                        hash.update(&buffer[..count]);
                        remaining -= count;
                    }
                    report.counts.records += 1;
                    report.counts.stored_bytes += u64::from(record.header.stored_size);
                    report.counts.zlib_bytes += u64::from(record.header.stored_size) - 4;
                    report.counts.decoded_bytes += record.payload.len() as u64;
                    report.counts.checksum_mismatches +=
                        u64::from(record.integrity_issue.is_some());
                    let kind = plugin::signature(record.header.kind);
                    *report.counts.record_kinds.entry(kind.clone()).or_default() += 1;
                    report.rows.push(Row {
                        record_kind: kind,
                        form_id: record.header.form_id,
                        record_file_offset: record.header.offset,
                        record_flags: record.header.flags,
                        stored_bytes: record.header.stored_size,
                        decoded_bytes: record.payload.len(),
                        stored_sha256: format!("{:x}", hash.finalize()),
                        decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
                        integrity_issue: record.integrity_issue.clone(),
                    });
                }
            }
            Ok(())
        },
    )?;
    Ok(report)
}
