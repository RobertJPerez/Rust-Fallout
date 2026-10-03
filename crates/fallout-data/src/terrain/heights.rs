//! Height conversion is separate from source decoding. This model follows the
//! pinned OpenMW ESM4 format convention; retail NV measurements remain pending.
use super::HeightMap;
use crate::{Error, Result};
use serde::Serialize;

pub const SIDE: usize = 33;
pub const SAMPLE_COUNT: usize = SIDE * SIDE;
pub const HEIGHT_MODEL: &str = "esm4-vhgt-f32-row-prefix-scale8-v1";
const CELL_SIZE: f64 = 4096.;
const SPACING: f64 = CELL_SIZE / (SIDE - 1) as f64;

#[derive(Debug, Serialize)]
pub struct HeightGrid {
    pub model: &'static str,
    // Construction validates every sample. Keep storage private so queries and
    // edge comparisons cannot receive a shortened or non-finite grid.
    height_bits: Vec<u32>,
    minimum_bits: u32,
    maximum_bits: u32,
}

impl HeightGrid {
    pub fn bits(&self) -> &[u32] {
        &self.height_bits
    }

    pub fn sample(&self, x: usize, y: usize) -> Option<f32> {
        (x < SIDE && y < SIDE).then(|| f32::from_bits(self.height_bits[y * SIDE + x]))
    }

    /// Source coordinates only. Rebase in f64 before a presentation adapter
    /// narrows to f32; integer cell coordinates can be much larger than a camera.
    pub fn position(&self, cell: [i32; 2], x: usize, y: usize) -> Option<[f64; 3]> {
        self.sample(x, y).map(|height| {
            [
                f64::from(cell[0]) * CELL_SIZE + x as f64 * SPACING,
                f64::from(cell[1]) * CELL_SIZE + y as f64 * SPACING,
                f64::from(height),
            ]
        })
    }
}

/// A row starts relative to the previous row's first sample, not its last one.
/// Each delta addition rounds in binary32 before the final scale by eight. An
/// integer prefix sum or f64 accumulator changes large-offset rounding behavior.
pub fn reconstruct(map: &HeightMap) -> Result<HeightGrid> {
    if map.deltas.len() != SAMPLE_COUNT {
        return Err(Error::Resolution("VHGT requires 1089 height deltas".into()));
    }
    let mut row_start = f32::from_bits(map.offset_bits);
    if !row_start.is_finite() {
        return Err(Error::Resolution("VHGT offset is non-finite".into()));
    }
    let mut height_bits = Vec::with_capacity(SAMPLE_COUNT);
    let mut minimum = f32::MAX;
    let mut maximum = f32::MIN;
    for row in map.deltas.as_chunks::<SIDE>().0 {
        row_start += f32::from(row[0]);
        let mut value = row_start;
        for (x, delta) in row.iter().enumerate() {
            if x != 0 {
                value += f32::from(*delta);
            }
            let height = value * 8.;
            if !height.is_finite() {
                return Err(Error::Resolution(format!(
                    "VHGT reconstruction overflow at sample {}",
                    height_bits.len()
                )));
            }
            // Retain the first occurrence when equal, including signed zero.
            if height < minimum {
                minimum = height;
            }
            if height > maximum {
                maximum = height;
            }
            height_bits.push(height.to_bits());
        }
    }
    Ok(HeightGrid {
        model: HEIGHT_MODEL,
        height_bits,
        minimum_bits: minimum.to_bits(),
        maximum_bits: maximum.to_bits(),
    })
}

#[derive(Debug, Serialize)]
pub struct EdgeMismatch {
    pub sample: usize,
    pub first_bits: u32,
    pub second_bits: u32,
}

#[derive(Debug, Serialize)]
pub struct EdgeComparison {
    pub direction: &'static str,
    pub samples_compared: usize,
    pub exact_bits_equal: bool,
    pub maximum_absolute_difference: f64,
    pub mismatches: Vec<EdgeMismatch>,
}

/// Compare a cardinal neighbor's duplicated boundary samples without changing
/// either surface. The caller must establish a common worldspace first.
pub fn compare_edges(
    first_cell: [i32; 2],
    first: &HeightGrid,
    second_cell: [i32; 2],
    second: &HeightGrid,
) -> Result<EdgeComparison> {
    let delta = [
        i64::from(second_cell[0]) - i64::from(first_cell[0]),
        i64::from(second_cell[1]) - i64::from(first_cell[1]),
    ];
    let (direction, starts, steps) = match delta {
        [1, 0] => ("east", [SIDE - 1, 0], [SIDE, SIDE]),
        [-1, 0] => ("west", [0, SIDE - 1], [SIDE, SIDE]),
        [0, 1] => ("north", [(SIDE - 1) * SIDE, 0], [1, 1]),
        [0, -1] => ("south", [0, (SIDE - 1) * SIDE], [1, 1]),
        _ => {
            return Err(Error::Resolution(
                "height edge requires cardinal neighbor cells".into(),
            ));
        }
    };
    let mut mismatches = Vec::new();
    let mut maximum_absolute_difference = 0f64;
    for sample in 0..SIDE {
        let left = first.height_bits[starts[0] + sample * steps[0]];
        let right = second.height_bits[starts[1] + sample * steps[1]];
        maximum_absolute_difference = maximum_absolute_difference
            .max((f64::from(f32::from_bits(left)) - f64::from(f32::from_bits(right))).abs());
        if left != right {
            mismatches.push(EdgeMismatch {
                sample,
                first_bits: left,
                second_bits: right,
            });
        }
    }
    Ok(EdgeComparison {
        direction,
        samples_compared: SIDE,
        exact_bits_equal: mismatches.is_empty(),
        maximum_absolute_difference,
        mismatches,
    })
}
