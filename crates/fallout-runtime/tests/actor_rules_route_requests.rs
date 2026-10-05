mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, package_dependencies, packages},
    condition_operands::Signatures,
    inventory,
    loaded_scripts::Catalogue,
    navigation,
    store::RecordStore,
    world::Transform,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::{
        package_lifecycle,
        packages::{Operation, Requests},
        route_requests::{
            self, Decision, DoorPolicy, Endpoint, Error, ExternalDecision, Limits, Outcome, Policy,
            Query,
        },
    },
    foreign::Content,
    identity::CampaignId,
    navigation::{Route, TriangleId},
    reference_state::{Pose, State},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn disk(kind: &[u8; 4], id: u32, raw: &[u8]) -> Vec<u8> {
    let mut bytes = record(kind, id, 0, raw);
    bytes[20..22].copy_from_slice(&15u16.to_le_bytes());
    bytes
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn operand(kind: &[u8; 4], discriminant: u32, target: u32) -> Vec<u8> {
    field(kind, &words(&[discriminant, target, (-100i32) as u32]))
}
fn package(package_type: u8, extra: &[u8]) -> Vec<u8> {
    [
        field(b"PKDT", &[0, 0, 0, 0, package_type, 0, 0, 0]),
        field(b"PSDT", &[255, 255, 255, 255, 0, 0, 0, 0]),
        extra.to_vec(),
    ]
    .concat()
}
fn nav_source(cell: u32, secondary: bool) -> (Vec<u8>, Value) {
    let vertices: Vec<[f32; 3]> = if secondary {
        vec![[10., 0., -0.], [11., 0., 0.], [10., 1., 0.]]
    } else {
        vec![
            [0., 0., -0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [1., 1., 0.],
            [2., 0., 0.],
            [2., 1., 0.],
            [10., 0., 0.],
            [11., 0., 0.],
            [10., 1., 0.],
        ]
    };
    let triangles: Vec<([u16; 3], [i16; 3], u16, u16)> = if secondary {
        vec![([0, 1, 2], [0, 1, -1], 3, 0x8811)]
    } else {
        vec![
            ([0, 1, 2], [0, 1, -1], 1, 0xABCD),
            ([1, 3, 2], [2, -1, 0], 0, 0x1234),
            ([1, 4, 3], [-1, 3, 1], 0, 0xFFFF),
            ([4, 5, 3], [-1, -1, 2], 0, 0x80),
            ([6, 7, 8], [-1; 3], 0, 0),
        ]
    };
    let links: Vec<(u32, u32, u16)> = if secondary {
        vec![(7, 0x500, 0), (2, 0x999, 7)]
    } else {
        vec![(7, 0x501, 0)]
    };
    let doors: Vec<(u32, u16, [u8; 2])> = if secondary {
        vec![]
    } else {
        vec![(0x600, 1, [0x81, 0x82])]
    };
    let raw_vertices: Vec<_> = vertices
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut raw_triangles = Vec::new();
    for (vertices, edges, flags, cover) in &triangles {
        raw_triangles.extend(vertices.iter().flat_map(|v| v.to_le_bytes()));
        raw_triangles.extend(edges.iter().flat_map(|v| v.to_le_bytes()));
        raw_triangles.extend(flags.to_le_bytes());
        raw_triangles.extend(cover.to_le_bytes());
    }
    let mut raw_links = Vec::new();
    for (kind, mesh, triangle) in &links {
        raw_links.extend(kind.to_le_bytes());
        raw_links.extend(mesh.to_le_bytes());
        raw_links.extend(triangle.to_le_bytes());
    }
    let mut raw_doors = Vec::new();
    for (door, triangle, unused) in &doors {
        raw_doors.extend(door.to_le_bytes());
        raw_doors.extend(triangle.to_le_bytes());
        raw_doors.extend(unused);
    }
    let input_fields = [
        (*b"NVER", words(&[11])),
        (
            *b"DATA",
            words(&[
                cell,
                vertices.len() as u32,
                triangles.len() as u32,
                links.len() as u32,
                1,
                doors.len() as u32,
            ]),
        ),
        (*b"NVVX", raw_vertices),
        (*b"NVTR", raw_triangles),
        (*b"NVEX", raw_links),
        (*b"NVCA", 0u16.to_le_bytes().to_vec()),
        (*b"NVDP", raw_doors),
        (*b"NVGD", vec![9, 8, 7, 6, 5]),
        (*b"ZZZZ", vec![1, 2]),
    ];
    let mut raw = Vec::new();
    let mut fields = Vec::new();
    for (kind, bytes) in input_fields {
        fields.push(json!({"kind":kind,"decoded_offset":raw.len(),"bytes":bytes}));
        raw.extend(field(&kind, &bytes));
    }
    let expected = json!({"decoded_bytes":raw.len(),"decoded_sha256":format!("{:x}",Sha256::digest(&raw)),"version":{"decoded_offset":0,"value":11},"cell_raw":{"decoded_offset":10,"value":cell},"vertices":vertices,"triangles":triangles.iter().map(|(vertices,edges,flags,cover)|json!({"vertices":vertices,"edges":edges,"flags":flags,"cover_flags":cover})).collect::<Vec<_>>(),"edge_links":links.iter().map(|(kind,mesh,triangle)|json!({"link_type":kind,"navmesh_raw":mesh,"triangle":triangle})).collect::<Vec<_>>(),"cover_triangles":[0],"door_links":doors.iter().map(|(door,triangle,unused)|json!({"door_raw":door,"triangle":triangle,"unused":unused})).collect::<Vec<_>>(),"fields":fields});
    (raw, expected)
}
fn fixture(path: &Path, marker: u8) -> Value {
    fs::create_dir_all(path.join("Data")).unwrap();
    let mut raw = [
        header(&[]),
        disk(b"CELL", 0x400, &field(b"DATA", &[1])),
        disk(b"CELL", 0x401, &field(b"DATA", &[1])),
        disk(
            b"CREA",
            0x200,
            &[
                field(b"ACBS", &[0; 24]),
                field(b"DATA", &[marker; 17]),
                field(b"PKID", &0x100u32.to_le_bytes()),
                field(b"PKID", &0x104u32.to_le_bytes()),
                field(b"PKID", &0x101u32.to_le_bytes()),
            ]
            .concat(),
        ),
        disk(
            b"REFR",
            0x600,
            &[
                field(b"NAME", &0x200u32.to_le_bytes()),
                field(b"DATA", &[0; 24]),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut package_offsets = Vec::new();
    for (id, package_type, extra) in [
        (
            0x100,
            6,
            [
                operand(b"PLDT", 1, 0x400),
                field(b"POBA", &[]),
                unit(&[], &[]),
                field(b"POCA", &[]),
                field(b"POEA", &[]),
            ]
            .concat(),
        ),
        (0x101, 6, operand(b"PTDT", 0, 0x600)),
        (
            0x102,
            6,
            [operand(b"PLDT", 1, 0x400), operand(b"PLDT", 1, 0x401)].concat(),
        ),
        (0x103, 6, operand(b"PLDT", 5, 28)),
        (0x104, 5, operand(b"PLDT", 1, 0x400)),
    ] {
        package_offsets.push(json!({"id":id,"offset":raw.len()}));
        raw.extend(disk(b"PACK", id, &package(package_type, &extra)));
    }
    let mut meshes = Vec::new();
    for (cell, id, secondary) in [(0x400, 0x500, false), (0x401, 0x501, true)] {
        let (body, mut mesh) = nav_source(cell, secondary);
        let at = raw.len() + 24;
        mesh["header"] = json!({"kind":b"NAVM","offset":at,"stored_size":body.len(),"flags":0,"form_id":id,"revision":[0,0,0,0],"version":15,"trailing_bytes":[0,0]});
        let source = json!({"key":form(id),"cell":form(cell),"source_plugin":"FalloutNV.esm","mesh":mesh,"external_targets":if secondary {vec![Some(form(0x500)),Some(form(0x999))]}else{vec![Some(form(0x501))]},"door_targets":if secondary {vec![]}else{vec![Some(form(0x600))]}});
        let wire = disk(b"NAVM", id, &body);
        let mut group = b"GRUP".to_vec();
        group.extend(((wire.len() + 24) as u32).to_le_bytes());
        group.extend(cell.to_le_bytes());
        group.extend(6u32.to_le_bytes());
        group.extend([0; 8]);
        group.extend(wire);
        raw.extend(group);
        meshes.push(source);
    }
    let hash = format!("{:x}", Sha256::digest(&raw));
    for mesh in &mut meshes {
        mesh["source_sha256"] = json!(hash);
    }
    fs::write(path.join("Data/FalloutNV.esm"), raw).unwrap();
    fs::write(path.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    json!({"source_sha256":hash,"package_offsets":package_offsets,"meshes":meshes})
}
fn with_sources(
    path: &Path,
    callback: impl FnOnce(&Catalogue, &Content, &mut RecordStore, &packages::Catalogue),
) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    let packages = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    callback(&scripts, &content, &mut store, &packages);
}
fn world(scripts: &Catalogue) -> World<'_> {
    World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x28; 16]).unwrap(),
    )
    .unwrap()
}
fn node(triangle: usize) -> TriangleId {
    TriangleId {
        mesh: form(0x500),
        triangle,
    }
}
fn chosen(spec: &Value, package: usize) -> Query {
    Query {
        package: form(spec["package_offsets"][package]["id"].as_u64().unwrap() as u32),
        source_sha256: spec["source_sha256"].as_str().unwrap().into(),
        record_file_offset: spec["package_offsets"][package]["offset"].as_u64().unwrap(),
        field_index: 2,
        field_decoded_offset: 28,
        destination: form(if package == 1 { 0x600 } else { 0x400 }),
        cells: vec![form(0x400), form(0x401)],
        start: Endpoint {
            cell: form(0x400),
            node: node(0),
        },
        goal: Endpoint {
            cell: form(0x400),
            node: node(3),
        },
        policy: Policy {
            local: Decision::Admit {
                cost_bits: 0.5f64.to_bits(),
            },
            external_fallback: Decision::Unavailable {
                reason: "caller_special_link_unverified".into(),
            },
            external: vec![ExternalDecision {
                link_type: 7,
                decision: Decision::Reject {},
            }],
            doors: DoorPolicy::Pass {},
        },
    }
}
fn cases(spec: &Value) -> Vec<(&'static str, Query)> {
    let found = chosen(spec, 0);
    let mut unreachable = found.clone();
    unreachable.goal.node = node(4);
    let mut reject = found.clone();
    reject.policy.doors = DoorPolicy::Reject {};
    let mut door = found.clone();
    door.policy.doors = DoorPolicy::Unavailable {
        reason: "caller_door_unverified".into(),
    };
    let mut missing = unreachable.clone();
    missing.policy.external[0].decision = Decision::Admit {
        cost_bits: 1f64.to_bits(),
    };
    missing.policy.external_fallback = Decision::Admit {
        cost_bits: 0.25f64.to_bits(),
    };
    let mut special = missing.clone();
    special.policy.external_fallback = Decision::Unavailable {
        reason: "caller_special_link_unverified".into(),
    };
    let mut zero = found.clone();
    zero.policy.local = Decision::Admit { cost_bits: 0 };
    zero.policy.external[0].decision = Decision::Admit { cost_bits: 0 };
    vec![
        ("found", found),
        ("unreachable", unreachable),
        ("door-reject", reject),
        ("door-unavailable", door),
        ("missing-neighbors", missing),
        ("special-unavailable", special),
        ("reference-mapping", chosen(spec, 1)),
        ("source-unavailable", chosen(spec, 2)),
        ("object-type", chosen(spec, 3)),
        ("zero-cost-cycle", zero),
    ]
}

#[test]
fn travel_candidate_binds_one_pkid_route_and_physical_event_sources_only() {
    let temp = tempfile::tempdir().unwrap();
    let spec = fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, store, packages| {
        let inventory = inventory::Catalogue::load(store, Default::default()).unwrap();
        let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
        let associations =
            associations::Catalogue::load(store, &actors, Default::default()).unwrap();
        let signatures = Signatures::default();
        let dependencies = package_dependencies::Catalogue::load(
            store,
            packages,
            scripts,
            &signatures,
            Default::default(),
        )
        .unwrap();
        let world = world(scripts);
        let before = world.snapshot();
        let requests = Requests::prepare(
            &world,
            &actors,
            &associations,
            &dependencies,
            &form(0x200),
            Default::default(),
        )
        .unwrap();
        let capability = requests
            .capability(
                &world,
                content,
                None,
                Operation::Scheduling,
                Default::default(),
            )
            .unwrap();

        let query = chosen(&spec, 0);
        let candidate = package_lifecycle::observe(
            &world,
            content,
            store,
            packages,
            package_lifecycle::Request {
                capability: &capability,
                occurrence_index: 0,
                query: &query,
            },
            Default::default(),
        )
        .unwrap();
        assert_eq!(candidate.occurrence.occurrence_index, 0);
        assert_eq!(candidate.occurrence.actor_field_index, 2);
        assert_eq!(candidate.package.key, form(0x100));
        assert_eq!(candidate.destination_cell, form(0x400));
        assert_eq!(candidate.event_declarations.len(), 3);
        assert_eq!(
            candidate
                .event_declarations
                .iter()
                .map(|event| event.marker.kind)
                .collect::<Vec<_>>(),
            vec![*b"POBA", *b"POCA", *b"POEA"]
        );
        assert!(
            candidate
                .event_declarations
                .windows(2)
                .all(|events| events[0].marker.field_index < events[1].marker.field_index)
        );
        let script = &candidate.event_declarations[0].scripts[0];
        assert_eq!(script.physical_marker.as_ref().unwrap().kind, *b"POBA");
        assert_eq!(script.source_sha256, candidate.package.source_sha256);
        assert_eq!(
            script.record_file_offset,
            candidate.package.record_file_offset
        );
        assert!(script.compiled_source.as_ref().unwrap().sha256.len() == 64);
        assert_eq!(candidate.event_declarations[1].scripts.len(), 0);
        assert_eq!(candidate.event_declarations[2].scripts.len(), 0);
        assert!(!candidate.lifecycle_dispatch_order_verified);
        assert!(!candidate.schedule_supported);
        assert!(!candidate.script_execution_supported);
        assert!(!candidate.movement_supported);
        assert!(!candidate.execution_supported);
        assert!(!candidate.state_changed);
        assert!(candidate.require_execution().is_err());
        assert!(matches!(candidate.route.route(), Some(Route::Found { .. })));

        let mut unsupported = query.clone();
        unsupported.package = form(0x104);
        unsupported.record_file_offset = spec["package_offsets"][4]["offset"].as_u64().unwrap();
        assert!(matches!(
            package_lifecycle::observe(
                &world,
                content,
                store,
                packages,
                package_lifecycle::Request {
                    capability: &capability,
                    occurrence_index: 1,
                    query: &unsupported,
                },
                Default::default(),
            ),
            Err(package_lifecycle::Error::UnsupportedPackageType(5))
        ));

        let reference_destination = chosen(&spec, 1);
        assert!(matches!(
            package_lifecycle::observe(
                &world,
                content,
                store,
                packages,
                package_lifecycle::Request {
                    capability: &capability,
                    occurrence_index: 2,
                    query: &reference_destination,
                },
                Default::default(),
            ),
            Err(package_lifecycle::Error::DestinationUnavailable)
        ));
        assert_eq!(world.snapshot(), before);
    });
}

