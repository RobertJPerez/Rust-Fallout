use dream_archive::bsa::tes4::Builder;
use skyrim_prep::{census, plugin};
use std::{fs, path::Path};

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend((data.len() as u16).to_le_bytes());
    out.extend(data);
    out
}
fn record(tag: &[u8; 4], flags: u32, id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend([0; 4]);
    out.extend([44, 0, 0, 0]);
    out.extend(payload);
    out
}
fn header(version: f32, flags: u32, masters: &[&str]) -> Vec<u8> {
    let mut hedr = version.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let mut body = field(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        body.extend(field(b"MAST", &name));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", flags, 0, &body)
}
fn vmad(status: u8) -> Vec<u8> {
    // Header, one script named Demo, no properties. No production serializer.
    vec![5, 0, 2, 0, 1, 0, 4, 0, b'D', b'e', b'm', b'o', status, 0, 0]
}
fn write_plugin(data: &Path, name: &str, mut h: Vec<u8>, body: &[u8], id: u32) {
    if !body.is_empty() {
        h.extend(record(b"ACTI", 0, id, body));
    }
    fs::write(data.join(name), h).unwrap();
}

#[test]
fn binding_export_retains_identity_order_and_visible_tail_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bindings.esp");
    let mut bytes = header(1.7, 0, &[]);
    let record_offset = bytes.len();
    let first = vmad(0);
    let mut unknown = vmad(2);
    unknown.extend([0xEE, 0xFF]);
    let mut fields = field(b"VMAD", &first);
    fields.extend(field(b"VMAD", &unknown));
    bytes.extend(record(b"ACTI", 0, 0x1234, &fields));
    fs::write(&path, &bytes).unwrap();
    let mut output = Vec::new();
    let summary = skyrim_prep::bindings::export(&path, &mut output).unwrap();
    assert_eq!(summary.bindings, 2);
    assert_eq!(summary.failures, 1);
    let rows: Vec<serde_json::Value> = output
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| serde_json::from_slice(b).unwrap())
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["type"], "source");
    assert_eq!(rows[0]["bytes"], bytes.len());
    assert_eq!(rows[1]["record_offset"], record_offset);
    assert_eq!(rows[1]["subrecord_offset_in_decoded_record"], 0);
    assert_eq!(rows[1]["vmad_offset_in_decoded_record"], 6);
    assert_eq!(rows[1]["form_id"], 0x1234);
    assert_eq!(rows[1]["attachment"]["scripts"][0]["status"], 0);
    assert_eq!(rows[2]["attachment"]["scripts"][0]["status"], 2);
    assert_eq!(
        rows[2]["attachment"]["tail"]["Unsupported"]["bytes"],
        serde_json::json!([238, 255])
    );
    assert_ne!(rows[1]["vmad_sha256"], rows[2]["vmad_sha256"]);
    assert_eq!(rows[3]["summary"]["failures"], 1);
    // Structural failure never emits a completeness trailer.
    fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
    output.clear();
    assert!(skyrim_prep::bindings::export(&path, &mut output).is_err());
    assert!(!String::from_utf8(output).unwrap().contains("\"complete\""));
}

#[test]
fn fragment_filename_without_executable_fragment_is_metadata_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = header(1.7, 0, &[]);
    // No primary scripts, no quest fragments, filename Old, no aliases.
    let vmad = [5, 0, 2, 0, 0, 0, 2, 0, 0, 3, 0, b'O', b'l', b'd', 0, 0];
    bytes.extend(record(b"QUST", 0, 0x800, &field(b"VMAD", &vmad)));
    fs::write(dir.path().join("Skyrim.esm"), bytes).unwrap();
    for name in census::REQUIRED_PLUGINS.iter().skip(1) {
        write_plugin(dir.path(), name, header(1.7, 1, &[]), &[], 0);
    }
    let report = census::inspect(dir.path(), None, |_| {}).unwrap();
    let source = report
        .plugins
        .iter()
        .find(|p| p.file == "Skyrim.esm")
        .unwrap();
    assert_eq!(source.vmad_decoded_tails, 1);
    assert_eq!(source.scripts[0].fragment_file_hints, 1);
    assert_eq!(source.scripts[0].fragment_references, 0);
    assert!(report.script_availability.is_empty());
}

