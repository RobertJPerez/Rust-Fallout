//! Bind source/table associations to independent reads, retaining every known
//! metadata exception and every foreign declaration still needing runtime state.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn add_counts(total: &mut Value, source: &Value) -> Result<()> {
    for (key, value) in source.as_object().ok_or("Missing operand counts")? {
        if let Some(value) = value.as_u64() {
            total[key] = (total[key].as_u64().unwrap_or(0) + value).into();
        } else {
            if total[key].is_null() {
                total[key] = json!({});
            }
            for (id, value) in value.as_object().ok_or("Invalid operand count map")? {
                total[key][id] = (total[key][id].as_u64().unwrap_or(0)
                    + value.as_u64().ok_or("Invalid count")?)
                .into();
            }
        }
    }
    Ok(())
}

pub(super) struct Oracles<'a> {
    pub operands: &'a Path,
    pub tables: &'a Path,
    pub arguments: &'a Path,
    pub expressions: &'a Path,
    pub catalogue: &'a Path,
}

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn fixture(compiled: &[u8], reference: Option<u32>, variable: Option<u32>) -> Vec<u8> {
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&u32::from(reference.is_some()).to_le_bytes());
    schr[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    let mut payload = [field(b"SCHR", &schr), field(b"SCDA", compiled)].concat();
    if let Some(index) = variable {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        payload.extend(field(b"SLSD", &declaration));
        payload.extend(field(b"SCVR", b"fixture\0"));
    }
    if let Some(form) = reference {
        payload.extend(field(b"SCRO", &form.to_le_bytes()));
    }
    [
        b"FRUNIT01SCPT".as_slice(),
        &1_u32.to_le_bytes(),
        &0_u64.to_le_bytes(),
        &(payload.len() as u32).to_le_bytes(),
        &payload,
    ]
    .concat()
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let arguments_directory = run.join("argument-regression");
    std::fs::create_dir(&arguments_directory)?;
    let arguments = super::argument_evidence::run(
        root,
        &arguments_directory,
        cli,
        super::argument_evidence::Oracles {
            arguments: oracles.arguments,
            expressions: oracles.expressions,
            catalogue: oracles.catalogue,
        },
        install,
    )?;
    let tables_directory = run.join("table-regression");
    std::fs::create_dir(&tables_directory)?;
    let tables =
        super::binding_evidence::run(root, &tables_directory, cli, oracles.tables, install)?;
    let rust_path = run.join("operand-bindings-rust.json");
    let bundle_path = run.join("operand-bindings.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("operand-bindings")
        .arg("--install")
        .arg(install)
        .arg("--defer-unrelated-payloads")
        .arg("--comparison-bundle")
        .arg(&bundle_path)
        .arg("--output")
        .arg(&rust_path);
    let output = run_logged_status(command, &run.join("operand-bindings-rust.log"), 1)?;
    if !String::from_utf8(output.stderr)?.contains("operand table associations have issues") {
        return Err("Operand inspection failed without its expected metadata report".into());
    }
    let rust = json_file(&rust_path)?;
    let mut command = Command::new(oracles.operands);
    command
        .current_dir(root)
        .arg(&bundle_path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("operand-bindings-native.log"))?;
    let native_path = run.join("operand-bindings-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if rust["comparison_bundle"]["sha256"] != digest(&bundle_path)?
        || native["bundle_sha256"] != digest(&bundle_path)?
        || rust["executable_source_sha256"] != native["executable_source_sha256"]
        || rust["decode_issues"] != 0
        || rust["missing_bindings"] != 0
        || rust["table_units_with_issues"] != 3
        || rust["execution_ready"] != false
        || native["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Operand association identity, issues or capability scope differs".into());
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    let units = native["compiled_units"]
        .as_array()
        .ok_or("Missing independent operand units")?;
    let plugins = rust["plugins"]
        .as_array()
        .ok_or("Missing operand sources")?;
    if plugins.len() != 10 {
        return Err("Unexpected official source count".into());
    }
    let mut cursor = 0;
    let mut totals = json!({});
    let mut summaries = Vec::new();
    let mut deferred = 0;
    for plugin in plugins {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing operand source name")?;
        let source = baseline["files"]
            .as_array()
            .ok_or("Missing baseline")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Operand source absent from baseline")?;
        let table = tables["plugins"]
            .as_array()
            .ok_or("Missing table regression sources")?
            .iter()
            .find(|p| p["source_name"] == name)
            .ok_or("Missing table regression source")?;
        let argument = arguments["plugins"]
            .as_array()
            .ok_or("Missing argument regression sources")?
            .iter()
            .find(|p| p["source_name"] == name)
            .ok_or("Missing argument regression source")?;
        if plugin["source_sha256"] != source["sha256"]
            || plugin["source_bytes"] != source["bytes"]
            || plugin["table_counts"] != table["counts"]
            || plugin["focused_scan"] != true
            || plugin["table_units_with_issues_and_compiled_bytes"] != 0
            || plugin["decode_issues"] != 0
            || plugin["missing_bindings"] != 0
            || plugin["execution_ready"] != false
            || plugin["retail_parity_accepted"] != false
        {
            return Err("Operand source/table coverage differs from independent regression".into());
        }
        let mut counts = json!({});
        for unit in plugin["compiled_units"]
            .as_array()
            .ok_or("Missing operand unit rows")?
        {
            if Some(unit) != units.get(cursor) {
                return Err(format!("Operand association differs: {name}/{cursor}").into());
            }
            add_counts(&mut counts, &unit["counts"])?;
            cursor += 1;
        }
        for key in ["roles", "statuses"] {
            if counts[key].is_null() {
                counts[key] = json!({});
            }
        }
        for (key, prior) in [
            ("instruction_calls", "top_level_calls"),
            ("expression_calls", "expression_calls"),
            ("regular_arguments", "arguments"),
            ("message_arguments", "message_arguments"),
        ] {
            if counts[key] != argument["counts"][prior] {
                return Err("Operand/native argument call coverage differs".into());
            }
        }
        add_counts(&mut totals, &counts)?;
        deferred += plugin["record_payloads_deferred"]
            .as_u64()
            .ok_or("Missing deferred count")?;
        summaries.push(json!({"source_name":name,"source_sha256":source["sha256"],"source_bytes":source["bytes"],
            "compiled_units":plugin["table_counts"]["compiled_bodies"],"counts":counts,"table_units_with_issues":plugin["table_units_with_issues"],
            "record_payloads_deferred":plugin["record_payloads_deferred"]}));
    }
    if cursor != units.len()
        || native["compiled_unit_count"] != cursor
        || arguments["compiled_bodies_compared"] != cursor
    {
        return Err("Operand compiled-unit coverage differs".into());
    }
    let cases = [
        ("short_magic", b"FRUNIT".to_vec()),
        (
            "zero_reference",
            fixture(&[1, 0x10, 5, 0, 1, 0, b'r', 0, 0], Some(123), None),
        ),
        (
            "reference_outside_table",
            fixture(&[1, 0x10, 5, 0, 1, 0, b'r', 2, 0], Some(123), None),
        ),
        (
            "missing_form_variable",
            fixture(&[1, 0x10, 5, 0, 1, 0, b'f', 42, 0], None, None),
        ),
        (
            "sparse_local_not_ordinal",
            fixture(&[1, 0x10, 5, 0, 1, 0, b'f', 1, 0], None, Some(42)),
        ),
        (
            "bad_expression",
            fixture(&[0x16, 0, 5, 0, 0, 0, 1, 0, b'n'], None, None),
        ),
    ];
    let mut negatives = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracles.operands);
        command
            .current_dir(root)
            .arg(&path)
            .arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(command, &run.join(format!("negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid operand bundle produced a complete report".into());
        }
        negatives.push(name);
    }
    let valid = fixture(&[1, 0x10, 5, 0, 1, 0, b'f', 42, 0], None, Some(42));
    let path = run.join("form-variable-fixture.bin");
    write_new(&path, &valid)?;
    let mut command = Command::new(oracles.operands);
    command
        .current_dir(root)
        .arg(&path)
        .arg(install.join("FalloutNV.exe"));
    let output = run_logged(command, &run.join("form-variable-fixture.log"))?;
    let independent: Value = serde_json::from_slice(&output.stdout)?;
    use fallout_data::{
        obscript::{
            self,
            argument_census::{CommandSignature, Signatures},
            arguments, expression, operand_binding,
        },
        plugin, script_units,
    };
    let record = plugin::Record {
        header: plugin::RecordHeader {
            kind: *b"SCPT",
            offset: 0,
            stored_size: 0,
            flags: 0,
            form_id: 1,
            revision: [0; 4],
            version: 15,
            trailing_bytes: [0; 2],
        },
        payload: valid[28..].to_vec(),
        integrity_issue: None,
    };
    let fixture_units =
        script_units::decode(&record, "Fixture.esm", script_units::Limits::default())?;
    let program = obscript::decode(
        fixture_units[0]
            .compiled
            .ok_or("Missing fixture compiled bytes")?
            .data,
        obscript::Limits::default(),
    )?;
    let operators = expression::Operators::new(vec![expression::Operator {
        code: 11,
        precedence: 4,
        spelling: b"+".to_vec(),
    }])?;
    let signatures: Signatures = [(
        0x1001,
        CommandSignature {
            convention: arguments::Convention::Default,
            parameters: vec![arguments::Parameter {
                type_id: 4,
                optional_word: 0,
            }],
        },
    )]
    .into_iter()
    .collect();
    let binding = operand_binding::bind(&fixture_units[0], &program, &operators, &signatures, 16)?;
    if independent["compiled_units"][0]["binding_sha256"] != operand_binding::digest(&binding.uses)
        || independent["compiled_units"][0]["counts"]["roles"]["11"] != 1
    {
        return Err("Independent form-variable association fixture differs".into());
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":summaries,"counts":totals,"compiled_units_compared":cursor,
        "all_unit_metadata_and_operand_binding_digests_equal":true,"all_prior_native_call_coverage_equal":true,
        "deferred_foreign_local_scope":"reference context associated; declaration in loaded target script and runtime value remain unverified",
        "known_metadata_issues":tables["known_metadata_issues"],"fresh_table_regression":{"units_compared":tables["units_compared"],
            "all_metadata_fields_and_digests_equal":tables["all_metadata_fields_and_digests_equal"],"native_report_sha256":tables["native_report_sha256"],
            "rust_report_sha256":tables["rust_report_sha256"],"strict_integrity_failure_retained":tables["strict_unrelated_integrity_failure_retained"]},
        "fresh_argument_regression":{"compiled_bodies_compared":arguments["compiled_bodies_compared"],"counts":arguments["counts"],
            "all_call_extents_and_typed_argument_digests_equal":arguments["all_call_extents_and_typed_argument_digests_equal"],
            "native_report_sha256":arguments["native_report_sha256"],"rust_report_sha256":arguments["rust_report_sha256"],
            "expression_regression":arguments["fresh_expression_regression"]},
        "native_negative_cases_rejected":negatives,"form_variable_fixture_equal":true,"record_payloads_deferred":deferred,
        "executable_source_sha256":rust["executable_source_sha256"],"rust_report_sha256":digest(&rust_path)?,
        "native_report_sha256":digest(&native_path)?,"comparison_bundle_sha256":digest(&bundle_path)?,
        "comparison_scope":"independent owning tables, every encoded index association and metadata/SCDA hashes from Rust-extracted records; no loaded target/value or execution comparison",
        "execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Foreign local declarations and loaded reference values","Actual referenced form existence and native type checks",
            "Event operands, expression semantics, control flow and native effects","Eight conflicting repeated local indices need retail loading observations",
            "Three stale empty-unit reference counts remain diagnostic findings","Extension encodings and other executable variants"]}),
    )
}
