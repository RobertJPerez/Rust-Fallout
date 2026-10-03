//! Descriptor evidence uses two direct executable readers. Usage links remain
//! metadata: the existence of an original handler does not implement its behavior.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let rust_path = run.join("catalogue-rust.json");
    let mut rust_command = Command::new(cli);
    rust_command
        .current_dir(root)
        .arg("command-catalogue")
        .arg("--install")
        .arg(install)
        .arg("--output")
        .arg(&rust_path);
    run_logged(rust_command, &run.join("catalogue-rust.log"))?;
    let rust = json_file(&rust_path)?;
    let mut native_command = Command::new(oracle);
    native_command
        .current_dir(root)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(native_command, &run.join("catalogue-native.log"))?;
    let native_path = run.join("catalogue-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    for (key, expected) in native.as_object().ok_or("Missing native catalogue")? {
        if rust[key] != *expected {
            return Err(format!("Independent descriptor comparison differs: {key}").into());
        }
    }
    if rust["execution_ready"] != false || rust["retail_parity_accepted"] != false {
        return Err("Descriptor metadata cannot accept execution/parity".into());
    }
    let commands = rust["script_commands"]
        .as_array()
        .ok_or("Missing script descriptors")?;
    let events = rust["event_blocks"]
        .as_array()
        .ok_or("Missing event descriptors")?;
    let statements = rust["statements"]
        .as_array()
        .ok_or("Missing statement descriptors")?;
    if commands.len() != 640 || events.len() != 38 || statements.len() != 16 {
        return Err("Pinned executable descriptor counts differ".into());
    }
    let parameters: usize = [commands, events, statements]
        .into_iter()
        .flat_map(|rows| rows.iter())
        .map(|row| {
            row["parameters"]
                .as_array()
                .map(Vec::len)
                .ok_or("Missing parameter descriptors")
        })
        .collect::<std::result::Result<Vec<_>, _>>()?
        .iter()
        .sum();
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let fingerprint = baseline["files"]
        .as_array()
        .ok_or("Missing baseline files")?
        .iter()
        .find(|row| row["path"] == "FalloutNV.exe")
        .ok_or("Executable absent from baseline")?;
    if fingerprint["sha256"] != rust["source_sha256"]
        || fingerprint["bytes"] != rust["source_bytes"]
    {
        return Err("Command source differs from original installation".into());
    }
    let framing_path = root.join("reports/checkpoint-15-compiled-scripts.json");
    let framing = json_file(&framing_path)?;
    let mut used_commands = Vec::new();
    for (id, occurrences) in framing["top_level_native_command_occurrences"]
        .as_object()
        .ok_or("Missing command usages")?
    {
        let id: u32 = id.parse()?;
        let row = commands
            .iter()
            .find(|row| row["id"] == id)
            .ok_or("Authored command ID absent from descriptor table")?;
        used_commands.push(json!({"command_id":id,"occurrences":occurrences,"descriptor":row}));
    }
    let mut used_events = Vec::new();
    for (id, occurrences) in framing["event_ids"]
        .as_object()
        .ok_or("Missing event usages")?
    {
        let id: u32 = id.parse()?;
        let row = events
            .iter()
            .find(|row| row["id"] == id)
            .ok_or("Authored event ID absent from descriptor table")?;
        used_events.push(json!({"event_id":id,"occurrences":occurrences,"descriptor":row}));
    }
    // The condition inventory has its own identifier space. This profile's
    // source declares a 0x1000 script-command base; bind it explicitly and keep
    // both numbers in the receipt. No condition is evaluated by this join.
    let condition_path = root.join("local/census-with-scripts.json");
    if digest(&condition_path)? != framing["prior_full_census_sha256"] {
        return Err("Condition census differs from the input bound by checkpoint 15".into());
    }
    let condition_census = json_file(&condition_path)?;
    let mut condition_counts = BTreeMap::<u32, u64>::new();
    for plugin in condition_census["plugins"]
        .as_array()
        .ok_or("Missing condition census plugins")?
    {
        for (id, usage) in plugin["scripts"]["condition_functions"]
            .as_object()
            .ok_or("Missing conditions")?
        {
            *condition_counts.entry(id.parse()?).or_default() += usage["occurrences"]
                .as_u64()
                .ok_or("Missing condition count")?;
        }
    }
    let mut used_conditions = Vec::new();
    for (id, occurrences) in &condition_counts {
        let command_id = id
            .checked_add(0x1000)
            .ok_or("Condition/command binding overflow")?;
        let row = commands
            .iter()
            .find(|row| row["id"] == command_id)
            .ok_or("Condition binding absent from descriptor table")?;
        if row["condition_handler_present"] != true {
            return Err("Authored condition binding lacks an original evaluator".into());
        }
        used_conditions.push(json!({"condition_function_id":id,"bound_vanilla_command_id":command_id,"occurrences":occurrences,"descriptor":row}));
    }
    let changed_install = run.join("changed-executable");
    std::fs::create_dir(&changed_install)?;
    let original = std::fs::read(install.join("FalloutNV.exe"))?;
    let mut changed = original;
    *changed.last_mut().ok_or("Empty executable")? ^= 1;
    let changed_path = changed_install.join("FalloutNV.exe");
    write_new(&changed_path, &changed)?;
    let rejection_path = run.join("rejected-catalogue.json");
    let mut reject_rust = Command::new(cli);
    reject_rust
        .current_dir(root)
        .arg("command-catalogue")
        .arg("--install")
        .arg(&changed_install)
        .arg("--output")
        .arg(&rejection_path);
    let output = run_logged_status(reject_rust, &run.join("changed-executable-rust.log"), 1)?;
    if !String::from_utf8(output.stderr)?.contains("Unsupported executable fingerprint")
        || rejection_path.exists()
    {
        return Err("Changed executable was accepted by the Rust descriptor reader".into());
    }
    let mut reject_native = Command::new(oracle);
    reject_native.current_dir(root).arg(&changed_path);
    let output = run_logged_status(reject_native, &run.join("changed-executable-native.log"), 1)?;
    if !String::from_utf8(output.stderr)?.contains("unsupported executable fingerprint")
        || !output.stdout.is_empty()
    {
        return Err("Changed executable was accepted by the native descriptor reader".into());
    }
    Ok(json!({
        "schema_version":1,"profile":"nv-original","source_sha256":rust["source_sha256"],
        "source_bytes":rust["source_bytes"],"image_base":rust["image_base"],"pe_timestamp":rust["pe_timestamp"],
        "script_descriptors_compared":commands.len(),"event_descriptors_compared":events.len(),
        "statement_descriptors_compared":statements.len(),"parameter_descriptors_compared":parameters,
        "all_descriptor_fields_equal":true,"changed_executable_rejected_by_both_readers":true,
        "top_level_command_ids_bound":used_commands.len(),"observed_event_ids_bound":used_events.len(),
        "condition_function_ids_bound":used_conditions.len(),"condition_occurrences":condition_counts.values().sum::<u64>(),
        "used_commands":used_commands,"used_events":used_events,"used_conditions":used_conditions,
        "framing_usage_receipt_sha256":digest(&framing_path)?,"condition_census_sha256":digest(&condition_path)?,
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "oracle_binary_sha256":digest(oracle)?,"comparison_scope":"independent direct reads of the exact executable; PE mapping, descriptor/string/parameter metadata and provenance; no execution",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Native return types, effects, errors and full caller semantics","Expression calls and parameter operand decoding","Condition subject/comparison/grouping behavior","Event lifecycle and scheduling","Extensions and other executable fingerprints"],
    }))
}
