//! Bind plans and findings to freshly extracted source bodies. The two planners
//! build trees in opposite directions; raw corpus bytes remain under local/.
use super::{Result, digest, json_file, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, process::Command};

const EXTRACTION: &str = "source-item-regression/native-migration-regression/item-state-regression/leveled-source-regression/base-inventory-regression/foreign-runtime-regression/quest-script-regression/operand-regression/argument-regression/expression-regression";

struct Inputs<'a> {
    root: &'a Path,
    run: &'a Path,
    cli: &'a Path,
    oracle: &'a Path,
    install: &'a Path,
}

fn compare(
    inputs: &Inputs<'_>,
    bundle: &Path,
    name: &str,
    diagnostic: bool,
    status: i32,
) -> Result<(Value, Value)> {
    let Inputs {
        root,
        run,
        cli,
        oracle,
        install,
    } = *inputs;
    let rust_path = run.join(format!("{name}-rust.json"));
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("expression-plans")
        .arg("--install")
        .arg(install)
        .arg("--bundle")
        .arg(bundle)
        .arg("--output")
        .arg(&rust_path);
    if diagnostic {
        command.arg("--diagnose-structure");
    }
    run_logged_status(command, &run.join(format!("{name}-rust.log")), status)?;
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(bundle)
        .arg(install.join("FalloutNV.exe"));
    if diagnostic {
        command.arg("--diagnose-structure");
    }
    let output = run_logged_status(command, &run.join(format!("{name}-native.log")), status)?;
    let native_path = run.join(format!("{name}-native.json"));
    write_new(&native_path, &output.stdout)?;
    let rust = json_file(&rust_path)?;
    let native = json_file(&native_path)?;
    for key in [
        "plans",
        "operator_descriptors",
        "executable_source_sha256",
        "execution_ready",
    ] {
        if rust[key] != native[key] {
            return Err(format!("Independent expression plan differs: {name}/{key}").into());
        }
    }
    if rust["plans"]["bundle_sha256"] != digest(bundle)?
        || rust["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Expression plan source/scope differs".into());
    }
    let receipt = json!({"name":name,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "bundle_sha256":digest(bundle)?,"counts":rust["plans"]["counts"],"rust_exit_code":status,"native_exit_code":status});
    Ok((rust, receipt))
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
    sources: &Value,
) -> Result<Value> {
    let extraction_dir = run.join(EXTRACTION);
    let source_path = extraction_dir.join("expressions-rust.json");
    let source = json_file(&source_path)?;
    let bundle = extraction_dir.join("expressions.bin");
    if source["comparison_bundle"]["sha256"] != digest(&bundle)? || source["issues"] != 0 {
        return Err("Fresh source expression extraction differs".into());
    }
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle,
        install,
    };
    let (rust, original) = compare(&inputs, &bundle, "expression-plans", true, 1)?;
    if rust["operator_descriptors"] != source["operator_descriptors"]
        || rust["executable_source_sha256"] != source["executable_source_sha256"]
    {
        return Err("Source and plan operator models differ".into());
    }
    let planned = rust["plans"]["bodies"]
        .as_array()
        .ok_or("Missing planned bodies")?;
    let source_map: BTreeMap<_, _> = sources
        .as_array()
        .ok_or("Missing source cohort")?
        .iter()
        .map(|s| {
            (
                s["source_name"].as_str().unwrap_or(""),
                (s["source_sha256"].clone(), s["source_bytes"].clone()),
            )
        })
        .collect();
    let mut seen = BTreeMap::new();
    let mut cursor = 0;
    let mut findings = Vec::new();
    for plugin in source["plugins"]
        .as_array()
        .ok_or("Missing source plugins")?
    {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing source name")?;
        let identity = (
            plugin["source_sha256"].clone(),
            plugin["source_bytes"].clone(),
        );
        if source_map.get(name) != Some(&identity) || seen.insert(name, identity).is_some() {
            return Err("Expression plan/query source cohorts differ".into());
        }
        for body in plugin["bodies"].as_array().ok_or("Missing source bodies")? {
            let plan = planned.get(cursor).ok_or("Missing planned source body")?;
            if plan["bytes"] != body["bytes"] || plan["sha256"] != body["sha256"] {
                return Err("Planned body differs from source extraction".into());
            }
            let original_rows = body["statements"]
                .as_array()
                .ok_or("Missing original statements")?;
            let plan_rows = plan["statements"]
                .as_array()
                .ok_or("Missing plan statements")?;
            if original_rows.len() != plan_rows.len() {
                return Err("Planned statement coverage differs".into());
            }
            for (before, after) in original_rows.iter().zip(plan_rows) {
                for key in [
                    "instruction_scda_offset",
                    "opcode",
                    "expression_operand_offset",
                    "token_sha256",
                ] {
                    if before[key] != after[key] {
                        return Err(format!("Planned source statement differs: {key}").into());
                    }
                }
                if !after["issue"].is_null() {
                    if !after["plan"].is_null() {
                        return Err("Incomplete structure has a complete plan".into());
                    }
                    findings.push(json!({"source_name":name,"source_sha256":plugin["source_sha256"],"site":body["site"],
                        "scda_sha256":body["sha256"],"statement":after}));
                }
            }
            cursor += 1;
        }
    }
    if cursor != planned.len()
        || seen != source_map
        || findings.len() != 3
        || rust["plans"]["counts"]["statements"] != 53_404
        || rust["plans"]["counts"]["complete_plans"] != 53_401
        || rust["plans"]["counts"]["structural_issues"] != findings.len()
    {
        return Err("Installed expression plan coverage/findings changed".into());
    }
    for finding in &findings {
        if finding["statement"]["issue"]["kind"] != "residual"
            || finding["statement"]["issue"]["available_operands"] != 2
        {
            return Err("Original structural finding differs".into());
        }
    }
    let fixtures = fixtures(root, run, cli, oracle, install)?;
    let mut build = Command::new("powershell");
    build.current_dir(root).args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "tools/cargo.ps1",
        "build",
        "--locked",
        "-p",
        "fallout-cli",
    ]);
    run_logged_status(build, &run.join("debug-cli-build.log"), 0)?;
    let debug = root.join("target/debug/fallout.exe");
    let debug_before = digest(&debug)?;
    let mut command = Command::new(&debug);
    command.arg("--help");
    let help = run_logged_status(command, &run.join("debug-cli-help.log"), 0)?;
    if !String::from_utf8_lossy(&help.stdout).contains("expression-plans")
        || digest(&debug)? != debug_before
    {
        return Err("Debug CLI startup evidence changed".into());
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Postfix structure under the pinned vanilla analyzer model; all source findings retained; no original execution acceptance",
        "sources":sources,"executable_source_sha256":rust["executable_source_sha256"],"operator_descriptors":rust["operator_descriptors"],
        "compiled_bodies_compared":cursor,"counts":rust["plans"]["counts"],"all_plan_tuples_and_findings_equal":true,
        "source_findings":findings,"original_comparison":original,"fixtures":fixtures,
        "source_extraction_report_sha256":digest(&source_path)?,"debug_cli_sha256":debug_before,"debug_cli_help_exit_code":0,
        "oracle_binary_sha256":digest(oracle)?,"execution_ready":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Measured retail handling of the three residual-stack expressions","Numeric conversion/precision, short-circuit behavior and native return coercion","Control-flow jump origins, runtime scheduling, effects and VM execution"]}),
    )
}

