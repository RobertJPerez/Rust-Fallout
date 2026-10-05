use fallout_data::obscript::{self, control_flow_bundle};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path, process::Command};

fn instruction(bytes: &mut Vec<u8>, opcode: u16, operands: &[u8]) {
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&(operands.len() as u16).to_le_bytes());
    bytes.extend_from_slice(operands);
}

fn valid_if() -> Vec<u8> {
    let mut body = Vec::new();
    instruction(&mut body, 0x16, &[0, 0, 1, 0, b'1']);
    instruction(&mut body, 0x19, &[]);
    body
}

fn valid_event() -> Vec<u8> {
    let middle = [0x12, 0, 0, 0];
    let mut body = Vec::new();
    let mut operands = vec![0, 0];
    operands.extend_from_slice(&8_u32.to_le_bytes());
    instruction(&mut body, 0x10, &operands);
    body.extend_from_slice(&middle);
    instruction(&mut body, 0x11, &[]);
    body
}

fn bundle(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = b"FROBS001".to_vec();
    for body in bodies {
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(body);
    }
    bytes
}

fn invoke(oracle: &Path, bundle: &Path, diagnostic: bool) -> std::process::Output {
    let mut command = Command::new(oracle);
    command.arg(bundle);
    if diagnostic {
        command.arg("--diagnose-structure");
    }
    command.output().expect("run the independent C++ oracle")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
#[ignore = "requires the private control-flow-oracle executable"]
fn forward_rust_and_backward_cpp_planners_reject_the_same_mutated_sources() {
    let oracle = env::var_os("RF_CONTROL_FLOW_ORACLE")
        .map(std::path::PathBuf::from)
        .expect("RF_CONTROL_FLOW_ORACLE must name the private C++ oracle");
    let temporary = tempfile::tempdir().expect("private temporary evidence scope");
    let mut raw_if = valid_if();
    raw_if[4] = 1;
    let mut orphan_else_if = valid_if();
    orphan_else_if[0] = 0x18;
    let mut orphan_end = valid_if();
    orphan_end[9] = 0x11;
    let mut raw_event = valid_event();
    raw_event[6] = 9;

    let deep = {
        let mut bytes = Vec::new();
        for _ in 0..65_537 {
            instruction(&mut bytes, 0x16, &[0, 0, 1, 0, b'1']);
        }
        bytes
    };
    let bodies = vec![
        valid_if(),
        raw_if,
        orphan_else_if,
        orphan_end,
        valid_event(),
        raw_event,
        deep,
    ];
    let expected = [
        None,
        Some("raw_distance_mismatch"),
        Some("orphan_arm"),
        Some("orphan_end"),
        None,
        Some("raw_distance_mismatch"),
        Some("depth_budget"),
    ];
    let encoded = bundle(&bodies);
    let rust = control_flow_bundle::inspect_diagnostic(&encoded).unwrap();
    assert_eq!(rust.counts.complete_bodies, 2);
    assert_eq!(rust.counts.structural_issues, 5);
    for (index, (body, expected_kind)) in bodies.iter().zip(expected).enumerate() {
        let observed = rust.bodies[index].issue.as_ref().map(|issue| issue.kind);
        assert_eq!(observed, expected_kind, "case {index}");
        println!(
            "MUTATION_CASE={index} BODY_SHA256={} ISSUE={observed:?}",
            sha256(body)
        );
    }
    assert!(control_flow_bundle::inspect(&encoded).is_err());

    let structural_path = temporary.path().join("mutated-structure.frobs");
    fs::write(&structural_path, &encoded).unwrap();
    let diagnostic = invoke(&oracle, &structural_path, true);
    assert_eq!(diagnostic.status.code(), Some(1));
    let native: serde_json::Value = serde_json::from_slice(&diagnostic.stdout).unwrap();
    assert_eq!(native["structure"], serde_json::to_value(&rust).unwrap());
    assert_eq!(native["execution_ready"], false);
    let strict = invoke(&oracle, &structural_path, false);
    assert_eq!(strict.status.code(), Some(1));
    assert!(strict.stdout.is_empty());

    let mut bad_length = valid_if();
    bad_length[2..4].copy_from_slice(&u16::MAX.to_le_bytes());
    let mut truncated = valid_if();
    truncated.pop();
    let body_limit = vec![0; obscript::Limits::default().max_bytes + 1];
    let mut instruction_limit = Vec::new();
    for _ in 0..=obscript::Limits::default().max_instructions {
        instruction(&mut instruction_limit, 0x1000, &[]);
    }
    for (name, body) in [
        ("mutated-operand-length", bad_length),
        ("mutated-truncated-body", truncated),
        ("body-byte-limit-plus-one", body_limit),
        ("instruction-limit-plus-one", instruction_limit),
    ] {
        let encoded = bundle(&[body.clone()]);
        assert!(control_flow_bundle::inspect(&encoded).is_err(), "{name}");
        assert!(
            control_flow_bundle::inspect_diagnostic(&encoded).is_err(),
            "{name}"
        );
        let path = temporary.path().join(format!("{name}.frobs"));
        fs::write(&path, encoded).unwrap();
        for diagnostic in [false, true] {
            let output = invoke(&oracle, &path, diagnostic);
            assert_eq!(
                output.status.code(),
                Some(1),
                "{name}, diagnostic={diagnostic}"
            );
            assert!(output.stdout.is_empty(), "{name}, diagnostic={diagnostic}");
        }
        println!("MALFORMED_CASE={name} BODY_SHA256={}", sha256(&body));
    }
}
