use fallout_data::{
    identity::{FormKey, ProfileId},
    navigation::{DoorLink, EdgeLink, NavMesh, SourceMesh, Triangle},
    plugin,
    world::SourceField,
};
use fallout_runtime::navigation::{overlay::*, *};
use std::cell::Cell;
mod common;
fn key(local_id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "overlay.esm".into(),
        local_id,
    }
}
fn id(triangle: usize) -> TriangleId {
    TriangleId {
        mesh: key(100),
        triangle,
    }
}
// Independent literal diamond: two parallel source slots to node1, another
// branch via2, a reverse cycle, and isolated4. Door annotations remain source.
fn fixture() -> SourceMesh {
    SourceMesh {
        key: key(100),
        cell: Some(key(10)),
        source_plugin: "overlay.esm".into(),
        source_sha256: "a".repeat(64),
        external_targets: vec![],
        door_targets: vec![Some(key(300))],
        mesh: NavMesh {
            header: plugin::RecordHeader {
                kind: *b"NAVM",
                offset: 100,
                stored_size: 0,
                flags: 0,
                form_id: 100,
                revision: [0; 4],
                version: 15,
                trailing_bytes: [0; 2],
            },
            decoded_bytes: 0,
            decoded_sha256: "b".repeat(64),
            version: SourceField {
                decoded_offset: 0,
                value: 11,
            },
            cell_raw: SourceField {
                decoded_offset: 0,
                value: 10,
            },
            vertices: vec![[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]],
            triangles: [[1, 1, 2], [3, 0, -1], [-1, 3, -1], [-1; 3], [-1; 3]]
                .into_iter()
                .map(|edges| Triangle {
                    vertices: [0, 1, 2],
                    edges,
                    flags: 0,
                    cover_flags: 0,
                })
                .collect(),
            edge_links: vec![],
            cover_triangles: vec![],
            door_links: vec![DoorLink {
                door_raw: 300,
                triangle: 1,
                unused: [0; 2],
            }],
            fields: vec![],
        },
    }
}
fn selection(source: usize, source_edge: usize, target: Option<usize>) -> DirectedEdgeSelection {
    DirectedEdgeSelection {
        source: id(source),
        source_edge,
        target: target.map(id),
    }
}
fn cost(_: &Node, _: &Link, _: Option<&Node>) -> CostDecision {
    Ok(Some(1.))
}
fn query(graph: &RouteGraph, overlay: &RouteOverlay<'_>, goal: usize) -> OverlayOutcome {
    graph
        .route_with_overlay(
            &id(0),
            &id(goal),
            RouteLimits::default(),
            overlay,
            OverlayQueryLimits::default(),
            &cost,
        )
        .unwrap()
}
fn found(outcome: OverlayOutcome, expect: &[usize], slots: &[usize], price: f64) {
    let Route::Found { nodes, links, cost } = outcome.route else {
        panic!("missing route")
    };
    assert_eq!(nodes, expect.iter().map(|&n| id(n)).collect::<Vec<_>>());
    assert_eq!(
        links.iter().map(|e| e.source_edge).collect::<Vec<_>>(),
        slots
    );
    assert_eq!(cost, price);
}
#[test]
fn literal_parallel_slots_penalty_diamond_cycles_and_no_geometry_rebuild() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let raw = serde_json::to_value(graph.nodes()).unwrap();
    let empty = RouteOverlay::prepare(&graph, &OverlayRequest::default(), OverlayLimits::default())
        .unwrap();
    found(query(&graph, &empty, 3), &[0, 1, 3], &[0, 0], 2.);
    let mut request = OverlayRequest {
        blocked_edges: vec![selection(0, 0, Some(1))],
        ..Default::default()
    };
    let overlay = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    // Exact directed edge0 is excluded; parallel source slot1 stays available.
    found(query(&graph, &overlay, 3), &[0, 1, 3], &[1, 0], 2.);
    request.penalties.push(PenaltySelection {
        edge: selection(0, 1, Some(1)),
        penalty: 5.,
    });
    let overlay = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    found(query(&graph, &overlay, 3), &[0, 2, 3], &[2, 1], 2.);
    request.blocked_edges.push(selection(2, 1, Some(3)));
    let overlay = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    found(query(&graph, &overlay, 3), &[0, 1, 3], &[1, 0], 7.);
    assert_eq!(serde_json::to_value(graph.nodes()).unwrap(), raw);
    assert!(matches!(
        query(&graph, &overlay, 4).route,
        Route::Unreachable
    ));
}
#[test]
fn node_exclusion_covers_endpoints_and_every_indirect_target() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    for blocked in [0, 3] {
        let overlay = RouteOverlay::prepare(
            &graph,
            &OverlayRequest {
                blocked_nodes: vec![id(blocked)],
                ..Default::default()
            },
            OverlayLimits::default(),
        )
        .unwrap();
        let calls = Cell::new(0);
        let policy = |a: &Node, b: &Link, c: Option<&Node>| {
            calls.set(calls.get() + 1);
            cost(a, b, c)
        };
        assert!(
            graph
                .route_with_overlay(
                    &id(0),
                    &id(3),
                    RouteLimits::default(),
                    &overlay,
                    OverlayQueryLimits::default(),
                    &policy
                )
                .is_err()
        );
        assert_eq!(calls.get(), 0);
    }
    let overlay = RouteOverlay::prepare(
        &graph,
        &OverlayRequest {
            blocked_nodes: vec![id(1)],
            ..Default::default()
        },
        OverlayLimits::default(),
    )
    .unwrap();
    let calls = Cell::new(0);
    let policy = |a: &Node, b: &Link, c: Option<&Node>| {
        assert_ne!(c.unwrap().id, id(1));
        calls.set(calls.get() + 1);
        cost(a, b, c)
    };
    let out = graph
        .route_with_overlay(
            &id(0),
            &id(3),
            RouteLimits::default(),
            &overlay,
            OverlayQueryLimits::default(),
            &policy,
        )
        .unwrap();
    found(out, &[0, 2, 3], &[2, 1], 2.);
    assert_eq!(calls.get(), 2);
    let both = RouteOverlay::prepare(
        &graph,
        &OverlayRequest {
            blocked_nodes: vec![id(1), id(2)],
            ..Default::default()
        },
        OverlayLimits::default(),
    )
    .unwrap();
    assert!(matches!(query(&graph, &both, 3).route, Route::Unreachable));
}
#[test]
fn exact_authority_and_normalized_policy_identity_reject_foreign_graphs_before_search() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let mut request = OverlayRequest {
        blocked_nodes: vec![id(4)],
        blocked_edges: vec![selection(1, 1, Some(0))],
        penalties: vec![
            PenaltySelection {
                edge: selection(0, 0, Some(1)),
                penalty: 2.,
            },
            PenaltySelection {
                edge: selection(0, 1, Some(1)),
                penalty: 3.,
            },
        ],
    };
    let overlay = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    request.penalties.reverse();
    let reversed = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    let view = serde_json::to_value(overlay.view()).unwrap();
    assert_eq!(view, serde_json::to_value(reversed.view()).unwrap());
    for changed in [false, true] {
        let mut source = fixture();
        if changed {
            source.source_sha256 = "c".repeat(64);
        }
        let foreign = RouteGraph::build(&[source], GraphLimits::default()).unwrap();
        let calls = Cell::new(0);
        let policy = |a: &Node, b: &Link, c: Option<&Node>| {
            calls.set(calls.get() + 1);
            cost(a, b, c)
        };
        assert!(
            foreign
                .route_with_overlay(
                    &id(0),
                    &id(3),
                    RouteLimits::default(),
                    &overlay,
                    OverlayQueryLimits::default(),
                    &policy
                )
                .is_err()
        );
        assert_eq!(calls.get(), 0);
        let new = RouteOverlay::prepare(&foreign, &request, OverlayLimits::default()).unwrap();
        let new = serde_json::to_value(new.view()).unwrap();
        assert_eq!(
            view["source_scope_sha256"] == new["source_scope_sha256"],
            !changed
        );
    }
    request.penalties[0].penalty = 0.;
    let zero = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    request.penalties[0].penalty = -0.;
    let negative_zero = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    assert_ne!(
        serde_json::to_value(zero.view()).unwrap()["policy_sha256"],
        serde_json::to_value(negative_zero.view()).unwrap()["policy_sha256"]
    );
}
#[test]
fn admission_rejects_duplicate_conflicting_unknown_exact_target_and_invalid_rows() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let edge = selection(0, 0, Some(1));
    let p = PenaltySelection {
        edge: edge.clone(),
        penalty: 1.,
    };
    let bad = [
        OverlayRequest {
            blocked_nodes: vec![id(1), id(1)],
            ..Default::default()
        },
        OverlayRequest {
            blocked_nodes: vec![id(9)],
            ..Default::default()
        },
        OverlayRequest {
            blocked_edges: vec![edge.clone(), edge.clone()],
            ..Default::default()
        },
        OverlayRequest {
            penalties: vec![p.clone(), p.clone()],
            ..Default::default()
        },
        OverlayRequest {
            blocked_edges: vec![edge],
            penalties: vec![p],
            ..Default::default()
        },
        OverlayRequest {
            blocked_edges: vec![selection(0, 3, Some(1))],
            ..Default::default()
        },
        OverlayRequest {
            blocked_edges: vec![selection(0, 0, None)],
            ..Default::default()
        },
        OverlayRequest {
            blocked_edges: vec![selection(0, 0, Some(2))],
            ..Default::default()
        },
        OverlayRequest {
            blocked_edges: vec![selection(9, 0, Some(1))],
            ..Default::default()
        },
    ];
    for request in bad {
        assert!(RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).is_err());
    }
    for penalty in [-1., f64::NAN, f64::INFINITY] {
        let request = OverlayRequest {
            penalties: vec![PenaltySelection {
                edge: selection(0, 0, Some(1)),
                penalty,
            }],
            ..Default::default()
        };
        assert!(RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).is_err());
    }
    let missing = serde_json::json!({"source":id(0),"source_edge":0});
    assert!(serde_json::from_value::<DirectedEdgeSelection>(missing).is_err());
    let explicit = serde_json::json!({"source":id(0),"source_edge":0,"target":null});
    assert!(serde_json::from_value::<DirectedEdgeSelection>(explicit).is_ok());
}
#[test]
fn special_missing_and_door_eligibility_stays_with_the_base_policy() {
    let mut source = fixture();
    source.mesh.triangles[3].edges[2] = 0;
    source.mesh.triangles[3].flags = 4;
    source.mesh.edge_links.push(EdgeLink {
        link_type: 7,
        navmesh_raw: 200,
        triangle: 9,
    });
    source.external_targets.push(Some(key(200)));
    let graph = RouteGraph::build(&[source], GraphLimits::default()).unwrap();
    let request = OverlayRequest {
        penalties: vec![
            PenaltySelection {
                edge: selection(0, 0, Some(1)),
                penalty: 0.,
            },
            PenaltySelection {
                edge: DirectedEdgeSelection {
                    source: id(3),
                    source_edge: 2,
                    target: Some(TriangleId {
                        mesh: key(200),
                        triangle: 9,
                    }),
                },
                penalty: 0.,
            },
        ],
        ..Default::default()
    };
    let overlay = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    let policy = |a: &Node, b: &Link, c: Option<&Node>| {
        if matches!(b.kind, LinkKind::External { .. }) {
            Err("special type7 unavailable".into())
        } else if c.is_some_and(|n| !n.doors.is_empty()) {
            Ok(None)
        } else {
            cost(a, b, c)
        }
    };
    let out = graph
        .route_with_overlay(
            &id(0),
            &id(4),
            RouteLimits::default(),
            &overlay,
            OverlayQueryLimits::default(),
            &policy,
        )
        .unwrap();
    let Route::Unsupported {
        links,
        missing_neighbors,
    } = out.route
    else {
        panic!()
    };
    assert_eq!(links.len(), 1);
    assert!(missing_neighbors.is_empty());
    assert_eq!(links[0].reason, "special type7 unavailable");
    let policy = |a: &Node, b: &Link, c: Option<&Node>| cost(a, b, c);
    let out = graph
        .route_with_overlay(
            &id(0),
            &id(4),
            RouteLimits::default(),
            &overlay,
            OverlayQueryLimits::default(),
            &policy,
        )
        .unwrap();
    assert!(matches!(out.route, Route::MissingNeighbors { .. }));
    // Explicit null means unresolved source identity, not an absent loaded node.
    let mut source = fixture();
    source.mesh.triangles[3].edges[2] = 0;
    source.mesh.triangles[3].flags = 4;
    source.mesh.edge_links.push(EdgeLink {
        link_type: 7,
        navmesh_raw: 200,
        triangle: 9,
    });
    source.external_targets.push(None);
    let unresolved = RouteGraph::build(&[source], GraphLimits::default()).unwrap();
    let request = OverlayRequest {
        blocked_edges: vec![selection(3, 2, None)],
        ..Default::default()
    };
    assert!(RouteOverlay::prepare(&unresolved, &request, OverlayLimits::default()).is_ok());
    assert!(RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).is_err());
}
#[test]
fn global_one_under_caps_overflow_and_late_query_failure_produce_no_route() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let request = OverlayRequest {
        blocked_nodes: vec![id(4)],
        blocked_edges: vec![selection(1, 1, Some(0))],
        penalties: vec![PenaltySelection {
            edge: selection(0, 0, Some(1)),
            penalty: 2.,
        }],
    };
    let overlay = RouteOverlay::prepare(&graph, &request, OverlayLimits::default()).unwrap();
    let a = overlay.admission();
    let exact = OverlayLimits {
        rows: a.rows,
        identity_bytes: a.identity_bytes,
        identity_bytes_per_key: key(0).origin_plugin.len(),
        retained_bytes: a.retained_bytes,
        prepare_visits: a.prepare_visits,
        hash_bytes: a.hash_bytes,
    };
    assert!(RouteOverlay::prepare(&graph, &request, exact).is_ok());
    for limits in [
        OverlayLimits {
            rows: a.rows - 1,
            ..exact
        },
        OverlayLimits {
            identity_bytes: a.identity_bytes - 1,
            ..exact
        },
        OverlayLimits {
            identity_bytes_per_key: exact.identity_bytes_per_key - 1,
            ..exact
        },
        OverlayLimits {
            retained_bytes: a.retained_bytes - 1,
            ..exact
        },
        OverlayLimits {
            prepare_visits: a.prepare_visits - 1,
            ..exact
        },
        OverlayLimits {
            hash_bytes: a.hash_bytes - 1,
            ..exact
        },
        OverlayLimits {
            rows: 10_001,
            ..exact
        },
    ] {
        assert!(RouteOverlay::prepare(&graph, &request, limits).is_err());
    }
    let out = query(&graph, &overlay, 3);
    let tests = out.lookup_tests;
    assert!(tests > 0);
    let limits = OverlayQueryLimits {
        lookup_tests: tests,
    };
    assert!(
        graph
            .route_with_overlay(
                &id(0),
                &id(3),
                RouteLimits::default(),
                &overlay,
                limits,
                &cost
            )
            .is_ok()
    );
    assert!(
        graph
            .route_with_overlay(
                &id(0),
                &id(3),
                RouteLimits::default(),
                &overlay,
                OverlayQueryLimits {
                    lookup_tests: tests - 1
                },
                &cost
            )
            .is_err()
    );
    for route_limits in [
        RouteLimits {
            expansions: 0,
            ..RouteLimits::default()
        },
        RouteLimits {
            edge_tests: 0,
            ..RouteLimits::default()
        },
        RouteLimits {
            path_nodes: 2,
            ..RouteLimits::default()
        },
    ] {
        assert!(
            graph
                .route_with_overlay(
                    &id(0),
                    &id(3),
                    route_limits,
                    &overlay,
                    OverlayQueryLimits::default(),
                    &cost
                )
                .is_err()
        );
    }
    let blocked = RouteOverlay::prepare(
        &graph,
        &OverlayRequest {
            blocked_edges: vec![selection(0, 0, Some(1))],
            ..Default::default()
        },
        OverlayLimits::default(),
    )
    .unwrap();
    for base in [-1., f64::NAN, f64::INFINITY] {
        let policy = |_: &Node, _: &Link, _: Option<&Node>| Ok(Some(base));
        assert!(
            graph
                .route_with_overlay(
                    &id(0),
                    &id(3),
                    RouteLimits::default(),
                    &blocked,
                    OverlayQueryLimits::default(),
                    &policy
                )
                .is_err()
        );
    }
    let huge = RouteOverlay::prepare(
        &graph,
        &OverlayRequest {
            penalties: vec![PenaltySelection {
                edge: selection(0, 0, Some(1)),
                penalty: f64::MAX,
            }],
            ..Default::default()
        },
        OverlayLimits::default(),
    )
    .unwrap();
    let policy = |_: &Node, _: &Link, _: Option<&Node>| Ok(Some(f64::MAX));
    assert!(
        graph
            .route_with_overlay(
                &id(0),
                &id(3),
                RouteLimits::default(),
                &huge,
                OverlayQueryLimits::default(),
                &policy
            )
            .is_err()
    );
    // Earlier failure does not poison the reusable overlay or its global budget.
    found(query(&graph, &overlay, 3), &[0, 1, 3], &[1, 0], 2.);
}