fn fixture(expression: &[u8]) -> Vec<u8> {
    let operands = [
        &[0; 2],
        &(expression.len() as u16).to_le_bytes(),
        expression,
    ]
    .concat();
    let body = [
        0x16_u16.to_le_bytes().as_slice(),
        &(operands.len() as u16).to_le_bytes(),
        &operands,
    ]
    .concat();
    [
        b"FROBS001".as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &body,
    ]
    .concat()
}
fn fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path, install: &Path) -> Result<Value> {
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle,
        install,
    };
    let positives: [(&str, &[u8]); 5] = [
        ("ordered-binary-unary", b"10 3 - 2 * ~"),
        ("logical-structure", b"1 2 && 3 ||"),
        ("opaque-literal", &[b'"', 4, 0, 0, 0xff, b'+', b'~']),
        ("explicit-context", &[b'r', 0, 0, b'f', 7, 0]),
        (
            "opaque-command",
            &[b'r', 2, 0, b'X', 0x2f, 0x10, 2, 0, 0xff, 0],
        ),
    ];
    let mut passed = Vec::new();
    for (name, expression) in positives {
        let path = run.join(format!("plan-fixture-{name}.bin"));
        write_new(&path, &fixture(expression))?;
        let (_, receipt) = compare(&inputs, &path, name, false, 0)?;
        passed.push(receipt);
    }
    let mut deep = b"1".to_vec();
    deep.extend(std::iter::repeat_n(b'~', 50_000));
    let path = run.join("plan-fixture-deep-unary.bin");
    write_new(&path, &fixture(&deep))?;
    let (deep_report, receipt) = compare(&inputs, &path, "deep-unary", false, 0)?;
    if deep_report["plans"]["counts"]["maximum_height"] != 50_001 {
        return Err("Independent deep plan height differs".into());
    }
    passed.push(receipt);
    let negatives: [(&str, &[u8]); 9] = [
        ("empty", b""),
        ("binary-underflow", b"1 +"),
        ("unary-underflow", b"~"),
        ("extra-operands", b"1 2"),
        ("left-bracket", b"1 ("),
        ("right-bracket", b"1 )"),
        ("dangling-prefix", &[b'r', 1, 0]),
        ("replaced-prefix", &[b'r', 1, 0, b'r', 2, 0, b'f', 3, 0]),
        ("unconsumed-prefix", &[b'r', 1, 0, b'G', 2, 0]),
    ];
    let mut rejected = Vec::new();
    for (name, expression) in negatives {
        let path = run.join(format!("plan-fixture-{name}.bin"));
        write_new(&path, &fixture(expression))?;
        let output_path = run.join(format!("strict-{name}-rust.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("expression-plans")
            .arg("--install")
            .arg(install)
            .arg("--bundle")
            .arg(&path)
            .arg("--output")
            .arg(&output_path);
        run_logged_status(command, &run.join(format!("strict-{name}-rust.log")), 1)?;
        if output_path.exists() {
            return Err("Strict invalid plan published a report".into());
        }
        let mut command = Command::new(oracle);
        command.arg(&path).arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(command, &run.join(format!("strict-{name}-native.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Strict invalid native plan published a report".into());
        }
        let (_, receipt) = compare(&inputs, &path, name, true, 1)?;
        rejected.push(receipt);
    }
    Ok(
        json!({"positive_comparisons":passed,"strict_rejections_and_diagnostic_comparisons":rejected}),
    )
}
