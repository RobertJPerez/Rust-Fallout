//! Synthetic GPU checks, kept separate from the archived game scene. Every square
//! has a known background and a center pixel derived from the source render rules.
use crate::{
    material::Raster,
    model::{Model, Part, Result},
    scene::{Instance, Prepared},
};
use bevy::{
    asset::RenderAssetUsages,
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use serde::Serialize;

const SOURCE: [f32; 4] = [0.5, 0.25, 0.125, 0.5];
const BACKGROUND: [f32; 4] = [0.16, 0.36, 0.64, 0.75];
const THRESHOLD: u8 = 128;
const TERRAIN_PALETTE: [[u8; 4]; 3] = [[255, 0, 0, 255], [0, 0, 255, 255], [0, 255, 0, 255]];

#[derive(Clone, Serialize)]
pub struct Case {
    name: String,
    pixel: [u32; 2],
    raster: Raster,
    source_rgba: [f32; 4],
    reverse_winding: bool,
    source_z: f32,
    expected_linear_rgb: [f32; 3],
    expected_srgb8: [u8; 3],
    measured_srgb8: Option<[u8; 3]>,
    passed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrain_weights: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    terrain_palette_rgba8: Option<Vec<[u8; 4]>>,
}

#[derive(Clone, Resource, Serialize)]
pub struct Report {
    schema_version: u32,
    fixture: &'static str,
    source_assets_used: bool,
    dimensions: [u32; 2],
    byte_tolerance: u8,
    tolerance_reason: &'static str,
    pub cases: Vec<Case>,
    all_passed: Option<bool>,
    retail_parity_accepted: bool,
}

fn srgb8(linear: f32) -> u8 {
    let value = linear.clamp(0., 1.);
    let encoded = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    };
    (encoded * 255.).round() as u8
}

impl Report {
    pub fn verify(&mut self, image: &image::RgbImage) -> Result<()> {
        if image.dimensions() != (1280, 900) {
            return Err("material fixture captured at the wrong resolution".into());
        }
        let mut failures = Vec::new();
        for case in &mut self.cases {
            let actual = image.get_pixel(case.pixel[0], case.pixel[1]).0;
            let passed = actual
                .iter()
                .zip(case.expected_srgb8)
                .all(|(a, e)| a.abs_diff(e) <= self.byte_tolerance);
            case.measured_srgb8 = Some(actual);
            case.passed = Some(passed);
            if !passed {
                failures.push(format!(
                    "{}: expected {:?}, measured {:?}",
                    case.name, case.expected_srgb8, actual
                ));
            }
        }
        self.all_passed = Some(failures.is_empty());
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

fn quad(center: Vec3, size: Vec2, color: [f32; 4], raster: Raster, reverse: bool) -> Part {
    let h = size * 0.5;
    let positions = [
        center + Vec3::new(-h.x, -h.y, 0.),
        center + Vec3::new(h.x, -h.y, 0.),
        center + Vec3::new(h.x, h.y, 0.),
        center + Vec3::new(-h.x, h.y, 0.),
    ];
    let indices = if reverse {
        vec![0, 2, 1, 0, 3, 2]
    } else {
        vec![0, 1, 2, 0, 2, 3]
    };
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        positions.map(|v| v.to_array()).to_vec(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0., 0.]; 4])
    .with_inserted_indices(Indices::U32(indices));
    Part {
        mesh,
        texture: None,
        color: Color::linear_rgba(color[0], color[1], color[2], color[3]),
        raster,
    }
}

struct Board {
    parts: Vec<Part>,
    cases: Vec<Case>,
}

impl Board {
    fn add(
        &mut self,
        name: String,
        color: [f32; 4],
        raster: Raster,
        reverse: bool,
        z: f32,
        expected: [f32; 3],
    ) -> Result<Vec3> {
        raster.validate()?;
        let index = self.cases.len();
        if index >= 64 {
            return Err("synthetic material fixture exceeds its grid".into());
        }
        let pixel = [80 + (index % 8) as u32 * 160, 55 + (index / 8) as u32 * 110];
        let center = Vec3::new(pixel[0] as f32 - 640., 450. - pixel[1] as f32, 0.);
        self.parts.push(quad(
            center,
            Vec2::new(124., 80.),
            BACKGROUND,
            default(),
            false,
        ));
        self.parts.push(quad(
            center + Vec3::Z * z,
            Vec2::new(80., 50.),
            color,
            raster,
            reverse,
        ));
        self.cases.push(Case {
            name,
            pixel,
            raster,
            source_rgba: color,
            reverse_winding: reverse,
            source_z: z,
            expected_linear_rgb: expected,
            expected_srgb8: expected.map(srgb8),
            measured_srgb8: None,
            passed: None,
            terrain_weights: None,
            terrain_palette_rgba8: None,
        });
        Ok(center)
    }
}

