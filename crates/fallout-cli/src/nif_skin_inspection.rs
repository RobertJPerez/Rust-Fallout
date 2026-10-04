//! Source inspection, exact offline comparison and explicit engineering poses.
use crate::Result;
use fallout_data::{
    baseline,
    nif_skin::{self, Skin, binding, partition},
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
    sha256: String,
    decoded_bytes: usize,
    tuple: Option<[u32; 3]>,
    skin: Option<Skin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    partitions: Option<partition::Catalogue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bindings: Option<binding::Catalogue>,
    error: Option<String>,
    comparison: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    partition_branch: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    binding_scope: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    binding_diagnostics: Option<usize>,
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

#[derive(Serialize)]
pub struct PoseReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    evaluation: Option<nif_skin::pose::Evaluation>,
    error: Option<String>,
    pub failures: usize,
}

/// Separate opt-in receipt; the three existing source-report schemas are intact.
pub fn inspect_pose(input: &Path, geometry: u32, absolute_tolerance: f64) -> Result<PoseReport> {
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let evaluated = nif_skin::pose::evaluate(
        &bytes,
        &input.display().to_string(),
        nif_skin::pose::Request {
            geometry,
            weights: nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance },
        },
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(pose) => (Some(pose), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(PoseReport {
        schema_version: 1,
        contract: "engineering-source-local-skin-v1",
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SampledPoseRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    absolute_weight_tolerance: f64,
    object: u32,
    controller: u32,
    source_time: f64,
    controller_policy: String,
}

#[derive(Serialize)]
pub struct SampledPoseReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    request: SampledPoseRequest,
    evaluation: Option<nif_skin::pose::EvaluationWithSample>,
    error: Option<String>,
    pub failures: usize,
}

pub fn inspect_sampled_pose(input: &Path, request_path: &Path) -> Result<SampledPoseReport> {
    let request: SampledPoseRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 || request.controller_policy != "refuse_other_required" {
        return Err(
            "sampled skin request requires schema1 and refuse_other_required policy".into(),
        );
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let evaluated = nif_skin::pose::evaluate_sampled(
        &bytes,
        &input.display().to_string(),
        nif_skin::pose::SampledRequest {
            expected_source_sha256: request.expected_source_sha256,
            skin: nif_skin::pose::Request {
                geometry: request.geometry,
                weights: nif_skin::pose::WeightPolicy::RequireUnitSum {
                    absolute_tolerance: request.absolute_weight_tolerance,
                },
            },
            controller_policy: nif_skin::pose::ControllerPolicy::RefuseOtherRequired,
        },
        fallout_data::nif_animation::pose::Request {
            object: request.object,
            controller: request.controller,
            source_time: request.source_time,
        },
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(SampledPoseReport {
        schema_version: 1,
        contract: "engineering-one-linked-sample-skin-v1",
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        request,
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
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

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum InfluenceWeightPolicy {
    PreserveRawNonnegative,
    RequireUnitSum { absolute_tolerance: f64 },
}
impl InfluenceWeightPolicy {
    fn policy(&self) -> nif_skin::pose::WeightPolicy {
        match *self {
            Self::PreserveRawNonnegative => nif_skin::pose::WeightPolicy::PreserveRawNonnegative,
            Self::RequireUnitSum { absolute_tolerance } => {
                nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance }
            }
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InfluencesRequest {
    schema_version: u32,
    expected_sha256: [u8; 32],
    geometry: u32,
    weights: InfluenceWeightPolicy,
}
#[derive(Serialize)]
struct InfluencesEvaluation {
    table: nif_skin::influences::Table,
    pose: nif_skin::pose::Evaluation,
}
#[derive(Serialize)]
pub struct InfluencesReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    evaluation: Option<InfluencesEvaluation>,
    error: Option<String>,
    pub failures: usize,
}

pub fn inspect_influences(input: &Path, request_path: &Path) -> Result<InfluencesReport> {
    let request: InfluencesRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("influences request requires schema1".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let source = input.display().to_string();
    let evaluated = (|| -> fallout_data::Result<InfluencesEvaluation> {
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != request.expected_sha256 {
            return Err(fallout_data::Error::Unsupported(format!(
                "{source}: influences source SHA256 differs"
            )));
        }
        let table =
            nif_skin::influences::prepare(&bytes, &source, request.geometry, Default::default())?;
        let pose = table.evaluate(
            &bytes,
            &source,
            nif_skin::pose::Request {
                geometry: request.geometry,
                weights: request.weights.policy(),
            },
            Default::default(),
        )?;
        Ok(InfluencesEvaluation { table, pose })
    })();
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(InfluencesReport {
        schema_version: 1,
        contract: "engineering-exact-raw-influence-csr-v1",
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalBoneRequest {
    bone_ordinal: usize,
    rig_node: u32,
    expected_skin_bone_name_bytes: Vec<u8>,
    expected_rig_node_name_bytes: Vec<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalSkinRequest {
    schema_version: u32,
    expected_skin_sha256: [u8; 32],
    expected_rig_sha256: [u8; 32],
    geometry: u32,
    rig_root: u32,
    explicit_bone_mapping: Vec<ExternalBoneRequest>,
    explicit_root_space_mapping: nif_skin::pose::Affine,
    weights: InfluenceWeightPolicy,
}
#[derive(Serialize)]
pub struct ExternalSkinReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    rig_input: PathBuf,
    skin_sha256: String,
    rig_sha256: String,
    evaluation: Option<nif_skin::external::Evaluation>,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_external_skin(
    input: &Path,
    rig: &Path,
    request_path: &Path,
) -> Result<ExternalSkinReport> {
    let request: ExternalSkinRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 || request.explicit_bone_mapping.len() > 4096 {
        return Err("external skin requires schema1 and at most 4096 explicit mapped bones".into());
    }
    let skin_bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let rig_bytes = read_bounded(rig, 64 * 1024 * 1024)?;
    let source = format!("{} + {}", input.display(), rig.display());
    let value = nif_skin::external::Request {
        expected_skin_sha256: request.expected_skin_sha256,
        expected_rig_sha256: request.expected_rig_sha256,
        geometry: request.geometry,
        rig_root: request.rig_root,
        explicit_bone_mapping: request
            .explicit_bone_mapping
            .into_iter()
            .map(|m| nif_skin::external::BoneMapping {
                bone_ordinal: m.bone_ordinal,
                rig_node: m.rig_node,
                expected_skin_bone_name_bytes: m.expected_skin_bone_name_bytes,
                expected_rig_node_name_bytes: m.expected_rig_node_name_bytes,
            })
            .collect(),
        explicit_root_space_mapping: request.explicit_root_space_mapping,
        weights: request.weights.policy(),
    };
    let evaluated =
        nif_skin::external::evaluate(&skin_bytes, &rig_bytes, &source, &value, Default::default());
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(ExternalSkinReport {
        schema_version: 1,
        contract: "engineering-exact-external-rig-skin-v1",
        input: input.into(),
        rig_input: rig.into(),
        skin_sha256: format!("{:x}", Sha256::digest(&skin_bytes)),
        rig_sha256: format!("{:x}", Sha256::digest(&rig_bytes)),
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
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
    if let Some(partitions) = &actual.partitions
        && serde_json::to_value(&partitions.blocks)? != expected["partitions"]
    {
        return Err("oracle partition source fields differ".into());
    }
    if let Some(bindings) = &actual.bindings {
        let projection = serde_json::json!({"ancestry_scope":bindings.ancestry_scope,"nodes":bindings.nodes,"instances":bindings.instances,
            "footer_roots":bindings.footer_roots,"unsupported_scene_edges":bindings.unsupported_scene_edges});
        if projection != expected["bindings"] {
            return Err("oracle authored node fields or decoded graph binding facts differ".into());
        }
    }
    Ok(())
}

pub fn inspect(
    input: &Path,
    oracle_path: Option<&Path>,
    include_partitions: bool,
    include_bindings: bool,
) -> Result<Report> {
    let include_partitions = include_partitions || include_bindings;
    let schema_version = if include_bindings {
        3
    } else if include_partitions {
        2
    } else {
        1
    };
    let branch = include_partitions.then_some("nv-canonical-flags-four-wide-or-empty");
    let mut report = Report {
        schema_version,
        partition_branch: branch,
        binding_scope: include_bindings.then_some("decoded-source-forest"),
        binding_diagnostics: include_bindings.then_some(0),
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
        if document["schema_version"] != schema_version
            || document["float_encoding"] != "ieee754-binary32-bits"
            || document["nifly_revision"] != "cca0a770094bb962fb28ea1fec5ea903e68fda8e"
            || document["prepare_data_called"] != false
            || document["raw_presence_and_vertex_counts_checked"] != true
        {
            return Err("oracle provenance or raw-field comparison contract is missing".into());
        }
        if let Some(branch) = branch
            && (document["partition_branch"] != branch
                || document["raw_partition_fields_checked"] != true)
        {
            return Err("oracle partition branch contract is missing".into());
        }
        if include_bindings
            && (document["binding_scope"] != "decoded-source-forest"
                || document["raw_node_fields_checked"] != true
                || document["graph_membership_checked"] != true)
        {
            return Err("oracle decoded graph binding contract is missing".into());
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
            partitions: None,
            bindings: None,
            error: None,
            comparison: None,
        };
        let skin_limits = nif_skin::Limits {
            skin_array_bytes: remaining.min(128 * 1024 * 1024),
            ..Default::default()
        };
        let partition_limits = partition::Limits {
            skin: skin_limits,
            array_bytes: remaining.min(128 * 1024 * 1024),
            ..Default::default()
        };
        let decoded = if include_bindings {
            binding::decode_with_limits(
                &bytes,
                &row.input.display().to_string(),
                binding::Limits {
                    partition: partition_limits,
                    array_bytes: remaining.min(64 * 1024 * 1024),
                    ..Default::default()
                },
            )
            .map(|(index, source)| {
                (
                    index,
                    source.skin.skin,
                    Some(source.skin.partitions),
                    Some(source.bindings),
                )
            })
        } else if include_partitions {
            partition::decode_with_limits(
                &bytes,
                &row.input.display().to_string(),
                partition_limits,
            )
            .map(|(index, source)| (index, source.skin, Some(source.partitions), None))
        } else {
            nif_skin::decode_with_limits(&bytes, &row.input.display().to_string(), skin_limits)
                .map(|(index, skin)| (index, skin, None, None))
        };
        match decoded {
            Ok((index, skin, partitions, bindings)) => {
                row.tuple = Some([index.version, index.user_version, index.bethesda_version]);
                remaining = remaining
                    .checked_sub(skin.retained_bytes)
                    .ok_or("aggregate skin catalogue budget exceeded")?;
                if let Some(partitions) = &partitions {
                    remaining = remaining
                        .checked_sub(partitions.retained_bytes)
                        .ok_or("aggregate partition catalogue budget exceeded")?;
                    *report
                        .block_counts
                        .entry("NiSkinPartition".into())
                        .or_default() += partitions.blocks.len();
                    report.unresolved_dependencies += partitions.dependencies.len();
                }
                if let Some(bindings) = &bindings {
                    remaining = remaining
                        .checked_sub(bindings.retained_bytes)
                        .ok_or("aggregate binding catalogue budget exceeded")?;
                    for node in &bindings.nodes {
                        *report
                            .block_counts
                            .entry(node.block_type.clone())
                            .or_default() += 1;
                    }
                    if let Some(count) = &mut report.binding_diagnostics {
                        *count += bindings.diagnostics.len();
                    }
                }
                for block in &skin.blocks {
                    *report
                        .block_counts
                        .entry(block.block_type.clone())
                        .or_default() += 1;
                }
                report.owners += skin.owners.len();
                report.unresolved_dependencies += skin.dependencies.len();
                row.skin = Some(skin);
                row.partitions = partitions;
                row.bindings = bindings;
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
