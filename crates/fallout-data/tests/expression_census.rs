use fallout_data::obscript::{
    expression::{Operator, Operators},
    expression_census,
};
use std::fs;

fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &[0; 4],
        &1_u32.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}

#[test]
fn unsupported_expression_does_not_count_later_bytes_as_a_successful_call() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Fixture.esm");
    // The first expression contains an unknown n token followed by a plausible
    // command. Skipping n and claiming that call would falsify coverage.
    let mut scda = vec![0x16, 0, 10, 0, 0, 0, 6, 0, b'n', b'X', 1, 0x10, 0, 0];
    scda.extend_from_slice(&[
        0x18, 0, 10, 0, 4, 0, 6, 0, b'X', 2, 0x10, 0, 0, b' ', 0x15, 0, 6, 0, b'G', 1, 0, 1, 0,
        b'0',
    ]);
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&(scda.len() as u32).to_le_bytes());
    let bytes = [
        record(b"TES4", &[]),
        record(
            b"SCPT",
            &[field(b"SCHR", &schr), field(b"SCDA", &scda)].concat(),
        ),
        record(b"LAND", &[0xff]),
    ]
    .concat();
    fs::write(&path, bytes).unwrap();
    let operators = Operators::new(vec![Operator {
        code: 11,
        precedence: 4,
        spelling: b"+".to_vec(),
    }])
    .unwrap();
    let report = expression_census::inspect(&path, true, &operators, |_| Ok(())).unwrap();
    assert_eq!(report.expression_issues, 1);
    assert_eq!(report.framing_issues, 0);
    assert_eq!(report.counts.statements, 3);
    assert_eq!(report.counts.command_calls.len(), 1);
    assert_eq!(report.counts.command_calls.get(&0x1002), Some(&1));
    assert!(report.bodies[0].statements[0].tokens.is_none());
    assert!(
        report.bodies[0].statements[0]
            .issue
            .as_ref()
            .unwrap()
            .contains("unsupported token 0x6E")
    );
    assert_eq!(report.bodies[0].statements[1].tokens, Some(1));
    assert_eq!(report.record_payloads_deferred, 1);
    assert!(expression_census::inspect(&path, false, &operators, |_| Ok(())).is_err());
    assert!(!report.execution_ready);
    assert!(!report.retail_parity_accepted);
}
