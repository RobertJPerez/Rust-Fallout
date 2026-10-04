//! Load winning condition fields and their immutable dependencies. Record
//! ownership stays attached; this scan does not merge unrelated AND/OR lists.
use super::{Result, command_catalogue, inspection_input::Order};
use fallout_data::{
    condition_census,
    condition_operands::{self, Parameter, Signature, Signatures},
    plugin, record_metadata,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};

const MAXIMUM_CANDIDATES: usize = 262_144;
const MAXIMUM_DECODED_BYTES: usize = 512 * 1024 * 1024;
const MAXIMUM_CONDITIONS: usize = 1_000_000;
const MAXIMUM_RETAINED_BYTES: usize = 128 * 1024 * 1024;

fn bounded_candidates<T>(candidates: impl Iterator<Item = T>, maximum: usize) -> Result<Vec<T>> {
    let mut admitted = Vec::new();
    for candidate in candidates {
        if admitted.len() >= maximum {
            return Err("condition candidate budget exceeded".into());
        }
        admitted.push(candidate);
    }
    Ok(admitted)
}

struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "condition retained-report byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn admit_record(
    records: &mut Vec<Value>,
    row: Value,
    retained: &mut usize,
    maximum: usize,
) -> Result<()> {
    let mut admission = Admission {
        bytes: 0,
        maximum: maximum.saturating_sub(*retained),
    };
    serde_json::to_writer(&mut admission, &row)?;
    *retained += admission.bytes;
    records.push(row);
    Ok(())
}

