mod common;
use common::{field, form, header, record};
use fallout_data::{plugin, store::RecordStore};
use fallout_runtime::navigation::{
    TriangleId,
    corridor::{InputLimits, PlaneContract},
    endpoint::*,
};
use std::fs;
fn group(body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &0x10u32.to_le_bytes(),
        &6i32.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn nav(mode: usize) -> Vec<u8> {
    let mut vertices = [[0f32, 0., -0.], [2., 0., 0.], [0., 2., 0.]];
    let mut ids = [0u16, 1, 2];
    match mode {
        1 => ids = [2, 1, 0],
        2 => ids = [0, 1, 0],
        3 => vertices[2][2] = 0.25,
        4 => vertices[2][0] = f32::NAN,
        7 => {
            vertices[1][0] = f32::from_bits(0x71800000);
            vertices[2][1] = f32::from_bits(0x0d800000);
        }
        8 => {
            for p in &mut vertices {
                *p = [p[2], p[0], p[1]];
            }
        }
        9 | 10 => {
            let scale = if mode == 9 {
                f32::from_bits(1)
            } else {
                f32::from_bits(0x71800000)
            };
            for p in &mut vertices {
                p[0] *= scale;
                p[1] *= scale;
            }
        }
        11 => vertices = [[-2., -2., -0.], [2., -2., 0.], [-2., 2., 0.]],
        _ => {}
    }
    let mut body = field(b"NVER", &11u32.to_le_bytes());
    body.extend(field(
        b"DATA",
        &[if mode == 5 { 0x20u32 } else { 0x10 }, 3, 4, 0, 1, 0]
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
    let mut triangles = Vec::new();
    for i in 0..4 {
        let selected = if i == 3 { ids } else { [0, 1, 2] };
        for n in [
            selected[0],
            selected[1],
            selected[2],
            0xffff,
            0xffff,
            0xffff,
            if i == 3 { 0x120 } else { 0 },
            if i == 3 { 0xabcd } else { 0 },
        ] {
            triangles.extend(n.to_le_bytes());
        }
    }
    body.extend(field(b"NVTR", &triangles));
    body.extend(field(b"NVCA", &3u16.to_le_bytes()));
    body.extend(field(b"ZZZZ", &[9, 8, if mode == 12 { 6 } else { 7 }]));
    if mode == 6 {
        body.pop();
    }
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
        let bytes = [
            header(&[]),
            record(
                if mode == 14 { b"STAT" } else { b"CELL" },
                0x10,
                if mode == 15 { plugin::DELETED } else { 0 },
                &[field(b"EDID", b"Endpoints\0"), field(b"DATA", &[1])].concat(),
            ),
            group(&record(
                b"NAVM",
                0x100,
                plugin::INITIALLY_DISABLED,
                &nav(if mode == 12 { 0 } else { mode }),
            )),
        ]
        .concat();
        fs::write(root.path().join("Data/FalloutNV.esm"), bytes).unwrap();
        let mut order = vec!["FalloutNV.esm".into()];
        if mode == 12 {
            fs::write(
                root.path().join("Data/Patch.esp"),
                [
                    header(&["FalloutNV.esm"]),
                    group(&record(
                        b"NAVM",
                        0x100,
                        plugin::INITIALLY_DISABLED,
                        &nav(12),
                    )),
                ]
                .concat(),
            )
            .unwrap();
            order.push("Patch.esp".into());
        }
        if mode == 13 {
            fs::write(root.path().join("Data/Other.esm"), header(&[])).unwrap();
            order.push("Other.esm".into());
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
fn request(point: [f64; 3]) -> EndpointRequest {
    EndpointRequest {
        triangle: TriangleId {
            mesh: form(0x100),
            triangle: 3,
        },
        point,
        plane: PlaneContract::AxisAlignedDyadic { normal_axis: 2 },
    }
}
fn value(query: &EndpointQuery<'_>, requests: &[EndpointRequest]) -> serde_json::Value {
    let batch = query.inspect(requests, EndpointLimits::default()).unwrap();
    serde_json::to_value(query.observations(&batch).unwrap()).unwrap()
}
#[test]
fn literal_rational_triangle_classifies_inside_edge_vertex_exterior_and_one_bit_plane_changes() {
    let fixture = Fixture::new(0);
    let mut store = fixture.store();
    let query = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
    let points = [
        [0.5, 0.5, 0.],
        [1., 1., 0.],
        [0., 0., -0.],
        [0., 1., 0.],
        [-0.5, 0.5, 0.],
        [1., f64::from_bits(0x3fefffffffffffff), 0.],
        [1., f64::from_bits(0x3ff0000000000001), 0.],
        [0.5, 0.5, f64::from_bits(1)],
    ];
    let requests = points.into_iter().map(request).collect::<Vec<_>>();
    let batch = query.inspect(&requests, EndpointLimits::default()).unwrap();
    let view = query.observations(&batch).unwrap();
    let obs = view.observations();
    assert!(matches!(obs[0].classification, Classification::Inside));
    assert!(matches!(
        obs[1].classification,
        Classification::OnEdge { edge: 1 }
    ));
    assert!(matches!(
        obs[2].classification,
        Classification::OnVertex { vertex: 0 }
    ));
    assert!(matches!(
        obs[3].classification,
        Classification::OnEdge { edge: 2 }
    ));
    assert!(matches!(
        obs[4].classification,
        Classification::Outside { off_plane: false }
    ));
    assert!(matches!(obs[5].classification, Classification::Inside));
    assert!(matches!(
        obs[6].classification,
        Classification::Outside { off_plane: false }
    ));
    assert!(matches!(
        obs[7].classification,
        Classification::Outside { off_plane: true }
    ));
    for (observation, point) in obs.iter().zip(points) {
        assert_eq!(observation.point_words, point.map(f64::to_bits));
        assert_eq!(observation.triangle.triangle, 3);
        assert_eq!(observation.cell, form(0x10));
        assert_eq!(observation.vertex_indices, [0, 1, 2]);
        assert_eq!(
            observation.vertex_words,
            [[0, 0, 0x80000000], [0x40000000, 0, 0], [0, 0x40000000, 0]]
        );
        assert_eq!(observation.record_flags, plugin::INITIALLY_DISABLED);
        assert_eq!(observation.triangle_flags, 0x120);
        assert_eq!(observation.cover_flags, 0xabcd);
        assert_eq!(observation.raw_edges, [-1; 3]);
        assert_eq!(observation.source_sha256.len(), 64);
        assert_eq!(observation.decoded_sha256.len(), 64);
    }
    assert_eq!(obs[2].point_words[2], 0x8000000000000000);
    assert_eq!(obs[7].point_words[2], 1);
    let json = serde_json::to_value(view).unwrap();
    assert_eq!(json["sources"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["cell"]["key"],
        serde_json::to_value(form(0x10)).unwrap()
    );
    assert_eq!(json["scope_sha256"].as_str().unwrap().len(), 64);
}
#[test]
fn reversed_winding_axis_contract_and_large_subnormal_negative_source_domains_are_exact() {
    for mode in [1, 8, 9, 10, 11] {
        let f = Fixture::new(mode);
        let mut store = f.store();
        let q = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
        let point = match mode {
            8 => [-0., 0.5, 0.5],
            _ => [0.5, 0.5, 0.],
        };
        let mut r = request(point);
        if mode == 8 {
            r.plane = PlaneContract::AxisAlignedDyadic { normal_axis: 0 };
        }
        if mode == 9 {
            r.point = [
                f64::from_bits(0x3690000000000000),
                f64::from_bits(0x3690000000000000),
                0.,
            ];
        }
        if mode == 10 {
            r.point = [
                f64::from_bits(0x4620000000000000),
                f64::from_bits(0x4620000000000000),
                0.,
            ];
        }
        if mode == 11 {
            r.point = [-1., -1., 0.];
        }
        let b = q.inspect(&[r], EndpointLimits::default()).unwrap();
        let v = q.observations(&b).unwrap();
        assert!(
            matches!(v.observations()[0].classification, Classification::Inside),
            "mode{mode}"
        );
        if mode == 9 {
            assert_eq!(v.observations()[0].vertex_words[1][0], 2);
            assert_eq!(v.observations()[0].point_words[0], 0x3690000000000000);
        }
        if mode == 10 {
            assert_eq!(v.observations()[0].vertex_words[1][0], 0x72000000);
        }
        if mode == 1 {
            let j = value(&q, &[request([1., 1., 0.]), request([0., 0., 0.])]);
            assert_eq!(
                j["observations"][0]["classification"],
                serde_json::json!({"kind":"on_edge","edge":0})
            );
            assert_eq!(
                j["observations"][1]["classification"],
                serde_json::json!({"kind":"on_vertex","vertex":2})
            );
        }
    }
}
#[test]
fn uncertain_degenerate_and_malformed_domains_never_certify_containment() {
    for (mode, reason) in [(2, "degenerate"), (3, "axis plane"), (7, "source dyadic")] {
        let f = Fixture::new(mode);
        let mut store = f.store();
        let q = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
        let b = q
            .inspect(&[request([0.5, 0.5, 0.])], EndpointLimits::default())
            .unwrap();
        let view = q.observations(&b).unwrap();
        assert!(!view.observations()[0].classification.contained());
        assert!(
            matches!(&view.observations()[0].classification,Classification::Unsupported{reason:r} if r.contains(reason))
        );
    }
    let f = Fixture::new(0);
    let mut store = f.store();
    let q = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
    let j = value(&q, &[request([1e-100, 0.5, 0.])]);
    assert!(
        j["observations"][0]["classification"]["reason"]
            .as_str()
            .unwrap()
            .contains("endpoint dyadic")
    );
    for mode in [4, 5, 6, 14, 15] {
        let f = Fixture::new(mode);
        let mut s = f.store();
        assert!(
            EndpointQuery::load(&mut s, &form(0x10), InputLimits::default()).is_err(),
            "mode{mode}"
        );
    }
    for point in [[f64::NAN, 0., 0.], [0., f64::INFINITY, 0.]] {
        assert!(matches!(
            q.inspect(
                &[request([0.5, 0.5, 0.]), request(point)],
                EndpointLimits::default()
            ),
            Err(EndpointError::Invalid(_))
        ));
    }
}
#[test]
fn the_whole_batch_and_preflight_final_source_reads_share_exact_work_and_retention_limits() {
    let f = Fixture::new(0);
    let mut s = f.store();
    let q = EndpointQuery::load(&mut s, &form(0x10), InputLimits::default()).unwrap();
    let requests = [request([0.5, 0.5, 0.]), request([1., 1., 0.])];
    let b = q.inspect(&requests, EndpointLimits::default()).unwrap();
    let v = q.observations(&b).unwrap();
    let u = v.usage();
    let exact = EndpointLimits {
        requests: u.requests,
        source_visits: u.source_visits,
        predicate_tests: u.predicate_tests,
        geometry_bytes: u.geometry_bytes,
        identity_bytes: u.identity_bytes,
    };
    assert!(q.inspect(&requests, exact).is_ok());
    for under in [
        EndpointLimits {
            requests: exact.requests - 1,
            ..exact
        },
        EndpointLimits {
            source_visits: exact.source_visits - 1,
            ..exact
        },
        EndpointLimits {
            predicate_tests: exact.predicate_tests - 1,
            ..exact
        },
        EndpointLimits {
            geometry_bytes: exact.geometry_bytes - 1,
            ..exact
        },
        EndpointLimits {
            identity_bytes: exact.identity_bytes - 1,
            ..exact
        },
    ] {
        assert!(matches!(
            q.inspect(&requests, under),
            Err(EndpointError::Budget(_))
        ));
    }
    let source = q.source_usage();
    let exactsource = InputLimits {
        source_bytes: source.source_bytes,
        index_visits: source.index_visits,
        meshes: source.meshes,
        records: fallout_data::navigation::Limits {
            record_bytes: source.record_bytes,
            fields: source.fields,
            elements: source.elements,
        },
        retained_bytes: source.retained_reservation_bytes,
        ..InputLimits::default()
    };
    let mut s2 = f.store();
    assert!(EndpointQuery::load(&mut s2, &form(0x10), exactsource).is_ok());
    for which in 0..7 {
        let mut limits = exactsource;
        match which {
            0 => limits.source_bytes -= 1,
            1 => limits.index_visits -= 1,
            2 => limits.meshes -= 1,
            3 => limits.records.record_bytes -= 1,
            4 => limits.records.fields -= 1,
            5 => limits.records.elements -= 1,
            6 => limits.retained_bytes -= 1,
            _ => unreachable!(),
        };
        let mut s2 = f.store();
        assert!(
            EndpointQuery::load(&mut s2, &form(0x10), limits).is_err(),
            "sourcebound{which}"
        );
    }
    let mut bad = requests.clone();
    bad[1].triangle.triangle = 99;
    assert!(q.inspect(&bad, EndpointLimits::default()).is_err());
    assert_eq!(q.observations(&b).unwrap().observations().len(), 2);
    assert!(q.inspect(&[], EndpointLimits::default()).is_err());
    let mut bad = requests.clone();
    bad[1].plane = PlaneContract::AxisAlignedDyadic { normal_axis: 3 };
    assert!(q.inspect(&bad, EndpointLimits::default()).is_err());
}
#[test]
fn store_borrow_owner_generation_release_drop_and_changed_cohort_reject_stale_batches() {
    let f = Fixture::new(0);
    let path = f.root.path().join("Data/FalloutNV.esm");
    let before = fs::read(&path).unwrap();
    let mut store = f.store();
    let batch;
    {
        let mut q = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
        batch = q
            .inspect(&[request([0.5, 0.5, 0.])], EndpointLimits::default())
            .unwrap();
        #[cfg(windows)]
        {
            assert!(fs::write(&path, &before).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
        }
        assert!(q.observations(&batch).is_ok());
        q.release();
        assert!(matches!(q.observations(&batch), Err(EndpointError::Stale)));
        assert!(matches!(
            q.inspect(&[request([0.5, 0.5, 0.])], EndpointLimits::default()),
            Err(EndpointError::Stale)
        ));
    }
    {
        let q = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
        assert!(matches!(q.observations(&batch), Err(EndpointError::Stale)));
    }
    let oldbatch;
    let oldscope;
    {
        let q = EndpointQuery::load(&mut store, &form(0x10), InputLimits::default()).unwrap();
        oldbatch = q
            .inspect(&[request([0.5, 0.5, 0.])], EndpointLimits::default())
            .unwrap();
        oldscope = serde_json::to_value(q.observations(&oldbatch).unwrap()).unwrap();
    }
    drop(store);
    let mut changed = before;
    *changed.last_mut().unwrap() ^= 1;
    fs::write(&path, &changed).unwrap();
    let mut s = f.store();
    let q = EndpointQuery::load(&mut s, &form(0x10), InputLimits::default()).unwrap();
    assert!(matches!(
        q.observations(&oldbatch),
        Err(EndpointError::Stale)
    ));
    let new = value(&q, &[request([0.5, 0.5, 0.])]);
    assert_ne!(oldscope["scope_sha256"], new["scope_sha256"]);
    assert_ne!(
        oldscope["observations"][0]["source_sha256"],
        new["observations"][0]["source_sha256"]
    );
    let other = Fixture::new(13);
    let mut s = other.store();
    let q = EndpointQuery::load(&mut s, &form(0x10), InputLimits::default()).unwrap();
    assert!(matches!(
        q.observations(&oldbatch),
        Err(EndpointError::Stale)
    ));
    assert_ne!(
        oldscope["scope_sha256"],
        value(&q, &[request([0.5, 0.5, 0.])])["scope_sha256"]
    );
}
#[test]
fn physical_master_override_and_request_permutations_preserve_source_and_point_identity() {
    let f = Fixture::new(12);
    let mut s = f.store();
    let q = EndpointQuery::load(&mut s, &form(0x10), InputLimits::default()).unwrap();
    let requests = [request([0., 0., -0.]), request([0.5, 0.5, 0.])];
    let forward = value(&q, &requests);
    let reverse = value(&q, &[requests[1].clone(), requests[0].clone()]);
    assert_eq!(forward["sources"].as_array().unwrap().len(), 2);
    assert_eq!(forward["scope_sha256"], reverse["scope_sha256"]);
    assert_eq!(forward["observations"][0], reverse["observations"][1]);
    assert_eq!(forward["observations"][1], reverse["observations"][0]);
    assert_eq!(forward["observations"][0]["source_plugin"], "Patch.esp");
    assert_eq!(
        forward["observations"][0]["triangle"]["mesh"],
        serde_json::to_value(form(0x100)).unwrap()
    );
    assert_ne!(
        forward["sources"][0]["source_sha256"],
        forward["observations"][0]["source_sha256"]
    );
}
#[test]
#[ignore = "explicit private endpoint CLI fixture export"]
fn endpoint_cli_fixture_export() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_ENDPOINT_FIXTURE").expect("explicit directory"),
    );
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    for mode in 0..16 {
        let f = Fixture::new(mode);
        let install = root.join(format!("install-{mode}"));
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        for name in &f.order {
            fs::copy(
                f.root.path().join("Data").join(name),
                install.join("Data").join(name),
            )
            .unwrap();
        }
        fs::write(
            root.join(format!("order-{mode}.json")),
            serde_json::to_vec_pretty(&f.order).unwrap(),
        )
        .unwrap();
    }
    let points = [
        [0.5, 0.5, 0.],
        [1., 1., 0.],
        [0., 0., -0.],
        [1., f64::from_bits(0x3ff0000000000001), 0.],
        [0.5, 0.5, f64::from_bits(1)],
    ];
    fs::write(root.join("request.json"),serde_json::to_vec_pretty(&serde_json::json!({"cell":form(0x10),"points":points.into_iter().map(request).collect::<Vec<_>>()})).unwrap()).unwrap();
}
