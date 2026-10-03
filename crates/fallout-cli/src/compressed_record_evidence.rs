//! Independent extraction from original compressed bytes, with authored format
//! boundaries. A checksum finding is evidence of damage, not runtime permission.
use super::{Result, digest, json_file, run_logged_status, write_new};
use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path, process::Command};

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
    for (key, value) in native.as_object().ok_or("Missing extraction object")? {
        if rust[key] != *value {
            return Err(format!("Independent compressed extraction differs: {key}").into());
        }
    }
    Ok(())
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
    let order = run.join("compressed-order.bin");
    write_new(&order, &order_bundle(&names))?;
    let mut strict = Command::new(cli);
    let strict_path = run.join("strict-rust.json");
    strict
        .current_dir(root)
        .arg("compressed-records")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order_path)
        .arg("--output")
        .arg(&strict_path);
    let strict_output = run_logged_status(strict, &run.join("strict-rust.log"), 1)?;
    let message = String::from_utf8(strict_output.stderr)?;
    if strict_path.exists()
        || !message.contains("strict integrity check failed")
        || !message.contains("FalloutNV.esm at 0xB0CFF04")
    {
        return Err("Strict compressed read must fail at its checksum boundary".into());
    }
    let mut strict = Command::new(oracle);
    strict
        .current_dir(root)
        .arg("--corpus")
        .arg(install.join("Data"))
        .arg(&order);
    let strict_output = run_logged_status(strict, &run.join("strict-native.log"), 1)?;
    let message = String::from_utf8(strict_output.stderr)?;
    if !strict_output.stdout.is_empty()
        || !message.contains("checksum mismatch")
        || !message.contains("185401092")
        || !message.contains("1380288")
    {
        return Err("Independent strict extraction failed at another boundary".into());
    }
    let rust_path = run.join("compressed-records-rust.json");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("compressed-records")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(&order_path)
        .arg("--inspect-checksum-mismatches")
        .arg("--output")
        .arg(&rust_path);
    let output = run_logged_status(command, &run.join("compressed-records-rust.log"), 1)?;
    if !String::from_utf8(output.stderr)?
        .contains("compressed record inspection retains checksum findings")
    {
        return Err("Compressed inspection failed without the expected finding".into());
    }
    let rust = json_file(&rust_path)?;
    let native_path = run.join("compressed-records-native.json");
    let mut command = Command::new(oracle);
    command
        .current_dir(root)
        .arg("--corpus")
        .arg(install.join("Data"))
        .arg(&order)
        .arg("--inspect-checksum-mismatches");
    let output = run_logged_status(command, &run.join("compressed-records-native.log"), 1)?;
    write_new(&native_path, &output.stdout)?;
    compare(&rust, &json_file(&native_path)?)?;
    let census_path = root.join("local/census-with-scripts.json");
    let census_receipt = json_file(&root.join("reports/checkpoint-15-compiled-scripts.json"))?;
    if census_receipt["prior_full_census_sha256"] != digest(&census_path)? {
        return Err("Prior complete census digest differs".into());
    }
    let census = json_file(&census_path)?;
    let plugins = rust["plugins"]
        .as_array()
        .ok_or("Missing compressed plugins")?;
    for plugin in plugins {
        let prior = census["plugins"]
            .as_array()
            .ok_or("Missing census plugins")?
            .iter()
            .find(|row| row["name"] == plugin["source_name"])
            .ok_or("Compressed source absent from census")?;
        if prior["compressed_records"] != plugin["counts"]["records"]
            || prior["source_bytes"] != plugin["source_bytes"]
        {
            return Err("Compressed extraction coverage differs from full census".into());
        }
    }
    let prior_metadata = json_file(&root.join("reports/checkpoint-25-quest-scripts.json"))?;
    if rust["metadata"] != prior_metadata["metadata"]
        || rust["counts"]["checksum_mismatches"] != 1
        || rust["runtime_checksum_policy"] != "strict"
        || rust["tainted_payloads_runtime_eligible"] != false
        || rust["retail_parity_accepted"] != false
    {
        return Err("Extraction source or runtime boundary differs".into());
    }
    let mut findings = Vec::new();
    let sources = plugins.iter().map(|plugin| {
        for row in plugin["rows"].as_array().expect("validated extraction rows") {
            if !row["integrity_issue"].is_null() {
                findings.push(json!({"source":plugin["source_name"],"record":row}));
            }
        }
        json!({"source_name":plugin["source_name"],"source_bytes":plugin["source_bytes"],
            "source_sha256":plugin["source_sha256"],"groups":plugin["groups"],"counts":plugin["counts"]})
    }).collect::<Vec<_>>();
    let fixtures = frame_fixtures(root, run, oracle)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","counts":rust["counts"],"sources":sources,
        "metadata":rust["metadata"],"rust_report_sha256":digest(&rust_path)?,"native_report_sha256":digest(&native_path)?,
        "oracle_binary_sha256":digest(oracle)?,"prior_census_sha256":digest(&census_path)?,
        "prior_census_receipt_sha256":digest(&root.join("reports/checkpoint-15-compiled-scripts.json"))?,
        "prior_header_receipt_sha256":digest(&root.join("reports/checkpoint-25-quest-scripts.json"))?,
        "complete_compressed_coverage_equal":true,"all_stored_and_decoded_hashes_equal":true,
        "strict_original_failure_reproduced_by_both":true,"checksum_findings":findings,"frame_fixtures":fixtures,
        "runtime_checksum_policy":"strict","tainted_payloads_runtime_eligible":false,"retail_parity_accepted":false,
        "accepted_scenarios":[],"scope":"All compressed source records; independent RFC byte extraction, not gameplay or checksum compatibility"}),
    )
}

