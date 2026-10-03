//! Source ownership proof, including original orphaned entries. No quest or
//! dialogue behavior is accepted by a matching structural receipt.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use fallout_data::{narrative, narrative_census, plugin};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

fn add_counts(total: &mut Value, source: &Value) -> Result<()> {
    if let Some(value) = source.as_u64() {
        *total = (total.as_u64().unwrap_or(0) + value).into();
    } else {
        if total.is_null() {
            *total = json!({});
        }
        for (key, value) in source.as_object().ok_or("Invalid narrative count map")? {
            add_counts(&mut total[key], value)?;
        }
    }
    Ok(())
}
fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn fixture_record(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &1_u32.to_le_bytes(),
        &0_u32.to_le_bytes(),
        &24_u64.to_le_bytes(),
        &(payload.len() as u32).to_le_bytes(),
        payload,
    ]
    .concat()
}
fn fixture(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [b"FRNARR01".as_slice(), &fixture_record(kind, payload)].concat()
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let rust_path = run.join("narrative-rust.json");
    let bundle_path = run.join("narrative.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("narrative")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    // Original orphaned connections and short headers are retained in a complete
    // report. An unsuccessful status keeps these source findings visible.
    run_logged_status(command, &run.join("narrative-rust.log"), 1)?;
    let rust = json_file(&rust_path)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(&bundle_path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("narrative-native.log"))?;
    let native_path = run.join("narrative-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if rust["comparison_bundle"]["sha256"] != digest(&bundle_path)?
        || native["bundle_sha256"] != digest(&bundle_path)?
        || rust["execution_ready"] != false
        || native["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Narrative source/evaluation boundary differs".into());
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let inventory_path = root.join("local/census-with-scripts.json");
    let inventory = json_file(&inventory_path)?;
    if digest(&inventory_path)?
        != json_file(&root.join("reports/checkpoint-15-compiled-scripts.json"))?["prior_full_census_sha256"]
    {
        return Err("Original complete inventory changed".into());
    }
    let tables_path = root.join("local/bindings-17-verified/bindings-rust.json");
    let tables = json_file(&tables_path)?;
    if digest(&tables_path)?
        != json_file(&root.join("reports/checkpoint-17-script-bindings.json"))?["rust_report_sha256"]
    {
        return Err("Original script table proof changed".into());
    }
    let prior_conditions = json_file(&root.join("reports/checkpoint-21-condition-fields.json"))?;
    let rows = native["rows"]
        .as_array()
        .ok_or("Missing native narrative rows")?;
    let mut cursor = 0;
    let mut totals = json!({});
    let mut findings = Vec::new();
    let mut plugins = Vec::new();
    let mut scripts_compared = 0;
    let mut deferred = 0;
    for report in rust["plugins"]
        .as_array()
        .ok_or("Missing narrative plugins")?
    {
        let name = report["source_name"]
            .as_str()
            .ok_or("Missing narrative source")?;
        let source = baseline["files"]
            .as_array()
            .ok_or("Missing baseline files")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Narrative source absent from baseline")?;
        if report["source_sha256"] != source["sha256"] || report["source_bytes"] != source["bytes"]
        {
            return Err(format!("Narrative source differs: {name}").into());
        }
        let original = inventory["plugins"]
            .as_array()
            .ok_or("Missing original plugins")?
            .iter()
            .find(|plugin| plugin["name"] == name)
            .ok_or("Original narrative plugin absent")?;
        let condition = prior_conditions["plugins"]
            .as_array()
            .ok_or("Missing prior condition plugins")?
            .iter()
            .find(|plugin| plugin["source_name"] == name)
            .ok_or("Prior condition plugin absent")?;
        if condition["source_sha256"] != source["sha256"] {
            return Err("Prior condition source differs".into());
        }
        let count = |value: &Value| value.as_u64().unwrap_or(0);
        for kind in ["QUST", "INFO", "DIAL"] {
            if count(&report["counts"]["record_kinds"][kind])
                != count(&original["record_kinds"][kind]["occurrences"])
            {
                return Err("Narrative record coverage differs from complete inventory".into());
            }
        }
        if count(&report["counts"]["conditions"])
            != count(&condition["counts"]["record_kinds"]["QUST"])
                + count(&condition["counts"]["record_kinds"]["INFO"])
        {
            return Err("Narrative condition coverage differs".into());
        }
        let table_plugin = tables["plugins"]
            .as_array()
            .ok_or("Missing script table plugins")?
            .iter()
            .find(|plugin| plugin["source_name"] == name)
            .ok_or("Script table plugin absent")?;
        if table_plugin["source_sha256"] != source["sha256"] {
            return Err("Prior script table source differs".into());
        }
        let mut table_rows = BTreeMap::new();
        for unit in table_plugin["units"]
            .as_array()
            .ok_or("Missing prior units")?
        {
            if matches!(unit["record_kind"].as_str(), Some("INFO" | "QUST")) {
                let key = (
                    unit["record_file_offset"]
                        .as_u64()
                        .ok_or("Missing record offset")?,
                    unit["header_decoded_offset"]
                        .as_u64()
                        .ok_or("Missing unit offset")?,
                );
                if table_rows.insert(key, unit).is_some() {
                    return Err("Duplicate prior narrative unit identity".into());
                }
            }
        }
        let mut plugin_scripts = 0;
        for row in report["rows"].as_array().ok_or("Missing narrative rows")? {
            if Some(row) != rows.get(cursor) {
                return Err(format!("Independent narrative differs: {name}/{cursor}").into());
            }
            for script in row["scripts"]
                .as_array()
                .ok_or("Missing narrative scripts")?
            {
                let key = (
                    row["record_file_offset"]
                        .as_u64()
                        .ok_or("Missing record offset")?,
                    script["header_decoded_offset"]
                        .as_u64()
                        .ok_or("Missing script offset")?,
                );
                let prior = table_rows
                    .remove(&key)
                    .ok_or("Narrative script absent from prior tables")?;
                if prior["metadata_sha256"] != script["metadata_sha256"]
                    || prior["compiled_bytes"] != script["compiled_bytes"]
                {
                    return Err("Narrative script metadata differs from prior proof".into());
                }
                plugin_scripts += 1;
            }
            if !row["findings"]
                .as_array()
                .ok_or("Missing source findings")?
                .is_empty()
            {
                findings.push(json!({"source_name":name,"record_kind":row["record_kind"],"form_id":row["form_id"],
                    "record_file_offset":row["record_file_offset"],"decoded_sha256":row["decoded_sha256"],"findings":row["findings"]}));
            }
            cursor += 1;
        }
        if !table_rows.is_empty() {
            return Err("Prior narrative scripts omitted".into());
        }
        scripts_compared += plugin_scripts;
        add_counts(&mut totals, &report["counts"])?;
        deferred += report["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        plugins.push(json!({"source_name":name,"source_sha256":source["sha256"],"source_bytes":source["bytes"],
            "counts":report["counts"],"record_payloads_decoded":report["record_payloads_decoded"],"record_payloads_deferred":report["record_payloads_deferred"]}));
    }
    if cursor != rows.len() || totals != native["counts"] || totals["findings"] != rust["findings"]
    {
        return Err("Narrative counts or record extents differ".into());
    }
    let strict_path = run.join("strict-narrative.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("narrative")
        .arg("--install")
        .arg(install)
        .arg("--output")
        .arg(&strict_path);
    let output = run_logged_status(command, &run.join("strict-narrative.log"), 1)?;
    let error = String::from_utf8(output.stderr)?;
    if !error.contains("0xB0CFF04")
        || !error.contains("strict integrity check failed")
        || strict_path.exists()
    {
        return Err("Narrative strict unrelated integrity guard failed".into());
    }
    let bad = [
        ("short_magic", b"FRNARR".to_vec()),
        ("wrong_magic", b"BRNARR01".to_vec()),
        ("short_record", b"FRNARR01QUST".to_vec()),
        ("wrong_kind", fixture(b"SCPT", &[])),
        ("short_field", fixture(b"QUST", b"DATA")),
        (
            "partial_quest_header",
            fixture(b"QUST", &field(b"DATA", &[0; 3])),
        ),
        (
            "partial_condition",
            fixture(b"INFO", &field(b"CTDA", &[0; 21])),
        ),
        (
            "partial_response",
            fixture(b"INFO", &field(b"TRDT", &[0; 21])),
        ),
        ("nonempty_next", fixture(b"INFO", &field(b"NEXT", &[0]))),
        (
            "orphan_script_field",
            fixture(b"INFO", &field(b"SCRO", &[0; 4])),
        ),
        (
            "orphan_extended",
            fixture(b"QUST", &field(b"XXXX", &8_u32.to_le_bytes())),
        ),
        (
            "duplicate_compiled",
            fixture(
                b"INFO",
                &[
                    field(b"SCHR", &[0; 20]),
                    field(b"SCDA", &[]),
                    field(b"SCDA", &[]),
                ]
                .concat(),
            ),
        ),
    ];
    let mut negatives = Vec::new();
    for (name, bytes) in bad {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(&path)
            .arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid narrative bundle produced a complete report".into());
        }
        negatives.push(name);
    }
    let fixture_result = compare_fixture(root, run, oracle, install)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":plugins,"counts":totals,
        "records_compared":cursor,"script_units_compared_against_prior_tables":scripts_compared,
        "all_typed_fields_source_extents_and_ownership_equal":true,"original_inventory_coverage_equal":true,
        "original_condition_coverage_equal":true,"source_findings":findings,"record_payloads_deferred":deferred,
        "source_layout_and_orphan_fixture_equal":fixture_result,"native_negative_cases_rejected":negatives,
        "strict_unrelated_integrity_failure_retained":true,"executable_source_sha256":native["executable_source_sha256"],
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"comparison_bundle_sha256":digest(&bundle_path)?,
        "prior_inventory_sha256":digest(&inventory_path)?,"prior_table_report_sha256":digest(&tables_path)?,
        "comparison_scope":"independent QUST/INFO/DIAL typed words, ordered section/source identities, condition and script owners from Rust-extracted decoded records",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Original loading of two short quest headers and eighteen orphaned shared-info entries",
            "Dialogue parent GRUP identity and canonical winning record/script identity","Loaded form existence and condition subject resolution",
            "Query implementation, condition evaluation, quest/stage/objective state and dialogue selection",
            "Retail script timing, native effects and actual gameplay acceptance"]}),
    )
}

