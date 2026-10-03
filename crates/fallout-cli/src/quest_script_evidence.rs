//! Independently compare static quest attachments and every foreign operand.
//! Runtime-dependent contexts remain reported outcomes, never fabricated values.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) struct Oracles<'a> {
    pub quests: &'a Path,
    pub scripts: &'a Path,
    pub records: &'a Path,
    pub operands: super::operand_evidence::Oracles<'a>,
}
struct Input<'a> {
    data: &'a Path,
    order: &'a Path,
    scripts: &'a Path,
    quests: &'a Path,
    executable: &'a Path,
}
fn native(
    root: &Path,
    run: &Path,
    oracle: &Path,
    input: Input<'_>,
    name: &str,
) -> Result<(Value, PathBuf)> {
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(input.data)
        .arg(input.order)
        .arg(input.scripts)
        .arg(input.quests)
        .arg(input.executable);
    let output = run_logged(command, &run.join(format!("{name}.log")))?;
    let path = run.join(format!("{name}.json"));
    write_new(&path, &output.stdout)?;
    Ok((json_file(&path)?, path))
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
fn compare(rust: &Value, native: &Value) -> Result<()> {
    for (key, value) in native.as_object().ok_or("Missing native quest object")? {
        if rust[key] != *value {
            return Err(format!("Independent quest script comparison differs: {key}").into());
        }
    }
    Ok(())
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
    let binary_order = run.join("quest-order.bin");
    write_new(&binary_order, &order_bundle(&names))?;
    let scripts = run.join("quest-scripts.bin");
    let quests = run.join("quest-attachments.bin");
    let rust_path = run.join("quest-scripts-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("quest-scripts")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order_path)
        .arg("--script-comparison-bundle")
        .arg(&scripts)
        .arg("--quest-comparison-bundle")
        .arg(&quests)
        .arg("--output")
        .arg(&rust_path);
    let output = run_logged_status(command, &run.join("quest-scripts-rust.log"), 1)?;
    if !String::from_utf8(output.stderr)?
        .contains("quest script inspection retains source or operand findings")
    {
        return Err("Quest inspection failed without expected diagnostic".into());
    }
    let rust = json_file(&rust_path)?;
    if rust["catalogue_counts"]["scripts_with_issues"] != 3
        || rust["quest_counts"]["source_findings"] != 2
        || rust["operand_counts"]["missing_bindings"] != 0
        || rust["operand_counts"]["decode_issues"] != 0
        || rust["live_values_resolved"] != false
        || rust["execution_ready"] != false
        || rust["retail_parity_accepted"] != false
        || rust["script_comparison_bundle"]["sha256"] != digest(&scripts)?
        || rust["quest_comparison_bundle"]["sha256"] != digest(&quests)?
    {
        return Err("Quest script source/behavior boundary differs".into());
    }
    let data = install.join("Data");
    let executable = install.join("FalloutNV.exe");
    let (native_report, native_path) = native(
        root,
        run,
        oracles.quests,
        Input {
            data: &data,
            order: &binary_order,
            scripts: &scripts,
            quests: &quests,
            executable: &executable,
        },
        "quest-scripts-native",
    )?;
    compare(&rust, &native_report)?;
    let fixture = fixtures(root, run, cli, oracles.quests, &executable)?;
    let negatives = negative_bundles(
        root,
        run,
        oracles.quests,
        Input {
            data: &data,
            order: &binary_order,
            scripts: &scripts,
            quests: &quests,
            executable: &executable,
        },
    )?;
    let loaded_directory = run.join("loaded-script-regression");
    fs::create_dir(&loaded_directory)?;
    let loaded = super::loaded_script_evidence::run(
        root,
        &loaded_directory,
        cli,
        super::loaded_script_evidence::Oracles {
            scripts: oracles.scripts,
            records: oracles.records,
        },
        install,
    )?;
    if loaded["counts"] != rust["catalogue_counts"]
        || loaded["plugins"] != rust["plugins"]
        || loaded["metadata"] != rust["metadata"]
    {
        return Err("Static quest loading changed definition catalogue coverage".into());
    }
    let operands_directory = run.join("operand-regression");
    fs::create_dir(&operands_directory)?;
    let operands =
        super::operand_evidence::run(root, &operands_directory, cli, oracles.operands, install)?;
    // Compare every winning compiled unit with the freshly repeated full source
    // operand scan. The full scan includes overrides that are correctly absent
    // from the loaded catalogue; source identity selects the surviving unit.
    let full_path = operands_directory.join("operand-bindings-rust.json");
    let full = json_file(&full_path)?;
    let mut expected = std::collections::BTreeMap::new();
    for plugin in full["plugins"]
        .as_array()
        .ok_or("Missing full operand plugins")?
    {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing operand source")?;
        for unit in plugin["compiled_units"]
            .as_array()
            .ok_or("Missing operand units")?
        {
            let key = (
                name.to_string(),
                unit["record_file_offset"]
                    .as_u64()
                    .ok_or("Missing operand record offset")?,
                unit["header_decoded_offset"]
                    .as_u64()
                    .ok_or("Missing operand marker")?,
            );
            if expected.insert(key, unit).is_some() {
                return Err("Duplicate full operand identity".into());
            }
        }
    }
    let loaded_report = json_file(&loaded_directory.join("loaded-scripts-rust.json"))?;
    let mut source_versions = std::collections::BTreeMap::new();
    for script in loaded_report["scripts"]
        .as_array()
        .ok_or("Missing loaded regression scripts")?
    {
        source_versions.insert(
            serde_json::to_string(&script["handle"])?,
            &script["version"],
        );
    }
    let units = rust["compiled_units"]
        .as_array()
        .ok_or("Missing loaded compiled units")?;
    for unit in units {
        let version = source_versions
            .get(&serde_json::to_string(&unit["handle"])?)
            .ok_or("Compiled script handle absent from loaded regression")?;
        let key = (
            version["source_plugin"]
                .as_str()
                .ok_or("Missing script source")?
                .to_string(),
            version["record_file_offset"]
                .as_u64()
                .ok_or("Missing source offset")?,
            unit["handle"]["key"]["header_decoded_offset"]
                .as_u64()
                .ok_or("Missing source marker")?,
        );
        let prior = expected
            .get(&key)
            .ok_or("Winning compiled unit absent from full operand scan")?;
        if prior["binding_sha256"] != unit["binding_sha256"]
            || prior["counts"] != unit["counts"]
            || prior["metadata_sha256"] != version["metadata_sha256"]
            || prior["compiled_sha256"] != version["compiled_sha256"]
        {
            return Err("Loaded operand/table interpretation differs from full source scan".into());
        }
    }
    let findings = rust["quests"]
        .as_array()
        .ok_or("Missing quest rows")?
        .iter()
        .filter(|row| row["findings"].as_array().is_some_and(|a| !a.is_empty()))
        .map(|row| json!({"quest":row["quest"],"source":row["source"],"findings":row["findings"]}))
        .collect::<Vec<_>>();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","plugins":rust["plugins"],"metadata":rust["metadata"],"catalogue_counts":rust["catalogue_counts"],
        "quest_counts":rust["quest_counts"],"operand_counts":rust["operand_counts"],"compiled_units_compared":units.len(),"known_quest_findings":findings,
        "all_static_attachments_equal":true,"all_foreign_operands_equal":true,"all_winning_operand_digests_match_full_scan":true,"source_fixtures":fixture,
        "native_negative_cases_rejected":negatives,"fresh_loaded_definition_regression":loaded,"fresh_operand_table_regression":operands,
        "full_operand_report_sha256":digest(&full_path)?,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "script_bundle_sha256":digest(&scripts)?,"quest_bundle_sha256":digest(&quests)?,"executable_sha256":rust["executable_sha256"],
        "comparison_scope":"original winning quest/header identities, authored SCRI attachments, versioned script handles and first-match foreign declarations; every compiled operand association independently repeated",
        "static_declarations_only":true,"live_event_lists_loaded":false,"live_values_resolved":false,"execution_ready":false,"retail_parity_accepted":false,
        "known_gaps":["authored quest SCRI is a static relation; the current live event-list script remains unmeasured","placed references require live ExtraScript/event-list state; no base-script substitution",
        "dynamic SCRV contexts, player bindings and runtime local values remain unavailable","original effective load order and record-specific merge behavior remain unmeasured",
        "compressed extraction remains Rust-owned; prior complete decoded table coverage is independently filtered","two original short quest headers and three stale empty-unit counts remain source findings",
        "no command effects, script scheduling, quest progression or retail acceptance"]}),
    )
}

