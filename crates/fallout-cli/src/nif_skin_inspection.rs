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

const TRANSPORT_JSON_BYTES: usize = 64 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TransportWeights {
    PreserveRawNonnegative {},
    RequireUnitSum { absolute_tolerance: f64 },
}
impl TransportWeights {
    fn policy(&self) -> nif_skin::pose::WeightPolicy {
        match *self {
            Self::PreserveRawNonnegative {} => nif_skin::pose::WeightPolicy::PreserveRawNonnegative,
            Self::RequireUnitSum { absolute_tolerance } => {
                nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance }
            }
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeometryStreamsRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    instance: u32,
    weights: TransportWeights,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TransportTopologyPolicy {
    AlternatingStripWindingSkipRepeatedIndexV1 {},
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionTrianglesRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    partition_block: u32,
    partition_ordinal: usize,
    policy: TransportTopologyPolicy,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TransportPrecision {
    FiniteNearestF32 { maximum_absolute_error: f64 },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PalettePacketRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    weights: TransportWeights,
    precision: TransportPrecision,
}
#[derive(Serialize)]
pub struct TransportReport<T> {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    evaluation: Option<T>,
    error: Option<String>,
    pub failures: usize,
    driver_charged_bytes: usize,
    json_byte_limit: usize,
}
#[derive(Serialize)]
pub struct GeometryStreamsEvaluation {
    streams: nif_skin::streams::PreparedGeometryStreams,
    stored_pose: nif_skin::streams::Evaluation,
}
fn transport_driver<T, R>(input: &Path) -> Result<usize> {
    // Include released request buffer, typed request/report, SHA, conservative
    // UTF8/OS path copies and diagnostic capacity. JSON gets its own allowance.
    input
        .as_os_str()
        .as_encoded_bytes()
        .len()
        .checked_mul(4)
        .and_then(|n| {
            n.checked_add(
                64 * 1024
                    + 4096
                    + 64
                    + std::mem::size_of::<R>()
                    + std::mem::size_of::<TransportReport<T>>(),
            )
        })
        .ok_or_else(|| "transport driver byte count overflow".into())
}
fn transport_sum(values: impl IntoIterator<Item = usize>) -> fallout_data::Result<usize> {
    values
        .into_iter()
        .try_fold(0usize, |a, b| a.checked_add(b))
        .ok_or_else(|| {
            fallout_data::Error::Unsupported("transport concurrent byte count overflow".into())
        })
}
fn transport_finish<T>(
    input: &Path,
    sha256: String,
    contract: &'static str,
    driver: usize,
    evaluated: fallout_data::Result<T>,
) -> TransportReport<T> {
    let (evaluation, error) = match evaluated {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e.to_string())),
    };
    TransportReport {
        schema_version: 1,
        contract,
        input: input.into(),
        sha256,
        failures: usize::from(error.is_some()),
        evaluation,
        error,
        driver_charged_bytes: driver,
        json_byte_limit: TRANSPORT_JSON_BYTES,
    }
}
pub fn inspect_geometry_streams(
    input: &Path,
    request_path: &Path,
) -> Result<TransportReport<GeometryStreamsEvaluation>> {
    let request: GeometryStreamsRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("geometry streams request requires schema1".into());
    }
    let driver = transport_driver::<GeometryStreamsEvaluation, GeometryStreamsRequest>(input)?;
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let source = input.display().to_string();
    let limits = nif_skin::streams::Limits::default();
    let initial = transport_sum([
        bytes.len(),
        limits.source.partition.skin.scene.array_bytes,
        limits.source.partition.skin.skin_array_bytes,
        limits.source.partition.array_bytes,
        limits.source.array_bytes,
        limits.source_metadata_array_bytes,
        limits.array_bytes,
        driver,
    ]);
    let prepared = initial.and_then(|n| {
        if n > 896 * 1024 * 1024 {
            return Err(fallout_data::Error::Unsupported(
                "geometry streams driver admission exceeded".into(),
            ));
        }
        nif_skin::streams::prepare(
            &bytes,
            &source,
            nif_skin::streams::Request {
                expected_source_sha256: request.expected_source_sha256,
                geometry: request.geometry,
            },
            limits,
        )
    });
    let sha = match &prepared {
        Ok(p) => p.identity().source_sha256.clone(),
        Err(_) => format!("{:x}", Sha256::digest(&bytes)),
    };
    drop(bytes);
    let evaluated = prepared.and_then(|streams| {
        let available = (896usize * 1024 * 1024)
            .checked_sub(driver)
            .and_then(|n| n.checked_sub(TRANSPORT_JSON_BYTES))
            .ok_or_else(|| {
                fallout_data::Error::Unsupported("geometry transport driver budget exceeded".into())
            })?;
        let limits = nif_skin::streams::EvaluationLimits::default();
        let stored_pose = streams.evaluate_stored(
            nif_skin::streams::EvaluationRequest {
                expected_source_sha256: request.expected_source_sha256,
                geometry: request.geometry,
                instance: request.instance,
                weights: request.weights.policy(),
            },
            nif_skin::streams::EvaluationLimits {
                max_combined_retained_bytes: limits.max_combined_retained_bytes.min(available),
                ..limits
            },
        )?;
        Ok(GeometryStreamsEvaluation {
            streams,
            stored_pose,
        })
    });
    Ok(transport_finish(
        input,
        sha,
        "engineering-whole-geometry-stream-transport-v1",
        driver,
        evaluated,
    ))
}
pub fn inspect_partition_triangles(
    input: &Path,
    request_path: &Path,
) -> Result<TransportReport<partition::streams::triangles::Packet>> {
    use partition::streams::triangles;
    let request: PartitionTrianglesRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("partition triangles request requires schema1".into());
    }
    let driver = transport_driver::<triangles::Packet, PartitionTrianglesRequest>(input)?;
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let source = input.display().to_string();
    let limits = partition::streams::Limits::default();
    let initial = transport_sum([
        bytes.len(),
        limits.source.skin.scene.array_bytes,
        limits.source.skin.skin_array_bytes,
        limits.source.array_bytes,
        limits.array_bytes,
        driver,
    ]);
    let prepared = initial.and_then(|n| {
        if n > 768 * 1024 * 1024 {
            return Err(fallout_data::Error::Unsupported(
                "partition triangles driver admission exceeded".into(),
            ));
        }
        partition::streams::prepare(
            &bytes,
            &source,
            partition::streams::Request {
                expected_source_sha256: request.expected_source_sha256,
                geometry: request.geometry,
                partition_block: request.partition_block,
                partition_ordinal: request.partition_ordinal,
            },
            limits,
        )
    });
    let sha = match &prepared {
        Ok(p) => p.identity().source_sha256.clone(),
        Err(_) => format!("{:x}", Sha256::digest(&bytes)),
    };
    drop(bytes);
    let evaluated = prepared.and_then(|streams| {
        let available = (160usize * 1024 * 1024)
            .checked_sub(driver)
            .and_then(|n| n.checked_sub(TRANSPORT_JSON_BYTES))
            .ok_or_else(|| {
                fallout_data::Error::Unsupported(
                    "partition triangles driver budget exceeded".into(),
                )
            })?;
        let limits = triangles::Limits::default();
        let TransportTopologyPolicy::AlternatingStripWindingSkipRepeatedIndexV1 {} = request.policy;
        streams.triangle_packet(
            triangles::Policy::AlternatingStripWindingSkipRepeatedIndexV1,
            triangles::Limits {
                max_combined_retained_bytes: limits.max_combined_retained_bytes.min(available),
                ..limits
            },
        )
    });
    Ok(transport_finish(
        input,
        sha,
        "engineering-partition-triangle-transport-v1",
        driver,
        evaluated,
    ))
}
pub fn inspect_palette_packet(
    input: &Path,
    request_path: &Path,
) -> Result<TransportReport<nif_skin::pose::palette_packet::Packet>> {
    use nif_skin::pose::palette_packet as palette;
    let request: PalettePacketRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("palette packet request requires schema1".into());
    }
    let driver = transport_driver::<palette::Packet, PalettePacketRequest>(input)?;
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let source = input.display().to_string();
    let available = (896usize * 1024 * 1024)
        .checked_sub(driver)
        .and_then(|n| n.checked_sub(TRANSPORT_JSON_BYTES))
        .ok_or("palette driver byte budget exceeded")?;
    let limits = palette::Limits::default();
    let TransportPrecision::FiniteNearestF32 {
        maximum_absolute_error,
    } = request.precision;
    let evaluated = palette::prepare(
        &bytes,
        &source,
        palette::Request {
            expected_source_sha256: request.expected_source_sha256,
            geometry: request.geometry,
            weights: request.weights.policy(),
            precision: palette::Precision::FiniteNearestF32 {
                maximum_absolute_error,
            },
        },
        palette::Limits {
            max_combined_retained_bytes: limits.max_combined_retained_bytes.min(available),
            ..limits
        },
    );
    let sha = match &evaluated {
        Ok(p) => p.source_sha256().to_owned(),
        Err(_) => format!("{:x}", Sha256::digest(&bytes)),
    };
    drop(bytes);
    Ok(transport_finish(
        input,
        sha,
        "engineering-finite-f32-palette-transport-v1",
        driver,
        evaluated,
    ))
}

