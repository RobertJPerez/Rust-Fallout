//! Direct original-byte condition comparison and winning full-scan coverage.
use super::{Result, digest, json_file, run_logged_status, write_new};
use fallout_data::{
    identity::{self, ProfileId},
    plugin,
    store::RecordStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn order_bundle(names: &[String]) -> Vec<u8> {
    let mut bytes = b"FRORDER1".to_vec();
    bytes.extend((names.len() as u16).to_le_bytes());
    for name in names {
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name.as_bytes());
    }
    bytes
}
struct Input<'a> {
    install: &'a Path,
    order: &'a Path,
    binary_order: &'a Path,
    cache: Option<&'a Path>,
    status: i32,
}
fn inspection(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    input: Input<'_>,
    name: &str,
) -> Result<(Value, PathBuf, PathBuf)> {
    let rust_path = run.join(format!("{name}-rust.json"));
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("condition-dependencies")
        .arg("--install")
        .arg(input.install)
        .arg("--load-order")
        .arg(input.order)
        .arg("--output")
        .arg(&rust_path);
    if let Some(cache) = input.cache {
        command.arg("--index-cache").arg(cache);
    }
    run_logged_status(command, &run.join(format!("{name}-rust.log")), input.status)?;
    let rust = json_file(&rust_path)?;
    let native_path = run.join(format!("{name}-native.json"));
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg(input.install.join("Data"))
        .arg(input.binary_order)
        .arg(input.install.join("FalloutNV.exe"));
    let output = run_logged_status(
        command,
        &run.join(format!("{name}-native.log")),
        input.status,
    )?;
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    for (key, value) in native
        .as_object()
        .ok_or("Missing native condition object")?
    {
        if rust[key] != *value {
            return Err(
                format!("Independent condition dependency comparison differs: {key}").into(),
            );
        }
    }
    if rust["live_values_resolved"] != false
        || rust["evaluation_ready"] != false
        || rust["retail_parity_accepted"] != false
        || rust["target_kind_acceptance_checked"] != false
    {
        return Err("Condition dependency behavior boundary differs".into());
    }
    Ok((rust, rust_path, native_path))
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    descriptor_oracle: &Path,
    install: &Path,
) -> Result<Value> {
    let order = root.join("profiles/nv-inspection-order.json");
    let names: Vec<String> = serde_json::from_value(json_file(&order)?)?;
    let binary_order = run.join("condition-dependency-order.bin");
    write_new(&binary_order, &order_bundle(&names))?;
    let (rust, rust_path, native_path) = inspection(
        root,
        run,
        cli,
        oracle,
        Input {
            install,
            order: &order,
            binary_order: &binary_order,
            cache: None,
            status: 1,
        },
        "condition-dependencies",
    )?;
    if rust["counts"]["unknown_parameters"] != 0
        || rust["counts"]["source_findings"] != 3
        || rust["metadata"]
            != json_file(&root.join("reports/checkpoint-26-compressed-records.json"))?["metadata"]
    {
        return Err("Original condition source findings or header cohort changed".into());
    }
    let coverage = winning_coverage(root, install, &names, &rust)?;
    let fixtures = fixtures(root, run, cli, oracle, &install.join("FalloutNV.exe"))?;
    let malformed = malformed(root, run, cli, oracle, &install.join("FalloutNV.exe"))?;
    let catalogue_directory = run.join("catalogue-regression");
    fs::create_dir(&catalogue_directory)?;
    let catalogue = super::catalogue_evidence::run(
        root,
        &catalogue_directory,
        cli,
        descriptor_oracle,
        install,
    )?;
    let mut findings = Vec::new();
    for record in rust["records"]
        .as_array()
        .ok_or("Missing condition records")?
    {
        for condition in record["conditions"]
            .as_array()
            .ok_or("Missing condition fields")?
        {
            if !condition["source_findings"]
                .as_array()
                .ok_or("Missing source findings")?
                .is_empty()
            {
                findings.push(json!({"key":record["key"],"source_name":record["source_name"],"record_file_offset":record["record_file_offset"],
                    "field_decoded_offset":condition["field_decoded_offset"],"findings":condition["source_findings"]}));
            }
        }
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","counts":rust["counts"],"metadata":rust["metadata"],"sources":rust["sources"],
        "executable_sha256":rust["executable_sha256"],"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "oracle_binary_sha256":digest(oracle)?,"all_original_payloads_and_condition_bindings_equal":true,"winning_full_scan_coverage":coverage,
        "source_findings":findings,"synthetic_fixtures":fixtures,"malformed_cases_rejected":malformed,"fresh_catalogue_regression":catalogue,
        "target_kind_acceptance_checked":false,"live_values_resolved":false,"evaluation_ready":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "scope":"Winning CTDA word types, source namespaces and immutable form dependencies; no query evaluation, grouping or runtime defaults"}),
    )
}
fn winning_coverage(root: &Path, install: &Path, names: &[String], rust: &Value) -> Result<Value> {
    let receipt_path = root.join("reports/checkpoint-21-condition-fields.json");
    let receipt = json_file(&receipt_path)?;
    let full_path = root.join("local/condition-fields-21-verified/conditions-rust.json");
    if receipt["rust_report_sha256"] != digest(&full_path)? {
        return Err("Bound full condition report changed".into());
    }
    let census_path = root.join("local/census-with-scripts.json");
    if json_file(&root.join("reports/checkpoint-15-compiled-scripts.json"))?["prior_full_census_sha256"]
        != digest(&census_path)?
    {
        return Err("Bound complete source/master census changed".into());
    }
    let census = json_file(&census_path)?;
    let full = json_file(&full_path)?;
    let mut store =
        RecordStore::open_nv_headers(&install.join("Data"), names, plugin::Limits::default())?;
    if serde_json::to_value(store.source_receipts()?)? != rust["sources"] {
        return Err("Condition coverage source cohort differs".into());
    }
    let mut expected = BTreeMap::new();
    let mut total = 0_u64;
    for plugin in full["plugins"]
        .as_array()
        .ok_or("Missing prior condition plugins")?
    {
        let name = plugin["source_name"]
            .as_str()
            .ok_or("Missing condition source")?;
        let prior = census["plugins"]
            .as_array()
            .ok_or("Missing census plugins")?
            .iter()
            .find(|p| p["name"] == name)
            .ok_or("Missing source master table")?;
        let masters: Vec<String> = serde_json::from_value(prior["masters"].clone())?;
        let source = rust["sources"]
            .as_array()
            .ok_or("Missing source receipts")?
            .iter()
            .find(|p| p["source_name"] == name)
            .ok_or("Missing condition source receipt")?;
        if source["source_sha256"] != plugin["source_sha256"] {
            return Err("Prior condition source digest differs".into());
        }
        for row in plugin["rows"]
            .as_array()
            .ok_or("Missing full condition rows")?
        {
            total += 1;
            let raw = u32::try_from(
                row["form_id"]
                    .as_u64()
                    .ok_or("Missing raw condition owner")?,
            )?;
            let key = identity::resolve_form(ProfileId::NvOriginal, name, &masters, raw)?
                .ok_or("Null condition owner")?;
            let location = store
                .winner(&key)
                .ok_or("Prior condition owner absent from winner index")?;
            let header = &store.definition(location).header;
            if header.flags & plugin::DELETED != 0
                || store.source_name(location) != name
                || row["record_file_offset"] != header.offset
            {
                continue;
            }
            let key = (
                name.to_string(),
                header.offset,
                row["field_decoded_offset"]
                    .as_u64()
                    .ok_or("Missing field offset")?,
            );
            if expected.insert(key, row).is_some() {
                return Err("Duplicate full condition identity".into());
            }
        }
    }
    let mut matched = 0_usize;
    for record in rust["records"]
        .as_array()
        .ok_or("Missing winning condition records")?
    {
        for field in record["conditions"]
            .as_array()
            .ok_or("Missing winning condition fields")?
        {
            let key = (
                record["source_name"]
                    .as_str()
                    .ok_or("Missing winning source")?
                    .to_string(),
                record["record_file_offset"]
                    .as_u64()
                    .ok_or("Missing winning offset")?,
                field["field_decoded_offset"]
                    .as_u64()
                    .ok_or("Missing field offset")?,
            );
            let prior = expected
                .remove(&key)
                .ok_or("Winning condition absent from full source coverage")?;
            for name in [
                "bytes",
                "sha256",
                "flags",
                "flag_padding",
                "function_id",
                "function_padding",
                "comparison_operator",
                "comparison_value",
                "parameter_words",
                "run_on_word",
                "reference_word",
                "preceding_field_kind",
                "preceding_field_decoded_offset",
            ] {
                if prior[name] != field[name] {
                    return Err(format!(
                        "Winning condition differs from full source field: {name}"
                    )
                    .into());
                }
            }
            matched += 1;
        }
    }
    if !expected.is_empty() || rust["counts"]["conditions"] != matched {
        return Err("Incomplete winning condition coverage".into());
    }
    Ok(
        json!({"full_source_fields":total,"winning_fields":matched,"excluded_nonwinning_or_deleted_fields":total-matched as u64,
        "full_report_sha256":digest(&full_path)?,"receipt_sha256":digest(&receipt_path)?,"source_master_census_sha256":digest(&census_path)?,
        "all_winning_source_fields_equal":true}),
    )
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
    for name in masters {
        body.extend(field(b"MAST", &[name.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn condition(function: u16, parameters: [u32; 2], run: u32, reference: u32) -> Vec<u8> {
    let mut bytes = vec![0; 28];
    bytes[8..10].copy_from_slice(&function.to_le_bytes());
    bytes[12..16].copy_from_slice(&parameters[0].to_le_bytes());
    bytes[16..20].copy_from_slice(&parameters[1].to_le_bytes());
    bytes[20..24].copy_from_slice(&run.to_le_bytes());
    bytes[24..28].copy_from_slice(&reference.to_le_bytes());
    bytes
}
fn fixtures(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    executable: &Path,
) -> Result<Value> {
    let install = run.join("condition-fixture-install");
    let data = install.join("Data");
    fs::create_dir_all(&data)?;
    fs::copy(executable, install.join("FalloutNV.exe"))?;
    let mut rows = Vec::new();
    for selector in 0..=18 {
        rows.push(condition(408, [selector, 0x201], 0, 0));
    }
    for raw in [0, 0x200, 0x201, 0x203, 0x14, 0xff000201] {
        rows.push(condition(1, [raw, 0xfeedface], 0, 0));
    }
    for run in 0..=7 {
        rows.push(condition(5, [0xfeedface, 0x10203040], run, 0x14));
    }
    for function in [106, 285] {
        rows.push(condition(function, [0, 0], 2, 0x201));
    }
    rows.push(condition(79, [0x201, 0xffff_ffd6], 0, 0));
    rows.push(condition(53, [0x201, 42], 0, 0));
    rows.push(condition(98, [0x201, 0x200], 0, 0));
    rows.push(condition(65535, [0x201, 0x200], 0, 0));
    rows.push(condition(427, [0x201, 0x12345678], 0, 0));
    rows.push(condition(278, [0, 0], 0, 0));
    for function in [14, 6, 70, 59, 420, 421, 438] {
        rows.push(condition(function, [0x201, 0xffff_ffd6], 0, 0));
    }
    let mut global = condition(5, [0, 0], 0, 0);
    global[0] = 4;
    global[4..8].copy_from_slice(&0x202_u32.to_le_bytes());
    rows.push(global);
    let mut nonfinite = condition(5, [0, 0], 0, 0);
    nonfinite[4..8].copy_from_slice(&0xffc01234_u32.to_le_bytes());
    rows.push(nonfinite);
    let mut flags = condition(5, [0, 0], 0, 0);
    flags[0] = 0xe3;
    flags[1] = 0x81;
    flags[10] = 0x6f;
    rows.push(flags);
    let mut legacy = condition(5, [0, 0], 0, 0);
    legacy[0] = 2;
    legacy.truncate(20);
    rows.push(legacy);
    let mut short = condition(1, [0x201, 0], 2, 0);
    short.truncate(24);
    rows.push(short);
    let body = rows
        .iter()
        .flat_map(|row| field(b"CTDA", row))
        .collect::<Vec<_>>();
    let source = [
        header(&[]),
        record(b"MISC", 0x200, 0, &[]),
        record(b"QUST", 0x201, 0, &[]),
        record(b"GLOB", 0x202, 0, &[]),
        record(
            b"PERK",
            0x500,
            0,
            &field(b"CTDA", &condition(5, [0, 0], 0, 0)),
        ),
        record(
            b"INFO",
            0x501,
            0,
            &field(b"CTDA", &condition(5, [0, 0], 0, 0)),
        ),
    ]
    .concat();
    let patch = [
        header(&["FalloutNV.esm"]),
        record(
            b"MISC",
            0x200,
            plugin::DELETED,
            b"deleted payload is not read",
        ),
        record(b"MISC", 0x01000201, 0, &[]),
        record(b"PERK", 0x500, 0, &body),
        record(
            b"INFO",
            0x501,
            plugin::DELETED,
            b"deleted condition owner is not read",
        ),
    ]
    .concat();
    write_new(&data.join("FalloutNV.esm"), &source)?;
    write_new(&data.join("Patch.esp"), &patch)?;
    write_new(&data.join("Unrelated.esp"), &header(&[]))?;
    let mut proof = Vec::new();
    let mut reference = None;
    let cache = run.join("condition-fixture-cache");
    fs::create_dir(&cache)?;
    for (name, names, cached) in [
        ("fixture-cold", vec!["FalloutNV.esm", "Patch.esp"], true),
        ("fixture-warm", vec!["FalloutNV.esm", "Patch.esp"], true),
        (
            "fixture-reordered",
            vec!["FalloutNV.esm", "Unrelated.esp", "Patch.esp"],
            false,
        ),
    ] {
        let names = names.into_iter().map(String::from).collect::<Vec<_>>();
        let order = run.join(format!("{name}-order.json"));
        write_new(&order, &serde_json::to_vec(&names)?)?;
        let binary = run.join(format!("{name}-order.bin"));
        write_new(&binary, &order_bundle(&names))?;
        let (report, rust_path, native_path) = inspection(
            root,
            run,
            cli,
            oracle,
            Input {
                install: &install,
                order: &order,
                binary_order: &binary,
                cache: cached.then_some(cache.as_path()),
                status: 1,
            },
            name,
        )?;
        if report["counts"]["conditions"] != rows.len()
            || report["counts"]["deleted_candidate_records"] != 1
        {
            return Err("Synthetic condition coverage or tombstone policy differs".into());
        }
        for state in [
            "defined",
            "deleted",
            "missing",
            "null",
            "runtime_dependency",
        ] {
            if report["counts"]["form_statuses"][state]
                .as_u64()
                .unwrap_or(0)
                == 0
            {
                return Err("Synthetic condition fixture misses a dependency state".into());
            }
        }
        if let Some(prior) = &reference {
            if *prior != report["records"] {
                return Err("Cache/reordering changed canonical condition bindings".into());
            }
        } else {
            reference = Some(report["records"].clone());
        }
        let cache_status = if cached {
            report["index_cache"]["plugins"]
                .as_array()
                .ok_or("Missing fixture cache receipts")?
                .iter()
                .map(|p| p["reused"].clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if cached
            && cache_status
                .iter()
                .any(|value| *value != (name == "fixture-warm"))
        {
            return Err("Synthetic condition cache did not build/reuse as expected".into());
        }
        proof.push(json!({"name":name,"counts":report["counts"],"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,"cache_reused":cache_status}));
    }
    Ok(
        json!({"conditions":rows.len(),"phases":proof,"cache_and_reordering_keep_bindings_equal":true,
        "scope":"Authored CTDA words; all VATS selector branches, dependency states, short layouts, unknown descriptors and tombstones"}),
    )
}
fn malformed(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    executable: &Path,
) -> Result<Vec<String>> {
    let install = run.join("condition-negative-install");
    fs::create_dir_all(install.join("Data"))?;
    fs::copy(executable, install.join("FalloutNV.exe"))?;
    let names = vec!["FalloutNV.esm".to_string()];
    let order = run.join("negative-condition-order.json");
    write_new(&order, &serde_json::to_vec(&names)?)?;
    let binary = run.join("negative-condition-order.bin");
    write_new(&binary, &order_bundle(&names))?;
    let mut cases = Vec::new();
    for length in [0, 19, 21, 25, 29] {
        cases.push((
            format!("ctda-length-{length}"),
            field(b"CTDA", &vec![0; length]),
            0,
        ));
    }
    cases.push(("partial-field-header".into(), b"CTDA".to_vec(), 0));
    cases.push((
        "orphan-extended-field".into(),
        field(b"XXXX", &28_u32.to_le_bytes()),
        0,
    ));
    cases.push(("invalid-extended-length".into(), field(b"XXXX", &[0; 3]), 0));
    cases.push((
        "field-overrun".into(),
        [b"CTDA".as_slice(), &28_u16.to_le_bytes(), &[0; 27]].concat(),
        0,
    ));
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    let payload = field(b"CTDA", &condition(5, [0, 0], 0, 0));
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&payload)?;
    let mut encoded = encoder.finish()?;
    *encoded.last_mut().ok_or("Missing fixture checksum")? ^= 1;
    cases.push((
        "compressed-checksum".into(),
        [&(payload.len() as u32).to_le_bytes()[..], &encoded].concat(),
        plugin::COMPRESSED,
    ));
    let mut rejected = Vec::new();
    for (name, body, flags) in cases {
        fs::write(
            install.join("Data/FalloutNV.esm"),
            [header(&[]), record(b"PERK", 0x500, flags, &body)].concat(),
        )?;
        let path = run.join(format!("bad-condition-{name}.json"));
        let mut command = Command::new(cli);
        command
            .current_dir(root)
            .arg("condition-dependencies")
            .arg("--install")
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--output")
            .arg(&path);
        run_logged_status(
            command,
            &run.join(format!("bad-condition-{name}-rust.log")),
            1,
        )?;
        if path.exists() {
            return Err("Malformed condition source published a Rust report".into());
        }
        let mut command = Command::new(oracle);
        command
            .current_dir(root)
            .arg(install.join("Data"))
            .arg(&binary)
            .arg(install.join("FalloutNV.exe"));
        let output = run_logged_status(
            command,
            &run.join(format!("bad-condition-{name}-native.log")),
            1,
        )?;
        if !output.stdout.is_empty() {
            return Err("Malformed condition source published a native report".into());
        }
        rejected.push(name);
    }
    Ok(rejected)
}
