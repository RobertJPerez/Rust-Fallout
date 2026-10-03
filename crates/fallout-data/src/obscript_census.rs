//! A source-bound census of compiled bodies, with instruction IDs kept separate
//! from event IDs. Counts describe authored definitions, not override winners.

use crate::{
    Result, baseline, io,
    obscript::{self, Kind},
    plugin::{self, SelectedEvent},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufReader, Seek, SeekFrom},
    path::Path,
};

/// The pinned FNV schema and existing full corpus census contain SCDA here.
/// Other record bodies remain explicitly deferred in the focused scan.
pub fn script_record(kind: [u8; 4]) -> bool {
    matches!(
        &kind,
        b"SCPT"
            | b"INFO"
            | b"QUST"
            | b"PACK"
            | b"PERK"
            | b"TERM"
            | b"REFR"
            | b"ACHR"
            | b"ACRE"
            | b"PGRE"
            | b"PMIS"
            | b"PBEA"
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct Site {
    pub record_kind: String,
    pub form_id: u32,
    pub record_file_offset: u64,
    pub scda_field_decoded_offset: usize,
    pub scda_data_decoded_offset: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub compiled_bodies: u64,
    pub compiled_bytes: u64,
    pub instructions: u64,
    pub reference_calls: u64,
    pub event_blocks: u64,
    pub instruction_opcodes: BTreeMap<u16, u64>,
    pub top_level_native_commands: BTreeMap<u16, u64>,
    pub statement_opcodes: BTreeMap<u16, u64>,
    pub unknown_opcodes: BTreeMap<u16, u64>,
    pub event_ids: BTreeMap<u16, u64>,
}

#[derive(Debug, Serialize)]
pub struct Body {
    pub site: Site,
    pub bytes: usize,
    pub sha256: String,
    pub declared_bytes: Option<u32>,
    pub declared_references: Option<u32>,
    pub declared_variables: Option<u32>,
    pub script_type: Option<u16>,
    pub instructions: Option<usize>,
    pub framing_sha256: Option<String>,
    pub reference_calls: usize,
    pub event_blocks: usize,
    pub issues: Vec<String>,
}

/// A fixed little-endian tuple for each header permits an offline implementation
/// to compare every boundary and ID without trusting our JSON serialization.
/// The source digest separately covers all opaque operand bytes.
pub fn framing_digest(program: &obscript::Program<'_>) -> String {
    let mut digest = Sha256::new();
    for instruction in &program.instructions {
        for offset in [
            instruction.bytes.start,
            instruction.bytes.end,
            instruction.operand_offset,
        ] {
            digest.update((offset as u32).to_le_bytes());
        }
        digest.update(instruction.opcode.to_le_bytes());
        digest.update([u8::from(instruction.calling_reference.is_some())]);
        digest.update(instruction.calling_reference.unwrap_or(0).to_le_bytes());
        digest.update([u8::from(instruction.event.is_some())]);
        digest.update(instruction.event.map_or(0, |event| event.id).to_le_bytes());
        digest.update(
            instruction
                .event
                .map_or(0, |event| event.end_jump_bytes)
                .to_le_bytes(),
        );
    }
    format!("{:x}", digest.finalize())
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub focused_scan: bool,
    pub record_payloads_decoded: u64,
    pub record_payloads_deferred: u64,
    pub counts: Counts,
    pub bodies: Vec<Body>,
    pub bodies_with_issues: usize,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
    pub unknown: Vec<&'static str>,
}

/// The observer is only for an offline comparison bundle. Source bytes stay
/// borrowed during the visit; the report contains counts, hashes and provenance.
pub fn inspect(
    path: &Path,
    focused: bool,
    mut observe: impl FnMut(&Site, &[u8], &obscript::Program<'_>) -> Result<()>,
) -> Result<Report> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| crate::Error::Unsupported("script source filename encoding".into()))?;
    crate::identity::plugin_name(name)?;
    let mut file = baseline::open_source(path)?;
    let (source_bytes, source_sha256) =
        baseline::digest_reader(&mut file).map_err(|error| io(path, error))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| io(path, error))?;
    let mut report = Report {
        schema_version: 1,
        source_name: name.into(),
        source_bytes,
        source_sha256,
        focused_scan: focused,
        record_payloads_decoded: 0,
        record_payloads_deferred: 0,
        counts: Counts::default(),
        bodies: Vec::new(),
        bodies_with_issues: 0,
        execution_ready: false,
        retail_parity_accepted: false,
        unknown: vec![
            "instruction operands, expression command calls and native argument signatures",
            "jump origin/targets, event execution and script scheduling",
            "script reference/local-variable binding and behavior",
            "effective winning embedded scripts and retail observations",
        ],
    };
    plugin::visit_selected(
        &mut BufReader::new(file),
        source_bytes,
        name,
        plugin::Limits::default(),
        |header| !focused || script_record(header.kind),
        |event| {
            let record = match event {
                SelectedEvent::Record(record) => record,
                SelectedEvent::Deferred(_) => {
                    report.record_payloads_deferred += 1;
                    return Ok(());
                }
                SelectedEvent::Group(_) => return Ok(()),
            };
            report.record_payloads_decoded += 1;
            let mut header: Option<[u8; 20]> = None;
            let mut malformed_header = false;
            plugin::visit_subrecords(record, name, |sub| {
                if sub.kind == *b"SCHR" {
                    header = sub.data.try_into().ok();
                    malformed_header = header.is_none();
                }
                if sub.kind != *b"SCDA" {
                    return Ok(());
                }
                if report.bodies.len() >= 262_144
                    || report.counts.compiled_bytes + sub.data.len() as u64 > 64 * 1024 * 1024
                {
                    return Err(crate::Error::Unsupported(
                        "compiled script census exceeds 262144 bodies or 64 MiB".into(),
                    ));
                }
                // A SCHR belongs to one compiled body. Reusing it for a second
                // SCDA would make malformed embedded-script ordering look valid.
                let body_header = header.take();
                let site = Site {
                    record_kind: plugin::signature(record.header.kind),
                    form_id: record.header.form_id,
                    record_file_offset: record.header.offset,
                    scda_field_decoded_offset: sub.payload_offset,
                    scda_data_decoded_offset: sub.payload_offset + 6,
                };
                let field32 = |offset: usize| {
                    body_header.map(|h| {
                        u32::from_le_bytes(h[offset..offset + 4].try_into().expect("fixed SCHR"))
                    })
                };
                let mut body = Body {
                    site,
                    bytes: sub.data.len(),
                    sha256: format!("{:x}", Sha256::digest(sub.data)),
                    declared_references: field32(4),
                    declared_bytes: field32(8),
                    declared_variables: field32(12),
                    script_type: body_header.map(|h| u16::from_le_bytes([h[16], h[17]])),
                    instructions: None,
                    framing_sha256: None,
                    reference_calls: 0,
                    event_blocks: 0,
                    issues: Vec::new(),
                };
                if malformed_header {
                    body.issues.push("preceding SCHR is not 20 bytes".into());
                } else if body_header.is_none() {
                    body.issues
                        .push("no preceding SCHR for compiled body".into());
                }
                if body
                    .declared_bytes
                    .is_some_and(|bytes| bytes as usize != body.bytes)
                {
                    body.issues
                        .push("SCHR compiled size differs from SCDA extent".into());
                }
                report.counts.compiled_bodies += 1;
                report.counts.compiled_bytes += body.bytes as u64;
                match obscript::decode(sub.data, obscript::Limits::default()) {
                    Err(error) => body.issues.push(error.to_string()),
                    Ok(program) => {
                        body.instructions = Some(program.instructions.len());
                        body.framing_sha256 = Some(framing_digest(&program));
                        report.counts.instructions += program.instructions.len() as u64;
                        for instruction in &program.instructions {
                            *report
                                .counts
                                .instruction_opcodes
                                .entry(instruction.opcode)
                                .or_default() += 1;
                            let counts = match instruction.kind() {
                                Kind::NativeCommand => &mut report.counts.top_level_native_commands,
                                Kind::Statement(_) => &mut report.counts.statement_opcodes,
                                Kind::Unknown => &mut report.counts.unknown_opcodes,
                            };
                            *counts.entry(instruction.opcode).or_default() += 1;
                            if instruction.calling_reference.is_some() {
                                body.reference_calls += 1;
                            }
                            if let Some(event) = instruction.event {
                                body.event_blocks += 1;
                                *report.counts.event_ids.entry(event.id).or_default() += 1;
                            }
                        }
                        report.counts.reference_calls += body.reference_calls as u64;
                        report.counts.event_blocks += body.event_blocks as u64;
                        // A failed observer prevents publication of a misleading complete scan.
                        observe(&body.site, sub.data, &program)?;
                    }
                }
                if !body.issues.is_empty() {
                    report.bodies_with_issues += 1;
                }
                report.bodies.push(body);
                Ok(())
            })
        },
    )?;
    Ok(report)
}