pub fn prepare() -> Result<(Prepared, Report)> {
    let mut board = Board {
        parts: vec![],
        cases: vec![],
    };
    // Rows are the specified truth table, rather than a second implementation of
    // the shader's comparison switch. Columns are below, equal and above 128/255.
    let outcomes = [
        [true, true, true],
        [true, false, false],
        [false, true, false],
        [true, true, false],
        [false, false, true],
        [true, false, true],
        [false, true, true],
        [false, false, false],
    ];
    for (function, row) in outcomes.iter().enumerate() {
        for (column, accepted) in row.iter().enumerate() {
            let alpha = f32::from(THRESHOLD) / 255. + (column as f32 - 1.) * 0.1;
            let color = [SOURCE[0], SOURCE[1], SOURCE[2], alpha];
            board.add(
                format!(
                    "alpha-test-{function}-{}",
                    ["below", "equal", "above"][column]
                ),
                color,
                Raster {
                    alpha_flags: 0x200 | (function as u16) << 10,
                    alpha_threshold: THRESHOLD,
                    ..default()
                },
                false,
                1.,
                if *accepted {
                    SOURCE[..3].try_into()?
                } else {
                    BACKGROUND[..3].try_into()?
                },
            )?;
        }
    }
    // Fixed numeric factors for the known source and destination colors. Keep
    // these independent of the adapter's mapping to wgpu BlendFactor variants.
    let factors = [
        [1.; 3],
        [0.; 3],
        [0.5, 0.25, 0.125],
        [0.5, 0.75, 0.875],
        [0.16, 0.36, 0.64],
        [0.84, 0.64, 0.36],
        [0.5; 3],
        [0.5; 3],
        [0.75; 3],
        [0.25; 3],
        [0.25; 3],
    ];
    for (factor, values) in factors.iter().enumerate() {
        for source in [true, false] {
            let expected = std::array::from_fn(|i| {
                if source {
                    SOURCE[i] * values[i]
                } else {
                    BACKGROUND[i] * values[i]
                }
            });
            let (src, dst) = if source {
                (factor as u16, 1)
            } else {
                (1, factor as u16)
            };
            board.add(
                format!(
                    "blend-{}-factor-{factor}",
                    if source { "source" } else { "destination" }
                ),
                SOURCE,
                Raster {
                    alpha_flags: 1 | src << 1 | dst << 5,
                    ..default()
                },
                false,
                1.,
                expected,
            )?;
        }
    }
    for (name, flags, expected) in [
        ("source-alpha", 0x10ed, [0.33, 0.305, 0.3825]),
        ("darkening", 0x1043, [0.08, 0.09, 0.08]),
        ("additive", 0x100d, [0.41, 0.485, 0.7025]),
    ] {
        board.add(
            name.into(),
            SOURCE,
            Raster {
                alpha_flags: flags,
                ..default()
            },
            false,
            1.,
            expected,
        )?;
    }
    for (column, accepted) in [false, false, true].iter().enumerate() {
        let alpha = f32::from(THRESHOLD) / 255. + (column as f32 - 1.) * 0.1;
        let expected = std::array::from_fn(|i| {
            if *accepted {
                SOURCE[i] * alpha + BACKGROUND[i] * (1. - alpha)
            } else {
                BACKGROUND[i]
            }
        });
        board.add(
            format!(
                "blend-and-test-greater-{}",
                ["below", "equal", "above"][column]
            ),
            [SOURCE[0], SOURCE[1], SOURCE[2], alpha],
            Raster {
                alpha_flags: 0x12ed,
                alpha_threshold: THRESHOLD,
                ..default()
            },
            false,
            1.,
            expected,
        )?;
    }
    for mode in 1..=3 {
        for reverse in [false, true] {
            let accepted = mode == 3 || (mode == 2) == reverse;
            board.add(
                format!("culling-mode-{mode}-{}", if reverse { "cw" } else { "ccw" }),
                SOURCE,
                Raster {
                    draw_mode: mode,
                    ..default()
                },
                reverse,
                1.,
                if accepted {
                    SOURCE[..3].try_into()?
                } else {
                    BACKGROUND[..3].try_into()?
                },
            )?;
        }
    }
    for depth_test in [true, false] {
        board.add(
            format!("depth-test-{depth_test}-behind-background"),
            SOURCE,
            Raster {
                alpha_flags: 0x21,
                depth_test,
                ..default()
            },
            false,
            -1.,
            if depth_test {
                BACKGROUND[..3].try_into()?
            } else {
                SOURCE[..3].try_into()?
            },
        )?;
    }
    for depth_write in [true, false] {
        let overlay = [0.1, 0.8, 0.2, 1.];
        let center = board.add(
            format!("depth-write-{depth_write}-blocks-later-layer"),
            SOURCE,
            Raster {
                depth_write,
                ..default()
            },
            false,
            1.,
            if depth_write {
                SOURCE[..3].try_into()?
            } else {
                overlay[..3].try_into()?
            },
        )?;
        board.parts.push(quad(
            center + Vec3::Z * 0.5,
            Vec2::new(60., 40.),
            overlay,
            Raster {
                alpha_flags: 0x21,
                ..default()
            },
            false,
        ));
    }
    // Exercise the actual terrain draw adapter with solid, original test images.
    // The golden colors are numeric expectations, not a second shader implementation.
    for (name, weights, palette, expected) in [
        (
            "terrain-half-red-blue",
            vec![128, 127],
            vec![0, 1],
            [128. / 255., 0., 127. / 255.],
        ),
        (
            "terrain-overfull-red-green",
            vec![0, 191, 191],
            vec![1, 0, 2],
            [191. / 255., 191. / 255., 0.],
        ),
    ] {
        let center = board.add(
            name.into(),
            [0., 0., 0., 1.],
            default(),
            false,
            1.,
            expected,
        )?;
        board.parts.pop();
        let case = board.cases.last_mut().ok_or("missing terrain GPU case")?;
        case.terrain_weights = Some(weights.clone());
        case.terrain_palette_rgba8 = Some(palette.iter().map(|i| TERRAIN_PALETTE[*i]).collect());
        case.source_rgba = TERRAIN_PALETTE[palette[0]].map(|v| f32::from(v) / 255.);
        for (pass, (weight, texture)) in weights.into_iter().zip(palette).enumerate() {
            let mut part = quad(
                center + Vec3::Z,
                Vec2::new(80., 50.),
                [1.; 4],
                crate::terrain_textures::layer_raster(pass != 0),
                false,
            );
            part.texture = Some(texture);
            part.mesh.insert_attribute(
                Mesh::ATTRIBUTE_COLOR,
                vec![
                    [
                        f32::from(weight) / 255.,
                        f32::from(weight) / 255.,
                        f32::from(weight) / 255.,
                        1.
                    ];
                    4
                ],
            );
            board.parts.push(part);
        }
    }
    let images = TERRAIN_PALETTE
        .map(|rgba| {
            Image::new_fill(
                Extent3d {
                    width: 4,
                    height: 4,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &rgba,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            )
        })
        .to_vec();
    let prepared = Prepared {
        models: vec![Model {
            parts: board.parts,
            center: Vec3::ZERO,
            radius: 1000.,
        }],
        instances: vec![Instance {
            visibility: Visibility::Inherited,
            canonical: None,
            model: 0,
            transform: Transform::IDENTITY,
            key: None,
        }],
        images,
        center: Vec3::ZERO,
        radius: 1000.,
        origin: [0.; 3],
    };
    let report = Report {
        schema_version: 1,
        fixture: "synthetic-source-material-states",
        source_assets_used: false,
        dimensions: [1280, 900],
        byte_tolerance: 2,
        tolerance_reason: "two sRGB bytes allow render target quantization; this is not a retail image tolerance",
        cases: board.cases,
        all_passed: None,
        retail_parity_accepted: false,
    };
    Ok((prepared, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_gpu_readback_is_rejected_and_keeps_failed_measurements() {
        let (_, mut report) = prepare().unwrap();
        let blank = image::RgbImage::new(1280, 900);
        assert!(report.verify(&blank).is_err());
        assert_eq!(report.all_passed, Some(false));
        assert!(
            report
                .cases
                .iter()
                .all(|case| case.measured_srgb8.is_some())
        );
        assert!(report.cases.iter().any(|case| case.passed == Some(false)));
    }
}
