//! Load winning condition fields and their immutable dependencies. Record
//! ownership stays attached; this scan does not merge unrelated AND/OR lists.
use super::{Result, command_catalogue, inspection_input::Order};
use fallout_data::{
    condition, condition_census,
    condition_operands::{self, Parameter, Signature, Signatures},
    plugin, record_metadata,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

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
    let candidates = store
        .winning_definitions()
        .filter(|(_, location)| {
            condition_census::condition_record(store.definition(*location).header.kind)
        })
        .map(|(key, location)| (key.clone(), location))
        .collect::<Vec<_>>();
    if candidates.len() > 262_144 {
        return Err("condition candidate budget exceeded".into());
    }
    let mut counts = json!({"candidate_records":candidates.len(),"deleted_candidate_records":0,"decoded_candidate_bytes":0,
        "records_with_conditions":0,"conditions":0,"source_findings":0,"unused_nonzero_words":0,"unknown_parameters":0,
        "variable_indices_need_live_state":0,"signature_statuses":{},"parameter_kinds":{},"parameter_domains":{},"subjects":{},"form_statuses":{}});
    let mut records = Vec::new();
    for (key, location) in candidates {
        if store.definition(location).header.flags & plugin::DELETED != 0 {
            number(&mut counts, "deleted_candidate_records", 1);
            continue;
        }
        let record = store.read(location)?;
        let source_name = store.source_name(location).to_string();
        number(
            &mut counts,
            "decoded_candidate_bytes",
            record.payload.len() as u64,
        );
        if counts["decoded_candidate_bytes"]
            .as_u64()
            .ok_or("Missing byte count")?
            > 512 * 1024 * 1024
        {
            return Err("condition decoded candidate budget exceeded".into());
        }
        let mut rows = Vec::new();
        let mut previous = None;
        let mut fields = 0_usize;
        plugin::visit_subrecords(&record, &source_name, |field| {
            fields += 1;
            if fields > 1_048_576 {
                return Err(fallout_data::Error::Unsupported(
                    "condition field budget exceeded".into(),
                ));
            }
            if field.kind == *b"CTDA" {
                if counts["conditions"].as_u64().expect("condition count") >= 1_000_000 {
                    return Err(fallout_data::Error::Unsupported(
                        "condition row budget exceeded".into(),
                    ));
                }
                let condition =
                    condition::decode(field.data).map_err(|error| fallout_data::Error::Format {
                        source_name: source_name.clone(),
                        offset: record.header.offset,
                        reason: format!("CTDA at decoded offset {}: {error}", field.payload_offset),
                    })?;
                let bound = condition_operands::bind(
                    &store,
                    location,
                    &condition,
                    signatures.get(&condition.function_id),
                )?;
                let binding = serde_json::to_value(&bound)
                    .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?;
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
                    let status = label(dependency.status)
                        .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?;
                    increment(&mut counts, "form_statuses", &status);
                }
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
                if matches!(bound.subject, condition_operands::Subject::Unknown { .. }) {
                    findings.push("unknown_subject_selector");
                }
                number(&mut counts, "source_findings", findings.len() as u64);
                rows.push(json!({"field_decoded_offset":field.payload_offset,"bytes":field.data.len(),"sha256":format!("{:x}",Sha256::digest(field.data)),
                    "preceding_field_kind":previous.as_ref().map(|(kind,_)|kind),"preceding_field_decoded_offset":previous.as_ref().map(|(_,offset)|offset),
                    "flags":condition.flags,"flag_padding":condition.flag_padding,"function_id":condition.function_id,"function_padding":condition.function_padding,
                    "comparison_operator":condition.comparison_operator(),"comparison_value":condition.comparison_value(),"or_flag":condition.or_flag(),
                    "parameter_words":condition.parameter_words,"run_on_word":condition.run_on_word,"reference_word":condition.reference_word,
                    "binding":binding,"source_findings":findings}));
            }
            previous = Some((plugin::signature(field.kind), field.payload_offset));
            Ok(())
        })?;
        if !rows.is_empty() {
            number(&mut counts, "records_with_conditions", 1);
            records.push(json!({"key":key,"source_name":source_name,"record_kind":plugin::signature(record.header.kind),
                "record_file_offset":record.header.offset,"record_flags":record.header.flags,"decoded_bytes":record.payload.len(),
                "decoded_sha256":format!("{:x}",Sha256::digest(&record.payload)),"binding_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&rows)?)),"conditions":rows}));
        }
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "metadata":metadata,"sources":sources,"executable_sha256":catalogue.source_sha256,"counts":counts,"records":records,
        "index_cache":store.index_cache_report(),"target_kind_acceptance_checked":false,"live_values_resolved":false,"evaluation_ready":false,"retail_parity_accepted":false}),
    )
}
