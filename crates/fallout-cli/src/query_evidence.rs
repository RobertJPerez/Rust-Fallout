//! Source entry metadata and engineering query results are proved separately.
use super::{
    Result, condition_dependency_evidence, digest, item_state_evidence, json_file, run_logged,
    source_item_evidence, write_new,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: item_state_evidence::Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let commands = oracles.content.inventory.runtime.quests.operands.catalogue;
    let regression = run.join("source-item-regression");
    fs::create_dir(&regression)?;
    let source = source_item_evidence::run(root, &regression, cli, oracles, install)?;
    let condition_directory = run.join("condition-source-regression");
    fs::create_dir(&condition_directory)?;
    let condition_oracle =
        root.join("local/condition-operand-oracle-build/Release/condition-operand-oracle.exe");
    let conditions = condition_dependency_evidence::run(
        root,
        &condition_directory,
        cli,
        &condition_oracle,
        commands,
        install,
    )?;
    let records = json_file(&condition_directory.join("condition-dependencies-rust.json"))?;
    let mut usage = BTreeMap::<u16, u64>::new();
    for record in records["records"].as_array().ok_or("Condition records")? {
        for condition in record["conditions"].as_array().ok_or("Condition fields")? {
            let id = u16::try_from(
                condition["function_id"]
                    .as_u64()
                    .ok_or("Condition function ID")?,
            )?;
            *usage.entry(id).or_default() += 1;
        }
    }
    if usage.values().sum::<u64>() != 80467 {
        return Err("Condition registry coverage differs".into());
    }
    let path = run.join("primitive-query-rust.json");
    let repository = run.join("primitive-query-repository");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("primitive-query-state")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--new-repository")
        .arg(&repository)
        .arg("--output")
        .arg(&path);
    run_logged(command, &run.join("primitive-query-rust.log"))?;
    let rust = json_file(&path)?;
    let catalogue_path = condition_directory.join("catalogue-regression/catalogue-rust.json");
    let catalogue = json_file(&catalogue_path)?;
    let descriptor = catalogue["script_commands"]
        .as_array()
        .ok_or("Independent command descriptors")?
        .iter()
        .find(|c| c["name"] == "GetItemCount")
        .ok_or("GetItemCount source descriptor")?;
    if rust["descriptor"] != *descriptor
        || rust["executable_sha256"] != catalogue["source_sha256"]
        || rust["source_content"] != source["source_content"]
        || rust["sources"] != source["sources"]
    {
        return Err("Primitive query source contracts differ".into());
    }
    for flag in [
        "all_adapter_core_traces_equal",
        "all_same_process_traces_equal",
        "canonical_state_unchanged",
        "unsupported_entry_and_implicit_subject_rejected",
    ] {
        if rust[flag] != true {
            return Err(format!("Primitive query proof failed: {flag}").into());
        }
    }
    for flag in [
        "original_numeric_return_verified",
        "original_argument_coercion_verified",
        "original_handler_executed",
        "bytecode_executed",
        "retail_parity_accepted",
    ] {
        if rust[flag] != false {
            return Err(format!("Primitive query exceeded engineering scope: {flag}").into());
        }
    }
    let traces = rust["traces"].as_array().ok_or("Query traces")?;
    if traces.len() != 12 {
        return Err("Shared query call coverage differs".into());
    }
    let expected = [3, 0, 21, 3, 0, 0];
    for (pair, &value) in traces.as_chunks::<2>().0.iter().zip(expected.iter()) {
        if pair[0]["query"] != pair[1]["query"]
            || pair[0]["query"]["result"] != value
            || pair.iter().any(|t| {
                !t["original_numeric_return"].is_null() || t["original_behavior_verified"] != false
            })
        {
            return Err("Shared query core or unresolved return differs".into());
        }
    }
    let inputs = run.join("primitive-query-inputs.json");
    write_new(&inputs, &serde_json::to_vec(&rust["engineering_inputs"])?)?;
    let cold_path = run.join("primitive-query-cold.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("primitive-query-load-probe")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--repository")
        .arg(&repository)
        .arg("--query-inputs")
        .arg(&inputs)
        .arg("--output")
        .arg(&cold_path);
    run_logged(command, &run.join("primitive-query-cold.log"))?;
    let cold = json_file(&cold_path)?;
    for key in [
        "traces",
        "descriptor",
        "snapshot_sha256",
        "snapshot_bytes",
        "receipt",
        "source_content",
    ] {
        if cold[key] != rust[key] {
            return Err(format!("Cold primitive queries differ: {key}").into());
        }
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Shared source entry IDs over exact explicit host inventory count traces; original argument/return coercion and execution unverified",
        "descriptor":descriptor,"executable_sha256":rust["executable_sha256"],"command_descriptors":catalogue["script_commands"],"condition_function_usage":usage,"winning_conditions":80467,
        "sources":rust["sources"],"engineering_inputs":rust["engineering_inputs"],"query_results":expected,"shared_calls":12,"all_adapter_core_traces_equal":true,"all_cold_traces_equal":true,"canonical_state_unchanged":true,
        "snapshot_bytes":rust["snapshot_bytes"],"snapshot_sha256":rust["snapshot_sha256"],"rust_report_sha256":digest(&path)?,"cold_report_sha256":digest(&cold_path)?,"catalogue_report_sha256":digest(&catalogue_path)?,
        "fresh_source_item_regression":source,"fresh_condition_source_regression":conditions,"original_numeric_return_verified":false,"original_argument_coercion_verified":false,"original_handler_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original form-list expansion, inventory count arithmetic and argument/return coercion","Original subject selection, comparison/group evaluation, script/native execution and timing","Original initialization/admission and complete mutable world persistence"]}),
    )
}
