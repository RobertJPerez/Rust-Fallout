//! Authored binary fixtures reach the existing decoder, conversion and query path.
use fallout_data::{coordinates::Affine, nif_collision};
use fallout_runtime::{identity::ReferenceId, physics::*};
use std::num::NonZeroU64;

fn words(b: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        b.extend(v.to_le_bytes());
    }
}
fn floats(b: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        b.extend(v.to_le_bytes());
    }
}
fn container(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut b, &[0x1402_0007]);
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
    b[4..8].copy_from_slice(&[5, 0xe7, 0x34, 0x12]);
    b[80..84].copy_from_slice(&1f32.to_le_bytes());
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
fn capsule(a: [f32; 3], b: [f32; 3], r: f32) -> Vec<u8> {
    let mut data = sphere(r);
    data.extend([0; 8]);
    floats(&mut data, &a);
    floats(&mut data, &[r]);
    floats(&mut data, &b);
    floats(&mut data, &[r]);
    data
}
fn transform(shape: u32, columns: [[f32; 4]; 4]) -> Vec<u8> {
    let mut b = Vec::new();
    words(&mut b, &[shape, 0]);
    floats(&mut b, &[0.]);
    b.extend([0; 8]);
    for col in columns {
        floats(&mut b, &col);
    }
    b
}
fn convex_cuboid_fixture() -> Vec<u8> {
    let mut b = sphere(0.25);
    words(&mut b, &[0, 0, 0x8000_0000, 0, 0, 0x8000_0000, 8]);
    for x in [1., 3.] {
        for y in [2., 6.] {
            for z in [-1., 1.] {
                floats(&mut b, &[x, y, z, 0.]);
            }
        }
    }
    words(&mut b, &[6]);
    for plane in [
        [-1., 0., 0., 1.],
        [1., 0., 0., -3.],
        [0., -1., 0., 2.],
        [0., 1., 0., -6.],
        [0., 0., -1., -1.],
        [0., 0., 1., -1.],
    ] {
        floats(&mut b, &plane);
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
fn placement() -> BodyPlacement {
    BodyPlacement {
        reference: ReferenceId(NonZeroU64::new(1).unwrap()),
        source_sha256: [9; 32],
        body_block: 0,
        attachment_to_source: identity(),
    }
}
fn scene(blocks: &[(&str, Vec<u8>)]) -> StaticScene {
    let (_, collision) = nif_collision::decode(&container(blocks), "authored queries").unwrap();
    StaticScene::build(&collision, &[placement()], units(), QueryLimits::default()).unwrap()
}
fn ray(o: [f64; 3], d: [f64; 3], max: f64) -> Ray {
    Ray {
        origin: o,
        direction: d,
        max_distance: max,
    }
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

#[test]
fn distant_sphere_and_capsule_rays_keep_perpendicular_misses_and_refuse_lost_entries() {
    for (name, geometry) in [
        ("bhkSphereShape", sphere(1.)),
        ("bhkCapsuleShape", capsule([0., 0., -2.], [0., 0., 2.], 1.)),
    ] {
        let scene = scene(&[("bhkRigidBody", body(1)), (name, geometry)]);
        // Independently: the line's perpendicular distance is2, exceeding1.
        for far in [1e9, 1e20] {
            if name == "bhkCapsuleShape" && far == 1e20 {
                assert!(matches!(
                    scene.ray_cast(
                        ray([far, 2., 0.], [-1., 0., 0.], far),
                        QueryBudget::default()
                    ),
                    Err(QueryError::Invalid(
                        "capsule projection exceeds numerical precision"
                    ))
                ));
                continue;
            }
            assert!(
                scene
                    .ray_cast(
                        ray([far, 2., 0.], [-1., 0., 0.], far),
                        QueryBudget::default()
                    )
                    .unwrap()
                    .is_empty(),
                "{name} {far}"
            );
            let hits = scene
                .ray_cast(
                    ray([far, 1., 0.], [-1., 0., 0.], far),
                    QueryBudget::default(),
                )
                .unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].distance, far);
            assert_eq!(hits[0].position, [0., 1., 0.]);
        }
        assert!(
            scene
                .ray_cast(
                    ray([6e8 - 1.6, 8e8 + 1.2, 0.], [-0.6, -0.8, 0.], 1e9 + 10.),
                    QueryBudget::default()
                )
                .unwrap()
                .is_empty()
        );
        let hits = scene
            .ray_cast(
                ray([1e9, 0., 0.], [-1., 0., 0.], 1e9),
                QueryBudget::default(),
            )
            .unwrap();
        assert_eq!(hits[0].distance, 1e9 - 1.);
        assert_eq!(hits[0].position, [1., 0., 0.]);
        assert!(matches!(
            scene.ray_cast(
                ray([1e20, 0., 0.], [-1., 0., 0.], 1e20),
                QueryBudget::default()
            ),
            Err(QueryError::Invalid(_))
        ));
        assert_eq!(
            scene
                .ray_cast(ray([0.; 3], [1., 0., 0.], 0.), QueryBudget::default())
                .unwrap()[0]
                .distance,
            0.
        );
    }
}

#[test]
fn certified_authored_convex_cuboid_keeps_offset_hull_shell_and_reflection() {
    let (_, collision) = nif_collision::decode(
        &container(&[
            ("bhkRigidBody", body(1)),
            ("bhkConvexVerticesShape", convex_cuboid_fixture()),
        ]),
        "offset authored cuboid",
    )
    .unwrap();
    let scene =
        StaticScene::build(&collision, &[placement()], units(), QueryLimits::default()).unwrap();
    let hits = scene
        .ray_cast(ray([0., 4., 0.], [1., 0., 0.], 10.), QueryBudget::default())
        .unwrap();
    assert_eq!(hits.len(), 1);
    close(hits[0].distance, 1.);
    assert_eq!(hits[0].source.shape_block, 1);
    assert_eq!(hits[0].material, 17);
    assert_eq!(hits[0].authored_shell_radius, 0.25);
    assert!(matches!(
        StaticScene::build(
            &collision,
            &[placement()],
            units(),
            QueryLimits {
                geometry_elements: 14,
                ..QueryLimits::default()
            }
        ),
        Err(QueryError::Budget("geometry elements"))
    ));
    assert!(
        scene
            .ray_cast(ray([0., 7., 0.], [1., 0., 0.], 10.), QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    close(
        scene
            .ray_cast(
                ray([-2., 0., 0.], [0.6, 0.8, 0.], 10.),
                QueryBudget::default(),
            )
            .unwrap()[0]
            .distance,
        5.,
    );
    assert!(
        scene
            .overlap_sphere([0.9, 4., 0.], 0., QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scene
            .overlap_sphere([0., 1., 0.], 2., QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
    assert!(
        scene
            .overlap_sphere([0., 1., 0.], 1., QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    let mut reflected = placement();
    reflected.attachment_to_source.rows[0] = [-1., 0., 0., 10.];
    let reflected =
        StaticScene::build(&collision, &[reflected], units(), QueryLimits::default()).unwrap();
    close(
        reflected
            .ray_cast(
                ray([10., 4., 0.], [-1., 0., 0.], 10.),
                QueryBudget::default(),
            )
            .unwrap()[0]
            .distance,
        1.,
    );
    assert!(!reflected.faithful_ready());
}

#[test]
fn convex_bounds_never_replace_missing_corners_or_inconsistent_source_planes() {
    for (offset, value) in [(36, 2.0f32), (48, 1.), (180, 2.), (172, 1.)] {
        let mut shape = convex_cuboid_fixture();
        shape[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let (_, collision) = nif_collision::decode(
            &container(&[("bhkRigidBody", body(1)), ("bhkConvexVerticesShape", shape)]),
            "invalid convex certificate",
        )
        .unwrap();
        assert!(
            matches!(
                StaticScene::build(&collision, &[placement()], units(), QueryLimits::default()),
                Err(QueryError::Unsupported { block: 1, .. })
            ),
            "offset {offset}"
        );
    }
    for degenerate in [false, true] {
        let mut shape = convex_cuboid_fixture();
        if degenerate {
            for i in 0..8 {
                shape[36 + 16 * i + 8..36 + 16 * i + 12].copy_from_slice(&0f32.to_le_bytes());
            }
        } else {
            let first: [u8; 16] = shape[36..52].try_into().unwrap();
            shape[52..68].copy_from_slice(&first);
        }
        let (_, collision) = nif_collision::decode(
            &container(&[("bhkRigidBody", body(1)), ("bhkConvexVerticesShape", shape)]),
            "missing or degenerate corners",
        )
        .unwrap();
        assert!(matches!(
            StaticScene::build(&collision, &[placement()], units(), QueryLimits::default()),
            Err(QueryError::Unsupported { block: 1, .. })
        ));
    }
    let mut nonfinite = convex_cuboid_fixture();
    nonfinite[36..40].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(
        nif_collision::decode(
            &container(&[
                ("bhkRigidBody", body(1)),
                ("bhkConvexVerticesShape", nonfinite)
            ]),
            "nonfinite corners"
        )
        .is_err()
    );
}

#[test]
fn original_skew_direction_preserves_exact_rational_perpendicular_miss() {
    let scene = scene(&[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))]);
    // Exact rational calculation over these binary64 inputs gives perpendicular
    // distance1.0185605679677912. Rescaling direction components must not alter it.
    let origin = [111343779979437.78, -652654951280784.6, -910656179607209.4];
    let direction = [-0.09889314254557549, 0.5796740432380284, 0.8088251664936846];
    assert!(
        scene
            .ray_cast(
                ray(origin, direction, 1125899906842626.),
                QueryBudget::default()
            )
            .unwrap()
            .is_empty()
    );
    // Skew grazing predicates without a decisive error interval explicitly refuse.
    let tiny = std::f64::consts::FRAC_1_SQRT_2;
    assert!(matches!(
        scene.ray_cast(
            ray([3., -3., 1.], [-tiny, tiny, 0.], 10.),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "sphere grazing predicate is numerically uncertain"
        ))
    ));
    // Exact rational .6^2+.8^2 exceeds1, although hypot rounds to1.
    assert!(matches!(
        scene.ray_cast(
            ray([0.6, 0.8, 0.], [0.6, 0.8, 0.], 0.),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "sphere containment predicate is numerically uncertain"
        ))
    ));
}

#[test]
fn near_grazing_literal_ray_matches_independent_rational_inputs() {
    let scene = scene(&[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))]);
    let result = scene
        .ray_cast(
            ray(
                [-229.11445911534562, 610.4858273951352, -758.1652980544283],
                [0.23005015640173943, -0.6101330235002654, 0.7581652980544282],
                2000.,
            ),
            QueryBudget::default(),
        )
        .unwrap();
    // Python Fraction over these exact binary64 words: perpendicular^2 <1.
    assert_eq!(result.len(), 1);
}

#[test]
fn capsule_initial_inside_predicate_refuses_rounded_boundary_false_hit() {
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkCapsuleShape", capsule([0., 0., -2.], [0., 0., 2.], 1.)),
    ]);
    // Exact binary64 .6^2+.8^2 >1; the outward zero-distance request cannot hit.
    assert!(matches!(
        scene.ray_cast(
            ray([0.6, 0.8, 0.], [0.6, 0.8, 0.], 0.),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "capsule containment predicate is numerically uncertain"
        ))
    ));
    assert_eq!(
        scene
            .ray_cast(ray([1., 0., 0.], [1., 0., 0.], 0.), QueryBudget::default())
            .unwrap()[0]
            .distance,
        0.
    );
    assert_eq!(
        scene
            .ray_cast(ray([0.; 3], [1., 0., 0.], 0.), QueryBudget::default())
            .unwrap()[0]
            .distance,
        0.
    );
}

