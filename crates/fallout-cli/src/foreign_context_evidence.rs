//! Live-list engineering proofs remain distinct from independent source facts.
use super::{
    Result, digest, json_file, native_save_evidence, quest_script_evidence, run_logged, write_new,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

pub(super) struct Oracles<'a> {
    pub contexts: &'a Path,
    pub saves: native_save_evidence::Oracles<'a>,
    pub quests: quest_script_evidence::Oracles<'a>,
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let container_oracle = oracles.saves.container;
    let native_directory = run.join("native-save-regression");
    fs::create_dir(&native_directory)?;
    let saves = native_save_evidence::run(root, &native_directory, cli, oracles.saves, install)?;
    let quest_directory = run.join("quest-script-regression");
    fs::create_dir(&quest_directory)?;
    let quests = quest_script_evidence::run(root, &quest_directory, cli, oracles.quests, install)?;
    let rust_path = run.join("foreign-context-rust.json");
    let repository = run.join("foreign-native-repository");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("foreign-context")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--new-repository")
        .arg(&repository)
        .arg("--output")
        .arg(&rust_path);
    run_logged(command, &run.join("foreign-context-rust.log"))?;
    let rust = json_file(&rust_path)?;
    let cold_path = run.join("foreign-context-cold.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("foreign-load-probe")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--repository")
        .arg(&repository)
        .arg("--player-id")
        .arg(
            rust["engineering_inputs"]["player_reference"]
                .as_u64()
                .ok_or("Missing explicit player identity")?
                .to_string(),
        )
        .arg("--output")
        .arg(&cold_path);
    run_logged(command, &run.join("foreign-context-cold.log"))?;
    let cold = json_file(&cold_path)?;
    for key in [
        "bound_statuses",
        "context_content",
        "instances",
        "lookup_results_sha256",
        "snapshot_sha256",
    ] {
        if cold[key] != rust[key] {
            return Err(format!("Cold foreign lookup proof differs: {key}").into());
        }
    }
    if cold["foreign_requests"] != rust["foreign_uses"]
        || cold["source_bound_restore"] != true
        || cold["receipt"]["metadata"] != rust["native_write"]["metadata"]
    {
        return Err("Cold foreign restore coverage differs".into());
    }
    let container_path = run.join("foreign-state-container-native.json");
    let mut command = Command::new(container_oracle);
    command
        .current_dir(root)
        .arg(repository.join("current.frsv"));
    let output = run_logged(command, &run.join("foreign-state-container-native.log"))?;
    write_new(&container_path, &output.stdout)?;
    let container = json_file(&container_path)?;
    if container["metadata"] != rust["native_write"]["metadata"] {
        return Err("Foreign state native container metadata differs".into());
    }
    let mut command = Command::new(oracles.contexts);
    command
        .current_dir(root)
        .arg(install.join("Data"))
        .arg(quest_directory.join("quest-order.bin"));
    let output = run_logged(command, &run.join("foreign-context-native.log"))?;
    let native_path = run.join("foreign-context-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    for key in ["context_content", "metadata", "sources"] {
        if rust[key] != native[key] {
            return Err(format!("Independent foreign context content differs: {key}").into());
        }
    }
    for key in [
        "canonical_state_round_trip_equal",
        "all_lookup_results_equal_after_restore",
        "old_handles_rejected",
    ] {
        if rust[key] != true {
            return Err(format!("Foreign runtime probe failed: {key}").into());
        }
    }
    if rust["original_live_values_captured"] != false
        || rust["bytecode_executed"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Foreign context proof crossed its engineering scope".into());
    }
    let static_report = json_file(&quest_directory.join("quest-scripts-rust.json"))?;
    if rust["sources"] != static_report["plugins"]
        || rust["metadata"] != static_report["metadata"]
        || rust["foreign_uses"] != static_report["operand_counts"]["foreign_uses"]
    {
        return Err(
            "Foreign live probe and independent static scan have different coverage".into(),
        );
    }
    let units = rust["compiled_units"]
        .as_array()
        .ok_or("Missing foreign compiled units")?;
    let expected = static_report["compiled_units"]
        .as_array()
        .ok_or("Missing static compiled units")?;
    if units.len() != expected.len() {
        return Err("Foreign compiled unit count differs".into());
    }
    for (unit, expected) in units.iter().zip(expected) {
        for key in ["handle", "binding_sha256", "counts", "foreign_uses"] {
            if unit[key] != expected[key] {
                return Err(format!("Foreign compiled unit coverage differs: {key}").into());
            }
        }
    }
    let mut observed = BTreeMap::<String, usize>::new();
    for row in rust["foreign_requests"]
        .as_array()
        .ok_or("Missing foreign requests")?
    {
        let key = serde_json::to_string(&json!([
            row["source_definition"]["key"],
            row["scda_offset"],
            row["role"],
            row["context_reference"],
            row["local_index"]
        ]))?;
        *observed.entry(key).or_default() += 1;
    }
    let mut expected = BTreeMap::<String, usize>::new();
    for row in static_report["foreign_operands"]
        .as_array()
        .ok_or("Missing static foreign operands")?
    {
        let key = serde_json::to_string(&json!([
            row["lookup"]["source_script"],
            row["scda_offset"],
            row["role"],
            row["lookup"]["context_reference"],
            row["lookup"]["local_index"]
        ]))?;
        *expected.entry(key).or_default() += 1;
    }
    if observed != expected {
        return Err("Foreign live probe lost or invented compiled requests".into());
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Explicit host live-list lookup/storage engineering; independent original header and compiled operand facts; no original live-value capture",
        "sources":rust["sources"],"metadata":rust["metadata"],"context_content":rust["context_content"],
        "compiled_units_compared":units.len(),"foreign_requests_compared":rust["foreign_uses"],"all_compiled_request_identities_equal":true,
        "engineering_inputs":rust["engineering_inputs"],"unbound_statuses":rust["unbound_statuses"],"bound_statuses":rust["bound_statuses"],"instances":rust["instances"],
        "snapshot_bytes":rust["snapshot_bytes"],"snapshot_sha256":rust["snapshot_sha256"],"lookup_results_sha256":rust["lookup_results_sha256"],
        "all_lookup_results_equal_after_restore":true,"all_cold_lookup_results_equal":true,"old_handles_rejected":true,
        "cold_report_sha256":digest(&cold_path)?,"native_container_report_sha256":digest(&container_path)?,"native_container":container["metadata"],
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"oracle_binary_sha256":digest(oracles.contexts)?,
        "fresh_native_save_regression":saves,"fresh_quest_operand_regression":quests,
        "original_live_values_captured":false,"bytecode_execution_ready":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original event-list creation, replacement and initialization timing are unmeasured","Placed probe definitions are explicit engineering templates, not original attachments",
        "Reference residency, lifetime, quest scheduling and native command effects remain unfinished","Full world save components, original save compatibility and retail differential traces remain open"]}),
    )
}