struct TransportWriter<W> {
    sink: W,
    count: usize,
    limit: usize,
}
impl<W: std::io::Write> std::io::Write for TransportWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .count
            .checked_add(bytes.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(std::io::Error::other("transport JSON byte budget exceeded"));
        }
        let written = self.sink.write(bytes)?;
        self.count += written;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.sink.flush()
    }
}
fn transport_json(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    use std::io::Write;
    // Count without a retained JSON buffer, then allocate only admitted bytes.
    let mut count = TransportWriter {
        sink: std::io::sink(),
        count: 0,
        limit,
    };
    serde_json::to_writer_pretty(&mut count, value)?;
    count.write_all(b"\n")?;
    let mut output = TransportWriter {
        sink: Vec::with_capacity(count.count),
        count: 0,
        limit: count.count,
    };
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    Ok(output.sink)
}
/// New transport modes alone use bounded preallocation before publication.
pub fn emit_transport(value: &impl Serialize, output: Option<&Path>) -> Result<()> {
    use std::io::Write;
    let bytes = transport_json(value, TRANSPORT_JSON_BYTES)?;
    if let Some(path) = output {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        eprintln!("Wrote {}", path.display());
    } else {
        std::io::stdout().lock().write_all(&bytes)?;
    }
    Ok(())
}
#[cfg(test)]
mod transport_tests {
    #[test]
    fn complete_json_is_admitted_before_output_allocation_and_exact_ceiling_is_closed() {
        let value = serde_json::json!({"raw":"quote\"\\\n","vertices":[[1,2,3],[4,5,6]]});
        let full = super::transport_json(&value, 1000).unwrap();
        assert_eq!(super::transport_json(&value, full.len()).unwrap(), full);
        assert!(super::transport_json(&value, full.len() - 1).is_err());
        assert!(super::transport_json(&value, 0).is_err());
    }
    #[test]
    fn every_new_nested_policy_is_strict_including_empty_variants() {
        for bad in [
            r#"{"kind":"preserve_raw_nonnegative","repair":true}"#,
            r#"{"kind":"require_unit_sum","absolute_tolerance":0,"repair":true}"#,
        ] {
            assert!(serde_json::from_str::<super::TransportWeights>(bad).is_err());
        }
        assert!(
            serde_json::from_str::<super::TransportTopologyPolicy>(
                r#"{"kind":"alternating_strip_winding_skip_repeated_index_v1","flip":true}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<super::TransportPrecision>(
                r#"{"kind":"finite_nearest_f32","maximum_absolute_error":0,"transpose":true}"#
            )
            .is_err()
        );
    }
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

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PoseSetChannelRequest {
    object: u32,
    controller: u32,
    source_time: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PoseSetRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    absolute_weight_tolerance: f64,
    controller_policy: String,
    channels: Vec<PoseSetChannelRequest>,
}

#[derive(Serialize)]
pub struct PoseSetReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    request: PoseSetRequest,
    evaluation: Option<nif_skin::pose::EvaluationWithSet>,
    error: Option<String>,
    pub failures: usize,
}

