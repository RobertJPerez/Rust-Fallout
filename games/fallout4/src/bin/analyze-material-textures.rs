//! Physical BA2 candidate search driven by the isolated BGSM/BGEM oracle output.
//! This does not choose a runtime archive/loose-file winner.
use dream_archive::ByteSlice;
use fallout_data::vfs::AssetPath;
use fallout4_prep::{
    Error, Result,
    archive::{Ba2, Limits},
    census,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

const REFERENCE_REVISION: &str = "21411873f17454a2442d6785d533499e14c63adb";
const MAX_ARCHIVES: usize = 512;
const MAX_REFERENCES: usize = 100_000;
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_CANDIDATES: usize = 1_000_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleSummary {
    reference_revision: String,
    results_sha256: String,
    bgsm: usize,
    bgem: usize,
    parsed: usize,
    json_parsed: usize,
    trailing_bytes: usize,
    errors: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleRow {
    archive: String,
    archive_sha256: String,
    entry_index: usize,
    member_name_bytes_hex: String,
    kind: String,
    bytes: usize,
    sha256: String,
    state: String,
    error: Option<String>,
    extracted_sha256: String,
    textures: Vec<OracleTexture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleTexture {
    field: String,
    value: String,
}

#[derive(Clone, Debug, Serialize)]
struct TextureUse {
    archive: String,
    material_entry_index: usize,
    material_member_name_bytes_hex: String,
    material_kind: String,
    material_sha256: String,
    field: String,
    authored_path: String,
}

#[derive(Debug, Serialize)]
struct Candidate {
    archive: String,
    archive_sha256: String,
    entry_index: usize,
    member_name_bytes_hex: String,
}

#[derive(Debug, Serialize)]
struct TextureRequest {
    canonical_path: String,
    authored_spellings: Vec<String>,
    references: Vec<TextureUse>,
    candidates: Vec<Candidate>,
}

#[derive(Debug, Serialize)]
struct InvalidTextureReference {
    authored_path: String,
    error: String,
    use_site: TextureUse,
}

#[derive(Debug, Serialize)]
struct ArchiveObservation {
    archive: String,
    archive_sha256: Option<String>,
    entries: usize,
    candidate_members: usize,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Manifest {
    schema_version: u32,
    reference_revision: String,
    oracle_results_sha256: String,
    profile: fallout_data::identity::ProfileId,
    observation_complete: bool,
    runtime_ready: bool,
    archives_scanned: usize,
    archives_with_errors: usize,
    archive_entries_scanned: usize,
    unique_texture_paths: usize,
    texture_reference_slots: usize,
    missing_unique_paths: usize,
    ambiguous_unique_paths: usize,
    candidate_members: usize,
    invalid_texture_references: Vec<InvalidTextureReference>,
    archives: Vec<ArchiveObservation>,
    textures: Vec<TextureRequest>,
    scope: &'static str,
}

#[derive(Debug, Serialize)]
struct Completion {
    schema_version: u32,
    manifest_sha256: String,
    oracle_results_sha256: String,
    archives_scanned: usize,
    archives_with_errors: usize,
    unique_texture_paths: usize,
    texture_reference_slots: usize,
    missing_unique_paths: usize,
    ambiguous_unique_paths: usize,
    candidate_members: usize,
    invalid_texture_references: usize,
    observation_complete: bool,
    runtime_ready: bool,
}

fn texture_key(raw: &[u8]) -> Result<AssetPath> {
    if raw.len() > 4096 {
        return Err(Error::Unsupported("texture path exceeds 4096 bytes".into()));
    }
    let path = AssetPath::new(raw)?;
    if path.bytes().starts_with(b"textures/") {
        return Ok(path);
    }
    let mut rooted = b"textures/".to_vec();
    rooted.extend_from_slice(path.bytes());
    AssetPath::new(&rooted).map_err(Into::into)
}

fn hex(raw: &[u8]) -> String {
    raw.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_stable(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata_before = fs::metadata(path)?;
    if !metadata_before.is_file() || metadata_before.len() > limit {
        return Err(Error::Unsupported(format!(
            "{}: source file type or byte budget exceeded",
            path.display()
        )));
    }
    let bytes = fs::read(path)?;
    let metadata_after = fs::metadata(path)?;
    if bytes.len() as u64 != metadata_before.len()
        || metadata_before.len() != metadata_after.len()
        || metadata_before.modified().ok() != metadata_after.modified().ok()
    {
        return Err(Error::Unsupported(format!(
            "{} changed during evidence read",
            path.display()
        )));
    }
    Ok(bytes)
}

fn archive_paths(data: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(data)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ba2"))
        {
            paths.push(entry.path());
        }
        if paths.len() > MAX_ARCHIVES {
            return Err(Error::Unsupported(format!(
                "archive count exceeds {MAX_ARCHIVES}"
            )));
        }
    }
    paths.sort_by(|left, right| {
        left.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(
                &right
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase(),
            )
    });
    Ok(paths)
}

fn oracle_rows(evidence: &Path) -> Result<(OracleSummary, Vec<OracleRow>)> {
    let summary_path = evidence.join("complete.json");
    let results_path = evidence.join("material-reference.jsonl");
    let summary_bytes = read_stable(&summary_path, 1024 * 1024)?;
    let results_bytes = read_stable(&results_path, MAX_SOURCE_BYTES)?;
    let summary: OracleSummary = serde_json::from_slice(&summary_bytes)?;
    let actual_hash = format!("{:x}", Sha256::digest(&results_bytes));
    if summary.reference_revision != REFERENCE_REVISION
        || summary.results_sha256 != actual_hash
        || summary.errors != 0
        || summary.trailing_bytes != 0
        || summary.parsed + summary.json_parsed != summary.bgsm + summary.bgem
    {
        return Err(Error::Unsupported(
            "material oracle summary/revision/hash is incomplete or inconsistent".into(),
        ));
    }
    let mut rows = Vec::new();
    for line in results_bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.len() > MAX_LINE_BYTES {
            return Err(Error::Unsupported(
                "material oracle row exceeds line byte budget".into(),
            ));
        }
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        if !line.is_empty() {
            rows.try_reserve(1)
                .map_err(|error| Error::Unsupported(format!("oracle rows: {error}")))?;
            rows.push(serde_json::from_slice::<OracleRow>(line)?);
            if rows.len() > MAX_REFERENCES {
                return Err(Error::Unsupported(format!(
                    "material count exceeds {MAX_REFERENCES}"
                )));
            }
        }
    }
    if rows.len() != summary.bgsm + summary.bgem
        || rows.iter().any(|row| {
            row.state != "parsed"
                || row.error.is_some()
                || row.sha256 != row.extracted_sha256
                || !matches!(row.kind.as_str(), "bgsm" | "bgem")
                || row.archive_sha256.len() != 64
                || row.member_name_bytes_hex.len() % 2 != 0
                || row.bytes == 0
        })
    {
        return Err(Error::Unsupported(
            "material oracle rows failed count, state, or identity validation".into(),
        ));
    }
    Ok((summary, rows))
}

fn output_directory(install: &Path, evidence: &Path, requested: &Path) -> Result<PathBuf> {
    let install = fs::canonicalize(install)?;
    let evidence = fs::canonicalize(evidence)?;
    let local = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("local"))?;
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)?;
    let name = requested
        .file_name()
        .ok_or_else(|| Error::Unsupported("new texture-analysis directory name required".into()))?;
    let output = parent.join(name);
    if output.starts_with(&install) || output.starts_with(&evidence) || !output.starts_with(&local)
    {
        return Err(Error::Unsupported(
            "output must be inside ignored local/ and outside the installation and oracle evidence"
                .into(),
        ));
    }
    Ok(output)
}

fn run(
    install_root: &Path,
    evidence: &Path,
    requested_output: &Path,
) -> Result<(Manifest, Completion)> {
    let install = fs::canonicalize(install_root)?;
    let evidence = fs::canonicalize(evidence)?;
    let output = output_directory(&install, &evidence, requested_output)?;
    let (summary, rows) = oracle_rows(&evidence)?;
    fs::create_dir(&output)?;

    let mut grouped = BTreeMap::<AssetPath, TextureRequest>::new();
    let mut spellings = BTreeMap::<AssetPath, BTreeSet<String>>::new();
    let mut invalid = Vec::new();
    let mut texture_reference_slots = 0usize;
    for row in &rows {
        for texture in &row.textures {
            if texture.value.is_empty() {
                continue;
            }
            texture_reference_slots = texture_reference_slots
                .checked_add(1)
                .ok_or_else(|| Error::Unsupported("texture reference count overflow".into()))?;
            let use_site = TextureUse {
                archive: row.archive.clone(),
                material_entry_index: row.entry_index,
                material_member_name_bytes_hex: row.member_name_bytes_hex.clone(),
                material_kind: row.kind.clone(),
                material_sha256: row.sha256.clone(),
                field: texture.field.clone(),
                authored_path: texture.value.clone(),
            };
            match texture_key(texture.value.as_bytes()) {
                Ok(key) => {
                    spellings
                        .entry(key.clone())
                        .or_default()
                        .insert(texture.value.clone());
                    grouped
                        .entry(key)
                        .and_modify(|request| request.references.push(use_site.clone()))
                        .or_insert_with(|| TextureRequest {
                            canonical_path: String::new(),
                            authored_spellings: Vec::new(),
                            references: vec![use_site],
                            candidates: Vec::new(),
                        });
                }
                Err(error) => invalid.push(InvalidTextureReference {
                    authored_path: texture.value.clone(),
                    error: error.to_string(),
                    use_site,
                }),
            }
        }
    }
    if grouped.len() > MAX_REFERENCES {
        return Err(Error::Unsupported(format!(
            "unique texture path count exceeds {MAX_REFERENCES}"
        )));
    }
    let mut textures = Vec::with_capacity(grouped.len());
    let mut requested_keys = BTreeMap::<AssetPath, usize>::new();
    for (key, mut request) in grouped {
        request.canonical_path = String::from_utf8_lossy(key.bytes()).into_owned();
        request.authored_spellings = spellings
            .remove(&key)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let index = textures.len();
        requested_keys.insert(key, index);
        textures.push(request);
    }

    let data = install.join("Data");
    let paths = archive_paths(&data)?;
    let mut archives = Vec::with_capacity(paths.len());
    let mut archive_entries_scanned = 0usize;
    let mut candidate_members = 0usize;
    for path in paths {
        let archive_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let metadata_before = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                archives.push(ArchiveObservation {
                    archive: archive_name,
                    archive_sha256: None,
                    entries: 0,
                    candidate_members: 0,
                    error: Some(error.to_string()),
                });
                continue;
            }
        };
        let digest_before = match census::hash_file(&path) {
            Ok(digest) => digest,
            Err(error) => {
                archives.push(ArchiveObservation {
                    archive: archive_name,
                    archive_sha256: None,
                    entries: 0,
                    candidate_members: 0,
                    error: Some(error.to_string()),
                });
                continue;
            }
        };
        let archive = match Ba2::open(&path, Limits::default()) {
            Ok(archive) => archive,
            Err(error) => {
                archives.push(ArchiveObservation {
                    archive: archive_name,
                    archive_sha256: Some(digest_before),
                    entries: 0,
                    candidate_members: 0,
                    error: Some(error.to_string()),
                });
                continue;
            }
        };
        let entries = archive.entries().len();
        let mut local_candidates = Vec::new();
        for (entry_index, entry) in archive.entries().iter().enumerate() {
            if entry.name().is_empty() {
                continue;
            }
            let key = match AssetPath::new(entry.name().as_bytes()) {
                Ok(key) => key,
                Err(_) => continue, // Ba2::open already reported invalid named paths.
            };
            if requested_keys.contains_key(&key) {
                local_candidates.push((key, entry_index, hex(entry.name().as_bytes())));
            }
        }
        drop(archive);
        let metadata_after = fs::metadata(&path);
        let digest_after = census::hash_file(&path);
        let stable = metadata_after.as_ref().is_ok_and(|metadata| {
            metadata.len() == metadata_before.len()
                && metadata.modified().ok() == metadata_before.modified().ok()
        }) && digest_after
            .as_ref()
            .is_ok_and(|digest| digest == &digest_before);
        if !stable {
            archives.push(ArchiveObservation {
                archive: archive_name,
                archive_sha256: digest_after.ok(),
                entries,
                candidate_members: 0,
                error: Some("source archive changed during candidate indexing".into()),
            });
            continue;
        }
        for (key, entry_index, member_name_bytes_hex) in &local_candidates {
            let request_index = requested_keys[key];
            textures[request_index].candidates.push(Candidate {
                archive: archive_name.clone(),
                archive_sha256: digest_before.clone(),
                entry_index: *entry_index,
                member_name_bytes_hex: member_name_bytes_hex.clone(),
            });
        }
        archive_entries_scanned = archive_entries_scanned
            .checked_add(entries)
            .ok_or_else(|| Error::Unsupported("archive entry count overflow".into()))?;
        candidate_members = candidate_members
            .checked_add(local_candidates.len())
            .ok_or_else(|| Error::Unsupported("texture candidate count overflow".into()))?;
        if candidate_members > MAX_CANDIDATES {
            return Err(Error::Unsupported(format!(
                "candidate member count exceeds {MAX_CANDIDATES}"
            )));
        }
        archives.push(ArchiveObservation {
            archive: archive_name,
            archive_sha256: Some(digest_before),
            entries,
            candidate_members: local_candidates.len(),
            error: None,
        });
    }

    let archives_with_errors = archives
        .iter()
        .filter(|archive| archive.error.is_some())
        .count();
    let missing_unique_paths = textures
        .iter()
        .filter(|request| request.candidates.is_empty())
        .count();
    let ambiguous_unique_paths = textures
        .iter()
        .filter(|request| request.candidates.len() > 1)
        .count();
    let observation_complete = archives_with_errors == 0 && invalid.is_empty();
    let manifest = Manifest {
        schema_version: 1,
        reference_revision: summary.reference_revision,
        oracle_results_sha256: summary.results_sha256,
        profile: fallout_data::identity::ProfileId::Fo4Original,
        observation_complete,
        runtime_ready: false,
        archives_scanned: archives.len(),
        archives_with_errors,
        archive_entries_scanned,
        unique_texture_paths: textures.len(),
        texture_reference_slots,
        missing_unique_paths,
        ambiguous_unique_paths,
        candidate_members,
        invalid_texture_references: invalid,
        archives,
        textures,
        scope: "Texture slot names resolved only to physical candidate members in direct Data BA2 archives. No active profile, loose-file scan, archive winner, path fallback beyond the implicit Textures root, rendering, or runtime readiness is asserted.",
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    fs::write(output.join("manifest.json"), &manifest_bytes)?;
    let completion = Completion {
        schema_version: 1,
        manifest_sha256: census::sha256(&manifest_bytes),
        oracle_results_sha256: manifest.oracle_results_sha256.clone(),
        archives_scanned: manifest.archives_scanned,
        archives_with_errors: manifest.archives_with_errors,
        unique_texture_paths: manifest.unique_texture_paths,
        texture_reference_slots: manifest.texture_reference_slots,
        missing_unique_paths: manifest.missing_unique_paths,
        ambiguous_unique_paths: manifest.ambiguous_unique_paths,
        candidate_members: manifest.candidate_members,
        invalid_texture_references: manifest.invalid_texture_references.len(),
        observation_complete: manifest.observation_complete,
        runtime_ready: manifest.runtime_ready,
    };
    fs::write(
        output.join("complete.json"),
        serde_json::to_vec_pretty(&completion)?,
    )?;
    Ok((manifest, completion))
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--help") || args.is_empty() {
        println!(
            "analyze-material-textures <install-root> <oracle-evidence-directory> <new-local-directory>\nOutput remains candidate-only; exit 2 means source/path observation incomplete."
        );
        return;
    }
    if args.len() != 3 {
        eprintln!("invalid arguments; use --help");
        std::process::exit(1);
    }
    match run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
    ) {
        Ok((manifest, completion)) => {
            match serde_json::to_writer_pretty(std::io::stdout().lock(), &completion) {
                Ok(()) => println!(),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            if !manifest.observation_complete {
                std::process::exit(2);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_paths_get_safe_implicit_root_and_ascii_case_folding() {
        assert_eq!(
            texture_key(br"AnimObjects\PipBoy\X_D.DDS").unwrap().bytes(),
            b"textures/animobjects/pipboy/x_d.dds"
        );
        assert_eq!(
            texture_key(b"Textures/AnimObjects/X.dds").unwrap().bytes(),
            b"textures/animobjects/x.dds"
        );
    }

    #[test]
    fn unsafe_or_unbounded_texture_paths_fail_closed() {
        for path in [
            b"../outside.dds".as_slice(),
            b"C:\\outside.dds",
            b"/outside.dds",
            b"x//y.dds",
        ] {
            assert!(texture_key(path).is_err(), "{path:?}");
        }
        assert!(texture_key(&vec![b'x'; 4097]).is_err());
    }

    #[test]
    fn output_must_be_new_local_and_separate_from_install_and_oracle() {
        let root = tempfile::tempdir_in({
            let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("local");
            fs::create_dir_all(&local).unwrap();
            local
        })
        .unwrap();
        let install = root.path().join("game");
        let evidence = root.path().join("oracle");
        fs::create_dir_all(install.join("Data")).unwrap();
        fs::create_dir(&evidence).unwrap();
        assert!(output_directory(&install, &evidence, &install.join("output")).is_err());
        assert!(output_directory(&install, &evidence, &evidence.join("output")).is_err());
        assert!(output_directory(&install, &evidence, &root.path().join("output")).is_ok());
    }
}
