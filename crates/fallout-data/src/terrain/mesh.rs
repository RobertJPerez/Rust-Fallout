//! A bounded source surface for inspection. The checkerboard diagonals follow
//! the pinned terrain reference; retail topology and edge-normal repair are open.
use super::{
    Landscape,
    heights::{self, SAMPLE_COUNT, SIDE},
};
use crate::{Error, Result};
use serde::Serialize;

pub const GEOMETRY_MODEL: &str = "esm4-source-grid-checkerboard-positive-z-v1";

#[derive(Debug, Serialize)]
pub struct SurfaceMesh {
    pub model: &'static str,
    pub local_positions: Vec<[f64; 3]>,
    pub normal_bits: Option<Vec<[u32; 3]>>,
    pub colors: Option<Vec<[u8; 3]>>,
    pub indices: Vec<u32>,
    pub hidden_quadrants: u8,
    pub bounds: [[f64; 3]; 2],
}

/// Interpret VNML as signed bytes, then normalize in f64 and narrow once. No
/// neighbor substitution, corner averaging or manufactured up vector is applied.
pub fn normal_bits(raw: [u8; 3]) -> Result<[u32; 3]> {
    let signed = raw.map(|byte| f64::from(byte as i8));
    let length = signed.iter().map(|v| v * v).sum::<f64>().sqrt();
    if length == 0. {
        return Err(Error::Resolution("zero authored terrain normal".into()));
    }
    Ok(signed.map(|value| ((value / length) as f32).to_bits()))
}

/// Four 16-by-16 quad regions share boundary vertices. Their hidden flags affect
/// triangles only; the complete authored vertex grid remains available.
pub fn indices(hidden: u8) -> Result<Vec<u32>> {
    if hidden & !15 != 0 {
        return Err(Error::Unsupported("unknown NV land hide flag bits".into()));
    }
    let mut out = Vec::with_capacity(32 * 32 * 6);
    for y in 0..SIDE - 1 {
        for x in 0..SIDE - 1 {
            let quadrant = usize::from(x >= 16) + 2 * usize::from(y >= 16);
            if hidden & (1 << quadrant) != 0 {
                continue;
            }
            let a = (y * SIDE + x) as u32;
            let b = a + 1;
            let c = a + SIDE as u32;
            let d = c + 1;
            // Positive source Z is the front face. The presentation basis is a
            // rotation, so it preserves winding when Z-up becomes Y-up.
            if (x + y) % 2 == 0 {
                out.extend([a, b, d, a, d, c]);
            } else {
                out.extend([a, b, c, b, d, c]);
            }
        }
    }
    Ok(out)
}

pub fn build(land: &Landscape, hidden: u8) -> Result<SurfaceMesh> {
    let height = land
        .heights
        .as_ref()
        .ok_or_else(|| Error::Unsupported("terrain mesh requires VHGT".into()))?;
    let grid = heights::reconstruct(&height.value)?;
    let normals = land
        .normals
        .as_ref()
        .map(|field| {
            if field.value.len() != SAMPLE_COUNT {
                return Err(Error::Resolution(
                    "terrain normal count differs from height grid".into(),
                ));
            }
            field
                .value
                .iter()
                .copied()
                .map(normal_bits)
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?;
    let colors = land
        .colors
        .as_ref()
        .map(|field| {
            if field.value.len() != SAMPLE_COUNT {
                return Err(Error::Resolution(
                    "terrain color count differs from height grid".into(),
                ));
            }
            Ok(field.value.clone())
        })
        .transpose()?;
    let mut local_positions = Vec::with_capacity(SAMPLE_COUNT);
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let position = grid
                .position([0, 0], x, y)
                .expect("bounded grid coordinates");
            for i in 0..3 {
                min[i] = min[i].min(position[i]);
                max[i] = max[i].max(position[i]);
            }
            local_positions.push(position);
        }
    }
    Ok(SurfaceMesh {
        model: GEOMETRY_MODEL,
        local_positions,
        normal_bits: normals,
        colors,
        indices: indices(hidden)?,
        hidden_quadrants: hidden,
        bounds: [min, max],
    })
}
