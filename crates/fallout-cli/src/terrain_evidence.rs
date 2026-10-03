//! Bounded exterior comparisons. Raw source fields and decoded bodies stay local.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_json, write_new};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

pub(super) const CELLS: &[&str] = &[
    "Goodsprings",
    "GoodspringsSource",
    "TownCenter",
    "NVDLC02PineCreek",
    "NVDLC03SLVillage",
    "NVDLC04DivideEast",
];

fn command(
    root: &Path,
    cli: &Path,
    install: &Path,
    cell: &str,
    output: &Path,
    oracle: &OracleRun<'_>,
) -> Command {
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
    if oracle.heights {
        command.arg("--reconstruct-heights");
    }
    if oracle.geometry {
        command.arg("--inspect-mesh");
    }
    if oracle.textures {
        command.arg("--inspect-textures");
    }
    if oracle.blends {
        command.arg("--inspect-blends");
    }
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
    if let Some(textures) = report["texture_dependencies"]["records"].as_array() {
        records.extend(textures);
    }
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
    if let Some(textures) = object.get_mut("texture_dependencies") {
        for record in textures["records"]
            .as_array_mut()
            .ok_or("Missing texture record array")?
        {
            record
                .as_object_mut()
                .ok_or("Missing texture entry")?
                .remove("body_cache");
        }
        for asset in textures["assets"]
            .as_array_mut()
            .ok_or("Missing texture asset array")?
        {
            asset
                .as_object_mut()
                .ok_or("Missing texture asset")?
                .remove("cache");
        }
    }
    Ok(report)
}

pub struct OracleRun<'a> {
    pub binary: &'a Path,
    pub sha256: &'a str,
    pub heights: bool,
    pub geometry: bool,
    pub textures: bool,
    pub blends: bool,
}

