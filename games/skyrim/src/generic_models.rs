//! Skyrim record-defined generic model attachments outside STAT.
//!
//! Record framing, compression, source hashing, and asset-path normalization
//! come from the pinned shared reader. This adapter only admits record kinds
//! whose Skyrim schema declares the generic model structure.
use crate::{Result, bad, static_models};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self, SelectedEvent},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{BufReader, Cursor, Seek, SeekFrom, Write},
    path::Path,
};

const RECORD_KINDS: [(&[u8; 4], &str); 39] = [
    (b"ACTI", "ACTI"),
    (b"TACT", "TACT"),
    (b"ALCH", "ALCH"),
    (b"AMMO", "AMMO"),
    (b"ANIO", "ANIO"),
    (b"BOOK", "BOOK"),
    (b"CLMT", "CLMT"),
    (b"CONT", "CONT"),
    (b"DOOR", "DOOR"),
    (b"FURN", "FURN"),
    (b"HDPT", "HDPT"),
    (b"MSTT", "MSTT"),
    (b"IDLM", "IDLM"),
    (b"PROJ", "PROJ"),
    (b"HAZD", "HAZD"),
    (b"SLGM", "SLGM"),
    (b"EXPL", "EXPL"),
    (b"BPTD", "BPTD"),
    (b"ADDN", "ADDN"),
    (b"CAMS", "CAMS"),
    (b"IPCT", "IPCT"),
    (b"ARTO", "ARTO"),
    (b"MATO", "MATO"),
    (b"GRAS", "GRAS"),
    (b"INGR", "INGR"),
    (b"KEYM", "KEYM"),
    (b"LIGH", "LIGH"),
    (b"LVLN", "LVLN"),
    (b"MISC", "MISC"),
    (b"APPA", "APPA"),
    (b"QUST", "QUST"),
    (b"RACE", "RACE"),
    (b"SCRL", "SCRL"),
    (b"TREE", "TREE"),
    (b"FLOR", "FLOR"),
    (b"WEAP", "WEAP"),
    (b"WTHR", "WTHR"),
    (b"WRLD", "WRLD"),
    (b"SCOL", "SCOL"),
];
const PATH_EVIDENCE_LIMIT: usize = 4096;

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub records_scanned: u64,
    pub records_by_kind: BTreeMap<&'static str, u64>,
    pub records_without_modl: u64,
    pub records_with_multiple_modl: u64,
    pub model_path_fields: u64,
    pub normalized_model_references: u64,
    pub malformed_model_paths: u64,
}

