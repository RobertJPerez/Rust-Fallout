//! Literal source controls and independently derived complete node matrices.
use fallout_data::{
    nif_animation::pose::spline::{self, Contract, Limits, LocalPolicy, Request},
    nif_skin::pose::Affine,
};
use sha2::{Digest, Sha256};
const NULL: u32 = u32::MAX;
const NAME: &[u8] = b"Compact Node\0\xff";
const R90: [[f32; 3]; 3] = [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]];
const RM90: [[f32; 3]; 3] = [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]];
fn words(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend(value.to_le_bytes());
    }
}
fn floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend(value.to_bits().to_le_bytes());
    }
}
fn node(
    name: u32,
    children: &[u32],
    t: [f32; 3],
    r: [[f32; 3]; 3],
    scale: f32,
    controller: u32,
) -> Vec<u8> {
    let mut out = Vec::new();
    words(&mut out, &[name, 0, controller, 0x1234_5678]);
    floats(&mut out, &t);
    for row in r {
        floats(&mut out, &row);
    }
    floats(&mut out, &[scale]);
    words(&mut out, &[0, NULL, children.len() as u32]);
    words(&mut out, children);
    words(&mut out, &[0]);
    out
}
fn blocks() -> Vec<(&'static str, Vec<u8>)> {
    let mut controller = Vec::new();
    words(&mut controller, &[NULL]);
    controller.extend(0x19a5u16.to_le_bytes());
    floats(&mut controller, &[-7., 11., 700., 800.]);
    words(&mut controller, &[1, 3]);
    let mut interp = Vec::new();
    floats(&mut interp, &[-2., 2.]);
    words(&mut interp, &[5, 4]);
    floats(&mut interp, &[-0., 800., 900., 2., -3., 4., -5., 777.]);
    words(&mut interp, &[2, 65535, 14]);
    floats(&mut interp, &[0., 2., -17., 19., 2., 1.]);
    assert_eq!(interp.len(), 84);
    let mut data = Vec::new();
    words(&mut data, &[2, 0x8000_0000, 0x3f80_0000, 20]);
    let raw: [i16; 20] = [
        -32768, 12345, -32767, 32767, -32767, -32767, 32767, -32767, 32767, 32767, -32767, 32767,
        32767, -32767, -32767, 0, 0, 32767, -12345, 32767,
    ];
    for value in raw {
        data.extend(value.to_le_bytes());
    }
    vec![
        ("NiNode", node(0, &[1], [10., 20., 30.], R90, 3., NULL)),
        ("NiNode", node(1, &[], [777., 888., 999.], RM90, 777., 2)),
        ("NiTransformController", controller),
        ("NiBSplineCompTransformInterpolator", interp),
        ("NiBSplineBasisData", 4u32.to_le_bytes().to_vec()),
        ("NiBSplineData", data),
    ]
}
fn container(blocks: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, 34]);
    out.extend([0; 3]);
    let mut names = Vec::new();
    for (name, _) in blocks {
        if !names.contains(name) {
            names.push(*name);
        }
    }
    out.extend((names.len() as u16).to_le_bytes());
    for name in &names {
        words(&mut out, &[name.len() as u32]);
        out.extend(name.as_bytes());
    }
    for (name, _) in blocks {
        out.extend((names.iter().position(|n| n == name).unwrap() as u16).to_le_bytes());
    }
    for (_, data) in blocks {
        words(&mut out, &[data.len() as u32]);
    }
    words(&mut out, &[2, NAME.len() as u32]);
    for name in [b"root".as_slice(), NAME] {
        words(&mut out, &[name.len() as u32]);
        out.extend(name);
    }
    words(&mut out, &[0]);
    for (_, data) in blocks {
        out.extend(data);
    }
    words(&mut out, &[1, 0]);
    out
}
fn request(bytes: &[u8], time: f64) -> Request<'static> {
    Request {
        expected_source_sha256: Sha256::digest(bytes).into(),
        object: 1,
        controller: 2,
        node_name_bytes: NAME,
        source_time: time,
        contract: Contract::EngineeringOpenUniformCubicComponentsV1,
        local_policy: LocalPolicy::ReplaceTranslationScaleKeepStoredNiAvRotation,
    }
}
fn fail(bytes: &[u8], req: Request<'_>, limits: Limits, reason: &str) {
    let saved = bytes.to_vec();
    let error = spline::evaluate(bytes, "literal", req, limits)
        .unwrap_err()
        .to_string();
    assert!(error.contains(reason), "{reason}: {error}");
    assert_eq!(bytes, saved);
}
fn fail_any(bytes: &[u8], req: Request<'_>, limits: Limits) {
    let saved = bytes.to_vec();
    assert!(spline::evaluate(bytes, "literal", req, limits).is_err());
    assert_eq!(bytes, saved);
}
#[test]
fn literal_cubic_source_pose_keeps_rotation_and_applies_static_parent_once() {
    let source_blocks = blocks();
    let bytes = container(&source_blocks);
    for (time, x, scale) in [
        (-2., -2., 1.),
        (-1., -1.375, 1.59375),
        (0., 0., 2.),
        (2., 2., 3.),
    ] {
        let result =
            spline::evaluate(&bytes, "literal", request(&bytes, time), Default::default()).unwrap();
        let local: Affine = [
            [0., scale, 0., x],
            [-scale, 0., 0., 2.],
            [0., 0., scale, -2.],
        ];
        let world: Affine = [
            [3. * scale, 0., 0., 4.],
            [0., 3. * scale, 0., 20. + 3. * x],
            [0., 0., 3. * scale, 24.],
        ];
        assert_eq!(result.local, local);
        assert_eq!(result.source_world, world);
        assert_eq!(result.requested_time_f64_bits, time.to_bits());
        assert_eq!(
            result.translation_compact_controls,
            [
                -32767, 32767, -32767, -32767, 32767, -32767, 32767, 32767, -32767, 32767, 32767,
                -32767
            ]
        );
        assert_eq!(result.scale_compact_controls, [-32767, 0, 0, 32767]);
        assert_eq!(result.translation.source_handle, 2);
        assert_eq!(result.scale.source_handle, 14);
        assert_eq!(result.translation.window_scalars, 12);
        assert_eq!(result.scale.window_scalars, 4);
        assert_eq!(
            result.translation.window_offset,
            result.translation.source_blocks[1].offset + 20
        );
        assert_eq!(
            result.scale.window_offset,
            result.scale.source_blocks[1].offset + 44
        );
        for diagnostic in [&result.translation, &result.scale] {
            assert_eq!(diagnostic.basis_count, 4);
            let raw: Vec<u8> = if diagnostic.source_handle == 2 {
                &result.translation_compact_controls
            } else {
                &result.scale_compact_controls
            }
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
            assert_eq!(
                diagnostic.window_sha256,
                format!("{:x}", Sha256::digest(&raw))
            );
            assert_eq!(
                &bytes[diagnostic.window_offset..diagnostic.window_offset + raw.len()],
                raw
            );
            for span in &diagnostic.source_blocks {
                assert_eq!(
                    span.sha256,
                    format!(
                        "{:x}",
                        Sha256::digest(&bytes[span.offset..span.offset + span.bytes])
                    )
                );
            }
        }
        assert_eq!(
            result
                .static_ancestors
                .iter()
                .map(|a| a.source.block)
                .collect::<Vec<_>>(),
            [0]
        );
        assert_eq!(result.object.block, 1);
        assert_eq!(result.controller.block, 2);
        assert_eq!(result.node_name_bytes, NAME);
        assert_eq!(result.object_flags, 0x1234_5678);
        assert_eq!(result.source_local.scale_bits, 777f32.to_bits());
        assert_eq!(
            result.unapplied_controller_fields.frequency_bits,
            (-7f32).to_bits()
        );
        assert_eq!(
            result.unapplied_controller_fields.start_bits,
            700f32.to_bits()
        );
        assert_eq!(result.sample_work.sampling_units, 74);
        assert_eq!(result.usage.whole_source_sha256_traversals, 1);
        assert_eq!(result.usage.component_decodes, 1);
        assert_eq!(result.usage.scene_decodes, 1);
        assert!(!result.runtime_ready && !result.retail_behavior_verified);
    }
}
#[test]
fn direct_links_raw_name_absence_rotation_and_time_are_not_repaired() {
    let source_blocks = blocks();
    let bytes = container(&source_blocks);
    let req = request(&bytes, 0.);
    fail(
        &bytes,
        Request {
            expected_source_sha256: [0; 32],
            ..req
        },
        Default::default(),
        "SHA256 differs",
    );
    fail(
        &bytes,
        Request {
            node_name_bytes: b"compact node",
            ..req
        },
        Default::default(),
        "raw node name differs",
    );
    fail(
        &bytes,
        Request {
            controller: 3,
            ..req
        },
        Default::default(),
        "object.controller differs",
    );
    for time in [-3., 3.] {
        fail(
            &bytes,
            Request {
                source_time: time,
                ..req
            },
            Default::default(),
            "extrapolate",
        );
    }
    fail(
        &bytes,
        Request {
            source_time: f64::NAN,
            ..req
        },
        Default::default(),
        "time must be finite",
    );
    for (block, offset, value, reason) in [
        (2, 22, 0, "controller.target differs"),
        (2, 0, 2, "controller chain"),
        (3, 52, 0, "active rotation"),
        (3, 48, 65535, "absent compact handle"),
        (3, 56, 65535, "absent compact handle"),
        (3, 56, u32::MAX, "window exceeds"),
        (4, 0, 3, "cubic basis count"),
        (3, 8, 4, "wrong target kind"),
        (3, 12, 5, "wrong target kind"),
    ] {
        let mut changed = blocks();
        changed[block].1[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        let input = container(&changed);
        fail(&input, request(&input, 0.), Default::default(), reason);
    }
    let mut changed = blocks();
    changed[0].1[8..12].copy_from_slice(&2u32.to_le_bytes());
    let input = container(&changed);
    fail(
        &input,
        request(&input, 0.),
        Default::default(),
        "ancestor 0 controller is unapplied",
    );
}
#[test]
fn exact_output_work_metadata_and_declared_combined_admission_refuse_one_below() {
    let bytes = container(&blocks());
    let req = request(&bytes, 0.);
    let limits = Limits::default();
    let result = spline::evaluate(&bytes, "literal", req, limits).unwrap();
    for (exact, below, reason) in [
        (
            Limits {
                array_bytes: result.usage.charged_output_bytes,
                ..limits
            },
            Limits {
                array_bytes: result.usage.charged_output_bytes - 1,
                ..limits
            },
            "storage budget",
        ),
        (
            Limits {
                work_units: result.usage.charged_work_units,
                ..limits
            },
            Limits {
                work_units: result.usage.charged_work_units - 1,
                ..limits
            },
            "work budget",
        ),
        (
            Limits {
                metadata_array_bytes: result.usage.metadata_peak_bytes,
                ..limits
            },
            Limits {
                metadata_array_bytes: result.usage.metadata_peak_bytes - 1,
                ..limits
            },
            "storage budget",
        ),
        (
            Limits {
                metadata_work_units: result.usage.metadata_work_units,
                ..limits
            },
            Limits {
                metadata_work_units: result.usage.metadata_work_units - 1,
                ..limits
            },
            "work budget",
        ),
        (
            Limits {
                decoder_array_admission_bytes: result.usage.decoder_array_admission_bytes,
                ..limits
            },
            Limits {
                decoder_array_admission_bytes: result.usage.decoder_array_admission_bytes - 1,
                ..limits
            },
            "decoder array admission",
        ),
        (
            Limits {
                decoder_check_admission_units: result.usage.decoder_check_admission_units,
                ..limits
            },
            Limits {
                decoder_check_admission_units: result.usage.decoder_check_admission_units - 1,
                ..limits
            },
            "decoder check admission",
        ),
        (
            Limits {
                ancestry_depth: 2,
                ..limits
            },
            Limits {
                ancestry_depth: 1,
                ..limits
            },
            "ancestry depth",
        ),
    ] {
        let actual = spline::evaluate(&bytes, "literal", req, exact).unwrap();
        assert_eq!(actual.local, result.local);
        assert_eq!(actual.source_world, result.source_world);
        fail(&bytes, req, below, reason);
    }
    let declared = bytes.len()
        + result.usage.decoder_array_admission_bytes
        + limits.metadata_array_bytes
        + limits.array_bytes;
    spline::evaluate(
        &bytes,
        "literal",
        req,
        Limits {
            combined_retained_bytes: declared,
            ..limits
        },
    )
    .unwrap();
    fail(
        &bytes,
        req,
        Limits {
            combined_retained_bytes: declared - 1,
            ..limits
        },
        "combined storage admission",
    );
}

#[test]
fn compact_pose_preserves_signed_minimum_and_negative_or_zero_channel_ranges() {
    let mut source_blocks = blocks();
    source_blocks[3].1[48..52].copy_from_slice(&0u32.to_le_bytes());
    source_blocks[3].1[60..64].copy_from_slice(&(-0.0f32).to_bits().to_le_bytes());
    source_blocks[3].1[64..68].copy_from_slice(&(-2.0f32).to_bits().to_le_bytes());
    let bytes = container(&source_blocks);
    let result =
        spline::evaluate(&bytes, "literal", request(&bytes, -2.), Default::default()).unwrap();
    let first = 65_536.0f64 / 32_767.0;
    assert_eq!(result.translation_compact_controls[0], i16::MIN);
    assert_eq!(
        result.translation.sample.evaluated_f64_bits[0],
        first.to_bits()
    );
    assert_eq!(result.local[0][3].to_bits(), first.to_bits());
    assert!(!result.runtime_ready && !result.retail_behavior_verified);

    let mut zero_range = blocks();
    zero_range[3].1[60..64].copy_from_slice(&0.5f32.to_bits().to_le_bytes());
    zero_range[3].1[64..68].copy_from_slice(&0.0f32.to_bits().to_le_bytes());
    let zero_bytes = container(&zero_range);
    let zero = spline::evaluate(
        &zero_bytes,
        "literal",
        request(&zero_bytes, 0.),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        zero.translation.sample.evaluated_f64_bits,
        [0.5f64.to_bits(); 3]
    );
    assert_eq!(zero.local.map(|row| row[3]), [0.5, 0.5, 0.5]);
}

#[test]
fn compact_pose_keeps_zero_and_reflected_scale_without_repair() {
    let mut reflected_blocks = blocks();
    reflected_blocks[3].1[76..80].copy_from_slice(&0.0f32.to_bits().to_le_bytes());
    reflected_blocks[3].1[80..84].copy_from_slice(&1.0f32.to_bits().to_le_bytes());
    let reflected_bytes = container(&reflected_blocks);
    let reflected = spline::evaluate(
        &reflected_bytes,
        "literal",
        request(&reflected_bytes, -2.),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        reflected.scale.sample.evaluated_f64_bits,
        [(-1.0f64).to_bits()]
    );
    assert_eq!(
        reflected.local,
        [[0., -1., 0., -2.], [1., 0., 0., 2.], [0., 0., -1., -2.]]
    );
    assert!(
        reflected
            .source_world
            .iter()
            .flatten()
            .all(|value| value.is_finite())
    );

    let mut zero_blocks = blocks();
    zero_blocks[3].1[76..80].copy_from_slice(&1.0f32.to_bits().to_le_bytes());
    zero_blocks[3].1[80..84].copy_from_slice(&1.0f32.to_bits().to_le_bytes());
    let zero_bytes = container(&zero_blocks);
    let zero = spline::evaluate(
        &zero_bytes,
        "literal",
        request(&zero_bytes, -2.),
        Default::default(),
    )
    .unwrap();
    assert_eq!(zero.scale.sample.evaluated_f64_bits, [0.0f64.to_bits()]);
    assert_eq!(
        zero.local,
        [[0., 0., 0., -2.], [0., 0., 0., 2.], [0., 0., 0., -2.]]
    );
}

#[test]
fn absent_links_and_input_boundaries_refuse_without_mutating_source() {
    let original = container(&blocks());
    let req = request(&original, 0.);
    for (offset, reason) in [
        (8, "missing data or basis link"),
        (12, "missing data or basis link"),
    ] {
        let mut changed = blocks();
        changed[3].1[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let input = container(&changed);
        fail(&input, request(&input, 0.), Default::default(), reason);
    }

    let exact_len = original.len();
    let mut exact = Limits::default();
    exact.components.splines.keyframes.animation.input_bytes = exact_len;
    exact.scene.input_bytes = exact_len;
    spline::evaluate(&original, "literal", req, exact).unwrap();
    exact.components.splines.keyframes.animation.input_bytes = exact_len - 1;
    fail(&original, req, exact, "source input byte budget exceeded");

    let mut truncated = original.clone();
    truncated.pop();
    fail_any(&truncated, request(&truncated, 0.), Default::default());
}
