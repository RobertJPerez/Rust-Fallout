//! Every CTDA field is compared independently, including short layouts and
//! uninterpreted source findings. Metadata equality is not condition evaluation.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn add_counts(total: &mut Value, source: &Value) -> Result<()> {
    for (key, value) in source.as_object().ok_or("Missing condition counts")? {
        if let Some(value) = value.as_u64() {
            total[key] = (total[key].as_u64().unwrap_or(0) + value).into();
        } else {
            if total[key].is_null() {
                total[key] = json!({});
            }
            for (id, value) in value.as_object().ok_or("Invalid condition count map")? {
                total[key][id] = (total[key][id].as_u64().unwrap_or(0)
                    + value.as_u64().ok_or("Invalid count")?)
                .into();
            }
        }
    }
    Ok(())
}
fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn fixture(payload: &[u8]) -> Vec<u8> {
    [
        b"FRCOND01INFO".as_slice(),
        &1_u32.to_le_bytes(),
        &0_u64.to_le_bytes(),
        &(payload.len() as u32).to_le_bytes(),
        payload,
    ]
    .concat()
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    command_oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let catalogue_directory = run.join("catalogue-regression");
    std::fs::create_dir(&catalogue_directory)?;
    let catalogue =
        super::catalogue_evidence::run(root, &catalogue_directory, cli, command_oracle, install)?;
    let rust_path = run.join("conditions-rust.json");
    let bundle_path = run.join("conditions.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("conditions")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    run_logged(command, &run.join("conditions-rust.log"))?;
    let rust = json_file(&rust_path)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(&bundle_path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("conditions-native.log"))?;
    let native_path = run.join("conditions-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if rust["comparison_bundle"]["sha256"] != digest(&bundle_path)?
        || native["bundle_sha256"] != digest(&bundle_path)?
        || rust["executable_source_sha256"] != native["executable_source_sha256"]
        || rust["evaluation_ready"] != false
        || native["evaluation_ready"] != false
        || rust["retail_parity_accepted"] != false
        || !rust["unresolved_condition_function_ids"]
            .as_array()
            .ok_or("Missing unresolved functions")?
            .is_empty()
    {
        return Err("Condition identity, function coverage or capability scope differs".into());
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let inventory_path = root.join("local/census-with-scripts.json");
    let inventory = json_file(&inventory_path)?;
    let original_receipt = json_file(&root.join("reports/checkpoint-15-compiled-scripts.json"))?;
    if original_receipt["prior_full_census_sha256"] != digest(&inventory_path)? {
        return Err("Original condition inventory no longer matches its source receipt".into());
    }
    let rows = native["rows"]
        .as_array()
        .ok_or("Missing independent conditions")?;
    let plugins = rust["plugins"]
        .as_array()
        .ok_or("Missing condition sources")?;
    if plugins.len() != 10 {
        return Err("Unexpected official condition source count".into());
    }
    let mut cursor = 0;
    let mut totals = json!({});
    let mut summaries = Vec::new();
    let mut findings = Vec::new();
    let mut deferred = 0;
    for plugin in plugins {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing condition source name")?;
        let source = baseline["files"]
            .as_array()
            .ok_or("Missing baseline")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Condition source absent from baseline")?;
        let prior = inventory["plugins"]
            .as_array()
            .ok_or("Missing original inventory sources")?
            .iter()
            .find(|plugin| plugin["name"] == name)
            .ok_or("Missing original inventory source")?;
        if plugin["source_sha256"] != source["sha256"]
            || plugin["source_bytes"] != source["bytes"]
            || plugin["counts"]["lengths"] != prior["scripts"]["condition_lengths"]
            || plugin["focused_scan"] != true
            || plugin["evaluation_ready"] != false
            || plugin["retail_parity_accepted"] != false
        {
            return Err("Condition source/layout counts differ from original inventory".into());
        }
        let prior_functions = prior["scripts"]["condition_functions"]
            .as_object()
            .ok_or("Missing original condition functions")?;
        let functions = plugin["counts"]["functions"]
            .as_object()
            .ok_or("Missing condition functions")?;
        if functions.len() != prior_functions.len()
            || functions.iter().any(|(id, count)| {
                prior_functions.get(id).map(|usage| &usage["occurrences"]) != Some(count)
            })
        {
            return Err("Condition function coverage differs from original full inventory".into());
        }
        for row in plugin["rows"].as_array().ok_or("Missing condition rows")? {
            if Some(row) != rows.get(cursor) {
                return Err(format!("Independent condition field differs: {name}/{cursor}").into());
            }
            let flags = row["flags"].as_u64().ok_or("Missing condition flags")?;
            let unknown_subject = row["run_on_domain"] == "subject_selection"
                && row["run_on_word"].as_u64().is_some_and(|word| word > 4);
            if flags & 0x1a != 0
                || flags >> 5 > 5
                || row["flag_padding"] != json!([0, 0, 0])
                || row["function_padding"] != json!([0, 0])
                || unknown_subject
            {
                findings.push(json!({"source_name":name,"condition":row,"scope":"source fields retained; handling/evaluation unverified"}));
            }
            cursor += 1;
        }
        add_counts(&mut totals, &plugin["counts"])?;
        deferred += plugin["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        summaries.push(json!({"source_name":name,"source_sha256":source["sha256"],"source_bytes":source["bytes"],
            "counts":plugin["counts"],"record_payloads_decoded":plugin["record_payloads_decoded"],"record_payloads_deferred":plugin["record_payloads_deferred"]}));
    }
    if cursor != rows.len() || totals != native["counts"] {
        return Err("Independent aggregate condition counts differ".into());
    }
    let strict_path = run.join("strict-conditions.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("conditions")
        .arg("--install")
        .arg(install)
        .arg("--output")
        .arg(&strict_path);
    let output = run_logged_status(command, &run.join("strict-conditions.log"), 1)?;
    let error = String::from_utf8(output.stderr)?;
    if !error.contains("0xB0CFF04")
        || !error.contains("strict integrity check failed")
        || strict_path.exists()
    {
        return Err("Strict unrelated LAND integrity guard failed".into());
    }
    let cases = [
        ("short_magic", b"FRCOND".to_vec()),
        ("wrong_magic", b"BRCOND01".to_vec()),
        ("short_record", b"FRCOND01INFO".to_vec()),
        ("short_field", fixture(b"CTDA")),
        ("short_ctda", fixture(&field(b"CTDA", &[0; 19]))),
        ("partial_run_on", fixture(&field(b"CTDA", &[0; 21]))),
        ("partial_reference", fixture(&field(b"CTDA", &[0; 25]))),
        ("surplus_ctda", fixture(&field(b"CTDA", &[0; 29]))),
        (
            "orphan_extended",
            fixture(&field(b"XXXX", &20_u32.to_le_bytes())),
        ),
        ("empty_record", fixture(&field(b"EDID", b"empty\0"))),
    ];
    let mut negatives = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(&path)
            .arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid condition bundle produced a complete report".into());
        }
        negatives.push(name);
    }
    // All three source layouts occur in the corpus. This separate fixture adds
    // nonfinite/unknown bit patterns without pretending they were authored there.
    let mut data = [0; 28];
    data[0] = 0xe4;
    data[1..4].copy_from_slice(&[1, 2, 3]);
    data[4..8].copy_from_slice(&0x7fc0_1234_u32.to_le_bytes());
    data[8..10].copy_from_slice(&285_u16.to_le_bytes());
    data[10..12].copy_from_slice(&[4, 5]);
    data[20..24].copy_from_slice(&20_u32.to_le_bytes());
    let bytes = fixture(
        &[
            field(b"CTDA", &data[..20]),
            field(b"CTDA", &data[..24]),
            field(b"CTDA", &data),
        ]
        .concat(),
    );
    let path = run.join("source-layout-fixture.bin");
    write_new(&path, &bytes)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(&path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("source-layout-fixture.log"))?;
    let independent: Value = serde_json::from_slice(&output.stdout)?;
    for (index, size) in [20, 24, 28].into_iter().enumerate() {
        let decoded = fallout_data::condition::decode(&data[..size])?;
        let row = &independent["rows"][index];
        if row["comparison_value"] != serde_json::to_value(decoded.comparison_value())?
            || row["comparison_operator"] != serde_json::to_value(decoded.comparison_operator())?
            || row["run_on_word"] != serde_json::to_value(decoded.run_on_word)?
            || row["reference_word"] != serde_json::to_value(decoded.reference_word)?
            || row["flag_padding"] != json!([1, 2, 3])
            || row["function_padding"] != json!([4, 5])
        {
            return Err("Independent source-layout fixture differs".into());
        }
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":summaries,"counts":totals,"condition_fields_compared":cursor,
        "all_condition_fields_and_source_extents_equal":true,"prior_full_inventory_coverage_equal":true,"source_findings":findings,
        "condition_function_links":rust["condition_function_links"],"condition_function_ids_bound":rust["condition_function_links"].as_array().ok_or("Missing links")?.len(),
        "fresh_catalogue_regression":{"all_descriptor_fields_equal":catalogue["all_descriptor_fields_equal"],
            "changed_executable_rejected":catalogue["changed_executable_rejected_by_both_readers"],"native_report_sha256":catalogue["native_report_sha256"],"rust_report_sha256":catalogue["rust_report_sha256"]},
        "record_payloads_deferred":deferred,"strict_unrelated_integrity_failure_retained":true,"native_negative_cases_rejected":negatives,
        "source_layout_fixture_equal":true,"executable_source_sha256":rust["executable_source_sha256"],
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"comparison_bundle_sha256":digest(&bundle_path)?,
        "original_inventory_sha256":digest(&inventory_path)?,"comparison_scope":"independent CTDA fields, exact unions, optional words, preceding-field provenance and counts from Rust-extracted decoded records; no grouping/migration or evaluation",
        "evaluation_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Unverified subject selector, low flag and nonzero padding source findings","Legacy short-layout loading/default/migration behavior",
            "Function-specific parameter meanings and reference existence","Condition ownership/grouping, query effects and evaluation order",
            "Numerical comparisons, original native errors and retail behavior"]}),
    )
}
