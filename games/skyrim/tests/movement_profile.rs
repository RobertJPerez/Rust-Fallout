use skyrim_prep::movement_profile;
use std::fs;

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((bytes.len() as u16).to_le_bytes());
    out.extend(bytes);
    out
}

fn record(kind: &[u8; 4], id: u32, fields: &[u8]) -> Vec<u8> {
    let mut out = kind.to_vec();
    out.extend((fields.len() as u32).to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(id.to_le_bytes());
    out.extend([0, 0, 0, 0, 44, 0, 0, 0]);
    out.extend(fields);
    out
}

fn plugin_header(masters: &[&str]) -> Vec<u8> {
    let mut hedr = 1.7f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let mut fields = field(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        fields.extend(field(b"MAST", &name));
        fields.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, &fields)
}

fn floats(bits: &[u32]) -> Vec<u8> {
    bits.iter().flat_map(|bits| bits.to_le_bytes()).collect()
}

fn json_lines(bytes: &[u8]) -> Vec<serde_json::Value> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}

fn find_record<'a>(
    rows: &'a [serde_json::Value],
    kind: &str,
    plugin: &str,
    form_id: u64,
) -> &'a serde_json::Value {
    rows.iter()
        .find(|row| {
            row["type"] == "movement-record"
                && row["record"]["record_kind"] == kind
                && row["record"]["plugin_file"] == plugin
                && row["record"]["form_id"] == form_id
        })
        .unwrap()
}

