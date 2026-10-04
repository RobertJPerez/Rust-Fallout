//! Selected-cell source routes through existing strict content reads. Costs and
//! eligibility are explicit engineering requests, never measured actor travel.
use crate::{Result, inspection_input::Order};
use fallout_data::{baseline, identity::FormKey, navigation, plugin};
use fallout_runtime::navigation::{self as route, LinkKind, RouteGraph, TriangleId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    start: TriangleId,
    goal: TriangleId,
    local_cost: f64,
    portal_cost: Option<f64>,
    special_costs: BTreeMap<u32, f64>,
    allow_disabled_records: bool,
    triangle_forbidden_mask: u16,
    permit_door_triangles: bool,
    navi_forms: Vec<FormKey>,
}
fn validate_request(request: &Request) -> Result<()> {
    if request.navi_forms.len() > 64 || request.special_costs.len() > 256 {
        return Err("navigation request identity/policy budget exceeded".into());
    }
    if std::iter::once(&request.local_cost)
        .chain(request.portal_cost.iter())
        .chain(request.special_costs.values())
        .any(|v| !v.is_finite() || *v < 0.)
    {
        return Err("navigation engineering costs must be finite and nonnegative".into());
    }
    Ok(())
}
fn eligible(request: &Request, node: &route::Node) -> bool {
    (request.allow_disabled_records || node.record_flags & plugin::INITIALLY_DISABLED == 0)
        && node.triangle_flags & request.triangle_forbidden_mask == 0
        && (request.permit_door_triangles || node.doors.is_empty())
}
fn validate_endpoint(
    request: &Request,
    endpoint: &TriangleId,
    node: Option<&route::Node>,
) -> Result<()> {
    let node = node.ok_or("selected-cell endpoint triangle is unavailable")?;
    if !eligible(request, node) {
        return Err(format!(
            "navigation endpoint {endpoint:?} rejected by explicit source eligibility policy"
        )
        .into());
    }
    Ok(())
}
fn route_cost(
    request: &Request,
    from: &route::Node,
    link: &route::Link,
    to: Option<&route::Node>,
) -> route::CostDecision {
    if !eligible(request, from) || to.is_some_and(|v| !eligible(request, v)) {
        return Ok(None);
    }
    match link.kind {
        LinkKind::Local => Ok(Some(request.local_cost)),
        LinkKind::External { link_type: 0, .. } => request
            .portal_cost
            .map(|v| Ok(Some(v)))
            .unwrap_or_else(|| Err("portal cost/admission is unavailable".into())),
        LinkKind::External { link_type, .. } => request
            .special_costs
            .get(&link_type)
            .map(|v| Ok(Some(*v)))
            .unwrap_or_else(|| {
                Err(format!(
                    "authored special link type {link_type} has no declared cost/admission"
                ))
            }),
    }
}
fn requested_route(
    graph: &RouteGraph,
    request: &Request,
    limits: route::RouteLimits,
) -> Result<route::Route> {
    for endpoint in [&request.start, &request.goal] {
        validate_endpoint(request, endpoint, graph.node(endpoint))?;
    }
    Ok(
        graph.route(&request.start, &request.goal, limits, |from, link, to| {
            route_cost(request, from, link, to)
        })?,
    )
}
pub fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    cell_name: &str,
    request_path: Option<&Path>,
) -> Result<Value> {
    let mut request_bytes = Vec::new();
    let request = if let Some(path) = request_path {
        baseline::open_source(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut request_bytes)?;
        if request_bytes.len() > 1024 * 1024 {
            return Err("navigation request byte budget exceeded".into());
        }
        let request: Request = serde_json::from_slice(&request_bytes)?;
        validate_request(&request)?;
        Some(request)
    } else {
        None
    };
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let cell = store.cell_by_editor_id(cell_name.as_bytes())?.0;
    let meshes = navigation::load_cell(&mut store, &cell, 10_000, navigation::Limits::default())?;
    let graph = RouteGraph::build(&meshes, route::GraphLimits::default())?;
    let Some(request) = request else {
        return Ok(
            json!({"schema_version":1,"cell":cell,"load_order_sha256":order.sha256,"sources":store.source_receipts()?,"meshes":meshes,"nodes":graph.nodes(),
            "route":null,"faithful_ready":false,"semantics":"authored selected-cell navigation inspection; no route cost/admission policy or movement requested"}),
        );
    };
    let result = requested_route(&graph, &request, route::RouteLimits::default())?;
    let mut infos = Vec::new();
    let mut info_bytes = 16 * 1024 * 1024;
    let mut info_elements = 1_000_000;
    let mut info_fields = 100_000;
    for key in &request.navi_forms {
        let at = store
            .winner(key)
            .ok_or("requested NAVI winner is unavailable")?;
        let record = store.read_bounded(at, info_bytes)?;
        let map = navigation::decode_info_map(
            &record,
            store.source_name(at),
            navigation::Limits {
                record_bytes: info_bytes,
                elements: info_elements,
                fields: info_fields,
            },
        )?;
        info_bytes = info_bytes
            .checked_sub(record.payload.len())
            .ok_or("NAVI aggregate byte budget exceeded")?;
        info_fields = info_fields
            .checked_sub(map.fields.len())
            .ok_or("NAVI aggregate field budget exceeded")?;
        let elements = map.infos.len()
            + map.connections.len()
            + map
                .infos
                .iter()
                .filter_map(|i| i.island.as_ref())
                .map(|i| i.vertices.len() + i.triangles.len())
                .sum::<usize>()
            + map
                .connections
                .iter()
                .map(|c| c.standard.len() + c.preferred.len() + c.doors.len())
                .sum::<usize>();
        info_elements = info_elements
            .checked_sub(elements)
            .ok_or("NAVI aggregate element budget exceeded")?;
        infos.push(json!({"key":key,"source_sha256":store.source_digest(at)?,"map":map}));
    }
    Ok(
        json!({"schema_version":1,"cell":cell,"load_order_sha256":order.sha256,"sources":store.source_receipts()?,"request_sha256":format!("{:x}",Sha256::digest(&request_bytes)),
        "meshes":meshes,"navi":infos,"nodes":graph.nodes(),"route":result,"faithful_ready":false,
        "semantics":"authored adjacency; caller-declared costs/eligibility; A* zero heuristic; doors remain annotations; no movement, funnel, dynamic obstacles or actor package execution"}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CellRouteRequest {
    start: TriangleId,
    goal: TriangleId,
    local_cost: f64,
    portal_cost: Option<f64>,
    special_costs: BTreeMap<u32, f64>,
    allow_disabled_records: bool,
    triangle_forbidden_mask: u16,
    permit_door_triangles: bool,
}
impl From<CellRouteRequest> for Request {
    fn from(v: CellRouteRequest) -> Self {
        Self {
            start: v.start,
            goal: v.goal,
            local_cost: v.local_cost,
            portal_cost: v.portal_cost,
            special_costs: v.special_costs,
            allow_disabled_records: v.allow_disabled_records,
            triangle_forbidden_mask: v.triangle_forbidden_mask,
            permit_door_triangles: v.permit_door_triangles,
            navi_forms: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceWork {
    cells: usize,
    meshes: usize,
    index_visits: usize,
    source_bytes: u64,
    record_bytes: usize,
    fields: usize,
    elements: usize,
    identity_metadata_bytes: usize,
}
impl Default for SourceWork {
    fn default() -> Self {
        let v = navigation::CellSetLimits::default();
        Self {
            cells: v.cells,
            meshes: v.meshes,
            index_visits: v.index_visits,
            source_bytes: v.source_bytes,
            record_bytes: v.records.record_bytes,
            fields: v.records.fields,
            elements: v.records.elements,
            identity_metadata_bytes: v.identity_metadata_bytes,
        }
    }
}
impl SourceWork {
    fn limits(self) -> Result<navigation::CellSetLimits> {
        let max = Self::default();
        for (n, ceiling) in [
            (self.cells, max.cells),
            (self.meshes, max.meshes),
            (self.index_visits, max.index_visits),
            (self.record_bytes, max.record_bytes),
            (self.fields, max.fields),
            (self.elements, max.elements),
            (self.identity_metadata_bytes, max.identity_metadata_bytes),
        ] {
            if n > ceiling {
                return Err("navigation source work exceeds engineering ceiling".into());
            }
        }
        if self.source_bytes > max.source_bytes {
            return Err("navigation source bytes exceed engineering ceiling".into());
        }
        Ok(navigation::CellSetLimits {
            cells: self.cells,
            meshes: self.meshes,
            index_visits: self.index_visits,
            source_bytes: self.source_bytes,
            records: navigation::Limits {
                record_bytes: self.record_bytes,
                fields: self.fields,
                elements: self.elements,
            },
            identity_metadata_bytes: self.identity_metadata_bytes,
        })
    }
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GraphWork {
    nodes: usize,
    edges: usize,
    door_links: usize,
}
impl Default for GraphWork {
    fn default() -> Self {
        let v = route::GraphLimits::default();
        Self {
            nodes: v.nodes,
            edges: v.edges,
            door_links: v.door_links,
        }
    }
}
impl GraphWork {
    fn limits(self) -> Result<route::GraphLimits> {
        let max = Self::default();
        if self.nodes > max.nodes || self.edges > max.edges || self.door_links > max.door_links {
            return Err("navigation graph work exceeds engineering ceiling".into());
        }
        Ok(route::GraphLimits {
            nodes: self.nodes,
            edges: self.edges,
            door_links: self.door_links,
        })
    }
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RouteWork {
    expansions: usize,
    edge_tests: usize,
    path_nodes: usize,
    diagnostics: usize,
}
impl Default for RouteWork {
    fn default() -> Self {
        let v = route::RouteLimits::default();
        Self {
            expansions: v.expansions,
            edge_tests: v.edge_tests,
            path_nodes: v.path_nodes,
            diagnostics: v.diagnostics,
        }
    }
}
impl RouteWork {
    fn limits(self) -> Result<route::RouteLimits> {
        let max = Self::default();
        if self.expansions > max.expansions
            || self.edge_tests > max.edge_tests
            || self.path_nodes > max.path_nodes
            || self.diagnostics > max.diagnostics
        {
            return Err("navigation route work exceeds engineering ceiling".into());
        }
        Ok(route::RouteLimits {
            expansions: self.expansions,
            edge_tests: self.edge_tests,
            path_nodes: self.path_nodes,
            diagnostics: self.diagnostics,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CellSetRequest {
    cells: Vec<FormKey>,
    route: Option<CellRouteRequest>,
    #[serde(default)]
    source_limits: SourceWork,
    #[serde(default)]
    graph_limits: GraphWork,
    #[serde(default)]
    route_limits: RouteWork,
}
#[derive(Serialize)]
struct GraphAdmission {
    nodes: usize,
    edges: usize,
    door_links: usize,
    identity_reservation_bytes: usize,
}
fn graph_admission(
    set: &navigation::CellSet,
    source: SourceWork,
    limits: GraphWork,
) -> Result<GraphAdmission> {
    let mut nodes = 0usize;
    let mut edges = 0usize;
    let mut doors = 0usize;
    let mut longest = 0usize;
    for mesh in &set.meshes {
        nodes = nodes
            .checked_add(mesh.mesh.triangles.len())
            .ok_or("navigation node count overflow")?;
        doors = doors
            .checked_add(mesh.mesh.door_links.len())
            .ok_or("navigation door count overflow")?;
        for triangle in &mesh.mesh.triangles {
            for i in 0..3 {
                if triangle.edges[i] != -1 || triangle.flags & (1 << i) != 0 {
                    edges = edges
                        .checked_add(1)
                        .ok_or("navigation edge count overflow")?;
                }
            }
        }
        longest = longest.max(mesh.key.origin_plugin.len());
        for key in mesh
            .cell
            .iter()
            .chain(mesh.external_targets.iter().flatten())
            .chain(mesh.door_targets.iter().flatten())
        {
            longest = longest.max(key.origin_plugin.len());
        }
    }
    if nodes > limits.nodes || edges > limits.edges || doors > limits.door_links {
        return Err("navigation graph admission budget exceeded".into());
    }
    // Static nonnegative CLI policy with h=0 settles each node once. Reserve
    // graph/maps plus frontier, path and all missing/unsupported link receipts
    // from the actual source N/E/D, rather than renewing metadata per CELL.
    let reserved = longest
        .checked_mul(4)
        .and_then(|n| n.checked_add(768))
        .and_then(|size| nodes.checked_mul(size))
        .and_then(|n| {
            longest
                .checked_mul(6)
                .and_then(|v| v.checked_add(2048))
                .and_then(|size| edges.checked_mul(size))
                .and_then(|v| n.checked_add(v))
        })
        .and_then(|n| {
            longest
                .checked_add(512)
                .and_then(|size| doors.checked_mul(size))
                .and_then(|v| n.checked_add(v))
        })
        .and_then(|n| {
            edges
                .checked_add(1)
                .and_then(|v| v.checked_mul(64))
                .and_then(|v| n.checked_add(v))
        })
        .ok_or("navigation graph identity reservation overflow")?;
    let remaining = source
        .identity_metadata_bytes
        .checked_sub(set.usage.identity_metadata_bytes)
        .ok_or("navigation source identity ledger differs")?;
    if reserved > remaining {
        return Err("navigation graph identity metadata budget exceeded".into());
    }
    Ok(GraphAdmission {
        nodes,
        edges,
        door_links: doors,
        identity_reservation_bytes: reserved,
    })
}
fn serialize_nodes<S: serde::Serializer>(
    graph: &RouteGraph,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    graph.nodes().serialize(serializer)
}
#[derive(Serialize)]
pub struct CellSetReport {
    schema_version: u32,
    load_order_sha256: String,
    request_sha256: String,
    sources: Vec<fallout_data::store::SourceReceipt>,
    cell_set: navigation::CellSet,
    source_limits: SourceWork,
    graph_limits: GraphWork,
    route_limits: RouteWork,
    graph_admission: GraphAdmission,
    #[serde(rename = "nodes", serialize_with = "serialize_nodes")]
    graph: RouteGraph,
    route: Option<route::Route>,
    semantics: &'static str,
    faithful_ready: bool,
}
pub fn inspect_cells(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
) -> Result<CellSetReport> {
    let mut request_bytes = Vec::new();
    baseline::open_source(request_path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut request_bytes)?;
    if request_bytes.len() > 1024 * 1024 {
        return Err("navigation cell-set request exceeds 1 MiB".into());
    }
    let request: CellSetRequest = serde_json::from_slice(&request_bytes)?;
    let source_limits = request.source_limits.limits()?;
    let graph_limits = request.graph_limits.limits()?;
    let route_limits = request.route_limits.limits()?;
    if request.cells.is_empty() || request.cells.len() > source_limits.cells {
        return Err("navigation selected cell budget exceeded".into());
    }
    let route_request = request.route.map(Request::from);
    if let Some(r) = &route_request {
        validate_request(r)?;
    }
    let order = Order::read(order_path)?;
    let mut store = bounded_store(install, &order, cache, source_limits)?;
    let sources = store.source_receipts()?;
    let set = navigation::load_cells(&mut store, &request.cells, source_limits)?;
    let admission = graph_admission(&set, request.source_limits, request.graph_limits)?;
    let graph = RouteGraph::build(&set.meshes, graph_limits)?;
    let result = route_request
        .as_ref()
        .map(|r| requested_route(&graph, r, route_limits))
        .transpose()?;
    let report = CellSetReport {
        schema_version: 1,
        load_order_sha256: order.sha256,
        request_sha256: format!("{:x}", Sha256::digest(&request_bytes)),
        sources,
        cell_set: set,
        source_limits: request.source_limits,
        graph_limits: request.graph_limits,
        route_limits: request.route_limits,
        graph_admission: admission,
        graph,
        route: result,
        faithful_ready: false,
        semantics: "explicit exact live CELL set; source-key ordering and whole-cohort/winner/master provenance; one graph and zero-heuristic route with caller costs/eligibility; raw portals remain in separate source cell frames; no NAVI policy, movement, funnel, dynamic obstacles or actor package execution",
    };
    serde_json::to_writer_pretty(&mut crate::collision::ReportCounter(1), &report)?;
    Ok(report)
}
fn bounded_store(
    install: &Path,
    order: &Order,
    cache: Option<&Path>,
    source_limits: navigation::CellSetLimits,
) -> Result<fallout_data::store::RecordStore> {
    let mut cohort_bytes = 0u64;
    for name in &order.names {
        fallout_data::identity::plugin_name(name)?;
        cohort_bytes = cohort_bytes
            .checked_add(std::fs::metadata(install.join("Data").join(name))?.len())
            .ok_or("navigation source byte overflow")?;
        if cohort_bytes > source_limits.source_bytes {
            return Err("navigation plugin cohort exceeds source byte budget".into());
        }
    }
    let index_limits = plugin::Limits {
        max_records: 4_000_000 / order.names.len() as u64,
        max_record_bytes: source_limits.records.record_bytes,
        max_decoded_bytes: 4 * 1024 * 1024 * 1024 / order.names.len() as u64,
        ..Default::default()
    };
    Ok(if let Some(cache) = cache {
        fallout_data::store::RecordStore::open_nv_headers_cached(
            &install.join("Data"),
            &order.names,
            index_limits,
            cache,
        )?
    } else {
        fallout_data::store::RecordStore::open_nv_headers(
            &install.join("Data"),
            &order.names,
            index_limits,
        )?
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorridorRequest {
    cell: FormKey,
    route: CellRouteRequest,
    plane: route::corridor::PlaneContract,
    #[serde(default)]
    source_limits: SourceWork,
    #[serde(default)]
    graph_limits: GraphWork,
    #[serde(default)]
    route_limits: RouteWork,
    #[serde(default)]
    corridor_limits: route::corridor::CorridorLimits,
}
#[derive(Serialize)]
pub struct CorridorReport {
    schema_version: u32,
    load_order_sha256: String,
    request_sha256: String,
    sources: Vec<fallout_data::store::SourceReceipt>,
    source_usage: route::corridor::InputUsage,
    source_limits: SourceWork,
    graph_limits: GraphWork,
    route_limits: RouteWork,
    corridor_limits: route::corridor::CorridorLimits,
    result: route::corridor::CorridorOutcome,
    faithful_ready: bool,
    semantics: &'static str,
}
pub fn inspect_corridor(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
) -> Result<CorridorReport> {
    let mut bytes = Vec::new();
    baseline::open_source(request_path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("navigation corridor request exceeds1 MiB".into());
    }
    let request: CorridorRequest = serde_json::from_slice(&bytes)?;
    let source = request.source_limits.limits()?;
    let graph = request.graph_limits.limits()?;
    let route_limits = request.route_limits.limits()?;
    if source.cells == 0 {
        return Err("navigation selected cell budget exceeded".into());
    }
    let policy = Request::from(request.route);
    validate_request(&policy)?;
    let order = Order::read(order_path)?;
    let mut store = bounded_store(install, &order, cache, source)?;
    let query = route::corridor::CorridorQuery::load(
        &mut store,
        &request.cell,
        route::corridor::InputLimits {
            source_bytes: source.source_bytes,
            index_visits: source.index_visits,
            meshes: source.meshes,
            records: source.records,
            retained_bytes: source.identity_metadata_bytes,
            graph,
        },
    )?;
    for endpoint in [&policy.start, &policy.goal] {
        validate_endpoint(&policy, endpoint, query.node(endpoint))?;
    }
    let result = query.route(
        &policy.start,
        &policy.goal,
        route_limits,
        request.corridor_limits,
        request.plane,
        |from, link, to| route_cost(&policy, from, link, to),
    )?;
    let report = CorridorReport {
        schema_version: 1,
        load_order_sha256: order.sha256,
        request_sha256: format!("{:x}", Sha256::digest(&bytes)),
        sources: store.source_receipts()?,
        source_usage: query.usage().clone(),
        source_limits: request.source_limits,
        graph_limits: request.graph_limits,
        route_limits: request.route_limits,
        corridor_limits: request.corridor_limits,
        result,
        faithful_ready: false,
        semantics: "protected single-cell source owner generates route internally; exact axis-plane dyadic predicates, reciprocal reverse directed vertex identity and opposite portal sides; raw binary32 vertices and source annotations; explicit costs/eligibility; no external/door traversal, smoothing, endpoints, funnel, canonical movement or gameplay",
    };
    serde_json::to_writer_pretty(&mut crate::collision::ReportCounter(1), &report)?;
    Ok(report)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointsRequest {
    cell: FormKey,
    points: Vec<route::endpoint::EndpointRequest>,
    #[serde(default)]
    source_limits: SourceWork,
    #[serde(default)]
    endpoint_limits: route::endpoint::EndpointLimits,
}
#[derive(Serialize)]
pub struct EndpointReport<'a> {
    schema_version: u32,
    load_order_sha256: String,
    request_sha256: String,
    source_limits: SourceWork,
    endpoint_limits: route::endpoint::EndpointLimits,
    source_usage: &'a route::corridor::InputUsage,
    result: route::endpoint::EndpointView<'a>,
    faithful_ready: bool,
    semantics: &'static str,
}
pub fn inspect_endpoints(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    request_path: &Path,
    consume: impl FnOnce(&EndpointReport<'_>) -> Result<()>,
) -> Result<()> {
    let mut bytes = Vec::new();
    baseline::open_source(request_path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("navigation endpoint request exceeds1 MiB".into());
    }
    let request: EndpointsRequest = serde_json::from_slice(&bytes)?;
    let source = request.source_limits.limits()?;
    let limits = request.endpoint_limits.validate()?;
    if source.cells == 0 || request.points.is_empty() || request.points.len() > limits.requests {
        return Err("navigation endpoint cell/point count budget exceeded".into());
    }
    for point in &request.points {
        let route::corridor::PlaneContract::AxisAlignedDyadic { normal_axis } = point.plane;
        if normal_axis > 2 || point.point.iter().any(|v| !v.is_finite()) {
            return Err(
                "navigation endpoints require finite points and explicit plane axis".into(),
            );
        }
    }
    let order = Order::read(order_path)?;
    let mut store = bounded_store(install, &order, cache, source)?;
    let query = route::endpoint::EndpointQuery::load(
        &mut store,
        &request.cell,
        route::corridor::InputLimits {
            source_bytes: source.source_bytes,
            index_visits: source.index_visits,
            meshes: source.meshes,
            records: source.records,
            retained_bytes: source.identity_metadata_bytes,
            ..Default::default()
        },
    )?;
    let batch = query.inspect(&request.points, limits)?;
    let report = EndpointReport {
        schema_version: 1,
        load_order_sha256: order.sha256,
        request_sha256: format!("{:x}", Sha256::digest(&bytes)),
        source_limits: request.source_limits,
        endpoint_limits: limits,
        source_usage: query.source_usage(),
        result: query.observations(&batch)?,
        faithful_ready: false,
        semantics: "explicit source CELL/triangle/finite point under named exact axis-aligned dyadic plane; raw f32 source and f64 point words, ordered protected source cohort; contained/outside/unsupported observations from one live sealed Store-borrowing query; no nearest triangle, point adjustment, source eligibility/movement defaults, canonical pose/state/save, residency authority or gameplay",
    };
    serde_json::to_writer_pretty(&mut crate::collision::ReportCounter(1), &report)?;
    // Consumer serialization finishes before the source owner/borrow drops.
    consume(&report)
}
