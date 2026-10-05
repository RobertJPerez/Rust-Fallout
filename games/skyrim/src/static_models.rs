//! Skyrim STAT model-field evidence built on the pinned TES4 record visitor.
//!
//! This records source-owned model and distant-LOD paths. It does not resolve
//! plugin overrides, archive winners, or runtime asset loading.
use crate::{Result, bad};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self, SelectedEvent},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    io::{BufReader, Cursor, Seek, SeekFrom, Write},
    path::Path,
};

const LOD_SLOT_BYTES: usize = 260;
const LOD_LEVELS: [&str; 4] = ["level_0", "level_1", "level_2", "level_3"];
const PATH_EVIDENCE_LIMIT: usize = 4096;

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub stat_records: u64,
    pub editor_id_fields: u64,
    pub malformed_editor_ids: u64,
    pub records_without_modl: u64,
    pub model_path_fields: u64,
    pub auxiliary_model_fields: u64,
    pub distant_lod_fields: u64,
    pub distant_lod_paths: u64,
    pub duplicate_model_fields: u64,
    pub malformed_model_paths: u64,
    pub malformed_lod_shapes: u64,
}

#[derive(Debug, Serialize)]
struct PathEvidence {
    /// The path bytes before the first NUL (or the available field bytes).
    raw_path: Vec<u8>,
    raw_path_truncated: bool,
    terminator_offset: Option<usize>,
    normalized_asset_path: Option<Vec<u8>>,
    lookup_candidates: Vec<LookupCandidate>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct EditorIdEvidence {
    field_header_offset_in_decoded_record: usize,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
    terminator_offset: Option<usize>,
    decoded_text: Option<String>,
    status: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LookupCandidate {
    pub path: Vec<u8>,
    pub basis: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssetReference {
    pub plugin_file: String,
    pub record_kind: &'static str,
    pub form_id: u32,
    pub record_offset: u64,
    pub source_field: String,
    pub field_payload_offset_in_decoded_record: usize,
    pub raw_path: Vec<u8>,
    pub normalized_asset_path: Vec<u8>,
    pub editor_ids: Vec<Vec<u8>>,
    pub lookup_candidates: Vec<LookupCandidate>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceFingerprint {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
struct ModelPathField {
    field_header_offset_in_decoded_record: usize,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    path: PathEvidence,
}

#[derive(Debug, Serialize)]
struct AuxiliaryField {
    tag: &'static str,
    field_header_offset_in_decoded_record: usize,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    interpretation: &'static str,
}

#[derive(Debug, Serialize)]
struct LodLevel {
    level: &'static str,
    field_payload_offset_in_decoded_record: usize,
    slot_bytes: usize,
    slot_sha256: String,
    path: PathEvidence,
}

#[derive(Debug, Serialize)]
struct DistantLodField {
    field_header_offset_in_decoded_record: usize,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    shape_status: &'static str,
    levels: Vec<LodLevel>,
}

#[derive(Debug, Serialize)]
struct StaticRecord {
    record_kind: &'static str,
    form_id: u32,
    record_flags: u32,
    record_offset: u64,
    editor_ids: Vec<EditorIdEvidence>,
    model_paths: Vec<ModelPathField>,
    auxiliary_model_fields: Vec<AuxiliaryField>,
    distant_lod_fields: Vec<DistantLodField>,
    issues: Vec<String>,
}

fn sha256(bytes: &[u8]) -> Result<String> {
    let (_, digest) = digest_reader(&mut Cursor::new(bytes))?;
    Ok(digest)
}

pub(crate) fn lookup_candidates(path: &[u8]) -> Vec<LookupCandidate> {
    let mut candidates = vec![LookupCandidate {
        path: path.to_vec(),
        basis: "field-path-exact",
    }];
    if !path.starts_with(b"meshes/") {
        let mut meshes_rooted = b"meshes/".to_vec();
        meshes_rooted.extend_from_slice(path);
        candidates.push(LookupCandidate {
            path: meshes_rooted,
            basis: "meshes-rooted-candidate",
        });
    }
    candidates
}

fn path_evidence(raw: &[u8], fixed_slot_padding: bool) -> PathEvidence {
    let terminator = raw.iter().position(|byte| *byte == 0);
    let candidate = terminator.map_or(raw, |at| &raw[..at]);
    let visible = candidate.len().min(PATH_EVIDENCE_LIMIT);
    let raw_path = candidate[..visible].to_vec();
    let raw_path_truncated = visible != candidate.len();
    let trailing_nonzero = !fixed_slot_padding
        && terminator.is_some_and(|at| raw[at + 1..].iter().any(|byte| *byte != 0));
    let (normalized_asset_path, status) = if terminator.is_none() {
        (None, "missing-terminator")
    } else if candidate.is_empty() {
        (None, "empty-path")
    } else if raw_path_truncated {
        (None, "path-evidence-over-4096-bytes")
    } else {
        match AssetPath::new(candidate) {
            Ok(path) if trailing_nonzero => (
                Some(path.bytes().to_vec()),
                "nonzero-bytes-after-terminator",
            ),
            Ok(path) => (Some(path.bytes().to_vec()), "normalized-asset-path"),
            Err(_) => (None, "invalid-asset-path"),
        }
    };
    let lookup_candidates = normalized_asset_path
        .as_deref()
        .map(lookup_candidates)
        .unwrap_or_default();
    PathEvidence {
        raw_path,
        raw_path_truncated,
        terminator_offset: terminator,
        normalized_asset_path,
        lookup_candidates,
        status,
    }
}

fn editor_id_evidence(raw: &[u8], field_header_offset: usize) -> Result<EditorIdEvidence> {
    let terminator = raw.iter().position(|byte| *byte == 0);
    let candidate = terminator.map_or(raw, |at| &raw[..at]);
    let visible = candidate.len().min(PATH_EVIDENCE_LIMIT);
    let raw_bytes = candidate[..visible].to_vec();
    let raw_bytes_truncated = visible != candidate.len();
    let trailing_nonzero = terminator.is_some_and(|at| raw[at + 1..].iter().any(|byte| *byte != 0));
    let decoded_text = std::str::from_utf8(&raw_bytes).ok().map(str::to_owned);
    let status = if terminator.is_none() {
        "missing-terminator"
    } else if raw_bytes_truncated {
        "editor-id-over-4096-bytes"
    } else if candidate.is_empty() {
        "empty-editor-id"
    } else if decoded_text.is_none() {
        "non-utf8-editor-id"
    } else if trailing_nonzero {
        "nonzero-bytes-after-terminator"
    } else {
        "terminated-utf8"
    };
    Ok(EditorIdEvidence {
        field_header_offset_in_decoded_record: field_header_offset,
        field_payload_offset_in_decoded_record: field_header_offset + 6,
        field_bytes: raw.len(),
        field_sha256: sha256(raw)?,
        raw_bytes,
        raw_bytes_truncated,
        terminator_offset: terminator,
        decoded_text,
        status,
    })
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn line_shared(writer: &mut impl Write, value: &impl Serialize) -> fallout_data::Result<()> {
    line(writer, value).map_err(|error| fallout_data::Error::Resolution(error.to_string()))
}

/// Export physical STAT record evidence as JSONL, preserving file-local IDs and
/// decoded-record field offsets. Source hashes and a completion trailer make
/// partial output distinguishable from a completed scan.
pub fn export(path: &Path, writer: &mut impl Write) -> Result<Summary> {
    export_with(path, writer, |_| Ok(())).map(|(summary, _)| summary)
}

pub fn export_with(
    path: &Path,
    writer: &mut impl Write,
    mut observe_asset: impl FnMut(AssetReference) -> Result<()>,
) -> Result<(Summary, SourceFingerprint)> {
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| bad("plugin", 0, "non-Unicode filename"))?;
    plugin_name(file)?;
    let name = path.display().to_string();
    let mut source = BufReader::new(open_source(path)?);
    let (bytes, source_sha256) = digest_reader(&mut source)?;
    source.seek(SeekFrom::Start(0))?;
    let source_fingerprint = SourceFingerprint {
        file: file.into(),
        bytes,
        sha256: source_sha256.clone(),
    };
    line(
        writer,
        &serde_json::json!({
            "type": "source",
            "schema_version": 1,
            "file": file,
            "bytes": bytes,
            "sha256": source_sha256,
            "identity": "physical Skyrim STAT records; source-owned file-local FormIDs; no override, order, archive winner, or runtime resolution",
            "offset_note": "record_offset addresses the source plugin; field offsets address the decoded record body, including for compressed records"
        }),
    )?;

    let mut summary = Summary::default();
    plugin::visit_selected(
        &mut source,
        bytes,
        &name,
        plugin::Limits::default(),
        |header| header.kind == *b"STAT",
        |event| {
            let SelectedEvent::Record(record) = event else {
                return Ok(());
            };
            if record.header.kind != *b"STAT" {
                return Ok(());
            }
            summary.stat_records += 1;
            let mut row = StaticRecord {
                record_kind: "STAT",
                form_id: record.header.form_id,
                record_flags: record.header.flags,
                record_offset: record.header.offset,
                editor_ids: Vec::new(),
                model_paths: Vec::new(),
                auxiliary_model_fields: Vec::new(),
                distant_lod_fields: Vec::new(),
                issues: Vec::new(),
            };
            plugin::visit_subrecords(record, &name, |field| {
                let header_offset = field.payload_offset;
                let data_offset = header_offset + 6;
                match &field.kind {
                    b"EDID" => {
                        summary.editor_id_fields += 1;
                        let evidence = editor_id_evidence(field.data, header_offset)
                            .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?;
                        if evidence.status != "terminated-utf8" {
                            summary.malformed_editor_ids += 1;
                            row.issues.push(format!("EDID: {}", evidence.status));
                        }
                        row.editor_ids.push(evidence);
                    }
                    b"MODL" => {
                        summary.model_path_fields += 1;
                        row.model_paths.push(ModelPathField {
                            field_header_offset_in_decoded_record: header_offset,
                            field_payload_offset_in_decoded_record: data_offset,
                            field_bytes: field.data.len(),
                            field_sha256: sha256(field.data)
                                .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?,
                            path: path_evidence(field.data, false),
                        });
                    }
                    b"MODT" | b"MODS" => {
                        summary.auxiliary_model_fields += 1;
                        row.auxiliary_model_fields.push(AuxiliaryField {
                            tag: if field.kind == *b"MODT" { "MODT" } else { "MODS" },
                            field_header_offset_in_decoded_record: header_offset,
                            field_payload_offset_in_decoded_record: data_offset,
                            field_bytes: field.data.len(),
                            field_sha256: sha256(field.data)
                                .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?,
                            interpretation: if field.kind == *b"MODT" {
                                "model metadata bytes retained by source hash and decoded offset; payload semantics unimplemented"
                            } else {
                                "alternate-texture bytes retained by source hash and decoded offset; payload semantics unimplemented"
                            },
                        });
                    }
                    b"MNAM" => {
                        summary.distant_lod_fields += 1;
                        let expected = LOD_SLOT_BYTES * LOD_LEVELS.len();
                        let shape_status = if field.data.len() == expected {
                            "four-fixed-260-byte-slots"
                        } else {
                            summary.malformed_lod_shapes += 1;
                            row.issues.push(format!(
                                "MNAM has {} bytes; schema expects {} bytes for four fixed slots",
                                field.data.len(),
                                expected
                            ));
                            "unexpected-length"
                        };
                        let mut levels = Vec::with_capacity(LOD_LEVELS.len());
                        for (index, level_name) in LOD_LEVELS.iter().enumerate() {
                            let start = index * LOD_SLOT_BYTES;
                            let Some(slot) = field
                                .data
                                .get(start..(start + LOD_SLOT_BYTES).min(field.data.len()))
                            else {
                                break;
                            };
                            if slot.is_empty() {
                                break;
                            }
                            // MNAM reserves 260 bytes per LOD name and its tail is
                            // documented as opaque filler, so only the NUL-terminated
                            // prefix is interpreted as a path.
                            let evidence = path_evidence(slot, true);
                            if evidence.normalized_asset_path.is_some() {
                                summary.distant_lod_paths += 1;
                            }
                            if evidence.status != "normalized-asset-path"
                                && evidence.status != "empty-path"
                            {
                                row.issues
                                    .push(format!("MNAM {level_name} path: {}", evidence.status));
                            }
                            levels.push(LodLevel {
                                level: level_name,
                                field_payload_offset_in_decoded_record: data_offset + start,
                                slot_bytes: slot.len(),
                                slot_sha256: sha256(slot)
                                    .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?,
                                path: evidence,
                            });
                        }
                        row.distant_lod_fields.push(DistantLodField {
                            field_header_offset_in_decoded_record: header_offset,
                            field_payload_offset_in_decoded_record: data_offset,
                            field_bytes: field.data.len(),
                            field_sha256: sha256(field.data)
                                .map_err(|e| fallout_data::Error::Resolution(e.to_string()))?,
                            shape_status,
                            levels,
                        });
                    }
                    _ => {}
                }
                Ok(())
            })?;
            if row.model_paths.is_empty() {
                summary.records_without_modl += 1;
                row.issues.push("STAT record has no MODL field".into());
            }
            if row.model_paths.len() > 1 {
                summary.duplicate_model_fields += 1;
                row.issues.push(format!(
                    "STAT record has {} MODL fields",
                    row.model_paths.len()
                ));
            }
            for model in &row.model_paths {
                if model.path.status != "normalized-asset-path" {
                    summary.malformed_model_paths += 1;
                    row.issues.push(format!("MODL path: {}", model.path.status));
                }
                if let Some(normalized) = &model.path.normalized_asset_path {
                    observe_asset(AssetReference {
                        plugin_file: file.to_owned(),
                        record_kind: "STAT",
                        form_id: record.header.form_id,
                        record_offset: record.header.offset,
                        source_field: "MODL".into(),
                        field_payload_offset_in_decoded_record: model
                            .field_payload_offset_in_decoded_record,
                        raw_path: model.path.raw_path.clone(),
                        normalized_asset_path: normalized.clone(),
                        editor_ids: row
                            .editor_ids
                            .iter()
                            .map(|editor| editor.raw_bytes.clone())
                            .collect(),
                        lookup_candidates: model.path.lookup_candidates.clone(),
                    })
                    .map_err(|error| fallout_data::Error::Resolution(error.to_string()))?;
                }
            }
            for distant in &row.distant_lod_fields {
                for level in &distant.levels {
                    let Some(normalized) = &level.path.normalized_asset_path else {
                        continue;
                    };
                    observe_asset(AssetReference {
                        plugin_file: file.to_owned(),
                        record_kind: "STAT",
                        form_id: record.header.form_id,
                        record_offset: record.header.offset,
                        source_field: format!("MNAM:{}", level.level),
                        field_payload_offset_in_decoded_record: level
                            .field_payload_offset_in_decoded_record,
                        raw_path: level.path.raw_path.clone(),
                        normalized_asset_path: normalized.clone(),
                        editor_ids: row
                            .editor_ids
                            .iter()
                            .map(|editor| editor.raw_bytes.clone())
                            .collect(),
                        lookup_candidates: level.path.lookup_candidates.clone(),
                    })
                    .map_err(|error| fallout_data::Error::Resolution(error.to_string()))?;
                }
            }
            line_shared(writer, &serde_json::json!({"type":"stat", "record":row}))
        },
    )?;
    line(
        writer,
        &serde_json::json!({"type":"complete", "summary":summary}),
    )?;
    Ok((summary, source_fingerprint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_keep_raw_bytes_and_expose_malformed_shapes() {
        let value = path_evidence(b"Meshes\\Clutter\\Test.NIF\0\0", false);
        assert_eq!(value.raw_path, b"Meshes\\Clutter\\Test.NIF");
        assert_eq!(
            value.normalized_asset_path.as_deref(),
            Some(&b"meshes/clutter/test.nif"[..])
        );
        assert_eq!(value.status, "normalized-asset-path");

        let missing = path_evidence(b"meshes/missing-terminator.nif", false);
        assert_eq!(missing.status, "missing-terminator");
        assert_eq!(missing.raw_path, b"meshes/missing-terminator.nif");

        let unsafe_path = path_evidence(b"../outside.nif\0", false);
        assert_eq!(unsafe_path.status, "invalid-asset-path");
        assert!(unsafe_path.normalized_asset_path.is_none());

        let editor = editor_id_evidence(b"TestStatic\0", 12).unwrap();
        assert_eq!(editor.decoded_text.as_deref(), Some("TestStatic"));
        assert_eq!(editor.status, "terminated-utf8");
        let malformed = editor_id_evidence(b"bad-id", 6).unwrap();
        assert_eq!(malformed.status, "missing-terminator");
    }
}
