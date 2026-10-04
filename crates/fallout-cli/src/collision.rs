//! Offline collision inspection. The runtime decoder has no dependency on nifly.
use crate::Result;
use fallout_data::{
    baseline,
    nif_collision::{self, Collision, Data},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
pub struct FileReport {
    input: PathBuf,
    decoded_bytes: usize,
    sha256: String,
    tuple: Option<[u32; 3]>,
    collision: Option<Collision>,
    error: Option<String>,
    comparison: Option<String>,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    input: PathBuf,
    oracle_report_sha256: Option<String>,
    oracle_binary_sha256: Option<String>,
    pub failures: usize,
    files: Vec<FileReport>,
    block_counts: BTreeMap<String, usize>,
    packed_vertices: usize,
    packed_triangles: usize,
    convex_vertices: usize,
    mopp_bytes: usize,
    comparison: &'static str,
    physics_ready: bool,
}

/// JSON stores source f32 as floating numbers and indices as integers. Turn only
/// floating numbers into their IEEE bits, matching the independent oracle's output.
fn float_bits(value: &mut Value) {
    match value {
        Value::Number(n) if n.is_f64() => {
            *value = Value::from((n.as_f64().expect("float") as f32).to_bits())
        }
        Value::Array(values) => {
            for value in values {
                float_bits(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                float_bits(value);
            }
        }
        _ => {}
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = baseline::open_source(path)?.take(limit + 1);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds input byte budget", path.display()).into());
    }
    Ok(bytes)
}

fn compare(row: &FileReport, expected: &Value) -> Result<()> {
    if expected.get("sha256").and_then(Value::as_str) != Some(row.sha256.as_str())
        || expected.get("decoded_bytes").and_then(Value::as_u64) != Some(row.decoded_bytes as u64)
    {
        return Err("oracle source digest or byte length differs".into());
    }
    let tuple = row.tuple.ok_or("decoded tuple missing")?;
    for (name, value) in ["version", "user_version", "bethesda_version"]
        .into_iter()
        .zip(tuple)
    {
        if expected.get(name).and_then(Value::as_u64) != Some(u64::from(value)) {
            return Err(format!("oracle {name} differs").into());
        }
    }
    let mut actual =
        serde_json::to_value(&row.collision.as_ref().ok_or("collision missing")?.blocks)?;
    float_bits(&mut actual);
    let oracle = expected
        .get("collisions")
        .ok_or("oracle collision fields missing")?;
    if &actual != oracle {
        let actual = actual.as_array().ok_or("collision array missing")?;
        let oracle = oracle.as_array().ok_or("oracle collision array missing")?;
        if actual.len() != oracle.len() {
            return Err("oracle collision block count differs".into());
        }
        for (left, right) in actual.iter().zip(oracle) {
            if left != right {
                return Err(
                    format!("oracle collision fields differ at block {}", left["block"]).into(),
                );
            }
        }
        return Err("oracle collision projection differs".into());
    }
    Ok(())
}

pub fn inspect(input: &Path, oracle_path: Option<&Path>) -> Result<Report> {
    let mut report = Report {
        schema_version: 1,
        input: input.to_path_buf(),
        oracle_report_sha256: None,
        oracle_binary_sha256: None,
        failures: 0,
        files: vec![],
        block_counts: BTreeMap::new(),
        packed_vertices: 0,
        packed_triangles: 0,
        convex_vertices: 0,
        mopp_bytes: 0,
        comparison: "stored f32 bits, source and block SHA256, topology, references and other projected fields exact; graph checks are separate synthetic tests",
        physics_ready: false,
    };
    let oracle = if let Some(path) = oracle_path {
        let bytes = read_bounded(path, 64 * 1024 * 1024)?;
        report.oracle_report_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        let document: Value = serde_json::from_slice(&bytes)?;
        if document["float_encoding"] != "ieee754-binary32-bits" {
            return Err("oracle float encoding is missing or unsupported".into());
        }
        report.oracle_binary_sha256 = document["oracle_binary_sha256"].as_str().map(str::to_owned);
        let mut rows = BTreeMap::new();
        for row in document["files"]
            .as_array()
            .ok_or("oracle file array missing")?
        {
            let name = row["file"]
                .as_str()
                .ok_or("oracle file name missing")?
                .to_owned();
            if rows.insert(name, row.clone()).is_some() {
                return Err("duplicate oracle file name".into());
            }
        }
        Some(rows)
    } else {
        None
    };
    let mut paths = if input.is_dir() {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(input)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("nif") || e.eq_ignore_ascii_case("blob")
                })
            {
                paths.push(entry.path());
                if paths.len() > 10_000 {
                    return Err("collision inspection file count budget exceeded".into());
                }
            }
        }
        paths
    } else {
        vec![input.to_path_buf()]
    };
    paths.sort();
    if paths.is_empty() {
        return Err("collision inspection found no inputs".into());
    }
    if let Some(oracle) = &oracle
        && oracle.len() != paths.len()
    {
        return Err("oracle file count differs from inspected inputs".into());
    }
    // Bound the whole retained report as well as individual model decoders.
    let mut remaining = 256 * 1024 * 1024;
    for path in paths {
        let bytes = read_bounded(&path, 64 * 1024 * 1024)?;
        let mut row = FileReport {
            input: path,
            decoded_bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            tuple: None,
            collision: None,
            error: None,
            comparison: None,
        };
        match nif_collision::decode_with_limits(
            &bytes,
            &row.input.display().to_string(),
            nif_collision::Limits {
                array_bytes: remaining,
                ..Default::default()
            },
        ) {
            Ok((index, collision)) => {
                row.tuple = Some([index.version, index.user_version, index.bethesda_version]);
                for block in &collision.blocks {
                    *report
                        .block_counts
                        .entry(block.block_type.clone())
                        .or_default() += 1;
                    match &block.data {
                        Data::PackedData {
                            vertex_count,
                            triangles,
                            ..
                        } => {
                            report.packed_vertices += *vertex_count as usize;
                            report.packed_triangles += triangles.len();
                        }
                        Data::ConvexVertices { vertices, .. } => {
                            report.convex_vertices += vertices.len()
                        }
                        Data::Mopp { code, .. } => report.mopp_bytes += code.len(),
                        _ => {}
                    }
                }
                let estimate = remaining
                    .checked_sub(collision.retained_bytes)
                    .ok_or("aggregate collision report budget exceeded")?;
                remaining = estimate;
                row.collision = Some(collision);
                if let Some(oracle) = &oracle {
                    let result = row
                        .input
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| oracle.get(name))
                        .ok_or("matching oracle input missing")
                        .map_err(Into::into)
                        .and_then(|expected| compare(&row, expected));
                    match result {
                        Ok(()) => row.comparison = Some("all_equal".into()),
                        Err(error) => {
                            row.comparison = Some("different".into());
                            row.error = Some(error.to_string());
                        }
                    }
                }
            }
            Err(error) => row.error = Some(error.to_string()),
        }
        report.failures += usize::from(row.error.is_some());
        report.files.push(row);
    }
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    reference: std::num::NonZeroU64,
    body_blocks: Vec<u32>,
    attachment_rows: [[f64; 4]; 3],
    units: fallout_runtime::physics::EngineeringUnits,
    ray: Option<fallout_runtime::physics::Ray>,
    overlap: Option<SphereRequest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphereRequest {
    center: [f64; 3],
    radius: f64,
}
#[derive(Serialize)]
pub struct QueryReport {
    source_sha256: String,
    request_sha256: String,
    units: fallout_runtime::physics::EngineeringUnits,
    primitive_count: usize,
    ray_hits: Vec<fallout_runtime::physics::Hit>,
    overlap_hits: Vec<fallout_runtime::physics::Hit>,
    query_semantics: &'static str,
    faithful_ready: bool,
}

