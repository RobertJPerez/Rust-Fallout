//! Verify retained profile evidence without opening or launching the installation.
use super::{Result, Tree, bind_profile};
use fallout_data::{baseline, vfs::profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportPin {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Roots {
    installation: PathBuf,
    documents: PathBuf,
    local_appdata: PathBuf,
    documents_resolution: String,
    local_appdata_resolution: String,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Copy {
    source: String,
    destination: String,
    bytes: usize,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    profile: String,
    producer_binary_sha256: String,
    input_roots: Roots,
    executable: baseline::Fingerprint,
    installation_content_fingerprint: String,
    profile_report: ReportPin,
    before_report: ReportPin,
    after_report: ReportPin,
    captured_files: Vec<Copy>,
    staged_documents: String,
    staged_local_appdata: String,
    original_source_and_save_hashes_unchanged: bool,
    original_process_started: bool,
    installation_copy_ready: bool,
    launch_admitted: bool,
    isolation_backend: Option<String>,
    blocked: Vec<String>,
    runtime_ready: bool,
    faithful_scenario_accepted: bool,
}

#[derive(Debug, Serialize)]
pub struct Verification {
    pub schema_version: u32,
    pub profile: &'static str,
    pub capture_receipt_sha256: String,
    pub producer_binary_sha256: String,
    pub executable_sha256: String,
    pub installation_content_fingerprint: String,
    pub profile_sources: usize,
    pub captured_files: usize,
    pub scope: &'static str,
    pub original_process_started: bool,
    pub runtime_ready: bool,
    pub faithful_scenario_accepted: bool,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("capture SHA-256 must be 64 lowercase hexadecimal digits".into());
    }
    Ok(())
}

/// Reject aliases before opening a member. Only package-relative normal components
/// are allowed; Windows alternate streams and slash aliases cannot name evidence.
fn member(package: &Path, relative: &str) -> Result<PathBuf> {
    if relative.is_empty()
        || relative.contains(['\\', ':'])
        || relative
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err("invalid capture member path".into());
    }
    let mut path = package.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err("capture member must stay inside its package".into());
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("capture member is a symbolic link".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err("capture member is a reparse point".into());
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

fn read(package: &Path, relative: &str, maximum: u64) -> Result<Vec<u8>> {
    let path = member(package, relative)?;
    let file = baseline::open_source(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err("capture member type or byte budget exceeded".into());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != metadata.len() || bytes.len() as u64 > maximum {
        return Err("capture member length changed or byte budget exceeded".into());
    }
    Ok(bytes)
}

fn report(package: &Path, pin: &ReportPin, name: &str) -> Result<Vec<u8>> {
    sha256(&pin.sha256)?;
    if pin.path != name {
        return Err("capture report pin names an unexpected member".into());
    }
    let bytes = read(package, name, 32 * 1024 * 1024)?;
    if digest(&bytes) != pin.sha256 {
        return Err(format!("capture report digest differs: {name}").into());
    }
    Ok(bytes)
}

fn validate_tree(tree: &Tree, installation: bool) -> Result<()> {
    if tree.present != tree.data.is_some() {
        return Err("capture manifest presence disagrees with its data".into());
    }
    let Some(data) = &tree.data else {
        return Ok(());
    };
    sha256(&data.content_fingerprint)?;
    if !data.root.is_absolute() || !data.missing_required.is_empty() || data.files.len() > 1_000_000
    {
        return Err("capture manifest has invalid roots, missing inputs or too many files".into());
    }
    let mut names = BTreeSet::new();
    let mut previous: Option<&str> = None;
    let mut total_bytes = 0u64;
    for file in &data.files {
        sha256(&file.sha256)?;
        if file.path.is_empty()
            || file.path.contains(['\\', ':'])
            || file
                .path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || previous.is_some_and(|p| p >= file.path.as_str())
            || !names.insert(file.path.to_ascii_lowercase())
        {
            return Err("capture manifest has unordered, duplicate or invalid file names".into());
        }
        previous = Some(&file.path);
        if !installation {
            total_bytes = total_bytes
                .checked_add(file.bytes)
                .ok_or("capture user-data byte budget exceeded")?;
        }
    }
    if !installation && (data.files.len() > 65_536 || total_bytes > 32 * 1024 * 1024 * 1024) {
        return Err("capture user-data manifest budget exceeded".into());
    }
    let calculated = if installation {
        if data.digest_recipe != "fallout-content-set-v1 length-prefixed manifest" {
            return Err("unsupported installation fingerprint recipe".into());
        }
        let mut set = Sha256::new();
        set.update(b"fallout-content-set-v1\0");
        for file in &data.files {
            set.update((file.path.len() as u64).to_le_bytes());
            set.update(file.path.as_bytes());
            set.update(file.bytes.to_le_bytes());
            set.update(file.sha256.as_bytes());
        }
        for required in std::iter::once("FalloutNV.exe".to_owned()).chain(
            baseline::OFFICIAL_PLUGINS
                .iter()
                .map(|p| format!("Data/{p}")),
        ) {
            if !names.contains(&required.to_ascii_lowercase()) {
                return Err("capture installation manifest lacks a required input".into());
            }
        }
        format!("{:x}", set.finalize())
    } else {
        if data.digest_recipe
            != "SHA256 of compact UTF-8 JSON files sorted by path; fields path,bytes,sha256"
        {
            return Err("unsupported user-data fingerprint recipe".into());
        }
        digest(&serde_json::to_vec(&data.files)?)
    };
    if calculated != data.content_fingerprint {
        return Err("capture content fingerprint disagrees with manifest files".into());
    }
    Ok(())
}

pub fn verify(
    package: &Path,
    expected_sha256: &str,
    require_process: bool,
) -> Result<Verification> {
    sha256(expected_sha256)?;
    let package = package.canonicalize()?;
    let bytes = read(&package, "capture.json", 1024 * 1024)?;
    if digest(&bytes) != expected_sha256 {
        return Err("capture receipt digest differs from expected identity".into());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes)?;
    if receipt.schema_version != 1 || receipt.profile != "nv-original" {
        return Err("unsupported capture schema or profile".into());
    }
    if !receipt.original_source_and_save_hashes_unchanged
        || receipt.original_process_started
        || receipt.installation_copy_ready
        || receipt.launch_admitted
        || receipt.isolation_backend.is_some()
        || receipt.runtime_ready
        || receipt.faithful_scenario_accepted
        || receipt.blocked.is_empty()
    {
        return Err(
            "profile-only capture contains unsupported execution or acceptance claims".into(),
        );
    }
    sha256(&receipt.producer_binary_sha256)?;
    sha256(&receipt.executable.sha256)?;
    sha256(&receipt.installation_content_fingerprint)?;
    let before = report(&package, &receipt.before_report, "before.json")?;
    let after = report(&package, &receipt.after_report, "after.json")?;
    if before != after {
        return Err("capture before/after source manifests differ".into());
    }
    let trees: Vec<Tree> = serde_json::from_slice(&before)?;
    let roots = &receipt.input_roots;
    let expected = [
        ("installation", roots.installation.clone()),
        (
            "documents-game-data-including-saves",
            roots.documents.join("My Games/FalloutNV"),
        ),
        (
            "local-appdata-game-data",
            roots.local_appdata.join("FalloutNV"),
        ),
    ];
    if trees.len() != expected.len()
        || !roots.installation.is_absolute()
        || !roots.documents.is_absolute()
        || !roots.local_appdata.is_absolute()
        || !["explicit", "windows-known-folder-dotnet"]
            .contains(&roots.documents_resolution.as_str())
        || !["explicit", "windows-known-folder-dotnet"]
            .contains(&roots.local_appdata_resolution.as_str())
    {
        return Err("capture roots or source scopes are invalid".into());
    }
    for (index, (tree, (scope, root))) in trees.iter().zip(expected).enumerate() {
        if tree.scope != scope || tree.data.as_ref().is_some_and(|data| data.root != root) {
            return Err("capture source root or scope disagrees with receipt".into());
        }
        validate_tree(tree, index == 0)?;
    }
    let install = trees[0]
        .data
        .as_ref()
        .ok_or("capture installation manifest is absent")?;
    if install.content_fingerprint != receipt.installation_content_fingerprint
        || !install.files.iter().any(|file| {
            file.path == receipt.executable.path
                && file.bytes == receipt.executable.bytes
                && file.sha256 == receipt.executable.sha256
        })
        || !receipt
            .executable
            .path
            .eq_ignore_ascii_case("FalloutNV.exe")
    {
        return Err("capture executable or installation identity disagrees with manifest".into());
    }
    let reported_profile: serde_json::Value =
        serde_json::from_slice(&report(&package, &receipt.profile_report, "profile.json")?)?;
    // Reuse the production parser so forged offsets/duplicates or missing-source
    // claims cannot be accepted merely because a report carries a matching hash.
    for name in [
        "installation/Fallout_default.ini",
        "installation/Data/ArchiveInvalidation.txt",
        "documents/My Games/FalloutNV/Fallout.ini",
        "documents/My Games/FalloutNV/FalloutPrefs.ini",
        "local-appdata/FalloutNV/plugins.txt",
        "local-appdata/FalloutNV/NVDLCList.txt",
        "local-appdata/FalloutNV/loadorder.txt",
    ] {
        member(&package, &format!("captured/{name}"))?;
        if name.starts_with("documents/") || name.starts_with("local-appdata/") {
            member(&package, &format!("staged/{name}"))?;
        }
    }
    let captured = package.join("captured");
    let observed = profile::observe(
        &captured.join("installation"),
        &captured.join("documents"),
        &captured.join("local-appdata"),
        Default::default(),
    )?;
    if serde_json::to_value(&observed)? != reported_profile {
        return Err("capture profile report disagrees with retained source bytes".into());
    }
    bind_profile(&observed, &trees)?;
    let staged = package.join("staged");
    let staged_profile = profile::observe(
        &captured.join("installation"),
        &staged.join("documents"),
        &staged.join("local-appdata"),
        Default::default(),
    )?;
    if serde_json::to_value(&staged_profile)? != reported_profile {
        return Err("staged configuration differs from captured source presence or bytes".into());
    }
    let mut expected_copies = Vec::new();
    for source in &observed.sources {
        if source.state == profile::State::Missing {
            continue;
        }
        let hash = source
            .sha256
            .clone()
            .ok_or("captured source has no digest")?;
        expected_copies.push(Copy {
            source: source.name.into(),
            destination: format!("captured/{}", source.name),
            bytes: source.raw_bytes().len(),
            sha256: hash,
        });
    }
    if receipt.captured_files != expected_copies
        || receipt.staged_documents != "staged/documents"
        || receipt.staged_local_appdata != "staged/local-appdata"
    {
        return Err("capture copy inventory or staged roots disagree".into());
    }
    let saves = member(&package, "staged/documents/My Games/FalloutNV/Saves")?;
    if fs::read_dir(saves)?.next().is_some() {
        return Err("profile capture staging contains save data".into());
    }
    if require_process {
        return Err(
            "profile-only capture has no original process or demonstrated isolation evidence"
                .into(),
        );
    }
    Ok(Verification {
        schema_version: 1,
        profile: "nv-original",
        capture_receipt_sha256: expected_sha256.into(),
        producer_binary_sha256: receipt.producer_binary_sha256,
        executable_sha256: receipt.executable.sha256,
        installation_content_fingerprint: receipt.installation_content_fingerprint,
        profile_sources: observed.sources.len(),
        captured_files: expected_copies.len(),
        scope: "Retained profile evidence only; original inputs are not remeasured and no process is launched.",
        original_process_started: false,
        runtime_ready: false,
        faithful_scenario_accepted: false,
    })
}
