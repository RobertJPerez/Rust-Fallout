//! Deterministic physical-content census, without assumed load order or winners.
use crate::{Error, Result, archive::Ba2, pex};
use dream_archive::ByteSlice;
use fallout_data::plugin::{self, Event};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, Read},
    path::Path,
};

pub const SHARED_REVISION: &str = "9265ef63f714cdf01ff02028640a582be9639c7a";
pub fn text(bytes: &[u8]) -> String {
    bytes.escape_ascii().to_string()
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn hash_file(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let buffer_bytes = file.metadata()?.len().clamp(8192, 1024 * 1024) as usize;
    let mut input = BufReader::new(file);
    let mut hash = Sha256::new();
    let mut buf = vec![0; buffer_bytes];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Default, Debug, Serialize)]
pub struct PluginReport {
    pub name: String,
    pub records: u64,
    pub groups: u64,
    pub compressed_records: u64,
    pub record_kinds: BTreeMap<String, u64>,
    pub subrecord_kinds: BTreeMap<String, u64>,
    pub record_versions: BTreeMap<u16, u64>,
    pub vmad_headers: BTreeMap<String, u64>,
    pub masters: Vec<String>,
    pub header_version_bits: Option<u32>,
    pub header_flags: u32,
    pub declared_records_and_groups: Option<u32>,
    pub decoded_bytes: u64,
    pub findings: Vec<String>,
}
pub fn plugin_census(path: &Path) -> Result<PluginReport> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut report = PluginReport {
        name: name.clone(),
        ..Default::default()
    };
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    plugin::visit(
        &mut BufReader::new(file),
        len,
        &name,
        plugin::Limits::default(),
        |event| {
            match event {
                Event::Group(_) => report.groups += 1,
                Event::Record(record) => {
                    report.records += 1;
                    report.decoded_bytes += record.payload.len() as u64;
                    if record.header.flags & plugin::COMPRESSED != 0 {
                        report.compressed_records += 1;
                    }
                    let kind = plugin::signature(record.header.kind);
                    *report.record_kinds.entry(kind.clone()).or_default() += 1;
                    *report
                        .record_versions
                        .entry(record.header.version)
                        .or_default() += 1;
                    if record.header.kind == *b"TES4" {
                        report.header_flags = record.header.flags;
                    }
                    plugin::visit_subrecords(record, &name, |sub| {
                        *report
                            .subrecord_kinds
                            .entry(format!("{kind}/{}", plugin::signature(sub.kind)))
                            .or_default() += 1;
                        if record.header.kind == *b"TES4" {
                            if sub.kind == *b"HEDR" {
                                if sub.data.len() != 12 || report.header_version_bits.is_some() {
                                    return Err(fallout_data::Error::Unsupported(format!(
                                        "{name}: invalid or duplicate HEDR"
                                    )));
                                }
                                report.header_version_bits =
                                    Some(u32::from_le_bytes(sub.data[..4].try_into().unwrap()));
                                report.declared_records_and_groups =
                                    Some(u32::from_le_bytes(sub.data[4..8].try_into().unwrap()));
                            }
                            if sub.kind == *b"MAST" {
                                report
                                    .masters
                                    .push(text(sub.data.strip_suffix(&[0]).unwrap_or(sub.data)));
                            }
                        }
                        if sub.kind == *b"VMAD" {
                            if sub.data.len() < 6 {
                                return Err(fallout_data::Error::Unsupported(format!(
                                    "{name}: short VMAD at {}",
                                    record.header.offset
                                )));
                            }
                            let version = u16::from_le_bytes(sub.data[..2].try_into().unwrap());
                            let format = u16::from_le_bytes(sub.data[2..4].try_into().unwrap());
                            // Only header presence is measured; script attachments are not decoded.
                            *report
                                .vmad_headers
                                .entry(format!("version={version},object_format={format}"))
                                .or_default() += 1;
                        }
                        Ok(())
                    })?;
                }
            }
            Ok(())
        },
    )?;
    if report.header_version_bits.is_none() {
        return Err(Error::Unsupported(format!("{name}: missing HEDR")));
    }
    let physical_count = report.records.saturating_sub(1) + report.groups;
    if report.declared_records_and_groups.map(u64::from) != Some(physical_count) {
        report.findings.push(format!("HEDR declares {:?} records/groups; physical walk found {physical_count} excluding TES4. Source header retained unchanged.",report.declared_records_and_groups));
    }
    Ok(report)
}

