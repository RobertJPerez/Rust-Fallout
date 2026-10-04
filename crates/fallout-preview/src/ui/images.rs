//! A selected literal source image and exact retained DDS under caller policies.
use super::{Document, Kind, Span, includes, rectangles, traits};
use crate::{loading, model, scene};
use bevy::{
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    prelude::*,
};
use fallout_data::{
    assets::ArchiveAssets,
    vfs::{AssetPath, AssetSource},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, mem::size_of, path::Path, sync::Arc};

const SAMPLING: &str = "nearest-clamp-edge-mip0";
#[derive(Clone, Copy)]
pub struct Limits {
    pub literal: traits::Limits,
    pub filename_bytes: usize,
    pub plan_copy_bytes: usize,
    pub plan_metadata: usize,
    pub dds_bytes: usize,
    pub mip_pixels: u64,
    pub mesh_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            literal: traits::Limits {
                rows: 32,
                ..traits::Limits::default()
            },
            filename_bytes: 4096,
            plan_copy_bytes: 16 * 1024,
            plan_metadata: 64 * 1024,
            dds_bytes: 8 * 1024 * 1024,
            mip_pixels: 4_194_304,
            mesh_bytes: 152,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub node: usize,
    pub span: Span,
    pub name: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub source: includes::Source,
    pub image: Selection,
    pub texture: includes::Source,
    pub policy: String,
    pub viewport: rectangles::Viewport,
    pub parent: rectangles::Parent,
    pub uv_rect: [f32; 4],
    pub sampling: String,
}
pub fn read_request(path: &Path, limits: Limits) -> model::Result<Request> {
    let request = super::read_json(path, limits.literal.request_bytes, "image")?;
    validate_request(&request)?;
    Ok(request)
}
pub fn validate_request(request: &Request) -> model::Result<()> {
    if request.schema_version != 1
        || request.policy != rectangles::POLICY
        || request.sampling != SAMPLING
    {
        return Err("Menu image schema/coordinate/sampling policy differs".into());
    }
    for source in [&request.source, &request.texture] {
        includes::path(&source.path)?;
        includes::hash(&source.archive_sha256)?;
        includes::hash(&source.payload_sha256)?;
    }
    let texture = includes::path(&request.texture.path)?;
    if !texture.bytes().starts_with(b"textures/") || !texture.bytes().ends_with(b".dds") {
        return Err("Menu image requires an exact full textures/... DDS member".into());
    }
    if request.image.span.start >= request.image.span.end
        || request
            .image
            .name
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.len() > 256)
    {
        return Err("Menu image selection span/name invalid".into());
    }
    let [u0, v0, u1, v1] = request.uv_rect;
    if request
        .uv_rect
        .iter()
        .any(|v| !v.is_finite() || v.is_subnormal() || !(0. ..=1.).contains(v))
        || u0 >= u1
        || v0 >= v1
        || (u1 - u0).is_subnormal()
        || (v1 - v0).is_subnormal()
    {
        return Err("Menu image explicit UV rectangle must be finite, ordered and bounded".into());
    }
    rectangles::validate_viewport(&request.viewport, &request.parent)
}
pub fn camera(request: &Request) -> (Transform, Projection) {
    rectangles::camera_for(&request.viewport)
}
#[derive(Serialize)]
pub struct Filename {
    pub node: usize,
    pub span: Span,
    pub inner_span: Span,
    pub resolved: String,
    pub normalized: AssetPath,
}
#[derive(Serialize)]
pub struct Plan {
    pub layout: rectangles::Rectangle,
    pub filename: Filename,
    pub uv: [[f32; 2]; 4],
    pub uv_bits: [u32; 4],
    pub projection_usage: traits::Usage,
    pub plan_copy_bytes: usize,
    pub plan_metadata: usize,
    pub mesh_bytes: usize,
}
#[derive(Serialize)]
pub struct Texture {
    pub source: AssetSource,
    pub path: AssetPath,
    pub archive_sha256: String,
    pub payload_sha256: String,
    pub input_bytes: usize,
    pub retained_bytes: usize,
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub mip_pixels: u64,
    pub format: String,
    pub sampling: &'static str,
}
#[derive(Serialize)]
pub struct Report<'a> {
    pub schema_version: u32,
    pub source: super::Report,
    pub request: &'a Request,
    pub plan: Plan,
    pub texture: Texture,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}
