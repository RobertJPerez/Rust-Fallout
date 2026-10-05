//! Exact PACK source operands joined to explicit engineering navigation inputs.
use crate::{World, foreign::Content, navigation, reference_state};
use fallout_data::{
    actors::packages::{self, destinations},
    identity::{self, FormKey, ProfileId},
    navigation::{self as source_navigation, SourceMesh},
    store::{RecordStore, SourceReceipt},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub cell: FormKey,
    pub node: navigation::TriangleId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    Admit { cost_bits: u64 },
    Reject {},
    Unavailable { reason: String },
}
impl Decision {
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Admit { cost_bits } => {
                let cost = f64::from_bits(*cost_bits);
                if !cost.is_finite() || cost < 0. {
                    return Err(Error::Invalid("cost must be finite and nonnegative"));
                }
            }
            Self::Unavailable { reason } if reason.len() > 1024 || reason.is_empty() => {
                return Err(Error::Invalid("unavailable reason needs 1..1024 bytes"));
            }
            _ => {}
        }
        Ok(())
    }
    fn choose(&self) -> navigation::CostDecision {
        match self {
            Self::Admit { cost_bits } => Ok(Some(f64::from_bits(*cost_bits))),
            Self::Reject {} => Ok(None),
            Self::Unavailable { reason } => Err(reason.clone()),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DoorPolicy {
    Pass {},
    Reject {},
    Unavailable { reason: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalDecision {
    pub link_type: u32,
    pub decision: Decision,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub local: Decision,
    pub external_fallback: Decision,
    pub external: Vec<ExternalDecision>,
    pub doors: DoorPolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub package: FormKey,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub destination: FormKey,
    pub cells: Vec<FormKey>,
    pub start: Endpoint,
    pub goal: Endpoint,
    pub policy: Policy,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub destination: destinations::Limits,
    pub navigation: source_navigation::Limits,
    pub graph: navigation::GraphLimits,
    pub route: navigation::RouteLimits,
    pub max_sources: usize,
    pub max_cells: usize,
    pub max_meshes: usize,
    pub max_policy_entries: usize,
    pub max_request_bytes: usize,
    pub max_view_bytes: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            destination: destinations::Limits::default(),
            navigation: source_navigation::Limits::default(),
            graph: navigation::GraphLimits::default(),
            route: navigation::RouteLimits::default(),
            max_sources: 256,
            max_cells: 16,
            max_meshes: 512,
            max_policy_entries: 256,
            max_request_bytes: 1024 * 1024,
            max_view_bytes: 64 * 1024,
            max_visits: 20_000_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("package route source cohort differs from canonical caller")]
    ContextChanged,
    #[error("package route physical source selection differs")]
    SourceChanged,
    #[error("invalid package route query: {0}")]
    Invalid(&'static str),
    #[error("package route {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Source(#[from] fallout_data::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Route(#[from] navigation::RouteError),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    DestinationUnavailable,
    EngineeringProposal,
}
#[derive(Debug, Clone, Copy, Serialize, thiserror::Error)]
#[error(
    "faithful AI eligibility, destination-to-node selection, traversal and movement are unverified"
)]
pub struct ExecutionRefusal {
    code: &'static str,
    dependencies: &'static [&'static str],
}

/// Internally constructed, read-only output; serialized reports have no ingress.
#[derive(Serialize)]
pub struct Observation<'a> {
    query: &'a Query,
    destination: destinations::Manifest<'a>,
    operand_index: usize,
    caller: Option<reference_state::View>,
    meshes: Vec<SourceMesh>,
    start: Option<navigation::Node>,
    goal: Option<navigation::Node>,
    route: Option<navigation::Route>,
    route_nodes: Vec<navigation::Node>,
    outcome: Outcome,
    visits: usize,
    destination_node_mapping_verified: bool,
    execution_supported: bool,
    state_changed: bool,
    refusal: ExecutionRefusal,
    scope: &'static str,
}
impl Observation<'_> {
    pub fn destination(&self) -> &destinations::Manifest<'_> {
        &self.destination
    }
    pub fn route(&self) -> Option<&navigation::Route> {
        self.route.as_ref()
    }
    pub fn meshes(&self) -> &[SourceMesh] {
        &self.meshes
    }
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    pub fn require_execution(&self) -> Result<(), ExecutionRefusal> {
        Err(self.refusal)
    }
}

fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("package route projection budget"));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn projection<T: Serialize>(value: &T, maximum: usize, label: &'static str) -> Result<(), Error> {
    serde_json::to_writer(ProjectionBudget { bytes: 0, maximum }, value)
        .map_err(|_| Error::Capacity(label))
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn key(value: &FormKey) -> Result<(), Error> {
    if value.profile != ProfileId::NvOriginal
        || value.local_id > 0x00ff_ffff
        || value.origin_plugin.len() > 255
        || !identity::plugin_name(&value.origin_plugin)
            .is_ok_and(|name| name == value.origin_plugin)
    {
        return Err(Error::Invalid("canonical source FormKey required"));
    }
    Ok(())
}
fn validate(query: &Query, limits: Limits) -> Result<(), Error> {
    admit(query.cells.len(), limits.max_cells, "cell")?;
    admit(
        query.policy.external.len(),
        limits.max_policy_entries,
        "policy entry",
    )?;
    projection(query, limits.max_request_bytes, "request byte")?;
    for form in [
        &query.package,
        &query.destination,
        &query.start.cell,
        &query.start.node.mesh,
        &query.goal.cell,
        &query.goal.node.mesh,
    ]
    .into_iter()
    .chain(&query.cells)
    {
        key(form)?;
    }
    if query.source_sha256.len() != 64
        || !query
            .source_sha256
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
        || query.cells.is_empty()
        || query.cells.iter().collect::<BTreeSet<_>>().len() != query.cells.len()
    {
        return Err(Error::Invalid(
            "exact source hash and unique explicit cells required",
        ));
    }
    if !query.cells.contains(&query.start.cell) || !query.cells.contains(&query.goal.cell) {
        return Err(Error::Invalid("endpoint cell is not explicitly loaded"));
    }
    query.policy.local.validate()?;
    query.policy.external_fallback.validate()?;
    let mut types = BTreeSet::new();
    for entry in &query.policy.external {
        if !types.insert(entry.link_type) {
            return Err(Error::Invalid("duplicate external link policy"));
        }
        entry.decision.validate()?;
    }
    if let DoorPolicy::Unavailable { reason } = &query.policy.doors
        && (reason.is_empty() || reason.len() > 1024)
    {
        return Err(Error::Invalid("unavailable reason needs 1..1024 bytes"));
    }
    Ok(())
}

pub fn observe<'a>(
    world: &World<'_>,
    content: &Content,
    store: &mut RecordStore,
    packages: &'a packages::Catalogue,
    query: &'a Query,
    caller: Option<&reference_state::View>,
    limits: Limits,
) -> Result<Observation<'a>, Error> {
    validate(query, limits)?;
    admit(packages.sources().len(), limits.max_sources, "source")?;
    content.validate_world(world)?;
    if !same_sources(packages.sources(), &world.catalogue().sources)
        || packages.winning_content_sha256() != world.catalogue().winning_content_sha256()
    {
        return Err(Error::ContextChanged);
    }
    if let Some(view) = caller {
        projection(view, limits.max_view_bytes, "caller view byte")?;
        let state = view
            .state()
            .ok_or(Error::Invalid("caller component is unavailable"))?;
        // Reuse canonical private-epoch/exact-state validation, without commit.
        drop(world.stage_reference_state(view, state.clone())?);
    }
    let destination = destinations::request(store, packages, &query.package, limits.destination)?;
    let source = content.source_form(world, &query.package)?;
    if source.kind != *b"PACK"
        || source.flags != destination.package.header.flags
        || destination.package.source.sha256 != query.source_sha256
        || destination.package.source.record_file_offset != query.record_file_offset
    {
        return Err(Error::SourceChanged);
    }
    let operand_index = destination
        .operands
        .iter()
        .position(|operand| operand.field_index == query.field_index)
        .ok_or(Error::SourceChanged)?;
    let operand = &destination.operands[operand_index];
    if operand.field_decoded_offset != query.field_decoded_offset {
        return Err(Error::SourceChanged);
    }
    let mut visits = destination
        .field_visits
        .checked_add(packages.sources().len())
        .and_then(|n| n.checked_add(query.policy.external.len()))
        .ok_or(Error::Capacity("visit"))?;
    admit(visits, limits.max_visits, "visit")?;
    let available = operand.binding_admitted;
    if available
        && operand.binding.as_ref().and_then(|b| b.key.as_ref()) != Some(&query.destination)
    {
        return Err(Error::SourceChanged);
    }
    if available
        && operand.alternative == destinations::Alternative::Cell
        && query.goal.cell != query.destination
    {
        return Err(Error::Invalid(
            "literal CELL operand differs from goal cell",
        ));
    }
    let mut result = Observation {
        query,
        destination,
        operand_index,
        caller: caller.cloned(),
        meshes: Vec::new(),
        start: None,
        goal: None,
        route: None,
        route_nodes: Vec::new(),
        outcome: Outcome::DestinationUnavailable,
        visits,
        destination_node_mapping_verified: false,
        execution_supported: false,
        state_changed: false,
        refusal: ExecutionRefusal {
            code: "faithful_ai_execution_unverified",
            dependencies: &[
                "package_eligibility",
                "destination_to_node_mapping",
                "door_traversal",
                "special_link_traversal",
                "scheduling",
                "canonical_movement",
            ],
        },
        scope: "Exact physical PACK operand joined to explicit caller-selected cell/node/cost/link inputs through existing source navigation and RouteGraph; reference/object destination mapping, door traversal, eligibility, scheduling and movement unverified; engineering proposal only, no canonical mutation",
    };
    if available {
        let winners = store
            .indices()
            .iter()
            .try_fold(0usize, |total, index| {
                total.checked_add(index.records.len())
            })
            .ok_or(Error::Capacity("visit"))?;
        visits = query
            .cells
            .len()
            .checked_mul(winners)
            .and_then(|n| n.checked_add(visits))
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;
        let mut remaining = limits.navigation;
        for cell in &query.cells {
            let batch = source_navigation::load_cell(
                store,
                cell,
                limits.max_meshes.saturating_sub(result.meshes.len()),
                remaining,
            )?;
            for mesh in &batch {
                let bytes = mesh.mesh.decoded_bytes;
                let fields = mesh.mesh.fields.len();
                let elements = [
                    mesh.mesh.vertices.len(),
                    mesh.mesh.triangles.len(),
                    mesh.mesh.edge_links.len(),
                    mesh.mesh.cover_triangles.len(),
                    mesh.mesh.door_links.len(),
                ]
                .into_iter()
                .try_fold(0usize, |total, count| total.checked_add(count))
                .ok_or(Error::Capacity("navigation element"))?;
                remaining.record_bytes = remaining
                    .record_bytes
                    .checked_sub(bytes)
                    .ok_or(Error::Capacity("navigation byte"))?;
                remaining.fields = remaining
                    .fields
                    .checked_sub(fields)
                    .ok_or(Error::Capacity("navigation field"))?;
                remaining.elements = remaining
                    .elements
                    .checked_sub(elements)
                    .ok_or(Error::Capacity("navigation element"))?;
                let source = content.source_form(world, &mesh.key)?;
                if source.kind != *b"NAVM" || source.flags != mesh.mesh.header.flags {
                    return Err(Error::SourceChanged);
                }
                let graph_work = mesh
                    .mesh
                    .triangles
                    .len()
                    .checked_mul(3)
                    .and_then(|n| n.checked_add(elements))
                    .and_then(|n| n.checked_add(fields))
                    .ok_or(Error::Capacity("visit"))?;
                visits = visits
                    .checked_add(graph_work)
                    .ok_or(Error::Capacity("visit"))?;
                admit(visits, limits.max_visits, "visit")?;
            }
            result.meshes.extend(batch);
        }
        let sort_work = result
            .meshes
            .len()
            .checked_mul(result.meshes.len())
            .ok_or(Error::Capacity("visit"))?;
        visits = visits
            .checked_add(sort_work)
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;
        let graph = navigation::RouteGraph::build(&result.meshes, limits.graph)?;
        let start = graph
            .node(&query.start.node)
            .ok_or(Error::Invalid("start node unavailable"))?;
        let goal = graph
            .node(&query.goal.node)
            .ok_or(Error::Invalid("goal node unavailable"))?;
        if start.cell.as_ref() != Some(&query.start.cell)
            || goal.cell.as_ref() != Some(&query.goal.cell)
        {
            return Err(Error::Invalid(
                "explicit endpoint cell differs from source node",
            ));
        }
        // Reserve the complete bounded policy/route work before traversal or
        // diagnostic copies, including policies for source special links.
        let policy_depth = usize::BITS - query.policy.external.len().leading_zeros();
        let reserved_work = limits
            .route
            .edge_tests
            .checked_mul(1 + policy_depth as usize)
            .and_then(|n| {
                graph
                    .nodes()
                    .len()
                    .checked_mul(2)
                    .and_then(|m| n.checked_add(m))
            })
            .and_then(|n| n.checked_add(limits.route.expansions))
            .and_then(|n| n.checked_add(limits.route.path_nodes))
            .ok_or(Error::Capacity("visit"))?;
        visits = visits
            .checked_add(reserved_work)
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;
        let external: BTreeMap<_, _> = query
            .policy
            .external
            .iter()
            .map(|entry| (entry.link_type, &entry.decision))
            .collect();
        let route = graph.route(
            &query.start.node,
            &query.goal.node,
            limits.route,
            |from, link, target| {
                if !from.doors.is_empty() || target.is_some_and(|to| !to.doors.is_empty()) {
                    match &query.policy.doors {
                        DoorPolicy::Pass {} => {}
                        DoorPolicy::Reject {} => return Ok(None),
                        DoorPolicy::Unavailable { reason } => return Err(reason.clone()),
                    }
                }
                match link.kind {
                    navigation::LinkKind::Local => query.policy.local.choose(),
                    navigation::LinkKind::External { link_type, .. } => external
                        .get(&link_type)
                        .copied()
                        .unwrap_or(&query.policy.external_fallback)
                        .choose(),
                }
            },
        )?;
        if let navigation::Route::Found { nodes, .. } = &route {
            for id in nodes {
                result.route_nodes.push(
                    graph
                        .node(id)
                        .ok_or(Error::Invalid("returned route node unavailable"))?
                        .clone(),
                );
            }
        }
        result.start = Some(start.clone());
        result.goal = Some(goal.clone());
        result.route = Some(route);
        result.outcome = Outcome::EngineeringProposal;
        result.visits = visits;
    }
    projection(&result, limits.max_projection_bytes, "projection byte")?;
    Ok(result)
}
