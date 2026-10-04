use super::*;
use bevy::image::ImageFilterMode;
use bevy::render::render_resource::TextureFormat;
use clap::Parser;
use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SOURCE: &str = "<image name='Panel'><x>8</x><y>6</y><width>20</width><height>16</height><depth>1</depth><red>255</red><green>255</green><blue>255</blue><alpha>255</alpha><visible>1</visible><filename>Textures\\authored.dds</filename></image>";
fn document(source: &str) -> Document {
    super::super::parse(source.as_bytes().to_vec(), super::super::Limits::default()).unwrap()
}
fn request(doc: &Document) -> Request {
    let node = doc
        .nodes
        .iter()
        .position(|n| n.name.is_some_and(|s| doc.text(s) == "image"))
        .unwrap();
    Request {
        schema_version: 1,
        source: includes::Source {
            path: "menus/authored.xml".into(),
            archive_sha256: "0".repeat(64),
            payload_sha256: format!("{:x}", Sha256::digest(doc.source_utf8.as_bytes())),
        },
        image: Selection {
            node,
            span: doc.nodes[node].span,
            name: Some("Panel".into()),
        },
        texture: includes::Source {
            path: "textures/authored.dds".into(),
            archive_sha256: "1".repeat(64),
            payload_sha256: "2".repeat(64),
        },
        policy: rectangles::POLICY.into(),
        viewport: rectangles::Viewport {
            width: 32,
            height: 32,
            background: [0., 0., 0., 1.],
            depth_range: [-10., 10.],
        },
        parent: rectangles::Parent {
            origin: [2., 3., 0.5],
            opacity: 0.5,
            visible: true,
        },
        uv_rect: [0.25, 0., 0.75, 1.],
        sampling: SAMPLING.into(),
    }
}
#[test]
fn exact_image_layout_filename_spans_words_and_optional_name_have_independent_expectations() {
    let doc = document(SOURCE);
    let request = request(&doc);
    let plan = project(&doc, &request, Limits::default()).unwrap();
    assert_eq!(plan.layout.bounds, [10., 9., 30., 25.]);
    assert_eq!(plan.layout.depth, 1.5);
    assert_eq!(plan.layout.rgba, [1., 1., 1., 0.5]);
    assert_eq!(
        plan.layout.positions,
        [
            [10., -9., 1.5],
            [30., -9., 1.5],
            [30., -25., 1.5],
            [10., -25., 1.5]
        ]
    );
    assert_eq!(plan.uv, [[0.25, 0.], [0.75, 0.], [0.75, 1.], [0.25, 1.]]);
    assert_eq!(plan.uv_bits, [0x3e800000, 0, 0x3f400000, 0x3f800000]);
    assert_eq!(plan.filename.resolved, "Textures\\authored.dds");
    assert_eq!(plan.filename.normalized.bytes(), b"textures/authored.dds");
    assert_eq!(
        doc.text(plan.filename.span),
        "<filename>Textures\\authored.dds</filename>"
    );
    assert_eq!(doc.text(plan.filename.inner_span), "Textures\\authored.dds");
    for field in &plan.layout.numeric {
        assert_eq!(
            field.bits,
            doc.text(field.inner_span).parse::<f32>().unwrap().to_bits()
        );
        assert_eq!(field.span, doc.nodes[field.node].span);
    }
    let doc = document(&format!("{SOURCE}<image name='Panel'/>"));
    let mut request = request_for(&doc);
    assert!(project(&doc, &request, Limits::default()).is_err());
    request.image.name = None;
    assert!(project(&doc, &request, Limits::default()).is_ok());
    let doc = document(&SOURCE.replace("Textures\\authored.dds", "Textures\\author&#101;d.dds"));
    let request = request_for(&doc);
    assert_eq!(
        project(&doc, &request, Limits::default())
            .unwrap()
            .filename
            .resolved,
        "Textures\\authored.dds"
    );
}
fn request_for(doc: &Document) -> Request {
    request(doc)
}
#[test]
fn source_image_requires_every_literal_and_refuses_unevaluated_original_flags_and_defaults() {
    for source in [
        SOURCE.replace("<red>255</red>", ""),
        SOURCE.replace("<filename>Textures\\authored.dds</filename>", ""),
        SOURCE.replace("<filename>Textures\\authored.dds</filename>", "<filename/>"),
        SOURCE.replace("Textures\\authored.dds", "authored.dds"),
        SOURCE.replace("Textures\\authored.dds", "&file;"),
        SOURCE.replace(
            "<width>20</width>",
            "<width><copy src='parent()' trait='width'/></width>",
        ),
        SOURCE.replace("<x>8</x>", "<x>8</x><x>8</x>"),
        SOURCE.replace("<visible>1</visible>", "<visible>&true;</visible>"),
        SOURCE.replace("</image>", "<repeatvertical>0</repeatvertical></image>"),
        SOURCE.replace("</image>", "<texatlas>0</texatlas></image>"),
        SOURCE.replace("</image>", "<rotateangle>0</rotateangle></image>"),
        SOURCE.replace("</image>", "<filewidth>8</filewidth></image>"),
        SOURCE.replace("</image>", "<rect name='Child'/></image>"),
        SOURCE.replace("name='Panel'", "name='Panel' extra='0'"),
        SOURCE.replace("<x>8</x>", "<x>NaN</x>"),
        SOURCE
            .replace("<image ", "<rect ")
            .replace("</image>", "</rect>"),
    ] {
        let doc = document(&source);
        let mut request = request_from_node(&doc);
        request.image.span = doc.nodes[request.image.node].span;
        assert!(
            project(&doc, &request, Limits::default()).is_err(),
            "{source}"
        );
    }
}
fn request_from_node(doc: &Document) -> Request {
    let mut request = request(&document(SOURCE));
    request.source.payload_sha256 = format!("{:x}", Sha256::digest(doc.source_utf8.as_bytes()));
    request.image.node = 0;
    request.image.span = doc.nodes[0].span;
    request
}
#[test]
fn exact_projection_plan_caps_and_strict_request_identity_uv_sampler_boundaries_refuse() {
    let doc = document(SOURCE);
    let base = request(&doc);
    let plan = project(&doc, &base, Limits::default()).unwrap();
    let exact = Limits {
        literal: traits::Limits {
            rows: 11,
            conversions: 11,
            copy_bytes: plan.projection_usage.reserved_copy_bytes,
            metadata_bytes: plan.projection_usage.metadata_bytes,
            ..traits::Limits::default()
        },
        filename_bytes: plan.filename.resolved.len(),
        plan_copy_bytes: plan.plan_copy_bytes,
        plan_metadata: plan.plan_metadata,
        mesh_bytes: 152,
        ..Limits::default()
    };
    assert!(project(&doc, &base, exact).is_ok());
    for limits in [
        Limits {
            literal: traits::Limits {
                rows: 10,
                ..exact.literal
            },
            ..exact
        },
        Limits {
            literal: traits::Limits {
                copy_bytes: exact.literal.copy_bytes - 1,
                ..exact.literal
            },
            ..exact
        },
        Limits {
            literal: traits::Limits {
                metadata_bytes: exact.literal.metadata_bytes - 1,
                ..exact.literal
            },
            ..exact
        },
        Limits {
            filename_bytes: exact.filename_bytes - 1,
            ..exact
        },
        Limits {
            plan_copy_bytes: exact.plan_copy_bytes - 1,
            ..exact
        },
        Limits {
            plan_metadata: exact.plan_metadata - 1,
            ..exact
        },
        Limits {
            mesh_bytes: 151,
            ..exact
        },
    ] {
        assert!(project(&doc, &base, limits).is_err());
    }
    for field in [
        "schema",
        "payload",
        "node",
        "span",
        "name",
        "filename",
        "policy",
        "sampling",
        "uv-order",
        "uv-range",
        "uv-nan",
        "uv-subnormal",
        "viewport",
        "opacity",
    ] {
        let mut request = request(&doc);
        match field {
            "schema" => request.schema_version = 2,
            "payload" => request.source.payload_sha256 = "f".repeat(64),
            "node" => request.image.node = usize::MAX,
            "span" => request.image.span.end += 1,
            "name" => request.image.name = Some("Missing".into()),
            "filename" => request.texture.path = "textures/other.dds".into(),
            "policy" => request.policy = "guess".into(),
            "sampling" => request.sampling = "original".into(),
            "uv-order" => request.uv_rect = [1., 0., 0., 1.],
            "uv-range" => request.uv_rect[0] = -1.,
            "uv-nan" => request.uv_rect[0] = f32::NAN,
            "uv-subnormal" => request.uv_rect[0] = f32::from_bits(1),
            "viewport" => request.viewport.width = 4097,
            "opacity" => request.parent.opacity = 2.,
            _ => unreachable!(),
        };
        assert!(
            project(&doc, &request, Limits::default()).is_err(),
            "{field}"
        );
    }
}
fn dds() -> Vec<u8> {
    let mut bytes = vec![0; 168];
    bytes[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124u32),
        (8, 0xa1007),
        (12, 8),
        (16, 8),
        (20, 32),
        (28, 2),
        (76, 32),
        (80, 4),
        (108, 0x401008),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[84..88].copy_from_slice(b"DXT1");
    // Independent authored top mip: four solid 4x4 blocks in row order. The
    // retained second mip is white, so selecting mip0 can be proved on the GPU.
    for (index, color) in [0xf800u16, 0x07e0, 0x001f, 0xffe0, 0xffff]
        .into_iter()
        .enumerate()
    {
        bytes[128 + index * 8..130 + index * 8].copy_from_slice(&color.to_le_bytes());
    }
    bytes
}
#[test]
fn sole_dds_adapter_preserves_default_and_bounds_base_all_mip_texels_and_input_bytes() {
    let bytes = dds();
    let image = model::decode_diffuse_bounded(&bytes, 0, 80, 168).unwrap();
    assert_eq!(image.texture_descriptor.size.width, 8);
    assert_eq!(image.texture_descriptor.size.height, 8);
    assert_eq!(image.texture_descriptor.mip_level_count, 2);
    assert_eq!(image.data.as_ref().unwrap(), &bytes[128..]);
    assert!(model::decode_diffuse(&bytes, 3).is_ok());
    for (pixels, maximum) in [(79, 168), (64, 168), (63, 168), (80, 167)] {
        assert!(model::decode_diffuse_bounded(&bytes, 0, pixels, maximum).is_err());
    }
    let mut missing = bytes.clone();
    missing.pop();
    assert!(model::decode_diffuse_bounded(&missing, 0, 80, 168).is_err());
}