#[test]
fn invalid_object_master_index_is_reported_without_rewriting_raw_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut body = vec![5, 0, 2, 0, 1, 0, 1, 0, b'S', 0, 1, 0];
    body.extend([1, 0, b'P', 1, 1, 0, 0, 255, 255, 0x34, 0x12, 0, 3]);
    write_plugin(
        dir.path(),
        "test.esp",
        header(1.7, 0, &["Skyrim.esm", "Update.esm"]),
        &field(b"VMAD", &body),
        0x02000800,
    );
    let report = plugin::inspect(&dir.path().join("test.esp")).unwrap();
    assert_eq!(report.vmad_object_index_findings, 1);
    assert_eq!(report.vmad_prefix_failures, 0);
    assert!(report.issue_examples[0].contains("03001234"));
    let decoded = skyrim_prep::vmad::decode(&body, "raw", Default::default()).unwrap();
    let skyrim_prep::vmad::Value::Object(object) = &decoded.scripts[0].properties[0].value else {
        panic!()
    };
    assert_eq!(object.form_id, 0x03001234);
    *body.last_mut().unwrap() = 2; // The plugin's own valid source index.
    write_plugin(
        dir.path(),
        "test.esp",
        header(1.7, 0, &["Skyrim.esm", "Update.esm"]),
        &field(b"VMAD", &body),
        0x02000800,
    );
    assert_eq!(
        plugin::inspect(&dir.path().join("test.esp"))
            .unwrap()
            .vmad_object_index_findings,
        0
    );
}

#[test]
fn installed_corpus_reaches_shared_plugin_reader_vmad_and_lz4_archive() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("Data");
    fs::create_dir(&data).unwrap();
    for name in census::REQUIRED_PLUGINS {
        write_plugin(&data, name, header(1.7, 1, &[]), &[], 0);
    }
    write_plugin(
        &data,
        "ccExample.esl",
        header(1.71, 0x280, &["Skyrim.esm"]),
        &field(b"VMAD", &vmad(0)),
        0x01000001,
    );
    let mut archive = Builder::skyrim_se();
    archive.set_compressed(true);
    archive
        .add_bytes(
            b"scripts/demo.pex",
            [0xFA, 0x57, 0xC0, 0xDE, 3, 2, 0, 1, 0xAA],
        )
        .unwrap();
    archive.write_path(data.join("ccExample.bsa")).unwrap();
    fs::create_dir(data.join("Scripts")).unwrap();
    fs::write(
        data.join("Scripts/loose.pex"),
        [0xFA, 0x57, 0xC0, 0xDE, 3, 2, 0, 1, 0xBB],
    )
    .unwrap();
    let report = census::inspect(&data, Some("1.7.104.0".into()), |_| {}).unwrap();
    assert_eq!(report.blocking_findings, 0, "{report:#?}");
    assert_eq!(report.plugins.len(), 6);
    assert_eq!(report.archives.len(), 1);
    assert_eq!(report.script_availability[0].containers.len(), 1);
    assert_eq!(report.script_availability[0].path, b"scripts/demo.pex");
    assert_eq!(report.creation_named_plugins, ["ccExample.esl"]);
    assert_eq!(report.schema_version, 3);
    assert_eq!(report.script_header_counts["big-endian/3.2/game-1"], 2);
    assert_eq!(report.script_header_findings, 0);
    assert_eq!(report.archives[0].scripts[0].dialect_prefix.game, Some(1));
    assert_eq!(report.loose_script_headers[0].dialect_prefix.minor, Some(2));
    let plugin = report
        .plugins
        .iter()
        .find(|p| p.file == "ccExample.esl")
        .unwrap();
    assert!(plugin.light && plugin.esl_extension && plugin.localized);
    assert_eq!(plugin.vmad_records, 1);
    let (bytes, sha256) = fallout_data::baseline::digest_file(&data.join("ccExample.bsa")).unwrap();
    assert_eq!(report.archives[0].sha256, sha256);
    assert_eq!(report.archives[0].bytes, bytes);
}

#[test]
fn light_header_version_and_master_override_are_distinguished() {
    let dir = tempfile::tempdir().unwrap();
    // New 1.70 light records cannot use the lower half, but an override can.
    write_plugin(
        dir.path(),
        "old.esp",
        header(1.7, 0x200, &["Skyrim.esm"]),
        &field(b"EDID", b"Demo\0"),
        0x01000001,
    );
    assert_eq!(
        plugin::inspect(&dir.path().join("old.esp"))
            .unwrap()
            .issue_count,
        1
    );
    write_plugin(
        dir.path(),
        "override.esl",
        header(1.7, 0, &["Skyrim.esm"]),
        &field(b"EDID", b"Demo\0"),
        0x00000001,
    );
    let extension_only = plugin::inspect(&dir.path().join("override.esl")).unwrap();
    assert_eq!(extension_only.issue_count, 0);
    assert!(!extension_only.light && extension_only.esl_extension);
}

