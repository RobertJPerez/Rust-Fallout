//! Source-bound script table inspection. This establishes authored associations,
//! not loaded forms, runtime reference values or winning embedded-script identity.

use crate::{
    Result, baseline, identity, io, malformed, obscript, obscript_census, plugin, script_units,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub units: u64,
    pub compiled_bodies: u64,
    pub compiled_bytes: u64,
    pub source_fields: u64,
    pub variables: u64,
    pub duplicate_variable_indices: u64,
    pub conflicting_variable_indices: u64,
    pub form_references: u64,
    pub variable_references: u64,
    pub reference_calls: u64,
    pub calls_to_forms: u64,
    pub calls_to_variables: u64,
    pub declared_variable_count_equals_length: u64,
    pub declared_variable_count_equals_max_index: u64,
    pub variable_type_bytes: BTreeMap<u8, u64>,
    pub script_types: BTreeMap<u16, u64>,
    pub script_flags: BTreeMap<u16, u64>,
}

#[derive(Debug, Serialize)]
pub struct UnitReport {
    pub record_kind: String,
    pub form_id: u32,
    pub record_file_offset: u64,
    pub header_decoded_offset: usize,
    pub script_fields: usize,
    pub metadata_sha256: String,
    pub declared_references: u32,
    pub declared_compiled_bytes: u32,
    pub declared_variables: u32,
    pub compiled_bytes: Option<usize>,
    pub variables: usize,
    pub duplicate_variable_indices: usize,
    pub conflicting_variable_indices: usize,
    pub max_variable_index: Option<u32>,
    pub references: usize,
    pub reference_calls: usize,
    pub calls_to_forms: usize,
    pub calls_to_variables: usize,
    pub caller_bindings_sha256: String,
    pub stable_form_keys_sha256: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub masters: Vec<String>,
    pub focused_scan: bool,
    pub record_payloads_decoded: u64,
    pub record_payloads_deferred: u64,
    pub counts: Counts,
    pub units: Vec<UnitReport>,
    pub units_with_issues: usize,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
}

/// Every authored script field contributes its signature, decoded header offset,
/// length (both u32 LE) and complete raw bytes. This includes unused bytes and
/// source text, without depending on a text encoding or publishing that text.
pub fn metadata_digest(unit: &script_units::Unit<'_>) -> String {
    let mut hash = Sha256::new();
    for field in &unit.fields {
        hash.update(field.kind);
        hash.update((field.offset as u32).to_le_bytes());
        hash.update((field.data.len() as u32).to_le_bytes());
        hash.update(field.data);
    }
    format!("{:x}", hash.finalize())
}

fn masters(record: &plugin::Record, name: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut unique = BTreeSet::new();
    let mut pending = false;
    let mut hedr = false;
    plugin::visit_subrecords(record, name, |sub| {
        let fail = |reason| malformed(name, 0, reason);
        if pending && sub.kind != *b"DATA" {
            return Err(fail("MAST lacks DATA"));
        }
        match &sub.kind {
            b"HEDR" => {
                if hedr || sub.data.len() != 12 {
                    return Err(fail("invalid HEDR"));
                }
                let version = u32::from_le_bytes(sub.data[..4].try_into().expect("checked HEDR"));
                if ![1.32_f32.to_bits(), 1.33_f32.to_bits(), 1.34_f32.to_bits()].contains(&version)
                {
                    return Err(fail("unsupported FNV header version"));
                }
                hedr = true;
            }
            b"MAST" => {
                let raw = sub
                    .data
                    .strip_suffix(&[0])
                    .ok_or_else(|| fail("unterminated MAST"))?;
                if raw.contains(&0) {
                    return Err(fail("embedded NUL in MAST"));
                }
                let master =
                    std::str::from_utf8(raw).map_err(|_| fail("master filename encoding"))?;
                let key = identity::plugin_name(master)?;
                if result.len() >= 254 || !unique.insert(key) {
                    return Err(fail("master count or duplicate"));
                }
                result.push(master.to_owned());
                pending = true;
            }
            b"DATA" if pending => {
                if sub.data.len() != 8 {
                    return Err(fail("master DATA must contain eight bytes"));
                }
                pending = false;
            }
            _ => {}
        }
        Ok(())
    })?;
    if !hedr || pending {
        return Err(malformed(name, 0, "incomplete plugin header"));
    }
    Ok(result)
}

