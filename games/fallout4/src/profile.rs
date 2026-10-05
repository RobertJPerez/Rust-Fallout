//! Fallout 4 profile-file observations. These inputs do not establish the
//! executable's effective activation, virtualized files, or plugin winners.
use crate::{Error, Result, census};
use fallout_data::identity::{self, ProfileId};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::Path,
    time::UNIX_EPOCH,
};

const MAX_PROFILE_BYTES: usize = 1024 * 1024;
const MAX_PROFILE_LINES: usize = 65_536;
const MAX_NAME_BYTES: usize = 1024;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FileState {
    Missing,
    Empty,
    Present,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DataStatus {
    UniqueFile,
    AmbiguousFiles,
    MissingFromDataTree,
    OnlyUnfollowedLink,
    InvalidName,
}

#[derive(Debug, Serialize)]
pub struct PluginRow {
    pub line: usize,
    /// Position among noncomment named rows in this file, in authored file order.
    pub ordinal: usize,
    pub authored_name: String,
    pub name_bytes_hex: String,
    pub normalized_name: Option<String>,
    /// A literal leading `*` observation, not proof the game activated this file.
    pub starred: bool,
    pub data_status: DataStatus,
    pub data_candidates: Vec<String>,
    pub duplicate_of_line: Option<usize>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProfileFile {
    pub logical_name: &'static str,
    pub state: FileState,
    pub bytes: Option<usize>,
    pub sha256: Option<String>,
    pub modified_unix_seconds: Option<u64>,
    pub rows: Vec<PluginRow>,
    pub ignored_lines: usize,
    pub malformed_lines: usize,
    pub starred_rows: usize,
    pub unstarred_rows: usize,
    pub rows_unique_in_data: usize,
    pub rows_missing_from_data: usize,
    pub rows_ambiguous_in_data: usize,
}

#[derive(Debug, Serialize)]
pub struct PhysicalPlugin {
    pub filename: String,
    pub normalized_name: Option<String>,
    pub kind: &'static str,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
    pub modified_unix_seconds: Option<u64>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProfileReport {
    pub schema_version: u32,
    pub profile: ProfileId,
    pub observation_only: bool,
    pub observation_complete: bool,
    pub runtime_ready: bool,
    pub physical_plugins: Vec<PhysicalPlugin>,
    pub plugins_txt: ProfileFile,
    pub loadorder_txt: ProfileFile,
    pub starred_names_absent_from_loadorder: Vec<String>,
    pub loadorder_names_not_in_plugins: Vec<String>,
    pub scope: &'static str,
}

impl ProfileReport {
    /// A complete observation has both files, valid unique plugin names, and a
    /// single regular-file candidate in Data for every named row. This is not a
    /// statement of activation or gameplay compatibility.
    fn calculated_complete(&self) -> bool {
        fn file_complete(file: &ProfileFile) -> bool {
            matches!(file.state, FileState::Present)
                && file.malformed_lines == 0
                && !file.rows.is_empty()
                && file.rows.iter().all(|row| {
                    row.duplicate_of_line.is_none()
                        && matches!(row.data_status, DataStatus::UniqueFile)
                        && row.diagnostics.is_empty()
                })
        }
        file_complete(&self.plugins_txt)
            && file_complete(&self.loadorder_txt)
            && self
                .physical_plugins
                .iter()
                .all(|plugin| plugin.normalized_name.is_some() && plugin.diagnostic.is_none())
    }
}

struct DataIndex {
    files: BTreeMap<String, Vec<String>>,
    links: BTreeMap<String, Vec<String>>,
}

fn plugin_extension(name: &[u8]) -> bool {
    name.rsplit(|byte| *byte == b'.').next().is_some_and(|ext| {
        ext.eq_ignore_ascii_case(b"esm")
            || ext.eq_ignore_ascii_case(b"esp")
            || ext.eq_ignore_ascii_case(b"esl")
    })
}

fn data_index(data: &Path) -> Result<(DataIndex, Vec<PhysicalPlugin>)> {
    let mut index = DataIndex {
        files: BTreeMap::new(),
        links: BTreeMap::new(),
    };
    let mut physical = Vec::new();
    for entry in fs::read_dir(data)? {
        let entry = entry?;
        let entry_path = entry.path();
        let Some(extension) = entry_path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        if !["esm", "esp", "esl"]
            .iter()
            .any(|expected| extension.eq_ignore_ascii_case(expected))
        {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().into_owned();
        let normalized_name = identity::plugin_name(&filename).ok();
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            let metadata = entry.metadata()?;
            let sha256 = census::hash_file(&entry_path)?;
            if let Some(name) = normalized_name.as_ref() {
                index
                    .files
                    .entry(name.clone())
                    .or_default()
                    .push(filename.clone());
            }
            physical.push(PhysicalPlugin {
                filename,
                normalized_name,
                kind: "regular-file",
                bytes: Some(metadata.len()),
                sha256: Some(sha256),
                modified_unix_seconds: metadata
                    .modified()
                    .ok()
                    .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                    .map(|value| value.as_secs()),
                diagnostic: None,
            });
        } else if file_type.is_symlink() {
            if let Some(name) = normalized_name.as_ref() {
                index
                    .links
                    .entry(name.clone())
                    .or_default()
                    .push(filename.clone());
            }
            physical.push(PhysicalPlugin {
                filename,
                normalized_name,
                kind: "symlink-not-followed",
                bytes: None,
                sha256: None,
                modified_unix_seconds: None,
                diagnostic: Some("link target was not followed".into()),
            });
        }
    }
    physical.sort_by(|a, b| {
        a.filename
            .to_ascii_lowercase()
            .cmp(&b.filename.to_ascii_lowercase())
    });
    for names in index.files.values_mut().chain(index.links.values_mut()) {
        names.sort_by_key(|name| name.to_ascii_lowercase());
    }
    Ok((index, physical))
}

fn trimmed_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn parse_present_file(
    logical_name: &'static str,
    bytes: &[u8],
    modified_unix_seconds: Option<u64>,
    index: &DataIndex,
) -> Result<ProfileFile> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(Error::Unsupported(format!(
            "{logical_name} exceeds the {MAX_PROFILE_BYTES}-byte observation limit"
        )));
    }
    let line_count = bytes.iter().filter(|b| **b == b'\n').count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
    if line_count > MAX_PROFILE_LINES {
        return Err(Error::Unsupported(format!(
            "{logical_name} exceeds the {MAX_PROFILE_LINES}-line observation limit"
        )));
    }
    let mut rows = Vec::new();
    let mut ignored_lines = 0;
    let mut malformed_lines = 0;
    let mut starred_rows = 0;
    let mut unstarred_rows = 0;
    let mut rows_unique_in_data = 0;
    let mut rows_missing_from_data = 0;
    let mut rows_ambiguous_in_data = 0;
    let mut seen = HashMap::<String, usize>::new();

    for (line_idx, original) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line_idx >= line_count {
            break;
        }
        let mut line = original.strip_suffix(b"\r").unwrap_or(original);
        if line_idx == 0 {
            line = line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(line);
        }
        let line = trimmed_ascii(line);
        if line.is_empty() || line.starts_with(b"#") {
            ignored_lines += 1;
            continue;
        }
        let mut diagnostics = Vec::new();
        if line
            .iter()
            .any(|byte| *byte == 0 || *byte == b'\r' || *byte < 0x20 && *byte != b'\t')
        {
            diagnostics.push("control byte in plugin list row".into());
        }
        let starred = line.starts_with(b"*");
        let name = if starred { &line[1..] } else { line };
        if name.starts_with(b"*") {
            diagnostics.push("multiple leading activation markers".into());
        }
        if name.is_empty() {
            diagnostics.push("empty plugin name".into());
        }
        if name.len() > MAX_NAME_BYTES {
            diagnostics.push("plugin name exceeds byte limit".into());
        }
        if !plugin_extension(name) {
            diagnostics.push("unrecognized plugin extension".into());
        }
        let authored_name = String::from_utf8_lossy(name).into_owned();
        let normalized_name = match identity::plugin_name(&authored_name) {
            Ok(value) if authored_name.as_bytes() == name => Some(value),
            _ => {
                diagnostics.push("plugin identity is not a safe ASCII filename".into());
                None
            }
        };
        let duplicate_of_line = normalized_name
            .as_ref()
            .and_then(|normalized| seen.get(normalized).copied());
        if let Some(normalized) = normalized_name.as_ref() {
            seen.entry(normalized.clone()).or_insert(line_idx + 1);
        }
        let (data_status, data_candidates) = match normalized_name.as_ref() {
            None => (DataStatus::InvalidName, Vec::new()),
            Some(normalized) => {
                let files = index.files.get(normalized).cloned().unwrap_or_default();
                let links = index.links.get(normalized).cloned().unwrap_or_default();
                let mut candidates = files.clone();
                candidates.extend(links);
                match (files.len(), candidates.len()) {
                    (1, 1) => {
                        rows_unique_in_data += 1;
                        (DataStatus::UniqueFile, candidates)
                    }
                    (_, count) if count > 1 => {
                        rows_ambiguous_in_data += 1;
                        (DataStatus::AmbiguousFiles, candidates)
                    }
                    (0, 1) => {
                        rows_missing_from_data += 1;
                        diagnostics.push(
                            "only a symlink candidate exists; its target was not read".into(),
                        );
                        (DataStatus::OnlyUnfollowedLink, candidates)
                    }
                    _ => {
                        rows_missing_from_data += 1;
                        (DataStatus::MissingFromDataTree, Vec::new())
                    }
                }
            }
        };
        if starred {
            starred_rows += 1;
        } else {
            unstarred_rows += 1;
        }
        if !diagnostics.is_empty() || duplicate_of_line.is_some() {
            malformed_lines += 1;
        }
        rows.push(PluginRow {
            line: line_idx + 1,
            ordinal: rows.len() + 1,
            authored_name,
            name_bytes_hex: name.iter().map(|b| format!("{b:02x}")).collect(),
            normalized_name,
            starred,
            data_status,
            data_candidates,
            duplicate_of_line,
            diagnostics,
        });
    }
    Ok(ProfileFile {
        logical_name,
        state: if bytes.is_empty() {
            FileState::Empty
        } else {
            FileState::Present
        },
        bytes: Some(bytes.len()),
        sha256: Some(census::sha256(bytes)),
        modified_unix_seconds,
        rows,
        ignored_lines,
        malformed_lines,
        starred_rows,
        unstarred_rows,
        rows_unique_in_data,
        rows_missing_from_data,
        rows_ambiguous_in_data,
    })
}

