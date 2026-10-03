//! Analytic fixtures cover integration order, binary32 rounding and duplicated
//! edges. These are original fixtures, not measured retail terrain behavior.
use fallout_data::terrain::{
    HeightMap,
    heights::{self, HeightGrid, SAMPLE_COUNT, SIDE},
};

fn map(offset: f32) -> HeightMap {
    HeightMap {
        offset_bits: offset.to_bits(),
        deltas: vec![0; SAMPLE_COUNT],
        unused: [17, 31, 255],
    }
}

fn plane(cell: [i32; 2]) -> HeightGrid {
    let mut input = map((cell[0] * 64 + cell[1] * 96) as f32);
    for y in 0..SIDE {
        input.deltas[y * SIDE] = if y == 0 { 0 } else { 3 };
        for x in 1..SIDE {
            input.deltas[y * SIDE + x] = 2;
        }
    }
    heights::reconstruct(&input).unwrap()
}

#[test]
fn rows_restart_at_previous_first_sample_and_apply_initial_delta() {
    let mut input = map(0.5);
    input.deltas[0] = 2;
    input.deltas[1] = 1;
    input.deltas[2] = -2;
    input.deltas[32] = 127;
    input.deltas[33] = 3;
    input.deltas[34] = -1;
    let grid = heights::reconstruct(&input).unwrap();
    assert_eq!(grid.sample(0, 0), Some(20.));
    assert_eq!(grid.sample(1, 0), Some(28.));
    assert_eq!(grid.sample(2, 0), Some(12.));
    assert_eq!(grid.sample(0, 1), Some(44.));
    assert_eq!(grid.sample(1, 1), Some(36.));
    assert_eq!(grid.sample(0, 32), Some(44.));
    assert_eq!(input.unused, [17, 31, 255]);
}

#[test]
fn signed_delta_extremes_and_fractional_negative_offset_are_exact() {
    let mut input = map(-0.25);
    input.deltas[0] = -128;
    input.deltas[1] = 127;
    let grid = heights::reconstruct(&input).unwrap();
    assert_eq!(grid.sample(0, 0), Some(-1026.));
    assert_eq!(grid.sample(1, 0), Some(-10.));
    assert_eq!(grid.sample(0, 1), Some(-1026.));
}

#[test]
fn additions_round_in_binary32_before_scaling() {
    let mut input = map(16_777_216.);
    input.deltas.fill(1);
    let grid = heights::reconstruct(&input).unwrap();
    assert!(
        grid.bits()
            .iter()
            .all(|bits| *bits == 134_217_728f32.to_bits())
    );
    // Combining the two increments first would produce a different answer.
    assert_ne!((16_777_216f32 + 2.) * 8., grid.sample(1, 0).unwrap());
}

#[test]
fn invalid_counts_offsets_and_scaled_overflow_fail() {
    for count in [0, SAMPLE_COUNT - 1, SAMPLE_COUNT + 1] {
        let mut input = map(0.);
        input.deltas.resize(count, 0);
        assert!(heights::reconstruct(&input).is_err());
    }
    for offset in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        -f32::MAX,
    ] {
        assert!(heights::reconstruct(&map(offset)).is_err());
    }
}

#[test]
fn queries_are_bounded_and_large_signed_cells_keep_vertex_spacing() {
    let grid = plane([0, 0]);
    assert!(grid.sample(SIDE, 0).is_none());
    assert!(grid.position([0, 0], 0, usize::MAX).is_none());
    let cell = [i32::MAX, i32::MIN];
    let first = grid.position(cell, 0, 0).unwrap();
    let next = grid.position(cell, 1, 0).unwrap();
    assert_eq!(next[0] - first[0], 128.);
    assert_eq!(first[1], f64::from(i32::MIN) * 4096.);
    assert_eq!(first[2], 0.);
    assert_eq!(
        grid.position([-4, 7], 32, 32).unwrap(),
        [-12288., 32768., 1280.]
    );
}

#[test]
fn all_cardinal_edges_match_the_same_analytic_plane() {
    let cell = [-4, 7];
    let first = plane(cell);
    for (neighbor, direction) in [
        ([-3, 7], "east"),
        ([-5, 7], "west"),
        ([-4, 8], "north"),
        ([-4, 6], "south"),
    ] {
        let result = heights::compare_edges(cell, &first, neighbor, &plane(neighbor)).unwrap();
        assert_eq!(result.direction, direction);
        assert_eq!(result.samples_compared, 33);
        assert!(result.exact_bits_equal && result.mismatches.is_empty());
        assert_eq!(result.maximum_absolute_difference, 0.);
    }
}

#[test]
fn a_changed_corner_is_reported_without_stitching_either_surface() {
    let mut input = map(0.);
    input.deltas[SAMPLE_COUNT - 1] = 1;
    let left = heights::reconstruct(&input).unwrap();
    let right = heights::reconstruct(&map(0.)).unwrap();
    let result = heights::compare_edges([0, 0], &left, [1, 0], &right).unwrap();
    assert!(!result.exact_bits_equal);
    assert_eq!(result.maximum_absolute_difference, 8.);
    assert_eq!(result.mismatches.len(), 1);
    assert_eq!(result.mismatches[0].sample, 32);
    assert_eq!(left.sample(32, 32), Some(8.));
    assert_eq!(right.sample(0, 32), Some(0.));
}

#[test]
fn non_neighbors_and_extreme_coordinate_differences_are_rejected() {
    let grid = plane([0, 0]);
    for other in [[0, 0], [1, 1], [2, 0], [i32::MAX, i32::MIN]] {
        assert!(heights::compare_edges([0, 0], &grid, other, &grid).is_err());
    }
    assert!(heights::compare_edges([i32::MIN, 0], &grid, [i32::MAX, 0], &grid).is_err());
}

#[test]
fn deterministic_delta_fields_match_closed_form_integer_paths() {
    let mut state = 0x2b7a_148du32;
    for _ in 0..64 {
        let mut input = map(-123.25);
        for delta in &mut input.deltas {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *delta = state as i8;
        }
        let grid = heights::reconstruct(&input).unwrap();
        for y in 0..SIDE {
            for x in 0..SIDE {
                let vertical: i32 = (0..=y).map(|r| i32::from(input.deltas[r * SIDE])).sum();
                let horizontal: i32 = (1..=x).map(|c| i32::from(input.deltas[y * SIDE + c])).sum();
                assert_eq!(
                    grid.sample(x, y),
                    Some((-123.25 + (vertical + horizontal) as f32) * 8.)
                );
            }
        }
    }
}
