mod common;
use common::{field, form, header, record};
use fallout_data::{plugin, store::RecordStore};
use fallout_runtime::navigation::{Route, RouteLimits, TriangleId, corridor::*};
use std::fs;
fn group(body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(24 + body.len() as u32).to_le_bytes(),
        &0x10u32.to_le_bytes(),
        &6i32.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn id(triangle: usize) -> TriangleId {
    TriangleId {
        mesh: form(0x100),
        triangle,
    }
}
fn plane() -> PlaneContract {
    PlaneContract::AxisAlignedDyadic { normal_axis: 2 }
}
fn mesh(mode: usize) -> Vec<u8> {
    let mut vertices = vec![
        [0f32, 0., -0.],
        [1., 0., 0.],
        [0., 1., 0.],
        [1., 1., 0.],
        [2., 0., 0.],
        [2., 1., 0.],
        [10., 0., 0.],
        [11., 0., 0.],
        [10., 1., 0.],
        [20., 0., 0.],
        [21., 0., 0.],
        [20., 1., 0.],
    ];
    let mut rows = [
        [6u16, 7, 8, 0xffff, 0xffff, 0xffff, 0x200, 0x1111],
        [1, 3, 2, 5, 0xffff, 4, 0x40, 0x2222],
        [4, 5, 3, 0xffff, 0xffff, 5, 0x100, 0x3333],
        [9, 10, 11, 0xffff, 0xffff, 0xffff, 0x400, 0x4444],
        [0, 1, 2, 0xffff, 1, 0xffff, 0x20, 0x5555],
        [1, 4, 3, 0xffff, 2, 1, 0x80, 0x6666],
    ];
    match mode {
        1 => rows[1][5] = 0xffff,
        2 => {
            rows[1] = [2, 3, 1, 0xffff, 5, 4, 0x40, 0x2222];
        }
        3 => vertices[3][2] = 0.25,
        4 => rows[4][2] = 0,
        5 => vertices[3][1] = f32::from_bits(1),
        7 | 8 => {
            rows[4][4] = 0;
            rows[4][6] |= 2;
        }
        9 => rows[1][2] = 8,
        10 => rows[4][4] = 0xfffe,
        13 => {
            for row in &mut rows {
                row.swap(0, 2);
                row.swap(3, 4);
            }
        }
        14 => {
            for p in &mut vertices {
                *p = [p[2], p[0], p[1]];
            }
        }
        15 => {
            for p in &mut vertices {
                p[0] *= f32::from_bits(1);
                p[1] *= f32::from_bits(1);
            }
        }
        16 => {
            for p in &mut vertices {
                p[0] *= f32::from_bits(0x71800000);
                p[1] *= f32::from_bits(0x71800000);
            }
        }
        _ => {}
    }
    let external = u32::from(matches!(mode, 7 | 8));
    let door = u32::from(mode == 6);
    let mut body = field(b"NVER", &11u32.to_le_bytes());
    body.extend(field(
        b"DATA",
        &[
            if mode == 12 { 0x20u32 } else { 0x10 },
            12,
            6,
            external,
            2,
            door,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>(),
    ));
    body.extend(field(
        b"NVVX",
        &vertices
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    ));
    let mut triangles = rows
        .into_iter()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    if mode == 11 {
        triangles.pop();
    }
    body.extend(field(b"NVTR", &triangles));
    if external != 0 {
        body.extend(field(
            b"NVEX",
            &[
                0u32.to_le_bytes().as_slice(),
                &(if mode == 8 { 0x200u32 } else { 0x100 }).to_le_bytes(),
                &1u16.to_le_bytes(),
            ]
            .concat(),
        ));
    }
    body.extend(field(
        b"NVCA",
        &[1u16.to_le_bytes(), 5u16.to_le_bytes()].concat(),
    ));
    if door != 0 {
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
}
impl Fixture {
    fn new(mode: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        let bytes = [
            header(&[]),
            record(
                b"CELL",
                0x10,
                0,
                &[field(b"EDID", b"Zigzag\0"), field(b"DATA", &[1])].concat(),
            ),
            group(&record(
                b"NAVM",
                0x100,
                plugin::INITIALLY_DISABLED,
                &mesh(mode),
            )),
            record(b"REFR", 0x300, 0, &[]),
        ]
        .concat();
        fs::write(root.path().join("Data/FalloutNV.esm"), bytes).unwrap();
        Self { root }
    }
    fn store(&self) -> RecordStore {
        RecordStore::open_nv_headers(
            &self.root.path().join("Data"),
            &["FalloutNV.esm".into()],
            Default::default(),
        )
        .unwrap()
    }
    fn query(&self) -> CorridorQuery {
        CorridorQuery::load(&mut self.store(), &form(0x10), InputLimits::default()).unwrap()
    }
}
fn request(query: &CorridorQuery, limits: CorridorLimits) -> CorridorOutcome {
    query
        .route(
            &id(4),
            &id(2),
            RouteLimits::default(),
            limits,
            plane(),
            |_, _, _| Ok(Some(1.)),
        )
        .unwrap()
}
#[test]
fn literal_zigzag_retains_exact_source_words_nonsequential_ids_and_reciprocal_portals() {
    let fixture = Fixture::new(0);
    let query = fixture.query();
    let outcome = request(&query, CorridorLimits::default());
    assert!(outcome.refusal.is_none());
    let Route::Found { nodes, links, cost } = &outcome.route else {
        panic!("not found")
    };
    assert_eq!(nodes.as_slice(), [id(4), id(1), id(5), id(2)]);
    assert_eq!(*cost, 3.);
    assert_eq!(links.len(), 3);
    let corridor = outcome.corridor().unwrap();
    assert_eq!(
        corridor
            .triangles()
            .iter()
            .map(|t| t.vertex_indices)
            .collect::<Vec<_>>(),
        vec![[0, 1, 2], [1, 3, 2], [1, 4, 3], [4, 5, 3]]
    );
    assert_eq!(
        corridor.triangles()[0].vertex_words,
        [[0, 0, 0x80000000], [0x3f800000, 0, 0], [0, 0x3f800000, 0]]
    );
    assert_eq!(
        corridor
            .triangles()
            .iter()
            .map(|t| t.triangle_flags)
            .collect::<Vec<_>>(),
        vec![0x20, 0x40, 0x80, 0x100]
    );
    assert_eq!(
        corridor
            .triangles()
            .iter()
            .map(|t| t.cover_flags)
            .collect::<Vec<_>>(),
        vec![0x5555, 0x2222, 0x6666, 0x3333]
    );
    assert_eq!(
        corridor
            .triangles()
            .iter()
            .map(|t| t.cover_listed)
            .collect::<Vec<_>>(),
        vec![false, true, true, false]
    );
    for t in corridor.triangles() {
        assert_eq!(t.record_flags, plugin::INITIALLY_DISABLED);
        assert_eq!(t.source_plugin, "FalloutNV.esm");
        assert_eq!(t.source_sha256.len(), 64);
        assert_eq!(t.decoded_sha256.len(), 64);
    }
    assert_eq!(
        corridor
            .portals()
            .iter()
            .map(|p| (p.source_edge, p.target_edge, p.vertex_indices))
            .collect::<Vec<_>>(),
        vec![(1, 2, [1, 2]), (0, 2, [1, 3]), (1, 2, [4, 3])]
    );
    assert_eq!(
        corridor
            .portals()
            .iter()
            .map(|p| p.vertices)
            .collect::<Vec<_>>(),
        vec![
            [[1., 0., 0.], [0., 1., 0.]],
            [[1., 0., 0.], [1., 1., 0.]],
            [[2., 0., 0.], [1., 1., 0.]]
        ]
    );
    let json = serde_json::to_value(corridor).unwrap();
    assert_eq!(json["winding"], 1);
    assert_eq!(json["cell"], serde_json::to_value(form(0x10)).unwrap());
}
#[test]
fn geometry_and_graph_refusals_are_atomic_without_snapping_or_door_external_contracts() {
    for (mode, reason) in [
        (1, "reciprocal"),
        (2, "winding"),
        (3, "axis plane"),
        (5, "dyadic coordinate"),
        (6, "door triangle"),
        (7, "external or special"),
    ] {
        let query = Fixture::new(mode).query();
        let outcome = request(&query, CorridorLimits::default());
        assert!(outcome.corridor().is_none(), "mode{mode}");
        assert!(outcome.refusal.unwrap().contains(reason), "mode{mode}");
        assert!(matches!(outcome.route, Route::Found { .. }));
    }
    for mode in [4, 9, 10, 11, 12] {
        assert!(
            CorridorQuery::load(
                &mut Fixture::new(mode).store(),
                &form(0x10),
                InputLimits::default()
            )
            .is_err(),
            "mode{mode}"
        );
    }
    let outcome = request(&Fixture::new(8).query(), CorridorLimits::default());
    assert!(outcome.corridor().is_none());
    assert!(matches!(outcome.route, Route::MissingNeighbors { .. }));
    let q = Fixture::new(0).query();
    let outcome = q
        .route(
            &id(4),
            &id(0),
            RouteLimits::default(),
            CorridorLimits::default(),
            plane(),
            |_, _, _| Ok(Some(1.)),
        )
        .unwrap();
    assert!(outcome.corridor().is_none());
    assert!(matches!(outcome.route, Route::Unreachable));
    let q = Fixture::new(7).query();
    let outcome = q
        .route(
            &id(4),
            &id(2),
            RouteLimits::default(),
            CorridorLimits::default(),
            plane(),
            |_, link, _| {
                if matches!(
                    link.kind,
                    fallout_runtime::navigation::LinkKind::External { .. }
                ) {
                    Err("no declared external policy".into())
                } else {
                    Ok(Some(1.))
                }
            },
        )
        .unwrap();
    assert!(outcome.corridor().is_none());
    assert!(matches!(outcome.route, Route::Unsupported { .. }));
}
#[test]
fn exact_corridor_and_double_decode_source_work_caps_apply_to_the_whole_result() {
    let fixture = Fixture::new(0);
    let query = fixture.query();
    let outcome = request(&query, CorridorLimits::default());
    let usage = outcome.corridor().unwrap().usage();
    let exact = CorridorLimits {
        triangles: usage.triangles,
        validation_visits: usage.validation_visits,
        geometry_bytes: usage.geometry_bytes,
        identity_bytes: usage.identity_bytes,
    };
    assert!(request(&query, exact).corridor().is_some());
    for under in [
        CorridorLimits {
            triangles: exact.triangles - 1,
            ..exact
        },
        CorridorLimits {
            validation_visits: exact.validation_visits - 1,
            ..exact
        },
        CorridorLimits {
            geometry_bytes: exact.geometry_bytes - 1,
            ..exact
        },
        CorridorLimits {
            identity_bytes: exact.identity_bytes - 1,
            ..exact
        },
    ] {
        assert!(matches!(
            query.route(
                &id(4),
                &id(2),
                RouteLimits::default(),
                under,
                plane(),
                |_, _, _| Ok(Some(1.))
            ),
            Err(CorridorError::Budget(_))
        ));
    }
    let u = query.usage();
    let mut exact = InputLimits {
        source_bytes: u.source_bytes,
        index_visits: u.index_visits,
        meshes: u.meshes,
        records: fallout_data::navigation::Limits {
            record_bytes: u.record_bytes,
            fields: u.fields,
            elements: u.elements,
        },
        retained_bytes: u.retained_reservation_bytes,
        graph: fallout_runtime::navigation::GraphLimits {
            nodes: 6,
            edges: 6,
            door_links: 0,
        },
    };
    assert!(CorridorQuery::load(&mut fixture.store(), &form(0x10), exact).is_ok());
    for which in 0..7 {
        let mut under = exact;
        match which {
            0 => under.source_bytes -= 1,
            1 => under.index_visits -= 1,
            2 => under.meshes -= 1,
            3 => under.records.record_bytes -= 1,
            4 => under.records.fields -= 1,
            5 => under.records.elements -= 1,
            6 => under.retained_bytes -= 1,
            _ => unreachable!(),
        };
        assert!(
            CorridorQuery::load(&mut fixture.store(), &form(0x10), under).is_err(),
            "bound{which}"
        );
    }
    exact.graph.nodes = 5;
    assert!(CorridorQuery::load(&mut fixture.store(), &form(0x10), exact).is_err());
    let mut long = form(0x10);
    long.origin_plugin = "A".repeat(100000);
    assert!(matches!(
        CorridorQuery::load(
            &mut fixture.store(),
            &long,
            InputLimits {
                retained_bytes: 4096,
                ..InputLimits::default()
            }
        ),
        Err(CorridorError::Budget("cell identity reservation"))
    ));
}
#[test]
fn reversal_single_triangle_and_explicit_plane_contract_remain_source_qualified() {
    let q = Fixture::new(13).query();
    let o = request(&q, CorridorLimits::default());
    assert!(o.corridor().is_some());
    assert_eq!(
        serde_json::to_value(o.corridor().unwrap()).unwrap()["winding"],
        -1
    );
    let q = Fixture::new(0).query();
    let o = q
        .route(
            &id(4),
            &id(4),
            RouteLimits::default(),
            CorridorLimits::default(),
            plane(),
            |_, _, _| Ok(Some(1.)),
        )
        .unwrap();
    assert_eq!(o.corridor().unwrap().triangles().len(), 1);
    assert!(o.corridor().unwrap().portals().is_empty());
    assert!(
        q.route(
            &id(4),
            &id(2),
            RouteLimits::default(),
            CorridorLimits::default(),
            PlaneContract::AxisAlignedDyadic { normal_axis: 3 },
            |_, _, _| Ok(Some(1.))
        )
        .is_err()
    );
    let o = q
        .route(
            &id(4),
            &id(2),
            RouteLimits::default(),
            CorridorLimits::default(),
            PlaneContract::AxisAlignedDyadic { normal_axis: 0 },
            |_, _, _| Ok(Some(1.)),
        )
        .unwrap();
    assert!(o.corridor().is_none());
    assert!(
        q.route(
            &id(99),
            &id(2),
            RouteLimits::default(),
            CorridorLimits::default(),
            plane(),
            |_, _, _| Ok(Some(1.))
        )
        .is_err()
    );
}
#[test]
fn exact_dyadic_contract_preserves_subnormal_large_and_cyclic_axis_source_words() {
    for (mode, axis, word) in [(14, 0, 0x3f800000u32), (15, 2, 1), (16, 2, 0x71800000)] {
        let query = Fixture::new(mode).query();
        let outcome = query
            .route(
                &id(4),
                &id(2),
                RouteLimits::default(),
                CorridorLimits::default(),
                PlaneContract::AxisAlignedDyadic { normal_axis: axis },
                |_, _, _| Ok(Some(1.)),
            )
            .unwrap();
        let corridor = outcome.corridor().unwrap();
        assert_eq!(
            corridor.triangles()[0].vertex_words[1][(axis + 1) % 3],
            word
        );
        assert_eq!(
            corridor
                .triangles()
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
            vec![id(4), id(1), id(5), id(2)]
        );
        assert_eq!(corridor.portals().len(), 3);
    }
}
#[test]
#[ignore = "explicit private source corridor CLI fixture export"]
fn corridor_cli_fixture_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_CORRIDOR_FIXTURE").expect("explicit directory"),
    );
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    for mode in 0..17 {
        let f = Fixture::new(mode);
        let install = root.join(format!("install-{mode}"));
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        fs::copy(
            f.root.path().join("Data/FalloutNV.esm"),
            install.join("Data/FalloutNV.esm"),
        )
        .unwrap();
    }
    fs::write(
        root.join("order.json"),
        b"[\"FalloutNV.esm\"]",
    )
    .unwrap();
    let route = serde_json::json!({"start":id(4),"goal":id(2),"local_cost":1.,"portal_cost":1.,"special_costs":{},"allow_disabled_records":true,"triangle_forbidden_mask":0,"permit_door_triangles":true});
    fs::write(root.join("request.json"),serde_json::to_vec_pretty(&serde_json::json!({"cell":form(0x10),"route":route,"plane":{"contract":"axis_aligned_dyadic","normal_axis":2}})).unwrap()).unwrap();
}