/// One real source file, existing decoder, explicit authored attachment frame and
/// immutable query geometry. An unsupported selected body fails the entire request.
pub fn query(input: &Path, request_path: &Path) -> Result<QueryReport> {
    use fallout_runtime::{
        identity::ReferenceId,
        physics::{BodyPlacement, QueryBudget, QueryLimits, StaticScene},
    };
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let request_bytes = read_bounded(request_path, 1024 * 1024)?;
    let request: QueryRequest = serde_json::from_slice(&request_bytes)?;
    if request.body_blocks.is_empty()
        || request.body_blocks.len() > 10_000
        || (request.ray.is_none() && request.overlap.is_none())
    {
        return Err("collision request needs 1..10000 bodies and a ray or overlap".into());
    }
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let (_, collision) = nif_collision::decode(&bytes, &input.display().to_string())?;
    let placements: Vec<_> = request
        .body_blocks
        .iter()
        .map(|&body_block| BodyPlacement {
            reference: ReferenceId(request.reference),
            source_sha256: digest,
            body_block,
            attachment_to_source: fallout_data::coordinates::Affine {
                rows: request.attachment_rows,
            },
        })
        .collect();
    let scene = StaticScene::build(
        &collision,
        &placements,
        request.units,
        QueryLimits::default(),
    )?;
    Ok(QueryReport {
        source_sha256: format!("{:x}", Sha256::digest(&bytes)),
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        units: request.units,
        primitive_count: scene.primitive_count(),
        ray_hits: request
            .ray
            .map(|ray| scene.ray_cast(ray, QueryBudget::default()))
            .transpose()?
            .unwrap_or_default(),
        overlap_hits: request
            .overlap
            .map(|s| scene.overlap_sphere(s.center, s.radius, QueryBudget::default()))
            .transpose()?
            .unwrap_or_default(),
        query_semantics: "authored core geometry; frozen bodies; all source filters included; two-sided triangles; convex/packed shell margins excluded; source axes retained",
        faithful_ready: scene.faithful_ready(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallout_data::nif_collision::{Block, Triangle};
    #[test]
    fn bit_projection_keeps_negative_zero_distinct_from_indices() {
        let mut value = serde_json::json!({"float":-0.0f32,"index":7u32,"values":[1.0f32,0.1f32]});
        float_bits(&mut value);
        assert_eq!(value["float"], 0x8000_0000u32);
        assert_eq!(value["index"], 7u32);
        assert_eq!(value["values"][1], 0.1f32.to_bits());
    }

    #[test]
    fn independent_comparison_rejects_changed_identity_fields_and_triangle_winding() {
        let row = FileReport {
            input: "authored.nif".into(),
            decoded_bytes: 123,
            sha256: "source digest".into(),
            tuple: Some([0x1402_0007, 11, 34]),
            collision: Some(Collision {
                blocks: vec![Block {
                    block: 2,
                    block_type: "hkPackedNiTriStripsData".into(),
                    source_offset: 80,
                    source_bytes: 41,
                    source_sha256: "block digest".into(),
                    data: Data::PackedData {
                        triangles: vec![Triangle {
                            indices: [2, 0, 1],
                            welding: 0xe123,
                        }],
                        vertex_count: 3,
                        compressed: false,
                        vertices: vec![[-0., 0., 0.], [1., 0., 0.], [0., 2., 0.]],
                        compressed_words: vec![],
                        subparts: vec![],
                    },
                }],
                unsupported_blocks: BTreeMap::new(),
                unsupported_links: vec![],
                shape_order: vec![2],
                retained_bytes: 0,
                units: "authored",
                physics_ready: false,
            }),
            error: None,
            comparison: None,
        };
        // Written independently, with known IEEE encodings rather than a
        // serialization of the row being tested.
        let expected = serde_json::json!({
            "sha256": "source digest", "decoded_bytes": 123,
            "version": 0x1402_0007u32, "user_version": 11, "bethesda_version": 34,
            "collisions": [{
                "block":2, "block_type":"hkPackedNiTriStripsData",
                "source_offset":80, "source_bytes":41, "source_sha256":"block digest",
                "data": {
                    "kind":"packed_data",
                    "triangles":[{"indices":[2,0,1],"welding":0xe123}],
                    "vertex_count":3, "compressed":false,
                    "vertices":[[0x8000_0000u32,0,0],[0x3f80_0000u32,0,0],[0,0x4000_0000u32,0]],
                    "compressed_words":[], "subparts":[]
                }
            }]
        });
        compare(&row, &expected).unwrap();
        for (pointer, changed) in [
            ("/sha256", Value::from("wrong source")),
            ("/decoded_bytes", Value::from(124)),
            ("/bethesda_version", Value::from(33)),
            ("/collisions/0/source_sha256", Value::from("wrong block")),
            ("/collisions/0/source_offset", Value::from(81)),
            ("/collisions/0/data/vertices/0/0", Value::from(0)),
            (
                "/collisions/0/data/triangles/0/indices",
                serde_json::json!([2, 1, 0]),
            ),
            ("/collisions/0/data/triangles/0/welding", Value::from(0)),
            ("/collisions", serde_json::json!([])),
        ] {
            let mut mutated = expected.clone();
            *mutated.pointer_mut(pointer).unwrap() = changed;
            assert!(compare(&row, &mutated).is_err(), "{pointer} was ignored");
        }
    }
}
