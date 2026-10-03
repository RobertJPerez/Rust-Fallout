use super::*;
use crate::obscript::expression::Operator;

fn operators() -> Operators {
    Operators::new(
        SPELLINGS
            .iter()
            .enumerate()
            .map(|(code, spelling)| Operator {
                code: code as u32,
                precedence: code as u8,
                spelling: spelling.to_vec(),
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn ordered_children_and_contiguous_subtrees_preserve_source_order() {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let plan = decode(b"10 3 - 2 * ~", &model, Limits::default()).unwrap();
    assert_eq!(plan.root(), 5);
    assert_eq!(plan.maximum_stack(), 2);
    assert_eq!(plan.height(), 4);
    assert_eq!(plan.nodes()[2].inputs, Inputs::Binary { left: 0, right: 1 });
    assert_eq!(plan.nodes()[4].inputs, Inputs::Binary { left: 2, right: 3 });
    assert_eq!(plan.nodes()[5].inputs, Inputs::Unary { operand: 4 });
    assert_eq!(plan.subtree(2), Some(0..3));
    assert_eq!(plan.subtree(4), Some(0..5));
    assert_eq!(plan.subtree(99), None);
    assert_eq!(plan.tokens()[0].bytes, 0..2);
    assert_eq!(plan.source_bytes(), b"10 3 - 2 * ~");
    let reversed = decode(b"3 10 - 2 * ~", &model, Limits::default()).unwrap();
    assert_ne!(plan.shape_sha256(), reversed.shape_sha256());
}

#[test]
fn context_prefixes_are_metadata_and_quoted_operator_bytes_are_operands() {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let bytes = [b'r', 0, 0, b'G', 1, 0, b'f', 2, 0, b'+'];
    let plan = decode(&bytes, &model, Limits::default()).unwrap();
    assert_eq!(plan.nodes().len(), 3);
    assert_eq!(plan.nodes()[0].token_index, 1);
    assert_eq!(plan.nodes()[1].token_index, 2);
    assert!(matches!(
        plan.tokens()[2].kind,
        Kind::Local {
            context_reference: Some(0),
            ..
        }
    ));
    let string = [b'"', 4, 0, 0, 0xff, b'+', b'~'];
    let plan = decode(&string, &model, Limits::default()).unwrap();
    assert_eq!(plan.nodes()[0].inputs, Inputs::Operand);
    assert!(matches!(plan.tokens()[0].kind, Kind::String(value) if value == &string[3..]));
    let command = [b'r', 7, 0, b'X', 0x2f, 0x10, 2, 0, 0xff, 0];
    let plan = decode(&command, &model, Limits::default()).unwrap();
    assert_eq!(plan.nodes().len(), 1);
    assert!(
        matches!(plan.tokens()[1].kind, Kind::Command { context_reference: Some(7), arguments, .. } if arguments == [0xff, 0])
    );
}

#[test]
fn malformed_stack_shapes_and_unconsumed_contexts_are_precise_failures() {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    for bytes in [b"+".as_slice(), b"1 +", b"~"] {
        assert!(matches!(
            decode(bytes, &model, Limits::default()),
            Err(Error::Underflow { .. })
        ));
    }
    for (bytes, operands) in [(b"".as_slice(), 0), (b"1 2", 2)] {
        assert!(
            matches!(decode(bytes, &model, Limits::default()), Err(Error::Residual { operands: n }) if n == operands)
        );
    }
    for bytes in [b"1 (".as_slice(), b"1 )"] {
        assert!(matches!(
            decode(bytes, &model, Limits::default()),
            Err(Error::Operator { .. })
        ));
    }
    for bytes in [
        [b'r', 1, 0].as_slice(),
        &[b'r', 1, 0, b'r', 2, 0, b'f', 1, 0],
        &[b'r', 1, 0, b'G', 2, 0],
    ] {
        assert!(matches!(
            decode(bytes, &model, Limits::default()),
            Err(Error::Context { offset: 0 })
        ));
    }
}

#[test]
fn exact_structural_limits_and_decoder_limits_are_enforced() {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let exact = Limits {
        max_nodes: 3,
        max_stack: 2,
        max_height: 2,
        ..Limits::default()
    };
    assert!(decode(b"1 2 +", &model, exact).is_ok());
    for (limits, kind) in [
        (
            Limits {
                max_nodes: 2,
                ..exact
            },
            "node",
        ),
        (
            Limits {
                max_stack: 1,
                ..exact
            },
            "stack",
        ),
        (
            Limits {
                max_height: 1,
                ..exact
            },
            "height",
        ),
        (
            Limits {
                max_nodes: 0,
                ..exact
            },
            "node",
        ),
        (
            Limits {
                max_stack: 0,
                ..exact
            },
            "stack",
        ),
        (
            Limits {
                max_height: 0,
                ..exact
            },
            "height",
        ),
    ] {
        assert!(
            matches!(decode(b"1 2 +", &model, limits), Err(Error::Limit { kind: actual, .. }) if actual == kind)
        );
    }
    let limits = Limits {
        decoding: expression::Limits {
            max_bytes: 1,
            ..expression::Limits::default()
        },
        ..exact
    };
    assert!(matches!(
        decode(b"123", &model, limits),
        Err(Error::Decode(expression::DecodeError::Limit { .. }))
    ));
}

#[test]
fn deep_plans_use_an_arena_without_recursive_construction_or_drop() {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let mut bytes = b"1".to_vec();
    bytes.extend(std::iter::repeat_n(b'~', 50_000));
    let plan = decode(&bytes, &model, Limits::default()).unwrap();
    assert_eq!(plan.height(), 50_001);
    assert_eq!(plan.maximum_stack(), 1);
    assert_eq!(plan.subtree(plan.root()), Some(0..50_001));
    drop(plan);
}

#[test]
fn unknown_or_changed_models_and_mutations_cannot_panic() {
    let mut entries: Vec<_> = operators().entries().to_vec();
    entries[15].spelling = b"!".to_vec();
    assert!(matches!(
        Model::vanilla(&Operators::new(entries).unwrap()),
        Err(Error::OperatorModel)
    ));
    assert!(matches!(
        Model::vanilla(
            &Operators::new(vec![Operator {
                code: 11,
                precedence: 4,
                spelling: b"+".to_vec()
            }])
            .unwrap()
        ),
        Err(Error::OperatorModel)
    ));
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let seed = [b'r', 1, 0, b'f', 2, 0, b' ', b'1', b' ', b'+', b'~'];
    let check = |bytes: &[u8]| {
        let result = std::panic::catch_unwind(|| decode(bytes, &model, Limits::default()));
        if let Ok(plan) = result.expect("structural decoding must not unwind") {
            for (index, node) in plan.nodes().iter().enumerate() {
                assert!(node.subtree_start <= index);
                match node.inputs {
                    Inputs::Operand => {}
                    Inputs::Unary { operand } => assert!(operand < index),
                    Inputs::Binary { left, right } => assert!(left < right && right < index),
                }
            }
        }
    };
    for end in 0..=seed.len() {
        check(&seed[..end]);
    }
    for at in 0..seed.len() {
        for byte in [0, 0xff, b'~', b'+'] {
            let mut changed = seed;
            changed[at] = byte;
            check(&changed);
        }
    }
}
