//! Bounded exterior comparisons. Raw source fields and decoded bodies stay local.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_json, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

const CELLS: &[&str] = &[
    "Goodsprings",
    "GoodspringsSource",
    "TownCenter",
    "NVDLC02PineCreek",
    "NVDLC03SLVillage",
    "NVDLC04DivideEast",
];

fn command(root: &Path, cli: &Path, install: &Path, cell: &str, output: &Path) -> Command {
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("terrain")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--editor-id")
        .arg(cell)
        .arg("--output")
        .arg(output);
    command
}

fn entries(report: &Value) -> Result<Vec<&Value>> {
    let mut records = vec![&report["cell"]];
    records.extend(
        report["world_chain"]
            .as_array()
            .ok_or("Missing world chain")?,
    );
    records.extend(
        report["landscapes"]
            .as_array()
            .ok_or("Missing landscapes")?,
    );
    Ok(records)
}

fn selected(report: &Value) -> Result<Value> {
    let mut report = report.clone();
    let object = report.as_object_mut().ok_or("Missing terrain report")?;
    object.remove("index_cache");
    object.remove("comparison");
    object["cell"]
        .as_object_mut()
        .ok_or("Missing cell entry")?
        .remove("body_cache");
    for name in ["world_chain", "landscapes"] {
        for record in object[name]
            .as_array_mut()
            .ok_or("Missing terrain entries")?
        {
            record
                .as_object_mut()
                .ok_or("Missing terrain entry")?
                .remove("body_cache");
        }
    }
    Ok(report)
}

