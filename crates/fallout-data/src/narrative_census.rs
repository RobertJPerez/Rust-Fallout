//! Compact source receipts for quest/dialogue structure. Text and script bytes
//! contribute hashes locally; the reports contain no original story text.
use crate::{Result, baseline, io, narrative, plugin, script_bindings};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufReader, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: u64,
    pub fields: u64,
    pub sections: u64,
    pub conditions: u64,
    pub script_units: u64,
    pub compiled_bodies: u64,
    pub compiled_bytes: u64,
    pub findings: u64,
    pub unowned_fields: u64,
    pub record_kinds: BTreeMap<String, u64>,
    pub field_kinds: BTreeMap<String, u64>,
    pub field_lengths: BTreeMap<String, BTreeMap<usize, u64>>,
    pub section_kinds: BTreeMap<String, u64>,
    pub condition_owners: BTreeMap<String, u64>,
    pub script_owners: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize)]
pub struct ScriptRow {
    pub header_decoded_offset: usize,
    pub owner: Option<usize>,
    pub metadata_sha256: String,
    pub compiled_bytes: Option<usize>,
    pub compiled_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub record_kind: String,
    pub form_id: u32,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_bytes: usize,
    pub decoded_sha256: String,
    pub fields: usize,
    pub fields_sha256: String,
    pub sections: Vec<narrative::Section>,
    pub scripts: Vec<ScriptRow>,
    pub findings: Vec<narrative::Finding>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub focused_scan: bool,
    pub record_payloads_decoded: u64,
    pub record_payloads_deferred: u64,
    pub counts: Counts,
    pub rows: Vec<Row>,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
}

fn section_name(kind: narrative::SectionKind) -> String {
    serde_json::to_value(kind)
        .expect("section enum")
        .as_str()
        .expect("enum string")
        .into()
}
fn owner_name(document: &narrative::Document<'_>, owner: Option<usize>) -> String {
    owner
        .map(|index| section_name(document.sections[index].kind))
        .unwrap_or_else(|| "unowned".into())
}

/// Each field contributes offset/signature/length/owner (u32), tag (u8), word
/// count (u32), decoded words (u64), then SHA256 of its complete original bytes.
pub fn fields_digest(document: &narrative::Document<'_>) -> String {
    let mut hash = Sha256::new();
    for field in &document.fields {
        hash.update((field.offset as u32).to_le_bytes());
        hash.update(field.kind);
        hash.update((field.data.len() as u32).to_le_bytes());
        hash.update(
            field
                .owner
                .map(|index| index as u32)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        let (tag, words) = narrative::evidence_words(&field.value);
        hash.update([tag]);
        hash.update((words.len() as u32).to_le_bytes());
        for word in words {
            hash.update(word.to_le_bytes());
        }
        hash.update(Sha256::digest(field.data));
    }
    format!("{:x}", hash.finalize())
}

pub fn summarize(
    record: &plugin::Record,
    document: narrative::Document<'_>,
    counts: &mut Counts,
) -> Row {
    counts.records += 1;
    counts.fields += document.fields.len() as u64;
    counts.sections += document.sections.len() as u64;
    counts.findings += document.findings.len() as u64;
    *counts
        .record_kinds
        .entry(plugin::signature(record.header.kind))
        .or_default() += 1;
    for section in &document.sections {
        *counts
            .section_kinds
            .entry(section_name(section.kind))
            .or_default() += 1;
    }
    for field in &document.fields {
        let signature = plugin::signature(field.kind);
        *counts.field_kinds.entry(signature.clone()).or_default() += 1;
        *counts
            .field_lengths
            .entry(signature)
            .or_default()
            .entry(field.data.len())
            .or_default() += 1;
        counts.unowned_fields += u64::from(field.owner.is_none());
        if matches!(field.value, narrative::Value::Condition(_)) {
            counts.conditions += 1;
            *counts
                .condition_owners
                .entry(owner_name(&document, field.owner))
                .or_default() += 1;
        }
    }
    let mut scripts = Vec::new();
    for script in &document.scripts {
        counts.script_units += 1;
        *counts
            .script_owners
            .entry(owner_name(&document, script.owner))
            .or_default() += 1;
        if let Some(compiled) = script.unit.compiled {
            counts.compiled_bodies += 1;
            counts.compiled_bytes += compiled.data.len() as u64;
        }
        scripts.push(ScriptRow {
            header_decoded_offset: script.unit.header.offset,
            owner: script.owner,
            metadata_sha256: script_bindings::metadata_digest(&script.unit),
            compiled_bytes: script.unit.compiled.map(|field| field.data.len()),
            compiled_sha256: script
                .unit
                .compiled
                .map(|field| format!("{:x}", Sha256::digest(field.data))),
        });
    }
    Row {
        record_kind: plugin::signature(record.header.kind),
        form_id: record.header.form_id,
        record_file_offset: record.header.offset,
        record_flags: record.header.flags,
        decoded_bytes: record.payload.len(),
        decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
        fields: document.fields.len(),
        fields_sha256: fields_digest(&document),
        sections: document.sections,
        scripts,
        findings: document.findings,
    }
}

pub fn inspect(
    path: &Path,
    focused: bool,
    mut observe: impl FnMut(&plugin::Record) -> Result<()>,
) -> Result<Report> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::Unsupported("narrative filename encoding".into()))?;
    crate::identity::plugin_name(name)?;
    let mut file = baseline::open_source(path)?;
    let (source_bytes, source_sha256) =
        baseline::digest_reader(&mut file).map_err(|error| io(path, error))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io(path, error))?;
    let mut report = Report {
        source_name: name.into(),
        source_bytes,
        source_sha256,
        focused_scan: focused,
        record_payloads_decoded: 0,
        record_payloads_deferred: 0,
        counts: Counts::default(),
        rows: Vec::new(),
        execution_ready: false,
        retail_parity_accepted: false,
    };
    plugin::visit_selected(
        &mut BufReader::new(file),
        source_bytes,
        name,
        plugin::Limits::default(),
        |header| !focused || narrative::narrative_record(header.kind),
        |event| {
            let record = match event {
                plugin::SelectedEvent::Record(record) => record,
                plugin::SelectedEvent::Deferred(_) => {
                    report.record_payloads_deferred += 1;
                    return Ok(());
                }
                plugin::SelectedEvent::Group(_) => return Ok(()),
            };
            report.record_payloads_decoded += 1;
            if !narrative::narrative_record(record.header.kind) {
                return Ok(());
            }
            if report.rows.len() >= 262_144 {
                return Err(crate::Error::Unsupported(
                    "narrative record budget exceeded".into(),
                ));
            }
            let document = narrative::decode(record, name, narrative::Limits::default())?;
            let row = summarize(record, document, &mut report.counts);
            if report.counts.fields > 4_000_000
                || report.counts.sections > 1_000_000
                || report.counts.findings > 262_144
            {
                return Err(crate::Error::Unsupported(
                    "narrative aggregate budget exceeded".into(),
                ));
            }
            report.rows.push(row);
            observe(record)?;
            Ok(())
        },
    )?;
    Ok(report)
}
