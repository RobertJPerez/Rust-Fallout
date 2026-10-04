//! Exact-source engineering policy rows. Graph ownership, not a receipt, binds use.
use super::*;
use sha2::{Digest, Sha256};
use std::{cell::Cell, io::Write, mem::size_of};

fn required_target<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<TriangleId>, D::Error> {
    Option::deserialize(d)
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DirectedEdgeSelection {
    pub source: TriangleId,
    pub source_edge: usize,
    #[serde(deserialize_with = "required_target")]
    pub target: Option<TriangleId>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PenaltySelection {
    pub edge: DirectedEdgeSelection,
    pub penalty: f64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayRequest {
    pub blocked_nodes: Vec<TriangleId>,
    pub blocked_edges: Vec<DirectedEdgeSelection>,
    pub penalties: Vec<PenaltySelection>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayLimits {
    pub rows: usize,
    pub identity_bytes: usize,
    pub identity_bytes_per_key: usize,
    pub retained_bytes: usize,
    pub prepare_visits: usize,
    pub hash_bytes: usize,
}
impl Default for OverlayLimits {
    fn default() -> Self {
        Self {
            rows: 10_000,
            identity_bytes: 4 * 1024 * 1024,
            identity_bytes_per_key: 4096,
            retained_bytes: 8 * 1024 * 1024,
            prepare_visits: 10_000_000,
            hash_bytes: 64 * 1024 * 1024,
        }
    }
}
impl OverlayLimits {
    pub fn validate(self) -> RouteResult<()> {
        let m = Self::default();
        for (n, cap) in [
            (self.rows, m.rows),
            (self.identity_bytes, m.identity_bytes),
            (self.identity_bytes_per_key, m.identity_bytes_per_key),
            (self.retained_bytes, m.retained_bytes),
            (self.prepare_visits, m.prepare_visits),
            (self.hash_bytes, m.hash_bytes),
        ] {
            if n > cap {
                return Err(RouteError::Budget("overlay limits may only lower ceilings"));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayQueryLimits {
    pub lookup_tests: usize,
}
impl Default for OverlayQueryLimits {
    fn default() -> Self {
        Self {
            lookup_tests: 20_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct OverlayAdmission {
    pub rows: usize,
    pub identity_bytes: usize,
    pub retained_bytes: usize,
    pub prepare_visits: usize,
    pub hash_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct OverlayOutcome {
    pub route: Route,
    pub lookup_tests: usize,
}
/// No public constructor, Clone or Deserialize. A different graph refuses even
/// when its keys, source bytes and geometry look identical.
pub struct RouteOverlay<'graph> {
    graph: &'graph RouteGraph,
    nodes: Vec<usize>,
    edges: Vec<(usize, usize)>,
    penalties: Vec<((usize, usize), f64)>,
    source_scope_sha256: String,
    policy_sha256: String,
    admission: OverlayAdmission,
    key_limit: usize,
}
struct HashWriter<'a> {
    hash: &'a mut Sha256,
    left: &'a mut usize,
}
impl Write for HashWriter<'_> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        *self.left = self
            .left
            .checked_sub(b.len())
            .ok_or_else(|| std::io::Error::other("overlay hash byte budget"))?;
        self.hash.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn hash_value<T: Serialize>(hash: &mut Sha256, left: &mut usize, value: &T) -> RouteResult<()> {
    let mut w = HashWriter { hash, left };
    serde_json::to_writer(&mut w, value).map_err(|_| RouteError::Budget("overlay hash bytes"))?;
    w.write_all(&[0])
        .map_err(|_| RouteError::Budget("overlay hash bytes"))
}
fn arithmetic() -> RouteError {
    RouteError::Budget("overlay size arithmetic overflow")
}
fn key_bytes(id: &TriangleId, limit: usize, total: &mut usize) -> RouteResult<()> {
    let n = id.mesh.origin_plugin.len();
    if n > limit {
        return Err(RouteError::Budget("overlay identity key bytes"));
    }
    *total = total.checked_add(n).ok_or_else(arithmetic)?;
    Ok(())
}
fn lookup_charge(n: usize) -> RouteResult<usize> {
    let depth = (usize::BITS - n.checked_add(1).ok_or_else(arithmetic)?.leading_zeros()) as usize;
    // A BTreeMap node has at most eleven keys. This conservative reservation
    // bounds source key comparisons, including the root and terminal miss.
    depth
        .checked_add(2)
        .and_then(|n| n.checked_mul(11))
        .ok_or_else(arithmetic)
}
fn node_index(graph: &RouteGraph, id: &TriangleId, left: &mut usize) -> RouteResult<usize> {
    charge(
        left,
        lookup_charge(graph.nodes.len())?,
        "overlay source lookups",
    )?;
    graph
        .lookup
        .get(id)
        .copied()
        .ok_or(RouteError::Invalid("overlay triangle is unavailable"))
}
fn edge_index(
    graph: &RouteGraph,
    row: &DirectedEdgeSelection,
    left: &mut usize,
) -> RouteResult<(usize, usize)> {
    let node = node_index(graph, &row.source, left)?;
    for (i, edge) in graph.edges[node].iter().enumerate() {
        charge(left, 1, "overlay source edge visits")?;
        if edge.link.source_edge == row.source_edge {
            if edge.link.target != row.target {
                return Err(RouteError::Invalid("overlay directed edge target differs"));
            }
            return Ok((node, i));
        }
    }
    Err(RouteError::Invalid("overlay source edge is unavailable"))
}
fn sort_reservation(n: usize) -> RouteResult<usize> {
    let bits = (usize::BITS - n.max(1).leading_zeros()) as usize;
    n.checked_mul(bits + 1)
        .and_then(|n| n.checked_mul(16))
        .ok_or_else(arithmetic)
}
fn contains<T: Ord>(values: &[T], needle: &T, left: &mut usize) -> RouteResult<bool> {
    let mut lower = 0;
    let mut upper = values.len();
    while lower < upper {
        charge(left, 1, "overlay lookup tests")?;
        let middle = lower + (upper - lower) / 2;
        match values[middle].cmp(needle) {
            Ordering::Less => lower = middle + 1,
            Ordering::Greater => upper = middle,
            Ordering::Equal => return Ok(true),
        }
    }
    Ok(false)
}
impl<'graph> RouteOverlay<'graph> {
    pub fn prepare(
        graph: &'graph RouteGraph,
        request: &OverlayRequest,
        limits: OverlayLimits,
    ) -> RouteResult<Self> {
        limits.validate()?;
        let rows = request
            .blocked_nodes
            .len()
            .checked_add(request.blocked_edges.len())
            .and_then(|n| n.checked_add(request.penalties.len()))
            .ok_or_else(arithmetic)?;
        if rows > limits.rows {
            return Err(RouteError::Budget("overlay rows"));
        }
        let mut visits = limits.prepare_visits;
        let mut identities = 0usize;
        for id in &request.blocked_nodes {
            charge(&mut visits, 1, "overlay row visits")?;
            key_bytes(id, limits.identity_bytes_per_key, &mut identities)?;
        }
        for row in request
            .blocked_edges
            .iter()
            .chain(request.penalties.iter().map(|p| &p.edge))
        {
            charge(&mut visits, 1, "overlay row visits")?;
            key_bytes(&row.source, limits.identity_bytes_per_key, &mut identities)?;
            if let Some(target) = &row.target {
                key_bytes(target, limits.identity_bytes_per_key, &mut identities)?;
            }
        }
        if identities > limits.identity_bytes {
            return Err(RouteError::Budget("overlay total identity bytes"));
        }
        let reserved = size_of::<Self>()
            .checked_add(128)
            .and_then(|n| {
                n.checked_add(
                    request
                        .blocked_nodes
                        .len()
                        .checked_mul(size_of::<usize>())?,
                )
            })
            .and_then(|n| {
                n.checked_add(
                    request
                        .blocked_edges
                        .len()
                        .checked_mul(size_of::<(usize, usize)>())?,
                )
            })
            .and_then(|n| {
                n.checked_add(
                    request
                        .penalties
                        .len()
                        .checked_mul(size_of::<((usize, usize), f64)>())?,
                )
            })
            .ok_or_else(arithmetic)?;
        if reserved > limits.retained_bytes {
            return Err(RouteError::Budget("overlay retained bytes"));
        }
        // No retained source key copies: only indices into this borrowed graph.
        let mut nodes = Vec::with_capacity(request.blocked_nodes.len());
        let mut edges = Vec::with_capacity(request.blocked_edges.len());
        let mut penalties = Vec::with_capacity(request.penalties.len());
        for id in &request.blocked_nodes {
            nodes.push(node_index(graph, id, &mut visits)?);
        }
        for row in &request.blocked_edges {
            edges.push(edge_index(graph, row, &mut visits)?);
        }
        for row in &request.penalties {
            if !row.penalty.is_finite() || row.penalty < 0. {
                return Err(RouteError::Invalid(
                    "overlay penalty must be finite and nonnegative",
                ));
            }
            penalties.push((edge_index(graph, &row.edge, &mut visits)?, row.penalty));
        }
        charge(
            &mut visits,
            sort_reservation(rows)?,
            "overlay sort reservation",
        )?;
        nodes.sort_unstable();
        edges.sort_unstable();
        penalties.sort_unstable_by_key(|r| r.0);
        for pair in nodes.windows(2) {
            charge(&mut visits, 1, "overlay duplicate visits")?;
            if pair[0] == pair[1] {
                return Err(RouteError::Invalid("duplicate overlay triangle"));
            }
        }
        for pair in edges.windows(2) {
            charge(&mut visits, 1, "overlay duplicate visits")?;
            if pair[0] == pair[1] {
                return Err(RouteError::Invalid("duplicate overlay edge"));
            }
        }
        for pair in penalties.windows(2) {
            charge(&mut visits, 1, "overlay duplicate visits")?;
            if pair[0].0 == pair[1].0 {
                return Err(RouteError::Invalid("duplicate overlay penalty"));
            }
        }
        for (edge, _) in &penalties {
            if contains(&edges, edge, &mut visits)? {
                return Err(RouteError::Invalid("blocked edge conflicts with penalty"));
            }
        }
        let mut hash_left = limits.hash_bytes;
        let mut hash = Sha256::new();
        hash.update(b"fallout-route-overlay-graph-v1\0");
        for node in &graph.nodes {
            charge(&mut visits, 1, "overlay graph visits")?;
            let mut names = 0;
            key_bytes(&node.id, limits.identity_bytes_per_key, &mut names)?;
            if node
                .cell
                .as_ref()
                .is_some_and(|cell| cell.origin_plugin.len() > limits.identity_bytes_per_key)
            {
                return Err(RouteError::Budget("overlay graph key bytes"));
            }
            for door in &node.doors {
                charge(&mut visits, 1, "overlay door visits")?;
                if door
                    .reference
                    .as_ref()
                    .is_some_and(|k| k.origin_plugin.len() > limits.identity_bytes_per_key)
                {
                    return Err(RouteError::Budget("overlay graph key bytes"));
                }
            }
            hash_value(&mut hash, &mut hash_left, node)?;
        }
        for neighbors in &graph.edges {
            for edge in neighbors {
                charge(&mut visits, 1, "overlay graph edge visits")?;
                let mut names = 0;
                key_bytes(&edge.link.source, limits.identity_bytes_per_key, &mut names)?;
                if let Some(target) = &edge.link.target {
                    key_bytes(target, limits.identity_bytes_per_key, &mut names)?;
                }
                hash_value(&mut hash, &mut hash_left, &(&edge.link, edge.target))?;
            }
        }
        let source_scope_sha256 = format!("{:x}", hash.finalize());
        let mut hash = Sha256::new();
        hash.update(b"fallout-route-overlay-policy-v1\0");
        hash_value(
            &mut hash,
            &mut hash_left,
            &(&source_scope_sha256, &nodes, &edges),
        )?;
        for (edge, cost) in &penalties {
            hash_value(&mut hash, &mut hash_left, &(edge, cost.to_bits()))?;
        }
        let policy_sha256 = format!("{:x}", hash.finalize());
        Ok(Self {
            graph,
            nodes,
            edges,
            penalties,
            source_scope_sha256,
            policy_sha256,
            key_limit: limits.identity_bytes_per_key,
            admission: OverlayAdmission {
                rows,
                identity_bytes: identities,
                retained_bytes: reserved,
                prepare_visits: limits.prepare_visits - visits,
                hash_bytes: limits.hash_bytes - hash_left,
            },
        })
    }
    pub fn admission(&self) -> OverlayAdmission {
        self.admission
    }
    pub fn view(&self) -> OverlayView<'_> {
        OverlayView { owner: self }
    }
    fn penalty(&self, key: (usize, usize), left: &mut usize) -> RouteResult<f64> {
        let mut lower = 0;
        let mut upper = self.penalties.len();
        while lower < upper {
            charge(left, 1, "overlay lookup tests")?;
            let m = lower + (upper - lower) / 2;
            match self.penalties[m].0.cmp(&key) {
                Ordering::Less => lower = m + 1,
                Ordering::Greater => upper = m,
                Ordering::Equal => return Ok(self.penalties[m].1),
            }
        }
        Ok(0.)
    }
}
pub struct OverlayView<'a> {
    owner: &'a RouteOverlay<'a>,
}
impl Serialize for OverlayView<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeSeq, SerializeStruct};
        struct Nodes<'a>(&'a RouteOverlay<'a>);
        impl Serialize for Nodes<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut out = s.serialize_seq(Some(self.0.nodes.len()))?;
                for &i in &self.0.nodes {
                    out.serialize_element(&self.0.graph.nodes[i].id)?;
                }
                out.end()
            }
        }
        struct Edges<'a>(&'a RouteOverlay<'a>);
        impl Serialize for Edges<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut out = s.serialize_seq(Some(self.0.edges.len()))?;
                for &(n, e) in &self.0.edges {
                    out.serialize_element(&self.0.graph.edges[n][e].link)?;
                }
                out.end()
            }
        }
        struct Penalties<'a>(&'a RouteOverlay<'a>);
        impl Serialize for Penalties<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                let mut out = s.serialize_seq(Some(self.0.penalties.len()))?;
                for &((n, e), cost) in &self.0.penalties {
                    out.serialize_element(&(&self.0.graph.edges[n][e].link, cost, cost.to_bits()))?;
                }
                out.end()
            }
        }
        let mut out = s.serialize_struct("SourceRouteOverlay", 6)?;
        out.serialize_field("source_scope_sha256", &self.owner.source_scope_sha256)?;
        out.serialize_field("policy_sha256", &self.owner.policy_sha256)?;
        out.serialize_field("admission", &self.owner.admission)?;
        out.serialize_field("blocked_nodes", &Nodes(self.owner))?;
        out.serialize_field("blocked_edges", &Edges(self.owner))?;
        out.serialize_field("penalties", &Penalties(self.owner))?;
        out.end()
    }
}
impl RouteGraph {
    pub fn route_with_overlay<F: Fn(&Node, &Link, Option<&Node>) -> CostDecision + ?Sized>(
        &self,
        start: &TriangleId,
        goal: &TriangleId,
        limits: RouteLimits,
        overlay: &RouteOverlay<'_>,
        query: OverlayQueryLimits,
        policy: &F,
    ) -> RouteResult<OverlayOutcome> {
        if !std::ptr::eq(self, overlay.graph) {
            return Err(RouteError::Invalid(
                "overlay belongs to a different source graph",
            ));
        }
        let max = RouteLimits::default();
        if query.lookup_tests > OverlayQueryLimits::default().lookup_tests
            || limits.expansions > max.expansions
            || limits.edge_tests > max.edge_tests
            || limits.path_nodes > max.path_nodes
            || limits.diagnostics > max.diagnostics
        {
            return Err(RouteError::Budget("overlay route ceiling"));
        }
        if start.mesh.origin_plugin.len() > overlay.key_limit
            || goal.mesh.origin_plugin.len() > overlay.key_limit
        {
            return Err(RouteError::Budget("overlay endpoint key bytes"));
        }
        let left = Cell::new(query.lookup_tests);
        let failed = Cell::new(false);
        let mut endpoint_left = left.get();
        for id in [start, goal] {
            let index = node_index(self, id, &mut endpoint_left)?;
            if contains(&overlay.nodes, &index, &mut endpoint_left)? {
                return Err(RouteError::Invalid("overlay excludes route endpoint"));
            }
        }
        left.set(endpoint_left);
        let route = self.route(start, goal, limits, |from, link, to| {
            let mut local = left.get();
            let decision = (|| -> RouteResult<CostDecision> {
                let source = node_index(self, &from.id, &mut local)?;
                if contains(&overlay.nodes, &source, &mut local)? {
                    return Ok(Ok(None));
                }
                if let Some(to) = to {
                    let target = node_index(self, &to.id, &mut local)?;
                    if contains(&overlay.nodes, &target, &mut local)? {
                        return Ok(Ok(None));
                    }
                }
                let decision = policy(from, link, to);
                // Base rejection/unavailable capability remains authoritative.
                let Ok(Some(base)) = decision else {
                    return Ok(decision);
                };
                // Preserve the route engine's atomic invalid-cost refusal even
                // when an explicit edge row would otherwise hide this cost.
                if !base.is_finite() || base < 0. {
                    return Ok(Ok(Some(base)));
                }
                let mut found = None;
                for (i, edge) in self.edges[source].iter().enumerate() {
                    charge(&mut local, 1, "overlay source edge lookup")?;
                    if std::ptr::eq(&edge.link, link) {
                        found = Some(i);
                        break;
                    }
                }
                let edge = found.ok_or(RouteError::Invalid("overlay source link is foreign"))?;
                let key = (source, edge);
                if contains(&overlay.edges, &key, &mut local)? {
                    return Ok(Ok(None));
                }
                Ok(Ok(Some(base + overlay.penalty(key, &mut local)?)))
            })();
            left.set(local);
            match decision {
                Ok(value) => value,
                Err(_) => {
                    failed.set(true);
                    Ok(Some(f64::INFINITY))
                }
            }
        });
        if failed.get() {
            return Err(RouteError::Budget("overlay lookup tests"));
        }
        Ok(OverlayOutcome {
            route: route?,
            lookup_tests: query.lookup_tests - left.get(),
        })
    }
}
