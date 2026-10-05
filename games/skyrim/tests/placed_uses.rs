use skyrim_prep::placed_uses;
use std::fs;

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((bytes.len() as u16).to_le_bytes());
    out.extend(bytes);
    out
}

fn record(kind: &[u8; 4], id: u32, flags: u32, fields: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((fields.len() as u32).to_le_bytes());
    out.extend(flags.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend([0, 0, 0, 0, 44, 0, 0, 0]);
    out.extend(fields);
    out
}

fn header(masters: &[&str], flags: u32) -> Vec<u8> {
    header_version(masters, flags, 1.7)
}

fn header_version(masters: &[&str], flags: u32, version: f32) -> Vec<u8> {
    let mut hedr = version.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let mut fields = field(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        fields.extend(field(b"MAST", &name));
        fields.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, flags, &fields)
}

fn group(kind: i32, label: u32, body: &[u8]) -> Vec<u8> {
    let mut out = b"GRUP".to_vec();
    out.extend((body.len() as u32 + 24).to_le_bytes());
    out.extend(label.to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend([0; 8]);
    out.extend(body);
    out
}

#[test]
fn matches_placed_refs_by_declared_master_identity_and_keeps_cell_context() {
    let dir = tempfile::tempdir().unwrap();
    let mut target = header(&["Skyrim.esm"], 0);
    let mut target_fields = field(b"EDID", b"ccBGS_ARTrigPressurePlate01\0");
    target_fields.extend(field(
        b"MODL",
        b"meshes\\creationclub\\ayleid\\pressureplate.nif\0",
    ));
    target.extend(record(b"ACTI", 0x0100_3206, 0, &target_fields));
    let target_path = dir.path().join("Update.esm");
    fs::write(&target_path, target).unwrap();

    let mut placed = header(&["Skyrim.esm", "Update.esm"], 0);
    let mut matching = field(b"EDID", b"PlacedTarget\0");
    matching.extend(field(b"NAME", &0x0100_3206u32.to_le_bytes()));
    let mut decoy = field(b"EDID", b"DifferentOriginSameLocalID\0");
    decoy.extend(field(b"NAME", &0x0000_3206u32.to_le_bytes()));
    let mut cell = record(b"CELL", 0x100, 0, &field(b"EDID", b"AyleidCell\0"));
    cell.extend(group(
        6,
        0x100,
        &group(9, 0x100, &record(b"REFR", 0x200, 0, &matching)),
    ));
    cell.extend(record(b"REFR", 0x201, 0, &decoy));
    placed.extend(group(6, 0x100, &cell));
    let placed_path = dir.path().join("Placed.esp");
    fs::write(&placed_path, placed).unwrap();

    let report = placed_uses::inspect(&target_path, 0x0100_3206, &[placed_path]).unwrap();
    assert_eq!(report.target.source_key.origin_plugin, "update.esm");
    assert_eq!(report.target.source_key.local_id, 0x3206);
    assert_eq!(
        report.target.editor_id_fields,
        [b"ccBGS_ARTrigPressurePlate01\0".to_vec()]
    );
    assert_eq!(report.target.model_fields.len(), 1);
    assert!(
        report.target.model_fields[0]
            .normalized_asset_path
            .is_some()
    );
    assert_eq!(report.placements.len(), 1);
    assert_eq!(
        report.placements[0].record.editor_id,
        Some(b"PlacedTarget".to_vec())
    );
    assert_eq!(
        report.placements[0]
            .record
            .containing_cell
            .as_ref()
            .unwrap()
            .local_id,
        0x100
    );
    assert_eq!(report.placements[0].record.containing_world, None);
}

#[test]
fn keeps_light_plugin_fe_name_links_unresolved_without_order() {
    let dir = tempfile::tempdir().unwrap();
    let mut target = header(&["Skyrim.esm"], 0);
    target.extend(record(
        b"ACTI",
        0x0100_3206,
        0,
        &field(b"EDID", b"Target\0"),
    ));
    let target_path = dir.path().join("Update.esm");
    fs::write(&target_path, target).unwrap();

    let mut light = header(&[], 0x200);
    light.extend(record(
        b"REFR",
        0xFE00_08AB,
        0,
        &field(b"NAME", &0xFE00_08AAu32.to_le_bytes()),
    ));
    let light_path = dir.path().join("Light.esl");
    fs::write(&light_path, light).unwrap();

    let report = placed_uses::inspect(&target_path, 0x0100_3206, &[light_path]).unwrap();
    let light_source = report
        .sources
        .iter()
        .find(|source| source.plugin == "Light.esl")
        .unwrap();
    assert_eq!(light_source.unresolved_name_fields, 1);
    assert_eq!(report.unresolved_name_examples.len(), 1);
    assert_eq!(
        report.unresolved_name_examples[0].field_bytes,
        0xFE00_08AAu32.to_le_bytes()
    );
    assert!(report.placements.is_empty());
}

#[test]
fn rejects_plugin_path_escape_and_preserves_malformed_name_fields() {
    assert!(placed_uses::validate_plugin_name("..\\outside.esm").is_err());
    assert!(placed_uses::validate_plugin_name("subdir/Update.esm").is_err());
    assert!(placed_uses::validate_plugin_name("Update.txt").is_err());
    assert_eq!(
        placed_uses::validate_plugin_name("Update.esm").unwrap(),
        "Update.esm"
    );

    let dir = tempfile::tempdir().unwrap();
    let mut target = header(&["Skyrim.esm"], 0);
    target.extend(record(
        b"ACTI",
        0x0100_3206,
        0,
        &field(b"EDID", b"Target\0"),
    ));
    let target_path = dir.path().join("Update.esm");
    fs::write(&target_path, target).unwrap();

    let mut source = header(&["Skyrim.esm", "Update.esm"], 0);
    source.extend(record(
        b"REFR",
        0x0100_1000,
        0,
        &field(b"NAME", &[0x06, 0x32, 0x00]),
    ));
    let source_path = dir.path().join("Malformed.esp");
    fs::write(&source_path, source).unwrap();

    let report = placed_uses::inspect(&target_path, 0x0100_3206, &[source_path]).unwrap();
    let source = report
        .sources
        .iter()
        .find(|source| source.plugin == "Malformed.esp")
        .unwrap();
    assert_eq!(source.malformed_name_fields, 1);
    assert_eq!(report.malformed_name_examples.len(), 1);
    assert_eq!(
        report.malformed_name_examples[0].field_bytes,
        [0x06, 0x32, 0x00]
    );
    assert!(report.placements.is_empty());
}

#[test]
fn accepts_expanded_171_light_record_ids_and_resolves_full_plugin_master_links() {
    let dir = tempfile::tempdir().unwrap();
    let mut target = header(&["Skyrim.esm"], 0);
    target.extend(record(
        b"ACTI",
        0x0100_3206,
        0,
        &field(b"EDID", b"Target\0"),
    ));
    let target_path = dir.path().join("Update.esm");
    fs::write(&target_path, target).unwrap();

    let mut light = header_version(&["Skyrim.esm", "Update.esm"], 0x200, 1.71);
    light.extend(record(
        b"REFR",
        0xFE00_0100,
        0,
        &field(b"NAME", &0x0100_3206u32.to_le_bytes()),
    ));
    let light_path = dir.path().join("Creation.esl");
    fs::write(&light_path, light).unwrap();

    let report = placed_uses::inspect(&target_path, 0x0100_3206, &[light_path]).unwrap();
    assert_eq!(report.placements.len(), 1);
    let placed = &report.placements[0].record;
    assert_eq!(
        placed.source_key.as_ref().unwrap().origin_plugin,
        "creation.esl"
    );
    assert_eq!(placed.source_key.as_ref().unwrap().local_id, 0x100);
    assert_eq!(report.sources[1].header_version_bits, 1.71f32.to_bits());
}
