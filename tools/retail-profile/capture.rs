//! Read-only retail profile capture. Staging files does not establish launch isolation:
//! the original executable asks Windows for user folders outside those staged roots.
use fallout_data::{baseline, vfs::profile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[path = "verification.rs"]
mod verification;
pub use verification::verify;

#[derive(Serialize)]
struct Roots {
    installation: PathBuf,
    documents: PathBuf,
    local_appdata: PathBuf,
    documents_resolution: &'static str,
    local_appdata_resolution: &'static str,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Tree {
    scope: String,
    present: bool,
    data: Option<TreeData>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TreeData {
    root: PathBuf,
    files: Vec<baseline::Fingerprint>,
    content_fingerprint: String,
    digest_recipe: String,
    missing_required: Vec<String>,
}

#[derive(Serialize)]
struct Copy {
    source: &'static str,
    destination: String,
    bytes: usize,
    sha256: String,
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_new(path, &bytes)
}

#[cfg(windows)]
fn inspect_userdata_entry(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    use std::{os::windows::fs::MetadataExt, process::Command};
    if metadata.file_type().is_symlink() {
        return Err(format!("symbolic link in user-data tree: {}", path.display()).into());
    }
    // Refuse offline/recall files before opening them. Capture records content,
    // without asking a cloud provider to download missing host bytes.
    if metadata.file_attributes() & (0x1000 | 0x40000 | 0x400000) != 0 {
        return Err(format!(
            "user-data cloud placeholder is not resident: {}",
            path.display()
        )
        .into());
    }
    if metadata.file_attributes() & 0x400 == 0 {
        return Ok(());
    }
    // OneDrive placeholders are reparse points too. Accept only Microsoft's cloud
    // family, whose tags do not redirect names. Junctions and unknown tags refuse.
    let windows = std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable")?;
    let output = Command::new(PathBuf::from(windows).join("System32/fsutil.exe"))
        .args(["reparsepoint", "query"])
        .arg(path)
        .output()?;
    if !output.status.success() || output.stdout.len() > 128 * 1024 {
        return Err(format!(
            "cannot classify user-data reparse point: {}",
            path.display()
        )
        .into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let tag = parse_reparse_tag(&text).ok_or("unrecognized fsutil reparse tag output")?;
    if !cloud_tag(tag) {
        return Err(format!(
            "unsupported user-data reparse tag {tag:#010x}: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn inspect_userdata_entry(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    if metadata.file_type().is_symlink() {
        return Err(format!("symbolic link in user-data tree: {}", path.display()).into());
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn cloud_tag(tag: u32) -> bool {
    tag & 0xffff_0fff == 0x9000_001a
}

#[cfg(any(windows, test))]
fn parse_reparse_tag(text: &str) -> Option<u32> {
    let mut tokens = text
        .lines()
        .next()?
        .split_ascii_whitespace()
        .filter(|t| t.starts_with("0x"));
    let token = tokens.next()?;
    if token.len() != 10 || tokens.next().is_some() {
        return None;
    }
    u32::from_str_radix(&token[2..], 16).ok()
}

fn userdata(root: &Path) -> Result<TreeData> {
    inspect_userdata_entry(root, &fs::symlink_metadata(root)?)?;
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut entries = 0usize;
    let mut total_bytes = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            entries += 1;
            if entries > 65_536 {
                return Err("user-data entry budget exceeded".into());
            }
            let metadata = fs::symlink_metadata(&path)?;
            inspect_userdata_entry(&path, &metadata)?;
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                total_bytes = total_bytes
                    .checked_add(metadata.len())
                    .ok_or("user-data byte budget exceeded")?;
                if total_bytes > 32 * 1024 * 1024 * 1024 {
                    return Err("user-data byte budget exceeded".into());
                }
                let (bytes, sha256) = baseline::digest_file(&path)?;
                if bytes != metadata.len() {
                    return Err("user-data length changed during fingerprint".into());
                }
                let spelling = path
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or("non-Unicode user-data path")?
                    .replace('\\', "/");
                files.push(baseline::Fingerprint {
                    path: spelling,
                    bytes,
                    sha256,
                });
            } else {
                return Err("unsupported user-data entry type".into());
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut names = std::collections::BTreeSet::new();
    for file in &files {
        if !names.insert(file.path.to_ascii_lowercase()) {
            return Err("case collision in user-data manifest".into());
        }
    }
    let content_fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&files)?));
    Ok(TreeData {
        root: root.to_path_buf(),
        files,
        content_fingerprint,
        digest_recipe:
            "SHA256 of compact UTF-8 JSON files sorted by path; fields path,bytes,sha256".into(),
        missing_required: Vec::new(),
    })
}

fn fingerprint(scope: &'static str, root: &Path) -> Result<Tree> {
    match root.try_exists()? {
        false => Ok(Tree {
            scope: scope.into(),
            present: false,
            data: None,
        }),
        true => {
            eprintln!("Fingerprinting {scope}: {}", root.display());
            let data = if scope == "installation" {
                let original = baseline::discover(root, |_| {})?;
                TreeData {
                    root: original.installation,
                    files: original.files,
                    content_fingerprint: original.content_fingerprint,
                    missing_required: original.missing_required,
                    digest_recipe: "fallout-content-set-v1 length-prefixed manifest".into(),
                }
            } else {
                userdata(root)?
            };
            Ok(Tree {
                scope: scope.into(),
                present: true,
                data: Some(data),
            })
        }
    }
}

fn bind_profile(snapshot: &profile::Snapshot, trees: &[Tree]) -> Result<()> {
    for source in &snapshot.sources {
        let (index, relative) = if let Some(path) = source.name.strip_prefix("installation/") {
            (0, path)
        } else if let Some(path) = source.name.strip_prefix("documents/My Games/FalloutNV/") {
            (1, path)
        } else if let Some(path) = source.name.strip_prefix("local-appdata/FalloutNV/") {
            (2, path)
        } else {
            return Err("profile source has an unknown root".into());
        };
        let fingerprint = trees[index].data.as_ref().and_then(|tree| {
            tree.files
                .iter()
                .find(|f| source_name_matches(&f.path, relative))
        });
        let matches = match (source.state == profile::State::Missing, fingerprint) {
            (true, None) => true,
            (false, Some(file)) => {
                source.bytes == usize::try_from(file.bytes).ok()
                    && source.sha256.as_ref() == Some(&file.sha256)
            }
            _ => false,
        };
        if !matches {
            return Err(format!(
                "captured profile source does not match tree fingerprint: {}",
                source.name
            )
            .into());
        }
    }
    Ok(())
}

fn source_name_matches(actual: &str, requested: &str) -> bool {
    if cfg!(windows) {
        actual.eq_ignore_ascii_case(requested)
    } else {
        actual == requested
    }
}

fn fingerprints(roots: &Roots) -> Result<Vec<Tree>> {
    Ok(vec![
        fingerprint("installation", &roots.installation)?,
        fingerprint(
            "documents-game-data-including-saves",
            &roots.documents.join("My Games/FalloutNV"),
        )?,
        fingerprint(
            "local-appdata-game-data",
            &roots.local_appdata.join("FalloutNV"),
        )?,
    ])
}

/// The parent must exist so canonicalization checks junctions before any write.
/// An existing destination is refused, including a dangling link or old partial capture.
fn destination(path: &Path, roots: &Roots) -> Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or("capture directory needs a final component")?;
    if name == "." || name == ".." {
        return Err("capture directory needs a normal final component".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    let path = parent.join(name);
    for source in [&roots.installation, &roots.documents, &roots.local_appdata] {
        if path.starts_with(source) || source.starts_with(&path) {
            return Err(
                "capture directory overlaps an original installation or user-data root".into(),
            );
        }
    }
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            return Err(
                "capture directory already exists; preserve it and choose a fresh directory".into(),
            );
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(path)
}

#[cfg(windows)]
fn known_folder(name: &str) -> Result<PathBuf> {
    use std::process::Command;
    // These names are fixed by the caller, never interpolated from CLI input.
    let script = match name {
        "documents" => {
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);[Environment]::GetFolderPath([Environment+SpecialFolder]::MyDocuments)"
        }
        "local-appdata" => {
            "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)"
        }
        _ => return Err("unknown Windows known folder".into()),
    };
    let windows = std::env::var_os("SystemRoot").ok_or("SystemRoot is unavailable")?;
    let shell = PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = Command::new(shell)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()?;
    if !output.status.success() {
        return Err(format!("Windows known-folder resolution failed: {name}").into());
    }
    let text = std::str::from_utf8(&output.stdout)?.trim();
    if text.is_empty() || text.contains(['\r', '\n']) {
        return Err(
            format!("Windows known-folder resolution returned an invalid path: {name}").into(),
        );
    }
    Ok(PathBuf::from(text))
}

#[cfg(not(windows))]
fn known_folder(_: &str) -> Result<PathBuf> {
    Err("explicit Documents and LocalAppData roots are required outside Windows".into())
}

fn resolve(path: Option<&Path>, name: &str) -> Result<(PathBuf, &'static str)> {
    let (path, method) = match path {
        Some(path) if !path.as_os_str().is_empty() => (path.to_path_buf(), "explicit"),
        Some(_) => return Err("profile roots cannot be empty".into()),
        None => (known_folder(name)?, "windows-known-folder-dotnet"),
    };
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err("profile root must be a directory".into());
    }
    Ok((path, method))
}

pub fn capture(
    install: &Path,
    documents: Option<&Path>,
    local_appdata: Option<&Path>,
    package: &Path,
) -> Result<()> {
    let (documents, documents_resolution) = resolve(documents, "documents")?;
    let (local_appdata, local_appdata_resolution) = resolve(local_appdata, "local-appdata")?;
    let installation = install.canonicalize()?;
    let roots = Roots {
        installation,
        documents,
        local_appdata,
        documents_resolution,
        local_appdata_resolution,
    };
    let package = destination(package, &roots)?;
    let before = fingerprints(&roots)?;
    let observed = profile::observe(
        &roots.installation,
        &roots.documents,
        &roots.local_appdata,
        Default::default(),
    )?;
    bind_profile(&observed, &before)?;
    let installation = before[0]
        .data
        .as_ref()
        .ok_or("installation does not exist")?;
    if !installation.missing_required.is_empty() {
        return Err(format!(
            "original profile is incomplete: {:?}",
            installation.missing_required
        )
        .into());
    }
    let executable = installation
        .files
        .iter()
        .find(|f| f.path.eq_ignore_ascii_case("FalloutNV.exe"))
        .ok_or("original executable is missing")?;
    let producer_sha256 = baseline::digest_file(&std::env::current_exe()?)?.1;

    fs::create_dir(&package)?;
    write_json(&package.join("before.json"), &before)?;
    write_json(&package.join("profile.json"), &observed)?;
    let mut copies = Vec::new();
    for source in &observed.sources {
        if source.state == profile::State::Missing {
            continue;
        }
        let destination = format!("captured/{}", source.name);
        let path = package.join(&destination);
        fs::create_dir_all(path.parent().ok_or("captured source lacks parent")?)?;
        write_new(&path, source.raw_bytes())?;
        let (bytes, sha256) = baseline::digest_file(&path)?;
        if Some(bytes as usize) != source.bytes || Some(&sha256) != source.sha256.as_ref() {
            return Err("captured raw profile file identity differs from source".into());
        }
        copies.push(Copy {
            source: source.name,
            destination,
            bytes: bytes as usize,
            sha256,
        });

        // Only configuration files enter the private staging area. Existing saves,
        // executable and assets are never linked or copied into a writable launch root.
        if source.name.starts_with("documents/") || source.name.starts_with("local-appdata/") {
            let path = package.join("staged").join(source.name);
            fs::create_dir_all(path.parent().ok_or("staged source lacks parent")?)?;
            write_new(&path, source.raw_bytes())?;
        }
    }
    fs::create_dir_all(package.join("staged/documents/My Games/FalloutNV/Saves"))?;
    fs::create_dir_all(package.join("staged/local-appdata/FalloutNV"))?;
    let after = fingerprints(&roots)?;
    write_json(&package.join("after.json"), &after)?;
    let profile_after = profile::observe(
        &roots.installation,
        &roots.documents,
        &roots.local_appdata,
        Default::default(),
    )?;
    bind_profile(&observed, &after)?;
    bind_profile(&profile_after, &after)?;
    if serde_json::to_vec(&before)? != serde_json::to_vec(&after)?
        || serde_json::to_vec(&observed)? != serde_json::to_vec(&profile_after)?
    {
        return Err("original source/user-data changed during capture; partial evidence retained, no completed receipt".into());
    }
    let receipt = serde_json::json!({
        "schema_version": 1,
        "profile": "nv-original",
        "producer_binary_sha256": producer_sha256,
        "input_roots": roots,
        "executable": executable,
        "installation_content_fingerprint": installation.content_fingerprint,
        "profile_report": {"path":"profile.json", "sha256":baseline::digest_file(&package.join("profile.json"))?.1},
        "before_report": {"path":"before.json", "sha256":baseline::digest_file(&package.join("before.json"))?.1},
        "after_report": {"path":"after.json", "sha256":baseline::digest_file(&package.join("after.json"))?.1},
        "captured_files": copies,
        "staged_documents": "staged/documents",
        "staged_local_appdata": "staged/local-appdata",
        "original_source_and_save_hashes_unchanged": true,
        "original_process_started": false,
        "installation_copy_ready": false,
        "launch_admitted": false,
        "isolation_backend": null,
        "blocked": ["No demonstrated per-process or separate-user filesystem isolation backend; original known-folder access is not redirected by this staging package.", "No retail trace or runtime-selected profile was measured."],
        "runtime_ready": false,
        "faithful_scenario_accepted": false
    });
    write_json(&package.join("capture.json"), &receipt)?;
    eprintln!(
        "Captured immutable profile: {}",
        package.join("capture.json").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "fallout-profile-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            for dir in [
                "install/Data",
                "redirected-documents/My Games/FalloutNV/Saves",
                "appdata/FalloutNV",
                "output",
            ] {
                fs::create_dir_all(path.join(dir)).unwrap();
            }
            fs::write(
                path.join("install/FalloutNV.exe"),
                b"authored executable identity",
            )
            .unwrap();
            for plugin in baseline::OFFICIAL_PLUGINS {
                fs::write(path.join("install/Data").join(plugin), plugin.as_bytes()).unwrap();
            }
            fs::write(
                path.join("redirected-documents/My Games/FalloutNV/Fallout.ini"),
                b"[General]\r\nSTestFile1=FalloutNV.esm\r\nSTestFile1=\r\n",
            )
            .unwrap();
            fs::write(
                path.join("redirected-documents/My Games/FalloutNV/Saves/private.fos"),
                b"private save content",
            )
            .unwrap();
            fs::write(path.join("appdata/FalloutNV/plugins.txt"), b"").unwrap();
            Self(path)
        }
        fn capture(&self, output: &Path) -> Result<()> {
            capture(
                &self.0.join("install"),
                Some(&self.0.join("redirected-documents")),
                Some(&self.0.join("appdata")),
                output,
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn captures_redirected_roots_raw_duplicates_and_empty_files_without_copying_saves() {
        let f = Fixture::new();
        let output = f.0.join("output/capture");
        f.capture(&output).unwrap();
        let receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("capture.json")).unwrap()).unwrap();
        assert_eq!(receipt["launch_admitted"], false);
        assert_eq!(receipt["original_process_started"], false);
        assert_eq!(receipt["original_source_and_save_hashes_unchanged"], true);
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("profile.json")).unwrap()).unwrap();
        assert_eq!(
            report["sources"][1]["lines"][2]["content"]["duplicate_of"],
            2
        );
        assert_eq!(report["sources"][3]["state"], "empty");
        assert_eq!(report["sources"][4]["state"], "missing");
        let raw =
            fs::read(f.0.join("redirected-documents/My Games/FalloutNV/Fallout.ini")).unwrap();
        assert_eq!(
            fs::read(output.join("captured/documents/My Games/FalloutNV/Fallout.ini")).unwrap(),
            raw
        );
        assert_eq!(
            fs::read(output.join("staged/documents/My Games/FalloutNV/Fallout.ini")).unwrap(),
            raw
        );
        assert_eq!(
            fs::read_dir(output.join("staged/documents/My Games/FalloutNV/Saves"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read(output.join("before.json")).unwrap(),
            fs::read(output.join("after.json")).unwrap()
        );
    }

    #[test]
    fn rejects_original_roots_and_existing_partial_packages_before_writing() {
        let f = Fixture::new();
        for root in ["install", "redirected-documents", "appdata"] {
            let output = f.0.join(root).join("forbidden");
            assert!(
                f.capture(&output)
                    .unwrap_err()
                    .to_string()
                    .contains("overlaps")
            );
            assert!(!output.exists());
        }
        let existing = f.0.join("output/partial");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("keep.txt"), b"preserved evidence").unwrap();
        assert!(
            f.capture(&existing)
                .unwrap_err()
                .to_string()
                .contains("already exists")
        );
        assert_eq!(
            fs::read(existing.join("keep.txt")).unwrap(),
            b"preserved evidence"
        );
    }

    #[cfg(windows)]
    #[test]
    fn canonicalizes_directory_junction_before_checking_protected_roots() {
        use std::process::Command;
        let f = Fixture::new();
        let alias = f.0.join("alias");
        // Command receives only unique test paths, never user-provided shell text.
        let result = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&alias)
            .arg(f.0.join("redirected-documents"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = alias.join("forbidden");
        assert!(
            f.capture(&output)
                .unwrap_err()
                .to_string()
                .contains("overlaps")
        );
        fs::remove_dir(&alias).unwrap();
    }

    #[test]
    fn incomplete_installation_does_not_create_completed_evidence() {
        let f = Fixture::new();
        fs::remove_file(f.0.join("install/Data/DeadMoney.esm")).unwrap();
        let output = f.0.join("output/incomplete");
        assert!(
            f.capture(&output)
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );
        assert!(!output.exists());
    }

    #[cfg(windows)]
    #[test]
    fn matches_windows_source_spelling_without_losing_manifest_name() {
        let f = Fixture::new();
        let root = f.0.join("redirected-documents/My Games/FalloutNV");
        fs::rename(root.join("Fallout.ini"), root.join("FALLOUT.INI")).unwrap();
        f.capture(&f.0.join("output/case-variant")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn user_data_junction_is_refused_without_fingerprinting_external_bytes() {
        use std::process::Command;
        let f = Fixture::new();
        let link = f.0.join("redirected-documents/My Games/FalloutNV/redirect");
        let result = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(f.0.join("install"))
            .output()
            .unwrap();
        assert!(result.status.success());
        let output = f.0.join("output/unsafe-junction");
        let error = f.capture(&output).unwrap_err().to_string();
        assert!(
            error.contains("reparse") || error.contains("symbolic link"),
            "{error}"
        );
        assert!(!output.exists());
        fs::remove_dir(&link).unwrap();
    }

    #[test]
    fn accepts_only_cloud_family_and_unambiguous_first_line_tag() {
        for index in 0..16 {
            assert!(cloud_tag(0x9000_001a | (index << 12)));
        }
        for tag in [
            0xa000_0003,
            0xa000_000c,
            0x8000_0017,
            0x9000_001b,
            0x9001_001a,
        ] {
            assert!(!cloud_tag(tag));
        }
        assert_eq!(
            parse_reparse_tag("Reparse Tag Value : 0x9000201a\r\nReparse Data Length: 0x195"),
            Some(0x9000201a)
        );
        assert_eq!(
            parse_reparse_tag("localized label: 0x9000e01a\n"),
            Some(0x9000e01a)
        );
        for text in [
            "0x9000201a 0xa0000003",
            "0x9000201",
            "error\n0x9000201a",
            "0x9000201z",
        ] {
            assert_eq!(parse_reparse_tag(text), None);
        }
    }

    #[test]
    fn profile_bytes_and_missing_sources_must_belong_to_manifest() {
        let f = Fixture::new();
        let (documents, documents_resolution) =
            resolve(Some(&f.0.join("redirected-documents")), "documents").unwrap();
        let (local_appdata, local_appdata_resolution) =
            resolve(Some(&f.0.join("appdata")), "local-appdata").unwrap();
        let roots = Roots {
            installation: f.0.join("install").canonicalize().unwrap(),
            documents,
            local_appdata,
            documents_resolution,
            local_appdata_resolution,
        };
        let observed = profile::observe(
            &roots.installation,
            &roots.documents,
            &roots.local_appdata,
            Default::default(),
        )
        .unwrap();
        let mut trees = fingerprints(&roots).unwrap();
        bind_profile(&observed, &trees).unwrap();
        let ini = trees[1]
            .data
            .as_mut()
            .unwrap()
            .files
            .iter_mut()
            .find(|f| f.path == "Fallout.ini")
            .unwrap();
        ini.sha256 = "different bytes".into();
        assert!(bind_profile(&observed, &trees).is_err());
        trees = fingerprints(&roots).unwrap();
        trees[2]
            .data
            .as_mut()
            .unwrap()
            .files
            .push(baseline::Fingerprint {
                path: "NVDLCList.txt".into(),
                bytes: 0,
                sha256: format!("{:x}", Sha256::digest([])),
            });
        assert!(bind_profile(&observed, &trees).is_err());
    }

    fn captured(fixture: &Fixture) -> PathBuf {
        let package = fixture.0.join("output/sealed");
        fixture.capture(&package).unwrap();
        package
    }

    fn receipt_hash(package: &Path) -> String {
        baseline::digest_file(&package.join("capture.json"))
            .unwrap()
            .1
    }

    fn alter_receipt(package: &Path, change: impl FnOnce(&mut serde_json::Value)) -> String {
        let path = package.join("capture.json");
        let mut value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut value);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        receipt_hash(package)
    }

    #[test]
    fn sealed_profile_verifies_but_cannot_satisfy_process_acceptance() {
        let f = Fixture::new();
        let package = captured(&f);
        let hash = receipt_hash(&package);
        let result = verify(&package, &hash, false).unwrap();
        assert_eq!(result.profile_sources, 7);
        assert_eq!(result.captured_files, 2);
        assert!(!result.original_process_started);
        assert!(!result.runtime_ready);
        assert!(!result.faithful_scenario_accepted);
        assert!(
            verify(&package, &hash, true)
                .unwrap_err()
                .to_string()
                .contains("no original process")
        );
        assert!(
            verify(&package, &"0".repeat(64), false)
                .unwrap_err()
                .to_string()
                .contains("expected identity")
        );
    }

    #[test]
    fn changed_retained_and_staged_configurations_refuse() {
        for relative in [
            "captured/documents/My Games/FalloutNV/Fallout.ini",
            "staged/documents/My Games/FalloutNV/Fallout.ini",
        ] {
            let f = Fixture::new();
            let package = captured(&f);
            let hash = receipt_hash(&package);
            fs::write(
                package.join(relative),
                b"[General]\nSTestFile1=another.esm\n",
            )
            .unwrap();
            assert!(verify(&package, &hash, false).is_err(), "{relative}");
            assert_eq!(
                fs::read(f.0.join("redirected-documents/My Games/FalloutNV/Saves/private.fos"))
                    .unwrap(),
                b"private save content"
            );
        }
    }

    #[test]
    fn staging_cannot_add_a_configuration_reported_as_missing() {
        let f = Fixture::new();
        let package = captured(&f);
        let hash = receipt_hash(&package);
        fs::write(
            package.join("staged/local-appdata/FalloutNV/NVDLCList.txt"),
            b"unreported.esm\n",
        )
        .unwrap();
        assert!(
            verify(&package, &hash, false)
                .unwrap_err()
                .to_string()
                .contains("source presence")
        );
    }

    #[test]
    fn resealed_profile_offsets_cannot_replace_raw_source_evidence() {
        let f = Fixture::new();
        let package = captured(&f);
        let path = package.join("profile.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["sources"][1]["lines"][2]["content"]["duplicate_of"] = 99.into();
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let altered = baseline::digest_file(&path).unwrap().1;
        let hash = alter_receipt(&package, |receipt| {
            receipt["profile_report"]["sha256"] = altered.into()
        });
        assert!(
            verify(&package, &hash, false)
                .unwrap_err()
                .to_string()
                .contains("retained source bytes")
        );
    }

    #[test]
    fn resealed_manifest_file_change_cannot_hide_behind_unchanged_set_digest() {
        let f = Fixture::new();
        let package = captured(&f);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(package.join("before.json")).unwrap()).unwrap();
        value[0]["data"]["files"][0]["bytes"] = 999.into();
        let bytes = serde_json::to_vec(&value).unwrap();
        for name in ["before.json", "after.json"] {
            fs::write(package.join(name), &bytes).unwrap();
        }
        let altered = format!("{:x}", Sha256::digest(&bytes));
        let hash = alter_receipt(&package, |receipt| {
            receipt["before_report"]["sha256"] = altered.clone().into();
            receipt["after_report"]["sha256"] = altered.into();
        });
        assert!(
            verify(&package, &hash, false)
                .unwrap_err()
                .to_string()
                .contains("content fingerprint")
        );
    }

    #[test]
    fn unsupported_execution_claims_and_foreign_profile_refuse() {
        for (field, value) in [
            ("original_process_started", serde_json::json!(true)),
            ("launch_admitted", serde_json::json!(true)),
            ("runtime_ready", serde_json::json!(true)),
            ("faithful_scenario_accepted", serde_json::json!(true)),
            (
                "isolation_backend",
                serde_json::json!("environment-override"),
            ),
            ("profile", serde_json::json!("fo3-original")),
            ("schema_version", serde_json::json!(2)),
        ] {
            let f = Fixture::new();
            let package = captured(&f);
            let hash = alter_receipt(&package, |receipt| receipt[field] = value);
            assert!(verify(&package, &hash, false).is_err(), "{field}");
        }
    }

    #[test]
    fn missing_reports_escaping_pins_and_copied_saves_refuse() {
        let f = Fixture::new();
        let package = captured(&f);
        let hash = alter_receipt(&package, |receipt| {
            receipt["before_report"]["path"] = "../before.json".into()
        });
        assert!(
            verify(&package, &hash, false)
                .unwrap_err()
                .to_string()
                .contains("unexpected member")
        );
        let f = Fixture::new();
        let package = captured(&f);
        let hash = receipt_hash(&package);
        fs::write(
            package.join("staged/documents/My Games/FalloutNV/Saves/extra.fos"),
            b"unexpected",
        )
        .unwrap();
        assert!(
            verify(&package, &hash, false)
                .unwrap_err()
                .to_string()
                .contains("save data")
        );
        fs::remove_file(package.join("before.json")).unwrap();
        assert!(verify(&package, &hash, false).is_err());
    }
}
