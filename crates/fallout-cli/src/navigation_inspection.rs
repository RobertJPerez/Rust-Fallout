//! Selected-cell source routes through existing strict content reads. Costs and
//! eligibility are explicit engineering requests, never measured actor travel.
use crate::{Result, inspection_input::Order};
use fallout_data::{baseline, identity::FormKey, navigation, plugin};
use fallout_runtime::navigation::{self as route, LinkKind, RouteGraph, TriangleId};
use serde::Deserialize;
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
    let eligible = |node: &route::Node| {
        (request.allow_disabled_records || node.record_flags & plugin::INITIALLY_DISABLED == 0)
            && node.triangle_flags & request.triangle_forbidden_mask == 0
            && (request.permit_door_triangles || node.doors.is_empty())
    };
    for endpoint in [&request.start, &request.goal] {
        let node = graph
            .node(endpoint)
            .ok_or("selected-cell endpoint triangle is unavailable")?;
        if !eligible(node) {
            return Err(format!(
                "navigation endpoint {endpoint:?} rejected by explicit source eligibility policy"
            )
            .into());
        }
    }
    let result = graph.route(
        &request.start,
        &request.goal,
        route::RouteLimits::default(),
        |from, link, to| {
            if !eligible(from) || to.is_some_and(|v| !eligible(v)) {
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
        },
    )?;
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
