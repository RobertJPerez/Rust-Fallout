use fallout_data::obscript::arguments::{
    self, Convention, DecodeError, Limits, Parameter, Signature, Value,
};

fn parameters(ids: &[u32]) -> Vec<Parameter> {
    ids.iter()
        .map(|&type_id| Parameter {
            type_id,
            optional_word: 0,
        })
        .collect()
}

#[test]
fn retains_all_vanilla_operand_classes_without_coercing_numbers_or_form_variables() {
    let params = parameters(&[0, 1, 2, 5, 8, 3, 3, 44, 45]);
    let mut bytes = vec![9, 0, 3, 0, b'X', 0, 0xff, b'n'];
    bytes.extend_from_slice(&i32::MIN.to_le_bytes());
    bytes.extend_from_slice(&[b'G', 7, 0, 0x34, 0x12, b'Z', b'r', 1, 0, b'f', 42, 0, b'z']);
    let nan_bits = 0x7ff8_0000_0000_1234_u64;
    bytes.extend_from_slice(&nan_bits.to_le_bytes());
    bytes.extend_from_slice(&[b'r', 3, 0, b's', 99, 0]);
    let decoded = arguments::decode(
        &bytes,
        Signature {
            convention: Convention::Default,
            parameters: &params,
        },
        Limits::default(),
    )
    .unwrap();
    let values: Vec<_> = decoded.arguments.iter().map(|arg| &arg.value).collect();
    assert_eq!(
        values,
        [
            &Value::String(&[b'X', 0, 0xff]),
            &Value::SignedInteger(i32::MIN),
            &Value::Global { reference_index: 7 },
            &Value::Short(0x1234),
            &Value::Byte(b'Z'),
            &Value::FormReference { reference_index: 1 },
            &Value::FormVariable { index: 42 },
            &Value::DoubleBits(nan_bits),
            &Value::Variable {
                type_byte: b's',
                index: 99,
                context_reference: Some(3)
            },
        ]
    );
    assert!(decoded.trailing.is_empty());
    assert_eq!(decoded.declared_count, Some(9));
    assert_eq!(
        decoded.arguments[6].bytes.end - decoded.arguments[6].bytes.start,
        3
    );
    if let Value::String(raw) = decoded.arguments[0].value {
        assert!(std::ptr::eq(raw.as_ptr(), bytes[4..].as_ptr()));
    }
}

#[test]
fn message_substitutions_and_absent_optional_counts_remain_distinct() {
    let params = parameters(&[61]);
    let bytes = [
        1, 0, b'r', 1, 0, 2, 0, b'n', 0xff, 0xff, 0xff, 0xff, b'f', 42, 0,
    ];
    let decoded = arguments::decode(
        &bytes,
        Signature {
            convention: Convention::Message,
            parameters: &params,
        },
        Limits::default(),
    )
    .unwrap();
    assert_eq!(decoded.declared_message_count, Some(2));
    assert_eq!(decoded.message_arguments[0].value, Value::SignedInteger(-1));
    assert!(matches!(
        arguments::decode(
            &bytes,
            Signature {
                convention: Convention::Message,
                parameters: &params
            },
            Limits {
                max_arguments: 2,
                ..Limits::default()
            }
        ),
        Err(DecodeError::Limit {
            kind: "argument",
            ..
        })
    ));
    assert_eq!(
        decoded.message_arguments[1].value,
        Value::Variable {
            type_byte: b'f',
            index: 42,
            context_reference: None
        }
    );
    let params = [Parameter {
        type_id: 1,
        optional_word: 1,
    }];
    let signature = Signature {
        convention: Convention::Default,
        parameters: &params,
    };
    let absent = arguments::decode(&[], signature, Limits::default()).unwrap();
    let explicit = arguments::decode(&[0, 0, 0xde, 0xad], signature, Limits::default()).unwrap();
    assert_eq!(absent.declared_count, None);
    assert_eq!(explicit.declared_count, Some(0));
    assert_eq!(explicit.trailing, &[0xde, 0xad]);
}

