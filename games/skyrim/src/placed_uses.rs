//! Source-only reverse references from placed Skyrim REFR records to one base record.
//!
//! This joins file-local FormIDs through each plugin's declared master list. It
//! does not infer active order, winners, or runtime placement.
use crate::{Error, Result, plugin, trace};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self as framing, SelectedEvent},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io::{BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const ROW_LIMIT: usize = 100_000;
const FINDING_SAMPLE_LIMIT: usize = 100;

#[derive(Debug, Serialize)]
pub struct ModelField {
    pub field_offset: usize,
    pub raw_bytes: Vec<u8>,
    pub terminated: bool,
    pub normalized_asset_path: Option<Vec<u8>>,
    pub finding: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Target {
    pub plugin: String,
    pub plugin_bytes: u64,
    pub plugin_sha256: String,
    pub record_kind: String,
    pub form_id: u32,
    pub source_key: trace::SourceKey,
    pub record_offset: u64,
    pub record_flags: u32,
    pub editor_id_fields: Vec<Vec<u8>>,
    pub model_fields: Vec<ModelField>,
}

#[derive(Debug, Serialize)]
pub struct SourceScan {
    pub plugin: String,
    pub bytes: u64,
    pub sha256: String,
    pub masters: Vec<String>,
    pub light: bool,
    pub header_version_bits: u32,
    pub placed_references: u64,
    pub name_fields: u64,
    pub resolved_name_fields: u64,
    pub matching_placements: u64,
    pub unresolved_name_fields: u64,
    pub malformed_name_fields: u64,
}

#[derive(Debug, Serialize)]
pub struct Placement {
    pub record: trace::Context,
    pub field: String,
    pub field_offset: usize,
    pub field_bytes: Vec<u8>,
    pub target_raw: u32,
    pub target: trace::SourceKey,
}

#[derive(Debug, Serialize)]
pub struct NameFinding {
    pub record: trace::Context,
    pub field_offset: usize,
    pub field_bytes: Vec<u8>,
    pub reason: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub target: Target,
    pub sources: Vec<SourceScan>,
    pub placements: Vec<Placement>,
    pub unresolved_name_examples: Vec<NameFinding>,
    pub malformed_name_examples: Vec<NameFinding>,
    pub limitations: Vec<&'static str>,
}

/// Finds physical REFR `NAME` links to the specified record across the supplied
/// source plugins. The target plugin is always included in the scan. Cross-file
/// FE links remain unresolved because this API intentionally has no load order.
pub fn inspect(
    target_plugin: &Path,
    target_form_id: u32,
    source_plugins: &[PathBuf],
) -> Result<Report> {
    let target_header = plugin::inspect(target_plugin)?;
    let target_key = trace::source_record_key(
        &target_header.file,
        &target_header.masters,
        target_form_id,
        target_header.light,
        target_header.header_version_bits,
    )?
    .ok_or_else(|| {
        Error::Unsupported(format!(
            "target FormID 0x{target_form_id:08X} cannot be resolved from its plugin header"
        ))
    })?;
    let target = read_target(target_plugin, &target_header, target_form_id, target_key)?;

    let mut paths = vec![target_plugin.to_path_buf()];
    paths.extend_from_slice(source_plugins);
    let mut canonical_seen = BTreeSet::new();
    let mut report = Report {
        schema_version: 1,
        target,
        sources: Vec::new(),
        placements: Vec::new(),
        unresolved_name_examples: Vec::new(),
        malformed_name_examples: Vec::new(),
        limitations: vec![
            "physical source references only; does not prove plugin activation, winning overrides, or runtime placement",
            "NAME links using FE light slots remain unresolved without an explicit active-order map",
            "REFR NAME links are not extended to temporary references, actors, leveled spawns, or Papyrus-created objects",
            "model path presence does not prove that this record or model is consumed by the runtime",
        ],
    };

    for path in paths {
        let canonical = path.canonicalize()?;
        if !canonical_seen.insert(canonical.clone()) {
            continue;
        }
        let source = plugin::inspect(&canonical)?;
        let mut scan = SourceScan {
            plugin: source.file.clone(),
            bytes: source.bytes,
            sha256: source.sha256.clone(),
            masters: source.masters.clone(),
            light: source.light,
            header_version_bits: source.header_version_bits,
            placed_references: 0,
            name_fields: 0,
            resolved_name_fields: 0,
            matching_placements: 0,
            unresolved_name_fields: 0,
            malformed_name_fields: 0,
        };

        let mut reader = BufReader::new(open_source(&canonical)?);
        let (bytes, sha256) = digest_reader(&mut reader)?;
        if bytes != source.bytes || sha256 != source.sha256 {
            return Err(Error::Unsupported(format!(
                "plugin changed between inspect and placement passes: {}",
                canonical.display()
            )));
        }
        reader.seek(SeekFrom::Start(0))?;
        let mut ancestry = trace::Ancestry::default();
        framing::visit_selected(
            &mut reader,
            bytes,
            &source.file,
            framing::Limits::default(),
            |header| header.kind == *b"REFR",
            |event| {
                let record = match event {
                    SelectedEvent::Group(group) => {
                        ancestry.group(group);
                        return Ok(());
                    }
                    SelectedEvent::Deferred(header) => {
                        ancestry.advance(header.offset);
                        return Ok(());
                    }
                    SelectedEvent::Record(record) => record,
                };
                ancestry.advance(record.header.offset);
                scan.placed_references += 1;
                let mut context = trace::context(record, &source.file, &ancestry)?;
                context
                    .resolve(&source.masters, source.light, source.header_version_bits)
                    .map_err(|error| fallout_data::Error::Resolution(error.to_string()))?;

                framing::visit_subrecords(record, &source.file, |field| {
                    if field.kind != *b"NAME" {
                        return Ok(());
                    }
                    scan.name_fields += 1;
                    if field.data.len() != 4 {
                        scan.malformed_name_fields += 1;
                        retain_finding(
                            &mut report.malformed_name_examples,
                            NameFinding {
                                record: context.clone(),
                                field_offset: field.payload_offset,
                                field_bytes: field.data.to_vec(),
                                reason: "NAME field is not a four-byte FormID",
                            },
                        )?;
                        return Ok(());
                    }
                    let target_raw = u32::from_le_bytes(field.data.try_into().unwrap());
                    if target_raw == 0 {
                        return Ok(());
                    }
                    let Some(key) = trace::source_key(&source.file, &source.masters, target_raw)
                        .map_err(|error| fallout_data::Error::Resolution(error.to_string()))?
                    else {
                        scan.unresolved_name_fields += 1;
                        retain_finding(
                            &mut report.unresolved_name_examples,
                            NameFinding {
                                record: context.clone(),
                                field_offset: field.payload_offset,
                                field_bytes: field.data.to_vec(),
                                reason: "master index unresolved without the active light-plugin slot map",
                            },
                        )?;
                        return Ok(());
                    };
                    scan.resolved_name_fields += 1;
                    if key == report.target.source_key {
                        if report.placements.len() >= ROW_LIMIT {
                            return Err(framing_error("placement row budget exceeded"));
                        }
                        scan.matching_placements += 1;
                        report.placements.push(Placement {
                            record: context.clone(),
                            field: "NAME".into(),
                            field_offset: field.payload_offset,
                            field_bytes: field.data.to_vec(),
                            target_raw,
                            target: key,
                        });
                    }
                    Ok(())
                })?;
                Ok(())
            },
        )?;
        report.sources.push(scan);
    }
    Ok(report)
}

fn read_target(
    path: &Path,
    source: &plugin::PluginReport,
    form_id: u32,
    key: trace::SourceKey,
) -> Result<Target> {
    let mut reader = BufReader::new(open_source(path)?);
    let (bytes, sha256) = digest_reader(&mut reader)?;
    if bytes != source.bytes || sha256 != source.sha256 {
        return Err(Error::Unsupported(format!(
            "target plugin changed between inspect and record lookup: {}",
            path.display()
        )));
    }
    reader.seek(SeekFrom::Start(0))?;
    let file = source.file.clone();
    let mut found = None;
    let mut target_count = 0usize;
    framing::visit_selected(
        &mut reader,
        bytes,
        &file,
        framing::Limits::default(),
        |header| header.form_id == form_id,
        |event| {
            let SelectedEvent::Record(record) = event else {
                return Ok(());
            };
            if record.header.form_id != form_id {
                return Ok(());
            }
            target_count += 1;
            if record.header.kind != *b"ACTI" {
                return Err(framing_error("target record is not an ACTI"));
            }
            let mut editor_id_fields = Vec::new();
            let mut model_fields = Vec::new();
            framing::visit_subrecords(record, &file, |field| {
                match &field.kind {
                    b"EDID" => {
                        bounded(editor_id_fields.len())?;
                        editor_id_fields.push(field.data.to_vec());
                    }
                    b"MODL" => {
                        bounded(model_fields.len())?;
                        let terminated = field.data.last() == Some(&0);
                        let path_bytes = field.data.strip_suffix(&[0]).unwrap_or(field.data);
                        let (normalized_asset_path, finding) = if !terminated {
                            (None, Some("model path is not NUL terminated"))
                        } else {
                            match AssetPath::new(path_bytes) {
                                Ok(path) => (Some(path.bytes().to_vec()), None),
                                Err(_) => {
                                    (None, Some("model path is rejected by shared AssetPath"))
                                }
                            }
                        };
                        model_fields.push(ModelField {
                            field_offset: field.payload_offset,
                            raw_bytes: field.data.to_vec(),
                            terminated,
                            normalized_asset_path,
                            finding,
                        });
                    }
                    _ => {}
                }
                Ok(())
            })?;
            found = Some(Target {
                plugin: file.clone(),
                plugin_bytes: source.bytes,
                plugin_sha256: source.sha256.clone(),
                record_kind: "ACTI".into(),
                form_id,
                source_key: key.clone(),
                record_offset: record.header.offset,
                record_flags: record.header.flags,
                editor_id_fields,
                model_fields,
            });
            Ok(())
        },
    )?;
    if target_count != 1 {
        return Err(Error::Unsupported(format!(
            "expected exactly one physical ACTI with FormID 0x{form_id:08X}; found {target_count}"
        )));
    }
    found.ok_or_else(|| Error::Unsupported("target ACTI was not found".into()))
}

fn retain_finding(rows: &mut Vec<NameFinding>, finding: NameFinding) -> fallout_data::Result<()> {
    if rows.len() < FINDING_SAMPLE_LIMIT {
        rows.push(finding);
    }
    Ok(())
}

fn bounded(len: usize) -> fallout_data::Result<()> {
    if len >= ROW_LIMIT {
        Err(framing_error("target field row budget exceeded"))
    } else {
        Ok(())
    }
}

fn framing_error(message: &str) -> fallout_data::Error {
    fallout_data::Error::Resolution(message.into())
}

/// Validate the caller-supplied plugin filename using the shared identity rules.
pub fn validate_plugin_name(name: &str) -> Result<String> {
    let path = Path::new(name);
    if path.components().count() != 1 {
        return Err(Error::Unsupported(
            "plugin argument must be a filename directly inside the selected Data directory".into(),
        ));
    }
    let normalized = plugin_name(name)?;
    let extension = Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(extension, "esm" | "esp" | "esl") {
        return Err(Error::Unsupported(format!(
            "unsupported plugin extension in {name:?}"
        )));
    }
    Ok(name.into())
}
