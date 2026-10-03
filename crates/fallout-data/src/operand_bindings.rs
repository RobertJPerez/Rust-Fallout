//! Whole-corpus operand/table associations. Source metadata inconsistencies and
//! foreign local declarations remain separate from malformed compiled operands.
use crate::{Result, obscript, plugin, script_bindings, script_units};
use obscript::{argument_census::Signatures, expression::Operators, operand_binding};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct UnitReport {
    pub record_kind: String,
    pub form_id: u32,
    pub record_file_offset: u64,
    pub header_decoded_offset: usize,
    pub metadata_sha256: String,
    pub compiled_bytes: usize,
    pub compiled_sha256: String,
    pub binding_sha256: String,
    pub counts: operand_binding::Counts,
    pub decode_issues: Vec<operand_binding::Issue>,
    pub missing_bindings: Vec<operand_binding::Use>,
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
    pub table_counts: script_bindings::Counts,
    pub table_units_with_issues: usize,
    pub table_units_with_issues_and_compiled_bytes: usize,
    pub decode_issues: usize,
    pub missing_bindings: u64,
    pub deferred_foreign_locals: u64,
    pub compiled_units: Vec<UnitReport>,
    pub execution_ready: bool,
    pub retail_parity_accepted: bool,
}

pub fn inspect(
    path: &Path,
    focused: bool,
    operators: &Operators,
    signatures: &Signatures,
    mut observe: impl FnMut(&plugin::Record, &[script_units::Unit<'_>]) -> Result<()>,
) -> Result<Report> {
    let mut compiled_units = Vec::new();
    let mut uses = 0_u64;
    let mut decode_issues = 0;
    let mut missing_bindings = 0;
    let mut deferred_foreign_locals = 0;
    let tables = script_bindings::inspect(path, focused, |record, units| {
        observe(record, units)?;
        for unit in units {
            let Some(compiled) = unit.compiled else {
                continue;
            };
            if compiled_units.len() >= 65_536 {
                return Err(crate::Error::Unsupported(
                    "operand binding unit budget exceeded".into(),
                ));
            }
            let program = obscript::decode(compiled.data, obscript::Limits::default())
                .map_err(|error| crate::Error::Resolution(error.to_string()))?;
            let binding = operand_binding::bind(unit, &program, operators, signatures, 262_144)?;
            uses += binding.counts.uses;
            if uses > 1_000_000 {
                return Err(crate::Error::Unsupported(
                    "operand binding plugin use budget exceeded".into(),
                ));
            }
            decode_issues += binding.decode_issues.len();
            missing_bindings += binding.counts.missing_bindings;
            deferred_foreign_locals += binding.counts.deferred_foreign_locals;
            compiled_units.push(UnitReport {
                record_kind: plugin::signature(record.header.kind),
                form_id: record.header.form_id,
                record_file_offset: record.header.offset,
                header_decoded_offset: unit.header.offset,
                metadata_sha256: script_bindings::metadata_digest(unit),
                compiled_bytes: compiled.data.len(),
                compiled_sha256: format!("{:x}", Sha256::digest(compiled.data)),
                binding_sha256: operand_binding::digest(&binding.uses),
                counts: binding.counts,
                decode_issues: binding.decode_issues,
                missing_bindings: binding
                    .uses
                    .into_iter()
                    .filter(|row| row.status >= 5)
                    .collect(),
            });
        }
        Ok(())
    })?;
    let compiled_with_issues = tables
        .units
        .iter()
        .filter(|unit| {
            !unit.issues.is_empty() && unit.compiled_bytes.is_some_and(|bytes| bytes > 0)
        })
        .count();
    Ok(Report {
        schema_version: 1,
        source_name: tables.source_name,
        source_bytes: tables.source_bytes,
        source_sha256: tables.source_sha256,
        focused_scan: focused,
        record_payloads_decoded: tables.record_payloads_decoded,
        record_payloads_deferred: tables.record_payloads_deferred,
        table_counts: tables.counts,
        table_units_with_issues: tables.units_with_issues,
        table_units_with_issues_and_compiled_bytes: compiled_with_issues,
        decode_issues,
        missing_bindings,
        deferred_foreign_locals,
        compiled_units,
        execution_ready: false,
        retail_parity_accepted: false,
    })
}
