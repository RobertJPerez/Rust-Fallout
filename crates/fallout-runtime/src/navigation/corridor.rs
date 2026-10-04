//! Source-qualified local corridors under an explicit exact plane contract.
//! Only this owner loads/builds/searches the graph; serialized routes are observations.
use super::{
    CostDecision, GraphLimits, Link, LinkKind, Node, Route, RouteGraph, RouteLimits, TriangleId,
};
use fallout_data::{
    identity::FormKey,
    navigation::{self, SourceMesh},
    store::RecordStore,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, thiserror::Error)]
pub enum CorridorError {
    #[error(transparent)]
    Data(#[from] fallout_data::Error),
    #[error(transparent)]
    Route(#[from] super::RouteError),
    #[error("corridor budget exceeded: {0}")]
    Budget(&'static str),
    #[error("invalid corridor input: {0}")]
    Invalid(&'static str),
}
pub type Result<T> = std::result::Result<T, CorridorError>;
pub(super) fn debit(left: &mut usize, n: usize, reason: &'static str) -> Result<()> {
    *left = left.checked_sub(n).ok_or(CorridorError::Budget(reason))?;
    Ok(())
}
pub(super) fn add(left: &mut usize, n: usize) -> Result<()> {
    *left = left
        .checked_add(n)
        .ok_or(CorridorError::Budget("reservation overflow"))?;
    Ok(())
}
#[derive(Clone, Copy, Debug)]
pub struct InputLimits {
    pub source_bytes: u64,
    pub index_visits: usize,
    pub meshes: usize,
    /// Total preflight and final decode work; no renewed per-mesh allowance.
    pub records: navigation::Limits,
    pub retained_bytes: usize,
    pub graph: GraphLimits,
}
impl Default for InputLimits {
    fn default() -> Self {
        Self {
            source_bytes: 4 * 1024 * 1024 * 1024,
            index_visits: 4_000_000,
            meshes: 10_000,
            records: navigation::Limits::default(),
            retained_bytes: 64 * 1024 * 1024,
            graph: GraphLimits::default(),
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorridorLimits {
    pub triangles: usize,
    pub validation_visits: usize,
    pub geometry_bytes: usize,
    pub identity_bytes: usize,
}
impl Default for CorridorLimits {
    fn default() -> Self {
        Self {
            triangles: 10_000,
            validation_visits: 100_000,
            geometry_bytes: 2 * 1024 * 1024,
            identity_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(tag = "contract", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlaneContract {
    /// Project cyclic axes following normal_axis, with exact constant source
    /// plane, same winding and reciprocal reverse directed vertex identities.
    AxisAlignedDyadic { normal_axis: usize },
}
#[derive(Clone, Debug, Serialize)]
pub struct InputUsage {
    pub source_bytes: u64,
    pub index_visits: usize,
    pub meshes: usize,
    pub record_bytes: usize,
    pub fields: usize,
    pub elements: usize,
    pub retained_reservation_bytes: usize,
}
/// Immutable source owner; it cannot be made from caller route/vertex JSON.
pub struct CorridorQuery {
    cell: FormKey,
    meshes: Vec<SourceMesh>,
    graph: RouteGraph,
    usage: InputUsage,
}
impl CorridorQuery {
    pub fn load(store: &mut RecordStore, cell: &FormKey, limits: InputLimits) -> Result<Self> {
        let source = super::source::SourceCohort::load(store, cell, limits, true)?;
        let graph = RouteGraph::build(&source.meshes, limits.graph)?;
        Ok(Self {
            cell: source.cell,
            meshes: source.meshes,
            graph,
            usage: source.usage,
        })
    }
    pub fn node(&self, id: &TriangleId) -> Option<&Node> {
        self.graph.node(id)
    }
    pub fn usage(&self) -> &InputUsage {
        &self.usage
    }
    pub fn route(
        &self,
        start: &TriangleId,
        goal: &TriangleId,
        route_limits: RouteLimits,
        limits: CorridorLimits,
        plane: PlaneContract,
        policy: impl FnMut(&Node, &Link, Option<&Node>) -> CostDecision,
    ) -> Result<CorridorOutcome> {
        let max = CorridorLimits::default();
        for (n, c) in [
            (limits.triangles, max.triangles),
            (limits.validation_visits, max.validation_visits),
            (limits.geometry_bytes, max.geometry_bytes),
            (limits.identity_bytes, max.identity_bytes),
        ] {
            if n > c {
                return Err(CorridorError::Invalid("corridor ceiling"));
            }
        }
        let max = RouteLimits::default();
        if route_limits.expansions > max.expansions
            || route_limits.edge_tests > max.edge_tests
            || route_limits.path_nodes > max.path_nodes
            || route_limits.diagnostics > max.diagnostics
        {
            return Err(CorridorError::Invalid("route ceiling"));
        }
        let PlaneContract::AxisAlignedDyadic { normal_axis } = plane;
        if normal_axis > 2 {
            return Err(CorridorError::Invalid("plane normal axis"));
        }
        let route = self.graph.route(start, goal, route_limits, policy)?;
        let corridor = match &route {
            Route::Found { nodes, links, cost } => {
                match self.derive(nodes, links, *cost, limits, normal_axis) {
                    Ok(c) => {
                        return Ok(CorridorOutcome {
                            route,
                            corridor: Some(c),
                            refusal: None,
                        });
                    }
                    Err(DeriveError::Budget(e)) => return Err(e),
                    Err(DeriveError::Refuse(reason)) => reason,
                }
            }
            Route::Unreachable => "route is unreachable",
            Route::MissingNeighbors { .. } => "route has missing source neighbors",
            Route::Unsupported { .. } => "route has unsupported source transitions",
        };
        Ok(CorridorOutcome {
            route,
            corridor: None,
            refusal: Some(corridor),
        })
    }
    fn derive(
        &self,
        nodes: &[TriangleId],
        links: &[Link],
        cost: f64,
        limits: CorridorLimits,
        normal: usize,
    ) -> std::result::Result<VerifiedCorridor, DeriveError> {
        if nodes.is_empty() || links.len() + 1 != nodes.len() {
            return Err(DeriveError::Refuse(
                "route is not a contiguous triangle chain",
            ));
        }
        let mut count = limits.triangles;
        debit(&mut count, nodes.len(), "required triangles")?;
        let geometry = nodes.len() * 128 + links.len() * 96;
        let mut visits = limits.validation_visits;
        let mut longest = self.cell.origin_plugin.len();
        for mesh in &self.meshes {
            debit(&mut visits, 1, "corridor source name visits")?;
            longest = longest
                .max(mesh.key.origin_plugin.len())
                .max(mesh.source_plugin.len());
        }
        let identity =
            4096 + nodes.len() * (2048 + 6 * longest) + links.len() * (1024 + 4 * longest);
        let mut left = limits.geometry_bytes;
        debit(&mut left, geometry, "corridor raw geometry")?;
        let mut left = limits.identity_bytes;
        debit(&mut left, identity, "corridor identities")?;
        let mut seen = BTreeSet::new();
        let mut triangles = Vec::with_capacity(nodes.len());
        let mut plane_value = None;
        let mut winding = None;
        for id in nodes {
            debit(&mut visits, 1, "corridor triangle visits")?;
            if !seen.insert(id.clone()) {
                return Err(DeriveError::Refuse("repeated route triangle"));
            }
            let mut selected = None;
            for mesh in &self.meshes {
                debit(&mut visits, 1, "corridor source joins")?;
                if mesh.key == id.mesh {
                    selected = Some(mesh);
                    break;
                }
            }
            let mesh = selected.ok_or(DeriveError::Refuse("triangle source is unavailable"))?;
            if mesh.cell.as_ref() != Some(&self.cell) {
                return Err(DeriveError::Refuse("triangle source cell differs"));
            }
            let tri = mesh
                .mesh
                .triangles
                .get(id.triangle)
                .ok_or(DeriveError::Refuse("triangle index outside source"))?;
            if !self
                .graph
                .node(id)
                .ok_or(DeriveError::Refuse("triangle graph join differs"))?
                .doors
                .is_empty()
            {
                return Err(DeriveError::Refuse(
                    "door triangle has no geometric traversal contract",
                ));
            }
            let mut vertices = [[0f32; 3]; 3];
            for (out, v) in vertices.iter_mut().zip(tri.vertices) {
                debit(&mut visits, 1, "corridor vertex visits")?;
                *out = *mesh
                    .mesh
                    .vertices
                    .get(usize::from(v))
                    .ok_or(DeriveError::Refuse("vertex outside source"))?;
            }
            if tri.vertices[0] == tri.vertices[1]
                || tri.vertices[1] == tri.vertices[2]
                || tri.vertices[2] == tri.vertices[0]
            {
                return Err(DeriveError::Refuse("repeated source vertex"));
            }
            for vertex in vertices {
                if vertex.iter().any(|n| !n.is_finite()) {
                    return Err(DeriveError::Refuse("nonfinite source vertex"));
                }
                if plane_value.is_some_and(|value| value != vertex[normal]) {
                    return Err(DeriveError::Refuse(
                        "source vertices are not in one exact axis plane",
                    ));
                }
                if plane_value.is_none() {
                    plane_value = Some(vertex[normal]);
                }
            }
            debit(&mut visits, 1, "corridor exact orientation")?;
            let sign = orientation(vertices, normal)?.signum();
            if sign == 0 {
                return Err(DeriveError::Refuse("degenerate source triangle"));
            }
            if winding.is_some_and(|value| value != sign) {
                return Err(DeriveError::Refuse("source corridor winding differs"));
            }
            winding = Some(sign);
            let mut cover_listed = false;
            for cover in &mesh.mesh.cover_triangles {
                debit(&mut visits, 1, "corridor cover visits")?;
                cover_listed |= usize::from(*cover) == id.triangle;
            }
            triangles.push(CorridorTriangle {
                id: id.clone(),
                source_plugin: mesh.source_plugin.clone(),
                source_sha256: mesh.source_sha256.clone(),
                decoded_sha256: mesh.mesh.decoded_sha256.clone(),
                record_offset: mesh.mesh.header.offset,
                record_flags: mesh.mesh.header.flags,
                vertex_indices: tri.vertices,
                vertices,
                vertex_words: vertices.map(|p| p.map(f32::to_bits)),
                raw_edges: tri.edges,
                triangle_flags: tri.flags,
                cover_flags: tri.cover_flags,
                cover_listed,
            });
        }
        let mut portals = Vec::with_capacity(links.len());
        for (index, link) in links.iter().enumerate() {
            debit(&mut visits, 1, "corridor link visits")?;
            let a = &triangles[index];
            let b = &triangles[index + 1];
            if !matches!(link.kind, LinkKind::Local) {
                return Err(DeriveError::Refuse(
                    "external or special link has no local geometric contract",
                ));
            }
            if link.source != a.id
                || link.target.as_ref() != Some(&b.id)
                || a.id.mesh != b.id.mesh
                || link.source_edge > 2
            {
                return Err(DeriveError::Refuse("route link identity is not contiguous"));
            }
            let edge = link.source_edge;
            if a.triangle_flags & (1 << edge) != 0
                || a.raw_edges[edge] < 0
                || a.raw_edges[edge] as usize != b.id.triangle
            {
                return Err(DeriveError::Refuse("source local edge annotation differs"));
            }
            let ids = [a.vertex_indices[edge], a.vertex_indices[(edge + 1) % 3]];
            let vertices = [a.vertices[edge], a.vertices[(edge + 1) % 3]];
            if link.portal != vertices.map(|p| p.map(f64::from)) {
                return Err(DeriveError::Refuse("graph portal differs from source edge"));
            }
            let mut reverse = None;
            for i in 0..3 {
                debit(&mut visits, 1, "corridor reciprocal edge visits")?;
                if b.vertex_indices[i] == ids[1] && b.vertex_indices[(i + 1) % 3] == ids[0] {
                    if reverse.is_some() {
                        return Err(DeriveError::Refuse("ambiguous reciprocal source edge"));
                    }
                    reverse = Some(i);
                }
            }
            let target_edge = reverse.ok_or(DeriveError::Refuse(
                "source portal has no reverse directed shared edge",
            ))?;
            if b.triangle_flags & (1 << target_edge) != 0
                || b.raw_edges[target_edge] < 0
                || b.raw_edges[target_edge] as usize != a.id.triangle
            {
                return Err(DeriveError::Refuse(
                    "source portal is not reciprocal local adjacency",
                ));
            }
            debit(&mut visits, 2, "corridor opposite-side orientations")?;
            let side_a = orientation(
                [vertices[0], vertices[1], a.vertices[(edge + 2) % 3]],
                normal,
            )?
            .signum();
            let side_b = orientation(
                [vertices[0], vertices[1], b.vertices[(target_edge + 2) % 3]],
                normal,
            )?
            .signum();
            if side_a == 0 || side_b == 0 || side_a == side_b {
                return Err(DeriveError::Refuse(
                    "source triangles are not on opposite portal sides",
                ));
            }
            portals.push(CorridorPortal {
                source: a.id.clone(),
                target: b.id.clone(),
                source_edge: edge,
                target_edge,
                vertex_indices: ids,
                vertices,
                vertex_words: vertices.map(|p| p.map(f32::to_bits)),
            });
        }
        Ok(VerifiedCorridor {
            cell: self.cell.clone(),
            plane: PlaneContract::AxisAlignedDyadic {
                normal_axis: normal,
            },
            plane_word: plane_value.unwrap().to_bits(),
            winding: winding.unwrap() as i8,
            cost,
            triangles,
            portals,
            usage: CorridorUsage {
                triangles: nodes.len(),
                validation_visits: limits.validation_visits - visits,
                geometry_bytes: geometry,
                identity_bytes: identity,
            },
        })
    }
}
enum DeriveError {
    Budget(CorridorError),
    Refuse(&'static str),
}
impl From<CorridorError> for DeriveError {
    fn from(e: CorridorError) -> Self {
        Self::Budget(e)
    }
}
/// Exact binary32 projection. Common dyadic coordinates must fit signed
/// magnitude30 bits; differences/products then fit checked i128 without rounding.
fn orientation(points: [[f32; 3]; 3], normal: usize) -> std::result::Result<i128, DeriveError> {
    let axes = [(normal + 1) % 3, (normal + 2) % 3];
    let mut words = [(0i128, 0i32); 6];
    for (i, p) in points.iter().enumerate() {
        for (j, axis) in axes.iter().enumerate() {
            let bits = p[*axis].to_bits();
            let exp = (bits >> 23) & 255;
            let frac = bits & 0x7fffff;
            if exp == 255 {
                return Err(DeriveError::Refuse("nonfinite exact predicate"));
            }
            let mut mant = if exp == 0 { frac } else { frac | 0x800000 };
            let mut exponent = if exp == 0 { -149 } else { exp as i32 - 150 };
            if mant != 0 {
                let shift = mant.trailing_zeros();
                mant >>= shift;
                exponent += shift as i32;
            }
            words[2 * i + j] = (
                if bits >> 31 == 0 {
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
        .filter(|(m, _)| *m != 0)
        .map(|(_, e)| *e)
        .min()
        .unwrap_or(0);
    let mut coords = [0i128; 6];
    for (out, (mant, exp)) in coords.iter_mut().zip(words) {
        if mant == 0 {
            continue;
        }
        let shift = u32::try_from(exp - common)
            .map_err(|_| DeriveError::Refuse("exact dyadic exponent domain"))?;
        let bits = 128 - mant.unsigned_abs().leading_zeros();
        if bits + shift > 30 {
            return Err(DeriveError::Refuse(
                "exact dyadic coordinate domain exceeds30 bits",
            ));
        }
        *out = mant
            .checked_shl(shift)
            .ok_or(DeriveError::Refuse("exact dyadic shift overflow"))?;
    }
    (coords[2] - coords[0])
        .checked_mul(coords[5] - coords[1])
        .and_then(|a| {
            (coords[3] - coords[1])
                .checked_mul(coords[4] - coords[0])
                .and_then(|b| a.checked_sub(b))
        })
        .ok_or(DeriveError::Refuse("exact orientation overflow"))
}
#[derive(Debug, Serialize)]
pub struct CorridorTriangle {
    pub id: TriangleId,
    pub source_plugin: String,
    pub source_sha256: String,
    pub decoded_sha256: String,
    pub record_offset: u64,
    pub record_flags: u32,
    pub vertex_indices: [u16; 3],
    pub vertices: [[f32; 3]; 3],
    pub vertex_words: [[u32; 3]; 3],
    pub raw_edges: [i16; 3],
    pub triangle_flags: u16,
    pub cover_flags: u16,
    pub cover_listed: bool,
}
#[derive(Debug, Serialize)]
pub struct CorridorPortal {
    pub source: TriangleId,
    pub target: TriangleId,
    pub source_edge: usize,
    pub target_edge: usize,
    pub vertex_indices: [u16; 2],
    pub vertices: [[f32; 3]; 2],
    pub vertex_words: [[u32; 3]; 2],
}
#[derive(Debug, Serialize)]
pub struct CorridorUsage {
    pub triangles: usize,
    pub validation_visits: usize,
    pub geometry_bytes: usize,
    pub identity_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct VerifiedCorridor {
    cell: FormKey,
    plane: PlaneContract,
    plane_word: u32,
    winding: i8,
    cost: f64,
    triangles: Vec<CorridorTriangle>,
    portals: Vec<CorridorPortal>,
    usage: CorridorUsage,
}
impl VerifiedCorridor {
    pub fn triangles(&self) -> &[CorridorTriangle] {
        &self.triangles
    }
    pub fn portals(&self) -> &[CorridorPortal] {
        &self.portals
    }
    pub fn usage(&self) -> &CorridorUsage {
        &self.usage
    }
}
#[derive(Debug, Serialize)]
pub struct CorridorOutcome {
    pub route: Route,
    corridor: Option<VerifiedCorridor>,
    pub refusal: Option<&'static str>,
}
impl CorridorOutcome {
    pub fn corridor(&self) -> Option<&VerifiedCorridor> {
        self.corridor.as_ref()
    }
}