fn negative_bundles(
    root: &Path,
    run: &Path,
    oracle: &Path,
    input: Input<'_>,
) -> Result<Vec<String>> {
    let valid = fs::read(input.quests)?;
    let mut cases = vec![
        ("short_magic", b"FRQUE".to_vec()),
        ("wrong_magic", b"BADQUEST".to_vec()),
        ("short_record", b"FRQUEST1\0QUST".to_vec()),
    ];
    for (name, at, replacement) in [
        ("invalid_source", 8, vec![254]),
        ("wrong_kind", 9, b"SCPT".to_vec()),
        ("null_form", 13, vec![0; 4]),
        ("different_offset", 21, u64::MAX.to_le_bytes().to_vec()),
        (
            "record_budget",
            29,
            (64_u32 * 1024 * 1024 + 1).to_le_bytes().to_vec(),
        ),
    ] {
        let mut bytes = valid.clone();
        bytes[at..at + replacement.len()].copy_from_slice(&replacement);
        cases.push((name, bytes));
    }
    let first = u32::from_le_bytes(valid[29..33].try_into()?) as usize;
    let mut duplicate = valid.clone();
    duplicate.extend(&valid[8..33 + first]);
    cases.push(("duplicate_quest", duplicate));
    cases.push(("truncated_body", valid[..32 + first].to_vec()));
    cases.push(("missing_quest_coverage", b"FRQUEST1".to_vec()));
    let mut rejected = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("quest-negative-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(input.data)
            .arg(input.order)
            .arg(input.scripts)
            .arg(path)
            .arg(input.executable);
        let output =
            run_logged_status(command, &run.join(format!("quest-negative-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid quest bundle emitted a report".into());
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
fn header() -> Vec<u8> {
    record(
        b"TES4",
        0,
        0,
        &field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    )
}
fn unit(compiled: Option<&[u8]>, references: &[(&[u8; 4], u32)], names: &[&[u8]]) -> Vec<u8> {
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&(references.len() as u32).to_le_bytes());
    schr[8..12].copy_from_slice(&(compiled.map_or(0, |b| b.len()) as u32).to_le_bytes());
    let mut bytes = field(b"SCHR", &schr);
    if let Some(compiled) = compiled {
        bytes.extend(field(b"SCDA", compiled));
    }
    for name in names {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&42_u32.to_le_bytes());
        bytes.extend(field(b"SLSD", &declaration));
        bytes.extend(field(b"SCVR", &[*name, &[0]].concat()));
    }
    for (kind, raw) in references {
        bytes.extend(field(kind, &raw.to_le_bytes()));
    }
    bytes
}
fn fixtures(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    executable: &Path,
) -> Result<Value> {
    let install = run.join("quest-fixture-install");
    let data = install.join("Data");
    fs::create_dir_all(&data)?;
    // The CLI verifies executable metadata even though this fixture contains no
    // native commands. Copy only into ignored local evidence, never publication.
    fs::copy(executable, install.join("FalloutNV.exe"))?;
    let mut plugin = header();
    for (raw, target) in [
        (0x100, 0x200_u32),
        (0x102, 0),
        (0x104, 0x201),
        (0x105, 0x202),
        (0x106, 0x203),
        (0x107, 0x204),
        (0x108, 0x205),
    ] {
        plugin.extend(record(
            b"QUST",
            raw,
            0,
            &field(b"SCRI", &target.to_le_bytes()),
        ));
    }
    plugin.extend(record(b"QUST", 0x101, 0, &[]));
    plugin.extend(record(
        b"QUST",
        0x103,
        0,
        &[
            field(b"SCRI", &0x200_u32.to_le_bytes()),
            field(b"SCRI", &0x200_u32.to_le_bytes()),
        ]
        .concat(),
    ));
    plugin.extend(record(b"QUST", 0x109, 0x20, &[]));
    plugin.extend(record(b"QUST", 0x110, 0, &field(b"DATA", &[5, 0])));
    plugin.extend(record(
        b"SCPT",
        0x200,
        0,
        &unit(None, &[], &[b"target_first", b"target_second"]),
    ));
    plugin.extend(record(b"SCPT", 0x202, 0x20, &[]));
    plugin.extend(record(b"ACTI", 0x203, 0, &[]));
    plugin.extend(record(b"SCPT", 0x204, 0, &[]));
    plugin.extend(record(
        b"SCPT",
        0x205,
        0,
        &[unit(None, &[], &[]), unit(None, &[], &[])].concat(),
    ));
    plugin.extend(record(b"REFR", 0x210, 0, &[]));
    let mut compiled = Vec::new();
    for (context, index) in [
        (1_u16, 42_u16),
        (1, 99),
        (2, 42),
        (3, 42),
        (4, 42),
        (5, 42),
        (6, 42),
        (7, 42),
        (8, 42),
        (9, 42),
    ] {
        let operands = [
            b"r".as_slice(),
            &context.to_le_bytes(),
            b"f",
            &index.to_le_bytes(),
            &1_u16.to_le_bytes(),
            b"0",
        ]
        .concat();
        compiled.extend(
            [
                &0x15_u16.to_le_bytes()[..],
                &(operands.len() as u16).to_le_bytes(),
                &operands,
            ]
            .concat(),
        );
    }
    plugin.extend(record(
        b"SCPT",
        0x300,
        0,
        &unit(
            Some(&compiled),
            &[
                (b"SCRO", 0x100),
                (b"SCRO", 0x210),
                (b"SCRO", 0x203),
                (b"SCRO", 0),
                (b"SCRO", 0x14),
                (b"SCRO", 0x999),
                (b"SCRO", 0x202),
                (b"SCRV", 42),
                (b"SCRO", 0x101),
            ],
            &[b"current_wrong"],
        ),
    ));
    write_new(&data.join("FalloutNV.esm"), &plugin)?;
    let names = vec!["FalloutNV.esm".to_string()];
    let order = run.join("quest-fixture-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("quest-fixture-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let scripts = run.join("quest-fixture-scripts.bin");
    let quests = run.join("quest-fixture-quests.bin");
    let report = run.join("quest-fixture-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("quest-scripts")
        .arg("--install")
        .arg(&install)
        .arg("--load-order")
        .arg(order)
        .arg("--script-comparison-bundle")
        .arg(&scripts)
        .arg("--quest-comparison-bundle")
        .arg(&quests)
        .arg("--output")
        .arg(&report);
    run_logged_status(command, &run.join("quest-fixture-rust.log"), 1)?;
    let rust = json_file(&report)?;
    let (native_report, native_path) = native(
        root,
        run,
        oracle,
        Input {
            data: &data,
            order: &binary,
            scripts: &scripts,
            quests: &quests,
            executable,
        },
        "quest-fixture-native",
    )?;
    compare(&rust, &native_report)?;
    let statuses = rust["operand_counts"]["declaration_statuses"]
        .as_object()
        .ok_or("Missing foreign fixture statuses")?;
    let states: BTreeSet<_> = statuses.keys().map(String::as_str).collect();
    if states
        != BTreeSet::from([
            "static_quest_declaration",
            "missing_foreign_declaration",
            "placed_reference_needs_event_list",
            "unsupported_context_kind",
            "null_context",
            "runtime_context",
            "missing_context_form",
            "deleted_context_form",
            "dynamic_context",
            "quest_script_unavailable",
        ])
        || rust["quest_counts"]["quests"] != 11
        || rust["operand_counts"]["foreign_uses"] != 10
        || rust["quest_counts"]["statuses"]
            .as_object()
            .ok_or("Missing attachment statuses")?
            .len()
            != 10
    {
        return Err("Static quest/foreign source fixture coverage differs".into());
    }
    Ok(
        json!({"quests_compared":11,"attachment_states_compared":10,"foreign_uses_compared":10,"declaration_states_compared":10,"all_fields_equal":true,
        "current_local_never_substituted":true,"live_values_resolved":false,"rust_report_sha256":digest(&report)?,"native_report_sha256":digest(&native_path)?}),
    )
}