fn increment(counts: &mut Value, map: &str, key: &str) {
    let count = counts[map][key].as_u64().unwrap_or(0);
    counts[map][key] = (count + 1).into();
}
fn number(counts: &mut Value, key: &str, amount: u64) {
    counts[key] = (counts[key].as_u64().expect("initialized condition count") + amount).into();
}
fn label(value: impl serde::Serialize) -> Result<String> {
    Ok(serde_json::to_value(value)?
        .as_str()
        .ok_or("Missing enum label")?
        .to_string())
}
pub(super) fn inspect(install: &Path, order_path: &Path, cache: Option<&Path>) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let sources = store.source_receipts()?;
    let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let signatures: Signatures = catalogue
        .script_commands
        .iter()
        .filter(|row| row.condition_handler_present)
        .map(|row| {
            (
                (row.id - 0x1000) as u16,
                Signature {
                    parameters: row
                        .parameters
                        .iter()
                        .map(|p| Parameter {
                            type_id: p.type_id,
                            optional_word: p.optional_word,
                        })
                        .collect(),
                },
            )
        })
        .collect();
    let candidates = bounded_candidates(
        store
            .winning_definitions()
            .filter(|(_, location)| {
                condition_census::condition_record(store.definition(*location).header.kind)
            })
            .map(|(_, location)| location),
        MAXIMUM_CANDIDATES,
    )?;
    let mut counts = json!({"candidate_records":candidates.len(),"deleted_candidate_records":0,"decoded_candidate_bytes":0,
        "records_with_conditions":0,"conditions":0,"source_findings":0,"unused_nonzero_words":0,"unknown_parameters":0,
        "variable_indices_need_live_state":0,"signature_statuses":{},"parameter_kinds":{},"parameter_domains":{},"subjects":{},"form_statuses":{}});
    let mut records = Vec::new();
    let mut retained = 0;
    for location in candidates {
        if store.definition(location).header.flags & plugin::DELETED != 0 {
            number(&mut counts, "deleted_candidate_records", 1);
            continue;
        }
        let prepared = condition_operands::prepare_record(
            &mut store,
            location,
            &signatures,
            condition_operands::RecordLimits {
                maximum_decoded_bytes: (MAXIMUM_DECODED_BYTES
                    - counts["decoded_candidate_bytes"]
                        .as_u64()
                        .expect("byte count") as usize)
                    .min(64 * 1024 * 1024),
                maximum_conditions: MAXIMUM_CONDITIONS
                    - counts["conditions"].as_u64().expect("condition count") as usize,
                maximum_retained_bytes: MAXIMUM_RETAINED_BYTES - retained,
                ..condition_operands::RecordLimits::default()
            },
        )?;
        let identity = prepared.identity();
        number(
            &mut counts,
            "decoded_candidate_bytes",
            identity.decoded_bytes as u64,
        );
        let mut rows = Vec::new();
        for site in prepared.sites() {
            let bound = site.binding();
            let binding = serde_json::to_value(bound)?;
            number(&mut counts, "conditions", 1);
            increment(
                &mut counts,
                "signature_statuses",
                binding["signature_status"].as_str().expect("status label"),
            );
            increment(
                &mut counts,
                "subjects",
                binding["subject"]["kind"].as_str().expect("subject label"),
            );
            for operand in binding["operands"].as_array().expect("two operands") {
                let kind = operand["value"]["kind"].as_str().expect("parameter label");
                increment(&mut counts, "parameter_kinds", kind);
                if let Some(domain) = operand["value"]["domain"].as_str() {
                    increment(&mut counts, "parameter_domains", domain);
                }
                if kind == "unused" && operand["value"]["raw_word"] != 0 {
                    number(&mut counts, "unused_nonzero_words", 1);
                }
                if kind == "unknown" {
                    number(&mut counts, "unknown_parameters", 1);
                }
                if kind == "variable_index" {
                    number(&mut counts, "variable_indices_need_live_state", 1);
                }
            }
            for dependency in bound
                .operands
                .iter()
                .filter_map(|p| p.form_dependency.as_ref())
                .chain(bound.comparison_global.iter())
                .chain(bound.subject_reference.iter())
            {
                let status = label(dependency.status)?;
                increment(&mut counts, "form_statuses", &status);
            }
            number(
                &mut counts,
                "source_findings",
                site.source_findings().len() as u64,
            );
            rows.push(serde_json::to_value(site.legacy_row())?);
        }
        if !rows.is_empty() {
            number(&mut counts, "records_with_conditions", 1);
            let row = json!({"key":identity.key,"source_name":identity.source_name,"record_kind":identity.record_kind,
                "record_file_offset":identity.record_file_offset,"record_flags":identity.record_flags,"decoded_bytes":identity.decoded_bytes,
                "decoded_sha256":identity.decoded_sha256,"binding_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&rows)?)),"conditions":rows});
            admit_record(&mut records, row, &mut retained, MAXIMUM_RETAINED_BYTES)?;
        }
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "metadata":metadata,"sources":sources,"executable_sha256":catalogue.source_sha256,"counts":counts,"records":records,
        "index_cache":store.index_cache_report(),"target_kind_acceptance_checked":false,"live_values_resolved":false,"evaluation_ready":false,"retail_parity_accepted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_overflow_stops_before_retention_or_unbounded_iteration() {
        let visits = std::cell::Cell::new(0);
        let input = (0..usize::MAX).inspect(|_| visits.set(visits.get() + 1));
        assert!(
            bounded_candidates(input, 2)
                .unwrap_err()
                .to_string()
                .contains("candidate budget")
        );
        assert_eq!(visits.get(), 3);
        assert_eq!(bounded_candidates(0..2, 2).unwrap(), vec![0, 1]);
        assert!(bounded_candidates(0..1, 0).is_err());
    }

    #[test]
    fn aggregate_admission_counts_escaped_utf8_bytes_and_is_atomic() {
        let row = json!({"source":"é\n\"\\", "conditions":[{"raw":0xffffffff_u32}]});
        let bytes = serde_json::to_vec(&row).unwrap().len();
        let mut records = Vec::new();
        let mut retained = 0;
        assert!(
            admit_record(&mut records, row.clone(), &mut retained, bytes - 1)
                .unwrap_err()
                .to_string()
                .contains("retained-report byte budget")
        );
        assert!(records.is_empty());
        assert_eq!(retained, 0);
        admit_record(&mut records, row.clone(), &mut retained, bytes * 2 - 1).unwrap();
        assert!(admit_record(&mut records, row, &mut retained, bytes * 2 - 1).is_err());
        assert_eq!(records.len(), 1);
        assert_eq!(retained, bytes);
    }
}