#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    count: usize,
}
impl Bits {
    fn word(&mut self, word: u32, width: u8) {
        for bit in 0..width {
            if self.count.is_multiple_of(8) {
                self.bytes.push(0);
            }
            self.bytes[self.count / 8] |= (((word >> bit) & 1) as u8) << (self.count % 8);
            self.count += 1;
        }
    }
    fn code(&mut self, code: u32, width: u8) {
        // RFC numeric words are LSB first; Huffman codes enter MSB first.
        for bit in (0..width).rev() {
            self.word((code >> bit) & 1, 1);
        }
    }
    fn fixed(&mut self, symbol: u32) {
        match symbol {
            0..=143 => self.code(symbol + 48, 8),
            144..=255 => self.code(symbol + 256, 9),
            256..=279 => self.code(symbol - 256, 7),
            280..=287 => self.code(symbol - 88, 8),
            _ => unreachable!("authored fixed symbol"),
        }
    }
    fn block(&mut self, final_block: bool, kind: u32) {
        self.word(u32::from(final_block), 1);
        self.word(kind, 2);
    }
}
fn frame(bits: Bits, plain: &[u8]) -> Vec<u8> {
    [
        vec![0x78, 0x01],
        bits.bytes,
        adler2::adler32_slice(plain).to_be_bytes().to_vec(),
    ]
    .concat()
}
fn bundle(rows: &[(String, Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"FRZLIB01".to_vec();
    for (_, plain, encoded) in rows {
        bytes.extend((plain.len() as u32).to_le_bytes());
        bytes.extend((encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded);
    }
    bytes
}
fn library_decode(encoded: &[u8], expected: &[u8]) -> Result<()> {
    let mut decoder = Decompress::new(true);
    let mut output = vec![0; expected.len() + 1];
    let status = decoder.decompress(encoded, &mut output, FlushDecompress::Finish)?;
    if status != Status::StreamEnd
        || decoder.total_in() != encoded.len() as u64
        || decoder.total_out() != expected.len() as u64
        || output[..expected.len()] != *expected
    {
        return Err("Authored frame differs from pinned Rust compression library".into());
    }
    Ok(())
}
fn dynamic_header(bits: &mut Bits, code_lengths: &[(usize, u32)]) {
    bits.block(true, 2);
    bits.word(0, 5);
    bits.word(0, 5);
    bits.word(14, 4);
    let order = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1];
    for symbol in order {
        bits.word(
            code_lengths
                .iter()
                .find(|(s, _)| *s == symbol)
                .map_or(0, |(_, w)| *w),
            3,
        );
    }
}
fn authored_frames() -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let mut rows = Vec::new();
    let mut bits = Bits::default();
    bits.block(true, 1);
    bits.fixed(256);
    rows.push(("fixed-empty".into(), vec![], frame(bits, &[])));
    let plain: Vec<_> = (0..=255).collect();
    let mut bits = Bits::default();
    bits.block(true, 1);
    for byte in &plain {
        bits.fixed(u32::from(*byte));
    }
    bits.fixed(256);
    rows.push((
        "fixed-all-literals".into(),
        plain.clone(),
        frame(bits, &plain),
    ));
    let plain = vec![b'A'; 259];
    let mut bits = Bits::default();
    bits.block(true, 1);
    bits.fixed(u32::from(b'A'));
    bits.fixed(285);
    bits.code(0, 5);
    bits.fixed(256);
    rows.push((
        "overlapping-distance-one".into(),
        plain.clone(),
        frame(bits, &plain),
    ));
    let plain = b"ABC".repeat(87);
    let mut bits = Bits::default();
    bits.block(false, 1);
    for byte in b"ABC" {
        bits.fixed(u32::from(*byte));
    }
    bits.fixed(256);
    bits.block(true, 1);
    bits.fixed(285);
    bits.code(2, 5);
    bits.fixed(256);
    rows.push((
        "cross-block-distance-three".into(),
        plain.clone(),
        frame(bits, &plain),
    ));
    let mut plain: Vec<_> = (0..32768).map(|i| (i % 251) as u8).collect();
    let mut bits = Bits::default();
    bits.block(true, 1);
    for byte in &plain {
        bits.fixed(u32::from(*byte));
    }
    bits.fixed(257);
    bits.code(29, 5);
    bits.word(8191, 13);
    bits.fixed(256);
    plain.extend([0, 1, 2]);
    rows.push((
        "maximum-window-distance".into(),
        plain.clone(),
        frame(bits, &plain),
    ));
    // Only the EOB symbol exists. The distance alphabet is empty and unused.
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (1, 1)]);
    for _ in 0..256 {
        bits.code(0, 1);
    }
    bits.code(1, 1);
    bits.code(0, 1);
    bits.code(0, 1);
    rows.push((
        "dynamic-single-eob-empty-distance".into(),
        vec![],
        frame(bits, &[]),
    ));
    // The same valid alphabets, now using repeat 18 across a long zero run.
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (1, 2), (18, 2)]);
    bits.code(3, 2);
    bits.word(127, 7);
    bits.code(3, 2);
    bits.word(107, 7);
    bits.code(2, 2);
    bits.code(0, 1);
    bits.code(0, 1);
    rows.push(("dynamic-repeat-eighteen".into(), vec![], frame(bits, &[])));
    // Literal A and EOB share a complete one-bit alphabet; no distance is needed.
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (1, 2), (18, 2)]);
    bits.code(3, 2);
    bits.word(54, 7);
    bits.code(2, 2);
    bits.code(3, 2);
    bits.word(127, 7);
    bits.code(3, 2);
    bits.word(41, 7);
    bits.code(2, 2);
    bits.code(0, 1);
    for _ in 0..1000 {
        bits.code(0, 1);
    }
    bits.code(1, 1);
    let plain = vec![b'A'; 1000];
    rows.push((
        "dynamic-literals-empty-distance".into(),
        plain.clone(),
        frame(bits, &plain),
    ));
    rows
}
fn malformed_frames(valid: &[(String, Vec<u8>, Vec<u8>)]) -> Vec<(String, Vec<u8>)> {
    let mut negatives = Vec::new();
    let wrap = |name: &str, encoded: Vec<u8>, expected: usize| {
        let mut bytes = b"FRZLIB01".to_vec();
        bytes.extend((expected as u32).to_le_bytes());
        bytes.extend((encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded);
        (name.to_string(), bytes)
    };
    let empty = valid
        .iter()
        .find(|(n, _, _)| n == "fixed-empty")
        .unwrap()
        .2
        .clone();
    for (name, cmf, flg) in [
        ("bad-method", 0x79, 0x18),
        ("oversized-window", 0x88, 0x1c),
        ("bad-header-check", 0x78, 0x02),
        ("preset-dictionary", 0x78, 0x20),
    ] {
        let mut bytes = empty.clone();
        bytes[0] = cmf;
        bytes[1] = flg;
        negatives.push(wrap(name, bytes, 0));
    }
    let mut bits = Bits::default();
    bits.block(true, 3);
    negatives.push(wrap("reserved-block", frame(bits, &[]), 0));
    let mut bits = Bits::default();
    bits.block(true, 1);
    bits.fixed(286);
    negatives.push(wrap("reserved-length", frame(bits, &[]), 3));
    let mut bits = Bits::default();
    bits.block(true, 1);
    bits.fixed(u32::from(b'A'));
    bits.fixed(257);
    bits.code(30, 5);
    negatives.push(wrap("reserved-distance", frame(bits, &[]), 4));
    let mut bits = Bits::default();
    bits.block(true, 1);
    bits.fixed(257);
    bits.code(0, 5);
    negatives.push(wrap("match-before-output", frame(bits, &[]), 3));
    let mut stored = vec![0x78, 0x01, 1, 1, 0, 0, 0, b'A'];
    stored.extend(1_u32.to_be_bytes());
    negatives.push(wrap("stored-complement", stored, 1));
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (16, 1)]);
    bits.code(1, 1);
    negatives.push(wrap("repeat-without-previous", frame(bits, &[]), 0));
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (18, 1)]);
    bits.code(1, 1);
    bits.word(127, 7);
    bits.code(1, 1);
    bits.word(127, 7);
    negatives.push(wrap("repeat-overflow", frame(bits, &[]), 0));
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (1, 1), (18, 1)]);
    negatives.push(wrap("oversubscribed-code-tree", frame(bits, &[]), 0));
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 2), (1, 2)]);
    negatives.push(wrap("incomplete-code-tree", frame(bits, &[]), 0));
    let mut bits = Bits::default();
    dynamic_header(&mut bits, &[(0, 1), (1, 1)]);
    for _ in 0..258 {
        bits.code(0, 1);
    }
    negatives.push(wrap("missing-eob", frame(bits, &[]), 0));
    let mut bits = Bits::default();
    bits.block(true, 2);
    bits.word(31, 5);
    bits.word(0, 5);
    bits.word(0, 4);
    negatives.push(wrap("oversized-literal-alphabet", frame(bits, &[]), 0));
    let mut surplus = empty.clone();
    surplus.insert(surplus.len() - 4, 0);
    negatives.push(wrap("surplus-deflate", surplus, 0));
    negatives.push(wrap(
        "truncated-frame",
        empty[..empty.len() - 1].to_vec(),
        0,
    ));
    negatives.push(wrap("wrong-decoded-length", empty.clone(), 1));
    negatives.push(wrap(
        "oversized-decoded-budget",
        empty.clone(),
        64 * 1024 * 1024 + 1,
    ));
    let window = valid
        .iter()
        .find(|(n, _, _)| n == "maximum-window-distance")
        .unwrap();
    let mut narrow = window.2.clone();
    narrow[0] = 8;
    narrow[1] = 29;
    negatives.push(wrap(
        "match-exceeds-advertised-window",
        narrow,
        window.1.len(),
    ));
    negatives.push(("empty-bundle".into(), b"FRZLIB01".to_vec()));
    negatives.push((
        "partial-bundle-header".into(),
        [b"FRZLIB01".as_slice(), &[0; 7]].concat(),
    ));
    let mut bad_extent = wrap("partial-bundle-frame", empty, 0);
    bad_extent.1.pop();
    negatives.push(bad_extent);
    negatives
}

