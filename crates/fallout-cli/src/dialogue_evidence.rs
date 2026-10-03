//! Direct source-header comparison plus cached/uncached dialogue membership.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn order_bundle(names: &[String]) -> Result<Vec<u8>> {
    if names.is_empty() || names.len() > 254 {
        return Err("Oracle order requires 1..=254 names".into());
    }
    let mut bytes = b"FRORDER1".to_vec();
    bytes.extend((names.len() as u16).to_le_bytes());
    for name in names {
        fallout_data::identity::plugin_name(name)?;
        let length = u16::try_from(name.len())?;
        bytes.extend(length.to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    Ok(bytes)
}
struct Inspection<'a> {
    install: &'a Path,
    order: &'a Path,
    cache: Option<&'a Path>,
}
fn membership(
    root: &Path,
    directory: &Path,
    cli: &Path,
    input: Inspection<'_>,
    name: &str,
    status: i32,
) -> Result<(Value, std::path::PathBuf)> {
    let Inspection {
        install,
        order,
        cache,
    } = input;
    let path = directory.join(format!("{name}.json"));
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("dialogue-membership")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(order)
        .arg("--output")
        .arg(&path);
    if let Some(cache) = cache {
        command.arg("--index-cache").arg(cache);
    }
    run_logged_status(command, &directory.join(format!("{name}.log")), status)?;
    Ok((json_file(&path)?, path))
}
fn compare_native(
    root: &Path,
    directory: &Path,
    oracle: &Path,
    data: &Path,
    order: &Path,
    name: &str,
) -> Result<(Value, std::path::PathBuf)> {
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(data).arg(order);
    let output = run_logged(command, &directory.join(format!("{name}.log")))?;
    let path = directory.join(format!("{name}.json"));
    write_new(&path, &output.stdout)?;
    Ok((json_file(&path)?, path))
}

pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let order_path = root.join("profiles/nv-inspection-order.json");
    let names: Vec<String> = serde_json::from_value(json_file(&order_path)?)?;
    let binary_order = run.join("order.bin");
    write_new(&binary_order, &order_bundle(&names)?)?;
    let cache = run.join("index-cache");
    fs::create_dir(&cache)?;
    let mut reports = Vec::new();
    let mut hashes = json!({});
    for phase in ["uncached", "cold", "warm"] {
        let (report, path) = membership(
            root,
            run,
            cli,
            Inspection {
                install,
                order: &order_path,
                cache: (phase != "uncached").then_some(cache.as_path()),
            },
            phase,
            0,
        )?;
        if report["runtime_ready"] != false
            || report["retail_parity_accepted"] != false
            || report["membership"]["payloads_validated"] != false
        {
            return Err("Dialogue metadata crossed its runtime/payload boundary".into());
        }
        hashes[phase] = digest(&path)?.into();
        reports.push(report);
    }
    let selected = &reports[0];
    for report in &reports[1..] {
        let mut a = selected.clone();
        let mut b = report.clone();
        a.as_object_mut()
            .ok_or("Missing dialogue object")?
            .remove("index_cache");
        b.as_object_mut()
            .ok_or("Missing dialogue object")?
            .remove("index_cache");
        if a != b {
            return Err("Cached and uncached dialogue metadata differ".into());
        }
    }
    let cold = reports[1]["index_cache"]["plugins"]
        .as_array()
        .ok_or("Missing cold entries")?;
    let warm = reports[2]["index_cache"]["plugins"]
        .as_array()
        .ok_or("Missing warm entries")?;
    if cold.len() != names.len()
        || warm.len() != names.len()
        || reports[2]["index_cache"]["format"] != "nv-header-index-v2"
    {
        return Err("Dialogue index version or plugin set differs".into());
    }
    let mut encoded_bytes = 0;
    for (a, b) in cold.iter().zip(warm) {
        if a["reused"] != false
            || b["reused"] != true
            || a["key"] != b["key"]
            || a["index_sha256"] != b["index_sha256"]
        {
            return Err("Dialogue cold/warm reuse differs".into());
        }
        let key = b["key"].as_str().ok_or("Missing cache key")?;
        if digest(&cache.join(format!("{key}.blob")))? != b["index_sha256"] {
            return Err("Dialogue cache digest differs".into());
        }
        encoded_bytes += b["index_bytes"].as_u64().ok_or("Missing index bytes")?;
    }
    let (native, native_path) = compare_native(
        root,
        run,
        oracle,
        &install.join("Data"),
        &binary_order,
        "native",
    )?;
    for key in ["plugins", "metadata", "membership"] {
        if selected[key] != native[key] {
            return Err(format!("Independent original-header comparison differs: {key}").into());
        }
    }
    let baseline = json_file(&root.join("local/baseline.json"))?;
    for source in selected["plugins"]
        .as_array()
        .ok_or("Missing source receipts")?
    {
        let name = source["source_name"]
            .as_str()
            .ok_or("Missing plugin name")?;
        let file = baseline["files"]
            .as_array()
            .ok_or("Missing baseline files")?
            .iter()
            .find(|file| file["path"] == format!("Data/{name}"))
            .ok_or("Source absent from baseline")?;
        if file["sha256"] != source["source_sha256"] || file["bytes"] != source["source_bytes"] {
            return Err("Dialogue source changed from baseline".into());
        }
    }
    // Damage a separate cache copy. The verified cold/warm artifacts stay intact.
    let corrupt = run.join("corrupt-cache");
    fs::create_dir(&corrupt)?;
    let key = warm.first().ok_or("Empty warm cache")?["key"]
        .as_str()
        .ok_or("Missing first cache key")?;
    let mut bytes = fs::read(cache.join(format!("{key}.blob")))?;
    bytes[0] ^= 1;
    write_new(&corrupt.join(format!("{key}.blob")), &bytes)?;
    write_new(
        &corrupt.join(format!("{key}.json")),
        &fs::read(cache.join(format!("{key}.json")))?,
    )?;
    let corrupt_report = run.join("corrupt.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("dialogue-membership")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order_path)
        .arg("--index-cache")
        .arg(&corrupt)
        .arg("--output")
        .arg(&corrupt_report);
    let output = run_logged_status(command, &run.join("corrupt.log"), 1)?;
    if corrupt_report.exists()
        || !String::from_utf8(output.stderr)?.contains("cache blob failed digest check")
    {
        return Err("Corrupt dialogue cache was not rejected".into());
    }
    let cell_directory = run.join("cell-index-regression");
    fs::create_dir(&cell_directory)?;
    let cell = super::index_evidence::run(root, &cell_directory, cli, install, &digest(cli)?)?;
    let fixtures = compare_fixtures(root, run, cli, oracle)?;
    let negatives = negative_orders(root, run, oracle, &install.join("Data"))?;
    let source_negatives = negative_sources(root, run, oracle)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":names,"load_order_sha256":digest(&order_path)?,
        "plugins":selected["plugins"],"metadata":selected["metadata"],"membership_counts":selected["membership"]["counts"],
        "all_direct_source_headers_keys_and_parent_contexts_equal":true,"all_winning_info_memberships_equal":true,
        "cached_uncached_equal":true,"cold_entries_built":cold.len(),"warm_entries_reused":warm.len(),"index_format":"nv-header-index-v2",
        "encoded_index_bytes":encoded_bytes,"cache_receipts":warm,"corrupt_cache_rejected_without_report":true,
        "selected_cell_cache_regression":cell,"source_fixtures":fixtures,"negative_order_cases_rejected":negatives,"negative_source_cases_rejected":source_negatives,
        "rust_report_sha256":hashes,"native_report_sha256":digest(&native_path)?,"order_bundle_sha256":digest(&binary_order)?,
        "comparison_scope":"independent direct original plugin-header framing, master-relative canonical keys, all record parent contexts and whole-record winning INFO topic membership",
        "record_payloads_validated":false,"retail_selection_order_verified":false,"runtime_ready":false,"retail_parity_accepted":false,
        "known_gaps":["Original effective load order remains unmeasured; this is the supplied inspection order",
            "Record-specific override exceptions and retail INFO ordering/selection","Deferred payload integrity and typed narrative/runtime state",
            "Condition subjects/queries, embedded-script scheduling and native effects"]}),
    )
}

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], form: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 4],
        &15_u16.to_le_bytes(),
        &[0; 2],
        payload,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut payload = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for name in masters {
        payload.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        payload.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &payload)
}
fn group(label: u32, kind: i32, payload: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(payload.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}
fn compare_fixtures(root: &Path, run: &Path, cli: &Path, oracle: &Path) -> Result<Value> {
    let install = run.join("source-fixture");
    fs::create_dir(&install)?;
    let data = install.join("Data");
    fs::create_dir(&data)?;
    write_new(&data.join("Other.esm"), &header(&[]))?;
    let base = [
        header(&[]),
        record(b"DIAL", 0x100, 0, &[]),
        record(b"DIAL", 0x200, 0, &[]),
        record(b"ACTI", 0x900, 0, &[]),
        group(0x100, 7, &record(b"INFO", 0x300, 0, &[])),
        record(b"INFO", 0x301, 0, &[]),
        group(0, 7, &record(b"INFO", 0x302, 0, &[])),
        group(0xa00, 7, &record(b"INFO", 0x303, 0, &[])),
        group(0x900, 7, &record(b"INFO", 0x304, 0, &[])),
        record(b"INFO", 0x305, fallout_data::plugin::DELETED, &[]),
        record(b"WRLD", 0x700, 0, &[]),
        group(
            0x700,
            1,
            &[
                record(b"CELL", 0x800, 0, &field(b"DATA", &[0])),
                group(0x800, 6, &group(0x800, 8, &record(b"REFR", 0x801, 0, &[]))),
            ]
            .concat(),
        ),
    ]
    .concat();
    write_new(&data.join("Base.esm"), &base)?;
    write_new(
        &data.join("Move.esp"),
        &[
            header(&["Other.esm", "Base.esm"]),
            group(0x0100_0200, 7, &record(b"INFO", 0x0100_0300, 0, &[])),
        ]
        .concat(),
    )?;
    write_new(
        &data.join("Delete.esp"),
        &[
            header(&["Base.esm"]),
            record(b"DIAL", 0x200, fallout_data::plugin::DELETED, &[]),
        ]
        .concat(),
    )?;
    let fixture_reports = run.join("source-fixture-reports");
    fs::create_dir(&fixture_reports)?;
    let cases = [
        vec!["Base.esm"],
        vec!["Other.esm", "Base.esm", "Move.esp"],
        vec!["Base.esm", "Other.esm", "Move.esp"],
        vec!["Base.esm", "Other.esm", "Move.esp", "Delete.esp"],
    ];
    let mut summaries = Vec::new();
    let mut moved = None;
    for (number, names) in cases.into_iter().enumerate() {
        let names: Vec<String> = names.into_iter().map(str::to_owned).collect();
        let order = install.join(format!("order-{number}.json"));
        write_new(&order, &serde_json::to_vec(&names)?)?;
        let binary_order = install.join(format!("order-{number}.bin"));
        write_new(&binary_order, &order_bundle(&names)?)?;
        let (rust, _) = membership(
            root,
            &fixture_reports,
            cli,
            Inspection {
                install: &install,
                order: &order,
                cache: None,
            },
            &format!("rust-{number}"),
            1,
        )?;
        let (native, _) = compare_native(
            root,
            &fixture_reports,
            oracle,
            &data,
            &binary_order,
            &format!("native-{number}"),
        )?;
        for key in ["plugins", "metadata", "membership"] {
            if rust[key] != native[key] {
                return Err("Independent source fixture differs".into());
            }
        }
        if number == 1 {
            moved = Some(rust["membership"].clone());
        }
        if number == 2 && moved != Some(rust["membership"].clone()) {
            return Err("Membership changed after unrelated source reorder".into());
        }
        summaries.push(json!({"order":names,"metadata":rust["metadata"],"membership_counts":rust["membership"]["counts"],"all_equal":true}));
    }
    Ok(
        json!({"cases":summaries,"all_statuses_source_rebasing_moved_overrides_and_deleted_topics_compared":true}),
    )
}
fn negative_orders(
    root: &Path,
    run: &Path,
    oracle: &Path,
    data: &Path,
) -> Result<Vec<&'static str>> {
    let cases = [
        ("short_magic", b"FRORDER".to_vec()),
        ("wrong_magic", b"BRORDER1\x01\x00".to_vec()),
        ("zero_count", b"FRORDER1\x00\x00".to_vec()),
        ("large_count", b"FRORDER1\xff\x00".to_vec()),
        ("short_name", b"FRORDER1\x01\x00\x04\x00A".to_vec()),
        (
            "unsafe_name",
            [b"FRORDER1\x01\x00\x08\x00".as_slice(), b"../A.esm"].concat(),
        ),
        (
            "duplicate_names",
            order_bundle(&["FalloutNV.esm".into(), "falloutnv.esm".into()])?,
        ),
        (
            "later_master",
            order_bundle(&["DeadMoney.esm".into(), "FalloutNV.esm".into()])?,
        ),
        (
            "surplus_order",
            [order_bundle(&["FalloutNV.esm".into()])?, vec![0]].concat(),
        ),
    ];
    let mut rejected = Vec::new();
    for (name, bytes) in cases {
        let path = run.join(format!("negative-order-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(data).arg(&path);
        let output =
            run_logged_status(command, &run.join(format!("negative-order-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Invalid order produced a complete report".into());
        }
        rejected.push(name);
    }
    Ok(rejected)
}

fn negative_sources(root: &Path, run: &Path, oracle: &Path) -> Result<Vec<&'static str>> {
    let mut bad_magic = header(&[]);
    bad_magic[..4].copy_from_slice(b"NOPE");
    let mut bad_version = header(&[]);
    bad_version[30..34].copy_from_slice(&0_u32.to_le_bytes());
    let mut bad_group = group(0x100, 7, &record(b"INFO", 0x300, 0, &[]));
    bad_group[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    let cases = [
        ("missing_tes4", bad_magic),
        ("duplicate_tes4", [header(&[]), header(&[])].concat()),
        ("bad_hedr_version", bad_version),
        ("later_missing_master", header(&["Later.esm"])),
        (
            "truncated_header",
            [header(&[]), b"DIAL\x00\x00".to_vec()].concat(),
        ),
        ("group_extent", [header(&[]), bad_group].concat()),
        (
            "duplicate_raw_id",
            [
                header(&[]),
                record(b"DIAL", 0x100, 0, &[]),
                record(b"DIAL", 0x100, 0, &[]),
            ]
            .concat(),
        ),
        (
            "null_definition",
            [header(&[]), record(b"DIAL", 0, 0, &[])].concat(),
        ),
        (
            "cell_child_label",
            [
                header(&[]),
                group(0x100, 6, &group(0x101, 8, &record(b"REFR", 0x300, 0, &[]))),
            ]
            .concat(),
        ),
    ];
    let order = run.join("negative-source-order.bin");
    write_new(&order, &order_bundle(&["Base.esm".into()])?)?;
    let mut rejected = Vec::new();
    for (name, bytes) in cases {
        let data = run.join(format!("negative-source-{name}"));
        fs::create_dir(&data)?;
        write_new(&data.join("Base.esm"), &bytes)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg(&data).arg(&order);
        let output =
            run_logged_status(command, &run.join(format!("negative-source-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Malformed source produced a complete report".into());
        }
        rejected.push(name);
    }
    Ok(rejected)
}
