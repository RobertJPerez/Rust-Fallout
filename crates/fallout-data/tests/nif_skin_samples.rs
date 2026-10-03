//! Explicit local evidence capture through the existing production archive reader.
//! This is ignored without original data; missing data is never a passing gate.
use fallout_data::{archive::NvArchive, nif};
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
    let mut decoded_bytes = 0u64;
    const BUDGET: u64 = 64 * 1024 * 1024;
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
                    "tuple": [index.version, index.user_version, index.bethesda_version], "block_counts": index.block_counts}));
                selected += 1;
            }
        }
    }
    let report = json!({"schema_version": 1, "scope": "bounded deterministic path/category sample; source field comparison runs separately",
        "decoded_scan_bytes": decoded_bytes, "budget_bytes": BUDGET, "samples": rows, "findings": attempts, "runtime_ready": false});
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
