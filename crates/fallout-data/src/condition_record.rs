//! Exact winning-record CTDA source preparation. No grouping or truth evaluation.
use super::{Binding, Signatures, Subject, bind};
use crate::{
    Error, Result, condition,
    identity::FormKey,
    plugin,
    store::{Location, RecordStore},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct RecordLimits {
    pub maximum_decoded_bytes: usize,
    pub maximum_fields: usize,
    pub maximum_conditions: usize,
    pub maximum_retained_bytes: usize,
}
impl Default for RecordLimits {
    fn default() -> Self {
        Self {
            maximum_decoded_bytes: 64 * 1024 * 1024,
            maximum_fields: 1_048_576,
            maximum_conditions: 1_000_000,
            maximum_retained_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RecordIdentity {
    pub key: FormKey,
    pub source_name: String,
    pub source_sha256: String,
    /// Ordered complete-source receipts, not a subset or runtime-state identity.
    pub source_cohort_sha256: String,
    pub record_kind: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_bytes: usize,
    pub decoded_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct ConditionSite {
    field_decoded_offset: usize,
    preceding_field_kind: Option<String>,
    preceding_field_decoded_offset: Option<usize>,
    raw_bytes: Vec<u8>,
    sha256: String,
    binding: Binding,
    source_findings: Vec<&'static str>,
}
impl ConditionSite {
    pub fn field_decoded_offset(&self) -> usize {
        self.field_decoded_offset
    }
    pub fn preceding_field_kind(&self) -> Option<&str> {
        self.preceding_field_kind.as_deref()
    }
    pub fn preceding_field_decoded_offset(&self) -> Option<usize> {
        self.preceding_field_decoded_offset
    }
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn source_findings(&self) -> &[&'static str] {
        &self.source_findings
    }
    pub fn condition(&self) -> condition::Condition<'_> {
        condition::decode(&self.raw_bytes).expect("privately admitted immutable CTDA bytes")
    }
    /// Existing source inspector projection. Extra raw/provenance fields remain
    /// accessible without changing its schema or binding digest.
    pub fn legacy_row(&self) -> impl Serialize + '_ {
        let condition = self.condition();
        LegacyRow {
            field_decoded_offset: self.field_decoded_offset,
            bytes: self.raw_bytes.len(),
            sha256: &self.sha256,
            preceding_field_kind: self.preceding_field_kind.as_deref(),
            preceding_field_decoded_offset: self.preceding_field_decoded_offset,
            flags: condition.flags,
            flag_padding: condition.flag_padding,
            function_id: condition.function_id,
            function_padding: condition.function_padding,
            comparison_operator: condition.comparison_operator(),
            comparison_value: condition.comparison_value(),
            or_flag: condition.or_flag(),
            parameter_words: condition.parameter_words,
            run_on_word: condition.run_on_word,
            reference_word: condition.reference_word,
            binding: &self.binding,
            source_findings: &self.source_findings,
        }
    }
}

#[derive(Serialize)]
struct LegacyRow<'a> {
    field_decoded_offset: usize,
    bytes: usize,
    sha256: &'a str,
    preceding_field_kind: Option<&'a str>,
    preceding_field_decoded_offset: Option<usize>,
    flags: u8,
    flag_padding: [u8; 3],
    function_id: u16,
    function_padding: [u8; 2],
    comparison_operator: condition::ComparisonOperator,
    comparison_value: condition::ComparisonValue,
    or_flag: bool,
    parameter_words: [u32; 2],
    run_on_word: Option<u32>,
    reference_word: Option<u32>,
    binding: &'a Binding,
    source_findings: &'a [&'static str],
}

/// Private fields prevent caller-assembled sites from acquiring source identity.
/// Physical ordering does not establish an owning AND/OR evaluation group.
#[derive(Debug, Serialize)]
pub struct PreparedRecord {
    identity: RecordIdentity,
    fields: usize,
    sites: Vec<ConditionSite>,
    retained_bytes: usize,
}
impl PreparedRecord {
    pub fn identity(&self) -> &RecordIdentity {
        &self.identity
    }
    pub fn sites(&self) -> &[ConditionSite] {
        &self.sites
    }
    pub fn fields(&self) -> usize {
        self.fields
    }
    /// Compact legacy row bytes plus raw CTDA bytes; not total heap accounting.
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "condition retained-site byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn prepare_record(
    store: &mut RecordStore,
    location: Location,
    signatures: &Signatures,
    limits: RecordLimits,
) -> Result<PreparedRecord> {
    let header = store
        .indices()
        .get(location.plugin)
        .and_then(|index| index.records.get(location.record))
        .ok_or_else(|| Error::Resolution("condition source location is outside this store".into()))?
        .header
        .clone();
    let key = store
        .key_for(location, header.form_id)?
        .ok_or_else(|| Error::Resolution("condition source has null record identity".into()))?;
    let winner = store
        .winner(&key)
        .ok_or_else(|| Error::Resolution("condition source has no winner".into()))?;
    if (winner.plugin, winner.record) != (location.plugin, location.record) {
        return Err(Error::Resolution(
            "condition source is not the exact winning record".into(),
        ));
    }
    if header.flags & plugin::DELETED != 0 {
        return Err(Error::Resolution(
            "condition source winning record is deleted".into(),
        ));
    }
    // Tighten both stored and decoded bounds before the strict reader allocates.
    let record = store.read_bounded(location, limits.maximum_decoded_bytes)?;
    if record.integrity_issue.is_some() {
        return Err(Error::Unsupported(
            "condition source cannot admit untrusted checksum recovery".into(),
        ));
    }
    let source_name = store.source_name(location).to_string();
    let receipts = store.source_receipts()?;
    let mut hash = Sha256::new();
    hash.update(b"FNVCTDASOURCES1");
    hash.update(
        serde_json::to_vec(&receipts).map_err(|error| Error::Resolution(error.to_string()))?,
    );
    let identity = RecordIdentity {
        key,
        source_name: source_name.clone(),
        source_sha256: store.source_digest(location)?,
        source_cohort_sha256: format!("{:x}", hash.finalize()),
        record_kind: plugin::signature(header.kind),
        record_file_offset: header.offset,
        record_flags: header.flags,
        decoded_bytes: record.payload.len(),
        decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
    };
    let mut sites = Vec::new();
    let mut previous = None;
    let mut fields = 0;
    let mut retained_bytes = 0;
    plugin::visit_subrecords(&record, &source_name, |field| {
        if fields >= limits.maximum_fields {
            return Err(Error::Unsupported("condition field budget exceeded".into()));
        }
        fields += 1;
        if field.kind == *b"CTDA" {
            if sites.len() >= limits.maximum_conditions {
                return Err(Error::Unsupported("condition row budget exceeded".into()));
            }
            let condition = condition::decode(field.data).map_err(|error| Error::Format {
                source_name: source_name.clone(),
                offset: header.offset,
                reason: format!("CTDA at decoded offset {}: {error}", field.payload_offset),
            })?;
            let binding = bind(
                store,
                location,
                &condition,
                signatures.get(&condition.function_id),
            )?;
            let mut findings = Vec::new();
            if condition.flags & 0x1a != 0 {
                findings.push("uninterpreted_low_flags");
            }
            if condition.flag_padding != [0; 3] {
                findings.push("nonzero_flag_padding");
            }
            if condition.function_padding != [0; 2] {
                findings.push("nonzero_function_padding");
            }
            if matches!(
                condition.comparison_operator(),
                condition::ComparisonOperator::Unknown(_)
            ) {
                findings.push("unknown_comparison_operator");
            }
            if condition.finite_float_comparison_word() == Some(false) {
                findings.push("nonfinite_comparison");
            }
            if matches!(binding.subject, Subject::Unknown { .. }) {
                findings.push("unknown_subject_selector");
            }
            let site = ConditionSite {
                field_decoded_offset: field.payload_offset,
                preceding_field_kind: previous.as_ref().map(|(kind, _)| plugin::signature(*kind)),
                preceding_field_decoded_offset: previous.as_ref().map(|(_, offset)| *offset),
                raw_bytes: field.data.to_vec(),
                sha256: format!("{:x}", Sha256::digest(field.data)),
                binding,
                source_findings: findings,
            };
            // Count escaped/UTF-8 JSON before retaining a row. Raw CTDA work is
            // charged separately so a different consumer cannot bypass it.
            let remaining = limits.maximum_retained_bytes.saturating_sub(retained_bytes);
            if site.raw_bytes.len() > remaining {
                return Err(Error::Unsupported(
                    "condition retained-site byte budget exceeded".into(),
                ));
            }
            let mut admission = Admission {
                bytes: 0,
                maximum: remaining - site.raw_bytes.len(),
            };
            serde_json::to_writer(&mut admission, &site.legacy_row())
                .map_err(|error| Error::Unsupported(error.to_string()))?;
            retained_bytes += site.raw_bytes.len() + admission.bytes;
            sites.push(site);
        }
        previous = Some((field.kind, field.payload_offset));
        Ok(())
    })?;
    Ok(PreparedRecord {
        identity,
        fields,
        sites,
        retained_bytes,
    })
}
