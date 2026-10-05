use skyrim_prep::static_models;
use std::{fs, path::Path};

fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    bytes.extend((data.len() as u16).to_le_bytes());
    bytes.extend(data);
    bytes
}

fn record(tag: &[u8; 4], id: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = tag.to_vec();
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(0u32.to_le_bytes()); // flags
    bytes.extend(id.to_le_bytes());
    bytes.extend([0; 8]); // revision, version and trailing record-header bytes
    bytes.extend(payload);
    bytes
}

fn plugin_header() -> Vec<u8> {
    record(
        b"TES4",
        0,
        &field(b"HEDR", &[0, 0, 0xD8, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0]),
    )
}

fn write_plugin(path: &Path, stat_payload: &[u8]) -> Vec<u8> {
    let mut bytes = plugin_header();
    bytes.extend(record(b"STAT", 0x1234, stat_payload));
    fs::write(path, &bytes).unwrap();
    bytes
}

#[test]
fn export_keeps_stat_source_paths_lod_slots_and_auxiliary_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Models.esp");
    let mut distant_lod = vec![0xA5; 4 * 260];
    let lod_path = b"meshes\\lod\\test_lod_0.nif\0";
    distant_lod[..lod_path.len()].copy_from_slice(lod_path);
    // Empty the remaining three names while leaving arbitrary slot filler intact.
    distant_lod[260..520].fill(0);
    distant_lod[520..780].fill(0);
    distant_lod[780..1040].fill(0);

    let mut payload = field(b"EDID", b"ClutterTestStatic\0");
    payload.extend(field(b"MODL", b"Meshes\\Clutter\\Test.NIF\0"));
    payload.extend(field(b"MODT", &[0x10, 0x20, 0x30]));
    payload.extend(field(b"MODS", &[0xAA, 0xBB]));
    payload.extend(field(b"MNAM", &distant_lod));
    let source = write_plugin(&path, &payload);
    let mut output = Vec::new();
    let summary = static_models::export(&path, &mut output).unwrap();
    assert_eq!(summary.stat_records, 1);
    assert_eq!(summary.editor_id_fields, 1);
    assert_eq!(summary.malformed_editor_ids, 0);
    assert_eq!(summary.records_without_modl, 0);
    assert_eq!(summary.model_path_fields, 1);
    assert_eq!(summary.auxiliary_model_fields, 2);
    assert_eq!(summary.distant_lod_fields, 1);
    assert_eq!(summary.distant_lod_paths, 1);
    assert_eq!(summary.malformed_model_paths, 0);
    assert_eq!(summary.malformed_lod_shapes, 0);

    let rows: Vec<serde_json::Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["type"], "source");
    assert_eq!(rows[0]["bytes"], source.len());
    assert_eq!(rows[1]["record"]["record_kind"], "STAT");
    assert_eq!(rows[1]["record"]["form_id"], 0x1234);
    assert_eq!(rows[1]["record"]["record_offset"], plugin_header().len());
    assert_eq!(
        rows[1]["record"]["editor_ids"][0]["decoded_text"],
        "ClutterTestStatic"
    );
    assert_eq!(
        rows[1]["record"]["model_paths"][0]["path"]["raw_path"],
        serde_json::json!(b"Meshes\\Clutter\\Test.NIF".to_vec())
    );
    assert_eq!(
        rows[1]["record"]["model_paths"][0]["path"]["normalized_asset_path"],
        serde_json::json!(b"meshes/clutter/test.nif".to_vec())
    );
    assert_eq!(
        rows[1]["record"]["distant_lod_fields"][0]["shape_status"],
        "four-fixed-260-byte-slots"
    );
    assert_eq!(
        rows[1]["record"]["distant_lod_fields"][0]["levels"][0]["path"]["normalized_asset_path"],
        serde_json::json!(b"meshes/lod/test_lod_0.nif".to_vec())
    );
    assert_eq!(
        rows[1]["record"]["distant_lod_fields"][0]["levels"][0]["path"]["status"],
        "normalized-asset-path"
    );
    assert_eq!(
        rows[1]["record"]["auxiliary_model_fields"][0]["tag"],
        "MODT"
    );
    assert_eq!(rows[2]["type"], "complete");
    assert_eq!(rows[2]["summary"]["distant_lod_paths"], 1);
}

#[test]
fn invalid_model_paths_and_truncated_lod_payloads_remain_findings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Malformed.esp");
    let mut payload = field(b"MODL", b"../escape.nif\0");
    payload.extend(field(b"MNAM", &[0; 259]));
    write_plugin(&path, &payload);
    let mut output = Vec::new();
    let summary = static_models::export(&path, &mut output).unwrap();
    assert_eq!(summary.malformed_model_paths, 1);
    assert_eq!(summary.malformed_lod_shapes, 1);
    let rows: Vec<serde_json::Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(
        rows[1]["record"]["model_paths"][0]["path"]["status"],
        "invalid-asset-path"
    );
    assert_eq!(
        rows[1]["record"]["distant_lod_fields"][0]["shape_status"],
        "unexpected-length"
    );
    assert_eq!(rows[2]["type"], "complete");
}

#[test]
fn malformed_source_never_gets_a_completion_trailer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Broken.esp");
    let mut source = plugin_header();
    source.extend(record(b"STAT", 0x10, &field(b"MODL", b"mesh.nif\0")));
    source.pop();
    fs::write(&path, source).unwrap();
    let mut output = Vec::new();
    assert!(static_models::export(&path, &mut output).is_err());
    assert!(
        !String::from_utf8(output)
            .unwrap()
            .contains("\"type\":\"complete\"")
    );
}
