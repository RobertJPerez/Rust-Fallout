use fallout_data::obscript::{
    self,
    expression::{self, DecodeError, Kind, Limits, Operator, Operators, StatementKind, Target},
};

fn operators() -> Operators {
    Operators::new(vec![
        Operator {
            code: 5,
            precedence: 3,
            spelling: b"<".to_vec(),
        },
        Operator {
            code: 4,
            precedence: 3,
            spelling: b"<=".to_vec(),
        },
        Operator {
            code: 8,
            precedence: 3,
            spelling: b"==".to_vec(),
        },
        Operator {
            code: 11,
            precedence: 4,
            spelling: b"+".to_vec(),
        },
        Operator {
            code: 15,
            precedence: 6,
            spelling: b"~".to_vec(),
        },
    ])
    .unwrap()
}

#[test]
fn preserves_numeric_lexemes_strings_and_opaque_command_bytes() {
    let mut bytes = b" 1.2500e-04 .5 + ".to_vec();
    bytes.extend_from_slice(&[b'"', 4, 0, b'X', 0, 0xff, b'~']);
    bytes.extend_from_slice(&[b'r', 7, 0, b'X', 0x34, 0x12, 4, 0, b'n', 1, 0, 0xff]);
    let expression = expression::decode(&bytes, &operators(), Limits::default()).unwrap();
    assert_eq!(expression.tokens.len(), 6);
    assert_eq!(expression.tokens[0].kind, Kind::Number(b"1.2500e-04"));
    assert_eq!(expression.tokens[1].kind, Kind::Number(b".5"));
    assert_eq!(
        expression.tokens[2].kind,
        Kind::Operator {
            code: 11,
            precedence: 4
        }
    );
    assert_eq!(
        expression.tokens[3].kind,
        Kind::String(&[b'X', 0, 0xff, b'~'])
    );
    assert_eq!(
        expression.tokens[4].kind,
        Kind::ReferencePrefix { reference_index: 7 }
    );
    assert_eq!(
        expression.tokens[5].kind,
        Kind::Command {
            opcode: 0x1234,
            context_reference: Some(7),
            arguments: &[b'n', 1, 0, 0xff]
        }
    );
    if let Kind::Number(number) = expression.tokens[0].kind {
        assert!(std::ptr::eq(
            number.as_ptr(),
            bytes[expression.tokens[0].bytes.start..].as_ptr()
        ));
    }
    assert_eq!(expression.pending_context_reference, None);
}

#[test]
fn longest_operator_and_context_consumption_match_inspected_source_structure() {
    let bytes = [
        b'r', 0, 0, b'G', 1, 0, b'Z', 2, 0, b'f', 42, 0, b'l', 43, 0, b'<', b'=', b'~', b'r', 9, 0,
    ];
    let expression = expression::decode(&bytes, &operators(), Limits::default()).unwrap();
    assert_eq!(
        expression.tokens[3].kind,
        Kind::Local {
            type_byte: b'f',
            index: 42,
            context_reference: Some(0)
        }
    );
    assert_eq!(
        expression.tokens[4].kind,
        Kind::Local {
            type_byte: b'l',
            index: 43,
            context_reference: None
        }
    );
    assert_eq!(
        expression.tokens[5].kind,
        Kind::Operator {
            code: 4,
            precedence: 3
        }
    );
    assert_eq!(expression.tokens[5].bytes, 15..17);
    assert_eq!(expression.pending_context_reference, Some(9));
    // An incomplete exponent must not absorb bytes which do not form a number.
    assert!(matches!(
        expression::decode(b"1e+", &operators(), Limits::default()),
        Err(DecodeError::Unsupported {
            offset: 1,
            byte: b'e'
        })
    ));
    for bytes in [b"n".as_slice(), b"z", b"0x1", b"nan", b"infinity"] {
        assert!(expression::decode(bytes, &operators(), Limits::default()).is_err());
    }
}

