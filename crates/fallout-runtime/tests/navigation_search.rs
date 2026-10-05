use fallout_data::{
    identity::{FormKey, ProfileId},
    navigation::{EdgeLink, NavMesh, SourceMesh, Triangle},
    plugin,
    world::SourceField,
};
use fallout_runtime::navigation::{search::*, *};
use std::{cell::Cell, rc::Rc};
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "search.esm".into(),
        local_id: id,
    }
}
fn id(triangle: usize) -> TriangleId {
    TriangleId {
        mesh: key(100),
        triangle,
    }
}
fn fixture() -> SourceMesh {
    let edges = [
        [1, 2, -1],
        [3, 4, 0],
        [3, 4, 0],
        [4, 1, -1],
        [5, 2, -1],
        [-1, 4, -1],
        [-1; 3],
        [-1; 3],
    ];
    SourceMesh {
        key: key(100),
        cell: Some(key(10)),
        source_plugin: "search.esm".into(),
        source_sha256: "a".repeat(64),
        external_targets: vec![],
        door_targets: vec![],
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
            triangles: edges
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
            door_links: vec![],
            fields: vec![],
        },
    }
}
fn costs(from: &Node, _: &Link, to: Option<&Node>) -> CostDecision {
    let to = to.unwrap().id.triangle;
    Ok(Some(match (from.id.triangle, to) {
        (0, 1) => 10.,
        (0, 2) => 1.,
        (1, 4) | (2, 4) => 20.,
        (_, 0) | (3, 1) | (4, 2) | (5, 4) => 0.,
        _ => 1.,
    }))
}
fn small() -> StepBudget {
    StepBudget {
        heap_pops: 1,
        expansions: 1,
        edge_tests: 1,
        path_nodes: 1,
        copies: 1,
        reverse_swaps: 1,
    }
}
fn assert_step(step: SearchWork, b: StepBudget) {
    assert!(
        step.heap_pops <= b.heap_pops
            && step.expansions <= b.expansions
            && step.edge_tests <= b.edge_tests
            && step.path_nodes <= b.path_nodes
            && step.copies <= b.copies
            && step.reverse_swaps <= b.reverse_swaps
    );
}
fn run<F: Fn(&Node, &Link, Option<&Node>) -> CostDecision>(
    graph: &RouteGraph,
    goal: usize,
    b: StepBudget,
    policy: F,
) -> (Route, SearchWork, usize) {
    let mut job = graph
        .start_search(&id(0), &id(goal), SearchLimits::default(), &policy)
        .unwrap();
    let mut advances = 0;
    loop {
        let p = job.advance(b).unwrap();
        advances += 1;
        assert_step(p.step, b);
        assert!(advances < 100);
        if p.state == SearchState::Complete {
            let work = job.work();
            return (job.finish().unwrap(), work, advances);
        }
    }
}
#[test]
fn literal_cycles_alternatives_zero_costs_and_source_order_match_every_slice() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let (r, w, n) = run(&graph, 5, small(), costs);
    assert!(n > 10);
    assert_eq!(w.path_nodes, 5);
    let Route::Found { nodes, links, cost } = &r else {
        panic!("no path")
    };
    assert_eq!(nodes, &vec![id(0), id(2), id(3), id(4), id(5)]);
    assert_eq!(*cost, 4.);
    assert_eq!(
        links.iter().map(|l| l.source_edge).collect::<Vec<_>>(),
        vec![1, 0, 0, 0]
    );
    assert_eq!(
        serde_json::to_value(&r).unwrap(),
        serde_json::to_value(
            graph
                .route(&id(0), &id(5), RouteLimits::default(), costs)
                .unwrap()
        )
        .unwrap()
    );
    for b in [
        StepBudget::default(),
        StepBudget {
            heap_pops: 3,
            expansions: 2,
            edge_tests: 2,
            path_nodes: 2,
            copies: 2,
            reverse_swaps: 1,
        },
    ] {
        let (other, work, _) = run(&graph, 5, b, costs);
        assert_eq!(
            serde_json::to_value(other).unwrap(),
            serde_json::to_value(&r).unwrap()
        );
        assert_eq!(
            serde_json::to_value(work).unwrap(),
            serde_json::to_value(w).unwrap()
        );
    }
    for zero in [true, false] {
        let policy = |_: &Node, _: &Link, _: Option<&Node>| Ok(Some(if zero { 0. } else { 1. }));
        let (r, _, _) = run(&graph, 5, small(), policy);
        let Route::Found { nodes, cost, .. } = r else {
            panic!()
        };
        assert_eq!(nodes, vec![id(0), id(1), id(4), id(5)]);
        assert_eq!(cost, if zero { 0. } else { 3. });
    }
}
#[test]
fn stale_pops_edge_cursor_and_reconstruction_never_restart() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let calls = Cell::new(0);
    let (r, w, _) = run(&graph, 6, small(), |a, b, c| {
        calls.set(calls.get() + 1);
        costs(a, b, c)
    });
    assert!(matches!(r, Route::Unreachable));
    assert_eq!(w.edge_tests, 13);
    assert_eq!(calls.get(), 13);
    assert_eq!(w.expansions, 6);
    assert_eq!(w.stale_pops, 2);
    assert_eq!(w.heap_pops, 8);
    let mut job = graph
        .start_search(&id(0), &id(5), SearchLimits::default(), &costs)
        .unwrap();
    let no_path = StepBudget {
        path_nodes: 0,
        ..StepBudget::default()
    };
    let p = job.advance(no_path).unwrap();
    assert_eq!(p.state, SearchState::Pending);
    assert_eq!(p.cumulative.path_nodes, 0);
    let before = job.work().edge_tests;
    assert_eq!(job.advance(no_path).unwrap().step.edge_tests, 0);
    while job.advance(small()).unwrap().state != SearchState::Complete {}
    assert_eq!(job.work().edge_tests, before);
    assert!(matches!(job.finish().unwrap(), Route::Found { .. }));
}
#[test]
fn admission_storage_copies_and_heap_have_exact_one_under_boundaries() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let policy: fn(&Node, &Link, Option<&Node>) -> CostDecision = costs;
    let job = graph
        .start_search(&id(0), &id(5), SearchLimits::default(), &policy)
        .unwrap();
    let a = job.admission();
    job.cancel();
    assert_eq!(a.nodes, 8);
    assert_eq!(a.edges, 13);
    assert_eq!(a.visits, 40);
    assert_eq!(a.heap_capacity, 14);
    let exact = SearchLimits {
        retained_bytes: a.reserved_bytes,
        admission_visits: a.visits,
        ..SearchLimits::default()
    };
    graph
        .start_search(&id(0), &id(5), exact, &policy)
        .unwrap()
        .cancel();
    for l in [
        SearchLimits {
            retained_bytes: a.reserved_bytes - 1,
            ..exact
        },
        SearchLimits {
            admission_visits: a.visits - 1,
            ..exact
        },
    ] {
        assert!(matches!(
            graph.start_search(&id(0), &id(5), l, &policy),
            Err(RouteError::Budget(_))
        ));
    }
    let mut job = graph
        .start_search(
            &id(0),
            &id(6),
            SearchLimits {
                heap_pops: 7,
                ..SearchLimits::default()
            },
            &policy,
        )
        .unwrap();
    assert!(job.advance(StepBudget::default()).is_err());
    assert!(job.advance(small()).is_err());
    assert!(job.finish().is_err());
    let mut job = graph
        .start_search(&id(0), &id(5), SearchLimits::default(), &policy)
        .unwrap();
    let p = job
        .advance(StepBudget {
            copies: 0,
            ..StepBudget::default()
        })
        .unwrap();
    assert_eq!(p.cumulative.edge_tests, 0);
    let p = job.advance(small()).unwrap();
    assert_eq!(p.step.edge_tests, 1);
    assert_eq!(p.step.copies, 1);
    job.cancel();
}
#[test]
fn missing_special_diagnostics_and_atomic_failures_keep_original_policy() {
    let mut source = fixture();
    source.mesh.triangles[0].edges[2] = 0;
    source.mesh.triangles[0].flags = 4;
    source.mesh.edge_links.push(EdgeLink {
        link_type: 7,
        navmesh_raw: 200,
        triangle: 9,
    });
    source.external_targets.push(Some(key(200)));
    let graph = RouteGraph::build(&[source], GraphLimits::default()).unwrap();
    for unsupported in [false, true] {
        let policy = |a: &Node, b: &Link, c: Option<&Node>| {
            if matches!(b.kind, LinkKind::External { .. }) {
                if unsupported {
                    Err("literal special capability unavailable".into())
                } else {
                    Ok(Some(1.))
                }
            } else {
                costs(a, b, c)
            }
        };
        let (r, _, _) = run(&graph, 6, small(), policy);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::to_value(
                graph
                    .route(&id(0), &id(6), RouteLimits::default(), policy)
                    .unwrap()
            )
            .unwrap()
        );
        if unsupported {
            assert!(matches!(r, Route::Unsupported { .. }));
        } else {
            assert!(matches!(r, Route::MissingNeighbors { .. }));
        }
    }
    for bad in [-1., f64::NAN, f64::INFINITY] {
        let policy = |_: &Node, _: &Link, _: Option<&Node>| Ok(Some(bad));
        let mut job = graph
            .start_search(&id(0), &id(5), SearchLimits::default(), &policy)
            .unwrap();
        assert!(job.advance(small()).is_err());
        assert!(job.finish().is_err());
    }
    let policy = |_: &Node, _: &Link, _: Option<&Node>| Err("x".repeat(1025));
    let mut job = graph
        .start_search(&id(0), &id(6), SearchLimits::default(), &policy)
        .unwrap();
    assert!(job.advance(small()).is_err());
    assert!(job.finish().is_err());
    for route in [
        RouteLimits {
            expansions: 0,
            ..RouteLimits::default()
        },
        RouteLimits {
            edge_tests: 0,
            ..RouteLimits::default()
        },
        RouteLimits {
            path_nodes: 1,
            ..RouteLimits::default()
        },
        RouteLimits {
            diagnostics: 0,
            ..RouteLimits::default()
        },
    ] {
        let policy = |a: &Node, b: &Link, c: Option<&Node>| {
            if matches!(b.kind, LinkKind::External { .. }) {
                Err("unsupported".into())
            } else {
                costs(a, b, c)
            }
        };
        let mut job = graph
            .start_search(
                &id(0),
                &id(5),
                SearchLimits {
                    route,
                    ..SearchLimits::default()
                },
                &policy,
            )
            .unwrap();
        assert!(job.advance(StepBudget::default()).is_err());
        assert!(job.finish().is_err());
    }
}
struct Watch(Rc<Cell<usize>>);
impl Drop for Watch {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
#[test]
fn cancel_drop_and_incomplete_finish_release_only_owned_state() {
    let graph = RouteGraph::build(&[fixture()], GraphLimits::default()).unwrap();
    let drops = Rc::new(Cell::new(0));
    for action in 0..3 {
        let watch = Watch(drops.clone());
        let policy = move |a: &Node, b: &Link, c: Option<&Node>| {
            let _ = &watch;
            costs(a, b, c)
        };
        let mut job = graph
            .start_search(&id(0), &id(5), SearchLimits::default(), &policy)
            .unwrap();
        assert_eq!(job.advance(small()).unwrap().state, SearchState::Pending);
        match action {
            0 => job.cancel(),
            1 => drop(job),
            _ => assert!(job.finish().is_err()),
        };
        assert_eq!(drops.get(), action);
        drop(policy);
        assert_eq!(drops.get(), action + 1);
    }
    let mut count = 0;
    let old = graph
        .route(&id(0), &id(5), RouteLimits::default(), |a, b, c| {
            count += 1;
            costs(a, b, c)
        })
        .unwrap();
    assert!(matches!(old, Route::Found { .. }));
    assert!(count > 0);
    let mut source = fixture();
    source.key.origin_plugin = "x".repeat(4097);
    let g = RouteGraph::build(&[source], GraphLimits::default()).unwrap();
    let start = g.nodes()[0].id.clone();
    let goal = g.nodes()[5].id.clone();
    assert!(
        g.start_search(&start, &goal, SearchLimits::default(), &costs)
            .is_err()
    );
}
