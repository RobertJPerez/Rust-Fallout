//! Source ownership is an engine guarantee. Existing independent readers bind
//! the source catalogue and schemas; worker/restoration checks cover our state.
use super::{Result, digest, json_file, run_logged};
use fallout_data::loaded_scripts::Handle;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const FOREIGN: &str = "source-item-regression/native-migration-regression/item-state-regression/leveled-source-regression/base-inventory-regression/foreign-runtime-regression";

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    install: &Path,
    sources: &Value,
) -> Result<Value> {
    let loaded_path = run
        .join(FOREIGN)
        .join("quest-script-regression/loaded-script-regression/loaded-scripts-rust.json");
    let loaded = json_file(&loaded_path)?;
    let schema_path = run
        .join(FOREIGN)
        .join("native-save-regression/schema-regression/script-state-rust.json");
    let schemas = json_file(&schema_path)?;
    let rows = loaded["scripts"]
        .as_array()
        .ok_or("Missing independently checked winning catalogue")?;
    let mut hash = Sha256::new();
    for row in rows {
        let handle: Handle = serde_json::from_value(row["handle"].clone())?;
        let bytes = serde_json::to_vec(&handle)?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    let expected_handles = format!("{:x}", hash.finalize());
    let cache = run.join("shared-runtime-cache");
    fs::create_dir(&cache)?;
    let mut phases = Vec::new();
    let mut first = None;
    for (name, reused) in [("cold", false), ("warm", true)] {
        let path = run.join(format!("shared-runtime-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("shared-runtime")
            .arg("--install")
            .arg(install)
            .arg("--load-order")
            .arg(root.join("profiles/nv-inspection-order.json"))
            .arg("--index-cache")
            .arg(&cache)
            .arg("--output")
            .arg(&path);
        run_logged(command, &run.join(format!("shared-runtime-{name}.log")))?;
        let report = json_file(&path)?;
        if report["sources"] != *sources
            || report["sources"] != loaded["plugins"]
            || report["source_definitions_checked"] != rows.len()
            || report["winning_handles_sha256"] != expected_handles
        {
            return Err(
                "Shared runtime source identities differ from the independent catalogue".into(),
            );
        }
        for key in [
            "instances",
            "pending_events",
            "snapshot_bytes",
            "snapshot_sha256",
            "catalogue_sha256",
        ] {
            if report[key] != schemas["state_probe"][key] {
                return Err(format!("Owned and borrowed engineering state differs: {key}").into());
            }
        }
        for key in [
            "worker_state_and_source_handles_equal",
            "old_handles_rejected",
            "failed_replacement_atomic",
            "owned_replacement_equal",
            "last_source_owner_released",
        ] {
            if report[key] != true {
                return Err(format!("Shared runtime check failed: {key}").into());
            }
        }
        for key in [
            "original_state_captured",
            "bytecode_executed",
            "retail_parity_accepted",
        ] {
            if report[key] != false {
                return Err("Shared runtime made an unsupported game claim".into());
            }
        }
        let plugins = report["index_cache"]["plugins"]
            .as_array()
            .ok_or("Missing shared runtime cache receipts")?;
        if plugins.len() != 10 || plugins.iter().any(|entry| entry["reused"] != reused) {
            return Err("Shared runtime cold/warm cache scope differs".into());
        }
        let mut comparable = report.clone();
        comparable
            .as_object_mut()
            .ok_or("Missing shared runtime object")?
            .remove("index_cache");
        if let Some(previous) = &first {
            if &comparable != previous {
                return Err("Shared runtime cold/warm reports differ".into());
            }
        } else {
            first = Some(comparable);
        }
        phases.push(json!({"name":name,"rust_report_sha256":digest(&path)?,"cache_reused":reused}));
    }
    let report = first.ok_or("Missing shared runtime phases")?;
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"Owned immutable source lifetime and worker/restoration engineering guarantees, bound to independently checked original definitions and schemas; no original execution acceptance",
        "sources":sources,"source_definitions_checked":rows.len(),"winning_handles_sha256":expected_handles,
        "all_independent_source_handles_and_borrowed_state_equal":true,"owned_runtime_probe":report,"phases":phases,
        "catalogue_report_sha256":digest(&loaded_path)?,"schema_report_sha256":digest(&schema_path)?,
        "bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Execution frames, semantic capabilities and observable effects","Actor/player/quest initialization and original scheduling","Presentation integration and retail scenarios"]}))
}
