//! Skyrim TXST source fields and names-only texture dependency lookup.
use crate::{Error, Result, asset_lookup, bad, plugin};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self as framing, SelectedEvent},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Cursor, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const PATH_LIMIT: usize = 1024;
const FIELD_EVIDENCE_LIMIT: usize = 1024;
const ROW_LIMIT: usize = 100_000;
const SUBRECORD_LIMIT: usize = 100_000;

const SLOTS: [(&[u8; 4], &str); 8] = [
    (b"TX00", "diffuse"),
    (b"TX01", "normal_gloss"),
    (b"TX02", "environment_mask_subsurface_tint"),
    (b"TX03", "glow_detail"),
    (b"TX04", "height"),
    (b"TX05", "environment"),
    (b"TX06", "multilayer"),
    (b"TX07", "backlight_mask_specular"),
];

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub plugin_files_scanned: u64,
    pub texture_set_records: u64,
    pub subrecords_scanned: u64,
    pub texture_fields: u64,
    pub empty_texture_slots: u64,
    pub normalized_texture_references: u64,
    pub malformed_texture_paths: u64,
    pub duplicate_texture_slots: u64,
    pub opaque_auxiliary_fields: u64,
    pub unknown_subrecords: u64,
    pub unique_lookup_paths: u64,
    pub references_with_loose_file: u64,
    pub references_with_archive_member: u64,
    pub references_without_candidate_match: u64,
    pub archives_indexed: u64,
    pub archive_index_failures: u64,
    pub archive_hash_only_entries: u64,
    pub archive_invalid_paths: u64,
    pub duplicate_archive_candidate_entries: u64,
}

#[derive(Clone, Debug, Serialize)]
struct SourceFingerprint {
    file: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct PluginScan {
    source: SourceFingerprint,
    records: u64,
    subrecords_scanned: u64,
    texture_fields: u64,
    empty_texture_slots: u64,
    normalized_texture_references: u64,
    malformed_texture_paths: u64,
    duplicate_texture_slots: u64,
    opaque_auxiliary_fields: u64,
    unknown_subrecords: u64,
}

#[derive(Debug, Serialize)]
struct ByteField {
    tag: String,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    raw_bytes: Vec<u8>,
    raw_bytes_truncated: bool,
}

#[derive(Debug, Serialize)]
struct TextureField {
    tag: String,
    slot: &'static str,
    field_payload_offset_in_decoded_record: usize,
    field_bytes: usize,
    field_sha256: String,
    raw_path: Vec<u8>,
    raw_path_truncated: bool,
    terminator_offset: Option<usize>,
    nonzero_bytes_after_terminator: bool,
    normalized_field_path: Option<Vec<u8>>,
    duplicate_slot: bool,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct TextureSetRecord {
    plugin_file: String,
    form_id: u32,
    record_offset: u64,
    record_flags: u32,
    editor_id_fields: Vec<ByteField>,
    texture_fields: Vec<TextureField>,
    opaque_auxiliary_fields: Vec<ByteField>,
    unknown_subrecords: Vec<ByteField>,
}

#[derive(Debug, Serialize)]
struct TextureLookup {
    data_asset_path: Vec<u8>,
    loose_file_status: &'static str,
    archive_matches: Vec<asset_lookup::ArchiveMatch>,
}

fn field_hash(bytes: &[u8]) -> Result<String> {
    Ok(digest_reader(&mut Cursor::new(bytes))?.1)
}

/// Skyrim TXST texture fields are relative to Data\\textures; archive and VFS
/// candidates therefore use one explicit `textures/` root before shared lookup.
fn data_texture_path(field_path: &[u8]) -> Result<AssetPath> {
    let mut path = b"textures/".to_vec();
    path.extend_from_slice(field_path);
    Ok(AssetPath::new(&path)?)
}

fn byte_field(tag: &[u8; 4], offset: usize, data: &[u8]) -> Result<ByteField> {
    let retained = data.len().min(FIELD_EVIDENCE_LIMIT);
    Ok(ByteField {
        tag: framing::signature(*tag),
        field_payload_offset_in_decoded_record: offset,
        field_bytes: data.len(),
        field_sha256: field_hash(data)?,
        raw_bytes: data[..retained].to_vec(),
        raw_bytes_truncated: retained != data.len(),
    })
}

fn slot(tag: &[u8; 4]) -> Option<&'static str> {
    SLOTS
        .iter()
        .find_map(|(known, name)| (tag == *known).then_some(*name))
}

fn texture_field(tag: &[u8; 4], offset: usize, data: &[u8]) -> Result<TextureField> {
    let terminator = data.iter().position(|byte| *byte == 0);
    let path = terminator.map_or(data, |at| &data[..at]);
    let retained = path.len().min(PATH_LIMIT);
    let raw_path = path[..retained].to_vec();
    let raw_path_truncated = retained != path.len();
    let nonzero_bytes_after_terminator =
        terminator.is_some_and(|at| data[at + 1..].iter().any(|byte| *byte != 0));
    let (normalized_field_path, status) = match terminator {
        None => (None, "missing-terminator"),
        Some(_) if path.is_empty() => (None, "empty-slot"),
        Some(_) if raw_path_truncated => (None, "path-evidence-over-4096-bytes"),
        Some(_) => match AssetPath::new(path) {
            Ok(asset) if nonzero_bytes_after_terminator => (
                Some(asset.bytes().to_vec()),
                "nonzero-bytes-after-terminator",
            ),
            Ok(asset) => (Some(asset.bytes().to_vec()), "normalized-asset-path"),
            Err(_) => (None, "invalid-asset-path"),
        },
    };
    Ok(TextureField {
        tag: framing::signature(*tag),
        slot: slot(tag).expect("texture slot tag checked"),
        field_payload_offset_in_decoded_record: offset + 6,
        field_bytes: data.len(),
        field_sha256: field_hash(data)?,
        raw_path,
        raw_path_truncated,
        terminator_offset: terminator,
        nonzero_bytes_after_terminator,
        normalized_field_path,
        duplicate_slot: false,
        status,
    })
}

fn scan_plugin(
    path: &Path,
    subrecord_budget: usize,
) -> Result<(PluginScan, Vec<TextureSetRecord>)> {
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| bad("plugin", 0, "non-Unicode filename"))?;
    plugin_name(file)?;
    let metadata = plugin::inspect(path)?;
    let mut source = BufReader::new(open_source(path)?);
    let (bytes, sha256) = digest_reader(&mut source)?;
    if bytes != metadata.bytes || sha256 != metadata.sha256 {
        return Err(Error::Unsupported(format!(
            "plugin changed between inspect and TXST scan: {}",
            path.display()
        )));
    }
    source.seek(SeekFrom::Start(0))?;

