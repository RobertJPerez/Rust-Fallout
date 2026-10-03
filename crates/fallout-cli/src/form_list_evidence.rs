//! Compare direct source reads, cache reuse and authored adversarial form lists.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn bundle(names: &[&str]) -> Vec<u8> {
    let mut bytes = b"FRORDER1".to_vec();
    bytes.extend((names.len() as u16).to_le_bytes());
    for name in names {
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    bytes
}
struct Inputs<'a> {
    root: &'a Path,
    run: &'a Path,
    cli: &'a Path,
    oracle: &'a Path,
    install: &'a Path,
    order: &'a Path,
    binary: &'a Path,
}
fn phase(
    i: &Inputs<'_>,
    name: &str,
    cache: Option<&Path>,
    root: Option<u32>,
) -> Result<(Value, Value)> {
    let rust_path = i.run.join(format!("{name}-rust.json"));
    let mut command = Command::new(i.cli);
    command
        .current_dir(i.root)
        .arg("form-lists")
        .arg("--install")
        .arg(i.install)
        .arg("--load-order")
        .arg(i.order)
        .arg("--output")
        .arg(&rust_path);
    if let Some(cache) = cache {
        command.arg("--index-cache").arg(cache);
    }
    if let Some(root) = root {
        command
            .arg("--root-plugin")
            .arg("Base.esm")
            .arg("--root-id")
            .arg(root.to_string());
    }
    run_logged(command, &i.run.join(format!("{name}-rust.log")))?;
    let native_path = i.run.join(format!("{name}-native.json"));
    let mut command = Command::new(i.oracle);
    command
        .current_dir(i.root)
        .arg(i.install.join("Data"))
        .arg(i.binary);
    let output = run_logged(command, &i.run.join(format!("{name}-native.log")))?;
    write_new(&native_path, &output.stdout)?;
    let rust = json_file(&rust_path)?;
    let native = json_file(&native_path)?;
    for (key, value) in native
        .as_object()
        .ok_or("Missing native form-list object")?
    {
        if rust[key] != *value {
            return Err(format!("Independent form-list comparison differs: {key}").into());
        }
    }
    if rust["live_list_state_initialized"] != false
        || rust["get_item_count_list_expansion_implemented"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Form list source inspection crossed its scope".into());
    }
    let receipt = json!({"name":name,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"counts":rust["counts"],"graph_counts":rust["dependency_graph"]["counts"],"closure":rust["closure"],
        "cache_reused":rust["index_cache"]["plugins"].as_array().map(|p|p.iter().map(|p|p["reused"].clone()).collect::<Vec<_>>())});
    Ok((rust, receipt))
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
    let binary = run.join("form-list-order.bin");
    write_new(
        &binary,
        &bundle(&names.iter().map(String::as_str).collect::<Vec<_>>()),
    )?;
    let i = Inputs {
        root,
        run,
        cli,
        oracle,
        install,
        order: &order,
        binary: &binary,
    };
    let (report, original) = phase(&i, "form-lists", None, None)?;
    let fixtures = fixtures(root, run, cli, oracle)?;
    let malformed = malformed(root, run, cli, oracle)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Independently compared original FLST field/member bindings and structural graph; no live initialization, list mutation, flattening or count expansion",
        "oracle_binary_sha256":digest(oracle)?,"sources":report["sources"],"metadata":report["metadata"],"counts":report["counts"],"graph_counts":report["dependency_graph"]["counts"],
        "rust_report_sha256":original["rust_report_sha256"],"native_report_sha256":original["native_report_sha256"],"all_original_fields_bindings_and_graph_equal":true,"synthetic_fixtures":fixtures,"malformed_cases_rejected":malformed,
        "get_item_count_list_expansion_implemented":false,"live_list_state_initialized":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original form-list count expansion, duplicate/null/nested-list behavior and argument/return coercion","Runtime script-added/removed members and persistence","Effective retail profile and measured original execution traces"]}),
    )
}
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
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
    let mut bytes = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for name in masters {
        bytes.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        bytes.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &bytes)
}
fn member(id: u32) -> Vec<u8> {
    field(b"LNAM", &id.to_le_bytes())
}
fn fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Vec<Value>> {
    let install = run.join("form-list-fixture");
    let data = install.join("Data");
    fs::create_dir_all(&data)?;
    write_new(
        &data.join("Base.esm"),
        &[
            header(&[]),
            record(b"MISC", 0x200, 0, &[]),
            record(
                b"FLST",
                0x100,
                0,
                &[
                    member(0x101),
                    member(0x101),
                    member(0x200),
                    member(0),
                    member(0x999),
                    member(0x103),
                    field(b"ZZZZ", &[255, 0, 1]),
                ]
                .concat(),
            ),
            record(b"FLST", 0x101, 0, &member(0x100)),
            record(
                b"FLST",
                0x102,
                0,
                &[field(b"XXXX", &4_u32.to_le_bytes()), member(0x102)].concat(),
            ),
            record(b"FLST", 0x103, 0, &member(0x200)),
            record(b"FLST", 0x104, 0, &[]),
        ]
        .concat(),
    )?;
    write_new(
        &data.join("Other.esm"),
        &[
            header(&[]),
            record(b"WEAP", 0x200, 0, &[]),
            record(b"FLST", 0x100, 0, &member(0x200)),
        ]
        .concat(),
    )?;
    let patch = |target| {
        [
            header(&["Base.esm", "Other.esm"]),
            record(b"FLST", 0x103, 0x20, b"bad tombstone body"),
            record(b"MISC", 0x01000100, 0, &[]),
            record(b"FLST", 0x02000100, 0, &member(target)),
        ]
        .concat()
    };
    write_new(&data.join("Patch.esp"), &patch(0x01000200))?;
    let order = run.join("form-list-fixture-order.json");
    write_new(&order, b"[\"Base.esm\",\"Other.esm\",\"Patch.esp\"]")?;
    let binary = run.join("form-list-fixture-order.bin");
    write_new(&binary, &bundle(&["Base.esm", "Other.esm", "Patch.esp"]))?;
    let i = Inputs {
        root,
        run,
        cli,
        oracle,
        install: &install,
        order: &order,
        binary: &binary,
    };
    let cache = run.join("form-list-fixture-cache");
    fs::create_dir(&cache)?;
    let (cold, a) = phase(&i, "form-list-cold", Some(&cache), Some(0x100))?;
    let (warm, b) = phase(&i, "form-list-warm", Some(&cache), Some(0x100))?;
    for key in [
        "counts",
        "definitions",
        "dependency_graph",
        "closure",
        "sources",
        "metadata",
    ] {
        if cold[key] != warm[key] {
            return Err(format!("Warm form list field differs: {key}").into());
        }
    }
    if a["cache_reused"] != json!([false, false, false])
        || b["cache_reused"] != json!([true, true, true])
    {
        return Err("Form list cold/warm reuse differs".into());
    }
    if cold["dependency_graph"]["counts"]["cyclic_components"] != 2
        || cold["dependency_graph"]["counts"]["unresolved_edges"] != 3
        || cold["counts"]["deleted_records"] != 1
        || cold["closure"]["edge_indices"] != json!([0, 1, 2, 3, 4, 5, 6])
    {
        return Err("Form list authored cycle/closure expectations differ".into());
    }
    fs::write(data.join("Patch.esp"), patch(0x200))?;
    let (_, c) = phase(&i, "form-list-changed-source", Some(&cache), Some(0x100))?;
    if c["cache_reused"] != json!([true, true, false]) {
        return Err("Form list source invalidation differs".into());
    }
    Ok(vec![a, b, c])
}
fn malformed(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Vec<Value>> {
    let install = run.join("form-list-negative");
    let data = install.join("Data");
    fs::create_dir_all(&data)?;
    let order = run.join("form-list-negative-order.json");
    write_new(&order, b"[\"Base.esm\"]")?;
    let binary = run.join("form-list-negative-order.bin");
    write_new(&binary, &bundle(&["Base.esm"]))?;
    let cases = [
        ("empty-member", field(b"LNAM", &[])),
        ("short-member", field(b"LNAM", &[0; 3])),
        ("long-member", field(b"LNAM", &[0; 5])),
        ("packed-members", field(b"LNAM", &[0; 8])),
        ("partial-header", b"LNAM".to_vec()),
        (
            "truncated-member",
            [b"LNAM".as_slice(), &4_u16.to_le_bytes(), &[0; 3]].concat(),
        ),
        ("orphan-extended", field(b"XXXX", &4_u32.to_le_bytes())),
        (
            "repeated-extended",
            [
                field(b"XXXX", &4_u32.to_le_bytes()),
                field(b"XXXX", &4_u32.to_le_bytes()),
                member(0),
            ]
            .concat(),
        ),
        ("short-extended-word", field(b"XXXX", &[0; 3])),
        (
            "oversized-extended",
            [field(b"XXXX", &0xffff_ffff_u32.to_le_bytes()), member(0)].concat(),
        ),
    ];
    let mut results = Vec::new();
    for (name, body) in cases {
        fs::write(
            data.join("Base.esm"),
            [header(&[]), record(b"FLST", 0x100, 0, &body)].concat(),
        )?;
        let output = run.join(format!("bad-form-list-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("form-lists")
            .arg("--install")
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--output")
            .arg(&output);
        let rust_log = run.join(format!("bad-form-list-{name}-rust.log"));
        run_logged_status(command, &rust_log, 1)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(&data).arg(&binary);
        let native_log = run.join(format!("bad-form-list-{name}-native.log"));
        run_logged_status(command, &native_log, 1)?;
        if output.exists() {
            return Err("Malformed list published a successful report".into());
        }
        results.push(json!({"name":name,"rust_exit_code":1,"native_exit_code":1,"rust_log_sha256":digest(&rust_log)?,"native_log_sha256":digest(&native_log)?}));
    }
    Ok(results)
}
