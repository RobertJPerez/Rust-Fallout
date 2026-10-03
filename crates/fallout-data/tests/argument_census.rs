use fallout_data::obscript::{
    argument_census::{self, CommandSignature, Signatures},
    arguments::{Convention, Parameter},
    expression::{Operator, Operators},
};
use std::fs;

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &[0; 4],
        &1_u32.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Signatures) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Fixture.esm");
    let scda = [
        0x1c, 0, 7, 0, 1, 0x10, 7, 0, 1, 0, b'n', 3, 0, 0, 0, 0x16, 0, 17, 0, 0, 0, 13, 0, b'r', 9,
        0, b'X', 2, 0x10, 5, 0, 1, 0, b'f', 42, 0, 3, 0x10, 0, 0,
    ];
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&(scda.len() as u32).to_le_bytes());
    fs::write(
        &path,
        [
            record(b"TES4", &[]),
            record(
                b"SCPT",
                &[field(b"SCHR", &schr), field(b"SCDA", &scda)].concat(),
            ),
            record(b"LAND", &[0xff]),
        ]
        .concat(),
    )
    .unwrap();
    let signatures = [(0x1001, 1), (0x1002, 3)]
        .into_iter()
        .map(|(id, type_id)| {
            (
                id,
                CommandSignature {
                    convention: Convention::Default,
                    parameters: vec![Parameter {
                        type_id,
                        optional_word: 0,
                    }],
                },
            )
        })
        .collect();
    (directory, path, signatures)
}

#[test]
fn calls_keep_context_and_exact_scda_offsets_without_claiming_missing_signatures() {
    let (_directory, path, signatures) = fixture();
    let operators = Operators::new(vec![Operator {
        code: 11,
        precedence: 4,
        spelling: b"+".to_vec(),
    }])
    .unwrap();
    let report = argument_census::inspect(
        &path,
        true,
        &operators,
        &signatures,
        argument_census::Limits::default(),
        |_| Ok(()),
    )
    .unwrap();
    let calls = &report.bodies[0].calls;
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].calling_reference, Some(7));
    assert_eq!(calls[0].arguments_scda_offset, 8);
    assert_eq!(calls[1].calling_reference, Some(9));
    assert_eq!(calls[1].expression_token_offset, Some(3));
    assert_eq!(calls[1].arguments_scda_offset, 31);
    assert_eq!(calls[1].arguments, Some(1));
    assert!(calls[2].arguments.is_none());
    assert!(
        calls[2]
            .issue
            .as_ref()
            .unwrap()
            .contains("no verified signature")
    );
    assert_eq!(report.counts.top_level_calls, 2);
    assert_eq!(report.counts.expression_calls, 1);
    assert_eq!(report.counts.arguments, 2);
    assert_eq!(report.counts.value_kinds.get(&9), Some(&1));
    assert_eq!(report.argument_issues, 1);
    assert_eq!(report.expression_issues, 0);
    assert_eq!(report.record_payloads_deferred, 1);
    assert!(!report.execution_ready);
    assert!(!report.retail_parity_accepted);
    assert!(
        argument_census::inspect(
            &path,
            false,
            &operators,
            &signatures,
            argument_census::Limits::default(),
            |_| Ok(())
        )
        .is_err()
    );
}

#[test]
fn row_budgets_reject_instead_of_publishing_a_partial_census() {
    let (_directory, path, signatures) = fixture();
    let operators = Operators::new(vec![Operator {
        code: 11,
        precedence: 4,
        spelling: b"+".to_vec(),
    }])
    .unwrap();
    for limits in [
        argument_census::Limits {
            max_calls: 2,
            ..Default::default()
        },
        argument_census::Limits {
            max_bodies: 0,
            ..Default::default()
        },
    ] {
        let error =
            argument_census::inspect(&path, true, &operators, &signatures, limits, |_| Ok(()))
                .unwrap_err();
        assert!(error.to_string().contains("budget exceeded"));
    }
}