    let mut records = Vec::new();
    let mut scan = PluginScan {
        source: SourceFingerprint {
            file: metadata.file.clone(),
            bytes,
            sha256,
        },
        records: 0,
        subrecords_scanned: 0,
        texture_fields: 0,
        empty_texture_slots: 0,
        normalized_texture_references: 0,
        malformed_texture_paths: 0,
        duplicate_texture_slots: 0,
        opaque_auxiliary_fields: 0,
        unknown_subrecords: 0,
    };
    framing::visit_selected(
        &mut source,
        bytes,
        &metadata.file,
        framing::Limits::default(),
        |header| header.kind == *b"TXST",
        |event| {
            let SelectedEvent::Record(record) = event else {
                return Ok(());
            };
            if record.header.kind != *b"TXST" {
                return Ok(());
            }
            if records.len() >= ROW_LIMIT {
                return Err(shared_error("texture-set record budget exceeded"));
            }
            scan.records += 1;
            let mut result = TextureSetRecord {
                plugin_file: metadata.file.clone(),
                form_id: record.header.form_id,
                record_offset: record.header.offset,
                record_flags: record.header.flags,
                editor_id_fields: Vec::new(),
                texture_fields: Vec::new(),
                opaque_auxiliary_fields: Vec::new(),
                unknown_subrecords: Vec::new(),
            };
            framing::visit_subrecords(record, &metadata.file, |field| {
                scan.subrecords_scanned += 1;
                if scan.subrecords_scanned as usize > subrecord_budget {
                    return Err(shared_error("TXST subrecord budget exceeded"));
                }
                match &field.kind {
                    b"EDID" => result.editor_id_fields.push(
                        byte_field(&field.kind, field.payload_offset, field.data)
                            .map_err(shared_error)?,
                    ),
                    b"OBND" | b"DODT" | b"DNAM" => {
                        scan.opaque_auxiliary_fields += 1;
                        result.opaque_auxiliary_fields.push(
                            byte_field(&field.kind, field.payload_offset, field.data)
                                .map_err(shared_error)?,
                        );
                    }
                    kind if slot(kind).is_some() => {
                        scan.texture_fields += 1;
                        let path = texture_field(kind, field.payload_offset, field.data)
                            .map_err(shared_error)?;
                        match path.status {
                            "empty-slot" => scan.empty_texture_slots += 1,
                            "normalized-asset-path" | "nonzero-bytes-after-terminator" => {
                                scan.normalized_texture_references += 1;
                                if path.nonzero_bytes_after_terminator {
                                    scan.malformed_texture_paths += 1;
                                }
                            }
                            _ => scan.malformed_texture_paths += 1,
                        }
                        result.texture_fields.push(path);
                    }
                    _ => {
                        scan.unknown_subrecords += 1;
                        result.unknown_subrecords.push(
                            byte_field(&field.kind, field.payload_offset, field.data)
                                .map_err(shared_error)?,
                        );
                    }
                }
                Ok(())
            })?;
            let mut counts = BTreeMap::<String, usize>::new();
            for field in &result.texture_fields {
                *counts.entry(field.tag.clone()).or_default() += 1;
            }
            for field in &mut result.texture_fields {
                if counts.get(&field.tag).is_some_and(|count| *count > 1) {
                    field.duplicate_slot = true;
                }
            }
            scan.duplicate_texture_slots += counts
                .values()
                .map(|count| count.saturating_sub(1) as u64)
                .sum::<u64>();
            records.push(result);
            Ok(())
        },
    )?;
    Ok((scan, records))
}

