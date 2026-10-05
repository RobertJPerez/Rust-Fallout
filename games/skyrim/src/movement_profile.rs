//! Skyrim player/race/movement record evidence. This remains a source graph:
//! it does not select winning overrides or claim locomotion behavior.
use crate::{Error, Result, plugin, trace};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self as framing, SelectedEvent},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Cursor, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const RECORD_LIMIT: usize = 20_000;
const SUBRECORD_LIMIT: usize = 1_000_000;
const FIELD_TAG_LIMIT: usize = 2_048;
const FIELD_LENGTH_LIMIT: usize = 4_096;
const RAW_PREVIEW_LIMIT: usize = 32;
const TEXT_LIMIT: usize = 1_024;

const RACE_MOVEMENT_DEFAULTS: [(&[u8; 4], &str); 6] = [
    (b"WKMV", "walk"),
    (b"RNMV", "run"),
    (b"SWMV", "swim"),
    (b"FLMV", "fly"),
    (b"SNMV", "sneak"),
    (b"SPMV", "sprint"),
];

const MOVT_SPEED_FIELDS: [&str; 11] = [
    "left_walk",
    "left_run",
    "right_walk",
    "right_run",
    "forward_walk",
    "forward_run",
    "back_walk",
    "back_run",
    "rotate_in_place_walk",
    "rotate_in_place_run",
    "rotate_while_moving_run",
];

const RACE_SPEED_FIELDS: [&str; 11] = [
    "left_walk",
    "left_run",
    "right_walk",
    "right_run",
    "forward_walk",
    "forward_run",
    "back_walk",
    "back_run",
    "rotate_walk",
    "rotate_run",
    "unknown",
];

const MOVT_THRESHOLD_FIELDS: [&str; 3] = ["directional", "movement_speed", "rotation_speed"];

