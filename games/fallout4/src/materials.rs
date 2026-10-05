//! FO4-specific discovery and private extraction of archived BGSM/BGEM sources.
//! This retains candidates and source bytes without choosing archive winners or
//! interpreting material fields.
use crate::{
    Error, Result,
    archive::{Ba2, Limits},
    census,
};
use dream_archive::ByteSlice;
use fallout_data::identity::ProfileId;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_ARCHIVES: usize = 512;
const MAX_MATERIALS: usize = 100_000;
const MAX_DECODED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct MaterialSource {
    pub archive: String,
    pub archive_sha256: String,
    pub entry_index: usize,
    pub member_name_bytes_hex: String,
    pub kind: &'static str,
    pub bytes: usize,
    pub sha256: String,
    pub extracted_file: String,
}

#[derive(Debug, Serialize)]
pub struct ArchiveSource {
    pub archive: String,
    pub archive_sha256: Option<String>,
    pub bgsm: usize,
    pub bgem: usize,
    pub decoded_material_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub profile: ProfileId,
    pub observation_only: bool,
    pub archives_scanned: usize,
    pub archives_with_materials: usize,
    pub bgsm: usize,
    pub bgem: usize,
    pub decoded_material_bytes: u64,
    pub archives: Vec<ArchiveSource>,
    pub materials: Vec<MaterialSource>,
    pub scope: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Completion {
    pub schema_version: u32,
    pub manifest_sha256: String,
    pub archives_scanned: usize,
    pub archives_with_materials: usize,
    pub bgsm: usize,
    pub bgem: usize,
    pub decoded_material_bytes: u64,
}

fn kind(name: &[u8]) -> Option<&'static str> {
    let extension_start = name
        .iter()
        .rposition(|byte| *byte == b'.')?
        .saturating_add(1);
    match &name[extension_start..] {
        ext if ext.eq_ignore_ascii_case(b"bgsm") => Some("bgsm"),
        ext if ext.eq_ignore_ascii_case(b"bgem") => Some("bgem"),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn archives(data: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(data)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ba2"))
        {
            paths.push(entry.path());
        }
        if paths.len() > MAX_ARCHIVES {
            return Err(Error::Unsupported(format!(
                "archive count exceeds {MAX_ARCHIVES}"
            )));
        }
    }
    paths.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(
                &b.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase(),
            )
    });
    Ok(paths)
}

fn output_directory(install: &Path, requested: &Path) -> Result<PathBuf> {
    let install = fs::canonicalize(install)?;
    let local = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("local"))?;
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)?;
    let name = requested
        .file_name()
        .ok_or_else(|| Error::Unsupported("new material output directory name required".into()))?;
    let output = parent.join(name);
    if output.starts_with(&install) || !output.starts_with(&local) {
        return Err(Error::Unsupported(
            "material output must be inside ignored local/ and outside the retail installation"
                .into(),
        ));
    }
    Ok(output)
}