/// Scan one or more physical Skyrim TXST sources and match normalized texture
/// names against loose files and the shared BSA 105 name indexes.
pub fn export_many(
    data_dir: &Path,
    plugin_paths: &[PathBuf],
    writer: &mut impl Write,
) -> Result<Summary> {
    if plugin_paths.is_empty() {
        return Err(Error::Unsupported(
            "at least one plugin source is required".into(),
        ));
    }
    let data_meta = std::fs::symlink_metadata(data_dir)?;
    if data_meta.file_type().is_symlink() || !data_meta.is_dir() {
        return Err(Error::Unsupported(
            "data root must be a regular non-symlink directory".into(),
        ));
    }
    let data_dir = data_dir.canonicalize()?;
    let mut scans = Vec::new();
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    let mut remaining_subrecords = SUBRECORD_LIMIT;
    for input in plugin_paths {
        let metadata = std::fs::symlink_metadata(input)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::Unsupported(format!(
                "plugin input must be a regular non-symlink file: {}",
                input.display()
            )));
        }
        let path = input.canonicalize()?;
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Unsupported("plugin filename is not Unicode".into()))?;
        let normalized_name = plugin_name(file)?;
        if !seen.insert(normalized_name.clone()) {
            return Err(Error::Unsupported(format!(
                "duplicate plugin source name in texture trace: {normalized_name}"
            )));
        }
        let (scan, mut source_records) = scan_plugin(&path, remaining_subrecords)?;
        if records.len().saturating_add(source_records.len()) > ROW_LIMIT {
            return Err(Error::Unsupported(
                "aggregate TXST record budget exceeded".into(),
            ));
        }
        remaining_subrecords -= scan.subrecords_scanned as usize;
        scans.push(scan);
        records.append(&mut source_records);
    }

    let mut requested = BTreeSet::new();
    for record in &records {
        for field in &record.texture_fields {
            if let Some(path) = &field.normalized_field_path {
                requested.insert(data_texture_path(path)?);
            }
        }
    }
    let lookup = asset_lookup::inspect(&data_dir, &requested)?;
    let mut summary = summarize(&scans);
    summary.unique_lookup_paths = requested.len() as u64;
    summary.archives_indexed = lookup.summary.archives_indexed;
    summary.archive_index_failures = lookup.summary.archive_index_failures;
    summary.archive_hash_only_entries = lookup.summary.archive_hash_only_entries;
    summary.archive_invalid_paths = lookup.summary.archive_invalid_paths;
    summary.duplicate_archive_candidate_entries =
        lookup.summary.duplicate_archive_candidate_entries;

    line(
        writer,
        &serde_json::json!({
            "type": "source",
            "schema_version": 1,
            "plugins": scans.iter().map(|scan| &scan.source).collect::<Vec<_>>(),
            "plugin_scans": scans,
            "data_directory": data_dir,
            "identity": "physical TXST texture fields only; plugin master order, overrides, archive winner, and runtime use unresolved",
            "texture_slots": SLOTS.iter().map(|(tag, name)| (framing::signature(**tag), *name)).collect::<Vec<_>>(),
            "normalization": "TXST paths are relative to Data/textures; prepend exactly one textures/ root, then use pinned fallout-data AssetPath for exact loose/BSA lookup",
            "opaque": "OBND, DODT, DNAM and unknown subrecords retain bounded raw bytes and hashes; no pixel, material, or decal semantics"
        }),
    )?;
    for archive in &lookup.archives {
        line(
            writer,
            &serde_json::json!({"type":"archive-index", "archive":archive}),
        )?;
    }
    for record in records {
        line(
            writer,
            &serde_json::json!({"type":"texture-set", "record":&record}),
        )?;
        let Some(source) = scans
            .iter()
            .find(|scan| scan.source.file == record.plugin_file)
        else {
            return Err(Error::Unsupported(
                "TXST source fingerprint disappeared".into(),
            ));
        };
        for field in &record.texture_fields {
            let Some(path) = &field.normalized_field_path else {
                if field.status != "empty-slot" {
                    line(
                        writer,
                        &serde_json::json!({"type":"texture-path-finding", "plugin":source.source, "form_id":record.form_id, "record_offset":record.record_offset, "tag":field.tag, "slot":field.slot, "field":field}),
                    )?;
                }
                continue;
            };
            let data_asset_path = data_texture_path(path)?;
            let lookup_path = data_asset_path.bytes().to_vec();
            let loose = asset_lookup::loose_file_status(&data_dir, &lookup_path);
            let archive_matches = lookup
                .matches
                .get(&lookup_path)
                .cloned()
                .unwrap_or_default();
            if loose == "present" {
                summary.references_with_loose_file += 1;
            }
            if !archive_matches.is_empty() {
                summary.references_with_archive_member += 1;
            }
            if loose != "present" && archive_matches.is_empty() {
                summary.references_without_candidate_match += 1;
            }
            line(
                writer,
                &serde_json::json!({
                    "type":"texture-reference",
                    "plugin":source.source,
                    "record_kind":"TXST",
                    "form_id":record.form_id,
                    "record_offset":record.record_offset,
                    "record_flags":record.record_flags,
                    "editor_id_fields":record.editor_id_fields,
                    "tag":field.tag,
                    "slot":field.slot,
                    "field":field,
                    "lookup":TextureLookup { data_asset_path: lookup_path, loose_file_status: loose, archive_matches }
                }),
            )?;
        }
    }
    line(
        writer,
        &serde_json::json!({
            "type":"complete",
            "summary":summary,
            "archive_index_failures":lookup.archives.iter().filter(|archive| archive.failure.is_some()).map(|archive| (&archive.file,&archive.failure)).collect::<Vec<_>>(),
            "scope":"physical TXST paths and names-only archive matches; no decompression, plugin/load-order resolution, archive precedence, material semantics, or runtime acceptance"
        }),
    )?;
    Ok(summary)
}