fn movt_speed_field_names(record_version: u16) -> &'static [&'static str] {
    if record_version >= 28 {
        &MOVT_SPEED_FIELDS
    } else {
        &MOVT_SPEED_FIELDS[..10]
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub plugin_files_scanned: u64,
    pub npc_records_scanned: u64,
    pub player_npc_records: u64,
    pub race_records: u64,
    pub movement_records: u64,
    pub subrecords_scanned: u64,
    pub player_race_links: u64,
    pub race_movement_links: u64,
    pub race_movement_override_fields: u64,
    pub movement_speed_fields: u64,
    pub race_movement_speed_fields: u64,
    pub animation_threshold_fields: u64,
    pub malformed_link_fields: u64,
    pub malformed_float_fields: u64,
    pub links_with_scanned_target_candidates: u64,
    pub links_without_scanned_target_candidates: u64,
    pub links_unresolved_without_profile: u64,
    pub null_links: u64,
    pub links_with_multiple_candidates: u64,
    pub opaque_fields_encountered: u64,
    pub record_versions: BTreeMap<String, BTreeMap<u16, u64>>,
    pub field_shapes: BTreeMap<String, BTreeMap<String, FieldShape>>,
}

#[derive(Debug, Default, Serialize)]
pub struct FieldShape {
    pub occurrences: u64,
    pub payload_length_counts: BTreeMap<usize, u64>,
}

#[derive(Debug, Serialize)]
struct SourceInfo {
    file: String,
    bytes: u64,
    sha256: String,
    masters: Vec<String>,
    light: bool,
    header_version_bits: u32,
    explicit_player_editor_id_rows_seen: u64,
    races_seen: u64,
    movement_records_seen: u64,
}

#[derive(Debug, Serialize)]
struct MovementRecord {
    record_kind: String,
    plugin_file: String,
    form_id: u32,
    source_key: Option<trace::SourceKey>,
    record_version: u16,
    record_offset: u64,
    record_flags: u32,
    player_editor_id_match: bool,
    player_record_selection: &'static str,
    #[serde(skip)]
    malformed_link_fields: u64,
    editor_id_fields: Vec<TextField>,
    movement_name_fields: Vec<TextField>,
    form_links: Vec<FormLink>,
    float_fields: Vec<FloatField>,
    opaque_fields: Vec<OpaqueField>,
}

#[derive(Debug, Serialize)]
struct TextField {
    tag: String,
    payload_offset: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    null_terminated: bool,
}

#[derive(Debug, Serialize)]
struct FormLink {
    tag: String,
    role: String,
    target_kind: String,
    payload_offset: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_form_id: u32,
    source_key: Option<trace::SourceKey>,
    status: &'static str,
    scanned_target_record_count: usize,
}

#[derive(Debug, Serialize)]
struct FloatField {
    tag: String,
    role: String,
    payload_offset: usize,
    payload_bytes: usize,
    sha256: String,
    status: &'static str,
    slots: Vec<FloatSlot>,
}

#[derive(Debug, Serialize)]
struct FloatSlot {
    name: String,
    raw_bits: u32,
    raw_bits_hex: String,
    ieee754_class: &'static str,
    finite_value: Option<f32>,
}

#[derive(Debug, Serialize)]
struct OpaqueField {
    tag: String,
    classification: &'static str,
    payload_offset: usize,
    payload_bytes: usize,
    sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
}

#[derive(Debug, Clone)]
struct PluginMetadata {
    file: String,
    bytes: u64,
    sha256: String,
    masters: Vec<String>,
    light: bool,
    header_version_bits: u32,
}

fn shared(error: impl std::fmt::Display) -> fallout_data::Error {
    fallout_data::Error::Resolution(error.to_string())
}

fn field_hash(data: &[u8]) -> Result<String> {
    Ok(digest_reader(&mut Cursor::new(data))?.1)
}

fn raw_preview(data: &[u8], limit: usize) -> (Vec<u8>, bool) {
    let end = data.len().min(limit);
    (data[..end].to_vec(), end != data.len())
}

fn text_field(tag: &[u8; 4], payload_offset: usize, data: &[u8]) -> Result<TextField> {
    let (raw_bytes, raw_bytes_truncated) = raw_preview(data, TEXT_LIMIT);
    Ok(TextField {
        tag: framing::signature(*tag),
        payload_offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes,
        raw_bytes_truncated,
        null_terminated: data.ends_with(&[0]),
    })
}

fn is_player_editor_id(data: &[u8]) -> bool {
    data.strip_suffix(&[0])
        .is_some_and(|text| text == b"Player")
}

fn float_slot(name: &str, raw_bits: u32) -> FloatSlot {
    let value = f32::from_bits(raw_bits);
    FloatSlot {
        name: name.into(),
        raw_bits,
        raw_bits_hex: format!("0x{raw_bits:08X}"),
        ieee754_class: if value.is_nan() {
            "nan"
        } else if value.is_infinite() {
            "infinite"
        } else {
            "finite"
        },
        finite_value: value.is_finite().then_some(value),
    }
}

fn float_field(
    tag: &[u8; 4],
    role: &str,
    payload_offset: usize,
    data: &[u8],
    expected_names: &[&str],
) -> Result<FloatField> {
    let bytes = expected_names.len() * 4;
    let status = if data.len() == bytes {
        "decoded-raw-ieee754-slots"
    } else {
        "unsupported-payload-length"
    };
    let slots = if data.len() == bytes {
        expected_names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let at = index * 4;
                let bits = u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
                float_slot(name, bits)
            })
            .collect()
    } else {
        Vec::new()
    };
    Ok(FloatField {
        tag: framing::signature(*tag),
        role: role.into(),
        payload_offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        status,
        slots,
    })
}

fn target_for(kind: &[u8; 4], tag: &[u8; 4]) -> Option<(&'static str, &'static str)> {
    if kind == b"NPC_" && tag == b"RNAM" {
        return Some(("RACE", "player-base-race"));
    }
    if kind == b"RACE" {
        if let Some((_, role)) = RACE_MOVEMENT_DEFAULTS
            .iter()
            .find(|(known_tag, _)| *known_tag == tag)
        {
            return Some(("MOVT", *role));
        }
        if tag == b"MTYP" {
            return Some(("MOVT", "race-movement-type-entry"));
        }
    }
    None
}