pub fn run(
    root: &Path,
    directory: &Path,
    cli: &Path,
    oracle: OracleRun<'_>,
    install: &Path,
    cli_sha: &str,
) -> Result<Value> {
    let oracle_sha = oracle.sha256;
    let cache = directory.join("index-cache");
    fs::create_dir(&cache)?;
    let texture_cache = directory.join("texture-cache");
    if oracle.textures {
        fs::create_dir(&texture_cache)?;
    }
    let mut datasets = Vec::new();
    let mut unique = BTreeSet::new();
    let mut negative = None;
    let mut record_appearances = 0usize;
    let mut height_samples = 0usize;
    let mut layers = 0usize;
    let mut alpha_vertices = 0usize;
    let mut normal_samples = 0usize;
    let mut color_samples = 0usize;
    let mut source_digests = std::collections::BTreeMap::new();
    for (case, cell) in CELLS.iter().enumerate() {
        let folder = directory.join(cell);
        fs::create_dir(&folder)?;
        let bodies = folder.join("bodies");
        fs::create_dir(&bodies)?;
        let uncached_path = folder.join("uncached.json");
        run_logged(
            command(root, cli, install, cell, &uncached_path, &oracle),
            &folder.join("uncached.log"),
        )?;
        let cached_path = folder.join("cached.json");
        let mut cached_command = command(root, cli, install, cell, &cached_path, &oracle);
        cached_command
            .arg("--index-cache")
            .arg(&cache)
            .arg("--body-cache")
            .arg(&bodies);
        if oracle.textures {
            cached_command.arg("--texture-cache").arg(&texture_cache);
        }
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
        let mut oracle_command = Command::new(oracle.binary);
        oracle_command.current_dir(root).arg(&bodies);
        if oracle.geometry {
            oracle_command.arg("--geometry");
        } else if oracle.heights {
            oracle_command.arg("--heights");
        }
        if oracle.blends {
            oracle_command.arg("--blends");
        }
        let raw = run_logged(oracle_command, &folder.join("oracle.log"))?;
        write_new(&oracle_path, &raw.stdout)?;
        let raw = json_file(&oracle_path)?;
        if raw["oracle_binary_sha256"] != oracle_sha {
            return Err("Terrain oracle binary identity differs".into());
        }
        let compared_path = folder.join("compared.json");
        let mut compared_command = command(root, cli, install, cell, &compared_path, &oracle);
        compared_command
            .arg("--index-cache")
            .arg(&cache)
            .arg("--body-cache")
            .arg(&bodies)
            .arg("--oracle-report")
            .arg(&oracle_path);
        if oracle.textures {
            compared_command.arg("--texture-cache").arg(&texture_cache);
        }
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
        if oracle.textures && compared["texture_dependencies"]["failures"] != 0 {
            return Err("Texture dependency closure failed".into());
        }
        if compared["comparison"]["records_compared"] != records.len() {
            return Err("Exterior record count differs".into());
        }
        if oracle.heights && compared["comparison"]["height_grids_compared"] != 1 {
            return Err("Exterior fixture did not compare exactly one reconstructed grid".into());
        }
        if oracle.geometry && compared["comparison"]["source_meshes_compared"] != 1 {
            return Err("Exterior fixture did not compare exactly one source mesh".into());
        }
        if oracle.blends && compared["comparison"]["blend_maps_compared"] != 1 {
            return Err("Exterior fixture did not compare exactly one blend map".into());
        }
        if oracle.geometry {
            let meshes = compared["source_meshes"]
                .as_array()
                .ok_or("Missing source meshes")?;
            if meshes.len() != 1
                || meshes[0]["geometry"]["hidden_quadrants"] != 0
                || meshes[0]["geometry"]["local_positions"]
                    .as_array()
                    .map(Vec::len)
                    != Some(1089)
                || meshes[0]["geometry"]["indices"].as_array().map(Vec::len) != Some(6144)
            {
                return Err("Real geometry fixture is not one complete unhidden grid".into());
            }
            normal_samples += meshes[0]["geometry"]["normal_bits"]
                .as_array()
                .map_or(0, Vec::len);
            color_samples += meshes[0]["geometry"]["colors"]
                .as_array()
                .map_or(0, Vec::len);
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
            if !source_digests.contains_key(plugin) {
                source_digests.insert(
                    plugin.to_owned(),
                    digest(&install.join("Data").join(plugin))?,
                );
            }
            if source_digests[plugin] != record["source_sha256"] {
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
                .find(|row| {
                    if oracle.blends {
                        row["blend_maps"]["quadrants"][0]["base"].is_object()
                    } else if oracle.textures {
                        row["fields"]["paths"][0]["value"]
                            .as_array()
                            .is_some_and(|p| !p.is_empty())
                    } else {
                        row["fields"]["heights"].is_object()
                    }
                })
                .ok_or("No height field for negative check")?;
            let changed_field = if oracle.blends {
                let byte = row["blend_maps"]["quadrants"][0]["base"]["weights"][0]
                    .as_u64()
                    .ok_or("Missing base weight")?;
                row["blend_maps"]["quadrants"][0]["base"]["weights"][0] = (byte ^ 1).into();
                "blend_maps.quadrants[0].base.weights[0]"
            } else if oracle.textures {
                let byte = row["fields"]["paths"][0]["value"][0]
                    .as_u64()
                    .ok_or("Missing authored path byte")?;
                row["fields"]["paths"][0]["value"][0] = (byte ^ 1).into();
                "fields.paths[0].value[0]"
            } else if oracle.geometry {
                row["source_mesh"]["indices"]
                    .as_array_mut()
                    .ok_or("Missing mesh indices")?
                    .swap(0, 1);
                "source_mesh.indices[0,1]"
            } else if oracle.heights {
                let bits = row["height_grid"]["height_bits"][1088]
                    .as_u64()
                    .ok_or("Missing derived height bits")?;
                row["height_grid"]["height_bits"][1088] = (bits ^ 1).into();
                "height_grid.height_bits[1088]"
            } else {
                let bits = row["fields"]["heights"]["value"]["offset_bits"]
                    .as_u64()
                    .ok_or("Missing height offset bits")?;
                row["fields"]["heights"]["value"]["offset_bits"] = (bits ^ 1).into();
                "fields.heights.value.offset_bits"
            };
            let altered_path = folder.join("oracle-altered.json");
            write_json(&altered_path, &altered)?;
            let negative_path = folder.join("negative.json");
            let mut negative_command = command(root, cli, install, cell, &negative_path, &oracle);
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
                return Err("Altered terrain projection was not rejected".into());
            }
            negative = Some(
                json!({"changed_field":changed_field, "alteration": if oracle.blends { "One calculated base-weight byte" } else if oracle.textures { "One authored texture path byte" } else if oracle.geometry { "Reverse one triangle's winding" } else { "One binary32 bit" },
                "exit_code":1, "comparison_failed":true, "report_sha256":digest(&negative_path)?}),
            );
        }
        datasets.push(json!({"cell_editor_id":cell, "grid":compared["cell"]["fields"]["grid"]["value"],
            "world_chain_records":compared["world_chain"].as_array().ok_or("Missing worlds")?.len(),
            "landscape_records":compared["landscapes"].as_array().ok_or("Missing landscapes")?.len(),
            "all_equal":true, "uncached_cached_fields_equal":true, "records":sources,
            "oracle_report_sha256":digest(&oracle_path)?, "compared_report_sha256":digest(&compared_path)?,
            "index_payloads_deferred":compared["index_payloads_deferred"]}));
        if oracle.geometry {
            let dataset = datasets.last_mut().ok_or("Missing geometry dataset")?;
            let mesh = &compared["source_meshes"][0]["geometry"];
            dataset["normal_samples"] = mesh["normal_bits"].as_array().map_or(0, Vec::len).into();
            dataset["color_samples"] = mesh["colors"].as_array().map_or(0, Vec::len).into();
            dataset["hidden_quadrants"] = mesh["hidden_quadrants"].clone();
        }
        if oracle.blends {
            let dataset = datasets.last_mut().ok_or("Missing blend dataset")?;
            let maps = &compared["blend_maps"][0]["blends"];
            dataset["blend_maps_sha256"] =
                format!("{:x}", Sha256::digest(serde_json::to_vec(maps)?)).into();
            dataset["blend_weight_values"] = (compared["landscapes"][0]["fields"]["layers"]
                .as_array()
                .ok_or("Missing layers")?
                .len()
                * 289)
                .into();
            for field in [
                "clamped_samples",
                "overfull_vertices",
                "unapplied_default_layers",
                "missing_base_quadrants",
            ] {
                dataset[field] = maps[field].clone();
            }
        }
    }
    let mut summary = json!({
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
    });
    if oracle.heights {
        summary["checkpoint"] = 10.into();
        summary["height_grids_compared"] = datasets.len().into();
        summary["reconstructed_samples_compared"] = height_samples.into();
        summary["height_model"] = fallout_data::terrain::heights::HEIGHT_MODEL.into();
        summary["oracle"] = "Authored C++ dependency-path height evaluation informed by pinned OpenMW ESM4 convention, plus xEdit field projection; neither upstream application nor retail engine was run".into();
        summary["scope"] = "Exact selected source fields and reconstructed height bits against separately authored C++ dependency-path evaluation; no retail terrain behavior comparison".into();
        summary["known_gaps"] = json!([
            "Compression and canonical override resolution are not independently compared by the field oracle",
            "Reference height convention is not measured retail NV axes/units or interpolation",
            "Parent-world inheritance and editor defaults are not applied",
            "Normal interpretation, meshes, seams correction, blending, textures, water and physics remain open",
            "Known unrelated LAND checksum defect remains strict; other payloads stay deferred",
            "Retail profiles, archive precedence, streaming, navigation, scripts and gameplay remain open"
        ]);
    }
    if oracle.geometry {
        summary["checkpoint"] = 11.into();
        summary["source_meshes_compared"] = datasets.len().into();
        summary["geometry_model"] = fallout_data::terrain::mesh::GEOMETRY_MODEL.into();
        summary["vertices_compared"] = height_samples.into();
        summary["normalized_normals_compared"] = normal_samples.into();
        summary["color_triplets_compared"] = color_samples.into();
        summary["triangles_compared"] = (datasets.len() * 2048).into();
        summary["scope"] = "Exact fields, height bits, source-local f64 positions, normalized f32 normal bits, optional color bytes, bounds and unhidden checkerboard indices against original C++ tooling; CELL hide masks tested synthetically; no measured retail topology or normal repair".into();
        summary["acceptance"] = "Source geometry inspection; GPU captures recorded separately; no gameplay or retail rendering acceptance".into();
        summary["known_gaps"] = json!([
            "Independent geometry oracle consumes Rust-decoded bodies; compression and canonical override resolution are outside its scope",
            "Checkerboard winding, height scale, axes, normals and color-space interpretation have no measured retail acceptance",
            "Authored edge/corner normals remain unchanged; reference engine normal repair is not implemented",
            "Parent inheritance, landscape texture closure and blending, water, props, streaming and physics remain open",
            "Original effective profiles, archive precedence and known vanilla format exceptions still block M1"
        ]);
    }
    if oracle.textures {
        summary["checkpoint"] = 12.into();
        summary["scope"] = "Exact selected WRLD/CELL/LAND/LTEX/TXST fields and source geometry against original C++ projection; authored texture assets and cold/warm cache compared separately".into();
        summary["known_gaps"] = json!([
            "Field oracle consumes Rust-decoded plugin bodies; compression and canonical resolution are not independently compared",
            "Selected archive bytes are compared separately with ba2; no loose-file or retail mount precedence is claimed",
            "Empty/missing fields are preserved without defaults; parent inheritance and grass asset closure remain open",
            "Texture pixels, LAND blending, material interpretation and retail rendering acceptance remain unfinished",
            "Original effective profiles, archive precedence and known vanilla format exceptions still block M1"
        ]);
    }
    if oracle.blends {
        summary["checkpoint"] = 13.into();
        summary["blend_model"] = fallout_data::terrain::blends::BLEND_MODEL.into();
        summary["blend_maps_compared"] = datasets.len().into();
        summary["blend_weight_values_compared"] = datasets
            .iter()
            .map(|d| d["blend_weight_values"].as_u64().unwrap_or(0))
            .sum::<u64>()
            .into();
        summary["scope"] = "Source fields, geometry and quadrant-local byte weights against original C++ calculation; sparse samples, absent bases, unapplied NULL defaults and overfull weights retained; no cross-cell blend-map repair or measured retail rendering".into();
    }
    Ok(summary)
}