#[test]
fn statement_envelopes_keep_assignment_targets_jumps_and_uninterpreted_tail() {
    let mut bytes = vec![
        0x15, 0, 12, 0, b'r', 3, 0, b's', 42, 0, 2, 0, b'1', b' ', 0xde, 0xad,
    ];
    bytes.extend_from_slice(&[0x16, 0, 6, 0, 0xff, 0x7f, 2, 0, b'1', b' ']);
    bytes.extend_from_slice(&[0x15, 0, 6, 0, b'G', 7, 0, 1, 0, b'0']);
    let program = obscript::decode(&bytes, obscript::Limits::default()).unwrap();
    let first = expression::statement(&program.instructions[0], &operators(), Limits::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        first.kind,
        StatementKind::Assignment(Target::Local {
            type_byte: b's',
            index: 42,
            context_reference: Some(3)
        })
    );
    assert_eq!(first.expression_operand_offset, 8);
    assert_eq!(first.trailing, &[0xde, 0xad]);
    let second = expression::statement(&program.instructions[1], &operators(), Limits::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        second.kind,
        StatementKind::Conditional {
            false_jump_bytes: 0x7fff
        }
    );
    assert_eq!(second.expression_operand_offset, 4);
    let third = expression::statement(&program.instructions[2], &operators(), Limits::default())
        .unwrap()
        .unwrap();
    assert_eq!(
        third.kind,
        StatementKind::Assignment(Target::Global { reference_index: 7 })
    );
    assert_eq!(third.expression.bytes, b"0");
}

#[test]
fn malformed_tokens_and_budget_exhaustion_never_escape_the_input() {
    let operators = operators();
    for bytes in [
        b"f".as_slice(),
        &[b'f', 1],
        &[b'X', 1, 0, 3, 0, 1],
        &[b'"', 3, 0, 1],
        &[b'r', 1],
    ] {
        assert!(matches!(
            expression::decode(bytes, &operators, Limits::default()),
            Err(DecodeError::Truncated { .. })
        ));
    }
    for (bytes, limits) in [
        (
            b"1234".as_slice(),
            Limits {
                max_bytes: 3,
                ..Limits::default()
            },
        ),
        (
            b"1 2",
            Limits {
                max_tokens: 1,
                ..Limits::default()
            },
        ),
        (
            &[b'"', 2, 0, 1, 2],
            Limits {
                max_literal_bytes: 1,
                ..Limits::default()
            },
        ),
        (
            b"1234",
            Limits {
                max_numeric_bytes: 3,
                ..Limits::default()
            },
        ),
    ] {
        assert!(matches!(
            expression::decode(bytes, &operators, limits),
            Err(DecodeError::Limit { .. })
        ));
    }
    let seed = [
        b'r', 1, 0, b'f', 2, 0, b' ', b'1', b'.', b'2', b' ', b'<', b'=', b'X', 1, 0x10, 2, 0, 0, 0,
    ];
    let check = |bytes: &[u8]| {
        let result =
            std::panic::catch_unwind(|| expression::decode(bytes, &operators, Limits::default()));
        if let Ok(expression) = result.expect("decoder must not unwind") {
            let mut end = 0;
            for token in expression.tokens {
                assert!(
                    token.bytes.start >= end
                        && token.bytes.start < token.bytes.end
                        && token.bytes.end <= bytes.len()
                );
                end = token.bytes.end;
            }
        }
    };
    for end in 0..=seed.len() {
        check(&seed[..end]);
    }
    for at in 0..seed.len() {
        for byte in [0, 0xff, b'n', b'z'] {
            let mut changed = seed;
            changed[at] = byte;
            check(&changed);
        }
    }
    let duplicate = vec![
        Operator {
            code: 1,
            precedence: 1,
            spelling: b"+".to_vec()
        };
        2
    ];
    assert_eq!(
        Operators::new(duplicate).unwrap_err(),
        DecodeError::OperatorTable
    );
}
