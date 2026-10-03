//! Item transactions and native restoration are engineering proofs, not retail traces.
use super::{Result, digest, json_file, leveled_evidence, run_logged, write_new};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
pub(super) struct Oracles<'a> {
    pub content: leveled_evidence::Oracles<'a>,
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let container = oracles.content.inventory.runtime.saves.container;
    let content_directory = run.join("leveled-source-regression");
    fs::create_dir(&content_directory)?;
    let content = leveled_evidence::run(root, &content_directory, cli, oracles.content, install)?;
    let repository = run.join("item-native-repository");
    let rust_path = run.join("item-state-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("item-state")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--new-repository")
        .arg(&repository)
        .arg("--output")
        .arg(&rust_path);
    run_logged(command, &run.join("item-state-rust.log"))?;
    let rust = json_file(&rust_path)?;
    for key in [
        "canonical_state_round_trip_equal",
        "all_query_traces_equal_after_restore",
        "rejected_mutations_preserved_state",
        "uninitialized_inventory_rejected",
        "worker_capture_isolated",
    ] {
        if rust[key] != true {
            return Err(format!("Item engineering proof failed: {key}").into());
        }
    }
    if rust["original_live_values_captured"] != false
        || rust["original_item_admission_verified"] != false
        || rust["bytecode_executed"] != false
        || rust["retail_parity_accepted"] != false
        || rust["state_schema"] != 3
    {
        return Err("Item state proof crossed its declared engineering scope".into());
    }
    if rust["sources"] != content["sources"]
        || rust["inventory_counts"] != content["fresh_base_inventory_regression"]["counts"]
    {
        return Err("Item/source cohort coverage differs".into());
    }
    let native_inventory =
        json_file(&content_directory.join("base-inventory-regression/base-inventory-native.json"))?;
    let definitions = native_inventory["definitions"]
        .as_array()
        .ok_or("Independent source definitions missing")?;
    let inputs = rust["source_item_inputs"]
        .as_array()
        .ok_or("Missing item source inputs")?;
    if inputs.len() != 3 {
        return Err("Item input source coverage differs".into());
    }
    for input in inputs {
        let definition = definitions
            .iter()
            .find(|d| d["key"] == input["parent"])
            .ok_or("Item source parent missing from independent content")?;
        let field = definition["fields"]
            .as_array()
            .ok_or("Independent item fields")?
            .iter()
            .find(|f| f["decoded_offset"] == input["field_decoded_offset"])
            .ok_or("Item source field missing")?;
        if field["value"]["item"] != input["binding"]
            || field["value"]["schema_kind_allowed"] != true
        {
            return Err("Item source binding differs from independent original facts".into());
        }
    }
    let values = rust["query_traces"]
        .as_array()
        .ok_or("Missing query traces")?
        .iter()
        .map(|t| t["result"].as_u64().ok_or("Query result missing"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if values != [3, 0, 12, 3, 0, 0] || rust["item_instances"] != 3 || rust["inventory_banks"] != 2
    {
        return Err("Item transactions/counts differ from explicit host inputs".into());
    }
    let query_path = run.join("item-query-inputs.json");
    write_new(
        &query_path,
        &serde_json::to_vec(
            &json!({"owners":rust["engineering_inputs"]["owners"],"item_keys":rust["engineering_inputs"]["item_keys"]}),
        )?,
    )?;
    let cold_path = run.join("item-state-cold.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("item-load-probe")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--repository")
        .arg(&repository)
        .arg("--query-inputs")
        .arg(&query_path)
        .arg("--output")
        .arg(&cold_path);
    run_logged(command, &run.join("item-state-cold.log"))?;
    let cold = json_file(&cold_path)?;
    for key in [
        "query_traces",
        "snapshot_bytes",
        "snapshot_sha256",
        "item_instances",
        "inventory_banks",
        "state_schema",
    ] {
        if cold[key] != rust[key] {
            return Err(format!("Cold item state differs: {key}").into());
        }
    }
    if cold["source_bound_restore"] != true
        || cold["receipt"]["metadata"] != rust["native_write"]["metadata"]
    {
        return Err("Cold item native metadata differs".into());
    }
    let native_path = run.join("item-container-native.json");
    let mut command = Command::new(container);
    command
        .current_dir(root)
        .arg(repository.join("current.frsv"));
    let output = run_logged(command, &run.join("item-container-native.log"))?;
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if native["metadata"] != rust["native_write"]["metadata"] {
        return Err("Independent item container metadata differs".into());
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Canonical explicit item state, count queries and native persistence engineering; independent container integrity and source identities only",
        "sources":rust["sources"],"engineering_inputs":rust["engineering_inputs"],"source_item_inputs":rust["source_item_inputs"],"all_source_item_bindings_independently_equal":true,
        "item_instances":rust["item_instances"],"inventory_banks":rust["inventory_banks"],"script_instances":rust["script_instances"],"state_schema":rust["state_schema"],
        "query_results":values,"all_cold_query_traces_equal":true,"snapshot_bytes":rust["snapshot_bytes"],"snapshot_sha256":rust["snapshot_sha256"],
        "worker_capture_isolated":true,"rejected_mutations_preserved_state":true,"rust_report_sha256":digest(&rust_path)?,"cold_report_sha256":digest(&cold_path)?,"native_container_report_sha256":digest(&native_path)?,"native_metadata":native["metadata"],
        "fresh_leveled_source_regression":content,"original_live_values_captured":false,"original_item_admission_verified":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original live inventory initialization, deltas, leveled selection and item admission","Original condition conversion, stack/equipment/ownership/ammo/modification semantics and callbacks","Native GetItemCount/condition adapters and numeric coercion","Automatic old native-envelope migration and retail .fos compatibility","Complete player/actor/quest/world persistence and retail differential traces"]}),
    )
}