pub fn export(data_dir: &Path, plugin_path: &Path, writer: &mut impl Write) -> Result<Summary> {
    export_many(data_dir, &[plugin_path.to_path_buf()], writer)
}

fn summarize(scans: &[PluginScan]) -> Summary {
    Summary {
        plugin_files_scanned: scans.len() as u64,
        texture_set_records: scans.iter().map(|scan| scan.records).sum(),
        subrecords_scanned: scans.iter().map(|scan| scan.subrecords_scanned).sum(),
        texture_fields: scans.iter().map(|scan| scan.texture_fields).sum(),
        empty_texture_slots: scans.iter().map(|scan| scan.empty_texture_slots).sum(),
        normalized_texture_references: scans
            .iter()
            .map(|scan| scan.normalized_texture_references)
            .sum(),
        malformed_texture_paths: scans.iter().map(|scan| scan.malformed_texture_paths).sum(),
        duplicate_texture_slots: scans.iter().map(|scan| scan.duplicate_texture_slots).sum(),
        opaque_auxiliary_fields: scans.iter().map(|scan| scan.opaque_auxiliary_fields).sum(),
        unknown_subrecords: scans.iter().map(|scan| scan.unknown_subrecords).sum(),
        ..Summary::default()
    }
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn shared_error(error: impl std::fmt::Display) -> fallout_data::Error {
    fallout_data::Error::Resolution(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dream_archive::bsa::tes4::Builder;
    use std::fs;

    fn field(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut value = tag.to_vec();
        value.extend((data.len() as u16).to_le_bytes());
        value.extend(data);
        value
    }

    fn record(tag: &[u8; 4], id: u32, fields: &[u8]) -> Vec<u8> {
        let mut value = tag.to_vec();
        value.extend((fields.len() as u32).to_le_bytes());
        value.extend(0u32.to_le_bytes());
        value.extend(id.to_le_bytes());
        value.extend([0, 0, 0, 0, 44, 0, 0, 0]);
        value.extend(fields);
        value
    }

    #[test]
    fn extracts_all_schema_slots_keeps_unknowns_and_matches_loose_and_archive_names() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("Data");
        fs::create_dir(&data).unwrap();
        let plugin_path = data.join("Textures.esp");
        let mut header = 1.7f32.to_le_bytes().to_vec();
        header.extend([0; 8]);
        let mut bytes = record(b"TES4", 0, &field(b"HEDR", &header));
        let mut fields = field(b"EDID", b"DemoTextureSet\0");
        fields.extend(field(b"OBND", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
        fields.extend(field(b"TX00", b"demo\\diffuse.dds\0"));
        fields.extend(field(b"TX00", b"demo/diffuse.dds\0"));
        fields.extend(field(b"TX01", b"demo/normal.dds\0"));
        fields.extend(field(b"TX02", b"\0"));
        fields.extend(field(b"TX03", b"demo/no-terminator.dds"));
        fields.extend(field(b"TX06", b"../escape.dds\0"));
        fields.extend(field(b"DODT", &[0xAA, 0xBB, 0xCC]));
        fields.extend(field(b"DNAM", &[0x34, 0x12]));
        fields.extend(field(b"ZZZZ", &[0xFE, 0xED]));
        bytes.extend(record(b"TXST", 0x1234, &fields));
        fs::write(&plugin_path, bytes).unwrap();

        let loose_dir = data.join("textures").join("demo");
        fs::create_dir_all(&loose_dir).unwrap();
        fs::write(loose_dir.join("normal.dds"), b"synthetic loose texture").unwrap();
        let mut archive = Builder::skyrim_se();
        archive.set_compressed(true);
        archive
            .add_bytes(b"textures/demo/diffuse.dds", b"synthetic archive texture")
            .unwrap();
        archive
            .write_path(data.join("Skyrim - Textures0.bsa"))
            .unwrap();

        let mut output = Vec::new();
        let summary = export(&data, &plugin_path, &mut output).unwrap();
        assert_eq!(summary.plugin_files_scanned, 1);
        assert_eq!(summary.texture_set_records, 1);
        assert_eq!(summary.subrecords_scanned, 11);
        assert_eq!(summary.texture_fields, 6);
        assert_eq!(summary.empty_texture_slots, 1);
        assert_eq!(summary.normalized_texture_references, 3);
        assert_eq!(summary.malformed_texture_paths, 2);
        assert_eq!(summary.duplicate_texture_slots, 1);
        assert_eq!(summary.opaque_auxiliary_fields, 3);
        assert_eq!(summary.unknown_subrecords, 1);
        assert_eq!(summary.archives_indexed, 1);
        assert_eq!(summary.references_with_loose_file, 1);
        assert_eq!(summary.references_with_archive_member, 2);
        assert_eq!(summary.references_without_candidate_match, 0);

        let rows = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let record = rows
            .iter()
            .find(|row| row["type"] == "texture-set")
            .unwrap();
        assert_eq!(
            record["record"]["editor_id_fields"][0]["raw_bytes"],
            serde_json::json!(b"DemoTextureSet\0".to_vec())
        );
        assert_eq!(
            record["record"]["opaque_auxiliary_fields"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(record["record"]["unknown_subrecords"][0]["tag"], "ZZZZ");
        let duplicate_slots = record["record"]["texture_fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|field| field["duplicate_slot"] == true)
            .count();
        assert_eq!(duplicate_slots, 2);
        assert!(rows.iter().any(|row| row["type"] == "texture-path-finding"
            && row["field"]["status"] == "missing-terminator"));
        assert!(rows.iter().any(|row| row["type"] == "texture-path-finding"
            && row["field"]["status"] == "invalid-asset-path"));
        assert_eq!(rows.last().unwrap()["type"], "complete");
        let reference = rows
            .iter()
            .find(|row| row["type"] == "texture-reference")
            .unwrap();
        assert_eq!(
            reference["lookup"]["data_asset_path"],
            serde_json::json!(b"textures/demo/diffuse.dds".to_vec())
        );
    }

    #[test]
    fn subrecord_budget_failure_stays_visible() {
        let root = tempfile::tempdir().unwrap();
        let plugin_path = root.path().join("Budget.esp");
        let mut header = 1.7f32.to_le_bytes().to_vec();
        header.extend([0; 8]);
        let mut bytes = record(b"TES4", 0, &field(b"HEDR", &header));
        let mut fields = field(b"EDID", b"BudgetSet\0");
        fields.extend(field(b"TX00", b"demo/diffuse.dds\0"));
        bytes.extend(record(b"TXST", 0x1234, &fields));
        fs::write(&plugin_path, bytes).unwrap();

        let error = scan_plugin(&plugin_path, 1).unwrap_err().to_string();
        assert!(error.contains("TXST subrecord budget exceeded"));
    }
}
