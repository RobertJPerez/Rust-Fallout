use serde_json::Value;
use std::{fs, process::Command};
use tempfile::tempdir;

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    let mut data = kind.to_vec();
    data.extend((bytes.len() as u16).to_le_bytes());
    data.extend(bytes);
    data
}

fn header(light: bool, masters: &[&str]) -> Vec<u8> {
    let mut hedr = 1.7f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let mut fields = field(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        fields.extend(field(b"MAST", &name));
        fields.extend(field(b"DATA", &[0; 8]));
    }
    let mut record = b"TES4".to_vec();
    record.extend((fields.len() as u32).to_le_bytes());
    record.extend((if light { 0x200u32 } else { 0 }).to_le_bytes());
    record.extend([0; 4]);
    record.extend([0; 8]);
    record.extend(fields);
    record
}

#[test]
fn map_profile_cli_reads_headers_and_separates_flagged_light_slots() {
    let game = tempdir().unwrap();
    let output_root = tempdir().unwrap();
    let data = game.path().join("Data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("Skyrim.esm"), header(false, &[])).unwrap();
    fs::write(data.join("Light.esp"), header(true, &["Skyrim.esm"])).unwrap();
    fs::write(data.join("SuffixOnly.esl"), header(false, &["Skyrim.esm"])).unwrap();
    fs::write(
        data.join("Patch.esp"),
        header(false, &["Skyrim.esm", "Light.esp"]),
    )
    .unwrap();
    let order = game.path().join("active-order.json");
    fs::write(
        &order,
        br#"{"schema_version":1,"active_plugins":["Skyrim.esm","Light.esp","SuffixOnly.esl","Patch.esp"]}"#,
    )
    .unwrap();
    let report_path = output_root.path().join("profile-map.json");
    let result = Command::new(env!("CARGO_BIN_EXE_skyrim-prep"))
        .args([
            "map-profile",
            "--data",
            data.to_str().unwrap(),
            "--order",
            order.to_str().unwrap(),
            "--output",
            report_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
    let plugins = report["mapping"]["plugins"].as_array().unwrap();
    assert_eq!(plugins[0]["full_slot"], 0);
    assert_eq!(plugins[1]["light_slot"], 0);
    assert_eq!(plugins[2]["light"], false);
    assert_eq!(plugins[2]["esl_extension"], true);
    assert_eq!(plugins[2]["full_slot"], 1);
    assert_eq!(plugins[3]["full_slot"], 2);
    assert!(
        report["mapping"]["status"]
            .as_str()
            .unwrap()
            .contains("not certified")
    );
    assert!(report["plugin_findings"].as_array().unwrap().is_empty());
}
