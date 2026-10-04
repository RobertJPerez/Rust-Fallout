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
    convex_bounds([1., 2., -1.], [3., 6., 1.])
}
fn convex_bounds(minimum: [f32; 3], maximum: [f32; 3]) -> Vec<u8> {
    let mut b = sphere(0.25);
    words(&mut b, &[0, 0, 0x8000_0000, 0, 0, 0x8000_0000, 8]);
    for x in [minimum[0], maximum[0]] {
        for y in [minimum[1], maximum[1]] {
            for z in [minimum[2], maximum[2]] {
                floats(&mut b, &[x, y, z, 0.]);
            }
        }
    }
    words(&mut b, &[6]);
    for plane in [
        [-1., 0., 0., minimum[0]],
        [1., 0., 0., -maximum[0]],
        [0., -1., 0., minimum[1]],
        [0., 1., 0., -maximum[1]],
        [0., 0., -1., minimum[2]],
        [0., 0., 1., -maximum[2]],
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

fn shape_list(children: &[u32]) -> Vec<u8> {
    let mut list = Vec::new();
    words(&mut list, &[children.len() as u32]);
    words(&mut list, children);
    words(&mut list, &[0; 7]);
    words(&mut list, &[children.len() as u32]);
    list.extend(vec![0; 4 * children.len()]);
    list
}
fn separated_cuboids(count: usize) -> Vec<(&'static str, Vec<u8>)> {
    let mut blocks = vec![
        ("bhkRigidBody", body(1)),
        (
            "bhkListShape",
            shape_list(&(2..2 + count as u32).collect::<Vec<_>>()),
        ),
    ];
    for i in 0..count {
        blocks.push((
            "bhkConvexVerticesShape",
            convex_bounds([4. * i as f32, 0., 0.], [4. * i as f32 + 1., 1., 1.]),
        ));
    }
    blocks
}

#[test]
fn source_index_preserves_independent_exhaustive_hits_with_bounded_work() {
    let blocks = separated_cuboids(64);
    let scene = scene(&blocks);
    let small = QueryBudget {
        primitive_tests: 10,
        geometry_tests: 0,
        hits: 1,
    };
    let first = scene
        .ray_cast(ray([-5., 0.5, 0.5], [1., 0., 0.], 6.), small)
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].distance, 5.);
    assert_eq!(first[0].source.shape_block, 2);
    assert_eq!(first[0].source.occurrence, 0);
    // A single node rejects this parallel miss, with no narrowed primitive.
    assert!(
        scene
            .ray_cast(
                ray([-5., 2., 0.5], [1., 0., 0.], 300.),
                QueryBudget {
                    primitive_tests: 1,
                    ..small
                }
            )
            .unwrap()
            .is_empty()
    );
    let all = scene
        .ray_cast(
            ray([-5., 0.5, 0.5], [1., 0., 0.], 300.),
            QueryBudget {
                primitive_tests: 64,
                hits: 64,
                ..small
            },
        )
        .unwrap();
    assert_eq!(all.len(), 64);
    //2P capped by the127 retained nodes admits the complete tree atP64;
    // reducingP to63 permits126 visits, exactly one below the required work.
    assert!(matches!(
        scene.ray_cast(
            ray([-5., 0.5, 0.5], [1., 0., 0.], 300.),
            QueryBudget {
                primitive_tests: 63,
                hits: 64,
                ..small
            }
        ),
        Err(QueryError::Budget("spatial index visits"))
    ));
    for (i, hit) in all.iter().enumerate() {
        assert_eq!(hit.distance, 5. + 4. * i as f64);
        assert_eq!(hit.source.shape_block, 2 + i as u32);
        assert_eq!(hit.source.occurrence, i);
    }
    let negative = scene
        .ray_cast(
            ray([130., 0.5, 0.5], [-1., 0., 0.], 20.),
            QueryBudget::default(),
        )
        .unwrap();
    assert_eq!(negative.len(), 5);
    for (i, hit) in negative.iter().enumerate() {
        assert_eq!(hit.distance, 1. + 4. * i as f64);
        assert_eq!(hit.source.occurrence, 32 - i);
    }
    let overlaps = scene
        .overlap_sphere([10., 0.5, 0.5], 5., QueryBudget::default())
        .unwrap();
    assert_eq!(
        overlaps
            .iter()
            .map(|h| h.source.occurrence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(matches!(
        scene.ray_cast(
            ray([-5., 0.5, 0.5], [1., 0., 0.], 6.),
            QueryBudget {
                primitive_tests: 1,
                ..small
            }
        ),
        Err(QueryError::Budget("spatial index visits"))
    ));
    assert!(matches!(
        scene.ray_cast(
            ray([-5., 0.5, 0.5], [1., 0., 0.], 300.),
            QueryBudget {
                primitive_tests: 64,
                hits: 63,
                ..small
            }
        ),
        Err(QueryError::Budget("query hits"))
    ));
    let (_, collision) = nif_collision::decode(&container(&blocks), "index accounting").unwrap();
    //64 cached hulls *15 source elements, plus127 balanced index nodes.
    assert!(
        StaticScene::build(
            &collision,
            &[placement()],
            units(),
            QueryLimits {
                geometry_elements: 1087,
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(matches!(
        StaticScene::build(
            &collision,
            &[placement()],
            units(),
            QueryLimits {
                geometry_elements: 1086,
                ..Default::default()
            }
        ),
        Err(QueryError::Budget("geometry/index elements"))
    ));
}

#[test]
fn source_index_preserves_reflections_and_approximate_frames() {
    let (_, collision) = nif_collision::decode(
        &container(&separated_cuboids(64)),
        "indexed and fallback sources",
    )
    .unwrap();
    for approximate in [false, true] {
        let mut other = placement();
        other.reference = ReferenceId(NonZeroU64::new(2).unwrap());
        if approximate {
            other.attachment_to_source.rows[0][1] = 1e-7;
        } else {
            other.attachment_to_source.rows[0] = [-1., 0., 0., 300.];
        }
        let scene = StaticScene::build(
            &collision,
            &[placement(), other],
            units(),
            QueryLimits::default(),
        )
        .unwrap();
        let hits = scene
            .ray_cast(
                ray([-5., 0.5, 0.5], [1., 0., 0.], 400.),
                QueryBudget::default(),
            )
            .unwrap();
        assert_eq!(hits.len(), 128);
        for hit in &hits {
            let ordinal = hit.source.occurrence;
            assert_eq!(hit.source.shape_block, 2 + ordinal as u32);
            let expected = if hit.source.reference.0.get() == 1 {
                5. + 4. * ordinal as f64
            } else if approximate {
                // Authored x'=x+1e-7*y shifts the y0.5 face by5e-8.
                5. + 4. * ordinal as f64 + 5e-8
            } else {
                304. - 4. * ordinal as f64
            };
            assert!((hit.distance - expected).abs() <= 4. * f64::EPSILON * expected);
        }
        if approximate {
            for pair in hits.as_chunks::<2>().0 {
                assert_eq!(pair[0].source.reference.0.get(), 1);
                assert_eq!(pair[1].source.reference.0.get(), 2);
                assert!(pair[0].distance < pair[1].distance);
            }
        } else {
            let overlaps = scene
                .overlap_sphere([50., 0.5, 0.5], 2., QueryBudget::default())
                .unwrap();
            assert_eq!(
                overlaps
                    .iter()
                    .map(|h| (h.source.reference.0.get(), h.source.occurrence))
                    .collect::<Vec<_>>(),
                vec![(1, 12), (1, 13), (2, 62), (2, 63)]
            );
        }
    }
}

#[test]
fn transformed_source_index_reaches_queries_with_small_budgets() {
    let (_, collision) = nif_collision::decode(
        &container(&separated_cuboids(64)),
        "transformed source index",
    )
    .unwrap();
    for (rows, scale) in [
        (
            [[1., 0., 0., 10.], [0., 1., 0., -20.], [0., 0., 1., 30.]],
            1.,
        ),
        (
            [[-1., 0., 0., 300.], [0., 1., 0., -20.], [0., 0., 1., 30.]],
            1.,
        ),
        (
            [[0., -1., 0., 10.], [1., 0., 0., -20.], [0., 0., 1., 30.]],
            1.,
        ),
        (
            [[2., 0., 0., 10.], [0., 2., 0., -20.], [0., 0., 2., 30.]],
            2.,
        ),
        (
            [[0.5, 0., 0., 10.], [0., 0.5, 0., -20.], [0., 0., 0.5, 30.]],
            0.5,
        ),
        (
            [
                [0.6, -0.8, 0., 10.],
                [0.8, 0.6, 0., -20.],
                [0., 0., 1., 30.],
            ],
            1.,
        ),
        (
            [[1., 1e-7, 0., 10.], [0., 1., 0., -20.], [0., 0., 1., 30.]],
            1.,
        ),
    ] {
        let frame = Affine { rows };
        let mut place = placement();
        place.attachment_to_source = frame;
        let scene = StaticScene::build(
            &collision,
            std::slice::from_ref(&place),
            units(),
            QueryLimits {
                geometry_elements: 1087,
                ..Default::default()
            },
        )
        .unwrap();
        let direction = std::array::from_fn(|i| rows[i][0] / scale);
        let small = QueryBudget {
            primitive_tests: 10,
            geometry_tests: 0,
            hits: 1,
        };
        let first = scene
            .ray_cast(
                ray(frame.point([-5., 0.5, 0.5]), direction, 6. * scale),
                small,
            )
            .unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].source.occurrence, 0);
        assert_eq!(first[0].source.shape_block, 2);
        assert!((first[0].distance - 5. * scale).abs() <= 16. * f64::EPSILON * (5. * scale));
        let overlaps = scene
            .overlap_sphere(frame.point([0.5; 3]), 0., small)
            .unwrap();
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].source, first[0].source);
        assert!(
            scene
                .ray_cast(
                    ray(frame.point([-5., 2., 0.5]), direction, 0.),
                    QueryBudget {
                        primitive_tests: 1,
                        ..small
                    }
                )
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            StaticScene::build(
                &collision,
                &[place],
                units(),
                QueryLimits {
                    geometry_elements: 1086,
                    ..Default::default()
                }
            ),
            Err(QueryError::Budget("geometry/index elements"))
        ));
    }
}

