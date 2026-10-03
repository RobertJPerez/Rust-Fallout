use super::*;

fn instruction(bytes: &mut Vec<u8>, opcode: u16, operands: &[u8]) {
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&(operands.len() as u16).to_le_bytes());
    bytes.extend_from_slice(operands);
}
fn condition(bytes: &mut Vec<u8>, opcode: u16, skip: u16) {
    let mut operands = skip.to_le_bytes().to_vec();
    operands.extend_from_slice(&1u16.to_le_bytes());
    operands.push(b'1');
    instruction(bytes, opcode, &operands);
}
fn diagnostic(bytes: &[u8]) -> Diagnostic {
    match decode(bytes, Limits::default()).unwrap_err() {
        Error::Structure(issue) => issue,
        error => panic!("expected structure failure, got {error}"),
    }
}

#[test]
fn nested_siblings_empty_arms_and_fragment_roots_have_exact_delimiters() {
    let mut bytes = Vec::new();
    condition(&mut bytes, 0x16, 3); // 0: nested group + native before else-if
    condition(&mut bytes, 0x16, 0); // 1: empty nested if
    instruction(&mut bytes, 0x19, &[]); // 2
    instruction(&mut bytes, 0x1000, &[0xff, 0]); // 3
    condition(&mut bytes, 0x18, 0); // 4: empty else-if
    instruction(&mut bytes, 0x17, &[0, 0]); // 5: empty else
    instruction(&mut bytes, 0x19, &[]); // 6
    let plan = decode(&bytes, Limits::default()).unwrap();
    assert_eq!(plan.bytes(), bytes);
    assert!(plan.events().is_empty());
    assert_eq!(plan.maximum_depth(), 2);
    assert_eq!(
        plan.arms(),
        &[
            Arm {
                instruction: 0,
                enclosing_if: 0,
                next_delimiter: 4,
                end_if: 6,
                depth: 1
            },
            Arm {
                instruction: 1,
                enclosing_if: 1,
                next_delimiter: 2,
                end_if: 2,
                depth: 2
            },
            Arm {
                instruction: 4,
                enclosing_if: 0,
                next_delimiter: 5,
                end_if: 6,
                depth: 1
            },
            Arm {
                instruction: 5,
                enclosing_if: 0,
                next_delimiter: 6,
                end_if: 6,
                depth: 1
            },
        ]
    );
    assert_eq!(plan.links()[0].raw_word, 3);
    assert_eq!(plan.links()[0].observed_distance, 3);
    assert_eq!(plan.links()[0].relation, Relation::InterveningInstructions);
}

#[test]
fn event_span_includes_end_header_and_preserves_event_arguments_and_callers() {
    let mut bytes = Vec::new();
    instruction(&mut bytes, 0x1d, &[]);
    // Native caller header (8) + payload (3) + return/end headers (4 each).
    instruction(&mut bytes, 0x10, &[3, 0, 19, 0, 0, 0, 0xff, 0]);
    bytes.extend_from_slice(&[0x1c, 0, 0, 0]);
    instruction(&mut bytes, 0x1000, &[9, 8, 7]);
    instruction(&mut bytes, 0x1e, &[]);
    instruction(&mut bytes, 0x11, &[]);
    instruction(&mut bytes, 0x10, &[6, 0, 4, 0, 0, 0]);
    instruction(&mut bytes, 0x11, &[]);
    let plan = decode(&bytes, Limits::default()).unwrap();
    assert_eq!(
        plan.events(),
        &[
            Event {
                begin_instruction: 1,
                end_instruction: 4,
                event_id: 3
            },
            Event {
                begin_instruction: 5,
                end_instruction: 6,
                event_id: 6
            },
        ]
    );
    assert_eq!(plan.instructions()[2].calling_reference, Some(0));
    assert_eq!(plan.instructions()[1].operands[6..], [0xff, 0]);
    assert_eq!(plan.links()[0].observed_distance, 19);
    assert_eq!(plan.links()[1].observed_distance, 4);
    assert_eq!(plan.links()[0].relation, Relation::EventByteSpan);
}

