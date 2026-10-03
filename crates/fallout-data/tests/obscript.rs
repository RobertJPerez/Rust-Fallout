//! Original byte streams exercise framing; retail execution is a separate gate.
use fallout_data::obscript::{self, DecodeError, Kind, Limits};

fn line(opcode: u16, operands: &[u8]) -> Vec<u8> {
    let mut bytes = opcode.to_le_bytes().to_vec();
    bytes.extend((operands.len() as u16).to_le_bytes());
    bytes.extend(operands);
    bytes
}

#[test]
fn script_framing_preserves_reference_prefixes_events_and_unknown_operands() {
    let mut bytes = line(0x1d, &[]);
    bytes.extend(line(0x10, &[3, 0, 27, 0, 0, 0, 1, 0, 0xff]));
    bytes.extend([0x1c, 0, 2, 0]);
    bytes.extend(line(0x1001, &[2, 0, 0x6e, 9, 0, 0, 0]));
    bytes.extend(line(0x42, &[0x1c, 0, 0xff, 0xff]));
    bytes.extend(line(0x11, &[]));
    let program = obscript::decode(&bytes, Limits::default()).unwrap();
    let instructions = &program.instructions;
    assert_eq!(instructions.len(), 5);
    assert_eq!(instructions[0].bytes, 0..4);
    assert_eq!(instructions[1].event.unwrap().id, 3);
    assert_eq!(instructions[1].event.unwrap().end_jump_bytes, 27);
    assert_eq!(instructions[2].bytes, 17..32);
    assert_eq!(instructions[2].operand_offset, 25);
    assert_eq!(instructions[2].opcode, 0x1001);
    assert_eq!(instructions[2].calling_reference, Some(2));
    assert_eq!(instructions[2].kind(), Kind::NativeCommand);
    assert_eq!(instructions[3].kind(), Kind::Unknown);
    assert_eq!(instructions[3].operands, [0x1c, 0, 0xff, 0xff]);
    assert_eq!(instructions[4].kind(), Kind::Statement("end"));
    let mut restored = Vec::<u8>::new();
    for instruction in instructions {
        restored.extend(&program.bytes[instruction.bytes.clone()]);
    }
    assert_eq!(restored, bytes);
}

#[test]
fn script_framing_rejects_every_partial_header_and_oversized_operand_extent() {
    for referenced in [false, true] {
        let mut bytes = Vec::new();
        if referenced {
            bytes.extend([0x1c, 0, 1, 0]);
        }
        bytes.extend(line(0x1001, &[1, 0, 0x72, 2, 0]));
        for cut in 1..bytes.len() {
            assert!(
                obscript::decode(&bytes[..cut], Limits::default()).is_err(),
                "accepted partial instruction at {cut}"
            );
        }
        let length_offset = if referenced { 6 } else { 2 };
        bytes[length_offset..length_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(matches!(
            obscript::decode(&bytes, Limits::default()),
            Err(DecodeError::Truncated { .. })
        ));
    }
    for bytes in 0..6 {
        assert!(matches!(
            obscript::decode(&line(0x10, &vec![0; bytes]), Limits::default()),
            Err(DecodeError::ShortEvent { .. })
        ));
    }
}

#[test]
fn script_framing_has_finite_budgets_even_for_zero_length_instructions() {
    let bytes = [line(0x1e, &[]), line(0x1e, &[])].concat();
    assert!(matches!(
        obscript::decode(
            &bytes,
            Limits {
                max_bytes: 7,
                max_instructions: 9
            }
        ),
        Err(DecodeError::ByteLimit { .. })
    ));
    assert_eq!(
        obscript::decode(
            &bytes,
            Limits {
                max_bytes: 8,
                max_instructions: 1
            }
        ),
        Err(DecodeError::InstructionLimit {
            offset: 4,
            maximum: 1
        })
    );
    assert!(
        obscript::decode(
            &[],
            Limits {
                max_bytes: 0,
                max_instructions: 0
            }
        )
        .is_ok()
    );
    assert!(
        obscript::decode(
            &line(0x1e, &[]),
            Limits {
                max_bytes: 4,
                max_instructions: 0
            }
        )
        .is_err()
    );
    let explicit_zero = [vec![0x1c, 0, 0, 0], line(0x1000, &[])].concat();
    assert_eq!(
        obscript::decode(&explicit_zero, Limits::default())
            .unwrap()
            .instructions[0]
            .calling_reference,
        Some(0)
    );
}

#[test]
fn adversarial_compiled_streams_never_panic_and_consume_exact_extents() {
    let seed = [
        line(0x1d, &[]),
        line(0x10, &[0; 6]),
        line(0x1000, &[1; 12]),
        line(0x11, &[]),
    ]
    .concat();
    for index in 0..seed.len() {
        for byte in [0, 0xff, 0x1c, 0x10, 0x80] {
            let mut bytes = seed.clone();
            bytes[index] = byte;
            if let Ok(program) = obscript::decode(&bytes, Limits::default()) {
                let mut next = 0;
                for instruction in program.instructions {
                    assert_eq!(instruction.bytes.start, next);
                    assert!(instruction.bytes.end > next);
                    assert!(instruction.operand_offset >= next + 4);
                    next = instruction.bytes.end;
                }
                assert_eq!(next, bytes.len());
            }
        }
    }
}
