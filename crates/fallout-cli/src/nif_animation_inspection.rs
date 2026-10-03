//! Exact source inspection/native projection comparison, with raw byte strings.
use crate::Result;
use fallout_data::{
    baseline,
    nif_animation::{self, Animation, Limits},
};
use serde::Serialize;
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
    sha256: String,
    decoded_bytes: usize,
    tuple: Option<[u32; 3]>,
    strings: Option<Vec<Vec<u8>>>,
    container_block_counts: Option<BTreeMap<String, usize>>,
    animation: Option<Animation>,
    error: Option<String>,
    comparison: Option<&'static str>,
}
#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    animation_branch: &'static str,
    float_encoding: &'static str,
    string_encoding: &'static str,
    input: PathBuf,
    oracle_report_sha256: Option<String>,
    oracle_binary_sha256: Option<String>,
    pub failures: usize,
    files: Vec<FileReport>,
    block_counts: BTreeMap<String, usize>,
    unresolved_dependencies: usize,
    diagnostics: usize,
    comparison: &'static str,
    runtime_ready: bool,
}
fn bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = baseline::open_source(path)?.take(limit + 1);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("animation inspection input exceeds byte budget".into());
    }
    Ok(bytes)
}
fn compare(actual: &FileReport, expected: &Value) -> Result<()> {
    if expected["sha256"].as_str() != Some(actual.sha256.as_str())
        || expected["decoded_bytes"].as_u64() != Some(actual.decoded_bytes as u64)
    {
        return Err("oracle animation source digest/length differs".into());
    }
    let tuple = actual.tuple.ok_or("animation tuple missing")?;
    for (field, value) in ["version", "user_version", "bethesda_version"]
        .into_iter()
        .zip(tuple)
    {
        if expected[field].as_u64() != Some(u64::from(value)) {
            return Err(format!("oracle animation {field} differs").into());
        }
    }
    let strings = actual
        .strings
        .as_ref()
        .ok_or("animation raw strings missing")?;
    if serde_json::to_value(strings)? != expected["strings"] {
        return Err("oracle raw string table bytes/order differ".into());
    }
    let normalizations = strings.iter().filter(|s| s.last() == Some(&0)).count();
    if expected["trailing_nul_normalizations"].as_u64() != Some(normalizations as u64) {
        return Err("oracle trailing-NUL supplement count differs".into());
    }
    let animation = actual
        .animation
        .as_ref()
        .ok_or("animation source missing")?;
    if serde_json::to_value(&animation.blocks)? != expected["animations"] {
        return Err("oracle animation block identity/span/hash or source fields differ".into());
    }
    Ok(())
}

