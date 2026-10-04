//! Source-backed engineering component requests, with independent Bezier math.
use fallout_data::nif_animation::spline::{
    self, components,
    sampling::{self, Budget, Channel, Limits},
};
use sha2::{Digest, Sha256};

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn container(blocks: &[(&str, Vec<u8>)], stream: u32) -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend(words(&[0x1402_0007]));
    bytes.push(1);
    bytes.extend(words(&[11, blocks.len() as u32, stream]));
    bytes.extend([0; 3]);
    bytes.extend((blocks.len() as u16).to_le_bytes());
    for (name, _) in blocks {
        bytes.extend(words(&[name.len() as u32]));
        bytes.extend(name.as_bytes());
    }
    for id in 0..blocks.len() {
        bytes.extend((id as u16).to_le_bytes());
    }
    for (_, payload) in blocks {
        bytes.extend(words(&[payload.len() as u32]));
    }
    bytes.extend(words(&[0, 0, 0]));
    for (_, payload) in blocks {
        bytes.extend(payload);
    }
    bytes.extend(words(&[1, 0]));
    bytes
}
fn blocks(
    channel: Channel,
    n: u32,
    handle: u32,
    raw: &[i16],
    range: f32,
) -> Vec<(&'static str, Vec<u8>)> {
    let (kind, fields) = match channel {
        Channel::Float => (
            "NiBSplineCompFloatInterpolator",
            vec![
                0,
                1f32.to_bits(),
                1,
                2,
                0x8000_0000,
                handle,
                0,
                range.to_bits(),
            ],
        ),
        Channel::Point3 => (
            "NiBSplineCompPoint3Interpolator",
            vec![
                0,
                1f32.to_bits(),
                1,
                2,
                0,
                1,
                0x8000_0000,
                handle,
                0,
                range.to_bits(),
            ],
        ),
        _ => (
            "NiBSplineCompTransformInterpolator",
            vec![
                0,
                1f32.to_bits(),
                1,
                2,
                0,
                1,
                0x8000_0000,
                2f32.to_bits(),
                3f32.to_bits(),
                4f32.to_bits(),
                5f32.to_bits(),
                1f32.to_bits(),
                handle,
                handle,
                handle,
                0,
                range.to_bits(),
                0,
                range.to_bits(),
                0,
                range.to_bits(),
            ],
        ),
    };
    let mut compact = words(&[0, raw.len() as u32]);
    for value in raw {
        compact.extend(value.to_le_bytes());
    }
    vec![
        (kind, words(&fields)),
        ("NiBSplineData", compact),
        ("NiBSplineBasisData", words(&[n])),
    ]
}
fn source(channel: Channel, n: u32, handle: u32, raw: &[i16], range: f32) -> components::Source {
    components::decode(
        &container(&blocks(channel, n, handle, raw, range), 34),
        "cubic.kf",
    )
    .unwrap()
    .1
}
fn evaluate(source: &components::Source, channel: Channel, time: f64) -> sampling::Diagnostic {
    sampling::evaluate(
        source,
        0,
        channel,
        time,
        &mut Budget::new(Limits::default()),
    )
    .unwrap()
}
fn refuses(source: &components::Source, channel: Channel, time: f64, reason: &str) {
    let error = sampling::evaluate(
        source,
        0,
        channel,
        time,
        &mut Budget::new(Limits::default()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains(reason), "{error}");
}
#[test]
fn authored_bezier_all_channels_streams_and_source_remains_exact() {
    for stream in [14, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        for (channel, width) in [
            (Channel::Float, 1),
            (Channel::Point3, 3),
            (Channel::Translation, 3),
            (Channel::Scale, 1),
            (Channel::RotationComponents, 4),
        ] {
            let raw: Vec<i16> = (0..4 * width)
                .map(|i| [-32768, -32767, 0, 1, 32767][i % 5])
                .collect();
            let bytes = container(&blocks(channel, 4, 0, &raw, -2.), stream);
            let (_, source) = components::decode(&bytes, "cubic.kf").unwrap();
            let before = serde_json::to_vec(&source).unwrap();
            for time in [0., 0.125, 0.5, 0.875, 1.] {
                let diagnostic = evaluate(&source, channel, time);
                for component in 0..width {
                    // Independent degree3 Bernstein polynomial, rather than de Boor.
                    let controls: Vec<f64> = (0..4)
                        .map(|i| f64::from(raw[i * width + component]) / 32767.0 * -2.)
                        .collect();
                    let expected = (1. - time).powi(3) * controls[0]
                        + 3. * time * (1. - time).powi(2) * controls[1]
                        + 3. * time.powi(2) * (1. - time) * controls[2]
                        + time.powi(3) * controls[3];
                    let actual = f64::from_bits(diagnostic.sample.evaluated_f64_bits[component]);
                    assert!(
                        (actual - expected).abs()
                            <= 2f64.powi(-42) * (2. * 32768. / 32767. + expected.abs())
                    );
                }
                assert!(!diagnostic.runtime_ready && !diagnostic.retail_behavior_verified);
            }
            assert_eq!(before, serde_json::to_vec(&source).unwrap());
        }
    }
}
#[test]
fn exact_window_hash_offset_and_endpoint_unclamped_minimum() {
    let raw = [17, 31, -32768, -1, 1, 32767, 42];
    let source = source(Channel::Float, 4, 2, &raw, 1.);
    let sample = evaluate(&source, Channel::Float, 0.);
    assert_eq!(sample.sample.evaluated_f64_bits.len(), 1);
    assert!(f64::from_bits(sample.sample.evaluated_f64_bits[0]) < -1.);
    assert_eq!(sample.window_offset, sample.source_blocks[1].offset + 8 + 4);
    let window: Vec<u8> = raw[2..6].iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(
        sample.window_sha256,
        format!("{:x}", Sha256::digest(window))
    );
    assert_eq!(
        f64::from_bits(
            evaluate(&source, Channel::Float, 1.)
                .sample
                .evaluated_f64_bits[0]
        ),
        1.
    );
}
#[test]
fn window_and_interval_admission_never_falls_back_to_static_values() {
    for n in [0, 3, 2_000_001, u32::MAX] {
        refuses(
            &source(Channel::Float, n, 0, &[0; 4], 1.),
            Channel::Float,
            0.,
            "basis count",
        );
    }
    for (handle, reason) in [
        (65535, "absent compact handle"),
        (u32::MAX, "window exceeds"),
        (1, "window exceeds"),
    ] {
        refuses(
            &source(Channel::Float, 4, handle, &[0; 4], 1.),
            Channel::Float,
            0.,
            reason,
        );
    }
    let mut raw = blocks(Channel::Float, 4, 0, &[0; 4], 1.);
    for (word, value, reason) in [
        (1, 0, "strictly increasing"),
        (0, 2f32.to_bits(), "strictly increasing"),
        (2, u32::MAX, "missing data"),
        (3, u32::MAX, "missing data"),
    ] {
        let before = raw[0].1.clone();
        raw[0].1[word * 4..word * 4 + 4].copy_from_slice(&value.to_le_bytes());
        let source = components::decode(&container(&raw, 34), "interval.kf")
            .unwrap()
            .1;
        refuses(&source, Channel::Float, 0., reason);
        raw[0].1 = before;
    }
}
#[test]
fn exact_and_one_under_budgets_admit_before_work() {
    let source = source(Channel::Float, 4, 0, &[0; 4], 1.);
    let diagnostic = evaluate(&source, Channel::Float, 0.5);
    let mut exact = Budget::new(Limits {
        validation_units: diagnostic.work.validation_units,
        sampling_units: diagnostic.work.sampling_units,
    });
    assert!(sampling::evaluate(&source, 0, Channel::Float, 0.5, &mut exact).is_ok());
    for validation in [true, false] {
        let mut under = Budget::new(Limits {
            validation_units: diagnostic.work.validation_units - usize::from(validation),
            sampling_units: diagnostic.work.sampling_units - usize::from(!validation),
        });
        let error = sampling::evaluate(&source, 0, Channel::Float, 0.5, &mut under).unwrap_err();
        assert!(error.to_string().contains("work budget exceeded"));
        if validation {
            assert_eq!(under.usage().sampling_units, 0);
        }
    }
    let mut budget = Budget::new(Limits::default());
    let window = sampling::prepare(&source, 0, Channel::Float, &mut budget).unwrap();
    let charged = budget.usage().validation_units;
    window.sample(0.25, &mut budget).unwrap();
    window.sample(0.75, &mut budget).unwrap();
    assert_eq!(budget.usage().validation_units, charged);
}
#[test]
fn explicit_time_and_channel_refusals_preserve_source() {
    let source = source(Channel::Float, 4, 0, &[0; 4], 0.);
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        refuses(&source, Channel::Float, time, "nonfinite requested");
    }
    for time in [-f64::from_bits(1), f64::from_bits(1f64.to_bits() + 1)] {
        refuses(&source, Channel::Float, time, "extrapolate");
    }
    refuses(&source, Channel::Point3, 0.5, "channel does not match");
    assert_eq!(
        evaluate(&source, Channel::Float, 0.5)
            .sample
            .evaluated_f64_bits,
        [0f64.to_bits()]
    );
}
#[test]
fn contradictory_typed_metadata_counts_links_and_hashes_are_refused() {
    let mut source = source(Channel::Float, 4, 0, &[0; 4], 1.);
    source.components.blocks[0].sha256 = "f".repeat(65);
    refuses(&source, Channel::Float, 0., "identity/span/hash");
    source.components.blocks[0].sha256 = "f".repeat(64);
    if let spline::Data::ControlPoints {
        declared_compact_count,
        ..
    } = &mut source.source.splines.blocks[0].data
    {
        *declared_compact_count = 3;
    }
    refuses(&source, Channel::Float, 0., "cardinality");
    if let spline::Data::ControlPoints {
        declared_compact_count,
        ..
    } = &mut source.source.splines.blocks[0].data
    {
        *declared_compact_count = 4;
    }
    source.source.splines.blocks[0].bytes += 1;
    refuses(&source, Channel::Float, 0., "array span");
    source.source.splines.blocks[0].bytes -= 1;
    if let components::Data::CompactFloat { spline_data, .. } =
        &mut source.components.blocks[0].data
    {
        *spline_data = Some(2);
    }
    refuses(&source, Channel::Float, 0., "wrong decoded kind");
}
#[test]
fn maximum_source_array_uses_local_scratch_at_knots() {
    let n = 2_000_000;
    let raw: Vec<i16> = (0..n)
        .map(|i| [-32768, -32767, 0, 1, 32767][i % 5])
        .collect();
    let source = source(Channel::Float, n as u32, 0, &raw, 1.);
    let mut budget = Budget::new(Limits::default());
    let window = sampling::prepare(&source, 0, Channel::Float, &mut budget).unwrap();
    for time in [0., 1. / (n - 3) as f64, 0.5, 1.] {
        let sample = window.sample(time, &mut budget).unwrap();
        assert!(sample.source_control_indices.iter().all(|i| *i < n));
        assert!(
            sample
                .evaluated_f64_bits
                .iter()
                .all(|v| f64::from_bits(*v).is_finite())
        );
    }
}
