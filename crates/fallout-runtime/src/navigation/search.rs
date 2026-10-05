//! One borrowed search frontier, including bounded stale pops and reconstruction.
use super::*;
use std::mem::size_of;

#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    pub route: RouteLimits,
    pub heap_pops: usize,
    pub retained_bytes: usize,
    pub admission_visits: usize,
    /// Maximum UTF-8 bytes in one source-qualified identity string.
    pub identity_bytes: usize,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            route: RouteLimits::default(),
            heap_pops: 300_001,
            retained_bytes: 64 * 1024 * 1024,
            admission_visits: 600_003,
            identity_bytes: 4096,
        }
    }
}
impl SearchLimits {
    fn validate(self) -> RouteResult<()> {
        let m = Self::default();
        for (n, cap) in [
            (self.route.expansions, m.route.expansions),
            (self.route.edge_tests, m.route.edge_tests),
            (self.route.path_nodes, m.route.path_nodes),
            (self.route.diagnostics, m.route.diagnostics),
            (self.heap_pops, m.heap_pops),
            (self.retained_bytes, m.retained_bytes),
            (self.admission_visits, m.admission_visits),
            (self.identity_bytes, m.identity_bytes),
        ] {
            if n > cap {
                return Err(RouteError::Budget("search limits may only lower ceilings"));
            }
        }
        Ok(())
    }
    pub(super) fn legacy(route: RouteLimits) -> Self {
        Self {
            route,
            heap_pops: usize::MAX,
            retained_bytes: usize::MAX,
            admission_visits: usize::MAX,
            identity_bytes: usize::MAX,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepBudget {
    pub heap_pops: usize,
    pub expansions: usize,
    pub edge_tests: usize,
    pub path_nodes: usize,
    /// One bounded edge decision/diagnostic or path-node copy reservation.
    pub copies: usize,
    pub reverse_swaps: usize,
}
impl Default for StepBudget {
    fn default() -> Self {
        Self {
            heap_pops: 300_001,
            expansions: 100_000,
            edge_tests: 300_000,
            path_nodes: 10_000,
            copies: 310_000,
            reverse_swaps: 10_000,
        }
    }
}
impl StepBudget {
    fn validate(self) -> RouteResult<()> {
        let cap = Self::default();
        if self.heap_pops > cap.heap_pops
            || self.expansions > cap.expansions
            || self.edge_tests > cap.edge_tests
            || self.path_nodes > cap.path_nodes
            || self.copies > cap.copies
            || self.reverse_swaps > cap.reverse_swaps
        {
            return Err(RouteError::Budget("search step exceeds fixed ceiling"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct SearchWork {
    pub heap_pops: usize,
    pub stale_pops: usize,
    pub expansions: usize,
    pub edge_tests: usize,
    pub path_nodes: usize,
    pub copies: usize,
    pub reverse_swaps: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchState {
    Pending,
    Complete,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SearchProgress {
    pub state: SearchState,
    pub step: SearchWork,
    pub cumulative: SearchWork,
    pub frontier: usize,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SearchAdmission {
    pub nodes: usize,
    pub edges: usize,
    pub visits: usize,
    pub longest_identity_bytes: usize,
    pub heap_capacity: usize,
    pub path_capacity: usize,
    pub diagnostic_capacity: usize,
    pub reserved_bytes: usize,
}
enum Phase {
    Pop,
    Expand { node: usize, cost: f64, edge: usize },
    Path { at: usize, cost: f64 },
    ReverseIds { at: usize, cost: f64 },
    ReverseLinks { at: usize, cost: f64 },
    Complete(Route),
    Failed,
}
/// No caller constructor, Deserialize or persistent continuation. The graph stays
/// borrowed and immutable until this job is finished, canceled or dropped.
pub struct RouteJob<'graph, F> {
    graph: &'graph RouteGraph,
    goal: usize,
    policy: F,
    left: RouteLimits,
    pops_left: usize,
    copies_left: usize,
    reversals_left: usize,
    distances: Vec<f64>,
    previous: Vec<Option<(usize, usize)>>,
    pending: BinaryHeap<Pending>,
    missing: Vec<Link>,
    unsupported: Vec<UnsupportedLink>,
    ids: Vec<TriangleId>,
    links: Vec<Link>,
    phase: Phase,
    work: SearchWork,
    admission: SearchAdmission,
}
fn overflow() -> RouteError {
    RouteError::Budget("search storage arithmetic overflow")
}
fn add_bytes(total: &mut usize, count: usize, item: usize) -> RouteResult<()> {
    *total = total
        .checked_add(count.checked_mul(item).ok_or_else(overflow)?)
        .ok_or_else(overflow)?;
    Ok(())
}
impl<'graph, F: FnMut(&Node, &Link, Option<&Node>) -> CostDecision> RouteJob<'graph, F> {
    pub(super) fn new(
        graph: &'graph RouteGraph,
        start: &TriangleId,
        goal: &TriangleId,
        limits: SearchLimits,
        policy: F,
        strict: bool,
    ) -> RouteResult<Self> {
        if strict {
            limits.validate()?;
        }
        if start.mesh.origin_plugin.len() > limits.identity_bytes
            || goal.mesh.origin_plugin.len() > limits.identity_bytes
        {
            return Err(RouteError::Budget("search endpoint identity bytes"));
        }
        let start = *graph
            .lookup
            .get(start)
            .ok_or(RouteError::Invalid("start triangle is unavailable"))?;
        let goal = *graph
            .lookup
            .get(goal)
            .ok_or(RouteError::Invalid("goal triangle is unavailable"))?;
        let mut visits = limits.admission_visits;
        charge(&mut visits, 3, "search admission visits")?;
        let mut longest = 0;
        let mut edges = 0usize;
        for (node, neighbors) in graph.nodes.iter().zip(&graph.edges) {
            // Name preflight and the two fixed array initializations.
            charge(&mut visits, 3, "search admission visits")?;
            longest = longest.max(node.id.mesh.origin_plugin.len());
            for edge in neighbors {
                charge(&mut visits, 1, "search admission visits")?;
                edges = edges.checked_add(1).ok_or_else(overflow)?;
                longest = longest.max(edge.link.source.mesh.origin_plugin.len());
                if let Some(t) = &edge.link.target {
                    longest = longest.max(t.mesh.origin_plugin.len());
                }
            }
        }
        if longest > limits.identity_bytes {
            return Err(RouteError::Budget("search source identity bytes"));
        }
        let n = graph.nodes.len();
        let heap_capacity = edges
            .min(limits.route.edge_tests)
            .checked_add(1)
            .ok_or_else(overflow)?;
        let path_capacity = n.min(limits.route.path_nodes);
        let diagnostic_capacity = edges.min(limits.route.diagnostics);
        let output_links = path_capacity.saturating_sub(1);
        let mut bytes = size_of::<Self>().checked_add(1024).ok_or_else(overflow)?;
        add_bytes(
            &mut bytes,
            n,
            size_of::<f64>() + size_of::<Option<(usize, usize)>>(),
        )?;
        add_bytes(&mut bytes, heap_capacity, size_of::<Pending>())?;
        add_bytes(
            &mut bytes,
            path_capacity,
            size_of::<TriangleId>()
                .checked_add(longest)
                .ok_or_else(overflow)?,
        )?;
        let link_size = size_of::<Link>()
            .checked_add(longest.checked_mul(2).ok_or_else(overflow)?)
            .ok_or_else(overflow)?;
        add_bytes(&mut bytes, output_links, link_size)?;
        let diag_size = size_of::<Link>() + size_of::<UnsupportedLink>();
        let diag_size = diag_size
            .checked_add(longest.checked_mul(2).ok_or_else(overflow)?)
            .and_then(|n| n.checked_add(1024))
            .ok_or_else(overflow)?;
        add_bytes(&mut bytes, diagnostic_capacity, diag_size)?;
        if bytes > limits.retained_bytes {
            return Err(RouteError::Budget("search retained bytes"));
        }
        let admission = SearchAdmission {
            nodes: n,
            edges,
            visits: limits.admission_visits - visits,
            longest_identity_bytes: longest,
            heap_capacity,
            path_capacity,
            diagnostic_capacity,
            reserved_bytes: bytes,
        };
        let mut distances = vec![f64::INFINITY; n];
        distances[start] = 0.;
        let mut pending = BinaryHeap::with_capacity(heap_capacity);
        pending.push(Pending {
            node: start,
            cost: 0.,
        });
        Ok(Self {
            graph,
            goal,
            policy,
            left: limits.route,
            pops_left: limits.heap_pops,
            copies_left: edges.checked_add(path_capacity).ok_or_else(overflow)?,
            reversals_left: path_capacity,
            distances,
            previous: vec![None; n],
            pending,
            missing: Vec::with_capacity(diagnostic_capacity),
            unsupported: Vec::with_capacity(diagnostic_capacity),
            ids: Vec::with_capacity(path_capacity),
            links: Vec::with_capacity(output_links),
            phase: Phase::Pop,
            work: SearchWork::default(),
            admission,
        })
    }
    pub fn admission(&self) -> SearchAdmission {
        self.admission
    }
    pub fn work(&self) -> SearchWork {
        self.work
    }
    pub fn cancel(self) {}
    pub fn finish(self) -> RouteResult<Route> {
        match self.phase {
            Phase::Complete(route) => Ok(route),
            _ => Err(RouteError::Invalid("route search is not complete")),
        }
    }
    pub fn advance(&mut self, mut budget: StepBudget) -> RouteResult<SearchProgress> {
        budget.validate()?;
        if matches!(self.phase, Phase::Failed) {
            return Err(RouteError::Invalid("route search has failed"));
        }
        let mut step = SearchWork::default();
        if let Err(error) = self.advance_inner(&mut budget, &mut step) {
            self.phase = Phase::Failed;
            return Err(error);
        }
        Ok(SearchProgress {
            state: if matches!(self.phase, Phase::Complete(_)) {
                SearchState::Complete
            } else {
                SearchState::Pending
            },
            step,
            cumulative: self.work,
            frontier: self.pending.len(),
        })
    }
    fn advance_inner(&mut self, b: &mut StepBudget, step: &mut SearchWork) -> RouteResult<()> {
        loop {
            match self.phase {
                Phase::Pop => {
                    let Some(next) = self.pending.peek() else {
                        self.phase = Phase::Complete(if !self.unsupported.is_empty() {
                            Route::Unsupported {
                                links: std::mem::take(&mut self.unsupported),
                                missing_neighbors: std::mem::take(&mut self.missing),
                            }
                        } else if !self.missing.is_empty() {
                            Route::MissingNeighbors {
                                links: std::mem::take(&mut self.missing),
                            }
                        } else {
                            Route::Unreachable
                        });
                        return Ok(());
                    };
                    let stale = next.cost != self.distances[next.node];
                    if b.heap_pops == 0 || (!stale && b.expansions == 0) {
                        return Ok(());
                    }
                    charge(&mut self.pops_left, 1, "search heap pops")?;
                    b.heap_pops -= 1;
                    step.heap_pops += 1;
                    self.work.heap_pops += 1;
                    let Pending { cost, node } = self.pending.pop().expect("peeked heap");
                    if stale {
                        step.stale_pops += 1;
                        self.work.stale_pops += 1;
                        continue;
                    }
                    charge(&mut self.left.expansions, 1, "route expansions")?;
                    b.expansions -= 1;
                    step.expansions += 1;
                    self.work.expansions += 1;
                    self.phase = if node == self.goal {
                        Phase::Path { at: node, cost }
                    } else {
                        Phase::Expand {
                            node,
                            cost,
                            edge: 0,
                        }
                    };
                }
                Phase::Expand {
                    node,
                    cost,
                    edge: index,
                } => {
                    if index == self.graph.edges[node].len() {
                        self.phase = Phase::Pop;
                        continue;
                    }
                    if b.edge_tests == 0 || b.copies == 0 {
                        return Ok(());
                    }
                    charge(&mut self.left.edge_tests, 1, "route edge tests")?;
                    charge(&mut self.copies_left, 1, "search copy reservations")?;
                    b.edge_tests -= 1;
                    b.copies -= 1;
                    step.edge_tests += 1;
                    self.work.edge_tests += 1;
                    step.copies += 1;
                    self.work.copies += 1;
                    let edge = &self.graph.edges[node][index];
                    let decision = (self.policy)(
                        &self.graph.nodes[node],
                        &edge.link,
                        edge.target.map(|i| &self.graph.nodes[i]),
                    );
                    // The cursor moves once; a yield never invokes the policy again.
                    self.phase = Phase::Expand {
                        node,
                        cost,
                        edge: index + 1,
                    };
                    let edge_cost = match decision {
                        Ok(None) => continue,
                        Ok(Some(cost)) => cost,
                        Err(reason) => {
                            if reason.len() > 1024 {
                                return Err(RouteError::Budget("route diagnostic bytes"));
                            }
                            charge(&mut self.left.diagnostics, 1, "route diagnostics")?;
                            self.unsupported.push(UnsupportedLink {
                                link: edge.link.clone(),
                                reason: reason.as_str().to_owned(),
                            });
                            continue;
                        }
                    };
                    if !edge_cost.is_finite() || edge_cost < 0. {
                        return Err(RouteError::Invalid(
                            "caller cost must be finite and nonnegative",
                        ));
                    }
                    let Some(target) = edge.target else {
                        charge(&mut self.left.diagnostics, 1, "route diagnostics")?;
                        self.missing.push(edge.link.clone());
                        continue;
                    };
                    let total = cost + edge_cost;
                    if !total.is_finite() {
                        return Err(RouteError::Invalid("route cost overflow"));
                    }
                    if total < self.distances[target] {
                        if self.pending.len() == self.admission.heap_capacity {
                            return Err(RouteError::Budget("search frontier capacity"));
                        }
                        self.distances[target] = total;
                        self.previous[target] = Some((node, index));
                        self.pending.push(Pending {
                            node: target,
                            cost: total,
                        });
                    }
                }
                Phase::Path { at, cost } => {
                    if b.path_nodes == 0 || b.copies == 0 {
                        return Ok(());
                    }
                    charge(&mut self.left.path_nodes, 1, "route path nodes")?;
                    charge(&mut self.copies_left, 1, "search copy reservations")?;
                    if self.ids.len() == self.admission.path_capacity {
                        return Err(RouteError::Budget("route path nodes"));
                    }
                    if self.previous[at].is_some()
                        && self.links.len() >= self.admission.path_capacity.saturating_sub(1)
                    {
                        return Err(RouteError::Budget("route path nodes"));
                    }
                    b.path_nodes -= 1;
                    b.copies -= 1;
                    step.path_nodes += 1;
                    self.work.path_nodes += 1;
                    step.copies += 1;
                    self.work.copies += 1;
                    self.ids.push(self.graph.nodes[at].id.clone());
                    if let Some((from, edge)) = self.previous[at] {
                        self.links.push(self.graph.edges[from][edge].link.clone());
                        self.phase = Phase::Path { at: from, cost };
                    } else {
                        self.phase = Phase::ReverseIds { at: 0, cost };
                    }
                }
                Phase::ReverseIds { at, cost } => {
                    if at == self.ids.len() / 2 {
                        self.phase = Phase::ReverseLinks { at: 0, cost };
                        continue;
                    }
                    if b.reverse_swaps == 0 {
                        return Ok(());
                    }
                    charge(&mut self.reversals_left, 1, "search reverse swaps")?;
                    b.reverse_swaps -= 1;
                    step.reverse_swaps += 1;
                    self.work.reverse_swaps += 1;
                    let opposite = self.ids.len() - 1 - at;
                    self.ids.swap(at, opposite);
                    self.phase = Phase::ReverseIds { at: at + 1, cost };
                }
                Phase::ReverseLinks { at, cost } => {
                    if at == self.links.len() / 2 {
                        self.phase = Phase::Complete(Route::Found {
                            nodes: std::mem::take(&mut self.ids),
                            links: std::mem::take(&mut self.links),
                            cost,
                        });
                        return Ok(());
                    }
                    if b.reverse_swaps == 0 {
                        return Ok(());
                    }
                    charge(&mut self.reversals_left, 1, "search reverse swaps")?;
                    b.reverse_swaps -= 1;
                    step.reverse_swaps += 1;
                    self.work.reverse_swaps += 1;
                    let opposite = self.links.len() - 1 - at;
                    self.links.swap(at, opposite);
                    self.phase = Phase::ReverseLinks { at: at + 1, cost };
                }
                Phase::Complete(_) => return Ok(()),
                Phase::Failed => return Err(RouteError::Invalid("route search has failed")),
            }
        }
    }
}