#[test]
fn transformed_culling_preserves_local_conversion_refusals() {
    let (_, collision) =
        nif_collision::decode(&container(&separated_cuboids(8)), "local-domain culling").unwrap();
    let mut place = placement();
    place.attachment_to_source.rows = [[0.5, 0., 0., 0.], [0., 0.5, 0., 0.], [0., 0., 0.5, 0.]];
    let scene = StaticScene::build(&collision, &[place], units(), QueryLimits::default()).unwrap();
    // World-domain inputs are valid, but their local projection is outside the
    // original predicate's domain. A bounding miss cannot turn refusal into clear.
    assert!(matches!(
        scene.ray_cast(ray([1e50; 3], [1., 0., 0.], 0.), QueryBudget::default()),
        Err(QueryError::Invalid("overflowing local ray"))
    ));
    assert!(matches!(
        scene.overlap_sphere([1e50; 3], 0., QueryBudget::default()),
        Err(QueryError::Invalid("overflowing local sphere"))
    ));
    assert!(matches!(
        scene.overlap_sphere([0.; 3], 1e50, QueryBudget::default()),
        Err(QueryError::Invalid("overflowing local sphere"))
    ));
}

#[test]
fn culling_keeps_uncertain_source_slab_and_subnormal_boundaries() {
    let scene = scene(&separated_cuboids(64));
    assert!(matches!(
        scene.ray_cast(
            ray(
                [-600_000_000_000_000., -799_999_999_999_999., 0.5],
                [0.6, 0.8, 0.],
                1_000_000_000_000_002.
            ),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "cuboid slab predicate is numerically uncertain"
        ))
    ));
    let tiny = f64::from_bits(1);
    assert!(
        scene
            .overlap_sphere([-tiny, 0.5, 0.5], 0., QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scene
            .overlap_sphere([-tiny, 0.5, 0.5], tiny, QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        scene
            .ray_cast(
                ray([-tiny, 0.5, 0.5], [1., 0., 0.], tiny),
                QueryBudget::default()
            )
            .unwrap()[0]
            .distance,
        tiny
    );
    assert!(
        scene
            .ray_cast(
                ray([-1., 0.5, 0.5], [tiny, 1., 0.], 10.),
                QueryBudget::default()
            )
            .is_err()
    );
}

#[test]
fn source_bounds_keep_all_supported_shapes_and_shared_occurrences() {
    let mut packed = Vec::new();
    words(&mut packed, &[1]);
    for word in [0u16, 1, 2, 0xabcd] {
        packed.extend(word.to_le_bytes());
    }
    words(&mut packed, &[3]);
    packed.push(0);
    floats(&mut packed, &[0., 0., 0., 0., 2., 0., 0., 0., 2.]);
    packed.extend(1u16.to_le_bytes());
    packed.extend([7, 0x81, 0x34, 0x12]);
    words(&mut packed, &[3, 42]);
    let mut packed_shape = Vec::new();
    words(&mut packed_shape, &[0, 0]);
    floats(&mut packed_shape, &[0.1]);
    words(&mut packed_shape, &[0]);
    floats(&mut packed_shape, &[1., 1., 1., 0., 0.1, 1., 1., 1., 0.]);
    words(&mut packed_shape, &[7]);
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkListShape", shape_list(&[2, 3, 4, 5, 6, 2, 3, 4])),
        ("bhkSphereShape", sphere(1.)),
        ("bhkBoxShape", bx()),
        ("bhkCapsuleShape", capsule([0., 0., -2.], [0., 0., 2.], 1.)),
        ("bhkConvexVerticesShape", convex_bounds([0.; 3], [1.; 3])),
        ("bhkPackedNiTriStripsShape", packed_shape),
        ("hkPackedNiTriStripsData", packed),
    ]);
    let hits = scene
        .ray_cast(
            ray([-5., 0.5, 0.5], [1., 0., 0.], 10.),
            QueryBudget::default(),
        )
        .unwrap();
    assert_eq!(hits.len(), 8);
    for hit in &hits {
        let expected = match hit.source.shape_block {
            2 => 5. - 0.5f64.sqrt(),
            3 => 4.,
            4 => 5. - 0.75f64.sqrt(),
            5 | 7 => 5.,
            _ => panic!("unexpected source"),
        };
        close(hit.distance, expected);
    }
    let overlaps = scene
        .overlap_sphere([0., 0.5, 0.5], 0., QueryBudget::default())
        .unwrap();
    assert_eq!(
        overlaps
            .iter()
            .map(|h| h.source.occurrence)
            .collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
    assert!(matches!(
        scene.ray_cast(
            ray([-5., 0.5, 0.5], [1., 0., 0.], 10.),
            QueryBudget {
                geometry_tests: 0,
                ..Default::default()
            }
        ),
        Err(QueryError::Budget("geometry tests"))
    ));
}