#[test]
fn rejects_unverified_signatures_extensions_missing_arguments_and_bad_boundaries() {
    let params = parameters(&[1]);
    let signature = Signature {
        convention: Convention::Default,
        parameters: &params,
    };
    for bytes in [
        &[1][..],
        &[1, 0, b'n', 1, 2, 3],
        &[1, 0, b'z', 0, 0],
        &[1, 0, b'r', 1, 0, b's', 2],
    ] {
        assert!(matches!(
            arguments::decode(bytes, signature, Limits::default()),
            Err(DecodeError::Truncated { .. })
        ));
    }
    for bytes in [&[0xff, 0xff][..], &[1, 0, 0xff, 0xff]] {
        assert!(matches!(
            arguments::decode(bytes, signature, Limits::default()),
            Err(DecodeError::Extension { .. })
        ));
    }
    assert!(matches!(
        arguments::decode(&[], signature, Limits::default()),
        Err(DecodeError::ArgumentCount { .. })
    ));
    assert!(matches!(
        arguments::decode(&[2, 0], signature, Limits::default()),
        Err(DecodeError::ArgumentCount { .. })
    ));
    assert_eq!(
        arguments::decode(
            &[],
            Signature {
                convention: Convention::Unknown,
                parameters: &[]
            },
            Limits::default()
        )
        .unwrap_err(),
        DecodeError::Convention
    );
    let uncertain = [Parameter {
        type_id: 1,
        optional_word: 2,
    }];
    assert!(matches!(
        arguments::decode(
            &[],
            Signature {
                convention: Convention::Default,
                parameters: &uncertain
            },
            Limits::default()
        ),
        Err(DecodeError::OptionalWord { .. })
    ));
    for type_id in [22, 46, 70, u32::MAX] {
        let params = parameters(&[type_id]);
        assert!(matches!(
            arguments::decode(
                &[1, 0, 0, 0],
                Signature {
                    convention: Convention::Default,
                    parameters: &params
                },
                Limits::default()
            ),
            Err(DecodeError::ParameterType { .. })
        ));
    }
    let signature = Signature {
        convention: Convention::Message,
        parameters: &[],
    };
    assert_eq!(
        arguments::decode(&[0, 0, 10, 0], signature, Limits::default()).unwrap_err(),
        DecodeError::MessageCount { count: 10 }
    );
}

#[test]
fn mutation_and_budget_checks_preserve_bounded_argument_extents() {
    let params = parameters(&[1, 3]);
    let signature = Signature {
        convention: Convention::Default,
        parameters: &params,
    };
    let seed = [2, 0, b'n', 1, 0, 0, 0, b'r', 1, 0];
    let check = |bytes: &[u8]| {
        let result =
            std::panic::catch_unwind(|| arguments::decode(bytes, signature, Limits::default()));
        if let Ok(decoded) = result.expect("untrusted arguments must not unwind") {
            let mut end = 2;
            for arg in decoded.arguments {
                assert!(
                    arg.bytes.start >= end
                        && arg.bytes.start < arg.bytes.end
                        && arg.bytes.end <= bytes.len()
                );
                end = arg.bytes.end;
            }
        }
    };
    for end in 0..=seed.len() {
        check(&seed[..end]);
    }
    for at in 0..seed.len() {
        for byte in [0, 0xff, b'z', b'f'] {
            let mut changed = seed;
            changed[at] = byte;
            check(&changed);
        }
    }
    for limits in [
        Limits {
            max_bytes: 9,
            ..Limits::default()
        },
        Limits {
            max_arguments: 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            arguments::decode(&seed, signature, limits),
            Err(DecodeError::Limit { .. })
        ));
    }
    let params = parameters(&[0]);
    assert!(matches!(
        arguments::decode(
            &[1, 0, 2, 0, b'A', b'B'],
            Signature {
                convention: Convention::Default,
                parameters: &params
            },
            Limits {
                max_string_bytes: 1,
                ..Limits::default()
            }
        ),
        Err(DecodeError::Limit { .. })
    ));
}
