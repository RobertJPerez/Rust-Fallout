//! Read-only installed corpus inventory; installed files do not imply activation.
use crate::{
    Result,
    archive::{ArchiveReport, SkyrimArchive},
    pex_header::{self, Observation as PexHeaderObservation},
    plugin::{self, PluginReport},
};
use fallout_data::{
    baseline::{Fingerprint, digest_file, digest_reader, open_source, sorted_files},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub const REQUIRED_PLUGINS: &[&str] = &[
    "Skyrim.esm",
    "Update.esm",
    "Dawnguard.esm",
    "HearthFires.esm",
    "Dragonborn.esm",
];
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub game: &'static str,
    pub data: PathBuf,
    pub runtime_version_reported: Option<String>,
    pub executable: Option<Fingerprint>,
    pub shared_revision: &'static str,
    pub missing_required: Vec<String>,
    pub plugins: Vec<PluginReport>,
    pub archives: Vec<ArchiveReport>,
    pub loose_scripts: Vec<Fingerprint>,
    pub loose_script_headers: Vec<LooseScriptHeader>,
    pub script_header_counts: BTreeMap<String, u64>,
    pub script_header_findings: u64,
    pub creation_named_plugins: Vec<String>,
    pub dependency_findings: Vec<String>,
    pub script_availability: Vec<ScriptAvailability>,
    pub failures: Vec<String>,
    pub blocking_findings: u64,
    pub limitations: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct LooseScriptHeader {
    pub path: String,
    pub dialect_prefix: PexHeaderObservation,
}
#[derive(Debug, Serialize)]
pub struct ScriptAvailability {
    pub path: Vec<u8>,
    pub containers: Vec<String>,
    pub referenced_by_plugins: Vec<String>,
    pub status: &'static str,
}

pub fn inspect(
    data: &Path,
    runtime_version: Option<String>,
    mut progress: impl FnMut(&Path),
) -> Result<Report> {
    let data = data.canonicalize()?;
    let files = sorted_files(&data)?;
    let top: Vec<_> = files
        .iter()
        .filter(|p| p.parent() == Some(data.as_path()))
        .collect();
    let mut report = Report {
        schema_version: 3,
        game: "skyrim-se-ae",
        data: data.clone(),
        runtime_version_reported: runtime_version,
        executable: None,
        shared_revision: "9265ef63f714cdf01ff02028640a582be9639c7a",
        missing_required: Vec::new(),
        plugins: Vec::new(),
        archives: Vec::new(),
        loose_scripts: Vec::new(),
        loose_script_headers: Vec::new(),
        script_header_counts: BTreeMap::new(),
        script_header_findings: 0,
        creation_named_plugins: Vec::new(),
        dependency_findings: Vec::new(),
        script_availability: Vec::new(),
        failures: Vec::new(),
        blocking_findings: 0,
        limitations: vec![
            "Inventory of installed files, not an active load order or selected override winners",
            "Creation-named plugins are filename observations, not proof of Anniversary Upgrade ownership or a complete paid-content set",
            "VMAD v4/v5 primary and quest-alias scripts plus supported record fragments; failures retain raw bytes and diagnostics",
            "Script file availability is not PEX class/function linkage or implemented Papyrus execution",
            "All plugin/archive inputs and loose PEX files hashed; only the first eight PEX header bytes are observed; non-PEX archive payloads not decompressed",
            "Executable version supplied by the caller; executable SHA-256 measured directly",
            "No Papyrus execution, quests, rendering, Havok behavior, native SKSE DLLs or retail saves accepted",
        ],
    };
    let mut named: BTreeMap<String, usize> = BTreeMap::new();
    for path in &top {
        let name = path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_ascii_lowercase();
        *named.entry(name).or_default() += 1;
    }
    for (name, count) in &named {
        if *count > 1 {
            report
                .failures
                .push(format!("case-colliding root files: {name}"));
        }
    }
    for name in REQUIRED_PLUGINS {
        if !named.contains_key(&name.to_ascii_lowercase()) {
            report.missing_required.push((*name).into());
        }
    }
    if let Some(parent) = data.parent() {
        let exe = parent.join("SkyrimSE.exe");
        if exe.is_file() {
            progress(&exe);
            let (bytes, sha256) = digest_file(&exe)?;
            report.executable = Some(Fingerprint {
                path: "SkyrimSE.exe".into(),
                bytes,
                sha256,
            });
        }
    }
    let mut script_sources: BTreeMap<Vec<u8>, Vec<String>> = BTreeMap::new();
    let mut archive_script_bytes = 0u64;
    for path in top {
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "esm" | "esp" | "esl") {
            progress(path);
            match plugin::inspect(path) {
                Ok(plugin) => {
                    if plugin.file.to_ascii_lowercase().starts_with("cc") {
                        report.creation_named_plugins.push(plugin.file.clone());
                    }
                    report.plugins.push(plugin);
                }
                Err(error) => report.failures.push(format!("{}: {error}", path.display())),
            }
        } else if extension == "bsa" {
            progress(path);
            match SkyrimArchive::open(path).and_then(|mut archive| archive.inspect()) {
                Ok(archive) => {
                    archive_script_bytes += archive.scripts.iter().map(|s| s.bytes).sum::<u64>();
                    if archive_script_bytes > 2 * 1024 * 1024 * 1024 {
                        return Err(crate::bad(
                            "corpus",
                            0,
                            "aggregate archived script budget exceeded",
                        ));
                    }
                    for script in &archive.scripts {
                        script_sources
                            .entry(script.path.clone())
                            .or_default()
                            .push(archive.file.clone());
                    }
                    for (key, count) in &archive.script_header_counts {
                        *report.script_header_counts.entry(key.clone()).or_default() += count;
                    }
                    report.script_header_findings += archive.script_header_findings as u64;
                    report.archives.push(archive);
                }
                Err(error) => report.failures.push(format!("{}: {error}", path.display())),
            }
        }
    }
    for path in &files {
        let relative = path.strip_prefix(&data).expect("enumerated under data");
        let raw = relative
            .to_str()
            .ok_or_else(|| crate::bad("loose asset", 0, "unsupported filename encoding"))?;
        let key = AssetPath::new(raw.as_bytes())?;
        if key.bytes().starts_with(b"scripts/") && key.bytes().ends_with(b".pex") {
            progress(path);
            let mut source = open_source(path)?;
            let (bytes, sha256) = digest_reader(&mut source)?;
            source.seek(SeekFrom::Start(0))?;
            let mut prefix = [0; 8];
            let mut prefix_bytes = 0;
            while prefix_bytes < prefix.len() {
                let read = source.read(&mut prefix[prefix_bytes..])?;
                if read == 0 {
                    break;
                }
                prefix_bytes += read;
            }
            let dialect_prefix = pex_header::observe(&prefix[..prefix_bytes]);
            *report
                .script_header_counts
                .entry(dialect_prefix.census_key())
                .or_default() += 1;
            if !dialect_prefix.is_skyrim() {
                report.script_header_findings += 1;
            }
            report.loose_script_headers.push(LooseScriptHeader {
                path: raw.into(),
                dialect_prefix,
            });
            script_sources
                .entry(key.bytes().to_vec())
                .or_default()
                .push(format!("loose:{raw}"));
            report.loose_scripts.push(Fingerprint {
                path: raw.into(),
                bytes,
                sha256,
            });
        }
    }
    let present: BTreeSet<_> = report
        .plugins
        .iter()
        .map(|p| p.file.to_ascii_lowercase())
        .collect();
    let mut used: BTreeMap<Vec<u8>, BTreeSet<String>> = BTreeMap::new();
    for plugin in &report.plugins {
        for master in &plugin.masters {
            if !present.contains(&master.to_ascii_lowercase()) {
                report.dependency_findings.push(format!(
                    "{} requires missing or unreadable master {master}",
                    plugin.file
                ));
            }
        }
        for script in &plugin.scripts {
            if script.non_removed_attachments == 0 && script.fragment_references == 0 {
                continue;
            }
            if let Some(path) = &script.asset_path {
                used.entry(path.clone())
                    .or_default()
                    .insert(plugin.file.clone());
            } else {
                report.dependency_findings.push(format!(
                    "{} has an unsafe or unsupported script path {:?}",
                    plugin.file, script.name_bytes
                ));
            }
        }
    }
    for (path, plugins) in used {
        let containers = script_sources.get(&path).cloned().unwrap_or_default();
        let status = match containers.len() {
            0 => "missing-in-installed-corpus",
            1 => "present; activation-unverified",
            _ => "multiple-candidates; precedence-unverified",
        };
        report.script_availability.push(ScriptAvailability {
            path,
            containers,
            referenced_by_plugins: plugins.into_iter().collect(),
            status,
        });
    }
    report.blocking_findings = (report.failures.len()
        + report.missing_required.len()
        + report.dependency_findings.len()) as u64
        + report.plugins.iter().map(|p| p.issue_count).sum::<u64>()
        + report
            .archives
            .iter()
            .map(|a| a.duplicate_paths as u64)
            .sum::<u64>()
        + report
            .script_availability
            .iter()
            .filter(|s| s.containers.is_empty())
            .count() as u64
        + report.script_header_findings;
    Ok(report)
}