fn compare_fixture(root: &Path, run: &Path, oracle: &Path, install: &Path) -> Result<bool> {
    let script = field(b"SCHR", &[0; 20]);
    let mut response = [0; 24];
    response[4..8].copy_from_slice(&(-7_i32).to_le_bytes());
    response[8..12].copy_from_slice(&[1, 2, 3, 4]);
    response[12..16].copy_from_slice(&[5, 6, 7, 8]);
    response[16..20].copy_from_slice(&0x123_u32.to_le_bytes());
    response[20..24].copy_from_slice(&[9, 10, 11, 12]);
    let records = [
        (*b"QUST", field(b"DATA", &[1, 2, 3, 4])),
        (
            *b"QUST",
            [
                field(b"DATA", &[1, 2]),
                field(b"INDX", &(-3_i16).to_le_bytes()),
                field(b"QSDT", &[7]),
                field(b"CTDA", &[0; 20]),
                script.clone(),
                field(b"QOBJ", &(-7_i32).to_le_bytes()),
                field(b"QSTA", &[1, 2, 3, 4, 5, 6, 7, 8]),
                field(b"CTDA", &[0; 24]),
            ]
            .concat(),
        ),
        (
            *b"INFO",
            [
                field(b"DATA", &[1, 2, 3]),
                field(b"TRDT", &response[..20]),
                field(b"NAM1", b"fixture\0"),
                field(b"TRDT", &response),
                field(b"CTDA", &[0; 28]),
                script.clone(),
                field(b"NEXT", &[]),
                script,
            ]
            .concat(),
        ),
        (
            *b"DIAL",
            [
                field(b"DATA", &[1]),
                field(b"INFC", &[0; 4]),
                field(b"INFX", &(-1_i32).to_le_bytes()),
                field(b"QSTI", &2_u32.to_le_bytes()),
                field(b"INFC", &3_u32.to_le_bytes()),
                field(b"INFX", &7_i32.to_le_bytes()),
                field(b"PNAM", &0x7fc0_1234_u32.to_le_bytes()),
                field(b"ZZZZ", &[9, 8, 7]),
            ]
            .concat(),
        ),
    ];
    let mut bundle = b"FRNARR01".to_vec();
    let mut expected = Vec::new();
    let mut counts = narrative_census::Counts::default();
    for (kind, payload) in records {
        bundle.extend(fixture_record(&kind, &payload));
        let record = plugin::Record {
            header: plugin::RecordHeader {
                kind,
                offset: 24,
                stored_size: payload.len() as u32,
                flags: 0,
                form_id: 1,
                revision: [0; 4],
                version: 15,
                trailing_bytes: [0; 2],
            },
            payload,
            integrity_issue: None,
        };
        let document = narrative::decode(&record, "Fixture.esm", narrative::Limits::default())?;
        expected.push(narrative_census::summarize(&record, document, &mut counts));
    }
    let path = run.join("source-layout-fixture.bin");
    write_new(&path, &bundle)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(&path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("source-layout-fixture.log"))?;
    let native: Value = serde_json::from_slice(&output.stdout)?;
    if native["rows"] != serde_json::to_value(expected)?
        || native["counts"] != serde_json::to_value(counts)?
    {
        return Err("Independent narrative fixture differs".into());
    }
    Ok(true)
}
