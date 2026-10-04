mod common;
use common::{field, form, header, record};
use fallout_data::{
    navigation::{self, CellSetLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::navigation::{
    GraphLimits, LinkKind, Route, RouteGraph, RouteLimits, TriangleId,
};
use std::fs;
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
fn nav(cell: u32, second: bool) -> Vec<u8> {
    let mut body = field(b"NVER", &11u32.to_le_bytes());
    let mut data = Vec::new();
    for n in [cell, 4, 2, 1, 1, u32::from(second)] {
        data.extend(n.to_le_bytes());
    }
    body.extend(field(b"DATA", &data));
    let shift = if second { 1000f32 } else { 0. };
    let mut vertices = Vec::new();
    for point in [[0f32, 0., 0.], [2., 0., 0.], [0., 2., 0.], [2., 2., 0.]] {
        for n in [point[0] + shift, point[1], point[2]] {
            vertices.extend(n.to_le_bytes());
        }
    }
    body.extend(field(b"NVVX", &vertices));
    let mut triangles = Vec::new();
    let rows = if second {
        [
            [0u16, 1, 2, 0xffff, 1, 0xffff, 0, 0xabcd],
            [1, 3, 2, 0, 0xffff, 0, 1, 0x1234],
        ]
    } else {
        [
            [0u16, 1, 2, 0, 0xffff, 0xffff, 1, 0xabcd],
            [1, 3, 2, 0xffff, 0xffff, 0xffff, 0, 0x1234],
        ]
    };
    for row in rows {
        for n in row {
            triangles.extend(n.to_le_bytes());
        }
    }
    body.extend(field(b"NVTR", &triangles));
    let edge = [
        (if second { 2u32 } else { 0 }).to_le_bytes().as_slice(),
        &(if second { 0x100u32 } else { 0x200 }).to_le_bytes(),
        &(if second { 1u16 } else { 0 }).to_le_bytes(),
    ]
    .concat();
    body.extend(field(b"NVEX", &edge));
    body.extend(field(b"NVCA", &1u16.to_le_bytes()));
    if second {
        body.extend(field(
            b"NVDP",
            &[
                0x300u32.to_le_bytes().as_slice(),
                &1u16.to_le_bytes(),
                &[0x81, 0x82],
            ]
            .concat(),
        ));
    }
    body.extend(field(b"ZZZZ", &[9, 8, 7]));
    body
}
struct Fixture {
    root: tempfile::TempDir,
    order: Vec<String>,
}
impl Fixture {
    fn new(mode: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        let mut bytes = header(&[]);
        for (id, name) in [(0x10, b"CellA\0".as_slice()), (0x20, b"CellB\0".as_slice())] {
            let body = [field(b"EDID", name), field(b"DATA", &[1])].concat();
            bytes.extend(record(
                if mode == 4 && id == 0x20 {
                    b"STAT"
                } else {
                    b"CELL"
                },
                id,
                if mode == 3 && id == 0x20 {
                    plugin::DELETED
                } else {
                    0
                },
                &body,
            ));
        }
        bytes.extend(group(0x10, &record(b"NAVM", 0x100, 0, &nav(0x10, false))));
        let mut second = nav(if mode == 1 { 0x10 } else { 0x20 }, true);
        if mode == 2 {
            second.truncate(second.len() - 1);
        }
        bytes.extend(group(0x20, &record(b"NAVM", 0x200, 0, &second)));
        bytes.extend(record(b"REFR", 0x300, 0, &[]));
        fs::write(root.path().join("Data/FalloutNV.esm"), bytes).unwrap();
        let mut order = vec!["FalloutNV.esm".into()];
        if mode == 5 {
            fs::write(
                root.path().join("Data/Patch.esp"),
                [
                    header(&["FalloutNV.esm"]),
                    group(0x10, &record(b"NAVM", 0x100, 0, &nav(0x10, false))),
                ]
                .concat(),
            )
            .unwrap();
            order.push("Patch.esp".into());
        }
        Self { root, order }
    }
    fn store(&self) -> RecordStore {
        RecordStore::open_nv_headers(
            &self.root.path().join("Data"),
            &self.order,
            Default::default(),
        )
        .unwrap()
    }
}
fn id(mesh: u32, triangle: usize) -> TriangleId {
    TriangleId {
        mesh: form(mesh),
        triangle,
    }
}
fn policy(
    from: &fallout_runtime::navigation::Node,
    link: &fallout_runtime::navigation::Link,
    to: Option<&fallout_runtime::navigation::Node>,
    special: bool,
    doors: bool,
) -> fallout_runtime::navigation::CostDecision {
    if !doors && (!from.doors.is_empty() || to.is_some_and(|n| !n.doors.is_empty())) {
        return Ok(None);
    }
    match link.kind {
        LinkKind::Local => Ok(Some(1.)),
        LinkKind::External { link_type: 0, .. } => Ok(Some(3.)),
        LinkKind::External { link_type: 2, .. } if special => Ok(Some(5.)),
        _ => Err("no explicit special-link policy".into()),
    }
}
#[test]
fn explicit_cells_close_exact_missing_neighbor_and_keep_unrelated_source_portals() {
    let fixture = Fixture::new(0);
    let mut store = fixture.store();
    let old = navigation::load_cell(&mut store, &form(0x10), 10_000, Default::default()).unwrap();
    let one = RouteGraph::build(&old, Default::default()).unwrap();
    let Route::MissingNeighbors { links } = one
        .route(
            &id(0x100, 0),
            &id(0x100, 1),
            Default::default(),
            |a, l, b| policy(a, l, b, true, true),
        )
        .unwrap()
    else {
        panic!("missing neighbor was hidden");
    };
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].target, Some(id(0x200, 0)));
    assert_eq!(links[0].source_edge, 0);
    assert_eq!(links[0].portal, [[0., 0., 0.], [2., 0., 0.]]);
    let set =
        navigation::load_cells(&mut store, &[form(0x20), form(0x10)], Default::default()).unwrap();
    assert_eq!(
        set.cells.iter().map(|c| c.key.local_id).collect::<Vec<_>>(),
        [0x10, 0x20]
    );
    assert_eq!(
        set.meshes
            .iter()
            .map(|m| m.key.local_id)
            .collect::<Vec<_>>(),
        [0x100, 0x200]
    );
    assert_eq!(set.usage.record_bytes, 382);
    assert_eq!(set.usage.fields, 19);
    assert_eq!(set.usage.elements, 17);
    let graph = RouteGraph::build(&set.meshes, Default::default()).unwrap();
    let route = graph
        .route(
            &id(0x100, 0),
            &id(0x100, 1),
            Default::default(),
            |a, l, b| policy(a, l, b, true, true),
        )
        .unwrap();
    let Route::Found { nodes, links, cost } = &route else {
        panic!("explicit source route missing");
    };
    assert_eq!(
        *nodes,
        [id(0x100, 0), id(0x200, 0), id(0x200, 1), id(0x100, 1)]
    );
    assert_eq!(*cost, 9.);
    assert_eq!(links[1].source_edge, 1);
    assert_eq!(links[1].portal, [[1002., 0., 0.], [1000., 2., 0.]]);
    assert_eq!(links[2].source_edge, 0);
    assert_eq!(links[2].portal, [[1002., 0., 0.], [1002., 2., 0.]]);
    assert!(matches!(
        links[2].kind,
        LinkKind::External {
            link_type: 2,
            navmesh_raw: 0x100
        }
    ));
    assert_eq!(
        graph.node(&id(0x200, 1)).unwrap().doors[0].unused,
        [0x81, 0x82]
    );
    let reverse =
        navigation::load_cells(&mut store, &[form(0x10), form(0x20)], Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(set).unwrap(),
        serde_json::to_value(&reverse).unwrap()
    );
    let reverse = RouteGraph::build(&reverse.meshes, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(route).unwrap(),
        serde_json::to_value(
            reverse
                .route(
                    &id(0x100, 0),
                    &id(0x100, 1),
                    Default::default(),
                    |a, l, b| policy(a, l, b, true, true)
                )
                .unwrap()
        )
        .unwrap()
    );
}
#[test]
fn all_cell_mesh_record_field_element_index_source_and_metadata_limits_are_global() {
    let fixture = Fixture::new(0);
    let mut store = fixture.store();
    let cells = [form(0x10), form(0x20)];
    let baseline = navigation::load_cells(&mut store, &cells, Default::default()).unwrap();
    let u = baseline.usage;
    let exact = CellSetLimits {
        cells: 2,
        meshes: 2,
        index_visits: u.index_visits,
        source_bytes: u.source_bytes,
        records: navigation::Limits {
            record_bytes: 382,
            fields: 19,
            elements: 17,
        },
        identity_metadata_bytes: u.identity_metadata_bytes,
    };
    assert!(navigation::load_cells(&mut store, &cells, exact).is_ok());
    for case in 0..8 {
        let mut limits = exact;
        match case {
            0 => limits.cells -= 1,
            1 => limits.meshes -= 1,
            2 => limits.index_visits -= 1,
            3 => limits.source_bytes -= 1,
            4 => limits.records.record_bytes -= 1,
            5 => limits.records.fields -= 1,
            6 => limits.records.elements -= 1,
            7 => limits.identity_metadata_bytes -= 1,
            _ => unreachable!(),
        };
        assert!(
            navigation::load_cells(&mut store, &cells, limits).is_err(),
            "case {case}"
        );
    }
}
#[test]
fn duplicate_missing_deleted_wrong_kind_and_late_bad_meshes_publish_no_selection() {
    let fixture = Fixture::new(0);
    let mut store = fixture.store();
    for cells in [
        vec![],
        vec![form(0x10), form(0x10)],
        vec![form(0x10), form(0x999)],
    ] {
        assert!(navigation::load_cells(&mut store, &cells, Default::default()).is_err());
    }
    let mut noncanonical = form(0x10);
    noncanonical.origin_plugin = "FalloutNV.esm".into();
    assert!(navigation::load_cells(&mut store, &[noncanonical], Default::default()).is_err());
    for mode in 1..=4 {
        let fixture = Fixture::new(mode);
        let mut store = fixture.store();
        assert!(
            navigation::load_cells(&mut store, &[form(0x10), form(0x20)], Default::default())
                .is_err(),
            "mode {mode}"
        );
        assert!(
            navigation::load_cells(&mut store, &[form(0x10)], Default::default()).is_ok(),
            "unselected neighbor mode {mode}"
        );
    }
    assert!(
        navigation::load_cells(
            &mut store,
            &[form(0x10)],
            CellSetLimits {
                cells: 65,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn global_graph_search_and_explicit_door_special_policy_stay_with_existing_solver() {
    let fixture = Fixture::new(0);
    let mut store = fixture.store();
    let set =
        navigation::load_cells(&mut store, &[form(0x10), form(0x20)], Default::default()).unwrap();
    let exact = GraphLimits {
        nodes: 4,
        edges: 4,
        door_links: 1,
    };
    let graph = RouteGraph::build(&set.meshes, exact).unwrap();
    for limits in [
        GraphLimits { nodes: 3, ..exact },
        GraphLimits { edges: 3, ..exact },
        GraphLimits {
            door_links: 0,
            ..exact
        },
    ] {
        assert!(RouteGraph::build(&set.meshes, limits).is_err());
    }
    let exact = RouteLimits {
        expansions: 4,
        edge_tests: 4,
        path_nodes: 4,
        diagnostics: 0,
    };
    assert!(matches!(
        graph
            .route(&id(0x100, 0), &id(0x100, 1), exact, |a, l, b| policy(
                a, l, b, true, true
            ))
            .unwrap(),
        Route::Found { cost: 9., .. }
    ));
    for limits in [
        RouteLimits {
            expansions: 3,
            ..exact
        },
        RouteLimits {
            edge_tests: 3,
            ..exact
        },
        RouteLimits {
            path_nodes: 3,
            ..exact
        },
    ] {
        assert!(
            graph
                .route(&id(0x100, 0), &id(0x100, 1), limits, |a, l, b| policy(
                    a, l, b, true, true
                ))
                .is_err()
        );
    }
    assert!(matches!(
        graph
            .route(
                &id(0x100, 0),
                &id(0x100, 1),
                Default::default(),
                |a, l, b| policy(a, l, b, false, true)
            )
            .unwrap(),
        Route::Unsupported { .. }
    ));
    assert!(matches!(
        graph
            .route(
                &id(0x100, 0),
                &id(0x100, 1),
                Default::default(),
                |a, l, b| policy(a, l, b, true, false)
            )
            .unwrap(),
        Route::Unreachable
    ));
}
#[test]
fn winning_override_uses_physical_master_table_and_full_source_digest() {
    let fixture = Fixture::new(5);
    let mut store = fixture.store();
    let set =
        navigation::load_cells(&mut store, &[form(0x10), form(0x20)], Default::default()).unwrap();
    assert_eq!(set.meshes[0].key, form(0x100));
    assert_eq!(set.meshes[0].source_plugin, "Patch.esp");
    assert_eq!(set.meshes[1].source_plugin, "FalloutNV.esm");
    assert_ne!(set.meshes[0].source_sha256, set.meshes[1].source_sha256);
    assert_eq!(set.meshes[0].external_targets, [Some(form(0x200))]);
    assert_eq!(set.cells[0].source_plugin, "FalloutNV.esm");
    let graph = RouteGraph::build(&set.meshes, Default::default()).unwrap();
    assert!(matches!(
        graph
            .route(
                &id(0x100, 0),
                &id(0x100, 1),
                Default::default(),
                |a, l, b| policy(a, l, b, true, true)
            )
            .unwrap(),
        Route::Found { cost: 9., .. }
    ));
}
#[test]
#[ignore = "explicit private navigation cell-set CLI fixture export"]
fn cell_set_cli_fixture_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_CELL_SET_FIXTURE").expect("private fixture root"),
    );
    fs::create_dir(&root).unwrap();
    for mode in 0..=5 {
        let fixture = Fixture::new(mode);
        let install = root.join(format!("install-{mode}"));
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        for name in &fixture.order {
            fs::copy(
                fixture.root.path().join("Data").join(name),
                install.join("Data").join(name),
            )
            .unwrap();
        }
        fs::write(
            root.join(format!("order-{mode}.json")),
            serde_json::to_vec_pretty(&fixture.order).unwrap(),
        )
        .unwrap();
    }
    let request = serde_json::json!({"cells":[form(0x10),form(0x20)],"route":{"start":id(0x100,0),"goal":id(0x100,1),
        "local_cost":1.0,"portal_cost":3.0,"special_costs":{"2":5.0},"allow_disabled_records":false,
        "triangle_forbidden_mask":0,"permit_door_triangles":true}});
    fs::write(
        root.join("request.json"),
        serde_json::to_vec_pretty(&request).unwrap(),
    )
    .unwrap();
}
