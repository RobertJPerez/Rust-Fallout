//! Exact source inspection/native projection comparison, with raw byte strings.
use crate::Result;
use fallout_data::{
    baseline,
    nif_animation::{self, Animation, Limits, boolean, keyframe, sampling, spline},
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
    #[serde(skip_serializing_if = "Option::is_none")]
    spline_components: Option<spline::components::Catalogue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    engineering_sample: Option<sampling::Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bool_interpolators: Option<boolean::Catalogue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bool_keys: Option<boolean::keyframes::Catalogue>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    spline_component_branch: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    engineering_sampling_contract: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bool_interpolator_branch: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bool_key_branch: Option<&'static str>,
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
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum SampleChannel {
    Translation,
    Scale,
}
pub struct SampleRequest {
    pub block: u32,
    pub channel: SampleChannel,
    pub time: f64,
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
    if let Some(components) = &actual.spline_components
        && !compare_fixed_blocks(&components.blocks, &expected["spline_components"])?
    {
        return Err(
            "oracle spline-component block identity/span/hash or source fields differ".into(),
        );
    }
    if let Some(booleans) = &actual.bool_interpolators
        && !compare_fixed_blocks(&booleans.blocks, &expected["bool_interpolators"])?
    {
        return Err(
            "oracle Boolean interpolator identity/span/hash or source fields differ".into(),
        );
    }
    if let Some(keys) = &actual.bool_keys
        && !compare_boolean_keys(&keys.blocks, &expected["bool_keys"])?
    {
        return Err("oracle Boolean-key identity/span/hash or ordered source fields differ".into());
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

fn compare_fixed_blocks<T: Serialize>(blocks: &[T], expected: &Value) -> Result<bool> {
    let Some(expected) = expected.as_array() else {
        return Ok(false);
    };
    if blocks.len() != expected.len() {
        return Ok(false);
    }
    for (block, row) in blocks.iter().zip(expected) {
        // Each block is a bounded fixed-size source product.
        if serde_json::to_value(block)? != *row {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_boolean_keys(blocks: &[boolean::keyframes::Block], expected: &Value) -> Result<bool> {
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
            || !exact_object(&row["data"], 3)
            || row["data"]["declared_keys"].as_u64() != Some(u64::from(block.data.declared_keys))
            || row["data"]["key_type"] != serde_json::to_value(block.data.key_type)?
            || !compare_key_array(&block.data.keys, &row["data"]["keys"])?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
#[derive(Clone, Copy, Default)]
pub struct SourceOptions {
    pub include_keyframes: bool,
    pub include_splines: bool,
    pub include_spline_components: bool,
    pub include_bool_interpolators: bool,
    pub include_bool_keys: bool,
}
struct Work {
    keys: usize,
    splines: usize,
    components: usize,
    booleans: usize,
    boolean_keys: usize,
    diagnostic_bytes: usize,
}
impl Default for Work {
    fn default() -> Self {
        Self {
            keys: 16_000_000,
            splines: 16_000_000,
            components: 16_000_000,
            booleans: 16_000_000,
            boolean_keys: 16_000_000,
            diagnostic_bytes: 0,
        }
    }
}

pub fn inspect(
    input: &Path,
    oracle_path: Option<&Path>,
    mut options: SourceOptions,
    sample: Option<SampleRequest>,
) -> Result<Report> {
    if sample
        .as_ref()
        .is_some_and(|request| !request.time.is_finite())
    {
        return Err("engineering sampling requires a finite explicit source time".into());
    }
    if sample.is_some() && input.is_dir() {
        return Err("engineering sampling requires one explicit input file".into());
    }
    options.include_keyframes |= sample.is_some();
    let mut report = inspect_with_work(
        input,
        oracle_path,
        options,
        Work {
            diagnostic_bytes: if sample.is_some() { 64 } else { 0 },
            ..Default::default()
        },
    )?;
    if let Some(request) = sample {
        report.engineering_sampling_contract = Some(sampling::CONTRACT);
        let mut budget = sampling::Budget::new(sampling::Limits::default());
        for row in &mut report.files {
            // Source decoding/comparison findings stay separate; only a
            // successfully decoded source can be a sampling diagnostic input.
            let Some(keys) = &row.keys else { continue };
            let result = (|| -> Result<_> {
                let mut selected = None;
                for block in &keys.blocks {
                    budget.source_block_visit()?;
                    if block.block == request.block {
                        selected = Some(block);
                        break;
                    }
                }
                let block = selected.ok_or(
                    "selected block is not in the decoded NiTransformData source catalogue",
                )?;
                let channel = match request.channel {
                    SampleChannel::Translation => sampling::Channel::Translation,
                    SampleChannel::Scale => sampling::Channel::Scale,
                };
                Ok(sampling::evaluate(
                    block,
                    channel,
                    request.time,
                    &mut budget,
                )?)
            })();
            match result {
                Ok(sample) => row.engineering_sample = Some(sample),
                Err(error) => {
                    if row.error.is_none() {
                        report.failures += 1;
                    }
                    let reason = format!("engineering sampling diagnostic failed: {error}");
                    row.error = Some(match row.error.take() {
                        Some(source_error) => format!("{source_error}; {reason}"),
                        None => reason,
                    });
                }
            }
        }
    }
    Ok(report)
}
#[cfg(test)]
fn inspect_with_key_work(
    input: &Path,
    oracle_path: Option<&Path>,
    include_keyframes: bool,
    key_work: usize,
) -> Result<Report> {
    inspect_with_work(
        input,
        oracle_path,
        SourceOptions {
            include_keyframes,
            ..Default::default()
        },
        Work {
            keys: key_work,
            splines: 0,
            components: 0,
            booleans: 0,
            boolean_keys: 0,
            diagnostic_bytes: 0,
        },
    )
}
fn inspect_with_work(
    input: &Path,
    oracle_path: Option<&Path>,
    options: SourceOptions,
    work: Work,
) -> Result<Report> {
    let include_bool_keys = options.include_bool_keys;
    let include_booleans = options.include_bool_interpolators || include_bool_keys;
    let include_components = options.include_spline_components || include_booleans;
    let include_splines = options.include_splines || include_components;
    let include_keyframes = options.include_keyframes || include_splines;
    let Work {
        keys: mut key_work,
        splines: mut spline_work,
        components: mut component_work,
        booleans: mut boolean_work,
        boolean_keys: mut boolean_key_work,
        diagnostic_bytes: reserved_diagnostic_bytes,
    } = work;
    let mut report = Report {
        schema_version: if include_bool_keys {
            6
        } else if include_booleans {
            5
        } else if include_components {
            4
        } else if include_splines {
            3
        } else if include_keyframes {
            2
        } else {
            1
        },
        animation_branch: "nv-four-source-classes",
        keyframe_branch: include_keyframes.then_some("nv-transform-data-source"),
        spline_branch: include_splines.then_some("nv-compact-transform-source"),
        spline_component_branch: include_components.then_some("nv-compact-components-source"),
        engineering_sampling_contract: None,
        bool_interpolator_branch: include_booleans.then_some("nv-bool-interpolator-source"),
        bool_key_branch: include_bool_keys.then_some("nv-bool-constant-key-source"),
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
        if include_components
            && (document["spline_component_branch"] != "nv-compact-components-source"
                || document["raw_component_fields_checked"] != true)
        {
            return Err("oracle spline-component source/provenance contract is missing".into());
        }
        if include_booleans
            && (document["bool_interpolator_branch"] != "nv-bool-interpolator-source"
                || document["raw_bool_fields_checked"] != true)
        {
            return Err("oracle Boolean interpolator source/provenance contract is missing".into());
        }
        if include_bool_keys
            && (document["bool_key_branch"] != "nv-bool-constant-key-source"
                || document["raw_bool_key_counts_checked"] != true)
        {
            return Err("oracle Boolean-key source/provenance contract is missing".into());
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
    remaining = remaining
        .checked_sub(reserved_diagnostic_bytes)
        .ok_or("engineering diagnostic storage budget exceeded")?;
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
            spline_components: None,
            engineering_sample: None,
            bool_interpolators: None,
            bool_keys: None,
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
        let spline_limits = spline::Limits {
            keyframes: key_limits,
            array_bytes: remaining.min(128 * 1024 * 1024),
            spline_work,
        };
        let component_limits = spline::components::Limits {
            splines: spline_limits,
            array_bytes: remaining.min(128 * 1024 * 1024),
            component_work,
        };
        let boolean_limits = boolean::Limits {
            components: component_limits,
            array_bytes: remaining.min(128 * 1024 * 1024),
            boolean_work,
        };
        let decoded =
            if include_bool_keys {
                boolean::keyframes::decode_with_limits(
                    &bytes,
                    &row.input.display().to_string(),
                    boolean::keyframes::Limits {
                        booleans: boolean_limits,
                        array_bytes: remaining.min(128 * 1024 * 1024),
                        key_work: boolean_key_work,
                    },
                )
                .map(|(index, decoded)| {
                    (
                        index,
                        decoded.source.source.source.animation,
                        Some(decoded.source.source.source.keys),
                        Some(decoded.source.source.source.splines),
                        Some(decoded.source.source.components),
                        Some(decoded.source.booleans),
                        Some(decoded.keys),
                    )
                })
            } else if include_booleans {
                boolean::decode_with_limits(
                    &bytes,
                    &row.input.display().to_string(),
                    boolean::Limits {
                        components: component_limits,
                        array_bytes: remaining.min(128 * 1024 * 1024),
                        boolean_work,
                    },
                )
                .map(|(index, decoded)| {
                    (
                        index,
                        decoded.source.source.animation,
                        Some(decoded.source.source.keys),
                        Some(decoded.source.source.splines),
                        Some(decoded.source.components),
                        Some(decoded.booleans),
                        None,
                    )
                })
            } else if include_components {
                spline::components::decode_with_limits(
                    &bytes,
                    &row.input.display().to_string(),
                    spline::components::Limits {
                        splines: spline_limits,
                        array_bytes: remaining.min(128 * 1024 * 1024),
                        component_work,
                    },
                )
                .map(|(index, decoded)| {
                    (
                        index,
                        decoded.source.animation,
                        Some(decoded.source.keys),
                        Some(decoded.source.splines),
                        Some(decoded.components),
                        None,
                        None,
                    )
                })
            } else if include_splines {
                spline::decode_with_limits(&bytes, &row.input.display().to_string(), spline_limits)
                    .map(|(index, source)| {
                        (
                            index,
                            source.animation,
                            Some(source.keys),
                            Some(source.splines),
                            None,
                            None,
                            None,
                        )
                    })
            } else if include_keyframes {
                keyframe::decode_with_limits(&bytes, &row.input.display().to_string(), key_limits)
                    .map(|(index, source)| {
                        (
                            index,
                            source.animation,
                            Some(source.keys),
                            None,
                            None,
                            None,
                            None,
                        )
                    })
            } else {
                nif_animation::decode_with_limits(&bytes, &row.input.display().to_string(), limits)
                    .map(|(index, animation)| (index, animation, None, None, None, None, None))
            };
        match decoded {
            Ok((index, animation, keys, splines, components, booleans, boolean_keys)) => {
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
                if let Some(components) = &components {
                    component_work = component_work
                        .checked_sub(components.work_units)
                        .ok_or("aggregate spline-component work budget exceeded")?;
                    remaining = remaining
                        .checked_sub(components.retained_bytes)
                        .ok_or("aggregate spline-component catalogue budget exceeded")?;
                    for block in &components.blocks {
                        *report
                            .block_counts
                            .entry(block.block_type.into())
                            .or_default() += 1;
                    }
                    report.unresolved_dependencies += components.dependencies.len();
                }
                if let Some(booleans) = &booleans {
                    boolean_work = boolean_work
                        .checked_sub(booleans.work_units)
                        .ok_or("aggregate Boolean source work budget exceeded")?;
                    remaining = remaining
                        .checked_sub(booleans.retained_bytes)
                        .ok_or("aggregate Boolean source catalogue budget exceeded")?;
                    for block in &booleans.blocks {
                        *report
                            .block_counts
                            .entry(block.block_type.into())
                            .or_default() += 1;
                    }
                    report.unresolved_dependencies += booleans.dependencies.len();
                }
                if let Some(keys) = &boolean_keys {
                    boolean_key_work = boolean_key_work
                        .checked_sub(keys.work_units)
                        .ok_or("aggregate Boolean-key work budget exceeded")?;
                    remaining = remaining
                        .checked_sub(keys.retained_bytes)
                        .ok_or("aggregate Boolean-key catalogue budget exceeded")?;
                    for block in &keys.blocks {
                        *report
                            .block_counts
                            .entry(block.block_type.into())
                            .or_default() += 1;
                    }
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
                row.spline_components = components;
                row.bool_interpolators = booleans;
                row.bool_keys = boolean_keys;
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
                if include_components {
                    component_work = 0;
                }
                if include_booleans {
                    boolean_work = 0;
                }
                if include_bool_keys {
                    boolean_key_work = 0;
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

    #[test]
    fn boolean_key_batch_work_and_failed_row_debit_keep_schema5_opaque() {
        let inputs = inputs(&[]);
        let mut raw = words(&[1, 5, 0x8000_0000]);
        raw.push(255);
        for id in 0..2 {
            std::fs::write(
                inputs.0.join(format!("{id}.kf")),
                source_of(&raw, "NiBoolData"),
            )
            .unwrap();
        }
        let options = SourceOptions {
            include_bool_keys: true,
            ..Default::default()
        };
        let limits = |boolean_keys| Work {
            boolean_keys,
            ..Default::default()
        };
        let exact = inspect_with_work(&inputs.0, None, options, limits(10)).unwrap();
        assert_eq!(exact.failures, 0);
        assert_eq!(exact.schema_version, 6);
        assert!(
            exact
                .files
                .iter()
                .all(|f| f.bool_keys.as_ref().unwrap().blocks[0].data.keys[0].raw_value == 255)
        );
        let under = inspect_with_work(&inputs.0, None, options, limits(9)).unwrap();
        assert_eq!(under.failures, 1);
        assert!(
            under.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("Boolean-key work budget exceeded")
        );
        std::fs::write(
            inputs.0.join("0.kf"),
            source_of(&words(&[1, 1]), "NiBoolData"),
        )
        .unwrap();
        let failed = inspect_with_work(&inputs.0, None, options, limits(100)).unwrap();
        assert_eq!(failed.failures, 2);
        assert!(
            failed.files[0]
                .error
                .as_ref()
                .unwrap()
                .contains("key type 1 unsupported")
        );
        assert!(
            failed.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("Boolean-key work budget exceeded")
        );
        let old = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_bool_interpolators: true,
                ..Default::default()
            },
            limits(0),
        )
        .unwrap();
        assert_eq!(old.failures, 0);
        assert_eq!(old.schema_version, 5);
        let old = serde_json::to_value(old).unwrap();
        assert!(old.get("bool_key_branch").is_none());
        assert!(old["files"][0].get("bool_keys").is_none());
    }
    #[test]
    fn boolean_key_comparison_rejects_byte_conversion_order_and_added_semantics() {
        let mut raw = words(&[2, 5, 0x4000_0000]);
        raw.push(2);
        raw.extend(words(&[0x8000_0000]));
        raw.push(255);
        let (_, source) =
            boolean::keyframes::decode(&source_of(&raw, "NiBoolData"), "keys.kf").unwrap();
        let blocks = &source.keys.blocks;
        let expected = serde_json::to_value(blocks).unwrap();
        assert!(compare_boolean_keys(blocks, &expected).unwrap());
        let mut value = expected.clone();
        value[0]["data"]["keys"][0]["raw_value"] = json!(1);
        assert!(!compare_boolean_keys(blocks, &value).unwrap());
        let mut order = expected.clone();
        order[0]["data"]["keys"].as_array_mut().unwrap().reverse();
        assert!(!compare_boolean_keys(blocks, &order).unwrap());
        let mut time = expected.clone();
        time[0]["data"]["keys"][1]["time_bits"] = json!(0);
        assert!(!compare_boolean_keys(blocks, &time).unwrap());
        let mut tag = expected.clone();
        tag[0]["data"]["key_type"] = json!(1);
        assert!(!compare_boolean_keys(blocks, &tag).unwrap());
        let mut extra = expected;
        extra[0]["data"]["truth"] = json!(true);
        assert!(!compare_boolean_keys(blocks, &extra).unwrap());
    }

    #[test]
    fn boolean_batch_work_and_failed_row_debit_preserve_earlier_schema() {
        let inputs = inputs(&[]);
        for id in 0..2 {
            std::fs::write(
                inputs.0.join(format!("{id}.kf")),
                source_of(&[2, 255, 255, 255, 255], "NiBoolTimelineInterpolator"),
            )
            .unwrap();
        }
        let limits = |booleans| Work {
            booleans,
            ..Default::default()
        };
        let exact = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_bool_interpolators: true,
                ..Default::default()
            },
            limits(10),
        )
        .unwrap();
        assert_eq!(exact.schema_version, 5);
        assert_eq!(exact.failures, 0);
        assert!(exact.files.iter().all(|f| {
            f.bool_interpolators.as_ref().unwrap().blocks[0]
                .data
                .raw_value
                == 2
        }));
        let under = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_bool_interpolators: true,
                ..Default::default()
            },
            limits(9),
        )
        .unwrap();
        assert_eq!(under.failures, 1);
        assert!(
            under.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("Boolean source work budget exceeded")
        );
        std::fs::write(
            inputs.0.join("0.kf"),
            source_of(&[2], "NiBoolTimelineInterpolator"),
        )
        .unwrap();
        let failed = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_bool_interpolators: true,
                ..Default::default()
            },
            limits(100),
        )
        .unwrap();
        assert_eq!(failed.failures, 2);
        assert!(
            failed.files[0]
                .error
                .as_ref()
                .unwrap()
                .contains("field exceeds")
        );
        assert!(
            failed.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("Boolean source work budget exceeded")
        );
        let old = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_spline_components: true,
                ..Default::default()
            },
            limits(0),
        )
        .unwrap();
        assert_eq!(old.failures, 0);
        let old = serde_json::to_value(old).unwrap();
        assert_eq!(old["schema_version"], 4);
        assert!(old.get("bool_interpolator_branch").is_none());
        assert!(old["files"][0].get("bool_interpolators").is_none());
    }
    #[test]
    fn boolean_comparison_rejects_normalization_links_and_extra_evaluated_fields() {
        let source = source_of(&[2, 255, 255, 255, 255], "NiBoolInterpolator");
        let (_, decoded) = boolean::decode(&source, "bool.kf").unwrap();
        let blocks = &decoded.booleans.blocks;
        let expected = serde_json::to_value(blocks).unwrap();
        assert!(compare_fixed_blocks(blocks, &expected).unwrap());
        for (field, value) in [("raw_value", 1), ("data", 0)] {
            let mut changed = expected.clone();
            changed[0]["data"][field] = json!(value);
            assert!(!compare_fixed_blocks(blocks, &changed).unwrap());
        }
        let mut changed = expected.clone();
        changed[0]["block_type"] = json!("NiBoolTimelineInterpolator");
        assert!(!compare_fixed_blocks(blocks, &changed).unwrap());
        let mut changed = expected;
        changed[0]["data"]["truth"] = json!(true);
        assert!(!compare_fixed_blocks(blocks, &changed).unwrap());
    }

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
        let exact = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_splines: true,
                ..Default::default()
            },
            Work {
                keys: 0,
                splines: 6,
                ..Default::default()
            },
        )
        .unwrap();
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
        let under = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_splines: true,
                ..Default::default()
            },
            Work {
                keys: 0,
                splines: 5,
                ..Default::default()
            },
        )
        .unwrap();
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
        let failed = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_splines: true,
                ..Default::default()
            },
            Work {
                keys: 0,
                splines: 100,
                ..Default::default()
            },
        )
        .unwrap();
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
        let old = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_keyframes: true,
                ..Default::default()
            },
            Work {
                keys: 0,
                splines: 0,
                ..Default::default()
            },
        )
        .unwrap();
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
    #[test]
    fn component_batch_admission_and_earlier_schema_opacity() {
        let scalar = words(&[
            0,
            0x3f80_0000,
            u32::MAX,
            u32::MAX,
            0x8000_0000,
            u32::MAX,
            0,
            1,
        ]);
        let inputs = inputs(&[]);
        for id in 0..2 {
            std::fs::write(
                inputs.0.join(format!("{id}.kf")),
                source_of(&scalar, "NiBSplineCompFloatInterpolator"),
            )
            .unwrap();
        }
        let limits = |components| Work {
            keys: 0,
            splines: 0,
            components,
            booleans: 0,
            boolean_keys: 0,
            diagnostic_bytes: 0,
        };
        let exact = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_spline_components: true,
                ..Default::default()
            },
            limits(18),
        )
        .unwrap();
        assert_eq!(exact.failures, 0);
        assert_eq!(exact.schema_version, 4);
        assert_eq!(
            exact
                .files
                .iter()
                .map(|f| f.spline_components.as_ref().unwrap().work_units)
                .sum::<usize>(),
            18
        );
        assert!(
            exact
                .files
                .iter()
                .all(|f| f.keys.is_some() && f.splines.is_some())
        );
        let under = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_spline_components: true,
                ..Default::default()
            },
            limits(17),
        )
        .unwrap();
        assert_eq!(under.failures, 1);
        assert!(
            under.files[1]
                .error
                .as_ref()
                .unwrap()
                .contains("spline-component work budget exceeded")
        );
        let mut bad = scalar;
        bad[16..20].copy_from_slice(&0x7fc0_0001u32.to_le_bytes());
        std::fs::write(
            inputs.0.join("0.kf"),
            source_of(&bad, "NiBSplineCompFloatInterpolator"),
        )
        .unwrap();
        let failed = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_spline_components: true,
                ..Default::default()
            },
            limits(100),
        )
        .unwrap();
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
                .contains("spline-component work budget exceeded")
        );
        let old = inspect_with_work(
            &inputs.0,
            None,
            SourceOptions {
                include_splines: true,
                ..Default::default()
            },
            limits(0),
        )
        .unwrap();
        assert_eq!(old.failures, 0);
        let old = serde_json::to_value(old).unwrap();
        assert_eq!(old["schema_version"], 3);
        assert!(old.get("spline_component_branch").is_none());
        assert!(old["files"][0].get("spline_components").is_none());
    }
    #[test]
    fn exact_component_comparison_includes_all_fixed_fields() {
        let source = source_of(
            &words(&[0, 1, u32::MAX, u32::MAX, 0x8000_0000, 65535, 1, 0x7f7f_ffff]),
            "NiBSplineCompFloatInterpolator",
        );
        let (_, decoded) = spline::components::decode(&source, "component.kf").unwrap();
        let blocks = &decoded.components.blocks;
        let expected = serde_json::to_value(blocks).unwrap();
        assert!(compare_fixed_blocks(blocks, &expected).unwrap());
        for (name, value) in [
            ("value_bits", 0),
            ("handle", u32::MAX),
            ("float_offset_bits", 0),
            ("float_half_range_bits", 0),
        ] {
            let mut changed = expected.clone();
            changed[0]["data"][name] = json!(value);
            assert!(!compare_fixed_blocks(blocks, &changed).unwrap(), "{name}");
        }
        let mut changed = expected.clone();
        changed[0]["sha256"] = json!("0".repeat(64));
        assert!(!compare_fixed_blocks(blocks, &changed).unwrap());
        let mut changed = expected;
        changed[0]["data"]["evaluated_pose"] = json!(true);
        assert!(!compare_fixed_blocks(blocks, &changed).unwrap());
    }
    #[test]
    fn explicit_engineering_sample_is_consumed_without_changing_source_catalogue() {
        let inputs = inputs(&[words(&[
            0,
            2,
            1,
            0,
            0x8000_0000,
            0x4000_0000,
            0xc080_0000,
            0x4000_0000,
            0x4080_0000,
            0xc000_0000,
            0x4100_0000,
            0,
        ])]);
        let path = inputs.0.join("0.kf");
        let source = inspect(
            &path,
            None,
            SourceOptions {
                include_keyframes: true,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        let sampled = inspect(
            &path,
            None,
            SourceOptions::default(),
            Some(SampleRequest {
                block: 0,
                channel: SampleChannel::Translation,
                time: 1.,
            }),
        )
        .unwrap();
        assert_eq!(sampled.failures, 0);
        assert_eq!(sampled.schema_version, 2);
        let diagnostic = sampled.files[0].engineering_sample.as_ref().unwrap();
        assert_eq!(diagnostic.source_block, 0);
        assert_eq!(diagnostic.work.validation_units, 10);
        assert_eq!(diagnostic.work.sampling_units, 8);
        let sampling::Evaluated::Translation {
            sample: Some(sample),
        } = &diagnostic.evaluation
        else {
            panic!("requested translation missing")
        };
        assert_eq!(sample.evaluated_f64_bits.map(f64::from_bits), [2., 0., 2.]);
        assert_eq!(sample.source_key_indices, [0, 1]);
        assert!(sample.source_value_bits.is_none());
        assert_eq!(
            serde_json::to_value(&source.files[0].keys).unwrap(),
            serde_json::to_value(&sampled.files[0].keys).unwrap()
        );
        let source = serde_json::to_value(source).unwrap();
        assert!(source.get("engineering_sampling_contract").is_none());
        assert!(source["files"][0].get("engineering_sample").is_none());
        let absent = inspect(
            &path,
            None,
            SourceOptions::default(),
            Some(SampleRequest {
                block: 0,
                channel: SampleChannel::Scale,
                time: 10.,
            }),
        )
        .unwrap();
        assert_eq!(absent.failures, 0);
        let absent = serde_json::to_value(absent).unwrap();
        assert_eq!(
            absent["files"][0]["engineering_sample"]["evaluation"],
            json!({"channel":"scale","sample":null})
        );
    }
    #[test]
    fn sampling_request_refusals_are_contextual_and_source_stays_available() {
        let inputs = inputs(&[words(&[0, 1, 1, 0, 0x3f80_0000, 0, 0, 0])]);
        let path = inputs.0.join("0.kf");
        for (block, time, reason) in [(0, 1., "extrapolate"), (9, 0., "selected block is not in")] {
            let report = inspect(
                &path,
                None,
                SourceOptions::default(),
                Some(SampleRequest {
                    block,
                    channel: SampleChannel::Translation,
                    time,
                }),
            )
            .unwrap();
            assert_eq!(report.failures, 1);
            assert!(report.files[0].keys.is_some());
            assert!(report.files[0].engineering_sample.is_none());
            assert!(report.files[0].error.as_ref().unwrap().contains(reason));
        }
        let request = || {
            Some(SampleRequest {
                block: 0,
                channel: SampleChannel::Translation,
                time: f64::NAN,
            })
        };
        assert!(
            inspect(&path, None, SourceOptions::default(), request())
                .err()
                .unwrap()
                .to_string()
                .contains("finite explicit source time")
        );
        assert!(
            inspect(
                &inputs.0,
                None,
                SourceOptions::default(),
                Some(SampleRequest {
                    block: 0,
                    channel: SampleChannel::Translation,
                    time: 0.
                })
            )
            .err()
            .unwrap()
            .to_string()
            .contains("one explicit input file")
        );
    }
}
