//! Bounded paths through source-authored triangles. Eligibility/cost decisions are
//! explicit caller inputs; paths are proposals, never canonical actor movement.
pub mod corridor;
pub mod endpoint;
pub mod search;
mod source;
use fallout_data::{identity::FormKey, navigation::SourceMesh};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BinaryHeap},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriangleId {
    pub mesh: FormKey,
    pub triangle: usize,
}
#[derive(Clone, Debug, Serialize)]
pub struct Door {
    pub raw: u32,
    pub reference: Option<FormKey>,
    pub unused: [u8; 2],
}
#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub id: TriangleId,
    pub cell: Option<FormKey>,
    pub source_sha256: String,
    pub record_flags: u32,
    pub triangle_flags: u16,
    pub cover_flags: u16,
    pub centroid: [f64; 3],
    pub doors: Vec<Door>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LinkKind {
    Local,
    External { link_type: u32, navmesh_raw: u32 },
}
#[derive(Clone, Debug, Serialize)]
pub struct Link {
    pub source: TriangleId,
    pub source_edge: usize,
    pub target: Option<TriangleId>,
    pub kind: LinkKind,
    /// Source triangle edge; cross-cell target coordinates are not fabricated.
    pub portal: [[f64; 3]; 2],
}
#[derive(Clone, Copy, Debug)]
pub struct GraphLimits {
    pub nodes: usize,
    pub edges: usize,
    pub door_links: usize,
}
impl Default for GraphLimits {
    fn default() -> Self {
        Self {
            nodes: 100_000,
            edges: 300_000,
            door_links: 100_000,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct RouteLimits {
    pub expansions: usize,
    pub edge_tests: usize,
    pub path_nodes: usize,
    pub diagnostics: usize,
}
impl Default for RouteLimits {
    fn default() -> Self {
        Self {
            expansions: 100_000,
            edge_tests: 300_000,
            path_nodes: 10_000,
            diagnostics: 10_000,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("navigation budget exceeded: {0}")]
    Budget(&'static str),
    #[error("invalid navigation request: {0}")]
    Invalid(&'static str),
    #[error("invalid source navigation at {triangle:?}: {reason}")]
    Source {
        triangle: TriangleId,
        reason: &'static str,
    },
}
pub type RouteResult<T> = std::result::Result<T, RouteError>;
#[derive(Clone, Debug, Serialize)]
pub struct UnsupportedLink {
    pub link: Link,
    pub reason: String,
}
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Route {
    Found {
        nodes: Vec<TriangleId>,
        links: Vec<Link>,
        cost: f64,
    },
    Unreachable,
    MissingNeighbors {
        links: Vec<Link>,
    },
    Unsupported {
        links: Vec<UnsupportedLink>,
        missing_neighbors: Vec<Link>,
    },
}
/// Explicit engineering/caller decision. None rejects a link; an error records
/// unavailable behavior instead of treating that capability as enabled.
pub type CostDecision = std::result::Result<Option<f64>, String>;
struct Edge {
    link: Link,
    target: Option<usize>,
}
pub struct RouteGraph {
    nodes: Vec<Node>,
    lookup: BTreeMap<TriangleId, usize>,
    edges: Vec<Vec<Edge>>,
}

#[derive(Clone, Copy)]
struct Pending {
    cost: f64,
    node: usize,
}
impl PartialEq for Pending {
    fn eq(&self, b: &Self) -> bool {
        self.cost.to_bits() == b.cost.to_bits() && self.node == b.node
    }
}
impl Eq for Pending {}
impl Ord for Pending {
    fn cmp(&self, b: &Self) -> Ordering {
        b.cost
            .total_cmp(&self.cost)
            .then_with(|| b.node.cmp(&self.node))
    }
}
impl PartialOrd for Pending {
    fn partial_cmp(&self, b: &Self) -> Option<Ordering> {
        Some(self.cmp(b))
    }
}
fn charge(left: &mut usize, n: usize, reason: &'static str) -> RouteResult<()> {
    *left = left.checked_sub(n).ok_or(RouteError::Budget(reason))?;
    Ok(())
}
fn source(id: &TriangleId, reason: &'static str) -> RouteError {
    RouteError::Source {
        triangle: id.clone(),
        reason,
    }
}

impl RouteGraph {
    pub fn build(meshes: &[SourceMesh], mut limits: GraphLimits) -> RouteResult<Self> {
        let mut sorted: Vec<_> = meshes.iter().collect();
        sorted.sort_by(|a, b| a.key.cmp(&b.key));
        let mut graph = Self {
            nodes: Vec::new(),
            lookup: BTreeMap::new(),
            edges: Vec::new(),
        };
        for mesh in &sorted {
            if mesh.external_targets.len() != mesh.mesh.edge_links.len()
                || mesh.door_targets.len() != mesh.mesh.door_links.len()
            {
                return Err(RouteError::Invalid("source identity table count differs"));
            }
            charge(&mut limits.nodes, mesh.mesh.triangles.len(), "graph nodes")?;
            for (index, triangle) in mesh.mesh.triangles.iter().enumerate() {
                let id = TriangleId {
                    mesh: mesh.key.clone(),
                    triangle: index,
                };
                let mut vertices = [[0.; 3]; 3];
                for (out, vertex) in vertices.iter_mut().zip(triangle.vertices) {
                    *out = mesh
                        .mesh
                        .vertices
                        .get(usize::from(vertex))
                        .ok_or_else(|| source(&id, "vertex index outside source mesh"))?
                        .map(f64::from);
                }
                let a = std::array::from_fn::<_, 3, _>(|i| vertices[1][i] - vertices[0][i]);
                let b = std::array::from_fn::<_, 3, _>(|i| vertices[2][i] - vertices[0][i]);
                let normal = [
                    a[1] * b[2] - a[2] * b[1],
                    a[2] * b[0] - a[0] * b[2],
                    a[0] * b[1] - a[1] * b[0],
                ];
                if normal.iter().any(|v| !v.is_finite()) || normal == [0.; 3] {
                    return Err(source(&id, "degenerate/nonfinite route triangle"));
                }
                let centroid = std::array::from_fn(|i| {
                    (vertices[0][i] + vertices[1][i] + vertices[2][i]) / 3.
                });
                if graph.lookup.insert(id.clone(), graph.nodes.len()).is_some() {
                    return Err(RouteError::Invalid("duplicate navmesh identity"));
                }
                graph.nodes.push(Node {
                    id,
                    cell: mesh.cell.clone(),
                    source_sha256: mesh.source_sha256.clone(),
                    record_flags: mesh.mesh.header.flags,
                    triangle_flags: triangle.flags,
                    cover_flags: triangle.cover_flags,
                    centroid,
                    doors: Vec::new(),
                });
                graph.edges.push(Vec::new());
            }
        }
        for mesh in &sorted {
            for (index, triangle) in mesh.mesh.triangles.iter().enumerate() {
                let id = TriangleId {
                    mesh: mesh.key.clone(),
                    triangle: index,
                };
                let source_index = graph.lookup[&id];
                for edge_index in 0..3 {
                    let raw = triangle.edges[edge_index];
                    if raw == -1 && triangle.flags & (1 << edge_index) == 0 {
                        continue;
                    }
                    if raw < 0 {
                        return Err(source(&id, "unknown negative/external edge index"));
                    }
                    charge(&mut limits.edges, 1, "graph edges")?;
                    let vertices = [
                        triangle.vertices[edge_index],
                        triangle.vertices[(edge_index + 1) % 3],
                    ];
                    let portal =
                        vertices.map(|v| mesh.mesh.vertices[usize::from(v)].map(f64::from));
                    let (target, kind) = if triangle.flags & (1 << edge_index) != 0 {
                        let link = mesh.mesh.edge_links.get(raw as usize).ok_or_else(|| {
                            source(&id, "external edge index outside source table")
                        })?;
                        (
                            mesh.external_targets[raw as usize]
                                .clone()
                                .map(|key| TriangleId {
                                    mesh: key,
                                    triangle: usize::from(link.triangle),
                                }),
                            LinkKind::External {
                                link_type: link.link_type,
                                navmesh_raw: link.navmesh_raw,
                            },
                        )
                    } else {
                        let neighbor =
                            mesh.mesh.triangles.get(raw as usize).ok_or_else(|| {
                                source(&id, "neighbor triangle outside source table")
                            })?;
                        if vertices.iter().any(|v| !neighbor.vertices.contains(v)) {
                            return Err(source(
                                &id,
                                "local neighbor does not share authored portal",
                            ));
                        }
                        (
                            Some(TriangleId {
                                mesh: mesh.key.clone(),
                                triangle: raw as usize,
                            }),
                            LinkKind::Local,
                        )
                    };
                    let target_index = target.as_ref().and_then(|id| graph.lookup.get(id)).copied();
                    graph.edges[source_index].push(Edge {
                        link: Link {
                            source: id.clone(),
                            source_edge: edge_index,
                            target,
                            kind,
                            portal,
                        },
                        target: target_index,
                    });
                }
            }
            for (door_index, door) in mesh.mesh.door_links.iter().enumerate() {
                charge(&mut limits.door_links, 1, "graph door links")?;
                let id = TriangleId {
                    mesh: mesh.key.clone(),
                    triangle: usize::from(door.triangle),
                };
                let index = *graph
                    .lookup
                    .get(&id)
                    .ok_or_else(|| source(&id, "door triangle outside source mesh"))?;
                graph.nodes[index].doors.push(Door {
                    raw: door.door_raw,
                    reference: mesh.door_targets[door_index].clone(),
                    unused: door.unused,
                });
            }
        }
        Ok(graph)
    }
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
    pub fn node(&self, id: &TriangleId) -> Option<&Node> {
        self.lookup.get(id).map(|i| &self.nodes[*i])
    }

    /// A* with h=0: admissible for every explicit nonnegative link cost, including
    /// zero-cost portals and special transitions between unrelated cell frames.
    /// No Euclidean cross-cell heuristic or guessed actor/door cost is injected.
    pub fn route(
        &self,
        start: &TriangleId,
        goal: &TriangleId,
        limits: RouteLimits,
        policy: impl FnMut(&Node, &Link, Option<&Node>) -> CostDecision,
    ) -> RouteResult<Route> {
        let mut job = search::RouteJob::new(
            self,
            start,
            goal,
            search::SearchLimits::legacy(limits),
            policy,
            false,
        )?;
        while job.advance(search::StepBudget::default())?.state != search::SearchState::Complete {}
        job.finish()
    }
    /// An immutable source graph and captured cost policy share one frontier.
    pub fn start_search<
        'graph,
        'policy,
        F: Fn(&Node, &Link, Option<&Node>) -> CostDecision + ?Sized,
    >(
        &'graph self,
        start: &TriangleId,
        goal: &TriangleId,
        limits: search::SearchLimits,
        policy: &'policy F,
    ) -> RouteResult<search::RouteJob<'graph, &'policy F>> {
        search::RouteJob::new(self, start, goal, limits, policy, true)
    }
}
