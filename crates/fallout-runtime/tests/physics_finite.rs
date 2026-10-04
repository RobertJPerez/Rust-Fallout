//! Original authored words and independently known finite intersections.
use fallout_data::{coordinates::Affine, nif_collision};
use fallout_runtime::{identity::ReferenceId, physics::*};
use std::num::NonZeroU64;

fn words(data: &mut Vec<u8>, values: &[u32]) {
    for v in values {
        data.extend(v.to_le_bytes());
    }
}
fn floats(data: &mut Vec<u8>, values: &[f32]) {
    for v in values {
        data.extend(v.to_le_bytes());
    }
}
fn container(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut data = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut data, &[0x14020007]);
    data.push(1);
    words(&mut data, &[11, blocks.len() as u32, 34]);
    data.extend([0; 3]);
    data.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        words(&mut data, &[name.len() as u32]);
        data.extend(name.as_bytes());
    }
    for i in 0..blocks.len() {
        data.extend((i as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        words(&mut data, &[payload.len() as u32]);
    }
    words(&mut data, &[0, 0, 0]);
    for (_, payload) in blocks {
        data.extend(payload);
    }
    words(&mut data, &[0]);
    data
}
fn body(shape: u32) -> Vec<u8> {
    let mut data = vec![0; 236];
    data[..4].copy_from_slice(&shape.to_le_bytes());
    data[4..8].copy_from_slice(&[5, 0xe7, 0x34, 0x12]);
    data[80..84].copy_from_slice(&1f32.to_le_bytes());
    data
}
fn sphere(radius: f32) -> Vec<u8> {
    let mut data = Vec::new();
    words(&mut data, &[17]);
    floats(&mut data, &[radius]);
    data
}
fn bx() -> Vec<u8> {
    let mut data = sphere(0.25);
    data.extend([0; 8]);
    floats(&mut data, &[1., 2., 3., 0.]);
    data
}
fn capsule(a: [f32; 3], b: [f32; 3], radius: f32) -> Vec<u8> {
    let mut data = sphere(radius);
    data.extend([0; 8]);
    floats(&mut data, &a);
    floats(&mut data, &[radius]);
    floats(&mut data, &b);
    floats(&mut data, &[radius]);
    data
}
fn convex() -> Vec<u8> {
    let mut data = sphere(0.25);
    words(&mut data, &[0, 0, 0x80000000, 0, 0, 0x80000000, 8]);
    for x in [1., 3.] {
        for y in [2., 6.] {
            for z in [-1., 1.] {
                floats(&mut data, &[x, y, z, 0.]);
            }
        }
    }
    words(&mut data, &[6]);
    for p in [
        [-1., 0., 0., 1.],
        [1., 0., 0., -3.],
        [0., -1., 0., 2.],
        [0., 1., 0., -6.],
        [0., 0., -1., -1.],
        [0., 0., 1., -1.],
    ] {
        floats(&mut data, &p);
    }
    data
}
fn triangle(vertices: [[f32; 3]; 3]) -> Vec<(&'static str, Vec<u8>)> {
    let mut shape = Vec::new();
    words(&mut shape, &[0, 0]);
    floats(&mut shape, &[0.125]);
    words(&mut shape, &[0]);
    floats(&mut shape, &[1., 1., 1., 0., 0.125, 1., 1., 1., 0.]);
    words(&mut shape, &[2]);
    let mut data = Vec::new();
    words(&mut data, &[1]);
    for v in [0u16, 1, 2, 0xabcd] {
        data.extend(v.to_le_bytes());
    }
    words(&mut data, &[3]);
    data.push(0);
    for v in vertices {
        floats(&mut data, &v);
    }
    data.extend(1u16.to_le_bytes());
    data.extend([7, 0x81, 0x34, 0x12]);
    words(&mut data, &[3, 42]);
    vec![
        ("bhkRigidBody", body(1)),
        ("bhkPackedNiTriStripsShape", shape),
        ("hkPackedNiTriStripsData", data),
    ]
}
fn units() -> EngineeringUnits {
    EngineeringUnits {
        havok_to_source: 1.,
        source_to_query: 1.,
        transform_tolerance: 1e-6,
    }
}
fn placement(body_block: u32) -> BodyPlacement {
    BodyPlacement {
        reference: ReferenceId(NonZeroU64::new(1).unwrap()),
        source_sha256: [9; 32],
        body_block,
        attachment_to_source: Affine {
            rows: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        },
    }
}
fn prepared(blocks: &[(&str, Vec<u8>)], placements: &[BodyPlacement]) -> StaticScene {
    let (_, source) =
        nif_collision::decode(&container(blocks), "literal finite authored geometry").unwrap();
    StaticScene::build(&source, placements, units(), QueryLimits::default()).unwrap()
}
fn scene(name: &str, payload: Vec<u8>) -> StaticScene {
    prepared(
        &[("bhkRigidBody", body(1)), (name, payload)],
        &[placement(0)],
    )
}
fn segment(start: [f64; 3], end: [f64; 3]) -> Segment {
    Segment { start, end }
}

fn exact(bounds: [f64; 2], value: f64) {
    assert_eq!(bounds, [value, value]);
}
fn metadata(hit: &Hit, position: [f64; 3], shell: f32) {
    assert_eq!(hit.position, position);
    assert_eq!(hit.material, 17);
    assert_eq!(hit.body_filter.flags_and_parts, 0xe7);
    assert_eq!(hit.body_filter.group, 0x1234);
    assert_eq!(hit.authored_shell_radius, shell);
}

#[test]
fn segment_original_axis_near_end_reverse_and_zero_length() {
    let scene = scene("bhkSphereShape", sphere(1.));
    for (input, entry, exit, position) in [
        (segment([-3., 0., 0.], [1., 0., 0.]), 0.5, 1., [-1., 0., 0.]),
        (segment([1., 0., 0.], [-3., 0., 0.]), 0., 0.5, [1., 0., 0.]),
        (segment([-2., 0., 0.], [-1., 0., 0.]), 1., 1., [-1., 0., 0.]),
        (segment([-0., 0., 0.], [-0., 0., 0.]), 0., 1., [-0., 0., 0.]),
    ] {
        let report = scene.segment_cast(input, Default::default()).unwrap();
        assert_eq!(report.results.len(), 1);
        let hit = &report.results[0];
        exact(hit.entry_parameter_bounds, entry);
        exact(hit.exit_parameter_bounds, exit);
        metadata(&hit.provenance, position, 0.);
        assert!(!scene.faithful_ready());
    }
    for input in [
        segment([-2., 0., 0.], [-1f64.next_up(), 0., 0.]),
        segment([-2., 1f64.next_up(), 0.], [2., 1f64.next_up(), 0.]),
        segment([2., 0., 0.], [2., 0., 0.]),
    ] {
        assert!(
            scene
                .segment_cast(input, Default::default())
                .unwrap()
                .results
                .is_empty()
        );
    }
}
#[test]
fn segment_original_skew_point_requires_line_and_body_witnesses() {
    let scene = scene("bhkSphereShape", sphere(1.));
    let input = segment([-3., 0.5, 0.], [3., 0.5, 0.]);
    let report = scene.segment_cast(input, Default::default()).unwrap();
    assert_eq!(report.results.len(), 1);
    let hit = &report.results[0];
    assert_eq!(hit.provenance.position[1..], [0.5, 0.]);
    assert_eq!(hit.provenance.position[0], 6. * hit.parameter - 3.);
    assert!(hit.provenance.position[0].abs() < 1.);
    assert!(hit.entry_parameter_bounds[0] < 0.5 && hit.exit_parameter_bounds[1] > 0.5);
    // A rounded subtraction is not used as an authoritative surrogate. Both
    // endpoints of this original axis segment are in the convex source ball.
    let lost = segment([2f64.powi(-60), 0., 0.], [1., 0., 0.]);
    let report = scene.segment_cast(lost, Default::default()).unwrap();
    exact(report.results[0].entry_parameter_bounds, 0.);
    exact(report.results[0].exit_parameter_bounds, 1.);
    assert_eq!(report.results[0].provenance.position, lost.start);
    let tiny = f64::from_bits(1);
    assert_eq!(
        scene
            .segment_cast(segment([0.; 3], [tiny, 0., 0.]), Default::default())
            .unwrap()
            .results[0]
            .provenance
            .position,
        [0.; 3]
    );
}
#[test]
fn segment_all_authored_leaf_kinds_keep_closed_geometry_and_metadata() {
    for (name, payload, input, position, shell) in [
        (
            "bhkBoxShape",
            bx(),
            segment([-3., 0., 0.], [1., 0., 0.]),
            [-1., 0., 0.],
            0.25,
        ),
        (
            "bhkConvexVerticesShape",
            convex(),
            segment([0., 4., 0.], [2., 4., 0.]),
            [1., 4., 0.],
            0.25,
        ),
        (
            "bhkCapsuleShape",
            capsule([-1., 0., 0.], [1., 0., 0.], 1.),
            segment([-3., 0., 0.], [1., 0., 0.]),
            [-2., 0., 0.],
            0.,
        ),
        (
            "bhkCapsuleShape",
            capsule([0.; 3], [4., 4., 0.], 1.),
            segment([2., 2., -3.], [2., 2., 1.]),
            [2., 2., -1.],
            0.,
        ),
    ] {
        let scene = scene(name, payload);
        let report = scene.segment_cast(input, Default::default()).unwrap();
        assert_eq!(report.results.len(), 1);
        metadata(&report.results[0].provenance, position, shell);
    }
    let scene = prepared(
        &triangle([[0.; 3], [2., 0., 0.], [0., 2., 0.]]),
        &[placement(0)],
    );
    let report = scene
        .segment_cast(segment([0.5, 0.5, -1.], [0.5, 0.5, 1.]), Default::default())
        .unwrap();
    let hit = &report.results[0];
    exact(hit.entry_parameter_bounds, 0.5);
    exact(hit.exit_parameter_bounds, 0.5);
    assert_eq!(hit.provenance.position, [0.5, 0.5, 0.]);
    assert_eq!(hit.provenance.material, 42);
    assert_eq!(hit.provenance.welding, Some(0xabcd));
    assert_eq!(hit.provenance.shape_filter.unwrap().flags_and_parts, 0x81);
    assert_eq!(hit.provenance.authored_shell_radius, 0.125);
    assert!(
        scene
            .segment_cast(segment([3., 3., -1.], [3., 3., 1.]), Default::default())
            .unwrap()
            .results
            .is_empty()
    );
}
#[test]
fn segment_coplanar_closed_and_degenerate_triangle_queries() {
    let scene = prepared(
        &triangle([[0.; 3], [2., 0., 0.], [0., 2., 0.]]),
        &[placement(0)],
    );
    let report = scene
        .segment_cast(segment([-1., 0.5, 0.], [2., 0.5, 0.]), Default::default())
        .unwrap();
    assert_eq!(report.results.len(), 1);
    let hit = &report.results[0];
    assert_eq!(hit.provenance.position[1..], [0.5, 0.]);
    assert!((0. ..=1.5).contains(&hit.provenance.position[0]));
    assert!(hit.entry_parameter_bounds[0] <= 1. / 3. && hit.entry_parameter_bounds[1] >= 1. / 3.);
    assert!(hit.exit_parameter_bounds[0] <= 5. / 6. && hit.exit_parameter_bounds[1] >= 5. / 6.);
    assert_eq!(
        scene
            .segment_cast(segment([0.; 3], [0.; 3]), Default::default())
            .unwrap()
            .results
            .len(),
        1
    );
    let line = prepared(
        &triangle([[-1., 0., 0.], [1., 0., 0.], [0.; 3]]),
        &[placement(0)],
    );
    let report = line
        .segment_cast(segment([-2., 0., 0.], [2., 0., 0.]), Default::default())
        .unwrap();
    exact(report.results[0].entry_parameter_bounds, 0.25);
    exact(report.results[0].exit_parameter_bounds, 0.75);
    assert_eq!(report.results[0].provenance.position, [-1., 0., 0.]);
    assert!(
        line.segment_cast(
            segment([-2., f64::from_bits(1), 0.], [2., f64::from_bits(1), 0.]),
            Default::default()
        )
        .is_err()
    );
}
#[test]
fn segment_exact_and_generic_source_transforms_are_not_renormalized() {
    let blocks = [("bhkRigidBody", body(1)), ("bhkSphereShape", sphere(1.))];
    let mut p = placement(0);
    p.attachment_to_source.rows = [[-2., 0., 0., 10.], [0., 2., 0., 0.], [0., 0., 2., 0.]];
    let scene = prepared(&blocks, &[p]);
    let hit = scene
        .segment_cast(segment([4., 0., 0.], [10., 0., 0.]), Default::default())
        .unwrap();
    assert!(matches!(
        hit.results[0].source_core,
        CoreGeometry::Sphere {
            radius_binary32: 0x3f800000
        }
    ));
    let mut transformed = body(1);
    for (i, value) in [0f32, 0., 0.6, 0.8].iter().enumerate() {
        transformed[68 + 4 * i..72 + 4 * i].copy_from_slice(&value.to_le_bytes());
    }
    let scene = prepared(
        &[
            ("bhkRigidBodyT", transformed),
            ("bhkSphereShape", sphere(1.)),
        ],
        &[placement(0)],
    );
    let hit = scene
        .segment_cast(segment([-3., 0., 0.], [3., 0., 0.]), Default::default())
        .unwrap();
    assert_eq!(hit.results[0].provenance.position, [0.; 3]);
    assert_eq!(hit.results[0].parameter, 0.5);
}





#[test]
fn finite_global_exact_and_one_under_work_rows_and_retained_capacity() {
    let blocks = [
        ("bhkRigidBody", body(2)),
        ("bhkRigidBody", body(2)),
        ("bhkSphereShape", sphere(1.)),
    ];
    let scene = prepared(&blocks, &[placement(0), placement(1)]);
    let input = segment([-3., 0., 0.], [1., 0., 0.]);
    let report = scene.segment_cast(input, Default::default()).unwrap();
    assert_eq!(report.results.len(), 2);
    assert!(report.results[0].provenance.source < report.results[1].provenance.source);
    let exact_limits = FiniteQueryLimits {
        admission_tests: 2,
        primitive_tests: 2,
        geometry_tests: 0,
        predicate_tests: 16384,
        rows: 2,
        retained_bytes: report.work.retained_bytes,
    };
    assert_eq!(
        scene
            .segment_cast(input, exact_limits)
            .unwrap()
            .results
            .len(),
        2
    );
    for reduced in [
        FiniteQueryLimits {
            admission_tests: 1,
            ..exact_limits
        },
        FiniteQueryLimits {
            primitive_tests: 1,
            ..exact_limits
        },
        FiniteQueryLimits {
            predicate_tests: 16383,
            ..exact_limits
        },
        FiniteQueryLimits {
            rows: 1,
            ..exact_limits
        },
        FiniteQueryLimits {
            retained_bytes: exact_limits.retained_bytes - 1,
            ..exact_limits
        },
    ] {
        assert!(matches!(
            scene.segment_cast(input, reduced),
            Err(QueryError::Budget(_))
        ));
    }
}
#[test]
fn finite_source_transform_arithmetic_refusal_and_ceilings_are_atomic() {
    let mut p = placement(1);
    p.attachment_to_source.rows = [[1., 0., 0., 1.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
    // Source body translation minsub-f32 disappears when the attachment adds1.
    let mut shifted = body(2);
    shifted[52..56].copy_from_slice(&f32::from_bits(1).to_le_bytes());
    let scene = prepared(
        &[
            ("bhkRigidBody", body(2)),
            ("bhkRigidBodyT", shifted),
            ("bhkSphereShape", sphere(1.)),
        ],
        &[placement(0), p],
    );
    assert!(matches!(
        scene.segment_cast(segment([-3., 0., 0.], [1., 0., 0.]), Default::default()),
        Err(QueryError::Unsupported { .. })
    ));
    assert!(
        scene
            .segment_cast(
                segment([0.; 3], [1.; 3]),
                FiniteQueryLimits {
                    rows: 10001,
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn segment_late_unrepresentable_triangle_intersection_refuses_whole_result() {
    let mut blocks = triangle([[0.; 3], [1., 0., 0.], [0., 1., 0.]]);
    blocks[0] = ("bhkRigidBody", body(1));
    blocks.push(("bhkRigidBody", body(4)));
    blocks.push(("bhkSphereShape", sphere(4.)));
    let scene = prepared(&blocks, &[placement(3), placement(0)]);
    // The source plane needs exact t=1/3. No binary64 t has z=-1+3t=0.
    // The earlier sphere is a valid hit, but cannot make this report usable.
    assert!(
        scene
            .segment_cast(
                segment([0.25, 0.25, -1.], [0.25, 0.25, 2.]),
                Default::default()
            )
            .is_err()
    );
}

#[test]
fn finite_geometry_work_covers_missed_triangles_and_numeric_domains() {
    let tri = prepared(
        &triangle([[0.; 3], [2., 0., 0.], [0., 2., 0.]]),
        &[placement(0)],
    );
    let limits = FiniteQueryLimits {
        geometry_tests: 0,
        ..Default::default()
    };
    assert!(matches!(
        tri.segment_cast(segment([100., 100., -1.], [100., 100., 1.]), limits),
        Err(QueryError::Budget(_))
    ));
    let scene = scene("bhkSphereShape", sphere(1.));
    for input in [
        segment([f64::INFINITY, 0., 0.], [0.; 3]),
        segment([f64::NAN, 0., 0.], [0.; 3]),
        segment([-1e51, 0., 0.], [1e51, 0., 0.]),
    ] {
        assert!(scene.segment_cast(input, Default::default()).is_err());
    }
}

#[test]
fn segment_nested_shape_transform_retains_source_axes_and_units() {
    let mut transformed = Vec::new();
    words(&mut transformed, &[2, 0]);
    floats(&mut transformed, &[0.]);
    transformed.extend([0; 8]);
    for column in [
        [0., 2., 0., 0.],
        [-2., 0., 0., 0.],
        [0., 0., 2., 0.],
        [10., 0., 0., 1.],
    ] {
        floats(&mut transformed, &column);
    }
    let blocks = [
        ("bhkRigidBody", body(1)),
        ("bhkTransformShape", transformed),
        ("bhkBoxShape", bx()),
    ];
    let (_, source) =
        nif_collision::decode(&container(&blocks), "finite rotated box words").unwrap();
    let explicit = EngineeringUnits {
        havok_to_source: 2.,
        source_to_query: 0.5,
        transform_tolerance: 1e-6,
    };
    let scene = StaticScene::build(&source, &[placement(0)], explicit, Default::default()).unwrap();
    let report = scene
        .segment_cast(segment([0., 0., 0.], [10., 0., 0.]), Default::default())
        .unwrap();
    assert_eq!(report.results[0].provenance.source.shape_block, 2);
    assert!(matches!(
        report.results[0].source_core,
        CoreGeometry::Box {
            half_extents_binary32: [0x3f800000, 0x40000000, 0x40400000]
        }
    ));
    // Composition keeps the declared placement translation and source axes:
    // output(1/2) * havok(2) * local => center10, world X half-extent4.
    assert_eq!(report.results[0].provenance.position[1..], [0., 0.]);
    assert!((6. ..=10.).contains(&report.results[0].provenance.position[0]));
    assert!(
        report.results[0].entry_parameter_bounds[0] <= 0.6
            && report.results[0].entry_parameter_bounds[1] >= 0.6
    );
}
