//! Source inspection and exact offline comparison; no animation evaluation.
use crate::Result;
use fallout_data::{
    baseline,
    nif_skin::{self, Skin},
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
    skin: Option<Skin>,
    error: Option<String>,
    comparison: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    float_encoding: &'static str,
    input: PathBuf,
    oracle_report_sha256: Option<String>,
    oracle_binary_sha256: Option<String>,
    pub failures: usize,
    files: Vec<FileReport>,
    block_counts: BTreeMap<String, usize>,
    owners: usize,
    unresolved_dependencies: usize,
    comparison: &'static str,
    runtime_ready: bool,
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut source = baseline::open_source(path)?.take(limit + 1);
    let mut bytes = Vec::new();
    source.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("skin inspection input exceeds byte budget".into());
    }
    Ok(bytes)
}

fn compare(actual: &FileReport, expected: &Value) -> Result<()> {
    if expected["sha256"].as_str() != Some(actual.sha256.as_str())
        || expected["decoded_bytes"].as_u64() != Some(actual.decoded_bytes as u64)
    {
        return Err("oracle source digest or byte length differs".into());
    }
    let tuple = actual.tuple.ok_or("skin tuple missing")?;
    for (field, value) in ["version", "user_version", "bethesda_version"]
        .into_iter()
        .zip(tuple)
    {
        if expected[field].as_u64() != Some(u64::from(value)) {
            return Err(format!("oracle {field} differs").into());
        }
    }
    let skin = actual.skin.as_ref().ok_or("decoded skin missing")?;
    let blocks = serde_json::to_value(&skin.blocks)?;
    if blocks != expected["skins"] {
        let actual = blocks.as_array().ok_or("skin array missing")?;
        let expected = expected["skins"]
            .as_array()
            .ok_or("oracle skin array missing")?;
        if actual.len() != expected.len() {
            return Err("oracle skin block count differs".into());
        }
        for (left, right) in actual.iter().zip(expected) {
            if left != right {
                return Err(format!("oracle skin fields differ at block {}", left["block"]).into());
            }
        }
        return Err("oracle skin projection differs".into());
    }
    if serde_json::to_value(&skin.owners)? != expected["owners"] {
        return Err("oracle skin owner associations differ".into());
    }
    Ok(())
}

pub fn inspect(input: &Path, oracle_path: Option<&Path>) -> Result<Report> {
    let mut report = Report {
        schema_version: 1,
        float_encoding: "ieee754-binary32-bits",
        input: input.into(),
        oracle_report_sha256: None,
        oracle_binary_sha256: None,
        failures: 0,
        files: Vec::new(),
        block_counts: BTreeMap::new(),
        owners: 0,
        unresolved_dependencies: 0,
        comparison: "source/block digests and spans, raw presence/count fields, float bits, integer/link arrays and geometry owner associations exact; graph validation is separate",
        runtime_ready: false,
    };
    let oracle = if let Some(path) = oracle_path {
        let bytes = read_bounded(path, 128 * 1024 * 1024)?;
        report.oracle_report_sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        let document: Value = serde_json::from_slice(&bytes)?;
        if document["schema_version"] != 1
            || document["float_encoding"] != "ieee754-binary32-bits"
            || document["nifly_revision"] != "cca0a770094bb962fb28ea1fec5ea903e68fda8e"
            || document["prepare_data_called"] != false
            || document["raw_presence_and_vertex_counts_checked"] != true
        {
            return Err("oracle provenance or raw-field comparison contract is missing".into());
        }
        let digest = document["oracle_binary_sha256"]
            .as_str()
            .ok_or("oracle binary digest missing")?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid oracle binary digest".into());
        }
        report.oracle_binary_sha256 = Some(digest.into());
        let mut files = BTreeMap::new();
        for row in document["files"].as_array().ok_or("oracle files missing")? {
            let name = row["file"]
                .as_str()
                .ok_or("oracle file name missing")?
                .to_owned();
            if files.insert(name, row.clone()).is_some() {
                return Err("duplicate oracle file name".into());
            }
        }
        Some(files)
    } else {
        None
    };
    let mut paths = if input.is_dir() {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(input)? {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && entry.path().extension().is_some_and(|extension| {
                    ["nif", "kf", "blob"]
                        .iter()
                        .any(|suffix| extension.eq_ignore_ascii_case(suffix))
                })
            {
                paths.push(entry.path());
                if paths.len() > 10_000 {
                    return Err("skin inspection file count budget exceeded".into());
                }
            }
        }
        paths
    } else {
        vec![input.to_path_buf()]
    };
    paths.sort();
    if paths.is_empty() {
        return Err("skin inspection found no inputs".into());
    }
    if let Some(oracle) = &oracle
        && oracle.len() != paths.len()
    {
        return Err("oracle file count differs from inspected inputs".into());
    }
    let mut remaining = 256 * 1024 * 1024;
    for path in paths {
        let bytes = read_bounded(&path, 64 * 1024 * 1024)?;
        let mut row = FileReport {
            input: path,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            decoded_bytes: bytes.len(),
            tuple: None,
            skin: None,
            error: None,
            comparison: None,
        };
        match nif_skin::decode_with_limits(
            &bytes,
            &row.input.display().to_string(),
            nif_skin::Limits {
                skin_array_bytes: remaining.min(128 * 1024 * 1024),
                ..Default::default()
            },
        ) {
            Ok((index, skin)) => {
                row.tuple = Some([index.version, index.user_version, index.bethesda_version]);
                remaining = remaining
                    .checked_sub(skin.retained_bytes)
                    .ok_or("aggregate skin catalogue budget exceeded")?;
                for block in &skin.blocks {
                    *report
                        .block_counts
                        .entry(block.block_type.clone())
                        .or_default() += 1;
                }
                report.owners += skin.owners.len();
                report.unresolved_dependencies += skin.dependencies.len();
                row.skin = Some(skin);
                if let Some(oracle) = &oracle {
                    let result = row
                        .input
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| oracle.get(name))
                        .ok_or("matching oracle file missing")
                        .map_err(Into::into)
                        .and_then(|expected| compare(&row, expected));
                    match result {
                        Ok(()) => row.comparison = Some("all_equal"),
                        Err(error) => {
                            row.comparison = Some("different");
                            row.error = Some(error.to_string());
                            report.failures += 1;
                        }
                    }
                }
            }
            Err(error) => {
                row.error = Some(error.to_string());
                report.failures += 1;
            }
        }
        report.files.push(row);
    }
    Ok(report)
}
