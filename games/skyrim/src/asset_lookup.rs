//! Shared names-only loose/BSA asset lookup for Skyrim source references.
use crate::{Error, Result, archive};
use fallout_data::vfs::AssetPath;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Serialize)]
pub(crate) struct Summary {
    pub archives_indexed: u64,
    pub archive_index_failures: u64,
    pub archive_hash_only_entries: u64,
    pub archive_invalid_paths: u64,
    pub duplicate_archive_candidate_entries: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ArchiveStatus {
    pub file: String,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
    pub entries: Option<usize>,
    pub matching_candidate_paths: u64,
    pub hash_only_entries: u64,
    pub invalid_asset_paths: u64,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ArchiveMatch {
    pub archive: String,
    pub actual_member_paths: Vec<Vec<u8>>,
    pub duplicate_entries: u64,
}

#[derive(Default)]
pub(crate) struct Lookup {
    pub summary: Summary,
    pub archives: Vec<ArchiveStatus>,
    pub matches: BTreeMap<Vec<u8>, Vec<ArchiveMatch>>,
}

/// Inspect every immediate BSA index once and retain all normalized name hits.
/// No archive payload is opened or precedence/winner rule applied.
pub(crate) fn inspect(data_dir: &Path, requested: &BTreeSet<AssetPath>) -> Result<Lookup> {
    let data_meta = fs::symlink_metadata(data_dir)?;
    if data_meta.file_type().is_symlink() || !data_meta.is_dir() {
        return Err(Error::Unsupported(
            "data root must be a regular non-symlink directory".into(),
        ));
    }
    let data_dir = data_dir.canonicalize()?;
    let mut summary = Summary::default();
    let mut matches = BTreeMap::<Vec<u8>, Vec<ArchiveMatch>>::new();
    let mut archives = Vec::new();
    let mut paths = Vec::new();
    for entry in fs::read_dir(&data_dir)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bsa"))
        {
            paths.push(path);
        }
    }
    paths.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
    });

    for path in paths {
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<non-unicode archive name>")
            .to_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            summary.archive_index_failures += 1;
            archives.push(ArchiveStatus {
                file,
                bytes: None,
                sha256: None,
                entries: None,
                matching_candidate_paths: 0,
                hash_only_entries: 0,
                invalid_asset_paths: 0,
                failure: Some("archive is not a regular non-symlink file".into()),
            });
            continue;
        }
        let mut archive = match archive::SkyrimArchive::open(&path) {
            Ok(archive) => archive,
            Err(error) => {
                summary.archive_index_failures += 1;
                archives.push(ArchiveStatus {
                    file,
                    bytes: Some(metadata.len()),
                    sha256: None,
                    entries: None,
                    matching_candidate_paths: 0,
                    hash_only_entries: 0,
                    invalid_asset_paths: 0,
                    failure: Some(error.to_string()),
                });
                continue;
            }
        };
        match archive.match_paths(requested) {
            Ok(query) => {
                summary.archives_indexed += 1;
                summary.archive_hash_only_entries += query.hash_only_entries;
                summary.archive_invalid_paths += query.invalid_asset_paths;
                for hit in &query.matches {
                    summary.duplicate_archive_candidate_entries += hit.duplicate_entries;
                    matches
                        .entry(hit.normalized_path.clone())
                        .or_default()
                        .push(ArchiveMatch {
                            archive: query.file.clone(),
                            actual_member_paths: hit.actual_member_paths.clone(),
                            duplicate_entries: hit.duplicate_entries,
                        });
                }
                archives.push(ArchiveStatus {
                    file: query.file,
                    bytes: Some(query.bytes),
                    sha256: Some(query.sha256),
                    entries: Some(query.entries),
                    matching_candidate_paths: query.matches.len() as u64,
                    hash_only_entries: query.hash_only_entries,
                    invalid_asset_paths: query.invalid_asset_paths,
                    failure: None,
                });
            }
            Err(error) => {
                summary.archive_index_failures += 1;
                archives.push(ArchiveStatus {
                    file,
                    bytes: Some(metadata.len()),
                    sha256: None,
                    entries: None,
                    matching_candidate_paths: 0,
                    hash_only_entries: 0,
                    invalid_asset_paths: 0,
                    failure: Some(error.to_string()),
                });
            }
        }
    }
    Ok(Lookup {
        summary,
        archives,
        matches,
    })
}

pub(crate) fn loose_file_status(root: &Path, path: &[u8]) -> &'static str {
    let Ok(text) = std::str::from_utf8(path) else {
        return "unrepresentable-path-bytes";
    };
    let mut current = PathBuf::from(root);
    let components = text.split('/').collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return "absent",
            Err(_) => return "inspection-error",
        };
        if metadata.file_type().is_symlink() {
            return "symlink-encountered";
        }
        if index + 1 < components.len() && !metadata.is_dir() {
            return "parent-not-directory";
        }
        if index + 1 == components.len() {
            return if metadata.is_file() {
                "present"
            } else {
                "not-a-file"
            };
        }
    }
    "absent"
}
