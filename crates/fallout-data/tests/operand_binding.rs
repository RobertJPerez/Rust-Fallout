use fallout_data::{
    obscript::{
        self,
        argument_census::{CommandSignature, Signatures},
        arguments::{Convention, Parameter},
        expression::{Operator, Operators},
        operand_binding,
    },
    plugin, script_units,
};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn fixture(compiled: &[u8]) -> plugin::Record {
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&2_u32.to_le_bytes());
    schr[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    let mut local = [0; 24];
    local[..4].copy_from_slice(&42_u32.to_le_bytes());
    local[16] = 7;
    plugin::Record {
        header: plugin::RecordHeader {
            kind: *b"SCPT",
            stored_size: 0,
            flags: 0,
            form_id: 1,
            revision: [0; 4],
            version: 15,
            trailing_bytes: [0; 2],
            offset: 0,
        },
        payload: [
            field(b"SCHR", &schr),
            field(b"SCDA", compiled),
            field(b"SLSD", &local),
            field(b"SCVR", b"sparse\0"),
            field(b"SCRO", &0x123_u32.to_le_bytes()),
            field(b"SCRV", &42_u32.to_le_bytes()),
        ]
        .concat(),
        integrity_issue: None,
    }
}
fn operators() -> Operators {
    Operators::new(vec![Operator {
        code: 11,
        precedence: 4,
        spelling: b"+".to_vec(),
    }])
    .unwrap()
}

#[test]
fn sparse_locals_and_reference_variables_keep_source_fields_and_missing_indices() {
    let record = fixture(&[]);
    let units =
        script_units::decode(&record, "Fixture.esm", script_units::Limits::default()).unwrap();
    let local = operand_binding::local(&units[0], 7, 4, 42, None);
    assert_eq!(local.status, 1);
    assert_eq!(local.local_type_byte, Some(7));
    assert_eq!(local.local_declaration_decoded_offset, Some(32));
    let reference = operand_binding::reference(&units[0], 9, 6, 2);
    assert_eq!(reference.status, 3);
    assert_eq!(reference.target_value, Some(42));
    assert_eq!(
        reference.local_declaration_decoded_offset,
        local.local_declaration_decoded_offset
    );
    assert_eq!(operand_binding::reference(&units[0], 0, 1, 0).status, 6);
    assert_eq!(operand_binding::reference(&units[0], 0, 1, 3).status, 6);
    assert_eq!(operand_binding::local(&units[0], 0, 4, 1, None).status, 5);
    // This index also exists locally, but its declaration belongs to another
    // script. A successful context association is still a deferred foreign local.
    let foreign = operand_binding::local(&units[0], 11, 4, 42, Some(1));
    assert_eq!(foreign.status, 4);
    assert_eq!(foreign.context_target_kind, 1);
    assert_eq!(foreign.context_target_value, 0x123);
    assert!(foreign.local_declaration_decoded_offset.is_none());
    assert!(foreign.local_type_byte.is_none());
}

#[test]
fn binds_instruction_expression_and_argument_indices_in_source_order() {
    let compiled = [
        0x1c, 0, 2, 0, 1, 0x10, 5, 0, 1, 0, b'f', 42, 0, 0x16, 0, 15, 0, 0, 0, 11, 0, b'r', 1, 0,
        b'f', 42, 0, b'G', 2, 0, b'Z', 0,
    ];
    // A truncated reference token blocks this expression; it must not fabricate
    // bindings for the preceding otherwise plausible tokens.
    let record = fixture(&compiled);
    let units =
        script_units::decode(&record, "Fixture.esm", script_units::Limits::default()).unwrap();
    let program = obscript::decode(&compiled, obscript::Limits::default()).unwrap();
    let signatures: Signatures = [(
        0x1001,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![Parameter {
                type_id: 4,
                optional_word: 0,
            }],
        },
    )]
    .into_iter()
    .collect();
    let binding =
        operand_binding::bind(&units[0], &program, &operators(), &signatures, 32).unwrap();
    assert_eq!(binding.uses.len(), 2);
    assert_eq!(binding.uses[0].role, 1);
    assert_eq!(binding.uses[0].status, 3);
    assert_eq!(binding.uses[1].role, 11);
    assert_eq!(binding.uses[1].scda_offset, 11);
    assert_eq!(binding.uses[1].status, 1);
    assert_eq!(binding.decode_issues.len(), 1);
    assert!(operand_binding::bind(&units[0], &program, &operators(), &signatures, 1).is_err());
    let unrelated = obscript::decode(&[], obscript::Limits::default()).unwrap();
    assert!(operand_binding::bind(&units[0], &unrelated, &operators(), &signatures, 32).is_err());
}

#[test]
fn foreign_targets_and_numeric_arguments_do_not_bind_to_the_current_script() {
    let compiled = [
        0x15, 0, 11, 0, b'r', 1, 0, b'f', 42, 0, 3, 0, b'f', 42, 0, 6, 0x10, 8, 0, 1, 0, b'r', 2,
        0, b's', 99, 0,
    ];
    let record = fixture(&compiled);
    let units =
        script_units::decode(&record, "Fixture.esm", script_units::Limits::default()).unwrap();
    let program = obscript::decode(&compiled, obscript::Limits::default()).unwrap();
    let signatures: Signatures = [(
        0x1006,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![Parameter {
                type_id: 1,
                optional_word: 0,
            }],
        },
    )]
    .into_iter()
    .collect();
    let binding =
        operand_binding::bind(&units[0], &program, &operators(), &signatures, 32).unwrap();
    assert_eq!(
        binding
            .uses
            .iter()
            .map(|row| (row.scda_offset, row.role, row.status))
            .collect::<Vec<_>>(),
        [(5, 7, 2), (8, 2, 4), (13, 4, 1), (22, 12, 3), (25, 8, 4)]
    );
    assert_eq!(binding.counts.deferred_foreign_locals, 2);
    assert_eq!(binding.uses[4].context_target_kind, 2);
    assert_eq!(binding.uses[4].context_target_value, 42);
    assert!(binding.uses[4].local_declaration_decoded_offset.is_none());
    assert!(binding.decode_issues.is_empty());
}