fn observe_file(path: &Path, logical_name: &'static str, index: &DataIndex) -> Result<ProfileFile> {
    match fs::File::open(path) {
        Ok(file) => {
            let before = file.metadata()?;
            if before.len() > MAX_PROFILE_BYTES as u64 {
                return Err(Error::Unsupported(format!(
                    "{logical_name} exceeds the {MAX_PROFILE_BYTES}-byte observation limit"
                )));
            }
            let mut reader = file.take(MAX_PROFILE_BYTES as u64 + 1);
            let mut bytes = Vec::with_capacity(before.len() as usize);
            reader.read_to_end(&mut bytes)?;
            if bytes.len() > MAX_PROFILE_BYTES {
                return Err(Error::Unsupported(format!(
                    "{logical_name} grew beyond the {MAX_PROFILE_BYTES}-byte observation limit while reading"
                )));
            }
            let after = reader.get_ref().metadata()?;
            if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
                return Err(Error::Unsupported(format!(
                    "{logical_name} changed while the profile snapshot was being read"
                )));
            }
            let modified_unix_seconds = after
                .modified()
                .ok()
                .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
                .map(|value| value.as_secs());
            parse_present_file(logical_name, &bytes, modified_unix_seconds, index)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ProfileFile {
            logical_name,
            state: FileState::Missing,
            bytes: None,
            sha256: None,
            modified_unix_seconds: None,
            rows: Vec::new(),
            ignored_lines: 0,
            malformed_lines: 0,
            starred_rows: 0,
            unstarred_rows: 0,
            rows_unique_in_data: 0,
            rows_missing_from_data: 0,
            rows_ambiguous_in_data: 0,
        }),
        Err(error) => Err(error.into()),
    }
}

