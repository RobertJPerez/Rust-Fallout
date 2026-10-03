//! Original gradient fixtures supplement the real-cell comparisons. Neighbor
//! boundaries are diagnostics, not a claim about retail stitching behavior.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use fallout_data::terrain::{HeightMap, heights};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn tagged(map: &HeightMap) -> Vec<u8> {
    let mut bytes = b"LANDVHGT".to_vec();
    bytes.extend(1096u16.to_le_bytes());
    bytes.extend(map.offset_bits.to_le_bytes());
    bytes.extend(map.deltas.iter().map(|v| *v as u8));
    bytes.extend(map.unused);
    bytes
}

fn synthetic(directory: &Path, oracle: &Path, oracle_sha: &str) -> Result<Value> {
    let folder = directory.join("synthetic-heights");
    fs::create_dir(&folder)?;
    let mut expected = BTreeMap::new();
    let mut seed = 0x9362_17acu32;
    for case in 0..36 {
        let offset: f32 = match case {
            0 => -0.,
            1 => 16_777_216.,
            2 => -16_777_216.,
            3 => f32::from_bits(1),
            _ => -123.25,
        };
        let mut deltas = Vec::with_capacity(heights::SAMPLE_COUNT);
        for _ in 0..heights::SAMPLE_COUNT {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            deltas.push(if matches!(case, 1 | 2) { 1 } else { seed as i8 });
        }
        let map = HeightMap {
            offset_bits: offset.to_bits(),
            deltas,
            unused: [9, 81, 255],
        };
        let name = format!("fixture-{case:02}.blob");
        let path = folder.join(&name);
        write_new(&path, &tagged(&map))?;
        expected.insert(
            name,
            (
                digest(&path)?,
                serde_json::to_value(heights::reconstruct(&map)?)?,
            ),
        );
    }
    let oracle_path = directory.join("synthetic-oracle.json");
    let mut command = Command::new(oracle);
    command.arg(&folder).arg("--heights");
    let output = run_logged(command, &directory.join("synthetic-oracle.log"))?;
    write_new(&oracle_path, &output.stdout)?;
    let report = json_file(&oracle_path)?;
    if report["oracle_binary_sha256"] != oracle_sha {
        return Err("Synthetic height oracle identity differs".into());
    }
    let rows = report["files"]
        .as_array()
        .ok_or("Missing synthetic oracle files")?;
    if rows.len() != expected.len() {
        return Err("Synthetic height input set differs".into());
    }
    for row in rows {
        let name = row["file"].as_str().ok_or("Missing synthetic body name")?;
        let (sha256, grid) = expected
            .remove(name)
            .ok_or("Unexpected or duplicate synthetic oracle body")?;
        if row["sha256"] != sha256 || row["height_grid"] != grid {
            return Err(format!("Synthetic height projection differs for {name}").into());
        }
    }
    if !expected.is_empty() {
        return Err("Synthetic oracle omitted height bodies".into());
    }
    let overflow = directory.join("overflow-heights");
    fs::create_dir(&overflow)?;
    let invalid = HeightMap {
        offset_bits: f32::MAX.to_bits(),
        deltas: vec![0; heights::SAMPLE_COUNT],
        unused: [0; 3],
    };
    if heights::reconstruct(&invalid).is_ok() {
        return Err("Rust admitted scaled height overflow".into());
    }
    write_new(&overflow.join("overflow.blob"), &tagged(&invalid))?;
    let mut command = Command::new(oracle);
    command.arg(&overflow).arg("--heights");
    let output = run_logged_status(command, &directory.join("overflow-oracle.log"), 1)?;
    if !String::from_utf8_lossy(&output.stderr).contains("height reconstruction overflow") {
        return Err("Native overflow check failed for an unrelated reason".into());
    }
    Ok(
        json!({"fixtures_compared":36, "samples_compared":36 * heights::SAMPLE_COUNT,
        "all_equal":true, "oracle_report_sha256":digest(&oracle_path)?,
        "scaled_overflow_rejected_by_both":true, "overflow_oracle_exit_code":1,
        "scope":"Original gradients, signed extremes, fractional/subnormal offsets and binary32 rounding; no retail assets"}),
    )
}

pub fn run(
    root: &Path,
    directory: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
    oracle_sha: &str,
) -> Result<Value> {
    let synthetic = synthetic(directory, oracle, oracle_sha)?;
    let mut boundaries = Vec::new();
    for (direction, id, coordinates) in [
        ("east", "DAEB9", [-17, 0]),
        ("west", "DAEBD", [-19, 0]),
        ("north", "DAEBA", [-18, 1]),
        ("south", "E1AA7", [-18, -1]),
    ] {
        let path = directory.join(format!("goodsprings-edge-{direction}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("terrain")
            .arg("--install")
            .arg(install)
            .arg("--load-order")
            .arg(root.join("profiles/nv-inspection-order.json"))
            .arg("--editor-id")
            .arg("Goodsprings")
            .arg("--reconstruct-heights")
            .arg("--index-cache")
            .arg(directory.join("index-cache"))
            .arg("--neighbor-form")
            .arg(format!("FalloutNV.esm:{id}"))
            .arg("--output")
            .arg(&path);
        run_logged(
            command,
            &directory.join(format!("goodsprings-edge-{direction}.log")),
        )?;
        let report = json_file(&path)?;
        if report["edge_comparison"]["direction"] != direction
            || report["edge_comparison"]["samples_compared"] != 33
            || report["neighbor_surface"]["coordinates"] != json!(coordinates)
            || report["surface"]["world"] != report["neighbor_surface"]["world"]
            || report["neighbor"]["integrity_failures"] != 0
            || report["neighbor"]["link_failures"] != 0
        {
            return Err("Goodsprings edge provenance or coordinates differ".into());
        }
        let neighbor = report["neighbor"]["landscapes"]
            .as_array()
            .ok_or("Missing neighbor LAND")?;
        if neighbor.len() != 1 {
            return Err("Ambiguous neighbor LAND".into());
        }
        let land = &neighbor[0];
        let plugin = land["source_plugin"]
            .as_str()
            .ok_or("Missing neighbor plugin")?;
        if digest(&install.join("Data").join(plugin))? != land["source_sha256"] {
            return Err("Neighbor source identity differs".into());
        }
        boundaries.push(json!({"direction":direction, "neighbor_cell":report["neighbor"]["cell"]["key"],
            "neighbor_coordinates":coordinates, "neighbor_land":land["key"], "neighbor_source_sha256":land["source_sha256"],
            "neighbor_decoded_sha256":land["decoded_sha256"], "comparison":report["edge_comparison"], "report_sha256":digest(&path)?}));
    }
    Ok(json!({"synthetic":synthetic, "boundaries":boundaries,
        "boundary_scope":"Four source-model Goodsprings edges; neighboring fields are strict reads, not independently compared retail stitching or inheritance"}))
}