#[derive(Debug, Serialize)]
struct PathField {
    plugin_file: String,
    record_kind: &'static str,
    form_id: u32,
    record_offset: u64,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    raw_path: Vec<u8>,
    raw_path_truncated: bool,
    terminator_offset: Option<usize>,
    normalized_asset_path: Option<Vec<u8>>,
    status: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceFingerprint {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

fn kind_name(kind: &[u8; 4]) -> Option<&'static str> {
    RECORD_KINDS
        .iter()
        .find_map(|(tag, name)| (kind == *tag).then_some(*name))
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn line_shared(writer: &mut impl Write, value: &impl Serialize) -> fallout_data::Result<()> {
    line(writer, value).map_err(|error| fallout_data::Error::Resolution(error.to_string()))
}

fn path_field(
    plugin_file: &str,
    record_kind: &'static str,
    form_id: u32,
    record_offset: u64,
    field_offset: usize,
    raw: &[u8],
) -> Result<PathField> {
    let terminator = raw.iter().position(|byte| *byte == 0);
    let candidate = terminator.map_or(raw, |at| &raw[..at]);
    let visible = candidate.len().min(PATH_EVIDENCE_LIMIT);
    let raw_path = candidate[..visible].to_vec();
    let raw_path_truncated = visible != candidate.len();
    let nonzero_after_terminator =
        terminator.is_some_and(|at| raw[at + 1..].iter().any(|byte| *byte != 0));
    let (normalized_asset_path, status) = if terminator.is_none() {
        (None, "missing-terminator")
    } else if candidate.is_empty() {
        (None, "empty-path")
    } else if raw_path_truncated {
        (None, "path-evidence-over-4096-bytes")
    } else {
        match AssetPath::new(candidate) {
            Ok(path) if nonzero_after_terminator => (
                Some(path.bytes().to_vec()),
                "nonzero-bytes-after-terminator",
            ),
            Ok(path) => (Some(path.bytes().to_vec()), "normalized-asset-path"),
            Err(_) => (None, "invalid-asset-path"),
        }
    };
    let (_, field_sha256) = digest_reader(&mut Cursor::new(raw))?;
    Ok(PathField {
        plugin_file: plugin_file.to_owned(),
        record_kind,
        form_id,
        record_offset,
        field_payload_offset_in_decoded_record: field_offset + 6,
        field_bytes: raw.len(),
        field_sha256,
        raw_path,
        raw_path_truncated,
        terminator_offset: terminator,
        normalized_asset_path,
        status,
    })
}

fn editor_ids(record: &plugin::Record) -> Result<Vec<Vec<u8>>> {
    let mut result = Vec::new();
    plugin::visit_subrecords(record, "generic-model record", |field| {
        if field.kind == *b"EDID"
            && let Some(end) = field.data.iter().position(|byte| *byte == 0)
            && end > 0
        {
            result.push(field.data[..end.min(PATH_EVIDENCE_LIMIT)].to_vec());
        }
        Ok(())
    })?;
    Ok(result)
}

/// Export Skyrim generic-model fields from schema-allowlisted record kinds.
/// `STAT` is intentionally handled by the richer dedicated static-model adapter.
pub fn export(path: &Path, writer: &mut impl Write) -> Result<Summary> {
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| bad("plugin", 0, "non-Unicode filename"))?;
    plugin_name(file)?;
    let name = path.display().to_string();
    let mut source = BufReader::new(open_source(path)?);
    let (bytes, source_sha256) = digest_reader(&mut source)?;
    source.seek(SeekFrom::Start(0))?;
    let fingerprint = SourceFingerprint {
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
            "identity": "physical Skyrim records whose pinned schema declares the generic model structure; source-owned file-local FormIDs; no override, order, archive winner, or runtime resolution",
            "offset_note": "record_offset addresses the source plugin; field offsets address the decoded record body, including for compressed records",
            "record_kinds": RECORD_KINDS.iter().map(|(_, kind)| *kind).collect::<Vec<_>>(),
            "stat_note": "STAT is exported separately with its fixed MNAM LOD slots"
        }),
    )?;

    let mut summary = Summary::default();
    plugin::visit_selected(
        &mut source,
        bytes,
        &name,
        plugin::Limits::default(),
        |header| kind_name(&header.kind).is_some(),
        |event| {
            let SelectedEvent::Record(record) = event else {
                return Ok(());
            };
            let Some(record_kind) = kind_name(&record.header.kind) else {
                return Ok(());
            };
            summary.records_scanned += 1;
            *summary.records_by_kind.entry(record_kind).or_default() += 1;
            let ids = editor_ids(record)
                .map_err(|error| fallout_data::Error::Resolution(error.to_string()))?;
            let mut model_count = 0u64;
            plugin::visit_subrecords(record, &name, |field| {
                if field.kind != *b"MODL" {
                    return Ok(());
                }
                model_count += 1;
                summary.model_path_fields += 1;
                let path = path_field(
                    file,
                    record_kind,
                    record.header.form_id,
                    record.header.offset,
                    field.payload_offset,
                    field.data,
                )
                .map_err(|error| fallout_data::Error::Resolution(error.to_string()))?;
                if path.status != "normalized-asset-path" {
                    summary.malformed_model_paths += 1;
                }
                let reference = path.normalized_asset_path.as_ref().map(|normalized| {
                    static_models::AssetReference {
                        plugin_file: file.to_owned(),
                        record_kind,
                        form_id: record.header.form_id,
                        record_offset: record.header.offset,
                        source_field: "MODL".into(),
                        field_payload_offset_in_decoded_record: path
                            .field_payload_offset_in_decoded_record,
                        raw_path: path.raw_path.clone(),
                        normalized_asset_path: normalized.clone(),
                        editor_ids: ids.clone(),
                        lookup_candidates: static_models::lookup_candidates(normalized),
                    }
                });
                if let Some(reference) = reference {
                    summary.normalized_model_references += 1;
                    line_shared(
                        writer,
                        &serde_json::json!({"type":"model-reference", "source":reference, "field":path}),
                    )?;
                } else {
                    line_shared(
                        writer,
                        &serde_json::json!({"type":"model-path-finding", "field":path}),
                    )?;
                }
                Ok(())
            })?;
            if model_count == 0 {
                summary.records_without_modl += 1;
            } else if model_count > 1 {
                summary.records_with_multiple_modl += 1;
            }
            Ok(())
        },
    )?;
    line(
        writer,
        &serde_json::json!({"type":"complete", "source":fingerprint, "summary":summary}),
    )?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut value = tag.to_vec();
        value.extend((data.len() as u16).to_le_bytes());
        value.extend(data);
        value
    }

    fn record(tag: &[u8; 4], id: u32, payload: &[u8]) -> Vec<u8> {
        let mut value = tag.to_vec();
        value.extend((payload.len() as u32).to_le_bytes());
        value.extend(0u32.to_le_bytes());
        value.extend(id.to_le_bytes());
        value.extend([0; 8]);
        value.extend(payload);
        value
    }

    #[test]
    fn only_schema_allowlisted_non_stat_generic_model_fields_become_references() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("GenericModels.esp");
        let mut bytes = record(
            b"TES4",
            0,
            &field(b"HEDR", &[0, 0, 0xD8, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0]),
        );
        let mut activator = field(b"EDID", b"AyleidPressurePlate\0");
        activator.extend(field(
            b"MODL",
            b"CreationClub\\_Shared\\Dungeons\\AyleidRuins\\Interior\\Triggers\\ArtRigPressurePlate01.nif\0",
        ));
        bytes.extend(record(b"ACTI", 0x1234, &activator));
        bytes.extend(record(b"CELL", 0x2345, &field(b"MODL", b"Ignored.nif\0")));
        bytes.extend(record(b"STAT", 0x3456, &field(b"MODL", b"Separate.nif\0")));
        fs::write(&path, bytes).unwrap();

        let mut output = Vec::new();
        let summary = export(&path, &mut output).unwrap();
        assert_eq!(summary.records_scanned, 1);
        assert_eq!(summary.records_by_kind.get("ACTI"), Some(&1));
        assert_eq!(summary.model_path_fields, 1);
        assert_eq!(summary.normalized_model_references, 1);
        assert_eq!(summary.malformed_model_paths, 0);
        let rows: Vec<serde_json::Value> = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(rows[1]["type"], "model-reference");
        assert_eq!(rows[1]["source"]["record_kind"], "ACTI");
        assert_eq!(
            rows[1]["source"]["editor_ids"][0],
            serde_json::json!(b"AyleidPressurePlate".to_vec())
        );
        assert_eq!(
            rows[1]["source"]["normalized_asset_path"],
            serde_json::json!(b"creationclub/_shared/dungeons/ayleidruins/interior/triggers/artrigpressureplate01.nif".to_vec())
        );
        assert_eq!(rows.last().unwrap()["type"], "complete");
    }

    #[test]
    fn malformed_generic_model_fields_are_reported_without_successful_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("MalformedModel.esp");
        let mut bytes = record(
            b"TES4",
            0,
            &field(b"HEDR", &[0, 0, 0xD8, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0]),
        );
        bytes.extend(record(b"DOOR", 0x9876, &field(b"MODL", b"missing.nif")));
        fs::write(&path, bytes).unwrap();
        let mut output = Vec::new();
        let summary = export(&path, &mut output).unwrap();
        assert_eq!(summary.model_path_fields, 1);
        assert_eq!(summary.normalized_model_references, 0);
        assert_eq!(summary.malformed_model_paths, 1);
        let rows: Vec<serde_json::Value> = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(rows[1]["type"], "model-path-finding");
        assert_eq!(rows[1]["field"]["status"], "missing-terminator");
        assert_eq!(rows.last().unwrap()["summary"]["malformed_model_paths"], 1);
    }
}