fn source_link_status(
    plugin_file: &str,
    masters: &[String],
    raw: u32,
) -> (Option<trace::SourceKey>, &'static str) {
    if raw == 0 {
        return (None, "null-form-id");
    }
    if raw >> 24 == 0xFE {
        return (None, "light-reference-needs-active-profile");
    }
    match trace::source_key(plugin_file, masters, raw) {
        Ok(Some(key)) => (Some(key), "source-key-resolved"),
        Ok(None) => (None, "master-index-out-of-range"),
        Err(_) => (None, "invalid-source-plugin-name"),
    }
}

fn track_shape(
    summary: &mut Summary,
    record_kind: &str,
    tag: &[u8; 4],
    payload_len: usize,
) -> Result<()> {
    let fields = summary.field_shapes.entry(record_kind.into()).or_default();
    let tag_name = framing::signature(*tag);
    if !fields.contains_key(&tag_name) && fields.len() >= FIELD_TAG_LIMIT {
        return Err(Error::Unsupported(
            "movement profile field-tag budget exceeded".into(),
        ));
    }
    let shape = fields.entry(tag_name).or_default();
    if !shape.payload_length_counts.contains_key(&payload_len)
        && shape.payload_length_counts.len() >= FIELD_LENGTH_LIMIT
    {
        return Err(Error::Unsupported(
            "movement profile field-length budget exceeded".into(),
        ));
    }
    shape.occurrences += 1;
    *shape.payload_length_counts.entry(payload_len).or_default() += 1;
    Ok(())
}