fn names(file: &ProfileFile) -> BTreeMap<String, String> {
    file.rows
        .iter()
        .filter_map(|row| {
            row.normalized_name
                .as_ref()
                .map(|normalized| (normalized.clone(), row.authored_name.clone()))
        })
        .collect()
}

pub fn observe(install: &Path, profile_root: &Path) -> Result<ProfileReport> {
    let data = install.join("Data");
    let (index, physical_plugins) = data_index(&data)?;
    let plugins_txt = observe_file(&profile_root.join("plugins.txt"), "plugins.txt", &index)?;
    let loadorder_txt = observe_file(&profile_root.join("loadorder.txt"), "loadorder.txt", &index)?;
    let plugin_names = names(&plugins_txt);
    let loadorder_names = names(&loadorder_txt);
    let starred_plugin_names = plugins_txt
        .rows
        .iter()
        .filter(|row| row.starred)
        .filter_map(|row| row.normalized_name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let starred_names_absent_from_loadorder = starred_plugin_names
        .into_iter()
        .filter(|name| !loadorder_names.contains_key(name))
        .collect();
    let loadorder_names_not_in_plugins = loadorder_names
        .keys()
        .filter(|name| !plugin_names.contains_key(*name))
        .cloned()
        .collect();
    let mut report = ProfileReport {
        schema_version: 1,
        profile: ProfileId::Fo4Original,
        observation_only: true,
        observation_complete: false,
        runtime_ready: false,
        physical_plugins,
        plugins_txt,
        loadorder_txt,
        starred_names_absent_from_loadorder,
        loadorder_names_not_in_plugins,
        scope: "Explicit plugins.txt/loadorder.txt bytes and direct Data-directory filename candidates only; no executable profile proof, VFS/deployment scan, ESL rebasing, master validation, archive precedence, record winners, VMAD relevance, or gameplay",
    };
    report.observation_complete = report.calculated_complete();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(names: &[&str]) -> DataIndex {
        DataIndex {
            files: names
                .iter()
                .map(|name| (name.to_ascii_lowercase(), vec![(*name).into()]))
                .collect(),
            links: BTreeMap::new(),
        }
    }

    #[test]
    fn parses_bom_comments_crlf_markers_and_case_folded_duplicates() {
        let bytes = b"\xef\xbb\xbf# generated\r\n*Fallout4.esm\r\nDLC.esm\r\n*fallout4.ESM\r\n";
        let file = parse_present_file(
            "plugins.txt",
            bytes,
            None,
            &index(&["Fallout4.esm", "DLC.esm"]),
        )
        .unwrap();
        assert_eq!(file.sha256.as_deref(), Some(census::sha256(bytes).as_str()));
        assert_eq!(file.ignored_lines, 1);
        assert_eq!(file.rows.len(), 3);
        assert!(file.rows[0].starred);
        assert!(!file.rows[1].starred);
        assert_eq!(file.rows[2].duplicate_of_line, Some(2));
        assert_eq!(file.rows[2].data_status, DataStatus::UniqueFile);
        assert_eq!(file.rows_unique_in_data, 3);
        assert_eq!(file.malformed_lines, 1);
    }

    #[test]
    fn malformed_names_and_control_bytes_are_reported_without_becoming_paths() {
        let bytes = b"*..\\outside.esl\n*bad/name.esp\n*\xff.esm\nvalid.esl\0tail\n";
        let file =
            parse_present_file("loadorder.txt", bytes, None, &index(&["valid.esl"])).unwrap();
        assert_eq!(file.rows.len(), 4);
        assert!(file.rows.iter().all(|row| !row.diagnostics.is_empty()));
        assert_eq!(file.rows[0].data_status, DataStatus::InvalidName);
        assert_eq!(file.rows[2].normalized_name, None);
        assert_eq!(file.rows[3].data_status, DataStatus::InvalidName);
        assert!(
            file.rows[3]
                .diagnostics
                .iter()
                .any(|item| item.contains("control byte"))
        );
        assert_eq!(file.malformed_lines, 4);
    }

    #[test]
    fn missing_profile_files_are_explicit_and_do_not_count_as_complete() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("game");
        let data = install.join("Data");
        let profile = dir.path().join("profile");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&profile).unwrap();
        let report = observe(&install, &profile).unwrap();
        assert!(matches!(report.plugins_txt.state, FileState::Missing));
        assert!(matches!(report.loadorder_txt.state, FileState::Missing));
        assert!(!report.observation_complete);
        assert!(!report.runtime_ready);
    }

    #[test]
    fn matching_explicit_lists_and_regular_data_files_complete_only_the_observation() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("game");
        let data = install.join("Data");
        let profile = dir.path().join("profile");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&profile).unwrap();
        fs::write(data.join("Fallout4.esm"), b"bounded filename fixture").unwrap();
        fs::write(profile.join("plugins.txt"), b"*Fallout4.esm\n").unwrap();
        fs::write(profile.join("loadorder.txt"), b"Fallout4.esm\n").unwrap();
        let report = observe(&install, &profile).unwrap();
        assert!(report.observation_complete);
        assert!(!report.runtime_ready);
        assert_eq!(report.plugins_txt.rows_unique_in_data, 1);
        assert_eq!(report.loadorder_txt.rows_unique_in_data, 1);
        assert!(report.starred_names_absent_from_loadorder.is_empty());
    }

    #[test]
    fn rejects_oversized_profile_bytes_before_parsing_rows() {
        let bytes = vec![b'x'; MAX_PROFILE_BYTES + 1];
        assert!(parse_present_file("plugins.txt", &bytes, None, &index(&[])).is_err());
    }
}
