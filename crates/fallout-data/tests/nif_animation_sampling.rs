//! Independent mathematical expectations; no original playback acceptance.
use fallout_data::nif_animation::{
    keyframe::{Group, Key},
    sampling::{self, Budget, Limits},
};

fn key<const N: usize>(time: f32, value: [u32; N]) -> Key<N> {
    Key {
        time_bits: time.to_bits(),
        value_bits: value,
        forward_bits: None,
        backward_bits: None,
        tbc_bits: None,
    }
}
fn scalar(tag: u32) -> Group<1> {
    Group {
        declared_keys: 3,
        key_type: Some(tag),
        keys: vec![
            key(0., [0x8000_0000]),
            key(2., [8f32.to_bits()]),
            key(4., [(-4f32).to_bits()]),
        ],
    }
}
fn error(error: fallout_data::Error, reason: &str) {
    assert!(
        error.to_string().contains(reason),
        "expected {reason}: {error}"
    );
}
#[test]
fn exact_keys_preserve_source_bits_signed_zero_and_indices() {
    let source = scalar(1);
    let mut budget = Budget::new(Default::default());
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    for (id, time) in [0., 2., 4.].into_iter().enumerate() {
        let sample = view.sample(time, &mut budget).unwrap();
        assert_eq!(sample.source_key_indices, [id, id]);
        assert_eq!(sample.source_value_bits, Some(source.keys[id].value_bits));
        assert_eq!(
            sample.evaluated_f64_bits,
            [f64::from(f32::from_bits(source.keys[id].value_bits[0])).to_bits()]
        );
        assert_eq!(sample.alpha_f64_bits, 0f64.to_bits());
    }
    assert_eq!(
        view.sample(-0., &mut budget).unwrap().evaluated_f64_bits,
        [(-0f64).to_bits()]
    );
}
#[test]
fn analytical_linear_scalar_vector_and_step_values() {
    let source = scalar(1);
    let mut budget = Budget::new(Default::default());
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    for (time, expected, pair) in [
        (0.5, 2., [0, 1]),
        (1., 4., [0, 1]),
        (3., 2., [1, 2]),
        (3.5, -1., [1, 2]),
    ] {
        let sample = view.sample(time, &mut budget).unwrap();
        assert_eq!(f64::from_bits(sample.evaluated_f64_bits[0]), expected);
        assert_eq!(sample.source_key_indices, pair);
        assert!(sample.source_value_bits.is_none());
    }
    let source = Group {
        declared_keys: 2,
        key_type: Some(1),
        keys: vec![
            key(-1., [0, 4f32.to_bits(), (-8f32).to_bits()]),
            key(3., [8f32.to_bits(), 0, 4f32.to_bits()]),
        ],
    };
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    let sample = view.sample(1., &mut budget).unwrap();
    assert_eq!(sample.evaluated_f64_bits.map(f64::from_bits), [4., 2., -2.]);
    let source = scalar(5);
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    for (time, id, pair) in [
        (1., 0, [0, 1]),
        (2., 1, [1, 1]),
        (3., 1, [1, 2]),
        (4., 2, [2, 2]),
    ] {
        let sample = view.sample(time, &mut budget).unwrap();
        assert_eq!(sample.source_key_indices, pair);
        assert_eq!(sample.source_value_bits, Some(source.keys[id].value_bits));
    }
}
#[test]
fn absence_singleton_and_no_extrapolation_are_explicit() {
    let mut budget = Budget::new(Default::default());
    let source = Group::<1> {
        declared_keys: 0,
        key_type: None,
        keys: vec![],
    };
    assert!(sampling::prepare(&source, &mut budget).unwrap().is_none());
    let source = Group {
        declared_keys: 1,
        key_type: Some(1),
        keys: vec![key(2., [3f32.to_bits()])],
    };
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    assert_eq!(
        view.sample(2., &mut budget).unwrap().source_value_bits,
        Some([3f32.to_bits()])
    );
    for time in [0., 3.] {
        error(view.sample(time, &mut budget).unwrap_err(), "extrapolate");
    }
    let source = scalar(5);
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    for time in [-1., 5., f64::MAX] {
        error(view.sample(time, &mut budget).unwrap_err(), "extrapolate");
    }
}
#[test]
fn duplicate_unsorted_signed_zero_times_rejected_without_modifying_source() {
    for times in [[0., 0., 4.], [0., -0., 4.], [2., 1., 4.], [0., 4., 2.]] {
        let mut source = scalar(1);
        for (key, time) in source.keys.iter_mut().zip(times) {
            key.time_bits = f32::to_bits(time);
        }
        let before = serde_json::to_value(&source).unwrap();
        let mut budget = Budget::new(Default::default());
        error(
            sampling::prepare(&source, &mut budget).err().unwrap(),
            "not strictly increasing",
        );
        assert_eq!(serde_json::to_value(&source).unwrap(), before);
    }
}
#[test]
fn unsupported_tags_cardinality_and_optional_layout_fields_rejected() {
    for tag in [0, 2, 3, 4, 6, u32::MAX] {
        let source = scalar(tag);
        let mut budget = Budget::new(Default::default());
        error(
            sampling::prepare(&source, &mut budget).err().unwrap(),
            "tag is unadmitted",
        );
    }
    let mut source = scalar(1);
    source.declared_keys = 2;
    error(
        sampling::prepare(&source, &mut Budget::new(Default::default()))
            .err()
            .unwrap(),
        "cardinality",
    );
    let source = Group::<1> {
        declared_keys: 0,
        key_type: Some(1),
        keys: vec![],
    };
    error(
        sampling::prepare(&source, &mut Budget::new(Default::default()))
            .err()
            .unwrap(),
        "contradictory present tag",
    );
    for field in 0..3 {
        let mut source = scalar(1);
        match field {
            0 => source.keys[0].forward_bits = Some([0]),
            1 => source.keys[0].backward_bits = Some([0]),
            _ => source.keys[0].tbc_bits = Some([0; 3]),
        }
        error(
            sampling::prepare(&source, &mut Budget::new(Default::default()))
                .err()
                .unwrap(),
            "fields contradict",
        );
    }
    let source = Group::<4> {
        declared_keys: 0,
        key_type: None,
        keys: vec![],
    };
    error(
        sampling::prepare(&source, &mut Budget::new(Default::default()))
            .err()
            .unwrap(),
        "only scalar/vector",
    );
}
#[test]
fn nonfinite_source_and_request_words_rejected() {
    for word in [0x7fc0_0001, 0x7f80_0000, 0xff80_0000] {
        for time in [false, true] {
            let mut source = scalar(1);
            if time {
                source.keys[1].time_bits = word;
            } else {
                source.keys[1].value_bits[0] = word;
            }
            error(
                sampling::prepare(&source, &mut Budget::new(Default::default()))
                    .err()
                    .unwrap(),
                "nonfinite source",
            );
        }
    }
    let source = scalar(1);
    let mut budget = Budget::new(Default::default());
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    for time in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        error(
            view.sample(time, &mut budget).unwrap_err(),
            "nonfinite requested",
        );
    }
}
#[test]
fn finite_extremes_subnormals_and_opposite_sign_cancellation_remain_finite() {
    let source = Group {
        declared_keys: 2,
        key_type: Some(1),
        keys: vec![
            key(0., [0x7f7f_ffff, 1, 0x8000_0001]),
            key(1., [0xff7f_ffff, 2, 0x8000_0002]),
        ],
    };
    let mut budget = Budget::new(Default::default());
    let view = sampling::prepare(&source, &mut budget).unwrap().unwrap();
    let sample = view.sample(0.5, &mut budget).unwrap();
    let values = sample.evaluated_f64_bits.map(f64::from_bits);
    assert_eq!(values[0], 0.);
    assert!(values.iter().all(|v| v.is_finite()));
    assert_eq!(values[1], 1.5 * f64::from(f32::from_bits(1)));
    assert_eq!(values[2], -values[1]);
}
#[test]
fn validation_and_repeated_sampling_have_exact_work_boundaries() {
    let source = scalar(1);
    let mut exact = Budget::new(Limits {
        validation_work: 7,
        sampling_work: 12,
    });
    let view = sampling::prepare(&source, &mut exact).unwrap().unwrap();
    // 1 request,2 endpoints,1 midpoint,2 scalar/alpha units each.
    for time in [1., 3.] {
        view.sample(time, &mut exact).unwrap();
    }
    assert_eq!(exact.usage().validation_units, 7);
    assert_eq!(exact.usage().sampling_units, 12);
    error(
        view.sample(2., &mut exact).unwrap_err(),
        "request-work budget exceeded",
    );
    let mut under = Budget::new(Limits {
        validation_work: 6,
        sampling_work: 100,
    });
    error(
        sampling::prepare(&source, &mut under).err().unwrap(),
        "validation-work budget exceeded",
    );
    let mut under = Budget::new(Limits {
        validation_work: 7,
        sampling_work: 5,
    });
    let view = sampling::prepare(&source, &mut under).unwrap().unwrap();
    error(
        view.sample(1., &mut under).unwrap_err(),
        "request-work budget exceeded",
    );
    assert_eq!(under.usage().sampling_units, 4);
}