fn collect_record(
    record: &framing::Record,
    metadata: &PluginMetadata,
    summary: &mut Summary,
    visited_subrecords: &mut usize,
) -> Result<Option<MovementRecord>> {
    let kind = record.header.kind;
    let kind_name = framing::signature(kind);
    let is_npc = kind == *b"NPC_";
    let is_race = kind == *b"RACE";
    let is_movement = kind == *b"MOVT";
    let source_key = trace::source_record_key(
        &metadata.file,
        &metadata.masters,
        record.header.form_id,
        metadata.light,
        metadata.header_version_bits,
    )
    .map_err(shared)?;
    let mut editor_id_fields = Vec::new();
    let mut movement_name_fields = Vec::new();
    let mut player_editor_id_match = false;
    let mut record_malformed_link_fields = 0;
    let mut form_links = Vec::new();
    let mut float_fields = Vec::new();
    let mut opaque_fields = Vec::new();

    framing::visit_subrecords(record, &metadata.file, |field| {
        if *visited_subrecords >= SUBRECORD_LIMIT {
            return Err(shared("movement profile subrecord budget exceeded"));
        }
        *visited_subrecords += 1;
        summary.subrecords_scanned += 1;
        track_shape(summary, &kind_name, &field.kind, field.data.len()).map_err(shared)?;

        if is_npc && field.kind == *b"EDID" {
            player_editor_id_match |= is_player_editor_id(field.data);
            editor_id_fields
                .push(text_field(&field.kind, field.payload_offset, field.data).map_err(shared)?);
            return Ok(());
        }
        if is_npc
            && field.kind == *b"RNAM"
            && let Some((target_kind, role)) = target_for(&kind, &field.kind)
        {
            if field.data.len() == 4 {
                let raw = u32::from_le_bytes(field.data.try_into().unwrap());
                let (key, status) = source_link_status(&metadata.file, &metadata.masters, raw);
                form_links.push(FormLink {
                    tag: framing::signature(field.kind),
                    role: role.into(),
                    target_kind: target_kind.into(),
                    payload_offset: field.payload_offset,
                    payload_bytes: field.data.len(),
                    sha256: field_hash(field.data).map_err(shared)?,
                    raw_bytes: field.data.to_vec(),
                    raw_form_id: raw,
                    source_key: key,
                    status,
                    scanned_target_record_count: 0,
                });
                return Ok(());
            }
            opaque_fields.push(
                opaque_field(
                    &field.kind,
                    field.payload_offset,
                    field.data,
                    "malformed-known-form-link",
                )
                .map_err(shared)?,
            );
            if is_npc {
                record_malformed_link_fields += 1;
            } else {
                summary.malformed_link_fields += 1;
            }
            return Ok(());
        }

        if is_race && let Some((target_kind, role)) = target_for(&kind, &field.kind) {
            if field.data.len() == 4 {
                let raw = u32::from_le_bytes(field.data.try_into().unwrap());
                let (key, status) = source_link_status(&metadata.file, &metadata.masters, raw);
                form_links.push(FormLink {
                    tag: framing::signature(field.kind),
                    role: role.into(),
                    target_kind: target_kind.into(),
                    payload_offset: field.payload_offset,
                    payload_bytes: field.data.len(),
                    sha256: field_hash(field.data).map_err(shared)?,
                    raw_bytes: field.data.to_vec(),
                    raw_form_id: raw,
                    source_key: key,
                    status,
                    scanned_target_record_count: 0,
                });
                return Ok(());
            }
            opaque_fields.push(
                opaque_field(
                    &field.kind,
                    field.payload_offset,
                    field.data,
                    "malformed-known-form-link",
                )
                .map_err(shared)?,
            );
            summary.malformed_link_fields += 1;
            return Ok(());
        }

        if is_race && field.kind == *b"EDID" {
            editor_id_fields
                .push(text_field(&field.kind, field.payload_offset, field.data).map_err(shared)?);
            return Ok(());
        }

        if is_movement && field.kind == *b"EDID" {
            editor_id_fields
                .push(text_field(&field.kind, field.payload_offset, field.data).map_err(shared)?);
            return Ok(());
        }
        if is_movement && field.kind == *b"MNAM" {
            movement_name_fields
                .push(text_field(&field.kind, field.payload_offset, field.data).map_err(shared)?);
            return Ok(());
        }
        let float_spec = if is_movement && field.kind == *b"SPED" {
            Some((
                "movement-default-speeds",
                movt_speed_field_names(record.header.version),
            ))
        } else if is_movement && field.kind == *b"INAM" {
            Some(("animation-change-thresholds", &MOVT_THRESHOLD_FIELDS[..]))
        } else if is_race && field.kind == *b"SPED" {
            Some(("race-movement-speed-overrides", &RACE_SPEED_FIELDS[..]))
        } else {
            None
        };
        if let Some((role, selected_names)) = float_spec {
            let parsed = float_field(
                &field.kind,
                role,
                field.payload_offset,
                field.data,
                selected_names,
            )
            .map_err(shared)?;
            if parsed.status == "unsupported-payload-length" {
                summary.malformed_float_fields += 1;
                opaque_fields.push(
                    opaque_field(
                        &field.kind,
                        field.payload_offset,
                        field.data,
                        "malformed-known-float-field",
                    )
                    .map_err(shared)?,
                );
            } else {
                match (is_movement, field.kind) {
                    (true, [b'S', b'P', b'E', b'D']) => summary.movement_speed_fields += 1,
                    (true, [b'I', b'N', b'A', b'M']) => summary.animation_threshold_fields += 1,
                    (false, [b'S', b'P', b'E', b'D']) => summary.race_movement_speed_fields += 1,
                    _ => {}
                }
                float_fields.push(parsed);
            }
            return Ok(());
        }

        // MTYP is a Skyrim RACE-specific MOVT reference used by movement
        // entries; preserve its order and every physical candidate.
        if is_race && field.kind == *b"MTYP" {
            if field.data.len() == 4 {
                let raw = u32::from_le_bytes(field.data.try_into().unwrap());
                let (key, status) = source_link_status(&metadata.file, &metadata.masters, raw);
                form_links.push(FormLink {
                    tag: framing::signature(field.kind),
                    role: "race-movement-type-entry".into(),
                    target_kind: "MOVT".into(),
                    payload_offset: field.payload_offset,
                    payload_bytes: field.data.len(),
                    sha256: field_hash(field.data).map_err(shared)?,
                    raw_bytes: field.data.to_vec(),
                    raw_form_id: raw,
                    source_key: key,
                    status,
                    scanned_target_record_count: 0,
                });
                summary.race_movement_override_fields += 1;
            } else {
                opaque_fields.push(
                    opaque_field(
                        &field.kind,
                        field.payload_offset,
                        field.data,
                        "malformed-known-form-link",
                    )
                    .map_err(shared)?,
                );
                summary.malformed_link_fields += 1;
            }
            return Ok(());
        }

        if is_npc {
            // NPC_ is scanned only to find Player by EDID and to retain the
            // race link on physical overrides that inherit their EDID.
            return Ok(());
        }
        opaque_fields.push(
            opaque_field(
                &field.kind,
                field.payload_offset,
                field.data,
                "outside-movement-scope",
            )
            .map_err(shared)?,
        );
        summary.opaque_fields_encountered += 1;
        Ok(())
    })
    .map_err(shared)?;

    if is_npc {
        summary.npc_records_scanned += 1;
    } else if is_race {
        summary.race_records += 1;
    } else if is_movement {
        summary.movement_records += 1;
    } else {
        return Ok(None);
    }

    *summary
        .record_versions
        .entry(kind_name.clone())
        .or_default()
        .entry(record.header.version)
        .or_default() += 1;

    Ok(Some(MovementRecord {
        record_kind: kind_name,
        plugin_file: metadata.file.clone(),
        form_id: record.header.form_id,
        source_key,
        record_version: record.header.version,
        record_offset: record.header.offset,
        record_flags: record.header.flags,
        player_editor_id_match,
        player_record_selection: if player_editor_id_match {
            "editor-id-player"
        } else {
            "source-key-candidate-pending"
        },
        malformed_link_fields: record_malformed_link_fields,
        editor_id_fields,
        movement_name_fields,
        form_links,
        float_fields,
        opaque_fields,
    }))
}

