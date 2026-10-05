//! Explicit local evidence capture through the existing production archive reader.
//! This is ignored without original data; missing data is never a passing gate.
use fallout_data::{
    archive::NvArchive,
    nif,
    nif_skin::{self, Data, pose},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::PathBuf};

fn category(path: &[u8]) -> Option<&'static str> {
    let normalized = String::from_utf8_lossy(path)
        .replace('\\', "/")
        .to_ascii_lowercase();
    if !normalized.ends_with(".nif") {
        return None;
    }
    if normalized.contains("1stperson") || normalized.contains("firstperson") {
        Some("first_person")
    } else if normalized.contains("/creatures/") {
        Some("creature")
    } else if normalized.contains("/characters/") {
        Some("character")
    } else if normalized.contains("/armor/") {
        Some("equipment")
    } else {
        None
    }
}

fn raw_weight_summary(skin: &nif_skin::Skin) -> (usize, usize) {
    let mut entries = 0usize;
    let mut peak_per_vertex = 0usize;
    for block in &skin.blocks {
        if let Data::SkinData { bones, .. } = &block.data {
            let mut per_vertex = BTreeMap::<u16, usize>::new();
            for weight in bones.iter().flat_map(|bone| &bone.weights) {
                entries += 1;
                *per_vertex.entry(weight.vertex).or_default() += 1;
            }
            peak_per_vertex = peak_per_vertex.max(per_vertex.values().copied().max().unwrap_or(0));
        }
    }
    (entries, peak_per_vertex)
}

