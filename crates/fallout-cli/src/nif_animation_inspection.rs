//! Exact source inspection/native projection comparison, with raw byte strings.
use crate::Result;
use fallout_data::{
    baseline,
    nif_animation::{self, Animation, Limits, keyframe, spline},
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
    #[serde(skip_serializing_if = "Option::is_none")]
    keys: Option<keyframe::Catalogue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    splines: Option<spline::Catalogue>,
    error: Option<String>,
    comparison: Option<&'static str>,
}
#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    animation_branch: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    keyframe_branch: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    spline_branch: Option<&'static str>,
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
    if let Some(keys) = &actual.keys
        && !compare_keys(&keys.blocks, &expected["keys"])?
    {
        return Err("oracle transform-key block identity/span/hash or source fields differ".into());
    }
    if let Some(splines) = &actual.splines
        && !compare_splines(&splines.blocks, &expected["splines"])?
    {
        return Err("oracle spline-source block identity/span/hash or source fields differ".into());
    }
    Ok(())
}

// Compare at most one small key value at a time. Constructing a second complete
// JSON key catalogue would duplicate the admitted source arrays during comparison.
fn exact_object(value: &Value, fields: usize) -> bool {
    value.as_object().is_some_and(|v| v.len() == fields)
}
fn compare_key_array<T: Serialize>(keys: &[T], expected: &Value) -> Result<bool> {
    let Some(expected) = expected.as_array() else {
        return Ok(false);
    };
    if keys.len() != expected.len() {
        return Ok(false);
    }
    for (key, expected) in keys.iter().zip(expected) {
        if serde_json::to_value(key)? != *expected {
            return Ok(false);
        }
    }
    Ok(true)
}
fn compare_group<const N: usize>(group: &keyframe::Group<N>, expected: &Value) -> Result<bool>
where
    [u32; N]: Serialize,
{
    Ok(exact_object(expected, 3)
        && expected["declared_keys"].as_u64() == Some(u64::from(group.declared_keys))
        && expected["key_type"] == serde_json::to_value(group.key_type)?
        && compare_key_array(&group.keys, &expected["keys"])?)
}
fn compare_keys(blocks: &[keyframe::Block], expected: &Value) -> Result<bool> {
    let Some(expected) = expected.as_array() else {
        return Ok(false);
    };
    if blocks.len() != expected.len() {
        return Ok(false);
    }
    for (block, row) in blocks.iter().zip(expected) {
        if !exact_object(row, 6)
            || row["block"].as_u64() != Some(u64::from(block.block))
            || row["block_type"].as_str() != Some(block.block_type)
            || row["offset"].as_u64() != Some(block.offset as u64)
            || row["bytes"].as_u64() != Some(block.bytes as u64)
            || row["sha256"].as_str() != Some(block.sha256.as_str())
        {
            return Ok(false);
        }
        let data = &row["data"];
        if !exact_object(data, 4)
            || data["declared_rotation_keys"].as_u64()
                != Some(u64::from(block.data.declared_rotation_keys))
        {
            return Ok(false);
        }
        let rotation = &data["rotation"];
        let equal = match &block.data.rotation {
            keyframe::Rotation::Absent => {
                exact_object(rotation, 1) && rotation["layout"] == "absent"
            }
            keyframe::Rotation::Quaternion { key_type, keys } => {
                exact_object(rotation, 3)
                    && rotation["layout"] == "quaternion"
                    && rotation["key_type"].as_u64() == Some(u64::from(*key_type))
                    && compare_key_array(keys, &rotation["keys"])?
            }
            keyframe::Rotation::Xyz { axes } => {
                if !exact_object(rotation, 2) || rotation["layout"] != "xyz" {
                    return Ok(false);
                }
                let Some(expected_axes) = rotation["axes"].as_array() else {
                    return Ok(false);
                };
                if expected_axes.len() != 3 {
                    return Ok(false);
                }
                let mut equal = true;
                for (axis, expected) in axes.iter().zip(expected_axes) {
                    equal &= compare_group(axis, expected)?;
                }
                equal
            }
        };
        if !equal
            || !compare_group(&block.data.translations, &data["translations"])?
            || !compare_group(&block.data.scales, &data["scales"])?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_splines(blocks: &[spline::Block], expected: &Value) -> Result<bool> {
    let Some(expected) = expected.as_array() else {
        return Ok(false);
    };
    if blocks.len() != expected.len() {
        return Ok(false);
    }
    for (block, row) in blocks.iter().zip(expected) {
        if !exact_object(row, 6)
            || row["block"].as_u64() != Some(u64::from(block.block))
            || row["block_type"].as_str() != Some(block.block_type)
            || row["offset"].as_u64() != Some(block.offset as u64)
            || row["bytes"].as_u64() != Some(block.bytes as u64)
            || row["sha256"].as_str() != Some(block.sha256.as_str())
        {
            return Ok(false);
        }
        let equal = match &block.data {
            spline::Data::ControlPoints {
                declared_float_count,
                float_bits,
                declared_compact_count,
                compact,
            } => {
                exact_object(&row["data"], 5)
                    && row["data"]["kind"] == "control_points"
                    && row["data"]["declared_float_count"].as_u64()
                        == Some(u64::from(*declared_float_count))
                    && row["data"]["declared_compact_count"].as_u64()
                        == Some(u64::from(*declared_compact_count))
                    && compare_key_array(float_bits, &row["data"]["float_bits"])?
                    && compare_key_array(compact, &row["data"]["compact"])?
            }
            // Both alternatives are bounded fixed-size scalar products.
            _ => serde_json::to_value(&block.data)? == row["data"],
        };
        if !equal {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn inspect(
    input: &Path,
    oracle_path: Option<&Path>,
    include_keyframes: bool,
    include_splines: bool,
) -> Result<Report> {
    inspect_with_work(
        input,
        oracle_path,
        include_keyframes,
        include_splines,
        16_000_000,
        16_000_000,
    )
}
#[cfg(test)]
fn inspect_with_key_work(
    input: &Path,
    oracle_path: Option<&Path>,
    include_keyframes: bool,
    key_work: usize,
) -> Result<Report> {
    inspect_with_work(input, oracle_path, include_keyframes, false, key_work, 0)
}
fn inspect_with_work(
    input: &Path,
    oracle_path: Option<&Path>,
    include_keyframes: bool,
    include_splines: bool,
    mut key_work: usize,
    mut spline_work: usize,
) -> Result<Report> {
    let include_keyframes = include_keyframes || include_splines;
    let mut report = Report {
        schema_version: if include_splines {
            3
        } else if include_keyframes {
            2
        } else {
            1
        },
        animation_branch: "nv-four-source-classes",
        keyframe_branch: include_keyframes.then_some("nv-transform-data-source"),
        spline_branch: include_splines.then_some("nv-compact-transform-source"),
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
        if document["schema_version"] != report.schema_version
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
        if include_keyframes
            && (document["keyframe_branch"] != "nv-transform-data-source"
                || document["raw_keyframe_counts_checked"] != true)
        {
            return Err("oracle transform-key source/provenance contract is missing".into());
        }
        if include_splines
            && (document["spline_branch"] != "nv-compact-transform-source"
                || document["raw_spline_counts_checked"] != true)
        {
            return Err("oracle spline-source/provenance contract is missing".into());
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
            keys: None,
            splines: None,
            error: None,
            comparison: None,
        };
        let limits = Limits {
            array_bytes: remaining.min(128 * 1024 * 1024),
            ..Default::default()
        };
        let key_limits = keyframe::Limits {
            animation: limits,
            array_bytes: remaining.min(128 * 1024 * 1024),
            max_combined_retained_bytes: remaining,
            key_work,
        };
        let decoded = if include_splines {
            spline::decode_with_limits(
                &bytes,
                &row.input.display().to_string(),
                spline::Limits {
                    keyframes: key_limits,
                    array_bytes: remaining.min(128 * 1024 * 1024),
                    spline_work,
                },
            )
            .map(|(index, source)| {
                (
                    index,
                    source.animation,
                    Some(source.keys),
                    Some(source.splines),
                )
            })
        } else if include_keyframes {
            keyframe::decode_with_limits(&bytes, &row.input.display().to_string(), key_limits)
                .map(|(index, source)| (index, source.animation, Some(source.keys), None))
        } else {
            nif_animation::decode_with_limits(&bytes, &row.input.display().to_string(), limits)
                .map(|(index, animation)| (index, animation, None, None))
        };
        match decoded {
            Ok((index, animation, keys, splines)) => {
                remaining = remaining
                    .checked_sub(animation.retained_bytes)
                    .ok_or("aggregate animation catalogue budget exceeded")?;
                if let Some(keys) = &keys {
                    key_work = key_work
                        .checked_sub(keys.work_units)
                        .ok_or("aggregate transform-key work budget exceeded")?;
                    remaining = remaining
                        .checked_sub(keys.retained_bytes)
                        .ok_or("aggregate transform-key catalogue budget exceeded")?;
                    for block in &keys.blocks {
                        *report
                            .block_counts
                            .entry(block.block_type.into())
                            .or_default() += 1;
                    }
                }
                if let Some(splines) = &splines {
                    spline_work = spline_work
                        .checked_sub(splines.work_units)
                        .ok_or("aggregate spline-source work budget exceeded")?;
                    remaining = remaining
                        .checked_sub(splines.retained_bytes)
                        .ok_or("aggregate spline-source catalogue budget exceeded")?;
                    for block in &splines.blocks {
                        *report
                            .block_counts
                            .entry(block.block_type.into())
                            .or_default() += 1;
                    }
                    report.unresolved_dependencies += splines.dependencies.len();
                }
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
                row.keys = keys;
                row.splines = splines;
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
                // Failed decodes cannot publish an exact successful work receipt.
                // Conservatively debit their entire remaining key allowance so
                // later failed rows cannot repeatedly spend the same budget.
                if include_keyframes {
                    key_work = 0;
                }
                if include_splines {
                    spline_work = 0;
                }
                row.error = Some(e.to_string());
                report.failures += 1;
            }
        }
        report.files.push(row);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Inputs(PathBuf);
    impl Drop for Inputs {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }
    fn source(payload: &[u8]) -> Vec<u8> {
        source_of(payload, "NiTransformData")
    }
    fn source_of(payload: &[u8], kind: &str) -> Vec<u8> {
        let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        bytes.extend(words(&[0x1402_0007]));
        bytes.push(1);
        bytes.extend(words(&[11, 1, 34]));
        bytes.extend([0; 3]);
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(words(&[kind.len() as u32]));
        bytes.extend(kind.as_bytes());
        bytes.extend(0u16.to_le_bytes());
        bytes.extend(words(&[payload.len() as u32, 0, 0, 0]));
        bytes.extend(payload);
        bytes.extend(words(&[1, 0]));
        bytes
    }
    fn inputs(payloads: &[Vec<u8>]) -> Inputs {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../local")
            .join(format!(
                "asset-key-inspector-test-{}-{unique}",
                std::process::id()
            ));
        std::fs::create_dir_all(directory.parent().unwrap()).unwrap();
        std::fs::create_dir(&directory).unwrap();
        let result = Inputs(directory);
        for (id, payload) in payloads.iter().enumerate() {
            std::fs::write(result.0.join(format!("{id}.kf")), source(payload)).unwrap();
        }
        result
    }
    #[test]
    fn batch_work_exact_and_one_under_use_one_budget() {
        let inputs = inputs(&[words(&[0, 0, 0]), words(&[0, 0, 0])]);
        let exact = inspect_with_key_work(&inputs.0, None, true, 8).unwrap();
        assert_eq!(exact.failures, 0);
        assert_eq!(
            exact
                .files
                .iter()
                .map(|f| f.keys.as_ref().unwrap().work_units)
                .sum::<usize>(),
            8
        );
        let under = inspect_with_key_work(&inputs.0, None, true, 7).unwrap();
        assert_eq!(under.failures, 1);
        assert_eq!(under.files[0].keys.as_ref().unwrap().work_units, 4);
        assert!(
            under.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("transform-key work budget exceeded")
        );
    }
    #[test]
    fn failed_source_exhausts_remaining_key_work_and_schema1_stays_opaque() {
        let inputs = inputs(&[words(&[0, 1, 6]), words(&[0, 0, 0])]);
        let failed = inspect_with_key_work(&inputs.0, None, true, 100).unwrap();
        assert_eq!(failed.failures, 2);
        assert!(
            failed.files[0]
                .error
                .as_ref()
                .unwrap()
                .contains("unadmitted transform key-group tag 6")
        );
        assert!(
            failed.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("transform-key work budget exceeded")
        );
        let old = inspect_with_key_work(&inputs.0, None, false, 0).unwrap();
        assert_eq!(old.failures, 0);
        let json = serde_json::to_value(old).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert!(json.get("keyframe_branch").is_none());
        assert!(json["files"][0].get("keys").is_none());
    }
    #[test]
    fn exact_key_comparison_rejects_missing_and_extra_source_fields() {
        let (_, decoded) = keyframe::decode(&source(&words(&[0, 0, 0])), "empty.kf").unwrap();
        let expected = serde_json::to_value(&decoded.keys.blocks).unwrap();
        assert!(compare_keys(&decoded.keys.blocks, &expected).unwrap());
        let mut extra = expected.clone();
        extra[0]["data"]["invented_default"] = json!(0);
        assert!(!compare_keys(&decoded.keys.blocks, &extra).unwrap());
        let mut extra = expected.clone();
        extra[0]["data"]["rotation"]["key_type"] = json!(0);
        assert!(!compare_keys(&decoded.keys.blocks, &extra).unwrap());
        let mut missing = expected;
        missing[0].as_object_mut().unwrap().remove("sha256");
        assert!(!compare_keys(&decoded.keys.blocks, &missing).unwrap());
    }
    #[test]
    fn spline_batch_work_exact_one_under_and_failed_admission() {
        let inputs = inputs(&[]);
        for id in 0..2 {
            std::fs::write(
                inputs.0.join(format!("{id}.kf")),
                source_of(&words(&[0, 0]), "NiBSplineData"),
            )
            .unwrap();
        }
        let exact = inspect_with_work(&inputs.0, None, false, true, 0, 6).unwrap();
        assert_eq!(exact.failures, 0);
        assert_eq!(exact.schema_version, 3);
        assert_eq!(
            exact
                .files
                .iter()
                .map(|f| f.splines.as_ref().unwrap().work_units)
                .sum::<usize>(),
            6
        );
        assert!(exact.files.iter().all(|f| f.keys.is_some()));
        let under = inspect_with_work(&inputs.0, None, false, true, 0, 5).unwrap();
        assert_eq!(under.failures, 1);
        assert!(
            under.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("spline-source work budget exceeded")
        );
        std::fs::write(
            inputs.0.join("0.kf"),
            source_of(&words(&[1, 0x7fc0_0001, 0]), "NiBSplineData"),
        )
        .unwrap();
        let failed = inspect_with_work(&inputs.0, None, false, true, 0, 100).unwrap();
        assert_eq!(failed.failures, 2);
        assert!(
            failed.files[0]
                .error
                .as_ref()
                .unwrap()
                .contains("nonfinite")
        );
        assert!(
            failed.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("spline-source work budget exceeded")
        );
        let old = inspect_with_work(&inputs.0, None, true, false, 0, 0).unwrap();
        assert_eq!(old.failures, 0);
        let old = serde_json::to_value(old).unwrap();
        assert_eq!(old["schema_version"], 2);
        assert!(old.get("spline_branch").is_none() && old["files"][0].get("splines").is_none());
    }
    #[test]
    fn exact_spline_array_comparison_rejects_sign_bits_order_and_extra_fields() {
        let mut points = words(&[2, 0x8000_0000, 1, 3]);
        for value in [i16::MIN, -1, 1] {
            points.extend(value.to_le_bytes());
        }
        let (_, decoded) =
            spline::decode(&source_of(&points, "NiBSplineData"), "points.kf").unwrap();
        let expected = serde_json::to_value(&decoded.splines.blocks).unwrap();
        assert!(compare_splines(&decoded.splines.blocks, &expected).unwrap());
        let mut sign = expected.clone();
        sign[0]["data"]["compact"][0] = json!(32768);
        assert!(!compare_splines(&decoded.splines.blocks, &sign).unwrap());
        let mut bits = expected.clone();
        bits[0]["data"]["float_bits"][0] = json!(0);
        assert!(!compare_splines(&decoded.splines.blocks, &bits).unwrap());
        let mut order = expected.clone();
        order[0]["data"]["compact"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert!(!compare_splines(&decoded.splines.blocks, &order).unwrap());
        let mut extra = expected;
        extra[0]["data"]["evaluated"] = json!(true);
        assert!(!compare_splines(&decoded.splines.blocks, &extra).unwrap());
    }
}
