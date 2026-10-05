//! Cross-check source-owned STAT model references against local loose files and
//! installed BSA names, while retaining every match instead of choosing winners.
use crate::{Result, asset_lookup, static_models};
use fallout_data::vfs::AssetPath;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub plugin_files_scanned: u64,
    pub source_stat_records: u64,
    pub source_path_findings: u64,
    pub source_editor_id_findings: u64,
    pub source_asset_references: u64,
    pub unique_lookup_candidates: u64,
    pub references_with_loose_file: u64,
    pub references_with_archive_member: u64,
    pub references_without_candidate_match: u64,
    pub archives_indexed: u64,
    pub archive_index_failures: u64,
    pub archive_hash_only_entries: u64,
    pub archive_invalid_paths: u64,
    pub duplicate_archive_candidate_entries: u64,
}

#[derive(Debug, Serialize)]
struct CandidateTrace {
    path: Vec<u8>,
    basis: &'static str,
    loose_file_status: &'static str,
    archive_matches: Vec<asset_lookup::ArchiveMatch>,
}

#[derive(Debug, Serialize)]
struct AssetTrace {
    source: static_models::AssetReference,
    candidates: Vec<CandidateTrace>,
}

#[derive(Debug, Serialize)]
struct PluginModelScan {
    source: static_models::SourceFingerprint,
    summary: static_models::Summary,
}

fn line(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    Ok(())
}

/// Produce JSONL source references and archive/loose-file candidates. BSA
/// contents remain read-only and only their names-only indexes are visited.
pub fn export(data_dir: &Path, plugin_path: &Path, writer: &mut impl Write) -> Result<Summary> {
    let plugins = [plugin_path.to_path_buf()];
    export_many(data_dir, &plugins, writer)
}

