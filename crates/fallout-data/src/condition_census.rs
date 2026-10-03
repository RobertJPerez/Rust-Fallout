//! Condition fields retain record/decoded offsets and authored order. This scan
//! does not combine conditions belonging to separate quest stages, perks or topics.
use crate::{Result, baseline, condition, io, malformed, plugin};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufReader, Seek, SeekFrom},
    path::Path,
};

/// These FNV records contain wbConditions directly or through wbEffects in the
/// pinned schema. Coverage is checked against the earlier complete corpus census.
pub fn condition_record(kind: [u8; 4]) -> bool {
    matches!(
        &kind,
        b"ALCH"
            | b"ENCH"
            | b"INGR"
            | b"SPEL"
            | b"TERM"
            | b"PERK"
            | b"CPTH"
            | b"MESG"
            | b"IDLE"
            | b"INFO"
            | b"PACK"
            | b"QUST"
            | b"RCPE"
    )
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub conditions: u64,
    pub record_kinds: BTreeMap<String, u64>,
    pub lengths: BTreeMap<usize, u64>,
    pub functions: BTreeMap<u16, u64>,
    pub flags: BTreeMap<u8, u64>,
    pub comparison_operators: BTreeMap<u8, u64>,
    pub or_flags: u64,
    pub global_comparisons: u64,
    pub nonfinite_float_comparisons: u64,
    pub nonzero_flag_padding: u64,
    pub nonzero_function_padding: u64,
    pub uninterpreted_low_flags: u64,
    pub unknown_comparison_operators: u64,
    pub absent_run_on_words: u64,
    pub absent_reference_words: u64,
    pub subject_run_on_words: BTreeMap<u32, u64>,
    pub animation_group_words: BTreeMap<u32, u64>,
    pub active_reference_words: u64,
    pub unverified_subject_selector_words: u64,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub record_kind: String,
    pub form_id: u32,
    pub record_file_offset: u64,
    pub field_decoded_offset: usize,
    pub preceding_field_kind: Option<String>,
    pub preceding_field_decoded_offset: Option<usize>,
    pub bytes: usize,
    pub sha256: String,
    pub flags: u8,
    pub flag_padding: [u8; 3],
    pub comparison_operator: condition::ComparisonOperator,
    pub comparison_value: condition::ComparisonValue,
    pub function_id: u16,
    pub function_padding: [u8; 2],
    pub parameter_words: [u32; 2],
    pub run_on_word: Option<u32>,
    pub run_on_domain: condition::RunOnDomain,
    pub reference_word: Option<u32>,
    pub reference_is_subject_selector: bool,
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
    pub rows: Vec<Row>,
    pub evaluation_ready: bool,
    pub retail_parity_accepted: bool,
}

pub fn inspect(
    path: &Path,
    focused: bool,
    mut observe: impl FnMut(&plugin::Record) -> Result<()>,
) -> Result<Report> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| crate::Error::Unsupported("condition filename encoding".into()))?;
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
        rows: Vec::new(),
        evaluation_ready: false,
        retail_parity_accepted: false,
    };
    plugin::visit_selected(
        &mut BufReader::new(file),
        source_bytes,
        name,
        plugin::Limits::default(),
        |header| !focused || condition_record(header.kind),
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
            let mut preceding = None;
            let mut field_count = 0;
            let start = report.rows.len();
            plugin::visit_subrecords(record, name, |field| {
                field_count += 1;
                if field_count > 1_048_576 {
                    return Err(crate::Error::Unsupported(
                        "condition record field budget exceeded".into(),
                    ));
                }
                if field.kind == *b"CTDA" {
                    if report.rows.len() >= 262_144 {
                        return Err(crate::Error::Unsupported(
                            "condition row budget exceeded".into(),
                        ));
                    }
                    let decoded = condition::decode(field.data).map_err(|error| {
                        malformed(
                            name,
                            record.header.offset,
                            format!("CTDA decoded offset 0x{:X}: {error}", field.payload_offset),
                        )
                    })?;
                    let counts = &mut report.counts;
                    counts.conditions += 1;
                    *counts
                        .record_kinds
                        .entry(plugin::signature(record.header.kind))
                        .or_default() += 1;
                    *counts.lengths.entry(field.data.len()).or_default() += 1;
                    *counts.functions.entry(decoded.function_id).or_default() += 1;
                    *counts.flags.entry(decoded.flags).or_default() += 1;
                    *counts
                        .comparison_operators
                        .entry(decoded.flags >> 5)
                        .or_default() += 1;
                    counts.or_flags += u64::from(decoded.or_flag());
                    counts.global_comparisons += u64::from(decoded.flags & 4 != 0);
                    counts.nonfinite_float_comparisons +=
                        u64::from(decoded.finite_float_comparison_word() == Some(false));
                    counts.nonzero_flag_padding += u64::from(decoded.flag_padding != [0; 3]);
                    counts.nonzero_function_padding +=
                        u64::from(decoded.function_padding != [0; 2]);
                    counts.uninterpreted_low_flags += u64::from(decoded.flags & 0x1a != 0);
                    counts.unknown_comparison_operators += u64::from(decoded.flags >> 5 > 5);
                    counts.absent_run_on_words += u64::from(decoded.run_on_word.is_none());
                    counts.absent_reference_words += u64::from(decoded.reference_word.is_none());
                    counts.active_reference_words += u64::from(
                        decoded.reference_is_subject_selector() && decoded.reference_word.is_some(),
                    );
                    if let Some(word) = decoded.run_on_word {
                        counts.unverified_subject_selector_words += u64::from(
                            decoded.run_on_domain() == condition::RunOnDomain::SubjectSelection
                                && word > 4,
                        );
                        let map = match decoded.run_on_domain() {
                            condition::RunOnDomain::AnimationGroup => {
                                &mut counts.animation_group_words
                            }
                            condition::RunOnDomain::SubjectSelection => {
                                &mut counts.subject_run_on_words
                            }
                        };
                        *map.entry(word).or_default() += 1;
                    }
                    report.rows.push(Row {
                        record_kind: plugin::signature(record.header.kind),
                        form_id: record.header.form_id,
                        record_file_offset: record.header.offset,
                        field_decoded_offset: field.payload_offset,
                        preceding_field_kind: preceding.map(|(kind, _)| plugin::signature(kind)),
                        preceding_field_decoded_offset: preceding.map(|(_, offset)| offset),
                        bytes: field.data.len(),
                        sha256: format!("{:x}", Sha256::digest(field.data)),
                        flags: decoded.flags,
                        flag_padding: decoded.flag_padding,
                        comparison_operator: decoded.comparison_operator(),
                        comparison_value: decoded.comparison_value(),
                        function_id: decoded.function_id,
                        function_padding: decoded.function_padding,
                        parameter_words: decoded.parameter_words,
                        run_on_word: decoded.run_on_word,
                        run_on_domain: decoded.run_on_domain(),
                        reference_word: decoded.reference_word,
                        reference_is_subject_selector: decoded.reference_is_subject_selector(),
                    });
                }
                preceding = Some((field.kind, field.payload_offset));
                Ok(())
            })?;
            if report.rows.len() > start {
                observe(record)?;
            }
            Ok(())
        },
    )?;
    Ok(report)
}