#[derive(Default, Debug, Serialize)]
pub struct PexSummary {
    pub path: String,
    pub sha256: String,
    pub bytes: usize,
    pub strings: usize,
    pub objects: usize,
    pub structs: u64,
    pub variables: u64,
    pub properties: u64,
    pub states: u64,
    pub functions: u64,
    pub instructions: u64,
    pub opcode_counts: BTreeMap<u8, u64>,
    pub native_declarations: Vec<NativeDeclaration>,
}
#[derive(Debug, Serialize)]
pub struct NativeDeclaration {
    pub class: String,
    pub parent: String,
    pub state: Option<String>,
    pub function: String,
    pub kind: String,
    pub flags: u8,
    pub return_type: String,
    pub parameters: Vec<(String, String)>,
    pub byte_offset: usize,
}
pub fn pex_summary(bytes: &[u8], name: &str) -> Result<PexSummary> {
    let decoded = pex::parse(bytes, name, pex::Limits::default())?;
    let string = |i: u16| text(decoded.strings[i as usize]);
    let mut out = PexSummary {
        path: name.into(),
        sha256: sha256(bytes),
        bytes: bytes.len(),
        strings: decoded.strings.len(),
        objects: decoded.objects.len(),
        ..Default::default()
    };
    for object in &decoded.objects {
        out.structs += u64::from(object.structs);
        out.variables += u64::from(object.variables);
        out.properties += u64::from(object.properties);
        out.states += u64::from(object.states);
        out.functions += object.functions.len() as u64;
        for f in &object.functions {
            out.instructions += f.instructions.len() as u64;
            for i in &f.instructions {
                *out.opcode_counts.entry(i.opcode).or_default() += 1;
            }
            if f.native() {
                out.native_declarations.push(NativeDeclaration {
                    class: string(object.name),
                    parent: string(object.parent),
                    state: f.state.map(string),
                    function: string(f.name),
                    kind: f.kind.into(),
                    flags: f.flags,
                    return_type: string(f.return_type),
                    parameters: f
                        .parameters
                        .iter()
                        .map(|(n, t)| (string(*n), string(*t)))
                        .collect(),
                    byte_offset: f.range.start,
                });
            }
        }
    }
    Ok(out)
}