#[derive(Component, Clone, Debug)]
pub struct ImageView {
    pub tile: rectangles::TileView,
    pub texture: Arc<rectangles::Receipt>,
    pub uv_bits: [u32; 4],
    pub sampling: &'static str,
}
impl ImageView {
    pub fn validate(&self) -> model::Result<()> {
        self.tile.source.validate()?;
        self.texture.validate()?;
        if self.tile.root_node != self.tile.node {
            return Err("Menu image label must identify the selected image root".into());
        }
        if !self.texture.path.bytes().starts_with(b"textures/")
            || !self.texture.path.bytes().ends_with(b".dds")
        {
            return Err("Menu image label requires a full DDS member".into());
        }
        if self.sampling != SAMPLING {
            return Err("Menu image label sampling policy differs".into());
        }
        let uv = self.uv_bits.map(f32::from_bits);
        if uv
            .iter()
            .any(|v| !v.is_finite() || v.is_subnormal() || !(0. ..=1.).contains(v))
            || uv[0] >= uv[2]
            || uv[1] >= uv[3]
            || (uv[2] - uv[0]).is_subnormal()
            || (uv[3] - uv[1]).is_subnormal()
        {
            return Err("Menu image label UV identity invalid".into());
        }
        Ok(())
    }
}
pub fn project(document: &Document, request: &Request, limits: Limits) -> model::Result<Plan> {
    validate_request(request)?;
    let plan_metadata = size_of::<Plan>() + 9 * size_of::<rectangles::Numeric>();
    if plan_metadata > limits.plan_metadata || limits.mesh_bytes < 152 {
        return Err("Menu image plan metadata/mesh budget exceeded".into());
    }
    if let Some(name) = &request.image.name
        && document.named_element(name)? != request.image.node
    {
        return Err("Menu image selected name/node differs".into());
    }
    let node = document
        .nodes
        .get(request.image.node)
        .filter(|n| n.kind == Kind::Element && n.span == request.image.span)
        .ok_or("Menu image selected node/span differs")?;
    if node.name.is_none_or(|span| document.text(span) != "image")
        || node.attributes.len() > 1
        || node
            .attributes
            .iter()
            .any(|a| document.text(a.name) != "name")
    {
        return Err(
            "Menu image requires an exact image tag with only optional name attribute".into(),
        );
    }
    for child in &node.children {
        let field = &document.nodes[*child];
        match field.kind {
            Kind::Element => {
                let tag = document.text(field.name.ok_or("Menu image field name missing")?);
                if tag != "visible" && tag != "filename" && !rectangles::NUMBERS.contains(&tag) {
                    return Err(format!("Menu image unsupported direct field/tile {tag}").into());
                }
            }
            Kind::Comment => {}
            Kind::Text
                if document
                    .text(field.value.ok_or("Menu image text span missing")?)
                    .trim_matches([' ', '\t', '\r', '\n'])
                    .is_empty() => {}
            _ => {
                return Err(
                    "Menu image direct source requires literal fields, comments or XML whitespace"
                        .into(),
                );
            }
        }
    }
    let conversions: Vec<_> = rectangles::NUMBERS
        .into_iter()
        .map(|name| traits::Conversion {
            name: name.into(),
            kind: traits::ConversionKind::FiniteF32,
        })
        .chain([
            traits::Conversion {
                name: "visible".into(),
                kind: traits::ConversionKind::Boolean01,
            },
            traits::Conversion {
                name: "filename".into(),
                kind: traits::ConversionKind::String,
            },
        ])
        .collect();
    let projection = traits::project_exact(
        document,
        &request.source.payload_sha256,
        request.image.node,
        request.image.span,
        &conversions,
        limits.literal,
    )?;
    let layout = rectangles::quad(document, &projection, &request.parent, &request.viewport)?;
    let row = projection
        .rows
        .iter()
        .find(|row| row.name == "filename")
        .expect("requested filename row");
    let traits::Outcome::Value {
        value: traits::Literal::String { value },
    } = &row.outcome
    else {
        return Err("Menu image requires explicit literal filename".into());
    };
    if value.is_empty() || value.len() > limits.filename_bytes {
        return Err("Menu image literal filename byte budget exceeded or empty".into());
    }
    let plan_copy_bytes = value
        .len()
        .checked_mul(2)
        .ok_or("Menu image filename copy count overflow")?;
    if plan_copy_bytes > limits.plan_copy_bytes {
        return Err("Menu image plan copy/metadata/mesh budget exceeded".into());
    }
    let path = includes::path(value)?;
    if path != includes::path(&request.texture.path)? {
        return Err(
            "Menu image literal filename differs from exact supplied texture member".into(),
        );
    }
    let filename = Filename {
        node: row.node.expect("literal source"),
        span: row.span.expect("literal span"),
        inner_span: row.inner_span.expect("literal inner"),
        resolved: value.clone(),
        normalized: path,
    };
    let [u0, v0, u1, v1] = request.uv_rect;
    Ok(Plan {
        layout,
        filename,
        uv: [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
        uv_bits: request.uv_rect.map(f32::to_bits),
        projection_usage: projection.usage,
        plan_copy_bytes,
        plan_metadata,
        mesh_bytes: 152,
    })
}
pub fn load<'a>(
    install: &Path,
    request: &'a Request,
    limits: Limits,
    context: &loading::Context,
    epoch: u64,
) -> model::Result<(scene::Prepared, Report<'a>, ImageView)> {
    validate_request(request)?;
    context.stage("Reading exact image tile source")?;
    let source = super::inspect(
        install,
        &includes::path(&request.source.path)?,
        None,
        limits.literal.document,
    )?;
    context.check()?;
    if source.archive_sha256 != request.source.archive_sha256
        || source.payload_sha256 != request.source.payload_sha256
    {
        return Err("Menu image XML archive/payload SHA differs".into());
    }
    let plan = project(&source.document, request, limits)?;
    context.stage("Reading and verifying exact DDS member")?;
    let mut assets = ArchiveAssets::open_nv(install)?;
    context.check()?;
    let path = &plan.filename.normalized;
    let (texture_source, bytes) = assets.read_unique_bounded(path, limits.dds_bytes as u64)?;
    context.check()?;
    let payload_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let archive_sha256 = assets.source_digest(&texture_source)?.to_owned();
    context.check()?;
    if payload_sha256 != request.texture.payload_sha256
        || archive_sha256 != request.texture.archive_sha256
    {
        return Err("Menu image DDS archive/payload SHA differs".into());
    }
    context.stage("Retaining bounded source DDS mip payload")?;
    let mut image = model::decode_diffuse_bounded(&bytes, 0, limits.mip_pixels, limits.dds_bytes)?;
    context.check()?;
    let size = image.texture_descriptor.size;
    let retained_bytes = image.data.as_ref().map_or(0, Vec::len);
    if retained_bytes > limits.dds_bytes {
        return Err("Menu image retained texture byte budget exceeded".into());
    }
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        lod_min_clamp: 0.,
        lod_max_clamp: 0.,
        ..ImageSamplerDescriptor::nearest()
    });
    let mip_levels = image.texture_descriptor.mip_level_count;
    let mip_pixels = model::diffuse_physical_mip_pixels(size.width, size.height, mip_levels)?;
    let texture = Texture {
        source: texture_source,
        path: path.clone(),
        archive_sha256: archive_sha256.clone(),
        payload_sha256: payload_sha256.clone(),
        input_bytes: bytes.len(),
        retained_bytes,
        width: size.width,
        height: size.height,
        mip_levels,
        mip_pixels,
        format: format!("{:?}", image.texture_descriptor.format),
        sampling: SAMPLING,
    };
    let tile = rectangles::TileView {
        source: Arc::new(rectangles::Receipt {
            path: includes::path(&request.source.path)?,
            archive_sha256: source.archive_sha256.clone(),
            payload_sha256: source.payload_sha256.clone(),
        }),
        node: request.image.node,
        span: request.image.span,
        root_node: request.image.node,
        epoch,
    };
    let view = ImageView {
        tile,
        texture: Arc::new(rectangles::Receipt {
            path: path.clone(),
            archive_sha256,
            payload_sha256,
        }),
        uv_bits: plan.uv_bits,
        sampling: SAMPLING,
    };
    let model = rectangles::draw_model(&plan.layout, Some(0), plan.uv);
    let instance = scene::Instance {
        model: 0,
        transform: Transform::from_translation(Vec3::from_array(plan.layout.translation)),
        key: None,
        canonical: None,
        visibility: if plan.layout.effective_visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        },
    };
    let viewport = &request.viewport;
    let prepared = scene::Prepared {
        models: vec![model],
        instances: vec![instance],
        images: vec![image],
        center: Vec3::new(
            viewport.width as f32 / 2.,
            -(viewport.height as f32) / 2.,
            0.,
        ),
        radius: viewport.width.max(viewport.height) as f32 / 2.,
        origin: [0.; 3],
    };
    context.check()?;
    Ok((
        prepared,
        Report {
            schema_version: 1,
            source,
            request,
            plan,
            texture,
            interpretation: "Exact literal source image and retained existing BC1/2/3 sRGB DDS. Explicit caller pixel/RGBA255 parent policy, UV rectangle and nearest/clamp-edge/mip0 sampling; no original defaults, repeat/atlas/crop/rotate flag interpretation, relative filename fallback, expressions, template, font, focus/action or whole original menu readiness",
            original_display_ready: false,
        },
        view,
    ))
}
pub fn write_report(writer: impl Write, report: &Report<'_>, limit: usize) -> model::Result<()> {
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
