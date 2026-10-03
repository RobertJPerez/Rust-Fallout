//! Compare exact source fields, including unknown bytes, without float tolerances.
use super::Result;
use fallout_data::{baseline, cache, terrain::TerrainReport};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};

#[derive(Serialize)]
pub struct Difference {
    pub body: String,
    pub fields: Vec<String>,
}
#[derive(Serialize)]
pub struct Comparison {
    pub all_equal: bool,
    pub records_compared: usize,
    pub unique_bodies: usize,
    pub height_grids_compared: usize,
    pub source_meshes_compared: usize,
    pub oracle_report_sha256: String,
    pub oracle_binary_sha256: String,
    pub differences: Vec<Difference>,
    pub scope: &'static str,
}

fn paths(left: &Value, right: &Value, path: &str, out: &mut Vec<String>) {
    if left == right || out.len() >= 16 {
        return;
    }
    match (left, right) {
        (Value::Object(left), Value::Object(right)) => {
            let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
            for key in keys {
                match (left.get(key), right.get(key)) {
                    (Some(left), Some(right)) => paths(left, right, &format!("{path}.{key}"), out),
                    _ if out.len() < 16 => out.push(format!("{path}.{key}")),
                    _ => {}
                }
            }
        }
        (Value::Array(left), Value::Array(right)) if left.len() == right.len() => {
            for (i, (left, right)) in left.iter().zip(right).enumerate() {
                paths(left, right, &format!("{path}[{i}]"), out);
            }
        }
        _ => out.push(path.into()),
    }
}

pub fn compare(
    report: &TerrainReport,
    oracle_path: &Path,
    root: &Path,
    source_tree: &Path,
    include_heights: bool,
    include_geometry: bool,
) -> Result<Comparison> {
    let mut bytes = Vec::new();
    baseline::open_source(oracle_path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("Terrain oracle report exceeds input budget".into());
    }
    let report_hash = format!("{:x}", Sha256::digest(&bytes));
    let oracle: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
    let binary = oracle["oracle_binary_sha256"]
        .as_str()
        .ok_or("Missing terrain oracle binary digest")?;
    if binary.len() != 64 || !binary.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err("Malformed terrain oracle binary digest".into());
    }
    let mut rows = BTreeMap::new();
    for row in oracle["files"]
        .as_array()
        .ok_or("Missing terrain oracle files")?
    {
        let name = row["file"]
            .as_str()
            .ok_or("Missing terrain oracle body name")?;
        if rows.insert(name, row).is_some() {
            return Err("Duplicate terrain oracle body".into());
        }
    }
    let mut expected = BTreeMap::new();
    let mut records_compared = 0;
    for entry in std::iter::once(&report.cell)
        .chain(&report.world_chain)
        .chain(&report.landscapes)
        .chain(
            report
                .texture_dependencies
                .iter()
                .flat_map(|textures| &textures.records),
        )
    {
        let Some(fields) = &entry.fields else {
            continue;
        };
        let receipt = entry
            .body_cache
            .as_ref()
            .ok_or("Terrain comparison requires cached decoded bodies")?;
        let (_, bytes) = cache::read_verified(
            root,
            source_tree,
            &receipt.manifest.identity,
            64 * 1024 * 1024 + 4,
        )?
        .ok_or("Terrain comparison body lacks a commit marker")?;
        if bytes.get(..4) != Some(entry.header.kind.as_slice())
            || bytes.len() as u64 != receipt.manifest.bytes
            || format!("{:x}", Sha256::digest(&bytes)) != receipt.manifest.sha256
            || Some(format!("{:x}", Sha256::digest(&bytes[4..]))) != entry.decoded_sha256
        {
            return Err("Terrain body no longer matches source receipt".into());
        }
        expected.insert(
            format!("{}.blob", receipt.key),
            (
                serde_json::to_value(fields)?,
                &receipt.manifest.sha256,
                if include_heights {
                    Some(match fields {
                        fallout_data::terrain::Fields::Land(land) => land
                            .heights
                            .as_ref()
                            .map(|field| fallout_data::terrain::heights::reconstruct(&field.value))
                            .transpose()?
                            .map(serde_json::to_value)
                            .transpose()?
                            .unwrap_or(Value::Null),
                        _ => Value::Null,
                    })
                } else {
                    None
                },
                if include_geometry {
                    Some(match fields {
                        fallout_data::terrain::Fields::Land(land) if land.heights.is_some() => {
                            serde_json::to_value(fallout_data::terrain::mesh::build(land, 0)?)?
                        }
                        _ => Value::Null,
                    })
                } else {
                    None
                },
            ),
        );
        records_compared += 1;
    }
    if expected.is_empty() || rows.len() != expected.len() {
        return Err("Terrain oracle input set differs".into());
    }
    let mut differences = Vec::new();
    let mut height_grids_compared = 0;
    let mut source_meshes_compared = 0;
    for (name, (fields, digest, height_grid, geometry)) in &expected {
        let row = rows
            .get(name.as_str())
            .ok_or("Terrain oracle omitted a selected body")?;
        if row["sha256"] != digest.as_str() {
            return Err("Terrain oracle body digest differs".into());
        }
        let mut differing = Vec::new();
        paths(fields, &row["fields"], "fields", &mut differing);
        if let Some(height_grid) = height_grid {
            let actual = row
                .get("height_grid")
                .ok_or("Terrain oracle omitted height projection; run it with --heights")?;
            paths(height_grid, actual, "height_grid", &mut differing);
            if !height_grid.is_null() {
                height_grids_compared += 1;
            }
        }
        if let Some(geometry) = geometry {
            let actual = row
                .get("source_mesh")
                .ok_or("Terrain oracle omitted geometry; run it with --geometry")?;
            paths(geometry, actual, "source_mesh", &mut differing);
            if !geometry.is_null() {
                source_meshes_compared += 1;
            }
        }
        if !differing.is_empty() {
            differences.push(Difference {
                body: name.clone(),
                fields: differing,
            });
        }
    }
    Ok(Comparison {
        all_equal: differences.is_empty(),
        records_compared,
        unique_bodies: expected.len(),
        height_grids_compared,
        source_meshes_compared,
        oracle_report_sha256: report_hash,
        oracle_binary_sha256: binary.into(),
        differences,
        scope: "Exact selected source fields and optional height bits/source-local unhidden mesh against authored C++; CELL hide-mask semantics, compression, overrides, retail terrain behavior and gameplay are not independently compared",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn changed_height_bits_and_missing_padding_report_exact_field_paths() {
        let original = json!({"heights":{"offset_bits":2147483648u32,"unused":[1,2,3]}});
        let changed = json!({"heights":{"offset_bits":0,"unused":[1,2]}});
        let mut out = Vec::new();
        paths(&original, &changed, "fields", &mut out);
        assert_eq!(out, ["fields.heights.offset_bits", "fields.heights.unused"]);
    }
}