#[derive(Default, Debug, Serialize)]
pub struct ArchiveReport {
    pub name: String,
    pub version: u32,
    pub format: String,
    pub entries: usize,
    pub chunks: u64,
    pub compressed_chunks: u64,
    pub declared_unpacked_bytes: u64,
    pub extensions: BTreeMap<String, u64>,
    pub texture_formats: BTreeMap<u8, u64>,
    pub ambiguous_paths: usize,
    pub nameless_entries: u64,
    pub pex: Vec<PexSummary>,
    pub pex_failures: Vec<Finding>,
}
pub fn archive_census(path: &Path) -> Result<ArchiveReport> {
    let archive = Ba2::open(path, Default::default())?;
    let mut report = ArchiveReport {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        version: archive.info().version as u32,
        format: format!("{:?}", archive.info().format),
        entries: archive.entries().len(),
        ambiguous_paths: archive.collisions(),
        ..Default::default()
    };
    for (i, entry) in archive.entries().iter().enumerate() {
        let raw = entry.name().as_bytes();
        if raw.is_empty() {
            report.nameless_entries += 1;
        }
        let extension = raw
            .rsplit(|c| *c == b'.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        *report.extensions.entry(text(&extension)).or_default() += 1;
        if let Some(h) = archive.texture(i) {
            *report.texture_formats.entry(h.format).or_default() += 1;
        }
        for chunk in entry.file().chunks() {
            report.chunks += 1;
            report.compressed_chunks += u64::from(chunk.is_compressed());
            report.declared_unpacked_bytes += u64::from(chunk.size());
        }
        if extension == b"pex" {
            let name = text(raw);
            match archive.read(i).and_then(|bytes| pex_summary(&bytes, &name)) {
                Ok(summary) => report.pex.push(summary),
                Err(error) => report.pex_failures.push(Finding {
                    source: name,
                    reason: error.to_string(),
                }),
            }
        }
    }
    Ok(report)
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub source: String,
    pub reason: String,
}
#[derive(Debug, Serialize)]
pub struct Fingerprint {
    pub path: String,
    pub bytes: u64,
    pub sha256: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Census {
    pub schema: u32,
    pub profile: &'static str,
    pub shared_revision: &'static str,
    pub scope: &'static str,
    pub full_hashes: bool,
    pub files: Vec<Fingerprint>,
    pub plugins: Vec<PluginReport>,
    pub archives: Vec<ArchiveReport>,
    pub failures: Vec<Finding>,
    pub missing_standard_masters: Vec<String>,
    pub unresolved_masters: Vec<String>,
    pub capabilities: Vec<&'static str>,
    pub unsupported: Vec<&'static str>,
}
impl Census {
    pub fn complete(&self) -> bool {
        self.failures.is_empty()
            && self.archives.iter().all(|a| a.pex_failures.is_empty())
            && self.missing_standard_masters.is_empty()
            && self.unresolved_masters.is_empty()
    }
}

pub fn installation(root: &Path, full_hashes: bool) -> Result<Census> {
    let data = root.join("Data");
    let mut paths = fs::read_dir(&data)?
        .map(|r| r.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|p| p.is_file());
    paths.push(root.join("Fallout4.exe"));
    paths.sort();
    let mut out = Census {
        schema: 1,
        profile: "fo4-original",
        shared_revision: SHARED_REVISION,
        scope: "Fallout4.exe and top-level Data files; physical corpus, not configured activation/load order",
        full_hashes,
        files: Vec::new(),
        plugins: Vec::new(),
        archives: Vec::new(),
        failures: Vec::new(),
        missing_standard_masters: Vec::new(),
        unresolved_masters: Vec::new(),
        capabilities: vec![
            "shared bounded TES4/GRUP/subrecord framing and strict zlib decoding",
            "BA2 1/7/8 GNRL and DX10 index; bounded member extraction",
            "FO4 PEX 3.9 full structural walk, operands and native signatures",
        ],
        unsupported: vec![
            "plugin field semantics, ESL identity/override resolution and active load order",
            "VMAD attachments beyond header; alias/scene linking",
            "Papyrus execution, native implementations, scheduling and saves",
            "materials/meshes/animation decoding, rendering and gameplay",
        ],
    };
    for path in paths {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        eprintln!("Inspecting {name}");
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let scan = (|| -> Result<()> {
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(Error::Unsupported(
                    "symlink source outside inventory policy".into(),
                ));
            }
            let hash = if full_hashes || matches!(extension.as_str(), "esm" | "esp" | "esl" | "exe")
            {
                Some(hash_file(&path)?)
            } else {
                None
            };
            out.files.push(Fingerprint {
                path: path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                bytes: metadata.len(),
                sha256: hash,
            });
            match extension.as_str() {
                "esm" | "esp" | "esl" => out.plugins.push(plugin_census(&path)?),
                "ba2" => out.archives.push(archive_census(&path)?),
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = scan {
            out.failures.push(Finding {
                source: name,
                reason: error.to_string(),
            });
        }
    }
    let present: Vec<String> = out
        .plugins
        .iter()
        .map(|p| p.name.to_ascii_lowercase())
        .collect();
    for name in [
        "Fallout4.esm",
        "DLCRobot.esm",
        "DLCworkshop01.esm",
        "DLCCoast.esm",
        "DLCworkshop02.esm",
        "DLCworkshop03.esm",
        "DLCNukaWorld.esm",
    ] {
        if !present.contains(&name.to_ascii_lowercase()) {
            out.missing_standard_masters.push(name.into());
        }
    }
    for plugin in &out.plugins {
        for master in &plugin.masters {
            if !present.contains(&master.to_ascii_lowercase()) {
                out.unresolved_masters
                    .push(format!("{} -> {master}", plugin.name));
            }
        }
    }
    Ok(out)
}
