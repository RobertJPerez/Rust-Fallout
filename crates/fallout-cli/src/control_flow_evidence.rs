//! Compare source-bound delimiter plans and retain every first unresolved body.
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
    let rust_path = inputs.run.join(format!("control-{name}-rust.json"));
    let mut rust = Command::new(inputs.cli);
    rust.current_dir(inputs.root)
        .arg("control-flow")
        .arg("--install")
        .arg(inputs.install)
        .arg("--bundle")
        .arg(bundle)
        .arg("--output")
        .arg(&rust_path);
    if diagnostic {
        rust.arg("--diagnose-structure");
    }
    run_logged_status(
        rust,
        &inputs.run.join(format!("control-{name}-rust.log")),
        status,
    )?;
    let mut native = Command::new(inputs.oracle);
    native.current_dir(inputs.root).arg(bundle);
    if diagnostic {
        native.arg("--diagnose-structure");
    }
    let output = run_logged_status(
        native,
        &inputs.run.join(format!("control-{name}-native.log")),
        status,
    )?;
    let native_path = inputs.run.join(format!("control-{name}-native.json"));
    write_new(&native_path, &output.stdout)?;
    let rust = json_file(&rust_path)?;
    let native = json_file(&native_path)?;
    if rust["structure"] != native["structure"]
        || rust["structure"]["bundle_sha256"] != digest(bundle)?
        || rust["execution_ready"] != false
        || native["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err(format!("Independent source control-flow differs: {name}").into());
    }
    let receipt = json!({"name":name,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "bundle_sha256":digest(bundle)?,"counts":rust["structure"]["counts"],"rust_exit_code":status,"native_exit_code":status});
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
    let source_path = run.join(EXTRACTION).join("expressions-rust.json");
    let source = json_file(&source_path)?;
    let bundle = run.join(EXTRACTION).join("expressions.bin");
    if source["comparison_bundle"]["sha256"] != digest(&bundle)? || source["issues"] != 0 {
        return Err("Fresh source extraction differs before structural comparison".into());
    }
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle,
        install,
    };
    let original_strict = reject(&inputs, &bundle, "original")?;
    let (rust, original) = compare(&inputs, &bundle, "original", true, 1)?;
    let planned = rust["structure"]["bodies"]
        .as_array()
        .ok_or("Missing source structures")?;
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
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
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
            return Err("Structural/query source cohorts differ".into());
        }
        for body in plugin["bodies"].as_array().ok_or("Missing source bodies")? {
            let plan = planned.get(cursor).ok_or("Missing planned body")?;
            if plan["bytes"] != body["bytes"] || plan["sha256"] != body["sha256"] {
                return Err("Structure differs from freshly extracted SCDA identity".into());
            }
            if !plan["issue"].is_null() {
                if !plan["structure"].is_null() {
                    return Err("Unresolved body exposes a complete structure".into());
                }
                let kind = plan["issue"]["kind"]
                    .as_str()
                    .ok_or("Missing finding kind")?;
                *kinds.entry(kind.to_owned()).or_default() += 1;
                findings.push(json!({"source_name":name,"source_sha256":plugin["source_sha256"],"site":body["site"],
                    "scda_sha256":body["sha256"],"issue":plan["issue"]}));
            } else if plan["structure"].is_null() {
                return Err("Source structure missing without a finding".into());
            }
            cursor += 1;
        }
    }
    let expected = json!({"instructions":142_218,"complete_bodies":14_461,"structural_issues":53,
        "events":5_592,"arms":24_636,"links":30_228,"maximum_depth":10});
    if cursor != planned.len()
        || cursor != 14_514
        || seen != source_map
        || rust["structure"]["counts"] != expected
        || kinds
            != BTreeMap::from([
                ("orphan_end_if".to_owned(), 37),
                ("orphan_arm".to_owned(), 7),
                ("arm_after_else".to_owned(), 4),
                ("raw_distance_mismatch".to_owned(), 5),
            ])
    {
        return Err("Installed structural coverage/findings changed".into());
    }
    let fixtures = fixtures(&inputs)?;
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"Source delimiter structure and raw distance relations; independent forward Rust/backward C++ pairing; no original VM semantics",
        "sources":sources,"compiled_bodies_compared":cursor,"counts":expected,
        "all_source_structures_and_findings_equal":true,"source_findings":findings,"finding_kinds":kinds,
        "original_comparison":original,"original_strict_rejection":original_strict,
        "fixtures":fixtures,"source_extraction_report_sha256":digest(&source_path)?,
        "oracle_binary_sha256":digest(oracle)?,"execution_ready":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original handling of irregular delimiters and distance fields","VM branch origins, conditional truth and short-circuit execution",
            "Event lifecycle, winning script-handle association, native effects and observable VM execution"]}))
}

