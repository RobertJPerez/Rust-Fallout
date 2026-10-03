//! Exact native-operand comparison; unresolved tails remain reported metadata.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

fn add_counts(total: &mut Value, source: &Value) -> Result<()> {
    for (key, value) in source.as_object().ok_or("Missing argument counts")? {
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

pub(super) struct Oracles<'a> {
    pub arguments: &'a Path,
    pub expressions: &'a Path,
    pub catalogue: &'a Path,
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    // The shared C++ expression reader changed its storage to expose call tokens.
    // Recheck its complete prior scope, including the changed descriptor field.
    let regression_directory = run.join("expression-regression");
    std::fs::create_dir(&regression_directory)?;
    let regression = super::expression_evidence::run(
        root,
        &regression_directory,
        cli,
        oracles.expressions,
        oracles.catalogue,
        install,
    )?;
    let rust_path = run.join("arguments-rust.json");
    let bundle_path = run.join("arguments.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("native-arguments")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    run_logged(command, &run.join("arguments-rust.log"))?;
    let rust = json_file(&rust_path)?;
    let mut command = Command::new(oracles.arguments);
    command
        .current_dir(root)
        .arg(&bundle_path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("arguments-native.log"))?;
    let native_path = run.join("arguments-native.json");
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
        return Err("Argument source, issues, operators or capability flags differ".into());
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let framing_path = root.join("reports/checkpoint-15-compiled-scripts.json");
    let framing = json_file(&framing_path)?;
    let expressions_path = root.join("reports/checkpoint-18-script-expressions.json");
    let expressions = json_file(&expressions_path)?;
    let bodies = native["bodies"].as_array().ok_or("Missing native bodies")?;
    let plugins = rust["plugins"]
        .as_array()
        .ok_or("Missing argument plugins")?;
    if plugins.len() != 10 {
        return Err("Unexpected official argument source count".into());
    }
    let mut cursor = 0;
    let mut totals = json!({});
    let mut summaries = Vec::new();
    let mut deferred = 0;
    let mut tails = BTreeMap::<u16, (u64, u64)>::new();
    for plugin in plugins {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing source name")?;
        let source = baseline["files"]
            .as_array()
            .ok_or("Missing baseline")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Source absent from baseline")?;
        let prior = framing["plugins"]
            .as_array()
            .ok_or("Missing framing sources")?
            .iter()
            .find(|p| p["source_name"] == name)
            .ok_or("Missing prior framing source")?;
        let prior_expression = expressions["plugins"]
            .as_array()
            .ok_or("Missing expression sources")?
            .iter()
            .find(|p| p["source_name"] == name)
            .ok_or("Missing prior expression source")?;
        if source["sha256"] != plugin["source_sha256"]
            || source["bytes"] != plugin["source_bytes"]
            || prior["counts"] != plugin["framing_counts"]
            || plugin["focused_scan"] != true
            || plugin["argument_issues"] != 0
            || plugin["expression_issues"] != 0
            || plugin["framing_issues"] != 0
            || plugin["execution_ready"] != false
            || plugin["retail_parity_accepted"] != false
        {
            return Err("Argument source/scope differs from prior framing evidence".into());
        }
        let mut independent = json!({});
        for body in plugin["bodies"]
            .as_array()
            .ok_or("Missing argument bodies")?
        {
            let reference = bodies
                .get(cursor)
                .ok_or("Missing independent argument body")?;
            for key in ["bytes", "sha256", "calls", "expression_issues"] {
                if body[key] != reference[key] {
                    return Err(
                        format!("Argument comparison differs: {name}/{cursor}/{key}").into(),
                    );
                }
            }
            add_counts(&mut independent, &reference["counts"])?;
            for call in body["calls"].as_array().ok_or("Missing calls")? {
                let bytes = call["trailing_operand_bytes"]
                    .as_u64()
                    .ok_or("Missing tail size")?;
                if bytes > 0 {
                    let id =
                        u16::try_from(call["command_id"].as_u64().ok_or("Missing command ID")?)?;
                    let total = tails.entry(id).or_default();
                    total.0 += 1;
                    total.1 += bytes;
                }
            }
            cursor += 1;
        }
        for key in ["command_calls", "value_kinds"] {
            if independent[key].is_null() {
                independent[key] = json!({});
            }
        }
        if independent != plugin["counts"] {
            return Err("Independent aggregate argument counts differ".into());
        }
        let mut expected_calls =
            json!({"command_calls":prior["counts"]["top_level_native_commands"]});
        add_counts(
            &mut expected_calls,
            &json!({"command_calls":prior_expression["counts"]["command_calls"]}),
        )?;
        if expected_calls["command_calls"] != plugin["counts"]["command_calls"] {
            return Err(
                "Native argument call coverage differs from prior instruction/expression census"
                    .into(),
            );
        }
        add_counts(&mut totals, &plugin["counts"])?;
        deferred += plugin["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        summaries.push(json!({"source_name":name,"source_sha256":plugin["source_sha256"],"source_bytes":plugin["source_bytes"],
            "counts":plugin["counts"],"compiled_bodies":plugin["framing_counts"]["compiled_bodies"],"record_payloads_deferred":plugin["record_payloads_deferred"]}));
    }
    if cursor != bodies.len()
        || native["compiled_bodies"] != cursor
        || framing["compiled_bodies_compared"] != cursor
    {
        return Err("Independent argument body coverage differs".into());
    }
    let cases = [
        ("short_magic", b"FROBS".to_vec()),
        ("wrong_magic", b"BROBS001".to_vec()),
        ("partial_length", b"FROBS001\x01".to_vec()),
        ("missing_required_form", fixture(0x1001, &[])),
        ("short_count", fixture(0x1001, &[1])),
        ("count_over_signature", fixture(0x1001, &[2, 0])),
        ("compiler_override", fixture(0x1001, &[0xff, 0xff])),
        ("inline_extension", fixture(0x1001, &[1, 0, 0xff, 0xff])),
        ("short_form", fixture(0x1001, &[1, 0, b'r', 1])),
        ("bad_form_prefix", fixture(0x1001, &[1, 0, b's', 1, 0])),
        ("unknown_command", fixture(0xffff, &[0, 0])),
    ];
    let mut negatives = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracles.arguments);
        command
            .current_dir(root)
            .arg(&path)
            .arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid argument bundle produced a complete report".into());
        }
        negatives.push(name);
    }
    // Form-variable operands do not occur in the supplied vanilla corpus. Exercise
    // the three-byte encoding against both readers using an original fixture.
    let form_variable = fixture(0x1001, &[1, 0, b'f', 42, 0]);
    let path = run.join("form-variable-fixture.bin");
    write_new(&path, &form_variable)?;
    let mut command = Command::new(oracles.arguments);
    command
        .current_dir(root)
        .arg(&path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("form-variable-fixture.log"))?;
    let fixture_native: Value = serde_json::from_slice(&output.stdout)?;
    use fallout_data::obscript::{argument_census, arguments};
    let fixture_rust = arguments::decode(
        &form_variable[16..],
        arguments::Signature {
            convention: arguments::Convention::Default,
            parameters: &[arguments::Parameter {
                type_id: 4,
                optional_word: 0,
            }],
        },
        arguments::Limits::default(),
    )?;
    let fixture_digest =
        argument_census::argument_digest(fixture_rust.arguments.iter().map(|arg| {
            (
                arg.bytes.start,
                arg.bytes.end,
                arg.parameter_type_id,
                &arg.value,
            )
        }));
    if fixture_native["bodies"][0]["calls"][0]["argument_sha256"] != fixture_digest
        || fixture_native["bodies"][0]["counts"]["value_kinds"]["9"] != 1
    {
        return Err("Independent form-variable fixture differs".into());
    }
    let links = rust["command_links"]
        .as_array()
        .ok_or("Missing command links")?;
    let command_names: BTreeMap<_, _> = links
        .iter()
        .map(|row| {
            (
                row["command_id"].as_u64().unwrap_or(u64::MAX),
                row["descriptor"]["name"].clone(),
            )
        })
        .collect();
    if command_names.len() != links.len() {
        return Err("Duplicate argument command links".into());
    }
    let trailing: Vec<_> = tails.into_iter().map(|(id, (calls, bytes))| json!({"command_id":id,
        "name":command_names[&u64::from(id)],"calls":calls,"bytes":bytes,"semantics":"unverified; preserved and hashed"})).collect();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":summaries,"counts":totals,
        "compiled_bodies_compared":cursor,"native_command_ids_bound":command_names.len(),
        "executable_source_sha256":rust["executable_source_sha256"],"all_call_extents_and_typed_argument_digests_equal":true,
        "prior_call_coverage_equal":true,"uninterpreted_trailing_operands":trailing,"record_payloads_deferred":deferred,
        "native_negative_cases_rejected":negatives,"form_variable_fixture_equal":true,
        "fresh_expression_regression":{"compiled_bodies":regression["compiled_bodies_compared"],"counts":regression["counts"],
            "all_statement_envelopes_and_token_digests_equal":regression["all_statement_envelopes_and_token_digests_equal"],
            "native_report_sha256":regression["native_report_sha256"],"rust_report_sha256":regression["rust_report_sha256"],
            "descriptor_fields_equal":regression["fresh_command_catalogue"]["all_descriptor_fields_equal"],
            "changed_executable_rejected":regression["fresh_command_catalogue"]["changed_executable_rejected_by_both_readers"],
            "strict_unrelated_integrity_failure_retained":regression["strict_unrelated_integrity_failure_retained"]},
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"comparison_bundle_sha256":digest(&bundle_path)?,
        "framing_receipt_sha256":digest(&framing_path)?,"expression_receipt_sha256":digest(&expressions_path)?,
        "comparison_scope":"independent argument extents, signature classifications and typed tuple hashes from Rust-extracted SCDA; executable metadata read independently",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["ShowMessage trailing words retain unverified semantics","Reference values and actual native parameter type checks",
            "Numeric conversions, return types and native side effects","Event operands, expression stack and control flow",
            "Extension encodings and other executable variants"]}),
    )
}