/// Trace one or more plugin sources while walking and hashing each BSA index
/// only once. Each reference retains its owning plugin filename and file-local ID.
pub fn export_many(
    data_dir: &Path,
    plugin_paths: &[PathBuf],
    writer: &mut impl Write,
) -> Result<Summary> {
    if plugin_paths.is_empty() {
        return Err(crate::Error::Unsupported(
            "at least one plugin source is required".into(),
        ));
    }
    let data_meta = fs::symlink_metadata(data_dir)?;
    if data_meta.file_type().is_symlink() || !data_meta.is_dir() {
        return Err(crate::Error::Unsupported(
            "data root must be a regular non-symlink directory".into(),
        ));
    }
    let data_dir = data_dir.canonicalize()?;
    let mut asset_references = Vec::new();
    let mut plugin_scans = Vec::with_capacity(plugin_paths.len());
    let mut seen_plugins = BTreeSet::new();
    for input_path in plugin_paths {
        let plugin_meta = fs::symlink_metadata(input_path)?;
        if plugin_meta.file_type().is_symlink() || !plugin_meta.is_file() {
            return Err(crate::Error::Unsupported(format!(
                "plugin input must be a regular non-symlink file: {}",
                input_path.display()
            )));
        }
        let plugin_path = input_path.canonicalize()?;
        let plugin_name = plugin_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| crate::Error::Unsupported("plugin filename is not Unicode".into()))?
            .to_ascii_lowercase();
        if !seen_plugins.insert(plugin_name.clone()) {
            return Err(crate::Error::Unsupported(format!(
                "duplicate plugin source name in trace input: {plugin_name}"
            )));
        }
        let (model_summary, source) =
            static_models::export_with(&plugin_path, &mut std::io::sink(), |asset| {
                asset_references.push(asset);
                Ok(())
            })?;
        plugin_scans.push(PluginModelScan {
            source,
            summary: model_summary,
        });
    }

    let mut requested = BTreeSet::new();
    for reference in &asset_references {
        for candidate in &reference.lookup_candidates {
            requested.insert(AssetPath::new(&candidate.path)?);
        }
    }
    let lookup = asset_lookup::inspect(&data_dir, &requested)?;
    let mut summary = Summary {
        plugin_files_scanned: plugin_scans.len() as u64,
        source_stat_records: plugin_scans
            .iter()
            .map(|scan| scan.summary.stat_records)
            .sum(),
        source_path_findings: plugin_scans
            .iter()
            .map(|scan| {
                scan.summary.malformed_model_paths
                    + scan.summary.malformed_lod_shapes
                    + scan.summary.records_without_modl
                    + scan.summary.duplicate_model_fields
            })
            .sum(),
        source_editor_id_findings: plugin_scans
            .iter()
            .map(|scan| scan.summary.malformed_editor_ids)
            .sum(),
        source_asset_references: asset_references.len() as u64,
        unique_lookup_candidates: requested.len() as u64,
        archives_indexed: lookup.summary.archives_indexed,
        archive_index_failures: lookup.summary.archive_index_failures,
        archive_hash_only_entries: lookup.summary.archive_hash_only_entries,
        archive_invalid_paths: lookup.summary.archive_invalid_paths,
        duplicate_archive_candidate_entries: lookup.summary.duplicate_archive_candidate_entries,
        ..Summary::default()
    };

    line(
        writer,
        &serde_json::json!({
            "type": "source",
            "schema_version": 1,
            "plugins": plugin_scans.iter().map(|scan| &scan.source).collect::<Vec<_>>(),
            "source_record_summaries": plugin_scans.iter().map(|scan| (&scan.source.file, &scan.summary)).collect::<Vec<_>>(),
            "data_directory": data_dir,
            "identity": "physical STAT source records only; no override, active-order, archive precedence, or runtime selection",
            "lookup_policy": "field path exactly as stored plus a separate meshes/ rooted candidate when the stored path lacks that prefix; candidates are hypotheses checked independently"
        }),
    )?;
    for archive in &lookup.archives {
        line(
            writer,
            &serde_json::json!({"type":"archive-index", "archive":archive}),
        )?;
    }

    for source in asset_references {
        let mut candidates = Vec::new();
        let mut reference_has_loose = false;
        let mut reference_has_archive = false;
        for candidate in &source.lookup_candidates {
            let loose = asset_lookup::loose_file_status(&data_dir, &candidate.path);
            reference_has_loose |= loose == "present";
            let normalized = AssetPath::new(&candidate.path)?.bytes().to_vec();
            let matches = lookup.matches.get(&normalized).cloned().unwrap_or_default();
            reference_has_archive |= !matches.is_empty();
            candidates.push(CandidateTrace {
                path: candidate.path.clone(),
                basis: candidate.basis,
                loose_file_status: loose,
                archive_matches: matches,
            });
        }
        if reference_has_loose {
            summary.references_with_loose_file += 1;
        }
        if reference_has_archive {
            summary.references_with_archive_member += 1;
        }
        if !reference_has_loose && !reference_has_archive {
            summary.references_without_candidate_match += 1;
        }
        line(
            writer,
            &serde_json::json!({"type":"asset-reference", "asset":AssetTrace { source, candidates }}),
        )?;
    }

    line(
        writer,
        &serde_json::json!({
            "type": "complete",
            "summary": summary,
            "model_scans": plugin_scans,
            "archive_index_failures": lookup.archives.iter().filter(|archive| archive.failure.is_some()).map(|archive| (&archive.file, &archive.failure)).collect::<Vec<_>>(),
            "scope": "names-only archive indexes and source paths; no BSA payload decompression, archive invalidation policy, override winner, or runtime load claim"
        }),
    )?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dream_archive::bsa::tes4::Builder;

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
    fn loose_candidate_and_bsa_name_matches_are_reported_without_winner_selection() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("Data");
        fs::create_dir(&data).unwrap();
        let plugin_path = data.join("Models.esp");
        // Keep this fixture independent of installed data and use a compressed
        // member to assert that path linking only needs archive names.
        let mut header = record(
            b"TES4",
            0,
            &field(b"HEDR", &[0, 0, 0xD8, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0]),
        );
        let mut payload = field(b"EDID", b"DemoStatic\0");
        payload.extend(field(b"MODL", b"LoadScreenArt\\Demo.NIF\0"));
        let mut lod = vec![0; 4 * 260];
        let lod_path = b"LoadScreenArt\\Demo_LOD.nif\0";
        lod[..lod_path.len()].copy_from_slice(lod_path);
        payload.extend(field(b"MNAM", &lod));
        header.extend(record(b"STAT", 0x123, &payload));
        fs::write(&plugin_path, header).unwrap();
        let second_plugin = data.join("Creation.esp");
        let mut creation = record(
            b"TES4",
            0,
            &field(b"HEDR", &[0, 0, 0xD8, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0]),
        );
        let mut creation_payload = field(b"EDID", b"CreationStatic\0");
        creation_payload.extend(field(b"MODL", b"Creation\\Item.NIF\0"));
        creation.extend(record(b"STAT", 0x800, &creation_payload));
        fs::write(&second_plugin, creation).unwrap();
        let mut archive = Builder::skyrim_se();
        archive.set_compressed(true);
        archive
            .add_bytes(b"Meshes/LoadScreenArt/Demo.NIF", b"synthetic NIF member")
            .unwrap();
        archive
            .add_bytes(
                b"Meshes/LoadScreenArt/Demo_LOD.nif",
                b"synthetic LOD member",
            )
            .unwrap();
        archive
            .add_bytes(b"Meshes/Creation/Item.NIF", b"synthetic Creation member")
            .unwrap();
        archive
            .write_path(data.join("Skyrim - Meshes1.bsa"))
            .unwrap();

        let mut output = Vec::new();
        let summary =
            export_many(&data, &[plugin_path.clone(), second_plugin], &mut output).unwrap();
        assert_eq!(summary.plugin_files_scanned, 2);
        assert_eq!(summary.archive_index_failures, 0);
        assert_eq!(summary.archives_indexed, 1);
        assert_eq!(summary.references_with_archive_member, 3);
        let rows: Vec<serde_json::Value> = output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        let references = rows
            .iter()
            .filter(|row| row["type"] == "asset-reference")
            .collect::<Vec<_>>();
        assert_eq!(references.len(), 3);
        assert_eq!(
            references[2]["asset"]["source"]["plugin_file"],
            "Creation.esp"
        );
        assert_eq!(
            references[0]["asset"]["source"]["editor_ids"][0],
            serde_json::json!(b"DemoStatic".to_vec())
        );
        assert_eq!(
            references[0]["asset"]["candidates"][0]["basis"],
            "field-path-exact"
        );
        assert!(references.iter().any(|row| {
            row["asset"]["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| {
                    candidate["basis"] == "meshes-rooted-candidate"
                        && candidate["path"]
                            == serde_json::json!(b"meshes/loadscreenart/demo.nif".to_vec())
                        && !candidate["archive_matches"].as_array().unwrap().is_empty()
                        && !candidate["archive_matches"][0]["actual_member_paths"]
                            .as_array()
                            .unwrap()
                            .is_empty()
                })
        }));
        assert_eq!(rows.last().unwrap()["type"], "complete");
        assert!(rows.iter().any(|row| {
            row["type"] == "archive-index"
                && row["archive"]["sha256"].as_str().is_some()
                && row["archive"]["entries"] == 3
        }));
    }
}
