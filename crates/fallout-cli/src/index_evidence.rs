//! Real-cell cached/uncached comparison. Retail-derived record fields remain local.
use super::{Result, digest, json_file, run_logged};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command, time::Instant};

pub fn run(
    root: &Path,
    directory: &Path,
    cli: &Path,
    install: &Path,
    binary: &str,
) -> Result<Value> {
    let cache = directory.join("index-cache");
    fs::create_dir(&cache)?;
    let mut reports = Vec::new();
    let mut timings = BTreeMap::new();
    let mut digests = BTreeMap::new();
    for name in ["uncached", "cold", "warm"] {
        let report_path = directory.join(format!("cell-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("cell")
            .arg("--install")
            .arg(install)
            .arg("--load-order")
            .arg(root.join("profiles/nv-inspection-order.json"))
            .args([
                "--editor-id",
                "GSDocMitchellHouse",
                "--defer-unread-payloads",
            ])
            .arg("--output")
            .arg(&report_path);
        if name != "uncached" {
            command.arg("--index-cache").arg(&cache);
        }
        let started = Instant::now();
        run_logged(command, &directory.join(format!("cell-{name}.log")))?;
        timings.insert(name, started.elapsed().as_secs_f64());
        digests.insert(name, digest(&report_path)?);
        let report = json_file(&report_path)?;
        if report["integrity_failures"] != 0
            || report["link_failures"] != 0
            || report["runtime_ready"] != false
        {
            return Err("Cell inspection failed or claimed gameplay acceptance".into());
        }
        reports.push(report);
    }
    let mut selected = reports[0].clone();
    selected
        .as_object_mut()
        .ok_or("Missing cell object")?
        .remove("index_cache");
    for report in &reports[1..] {
        let mut compared = report.clone();
        compared
            .as_object_mut()
            .ok_or("Missing cell object")?
            .remove("index_cache");
        if compared != selected {
            return Err("Cached and uncached selected fields differ".into());
        }
    }
    let cold = &reports[1]["index_cache"];
    let warm = &reports[2]["index_cache"];
    let cold_plugins = cold["plugins"].as_array().ok_or("Missing cold receipts")?;
    let warm_plugins = warm["plugins"].as_array().ok_or("Missing warm receipts")?;
    let order = json_file(&root.join("profiles/nv-inspection-order.json"))?;
    if cold_plugins.is_empty()
        || cold_plugins.len() != warm_plugins.len()
        || cold_plugins.len() != order.as_array().ok_or("Missing inspection order")?.len()
        || cold["ordered_source_sha256"] != warm["ordered_source_sha256"]
    {
        return Err("Cache receipt set or supplied order differs".into());
    }
    let mut plugin_names = Vec::new();
    let mut metadata_bytes = 0u64;
    let mut record_count = 0u64;
    for (i, (cold, warm)) in cold_plugins.iter().zip(warm_plugins).enumerate() {
        if cold["reused"] != false
            || warm["reused"] != true
            || cold["key"] != warm["key"]
            || warm["plugin"] != order[i]
        {
            return Err("Expected complete cold build and complete warm reuse".into());
        }
        let name = warm["plugin"]
            .as_str()
            .ok_or("Missing cached plugin name")?;
        if digest(&install.join("Data").join(name))? != warm["source_sha256"] {
            return Err("Plugin source changed after cached inspection".into());
        }
        let key = warm["key"].as_str().ok_or("Missing artifact key")?;
        let blob = cache.join(format!("{key}.blob"));
        if digest(&blob)? != warm["index_sha256"] || !cache.join(format!("{key}.json")).is_file() {
            return Err("Published index artifact failed verification".into());
        }
        metadata_bytes += warm["index_bytes"].as_u64().ok_or("Missing index size")?;
        record_count += warm["records"].as_u64().ok_or("Missing record count")?;
        plugin_names.push(name);
    }
    Ok(json!({
        "schema_version":1, "checkpoint":8, "cell":"GSDocMitchellHouse",
        "release_cli_sha256":binary, "all_selected_fields_equal":true,
        "references":selected["references"].as_array().ok_or("Missing references")?.len(),
        "base_records":selected["models"].as_array().ok_or("Missing bases")?.len(),
        "integrity_failures":0, "link_failures":0,
        "index_payloads_deferred":selected["index_payloads_deferred"],
        "plugins":warm_plugins, "ordered_source_sha256":warm["ordered_source_sha256"],
        "source_bytes_hashed_per_cached_open":warm["source_bytes_hashed"],
        "cached_record_definitions":record_count, "encoded_index_bytes":metadata_bytes,
        "cold_entries_built":cold_plugins.len(), "warm_entries_reused":warm_plugins.len(),
        "inspection_order":plugin_names, "local_report_sha256":digests,
        "whole_cell_cli_seconds":timings, "timing_samples_per_phase":1,
        "timing_scope":"Sequential wall-clock samples for complete CLI cell inspection, including archive indexes and dependency reads; not a startup SLA or sustained benchmark",
        "comparison":"Every serialized selected field exact after removing only the cache receipt; no tolerance",
        "process_termination_tests":"Workspace unit test kills publication children after blob staging, blob publication and marker staging; recovery verifies bytes and keeps sources unchanged",
        "acceptance":"Metadata caching verified; no gameplay scenario accepted",
        "known_gaps":[
            "Every source is fully hashed on each cached open; no timestamp-only shortcut",
            "Winning identities are rebuilt; no persisted linked or canonical content store",
            "Other record bodies stay deferred and unvalidated until strict access",
            "Rebuildable cache durability does not establish save or job-journal durability",
            "Archive/loose precedence, real physics, script execution and gameplay remain open"
        ]
    }))
}