fn authored_dds_extent(width: u32, height: u32, levels: u32, fourcc: &[u8; 4]) -> Vec<u8> {
    let block_bytes = if fourcc == b"DXT1" { 8 } else { 16 };
    let payload: usize = (0..levels)
        .map(|mip| {
            (width >> mip).max(1).div_ceil(4) as usize
                * (height >> mip).max(1).div_ceil(4) as usize
                * block_bytes
        })
        .sum();
    let mut bytes = vec![0; 128 + payload];
    bytes[..4].copy_from_slice(b"DDS ");
    for (offset, value) in [
        (4, 124),
        (8, if levels > 1 { 0xa1007 } else { 0x81007 }),
        (12, height),
        (16, width),
        (
            20,
            width.div_ceil(4) * height.div_ceil(4) * block_bytes as u32,
        ),
        (28, levels),
        (76, 32),
        (80, 4),
        (108, if levels > 1 { 0x401008 } else { 0x1000 }),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[84..88].copy_from_slice(fourcc);
    bytes
}

#[test]
fn physical_dds_extent_refuses_the_independent_raw_under_physical_over_case_before_decode() {
    // Root's concrete dimensions: raw 4,194,300, actual Bevy 4,202,496.
    let bytes = authored_dds_extent(2050, 2046, 1, b"DXT1");
    assert_eq!(bytes.len(), 2_101_376);
    let unbounded = model::decode_diffuse(&bytes, 3).unwrap();
    assert_eq!(unbounded.texture_descriptor.size.width, 2052);
    assert_eq!(unbounded.texture_descriptor.size.height, 2048);
    assert_eq!(unbounded.data.as_ref().unwrap().len(), 2_101_248);
    assert_eq!(
        model::diffuse_physical_mip_pixels(2050, 2046, 1).unwrap(),
        4_202_496
    );
    for cap in [4_194_300, 4_194_304, 4_202_495] {
        let error = model::decode_diffuse_bounded(&bytes, 0, cap, bytes.len())
            .expect_err("Physical extent must refuse the complete image");
        assert!(error.to_string().contains("physical mip texel budget"));
    }
    assert!(model::decode_diffuse_bounded(&bytes, 0, 4_202_496, bytes.len()).is_ok());
    let aligned = authored_dds_extent(2048, 2048, 1, b"DXT1");
    assert!(model::decode_diffuse_bounded(&aligned, 0, 4_194_304, aligned.len()).is_ok());
    assert!(model::decode_diffuse_bounded(&aligned, 0, 4_194_303, aligned.len()).is_err());
    // An invalid header would fail in ddsfile. The physical refusal arrives
    // first, establishing that Image::from_buffer has not been reached.
    let mut invalid = bytes;
    invalid[4..8].copy_from_slice(&0u32.to_le_bytes());
    let error = model::decode_diffuse_bounded(&invalid, 0, 4_194_304, invalid.len())
        .err()
        .unwrap();
    assert!(error.to_string().contains("physical mip texel budget"));
    assert!(model::decode_diffuse(&invalid, 0).is_err());
}

#[test]
fn physical_tail_mips_and_existing_bc_format_clamp_payload_policies_keep_exact_bounds() {
    for (width, height, levels, physical_pixels, bc1_payload) in [
        (1, 1, 1, 16, 8),
        (4, 4, 3, 48, 24),
        (7, 5, 3, 96, 48),
        (8, 8, 2, 80, 40),
    ] {
        for fourcc in [b"DXT1", b"DXT2", b"DXT3", b"DXT4", b"DXT5"] {
            let bytes = authored_dds_extent(width, height, levels, fourcc);
            let payload = if fourcc == b"DXT1" {
                bc1_payload
            } else {
                bc1_payload * 2
            };
            assert_eq!(bytes.len(), 128 + payload);
            assert_eq!(
                model::diffuse_physical_mip_pixels(width, height, levels).unwrap(),
                physical_pixels
            );
            for clamp in 0..4 {
                let image =
                    model::decode_diffuse_bounded(&bytes, clamp, physical_pixels, bytes.len())
                        .unwrap();
                assert_eq!(image.texture_descriptor.mip_level_count, levels);
                assert_eq!(image.data.as_ref().unwrap(), &bytes[128..]);
                assert!(matches!(
                    image.texture_descriptor.format,
                    TextureFormat::Bc1RgbaUnormSrgb
                        | TextureFormat::Bc2RgbaUnormSrgb
                        | TextureFormat::Bc3RgbaUnormSrgb
                ));
                assert!(model::decode_diffuse(&bytes, clamp).is_ok());
            }
            assert!(
                model::decode_diffuse_bounded(&bytes, 0, physical_pixels - 1, bytes.len()).is_err()
            );
            assert!(
                model::decode_diffuse_bounded(&bytes, 0, physical_pixels, bytes.len() - 1).is_err()
            );
            assert!(
                model::decode_diffuse_bounded(
                    &bytes[..bytes.len() - 1],
                    0,
                    physical_pixels,
                    bytes.len()
                )
                .is_err()
            );
            assert!(
                model::decode_diffuse_bounded(&bytes, 4, physical_pixels, bytes.len()).is_err()
            );
        }
    }
}

#[test]
fn incompatible_raw_mip_layouts_refuse_before_decoder_even_when_payload_and_cap_pass() {
    // Independent pinned uploader derivation: raw9x4 BC1 mips24+8, rounded
    // descriptor12x4 mips24+16. The retained32 bytes cannot supply slice24..40.
    for (width, height, levels, bc1_bytes, physical_pixels, failing_mip) in [
        (9, 4, 2, 32, 80, 1),
        (4, 9, 2, 32, 80, 1),
        (9, 9, 2, 80, 208, 1),
        (17, 4, 2, 56, 128, 1),
        (4, 17, 2, 56, 128, 1),
        // First downsample still agrees; the third mip exposes the mismatch.
        (19, 4, 3, 72, 160, 2),
        (4, 19, 3, 72, 160, 2),
    ] {
        for fourcc in [b"DXT1", b"DXT2", b"DXT3", b"DXT4", b"DXT5"] {
            let bytes = authored_dds_extent(width, height, levels, fourcc);
            let expected_bytes = if fourcc == b"DXT1" {
                bc1_bytes
            } else {
                bc1_bytes * 2
            };
            assert_eq!(bytes.len(), 128 + expected_bytes);
            assert_eq!(
                model::diffuse_physical_mip_pixels(width, height, levels).unwrap(),
                physical_pixels
            );
            for clamp in 0..4 {
                let error =
                    model::decode_diffuse_bounded(&bytes, clamp, physical_pixels, bytes.len())
                        .expect_err("Incompatible mip data must never reach upload");
                assert!(
                    error
                        .to_string()
                        .contains(&format!("mip {failing_mip} raw block layout")),
                    "{error}"
                );
                assert!(model::decode_diffuse(&bytes, clamp).is_err());
            }
            // Invalid DDS header would fail in ddsfile; layout refusal wins,
            // proving that the source payload has not entered Image::from_buffer.
            let mut invalid = bytes;
            invalid[4..8].copy_from_slice(&0u32.to_le_bytes());
            let error = model::decode_diffuse(&invalid, 0).expect_err("Predecode layout refusal");
            assert!(error.to_string().contains("raw block layout"), "{error}");
        }
    }
}

#[test]
fn compatible_unaligned_single_mip_and_mip_tails_retain_exact_source_bytes() {
    // Literal independent totals include a base whose multi-mip form refuses.
    for (width, height, levels, bc1_bytes, physical_pixels) in [
        (9, 4, 1, 24, 48),
        (4, 9, 1, 24, 48),
        (5, 5, 3, 48, 96),
        (7, 5, 3, 48, 96),
        (12, 12, 4, 120, 240),
        (16, 16, 5, 184, 368),
    ] {
        for fourcc in [b"DXT1", b"DXT2", b"DXT3", b"DXT4", b"DXT5"] {
            let bytes = authored_dds_extent(width, height, levels, fourcc);
            let expected_bytes = if fourcc == b"DXT1" {
                bc1_bytes
            } else {
                bc1_bytes * 2
            };
            assert_eq!(bytes.len(), 128 + expected_bytes);
            assert_eq!(
                model::diffuse_physical_mip_pixels(width, height, levels).unwrap(),
                physical_pixels
            );
            for clamp in 0..4 {
                let bounded =
                    model::decode_diffuse_bounded(&bytes, clamp, physical_pixels, bytes.len())
                        .unwrap();
                let default = model::decode_diffuse(&bytes, clamp).unwrap();
                assert_eq!(bounded.data.as_ref().unwrap(), &bytes[128..]);
                assert_eq!(default.data, bounded.data);
                assert_eq!(bounded.texture_descriptor.mip_level_count, levels);
            }
            assert!(
                model::decode_diffuse_bounded(&bytes, 0, physical_pixels - 1, bytes.len()).is_err()
            );
            assert!(
                model::decode_diffuse_bounded(&bytes, 0, physical_pixels, bytes.len() - 1).is_err()
            );
        }
    }
}

#[test]
fn hostile_dimensions_optional_mip_header_and_unsupported_formats_never_expand_admission() {
    let valid = authored_dds_extent(4, 4, 1, b"DXT1");
    for (offset, value) in [
        (12, 0),
        (16, 0),
        (12, u32::MAX),
        (16, u32::MAX),
        (16, 16385),
    ] {
        let mut bytes = valid.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(model::decode_diffuse_bounded(&bytes, 0, u64::MAX, bytes.len()).is_err());
    }
    for (width, height, levels) in [
        (u32::MAX, 1, 1),
        (1, u32::MAX, 1),
        (4, 4, 0),
        (4, 4, u32::MAX),
        (4, 4, 4),
    ] {
        assert!(model::diffuse_physical_mip_pixels(width, height, levels).is_err());
    }
    let mut bytes = valid.clone();
    bytes[28..32].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        model::decode_diffuse_bounded(&bytes, 0, 16, bytes.len())
            .unwrap()
            .texture_descriptor
            .mip_level_count,
        1
    );
    bytes[8..12].copy_from_slice(&0xa1007u32.to_le_bytes());
    assert!(model::decode_diffuse_bounded(&bytes, 0, u64::MAX, bytes.len()).is_err());
    bytes[28..32].copy_from_slice(&0u32.to_le_bytes());
    assert!(model::decode_diffuse_bounded(&bytes, 0, 16, bytes.len()).is_ok());
    for fourcc in [b"NOPE", b"ATI1", b"ATI2", b"DX10"] {
        let mut bytes = valid.clone();
        bytes[84..88].copy_from_slice(fourcc);
        assert!(model::decode_diffuse_bounded(&bytes, 0, 16, bytes.len()).is_err());
    }
    let mut volume = valid;
    volume[8..12].copy_from_slice(&0x881007u32.to_le_bytes());
    volume[24..28].copy_from_slice(&2u32.to_le_bytes());
    assert!(model::decode_diffuse_bounded(&volume, 0, 16, volume.len()).is_err());
}
fn archive(folder: &[u8], name: &[u8], payload: &[u8]) -> Vec<u8> {
    let table = 54 + folder.len();
    let offset = table + 16 + name.len() + 1;
    let mut bytes = vec![0; offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, v) in [
        (4, 104u32),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 1),
        (24, folder.len() as u32 + 1),
        (28, name.len() as u32 + 1),
        (44, 1),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    bytes[52] = folder.len() as u8 + 1;
    bytes[53..53 + folder.len()].copy_from_slice(folder);
    bytes[table..table + 8].copy_from_slice(&1u64.to_le_bytes());
    bytes[table + 8..table + 12].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[table + 12..table + 16].copy_from_slice(&(offset as u32).to_le_bytes());
    bytes[table + 16..table + 16 + name.len()].copy_from_slice(name);
    bytes.extend(payload);
    bytes
}
struct Fixture {
    root: PathBuf,
    request: Arc<Request>,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../local")
            .join(format!(
                "v3-view-21-fixture-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(root.join("install/Data")).unwrap();
        let doc = document(SOURCE);
        let mut request = request(&doc);
        let xml = archive(b"menus", b"authored.xml", SOURCE.as_bytes());
        let dds = dds();
        let texture = archive(b"textures", b"authored.dds", &dds);
        request.source.archive_sha256 = format!("{:x}", Sha256::digest(&xml));
        request.texture.archive_sha256 = format!("{:x}", Sha256::digest(&texture));
        request.texture.payload_sha256 = format!("{:x}", Sha256::digest(&dds));
        fs::write(root.join("install/Data/xml.bsa"), xml).unwrap();
        fs::write(root.join("install/Data/dds.bsa"), texture).unwrap();
        Self {
            root,
            request: Arc::new(request),
        }
    }
    fn options(&self) -> crate::Options {
        let mut options = crate::Options::try_parse_from([
            "fallout-preview",
            "--install",
            self.root.join("install").to_str().unwrap(),
            "--menu-image",
            "request.json",
            "--report",
            self.root.join("report.json").to_str().unwrap(),
        ])
        .unwrap();
        options.image_request = Some(self.request.clone());
        options
    }
}
fn until(app: &mut App, done: impl Fn(&crate::Phase) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(&app.world().resource::<crate::Loading>().phase) {
        assert!(Instant::now() < deadline, "source transition failed");
        app.update();
        std::thread::yield_now();
    }
}
fn window(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(app.world())
        .unwrap()
}
#[test]
fn actual_source_image_archive_sampler_labels_and_owned_image_retirement_work_through_host() {
    let fixture = Fixture::new();
    let mut app = crate::tests::loading_app(crate::Phase::WaitingForWindow);
    app.insert_resource(fixture.options());
    app.update();
    assert!(!fixture.root.join("report.json").exists());
    let window = window(&mut app);
    app.world_mut()
        .write_message(bevy::window::WindowCreated { window });
    app.update();
    until(&mut app, |p| matches!(p, crate::Phase::Ready(_)));
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .iter()
        .next()
        .unwrap()
        .1;
    assert_eq!(image.data.as_ref().unwrap(), &dds()[128..]);
    let ImageSampler::Descriptor(sampler) = &image.sampler else {
        panic!("explicit sampler missing")
    };
    assert_eq!(sampler.mag_filter, ImageFilterMode::Nearest);
    assert_eq!(sampler.min_filter, ImageFilterMode::Nearest);
    assert_eq!(sampler.mipmap_filter, ImageFilterMode::Nearest);
    assert_eq!(sampler.address_mode_u, ImageAddressMode::ClampToEdge);
    assert_eq!(sampler.address_mode_v, ImageAddressMode::ClampToEdge);
    assert_eq!(sampler.lod_max_clamp, 0.);
    let mut query = app.world_mut().query::<&ImageView>();
    let views = query.iter(app.world()).collect::<Vec<_>>();
    assert_eq!(views.len(), 2);
    for v in views {
        assert_eq!(v.tile.epoch, 7);
        assert_eq!(
            v.texture.payload_sha256,
            fixture.request.texture.payload_sha256
        );
        assert_eq!(v.uv_bits, [0x3e800000, 0, 0x3f400000, 0x3f800000]);
    }
    let unrelated = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    let entity = app.world_mut().spawn(Transform::IDENTITY).id();
    app.world_mut()
        .write_message(bevy::window::WindowCloseRequested { window });
    app.update();
    until(&mut app, |p| matches!(p, crate::Phase::Cancelled));
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    assert!(
        app.world()
            .resource::<Assets<Image>>()
            .get(&unrelated)
            .is_some()
    );
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert!(
        app.world()
            .resource::<Assets<crate::material::InspectionMaterial>>()
            .is_empty()
    );
    assert!(app.world().get_entity(entity).is_ok());
    assert_eq!(
        app.world_mut()
            .query::<&ImageView>()
            .iter(app.world())
            .count(),
        0
    );
}
#[test]
fn missing_duplicate_conflicting_or_changed_dds_source_never_admits_image_or_creates_report() {
    for failure in ["missing", "duplicate", "conflict", "payload", "archive"] {
        let mut fixture = Fixture::new();
        let original = fixture.root.join("install/Data/dds.bsa");
        match failure {
            "missing" => fs::remove_file(&original).unwrap(),
            "duplicate" => {
                fs::copy(&original, fixture.root.join("install/Data/duplicate.bsa")).unwrap();
            }
            "conflict" => {
                let mut bytes = dds();
                bytes[128] = 0;
                fs::write(
                    fixture.root.join("install/Data/conflict.bsa"),
                    archive(b"textures", b"authored.dds", &bytes),
                )
                .unwrap();
            }
            "payload" => {
                Arc::get_mut(&mut fixture.request)
                    .unwrap()
                    .texture
                    .payload_sha256 = "f".repeat(64)
            }
            "archive" => {
                Arc::get_mut(&mut fixture.request)
                    .unwrap()
                    .texture
                    .archive_sha256 = "f".repeat(64)
            }
            _ => unreachable!(),
        }
        let mut app = crate::tests::loading_app(crate::Phase::WaitingForWindow);
        app.insert_resource(fixture.options());
        let window = window(&mut app);
        app.world_mut()
            .write_message(bevy::window::WindowCreated { window });
        app.update();
        until(&mut app, |p| matches!(p, crate::Phase::Failed(_)));
        assert!(app.world().resource::<Assets<Image>>().is_empty());
        assert!(app.world().resource::<Assets<Mesh>>().is_empty());
        assert!(!fixture.root.join("report.json").exists(), "{failure}");
    }
}
#[test]
fn completed_image_source_after_cancel_stays_owned_until_drain_and_cannot_publish() {
    let fixture = Fixture::new();
    let options = fixture.options();
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let job = loading::Job::start(7, move |context| {
        let result = crate::prepare_scene(&options, &context, 7).map_err(|e| e.to_string())?;
        entered.send(()).unwrap();
        gate.recv().unwrap();
        Ok(result)
    })
    .unwrap();
    started.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut app = crate::tests::loading_app(crate::Phase::Preparing(job));
    app.world_mut()
        .resource_mut::<crate::input::Actions>()
        .cancel_loading = true;
    app.update();
    assert!(matches!(
        app.world().resource::<crate::Loading>().phase,
        crate::Phase::Draining(_)
    ));
    release.send(()).unwrap();
    until(&mut app, |p| matches!(p, crate::Phase::Cancelled));
    assert!(app.world().resource::<Assets<Image>>().is_empty());
    assert!(app.world().resource::<Assets<Mesh>>().is_empty());
    assert_eq!(
        app.world_mut()
            .query::<&ImageView>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn actual_image_admission_refuses_stale_labels_uv_sources_and_wrong_draw_cardinality() {
    let fixture = Fixture::new();
    for failure in [
        "epoch",
        "root",
        "hash",
        "path",
        "uv",
        "sampling",
        "images",
        "models",
        "parts",
        "texture-slot",
        "instances",
    ] {
        let request = fixture.request.clone();
        let install = fixture.root.join("install");
        let mut job = loading::Job::start(7, move |context| {
            let (prepared, _, view) = load(&install, &request, Limits::default(), &context, 7)
                .map_err(|e| e.to_string())?;
            Ok((prepared, view))
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut prepared, mut view) = loop {
            match job.poll(7) {
                loading::Poll::Ready(result) => break result,
                loading::Poll::Pending => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
                loading::Poll::Failed(error) => panic!("image preparation failed: {error}"),
                _ => panic!("image preparation ended without a result"),
            }
        };
        match failure {
            "epoch" => view.tile.epoch = 8,
            "root" => view.tile.root_node += 1,
            "hash" | "path" => {
                view.texture = Arc::new(rectangles::Receipt {
                    path: if failure == "path" {
                        AssetPath::new(b"menus/authored.xml").unwrap()
                    } else {
                        view.texture.path.clone()
                    },
                    archive_sha256: if failure == "hash" {
                        "bad".into()
                    } else {
                        view.texture.archive_sha256.clone()
                    },
                    payload_sha256: view.texture.payload_sha256.clone(),
                });
            }
            "uv" => view.uv_bits[0] = f32::NAN.to_bits(),
            "sampling" => view.sampling = "guessed",
            "images" => prepared.images.clear(),
            "models" => prepared.models.clear(),
            "parts" => prepared.models[0].parts.clear(),
            "texture-slot" => prepared.models[0].parts[0].texture = Some(1),
            "instances" => prepared.instances.clear(),
            _ => unreachable!(),
        }
        assert!(
            crate::upload::Queue::new_image(7, prepared, view).is_err(),
            "{failure}"
        );
    }
}