#[test]
#[ignore = "Requires explicit ASSET_SKIN_ARCHIVES JSON and new ASSET_SKIN_EVIDENCE directory under this worktree's local directory"]
fn capture_bounded_original_skin_sources() {
    let archives: Vec<PathBuf> = serde_json::from_str(
        &std::env::var("ASSET_SKIN_ARCHIVES").expect("explicit archive list required"),
    )
    .unwrap();
    assert!(!archives.is_empty() && archives.len() <= 16);
    let output = PathBuf::from(
        std::env::var("ASSET_SKIN_EVIDENCE").expect("explicit new evidence path required"),
    );
    let local = std::env::current_dir()
        .unwrap()
        .join("../../local")
        .canonicalize()
        .unwrap();
    // Cargo runs integration tests in the package directory. Check the resolved
    // destination parent before creating files, and require an entirely new run.
    let parent = output.parent().unwrap().canonicalize().unwrap();
    assert!(
        parent.starts_with(&local),
        "evidence must stay in the assigned local tree"
    );
    std::fs::create_dir(&output).unwrap();
    let inputs = output.join("inputs");
    std::fs::create_dir(&inputs).unwrap();
    let mut rows = Vec::new();
    let mut attempts = Vec::new();
    let mut decoded_structures = BTreeMap::<String, usize>::new();
    let mut posed_structures = BTreeMap::<String, usize>::new();
    let mut pose_attempts = 0usize;
    let mut decoded_bytes = 0u64;
    const BUDGET: u64 = 64 * 1024 * 1024;
    const MAX_POSE_ATTEMPTS: usize = 32;
    for archive_path in archives {
        let archive = NvArchive::open(&archive_path).unwrap();
        let mut categories = BTreeMap::new();
        for (id, entry) in archive.backend().entries_with_ids() {
            if let Some(path) = entry.path() {
                let bytes: &[u8] = path.as_ref();
                if let Some(category) = category(bytes) {
                    categories
                        .entry(category)
                        .or_insert_with(Vec::new)
                        .push((bytes.to_vec(), id));
                }
            }
        }
        for (category, mut members) in categories {
            members.sort_by(|left, right| left.0.cmp(&right.0));
            let mut selected = 0;
            for (path, id) in members.into_iter().take(48) {
                if selected == 2 || rows.len() == 64 || decoded_bytes == BUDGET {
                    break;
                }
                let bytes = match archive.read_bounded(id, BUDGET - decoded_bytes) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        attempts.push(json!({"archive": archive_path, "path_bytes": path, "category": category, "error": error.to_string()}));
                        continue;
                    }
                };
                decoded_bytes += bytes.len() as u64;
                let source = format!(
                    "{}:{}",
                    archive_path.display(),
                    String::from_utf8_lossy(&path)
                );
                let index = match nif::inspect(&bytes, &source) {
                    Ok(index) => index,
                    Err(error) => {
                        attempts.push(json!({"archive": archive_path, "path_bytes": path, "category": category, "error": error.to_string()}));
                        continue;
                    }
                };
                if !index.block_counts.keys().any(|name| {
                    matches!(
                        name.as_str(),
                        "NiSkinData" | "NiSkinInstance" | "BSDismemberSkinInstance"
                    )
                }) {
                    continue;
                }
                let decoded_skin = match nif_skin::decode(&bytes, &source) {
                    Ok((_, skin)) => Some(skin),
                    Err(error) => {
                        attempts.push(json!({"archive": archive_path, "path_bytes": path,
                            "category": category, "skin_decode_error": error.to_string()}));
                        None
                    }
                };
                let mut instance_counts = BTreeMap::<String, usize>::new();
                let mut owner_count = 0usize;
                let mut raw_influence_records = 0usize;
                let mut max_raw_influences_per_vertex = 0usize;
                let mut pose_evaluation = None;
                let mut pose_refusals = Vec::new();
                if let Some(skin) = &decoded_skin {
                    owner_count = skin.owners.len();
                    for block in &skin.blocks {
                        if matches!(
                            block.block_type.as_str(),
                            "NiSkinInstance" | "BSDismemberSkinInstance"
                        ) {
                            *instance_counts.entry(block.block_type.clone()).or_default() += 1;
                            *decoded_structures
                                .entry(block.block_type.clone())
                                .or_default() += 1;
                        }
                    }
                    (raw_influence_records, max_raw_influences_per_vertex) =
                        raw_weight_summary(skin);
                    for owner in skin.owners.iter().take(2) {
                        if pose_attempts >= MAX_POSE_ATTEMPTS {
                            break;
                        }
                        let Some(instance) = skin.blocks.iter().find(|b| b.block == owner.instance)
                        else {
                            continue;
                        };
                        let instance_type = instance.block_type.clone();
                        if !matches!(
                            instance_type.as_str(),
                            "NiSkinInstance" | "BSDismemberSkinInstance"
                        ) || posed_structures.contains_key(&instance_type)
                        {
                            continue;
                        }
                        pose_attempts += 1;
                        match pose::evaluate(
                            &bytes,
                            &source,
                            pose::Request {
                                geometry: owner.geometry,
                                weights: pose::WeightPolicy::PreserveRawNonnegative,
                            },
                            Default::default(),
                        ) {
                            Ok(evaluation) => {
                                *posed_structures.entry(instance_type.clone()).or_default() += 1;
                                pose_evaluation = Some(json!({
                                    "instance_type": instance_type,
                                    "geometry": evaluation.geometry,
                                    "instance": evaluation.instance,
                                    "skeleton_root": evaluation.skeleton_root,
                                    "palette_bones": evaluation.palette.len(),
                                    "vertices": evaluation.positions.len(),
                                    "weight_sums": evaluation.weight_sums.len(),
                                    "weight_policy": evaluation.weights,
                                    "retail_behavior_verified": false
                                }));
                                break;
                            }
                            Err(error) => pose_refusals.push(json!({
                                "geometry": owner.geometry,
                                "instance": owner.instance,
                                "instance_type": instance_type,
                                "error": error.to_string()
                            })),
                        }
                    }
                }
                let digest = format!("{:x}", Sha256::digest(&bytes));
                let filename = format!("{:03}_{digest}.blob", rows.len());
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(inputs.join(&filename))
                    .unwrap();
                file.write_all(&bytes).unwrap();
                rows.push(json!({"archive": archive_path, "entry_index": id.index(), "path_bytes": path,
                    "category": category, "file": filename, "sha256": digest, "decoded_bytes": bytes.len(),
                    "tuple": [index.version, index.user_version, index.bethesda_version], "block_counts": index.block_counts,
                    "decoded_skin_structures": instance_counts, "skin_owner_count": owner_count,
                    "raw_influence_records": raw_influence_records,
                    "max_raw_influences_per_vertex": max_raw_influences_per_vertex,
                    "pose_evaluation": pose_evaluation, "pose_refusals": pose_refusals}));
                selected += 1;
            }
        }
    }
    for expected in ["NiSkinInstance", "BSDismemberSkinInstance"] {
        assert!(
            decoded_structures.get(expected).copied().unwrap_or(0) > 0,
            "no source skin decoded with structure {expected}"
        );
        assert!(
            posed_structures.get(expected).copied().unwrap_or(0) > 0,
            "no source pose completed with structure {expected}; refusals are retained"
        );
    }
    let report = json!({"schema_version": 2,
        "scope": "bounded deterministic local archive sample through the existing source skin decoder and stored-pose evaluator; no GPU or gameplay proof",
        "decoded_scan_bytes": decoded_bytes, "budget_bytes": BUDGET,
        "decoded_skin_structures": decoded_structures, "posed_skin_structures": posed_structures,
        "pose_attempts": pose_attempts, "samples": rows, "findings": attempts, "runtime_ready": false});
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("source-manifest.json"))
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &report).unwrap();
    writeln!(file).unwrap();
    assert!(
        !rows.is_empty(),
        "no skin sources sampled; see retained findings"
    );
    println!(
        "Captured {} original skin sources across {} decoded scan bytes; {} retained selection findings",
        rows.len(),
        decoded_bytes,
        attempts.len()
    );
}