pub fn inspect_pose_set(input: &Path, request_path: &Path) -> Result<PoseSetReport> {
    let request: PoseSetRequest = serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1
        || request.controller_policy != "require_exact_required_forest"
        || request.channels.len() > 256
    {
        return Err(
            "pose set skin requires schema1, exact required forest policy and at most256 channels"
                .into(),
        );
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let channels: Vec<_> = request
        .channels
        .iter()
        .map(|channel| fallout_data::nif_animation::pose::Request {
            object: channel.object,
            controller: channel.controller,
            source_time: channel.source_time,
        })
        .collect();
    let evaluated = nif_skin::pose::evaluate_set_sampled(
        &bytes,
        &input.display().to_string(),
        nif_skin::pose::SetRequest {
            expected_source_sha256: request.expected_source_sha256,
            skin: nif_skin::pose::Request {
                geometry: request.geometry,
                weights: nif_skin::pose::WeightPolicy::RequireUnitSum {
                    absolute_tolerance: request.absolute_weight_tolerance,
                },
            },
        },
        &channels,
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(PoseSetReport {
        schema_version: 1,
        contract: "engineering-complete-required-pose-set-skin-v1",
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum BoundsPoseRequest {
    Stored {},
    Sampled {
        object: u32,
        controller: u32,
        source_time: f64,
        controller_policy: String,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BoundsRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    weights: BoundsWeightPolicy,
    pose: BoundsPoseRequest,
}
// Empty struct variants enforce deny_unknown_fields; tagged unit variants
// otherwise accept ignored data alongside the tag in serde's current format.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum BoundsWeightPolicy {
    PreserveRawNonnegative {},
    RequireUnitSum { absolute_tolerance: f64 },
}
impl BoundsWeightPolicy {
    fn policy(&self) -> nif_skin::pose::WeightPolicy {
        match *self {
            Self::PreserveRawNonnegative {} => nif_skin::pose::WeightPolicy::PreserveRawNonnegative,
            Self::RequireUnitSum { absolute_tolerance } => {
                nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance }
            }
        }
    }
}
#[derive(Serialize)]
pub struct BoundsReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    request: BoundsRequest,
    evaluation: Option<nif_skin::bounds::Evaluation>,
    error: Option<String>,
    pub failures: usize,
}

#[cfg(test)]
mod bounds_request_tests {
    use super::*;

    #[test]
    fn every_nested_bounds_request_variant_rejects_unknown_fields_and_keeps_explicit_tags() {
        let digest = [0u8; 32];
        for pose in [
            serde_json::json!({"kind":"stored"}),
            serde_json::json!({"kind":"sampled","object":1,"controller":7,"source_time":-0.0,"controller_policy":"refuse_other_required"}),
        ] {
            for weights in [
                serde_json::json!({"kind":"preserve_raw_nonnegative"}),
                serde_json::json!({"kind":"require_unit_sum","absolute_tolerance":0.0}),
            ] {
                let valid = serde_json::json!({"schema_version":1,"expected_source_sha256":digest,"geometry":3,"weights":weights,"pose":pose});
                let request: BoundsRequest = serde_json::from_value(valid.clone()).unwrap();
                assert_eq!(serde_json::to_value(request).unwrap(), valid);
                for field in [None, Some("pose"), Some("weights")] {
                    let mut invalid = valid.clone();
                    let object = match field {
                        Some(key) => &mut invalid[key],
                        None => &mut invalid,
                    };
                    object["extra"] = serde_json::json!(1);
                    assert!(
                        serde_json::from_value::<BoundsRequest>(invalid).is_err(),
                        "variant {field:?}"
                    );
                }
                for missing in ["pose", "weights", "expected_source_sha256"] {
                    let mut invalid = valid.clone();
                    invalid.as_object_mut().unwrap().remove(missing);
                    assert!(serde_json::from_value::<BoundsRequest>(invalid).is_err());
                }
            }
        }
    }
}

pub fn inspect_bounds(input: &Path, request_path: &Path) -> Result<BoundsReport> {
    let request: BoundsRequest = serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("skin bounds requires schema1".into());
    }
    let selected = match &request.pose {
        BoundsPoseRequest::Stored {} => nif_skin::bounds::Pose::Stored,
        BoundsPoseRequest::Sampled {
            object,
            controller,
            source_time,
            controller_policy,
        } => {
            if controller_policy != "refuse_other_required" {
                return Err("sampled skin bounds requires refuse_other_required policy".into());
            }
            nif_skin::bounds::Pose::Sampled {
                controller_policy: nif_skin::pose::ControllerPolicy::RefuseOtherRequired,
                animation: fallout_data::nif_animation::pose::Request {
                    object: *object,
                    controller: *controller,
                    source_time: *source_time,
                },
            }
        }
    };
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let evaluated = nif_skin::bounds::evaluate(
        &bytes,
        &input.display().to_string(),
        nif_skin::bounds::Request {
            expected_source_sha256: request.expected_source_sha256,
            skin: nif_skin::pose::Request {
                geometry: request.geometry,
                weights: request.weights.policy(),
            },
            pose: selected,
        },
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(BoundsReport {
        schema_version: 1,
        contract: nif_skin::bounds::CONTRACT,
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        request,
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
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

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum PreparedInfluenceWeightPolicy {
    PreserveRawNonnegative {},
    RequireUnitSum { absolute_tolerance: f64 },
}
impl PreparedInfluenceWeightPolicy {
    fn policy(&self) -> nif_skin::pose::WeightPolicy {
        match *self {
            Self::PreserveRawNonnegative {} => nif_skin::pose::WeightPolicy::PreserveRawNonnegative,
            Self::RequireUnitSum { absolute_tolerance } => {
                nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance }
            }
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedInfluencesRequest {
    schema_version: u32,
    expected_sha256: [u8; 32],
    geometry: u32,
    weight_policies: Vec<PreparedInfluenceWeightPolicy>,
}
#[derive(Serialize)]
struct PreparedInfluencesReuse {
    initial_binding_decodes: usize,
    initial_scene_decodes: usize,
    initial_source_hash_byte_visits: usize,
    initial_csr_constructions: usize,
    binding_decodes_per_evaluation: usize,
    scene_decodes_per_evaluation: usize,
    source_hash_byte_visits_per_evaluation: usize,
    csr_constructions_per_evaluation: usize,
}
#[derive(Serialize)]
struct PreparedInfluencesEvaluation {
    preparation: nif_skin::pose::PreparationUsage,
    table: nif_skin::influences::Table,
    poses: Vec<nif_skin::pose::Evaluation>,
    reuse: PreparedInfluencesReuse,
    /// Conservative sum of each call's live-source/table/output admission.
    retained_bytes: usize,
    work_units: usize,
    retail_behavior_verified: bool,
}
#[derive(Serialize)]
pub struct PreparedInfluencesReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    request: PreparedInfluencesRequest,
    evaluation: Option<PreparedInfluencesEvaluation>,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_prepared_influences(
    input: &Path,
    request_path: &Path,
) -> Result<PreparedInfluencesReport> {
    let request: PreparedInfluencesRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("prepared influences request requires schema1".into());
    }
    if request.weight_policies.is_empty() || request.weight_policies.len() > 64 {
        return Err("prepared influences requires 1..64 explicit weight policies".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let source = input.display().to_string();
    let prepared = nif_skin::pose::PreparedSkinSource::prepare(&bytes, &source, Default::default());
    let sha256 = match &prepared {
        Ok(value) => value.source_sha256().to_owned(),
        Err(_) => format!("{:x}", Sha256::digest(&bytes)),
    };
    let evaluated = (|| -> fallout_data::Result<PreparedInfluencesEvaluation> {
        let prepared = prepared?;
        let table =
            nif_skin::influences::prepare(&bytes, &source, request.geometry, Default::default())?;
        let preparation = prepared.usage();
        let table_source_hash_byte_visits = bytes.len();
        // Both sealed producers own their data. Evaluations cannot see or hash
        // the input buffer; two independent initial preparations remain explicit.
        drop(bytes);
        let mut retained_bytes = std::mem::size_of::<PreparedInfluencesEvaluation>()
            + request.weight_policies.len() * std::mem::size_of::<nif_skin::pose::Evaluation>();
        let mut work_units = preparation
            .work_units
            .checked_add(table.usage().work_units)
            .and_then(|n| n.checked_add(table_source_hash_byte_visits))
            .ok_or_else(|| {
                fallout_data::Error::Unsupported("prepared influences work sum overflow".into())
            })?;
        let mut poses = Vec::with_capacity(request.weight_policies.len());
        for weights in &request.weight_policies {
            let remaining_bytes = (128usize * 1024 * 1024)
                .checked_sub(retained_bytes)
                .ok_or_else(|| {
                    fallout_data::Error::Unsupported(
                        "prepared influences aggregate storage exceeded".into(),
                    )
                })?;
            let remaining_work = 128_000_000usize.checked_sub(work_units).ok_or_else(|| {
                fallout_data::Error::Unsupported(
                    "prepared influences aggregate work exceeded".into(),
                )
            })?;
            let limits = nif_skin::pose::GeometryLimits::default();
            let pose = prepared.evaluate_table(
                request.expected_sha256,
                nif_skin::pose::Request {
                    geometry: request.geometry,
                    weights: weights.policy(),
                },
                &table,
                nif_skin::pose::GeometryLimits {
                    array_bytes: limits.array_bytes.min(remaining_bytes),
                    work_units: limits.work_units.min(remaining_work),
                    ancestry_depth: limits.ancestry_depth,
                },
            )?;
            retained_bytes += pose.retained_bytes;
            work_units += pose.work_units;
            poses.push(pose);
        }
        Ok(PreparedInfluencesEvaluation {
            preparation,
            table,
            poses,
            reuse: PreparedInfluencesReuse {
                initial_binding_decodes: 2,
                initial_scene_decodes: 2,
                initial_source_hash_byte_visits: preparation.source_bytes * 2,
                initial_csr_constructions: 1,
                binding_decodes_per_evaluation: 0,
                scene_decodes_per_evaluation: 0,
                source_hash_byte_visits_per_evaluation: 0,
                csr_constructions_per_evaluation: 0,
            },
            retained_bytes,
            work_units,
            retail_behavior_verified: false,
        })
    })();
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(PreparedInfluencesReport {
        schema_version: 1,
        contract: "engineering-prepared-source-influence-skin-v1",
        input: input.into(),
        sha256,
        request,
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionStreamsRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    partition_block: u32,
    partition_ordinal: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartitionPoseRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometry: u32,
    partition_block: u32,
    partition_ordinal: usize,
    weights: InfluenceWeightPolicy,
}
#[derive(Serialize)]
pub struct PartitionPoseReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    evaluation: Option<nif_skin::pose::partition::Evaluation>,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_partition_pose(input: &Path, request_path: &Path) -> Result<PartitionPoseReport> {
    let request: PartitionPoseRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("partition pose request requires schema1".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let evaluated = nif_skin::pose::partition::evaluate(
        &bytes,
        &input.display().to_string(),
        nif_skin::pose::partition::Request {
            expected_source_sha256: request.expected_source_sha256,
            skin: nif_skin::pose::Request {
                geometry: request.geometry,
                weights: request.weights.policy(),
            },
            partition_block: request.partition_block,
            partition_ordinal: request.partition_ordinal,
        },
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(PartitionPoseReport {
        schema_version: 1,
        contract: "engineering-source-geometry-partition-subset-v1",
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
}
#[derive(Serialize)]
pub struct PartitionStreamsReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    evaluation: Option<partition::streams::Streams>,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_partition_streams(
    input: &Path,
    request_path: &Path,
) -> Result<PartitionStreamsReport> {
    let request: PartitionStreamsRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 {
        return Err("partition streams request requires schema1".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let evaluated = partition::streams::prepare(
        &bytes,
        &input.display().to_string(),
        partition::streams::Request {
            expected_source_sha256: request.expected_source_sha256,
            geometry: request.geometry,
            partition_block: request.partition_block,
            partition_ordinal: request.partition_ordinal,
        },
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(PartitionStreamsReport {
        schema_version: 1,
        contract: "source-qualified-authored-partition-streams-v1",
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedGeometryRequest {
    geometry: u32,
    weights: InfluenceWeightPolicy,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedSkinRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometries: Vec<SharedGeometryRequest>,
}
#[derive(Serialize)]
pub struct SharedSkinReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    evaluation: Option<nif_skin::pose::GeometryBatch>,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_shared_skin(input: &Path, request_path: &Path) -> Result<SharedSkinReport> {
    let request: SharedSkinRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 || request.geometries.len() > 64 {
        return Err("shared skin requires schema1 and at most64 geometry requests".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let geometries: Vec<_> = request
        .geometries
        .iter()
        .map(|request| nif_skin::pose::Request {
            geometry: request.geometry,
            weights: request.weights.policy(),
        })
        .collect();
    let evaluated = nif_skin::pose::evaluate_many(
        &bytes,
        &input.display().to_string(),
        request.expected_source_sha256,
        &geometries,
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(SharedSkinReport {
        schema_version: 1,
        contract: "engineering-shared-source-skin-batch-v1",
        input: input.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        failures: usize::from(error.is_some()),
        evaluation,
        error,
    })
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum JobWeightPolicy {
    PreserveRawNonnegative {},
    RequireUnitSum { absolute_tolerance: f64 },
}
impl JobWeightPolicy {
    fn policy(&self) -> nif_skin::pose::WeightPolicy {
        match *self {
            Self::PreserveRawNonnegative {} => nif_skin::pose::WeightPolicy::PreserveRawNonnegative,
            Self::RequireUnitSum { absolute_tolerance } => {
                nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance }
            }
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JobGeometryRequest {
    geometry: u32,
    weights: JobWeightPolicy,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SkinJobRequest {
    schema_version: u32,
    expected_source_sha256: [u8; 32],
    geometries: Vec<JobGeometryRequest>,
    step_geometry_caps: Vec<usize>,
    cancel_after_completed: Option<usize>,
}
#[derive(Serialize)]
pub struct SkinJobReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    sha256: String,
    request: SkinJobRequest,
    admission: Option<nif_skin::pose::JobAdmission>,
    progress: Vec<nif_skin::pose::Progress>,
    evaluation: Option<nif_skin::pose::GeometryBatch>,
    driver_retained_bytes: usize,
    retained_bytes: usize,
    work_units: usize,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_skin_job(input: &Path, request_path: &Path) -> Result<SkinJobReport> {
    use nif_skin::pose::{EvaluationState, GeometryStepBudget};
    let request: SkinJobRequest = serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 || request.geometries.len() > 64 {
        return Err("skin job requires schema1 and at most64 geometries".into());
    }
    if request.step_geometry_caps.is_empty()
        || request.step_geometry_caps.len() > 128
        || request.step_geometry_caps.iter().any(|cap| *cap > 64)
    {
        return Err("skin job requires 1..128 step caps each in0..64".into());
    }
    if request
        .cancel_after_completed
        .is_some_and(|n| n > request.geometries.len())
    {
        return Err("skin job cancellation boundary exceeds geometry count".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let source = input.display().to_string();
    let prepared = nif_skin::pose::PreparedSkinSource::prepare(&bytes, &source, Default::default());
    let sha256 = match &prepared {
        Ok(value) => value.source_sha256().to_owned(),
        Err(_) => format!("{:x}", Sha256::digest(&bytes)),
    };
    drop(bytes);
    let geometries: Vec<_> = request
        .geometries
        .iter()
        .map(|r| nif_skin::pose::Request {
            geometry: r.geometry,
            weights: r.weights.policy(),
        })
        .collect();
    let progress_capacity = request.step_geometry_caps.len() + 2;
    let driver_retained_bytes = std::mem::size_of::<SkinJobReport>()
        + 64
        + progress_capacity * std::mem::size_of::<nif_skin::pose::Progress>()
        + geometries.len() * std::mem::size_of::<nif_skin::pose::Request>();
    let mut admission = None;
    let mut progress = Vec::with_capacity(progress_capacity);
    let evaluated = (|| -> fallout_data::Result<nif_skin::pose::GeometryBatch> {
        let prepared = prepared?;
        let defaults = nif_skin::pose::BatchEvaluationLimits::default();
        let limits = nif_skin::pose::BatchEvaluationLimits {
            array_bytes: defaults
                .array_bytes
                .checked_sub(driver_retained_bytes)
                .ok_or_else(|| {
                    fallout_data::Error::Unsupported("skin job driver storage exceeded".into())
                })?,
            ..defaults
        };
        let mut job =
            prepared.begin_evaluation(request.expected_source_sha256, &geometries, limits)?;
        admission = Some(job.admission());
        progress.push(job.progress());
        if request.cancel_after_completed == Some(0) {
            progress.push(job.cancel());
            return Err(fallout_data::Error::Unsupported(
                "skin job cancelled before first geometry".into(),
            ));
        }
        for cap in &request.step_geometry_caps {
            let before = job.progress();
            let effective_cap = request.cancel_after_completed.map_or(*cap, |boundary| {
                (*cap).min(boundary - before.completed_geometries)
            });
            match job.advance(GeometryStepBudget {
                geometries: effective_cap,
            }) {
                Ok(value) => progress.push(value),
                Err(error) => {
                    let mut value = job.progress();
                    value.advanced_geometries =
                        value.completed_geometries - before.completed_geometries;
                    value.step_retained_bytes =
                        value.evaluation_retained_bytes - before.evaluation_retained_bytes;
                    value.step_work_units =
                        value.evaluation_work_units - before.evaluation_work_units;
                    progress.push(value);
                    return Err(error);
                }
            }
            let current = job.progress();
            if request.cancel_after_completed == Some(current.completed_geometries) {
                progress.push(job.cancel());
                return Err(fallout_data::Error::Unsupported(
                    "skin job cancelled at requested geometry boundary".into(),
                ));
            }
            if current.state == EvaluationState::Complete {
                break;
            }
        }
        job.finish()
    })();
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let retained_bytes = driver_retained_bytes
        + admission.map_or(0, |a| a.retained_bytes)
        + progress.last().map_or(0, |p| p.evaluation_retained_bytes);
    let work_units = admission.map_or(0, |a| a.work_units)
        + progress.last().map_or(0, |p| p.evaluation_work_units);
    Ok(SkinJobReport {
        schema_version: 1,
        contract: "engineering-cooperative-skin-job-v1",
        input: input.into(),
        sha256,
        request,
        admission,
        progress,
        evaluation,
        driver_retained_bytes,
        retained_bytes,
        work_units,
        failures: usize::from(error.is_some()),
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

// Keep the new clip request strict without changing older weight ingress.
// Empty struct variants reject fields that serde's tagged unit variant ignores.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ExternalClipWeightPolicy {
    PreserveRawNonnegative {},
    RequireUnitSum { absolute_tolerance: f64 },
}
impl ExternalClipWeightPolicy {
    fn policy(&self) -> nif_skin::pose::WeightPolicy {
        match *self {
            Self::PreserveRawNonnegative {} => nif_skin::pose::WeightPolicy::PreserveRawNonnegative,
            Self::RequireUnitSum { absolute_tolerance } => {
                nif_skin::pose::WeightPolicy::RequireUnitSum { absolute_tolerance }
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExternalClipSkinRequest {
    schema_version: u32,
    expected_skin_sha256: [u8; 32],
    expected_rig_sha256: [u8; 32],
    expected_clip_sha256: [u8; 32],
    geometry: u32,
    rig_root: u32,
    explicit_bone_mapping: Vec<ExternalBoneRequest>,
    explicit_root_space_mapping: nif_skin::pose::Affine,
    weights: ExternalClipWeightPolicy,
    clip_object: u32,
    clip_node_name_bytes: Vec<u8>,
    clip_sequence: u32,
    clip_controlled_ordinal: usize,
    source_time: f64,
}

#[cfg(test)]
mod external_clip_request_tests {
    use super::*;

    #[test]
    fn both_clip_weight_variants_are_strict_without_changing_older_ingress() {
        let digest = [0u8; 32];
        for weights in [
            serde_json::json!({"kind":"preserve_raw_nonnegative"}),
            serde_json::json!({"kind":"require_unit_sum","absolute_tolerance":0.0}),
        ] {
            let valid = serde_json::json!({
                "schema_version":1,
                "expected_skin_sha256":digest,"expected_rig_sha256":digest,"expected_clip_sha256":digest,
                "geometry":3,"rig_root":4,"explicit_bone_mapping":[],
                "explicit_root_space_mapping":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]],
                "weights":weights,"clip_object":3,"clip_node_name_bytes":[82,105,103,0],
                "clip_sequence":0,"clip_controlled_ordinal":0,"source_time":-0.0
            });
            let request: ExternalClipSkinRequest = serde_json::from_value(valid.clone()).unwrap();
            assert_eq!(serde_json::to_value(&request.weights).unwrap(), weights);
            let old: InfluenceWeightPolicy = serde_json::from_value(weights.clone()).unwrap();
            assert_eq!(
                serde_json::to_value(request.weights.policy()).unwrap(),
                serde_json::to_value(old.policy()).unwrap()
            );
            for field in [None, Some("weights")] {
                let mut invalid = valid.clone();
                match field {
                    Some(key) => invalid[key]["extra"] = serde_json::json!(1),
                    None => invalid["extra"] = serde_json::json!(1),
                }
                assert!(serde_json::from_value::<ExternalClipSkinRequest>(invalid).is_err());
            }
            for missing in ["weights", "expected_skin_sha256", "source_time"] {
                let mut invalid = valid.clone();
                invalid.as_object_mut().unwrap().remove(missing);
                assert!(serde_json::from_value::<ExternalClipSkinRequest>(invalid).is_err());
            }
        }
        for invalid in [
            serde_json::json!({"kind":"preserve_raw_nonnegative","absolute_tolerance":0.0}),
            serde_json::json!({"kind":"require_unit_sum"}),
            serde_json::json!({"kind":"require_unit_sum","absolute_tolerance":"0"}),
            serde_json::json!({"kind":"normalize"}),
            serde_json::json!({}),
        ] {
            assert!(serde_json::from_value::<ExternalClipWeightPolicy>(invalid).is_err());
        }
        // Older requests deliberately retain their previous tagged-unit behavior.
        let old: InfluenceWeightPolicy = serde_json::from_value(
            serde_json::json!({"kind":"preserve_raw_nonnegative","extra":1}),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(old.policy()).unwrap(),
            serde_json::to_value(nif_skin::pose::WeightPolicy::PreserveRawNonnegative).unwrap()
        );
    }
}
#[derive(Serialize)]
pub struct ExternalClipSkinReport {
    schema_version: u32,
    contract: &'static str,
    input: PathBuf,
    rig_input: PathBuf,
    clip_input: PathBuf,
    skin_sha256: String,
    rig_sha256: String,
    clip_sha256: String,
    evaluation: Option<nif_skin::external::sampled::Evaluation>,
    error: Option<String>,
    pub failures: usize,
}
pub fn inspect_external_clip_skin(
    input: &Path,
    rig: &Path,
    clip: &Path,
    request_path: &Path,
) -> Result<ExternalClipSkinReport> {
    let request: ExternalClipSkinRequest =
        serde_json::from_slice(&read_bounded(request_path, 64 * 1024)?)?;
    if request.schema_version != 1 || request.explicit_bone_mapping.len() > 4096 {
        return Err(
            "external clip skin requires schema1 and at most4096 explicit mapped bones".into(),
        );
    }
    let skin_bytes = read_bounded(input, 64 * 1024 * 1024)?;
    let rig_bytes = read_bounded(rig, 64 * 1024 * 1024)?;
    let clip_bytes = read_bounded(clip, 64 * 1024 * 1024)?;
    let mapping = nif_skin::external::Request {
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
    let evaluated = nif_skin::external::sampled::evaluate(
        &skin_bytes,
        &rig_bytes,
        &clip_bytes,
        &format!(
            "{} + {} + {}",
            input.display(),
            rig.display(),
            clip.display()
        ),
        nif_skin::external::sampled::Request {
            mapping: &mapping,
            clip: fallout_data::nif_animation::clip::Request {
                expected_skeleton_sha256: request.expected_rig_sha256,
                expected_clip_sha256: request.expected_clip_sha256,
                object: request.clip_object,
                node_name_bytes: &request.clip_node_name_bytes,
                sequence: request.clip_sequence,
                controlled_ordinal: request.clip_controlled_ordinal,
                source_time: request.source_time,
            },
        },
        Default::default(),
    );
    let (evaluation, error) = match evaluated {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(ExternalClipSkinReport {
        schema_version: 1,
        contract: nif_skin::external::sampled::CONTRACT,
        input: input.into(),
        rig_input: rig.into(),
        clip_input: clip.into(),
        skin_sha256: format!("{:x}", Sha256::digest(&skin_bytes)),
        rig_sha256: format!("{:x}", Sha256::digest(&rig_bytes)),
        clip_sha256: format!("{:x}", Sha256::digest(&clip_bytes)),
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
