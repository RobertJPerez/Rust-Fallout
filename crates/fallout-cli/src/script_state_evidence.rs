//! Source-schema agreement and canonical-state engineering proofs. Neither
//! reader observes original running instances or accepts gameplay behavior.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn order_bundle(names: &[String]) -> Vec<u8> {
    let mut bytes = b"FRORDER1".to_vec();
    bytes.extend((names.len() as u16).to_le_bytes());
    for name in names {
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    bytes
}
fn compare(rust: &Value, native: &Value) -> Result<()> {
    for (key, value) in native.as_object().ok_or("Missing native schema object")? {
        if rust[key] != *value {
            return Err(format!("Script schema differs: {key}").into());
        }
    }
    for key in [
        "canonical_bytes_equal",
        "old_handles_rejected",
        "persistent_ids_preserved",
    ] {
        if rust["state_probe"][key] != true {
            return Err(format!("Canonical state check failed: {key}").into());
        }
    }
    if rust["state_probe"]["bytecode_executed"] != false
        || rust["state_probe"]["original_state_captured"] != false
    {
        return Err("Canonical probe made an unsupported execution claim".into());
    }
    Ok(())
}
struct Inputs<'a> {
    root: &'a Path,
    run: &'a Path,
    cli: &'a Path,
    oracle: &'a Path,
    install: &'a Path,
    order: &'a Path,
    binary_order: &'a Path,
}
fn phase(inputs: &Inputs<'_>, name: &str, cache: Option<&Path>) -> Result<(Value, Value)> {
    let rust_path = inputs.run.join(format!("{name}-rust.json"));
    let mut command = Command::new(inputs.cli);
    command
        .current_dir(inputs.root)
        .arg("script-state")
        .arg("--install")
        .arg(inputs.install)
        .arg("--load-order")
        .arg(inputs.order)
        .arg("--output")
        .arg(&rust_path);
    if let Some(cache) = cache {
        command.arg("--index-cache").arg(cache);
    }
    run_logged(command, &inputs.run.join(format!("{name}-rust.log")))?;
    let native_path = inputs.run.join(format!("{name}-native.json"));
    let mut command = Command::new(inputs.oracle);
    command
        .current_dir(inputs.root)
        .arg(inputs.install.join("Data"))
        .arg(inputs.binary_order);
    let output = run_logged(command, &inputs.run.join(format!("{name}-native.log")))?;
    write_new(&native_path, &output.stdout)?;
    let rust = json_file(&rust_path)?;
    let native = json_file(&native_path)?;
    compare(&rust, &native)?;
    let receipt = json!({"name":name,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "counts":rust["counts"],"state_probe":rust["state_probe"],"cache_reused":rust["index_cache"]["plugins"].as_array().map(|entries| entries.iter().map(|entry| entry["reused"].clone()).collect::<Vec<_>>()).unwrap_or_default()});
    Ok((rust, receipt))
}
fn previous_coverage(root: &Path, report: &Value) -> Result<Value> {
    let receipt_path = root.join("reports/checkpoint-24-loaded-scripts.json");
    let receipt = json_file(&receipt_path)?;
    let old_path = root.join("local/loaded-scripts-24-verified/loaded-scripts-rust.json");
    if digest(&old_path)? != receipt["rust_report_sha256"] {
        return Err("Bound loaded-script report changed".into());
    }
    let old = json_file(&old_path)?;
    if old["plugins"] != report["sources"] || old["metadata"] != report["metadata"] {
        return Err("Loaded-script source cohort changed".into());
    }
    let mut units = BTreeMap::new();
    for record in report["records"]
        .as_array()
        .ok_or("Missing schema records")?
    {
        for unit in record["units"].as_array().ok_or("Missing schema units")? {
            units.insert(
                (
                    serde_json::to_string(&record["key"])?,
                    unit["header_decoded_offset"]
                        .as_u64()
                        .ok_or("Schema marker")?,
                ),
                (record, unit),
            );
        }
    }
    let mut checked = 0;
    for script in old["scripts"]
        .as_array()
        .ok_or("Missing prior loaded scripts")?
    {
        let key = &script["handle"]["key"];
        let (record, unit) = units
            .remove(&(
                serde_json::to_string(&key["record"])?,
                key["header_decoded_offset"]
                    .as_u64()
                    .ok_or("Prior marker")?,
            ))
            .ok_or("Prior script missing from new schema")?;
        for (new, prior) in [
            ("source_name", "source_plugin"),
            ("record_file_offset", "record_file_offset"),
            ("record_flags", "record_flags"),
            ("decoded_sha256", "decoded_record_sha256"),
        ] {
            if record[new] != script["version"][prior] {
                return Err("Prior script source provenance differs".into());
            }
        }
        let mut declarations = BTreeMap::new();
        for declaration in script["declarations"]
            .as_array()
            .ok_or("Prior declarations")?
        {
            declarations
                .entry(declaration["index"].as_u64().ok_or("Prior local index")?)
                .or_insert(declaration);
        }
        let locals = unit["locals"].as_array().ok_or("New schema locals")?;
        if locals.len() != declarations.len() {
            return Err("Prior unique declarations differ".into());
        }
        for local in locals {
            let declaration = declarations
                .remove(&local["index"].as_u64().ok_or("New local index")?)
                .ok_or("Prior local absent")?;
            if local["declaration_decoded_offset"] != declaration["decoded_offset"] {
                return Err("First declaration offset changed".into());
            }
        }
        checked += 1;
    }
    if !units.is_empty() {
        return Err("Unaccounted new schema units".into());
    }
    Ok(
        json!({"bound_report_sha256":digest(&old_path)?,"receipt_sha256":digest(&receipt_path)?,"units":checked,"all_winning_source_units_accounted_for":true}),
    )
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let order = root.join("profiles/nv-inspection-order.json");
    let names: Vec<String> = serde_json::from_slice(&fs::read(&order)?)?;
    let binary = run.join("script-state-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle,
        install,
        order: &order,
        binary_order: &binary,
    };
    let (report, original) = phase(&inputs, "script-state", None)?;
    let coverage = previous_coverage(root, &report)?;
    let fixtures = fixtures(root, run, cli, oracle)?;
    let malformed = malformed(root, run, cli, oracle)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Independent original compiled schemas and native canonical state engineering checks; no captured retail live values",
        "oracle_binary_sha256":digest(oracle)?,"sources":report["sources"],"metadata":report["metadata"],"counts":report["counts"],
        "rust_report_sha256":original["rust_report_sha256"],"native_report_sha256":original["native_report_sha256"],
        "all_original_compiled_schemas_equal":true,"canonical_state_probe":report["state_probe"],"prior_winning_source_coverage":coverage,
        "synthetic_fixtures":fixtures,"malformed_cases_rejected":malformed,"catalogue_source_findings":report["catalogue_source_findings"],
        "constructor_defaults_verified":false,"bytecode_execution_ready":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original live initialization, lifecycle, scheduling and quest-delay timing","Native query/command execution and original numeric coercion","Original save import and full mutable world state","Cross-owner event-list resolution and retail differential traces"]}),
    )
}

