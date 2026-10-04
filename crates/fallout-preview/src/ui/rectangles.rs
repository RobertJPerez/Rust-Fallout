//! Opt-in literal rectangles under an explicit caller inspection policy.
use super::{Document, Kind, Span, includes, traits};
use crate::{loading, material, model, scene};
use bevy::{
    asset::RenderAssetUsages,
    camera::ScalingMode,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use fallout_data::vfs::AssetPath;
use serde::{Deserialize, Serialize};
use std::{io::Write, mem::size_of, path::Path, sync::Arc};

const NUMBERS: [&str; 9] = [
    "x", "y", "width", "height", "depth", "red", "green", "blue", "alpha",
];
const POLICY: &str = "parent-relative-pixels-rgba255";
const COORDINATE: f64 = 1_048_576.;
const PIXELS: u64 = 4_194_304;

#[derive(Clone, Copy)]
pub struct Limits {
    pub document: super::Limits,
    pub request_bytes: usize,
    pub rectangles: usize,
    pub depth: usize,
    pub work: usize,
    pub projection_copies: usize,
    pub projection_metadata: usize,
    pub plan_metadata: usize,
    pub mesh_bytes: usize,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            document: super::Limits::default(),
            request_bytes: 128 * 1024,
            rectangles: 256,
            depth: 64,
            work: 32768,
            projection_copies: 8 * 1024 * 1024,
            projection_metadata: 4 * 1024 * 1024,
            plan_metadata: 1024 * 1024,
            mesh_bytes: 256 * 1024,
            output_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub background: [f32; 4],
    pub depth_range: [f32; 2],
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Parent {
    pub origin: [f64; 3],
    pub opacity: f64,
    pub visible: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub source: includes::Source,
    pub tile: traits::Tile,
    pub policy: String,
    pub viewport: Viewport,
    pub parent: Parent,
}
pub fn read_request(path: &Path, limits: Limits) -> model::Result<Request> {
    let request = super::read_json(path, limits.request_bytes, "rectangle")?;
    validate_request(&request)?;
    Ok(request)
}
pub fn validate_request(request: &Request) -> model::Result<()> {
    if request.schema_version != 1 || request.policy != POLICY {
        return Err("Menu rectangle schema/policy differs".into());
    }
    includes::path(&request.source.path)?;
    includes::hash(&request.source.archive_sha256)?;
    includes::hash(&request.source.payload_sha256)?;
    if request.tile.name.is_empty() || request.tile.name.len() > 256 {
        return Err("Menu rectangle selected name requires 1..256 bytes".into());
    }
    let viewport = &request.viewport;
    if !(1..=4096).contains(&viewport.width)
        || !(1..=4096).contains(&viewport.height)
        || u64::from(viewport.width) * u64::from(viewport.height) > PIXELS
        || viewport
            .background
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
    {
        return Err("Menu rectangle viewport/background budget or range exceeded".into());
    }
    let [near, far] = viewport.depth_range;
    if !near.is_finite()
        || !far.is_finite()
        || near >= far
        || f64::from(far) - f64::from(near) > COORDINATE
        || f64::from(near).abs() > COORDINATE
        || f64::from(far).abs() > COORDINATE
        || request
            .parent
            .origin
            .iter()
            .any(|v| !v.is_finite() || v.abs() > COORDINATE)
        || !request.parent.opacity.is_finite()
        || !(0. ..=1.).contains(&request.parent.opacity)
    {
        return Err("Menu rectangle parent/depth range must be finite and bounded".into());
    }
    let (camera, _) = camera(request);
    if !camera.is_finite() || camera.translation.z <= far {
        return Err("Menu rectangle camera precision collapsed".into());
    }
    Ok(())
}
pub fn camera(request: &Request) -> (Transform, Projection) {
    let viewport = &request.viewport;
    let center = Vec3::new(
        viewport.width as f32 / 2.,
        -(viewport.height as f32) / 2.,
        viewport.depth_range[1],
    );
    (
        Transform::from_translation(center + Vec3::Z * 100.).looking_at(center, Vec3::Y),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: viewport.width as f32,
                height: viewport.height as f32,
            },
            near: 0.1,
            far: (f64::from(viewport.depth_range[1]) - f64::from(viewport.depth_range[0]) + 200.)
                as f32,
            ..OrthographicProjection::default_3d()
        }),
    )
}
#[derive(Serialize)]
pub struct Numeric {
    pub name: &'static str,
    pub node: usize,
    pub span: Span,
    pub inner_span: Span,
    pub value: f32,
    pub bits: u32,
}
#[derive(Serialize)]
pub struct Boolean {
    pub node: usize,
    pub span: Span,
    pub inner_span: Span,
    pub value: bool,
}
#[derive(Serialize)]
pub struct Rectangle {
    pub node: usize,
    pub span: Span,
    pub name_span: Option<Span>,
    pub parent_rectangle: Option<usize>,
    pub numeric: Vec<Numeric>,
    pub visible: Boolean,
    /// Screen right/down bounds; geometry maps y to world -y.
    pub bounds: [f64; 4],
    pub depth: f64,
    pub effective_opacity: f64,
    pub effective_visible: bool,
    pub rgba: [f32; 4],
    pub positions: [[f32; 3]; 4],
    pub translation: [f32; 3],
    pub mesh_positions: [[f32; 3]; 4],
}
#[derive(Default, Serialize)]
pub struct Usage {
    pub rectangles: usize,
    pub traversal_work: usize,
    pub reserved_projection_copies: usize,
    pub projection_metadata: usize,
    pub plan_metadata: usize,
    pub mesh_bytes: usize,
}
#[derive(Serialize)]
pub struct Plan {
    pub rectangles: Vec<Rectangle>,
    pub usage: Usage,
}
#[derive(Serialize)]
pub struct Report<'a> {
    pub schema_version: u32,
    pub source: super::Report,
    pub request: &'a Request,
    pub plan: Plan,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}