#[test]
fn structural_failures_do_not_produce_partial_plans() {
    for (bytes, expected) in [
        (vec![0x19, 0, 0, 0], "orphan_end_if"),
        (vec![0x11, 0, 0, 0], "orphan_end"),
        (vec![0x17, 0, 2, 0, 0, 0], "orphan_arm"),
        (vec![0x20, 0, 0, 0], "unsupported_opcode"),
        (vec![0x1e, 0, 1, 0, 0], "statement_operands"),
        (
            vec![0x16, 0, 4, 0, 0, 0, 1, 0],
            "conditional_expression_extent",
        ),
        (
            vec![0x1c, 0, 0, 0, 0x1e, 0, 0, 0],
            "statement_reference_prefix",
        ),
    ] {
        assert_eq!(diagnostic(&bytes).kind, expected);
    }
    let mut bytes = Vec::new();
    condition(&mut bytes, 0x16, 0);
    assert_eq!(diagnostic(&bytes).kind, "unclosed_conditional");
    instruction(&mut bytes, 0x17, &[0, 0]);
    condition(&mut bytes, 0x18, 0);
    assert_eq!(diagnostic(&bytes).kind, "arm_after_else");
    let mut bytes = Vec::new();
    instruction(&mut bytes, 0x10, &[0; 6]);
    assert_eq!(diagnostic(&bytes).kind, "unclosed_event");
    condition(&mut bytes, 0x16, 0);
    instruction(&mut bytes, 0x11, &[]);
    assert_eq!(diagnostic(&bytes).kind, "conditional_crosses_event");
    let mut bytes = Vec::new();
    condition(&mut bytes, 0x16, 0);
    instruction(&mut bytes, 0x10, &[0; 6]);
    assert_eq!(diagnostic(&bytes).kind, "nested_event");
}

#[test]
fn raw_words_are_checked_against_structure_without_defining_vm_successors() {
    let mut bytes = Vec::new();
    condition(&mut bytes, 0x16, 1);
    instruction(&mut bytes, 0x19, &[]);
    let issue = diagnostic(&bytes);
    assert_eq!(issue.kind, "raw_distance_mismatch");
    assert_eq!(issue.instruction_scda_offset, 0);
    assert_eq!(issue.raw_word, Some(1));
    assert_eq!(issue.observed_distance, Some(0));
    let mut bytes = Vec::new();
    instruction(&mut bytes, 0x10, &[0, 0, 0xff, 0xff, 0xff, 0xff]);
    instruction(&mut bytes, 0x11, &[]);
    assert_eq!(diagnostic(&bytes).raw_word, Some(u32::MAX));
}

#[test]
fn thirty_thousand_levels_are_iterative_and_limits_are_exact() {
    let depth = 30_000usize;
    let mut bytes = Vec::new();
    for level in 0..depth {
        condition(&mut bytes, 0x16, (2 * (depth - level - 1)) as u16);
    }
    for _ in 0..depth {
        instruction(&mut bytes, 0x19, &[]);
    }
    let limits = Limits {
        decode: super::super::Limits {
            max_bytes: bytes.len(),
            max_instructions: depth * 2,
        },
        maximum_depth: depth,
    };
    let plan = decode(&bytes, limits).unwrap();
    assert_eq!(plan.maximum_depth(), depth);
    assert_eq!(plan.arms()[0].end_if, depth * 2 - 1);
    assert_eq!(plan.arms()[depth - 1].end_if, depth);
    assert!(
        decode(
            &bytes,
            Limits {
                maximum_depth: depth - 1,
                ..limits
            }
        )
        .is_err()
    );
    assert!(
        decode(
            &bytes,
            Limits {
                decode: super::super::Limits {
                    max_bytes: bytes.len() - 1,
                    ..limits.decode
                },
                ..limits
            }
        )
        .is_err()
    );
    assert!(
        decode(
            &bytes,
            Limits {
                decode: super::super::Limits {
                    max_instructions: depth * 2 - 1,
                    ..limits.decode
                },
                ..limits
            }
        )
        .is_err()
    );
    assert!(
        decode(
            &[],
            Limits {
                maximum_depth: 0,
                ..limits
            }
        )
        .is_ok()
    );
}

#[test]
fn bounded_mutations_and_all_truncations_never_panic_or_escape_source() {
    let mut bytes = Vec::new();
    condition(&mut bytes, 0x16, 1);
    instruction(&mut bytes, 0x1000, &[1, 2]);
    instruction(&mut bytes, 0x19, &[]);
    for end in 0..=bytes.len() {
        let _ = decode(&bytes[..end], Limits::default());
    }
    for offset in 0..bytes.len() {
        for replacement in [0, 0x10, 0x16, 0x17, 0x19, 0x1c, 0xff] {
            let mut changed = bytes.clone();
            changed[offset] = replacement;
            if let Ok(plan) = decode(&changed, Limits::default()) {
                assert_eq!(plan.bytes(), changed);
                for link in plan.links() {
                    assert!(link.delimiter_instruction < plan.instructions().len());
                    assert!(link.source_instruction < link.delimiter_instruction);
                }
            }
        }
    }
}
