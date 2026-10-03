//! Compare raw table metadata and caller associations. Source consistency issues
//! remain published findings; equality never means those inputs are executable.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}

fn bundle(payload: &[u8]) -> Vec<u8> {
    [
        b"FRUNIT01INFO".as_slice(),
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
    install: &Path,
) -> Result<Value> {
    let rust_path = run.join("bindings-rust.json");
    let bundle_path = run.join("bindings.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("script-bindings")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    // The current original corpus has three stale empty-unit counts. The CLI
    // reports them and exits one; it must not silently declare a clean census.
    let output = run_logged_status(command, &run.join("bindings-rust.log"), 1)?;
    if !String::from_utf8(output.stderr)?.contains("script table associations have issues") {
        return Err("Binding census failed without its expected diagnostic report".into());
    }
    let rust = json_file(&rust_path)?;
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(&bundle_path);
    let output = run_logged(command, &run.join("bindings-native.log"))?;
    let native_path = run.join("bindings-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if native["bundle_sha256"] != digest(&bundle_path)?
        || native["bundle_sha256"] != rust["comparison_bundle"]["sha256"]
        || rust["execution_ready"] != false
        || native["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
        || rust["units_with_issues"] != 3
    {
        return Err("Binding corpus identity, known issues or capability flags differ".into());
    }
    let earlier_path = root.join("local/census-with-scripts.json");
    let earlier = json_file(&earlier_path)?;
    let framing_path = root.join("reports/checkpoint-15-compiled-scripts.json");
    let framing = json_file(&framing_path)?;
    if framing["prior_full_census_sha256"] != digest(&earlier_path)? {
        return Err("Full script inventory differs from the bound checkpoint 15 source".into());
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let native_units = native["units"]
        .as_array()
        .ok_or("Missing independent units")?;
    let plugins = rust["plugins"]
        .as_array()
        .ok_or("Missing binding plugins")?;
    if plugins.len() != 10 {
        return Err("Unexpected official script source count".into());
    }
    let mut cursor = 0;
    let mut totals = BTreeMap::<String, u64>::new();
    let mut summaries = Vec::new();
    let mut known_issues = Vec::new();
    let mut duplicate_sites = Vec::new();
    let mut compiled_with_issues = 0;
    let mut deferred = 0;
    for plugin in plugins {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing source name")?;
        let source = baseline["files"]
            .as_array()
            .ok_or("Missing baseline")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Source absent from baseline")?;
        if source["sha256"] != plugin["source_sha256"]
            || source["bytes"] != plugin["source_bytes"]
            || plugin["focused_scan"] != true
            || plugin["execution_ready"] != false
            || plugin["retail_parity_accepted"] != false
        {
            return Err("Binding source fingerprint or scope differs".into());
        }
        let prior = earlier["plugins"]
            .as_array()
            .ok_or("Missing earlier plugins")?
            .iter()
            .find(|p| p["name"] == name)
            .ok_or("Missing earlier source")?;
        for (current, previous) in [
            ("units", "headers"),
            ("compiled_bodies", "compiled_bodies"),
            ("compiled_bytes", "compiled_bytes"),
            ("source_fields", "source_text_fields"),
            ("form_references", "explicit_form_references"),
            ("variable_references", "local_variable_references"),
        ] {
            if plugin["counts"][current] != prior["scripts"][previous] {
                return Err(format!(
                    "Binding census differs from full inventory: {name}/{current}"
                )
                .into());
            }
        }
        for unit in plugin["units"].as_array().ok_or("Missing authored units")? {
            let reference = native_units.get(cursor).ok_or("Independent unit missing")?;
            for (key, value) in reference.as_object().ok_or("Invalid independent unit")? {
                if unit[key] != *value {
                    return Err(
                        format!("Independent binding differs: {name}/{cursor}/{key}").into(),
                    );
                }
            }
            cursor += 1;
            let issues = unit["issues"].as_array().ok_or("Missing unit issues")?;
            if !issues.is_empty() {
                compiled_with_issues += usize::from(!unit["compiled_bytes"].is_null());
                known_issues.push(json!({"source_name":name,"record_kind":unit["record_kind"],"form_id":unit["form_id"],
                    "record_file_offset":unit["record_file_offset"],"header_decoded_offset":unit["header_decoded_offset"],
                    "metadata_sha256":unit["metadata_sha256"],"declared_references":unit["declared_references"],
                    "references":unit["references"],"compiled_bytes":unit["compiled_bytes"],"issues":issues}));
            }
            if unit["duplicate_variable_indices"]
                .as_u64()
                .ok_or("Missing duplicate count")?
                > 0
            {
                duplicate_sites.push(json!({"source_name":name,"form_id":unit["form_id"],"record_file_offset":unit["record_file_offset"],
                    "header_decoded_offset":unit["header_decoded_offset"],"metadata_sha256":unit["metadata_sha256"],
                    "duplicate_variable_indices":unit["duplicate_variable_indices"],"conflicting_variable_indices":unit["conflicting_variable_indices"]}));
            }
        }
        for (key, count) in plugin["counts"].as_object().ok_or("Missing counts")? {
            if let Some(count) = count.as_u64() {
                *totals.entry(key.clone()).or_default() += count;
            }
        }
        deferred += plugin["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        summaries.push(json!({"source_name":name,"source_sha256":plugin["source_sha256"],"source_bytes":plugin["source_bytes"],
            "counts":plugin["counts"],"units_with_issues":plugin["units_with_issues"],"record_payloads_decoded":plugin["record_payloads_decoded"],
            "record_payloads_deferred":plugin["record_payloads_deferred"]}));
    }
    if cursor != native_units.len()
        || native["unit_count"] != cursor
        || compiled_with_issues != 0
        || totals["reference_calls"] != totals["calls_to_forms"] + totals["calls_to_variables"]
        || framing["reference_calls_compared"] != totals["reference_calls"]
    {
        return Err("Caller coverage, compiled consistency or independent count differs".into());
    }
    let expected = [
        ("DeadMoney.esm", 0x0100923d_u64, 241_u64, 2_u64, 1_u64),
        ("FalloutNV.esm", 0x0015ad0d, 264, 1, 0),
        ("OldWorldBlues.esm", 0x01004902, 458, 1, 0),
    ];
    if known_issues.len() != expected.len() {
        return Err("Known metadata issue set changed".into());
    }
    for (issue, (name, form, offset, declared, actual)) in known_issues.iter().zip(expected) {
        if issue["source_name"] != name
            || issue["form_id"] != form
            || issue["header_decoded_offset"] != offset
            || issue["declared_references"] != declared
            || issue["references"] != actual
            || issue["compiled_bytes"] != Value::Null
            || issue["issues"] != json!(["SCHR reference count differs from ordered table length"])
        {
            return Err(
                "Known stale-header findings differ; review source before publication".into(),
            );
        }
    }
    let strict_path = run.join("strict-bindings.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("script-bindings")
        .arg("--install")
        .arg(install)
        .arg("--output")
        .arg(&strict_path);
    let output = run_logged_status(command, &run.join("strict-bindings.log"), 1)?;
    let error = String::from_utf8(output.stderr)?;
    if !error.contains("0xB0CFF04")
        || !error.contains("strict integrity check failed")
        || strict_path.exists()
    {
        return Err("Strict unrelated LAND integrity guard failed".into());
    }
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&1_u32.to_le_bytes());
    schr[8..12].copy_from_slice(&8_u32.to_le_bytes());
    let caller = [0x1c, 0, 0, 0, 1, 0x10, 0, 0];
    let cases = [
        ("short_magic", b"FRUNIT".to_vec()),
        ("wrong_magic", b"BRUNIT01".to_vec()),
        ("short_record", b"FRUNIT01INFO".to_vec()),
        ("short_schr", bundle(&field(b"SCHR", &[0; 19]))),
        ("orphan_reference", bundle(&field(b"SCRO", &[0; 4]))),
        (
            "duplicate_body",
            bundle(
                &[
                    field(b"SCHR", &[0; 20]),
                    field(b"SCDA", &[]),
                    field(b"SCDA", &[]),
                ]
                .concat(),
            ),
        ),
        ("orphan_xxxx", bundle(&field(b"XXXX", &4_u32.to_le_bytes()))),
        (
            "short_reference",
            bundle(&[field(b"SCHR", &[0; 20]), field(b"SCRO", &[0; 3])].concat()),
        ),
        (
            "unnamed_variable",
            bundle(&[field(b"SCHR", &[0; 20]), field(b"SLSD", &[0; 24])].concat()),
        ),
        (
            "unterminated_name",
            bundle(
                &[
                    field(b"SCHR", &[0; 20]),
                    field(b"SLSD", &[0; 24]),
                    field(b"SCVR", b"open"),
                ]
                .concat(),
            ),
        ),
        (
            "zero_caller",
            bundle(
                &[
                    field(b"SCHR", &schr),
                    field(b"SCDA", &caller),
                    field(b"SCRO", &[1; 4]),
                ]
                .concat(),
            ),
        ),
        (
            "missing_local",
            bundle(
                &[
                    field(b"SCHR", &[0; 20]),
                    field(b"SCRV", &77_u32.to_le_bytes()),
                ]
                .concat(),
            ),
        ),
        (
            "record_budget",
            [
                b"FRUNIT01INFO".as_slice(),
                &1_u32.to_le_bytes(),
                &0_u64.to_le_bytes(),
                &u32::MAX.to_le_bytes(),
            ]
            .concat(),
        ),
    ];
    let mut negatives = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(&path);
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Malformed binding bundle produced a complete report".into());
        }
        negatives.push(name);
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":summaries,"counts":totals,
        "units_compared":cursor,"all_metadata_fields_and_digests_equal":true,"all_top_level_caller_bindings_equal":true,
        "compiled_units_with_issues":compiled_with_issues,"known_metadata_issues":known_issues,
        "duplicate_variable_sites":duplicate_sites,"duplicate_lookup":"preserve all declarations; source-reference first-match lookup; retail loading remains unmeasured",
        "variable_count_semantics":"raw declared value retained; equality to declaration length and maximum index are observations only",
        "record_payloads_deferred":deferred,"strict_unrelated_integrity_failure_retained":true,
        "native_negative_cases_rejected":negatives,"comparison_bundle_sha256":digest(&bundle_path)?,
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "full_inventory_sha256":digest(&earlier_path)?,"framing_receipt_sha256":digest(&framing_path)?,
        "comparison_scope":"independent field/table/caller parsing from Rust-extracted decoded records; extraction/decompression, stable identity conversion, runtime values and execution are not independently compared here",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Three stale empty-unit reference counts","Eight conflicting duplicate variable indices need retail loading observations","Winning embedded-script identity and actual referenced-form existence","Expression/native arguments, jumps, scheduling and effects"]}),
    )
}