#[test]
fn cuboid_slab_intervals_and_query_cutoff_never_admit_rounded_false_hits() {
    let cube = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkConvexVerticesShape", convex_bounds([0.; 3], [1.; 3])),
    ]);
    // Fraction over the literal binary64 inputs gives disjoint slab intervals,
    // although their nearest-rounded endpoints both equal1e15.
    assert!(matches!(
        cube.ray_cast(
            ray(
                [-600_000_000_000_000., -799_999_999_999_999., 0.5],
                [0.6, 0.8, 0.],
                1_000_000_000_000_002.
            ),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "cuboid slab predicate is numerically uncertain"
        ))
    ));
    let offset = scene(&[
        ("bhkRigidBody", body(1)),
        (
            "bhkConvexVerticesShape",
            convex_bounds([1., 0., 0.], [2., 1., 1.]),
        ),
    ]);
    // The exact entrance is1+binary64(1e-16), strictly beyond the cutoff1.
    assert!(
        offset
            .ray_cast(
                ray([-1e-16, 0.5, 0.5], [1., 0., 0.], 1.),
                QueryBudget::default()
            )
            .is_err()
    );
    let hit = offset
        .ray_cast(
            ray([-1e-16, 0.5, 0.5], [1., 0., 0.], 2.),
            QueryBudget::default(),
        )
        .unwrap();
    assert!(hit[0].distance > 1. && hit[0].position[0] >= 1.);
    for (origin, direction, max, distance) in [
        ([0., 0.5, 0.5], [1., 0., 0.], 1., 1.),
        ([3., 0.5, 0.5], [-1., 0., 0.], 1., 1.),
        ([1.5, 0.5, 0.5], [1., 0., 0.], 0., 0.),
    ] {
        assert_eq!(
            offset
                .ray_cast(ray(origin, direction, max), QueryBudget::default())
                .unwrap()[0]
                .distance,
            distance
        );
    }
    assert!(
        offset
            .ray_cast(
                ray([0., 2., 0.5], [1., 0., 0.], 10.),
                QueryBudget::default()
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        cube.ray_cast(
            ray([-1., 0.5, 0.5], [f64::from_bits(1), 1., 0.], 10.),
            QueryBudget::default()
        )
        .is_err()
    );
    let tiny = f64::from_bits(1);
    assert_eq!(
        cube.ray_cast(
            ray([-tiny, 0.5, 0.5], [1., 0., 0.], tiny),
            QueryBudget::default()
        )
        .unwrap()[0]
            .distance,
        tiny
    );

    let box_scene = scene(&[("bhkRigidBody", body(1)), ("bhkBoxShape", bx())]);
    assert_eq!(
        box_scene
            .ray_cast(ray([-1., 0., 0.], [1., 0., 0.], 0.), QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
    assert!(
        box_scene
            .ray_cast(
                ray([(-1f64).next_down(), 0., 0.], [1., 0., 0.], 0.),
                QueryBudget::default()
            )
            .unwrap()
            .is_empty()
    );
    let mut half_box = bx();
    half_box[16..28].copy_from_slice(&[0.5f32.to_le_bytes(); 3].concat());
    let half_box = scene(&[("bhkRigidBody", body(1)), ("bhkBoxShape", half_box)]);
    assert!(
        half_box
            .ray_cast(
                ray(
                    [-600_000_000_000_000.5, -799_999_999_999_999.5, 0.],
                    [0.6, 0.8, 0.],
                    1_000_000_000_000_002.
                ),
                QueryBudget::default()
            )
            .is_err()
    );
    assert!(
        cube.ray_cast(
            ray([-tiny, 0.5, 0.5], [1. + f64::EPSILON, 0., 0.], tiny),
            QueryBudget::default()
        )
        .is_err()
    );
}

#[test]
fn cuboid_uncertainty_discards_earlier_leaf_hits_atomically() {
    let (_, collision) = nif_collision::decode(
        &container(&[
            ("bhkRigidBody", body(1)),
            (
                "bhkConvexVerticesShape",
                convex_bounds([1., 0., 0.], [2., 1., 1.]),
            ),
        ]),
        "atomic cuboid queries",
    )
    .unwrap();
    let mut first = placement();
    first.attachment_to_source.rows[0][3] = -1.;
    let mut second = placement();
    second.reference = ReferenceId(NonZeroU64::new(2).unwrap());
    let scene = StaticScene::build(
        &collision,
        &[first, second],
        units(),
        QueryLimits::default(),
    )
    .unwrap();
    assert!(
        scene
            .ray_cast(
                ray([-1e-16, 0.5, 0.5], [1., 0., 0.], 1.),
                QueryBudget::default()
            )
            .is_err()
    );
    assert!(
        scene
            .overlap_sphere([-1e-16, 0.5, 0.5], 1., QueryBudget::default())
            .is_err()
    );
}

#[test]
fn cuboid_overlap_preserves_subnormals_and_refuses_lost_subtraction() {
    let cube = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkConvexVerticesShape", convex_bounds([0.; 3], [1.; 3])),
    ]);
    let tiny = f64::from_bits(1);
    for face in [0., -0., 1.] {
        assert_eq!(
            cube.overlap_sphere([face, 0.5, 0.5], 0., QueryBudget::default())
                .unwrap()
                .len(),
            1
        );
    }
    assert!(
        cube.overlap_sphere([-tiny, 0.5, 0.5], 0., QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        cube.overlap_sphere([-tiny, 0.5, 0.5], tiny, QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
    assert!(
        cube.overlap_sphere([-2. * tiny, 0.5, 0.5], tiny, QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    let offset = scene(&[
        ("bhkRigidBody", body(1)),
        (
            "bhkConvexVerticesShape",
            convex_bounds([1., 0., 0.], [2., 1., 1.]),
        ),
    ]);
    assert!(
        offset
            .overlap_sphere([-1e-16, 0.5, 0.5], 1., QueryBudget::default())
            .is_err()
    );
    assert!(
        offset
            .overlap_sphere([-1e-16, 0.5, 0.5], 0.5, QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        offset
            .overlap_sphere([-1e-16, 0.5, 0.5], 2., QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
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
fn long_skew_capsule_original_axis_refuses_independent_exact_line_miss() {
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        (
            "bhkCapsuleShape",
            capsule(
                [0.; 3],
                [52_261_384_192., 84_231_479_296., 55_128_973_312.],
                1.,
            ),
        ),
    ]);
    // Independent Fraction over these exact source-f32 / request-f64 words:
    // infinite-line distance^2 is13391122364968215868819298297140779722394308083273962324769 /
    // 13391094655208010850365703542705265712767206248445162553344 >1.
    // Neither finite subset can intersect. The declared interval refuses it.
    assert!(matches!(
        scene.ray_cast(
            ray(
                [26130692086.14725, 42115739648.971, 27564486654.273586],
                [0.9951285574918328, -0.04695609615116375, 0.0866849415900279],
                20.
            ),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "capsule side predicate is numerically uncertain"
        ))
    ));
}

#[test]
fn ordinary_skew_capsule_side_and_exact_parallel_cap_keep_analytic_entries() {
    let scene = scene(&[
        ("bhkRigidBody", body(1)),
        ("bhkCapsuleShape", capsule([0.; 3], [4., 4., 0.], 1.)),
    ]);
    close(
        scene
            .ray_cast(
                ray([2., 2., -3.], [0., 0., 1.], 10.),
                QueryBudget::default(),
            )
            .unwrap()[0]
            .distance,
        2.,
    );
    let half = std::f64::consts::FRAC_1_SQRT_2;
    close(
        scene
            .ray_cast(
                ray([-2., -2., 0.], [half, half, 0.], 10.),
                QueryBudget::default(),
            )
            .unwrap()[0]
            .distance,
        8f64.sqrt() - 1.,
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

fn packed_triangle_fixture(vertices: [[f32; 3]; 3]) -> Vec<(&'static str, Vec<u8>)> {
    let mut data = Vec::new();
    words(&mut data, &[1]);
    for value in [0u16, 1, 2, 0xabcd] {
        data.extend(value.to_le_bytes());
    }
    words(&mut data, &[3]);
    data.push(0);
    for vertex in vertices {
        floats(&mut data, &vertex);
    }
    data.extend(1u16.to_le_bytes());
    data.extend([7, 0x81, 0x34, 0x12]);
    words(&mut data, &[3, 42]);
    let mut shape = Vec::new();
    words(&mut shape, &[0, 0]);
    floats(&mut shape, &[0.]);
    words(&mut shape, &[0]);
    floats(&mut shape, &[1., 1., 1., 0., 0., 1., 1., 1., 0.]);
    words(&mut shape, &[2]);
    vec![
        ("bhkRigidBody", body(1)),
        ("bhkPackedNiTriStripsShape", shape),
        ("hkPackedNiTriStripsData", data),
    ]
}
#[test]
fn source_thin_triangle_true_crossing_cannot_become_a_parallel_miss() {
    // Literal f32 epsilon2^-23. Independent Fraction over these EXACT source
    // and request words gives determinant -2^-76 and t=1, barycentric1/2,1/4,1/4.
    let eps = f32::from_bits(0x3400_0000);
    let scene = scene(&packed_triangle_fixture([
        [0.; 3],
        [1.; 3],
        [1., 1. + eps, 1. - eps],
    ]));
    let q = f64::from_bits(0x3fe2_79a7_4590_331d);
    let direction = [q, q, f64::from_bits(q.to_bits() + 1)];
    let point = [0.5, 0.5 + f64::from(eps) / 4., 0.5 - f64::from(eps) / 4.];
    let origin = std::array::from_fn(|i| point[i] - direction[i]);
    let result = scene.ray_cast(ray(origin, direction, 2.), QueryBudget::default());
    assert!(
        matches!(result, Err(QueryError::Invalid(_))),
        "uncertain source crossing falsely accepted: {result:?}"
    );
}

#[test]
fn thin_source_triangle_raw_word_family_winding_and_cyclic_axes_refuse_atomically() {
    let eps = f32::from_bits(0x3400_0000);
    for axis in 0..3 {
        for reverse in [false, true] {
            let rotate = |p: [f32; 3]| [p[axis], p[(axis + 1) % 3], p[(axis + 2) % 3]];
            let mut vertices = [[0.; 3], [1.; 3], [1., 1. + eps, 1. - eps]].map(rotate);
            if reverse {
                vertices.swap(1, 2);
            }
            let scene = scene(&packed_triangle_fixture(vertices));
            for offset in -10..=10 {
                let bits = 0x3fe2_79a7_4590_331du64.checked_add_signed(offset).unwrap();
                for changed in 0..3 {
                    let mut direction = [f64::from_bits(bits); 3];
                    direction[changed] = f64::from_bits(bits + 1);
                    let point = [0.5, 0.5 + f64::from(eps) / 4., 0.5 - f64::from(eps) / 4.];
                    let origin = std::array::from_fn(|i| point[i] - direction[i]);
                    let rotate = |p: [f64; 3]| [p[axis], p[(axis + 1) % 3], p[(axis + 2) % 3]];
                    let result = scene.ray_cast(
                        ray(rotate(origin), rotate(direction), 2.),
                        QueryBudget::default(),
                    );
                    assert!(
                        matches!(result, Err(QueryError::Invalid(_))),
                        "axis={axis} reverse={reverse} offset={offset} changed={changed}: {result:?}"
                    );
                }
            }
        }
    }
    let mut blocks = packed_triangle_fixture([[0.; 3], [1.; 3], [1., 1. + eps, 1. - eps]]);
    blocks.push(("bhkRigidBody", body(4)));
    blocks.push(("bhkSphereShape", sphere(0.1)));
    let (_, collision) =
        nif_collision::decode(&container(&blocks), "partial hit before uncertain source").unwrap();
    let mut sphere_placement = placement();
    sphere_placement.body_block = 3;
    let scene = StaticScene::build(
        &collision,
        &[sphere_placement, placement()],
        units(),
        QueryLimits::default(),
    )
    .unwrap();
    let q = f64::from_bits(0x3fe2_79a7_4590_331d);
    let direction = [q, q, f64::from_bits(q.to_bits() + 1)];
    let point = [0.5, 0.5 + f64::from(eps) / 4., 0.5 - f64::from(eps) / 4.];
    let origin = std::array::from_fn(|i| point[i] - direction[i]);
    assert!(matches!(
        scene.ray_cast(ray(origin, direction, 2.), QueryBudget::default()),
        Err(QueryError::Invalid(_))
    ));
}

#[test]
fn exact_parallel_coplanar_degenerate_and_ordinary_triangle_controls_stay_available() {
    let ordinary = scene(&packed_triangle_fixture([
        [0.; 3],
        [2., 0., 0.],
        [0., 2., 0.],
    ]));
    for z in [-3., 3.] {
        let hits = ordinary
            .ray_cast(
                ray([0.5, 0.5, z], [0., 0., -z.signum()], 10.),
                QueryBudget::default(),
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].distance, 3.);
        assert_eq!(hits[0].material, 42);
        assert_eq!(hits[0].welding, Some(0xabcd));
        assert_eq!(hits[0].shape_filter.unwrap().flags_and_parts, 0x81);
    }
    for o in [[0.5, 0.5, 3.], [0.5, 0.5, 0.]] {
        assert!(
            ordinary
                .ray_cast(ray(o, [1., 0., 0.], 10.), QueryBudget::default())
                .unwrap()
                .is_empty()
        );
    }
    let eps = f32::from_bits(0x3400_0000);
    let thin = scene(&packed_triangle_fixture([
        [0.; 3],
        [1.; 3],
        [1., 1. + eps, 1. - eps],
    ]));
    let q = f64::from_bits(0x3fe2_79a7_4590_331d);
    assert!(
        thin.ray_cast(ray([0.; 3], [q; 3], 2.), QueryBudget::default())
            .unwrap()
            .is_empty()
    );
    let degenerate = scene(&packed_triangle_fixture([[0.; 3], [1.; 3], [2.; 3]]));
    assert!(
        degenerate
            .ray_cast(
                ray([0., 0., 3.], [0., 0., -1.], 10.),
                QueryBudget::default()
            )
            .unwrap()
            .is_empty()
    );
    let skew_plane = scene(&packed_triangle_fixture([[0.; 3], [1.; 3], [3., 2., 2.]]));
    assert!(
        skew_plane
            .ray_cast(ray([0., 3., 0.], [1., 0., 0.], 10.), QueryBudget::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn triangle_closed_contacts_and_range_remain_available_across_source_scales() {
    for exponent in [-80, -40, 0, 40, 80] {
        let scale = 2f32.powi(exponent);
        let s = f64::from(scale);
        let scene = scene(&packed_triangle_fixture([
            [0.; 3],
            [scale, 0., 0.],
            [0., scale, 0.],
        ]));
        for xy in [[0., 0.], [s, 0.], [s / 2., s / 2.], [s / 4., s / 4.]] {
            let r = ray([xy[0], xy[1], 3. * s], [0., -0., -1.], 3. * s);
            let hits = scene.ray_cast(r, QueryBudget::default()).unwrap();
            assert_eq!(hits.len(), 1, "exponent={exponent} xy={xy:?}");
            assert_eq!(hits[0].distance, 3. * s);
            assert!(
                scene
                    .ray_cast(
                        Ray {
                            max_distance: (3. * s).next_down(),
                            ..r
                        },
                        QueryBudget::default()
                    )
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(
            scene
                .ray_cast(
                    ray([s, s, 3. * s], [0., 0., -1.], 4. * s),
                    QueryBudget::default()
                )
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn triangle_inexact_source_and_origin_subtraction_never_report_clear() {
    let small = 2f32.powi(-100);
    let edges = scene(&packed_triangle_fixture([
        [small, 0., 0.],
        [1., 0., 0.],
        [0., 1., 0.],
    ]));
    assert!(matches!(
        edges.ray_cast(
            ray([0.25, 0.25, 1.], [0., 0., -1.], 2.),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "triangle source edge subtraction is numerically uncertain"
        ))
    ));
    let origin = scene(&packed_triangle_fixture([
        [1., 0., 0.],
        [2., 0., 0.],
        [1., 1., 0.],
    ]));
    assert!(matches!(
        origin.ray_cast(
            ray([2f64.powi(-60), 0.25, 1.], [0., 0., -1.], 2.),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(
            "triangle ray origin subtraction is numerically uncertain"
        ))
    ));
}

#[test]
fn indexed_triangle_uncertainty_discards_earlier_source_hits_and_keeps_budgets() {
    let eps = f32::from_bits(0x3400_0000);
    let mut blocks = packed_triangle_fixture([[0.; 3], [1.; 3], [1., 1. + eps, 1. - eps]]);
    blocks.push(("bhkRigidBody", body(4)));
    blocks.push(("bhkSphereShape", sphere(0.1)));
    let (_, collision) =
        nif_collision::decode(&container(&blocks), "indexed literal source crossing").unwrap();
    let mut placements: Vec<_> = (1..=8)
        .map(|reference| BodyPlacement {
            reference: ReferenceId(NonZeroU64::new(reference).unwrap()),
            ..placement()
        })
        .collect();
    placements[0].body_block = 3;
    let scene =
        StaticScene::build(&collision, &placements, units(), QueryLimits::default()).unwrap();
    assert_eq!(scene.primitive_count(), 8);
    let q = f64::from_bits(0x3fe2_79a7_4590_331d);
    let d = [q, q, q.next_up()];
    let point = [0.5, 0.5 + f64::from(eps) / 4., 0.5 - f64::from(eps) / 4.];
    let r = ray(std::array::from_fn(|i| point[i] - d[i]), d, 2.);
    let sphere_scene = StaticScene::build(
        &collision,
        &placements[..1],
        units(),
        QueryLimits::default(),
    )
    .unwrap();
    assert_eq!(
        sphere_scene
            .ray_cast(r, QueryBudget::default())
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        scene.ray_cast(r, QueryBudget::default()),
        Err(QueryError::Invalid(_))
    ));
    for budget in [
        QueryBudget {
            primitive_tests: 0,
            ..QueryBudget::default()
        },
        QueryBudget {
            geometry_tests: 0,
            ..QueryBudget::default()
        },
    ] {
        assert!(matches!(
            scene.ray_cast(r, budget),
            Err(QueryError::Budget(_))
        ));
    }
    let mut scaled_placements = placements;
    for p in &mut scaled_placements {
        p.attachment_to_source = Affine {
            rows: [[-2., 0., 0., 0.], [0., 2., 0., 0.], [0., 0., 2., 0.]],
        };
    }
    let scaled = StaticScene::build(
        &collision,
        &scaled_placements,
        units(),
        QueryLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        scaled.ray_cast(
            ray(
                [-2. * r.origin[0], 2. * r.origin[1], 2. * r.origin[2]],
                [-d[0], d[1], d[2]],
                4.
            ),
            QueryBudget::default()
        ),
        Err(QueryError::Invalid(_))
    ));
}

#[test]
#[ignore = "explicit private source-triangle CLI fixture export"]
fn triangle_ray_cli_fixture_export() {
    use std::io::Write;
    let root = std::path::PathBuf::from(std::env::var_os("FALLOUT_TRIANGLE_FIXTURE").unwrap());
    std::fs::create_dir(&root).unwrap();
    let eps = f32::from_bits(0x3400_0000);
    for axis in 0..3 {
        for reverse in [false, true] {
            let mut vertices = [[0.; 3], [1.; 3], [1., 1. + eps, 1. - eps]]
                .map(|p| [p[axis], p[(axis + 1) % 3], p[(axis + 2) % 3]]);
            if reverse {
                vertices.swap(1, 2);
            }
            let name = format!("thin-{axis}-{reverse}.nif");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            file.write_all(&container(&packed_triangle_fixture(vertices)))
                .unwrap();
        }
    }
    let mut mixed = packed_triangle_fixture([[0.; 3], [1.; 3], [1., 1. + eps, 1. - eps]]);
    mixed.push(("bhkRigidBody", body(4)));
    mixed.push(("bhkSphereShape", sphere(0.1)));
    for _ in 0..6 {
        mixed.push(("bhkRigidBody", body(1)));
    }
    for (name, blocks) in [
        ("mixed-indexed.nif", mixed),
        (
            "ordinary.nif",
            packed_triangle_fixture([[0.; 3], [2., 0., 0.], [0., 2., 0.]]),
        ),
        (
            "degenerate.nif",
            packed_triangle_fixture([[0.; 3], [1.; 3], [2.; 3]]),
        ),
        (
            "inexact-edge.nif",
            packed_triangle_fixture([[2f32.powi(-100), 0., 0.], [1., 0., 0.], [0., 1., 0.]]),
        ),
        (
            "inexact-origin.nif",
            packed_triangle_fixture([[1., 0., 0.], [2., 0., 0.], [1., 1., 0.]]),
        ),
        (
            "sphere.nif",
            vec![("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))],
        ),
        (
            "box.nif",
            vec![("bhkRigidBody", body(1)), ("bhkBoxShape", bx())],
        ),
    ] {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join(name))
            .unwrap();
        file.write_all(&container(&blocks)).unwrap();
    }
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
