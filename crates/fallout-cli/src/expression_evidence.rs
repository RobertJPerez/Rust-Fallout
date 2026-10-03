//! Independent token/envelope comparison. Numerical evaluation, argument
//! extraction and native behavior are deliberately outside this checkpoint.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

fn add_counts(total: &mut Value, source: &Value) -> Result<()> {
    for (key, value) in source.as_object().ok_or("Missing expression counts")? {
        if let Some(value) = value.as_u64() {
            total[key] = (total[key].as_u64().unwrap_or(0) + value).into();
        } else {
            if total[key].is_null() {
                total[key] = json!({});
            }
            for (id, value) in value.as_object().ok_or("Invalid count map")? {
                total[key][id] = (total[key][id].as_u64().unwrap_or(0)
                    + value.as_u64().ok_or("Invalid count")?)
                .into();
            }
        }
    }
    Ok(())
}

fn fixture(opcode: u16, operands: &[u8]) -> Vec<u8> {
    [
        b"FROBS001".as_slice(),
        &((operands.len() + 4) as u32).to_le_bytes(),
        &opcode.to_le_bytes(),
        &(operands.len() as u16).to_le_bytes(),
        operands,
    ]
    .concat()
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    command_oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let catalogue = super::catalogue_evidence::run(root, run, cli, command_oracle, install)?;
    let rust_path = run.join("expressions-rust.json");
    let bundle_path = run.join("expressions.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("expressions")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    run_logged(command, &run.join("expressions-rust.log"))?;
    let rust = json_file(&rust_path)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(&bundle_path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("expressions-native.log"))?;
    let native_path = run.join("expressions-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if rust["issues"] != 0
        || rust["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
        || native["execution_ready"] != false
        || rust["comparison_bundle"]["sha256"] != digest(&bundle_path)?
        || native["bundle_sha256"] != digest(&bundle_path)?
        || rust["executable_source_sha256"] != native["executable_source_sha256"]
        || rust["operator_descriptors"] != native["operator_descriptors"]
        || !rust["unresolved_command_ids"]
            .as_array()
            .ok_or("Missing unresolved commands")?
            .is_empty()
    {
        return Err("Expression source, issues, operators or capability flags differ".into());
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let framing_path = root.join("reports/checkpoint-15-compiled-scripts.json");
    let framing = json_file(&framing_path)?;
    let bodies = native["bodies"].as_array().ok_or("Missing native bodies")?;
    let plugins = rust["plugins"]
        .as_array()
        .ok_or("Missing expression plugins")?;
    if plugins.len() != 10 {
        return Err("Unexpected official expression source count".into());
    }
    let mut cursor = 0;
    let mut totals = json!({});
    let mut summaries = Vec::new();
    let mut deferred = 0;
    for plugin in plugins {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing source name")?;
        let source = baseline["files"]
            .as_array()
            .ok_or("Missing baseline files")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Expression source absent from baseline")?;
        let prior = framing["plugins"]
            .as_array()
            .ok_or("Missing framing sources")?
            .iter()
            .find(|p| p["source_name"] == name)
            .ok_or("Missing prior framing source")?;
        if source["sha256"] != plugin["source_sha256"]
            || source["bytes"] != plugin["source_bytes"]
            || prior["counts"] != plugin["framing_counts"]
            || plugin["focused_scan"] != true
            || plugin["expression_issues"] != 0
            || plugin["framing_issues"] != 0
            || plugin["execution_ready"] != false
            || plugin["retail_parity_accepted"] != false
        {
            return Err("Expression source/scope differs from prior framing evidence".into());
        }
        let mut independent_counts = json!({});
        for body in plugin["bodies"]
            .as_array()
            .ok_or("Missing expression bodies")?
        {
            let reference = bodies
                .get(cursor)
                .ok_or("Missing independent expression body")?;
            for key in ["bytes", "sha256", "statements"] {
                if body[key] != reference[key] {
                    return Err(
                        format!("Expression comparison differs: {name}/{cursor}/{key}").into(),
                    );
                }
            }
            add_counts(&mut independent_counts, &reference["counts"])?;
            cursor += 1;
        }
        // Empty maps remain present in Rust even when no body contributes to one.
        for key in ["command_calls", "operator_codes", "token_kinds"] {
            if independent_counts[key].is_null() {
                independent_counts[key] = json!({});
            }
        }
        if independent_counts != plugin["counts"] {
            return Err("Independent aggregate expression counts differ".into());
        }
        add_counts(&mut totals, &plugin["counts"])?;
        deferred += plugin["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        summaries.push(json!({"source_name":name,"source_sha256":plugin["source_sha256"],"source_bytes":plugin["source_bytes"],"counts":plugin["counts"],"compiled_bodies":plugin["framing_counts"]["compiled_bodies"],"record_payloads_deferred":plugin["record_payloads_deferred"]}));
    }
    if cursor != bodies.len()
        || native["compiled_bodies"] != cursor
        || framing["compiled_bodies_compared"] != cursor
    {
        return Err("Independent expression body coverage differs".into());
    }
    let strict_path = run.join("strict-expressions.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("expressions")
        .arg("--install")
        .arg(install)
        .arg("--output")
        .arg(&strict_path);
    let output = run_logged_status(command, &run.join("strict-expressions.log"), 1)?;
    let error = String::from_utf8(output.stderr)?;
    if !error.contains("0xB0CFF04")
        || !error.contains("strict integrity check failed")
        || strict_path.exists()
    {
        return Err("Strict LAND integrity guard failed".into());
    }
    let cases = [
        ("short_magic", b"FROBS".to_vec()),
        ("wrong_magic", b"BROBS001".to_vec()),
        ("partial_length", b"FROBS001\x01".to_vec()),
        (
            "operand_extent",
            b"FROBS001\x04\0\0\0\x16\0\xff\xff".to_vec(),
        ),
        ("short_jump", fixture(0x16, &[1])),
        ("short_expression_length", fixture(0x16, &[1, 0, 2])),
        ("expression_extent", fixture(0x16, &[0, 0, 3, 0, b'1'])),
        ("short_local", fixture(0x16, &[0, 0, 1, 0, b'f'])),
        (
            "command_extent",
            fixture(0x16, &[0, 0, 5, 0, b'X', 1, 0x10, 2, 0]),
        ),
        ("unknown_n", fixture(0x16, &[0, 0, 1, 0, b'n'])),
        ("unknown_z", fixture(0x16, &[0, 0, 1, 0, b'z'])),
        ("bad_target", fixture(0x15, &[b'l', 1, 0, 0, 0])),
        (
            "unterminated_literal",
            fixture(0x16, &[0, 0, 4, 0, b'"', 2, 0, 1]),
        ),
        ("literal_budget", fixture(0x16, &[0, 0, 3, 0, b'"', 1, 2])),
        (
            "hex_numeric",
            fixture(0x16, &[0, 0, 3, 0, b'0', b'x', b'1']),
        ),
        (
            "incomplete_exponent",
            fixture(0x16, &[0, 0, 3, 0, b'1', b'e', b'+']),
        ),
    ];
    let mut negatives = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(&path)
            .arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid expression bundle produced a complete report".into());
        }
        negatives.push(name);
    }
    // Synthetic quoted bytes provide coverage absent from the official expression
    // corpus. A literal may contain NUL, an operator or non-ASCII bytes verbatim.
    let literal = fixture(0x16, &[0, 0, 7, 0, b'"', 4, 0, b'X', 0, 0xff, b'~']);
    let literal_path = run.join("quoted-fixture.bin");
    write_new(&literal_path, &literal)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(&literal_path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("quoted-fixture.log"))?;
    let literal_native: Value = serde_json::from_slice(&output.stdout)?;
    if literal_native["bodies"][0]["counts"]["tokens"] != 1
        || literal_native["bodies"][0]["counts"]["token_kinds"]["6"] != 1
    {
        return Err("Quoted-byte independent fixture failed".into());
    }
    use fallout_data::obscript::{self, expression, expression_census};
    let fixture_operators = expression::Operators::new(vec![expression::Operator {
        code: 11,
        precedence: 4,
        spelling: b"+".to_vec(),
    }])?;
    let fixture_program = obscript::decode(&literal[12..], obscript::Limits::default())?;
    let fixture_statement = expression::statement(
        &fixture_program.instructions[0],
        &fixture_operators,
        expression::Limits::default(),
    )?
    .ok_or("Missing fixture expression")?;
    if literal_native["bodies"][0]["statements"][0]["token_sha256"]
        != expression_census::token_digest(&fixture_statement.expression.tokens)
    {
        return Err("Quoted-byte token digest differs between independent readers".into());
    }
    let unique: BTreeMap<_, _> = rust["command_links"]
        .as_array()
        .ok_or("Missing expression commands")?
        .iter()
        .map(|row| {
            (
                row["command_id"].as_u64().unwrap_or(u64::MAX),
                row["occurrences"].clone(),
            )
        })
        .collect();
    if unique.len()
        != rust["command_links"]
            .as_array()
            .ok_or("Missing links")?
            .len()
    {
        return Err("Duplicate expression command metadata links".into());
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":summaries,"counts":totals,"compiled_bodies_compared":cursor,"fresh_command_catalogue":catalogue,
        "operator_descriptors":rust["operator_descriptors"],"executable_source_sha256":rust["executable_source_sha256"],
        "all_statement_envelopes_and_token_digests_equal":true,"expression_command_ids_bound":unique.len(),"command_links":rust["command_links"],
        "record_payloads_deferred":deferred,"strict_unrelated_integrity_failure_retained":true,"native_negative_cases_rejected":negatives,
        "quoted_fixture_scope":"original synthetic byte literal; absent from authored corpus; separately exercised in Rust tests",
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"comparison_bundle_sha256":digest(&bundle_path)?,
        "framing_receipt_sha256":digest(&framing_path)?,"comparison_scope":"independent envelopes, token boundaries/fields and opaque argument/string/number hashes from Rust-extracted SCDA; operator descriptors independently read from executable",
        "numeric_scope":"decimal token boundaries only; current CRT strtod boundary comparison, no value/rounding or original CRT/retail evaluation claim",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Expression operand/reference binding and postfix stack semantics","Native argument decoding and return types","Control-flow jump origin and targets","Numerical evaluation, scheduling and side effects","Extension expressions and other executable variants"]}),
    )
}
