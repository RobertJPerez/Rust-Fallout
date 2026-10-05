use skyrim_prep::{plugin, trace};
use std::{fs, path::Path};

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
fn header(masters: &[&str]) -> Vec<u8> {
    let mut hedr = 1.7f32.to_le_bytes().to_vec();
    hedr.extend([0; 8]);
    let mut fields = field(b"HEDR", &hedr);
    for master in masters {
        let mut name = master.as_bytes().to_vec();
        name.push(0);
        fields.extend(field(b"MAST", &name));
        fields.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &fields)
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
fn script() -> Vec<u8> {
    let mut out = vec![5, 0, 2, 0, 1, 0, 7, 0];
    out.extend(b"Missing");
    out.extend([0, 0, 0]);
    field(b"VMAD", &out)
}
fn census(root: &Path, files: &[&str]) -> std::path::PathBuf {
    let plugins: Vec<_> = files
        .iter()
        .map(|name| {
            let p = plugin::inspect(&root.join(name)).unwrap();
            serde_json::json!({"file": name, "bytes": p.bytes, "sha256": p.sha256,
            "masters": ["Tampered.esm"]}) // Must not be trusted by the trace.
        })
        .collect();
    let path = root.join("census.json");
    fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2, "data": root, "plugins": plugins,
            "script_availability": [{"path": b"scripts/missing.pex", "containers": []}]
        }))
        .unwrap(),
    )
    .unwrap();
    path
}

#[test]
fn source_identity_joins_master_overrides_but_does_not_adopt_noncanonical_fallback() {
    let a = trace::source_key("Base.esm", &[], 0x1234).unwrap();
    assert_eq!(
        a,
        trace::source_key("Patch.esp", &["BASE.ESM".into()], 0x1234).unwrap()
    );
    assert_ne!(a, trace::source_key("Other.esm", &[], 0x1234).unwrap());
    assert_eq!(
        trace::source_key("Patch.esp", &["Base.esm".into()], 0x02001234).unwrap(),
        None
    );
    assert!(
        trace::source_key("Patch.esp", &["Base.esm".into()], 0x01001234)
            .unwrap()
            .is_some()
    );
    assert_eq!(trace::source_key("Base.esm", &[], 0).unwrap(), None);
}

#[test]
fn light_plugin_owned_records_are_file_local_but_fe_links_need_load_order() {
    let root = tempfile::tempdir().unwrap();
    let mut light = header(&[]);
    light[8..12].copy_from_slice(&0x200u32.to_le_bytes());
    let cell_id = 0xFE00_0800u32;
    let npc_id = 0xFE00_08AAu32;
    let mut cell_fields = field(b"EDID", b"LightCell\0");
    cell_fields.extend(field(b"DATA", &[0, 0]));
    let cell = record(b"CELL", cell_id, 0, &cell_fields);
    let mut placed = field(b"EDID", b"LightPlaced\0");
    placed.extend(field(b"NAME", &npc_id.to_le_bytes()));
    placed.extend(script());
    let placement = record(b"ACHR", 0xFE00_08AB, 0, &placed);
    light.extend(group(6, cell_id, &cell));
    light.extend(group(6, cell_id, &group(9, cell_id, &placement)));
    fs::write(root.path().join("Light.esl"), light).unwrap();

    let input = census(root.path(), &["Light.esl"]);
    let report = trace::inspect(&input, |_| {}).unwrap();
    let owner = &report.attachments[0].record;
    assert_eq!(owner.source_plugin, "Light.esl");
    assert_eq!(
        owner.source_key.as_ref().unwrap().origin_plugin,
        "light.esl"
    );
    assert_eq!(owner.source_key.as_ref().unwrap().local_id, 0x8AB);
    assert_eq!(
        owner.containing_cell.as_ref().unwrap().origin_plugin,
        "light.esl"
    );
    assert_eq!(owner.containing_cell.as_ref().unwrap().local_id, 0x800);
    // The same FE bits in NAME refer through an active light slot, which this
    // source-only pass deliberately does not synthesize.
    assert_eq!(report.unresolved_link_indices, 1);
    assert_eq!(trace::source_key("Light.esl", &[], npc_id).unwrap(), None);
    assert_eq!(
        trace::source_record_key("Light.esl", &[], npc_id, true, 1.7f32.to_bits())
            .unwrap()
            .unwrap()
            .local_id,
        0x8AA
    );
}

