//! Offline compiled-script evidence. Comparisons are deliberately limited to
//! headers; passing them never establishes operand or execution semantics.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

fn sum_counts(destination: &mut BTreeMap<String, u64>, source: &Value) -> Result<()> {
    for (key, value) in source.as_object().ok_or("Missing script counts")? {
        *destination.entry(key.clone()).or_default() += value.as_u64().ok_or("Invalid count")?;
    }
    Ok(())
}

fn oracle_run(root: &Path, run: &Path, oracle: &Path, bundle: &Path, name: &str) -> Result<Value> {
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(bundle);
    let output = run_logged(command, &run.join(format!("{name}.log")))?;
    let path = run.join(format!("{name}.json"));
    write_new(&path, &output.stdout)?;
    json_file(&path)
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let rust_path = run.join("scripts-rust.json");
    let bundle_path = run.join("scripts.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("scripts")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    run_logged(command, &run.join("scripts-rust.log"))?;
    let rust = json_file(&rust_path)?;
    let native = oracle_run(root, run, oracle, &bundle_path, "scripts-native")?;
    if rust["bodies_with_issues"] != 0
        || rust["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
        || native["execution_ready"] != false
        || native["bundle_sha256"] != digest(&bundle_path)?
        || native["bundle_sha256"] != rust["comparison_bundle"]["sha256"]
    {
        return Err("Script census identity, issues or capability flags differ".into());
    }
    let plugins = rust["plugins"].as_array().ok_or("Missing script plugins")?;
    let native_bodies = native["bodies"]
        .as_array()
        .ok_or("Missing native script bodies")?;
    let mut cursor = 0;
    let mut instructions = 0;
    let mut compiled_bytes = 0;
    let mut references = 0;
    let mut events = 0;
    let mut opcodes = BTreeMap::new();
    let mut event_ids = BTreeMap::new();
    let mut commands = BTreeMap::new();
    let mut summaries = Vec::new();
    let mut deferred = 0;
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let earlier = json_file(&root.join("local/census-with-scripts.json"))?;
    let earlier_plugins = earlier["plugins"].as_array().ok_or("Missing full census")?;
    for plugin in plugins {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing source name")?;
        let fingerprint = baseline["files"]
            .as_array()
            .ok_or("Missing baseline files")?
            .iter()
            .find(|row| row["path"] == format!("Data/{name}"))
            .ok_or("Script source absent from baseline")?;
        if fingerprint["sha256"] != plugin["source_sha256"]
            || fingerprint["bytes"] != plugin["source_bytes"]
        {
            return Err("Script source differs from original baseline".into());
        }
        let prior = earlier_plugins
            .iter()
            .find(|row| row["name"] == name)
            .ok_or("Script source absent from earlier census")?;
        for key in ["compiled_bodies", "compiled_bytes"] {
            if prior["scripts"][key] != plugin["counts"][key] {
                return Err(format!("Focused scan differs from full census: {name}/{key}").into());
            }
        }
        if plugin["focused_scan"] != true
            || plugin["bodies_with_issues"] != 0
            || plugin["execution_ready"] != false
            || plugin["retail_parity_accepted"] != false
            || !plugin["counts"]["unknown_opcodes"]
                .as_object()
                .ok_or("Missing unknown opcode counts")?
                .is_empty()
        {
            return Err("Focused script scope or unknown headers require review".into());
        }
        let mut local_opcodes = BTreeMap::new();
        let mut local_events = BTreeMap::new();
        for body in plugin["bodies"].as_array().ok_or("Missing script bodies")? {
            let oracle_body = native_bodies
                .get(cursor)
                .ok_or("Native comparison is missing a script")?;
            for key in [
                "bytes",
                "sha256",
                "framing_sha256",
                "instructions",
                "reference_calls",
                "event_blocks",
            ] {
                if body[key].is_null() || body[key] != oracle_body[key] {
                    return Err(format!(
                        "Script header comparison differs: {name}, body {cursor}, {key}"
                    )
                    .into());
                }
            }
            cursor += 1;
            instructions += body["instructions"]
                .as_u64()
                .ok_or("Missing instruction count")?;
            compiled_bytes += body["bytes"].as_u64().ok_or("Missing script bytes")?;
            references += body["reference_calls"]
                .as_u64()
                .ok_or("Missing reference calls")?;
            events += body["event_blocks"].as_u64().ok_or("Missing events")?;
            sum_counts(&mut local_opcodes, &oracle_body["instruction_opcodes"])?;
            sum_counts(&mut local_events, &oracle_body["event_ids"])?;
        }
        if serde_json::to_value(&local_opcodes)? != plugin["counts"]["instruction_opcodes"]
            || serde_json::to_value(&local_events)? != plugin["counts"]["event_ids"]
        {
            return Err("Independent aggregate instruction/event counts differ".into());
        }
        sum_counts(&mut opcodes, &plugin["counts"]["instruction_opcodes"])?;
        sum_counts(&mut event_ids, &plugin["counts"]["event_ids"])?;
        sum_counts(
            &mut commands,
            &plugin["counts"]["top_level_native_commands"],
        )?;
        deferred += plugin["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        summaries.push(
            json!({"source_name":name,"source_sha256":plugin["source_sha256"],
            "source_bytes":plugin["source_bytes"],"counts":plugin["counts"],
            "record_payloads_decoded":plugin["record_payloads_decoded"],
            "record_payloads_deferred":plugin["record_payloads_deferred"]}),
        );
    }
    if cursor != native_bodies.len() || native["compiled_bodies"] != cursor || plugins.len() != 10 {
        return Err("Independent body count or official source set differs".into());
    }
    // The full strict scan must still expose the known LAND defect. Focused
    // inspection is not permission to recover or validate that unrelated body.
    let strict_report = run.join("strict-scripts.json");
    let mut strict = Command::new(cli);
    strict
        .current_dir(root)
        .arg("scripts")
        .arg("--install")
        .arg(install)
        .arg("--output")
        .arg(&strict_report);
    let output = run_logged_status(strict, &run.join("strict-scripts.log"), 1)?;
    let error = String::from_utf8(output.stderr)?;
    if !error.contains("0xB0CFF04")
        || !error.contains("strict integrity check failed")
        || strict_report.exists()
    {
        return Err("Strict corpus guard failed or published a misleading report".into());
    }
    let negative_inputs: &[(&str, &[u8])] = &[
        ("short_magic", b"FROBS"),
        ("wrong_magic", b"BROBS001"),
        ("partial_length", b"FROBS001\x01"),
        ("short_body", b"FROBS001\x04\0\0\0\x1e\0"),
        ("partial_header", b"FROBS001\x02\0\0\0\x1e\0"),
        ("short_reference", b"FROBS001\x04\0\0\0\x1c\0\x01\0"),
        ("operand_extent", b"FROBS001\x04\0\0\0\x01\x10\xff\xff"),
        ("short_begin", b"FROBS001\x04\0\0\0\x10\0\0\0"),
        ("body_budget", b"FROBS001\xff\xff\xff\xff"),
    ];
    for (name, bytes) in negative_inputs {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, bytes)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(&path);
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid comparison bundle produced a report".into());
        }
    }
    Ok(json!({
        "schema_version":1,"profile":"nv-original","plugins":summaries,
        "compiled_bodies_compared":cursor,"compiled_bytes_compared":compiled_bytes,
        "instruction_headers_compared":instructions,"reference_calls_compared":references,
        "event_headers_compared":events,"top_level_native_command_ids":commands.len(),
        "top_level_native_command_occurrences":commands,"instruction_opcodes":opcodes,"event_ids":event_ids,
        "record_payloads_deferred":deferred,"all_header_tuples_equal":true,
        "body_counts_equal_prior_full_diagnostic_census":true,
        "prior_full_census_sha256":digest(&root.join("local/census-with-scripts.json"))?,
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&run.join("scripts-native.json"))?,
        "comparison_bundle_sha256":digest(&bundle_path)?,"oracle_binary_sha256":digest(oracle)?,
        "negative_bundle_cases":negative_inputs.len(),"strict_land_failure_preserved":true,
        "comparison_scope":"original independent C++ header framing and CNG hashing of Rust-extracted SCDA; no independent plugin decompression or field extraction claim",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["expression calls and native argument decoding","reference/local-variable binding","jump execution origin and targets","event names/scheduling and native behavior","winning embedded scripts and retail acceptance"],
    }))
}