pub fn run(
    root: &Path,
    directory: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
    cli_sha: &str,
    oracle_sha: &str,
) -> Result<Value> {
    let cache = directory.join("index-cache");
    fs::create_dir(&cache)?;
    let mut datasets = Vec::new();
    let mut unique = BTreeSet::new();
    let mut negative = None;
    let mut record_appearances = 0usize;
    let mut height_samples = 0usize;
    let mut layers = 0usize;
    let mut alpha_vertices = 0usize;
    for (case, cell) in CELLS.iter().enumerate() {
        let folder = directory.join(cell);
        fs::create_dir(&folder)?;
        let bodies = folder.join("bodies");
        fs::create_dir(&bodies)?;
        let uncached_path = folder.join("uncached.json");
        run_logged(
            command(root, cli, install, cell, &uncached_path),
            &folder.join("uncached.log"),
        )?;
        let cached_path = folder.join("cached.json");
        let mut cached_command = command(root, cli, install, cell, &cached_path);
        cached_command
            .arg("--index-cache")
            .arg(&cache)
            .arg("--body-cache")
            .arg(&bodies);
        run_logged(cached_command, &folder.join("cached.log"))?;
        let cached = json_file(&cached_path)?;
        let uncached = json_file(&uncached_path)?;
        if selected(&uncached)? != selected(&cached)? {
            return Err("Uncached/cached exterior fields differ".into());
        }
        let receipts = cached["index_cache"]["plugins"]
            .as_array()
            .ok_or("Missing exterior index receipts")?;
        let order = json_file(&root.join("profiles/nv-inspection-order.json"))?;
        if receipts.len() != order.as_array().ok_or("Missing inspection order")?.len() {
            return Err("Exterior index receipt count differs from supplied order".into());
        }
        for (i, receipt) in receipts.iter().enumerate() {
            if receipt["plugin"] != order[i] || receipt["reused"] != (case != 0) {
                return Err("Exterior index order/build/reuse differs".into());
            }
        }
        let oracle_path = folder.join("oracle.json");
        let mut oracle_command = Command::new(oracle);
        oracle_command.current_dir(root).arg(&bodies);
        let raw = run_logged(oracle_command, &folder.join("oracle.log"))?;
        write_new(&oracle_path, &raw.stdout)?;
        let raw = json_file(&oracle_path)?;
        if raw["oracle_binary_sha256"] != oracle_sha {
            return Err("Terrain oracle binary identity differs".into());
        }
        let compared_path = folder.join("compared.json");
        let mut compared_command = command(root, cli, install, cell, &compared_path);
        compared_command
            .arg("--index-cache")
            .arg(&cache)
            .arg("--body-cache")
            .arg(&bodies)
            .arg("--oracle-report")
            .arg(&oracle_path);
        run_logged(compared_command, &folder.join("compared.log"))?;
        let compared = json_file(&compared_path)?;
        if selected(&cached)? != selected(&compared)?
            || compared["comparison"]["all_equal"] != true
            || compared["comparison"]["oracle_binary_sha256"] != oracle_sha
            || compared["comparison"]["oracle_report_sha256"] != digest(&oracle_path)?
            || compared["link_failures"] != 0
            || compared["integrity_failures"] != 0
            || compared["runtime_ready"] != false
        {
            return Err("Exterior comparison or acceptance checks failed".into());
        }
        let records = entries(&compared)?;
        if compared["comparison"]["records_compared"] != records.len() {
            return Err("Exterior record count differs".into());
        }
        record_appearances += records.len();
        let mut sources = Vec::new();
        for record in records {
            let body = record["body_cache"]["key"]
                .as_str()
                .ok_or("Missing compared body")?;
            unique.insert(body.to_owned());
            let plugin = record["source_plugin"]
                .as_str()
                .ok_or("Missing terrain source")?;
            if digest(&install.join("Data").join(plugin))? != record["source_sha256"] {
                return Err("Selected terrain source changed after inspection".into());
            }
            sources.push(json!({"kind":record["header"]["kind"], "key":record["key"],
                "source_plugin":plugin, "source_sha256":record["source_sha256"], "record_offset":record["header"]["offset"],
                "decoded_sha256":record["decoded_sha256"], "tagged_body_sha256":record["body_cache"]["manifest"]["sha256"]}));
            if let Some(deltas) = record["fields"]["heights"]["value"]["deltas"].as_array() {
                height_samples += deltas.len();
            }
            if let Some(values) = record["fields"]["layers"].as_array() {
                layers += values.len();
                alpha_vertices += values
                    .iter()
                    .filter_map(|layer| layer["alpha"]["value"].as_array())
                    .map(Vec::len)
                    .sum::<usize>();
            }
        }
        if case == 0 {
            let mut altered = raw.clone();
            let row = altered["files"]
                .as_array_mut()
                .ok_or("Missing oracle rows")?
                .iter_mut()
                .find(|row| row["fields"]["heights"].is_object())
                .ok_or("No height field for negative check")?;
            let bits = row["fields"]["heights"]["value"]["offset_bits"]
                .as_u64()
                .ok_or("Missing height offset bits")?;
            row["fields"]["heights"]["value"]["offset_bits"] = (bits ^ 1).into();
            let altered_path = folder.join("oracle-altered.json");
            write_json(&altered_path, &altered)?;
            let negative_path = folder.join("negative.json");
            let mut negative_command = command(root, cli, install, cell, &negative_path);
            negative_command
                .arg("--index-cache")
                .arg(&cache)
                .arg("--body-cache")
                .arg(&bodies)
                .arg("--oracle-report")
                .arg(&altered_path);
            run_logged_status(negative_command, &folder.join("negative.log"), 1)?;
            let rejected = json_file(&negative_path)?;
            if rejected["comparison"]["all_equal"] != false
                || rejected["comparison"]["differences"]
                    .as_array()
                    .is_none_or(Vec::is_empty)
            {
                return Err("Altered height field was not rejected".into());
            }
            negative = Some(
                json!({"changed_field":"fields.heights.value.offset_bits", "alteration":"One stored binary32 bit",
                "exit_code":1, "comparison_failed":true, "report_sha256":digest(&negative_path)?}),
            );
        }
        datasets.push(json!({"cell_editor_id":cell, "grid":compared["cell"]["fields"]["grid"]["value"],
            "world_chain_records":compared["world_chain"].as_array().ok_or("Missing worlds")?.len(),
            "landscape_records":compared["landscapes"].as_array().ok_or("Missing landscapes")?.len(),
            "all_equal":true, "uncached_cached_fields_equal":true, "records":sources,
            "oracle_report_sha256":digest(&oracle_path)?, "compared_report_sha256":digest(&compared_path)?,
            "index_payloads_deferred":compared["index_payloads_deferred"]}));
    }
    Ok(json!({
        "schema_version":1, "checkpoint":9, "release_cli_sha256":cli_sha, "terrain_oracle_binary_sha256":oracle_sha,
        "cells_compared":datasets.len(), "record_appearances_compared":record_appearances, "unique_tagged_bodies":unique.len(),
        "height_deltas_compared":height_samples, "texture_layers_compared":layers, "alpha_vertices_compared":alpha_vertices,
        "all_equal":true, "negative_comparison":negative, "datasets":datasets,
        "oracle":"Authored independent C++ field projection from pinned xEdit FNV layouts; this is not an xEdit executable or retail runtime comparison",
        "scope":"Exact selected WRLD/CELL/LAND fields, float bits, padding, unknown bytes and body digests; source-local membership and strict access tested separately",
        "acceptance":"Exterior source inspection; no rendered terrain or gameplay accepted",
        "known_gaps":[
            "Compression and canonical override resolution are not independently compared by the field oracle",
            "Height-delta reconstruction, normal interpretation and engine axes/units remain unverified",
            "Parent-world inheritance and editor defaults are not applied",
            "Terrain meshes, seams, layer blending, texture closure, water and physics are not implemented",
            "Known unrelated LAND checksum defect remains strict; other payloads stay deferred",
            "Retail archive precedence, streaming, navigation, scripts and gameplay remain open"
        ]
    }))
}
