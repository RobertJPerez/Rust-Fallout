//! Compare authored inventory facts directly, before adding mutable item state.
use super::{
    Result, digest, foreign_context_evidence, json_file, run_logged, run_logged_status, write_new,
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

pub(super) struct Oracles<'a> {
    pub inventory: &'a Path,
    pub runtime: foreign_context_evidence::Oracles<'a>,
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
struct Inputs<'a> {
    root: &'a Path,
    run: &'a Path,
    cli: &'a Path,
    oracle: &'a Path,
    install: &'a Path,
    order: &'a Path,
    binary_order: &'a Path,
}
fn phase(
    inputs: &Inputs<'_>,
    name: &str,
    cache: Option<&Path>,
    expected: i32,
) -> Result<(Value, Value)> {
    let rust_path = inputs.run.join(format!("{name}-rust.json"));
    let mut command = Command::new(inputs.cli);
    command
        .current_dir(inputs.root)
        .arg("base-inventory")
        .arg("--install")
        .arg(inputs.install)
        .arg("--load-order")
        .arg(inputs.order)
        .arg("--output")
        .arg(&rust_path);
    if let Some(cache) = cache {
        command.arg("--index-cache").arg(cache);
    }
    run_logged_status(
        command,
        &inputs.run.join(format!("{name}-rust.log")),
        expected,
    )?;
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
    for (key, value) in native
        .as_object()
        .ok_or("Missing native inventory object")?
    {
        if rust[key] != *value {
            return Err(format!("Independent inventory differs: {key}").into());
        }
    }
    if rust["live_inventory_initialized"] != false
        || rust["retail_parity_accepted"] != false
        || rust["accepted_scenarios"] != json!([])
    {
        return Err("Inventory proof crossed its source scope".into());
    }
    let receipt = json!({"name":name,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "counts":rust["counts"],"cache_reused":rust["index_cache"]["plugins"].as_array().map(|rows| rows.iter().map(|row| row["reused"].clone()).collect::<Vec<_>>()).unwrap_or_default()});
    Ok((rust, receipt))
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracles: Oracles<'_>,
    install: &Path,
) -> Result<Value> {
    let order = root.join("profiles/nv-inspection-order.json");
    let names: Vec<String> = serde_json::from_slice(&fs::read(&order)?)?;
    let binary = run.join("inventory-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle: oracles.inventory,
        install,
        order: &order,
        binary_order: &binary,
    };
    let (report, original) = phase(&inputs, "base-inventory", None, 0)?;
    let fixtures = fixtures(root, run, cli, oracles.inventory)?;
    let malformed = malformed(root, run, cli, oracles.inventory)?;
    let regression = run.join("foreign-runtime-regression");
    fs::create_dir(&regression)?;
    let runtime = foreign_context_evidence::run(root, &regression, cli, oracles.runtime, install)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Independent original authored base inventory fields and source bindings; no live inventory or retail gameplay acceptance",
        "oracle_binary_sha256":digest(oracles.inventory)?,"sources":report["sources"],"metadata":report["metadata"],"counts":report["counts"],
        "rust_report_sha256":original["rust_report_sha256"],"native_report_sha256":original["native_report_sha256"],
        "all_original_inventory_fields_equal":true,"synthetic_fixtures":fixtures,"malformed_cases_rejected":malformed,
        "fresh_foreign_runtime_regression":runtime,"live_inventory_initialized":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original template inheritance and leveled-list expansion","Live count deltas, item instances, equipment and persistence","Original ownership, condition coercion, respawn and primitive query behavior","Retail differential traces and effective runtime load order"]}),
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
fn item(id: u32, count: i32) -> Vec<u8> {
    field(b"CNTO", &[id.to_le_bytes(), count.to_le_bytes()].concat())
}
fn extra(owner: u32, word: u32, condition: u32) -> Vec<u8> {
    field(
        b"COED",
        &[
            owner.to_le_bytes(),
            word.to_le_bytes(),
            condition.to_le_bytes(),
        ]
        .concat(),
    )
}
fn fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Value> {
    let install = run.join("inventory-fixture-install");
    fs::create_dir_all(install.join("Data"))?;
    let mut actor = [0; 24];
    actor[..4].copy_from_slice(&u32::MAX.to_le_bytes());
    actor[22..].copy_from_slice(&0x8100_u16.to_le_bytes());
    let base = [
        header(&[]),
        record(b"NPC_", 0x101, 0, &field(b"ACBS", &actor)),
        record(b"FACT", 0x102, 0, &[]),
        record(b"GLOB", 0x103, 0, &[]),
        record(b"MISC", 0x104, 0, &[]),
        record(b"CONT", 0x400, 0, &item(0x104, 17)),
    ]
    .concat();
    write_new(&install.join("Data/FalloutNV.esm"), &base)?;
    write_new(
        &install.join("Data/Other.esm"),
        &[header(&[]), record(b"MISC", 0x104, 0, &[])].concat(),
    )?;
    let mut body = field(
        b"DATA",
        &[&[0xff][..], &0x7fc0_1234_u32.to_le_bytes()].concat(),
    );
    body.extend(item(0x104, i32::MIN));
    body.extend(extra(0x101, 0x103, 0x7fc0_1234));
    body.extend(item(0x104, 0));
    body.extend(extra(0x102, u32::MAX, 0x8000_0000));
    body.extend(item(0x0100_0104, i32::MAX));
    body.extend(extra(0, 0xdead_beef, 0xffff_ffff));
    body.extend(item(0x777, -1));
    body.extend(extra(0x777, 0xfeed_face, 0));
    body.extend(item(0x101, 1));
    body.extend(extra(0x104, 0x1234_5678, 0x3f80_0000));
    body.extend(field(b"ZZZZ", b"unknown bytes stay intact"));
    let addon = [
        header(&["FalloutNV.esm", "Other.esm"]),
        record(b"CONT", 0x0200_0300, 0, &body),
        record(
            b"CONT",
            0x400,
            fallout_data::plugin::DELETED,
            b"malformed tombstone body",
        ),
        record(
            b"NPC_",
            0x0200_0301,
            0,
            &[
                field(b"ACBS", &actor),
                field(b"TPLT", &0x101_u32.to_le_bytes()),
                item(0x104, 2),
            ]
            .concat(),
        ),
    ]
    .concat();
    write_new(&install.join("Data/Addon.esm"), &addon)?;
    let names = vec![
        "FalloutNV.esm".into(),
        "Other.esm".into(),
        "Addon.esm".into(),
    ];
    let order = run.join("inventory-fixture-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("inventory-fixture-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let cache = run.join("inventory-fixture-cache");
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
    let (cold, cold_receipt) = phase(&inputs, "inventory-fixture-cold", Some(&cache), 0)?;
    let (warm, warm_receipt) = phase(&inputs, "inventory-fixture-warm", Some(&cache), 0)?;
    if cold_receipt["cache_reused"] != json!([false, false, false])
        || warm_receipt["cache_reused"] != json!([true, true, true])
    {
        return Err("Inventory cache state differs".into());
    }
    let swapped = vec![names[1].clone(), names[0].clone(), names[2].clone()];
    let reordered = run.join("inventory-fixture-reordered.json");
    write_new(&reordered, &serde_json::to_vec(&swapped)?)?;
    let reordered_binary = run.join("inventory-fixture-reordered.bin");
    write_new(&reordered_binary, &order_bundle(&swapped))?;
    let reordered_inputs = Inputs {
        order: &reordered,
        binary_order: &reordered_binary,
        ..inputs
    };
    let (reordered, reordered_receipt) =
        phase(&reordered_inputs, "inventory-fixture-reordered", None, 0)?;
    for key in ["definitions", "counts"] {
        if cold[key] != warm[key] || cold[key] != reordered[key] {
            return Err(format!("Canonical inventory changed: {key}").into());
        }
    }
    // Ambiguity is a retained source finding. It must not silently choose an owner.
    let ambiguous = [
        extra(0, 0, 0),
        item(0x104, 1),
        extra(0, 0, 0),
        extra(0, 1, 0),
        field(b"ZZZZ", &[]),
        extra(0, 2, 0),
    ]
    .concat();
    fs::write(
        install.join("Data/Addon.esm"),
        [
            header(&["FalloutNV.esm", "Other.esm"]),
            record(b"CONT", 0x0200_0300, 0, &ambiguous),
        ]
        .concat(),
    )?;
    let (_, ambiguity) = phase(&inputs, "inventory-fixture-ambiguity", None, 1)?;
    if ambiguity["counts"]["source_findings"] != 3 {
        return Err("Inventory association findings were lost".into());
    }
    Ok(
        json!({"scope":"Authored namespaces, winning tombstones, duplicate/signed counts, owner-dependent unions, NaN bits, unknown flags/fields and ambiguous extra associations",
        "phases":[cold_receipt,warm_receipt,reordered_receipt,ambiguity],"cache_and_reordering_keep_canonical_definitions_equal":true}),
    )
}
fn malformed(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Vec<String>> {
    let install = run.join("bad-inventory-install");
    fs::create_dir_all(install.join("Data"))?;
    let names = vec!["FalloutNV.esm".into()];
    let order = run.join("bad-inventory-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("bad-inventory-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let mut cases = vec![
        ("short-item", *b"CONT", field(b"CNTO", &[0; 7]), 0),
        ("long-item", *b"CONT", field(b"CNTO", &[0; 9]), 0),
        ("short-extra", *b"CONT", field(b"COED", &[0; 11]), 0),
        ("long-extra", *b"CONT", field(b"COED", &[0; 13]), 0),
        ("short-actor-base", *b"NPC_", field(b"ACBS", &[0; 23]), 0),
        ("long-actor-base", *b"CREA", field(b"ACBS", &[0; 25]), 0),
        ("short-template", *b"NPC_", field(b"TPLT", &[0; 3]), 0),
        ("short-container-data", *b"CONT", field(b"DATA", &[0; 4]), 0),
        ("partial-field-header", *b"CONT", b"CNTO".to_vec(), 0),
        (
            "orphan-extended",
            *b"CONT",
            field(b"XXXX", &8_u32.to_le_bytes()),
            0,
        ),
        ("short-extended", *b"CONT", field(b"XXXX", &[0; 3]), 0),
        (
            "repeated-extended",
            *b"CONT",
            [
                field(b"XXXX", &8_u32.to_le_bytes()),
                field(b"XXXX", &8_u32.to_le_bytes()),
                item(0, 0),
            ]
            .concat(),
            0,
        ),
        (
            "field-overrun",
            *b"CONT",
            [b"CNTO".as_slice(), &8_u16.to_le_bytes(), &[0; 7]].concat(),
            0,
        ),
    ];
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let payload = item(0, 1);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&payload)?;
    let mut encoded = encoder.finish()?;
    *encoded.last_mut().ok_or("Inventory checksum fixture")? ^= 1;
    cases.push((
        "compressed-checksum",
        *b"CONT",
        [&(payload.len() as u32).to_le_bytes()[..], &encoded].concat(),
        fallout_data::plugin::COMPRESSED,
    ));
    let mut rejected = Vec::new();
    for (name, kind, body, flags) in cases {
        fs::write(
            install.join("Data/FalloutNV.esm"),
            [header(&[]), record(&kind, 0x300, flags, &body)].concat(),
        )?;
        let report = run.join(format!("bad-inventory-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("base-inventory")
            .arg("--install")
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--output")
            .arg(&report);
        run_logged_status(
            command,
            &run.join(format!("bad-inventory-{name}-rust.log")),
            1,
        )?;
        if report.exists() {
            return Err("Malformed inventory published a Rust report".into());
        }
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(install.join("Data"))
            .arg(&binary);
        let output = run_logged_status(
            command,
            &run.join(format!("bad-inventory-{name}-native.log")),
            1,
        )?;
        if !output.stdout.is_empty() {
            return Err("Malformed inventory published a native report".into());
        }
        rejected.push(name.into());
    }
    Ok(rejected)
}