// Raw authored plugin fixture passes through the existing RecordStore/NAVM
// decoder in frozen CLI proofs. This encodes literal rows, not a second parser.
fn authored_nav(mode: usize) -> Vec<u8> {
    let mut body = common::field(b"NVER", &11u32.to_le_bytes());
    let cell = if mode == 3 { 20u32 } else { 10 };
    let mut data = Vec::new();
    for n in [cell, 3, 5, u32::from(mode == 1 || mode == 6), 1, 1] {
        data.extend(n.to_le_bytes());
    }
    body.extend(common::field(b"DATA", &data));
    let mut vertices = Vec::new();
    for p in [[0f32, 0., 0.], [2., 0., 0.], [0., 2., 0.]] {
        for v in p {
            vertices.extend(v.to_le_bytes());
        }
    }
    body.extend(common::field(b"NVVX", &vertices));
    let mut triangles = Vec::new();
    for (i, edges) in [[1i16, 1, 2], [3, 0, -1], [-1, 3, -1], [-1; 3], [-1; 3]]
        .into_iter()
        .enumerate()
    {
        for v in [0u16, 1, 2] {
            triangles.extend(v.to_le_bytes());
        }
        for (slot, e) in edges.into_iter().enumerate() {
            // An external slot stores the literal NVEX row index.
            let value = if i == 2 && slot == 1 && (mode == 1 || mode == 6) {
                0
            } else {
                e
            };
            triangles.extend(value.to_le_bytes());
        }
        triangles.extend(
            (if i == 2 && (mode == 1 || mode == 6) {
                2u16
            } else {
                0
            })
            .to_le_bytes(),
        );
        triangles.extend((if mode == 2 || mode == 5 { 0x1234u16 } else { 0 }).to_le_bytes());
    }
    body.extend(common::field(b"NVTR", &triangles));
    if mode == 1 || mode == 6 {
        body.extend(common::field(
            b"NVEX",
            &[
                7u32.to_le_bytes().as_slice(),
                &(if mode == 6 { 200u32 } else { 100 }).to_le_bytes(),
                &3u16.to_le_bytes(),
            ]
            .concat(),
        ));
    }
    body.extend(common::field(b"NVCA", &1u16.to_le_bytes()));
    body.extend(common::field(
        b"NVDP",
        &[
            300u32.to_le_bytes().as_slice(),
            &1u16.to_le_bytes(),
            &[0x81, 0x82],
        ]
        .concat(),
    ));
    body
}
fn group(cell: u32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &cell.to_le_bytes(),
        &6i32.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
#[test]
#[ignore]
fn overlay_cli_fixture_export() {
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::var_os("FALLOUT_OVERLAY_FIXTURE").expect("fixture output"));
    fs::create_dir(&root).unwrap();
    for mode in 0..=6 {
        let install = root.join(format!("install{mode}"));
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        let cell = [
            common::field(b"EDID", b"OverlayCell\0"),
            common::field(b"DATA", &[1]),
        ]
        .concat();
        let base = [
            common::header(&[]),
            common::record(
                b"CELL",
                10,
                if mode == 4 { plugin::DELETED } else { 0 },
                &cell,
            ),
            group(10, &common::record(b"NAVM", 100, 0, &authored_nav(mode))),
            common::record(b"REFR", 300, 0, &[]),
        ]
        .concat();
        fs::write(install.join("Data/FalloutNV.esm"), base).unwrap();
        let mut order = vec!["FalloutNV.esm"];
        if mode == 5 {
            fs::write(
                install.join("Data/Patch.esp"),
                [
                    common::header(&["FalloutNV.esm"]),
                    group(10, &common::record(b"NAVM", 100, 0, &authored_nav(2))),
                ]
                .concat(),
            )
            .unwrap();
            order.push("Patch.esp");
        }
        fs::write(
            root.join(format!("order{mode}.json")),
            serde_json::to_vec_pretty(&order).unwrap(),
        )
        .unwrap();
    }
}
