//! Expand authored quadrant alpha samples into a bounded inspection model.
//! The source fields stay untouched. This model follows the pinned ESM4 reference
//! convention; matching it does not establish New Vegas rendering parity.
use super::Landscape;
use crate::{Error, Result};
use serde::Serialize;

pub const BLEND_MODEL: &str = "esm4-local-u8-residual-base-v1";
pub const SIDE: usize = 17;
pub const SAMPLES: usize = SIDE * SIDE;
const MAX_LAYERS: usize = 256;

#[derive(Debug, Serialize)]
pub struct WeightLayer {
    pub source_layer: usize,
    pub texture_raw: u32,
    pub alpha_missing: bool,
    pub weights: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct Quadrant {
    pub quadrant: u8,
    pub base: Option<WeightLayer>,
    pub overlays: Vec<WeightLayer>,
}

#[derive(Debug, Serialize)]
pub struct BlendMaps {
    pub model: &'static str,
    pub quadrants: Vec<Quadrant>,
    pub clamped_samples: usize,
    pub overfull_vertices: usize,
    pub unapplied_default_layers: usize,
    pub missing_base_quadrants: Vec<u8>,
}

/// Treat each quadrant independently, including its boundary vertices. No neighbor
/// repair, inferred base texture, normalization or layer merging is performed.
pub fn build(land: &Landscape) -> Result<BlendMaps> {
    if land.layers.len() > MAX_LAYERS {
        return Err(Error::Unsupported(
            "terrain blend layer budget exceeded".into(),
        ));
    }
    let mut quadrants: Vec<_> = (0..4)
        .map(|quadrant| Quadrant {
            quadrant,
            base: None,
            overlays: Vec::new(),
        })
        .collect();
    let mut clamped = 0;
    let mut defaults = 0;
    for (index, layer) in land.layers.iter().enumerate() {
        let quadrant = quadrants
            .get_mut(usize::from(layer.quadrant))
            .ok_or_else(|| Error::Resolution("invalid blend quadrant".into()))?;
        defaults += usize::from(layer.texture_raw == 0);
        let mut weights = WeightLayer {
            source_layer: index,
            texture_raw: layer.texture_raw,
            alpha_missing: layer.alpha.is_none(),
            weights: vec![0; SAMPLES],
        };
        match layer.kind.as_str() {
            "BTXT" => {
                if quadrant.base.is_some() || layer.alpha.is_some() {
                    return Err(Error::Unsupported("ambiguous terrain base layer".into()));
                }
                weights.weights.fill(255);
                quadrant.base = Some(weights);
            }
            "ATXT" => {
                // The reference loader checks contiguous, source-ordered indices.
                // Reject ambiguous order rather than silently sorting the source.
                if i32::from(layer.layer) != quadrant.overlays.len() as i32 {
                    return Err(Error::Unsupported(
                        "noncontiguous terrain alpha layers".into(),
                    ));
                }
                let mut seen = [false; SAMPLES];
                if let Some(alpha) = &layer.alpha {
                    if alpha.value.len() > SAMPLES {
                        return Err(Error::Unsupported(
                            "terrain alpha sample budget exceeded".into(),
                        ));
                    }
                    for vertex in &alpha.value {
                        let at = usize::from(vertex.position);
                        if at >= SAMPLES || seen[at] {
                            return Err(Error::Unsupported(
                                "invalid or repeated terrain alpha position".into(),
                            ));
                        }
                        seen[at] = true;
                        let opacity = f32::from_bits(vertex.opacity_bits);
                        if !opacity.is_finite() {
                            return Err(Error::Resolution("nonfinite terrain alpha".into()));
                        }
                        clamped += usize::from(!(0. ..=1.).contains(&opacity));
                        // Multiply in binary32 and truncate toward zero. Clamping
                        // before multiplication avoids overflow for finite extremes.
                        weights.weights[at] = (opacity.clamp(0., 1.) * 255.) as u8;
                    }
                }
                quadrant.overlays.push(weights);
            }
            _ => return Err(Error::Unsupported("unknown terrain layer kind".into())),
        }
    }
    let mut overfull = 0;
    let mut missing = Vec::new();
    for quadrant in &mut quadrants {
        if quadrant.base.is_none() {
            missing.push(quadrant.quadrant);
        }
        for vertex in 0..SAMPLES {
            let total: u32 = quadrant
                .overlays
                .iter()
                .map(|layer| u32::from(layer.weights[vertex]))
                .sum();
            overfull += usize::from(total > 255);
            if let Some(base) = &mut quadrant.base {
                base.weights[vertex] = 255u32.saturating_sub(total) as u8;
            }
        }
    }
    Ok(BlendMaps {
        model: BLEND_MODEL,
        quadrants,
        clamped_samples: clamped,
        overfull_vertices: overfull,
        unapplied_default_layers: defaults,
        missing_base_quadrants: missing,
    })
}

/// Map a quadrant-local vertex to the complete 33-by-33 source height grid.
pub fn source_vertex(quadrant: u8, x: usize, y: usize) -> Option<usize> {
    if quadrant > 3 || x >= SIDE || y >= SIDE {
        return None;
    }
    let start_x = usize::from(quadrant & 1) * 16;
    let start_y = usize::from(quadrant >> 1) * 16;
    Some((start_y + y) * 33 + start_x + x)
}

/// Use the same checkerboard as the full source surface. All quadrant origins
/// have even coordinates, so their local diagonal parity agrees globally.
pub fn indices() -> Vec<u32> {
    let mut indices = Vec::with_capacity(16 * 16 * 6);
    for y in 0..16 {
        for x in 0..16 {
            let a = (y * SIDE + x) as u32;
            let b = a + 1;
            let c = a + SIDE as u32;
            let d = c + 1;
            if (x + y) % 2 == 0 {
                indices.extend([a, b, d, a, d, c]);
            } else {
                indices.extend([a, b, c, b, d, c]);
            }
        }
    }
    indices
}