#[test]
fn trace_preserves_all_candidates_and_distinguishes_source_flags_from_activation() {
    let root = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    let mut fields = field(b"EDID", b"MissingOwner\0");
    fields.extend(script());
    base.extend(record(b"NPC_", 0x1234, 0, &fields));
    let mut cell = field(b"EDID", b"TestCell\0");
    cell.extend(field(b"DATA", &[1, 1])); // Preserve the second Skyrim flag byte.
    base.extend(record(b"CELL", 0x100, 0, &cell));
    let mut placed = field(b"EDID", b"Placed\0");
    placed.extend(field(b"NAME", &0x1234u32.to_le_bytes()));
    base.extend(group(
        6,
        0x100,
        &group(9, 0x100, &record(b"ACHR", 0x200, 0x800, &placed)),
    ));
    // A sibling outside the cell group must not inherit its ancestry.
    base.extend(record(b"ACHR", 0x201, 0, &placed));
    fs::write(root.path().join("Base.esm"), &base).unwrap();
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(
        b"NPC_",
        0x1234,
        0,
        &field(b"EDID", b"OverrideWithoutVMAD\0"),
    ));
    patch.extend(group(6, 0x100, &record(b"ACHR", 0x01000202, 0x20, &placed)));
    fs::write(root.path().join("Patch.esp"), patch).unwrap();
    let mut other = header(&[]);
    other.extend(record(
        b"NPC_",
        0x1234,
        0,
        &field(b"EDID", b"DifferentOrigin\0"),
    ));
    other.extend(record(b"ACHR", 0x203, 0, &placed));
    fs::write(root.path().join("Other.esm"), other).unwrap();
    let input = census(root.path(), &["Base.esm", "Patch.esp", "Other.esm"]);
    let report = trace::inspect(&input, |_| {}).unwrap();
    assert_eq!(report.attachments.len(), 1);
    assert_eq!(report.definition_candidates.len(), 2);
    assert!(report.definition_candidates[0].vmad_present);
    assert!(!report.definition_candidates[1].vmad_present);
    assert_eq!(report.direct_references.len(), 3);
    let inside = &report.direct_references[0].record;
    assert_eq!(inside.initially_disabled, Some(true));
    assert_eq!(
        inside.containing_cell.as_ref().unwrap().origin_plugin,
        "base.esm"
    );
    assert_eq!(inside.containing_cell.as_ref().unwrap().local_id, 0x100);
    assert!(report.direct_references[1].record.containing_cell.is_none());
    assert!(report.direct_references[2].record.deleted);
    assert_eq!(report.containing_cell_candidates.len(), 1);
    assert_eq!(
        report.containing_cell_candidates[0].cell_flags_raw,
        Some(vec![1, 1])
    );
    assert_eq!(report.unresolved_link_indices, 0);
    assert_eq!(report.vmad_failures, 0);
    // A changed source cannot be joined against stale census evidence.
    base.extend(record(b"MISC", 0xFFF, 0, &[]));
    fs::write(root.path().join("Base.esm"), base).unwrap();
    assert!(
        trace::inspect(&input, |_| {})
            .unwrap_err()
            .to_string()
            .contains("changed since census")
    );
}

