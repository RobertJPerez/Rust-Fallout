//! Exact selected source endpoints; no nearest triangle or point adjustment.
//! The query borrows the protected Store until drop. Batches have no public
//! receipt constructor and are visible only through their live query owner.
use super::{
    TriangleId,
    corridor::{CorridorError, InputLimits, InputUsage, PlaneContract},
    source::SourceCohort,
};
use fallout_data::{
    identity::FormKey,
    plugin,
    store::{RecordStore, SourceReceipt},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, thiserror::Error)]
pub enum EndpointError {
    #[error(transparent)]
    Source(#[from] CorridorError),
    #[error(transparent)]
    Data(#[from] fallout_data::Error),
    #[error("endpoint budget exceeded: {0}")]
    Budget(&'static str),
    #[error("invalid source endpoint: {0}")]
    Invalid(&'static str),
    #[error("endpoint batch belongs to another or ended source query")]
    Stale,
}
pub type Result<T> = std::result::Result<T, EndpointError>;
fn debit(left: &mut usize, n: usize, reason: &'static str) -> Result<()> {
    *left = left.checked_sub(n).ok_or(EndpointError::Budget(reason))?;
    Ok(())
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointRequest {
    pub triangle: TriangleId,
    pub point: [f64; 3],
    pub plane: PlaneContract,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointLimits {
    pub requests: usize,
    pub source_visits: usize,
    pub predicate_tests: usize,
    pub geometry_bytes: usize,
    pub identity_bytes: usize,
}
impl Default for EndpointLimits {
    fn default() -> Self {
        Self {
            requests: 1024,
            source_visits: 100_000,
            predicate_tests: 10_000,
            geometry_bytes: 512 * 1024,
            identity_bytes: 8 * 1024 * 1024,
        }
    }
}
impl EndpointLimits {
    pub fn validate(self) -> Result<Self> {
        let max = Self::default();
        for (n, c) in [
            (self.requests, max.requests),
            (self.source_visits, max.source_visits),
            (self.predicate_tests, max.predicate_tests),
            (self.geometry_bytes, max.geometry_bytes),
            (self.identity_bytes, max.identity_bytes),
        ] {
            if n > c {
                return Err(EndpointError::Invalid("work ceiling"));
            }
        }
        Ok(self)
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Classification {
    Inside,
    OnEdge { edge: usize },
    OnVertex { vertex: usize },
    Outside { off_plane: bool },
    Unsupported { reason: &'static str },
}
impl Classification {
    pub fn contained(&self) -> bool {
        matches!(
            self,
            Self::Inside | Self::OnEdge { .. } | Self::OnVertex { .. }
        )
    }
}
#[derive(Debug, Serialize)]
pub struct EndpointObservation {
    pub triangle: TriangleId,
    pub cell: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub decoded_sha256: String,
    pub record_offset: u64,
    pub record_flags: u32,
    pub triangle_flags: u16,
    pub cover_flags: u16,
    pub vertex_indices: [u16; 3],
    pub raw_edges: [i16; 3],
    pub vertices: [[f32; 3]; 3],
    pub vertex_words: [[u32; 3]; 3],
    pub point: [f64; 3],
    pub point_words: [u64; 3],
    pub plane: PlaneContract,
    pub classification: Classification,
}
#[derive(Debug, Serialize)]
pub struct EndpointUsage {
    pub requests: usize,
    pub source_visits: usize,
    pub predicate_tests: usize,
    pub geometry_bytes: usize,
    pub identity_bytes: usize,
}
struct Authority {
    live: AtomicBool,
}
/// The data remains private even after owner release; a serialized observation
/// cannot be deserialized into this authority-bearing batch.
pub struct EndpointBatch {
    authority: Arc<Authority>,
    observations: Vec<EndpointObservation>,
    usage: EndpointUsage,
}
#[derive(Serialize)]
struct CellBinding {
    key: FormKey,
    source_plugin: String,
    source_sha256: String,
    header: plugin::RecordHeader,
}
pub struct EndpointQuery<'store> {
    _store: &'store mut RecordStore,
    source: SourceCohort,
    sources: Vec<SourceReceipt>,
    cell: CellBinding,
    scope_sha256: String,
    authority: Arc<Authority>,
}
struct HashWriter<'a>(&'a mut Sha256);
impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'store> EndpointQuery<'store> {
    pub fn load(
        store: &'store mut RecordStore,
        cell: &FormKey,
        limits: InputLimits,
    ) -> Result<Self> {
        let source = SourceCohort::load(store, cell, limits, false)?;
        let at = store
            .winner(cell)
            .ok_or(EndpointError::Invalid("CELL winner is missing"))?;
        let header = store.definition(at).header.clone();
        let source_plugin = store.source_name(at).to_owned();
        let source_sha256 = store.source_digest(at)?;
        let cell = CellBinding {
            key: cell.clone(),
            source_plugin,
            source_sha256,
            header,
        };
        let sources = store.source_receipts()?;
        let mut hash = Sha256::new();
        hash.update(b"fallout-source-endpoints-v1\0");
        serde_json::to_writer(HashWriter(&mut hash), &(&cell, &sources))
            .map_err(|_| EndpointError::Invalid("source scope serialization"))?;
        let scope_sha256 = format!("{:x}", hash.finalize());
        Ok(Self {
            _store: store,
            source,
            sources,
            cell,
            scope_sha256,
            authority: Arc::new(Authority {
                live: AtomicBool::new(true),
            }),
        })
    }
    pub fn source_usage(&self) -> &InputUsage {
        &self.source.usage
    }
    pub fn inspect(
        &self,
        requests: &[EndpointRequest],
        limits: EndpointLimits,
    ) -> Result<EndpointBatch> {
        if !self.authority.live.load(Ordering::Acquire) {
            return Err(EndpointError::Stale);
        }
        let limits = limits.validate()?;
        if requests.is_empty() || requests.len() > limits.requests {
            return Err(EndpointError::Budget("request count"));
        }
        let mut visits = limits.source_visits;
        let mut longest = self.source.cell.origin_plugin.len();
        for mesh in &self.source.meshes {
            debit(&mut visits, 1, "source name visits")?;
            longest = longest
                .max(mesh.key.origin_plugin.len())
                .max(mesh.source_plugin.len());
        }
        for request in requests {
            debit(&mut visits, 1, "request visits")?;
            longest = longest.max(request.triangle.mesh.origin_plugin.len());
            let PlaneContract::AxisAlignedDyadic { normal_axis } = request.plane;
            if normal_axis > 2 || request.point.iter().any(|v| !v.is_finite()) {
                return Err(EndpointError::Invalid(
                    "finite point and explicit plane axis required",
                ));
            }
        }
        let identity = longest
            .checked_mul(8)
            .and_then(|n| n.checked_add(4096))
            .and_then(|n| n.checked_mul(requests.len()))
            .and_then(|n| n.checked_add(4096))
            .ok_or(EndpointError::Budget("identity overflow"))?;
        let geometry = requests
            .len()
            .checked_mul(256)
            .ok_or(EndpointError::Budget("geometry overflow"))?;
        let mut left = limits.identity_bytes;
        debit(&mut left, identity, "retained identities")?;
        let mut left = limits.geometry_bytes;
        debit(&mut left, geometry, "retained geometry")?;
        let mut predicates = limits.predicate_tests;
        let mut observations = Vec::with_capacity(requests.len());
        for request in requests {
            let mut selected = None;
            for mesh in &self.source.meshes {
                debit(&mut visits, 1, "source mesh joins")?;
                if mesh.key == request.triangle.mesh {
                    selected = Some(mesh);
                    break;
                }
            }
            let mesh = selected.ok_or(EndpointError::Invalid(
                "selected source mesh is unavailable",
            ))?;
            debit(&mut visits, 1, "source triangle visits")?;
            let triangle = mesh.mesh.triangles.get(request.triangle.triangle).ok_or(
                EndpointError::Invalid("selected source triangle is unavailable"),
            )?;
            if mesh.cell.as_ref() != Some(&self.source.cell) {
                return Err(EndpointError::Invalid("source triangle CELL join differs"));
            }
            let mut vertices = [[0f32; 3]; 3];
            for (out, index) in vertices.iter_mut().zip(triangle.vertices) {
                debit(&mut visits, 1, "source vertex visits")?;
                *out = *mesh
                    .mesh
                    .vertices
                    .get(usize::from(index))
                    .ok_or(EndpointError::Invalid("source vertex index differs"))?;
            }
            let classification = classify(vertices, request.point, request.plane, &mut predicates)?;
            observations.push(EndpointObservation {
                triangle: request.triangle.clone(),
                cell: self.source.cell.clone(),
                source_plugin: mesh.source_plugin.clone(),
                source_sha256: mesh.source_sha256.clone(),
                decoded_sha256: mesh.mesh.decoded_sha256.clone(),
                record_offset: mesh.mesh.header.offset,
                record_flags: mesh.mesh.header.flags,
                triangle_flags: triangle.flags,
                cover_flags: triangle.cover_flags,
                vertex_indices: triangle.vertices,
                raw_edges: triangle.edges,
                vertices,
                vertex_words: vertices.map(|p| p.map(f32::to_bits)),
                point: request.point,
                point_words: request.point.map(f64::to_bits),
                plane: request.plane,
                classification,
            });
        }
        if !self.authority.live.load(Ordering::Acquire) {
            return Err(EndpointError::Stale);
        }
        Ok(EndpointBatch {
            authority: self.authority.clone(),
            observations,
            usage: EndpointUsage {
                requests: requests.len(),
                source_visits: limits.source_visits - visits,
                predicate_tests: limits.predicate_tests - predicates,
                geometry_bytes: geometry,
                identity_bytes: identity,
            },
        })
    }
    /// Borrowing the owner keeps its immutable Store cohort and live authority
    /// through serialization. Another owner cannot open this batch's contents.
    pub fn observations<'a>(&'a self, batch: &'a EndpointBatch) -> Result<EndpointView<'a>> {
        if !self.authority.live.load(Ordering::Acquire)
            || !Arc::ptr_eq(&self.authority, &batch.authority)
        {
            return Err(EndpointError::Stale);
        }
        Ok(EndpointView {
            scope_sha256: &self.scope_sha256,
            sources: &self.sources,
            cell: &self.cell,
            observations: &batch.observations,
            usage: &batch.usage,
        })
    }
    pub fn release(&mut self) {
        self.authority.live.store(false, Ordering::Release);
    }
}
impl Drop for EndpointQuery<'_> {
    fn drop(&mut self) {
        self.authority.live.store(false, Ordering::Release);
    }
}
#[derive(Serialize)]
pub struct EndpointView<'a> {
    scope_sha256: &'a str,
    sources: &'a [SourceReceipt],
    cell: &'a CellBinding,
    observations: &'a [EndpointObservation],
    usage: &'a EndpointUsage,
}
impl EndpointView<'_> {
    pub fn observations(&self) -> &[EndpointObservation] {
        self.observations
    }
    pub fn usage(&self) -> &EndpointUsage {
        self.usage
    }
}
fn unsupported(reason: &'static str) -> Classification {
    Classification::Unsupported { reason }
}
fn classify(
    vertices: [[f32; 3]; 3],
    point: [f64; 3],
    plane: PlaneContract,
    left: &mut usize,
) -> Result<Classification> {
    let PlaneContract::AxisAlignedDyadic { normal_axis } = plane;
    let axis = normal_axis;
    if vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Ok(unsupported("nonfinite source triangle"));
    }
    if vertices.iter().any(|p| p[axis] != vertices[0][axis]) {
        return Ok(unsupported(
            "source triangle is not in the explicit exact axis plane",
        ));
    }
    let source = vertices.map(|p| p.map(f64::from));
    debit(left, 1, "exact source area")?;
    let Some(coords) = project([source[0], source[1], source[2], source[0]], axis) else {
        return Ok(unsupported(
            "source dyadic coordinate domain exceeds60 bits",
        ));
    };
    let Some(area) = orientation(coords[0], coords[1], coords[2]) else {
        return Ok(unsupported("source exact area is unrepresentable"));
    };
    if area == 0 {
        return Ok(unsupported("source triangle is degenerate"));
    }
    if point[axis] != source[0][axis] {
        return Ok(Classification::Outside { off_plane: true });
    }
    let Some(coords) = project([source[0], source[1], source[2], point], axis) else {
        return Ok(unsupported(
            "endpoint dyadic coordinate domain exceeds60 bits",
        ));
    };
    let mut sides = [0i128; 3];
    for i in 0..3 {
        debit(left, 1, "exact endpoint side")?;
        let Some(side) = orientation(coords[i], coords[(i + 1) % 3], coords[3]) else {
            return Ok(unsupported("endpoint exact side is unrepresentable"));
        };
        sides[i] = side.signum();
    }
    if sides
        .iter()
        .any(|side| *side != 0 && *side != area.signum())
    {
        return Ok(Classification::Outside { off_plane: false });
    }
    if let Some(vertex) = coords[..3].iter().position(|p| *p == coords[3]) {
        return Ok(Classification::OnVertex { vertex });
    }
    if let Some(edge) = sides.iter().position(|side| *side == 0) {
        return Ok(Classification::OnEdge { edge });
    }
    Ok(Classification::Inside)
}
/// Every binary32 source coordinate converts exactly to binary64. Decompose all
/// projected values into normalized dyadics and admit common-scaled magnitudes
/// <=60 bits; differences <=61 bits and determinant terms <=123 fit i128.
fn project(points: [[f64; 3]; 4], axis: usize) -> Option<[[i128; 2]; 4]> {
    let axes = [(axis + 1) % 3, (axis + 2) % 3];
    let mut words = [[(0i128, 0i32); 2]; 4];
    for (i, p) in points.iter().enumerate() {
        for (j, axis) in axes.iter().enumerate() {
            let bits = p[*axis].to_bits();
            let exp = (bits >> 52) & 2047;
            let fraction = bits & 0xfffffffffffff;
            if exp == 2047 {
                return None;
            }
            let mut mant = if exp == 0 {
                fraction
            } else {
                fraction | 0x10000000000000
            };
            let mut exponent = if exp == 0 { -1074 } else { exp as i32 - 1075 };
            if mant != 0 {
                let shift = mant.trailing_zeros();
                mant >>= shift;
                exponent += shift as i32;
            }
            words[i][j] = (
                if bits >> 63 == 0 {
                    i128::from(mant)
                } else {
                    -i128::from(mant)
                },
                exponent,
            );
        }
    }
    let common = words
        .iter()
        .flatten()
        .filter(|(m, _)| *m != 0)
        .map(|(_, e)| *e)
        .min()
        .unwrap_or(0);
    let mut out = [[0i128; 2]; 4];
    for (dest, pair) in out.iter_mut().flatten().zip(words.into_iter().flatten()) {
        let (mant, exp) = pair;
        if mant == 0 {
            continue;
        }
        let shift = u32::try_from(exp - common).ok()?;
        if 128 - mant.unsigned_abs().leading_zeros() + shift > 60 {
            return None;
        }
        *dest = mant.checked_shl(shift)?;
    }
    Some(out)
}
fn orientation(a: [i128; 2], b: [i128; 2], c: [i128; 2]) -> Option<i128> {
    (b[0] - a[0])
        .checked_mul(c[1] - a[1])?
        .checked_sub((b[1] - a[1]).checked_mul(c[0] - a[0])?)
}
