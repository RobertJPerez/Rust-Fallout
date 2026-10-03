//! Direct leveled source/graph comparison and a fresh base-inventory regression.
use super::{
    Result, digest, inventory_evidence, json_file, run_logged, run_logged_status, write_new,
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
pub(super) struct Oracles<'a> {
    pub lists: &'a Path,
    pub inventory: inventory_evidence::Oracles<'a>,
}
fn bundle(names: &[String]) -> Vec<u8> {
    let mut b = b"FRORDER1".to_vec();
    b.extend((names.len() as u16).to_le_bytes());
    for n in names {
        b.extend((n.len() as u16).to_le_bytes());
        b.extend(n.as_bytes());
    }
    b
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
    expected: i32,
    root: Option<(&str, u32)>,
) -> Result<(Value, Value)> {
    let rust_path = i.run.join(format!("{name}-rust.json"));
    let mut command = Command::new(i.cli);
    command
        .current_dir(i.root)
        .arg("leveled-lists")
        .arg("--install")
        .arg(i.install)
        .arg("--load-order")
        .arg(i.order)
        .arg("--output")
        .arg(&rust_path);
    if let Some(cache) = cache {
        command.arg("--index-cache").arg(cache);
    }
    if let Some((name, id)) = root {
        command
            .arg("--root-plugin")
            .arg(name)
            .arg("--root-id")
            .arg(id.to_string());
    }
    run_logged_status(command, &i.run.join(format!("{name}-rust.log")), expected)?;
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
    for (key, value) in native.as_object().ok_or("Missing native leveled object")? {
        if rust[key] != *value {
            return Err(format!("Independent leveled comparison differs: {key}").into());
        }
    }
    if rust["random_draws"] != 0
        || rust["live_inventory_initialized"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Leveled inspection crossed its source scope".into());
    }
    let receipt = json!({"name":name,"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"counts":rust["counts"],"graph_counts":rust["dependency_graph"]["counts"],"closure":rust["closure"],
        "cache_reused":rust["index_cache"]["plugins"].as_array().map(|r|r.iter().map(|e|e["reused"].clone()).collect::<Vec<_>>()).unwrap_or_default()});
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
    let binary = run.join("leveled-order.bin");
    write_new(&binary, &bundle(&names))?;
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle: oracles.lists,
        install,
        order: &order,
        binary: &binary,
    };
    let (report, original) = phase(&inputs, "leveled-lists", None, 0, None)?;
    let fixtures = fixtures(root, run, cli, oracles.lists)?;
    let malformed = malformed(root, run, cli, oracles.lists)?;
    let regression = run.join("base-inventory-regression");
    fs::create_dir(&regression)?;
    let inventory = inventory_evidence::run(root, &regression, cli, oracles.inventory, install)?;
    if report["sources"] != inventory["sources"]
        || report["metadata"] != inventory["metadata"]
        || report["inventory_counts"] != inventory["counts"]
    {
        return Err("Leveled graph/base inventory cohorts differ".into());
    }
    let mismatches = report["dependency_graph"]["edges"]
        .as_array()
        .ok_or("Missing source edges")?
        .iter()
        .filter(|e| e["schema_kind_allowed"] == false)
        .cloned()
        .collect::<Vec<_>>();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","scope":"Independent authored leveled-list fields and structural inventory/template/list graph; no random draws, inheritance or gameplay acceptance",
        "oracle_binary_sha256":digest(oracles.lists)?,"sources":report["sources"],"metadata":report["metadata"],"counts":report["counts"],"graph_counts":report["dependency_graph"]["counts"],"schema_domain_mismatches":mismatches,
        "rust_report_sha256":original["rust_report_sha256"],"native_report_sha256":original["native_report_sha256"],"all_original_fields_and_graph_equal":true,"synthetic_fixtures":fixtures,"malformed_cases_rejected":malformed,
        "fresh_base_inventory_regression":inventory,"random_draws":0,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original level selection, chance/global behavior, count coercion and random draw order","Template inheritance, script-added entries, respawn and live item persistence","Full content/asset dependency closure beyond this structural domain","Retail differential traces and effective runtime load order"]}),
    )
}
fn field(k: &[u8; 4], b: &[u8]) -> Vec<u8> {
    [k.as_slice(), &(b.len() as u16).to_le_bytes(), b].concat()
}
fn record(k: &[u8; 4], id: u32, f: u32, b: &[u8]) -> Vec<u8> {
    [
        k.as_slice(),
        &(b.len() as u32).to_le_bytes(),
        &f.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        b,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut b = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for m in masters {
        b.extend(field(b"MAST", &[m.as_bytes(), &[0]].concat()));
        b.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &b)
}
fn entry(id: u32, level: u16, count: Option<u16>, pad: bool) -> Vec<u8> {
    let mut b = [
        level.to_le_bytes().as_slice(),
        &0xabcd_u16.to_le_bytes(),
        &id.to_le_bytes(),
    ]
    .concat();
    if let Some(c) = count {
        b.extend(c.to_le_bytes());
        if pad {
            b.extend(0x9876_u16.to_le_bytes());
        }
    }
    field(b"LVLO", &b)
}
fn extra(owner: u32, word: u32) -> Vec<u8> {
    field(
        b"COED",
        &[
            owner.to_le_bytes(),
            word.to_le_bytes(),
            0x7fc0_1234_u32.to_le_bytes(),
        ]
        .concat(),
    )
}
fn fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Value> {
    let install = run.join("leveled-fixture-install");
    fs::create_dir_all(install.join("Data"))?;
    let base = [
        header(&[]),
        record(b"MISC", 0x104, 0, &[]),
        record(b"NPC_", 0x101, 0, &[]),
        record(b"FACT", 0x102, 0, &[]),
        record(b"GLOB", 0x103, 0, &[]),
        record(
            b"CONT",
            0x200,
            0,
            &field(
                b"CNTO",
                &[0x300_u32.to_le_bytes(), 2_i32.to_le_bytes()].concat(),
            ),
        ),
        record(
            b"LVLI",
            0x300,
            0,
            &[
                entry(0x301, 0xffff, None, false),
                entry(0x301, 1, Some(0), false),
                entry(0x104, 1, Some(0xffff), true),
                extra(0x101, 0x103),
                entry(0x777, 1, Some(1), true),
            ]
            .concat(),
        ),
        record(
            b"LVLI",
            0x301,
            0,
            &[
                field(b"LVLF", &[255]),
                field(b"LVLD", &[255]),
                field(b"LVLG", &0x103_u32.to_le_bytes()),
                entry(0x300, 1, Some(1), true),
                extra(0x102, u32::MAX),
            ]
            .concat(),
        ),
        record(b"LVLI", 0x302, 0, &entry(0x302, 1, Some(1), true)),
        record(b"LVLI", 0x303, 0, &entry(0x104, 1, Some(1), true)),
        record(
            b"LVLC",
            0x400,
            0,
            &[
                field(b"LVLF", &[255]),
                field(b"LVLG", &[1, 2]),
                entry(0x104, 0x8000, Some(1), true),
            ]
            .concat(),
        ),
        record(
            b"LVLN",
            0x401,
            0,
            &[
                field(b"XXXX", &8_u32.to_le_bytes()),
                entry(0x101, 1, None, false),
            ]
            .concat(),
        ),
    ]
    .concat();
    write_new(&install.join("Data/FalloutNV.esm"), &base)?;
    write_new(
        &install.join("Data/Other.esm"),
        &[header(&[]), record(b"MISC", 0x104, 0, &[])].concat(),
    )?;
    let addon = [
        header(&["FalloutNV.esm", "Other.esm"]),
        record(
            b"LVLI",
            0x303,
            fallout_data::plugin::DELETED,
            b"malformed tombstone",
        ),
        record(
            b"LVLI",
            0x0200_0300,
            0,
            &[
                entry(0x0100_0104, 1, Some(1), true),
                entry(0x303, 1, None, false),
                extra(0x303, 0xdead_beef),
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
    let order = run.join("leveled-fixture-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("leveled-fixture-order.bin");
    write_new(&binary, &bundle(&names))?;
    let cache = run.join("leveled-fixture-cache");
    fs::create_dir(&cache)?;
    let inputs = Inputs {
        root,
        run,
        cli,
        oracle,
        install: &install,
        order: &order,
        binary: &binary,
    };
    let selected = Some(("FalloutNV.esm", 0x200));
    let (cold, cold_receipt) = phase(&inputs, "leveled-fixture-cold", Some(&cache), 0, selected)?;
    let (warm, warm_receipt) = phase(&inputs, "leveled-fixture-warm", Some(&cache), 0, selected)?;
    if cold_receipt["cache_reused"] != json!([false, false, false])
        || warm_receipt["cache_reused"] != json!([true, true, true])
    {
        return Err("Leveled cache state differs".into());
    }
    if cold["dependency_graph"]["counts"]["cyclic_components"] != 2
        || cold["closure"]["nodes"].as_array().map(Vec::len) != Some(3)
        || cold["closure"]["edge_indices"].as_array().map(Vec::len) != Some(6)
    {
        return Err("Authored cyclic/closure coverage differs".into());
    }
    let swapped = vec![names[1].clone(), names[0].clone(), names[2].clone()];
    let order2 = run.join("leveled-fixture-reordered.json");
    write_new(&order2, &serde_json::to_vec(&swapped)?)?;
    let binary2 = run.join("leveled-fixture-reordered.bin");
    write_new(&binary2, &bundle(&swapped))?;
    let reordered = Inputs {
        order: &order2,
        binary: &binary2,
        ..inputs
    };
    let (reordered, reordered_receipt) =
        phase(&reordered, "leveled-fixture-reordered", None, 0, selected)?;
    for k in ["definitions", "counts", "dependency_graph", "closure"] {
        if cold[k] != warm[k] || cold[k] != reordered[k] {
            return Err(format!("Canonical leveled data changed: {k}").into());
        }
    }
    let ambiguous = [
        extra(0, 0),
        entry(0x104, 1, None, false),
        extra(0, 0),
        extra(0, 1),
        field(b"LVLD", &[0]),
        field(b"LVLD", &[255]),
        field(b"LVLF", &[0]),
        field(b"LVLF", &[255]),
    ]
    .concat();
    fs::write(
        install.join("Data/Addon.esm"),
        [
            header(&["FalloutNV.esm", "Other.esm"]),
            record(b"LVLI", 0x0200_0300, 0, &ambiguous),
        ]
        .concat(),
    )?;
    let (_, ambiguity) = phase(&inputs, "leveled-fixture-ambiguity", None, 1, None)?;
    if ambiguity["counts"]["source_findings"] != 4 {
        return Err("Leveled findings lost".into());
    }
    Ok(
        json!({"scope":"Authored short/full entries, exact bits/padding, all list kinds, owner unions, namespaces, deleted winning lists, duplicate edges, SCCs and bounded closure","phases":[cold_receipt,warm_receipt,reordered_receipt,ambiguity],"cache_and_reordering_keep_definitions_graph_and_closure_equal":true}),
    )
}
fn malformed(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Vec<String>> {
    let install = run.join("bad-leveled-install");
    fs::create_dir_all(install.join("Data"))?;
    let names = vec!["FalloutNV.esm".into()];
    let order = run.join("bad-leveled-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("bad-leveled-order.bin");
    write_new(&binary, &bundle(&names))?;
    let mut cases = vec![
        ("short-entry", field(b"LVLO", &[0; 7]), 0),
        ("partial-count", field(b"LVLO", &[0; 9]), 0),
        ("partial-padding", field(b"LVLO", &[0; 11]), 0),
        ("long-entry", field(b"LVLO", &[0; 13]), 0),
        ("short-extra", field(b"COED", &[0; 11]), 0),
        ("long-extra", field(b"COED", &[0; 13]), 0),
        ("short-chance", field(b"LVLD", &[]), 0),
        ("long-flags", field(b"LVLF", &[0; 2]), 0),
        ("short-global", field(b"LVLG", &[0; 3]), 0),
        ("partial-field-header", b"LVLO".to_vec(), 0),
        ("orphan-extended", field(b"XXXX", &8_u32.to_le_bytes()), 0),
        ("short-extended", field(b"XXXX", &[0; 3]), 0),
        (
            "repeated-extended",
            [
                field(b"XXXX", &8_u32.to_le_bytes()),
                field(b"XXXX", &8_u32.to_le_bytes()),
                entry(0, 0, None, false),
            ]
            .concat(),
            0,
        ),
    ];
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let payload = entry(0, 0, None, false);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&payload)?;
    let mut encoded = encoder.finish()?;
    *encoded.last_mut().ok_or("Leveled checksum fixture")? ^= 1;
    cases.push((
        "compressed-checksum",
        [&(payload.len() as u32).to_le_bytes()[..], &encoded].concat(),
        fallout_data::plugin::COMPRESSED,
    ));
    let mut rejected = Vec::new();
    for (name, body, flags) in cases {
        fs::write(
            install.join("Data/FalloutNV.esm"),
            [header(&[]), record(b"LVLI", 0x300, flags, &body)].concat(),
        )?;
        let report = run.join(format!("bad-leveled-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("leveled-lists")
            .arg("--install")
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--output")
            .arg(&report);
        run_logged_status(
            command,
            &run.join(format!("bad-leveled-{name}-rust.log")),
            1,
        )?;
        if report.exists() {
            return Err("Malformed list published Rust report".into());
        }
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(install.join("Data"))
            .arg(&binary);
        let output = run_logged_status(
            command,
            &run.join(format!("bad-leveled-{name}-native.log")),
            1,
        )?;
        if !output.stdout.is_empty() {
            return Err("Malformed list published native report".into());
        }
        rejected.push(name.into());
    }
    Ok(rejected)
}