fn opaque_field(
    tag: &[u8; 4],
    payload_offset: usize,
    data: &[u8],
    classification: &'static str,
) -> Result<OpaqueField> {
    let (raw_bytes, raw_bytes_truncated) = raw_preview(data, RAW_PREVIEW_LIMIT);
    Ok(OpaqueField {
        tag: framing::signature(*tag),
        classification,
        payload_offset,
        payload_bytes: data.len(),
        sha256: field_hash(data)?,
        raw_bytes,
        raw_bytes_truncated,
    })
}

fn plugin_metadata(path: &Path) -> Result<PluginMetadata> {
    let report = plugin::inspect(path)?;
    Ok(PluginMetadata {
        file: report.file,
        bytes: report.bytes,
        sha256: report.sha256,
        masters: report.masters,
        light: report.light,
        header_version_bits: report.header_version_bits,
    })
}

fn scan_plugin(
    path: &Path,
    metadata: &PluginMetadata,
    summary: &mut Summary,
    rows: &mut Vec<MovementRecord>,
    visited_subrecords: &mut usize,
) -> Result<SourceInfo> {
    let mut reader = BufReader::new(open_source(path)?);
    let (bytes, sha256) = digest_reader(&mut reader)?;
    if bytes != metadata.bytes || sha256 != metadata.sha256 {
        return Err(Error::Unsupported(format!(
            "source changed since movement profile metadata pass: {}",
            metadata.file
        )));
    }
    reader.seek(SeekFrom::Start(0))?;
    let row_start = rows.len();
    framing::visit_selected(
        &mut reader,
        bytes,
        &metadata.file,
        framing::Limits::default(),
        |header| matches!(&header.kind, b"NPC_" | b"RACE" | b"MOVT"),
        |event| {
            let SelectedEvent::Record(record) = event else {
                return Ok(());
            };
            if rows.len() >= RECORD_LIMIT {
                return Err(shared("movement profile selected-record budget exceeded"));
            }
            if let Some(row) =
                collect_record(record, metadata, summary, visited_subrecords).map_err(shared)?
            {
                rows.push(row);
            }
            Ok(())
        },
    )?;
    summary.plugin_files_scanned += 1;
    let player_records_seen = rows[row_start..]
        .iter()
        .filter(|row| row.record_kind == "NPC_" && row.player_editor_id_match)
        .count() as u64;
    let races_seen = rows[row_start..]
        .iter()
        .filter(|row| row.record_kind == "RACE")
        .count() as u64;
    let movement_records_seen = rows[row_start..]
        .iter()
        .filter(|row| row.record_kind == "MOVT")
        .count() as u64;
    Ok(SourceInfo {
        file: metadata.file.clone(),
        bytes,
        sha256,
        masters: metadata.masters.clone(),
        light: metadata.light,
        header_version_bits: metadata.header_version_bits,
        explicit_player_editor_id_rows_seen: player_records_seen,
        races_seen,
        movement_records_seen,
    })
}

