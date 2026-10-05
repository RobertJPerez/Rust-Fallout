//! Literal NIF bytes, through the actual decoder and immutable source scene.
use fallout_data::{coordinates::Affine, nif_collision};
use fallout_runtime::{
    identity::ReferenceId,
    physics::{sweep::*, *},
};
use std::num::NonZeroU64;
fn words(bytes: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        bytes.extend(v.to_le_bytes());
    }
}
fn floats(bytes: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        bytes.extend(v.to_le_bytes());
    }
}
fn container(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut b, &[0x14020007]);
    b.push(1);
    words(&mut b, &[11, blocks.len() as u32, 34]);
    b.extend([0; 3]);
    b.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        words(&mut b, &[name.len() as u32]);
        b.extend(name.as_bytes());
    }
    for i in 0..blocks.len() {
        b.extend((i as u16).to_le_bytes());
    }
    for (_, data) in blocks {
        words(&mut b, &[data.len() as u32]);
    }
    words(&mut b, &[0, 0, 0]);
    for (_, data) in blocks {
        b.extend(data);
    }
    words(&mut b, &[0]);
    b
}
fn body(shape: u32) -> Vec<u8> {
    let mut b = vec![0; 236];
    b[..4].copy_from_slice(&shape.to_le_bytes());
    b[80..84].copy_from_slice(&0x3f800000u32.to_le_bytes());
    b
}
fn sphere(radius: f32) -> Vec<u8> {
    let mut b = Vec::new();
    words(&mut b, &[17]);
    floats(&mut b, &[radius]);
    b
}
fn bx() -> Vec<u8> {
    let mut b = sphere(0.25);
    b.extend([0; 8]);
    floats(&mut b, &[1., 2., 3., 0.]);
    b
}
fn transform(shape: u32, scale: f32, radius: f32) -> Vec<u8> {
    let mut b = Vec::new();
    words(&mut b, &[shape, 31]);
    floats(&mut b, &[radius]);
    b.extend([0; 8]);
    for row in [
        [scale, 0., 0., 0.],
        [0., scale, 0., 0.],
        [0., 0., scale, 0.],
        [4., 0., 0., 1.],
    ] {
        floats(&mut b, &row);
    }
    b
}
fn identity() -> Affine {
    Affine {
        rows: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
    }
}
fn units() -> EngineeringUnits {
    EngineeringUnits {
        havok_to_source: 1.,
        source_to_query: 1.,
        transform_tolerance: 1e-6,
    }
}
fn build(blocks: &[(&str, Vec<u8>)], frame: Affine) -> StaticScene {
    let (_, data) = nif_collision::decode(&container(blocks), "authored sweep").unwrap();
    StaticScene::build(
        &data,
        &[BodyPlacement {
            reference: ReferenceId(NonZeroU64::new(7).unwrap()),
            source_sha256: [9; 32],
            body_block: 0,
            attachment_to_source: frame,
        }],
        units(),
        QueryLimits::default(),
    )
    .unwrap()
}
fn scene() -> StaticScene {
    build(
        &[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))],
        identity(),
    )
}
fn request(start: [f64; 3], end: [f64; 3], radius: f64) -> SphereSweep {
    SphereSweep {
        start,
        end,
        radius,
        contact_tolerance: 1e-9,
        scope: SweepScope::ZeroTaggedFrozenSphereCores,
    }
}
fn sweep(scene: &StaticScene, r: SphereSweep) -> SweepProposal {
    scene.sweep_sphere(r, SweepLimits::default()).unwrap()
}
fn json(proposal: &SweepProposal) -> serde_json::Value {
    serde_json::to_value(proposal).unwrap()
}
#[test]
fn finite_radius_contact_preserves_full_source_and_original_segment() {
    let scene = scene();
    let p = sweep(&scene, request([-5., 0., 0.], [5., 0., 0.], 1.));
    assert_eq!(p.state(), SweepState::Contact);
    assert_eq!(p.parameter(), 0.3);
    assert_eq!(p.proposed_center(), [-2., 0., 0.]);
    let j = json(&p);
    let c = &j["contacts"][0];
    assert_eq!(c["provenance"]["source"]["reference"], 7);
    assert_eq!(c["provenance"]["source"]["body_block"], 0);
    assert_eq!(c["provenance"]["source"]["shape_block"], 1);
    assert_eq!(
        c["provenance"]["source"]["source_sha256"],
        serde_json::json!(vec![9; 32])
    );
    assert_eq!(c["provenance"]["material"], 17);
    assert_eq!(c["expanded_radius"], 2.);
    assert_eq!(
        c["provenance"]["body_filter"],
        serde_json::json!({"layer":0,"flags_and_parts":0,"group":0})
    );
    assert_eq!(c["provenance"]["shape_filter"], serde_json::Value::Null);
    assert_eq!(j["work"]["predicate_tests"], 1024);
    assert_eq!(j["work"]["iterations"], 1);
    assert!(!scene.faithful_ready());
}
#[test]
fn start_overlap_tangent_zero_motion_and_exact_closed_range() {
    let scene = scene();
    for (start, end, radius, state) in [
        ([0., 0., 0.], [0., 0., 0.], 1., SweepState::StartOverlap),
        ([-2., 0., 0.], [-2., 0., 0.], 1., SweepState::StartTangent),
        ([-3., 0., 0.], [-3., 0., 0.], 1., SweepState::Clear),
        ([-3., 0., 0.], [-2., 0., 0.], 1., SweepState::Contact),
        (
            [-3., 0., 0.],
            [-2f64.next_up(), 0., 0.],
            1.,
            SweepState::Clear,
        ),
        (
            [-3., 0., 0.],
            [-2f64.next_down(), 0., 0.],
            1.,
            SweepState::Contact,
        ),
        ([-3., 2., 0.], [3., 2., 0.], 1., SweepState::Contact),
        ([-3., 2.01, 0.], [3., 2.01, 0.], 1., SweepState::Clear),
        ([-3., 0., 0.], [-4., 0., 0.], 1., SweepState::Clear),
    ] {
        match scene.sweep_sphere(request(start, end, radius), SweepLimits::default()) {
            Ok(p) => assert_eq!(p.state(), state, "{start:?} {end:?}"),
            Err(_) if end[0] == -2f64.next_up() || end[0] == -2f64.next_down() => {}
            Err(error) => panic!("{start:?} {end:?}: {error}"),
        }
    }
    let p = sweep(&scene, request([-3., 0., 0.], [-2., 0., 0.], 1.));
    assert_eq!(p.parameter(), 1.);
    assert_eq!(p.proposed_center(), [-2., 0., 0.]);
}
#[test]
fn exact_skew_segment_uses_no_normalized_component_substitution() {
    // 3-4-5 line: length of start is10, expanded radius5, exact t=1/4.
    let p = sweep(&scene(), request([-6., -8., 0.], [6., 8., 0.], 4.));
    assert_eq!(p.parameter(), 0.25);
    assert_eq!(p.proposed_center(), [-3., -4., 0.]);
    let j = json(&p);
    assert_eq!(
        j["contacts"][0]["parameter_bounds"],
        serde_json::json!([0.25, 0.25])
    );
    assert_eq!(j["center_error_bounds"], serde_json::json!([0., 0., 0.]));
    // Fractional endpoints whose subtraction cannot retain the original segment
    // exactly are refused, even when a normalized approximate ray would hit.
    assert!(
        scene()
            .sweep_sphere(
                request([1e-20, 0., 0.], [4., 0., 0.], 1.),
                SweepLimits::default()
            )
            .is_err()
    );
}
#[test]
fn uncertain_numeric_domains_refuse_atomically() {
    let scene = scene();
    for mut r in [
        request([-1e16, 1.00001, 0.], [1e16, 1.00001, 0.], 0.),
        request([-5., 0., 0.], [5., 0., 0.], f64::from_bits(1)),
        request([-5., 0., 0.], [5., 0., 0.], 1e50),
        request([f64::NAN, 0., 0.], [1., 0., 0.], 0.),
        request([-3., 2f64.next_up(), 0.], [3., 2f64.next_up(), 0.], 1.),
    ] {
        r.contact_tolerance = 1e-20;
        assert!(scene.sweep_sphere(r, SweepLimits::default()).is_err());
    }
    let tiny = build(
        &[
            ("bhkRigidBody", body(1)),
            ("bhkSphereShape", sphere(f32::from_bits(1))),
        ],
        identity(),
    );
    assert!(
        tiny.sweep_sphere(
            request([-5., 0., 0.], [5., 0., 0.], 1.),
            SweepLimits::default()
        )
        .is_err()
    );
}
#[test]
fn exact_source_transforms_admitted_other_frames_margins_and_tags_refuse() {
    let scene = build(
        &[
            ("bhkRigidBody", body(1)),
            ("bhkTransformShape", transform(2, 2., 0.)),
            ("bhkSphereShape", sphere(1.)),
        ],
        identity(),
    );
    let p = sweep(&scene, request([-5., 0., 0.], [8., 0., 0.], 1.));
    assert!((p.proposed_center()[0] - 1.).abs() <= 1e-9);
    let mut reflected = identity();
    reflected.rows = [[0., -2., 0., 4.], [2., 0., 0., 0.], [0., 0., -2., 0.]];
    let p = sweep(
        &build(
            &[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))],
            reflected,
        ),
        request([-5., 0., 0.], [8., 0., 0.], 1.),
    );
    assert!((p.proposed_center()[0] - 1.).abs() <= 1e-9);
    for frame in [
        Affine {
            rows: [[1.1, 0., 0., 0.], [0., 1.1, 0., 0.], [0., 0., 1.1, 0.]],
        },
        Affine {
            rows: [[1., 0., 0., 0.], [0., 1. + 1e-7, 0., 0.], [0., 0., 1., 0.]],
        },
    ] {
        let scene = build(
            &[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))],
            frame,
        );
        assert!(
            scene
                .sweep_sphere(
                    request([-5., 0., 0.], [5., 0., 0.], 1.),
                    SweepLimits::default()
                )
                .is_err()
        );
    }
    let margin = build(
        &[
            ("bhkRigidBody", body(1)),
            ("bhkTransformShape", transform(2, 1., 0.25)),
            ("bhkSphereShape", sphere(1.)),
        ],
        identity(),
    );
    assert!(
        margin
            .sweep_sphere(
                request([-5., 0., 0.], [5., 0., 0.], 1.),
                SweepLimits::default()
            )
            .is_err()
    );
    for offset in [4, 36, 28, 44, 84, 212, 215, 232] {
        let mut b = body(1);
        b[offset] = 1;
        let scene = build(
            &[("bhkRigidBody", b), ("bhkSphereShape", sphere(1.))],
            identity(),
        );
        assert!(
            scene
                .sweep_sphere(
                    request([-5., 0., 0.], [5., 0., 0.], 1.),
                    SweepLimits::default()
                )
                .is_err(),
            "offset {offset}"
        );
    }
}
#[test]
fn full_scene_and_global_work_never_return_a_partial_clearance() {
    let mut list = Vec::new();
    words(&mut list, &[2, 2, 3]);
    words(&mut list, &[0; 7]);
    words(&mut list, &[2, 0, 0]);
    let scene = build(
        &[
            ("bhkRigidBody", body(1)),
            ("bhkListShape", list.clone()),
            ("bhkSphereShape", sphere(1.)),
            ("bhkBoxShape", bx()),
        ],
        identity(),
    );
    assert!(
        scene
            .sweep_sphere(
                request([-5., 0., 0.], [5., 0., 0.], 1.),
                SweepLimits::default()
            )
            .is_err()
    );
    let scene = build(
        &[
            ("bhkRigidBody", body(1)),
            ("bhkListShape", list),
            ("bhkSphereShape", sphere(1.)),
            ("bhkSphereShape", sphere(1.)),
        ],
        identity(),
    );
    let r = request([-5., 0., 0.], [5., 0., 0.], 1.);
    let p = sweep(&scene, r);
    assert_eq!(json(&p)["contacts"].as_array().unwrap().len(), 2);
    for limits in [
        SweepLimits {
            primitive_tests: 1,
            ..SweepLimits::default()
        },
        SweepLimits {
            predicate_tests: 2047,
            ..SweepLimits::default()
        },
        SweepLimits {
            iterations: 1,
            ..SweepLimits::default()
        },
        SweepLimits {
            contacts: 1,
            ..SweepLimits::default()
        },
        SweepLimits {
            primitive_tests: 10001,
            ..SweepLimits::default()
        },
    ] {
        assert!(scene.sweep_sphere(r, limits).is_err());
    }
    let tight = SweepLimits {
        primitive_tests: 2,
        predicate_tests: 2048,
        iterations: 2,
        contacts: 2,
    };
    assert_eq!(
        scene.sweep_sphere(r, tight).unwrap().state(),
        SweepState::Contact
    );
}
#[test]
#[ignore = "explicit fixture export into private evidence directory"]
fn sweep_cli_fixture_export() {
    let root = std::path::PathBuf::from(std::env::var_os("FALLOUT_SWEEP_FIXTURE").unwrap());
    std::fs::create_dir(&root).unwrap();
    let mut unknown = body(1);
    unknown[4] = 5;
    for (name, blocks) in [
        (
            "sphere.nif",
            vec![("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))],
        ),
        (
            "tiny.nif",
            vec![
                ("bhkRigidBody", body(1)),
                ("bhkSphereShape", sphere(f32::from_bits(1))),
            ],
        ),
        (
            "point.nif",
            vec![("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(0.))],
        ),
        (
            "unknown.nif",
            vec![("bhkRigidBody", unknown), ("bhkSphereShape", sphere(1.))],
        ),
        (
            "box.nif",
            vec![("bhkRigidBody", body(1)), ("bhkBoxShape", bx())],
        ),
        (
            "transformed.nif",
            vec![
                ("bhkRigidBody", body(1)),
                ("bhkTransformShape", transform(2, 2., 0.)),
                ("bhkSphereShape", sphere(1.)),
            ],
        ),
        (
            "margin.nif",
            vec![
                ("bhkRigidBody", body(1)),
                ("bhkTransformShape", transform(2, 2., 0.25)),
                ("bhkSphereShape", sphere(1.)),
            ],
        ),
    ] {
        std::fs::write(root.join(name), container(&blocks)).unwrap();
    }
}

#[test]
fn late_unsupported_obstacle_outside_index_range_still_refuses_the_sweep() {
    let mut list = Vec::new();
    words(&mut list, &[9]);
    words(&mut list, &(2..=10).collect::<Vec<_>>());
    words(&mut list, &[0; 7]);
    words(&mut list, &[9]);
    words(&mut list, &[0; 9]);
    let mut blocks = vec![("bhkRigidBody", body(1)), ("bhkListShape", list)];
    for _ in 0..8 {
        blocks.push(("bhkSphereShape", sphere(1.)));
    }
    let mut far = transform(11, 1., 0.);
    far[68..72].copy_from_slice(&10000f32.to_le_bytes());
    blocks.push(("bhkTransformShape", far));
    blocks.push(("bhkBoxShape", bx()));
    let scene = build(&blocks, identity());
    assert_eq!(scene.primitive_count(), 9);
    // Old rays reach the eight spheres and cull the ninth distant obstacle.
    assert_eq!(
        scene
            .ray_cast(
                Ray {
                    origin: [-5., 0., 0.],
                    direction: [1., 0., 0.],
                    max_distance: 10.
                },
                QueryBudget::default()
            )
            .unwrap()
            .len(),
        8
    );
    assert!(matches!(
        scene.sweep_sphere(
            request([-5., 0., 0.], [5., 0., 0.], 1.),
            SweepLimits::default()
        ),
        Err(QueryError::Unsupported { block: 11, .. })
    ));
    let source_scene = build(
        &[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))],
        identity(),
    );
    let p = sweep(&source_scene, request([-5., 0., 0.], [5., 0., 0.], 0.));
    assert!((p.proposed_center()[0] + 1.).abs() < 1e-9);
    let point = build(
        &[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(0.))],
        identity(),
    );
    let p = sweep(&point, request([-1., 0., 0.], [1., 0., 0.], 0.));
    assert_eq!(p.parameter(), 0.5);
    assert_eq!(p.proposed_center(), [0.; 3]);
}
