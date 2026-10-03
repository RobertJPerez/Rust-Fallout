//! Header/source agreement and host item rules have separate evidence scopes.
use super::{
    Result, digest, item_state_evidence, json_file, native_migration_evidence, run_logged,
    write_new,
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: item_state_evidence::Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let container = oracles.content.inventory.runtime.saves.container;
    let regression = run.join("native-migration-regression");
    fs::create_dir(&regression)?;
    let migration = native_migration_evidence::run(root, &regression, cli, oracles, install)?;
    let repository = run.join("source-item-repository");
    let report_path = run.join("source-item-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("source-item-state")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--new-repository")
        .arg(&repository)
        .arg("--output")
        .arg(&report_path);
    run_logged(command, &run.join("source-item-rust.log"))?;
    let rust = json_file(&report_path)?;
    let items = &migration["fresh_item_state_regression"];
    let foreign = &items["fresh_leveled_source_regression"]["fresh_base_inventory_regression"]["fresh_foreign_runtime_regression"];
    if rust["source_content"] != foreign["context_content"]
        || rust["sources"] != items["sources"]
        || rust["source_item_inputs"] != items["source_item_inputs"]
    {
        return Err(
            "Source item identities/header cohort differ from independent regressions".into(),
        );
    }
    if rust["original_item_admission_verified"] != false
        || rust["original_live_values_captured"] != false
        || rust["all_same_process_observations_equal"] != true
        || rust["rejected_source_mutations_preserved_state"] != true
    {
        return Err("Source item scope or mutation proof differs".into());
    }
    let values = rust["observations"]["queries"]
        .as_array()
        .ok_or("Source item queries")?
        .iter()
        .map(|t| t["result"].as_u64().ok_or("Source item count"))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if values != [3, 0, 21, 3, 0, 0]
        || rust["observations"]["items"]
            .as_array()
            .ok_or("Source items")?
            .len()
            != 4
    {
        return Err("Source validated item counts differ from explicit operations".into());
    }
    let bindings = rust["source_item_inputs"]
        .as_array()
        .ok_or("Source item bindings")?;
    let mut checked = 0;
    for item in rust["observations"]["items"]
        .as_array()
        .ok_or("Source item observations")?
    {
        for form in item["proof"]["forms"]
            .as_array()
            .ok_or("Source form proofs")?
        {
            let binding = bindings
                .iter()
                .find(|b| b["binding"]["key"] == form["key"])
                .ok_or("Validated form missing independent original binding")?;
            let target = &binding["binding"]["target"];
            if form["source"]["kind"] != target["kind"]
                || form["source"]["flags"] != target["record_flags"]
            {
                return Err(
                    "Validated source form differs from independent original header facts".into(),
                );
            }
            checked += 1;
        }
    }
    let inputs = run.join("source-item-query-inputs.json");
    write_new(
        &inputs,
        &serde_json::to_vec(
            &json!({"owners":rust["engineering_inputs"]["owners"],"item_keys":rust["engineering_inputs"]["item_keys"]}),
        )?,
    )?;
    let cold_path = run.join("source-item-cold.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("source-item-load-probe")
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
    run_logged(command, &run.join("source-item-cold.log"))?;
    let cold = json_file(&cold_path)?;
    for key in [
        "policy",
        "source_content",
        "observations",
        "snapshot_bytes",
        "snapshot_sha256",
    ] {
        if cold[key] != rust[key] {
            return Err(format!("Cold source item differs: {key}").into());
        }
    }
    if cold["source_bound_restore"] != true
        || cold["receipt"]["metadata"] != rust["native_write"]["metadata"]
    {
        return Err("Cold source item native metadata differs".into());
    }
    let native_path = run.join("source-item-container-native.json");
    let mut command = Command::new(container);
    command
        .current_dir(root)
        .arg(repository.join("current.frsv"));
    let output = run_logged(command, &run.join("source-item-container-native.log"))?;
    write_new(&native_path, &output.stdout)?;
    if json_file(&native_path)?["metadata"] != rust["native_write"]["metadata"] {
        return Err("Source item independent container differs".into());
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Source-bound explicit host item validation; independent header facts/container integrity, no original admission behavior",
        "source_content":rust["source_content"],"sources":rust["sources"],"policy":rust["policy"],"engineering_inputs":rust["engineering_inputs"],"source_item_inputs":rust["source_item_inputs"],
        "checked_source_form_occurrences":checked,"all_validated_source_forms_independently_equal":true,"rejected_source_mutations_preserved_state":true,"all_cold_observations_equal":true,
        "item_instances":4,"query_results":values,"snapshot_bytes":rust["snapshot_bytes"],"snapshot_sha256":rust["snapshot_sha256"],"native_metadata":rust["native_write"]["metadata"],
        "rust_report_sha256":digest(&report_path)?,"cold_report_sha256":digest(&cold_path)?,"native_report_sha256":digest(&native_path)?,"fresh_native_migration_regression":migration,
        "original_item_admission_verified":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original item initialization, admission domains, stack/equipment/ownership rules and callbacks","Immutable header validity is not body/asset residency or behavioral eligibility","Original primitive query coercion/execution and complete mutable world persistence"]}),
    )
}