pub fn export_many(
    data_dir: &Path,
    plugin_paths: &[PathBuf],
    mut writer: impl Write,
) -> Result<Summary> {
    if plugin_paths.is_empty() {
        return Err(Error::Unsupported(
            "movement profile needs at least one plugin".into(),
        ));
    }
    let mut metadata = Vec::with_capacity(plugin_paths.len());
    let mut seen = BTreeSet::new();
    for path in plugin_paths {
        let item = plugin_metadata(path)?;
        let normalized = plugin_name(&item.file)?;
        if !seen.insert(normalized.clone()) {
            return Err(Error::Unsupported(format!(
                "duplicate plugin source name in movement profile: {normalized}"
            )));
        }
        metadata.push((path.clone(), item));
    }

    let mut summary = Summary::default();
    let mut rows = Vec::new();
    let mut sources = Vec::with_capacity(plugin_paths.len());
    let mut visited_subrecords = 0;
    for (path, item) in &metadata {
        sources.push(scan_plugin(
            path,
            item,
            &mut summary,
            &mut rows,
            &mut visited_subrecords,
        )?);
    }

    let player_source_keys: BTreeSet<_> = rows
        .iter()
        .filter(|row| row.record_kind == "NPC_" && row.player_editor_id_match)
        .filter_map(|row| row.source_key.clone())
        .collect();
    rows.retain_mut(|row| {
        if row.record_kind != "NPC_" {
            return true;
        }
        if row.player_editor_id_match {
            row.player_record_selection = "editor-id-player";
            return true;
        }
        if row
            .source_key
            .as_ref()
            .is_some_and(|key| player_source_keys.contains(key))
        {
            row.player_record_selection = "same-source-key-as-player-edid";
            true
        } else {
            false
        }
    });
    summary.player_npc_records = rows.iter().filter(|row| row.record_kind == "NPC_").count() as u64;
    summary.malformed_link_fields += rows
        .iter()
        .filter(|row| row.record_kind == "NPC_")
        .map(|row| row.malformed_link_fields)
        .sum::<u64>();

    let mut candidates = BTreeMap::<(String, trace::SourceKey), usize>::new();
    for row in &rows {
        if let Some(key) = &row.source_key {
            *candidates
                .entry((row.record_kind.clone(), key.clone()))
                .or_default() += 1;
        }
    }
    for row in &mut rows {
        for link in &mut row.form_links {
            if let Some(key) = &link.source_key {
                link.scanned_target_record_count = candidates
                    .get(&(link.target_kind.clone(), key.clone()))
                    .copied()
                    .unwrap_or_default();
                if link.scanned_target_record_count == 0 {
                    link.status = "source-key-resolved-no-scanned-target";
                    summary.links_without_scanned_target_candidates += 1;
                } else {
                    link.status = "scanned-target-candidates";
                    summary.links_with_scanned_target_candidates += 1;
                    if link.scanned_target_record_count > 1 {
                        summary.links_with_multiple_candidates += 1;
                    }
                }
            } else {
                match link.status {
                    "null-form-id" => summary.null_links += 1,
                    "light-reference-needs-active-profile" => {
                        summary.links_unresolved_without_profile += 1
                    }
                    _ => {}
                }
            }
            match link.tag.as_str() {
                "RNAM" => summary.player_race_links += 1,
                "WKMV" | "RNMV" | "SWMV" | "FLMV" | "SNMV" | "SPMV" | "MTYP" => {
                    summary.race_movement_links += 1
                }
                _ => {}
            }
        }
    }

    line(
        &mut writer,
        &serde_json::json!({
            "type": "source",
            "schema_version": 1,
            "data_directory": data_dir,
            "plugins": sources,
            "record_scope": ["NPC_ records named Player and same-source-key physical overrides", "RACE", "MOVT"],
            "link_scope": "NPC_ RNAM -> RACE; RACE WKMV/RNMV/SWMV/FLMV/SNMV/SPMV and MTYP -> MOVT",
            "identity": "physical records use the Skyrim source-record identity; ordinary FormIDs resolve through declared masters; cross-file FE references remain profile-dependent; all scanned physical candidates are retained",
            "float_scope": "MOVT SPED uses ten slots before record version 28 and eleven slots from version 28; MOVT INAM has three slots; RACE SPED has eleven slots; IEEE-754 raw bits are retained, angle values are not converted, and unexpected lengths remain malformed fields",
            "interpretation": "source values and record links only; NPC_ Player selection requires an exact NUL-terminated EDID Player in at least one scanned physical version, and all same-source-key versions are retained; no load-order winner, unit conversion, movement equation, or retail gameplay behavior is selected",
            "limits": {"selected_rows": RECORD_LIMIT, "visited_subrecords": SUBRECORD_LIMIT, "field_tags_per_record_kind": FIELD_TAG_LIMIT, "payload_lengths_per_tag": FIELD_LENGTH_LIMIT, "retained_raw_preview_bytes": RAW_PREVIEW_LIMIT, "retained_text_bytes": TEXT_LIMIT}
        }),
    )?;
    for row in rows {
        line(
            &mut writer,
            &serde_json::json!({"type":"movement-record", "record":row}),
        )?;
    }
    line(
        &mut writer,
        &serde_json::json!({
            "type": "complete",
            "summary": summary,
            "scope": "bounded Skyrim Player NPC to Race to Movement Type source evidence; no winning override, active light-plugin profile, movement units, engine formula, or gameplay acceptance"
        }),
    )?;
    Ok(summary)
}