pub fn inspect(
    path: &Path,
    focused: bool,
    mut observe: impl FnMut(&plugin::Record, &[script_units::Unit<'_>]) -> Result<()>,
) -> Result<Report> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::Unsupported("script source filename encoding".into()))?;
    identity::plugin_name(name)?;
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
        masters: Vec::new(),
        focused_scan: focused,
        record_payloads_decoded: 0,
        record_payloads_deferred: 0,
        counts: Counts::default(),
        units: Vec::new(),
        units_with_issues: 0,
        execution_ready: false,
        retail_parity_accepted: false,
    };
    plugin::visit_selected(
        &mut BufReader::new(file),
        source_bytes,
        name,
        plugin::Limits::default(),
        |header| !focused || obscript_census::script_record(header.kind),
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
            if record.header.kind == *b"TES4" {
                report.masters = masters(record, name)?;
                return Ok(());
            }
            let units = script_units::decode(record, name, script_units::Limits::default())?;
            if report.units.len() + units.len() > 262_144 {
                return Err(crate::Error::Unsupported(
                    "script unit census budget exceeded".into(),
                ));
            }
            if !units.is_empty() {
                observe(record, &units)?;
            }
            for unit in units {
                let mut row = UnitReport {
                    record_kind: plugin::signature(record.header.kind),
                    form_id: record.header.form_id,
                    record_file_offset: record.header.offset,
                    header_decoded_offset: unit.header.offset,
                    script_fields: unit.fields.len(),
                    metadata_sha256: metadata_digest(&unit),
                    declared_references: unit.declared_references(),
                    declared_compiled_bytes: unit.declared_compiled_bytes(),
                    declared_variables: unit.declared_variables(),
                    compiled_bytes: unit.compiled.map(|field| field.data.len()),
                    variables: unit.variables.len(),
                    duplicate_variable_indices: 0,
                    conflicting_variable_indices: 0,
                    max_variable_index: unit.variables.iter().map(|v| v.index).max(),
                    references: unit.references.len(),
                    reference_calls: 0,
                    calls_to_forms: 0,
                    calls_to_variables: 0,
                    caller_bindings_sha256: String::new(),
                    stable_form_keys_sha256: String::new(),
                    issues: Vec::new(),
                };
                report.counts.units += 1;
                report.counts.source_fields += u64::from(unit.source.is_some());
                report.counts.variables += unit.variables.len() as u64;
                report.counts.declared_variable_count_equals_length +=
                    u64::from(row.declared_variables as usize == row.variables);
                report.counts.declared_variable_count_equals_max_index +=
                    u64::from(row.declared_variables == row.max_variable_index.unwrap_or(0));
                *report
                    .counts
                    .script_types
                    .entry(unit.script_type())
                    .or_default() += 1;
                *report.counts.script_flags.entry(unit.flags()).or_default() += 1;
                for variable in &unit.variables {
                    *report
                        .counts
                        .variable_type_bytes
                        .entry(variable.type_byte)
                        .or_default() += 1;
                }
                let mut seen = BTreeSet::new();
                for variable in &unit.variables {
                    if !seen.insert(variable.index) {
                        row.duplicate_variable_indices += 1;
                        let first = unit.variable(variable.index).expect("indexed declaration");
                        row.conflicting_variable_indices += usize::from(
                            first.declaration.data != variable.declaration.data
                                || first.name.data != variable.name.data,
                        );
                    }
                }
                report.counts.duplicate_variable_indices += row.duplicate_variable_indices as u64;
                report.counts.conflicting_variable_indices +=
                    row.conflicting_variable_indices as u64;
                if row.declared_references as usize != row.references {
                    row.issues
                        .push("SCHR reference count differs from ordered table length".into());
                }
                if row.declared_compiled_bytes as usize != row.compiled_bytes.unwrap_or(0) {
                    row.issues
                        .push("SCHR compiled size differs from SCDA extent".into());
                }
                let mut stable_forms = Sha256::new();
                for (position, reference) in unit.references.iter().enumerate() {
                    match reference.target {
                        script_units::Reference::Form(raw) => {
                            report.counts.form_references += 1;
                            let key = identity::resolve_form(
                                identity::ProfileId::NvOriginal,
                                name,
                                &report.masters,
                                raw,
                            )?;
                            let bytes = serde_json::to_vec(&key).expect("FormKey serialization");
                            stable_forms.update(((position + 1) as u32).to_le_bytes());
                            stable_forms.update((bytes.len() as u32).to_le_bytes());
                            stable_forms.update(bytes);
                        }
                        script_units::Reference::Variable(index) => {
                            report.counts.variable_references += 1;
                            if unit.variable(index).is_none() {
                                row.issues
                                    .push(format!("SCRV index {index} has no local declaration"));
                            }
                        }
                    }
                }
                row.stable_form_keys_sha256 = format!("{:x}", stable_forms.finalize());
                let mut calls = Sha256::new();
                if let Some(compiled) = unit.compiled {
                    report.counts.compiled_bodies += 1;
                    report.counts.compiled_bytes += compiled.data.len() as u64;
                    if report.counts.compiled_bytes > 64 * 1024 * 1024 {
                        return Err(crate::Error::Unsupported(
                            "script compiled-byte census budget exceeded".into(),
                        ));
                    }
                    match obscript::decode(compiled.data, obscript::Limits::default()) {
                        Err(error) => row.issues.push(error.to_string()),
                        Ok(program) => {
                            for instruction in program.instructions {
                                if let Some(index) = instruction.calling_reference {
                                    row.reference_calls += 1;
                                    let Some(reference) = unit.reference(u32::from(index)) else {
                                        row.issues.push(format!("SCDA byte 0x{:X}: caller reference {index} is outside the one-based table", instruction.bytes.start));
                                        continue;
                                    };
                                    calls.update((instruction.bytes.start as u32).to_le_bytes());
                                    calls.update(index.to_le_bytes());
                                    let (kind, value) = match reference.target {
                                        script_units::Reference::Form(raw) => {
                                            row.calls_to_forms += 1;
                                            (0, raw)
                                        }
                                        script_units::Reference::Variable(index) => {
                                            row.calls_to_variables += 1;
                                            (1, index)
                                        }
                                    };
                                    calls.update([kind]);
                                    calls.update(value.to_le_bytes());
                                }
                            }
                        }
                    }
                }
                row.caller_bindings_sha256 = format!("{:x}", calls.finalize());
                report.counts.reference_calls += row.reference_calls as u64;
                report.counts.calls_to_forms += row.calls_to_forms as u64;
                report.counts.calls_to_variables += row.calls_to_variables as u64;
                report.units_with_issues += usize::from(!row.issues.is_empty());
                report.units.push(row);
            }
            Ok(())
        },
    )?;
    Ok(report)
}