/// Extract every archived material to a new ignored/local directory. Entry and
/// archive identities remain in the manifest; no archive precedence is applied.
pub fn extract(install_root: &Path, requested_output: &Path) -> Result<(Manifest, Completion)> {
    let install = fs::canonicalize(install_root)?;
    let data = install.join("Data");
    let output = output_directory(&install, requested_output)?;
    fs::create_dir(&output)?;
    let paths = archives(&data)?;
    let mut archive_sources = Vec::new();
    let mut materials = Vec::new();
    let mut bgsm = 0usize;
    let mut bgem = 0usize;
    let mut total_bytes = 0u64;
    let mut material_archive_index = 0usize;

    for (archive_index, path) in paths.iter().enumerate() {
        let source_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let metadata_before = fs::metadata(path)?;
        let archive = Ba2::open(path, Limits::default())?;
        let candidates = archive
            .entries()
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| kind(entry.name().as_bytes()).map(|kind| (index, kind)))
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            archive_sources.push(ArchiveSource {
                archive: source_name,
                archive_sha256: None,
                bgsm: 0,
                bgem: 0,
                decoded_material_bytes: 0,
            });
            continue;
        }
        if materials.len().saturating_add(candidates.len()) > MAX_MATERIALS {
            return Err(Error::Unsupported(format!(
                "material entry count exceeds {MAX_MATERIALS}"
            )));
        }
        let digest_before = census::hash_file(path)?;
        let mut archive_bgsm = 0usize;
        let mut archive_bgem = 0usize;
        let mut archive_bytes = 0u64;

        for (entry_index, material_kind) in candidates {
            let entry = &archive.entries()[entry_index];
            let raw_name = entry.name().as_bytes();
            let payload = archive.read(entry_index)?;
            total_bytes = total_bytes
                .checked_add(payload.len() as u64)
                .ok_or_else(|| Error::Unsupported("material byte total overflow".into()))?;
            archive_bytes = archive_bytes
                .checked_add(payload.len() as u64)
                .ok_or_else(|| Error::Unsupported("archive material byte total overflow".into()))?;
            if total_bytes > MAX_DECODED_BYTES {
                return Err(Error::Unsupported(format!(
                    "decoded material bytes exceed {MAX_DECODED_BYTES}"
                )));
            }
            let extracted_file =
                format!("archive-{archive_index:03}-member-{entry_index:06}.{material_kind}");
            fs::write(output.join(&extracted_file), &payload)?;
            match material_kind {
                "bgsm" => {
                    bgsm += 1;
                    archive_bgsm += 1;
                }
                "bgem" => {
                    bgem += 1;
                    archive_bgem += 1;
                }
                _ => unreachable!("material kind is extension-derived"),
            }
            materials.push(MaterialSource {
                archive: source_name.clone(),
                archive_sha256: String::new(),
                entry_index,
                member_name_bytes_hex: hex(raw_name),
                kind: material_kind,
                bytes: payload.len(),
                sha256: census::sha256(&payload),
                extracted_file,
            });
        }
        drop(archive);
        let metadata_after = fs::metadata(path)?;
        let digest_after = census::hash_file(path)?;
        if metadata_before.len() != metadata_after.len()
            || metadata_before.modified().ok() != metadata_after.modified().ok()
            || digest_before != digest_after
        {
            return Err(Error::Unsupported(format!(
                "source archive {source_name:?} changed during material extraction"
            )));
        }
        for material in materials
            .iter_mut()
            .filter(|material| material.archive == source_name)
        {
            material.archive_sha256.clone_from(&digest_after);
        }
        archive_sources.push(ArchiveSource {
            archive: source_name,
            archive_sha256: Some(digest_after),
            bgsm: archive_bgsm,
            bgem: archive_bgem,
            decoded_material_bytes: archive_bytes,
        });
        material_archive_index += 1;
    }

    let manifest = Manifest {
        schema_version: 1,
        profile: ProfileId::Fo4Original,
        observation_only: true,
        archives_scanned: paths.len(),
        archives_with_materials: material_archive_index,
        bgsm,
        bgem,
        decoded_material_bytes: total_bytes,
        archives: archive_sources,
        materials,
        scope: "All direct Data BA2 material members extracted by stable archive/member identity; no parser semantics, active profile, archive winner, texture dependencies, or rendering",
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let manifest_path = output.join("manifest.json");
    fs::write(&manifest_path, &manifest_bytes)?;
    let completion = Completion {
        schema_version: 1,
        manifest_sha256: census::sha256(&manifest_bytes),
        archives_scanned: manifest.archives_scanned,
        archives_with_materials: manifest.archives_with_materials,
        bgsm: manifest.bgsm,
        bgem: manifest.bgem,
        decoded_material_bytes: manifest.decoded_material_bytes,
    };
    fs::write(
        output.join("complete.json"),
        serde_json::to_vec_pretty(&completion)?,
    )?;
    Ok((manifest, completion))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_material_extensions_without_case_or_path_assumptions() {
        assert_eq!(kind(b"materials/foo.BGSM"), Some("bgsm"));
        assert_eq!(kind(b"materials/effects/plasma.bgem"), Some("bgem"));
        assert_eq!(kind(b"textures/common/tile.dds"), None);
        assert_eq!(kind(b"bgsm"), None);
    }

    #[test]
    fn rejects_output_under_retail_root_and_never_reuses_an_existing_path() {
        let dir =
            tempfile::tempdir().unwrap();
        let install = dir.path().join("game");
        fs::create_dir_all(install.join("Data")).unwrap();
        let inside = install.join("local-materials");
        assert!(output_directory(&install, &inside).is_err());
        let outside = dir.path().join("local-materials");
        fs::create_dir(&outside).unwrap();
        assert!(fs::create_dir(output_directory(&install, &outside).unwrap()).is_err());
    }
}
