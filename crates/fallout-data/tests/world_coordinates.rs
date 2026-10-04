use fallout_data::{
    coordinates::{self, Affine},
    world::Transform,
};

#[test]
fn finite_extreme_rebasing_refuses_subtraction_and_narrowing_overflow() {
    let maximum = f64::from(f32::MAX);
    let over = f64::from_bits(maximum.to_bits() + 1);
    assert_eq!(
        coordinates::try_source_to_view_f32([maximum, -maximum, maximum], [0.; 3]).unwrap(),
        [f32::MAX; 3]
    );
    for value in [over, -over] {
        assert!(coordinates::try_source_to_view_f32([value, 0., 0.], [0.; 3]).is_err());
        assert!(coordinates::try_source_to_view_f32([0.; 3], [value, 0., 0.]).is_err());
    }
    assert!(coordinates::try_source_to_view([f64::MAX, 0., 0.], [-f64::MAX, 0., 0.]).is_err());
    assert!(coordinates::try_source_to_view([0., -f64::MAX, 0.], [0., f64::MAX, 0.]).is_err());
    // Huge absolute coordinates are valid when rebasing leaves a finite delta.
    let next = f64::from_bits(maximum.to_bits() - 1);
    assert_eq!(
        coordinates::try_source_to_view_f32([maximum, next, maximum], [maximum; 3]).unwrap(),
        [0., 0., (maximum - next) as f32]
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for axis in 0..3 {
            let mut point = [0.; 3];
            point[axis] = invalid;
            assert!(coordinates::try_source_to_view(point, [0.; 3]).is_err());
            assert!(coordinates::try_source_to_view([0.; 3], point).is_err());
        }
    }
}

#[test]
fn checked_affine_matches_existing_source_convention_and_refuses_bad_products() {
    let affine = Affine::nv_reference(
        &Transform {
            position: [10., 20., 30.],
            rotation: [0., 0., std::f32::consts::FRAC_PI_2],
        },
        2.,
    )
    .unwrap();
    let point = [1., 2., 3.];
    assert_eq!(affine.try_point(point).unwrap(), affine.point(point));
    assert_eq!(
        affine.try_relative_view([1., 2., 3.]).unwrap().rows,
        affine.relative_view([1., 2., 3.]).rows
    );
    assert_eq!(
        affine.try_relative_view_f32([1., 2., 3.]).unwrap(),
        affine
            .relative_view([1., 2., 3.])
            .rows
            .map(|row| row.map(|v| v as f32))
    );
    let huge = Affine {
        rows: [[f64::MAX, 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
    };
    assert!(huge.try_point([2., 0., 0.]).is_err());
    assert!(huge.try_relative_view_f32([0.; 3]).is_err());
    let maximum = f64::from(f32::MAX);
    let mut translated = Affine {
        rows: [
            [1., 0., 0., maximum],
            [0., 1., 0., maximum],
            [0., 0., 1., maximum],
        ],
    };
    assert_eq!(
        translated.try_relative_view_f32([maximum; 3]).unwrap(),
        [[1., 0., 0., 0.], [0., 1., -0., 0.], [-0., -0., 1., -0.]]
    );
    assert!(translated.try_relative_view_f32([-maximum; 3]).is_err());
    translated.rows[0][3] = f64::MAX;
    assert!(translated.try_relative_view([-f64::MAX, 0., 0.]).is_err());
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(affine.try_point([invalid, 0., 0.]).is_err());
        assert!(affine.try_relative_view([invalid, 0., 0.]).is_err());
        let mut bad = affine;
        bad.rows[1][2] = invalid;
        assert!(bad.try_point([0.; 3]).is_err());
        assert!(bad.try_relative_view([0.; 3]).is_err());
    }
}

#[test]
fn extreme_cell_boundary_samples_rebase_before_f32_narrowing() {
    use fallout_data::terrain::{self, HeightMap};
    let grid = terrain::heights::reconstruct(&HeightMap {
        offset_bits: 0f32.to_bits(),
        deltas: vec![0; terrain::heights::SAMPLE_COUNT],
        unused: [0; 3],
    })
    .unwrap();
    for cell in [[i32::MIN, i32::MAX], [i32::MAX, i32::MIN]] {
        let origin = grid.position(cell, 31, 31).unwrap();
        let corner = grid.position(cell, 32, 32).unwrap();
        // Existing pinned height-model spacing; no measured retail unit claim.
        assert_eq!(
            coordinates::try_source_to_view_f32(corner, origin).unwrap(),
            [128., 0., -128.]
        );
        assert!(grid.position(cell, 33, 32).is_none());
    }
    let edge = grid.position([i32::MAX, 0], 32, 0).unwrap();
    assert_eq!(edge[0], (f64::from(i32::MAX) + 1.) * 4096.);
    let zero = coordinates::try_source_to_view_f32(edge, edge).unwrap();
    assert!(zero.into_iter().all(|value| value == 0.));
}
