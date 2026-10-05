use dream_archive::bsa::tes4::Builder;
use serde_json::Value;
use std::process::Command;

fn se_header(stream_version: u32) -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend_from_slice(&0x1402_0007u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&stream_version.to_le_bytes());
    bytes.extend_from_slice(b"opaque synthetic block bytes");
    bytes
}

fn fixture_archive(path: &std::path::Path, stream_version: u32) {
    let mut builder = Builder::skyrim_se();
    builder.set_compressed(true);
    builder
        .add_bytes(
            b"Meshes\\Actors\\Character\\skeleton.nif",
            se_header(stream_version),
        )
        .unwrap();
    builder
        .add_bytes(
            b"Meshes\\Actors\\Character\\face.nif",
            se_header(stream_version),
        )
        .unwrap();
    builder.write_path(path).unwrap();
}

fn push_sized(bytes: &[u8], output: &mut Vec<u8>) {
    output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    output.extend_from_slice(bytes);
}

fn sse_nif_fixture() -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend_from_slice(&0x1402_0007u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&100u32.to_le_bytes());
    for value in [b"author\0".as_slice(), b"process\0", b"export\0"] {
        bytes.push(value.len() as u8);
        bytes.extend_from_slice(value);
    }
    bytes.extend_from_slice(&2u16.to_le_bytes());
    push_sized(b"NiNode", &mut bytes);
    push_sized(b"BSTriShape", &mut bytes);
    for index in [0u16, 1, 1] {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    for size in [1u32, 2, 0] {
        bytes.extend_from_slice(&size.to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&4u32.to_le_bytes());
    push_sized(b"root", &mut bytes);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(b"abc");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes
}

#[test]
fn nif_header_cli_reads_a_case_normalized_synthetic_v105_member() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("synthetic.bsa");
    fixture_archive(&archive, 100);
    let result = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "nif-header",
            "--archive",
            archive.to_str().unwrap(),
            "--member",
            "meshes/actors/character/SKELETON.NIF",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["archive_version"], 105);
    assert_eq!(report["header"]["user_version"], 12);
    assert_eq!(report["header"]["user_version_2"], 100);
    assert_eq!(report["header"]["block_count"], 4);
    assert_eq!(report["header"]["skyrim_se_header_tuple"], true);
    assert!(report["scope"].as_str().unwrap().contains("no block"));
}

#[test]
fn nif_header_cli_surfaces_unrecognized_tuples_as_findings() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("synthetic.bsa");
    fixture_archive(&archive, 999);
    let result = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "nif-header",
            "--archive",
            archive.to_str().unwrap(),
            "--member",
            "meshes\\actors\\character\\skeleton.nif",
        ])
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["header"]["skyrim_se_header_tuple"], false);
    assert_eq!(
        report["header"]["classification"],
        "unclassified NIF header; block layout unsupported by this probe"
    );
}

#[test]
fn nif_list_cli_is_bounded_and_does_not_need_mesh_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("synthetic.bsa");
    fixture_archive(&archive, 100);
    let result = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "nif-list",
            "--archive",
            archive.to_str().unwrap(),
            "--prefix",
            "meshes/actors/character",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["matching_members"], 2);
    assert_eq!(report["returned_members"].as_array().unwrap().len(), 1);
    assert_eq!(report["offset"], 0);
    assert_eq!(report["truncated"], true);
    assert_eq!(report["duplicate_normalized_paths"], 0);
    assert_eq!(
        report["scope"],
        "archive index paths only; no member payloads decompressed; returned order follows the archive table"
    );

    let next_page = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "nif-list",
            "--archive",
            archive.to_str().unwrap(),
            "--offset",
            "1",
            "--limit",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        next_page.status.success(),
        "{}",
        String::from_utf8_lossy(&next_page.stderr)
    );
    let next_page_report: Value = serde_json::from_slice(&next_page.stdout).unwrap();
    assert_eq!(next_page_report["matching_members"], 2);
    assert_eq!(next_page_report["offset"], 1);
    assert_eq!(
        next_page_report["returned_members"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(next_page_report["truncated"], false);
    assert_ne!(
        report["returned_members"][0],
        next_page_report["returned_members"][0]
    );
}

#[test]
fn nif_index_cli_reports_sse_block_spans_without_decoding_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("synthetic.bsa");
    let mut builder = Builder::skyrim_se();
    builder.set_compressed(true);
    builder
        .add_bytes(b"Meshes\\Actors\\Character\\indexed.nif", sse_nif_fixture())
        .unwrap();
    builder.write_path(&archive).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "nif-index",
            "--archive",
            archive.to_str().unwrap(),
            "--member",
            "meshes/actors/character/INDEXED.NIF",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["archive_version"], 105);
    assert_eq!(report["index"]["user_version"], 12);
    assert_eq!(report["index"]["stream_version"], 100);
    assert_eq!(report["index"]["block_types"][0], "NiNode");
    assert_eq!(report["index"]["blocks"].as_array().unwrap().len(), 3);
    assert_eq!(report["index"]["blocks"][1]["type_name"], "BSTriShape");
    assert_eq!(report["index"]["blocks"][0]["bytes"], 1);
    assert_eq!(report["index"]["roots"][0], 0);
    assert!(
        report["index"]["semantics"]
            .as_str()
            .unwrap()
            .contains("block payloads")
    );
}

#[test]
fn nif_index_cli_rejects_a_non_sse_stream_tuple() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("legacy.bsa");
    fixture_archive(&archive, 83);
    let result = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "nif-index",
            "--archive",
            archive.to_str().unwrap(),
            "--member",
            "meshes/actors/character/skeleton.nif",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("requires 20.2.0.7/user-12/stream-100")
    );
}