#[test]
fn traces_player_race_movement_candidates_without_selecting_overrides() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("Base.esm");
    let patch = root.path().join("Patch.esp");
    let mut base_bytes = plugin_header(&[]);

    let movt_speeds = floats(&[
        0x8000_0000,
        0x3F80_0000,
        0x4000_0000,
        0x4040_0000,
        0x4080_0000,
        0x40A0_0000,
        0x40C0_0000,
        0x40E0_0000,
        0x4100_0000,
        0x4110_0000,
        0x7FC1_2345,
    ]);
    let mut movement_fields = field(b"EDID", b"DefaultMovement\0");
    movement_fields.extend(field(b"MNAM", b"Default\0"));
    movement_fields.extend(field(b"SPED", &movt_speeds));
    movement_fields.extend(field(b"INAM", &floats(&[1, 2, 3])));
    movement_fields.extend(field(b"ZZZZ", &[0xAA, 0xBB, 0xCC]));
    base_bytes.extend(record(b"MOVT", 0x100, &movement_fields));

    let mut race_fields = field(b"EDID", b"NordRace\0");
    race_fields.extend(field(b"WKMV", &0x100u32.to_le_bytes()));
    race_fields.extend(field(b"RNMV", &0x100u32.to_le_bytes()));
    race_fields.extend(field(b"MTYP", &0x100u32.to_le_bytes()));
    race_fields.extend(field(b"SPED", &floats(&[7; 11])));
    race_fields.extend(field(b"ZZZZ", &[0xDD]));
    base_bytes.extend(record(b"RACE", 0x200, &race_fields));

    let mut player_fields = field(b"EDID", b"Player\0");
    player_fields.extend(field(b"RNAM", &0x200u32.to_le_bytes()));
    base_bytes.extend(record(b"NPC_", 0x300, &player_fields));
    fs::write(&base, base_bytes).unwrap();

    let mut patch_bytes = plugin_header(&["Base.esm"]);
    patch_bytes.extend(record(b"MOVT", 0x100, &movement_fields));
    patch_bytes.extend(record(b"RACE", 0x200, &race_fields));
    let inherited_player_fields = field(b"RNAM", &0x200u32.to_le_bytes());
    patch_bytes.extend(record(b"NPC_", 0x300, &inherited_player_fields));
    fs::write(&patch, patch_bytes).unwrap();

    let mut output = Vec::new();
    let summary =
        movement_profile::export_many(root.path(), &[base.clone(), patch.clone()], &mut output)
            .unwrap();
    let rows = json_lines(&output);

    assert_eq!(summary.player_npc_records, 2);
    assert_eq!(summary.race_records, 2);
    assert_eq!(summary.movement_records, 2);
    assert_eq!(summary.player_race_links, 2);
    assert_eq!(summary.race_movement_links, 6);
    assert_eq!(summary.links_without_scanned_target_candidates, 0);
    assert_eq!(summary.links_unresolved_without_profile, 0);
    assert_eq!(summary.links_with_multiple_candidates, 8);

    let player = find_record(&rows, "NPC_", "Base.esm", 0x300);
    let race_link = player["record"]["form_links"][0].clone();
    assert_eq!(race_link["target_kind"], "RACE");
    assert_eq!(race_link["scanned_target_record_count"], 2);
    assert_eq!(race_link["status"], "scanned-target-candidates");
    let inherited_player = find_record(&rows, "NPC_", "Patch.esp", 0x300);
    assert_eq!(
        inherited_player["record"]["player_record_selection"],
        "same-source-key-as-player-edid"
    );
    assert_eq!(
        inherited_player["record"]["editor_id_fields"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let race = find_record(&rows, "RACE", "Base.esm", 0x200);
    assert_eq!(
        race["record"]["editor_id_fields"][0]["raw_bytes"],
        serde_json::to_value(b"NordRace\0".to_vec()).unwrap()
    );
    let walk_link = race["record"]["form_links"]
        .as_array()
        .unwrap()
        .iter()
        .find(|link| link["tag"] == "WKMV")
        .unwrap();
    assert_eq!(walk_link["role"], "walk");
    assert_eq!(walk_link["scanned_target_record_count"], 2);
    let speed_override = race["record"]["float_fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["role"] == "race-movement-speed-overrides")
        .unwrap();
    assert_eq!(speed_override["slots"].as_array().unwrap().len(), 11);
    assert_eq!(speed_override["slots"][10]["name"], "unknown");

    let movement = find_record(&rows, "MOVT", "Base.esm", 0x100);
    let speeds = movement["record"]["float_fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["role"] == "movement-default-speeds")
        .unwrap();
    assert_eq!(speeds["payload_bytes"], 44);
    assert_eq!(speeds["slots"][0]["raw_bits_hex"], "0x80000000");
    assert_eq!(speeds["slots"][10]["ieee754_class"], "nan");
    assert_eq!(speeds["slots"][10]["finite_value"], serde_json::Value::Null);
    let opaque = movement["record"]["opaque_fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["tag"] == "ZZZZ")
        .unwrap();
    assert_eq!(opaque["classification"], "outside-movement-scope");
    assert_eq!(opaque["payload_bytes"], 3);
    assert_eq!(rows.last().unwrap()["type"], "complete");
    assert!(
        rows[0]["interpretation"]
            .as_str()
            .unwrap()
            .contains("no load-order winner")
    );

    // The fixture and its paths are synthetic; no installed game files are
    // used in this test.
    assert!(base.is_file());
}

#[test]
fn malformed_movement_fields_remain_findings_with_a_completion_row() {
    let root = tempfile::tempdir().unwrap();
    let plugin = root.path().join("Malformed.esm");
    let mut bytes = plugin_header(&[]);
    bytes.extend(record(b"MOVT", 0x100, &field(b"SPED", &[0x11; 40])));
    let mut race = field(b"WKMV", &[0, 1, 2]);
    race.extend(field(b"SPED", &[0x22; 43]));
    bytes.extend(record(b"RACE", 0x200, &race));
    bytes.extend(record(b"NPC_", 0x300, &{
        let mut fields = field(b"EDID", b"Player\0");
        fields.extend(field(b"RNAM", &0x200u32.to_le_bytes()));
        fields
    }));
    fs::write(&plugin, bytes).unwrap();

    let mut output = Vec::new();
    let summary = movement_profile::export_many(root.path(), &[plugin], &mut output).unwrap();
    let rows = json_lines(&output);
    assert_eq!(summary.malformed_link_fields, 1);
    assert_eq!(summary.malformed_float_fields, 2);
    assert_eq!(rows.last().unwrap()["type"], "complete");
    let race = find_record(&rows, "RACE", "Malformed.esm", 0x200);
    let opaque = race["record"]["opaque_fields"].as_array().unwrap();
    assert!(opaque.iter().any(|field| {
        field["tag"] == "WKMV" && field["classification"] == "malformed-known-form-link"
    }));
    assert!(opaque.iter().any(|field| {
        field["tag"] == "SPED" && field["classification"] == "malformed-known-float-field"
    }));
}