#[test]
fn noncanonical_record_and_link_indices_stay_unresolved() {
    let root = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(record(b"NPC_", 0x01001234, 0, &script()));
    base.extend(record(
        b"ACHR",
        0x200,
        0,
        &field(b"NAME", &0x01001234u32.to_le_bytes()),
    ));
    fs::write(root.path().join("Base.esm"), base).unwrap();
    let input = census(root.path(), &["Base.esm"]);
    let report = trace::inspect(&input, |_| {}).unwrap();
    assert_eq!(report.attachments[0].record.header.form_id, 0x01001234);
    assert_eq!(report.unresolved_selected_record_keys, 1);
    assert_eq!(report.unresolved_link_indices, 1);
    assert!(report.definition_candidates.is_empty());
    assert!(report.direct_references.is_empty());
}

#[test]
fn incomplete_vmad_and_unsafe_census_inputs_cannot_silently_pass() {
    let root = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(record(
        b"NPC_",
        0x1234,
        0,
        &field(b"VMAD", &[9, 0, 2, 0, 0, 0]),
    ));
    fs::write(root.path().join("Base.esm"), base).unwrap();
    let input = census(root.path(), &["Base.esm"]);
    assert_eq!(trace::inspect(&input, |_| {}).unwrap().vmad_failures, 1);
    assert_eq!(
        trace::inspect(&input, |_| {})
            .unwrap()
            .unlocated_script_paths,
        vec![b"scripts/missing.pex".to_vec()]
    );
    let mut json: serde_json::Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    json["plugins"][0]["file"] = "../outside.esm".into();
    fs::write(&input, serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(trace::inspect(&input, |_| {}).is_err());
}

#[test]
fn effect_edges_keep_multiplicity_and_enable_parent_bytes_are_preserved() {
    let root = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(record(b"MGEF", 0x1234, 0, &script()));
    let mut effects = field(b"EFID", &0x1234u32.to_le_bytes());
    effects.extend(field(b"EFID", &0x1234u32.to_le_bytes()));
    base.extend(record(b"SPEL", 0x2222, 0, &effects));
    base.extend(record(b"ACTI", 0x789, 0, &[]));
    let mut target = field(b"NAME", &0x789u32.to_le_bytes());
    target.extend(script());
    base.extend(record(b"REFR", 0x456, 0, &target));
    let mut parent = 0x456u32.to_le_bytes().to_vec();
    parent.extend([3, 0xAA, 0xBB, 0xCC]);
    let mut child = field(b"NAME", &0x789u32.to_le_bytes());
    child.extend(field(b"XESP", &parent));
    base.extend(record(b"REFR", 0x600, 0x800, &child));
    fs::write(root.path().join("Base.esm"), &base).unwrap();
    let input = census(root.path(), &["Base.esm"]);
    let report = trace::inspect(&input, |_| {}).unwrap();
    assert_eq!(report.direct_references.len(), 3);
    assert_eq!(report.direct_references[0].field, "EFID");
    assert_eq!(report.direct_references[1].field, "EFID");
    assert_ne!(
        report.direct_references[0].field_offset,
        report.direct_references[1].field_offset
    );
    assert_eq!(report.direct_references[2].field, "XESP");
    assert_eq!(report.direct_references[2].field_bytes, parent);
    assert_eq!(report.direct_references[2].target_raw, 0x456);
    assert!(report.unlocated_script_paths.is_empty());
    base.extend(record(b"SPEL", 0x4444, 0, &field(b"EFID", &[0; 8])));
    fs::write(root.path().join("Base.esm"), base).unwrap();
    let input = census(root.path(), &["Base.esm"]);
    assert!(
        trace::inspect(&input, |_| {})
            .unwrap_err()
            .to_string()
            .contains("link field size")
    );
}

#[test]
fn missing_effect_trace_walks_spells_templates_leveled_lists_and_placed_actors() {
    let root = tempfile::tempdir().unwrap();
    let mut base = header(&[]);
    base.extend(record(b"MGEF", 0x100, 0, &script()));
    base.extend(record(
        b"SPEL",
        0x200,
        0,
        &field(b"EFID", &0x100u32.to_le_bytes()),
    ));

    let mut actor = field(b"EDID", b"SpellActor\0");
    actor.extend(field(b"SPLO", &0x200u32.to_le_bytes()));
    base.extend(record(b"NPC_", 0x300, 0, &actor));

    let mut child = field(b"EDID", b"TemplateChild\0");
    child.extend(field(b"TPLT", &0x300u32.to_le_bytes()));
    base.extend(record(b"NPC_", 0x301, 0, &child));

    let mut leveled_spell = 5u16.to_le_bytes().to_vec();
    leveled_spell.extend([0, 0]);
    leveled_spell.extend(0x200u32.to_le_bytes());
    leveled_spell.extend(1u16.to_le_bytes());
    leveled_spell.extend(0u16.to_le_bytes());
    base.extend(record(b"LVSP", 0x400, 0, &field(b"LVLO", &leveled_spell)));
    base.extend(record(
        b"NPC_",
        0x302,
        0,
        &field(b"SPLO", &0x400u32.to_le_bytes()),
    ));

    let mut entry = 1u16.to_le_bytes().to_vec();
    entry.extend([0, 0]);
    entry.extend(0x301u32.to_le_bytes());
    entry.extend(1u16.to_le_bytes());
    entry.extend(0u16.to_le_bytes());
    base.extend(record(b"LVLN", 0x500, 0, &field(b"LVLO", &entry)));
    let mut parent_entry = 1u16.to_le_bytes().to_vec();
    parent_entry.extend([0, 0]);
    parent_entry.extend(0x500u32.to_le_bytes());
    parent_entry.extend(1u16.to_le_bytes());
    parent_entry.extend(0u16.to_le_bytes());
    base.extend(record(b"LVLN", 0x501, 0, &field(b"LVLO", &parent_entry)));
    // This source selector cannot be assigned to a full plugin or an ESL
    // without its header; the connected edge is retained with explicit identity
    // failure rather than dropped from the path.
    base.extend(record(
        b"NPC_",
        0xFE00_0555,
        0,
        &field(b"SPLO", &0x200u32.to_le_bytes()),
    ));
    base.extend(record(
        b"ACHR",
        0x600,
        0,
        &field(b"NAME", &0x501u32.to_le_bytes()),
    ));
    // An unsupported SPLO layout remains visible instead of becoming a no-op.
    base.extend(record(b"NPC_", 0x999, 0, &field(b"SPLO", &[1, 2, 3])));

    fs::write(root.path().join("Base.esm"), &base).unwrap();
    let input = census(root.path(), &["Base.esm"]);
    let report = trace::inspect(&input, |_| {}).unwrap();
    let paths = report.actor_paths;
    assert_eq!(paths.missing_script_effects.len(), 1);
    assert_eq!(paths.spell_candidates.len(), 1);
    assert_eq!(paths.spell_candidates[0].header.form_id, 0x200);
    assert_eq!(
        paths.links.len(),
        8,
        "{:?}",
        paths
            .links
            .iter()
            .map(|edge| (
                edge.record.header.form_id,
                edge.field.as_str(),
                edge.target_raw
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(paths.actor_definitions.len(), 7);
    assert_eq!(paths.unresolved_source_record_keys, 1);
    assert!(
        paths
            .actor_definitions
            .iter()
            .any(|row| row.header.form_id == 0x600)
    );
    assert!(paths.links.iter().any(|edge| {
        edge.record.header.form_id == 0x301
            && edge.field == "TPLT"
            && edge.target_raw == 0x300
            && edge.field_bytes == 0x300u32.to_le_bytes()
    }));
    assert!(paths.links.iter().any(|edge| {
        edge.record.header.form_id == 0x400
            && edge.field == "LVLO"
            && edge.target_raw == 0x200
            && edge.field_bytes == leveled_spell
    }));
    assert_eq!(paths.unresolved_links.len(), 1);
    assert_eq!(
        paths.unresolved_links[0].reason,
        "unsupported-subrecord-size"
    );
    assert_eq!(paths.unresolved_links[0].field_bytes, [1, 2, 3]);
}