#[test]
fn literal_source_corridor_cycles_raw_mesh_words_and_caller_policy_join_without_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let spec = fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, store, packages| {
        let world = world(scripts);
        let before = world.snapshot();
        let query = chosen(&spec, 0);
        let observed = route_requests::observe(
            &world,
            content,
            store,
            packages,
            &query,
            None,
            Limits::default(),
        )
        .unwrap();
        let Route::Found { nodes, links, cost } = observed.route().unwrap() else {
            panic!("no route")
        };
        assert_eq!(nodes, &vec![node(0), node(1), node(2), node(3)]);
        assert_eq!(*cost, 1.5);
        assert_eq!(links[0].portal, [[1., 0., 0.], [0., 1., 0.]]);
        assert_eq!(
            serde_json::to_value(observed.meshes()).unwrap(),
            spec["meshes"]
        );
        assert_eq!(
            observed.meshes()[0].mesh.vertices[0][2].to_bits(),
            0x8000_0000
        );
        assert_eq!(observed.meshes()[0].mesh.door_links[0].unused, [0x81, 0x82]);
        assert_eq!(observed.destination().operands[0].signed_scalar, Some(-100));
        assert!(observed.require_execution().is_err());
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn door_special_missing_unreachable_and_zero_cost_cycles_use_existing_graph_outcomes() {
    let temp = tempfile::tempdir().unwrap();
    let spec = fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, store, packages| {
        let world = world(scripts);
        let before = world.snapshot();
        for (name, query) in cases(&spec) {
            let output = route_requests::observe(
                &world,
                content,
                store,
                packages,
                &query,
                None,
                Limits::default(),
            )
            .unwrap();
            match name {
                "unreachable" | "door-reject" => {
                    assert!(matches!(output.route(), Some(Route::Unreachable)))
                }
                "door-unavailable" => {
                    let Some(Route::Unsupported {
                        links,
                        missing_neighbors,
                    }) = output.route()
                    else {
                        panic!("door")
                    };
                    assert_eq!(links.len(), 1);
                    assert_eq!(links[0].reason, "caller_door_unverified");
                    assert!(missing_neighbors.is_empty());
                }
                "missing-neighbors" => {
                    let Some(Route::MissingNeighbors { links }) = output.route() else {
                        panic!("missing")
                    };
                    assert_eq!(links.len(), 1);
                    assert_eq!(links[0].target.as_ref().unwrap().mesh, form(0x999));
                }
                "special-unavailable" => {
                    let Some(Route::Unsupported { links, .. }) = output.route() else {
                        panic!("special")
                    };
                    assert_eq!(links.len(), 1);
                    assert_eq!(links[0].reason, "caller_special_link_unverified");
                }
                "source-unavailable" | "object-type" => {
                    assert_eq!(output.outcome(), Outcome::DestinationUnavailable);
                    assert!(output.route().is_none());
                    assert!(output.meshes().is_empty());
                }
                "zero-cost-cycle" => {
                    let Some(Route::Found { nodes, cost, .. }) = output.route() else {
                        panic!("zero")
                    };
                    assert_eq!(nodes, &vec![node(0), node(1), node(2), node(3)]);
                    assert_eq!(*cost, 0.);
                }
                _ => assert!(matches!(output.route(),Some(Route::Found{cost,..})if *cost==1.5)),
            }
            assert!(output.require_execution().is_err());
            assert_eq!(world.snapshot(), before);
        }
    });
}
#[test]
fn wrong_physical_source_destination_endpoint_and_even_unused_cost_refuse_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let spec = fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, store, packages| {
        let world = world(scripts);
        let before = world.snapshot();
        let base = chosen(&spec, 0);
        for mutation in 0..13 {
            let mut query = base.clone();
            match mutation {
                0 => query.source_sha256 = "a".repeat(64),
                1 => query.record_file_offset += 1,
                2 => query.field_index = 1,
                3 => query.field_decoded_offset += 1,
                4 => query.destination = form(0x401),
                5 => query.goal.cell = form(0x401),
                6 => query.start.cell = form(0x401),
                7 => query.goal.node.triangle = 99,
                8 => query.cells.push(form(0x400)),
                9 => query.policy.external.push(query.policy.external[0].clone()),
                10 => {
                    query.policy.external_fallback = Decision::Admit {
                        cost_bits: f64::NAN.to_bits(),
                    }
                }
                11 => {
                    query.policy.external[0].decision = Decision::Admit {
                        cost_bits: (-1f64).to_bits(),
                    }
                }
                12 => query.package.origin_plugin = "FalloutNV.esm".into(),
                _ => unreachable!(),
            }
            assert!(
                route_requests::observe(
                    &world,
                    content,
                    store,
                    packages,
                    &query,
                    None,
                    Limits::default()
                )
                .is_err(),
                "{mutation}"
            );
            assert_eq!(world.snapshot(), before);
        }
    });
}
#[test]
fn private_caller_epoch_revision_campaign_and_absent_component_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let spec = fixture(temp.path(), 0);
    with_sources(temp.path(), |scripts, content, store, packages| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x600))).unwrap();
        let query = chosen(&spec, 0);
        let absent = world.reference_view(reference).unwrap();
        assert!(matches!(
            route_requests::observe(
                &world,
                content,
                store,
                packages,
                &query,
                Some(&absent),
                Limits::default()
            ),
            Err(Error::Invalid("caller component is unavailable"))
        ));
        let state = State::new(
            form(0x400),
            Pose::from_source(
                &Transform {
                    position: [8192.25, -0., -30.5],
                    rotation: [0.125, -0.75, 1.5],
                },
                Some(0.75),
            )
            .unwrap(),
            false,
        )
        .unwrap();
        let stage = world.stage_reference_state(&absent, state).unwrap();
        world.commit_reference_state(stage).unwrap();
        let view = world.reference_view(reference).unwrap();
        let saved = world.snapshot();
        assert!(
            route_requests::observe(
                &world,
                content,
                store,
                packages,
                &query,
                Some(&view),
                Limits::default()
            )
            .is_ok()
        );
        assert_eq!(world.snapshot(), saved);
        let cold = World::restore(scripts, saved.clone(), WorldLimits::default()).unwrap();
        assert!(matches!(
            route_requests::observe(
                &cold,
                content,
                store,
                packages,
                &query,
                Some(&view),
                Limits::default()
            ),
            Err(Error::State(fallout_runtime::Error::StaleHandle))
        ));
        let other = World::with_campaign(
            scripts,
            WorldLimits::default(),
            CampaignId::from_bytes([9; 16]).unwrap(),
        )
        .unwrap();
        assert!(
            route_requests::observe(
                &other,
                content,
                store,
                packages,
                &query,
                Some(&view),
                Limits::default()
            )
            .is_err()
        );
        world.register_reference(None).unwrap();
        let after = world.snapshot();
        assert!(
            route_requests::observe(
                &world,
                content,
                store,
                packages,
                &query,
                Some(&view),
                Limits::default()
            )
            .is_err()
        );
        assert_eq!(world.snapshot(), after);
    });
}
#[test]
fn other_source_body_with_identical_headers_cannot_enter_existing_canonical_cohort() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let spec = fixture(first.path(), 0);
    fixture(second.path(), 1);
    with_sources(first.path(), |scripts, _, _, _| {
        let world = world(scripts);
        let before = world.snapshot();
        let query = chosen(&spec, 0);
        with_sources(second.path(), |_, content, store, packages| {
            assert!(
                route_requests::observe(
                    &world,
                    content,
                    store,
                    packages,
                    &query,
                    None,
                    Limits::default()
                )
                .is_err()
            );
        });
        assert_eq!(world.snapshot(), before);
    });
}
#[test]
fn strict_nested_decisions_and_all_exact_projection_work_and_source_bounds() {
    let temp = tempfile::tempdir().unwrap();
    let spec = fixture(temp.path(), 0);
    let query = chosen(&spec, 0);
    let valid = serde_json::to_value(&query).unwrap();
    for pointer in [
        "",
        "/policy/local",
        "/policy/external_fallback",
        "/policy/external/0/decision",
        "/policy/doors",
        "/start/node",
    ] {
        let mut malformed = valid.clone();
        malformed.pointer_mut(pointer).unwrap()["unexpected"] = json!(1);
        assert!(
            serde_json::from_value::<Query>(malformed).is_err(),
            "{pointer}"
        );
    }
    with_sources(temp.path(), |scripts, content, store, packages| {
        let world = world(scripts);
        let before = world.snapshot();
        let output = route_requests::observe(
            &world,
            content,
            store,
            packages,
            &query,
            None,
            Limits::default(),
        )
        .unwrap();
        let value = serde_json::to_value(&output).unwrap();
        let output_bytes = serde_json::to_vec(&output).unwrap().len();
        let request_bytes = serde_json::to_vec(&query).unwrap().len();
        let visits = value["visits"].as_u64().unwrap() as usize;
        drop(output);
        for exact in [
            Limits {
                max_request_bytes: request_bytes,
                ..Limits::default()
            },
            Limits {
                max_projection_bytes: output_bytes,
                ..Limits::default()
            },
            Limits {
                max_visits: visits,
                ..Limits::default()
            },
        ] {
            assert!(
                route_requests::observe(&world, content, store, packages, &query, None, exact)
                    .is_ok()
            );
        }
        let graph = fallout_runtime::navigation::GraphLimits {
            nodes: 0,
            ..Default::default()
        };
        let route = fallout_runtime::navigation::RouteLimits {
            expansions: 0,
            ..Default::default()
        };
        for limited in [
            Limits {
                max_sources: 0,
                ..Limits::default()
            },
            Limits {
                max_cells: 1,
                ..Limits::default()
            },
            Limits {
                max_meshes: 1,
                ..Limits::default()
            },
            Limits {
                max_policy_entries: 0,
                ..Limits::default()
            },
            Limits {
                max_request_bytes: request_bytes - 1,
                ..Limits::default()
            },
            Limits {
                max_projection_bytes: output_bytes - 1,
                ..Limits::default()
            },
            Limits {
                max_visits: visits - 1,
                ..Limits::default()
            },
            Limits {
                navigation: navigation::Limits {
                    record_bytes: 1,
                    ..Default::default()
                },
                ..Limits::default()
            },
            Limits {
                navigation: navigation::Limits {
                    fields: 1,
                    ..Default::default()
                },
                ..Limits::default()
            },
            Limits {
                navigation: navigation::Limits {
                    elements: 1,
                    ..Default::default()
                },
                ..Limits::default()
            },
            Limits {
                graph,
                ..Limits::default()
            },
            Limits {
                route,
                ..Limits::default()
            },
        ] {
            assert!(
                route_requests::observe(&world, content, store, packages, &query, None, limited)
                    .is_err()
            );
            assert_eq!(world.snapshot(), before);
        }
    });
}
#[test]
fn export_authored_literal_nav_source_and_actual_join_when_requested() {
    let Some(root) = std::env::var_os("FALLOUT_ACTOR_ROUTE_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&root);
    fs::create_dir_all(root).unwrap();
    let path = root.join("fixture");
    let spec = fixture(&path, 0);
    fs::write(
        path.join("authored-inputs.json"),
        serde_json::to_vec(&spec).unwrap(),
    )
    .unwrap();
    with_sources(&path, |scripts, content, store, packages| {
        let mut world = world(scripts);
        let reference = world.register_reference(Some(form(0x600))).unwrap();
        let view = world.reference_view(reference).unwrap();
        let state = State::new(
            form(0x400),
            Pose::from_source(
                &Transform {
                    position: [8192.25, -0., -30.5],
                    rotation: [0.125, -0.75, 1.5],
                },
                Some(0.75),
            )
            .unwrap(),
            false,
        )
        .unwrap();
        let stage = world.stage_reference_state(&view, state).unwrap();
        world.commit_reference_state(stage).unwrap();
        let snapshot = world.snapshot();
        let view = world.reference_view(reference).unwrap();
        fs::write(
            path.join("snapshot.json"),
            serde_json::to_vec_pretty(&snapshot).unwrap(),
        )
        .unwrap();
        for (name, query) in cases(&spec) {
            fs::write(
                path.join(format!("{name}-query.json")),
                serde_json::to_vec_pretty(&query).unwrap(),
            )
            .unwrap();
            let output = route_requests::observe(
                &world,
                content,
                store,
                packages,
                &query,
                Some(&view),
                Limits::default(),
            )
            .unwrap();
            fs::write(
                root.join(format!("{name}-host.json")),
                serde_json::to_vec_pretty(&output).unwrap(),
            )
            .unwrap();
            assert_eq!(world.snapshot(), snapshot);
        }
    });
}
