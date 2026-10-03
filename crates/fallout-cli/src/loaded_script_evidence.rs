//! Winning script definitions, independently rebuilt source identities and
//! completeness against checkpoint 17's bound full source-table scan.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) struct Oracles<'a> {
    pub scripts: &'a Path,
    pub records: &'a Path,
}
fn order_bundle(names: &[String]) -> Vec<u8> {
    let mut bytes = b"FRORDER1".to_vec();
    bytes.extend((names.len() as u16).to_le_bytes());
    for name in names {
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    bytes
}
fn native(
    root: &Path,
    run: &Path,
    oracle: &Path,
    data: &Path,
    order: &Path,
    bundle: &Path,
    name: &str,
) -> Result<(Value, PathBuf)> {
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(data).arg(order).arg(bundle);
    let output = run_logged(command, &run.join(format!("{name}.log")))?;
    let path = run.join(format!("{name}.json"));
    write_new(&path, &output.stdout)?;
    Ok((json_file(&path)?, path))
}
fn compare(rust: &Value, native: &Value) -> Result<()> {
    for key in [
        "plugins",
        "metadata",
        "scripts",
        "execution_ready",
        "live_event_lists_loaded",
        "retail_parity_accepted",
    ] {
        if rust[key] != native[key] {
            return Err(format!("Loaded-script comparison differs: {key}").into());
        }
    }
    for (key, value) in native["catalogue_counts"]
        .as_object()
        .ok_or("Missing native counts")?
    {
        if rust["counts"][key] != *value {
            return Err(format!("Loaded-script count differs: {key}").into());
        }
    }
    Ok(())
}
fn tuple_text(hash: &mut Sha256, value: &str) {
    hash.update((value.len() as u32).to_le_bytes());
    hash.update(value.as_bytes());
}
fn metadata_digest(report: &Value) -> Result<String> {
    let mut hash = Sha256::new();
    for script in report["scripts"]
        .as_array()
        .ok_or("Missing loaded scripts")?
    {
        let key = &script["handle"]["key"];
        tuple_text(
            &mut hash,
            key["record"]["origin_plugin"]
                .as_str()
                .ok_or("Missing origin")?,
        );
        hash.update(
            (key["record"]["local_id"]
                .as_u64()
                .ok_or("Missing local id")? as u32)
                .to_le_bytes(),
        );
        tuple_text(
            &mut hash,
            script["version"]["source_plugin"]
                .as_str()
                .ok_or("Missing source")?,
        );
        hash.update(
            script["version"]["record_file_offset"]
                .as_u64()
                .ok_or("Missing record offset")?
                .to_le_bytes(),
        );
        hash.update(
            (key["header_decoded_offset"]
                .as_u64()
                .ok_or("Missing marker")? as u32)
                .to_le_bytes(),
        );
        tuple_text(
            &mut hash,
            script["version"]["metadata_sha256"]
                .as_str()
                .ok_or("Missing metadata hash")?,
        );
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn coverage_bundle(root: &Path, run: &Path, names: &[String]) -> Result<(PathBuf, Value)> {
    let old_report_path = root.join("local/bindings-17-verified/bindings-rust.json");
    let old_bundle_path = root.join("local/bindings-17-verified/bindings.bin");
    let receipt_path = root.join("reports/checkpoint-17-script-bindings.json");
    let receipt = json_file(&receipt_path)?;
    if digest(&old_report_path)? != receipt["rust_report_sha256"]
        || digest(&old_bundle_path)? != receipt["comparison_bundle_sha256"]
    {
        return Err("Bound full source-table evidence changed".into());
    }
    let old = json_file(&old_report_path)?;
    let bytes = fs::read(&old_bundle_path)?;
    if bytes.get(..8) != Some(b"FRUNIT01") {
        return Err("Full table bundle magic differs".into());
    }
    let mut cursor = 8;
    let mut output = b"FRCOVER1".to_vec();
    let mut records = 0_u64;
    let mut units = 0_u64;
    for source in old["plugins"]
        .as_array()
        .ok_or("Missing full source plugins")?
    {
        let name = source["source_name"]
            .as_str()
            .ok_or("Missing full source name")?;
        let index = names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or("Full source absent from inspection order")?;
        let mut locations = BTreeMap::new();
        for unit in source["units"]
            .as_array()
            .ok_or("Missing full source units")?
        {
            let offset = unit["record_file_offset"]
                .as_u64()
                .ok_or("Missing full record offset")?;
            let row = (
                unit["record_kind"]
                    .as_str()
                    .ok_or("Missing record kind")?
                    .to_string(),
                unit["form_id"].as_u64().ok_or("Missing raw id")? as u32,
            );
            if locations
                .insert(offset, row.clone())
                .is_some_and(|prior| prior != row)
            {
                return Err("Full source record identity conflict".into());
            }
            units += 1;
        }
        for (offset, (kind, raw)) in locations {
            let header = bytes
                .get(cursor..cursor + 20)
                .ok_or("Short full bundle record")?;
            if &header[..4] != kind.as_bytes()
                || u32::from_le_bytes(header[4..8].try_into()?) != raw
                || u64::from_le_bytes(header[8..16].try_into()?) != offset
            {
                return Err("Full table bundle record differs from its bound census".into());
            }
            let length = u32::from_le_bytes(header[16..20].try_into()?) as usize;
            let row = bytes
                .get(cursor..cursor + 20 + length)
                .ok_or("Short full bundle body")?;
            output.push(index as u8);
            output.extend(row);
            cursor += row.len();
            records += 1;
        }
    }
    if cursor != bytes.len() {
        return Err("Full table bundle has surplus records".into());
    }
    let path = run.join("full-source-tables.bin");
    write_new(&path, &output)?;
    Ok((
        path,
        json!({"source_checkpoint":17,"receipt_sha256":digest(&receipt_path)?,"full_report_sha256":digest(&old_report_path)?,
        "full_bundle_sha256":digest(&old_bundle_path)?,"records":records,"units":units}),
    ))
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let order_path = root.join("profiles/nv-inspection-order.json");
    let names: Vec<String> = serde_json::from_value(json_file(&order_path)?)?;
    let binary_order = run.join("script-order.bin");
    write_new(&binary_order, &order_bundle(&names))?;
    let rust_path = run.join("loaded-scripts-rust.json");
    let bundle = run.join("loaded-scripts.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("loaded-scripts")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order_path)
        .arg("--comparison-bundle")
        .arg(&bundle)
        .arg("--output")
        .arg(&rust_path);
    let output = run_logged_status(command, &run.join("loaded-scripts-rust.log"), 1)?;
    if !String::from_utf8(output.stderr)?
        .contains("loaded script catalogue retains source findings")
    {
        return Err("Script catalogue failed without expected source diagnostic".into());
    }
    let rust = json_file(&rust_path)?;
    if rust["counts"]["scripts_with_issues"] != 3
        || rust["counts"]["source_ownership_findings"] != 0
        || rust["execution_ready"] != false
        || rust["live_event_lists_loaded"] != false
        || rust["retail_parity_accepted"] != false
        || rust["comparison_bundle"]["sha256"] != digest(&bundle)?
    {
        return Err("Loaded-script source boundary differs".into());
    }
    let (native_report, native_path) = native(
        root,
        run,
        oracles.scripts,
        &install.join("Data"),
        &binary_order,
        &bundle,
        "loaded-scripts-native",
    )?;
    compare(&rust, &native_report)?;
    let (full_bundle, bound) = coverage_bundle(root, run, &names)?;
    let (coverage_report, coverage_path) = native(
        root,
        run,
        oracles.scripts,
        &install.join("Data"),
        &binary_order,
        &full_bundle,
        "loaded-scripts-coverage",
    )?;
    if coverage_report["plugins"] != rust["plugins"]
        || coverage_report["metadata"] != rust["metadata"]
        || coverage_report["coverage"]["winning_metadata_sha256"] != metadata_digest(&rust)?
        || coverage_report["coverage"]["winning_units"] != rust["counts"]["scripts"]
        || coverage_report["coverage"]["winning_records"] != rust["counts"]["records_retained"]
        || coverage_report["coverage"]["full_source_units"] != bound["units"]
        || coverage_report["coverage"]["full_source_records"] != bound["records"]
    {
        return Err("Full source-table winning coverage differs".into());
    }
    let native_negatives = negatives(
        root,
        run,
        oracles.scripts,
        &install.join("Data"),
        &binary_order,
        &bundle,
    )?;
    let fixtures = fixtures(root, run, cli, oracles.scripts)?;
    // The shared original header reader changed location. Re-run its complete
    // header/membership/cache/source-fixture comparison at this source revision.
    let header_regression =
        super::dialogue_evidence::run(root, run, cli, oracles.records, install)?;
    let issues = rust["scripts"].as_array().ok_or("Missing script rows")?.iter().filter(|row| row["issues"].as_array().is_some_and(|a| !a.is_empty())).map(|row|
        json!({"key":row["handle"]["key"],"source_plugin":row["version"]["source_plugin"],"record_file_offset":row["version"]["record_file_offset"],"issues":row["issues"]})).collect::<Vec<_>>();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","counts":rust["counts"],"plugins":rust["plugins"],"metadata":rust["metadata"],
        "all_loaded_definitions_equal":true,"all_version_digests_equal":true,"all_source_reference_states_equal":true,
        "completeness":coverage_report["coverage"],"bound_full_scan":bound,"known_source_issues":issues,"source_fixtures":fixtures,
        "native_negative_cases_rejected":native_negatives,"fresh_header_membership_regression":header_regression,
        "rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"coverage_report_sha256":digest(&coverage_path)?,
        "comparison_bundle_sha256":digest(&bundle)?,"coverage_bundle_sha256":digest(&full_bundle)?,
        "comparison_scope":"direct original header identities and whole-record winners; decoded script fields, owners, ordered references, declarations and version digests; completeness filtered independently from the bound full source scan",
        "execution_ready":false,"live_event_lists_loaded":false,"retail_parity_accepted":false,
        "known_gaps":["compressed payload extraction remains Rust-owned; native reads original compressed decoded-size prefixes and compares previously bound decoded table coverage",
        "source-defined winner policy is not retail record-specific merge certification","PACK, TERM, PERK and placed-reference script owner roles remain unverified",
        "SCRV values and the hardcoded player reference need live runtime bindings","no script scheduling, evaluation, command effects or quest behavior"]}),
    )
}

fn negatives(
    root: &Path,
    run: &Path,
    oracle: &Path,
    data: &Path,
    order: &Path,
    valid: &Path,
) -> Result<Vec<String>> {
    let bytes = fs::read(valid)?;
    let mut cases = Vec::new();
    cases.push(("short_magic", b"FRCAT".to_vec()));
    cases.push(("wrong_magic", b"NOTACAT!".to_vec()));
    cases.push(("short_record", b"FRCAT001\0SCPT".to_vec()));
    for (name, at, replacement) in [
        ("invalid_source", 8, vec![254]),
        ("unsupported_kind", 9, b"LAND".to_vec()),
        ("null_form", 13, vec![0; 4]),
        ("different_flags", 17, 0x20_u32.to_le_bytes().to_vec()),
        ("different_offset", 21, u64::MAX.to_le_bytes().to_vec()),
        (
            "record_budget",
            29,
            (64_u32 * 1024 * 1024 + 1).to_le_bytes().to_vec(),
        ),
    ] {
        let mut bad = bytes.clone();
        bad[at..at + replacement.len()].copy_from_slice(&replacement);
        cases.push((name, bad));
    }
    let first_length = u32::from_le_bytes(bytes[29..33].try_into()?) as usize;
    cases.push(("truncated_payload", bytes[..32 + first_length].to_vec()));
    let mut duplicate = bytes.clone();
    duplicate.extend(&bytes[8..33 + first_length]);
    cases.push(("duplicate_record", duplicate));
    let mut surplus = bytes.clone();
    surplus.push(0);
    cases.push(("surplus_byte", surplus));
    let mut rejected = Vec::new();
    for (name, bad) in cases {
        let path = run.join(format!("catalogue-negative-{name}.bin"));
        write_new(&path, &bad)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(data).arg(order).arg(path);
        let output = run_logged_status(
            command,
            &run.join(format!("catalogue-negative-{name}.log")),
            1,
        )?;
        if !output.stdout.is_empty() {
            return Err("Invalid catalogue emitted a report".into());
        }
        rejected.push(name.into());
    }
    Ok(rejected)
}

fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn record(kind: &[u8; 4], raw: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &raw.to_le_bytes(),
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
fn fixture_unit() -> Vec<u8> {
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&7_u32.to_le_bytes());
    schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
    let mut bytes = field(b"SCHR", &schr);
    bytes.extend(field(b"SCDA", &[0x1d, 0, 0, 0]));
    for (index, name) in [
        (42_u32, b"first_\xe9\0".as_slice()),
        (42, b"second\0"),
        (0, b"zero\0"),
    ] {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        bytes.extend(field(b"SLSD", &declaration));
        bytes.extend(field(b"SCVR", name));
    }
    for (kind, raw) in [
        (b"SCRO", 0x100_u32),
        (b"SCRO", 0x200),
        (b"SCRO", 0x300),
        (b"SCRO", 0),
        (b"SCRO", 0x14),
        (b"SCRV", 42),
        (b"SCRV", 99),
    ] {
        bytes.extend(field(kind, &raw.to_le_bytes()));
    }
    bytes
}
fn fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Value> {
    let install = run.join("script-fixture-install");
    let data = install.join("Data");
    fs::create_dir_all(&data)?;
    write_new(&data.join("Other.esm"), &header(&[]))?;
    write_new(
        &data.join("FalloutNV.esm"),
        &[
            header(&[]),
            record(b"ACTI", 0x100, 0, &[]),
            record(b"ACTI", 0x200, 0, &[]),
            record(b"SCPT", 0x400, 0, &fixture_unit()),
            record(b"SCPT", 0x500, 0, &fixture_unit()),
        ]
        .concat(),
    )?;
    let unit = fixture_unit();
    let info = [unit.clone(), field(b"NEXT", &[]), unit.clone()].concat();
    let quest = [
        field(b"INDX", &7_i16.to_le_bytes()),
        field(b"QSDT", &[0]),
        unit.clone(),
    ]
    .concat();
    write_new(
        &data.join("Patch.esp"),
        &[
            header(&["FalloutNV.esm"]),
            record(b"ACTI", 0x200, 0x20, &[]),
            record(b"SCPT", 0x400, 0, &unit),
            record(b"SCPT", 0x500, 0x20, &[]),
            record(b"INFO", 0x0100_0600, 0, &info),
            record(b"QUST", 0x0100_0700, 0, &quest),
            record(b"PACK", 0x0100_0800, 0, &unit),
        ]
        .concat(),
    )?;
    let names = vec![
        "FalloutNV.esm".to_string(),
        "Other.esm".into(),
        "Patch.esp".into(),
    ];
    let order = run.join("script-fixture-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("script-fixture-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let bundle = run.join("script-fixture.bin");
    let report = run.join("script-fixture.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("loaded-scripts")
        .arg("--install")
        .arg(&install)
        .arg("--load-order")
        .arg(&order)
        .arg("--comparison-bundle")
        .arg(&bundle)
        .arg("--output")
        .arg(&report);
    run_logged(command, &run.join("script-fixture.log"))?;
    let rust = json_file(&report)?;
    let (native_report, native_path) = native(
        root,
        run,
        oracle,
        &data,
        &binary,
        &bundle,
        "script-fixture-native",
    )?;
    compare(&rust, &native_report)?;
    let statuses = rust["counts"]["reference_statuses"]
        .as_object()
        .ok_or("Missing fixture reference states")?;
    let states: BTreeSet<_> = statuses.keys().map(String::as_str).collect();
    if states
        != BTreeSet::from([
            "defined_form",
            "deleted_form",
            "missing_form",
            "null_form",
            "runtime_dependency",
            "dynamic_variable",
            "missing_variable_declaration",
        ])
        || rust["counts"]["scripts"] != 5
        || rust["counts"]["deleted_candidates_skipped"] != 1
        || rust["counts"]["duplicate_variable_indices"] != 5
    {
        return Err("Loaded script source fixture coverage differs".into());
    }
    Ok(
        json!({"scripts_compared":5,"resolution_states_compared":7,"all_fields_equal":true,"deleted_winner_skipped":true,
        "duplicate_sparse_and_zero_variables_preserved":true,"begin_end_and_log_entry_owners_compared":true,"unknown_package_owner_retained":true,
        "rust_report_sha256":digest(&report)?,"native_report_sha256":digest(&native_path)?}),
    )
}