pub fn inspect(input: &Path, oracle_path: Option<&Path>) -> Result<Report> {
    let mut report = Report {
        schema_version: 1,
        animation_branch: "nv-four-source-classes",
        float_encoding: "ieee754-binary32-bits",
        string_encoding: "raw-byte-arrays",
        input: input.into(),
        oracle_report_sha256: None,
        oracle_binary_sha256: None,
        failures: 0,
        files: Vec::new(),
        block_counts: BTreeMap::new(),
        unresolved_dependencies: 0,
        diagnostics: 0,
        comparison: "exact source/block hashes and spans, ordered fields/links/counts, finite float bits and raw global string bytes; evaluation/name binding unverified",
        runtime_ready: false,
    };
    let oracle = if let Some(path) = oracle_path {
        let bytes = bounded(path, 64 * 1024 * 1024)?;
        report.oracle_report_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        let document: Value = serde_json::from_slice(&bytes)?;
        if document["schema_version"] != 1
            || document["animation_branch"] != report.animation_branch
            || document["float_encoding"] != report.float_encoding
            || document["string_encoding"] != report.string_encoding
            || document["nifly_revision"] != "cca0a770094bb962fb28ea1fec5ea903e68fda8e"
            || document["prepare_data_called"] != false
            || document["raw_string_table_checked"] != true
            || document["raw_count_fields_checked"] != true
            || document["runtime_ready"] != false
        {
            return Err("oracle animation source/provenance contract is missing".into());
        }
        let hash = document["oracle_binary_sha256"]
            .as_str()
            .ok_or("oracle binary digest missing")?;
        if hash.len() != 64 || !hash.bytes().all(|v| v.is_ascii_hexdigit()) {
            return Err("invalid oracle binary digest".into());
        }
        report.oracle_binary_sha256 = Some(hash.into());
        Some(document)
    } else {
        None
    };
    // Store indices into the one retained JSON tree, not cloned whole file rows.
    let mut expected = BTreeMap::new();
    if let Some(document) = &oracle {
        let files = document["files"]
            .as_array()
            .ok_or("oracle animation files missing")?;
        if files.len() > 10_000 {
            return Err("oracle animation file count exceeds budget".into());
        }
        for (index, row) in files.iter().enumerate() {
            let name = row["file"]
                .as_str()
                .ok_or("oracle animation file name missing")?;
            if expected.insert(name, index).is_some() {
                return Err("duplicate oracle animation file name".into());
            }
        }
    }
    let mut paths = Vec::new();
    if input.is_dir() {
        for entry in std::fs::read_dir(input)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().is_some_and(|ext| {
                    ["nif", "kf", "blob"]
                        .iter()
                        .any(|s| ext.eq_ignore_ascii_case(s))
                })
            {
                if paths.len() == 10_000 {
                    return Err("animation inspection file count budget exceeded".into());
                }
                paths.push(entry.path());
            }
        }
    } else {
        paths.push(input.to_path_buf());
    }
    paths.sort();
    if paths.is_empty() {
        return Err("animation inspection found no inputs".into());
    }
    if oracle.is_some() && expected.len() != paths.len() {
        return Err("oracle animation file count differs".into());
    }
    let mut remaining: usize = 256 * 1024 * 1024;
    let row_charge = paths
        .len()
        .checked_mul(std::mem::size_of::<FileReport>() + 64)
        .ok_or("animation report storage overflow")?;
    remaining = remaining
        .checked_sub(row_charge)
        .ok_or("animation report storage budget exceeded")?;
    report.files = Vec::with_capacity(paths.len());
    for path in paths {
        remaining = remaining
            .checked_sub(path.as_os_str().len() * std::mem::size_of::<u16>())
            .ok_or("animation report path storage budget exceeded")?;
        let bytes = bounded(&path, 64 * 1024 * 1024)?;
        let mut row = FileReport {
            input: path,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            decoded_bytes: bytes.len(),
            tuple: None,
            strings: None,
            container_block_counts: None,
            animation: None,
            error: None,
            comparison: None,
        };
        let limits = Limits {
            array_bytes: remaining.min(128 * 1024 * 1024),
            ..Default::default()
        };
        match nif_animation::decode_with_limits(&bytes, &row.input.display().to_string(), limits) {
            Ok((index, animation)) => {
                remaining = remaining
                    .checked_sub(animation.retained_bytes)
                    .ok_or("aggregate animation catalogue budget exceeded")?;
                row.tuple = Some([index.version, index.user_version, index.bethesda_version]);
                row.strings = Some(index.strings);
                row.container_block_counts = Some(index.block_counts);
                for block in &animation.blocks {
                    *report
                        .block_counts
                        .entry(block.block_type.into())
                        .or_default() += 1;
                }
                report.unresolved_dependencies += animation.dependencies.len();
                report.diagnostics += animation.diagnostics.len();
                row.animation = Some(animation);
                if let Some(document) = &oracle {
                    let result = row
                        .input
                        .file_name()
                        .and_then(|s| s.to_str())
                        .and_then(|name| expected.get(name))
                        .ok_or("matching animation oracle file missing")
                        .map_err(Into::into)
                        .and_then(|&i| compare(&row, &document["files"][i]));
                    match result {
                        Ok(()) => row.comparison = Some("all_equal"),
                        Err(e) => {
                            row.error = Some(e.to_string());
                            row.comparison = Some("different");
                            report.failures += 1;
                        }
                    }
                }
            }
            Err(e) => {
                row.error = Some(e.to_string());
                report.failures += 1;
            }
        }
        report.files.push(row);
    }
    Ok(report)
}