fn instruction(bytes: &mut Vec<u8>, opcode: u16, operands: &[u8]) {
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&(operands.len() as u16).to_le_bytes());
    bytes.extend_from_slice(operands);
}
fn condition(bytes: &mut Vec<u8>, opcode: u16, skip: u16) {
    let operands = [&skip.to_le_bytes()[..], &[1, 0, b'1'][..]].concat();
    instruction(bytes, opcode, &operands);
}
fn bundle(body: &[u8]) -> Vec<u8> {
    [
        b"FROBS001".as_slice(),
        &(body.len() as u32).to_le_bytes(),
        body,
    ]
    .concat()
}
fn reject(inputs: &Inputs<'_>, path: &Path, name: &str) -> Result<Value> {
    let report = inputs.run.join(format!("control-strict-{name}.json"));
    let mut rust = Command::new(inputs.cli);
    rust.arg("control-flow")
        .arg("--install")
        .arg(inputs.install)
        .arg("--bundle")
        .arg(path)
        .arg("--output")
        .arg(&report);
    run_logged_status(
        rust,
        &inputs.run.join(format!("control-strict-{name}-rust.log")),
        1,
    )?;
    if report.exists() {
        return Err("Strict invalid source structure published a report".into());
    }
    let mut native = Command::new(inputs.oracle);
    native.arg(path);
    let output = run_logged_status(
        native,
        &inputs.run.join(format!("control-strict-{name}-native.log")),
        1,
    )?;
    if !output.stdout.is_empty() {
        return Err("Strict invalid native structure published a report".into());
    }
    Ok(
        json!({"name":name,"bundle_sha256":digest(path)?,"rust_exit_code":1,"native_exit_code":1,"reports_published":false}),
    )
}
fn fixtures(inputs: &Inputs<'_>) -> Result<Value> {
    let mut nested = Vec::new();
    condition(&mut nested, 0x16, 2);
    condition(&mut nested, 0x16, 0);
    instruction(&mut nested, 0x19, &[]);
    condition(&mut nested, 0x18, 0);
    instruction(&mut nested, 0x17, &[0, 0]);
    instruction(&mut nested, 0x19, &[]);
    let mut events = Vec::new();
    instruction(&mut events, 0x1d, &[]);
    instruction(&mut events, 0x10, &[3, 0, 19, 0, 0, 0, 0xff, 0]);
    events.extend_from_slice(&[0x1c, 0, 0, 0]);
    instruction(&mut events, 0x1000, &[9, 8, 7]);
    instruction(&mut events, 0x1e, &[]);
    instruction(&mut events, 0x11, &[]);
    instruction(&mut events, 0x10, &[6, 0, 4, 0, 0, 0]);
    instruction(&mut events, 0x11, &[]);
    let mut opaque = Vec::new();
    for opcode in [0x12, 0x13, 0x14, 0x15, 0x1f, 0x1000] {
        instruction(&mut opaque, opcode, &[0xff, 0]);
    }
    let mut deep = Vec::new();
    for level in 0..30_000 {
        condition(&mut deep, 0x16, (2 * (29_999 - level)) as u16);
    }
    for _ in 0..30_000 {
        instruction(&mut deep, 0x19, &[]);
    }
    let mut positives = Vec::new();
    for (name, body) in [
        ("empty", Vec::new()),
        ("nested-fragment", nested),
        ("event-arguments-and-caller", events),
        ("opaque-other-operands", opaque),
        ("deep-nesting", deep),
    ] {
        let path = inputs.run.join(format!("control-fixture-{name}.bin"));
        write_new(&path, &bundle(&body))?;
        let (report, receipt) = compare(inputs, &path, name, false, 0)?;
        if name == "deep-nesting" && report["structure"]["counts"]["maximum_depth"] != 30_000 {
            return Err("Deep independent structural depth differs".into());
        }
        positives.push(receipt);
    }
    let mut after_else = Vec::new();
    condition(&mut after_else, 0x16, 0);
    instruction(&mut after_else, 0x17, &[0, 0]);
    condition(&mut after_else, 0x18, 0);
    let mut crossing = Vec::new();
    instruction(&mut crossing, 0x10, &[0; 6]);
    condition(&mut crossing, 0x16, 0);
    instruction(&mut crossing, 0x11, &[]);
    let mut bad_distance = Vec::new();
    condition(&mut bad_distance, 0x16, 1);
    instruction(&mut bad_distance, 0x19, &[]);
    let mut nested_event = Vec::new();
    instruction(&mut nested_event, 0x10, &[0; 6]);
    instruction(&mut nested_event, 0x10, &[0; 6]);
    let mut too_deep = Vec::new();
    for _ in 0..65_537 {
        condition(&mut too_deep, 0x16, 0);
    }
    let mut negatives = Vec::new();
    for (name, body, expected) in [
        ("orphan-endif", vec![0x19, 0, 0, 0], "orphan_end_if"),
        ("orphan-end", vec![0x11, 0, 0, 0], "orphan_end"),
        ("orphan-arm", vec![0x17, 0, 2, 0, 0, 0], "orphan_arm"),
        ("unknown-opcode", vec![0x20, 0, 0, 0], "unsupported_opcode"),
        ("return-tail", vec![0x1e, 0, 1, 0, 0], "statement_operands"),
        (
            "condition-extent",
            vec![0x16, 0, 4, 0, 0, 0, 1, 0],
            "conditional_expression_extent",
        ),
        (
            "statement-caller",
            vec![0x1c, 0, 0, 0, 0x1e, 0, 0, 0],
            "statement_reference_prefix",
        ),
        (
            "unclosed-if",
            vec![0x16, 0, 4, 0, 0, 0, 0, 0],
            "unclosed_conditional",
        ),
        (
            "unclosed-event",
            vec![0x10, 0, 6, 0, 0, 0, 0, 0, 0, 0],
            "unclosed_event",
        ),
        ("arm-after-else", after_else, "arm_after_else"),
        ("event-crossing", crossing, "conditional_crosses_event"),
        ("raw-distance", bad_distance, "raw_distance_mismatch"),
        ("nested-event", nested_event, "nested_event"),
        ("depth-budget", too_deep, "depth_budget"),
    ] {
        let path = inputs.run.join(format!("control-fixture-{name}.bin"));
        write_new(&path, &bundle(&body))?;
        let strict = reject(inputs, &path, name)?;
        let (report, mut receipt) = compare(inputs, &path, name, true, 1)?;
        if report["structure"]["bodies"][0]["issue"]["kind"] != expected {
            return Err("Fixture finding differs".into());
        }
        receipt["strict_rejection"] = strict;
        negatives.push(receipt);
    }
    let mut malformed = Vec::new();
    for (name, bytes) in [
        ("bad-magic", b"FROBS002".to_vec()),
        ("short-length", b"FROBS001\0".to_vec()),
        (
            "truncated-body",
            [b"FROBS001".as_slice(), &[4, 0, 0, 0, 0]].concat(),
        ),
        ("truncated-header", bundle(&[0x16, 0])),
        ("short-event", bundle(&[0x10, 0, 1, 0, 0])),
        ("truncated-caller", bundle(&[0x1c, 0, 0, 0])),
    ] {
        let path = inputs.run.join(format!("control-fixture-{name}.bin"));
        write_new(&path, &bytes)?;
        malformed.push(reject(inputs, &path, name)?);
    }
    let mut oversized_instructions = Vec::new();
    for _ in 0..262_145 {
        instruction(&mut oversized_instructions, 0x1000, &[]);
    }
    let oversized_body = bundle(&vec![0; 4_194_305]);
    let mut aggregate = b"FROBS001".to_vec();
    let body = &oversized_instructions[..262_144 * 4];
    for _ in 0..8 {
        aggregate.extend_from_slice(&(body.len() as u32).to_le_bytes());
        aggregate.extend_from_slice(body);
    }
    let mut too_many_bodies = b"FROBS001".to_vec();
    for _ in 0..65_537 {
        too_many_bodies.extend_from_slice(&0u32.to_le_bytes());
    }
    let mut budgets = Vec::new();
    for (name, bytes) in [
        ("instruction-budget", bundle(&oversized_instructions)),
        ("body-byte-budget", oversized_body),
        ("aggregate-instruction-budget", aggregate),
        ("body-count-budget", too_many_bodies),
    ] {
        let path = inputs.run.join(format!("control-fixture-{name}.bin"));
        write_new(&path, &bytes)?;
        budgets.push(reject(inputs, &path, name)?);
    }
    Ok(
        json!({"positive_comparisons":positives,"strict_rejections_and_diagnostic_comparisons":negatives,
        "framing_rejections":malformed,"hard_budget_rejections":budgets}),
    )
}