#[derive(Debug)]
pub struct Receipt {
    pub path: AssetPath,
    pub archive_sha256: String,
    pub payload_sha256: String,
}
impl Receipt {
    pub fn validate(&self) -> model::Result<()> {
        includes::path(std::str::from_utf8(self.path.bytes())?)?;
        includes::hash(&self.archive_sha256)?;
        includes::hash(&self.payload_sha256)?;
        Ok(())
    }
}
#[derive(Component, Clone, Debug)]
pub struct TileView {
    pub source: Arc<Receipt>,
    pub node: usize,
    pub span: Span,
    pub root_node: usize,
    pub epoch: u64,
}
fn charge(value: &mut usize, amount: usize, limit: usize, name: &str) -> model::Result<()> {
    *value = value
        .checked_add(amount)
        .filter(|v| *v <= limit)
        .ok_or_else(|| format!("Menu rectangle {name} budget exceeded"))?;
    Ok(())
}
fn number(projection: &traits::Projection, name: &'static str) -> model::Result<Numeric> {
    let row = projection
        .rows
        .iter()
        .find(|row| row.name == name)
        .ok_or("Menu rectangle field missing")?;
    if let traits::Outcome::Value {
        value: traits::Literal::FiniteF32 { value, bits },
    } = &row.outcome
    {
        return Ok(Numeric {
            name,
            node: row.node.expect("literal source"),
            span: row.span.expect("literal span"),
            inner_span: row.inner_span.expect("literal inner"),
            value: *value,
            bits: *bits,
        });
    }
    Err(format!("Menu rectangle requires explicit literal {name}").into())
}
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Pure source projection is also a consumer seam; the host uses cancellable projection"
    )
)]
pub fn project(document: &Document, request: &Request, limits: Limits) -> model::Result<Plan> {
    project_checked(document, request, limits, || Ok(()))
}
fn project_checked(
    document: &Document,
    request: &Request,
    limits: Limits,
    mut check: impl FnMut() -> model::Result<()>,
) -> model::Result<Plan> {
    validate_request(request)?;
    if document.named_element(&request.tile.name)? != request.tile.node {
        return Err("Menu rectangle selected name/node differs".into());
    }
    let conversions: Vec<_> = NUMBERS
        .into_iter()
        .map(|name| traits::Conversion {
            name: name.into(),
            kind: traits::ConversionKind::FiniteF32,
        })
        .chain([traits::Conversion {
            name: "visible".into(),
            kind: traits::ConversionKind::Boolean01,
        }])
        .collect();
    let mut usage = Usage::default();
    let mut rectangles: Vec<Rectangle> = Vec::new();
    let mut stack = vec![(request.tile.node, None, 1usize)];
    while let Some((id, parent, depth)) = stack.pop() {
        check()?;
        charge(&mut usage.rectangles, 1, limits.rectangles, "draw count")?;
        if depth > limits.depth {
            return Err("Menu rectangle subtree depth budget exceeded".into());
        }
        let node = document
            .nodes
            .get(id)
            .ok_or("Menu rectangle source node missing")?;
        if node.kind != Kind::Element
            || node.name.is_none_or(|span| document.text(span) != "rect")
            || (parent.is_none() && node.span != request.tile.span)
            || node.attributes.len() > 1
            || node
                .attributes
                .iter()
                .any(|a| document.text(a.name) != "name")
        {
            return Err(
                "Menu rectangle needs exact rect identity and only optional name attribute".into(),
            );
        }
        // Admit traversal and stack metadata before projection copies.
        charge(
            &mut usage.traversal_work,
            1 + node.children.len(),
            limits.work,
            "traversal work",
        )?;
        charge(
            &mut usage.plan_metadata,
            size_of::<Rectangle>()
                + 9 * size_of::<Numeric>()
                + size_of::<(usize, Option<usize>, usize)>(),
            limits.plan_metadata,
            "plan metadata",
        )?;
        let mut children = 0usize;
        for child in &node.children {
            let child_node = &document.nodes[*child];
            match child_node.kind {
                Kind::Element => {
                    let tag = document.text(child_node.name.ok_or("Menu rectangle child tag missing")?);
                    if tag == "rect" { children += 1; }
                    else if tag != "visible" && !NUMBERS.contains(&tag) {
                        return Err(format!("Menu rectangle unsupported direct field/tile {tag}").into());
                    }
                }
                Kind::Comment => {}
                Kind::Text if document.text(child_node.value.ok_or("Menu rectangle text span missing")?).trim_matches([' ', '\t', '\r', '\n']).is_empty() => {}
                _ => return Err("Menu rectangle direct source requires fields, rectangles, comments or XML whitespace".into()),
            }
        }
        if usage
            .rectangles
            .checked_add(stack.len())
            .and_then(|n| n.checked_add(children))
            .is_none_or(|n| n > limits.rectangles)
        {
            return Err("Menu rectangle draw count budget exceeded".into());
        }
        charge(
            &mut usage.plan_metadata,
            children * size_of::<(usize, Option<usize>, usize)>(),
            limits.plan_metadata,
            "stack metadata",
        )?;
        let projection = traits::project_exact(
            document,
            &request.source.payload_sha256,
            id,
            node.span,
            &conversions,
            traits::Limits {
                document: limits.document,
                copy_bytes: limits.projection_copies - usage.reserved_projection_copies,
                metadata_bytes: limits.projection_metadata - usage.projection_metadata,
                ..traits::Limits::default()
            },
        )?;
        charge(
            &mut usage.reserved_projection_copies,
            projection.usage.reserved_copy_bytes,
            limits.projection_copies,
            "projection copy",
        )?;
        charge(
            &mut usage.projection_metadata,
            projection.usage.metadata_bytes,
            limits.projection_metadata,
            "projection metadata",
        )?;
        let numeric = NUMBERS
            .into_iter()
            .map(|name| number(&projection, name))
            .collect::<model::Result<Vec<_>>>()?;
        let row = projection
            .rows
            .iter()
            .find(|row| row.name == "visible")
            .expect("requested visible row");
        let traits::Outcome::Value {
            value: traits::Literal::Boolean01 { value },
        } = &row.outcome
        else {
            return Err("Menu rectangle requires explicit literal visible (0 or 1)".into());
        };
        let visible = Boolean {
            node: row.node.expect("boolean source"),
            span: row.span.expect("boolean span"),
            inner_span: row.inner_span.expect("boolean inner"),
            value: *value,
        };
        let values: Vec<_> = numeric.iter().map(|n| f64::from(n.value)).collect();
        let [x, y, width, height, z, r, g, b, alpha]: [f64; 9] =
            values.try_into().expect("nine numeric fields");
        if width <= 0. || height <= 0. || [r, g, b, alpha].iter().any(|v| !(0. ..=255.).contains(v))
        {
            return Err("Menu rectangle requires positive dimensions and RGBA in 0..255".into());
        }
        let (origin, opacity, shown) = parent.map_or(
            (
                request.parent.origin,
                request.parent.opacity,
                request.parent.visible,
            ),
            |index: usize| {
                let p = &rectangles[index];
                (
                    [p.bounds[0], p.bounds[1], p.depth],
                    p.effective_opacity,
                    p.effective_visible,
                )
            },
        );
        let x = x + origin[0];
        let y = y + origin[1];
        let z = z + origin[2];
        let bounds = [x, y, x + width, y + height];
        if bounds
            .iter()
            .chain([&z])
            .any(|v| !v.is_finite() || v.abs() > COORDINATE)
            || z < f64::from(request.viewport.depth_range[0])
            || z > f64::from(request.viewport.depth_range[1])
        {
            return Err(
                "Menu rectangle resolved coordinates/depth outside bounded viewport policy".into(),
            );
        }
        let positions = [
            [x as f32, -(y as f32), z as f32],
            [(x + width) as f32, -(y as f32), z as f32],
            [(x + width) as f32, -((y + height) as f32), z as f32],
            [x as f32, -((y + height) as f32), z as f32],
        ];
        if positions[0][0] >= positions[1][0] || positions[0][1] <= positions[3][1] {
            return Err("Menu rectangle f32 geometry precision collapsed".into());
        }
        let area = (positions[1][0] - positions[0][0]) * (positions[0][1] - positions[3][1]);
        if !area.is_finite() || area <= 0. || area.is_subnormal() {
            return Err("Menu rectangle draw area precision collapsed".into());
        }
        let effective_opacity = opacity * (alpha / 255.);
        if (opacity > 0. && alpha > 0. && effective_opacity == 0.)
            || (effective_opacity > 0. && effective_opacity as f32 == 0.)
        {
            return Err("Menu rectangle effective opacity precision collapsed".into());
        }
        let translation = [
            ((x + (x + width)) / 2.) as f32,
            -(((y + (y + height)) / 2.) as f32),
            z as f32,
        ];
        let mesh_positions = positions.map(|p| {
            [
                p[0] - translation[0],
                p[1] - translation[1],
                p[2] - translation[2],
            ]
        });
        if mesh_positions
            .iter()
            .zip(positions)
            .any(|(p, expected)| (0..3).any(|i| p[i] + translation[i] != expected[i]))
        {
            return Err("Menu rectangle local mesh reconstruction precision differs".into());
        }
        if mesh_positions
            .iter()
            .flatten()
            .chain(translation.iter())
            .any(|v| v.is_subnormal())
        {
            return Err("Menu rectangle GPU coordinate precision is subnormal".into());
        }
        let rgba = [
            (r / 255.) as f32,
            (g / 255.) as f32,
            (b / 255.) as f32,
            effective_opacity as f32,
        ];
        if rgba.iter().any(|v| v.is_subnormal())
            || [r, g, b, effective_opacity]
                .iter()
                .zip(rgba)
                .any(|(source, draw)| *source > 0. && draw == 0.)
        {
            return Err("Menu rectangle GPU color/opacity precision collapsed".into());
        }
        let effective_visible = shown && visible.value;
        // Equal-depth overlapping translucent draws have no certified source
        // ordering policy here. Refuse them instead of inventing a tie breaker.
        if effective_visible
            && effective_opacity > 0.
            && rectangles.iter().any(|p| {
                p.effective_visible
                    && p.effective_opacity > 0.
                    && p.positions[0][2] == z as f32
                    && bounds[0] < p.bounds[2]
                    && bounds[2] > p.bounds[0]
                    && bounds[1] < p.bounds[3]
                    && bounds[3] > p.bounds[1]
            })
        {
            return Err(
                "Menu rectangle overlapping equal-depth draws need an explicit supported order"
                    .into(),
            );
        }
        charge(
            &mut usage.mesh_bytes,
            4 * (12 + 12 + 8) + 6 * 4,
            limits.mesh_bytes,
            "mesh byte",
        )?;
        let index = rectangles.len();
        rectangles.push(Rectangle {
            node: id,
            span: node.span,
            name_span: node.attributes.first().map(|a| a.raw_value),
            parent_rectangle: parent,
            numeric,
            visible,
            bounds,
            depth: z,
            effective_opacity,
            effective_visible,
            rgba,
            positions,
            translation,
            mesh_positions,
        });
        stack.extend(
            node.children
                .iter()
                .rev()
                .copied()
                .filter(|child| {
                    document.nodes[*child]
                        .name
                        .is_some_and(|span| document.text(span) == "rect")
                })
                .map(|child| (child, Some(index), depth + 1)),
        );
    }
    check()?;
    Ok(Plan { rectangles, usage })
}
pub fn load<'a>(
    install: &Path,
    request: &'a Request,
    limits: Limits,
    context: &loading::Context,
    epoch: u64,
) -> model::Result<(scene::Prepared, Report<'a>, Vec<TileView>)> {
    validate_request(request)?;
    context.stage("Reading exact rectangle source")?;
    let source = super::inspect(
        install,
        &includes::path(&request.source.path)?,
        None,
        limits.document,
    )?;
    context.check()?;
    if source.archive_sha256 != request.source.archive_sha256
        || source.payload_sha256 != request.source.payload_sha256
    {
        return Err("Menu rectangle source archive/payload SHA differs".into());
    }
    let plan = project_checked(&source.document, request, limits, || {
        context.check()?;
        Ok(())
    })?;
    let receipt = Arc::new(Receipt {
        path: includes::path(&request.source.path)?,
        archive_sha256: source.archive_sha256.clone(),
        payload_sha256: source.payload_sha256.clone(),
    });
    let mut models = Vec::with_capacity(plan.rectangles.len());
    let mut instances = Vec::with_capacity(plan.rectangles.len());
    let mut views = Vec::with_capacity(plan.rectangles.len());
    for rectangle in &plan.rectangles {
        context.check()?;
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, rectangle.mesh_positions.to_vec());
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0., 0., 1.]; 4]);
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
        );
        mesh.insert_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]));
        let [r, g, b, a] = rectangle.rgba;
        let index = models.len();
        models.push(model::Model {
            parts: vec![model::Part {
                mesh,
                texture: None,
                color: Color::srgba(r, g, b, a),
                raster: material::Raster {
                    alpha_flags: 1 | (6 << 1) | (7 << 5),
                    alpha_threshold: 0,
                    draw_mode: 1,
                    depth_test: false,
                    depth_write: false,
                },
            }],
            center: Vec3::ZERO,
            radius: ((rectangle.bounds[2] - rectangle.bounds[0])
                .hypot(rectangle.bounds[3] - rectangle.bounds[1])
                / 2.) as f32,
        });
        instances.push(scene::Instance {
            model: index,
            transform: Transform::from_translation(Vec3::from_array(rectangle.translation)),
            key: None,
            canonical: None,
            visibility: if rectangle.effective_visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        });
        views.push(TileView {
            source: receipt.clone(),
            node: rectangle.node,
            span: rectangle.span,
            root_node: request.tile.node,
            epoch,
        });
    }
    context.check()?;
    let viewport = &request.viewport;
    let prepared = scene::Prepared {
        models,
        instances,
        images: Vec::new(),
        center: Vec3::new(
            viewport.width as f32 / 2.,
            -(viewport.height as f32) / 2.,
            0.,
        ),
        radius: (viewport.width.max(viewport.height) as f32) / 2.,
        origin: [0.; 3],
    };
    Ok((
        prepared,
        Report {
            schema_version: 1,
            source,
            request,
            plan,
            interpretation: "Caller inspection policy: explicit parent-relative right/down pixels, additive depth (larger in front), RGBA255 sRGB straight alpha, inherited visibility AND and opacity product; fixed orthographic unlit draw; equal-depth overlapping visible draws refused; no expressions, templates, text, focus, actions or original menu readiness",
            original_display_ready: false,
        },
        views,
    ))
}
pub fn write_report(writer: impl Write, report: &Report<'_>, limit: usize) -> model::Result<()> {
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