fn line(writer: &mut impl Write, value: &serde_json::Value) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movt_float_layout_tracks_record_version_and_preserves_exact_bits() {
        let mut extended = Vec::new();
        for bits in [
            0x8000_0000u32,
            0x3F80_0000,
            0x7FC1_2345,
            0x7F80_0000,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
        ] {
            extended.extend_from_slice(&bits.to_le_bytes());
        }
        assert_eq!(movt_speed_field_names(27).len(), 10);
        assert_eq!(movt_speed_field_names(28).len(), 11);
        let field = float_field(
            b"SPED",
            "movement-default-speeds",
            0,
            &extended,
            movt_speed_field_names(28),
        )
        .unwrap();
        assert_eq!(field.status, "decoded-raw-ieee754-slots");
        assert_eq!(field.slots.len(), 11);
        assert_eq!(field.slots[0].raw_bits_hex, "0x80000000");
        assert_eq!(field.slots[0].finite_value.unwrap().to_bits(), 0x8000_0000);
        assert_eq!(field.slots[2].ieee754_class, "nan");
        assert_eq!(field.slots[2].finite_value, None);
        assert_eq!(field.slots[3].ieee754_class, "infinite");
        assert_eq!(field.slots[3].finite_value, None);

        let legacy = vec![0u8; 40];
        let field = float_field(
            b"SPED",
            "movement-default-speeds",
            0,
            &legacy,
            movt_speed_field_names(27),
        )
        .unwrap();
        assert_eq!(field.slots.len(), 10);
        let wrong = float_field(
            b"SPED",
            "movement-default-speeds",
            0,
            &legacy,
            movt_speed_field_names(28),
        )
        .unwrap();
        assert_eq!(wrong.status, "unsupported-payload-length");
        assert!(wrong.slots.is_empty());
    }

    #[test]
    fn player_editor_id_is_exact_and_null_terminated_or_plain() {
        assert!(is_player_editor_id(b"Player\0"));
        assert!(!is_player_editor_id(b"Player"));
        assert!(!is_player_editor_id(b"PlayerRace\0"));
        assert!(!is_player_editor_id(b"player\0"));
        assert!(!is_player_editor_id(b"Player\0junk"));
    }

    #[test]
    fn light_formid_needs_profile_and_bad_master_index_stays_distinct() {
        assert_eq!(
            source_link_status("Patch.esp", &[], 0),
            (None, "null-form-id")
        );
        assert_eq!(
            source_link_status("Patch.esp", &[], 0xFE00_0900),
            (None, "light-reference-needs-active-profile")
        );
        assert_eq!(
            source_link_status("Patch.esp", &[], 0x0200_0900),
            (None, "master-index-out-of-range")
        );
        assert_eq!(
            source_link_status("Patch.esp", &[], 0x0000_0900).1,
            "source-key-resolved"
        );
    }
}