fn frame_fixtures(root: &Path, run: &Path, oracle: &Path) -> Result<Value> {
    let mut rows = authored_frames();
    for size in [0, 1, 2, 7, 31, 257, 1024, 4096, 32768, 65535, 65536, 100000] {
        let patterns = [
            vec![0xf3; size],
            (0..size).map(|i| ((i * 17 + i / 31) % 251) as u8).collect(),
            (0..size)
                .scan(0x7ac3_5219_u32, |state, _| {
                    *state ^= *state << 13;
                    *state ^= *state >> 17;
                    *state ^= *state << 5;
                    Some(*state as u8)
                })
                .collect(),
        ];
        for (pattern, plain) in patterns.into_iter().enumerate() {
            for level in [0, 1, 9] {
                let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(level));
                encoder.write_all(&plain)?;
                rows.push((
                    format!("library-{size}-{pattern}-{level}"),
                    plain.clone(),
                    encoder.finish()?,
                ));
            }
        }
    }
    for (_, plain, encoded) in &rows {
        library_decode(encoded, plain)?;
    }
    let path = run.join("valid-zlib-frames.bin");
    write_new(&path, &bundle(&rows))?;
    let mut command = Command::new(oracle);
    command.current_dir(root).arg("--frames").arg(&path);
    let output = run_logged_status(command, &run.join("valid-zlib-frames.log"), 0)?;
    let report_path = run.join("valid-zlib-frames.json");
    write_new(&report_path, &output.stdout)?;
    let native = json_file(&report_path)?;
    let frames = native["frames"].as_array().ok_or("Missing frame rows")?;
    if frames.len() != rows.len()
        || native["bundle_sha256"] != digest(&path)?
        || native["checksum_mismatches"] != 0
    {
        return Err("Independent frame fixture coverage differs".into());
    }
    let mut blocks = [0_u64; 3];
    let mut matches = 0_u64;
    for ((_, plain, encoded), native) in rows.iter().zip(frames) {
        if native["decoded_sha256"] != sha(plain)
            || native["frame_sha256"] != sha(encoded)
            || native["decoded_bytes"] != plain.len()
            || native["checksum_valid"] != true
            || native["calculated_adler32"] != adler2::adler32_slice(plain)
        {
            return Err("Independent authored frame bytes/checksum differ".into());
        }
        for (i, count) in blocks.iter_mut().enumerate() {
            *count += native["block_counts"][i]
                .as_u64()
                .ok_or("Missing block count")?;
        }
        matches += native["matches"].as_u64().ok_or("Missing match count")?;
    }
    if blocks.contains(&0) || matches == 0 {
        return Err("Frame fixtures miss a block kind or backreference".into());
    }
    let mut rejected = Vec::new();
    for (name, bytes) in malformed_frames(&rows) {
        let path = run.join(format!("bad-zlib-{name}.bin"));
        write_new(&path, &bytes)?;
        let mut command = Command::new(oracle);
        command.current_dir(root).arg("--frames").arg(&path);
        let output = run_logged_status(command, &run.join(format!("bad-zlib-{name}.log")), 1)?;
        if !output.stdout.is_empty() {
            return Err("Malformed frame produced a report".into());
        }
        rejected.push(name);
    }
    let (_, plain, encoded) = &rows[2];
    let mut encoded = encoded.clone();
    *encoded.last_mut().ok_or("Missing footer")? ^= 1;
    if library_decode(&encoded, plain).is_ok() {
        return Err("Rust accepted damaged checksum".into());
    }
    let path = run.join("checksum-zlib-frame.bin");
    write_new(
        &path,
        &bundle(&[("checksum".into(), plain.clone(), encoded)]),
    )?;
    let mut command = Command::new(oracle);
    command.current_dir(root).arg("--frames").arg(&path);
    let output = run_logged_status(command, &run.join("checksum-zlib-frame.log"), 1)?;
    let report_path = run.join("checksum-zlib-frame.json");
    write_new(&report_path, &output.stdout)?;
    let damaged = json_file(&report_path)?;
    if damaged["checksum_mismatches"] != 1
        || damaged["frames"][0]["decoded_sha256"] != sha(plain)
        || damaged["tainted_payloads_runtime_eligible"] != false
    {
        return Err("Checksum diagnostic changed decoded bytes or taint".into());
    }
    Ok(
        json!({"valid_frames":rows.len(),"valid_bundle_sha256":digest(&run.join("valid-zlib-frames.bin"))?,
        "native_report_sha256":digest(&run.join("valid-zlib-frames.json"))?,"block_counts":blocks,"matches":matches,
        "authored_format_cases":rows.iter().take(8).map(|(name,_,_)|name).collect::<Vec<_>>(),
        "malformed_cases_rejected":rejected,"checksum_diagnostic_sha256":digest(&report_path)?,
        "rust_library_rejects_damaged_checksum":true,"scope":"Authored edge frames and pinned-library-generated deterministic fixtures"}),
    )
}