#[test]
fn missing_and_duplicate_script_candidates_stay_explicit() {
    let dir = tempfile::tempdir().unwrap();
    write_plugin(
        dir.path(),
        "Mod.esp",
        header(1.7, 0, &["Missing.esm"]),
        &field(b"VMAD", &vmad(0)),
        0x01000800,
    );
    let report = census::inspect(dir.path(), None, |_| {}).unwrap();
    assert_eq!(report.missing_required.len(), 5);
    assert_eq!(report.dependency_findings.len(), 1);
    assert_eq!(
        report.script_availability[0].status,
        "missing-in-installed-corpus"
    );
    fs::create_dir(dir.path().join("Scripts")).unwrap();
    fs::write(dir.path().join("Scripts/Demo.pex"), b"loose").unwrap();
    let mut archive = Builder::skyrim_se();
    archive.add_bytes(b"scripts/demo.pex", b"archive").unwrap();
    archive.write_path(dir.path().join("Mod.bsa")).unwrap();
    let report = census::inspect(dir.path(), None, |_| {}).unwrap();
    assert_eq!(report.script_availability[0].containers.len(), 2);
    assert_eq!(
        report.script_availability[0].status,
        "multiple-candidates; precedence-unverified"
    );
    assert_eq!(report.script_header_findings, 2);
    assert_eq!(report.blocking_findings, 8);
    assert!(
        report
            .script_header_counts
            .contains_key("truncated-dialect-prefix")
    );
}

#[test]
fn unknown_vmad_and_fragments_cannot_be_claimed_fully_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let mut unknown = vmad(0);
    unknown[0] = 6;
    write_plugin(
        dir.path(),
        "Mod.esp",
        header(1.7, 0, &[]),
        &field(b"VMAD", &unknown),
        0x800,
    );
    let report = plugin::inspect(&dir.path().join("Mod.esp")).unwrap();
    assert_eq!(report.vmad_prefix_failures, 1);
    assert_eq!(report.issue_count, 1);
    let mut fragment = vmad(0);
    fragment.extend([0xDE, 0xAD]);
    write_plugin(
        dir.path(),
        "Mod.esp",
        header(1.7, 0, &[]),
        &field(b"VMAD", &fragment),
        0x800,
    );
    let report = plugin::inspect(&dir.path().join("Mod.esp")).unwrap();
    assert_eq!(report.vmad_prefix_failures, 0);
    assert_eq!(report.vmad_tails, 1);
}

#[test]
fn removed_attachment_is_preserved_but_not_a_required_script_file() {
    let dir = tempfile::tempdir().unwrap();
    write_plugin(
        dir.path(),
        "Removed.esp",
        header(1.7, 0, &[]),
        &field(b"VMAD", &vmad(2)),
        0x800,
    );
    let report = census::inspect(dir.path(), None, |_| {}).unwrap();
    assert!(report.script_availability.is_empty());
    assert_eq!(report.plugins[0].scripts[0].status_bytes[&2], 1);
}

#[test]
fn malformed_plugin_is_reported_and_other_plugins_are_still_inspected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Broken.esm"), b"TES4").unwrap();
    write_plugin(dir.path(), "Good.esm", header(1.7, 1, &[]), &[], 0);
    let report = census::inspect(dir.path(), None, |_| {}).unwrap();
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.plugins.len(), 1);
    assert_eq!(report.plugins[0].file, "Good.esm");
}

#[test]
fn missing_hedr_and_duplicate_masters_fail() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Missing.esm"), record(b"TES4", 0, 0, &[])).unwrap();
    assert!(plugin::inspect(&dir.path().join("Missing.esm")).is_err());
    write_plugin(
        dir.path(),
        "Dup.esm",
        header(1.7, 0, &["Skyrim.esm", "SKYRIM.ESM"]),
        &[],
        0,
    );
    assert!(plugin::inspect(&dir.path().join("Dup.esm")).is_err());
}
