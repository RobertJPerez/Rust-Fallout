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
    ray_first: Option<FirstRayRequest>,
    segment_cast: Option<SegmentCastRequest>,
    ray_intervals: Option<RayIntervalsRequest>,
    overlap: Option<SphereRequest>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FirstRayRequest {
    ray: fallout_runtime::physics::Ray,
    budget: fallout_runtime::physics::FirstHitBudget,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SegmentCastRequest {
    segment: fallout_runtime::physics::Segment,
    limits: fallout_runtime::physics::SegmentQueryLimits,
    output_bytes: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RayIntervalsRequest {
    ray: fallout_runtime::physics::Ray,
    limits: fallout_runtime::physics::IntervalQueryLimits,
    output_bytes: usize,
}
#[derive(Serialize)]
struct FiniteEnvironmentInput {
    units_binary64_hex: [String; 3],
    attachment_binary64_hex: [[String; 4]; 3],
}
#[derive(Serialize)]
struct SegmentNumericInput {
    start_binary64_hex: [String; 3],
    end_binary64_hex: [String; 3],
}
#[derive(Serialize)]
struct SegmentCastReport {
    numeric_input: SegmentNumericInput,
    environment_numeric_input: FiniteEnvironmentInput,
    limits: fallout_runtime::physics::SegmentQueryLimits,
    output_bytes: usize,
    query:
        fallout_runtime::physics::FiniteQueryReport<fallout_runtime::physics::SegmentIntersection>,
    query_semantics: &'static str,
}
#[derive(Serialize)]
struct RayIntervalsReport {
    numeric_input: RayNumericInput,
    environment_numeric_input: FiniteEnvironmentInput,
    limits: fallout_runtime::physics::IntervalQueryLimits,
    output_bytes: usize,
    query: fallout_runtime::physics::FiniteQueryReport<fallout_runtime::physics::SolidOccupancy>,
    query_semantics: &'static str,
}
fn finite_environment(request: &QueryRequest) -> FiniteEnvironmentInput {
    let units = request.units;
    FiniteEnvironmentInput {
        units_binary64_hex: [
            units.havok_to_source,
            units.source_to_query,
            units.transform_tolerance,
        ]
        .map(|v| format!("{:016x}", v.to_bits())),
        attachment_binary64_hex: request
            .attachment_rows
            .map(|row| row.map(|v| format!("{:016x}", v.to_bits()))),
    }
}
struct FiniteReportCounter(usize);
fn count_finite_report(report: &impl Serialize, limit: usize) -> serde_json::Result<()> {
    // The existing common emitter appends one newline after its pretty JSON.
    // Reserve it before serialization, including the zero-budget case.
    serde_json::to_writer_pretty(&mut FiniteReportCounter(limit.saturating_sub(1)), report)
}
impl std::io::Write for FiniteReportCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.checked_sub(bytes.len()).ok_or_else(|| {
            std::io::Error::other("finite collision report output budget exceeded")
        })?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SphereRequest {
    center: [f64; 3],
    radius: f64,
}
#[derive(Serialize)]
struct RayNumericInput {
    origin_binary64_hex: [String; 3],
    direction_binary64_hex: [String; 3],
    max_distance_binary64_hex: String,
}
#[derive(Serialize)]
struct OverlapNumericInput {
    center_binary64_hex: [String; 3],
    radius_binary64_hex: String,
}
#[derive(Serialize)]
struct FirstRayReport {
    numeric_input: RayNumericInput,
    budget: fallout_runtime::physics::FirstHitBudget,
    hit: Option<fallout_runtime::physics::Hit>,
    query_semantics: &'static str,
}
fn ray_numeric_input(ray: fallout_runtime::physics::Ray) -> RayNumericInput {
    RayNumericInput {
        origin_binary64_hex: ray.origin.map(|v| format!("{:016x}", v.to_bits())),
        direction_binary64_hex: ray.direction.map(|v| format!("{:016x}", v.to_bits())),
        max_distance_binary64_hex: format!("{:016x}", ray.max_distance.to_bits()),
    }
}
fn overlap_numeric_input(s: &SphereRequest) -> OverlapNumericInput {
    OverlapNumericInput {
        center_binary64_hex: s.center.map(|v| format!("{:016x}", v.to_bits())),
        radius_binary64_hex: format!("{:016x}", s.radius.to_bits()),
    }
}
#[derive(Serialize)]
pub struct QueryReport {
    source_sha256: String,
    request_sha256: String,
    units: fallout_runtime::physics::EngineeringUnits,
    ray_numeric_input: Option<RayNumericInput>,
    overlap_numeric_input: Option<OverlapNumericInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    first_ray: Option<FirstRayReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    segment_cast: Option<SegmentCastReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ray_intervals: Option<RayIntervalsReport>,
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
        || (request.ray.is_none()
            && request.ray_first.is_none()
            && request.overlap.is_none()
            && request.segment_cast.is_none()
            && request.ray_intervals.is_none())
    {
        return Err("collision request needs 1..10000 bodies and a ray or overlap".into());
    }
    if request.ray.is_some() && request.ray_first.is_some() {
        return Err("collision request cannot combine ray and ray_first".into());
    }
    let finite_request = request.segment_cast.is_some() || request.ray_intervals.is_some();
    if finite_request
        && (request.ray.is_some()
            || request.ray_first.is_some()
            || request.overlap.is_some()
            || (request.segment_cast.is_some() && request.ray_intervals.is_some()))
    {
        return Err("finite collision request selects exactly one finite query form".into());
    }
    let output_limit = request
        .segment_cast
        .as_ref()
        .map(|v| v.output_bytes)
        .or_else(|| request.ray_intervals.as_ref().map(|v| v.output_bytes));
    if output_limit.is_some_and(|v| v > 64 * 1024 * 1024) {
        return Err("finite output budget must only reduce 64 MiB ceiling".into());
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
    let ray_numeric_input = request.ray.map(ray_numeric_input);
    let overlap_numeric_input = request.overlap.as_ref().map(overlap_numeric_input);
    let segment_cast=request.segment_cast.as_ref().map(|finite| {
        let numeric_input=SegmentNumericInput {
            start_binary64_hex:finite.segment.start.map(|v|format!("{:016x}",v.to_bits())),
            end_binary64_hex:finite.segment.end.map(|v|format!("{:016x}",v.to_bits())),
        };
        let environment_numeric_input=finite_environment(&request);
        let query=scene.segment_cast(finite.segment,finite.limits).map_err(|error|format!(
            "collision finite segment refused: {error}; segment_numeric_input={}; environment_numeric_input={}",
            serde_json::to_string(&numeric_input).expect("numeric audit"),serde_json::to_string(&environment_numeric_input).expect("numeric audit")))?;
        Ok::<_,String>(SegmentCastReport {
            numeric_input,environment_numeric_input,limits:finite.limits,output_bytes:finite.output_bytes,query,
            query_semantics:"original closed endpoint segment; zero length certified point query; authored core/all filters; exact source matrix recipe and inverse enclosures; independently certified original-line/core point, occupied entry/exit enclosures and distance bounds; witness may follow entry; complete global admission/work/storage/output; uncertainty refuses; no motion or gameplay certificate",
        })
    }).transpose()?;
    let ray_intervals=request.ray_intervals.as_ref().map(|finite| {
        let numeric_input=self::ray_numeric_input(finite.ray);
        let environment_numeric_input=finite_environment(&request);
        let query=scene.ray_intervals(finite.ray,finite.limits).map_err(|error|format!(
            "collision solid intervals refused: {error}; ray_numeric_input={}; environment_numeric_input={}",
            serde_json::to_string(&numeric_input).expect("numeric audit"),serde_json::to_string(&environment_numeric_input).expect("numeric audit")))?;
        Ok::<_,String>(RayIntervalsReport {
            numeric_input,environment_numeric_input,limits:finite.limits,output_bytes:finite.output_bytes,query,
            query_semantics:"closed Sphere/Box/ConvexCuboid source cores only; exact signed-axis/power-of-two source similarities; complete kind/frame admission before scan; original caller direction/range, directed clipped occupied boundaries and independently certified source-core point; all filters/shell excluded; bounded global work/storage/output; no Capsule/Triangle occupancy or material/gameplay response",
        })
    }).transpose()?;
    let ray_hits = request
        .ray
        .map(|ray| scene.ray_cast(ray, QueryBudget::default()))
        .transpose()
        .map_err(|error| {
            // Refusals still expose consumed words for an independent numeric
            // audit, without creating a usable hit or partial output report.
            format!(
                "collision ray refused: {error}; ray_numeric_input={}",
                serde_json::to_string(&ray_numeric_input).expect("string-only numeric audit")
            )
        })?
        .unwrap_or_default();
    let first_ray = request.ray_first.map(|first| {
        let numeric_input = self::ray_numeric_input(first.ray);
        let hit = scene.ray_first(first.ray,first.budget).map_err(|error| {
            format!("collision first ray refused: {error}; ray_numeric_input={}",serde_json::to_string(&numeric_input).expect("string-only numeric audit"))
        })?;
        Ok::<_,String>(FirstRayReport {
            numeric_input,
            budget:first.budget,
            hit,
            query_semantics:"existing authored core predicates and original bounded ray; one distance/source-ordered result; uncertifiably farther candidates remain visited; complete result refuses numerical uncertainty or exhausted cumulative work; no retail movement or exact entry-distance certificate",
        })
    }).transpose()?;
    let overlap_hits = request
        .overlap
        .map(|s| scene.overlap_sphere(s.center, s.radius, QueryBudget::default()))
        .transpose()
        .map_err(|error| {
            format!(
                "collision overlap refused: {error}; overlap_numeric_input={}",
                serde_json::to_string(&overlap_numeric_input).expect("string-only numeric audit")
            )
        })?
        .unwrap_or_default();
    let report = QueryReport {
        source_sha256: format!("{:x}", Sha256::digest(&bytes)),
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        units: request.units,
        ray_numeric_input,
        overlap_numeric_input,
        first_ray,
        segment_cast,
        ray_intervals,
        primitive_count: scene.primitive_count(),
        ray_hits,
        overlap_hits,
        query_semantics: "authored core geometry; frozen bodies; all source filters included; two-sided triangles; certified convex cuboids use exact eight-corner vertex hull with source-f32 supporting-plane certificate; box/cuboid slabs include query distance range and conservative representable entry witness; uncertain cuboid predicates refuse; convex/packed shell margins excluded; source axes retained",
        faithful_ready: scene.faithful_ready(),
    };
    if let Some(limit) = output_limit {
        count_finite_report(&report,limit).map_err(|error| {
            format!("collision finite report refused: {error}; segment_numeric_input={}; ray_intervals_numeric_input={}; environment_numeric_input={}",
                serde_json::to_string(&report.segment_cast.as_ref().map(|v|&v.numeric_input)).expect("numeric audit"),
                serde_json::to_string(&report.ray_intervals.as_ref().map(|v|&v.numeric_input)).expect("numeric audit"),
                serde_json::to_string(&report.segment_cast.as_ref().map(|v|&v.environment_numeric_input).or_else(||report.ray_intervals.as_ref().map(|v|&v.environment_numeric_input))).expect("numeric audit"))
        })?;
    }
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepRequest {
    reference: std::num::NonZeroU64,
    body_blocks: Vec<u32>,
    attachment_rows: [[f64; 4]; 3],
    units: fallout_runtime::physics::EngineeringUnits,
    sweep: fallout_runtime::physics::sweep::SphereSweep,
    limits: fallout_runtime::physics::sweep::SweepLimits,
}
#[derive(Serialize)]
pub struct SweepReport {
    schema_version: u32,
    source_sha256: String,
    request_sha256: String,
    units: fallout_runtime::physics::EngineeringUnits,
    input: fallout_runtime::physics::sweep::SphereSweep,
    start_binary64_hex: [String; 3],
    end_binary64_hex: [String; 3],
    radius_binary64_hex: String,
    tolerance_binary64_hex: String,
    limits: fallout_runtime::physics::sweep::SweepLimits,
    primitive_count: usize,
    proposal: fallout_runtime::physics::sweep::SweepProposal,
    query_semantics: &'static str,
    faithful_ready: bool,
}
/// One decoder/build/query cohort, then count the complete report before emit.
pub fn sweep_query(input: &Path, request_path: &Path) -> Result<SweepReport> {
    use fallout_runtime::{
        identity::ReferenceId,
        physics::{BodyPlacement, QueryLimits, StaticScene},
    };
    let request_bytes = read_bounded(request_path, 1024 * 1024)?;
    let request: SweepRequest = serde_json::from_slice(&request_bytes)?;
    if request.body_blocks.is_empty() || request.body_blocks.len() > 10_000 {
        return Err("sweep requires 1..10000 selected source bodies".into());
    }
    let bytes = read_bounded(input, 64 * 1024 * 1024)?;
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
    let proposal = scene.sweep_sphere(request.sweep, request.limits)?;
    let report = SweepReport {
        schema_version: 1,
        source_sha256: format!("{:x}", Sha256::digest(&bytes)),
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        units: request.units,
        input: request.sweep,
        start_binary64_hex: request.sweep.start.map(|v| format!("{:016x}", v.to_bits())),
        end_binary64_hex: request.sweep.end.map(|v| format!("{:016x}", v.to_bits())),
        radius_binary64_hex: format!("{:016x}", request.sweep.radius.to_bits()),
        tolerance_binary64_hex: format!("{:016x}", request.sweep.contact_tolerance.to_bits()),
        limits: request.limits,
        primitive_count: scene.primitive_count(),
        proposal,
        query_semantics: "explicit zero-tagged frozen sphere-core fixture profile; exact composed signed-axis/power-of-two frames; exact Minkowski sum; original endpoint segment, no normalization; outward-rounded range/contact/center certificates; every selected obstacle checked; approximate source/dynamics/filter/margin semantics refuse; immutable engineering proposal only",
        faithful_ready: scene.faithful_ready(),
    };
    serde_json::to_writer_pretty(&mut ReportCounter(1), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CellRequest {
    model_index: usize,
    source_sha256: String,
    query: QueryRequest,
    /// Explicit offline engineering IO deadline, unrelated to game scheduling.
    io_deadline_ms: u64,
    #[serde(default)]
    verify_unload: bool,
}

pub fn cell_query(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    source_cache: Option<&Path>,
    editor_id: &str,
    request_path: &Path,
) -> Result<Value> {
    use fallout_data::{
        archive::NvArchive,
        vfs::MountIndex,
        world::{
            preparation::CellModelPlan,
            residency::{self, CellResidency, Stage},
        },
    };
    use fallout_runtime::{
        identity::ReferenceId,
        physics::{BodyPlacement, QueryBudget, QueryLimits, cell::CellCollision},
    };
    use std::time::{Duration, Instant};
    let request_bytes = read_bounded(request_path, 1024 * 1024)?;
    let request: CellRequest = serde_json::from_slice(&request_bytes)?;
    if request.query.segment_cast.is_some() || request.query.ray_intervals.is_some() {
        return Err("resident cell request does not support raw-source finite query forms".into());
    }
    if !(1..=60_000).contains(&request.io_deadline_ms)
        || request.query.body_blocks.is_empty()
        || request.query.body_blocks.len() > 10_000
        || (request.query.ray.is_none() && request.query.overlap.is_none())
        || request.source_sha256.len() != 64
        || !request.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(
            "cell collision needs explicit bounded IO deadline, source SHA, bodies and query"
                .into(),
        );
    }
    let expected_sha = std::array::from_fn(|i| {
        u8::from_str_radix(&request.source_sha256[2 * i..2 * i + 2], 16).expect("checked ASCII hex")
    });
    let order = crate::inspection_input::Order::read(order_path)?;
    let mut store = order.store(install, index_cache)?;
    let root = store.cell_by_editor_id(editor_id.as_bytes())?.0;
    let mut mounts = MountIndex::default();
    for path in crate::data_files(install, &["bsa"])? {
        NvArchive::open(&path)?.census(&mut mounts)?;
    }
    let plan = CellModelPlan::load(&mut store, &root, &mounts, Default::default())?;
    let mut world = CellResidency::new(
        install,
        source_cache,
        residency::Limits {
            workers: 1,
            ..Default::default()
        },
    )?;
    let ticket = world.request(plan)?;
    let deadline = Instant::now() + Duration::from_millis(request.io_deadline_ms);
    loop {
        let snapshot = world.poll()?;
        if snapshot.stage == Stage::Decoded {
            break;
        }
        if snapshot.stage == Stage::Failed {
            return Err(format!("cell collision source IO failed: {:?}", snapshot.failure).into());
        }
        if Instant::now() >= deadline {
            return Err("cell collision source IO deadline exhausted".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let selected_model = {
        let sources = world.sources(&ticket)?;
        let plan = sources.plan()?;
        let source = plan
            .receipt()
            .requests
            .get(request.model_index)
            .ok_or("resident model receipt index out of range")?;
        serde_json::to_value(source)?
    };
    let placements: Vec<_> = request
        .query
        .body_blocks
        .iter()
        .map(|&body_block| BodyPlacement {
            reference: ReferenceId(request.query.reference),
            source_sha256: expected_sha,
            body_block,
            attachment_to_source: fallout_data::coordinates::Affine {
                rows: request.query.attachment_rows,
            },
        })
        .collect();
    let mut collision = CellCollision::default();
    let scope = collision.admit(
        &mut world,
        &ticket,
        request.model_index,
        &placements,
        request.query.units,
        QueryLimits::default(),
    )?;
    let ray_numeric_input = request.query.ray.map(ray_numeric_input);
    let overlap_numeric_input = request.query.overlap.as_ref().map(overlap_numeric_input);
    let ray = if let Some(ray) = request.query.ray {
        Some(
            collision
                .ray_cast(&world, ray, QueryBudget::default())
                .map_err(|error| {
                    format!(
                        "cell collision ray refused: {error}; ray_numeric_input={}",
                        serde_json::to_string(&ray_numeric_input)
                            .expect("string-only numeric audit")
                    )
                })?,
        )
    } else {
        None
    };
    let overlap = if let Some(s) = request.query.overlap {
        Some(
            collision
                .overlap_sphere(&world, s.center, s.radius, QueryBudget::default())
                .map_err(|error| {
                    format!(
                        "cell collision overlap refused: {error}; overlap_numeric_input={}",
                        serde_json::to_string(&overlap_numeric_input)
                            .expect("string-only numeric audit")
                    )
                })?,
        )
    } else {
        None
    };
    // Serialize only validated borrowed hits while the owner is current.
    let ray_hits = serde_json::to_value(
        ray.as_ref()
            .map(|r| r.hits(&world))
            .transpose()?
            .unwrap_or(&[]),
    )?;
    let overlap_hits = serde_json::to_value(
        overlap
            .as_ref()
            .map(|r| r.hits(&world))
            .transpose()?
            .unwrap_or(&[]),
    )?;
    let before = world.snapshot();
    let primitive_count = collision.retained_primitive_count();
    let unload = if request.verify_unload {
        world.unload()?;
        let receipts_refuse = ray.as_ref().is_none_or(|r| r.hits(&world).is_err())
            && overlap.as_ref().is_none_or(|r| r.hits(&world).is_err());
        let geometry_released =
            collision.invalidate(&world) && collision.retained_primitive_count() == 0;
        let after = world.poll()?;
        if !receipts_refuse
            || !geometry_released
            || after.pinned_source_bytes != 0
            || after.retained_plans != 0
            || after.outstanding != 0
        {
            return Err(
                "cell collision unload did not revoke queries and release owned pins".into(),
            );
        }
        Some(
            serde_json::json!({"receipts_refuse":receipts_refuse,"geometry_released":geometry_released,"residency":after}),
        )
    } else {
        None
    };
    Ok(
        serde_json::json!({"schema_version":1,"scope":scope,"request_sha256":format!("{:x}",Sha256::digest(&request_bytes)),
        "load_order_sha256":order.sha256,"sources":store.source_receipts()?,"selected_model":selected_model,"primitive_count":primitive_count,
        "ray_hits":ray_hits,"overlap_hits":overlap_hits,"ray_numeric_input":ray_numeric_input,"overlap_numeric_input":overlap_numeric_input,"residency":before,"unload":unload,"faithful_ready":false,
        "query_semantics":"selected source-model engineering geometry scoped to cell owner/epoch; explicit caller attachment/reference; canonical pose/revision binding unimplemented; whole-cell faithful collision Unsupported"}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceBodyRequest {
    reference: std::num::NonZeroU64,
    authored: fallout_data::identity::FormKey,
    body_blocks: Vec<u32>,
    attachment_rows: [[f64; 4]; 3],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceRequest {
    model_index: usize,
    source_sha256: String,
    placements: Vec<ReferenceBodyRequest>,
    units: fallout_runtime::physics::EngineeringUnits,
    ray: Option<fallout_runtime::physics::Ray>,
    overlap: Option<SphereRequest>,
    io_deadline_ms: u64,
    #[serde(default)]
    verify_unload: bool,
}
#[derive(Serialize)]
pub struct ReferenceReport {
    schema_version: u32,
    scope: fallout_runtime::physics::reference::Scope,
    native_load: fallout_runtime::save::LoadReceipt,
    canonical_snapshot_sha256: String,
    canonical_snapshot_unchanged: bool,
    request_sha256: String,
    load_order_sha256: String,
    sources: Vec<fallout_data::store::SourceReceipt>,
    selected_model: Value,
    primitive_count: usize,
    ray_hits: Vec<fallout_runtime::physics::Hit>,
    overlap_hits: Vec<fallout_runtime::physics::Hit>,
    ray_numeric_input: Option<RayNumericInput>,
    overlap_numeric_input: Option<OverlapNumericInput>,
    residency: fallout_data::world::residency::Snapshot,
    unload: Option<Value>,
    query_semantics: &'static str,
    faithful_ready: bool,
}

// Count the complete pretty report before emit allocates it or opens an output.
// Failure leaves no report file and cannot publish a partial successful report.
pub(super) struct ReportCounter(pub(super) usize);
impl std::io::Write for ReportCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|v| *v <= 64 * 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("inspection report exceeds 64 MiB"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// Resolve the nearest existing parent without creating a cache. Canonicalizing
// it also catches an existing link into either protected input tree.
fn writable_destination(path: &Path, protected: &[PathBuf]) -> Result<()> {
    let mut ancestor = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
    }
    let parent = ancestor.canonicalize()?;
    if protected.iter().any(|root| parent.starts_with(root)) {
        return Err(
            "reference collision report/cache must be outside installation and input repository"
                .into(),
        );
    }
    // A cache may already exist: inspect its resolved destination too.
    if path.exists() {
        let destination = path.canonicalize()?;
        if protected.iter().any(|root| destination.starts_with(root)) {
            return Err("reference collision report/cache resolves into protected input".into());
        }
    }
    Ok(())
}

/// Read an existing source-bound native World and join explicit engineering
/// placements. This consumer never creates a World or registers a reference.
#[allow(clippy::too_many_arguments)]
pub fn reference_query(
    install: &Path,
    order_path: &Path,
    save_root: &Path,
    index_cache: Option<&Path>,
    source_cache: Option<&Path>,
    editor_id: &str,
    request_path: &Path,
    output: Option<&Path>,
) -> Result<ReferenceReport> {
    use fallout_data::{
        archive::NvArchive,
        loaded_scripts,
        vfs::MountIndex,
        world::{
            preparation::CellModelPlan,
            residency::{self, CellResidency, Stage},
        },
    };
    use fallout_runtime::{
        identity::ReferenceId,
        physics::{
            BodyPlacement, QueryBudget,
            reference::{self, ReferenceCollision, ReferencePlacement},
        },
        save::{Recovery, Repository},
    };
    use std::time::{Duration, Instant};
    let protected = [crate::protected_tree(install)?, save_root.canonicalize()?];
    for path in [output, index_cache, source_cache].into_iter().flatten() {
        writable_destination(path, &protected)?;
    }
    let request_bytes = read_bounded(request_path, 1024 * 1024)?;
    let request: ReferenceRequest = serde_json::from_slice(&request_bytes)?;
    let body_count = request
        .placements
        .iter()
        .try_fold(0usize, |n, p| {
            if p.body_blocks.is_empty() {
                return None;
            }
            n.checked_add(p.body_blocks.len())
        })
        .ok_or("reference collision body count invalid")?;
    if body_count == 0
        || body_count > 1024
        || !(1..=60_000).contains(&request.io_deadline_ms)
        || (request.ray.is_none() && request.overlap.is_none())
        || request.source_sha256.len() != 64
        || !request.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("reference collision needs bounded explicit placements, source SHA, IO deadline and query".into());
    }
    let expected_sha = std::array::from_fn(|i| {
        u8::from_str_radix(&request.source_sha256[2 * i..2 * i + 2], 16).expect("checked ASCII hex")
    });
    let order = crate::inspection_input::Order::read(order_path)?;
    let mut source_bytes = 0u64;
    for name in &order.names {
        fallout_data::identity::plugin_name(name)?;
        source_bytes = source_bytes
            .checked_add(std::fs::metadata(install.join("Data").join(name))?.len())
            .ok_or("reference collision source bytes overflow")?;
        if source_bytes > reference::Limits::default().source_bytes {
            return Err("reference collision plugin cohort exceeds 4 GiB".into());
        }
    }
    let index_limits = fallout_data::plugin::Limits {
        max_records: 4_000_000 / order.names.len() as u64,
        max_record_bytes: 4 * 1024 * 1024,
        max_decoded_bytes: 4 * 1024 * 1024 * 1024 / order.names.len() as u64,
        ..Default::default()
    };
    let data = install.join("Data");
    let mut store = if let Some(cache) = index_cache {
        fallout_data::store::RecordStore::open_nv_headers_cached(
            &data,
            &order.names,
            index_limits,
            cache,
        )?
    } else {
        fallout_data::store::RecordStore::open_nv_headers(&data, &order.names, index_limits)?
    };
    // Fixed phase ceilings bound this whole consumer: catalogue reads 64 MiB,
    // plan reads 64 MiB, reference admission (including plan/model) 64 MiB.
    // The phases do not multiply their allowances by placement/query count.
    let catalogue = loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits {
            max_candidate_read_bytes: 64 * 1024 * 1024,
            max_candidate_record_bytes: 4 * 1024 * 1024,
            max_retained_bytes: 16 * 1024 * 1024,
            ..Default::default()
        },
        |_, _| Ok(()),
    )?;
    let repository = Repository::open(save_root, &[install.into()])?;
    let (world, native_load) = repository.load(
        &catalogue,
        fallout_runtime::Limits::default(),
        Recovery::Strict,
    )?;
    let before = world.snapshot();
    let canonical_bytes = before.encode(fallout_runtime::Limits::default().max_snapshot_bytes)?;
    let canonical_snapshot_sha256 = format!("{:x}", Sha256::digest(&canonical_bytes));
    let sources = store.source_receipts()?;
    let root = store.cell_by_editor_id(editor_id.as_bytes())?.0;
    let mut mounts = MountIndex::default();
    let archives = crate::data_files(install, &["bsa"])?;
    if archives.len() > 8 {
        return Err("reference collision archive count exceeds eight".into());
    }
    let archive_bytes = archives.iter().try_fold(0u64, |sum, path| {
        sum.checked_add(std::fs::metadata(path)?.len())
            .ok_or_else(|| std::io::Error::other("archive bytes overflow"))
    })?;
    if archive_bytes > 16 * 1024 * 1024 * 1024 {
        return Err("reference collision archive sources exceed 16 GiB".into());
    }
    for path in archives {
        NvArchive::open(&path)?.census(&mut mounts)?;
    }
    let plan = CellModelPlan::load(&mut store, &root, &mounts, Default::default())?;
    let mut owner = CellResidency::new(
        install,
        source_cache,
        residency::Limits {
            workers: 1,
            source_bytes: 64 * 1024 * 1024,
            ..Default::default()
        },
    )?;
    let ticket = owner.request(plan)?;
    let deadline = Instant::now() + Duration::from_millis(request.io_deadline_ms);
    loop {
        let snapshot = owner.poll()?;
        if snapshot.stage == Stage::Decoded {
            break;
        }
        if snapshot.stage == Stage::Failed {
            return Err(format!(
                "reference collision source IO failed: {:?}",
                snapshot.failure
            )
            .into());
        }
        if Instant::now() >= deadline {
            return Err("reference collision source IO deadline exhausted".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let selected_model = {
        let resident = owner.sources(&ticket)?;
        let plan = resident.plan()?;
        serde_json::to_value(
            plan.receipt()
                .requests
                .get(request.model_index)
                .ok_or("resident model receipt index out of range")?,
        )?
    };
    let mut placements = Vec::with_capacity(body_count);
    for p in &request.placements {
        for &body_block in &p.body_blocks {
            placements.push(ReferencePlacement {
                authored: p.authored.clone(),
                body: BodyPlacement {
                    reference: ReferenceId(p.reference),
                    source_sha256: expected_sha,
                    body_block,
                    attachment_to_source: fallout_data::coordinates::Affine {
                        rows: p.attachment_rows,
                    },
                },
            });
        }
    }
    let mut collision = ReferenceCollision::default();
    let scope = collision.admit(
        &world,
        &mut owner,
        &ticket,
        &mut store,
        request.model_index,
        &placements,
        request.units,
        reference::Limits::default(),
    )?;
    let ray_numeric_input = request.ray.map(ray_numeric_input);
    let overlap_numeric_input = request.overlap.as_ref().map(overlap_numeric_input);
    let queries = usize::from(request.ray.is_some()) + usize::from(request.overlap.is_some());
    let total = QueryBudget::default();
    let budget = reference::Budget {
        reference_checks: 4096 / queries,
        geometry: QueryBudget {
            primitive_tests: total.primitive_tests / queries,
            geometry_tests: total.geometry_tests / queries,
            hits: total.hits / queries,
        },
    };
    let ray = request
        .ray
        .map(|r| collision.ray_cast(&world, &owner, r, budget))
        .transpose()
        .map_err(|e| {
            format!(
                "reference collision ray refused: {e}; ray_numeric_input={}",
                serde_json::to_string(&ray_numeric_input).expect("numeric audit")
            )
        })?;
    let overlap = request
        .overlap
        .map(|s| collision.overlap_sphere(&world, &owner, s.center, s.radius, budget))
        .transpose()
        .map_err(|e| {
            format!(
                "reference collision overlap refused: {e}; overlap_numeric_input={}",
                serde_json::to_string(&overlap_numeric_input).expect("numeric audit")
            )
        })?;
    let ray_hits = ray
        .as_ref()
        .map(|r| r.hits(&world, &owner))
        .transpose()?
        .unwrap_or(&[])
        .to_vec();
    let overlap_hits = overlap
        .as_ref()
        .map(|r| r.hits(&world, &owner))
        .transpose()?
        .unwrap_or(&[])
        .to_vec();
    let residency = owner.snapshot();
    let primitive_count = collision.retained_primitive_count();
    let unload = if request.verify_unload {
        owner.unload()?;
        let receipts_refuse = ray.as_ref().is_none_or(|r| r.hits(&world, &owner).is_err())
            && overlap
                .as_ref()
                .is_none_or(|r| r.hits(&world, &owner).is_err());
        let geometry_released =
            collision.invalidate(&world, &owner) && collision.retained_primitive_count() == 0;
        let after = owner.poll()?;
        if !receipts_refuse
            || !geometry_released
            || after.pinned_source_bytes != 0
            || after.retained_plans != 0
            || after.outstanding != 0
        {
            return Err(
                "reference collision unload did not revoke results and release owned pins".into(),
            );
        }
        Some(
            serde_json::json!({"receipts_refuse":receipts_refuse,"geometry_released":geometry_released,"residency":after}),
        )
    } else {
        None
    };
    if world.snapshot() != before {
        return Err("reference collision changed canonical World".into());
    }
    let report = ReferenceReport {
        schema_version: 1,
        scope,
        native_load,
        canonical_snapshot_sha256,
        canonical_snapshot_unchanged: true,
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        load_order_sha256: order.sha256,
        sources,
        selected_model,
        primitive_count,
        ray_hits,
        overlap_hits,
        ray_numeric_input,
        overlap_numeric_input,
        residency,
        unload,
        query_semantics: "selected core engineering geometry; exact canonical reference/source join; explicit caller query frame distinct from source DATA and saved canonical pose; all filters retained; whole-cell collision Unsupported; immutable World",
        faithful_ready: false,
    };
    serde_json::to_writer_pretty(&mut ReportCounter(1), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiBodyRequest {
    reference: std::num::NonZeroU64,
    body_blocks: Vec<u32>,
    attachment_rows: [[f64; 4]; 3],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiModelRequest {
    model_index: usize,
    source_sha256: String,
    placements: Vec<MultiBodyRequest>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QueryWork {
    primitive_tests: usize,
    geometry_tests: usize,
    hits: usize,
}
impl Default for QueryWork {
    fn default() -> Self {
        let b = fallout_runtime::physics::QueryBudget::default();
        Self {
            primitive_tests: b.primitive_tests,
            geometry_tests: b.geometry_tests,
            hits: b.hits,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MultiRequest {
    models: Vec<MultiModelRequest>,
    units: fallout_runtime::physics::EngineeringUnits,
    ray: Option<fallout_runtime::physics::Ray>,
    overlap: Option<SphereRequest>,
    io_deadline_ms: u64,
    #[serde(default)]
    verify_unload: bool,
    #[serde(default)]
    query_budget: QueryWork,
}
#[derive(Serialize)]
pub struct MultiReport {
    schema_version: u32,
    scope: fallout_runtime::physics::multi::Scope,
    request_sha256: String,
    load_order_sha256: String,
    sources: Vec<fallout_data::store::SourceReceipt>,
    selected_models: Vec<Value>,
    primitive_count: usize,
    query_budget: QueryWork,
    ray_hits: Vec<fallout_runtime::physics::Hit>,
    overlap_hits: Vec<fallout_runtime::physics::Hit>,
    ray_numeric_input: Option<RayNumericInput>,
    overlap_numeric_input: Option<OverlapNumericInput>,
    residency: fallout_data::world::residency::Snapshot,
    unload: Option<Value>,
    query_semantics: &'static str,
    faithful_ready: bool,
}

/// One explicit selected subset, one retained source lease and one scene/index.
/// Engineering references/frames do not create or mutate canonical World state.
pub fn multi_query(
    install: &Path,
    order_path: &Path,
    index_cache: Option<&Path>,
    source_cache: Option<&Path>,
    editor_id: &str,
    request_path: &Path,
) -> Result<MultiReport> {
    use fallout_data::{
        archive::NvArchive,
        vfs::MountIndex,
        world::{
            preparation::CellModelPlan,
            residency::{self, CellResidency, Stage},
        },
    };
    use fallout_runtime::{
        identity::ReferenceId,
        physics::{
            BodyPlacement, QueryBudget,
            multi::{self, CellSelection, ModelSelection},
        },
    };
    use std::time::{Duration, Instant};
    let request_bytes = read_bounded(request_path, 1024 * 1024)?;
    let request: MultiRequest = serde_json::from_slice(&request_bytes)?;
    if request.models.is_empty()
        || request.models.len() > 64
        || !(1..=60_000).contains(&request.io_deadline_ms)
        || (request.ray.is_none() && request.overlap.is_none())
    {
        return Err(
            "multi-model collision needs 1..64 models, bounded IO deadline and a query".into(),
        );
    }
    let max = QueryWork::default();
    if request.query_budget.primitive_tests > max.primitive_tests
        || request.query_budget.geometry_tests > max.geometry_tests
        || request.query_budget.hits > max.hits
    {
        return Err("multi-model query budget exceeds engineering ceiling".into());
    }
    let mut count = 0usize;
    let mut selections = Vec::with_capacity(request.models.len());
    // Check total bodies and all textual SHA fields before constructing placements.
    for model in &request.models {
        if model.placements.is_empty()
            || model.source_sha256.len() != 64
            || !model.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("multi-model collision requires placements and exact source SHA".into());
        }
        for p in &model.placements {
            if p.body_blocks.is_empty() {
                return Err("multi-model collision body selection is empty".into());
            }
            count = count
                .checked_add(p.body_blocks.len())
                .ok_or("multi-model body count overflow")?;
            if count > 1024 {
                return Err("multi-model collision exceeds 1024 total body placements".into());
            }
        }
    }
    for model in &request.models {
        let sha = std::array::from_fn(|i| {
            u8::from_str_radix(&model.source_sha256[2 * i..2 * i + 2], 16)
                .expect("checked ASCII hex")
        });
        let mut placements = Vec::new();
        for p in &model.placements {
            for &body_block in &p.body_blocks {
                placements.push(BodyPlacement {
                    reference: ReferenceId(p.reference),
                    source_sha256: sha,
                    body_block,
                    attachment_to_source: fallout_data::coordinates::Affine {
                        rows: p.attachment_rows,
                    },
                });
            }
        }
        selections.push(ModelSelection {
            model_index: model.model_index,
            placements,
        });
    }
    let order = crate::inspection_input::Order::read(order_path)?;
    let mut plugin_bytes = 0u64;
    for name in &order.names {
        fallout_data::identity::plugin_name(name)?;
        plugin_bytes = plugin_bytes
            .checked_add(std::fs::metadata(install.join("Data").join(name))?.len())
            .ok_or("multi-model plugin bytes overflow")?;
        if plugin_bytes > 4 * 1024 * 1024 * 1024 {
            return Err("multi-model plugin cohort exceeds 4 GiB".into());
        }
    }
    let index_limits = fallout_data::plugin::Limits {
        max_records: 4_000_000 / order.names.len() as u64,
        max_record_bytes: 4 * 1024 * 1024,
        max_decoded_bytes: 4 * 1024 * 1024 * 1024 / order.names.len() as u64,
        ..Default::default()
    };
    let data = install.join("Data");
    let mut store = if let Some(cache) = index_cache {
        fallout_data::store::RecordStore::open_nv_headers_cached(
            &data,
            &order.names,
            index_limits,
            cache,
        )?
    } else {
        fallout_data::store::RecordStore::open_nv_headers(&data, &order.names, index_limits)?
    };
    let sources = store.source_receipts()?;
    let root = store.cell_by_editor_id(editor_id.as_bytes())?.0;
    let archives = crate::data_files(install, &["bsa"])?;
    if archives.len() > 8 {
        return Err("multi-model archive count exceeds eight".into());
    }
    let archive_bytes = archives.iter().try_fold(0u64, |n, p| {
        n.checked_add(std::fs::metadata(p)?.len())
            .ok_or_else(|| std::io::Error::other("archive bytes overflow"))
    })?;
    if archive_bytes > 16 * 1024 * 1024 * 1024 {
        return Err("multi-model archive sources exceed 16 GiB".into());
    }
    let mut mounts = MountIndex::default();
    for path in archives {
        NvArchive::open(&path)?.census(&mut mounts)?;
    }
    let plan = CellModelPlan::load(&mut store, &root, &mounts, Default::default())?;
    let mut owner = CellResidency::new(
        install,
        source_cache,
        residency::Limits {
            workers: 1,
            source_bytes: 64 * 1024 * 1024,
            ..Default::default()
        },
    )?;
    let ticket = owner.request(plan)?;
    let deadline = Instant::now() + Duration::from_millis(request.io_deadline_ms);
    loop {
        let state = owner.poll()?;
        if state.stage == Stage::Decoded {
            break;
        }
        if state.stage == Stage::Failed {
            return Err(format!("multi-model collision IO failed: {:?}", state.failure).into());
        }
        if Instant::now() >= deadline {
            return Err("multi-model collision IO deadline exhausted".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let selected_models = {
        let resident = owner.sources(&ticket)?;
        let plan = resident.plan()?;
        let mut rows = Vec::with_capacity(request.models.len());
        for model in &request.models {
            rows.push(serde_json::to_value(
                plan.receipt()
                    .requests
                    .get(model.model_index)
                    .ok_or("resident model receipt index out of range")?,
            )?);
        }
        rows
    };
    let mut collision = CellSelection::default();
    let scope = collision.admit(
        &mut owner,
        &ticket,
        &selections,
        request.units,
        multi::Limits::default(),
    )?;
    let ray_numeric_input = request.ray.map(ray_numeric_input);
    let overlap_numeric_input = request.overlap.as_ref().map(overlap_numeric_input);
    let queries = usize::from(request.ray.is_some()) + usize::from(request.overlap.is_some());
    let budget = QueryBudget {
        primitive_tests: request.query_budget.primitive_tests / queries,
        geometry_tests: request.query_budget.geometry_tests / queries,
        hits: request.query_budget.hits / queries,
    };
    let ray = request
        .ray
        .map(|r| collision.ray_cast(&owner, r, budget))
        .transpose()
        .map_err(|e| {
            format!(
                "multi-model ray refused: {e}; ray_numeric_input={}",
                serde_json::to_string(&ray_numeric_input).expect("numeric audit")
            )
        })?;
    let overlap = request
        .overlap
        .map(|s| collision.overlap_sphere(&owner, s.center, s.radius, budget))
        .transpose()
        .map_err(|e| {
            format!(
                "multi-model overlap refused: {e}; overlap_numeric_input={}",
                serde_json::to_string(&overlap_numeric_input).expect("numeric audit")
            )
        })?;
    let ray_hits = ray
        .as_ref()
        .map(|r| r.hits(&owner))
        .transpose()?
        .unwrap_or(&[])
        .to_vec();
    let overlap_hits = overlap
        .as_ref()
        .map(|r| r.hits(&owner))
        .transpose()?
        .unwrap_or(&[])
        .to_vec();
    let residency = owner.snapshot();
    let primitive_count = collision.retained_primitive_count();
    let unload = if request.verify_unload {
        owner.unload()?;
        let receipts_refuse = ray.as_ref().is_none_or(|r| r.hits(&owner).is_err())
            && overlap.as_ref().is_none_or(|r| r.hits(&owner).is_err());
        let geometry_released =
            collision.invalidate(&owner) && collision.retained_primitive_count() == 0;
        let after = owner.poll()?;
        if !receipts_refuse
            || !geometry_released
            || after.pinned_source_bytes != 0
            || after.retained_plans != 0
            || after.outstanding != 0
        {
            return Err(
                "multi-model unload did not revoke all receipts and release owned pins".into(),
            );
        }
        Some(
            serde_json::json!({"receipts_refuse":receipts_refuse,"geometry_released":geometry_released,"residency":after}),
        )
    } else {
        None
    };
    let report = MultiReport {
        schema_version: 1,
        scope,
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        load_order_sha256: order.sha256,
        sources,
        selected_models,
        primitive_count,
        query_budget: request.query_budget,
        ray_hits,
        overlap_hits,
        ray_numeric_input,
        overlap_numeric_input,
        residency,
        unload,
        faithful_ready: false,
        query_semantics: "explicit resident model subset; one frozen source-core scene/index and global query budget; all source filters retained; engineering reference/frame inputs; whole-cell collision Unsupported; no canonical World mutation",
    };
    serde_json::to_writer_pretty(&mut ReportCounter(1), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentLimits {
    source_bytes: usize,
    blocks: usize,
    decoded_metadata_bytes: usize,
    array_bytes: usize,
    source_link_visits: usize,
    ancestry_visits: usize,
    scope_metadata_bytes: usize,
}
impl From<AttachmentLimits> for fallout_runtime::physics::attachment::Limits {
    fn from(v: AttachmentLimits) -> Self {
        Self {
            source_bytes: v.source_bytes,
            blocks: v.blocks,
            decoded_metadata_bytes: v.decoded_metadata_bytes,
            array_bytes: v.array_bytes,
            source_link_visits: v.source_link_visits,
            ancestry_visits: v.ancestry_visits,
            scope_metadata_bytes: v.scope_metadata_bytes,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttachmentRequest {
    reference: std::num::NonZeroU64,
    source_sha256: String,
    collision_object: u32,
    body_block: u32,
    target_block: u32,
    placement_rows: [[f64; 4]; 3],
    units: fallout_runtime::physics::EngineeringUnits,
    ray: Option<fallout_runtime::physics::Ray>,
    overlap: Option<SphereRequest>,
    limits: Option<AttachmentLimits>,
    #[serde(default)]
    query_budget: QueryWork,
}
#[derive(Serialize)]
pub struct AttachmentReport {
    schema_version: u32,
    input: PathBuf,
    request_sha256: String,
    scope: fallout_runtime::physics::attachment::Scope,
    query_budget: QueryWork,
    primitive_count: usize,
    ray_hits: Vec<fallout_runtime::physics::Hit>,
    overlap_hits: Vec<fallout_runtime::physics::Hit>,
    ray_numeric_input: Option<RayNumericInput>,
    overlap_numeric_input: Option<OverlapNumericInput>,
    query_semantics: &'static str,
    faithful_ready: bool,
}
/// Select one exact source collision object. Its diagnostic scope distinguishes
/// separate shared-body occurrences without changing the existing SourceId API.
pub fn attachment_query(input: &Path, request_path: &Path) -> Result<AttachmentReport> {
    use fallout_runtime::physics::{
        QueryBudget,
        attachment::{Selection, SourceAttachment},
    };
    let request_bytes = read_bounded(request_path, 1024 * 1024)?;
    let request: AttachmentRequest = serde_json::from_slice(&request_bytes)?;
    if request.source_sha256.len() != 64
        || !request.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("attachment query requires exact source SHA".into());
    }
    let queries = usize::from(request.ray.is_some()) + usize::from(request.overlap.is_some());
    if queries == 0 {
        return Err("attachment query requires a ray and/or overlap".into());
    }
    let ceiling = QueryWork::default();
    if request.query_budget.primitive_tests > ceiling.primitive_tests
        || request.query_budget.geometry_tests > ceiling.geometry_tests
        || request.query_budget.hits > ceiling.hits
    {
        return Err("attachment query budget exceeds engineering ceiling".into());
    }
    let limits = request.limits.map(Into::into).unwrap_or_default();
    let bytes = read_bounded(input, 4 * 1024 * 1024)?;
    let source_sha256 = std::array::from_fn(|i| {
        u8::from_str_radix(&request.source_sha256[2 * i..2 * i + 2], 16).expect("checked ASCII hex")
    });
    let attachment = SourceAttachment::derive(
        &bytes,
        Selection {
            reference: fallout_runtime::identity::ReferenceId(request.reference),
            source_sha256,
            collision_object: request.collision_object,
            body_block: request.body_block,
            target_block: request.target_block,
            placement_to_source: fallout_data::coordinates::Affine {
                rows: request.placement_rows,
            },
        },
        request.units,
        limits,
    )?;
    let scene = attachment.build_scene(Default::default())?;
    let budget = QueryBudget {
        primitive_tests: request.query_budget.primitive_tests / queries,
        geometry_tests: request.query_budget.geometry_tests / queries,
        hits: request.query_budget.hits / queries,
    };
    let ray_numeric_input = request.ray.map(ray_numeric_input);
    let overlap_numeric_input = request.overlap.as_ref().map(overlap_numeric_input);
    let ray_hits = request
        .ray
        .map(|r| scene.ray_cast(r, budget))
        .transpose()
        .map_err(|e| {
            format!(
                "attachment ray refused: {e}; ray_numeric_input={}",
                serde_json::to_string(&ray_numeric_input).expect("numeric audit")
            )
        })?
        .unwrap_or_default();
    let overlap_hits = request
        .overlap
        .map(|s| scene.overlap_sphere(s.center, s.radius, budget))
        .transpose()
        .map_err(|e| {
            format!(
                "attachment overlap refused: {e}; overlap_numeric_input={}",
                serde_json::to_string(&overlap_numeric_input).expect("numeric audit")
            )
        })?
        .unwrap_or_default();
    let report = AttachmentReport {
        schema_version: 1,
        input: input.to_owned(),
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        scope: attachment.scope().clone(),
        query_budget: request.query_budget,
        primitive_count: scene.primitive_count(),
        ray_hits,
        overlap_hits,
        ray_numeric_input,
        overlap_numeric_input,
        faithful_ready: false,
        query_semantics: "one exact collision-object occurrence; supported static source ancestry and explicit caller placement composed once; active body/shape units and predicates unchanged; raw source filters retained; no runtime pose, Havok attachment policy or whole-cell readiness",
    };
    serde_json::to_writer_pretty(&mut ReportCounter(1), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallout_data::nif_collision::{Block, Triangle};
    #[test]
    fn finite_report_budget_includes_the_existing_emitter_newline() {
        let literal = b"{\n  \"ok\": true\n}\n";
        let value = serde_json::json!({"ok":true});
        count_finite_report(&value, literal.len()).unwrap();
        assert!(count_finite_report(&value, literal.len() - 1).is_err());
        assert!(count_finite_report(&value, 0).is_err());
    }
    #[test]
    fn finite_output_counter_charges_all_writes_without_renewal() {
        use std::io::Write;
        let mut counter = FiniteReportCounter(4);
        counter.write_all(b"ab").unwrap();
        counter.write_all(b"cd").unwrap();
        assert!(counter.write_all(b"e").is_err());
        assert_eq!(counter.0, 0);
    }
    #[test]
    fn reference_output_counter_rejects_the_first_excess_byte() {
        use std::io::Write;
        let mut counter = ReportCounter(64 * 1024 * 1024 - 2);
        counter.write_all(&[0; 2]).unwrap();
        assert!(counter.write_all(&[0]).is_err());
    }
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
