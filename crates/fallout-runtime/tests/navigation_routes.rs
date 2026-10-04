use fallout_data::{
    identity::{FormKey, ProfileId},
    navigation::{self, NavMesh, SourceMesh, Triangle},
    plugin,
    world::SourceField,
};
use fallout_runtime::navigation::*;
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "authored.esm".into(),
        local_id: id,
    }
}
fn input() -> SourceMesh {
    let vertices = vec![
        [0., 0., 0.],
        [1., 0., 0.],
        [0., 1., 0.],
        [1., 1., 0.],
        [2., 0., 0.],
        [2., 1., 0.],
        [10., 0., 0.],
        [11., 0., 0.],
        [10., 1., 0.],
    ];
    let triangles = vec![
        Triangle {
            vertices: [0, 1, 2],
            edges: [-1, 1, -1],
            flags: 0,
            cover_flags: 0,
        },
        Triangle {
            vertices: [1, 3, 2],
            edges: [2, -1, 0],
            flags: 0,
            cover_flags: 0,
        },
        Triangle {
            vertices: [1, 4, 3],
            edges: [-1, 3, 1],
            flags: 0,
            cover_flags: 0,
        },
        Triangle {
            vertices: [4, 5, 3],
            edges: [-1, -1, 2],
            flags: 0,
            cover_flags: 0,
        },
        Triangle {
            vertices: [6, 7, 8],
            edges: [-1; 3],
            flags: 0,
            cover_flags: 0,
        },
    ];
    SourceMesh {
        key: key(0x123),
        cell: Some(key(0x10)),
        source_plugin: "authored.esm".into(),
        source_sha256: "a".repeat(64),
        external_targets: vec![],
        door_targets: vec![],
        mesh: NavMesh {
            header: plugin::RecordHeader {
                kind: *b"NAVM",
                offset: 100,
                stored_size: 0,
                flags: 0,
                form_id: 0x123,
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
                value: 0x10,
            },
            vertices,
            triangles,
            edge_links: vec![],
            cover_triangles: vec![],
            door_links: vec![],
            fields: vec![],
        },
    }
}
fn triangle(index: usize) -> TriangleId {
    TriangleId {
        mesh: key(0x123),
        triangle: index,
    }
}
#[test]
fn authored_corridor_cycles_and_unreachable_component_have_bounded_exact_routes() {
    let graph = RouteGraph::build(&[input()], GraphLimits::default()).unwrap();
    let Route::Found { nodes, links, cost } = graph
        .route(
            &triangle(0),
            &triangle(3),
            RouteLimits::default(),
            |_, _, _| Ok(Some(1.)),
        )
        .unwrap()
    else {
        panic!("route missing");
    };
    assert_eq!(
        nodes,
        vec![triangle(0), triangle(1), triangle(2), triangle(3)]
    );
    assert_eq!(cost, 3.);
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].portal, [[1., 0., 0.], [0., 1., 0.]]);
    assert!(matches!(
        graph
            .route(
                &triangle(0),
                &triangle(4),
                RouteLimits::default(),
                |_, _, _| Ok(Some(0.))
            )
            .unwrap(),
        Route::Unreachable
    ));
    assert!(matches!(
        graph
            .route(
                &triangle(0),
                &triangle(3),
                RouteLimits::default(),
                |_, _, _| Ok(None)
            )
            .unwrap(),
        Route::Unreachable
    ));
}
#[test]
fn missing_neighbors_special_link_decisions_and_doors_remain_explicit() {
    let mut source = input();
    source.mesh.triangles[0].edges[0] = 0;
    source.mesh.triangles[0].flags |= 1;
    source.mesh.edge_links.push(navigation::EdgeLink {
        link_type: 1,
        navmesh_raw: 0x456,
        triangle: 7,
    });
    source.external_targets.push(Some(key(0x456)));
    source.mesh.door_links.push(navigation::DoorLink {
        door_raw: 0x789,
        triangle: 0,
        unused: [0x81, 0x82],
    });
    source.door_targets.push(Some(key(0x789)));
    let graph = RouteGraph::build(&[source], GraphLimits::default()).unwrap();
    assert_eq!(
        graph.node(&triangle(0)).unwrap().doors[0].unused,
        [0x81, 0x82]
    );
    assert!(matches!(
        graph
            .route(
                &triangle(0),
                &triangle(4),
                RouteLimits::default(),
                |_, _, _| Ok(Some(1.))
            )
            .unwrap(),
        Route::MissingNeighbors { .. }
    ));
    let outcome = graph
        .route(
            &triangle(0),
            &triangle(4),
            RouteLimits::default(),
            |_, link, _| match link.kind {
                LinkKind::External { link_type: 1, .. } => {
                    Err("ledge movement has no measured policy".into())
                }
                _ => Ok(Some(1.)),
            },
        )
        .unwrap();
    let Route::Unsupported { links, .. } = outcome else {
        panic!("special link silently admitted");
    };
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].link.target,
        Some(TriangleId {
            mesh: key(0x456),
            triangle: 7
        })
    );
    // A known alternative route is valid without claiming special-link movement.
    assert!(matches!(
        graph
            .route(
                &triangle(0),
                &triangle(3),
                RouteLimits::default(),
                |_, link, _| match link.kind {
                    LinkKind::External { .. } => Err("unavailable".into()),
                    _ => Ok(Some(1.)),
                }
            )
            .unwrap(),
        Route::Found { .. }
    ));
}
#[test]
fn budgets_bad_costs_nonshared_portals_and_duplicate_source_identity_fail() {
    assert!(RouteGraph::build(&[input(), input()], GraphLimits::default()).is_err());
    assert!(
        RouteGraph::build(
            &[input()],
            GraphLimits {
                nodes: 0,
                ..GraphLimits::default()
            }
        )
        .is_err()
    );
    let mut bad = input();
    bad.mesh.triangles[0].edges[1] = 4;
    assert!(RouteGraph::build(&[bad], GraphLimits::default()).is_err());
    let graph = RouteGraph::build(&[input()], GraphLimits::default()).unwrap();
    for limits in [
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
    ] {
        assert!(matches!(
            graph.route(&triangle(0), &triangle(3), limits, |_, _, _| Ok(Some(1.))),
            Err(RouteError::Budget(_))
        ));
    }
    for cost in [-1., f64::NAN, f64::INFINITY] {
        assert!(matches!(
            graph.route(
                &triangle(0),
                &triangle(3),
                RouteLimits::default(),
                |_, _, _| Ok(Some(cost))
            ),
            Err(RouteError::Invalid(_))
        ));
    }
}