#[test]
fn capsule_endpoint_subtraction_cannot_round_an_outside_origin_onto_surface() {
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkCapsuleShape", capsule([1., 0., 0.], [2., 0., 0.], 1.)),
    ]);
    // Exact distance to endpoint 1 is 1 + 2^-1074, despite rounded subtraction.
    assert!(matches!(
        scene.ray_cast(
            ray([-f64::from_bits(1), 0., 0.], [-1., 0., 0.], 0.),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "capsule containment predicate is numerically uncertain"
        ))
    ));
}

#[test]
fn source_sphere_ray_units_pose_range_and_filter_bits() {
    let (_, collision) = nif_collision::decode(
        &container(&[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(2.))]),
        "sphere",
    )
    .unwrap();
    let mut place = placement();
    place.attachment_to_source.rows[0][3] = 10.;
    let scene = StaticScene::build(
        &collision,
        &[place],
        EngineeringUnits {
            havok_to_source: 3.,
            source_to_query: 2.,
            ..units()
        },
        QueryLimits::default(),
    )
    .unwrap();
    let hits = scene
        .ray_cast(ray([0.; 3], [1., 0., 0.], 50.), QueryBudget::default())
        .unwrap();
    close(hits[0].distance, 8.);
    close(hits[0].position[0], 8.);
    assert_eq!(&hits[0].position[1..], &[0., 0.]);
    assert_eq!(hits[0].body_filter.flags_and_parts, 0xe7);
    assert_eq!(hits[0].body_filter.group, 0x1234);
    assert_eq!(hits[0].source.shape_block, 1);
    assert_eq!(hits[0].source.source_sha256, [9; 32]);
    assert!(
        scene
            .ray_cast(ray([0.; 3], [1., 0., 0.], 7.), QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert!(!scene.faithful_ready());
    close(
        scene
            .ray_cast(ray([20., 0., 0.], [1., 0., 0.], 0.), QueryBudget::default())
            .unwrap()[0]
            .distance,
        0.,
    );
}

#[test]
fn box_closed_volume_and_sphere_corner_overlap_use_geometry() {
    let scene = scene(&[("bhkRigidBody", body(1)), ("bhkBoxShape", bx())]);
    let hit = scene
        .ray_cast(ray([-5., 0., 0.], [1., 0., 0.], 9.), QueryBudget::default())
        .unwrap()
        .remove(0);
    close(hit.distance, 4.);
    assert_eq!(hit.authored_shell_radius, 0.25);
    assert!(
        scene
            .ray_cast(
                ray([-5., 2.01, 0.], [1., 0., 0.], 9.),
                QueryBudget::default()
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scene
            .overlap_sphere([1., 2., 3.], 0., QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
    assert!(
        scene
            .overlap_sphere([2., 3., 4.], 1.7, QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scene
            .overlap_sphere([2., 3., 4.], 1.8, QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn capsule_side_caps_parallel_axis_and_collapsed_segment() {
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkCapsuleShape", capsule([0., 0., -2.], [0., 0., 2.], 1.)),
    ]);
    for (o, d, expected) in [
        ([-5., 0., 0.], [1., 0., 0.], 4.),
        ([0., 0., -5.], [0., 0., 1.], 2.),
        ([-5., 0., 2.5], [1., 0., 0.], 5. - 0.75f64.sqrt()),
    ] {
        close(
            scene
                .ray_cast(ray(o, d, 10.), QueryBudget::default())
                .unwrap()[0]
                .distance,
            expected,
        );
    }
    assert!(
        scene
            .overlap_sphere([2.1, 0., 0.], 1., QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    let collapsed = scene_for_collapsed();
    close(
        collapsed
            .ray_cast(
                ray([-4., 0., 0.], [1., 0., 0.], 10.),
                QueryBudget::default(),
            )
            .unwrap()[0]
            .distance,
        3.,
    );
}
fn scene_for_collapsed() -> StaticScene {
    scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkCapsuleShape", capsule([0.; 3], [0.; 3], 1.)),
    ])
}

#[test]
fn shared_shape_paths_apply_column_matrix_and_keep_occurrence_identity() {
    let mut list = Vec::new();
    words(&mut list, &[2, 2, 3, 0, 0, 0, 0, 0, 0, 0, 2]);
    list.extend([0; 8]);
    let columns = [
        [0., 1., 0., 0.],
        [-1., 0., 0., 0.],
        [0., 0., 1., 0.],
        [5., 0., 0., 1.],
    ];
    let reflected = [
        [-1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [10., 0., 0., 1.],
    ];
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkListShape", list),
        ("bhkTransformShape", transform(4, columns)),
        ("bhkTransformShape", transform(4, reflected)),
        ("bhkBoxShape", bx()),
    ]);
    assert_eq!(scene.primitive_count(), 2);
    let hits = scene
        .ray_cast(ray([0.; 3], [1., 0., 0.], 20.), QueryBudget::default())
        .unwrap();
    close(hits[0].distance, 3.);
    close(hits[1].distance, 9.);
    assert_eq!(hits[0].source.shape_block, hits[1].source.shape_block);
    assert_ne!(hits[0].source.occurrence, hits[1].source.occurrence);
}

#[test]
fn body_transform_is_applied_only_for_t_suffix() {
    for (name, expected) in [("bhkRigidBody", 4.), ("bhkRigidBodyT", 9.)] {
        let mut data = body(1);
        data[52..56].copy_from_slice(&5f32.to_le_bytes());
        let scene = scene(&[(name, data), ("bhkSphereShape", sphere(1.))]);
        close(
            scene
                .ray_cast(
                    ray([-5., 0., 0.], [1., 0., 0.], 20.),
                    QueryBudget::default(),
                )
                .unwrap()[0]
                .distance,
            expected,
        );
    }
}

#[test]
fn packed_triangle_winding_degenerate_edges_and_source_metadata() {
    let mut data = Vec::new();
    words(&mut data, &[2]);
    for v in [0u16, 1, 2, 0xabcd, 0, 0, 1, 0x1234] {
        data.extend(v.to_le_bytes());
    }
    words(&mut data, &[3]);
    data.push(0);
    floats(&mut data, &[0., 0., 0., 2., 0., 0., 0., 2., 0.]);
    data.extend(1u16.to_le_bytes());
    data.extend([7, 0x81, 0x34, 0x12]);
    words(&mut data, &[3, 42]);
    let mut shape = Vec::new();
    words(&mut shape, &[0, 0]);
    floats(&mut shape, &[0.1]);
    words(&mut shape, &[0]);
    floats(&mut shape, &[1., 1., 1., 0., 0.1, 1., 1., 1., 0.]);
    words(&mut shape, &[2]);
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkPackedNiTriStripsShape", shape),
        ("hkPackedNiTriStripsData", data),
    ]);
    let hits = scene
        .ray_cast(
            ray([0.5, 0.5, 3.], [0., 0., -1.], 10.),
            QueryBudget::default(),
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    close(hits[0].distance, 3.);
    assert_eq!(hits[0].material, 42);
    assert_eq!(hits[0].welding, Some(0xabcd));
    assert_eq!(hits[0].source.triangle, Some(0));
    assert_eq!(hits[0].authored_shell_radius, 0.1);
    assert_eq!(
        scene
            .overlap_sphere([1., 0., 0.], 0., QueryBudget::default())
            .unwrap()
            .len(),
        2
    );
    assert!(
        scene
            .overlap_sphere([1.9, 1.9, 0.], 0.1, QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scene
            .ray_cast(
                ray([0.5, 0.5, -3.], [0., 0., 1.], 10.),
                QueryBudget::default()
            )
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn invalid_units_queries_and_budgets_fail_without_partial_hits() {
    let (_, collision) = nif_collision::decode(
        &container(&[("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))]),
        "budget",
    )
    .unwrap();
    assert!(
        StaticScene::build(
            &collision,
            &[placement()],
            EngineeringUnits {
                havok_to_source: 0.,
                ..units()
            },
            QueryLimits::default()
        )
        .is_err()
    );
    for limits in [
        QueryLimits {
            blocks: 0,
            ..QueryLimits::default()
        },
        QueryLimits {
            shape_visits: 0,
            ..QueryLimits::default()
        },
        QueryLimits {
            primitives: 0,
            ..QueryLimits::default()
        },
        QueryLimits {
            geometry_elements: 0,
            ..QueryLimits::default()
        },
    ] {
        assert!(matches!(
            StaticScene::build(&collision, &[placement()], units(), limits),
            Err(QueryError::Budget(_))
        ));
    }
    let scene =
        StaticScene::build(&collision, &[placement()], units(), QueryLimits::default()).unwrap();
    for query in [
        ray([f64::NAN, 0., 0.], [1., 0., 0.], 10.),
        ray([0.; 3], [0.; 3], 10.),
        ray([0.; 3], [2., 0., 0.], 10.),
        ray([0.; 3], [1., 0., 0.], f64::INFINITY),
    ] {
        assert!(scene.ray_cast(query, QueryBudget::default()).is_err());
    }
    assert!(
        scene
            .overlap_sphere([f64::MAX, 0., 0.], 1., QueryBudget::default())
            .is_err()
    );
    assert!(
        scene
            .ray_cast(
                ray([f64::MAX, 0., 0.], [1., 0., 0.], 10.),
                QueryBudget::default()
            )
            .is_err()
    );
    assert!(
        StaticScene::build(
            &collision,
            &[placement(), placement()],
            units(),
            QueryLimits::default()
        )
        .is_err()
    );
    for budget in [
        QueryBudget {
            primitive_tests: 0,
            ..QueryBudget::default()
        },
        QueryBudget {
            hits: 0,
            ..QueryBudget::default()
        },
    ] {
        assert!(matches!(
            scene.ray_cast(ray([-5., 0., 0.], [1., 0., 0.], 10.), budget),
            Err(QueryError::Budget(_))
        ));
        assert!(matches!(
            scene.overlap_sphere([0.; 3], 1., budget),
            Err(QueryError::Budget(_))
        ));
    }
}

#[test]
fn unsupported_or_singular_authored_geometry_never_yields_partial_scene() {
    for columns in [
        [[0.; 4]; 4],
        [
            [1., 0., 0., 0.],
            [1., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
        [
            [2., 0., 0., 0.],
            [0., 1., 0., 0.],
            [0., 0., 1., 0.],
            [0., 0., 0., 1.],
        ],
    ] {
        let (_, collision) = nif_collision::decode(
            &container(&[
                ("bhkRigidBody", body(1)),
                ("bhkTransformShape", transform(2, columns)),
                ("bhkSphereShape", sphere(1.)),
            ]),
            "bad frame",
        )
        .unwrap();
        assert!(
            StaticScene::build(&collision, &[placement()], units(), QueryLimits::default())
                .is_err()
        );
    }
    let (_, collision) = nif_collision::decode(
        &container(&[
            ("bhkRigidBody", body(1)),
            ("bhkCapsuleShape", capsule([0.; 3], [0., 0., 2.], -1.)),
        ]),
        "bad radius",
    )
    .unwrap();
    assert!(matches!(
        StaticScene::build(&collision, &[placement()], units(), QueryLimits::default()),
        Err(QueryError::Unsupported { block: 1, .. })
    ));
}