fn field(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u16).to_le_bytes(),
        payload,
    ]
    .concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn declaration(index: u32, type_byte: u8) -> Vec<u8> {
    let mut data = [0; 24];
    data[..4].copy_from_slice(&index.to_le_bytes());
    data[16] = type_byte;
    [field(b"SLSD", &data), field(b"SCVR", b"fixture\0")].concat()
}
fn fixture_unit() -> Vec<u8> {
    let compiled = [0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0];
    let mut header = [0; 20];
    header[4..8].copy_from_slice(&3_u32.to_le_bytes());
    header[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    let mut body = [field(b"SCHR", &header), field(b"SCDA", &compiled)].concat();
    for (index, kind) in [
        (2, 1),
        (42, 0),
        (90, 0),
        (42, 1),
        (99, 7),
        (0, 0),
        (u32::MAX, 1),
    ] {
        body.extend(declaration(index, kind));
    }
    body.extend(field(b"SCRV", &90_u32.to_le_bytes()));
    body.extend(field(b"SCRV", &777_u32.to_le_bytes()));
    body.extend(field(b"SCRO", &0_u32.to_le_bytes()));
    body
}
fn fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Value> {
    let install = run.join("schema-fixture-install");
    fs::create_dir_all(install.join("Data"))?;
    write_new(
        &install.join("Data/FalloutNV.esm"),
        &[header(&[]), record(b"SCPT", 0x300, 0, &fixture_unit())].concat(),
    )?;
    write_new(&install.join("Data/Other.esm"), &header(&[]))?;
    let names = vec!["FalloutNV.esm".to_string(), "Other.esm".to_string()];
    let order = run.join("schema-fixture-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("schema-fixture-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let cache = run.join("schema-fixture-cache");
    fs::create_dir(&cache)?;
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle,
        install: &install,
        order: &order,
        binary_order: &binary,
    };
    let (cold, cold_receipt) = phase(&inputs, "schema-fixture-cold", Some(&cache))?;
    let (warm, warm_receipt) = phase(&inputs, "schema-fixture-warm", Some(&cache))?;
    if cold_receipt["cache_reused"] != json!([false, false])
        || warm_receipt["cache_reused"] != json!([true, true])
    {
        return Err("Schema cache state differs".into());
    }
    let reversed = names.into_iter().rev().collect::<Vec<_>>();
    let reordered = run.join("schema-fixture-reordered.json");
    write_new(&reordered, &serde_json::to_vec(&reversed)?)?;
    let reordered_binary = run.join("schema-fixture-reordered.bin");
    write_new(&reordered_binary, &order_bundle(&reversed))?;
    let inputs = Inputs {
        order: &reordered,
        binary_order: &reordered_binary,
        ..inputs
    };
    let (reordered, reordered_receipt) = phase(&inputs, "schema-fixture-reordered", None)?;
    if cold["records"] != warm["records"]
        || cold["records"] != reordered["records"]
        || cold["state_probe"] != reordered["state_probe"]
        || cold["state_probe"] != warm["state_probe"]
    {
        return Err("Canonical source schemas/state changed after cache or reordering".into());
    }
    Ok(
        json!({"scope":"Authored sparse/duplicate/reference/unknown/zero/full-width declarations without SCTX; missing SCRV declaration is retained as a dependency","phases":[cold_receipt,warm_receipt,reordered_receipt],"cache_and_reordering_keep_canonical_state_equal":true}),
    )
}
fn malformed(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Vec<String>> {
    let install = run.join("bad-schema-install");
    fs::create_dir_all(install.join("Data"))?;
    let names = vec!["FalloutNV.esm".to_string()];
    let order = run.join("bad-schema-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("bad-schema-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let marker = field(b"SCHR", &[0; 20]);
    let mut cases = vec![
        (
            "short-declaration",
            [marker.clone(), field(b"SLSD", &[0; 23])].concat(),
            0,
        ),
        (
            "unnamed-declaration",
            [marker.clone(), field(b"SLSD", &[0; 24])].concat(),
            0,
        ),
        (
            "orphan-name",
            [marker.clone(), field(b"SCVR", b"fixture\0")].concat(),
            0,
        ),
        (
            "unterminated-name",
            [
                marker.clone(),
                field(b"SLSD", &[0; 24]),
                field(b"SCVR", b"fixture"),
            ]
            .concat(),
            0,
        ),
        (
            "duplicate-compiled-body",
            [marker.clone(), field(b"SCDA", &[]), field(b"SCDA", &[])].concat(),
            0,
        ),
        (
            "short-begin",
            [marker, field(b"SCDA", &[0x10, 0, 0, 0])].concat(),
            0,
        ),
        ("unowned-compiled-field", field(b"SCDA", &[]), 0),
        ("partial-field-header", b"SCHR".to_vec(), 0),
        (
            "orphan-extended-field",
            field(b"XXXX", &20_u32.to_le_bytes()),
            0,
        ),
    ];
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let payload = fixture_unit();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&payload)?;
    let mut encoded = encoder.finish()?;
    *encoded.last_mut().ok_or("Checksum fixture")? ^= 1;
    cases.push((
        "compressed-checksum",
        [&(payload.len() as u32).to_le_bytes()[..], &encoded].concat(),
        fallout_data::plugin::COMPRESSED,
    ));
    let mut rejected = Vec::new();
    for (name, body, flags) in cases {
        fs::write(
            install.join("Data/FalloutNV.esm"),
            [header(&[]), record(b"SCPT", 0x300, flags, &body)].concat(),
        )?;
        let path = run.join(format!("bad-schema-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("script-state")
            .arg("--install")
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--output")
            .arg(&path);
        run_logged_status(command, &run.join(format!("bad-schema-{name}-rust.log")), 1)?;
        if path.exists() {
            return Err("Malformed schema published a Rust report".into());
        }
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(install.join("Data"))
            .arg(&binary);
        let output = run_logged_status(
            command,
            &run.join(format!("bad-schema-{name}-native.log")),
            1,
        )?;
        if !output.stdout.is_empty() {
            return Err("Malformed schema published a native report".into());
        }
        rejected.push(name.into());
    }
    Ok(rejected)
}
