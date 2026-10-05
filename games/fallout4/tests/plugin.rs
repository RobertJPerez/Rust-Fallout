use fallout4_prep::census;
use std::fs;

fn sub(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut b = kind.to_vec();
    b.extend((data.len() as u16).to_le_bytes());
    b.extend(data);
    b
}
fn record(kind: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = kind.to_vec();
    b.extend((body.len() as u32).to_le_bytes());
    b.extend(flags.to_le_bytes());
    b.extend([0u8; 8]);
    b.extend(131u16.to_le_bytes());
    b.extend([0u8; 2]);
    b.extend(body);
    b
}
#[test]
fn fo4_census_uses_shared_framing_without_nv_identity_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Probe.esl");
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend(1u32.to_le_bytes());
    hedr.extend(0x800u32.to_le_bytes());
    let mut header = sub(b"HEDR", &hedr);
    header.extend(sub(b"MAST", b"Fallout4.esm\0"));
    let mut data = record(b"TES4", 0x201, &header);
    data.extend(record(b"QUST", 0, &sub(b"VMAD", &[6, 0, 2, 0, 0, 0])));
    fs::write(&path, &data).unwrap();
    let report = census::plugin_census(&path).unwrap();
    assert_eq!(report.records, 2);
    assert_eq!(report.record_versions[&131], 2);
    assert_eq!(report.header_flags, 0x201);
    assert_eq!(report.masters, ["Fallout4.esm"]);
    assert_eq!(report.vmad_headers["version=6,object_format=2"], 1);
    assert_eq!(report.subrecord_kinds["QUST/VMAD"], 1);
    data.pop();
    fs::write(&path, &data).unwrap();
    assert!(census::plugin_census(&path).is_err());
}
