use crate::{Error, Result, io};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

pub const OFFICIAL_PLUGINS: &[&str] = &[
    "FalloutNV.esm",
    "DeadMoney.esm",
    "HonestHearts.esm",
    "OldWorldBlues.esm",
    "LonesomeRoad.esm",
    "GunRunnersArsenal.esm",
    "CaravanPack.esm",
    "ClassicPack.esm",
    "MercenaryPack.esm",
    "TribalPack.esm",
];

/// Windows denies writes/deletes while a source is open. On other platforms the
/// caller must keep the installation immutable during inspection.
pub fn open_source(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // FILE_SHARE_READ
    }
    options.open(path).map_err(|e| io(path, e))
}

#[derive(Debug, Clone, Serialize)]
pub struct Fingerprint {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Baseline {
    pub schema_version: u32,
    pub profile: &'static str,
    pub installation: PathBuf,
    pub content_fingerprint: String,
    pub files: Vec<Fingerprint>,
    pub missing_required: Vec<String>,
    pub unverified: Vec<&'static str>,
}

pub fn digest_reader(reader: &mut impl Read) -> std::io::Result<(u64, String)> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    let mut size = 0;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        digest.update(&buffer[..n]);
    }
    Ok((size, format!("{:x}", digest.finalize())))
}

pub fn digest_file(path: &Path) -> Result<(u64, String)> {
    digest_reader(&mut open_source(path)?).map_err(|e| io(path, e))
}

pub fn sorted_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut entries_seen = 0usize;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| io(&dir, e))? {
            let entry = entry.map_err(|e| io(&dir, e))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|e| io(&path, e))?;
            entries_seen += 1;
            if entries_seen > 1_000_000 {
                return Err(Error::Resolution(
                    "installation entry budget exceeded".into(),
                ));
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(Error::Unsupported(format!(
                        "reparse point in source tree: {}",
                        path.display()
                    )));
                }
            }
            if metadata.file_type().is_symlink() {
                return Err(Error::Unsupported(format!(
                    "symbolic link in source tree: {}",
                    path.display()
                )));
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn discover(root: &Path, mut progress: impl FnMut(&Path)) -> Result<Baseline> {
    let root = root.canonicalize().map_err(|e| io(root, e))?;
    let mut files = Vec::new();
    let mut names = BTreeSet::new();
    for path in sorted_files(&root)? {
        let relative = path
            .strip_prefix(&root)
            .map_err(|e| Error::Resolution(e.to_string()))?;
        let spelling = relative
            .to_str()
            .ok_or_else(|| Error::Unsupported("non-Unicode filesystem path".into()))?
            .replace('\\', "/");
        if !names.insert(spelling.to_ascii_lowercase()) {
            return Err(Error::Resolution(format!("case collision: {spelling}")));
        }
        progress(&path);
        let (bytes, sha256) = digest_file(&path)?;
        files.push(Fingerprint {
            path: spelling,
            bytes,
            sha256,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut set = Sha256::new();
    // Length prefixes make the manifest hash unambiguous; absolute paths and timestamps
    // are deliberately absent so moving an installation does not change its identity.
    set.update(b"fallout-content-set-v1\0");
    for f in &files {
        set.update((f.path.len() as u64).to_le_bytes());
        set.update(f.path.as_bytes());
        set.update(f.bytes.to_le_bytes());
        set.update(f.sha256.as_bytes());
    }
    let mut required = vec!["FalloutNV.exe".to_owned()];
    required.extend(OFFICIAL_PLUGINS.iter().map(|n| format!("Data/{n}")));
    let missing_required = required
        .into_iter()
        .filter(|n| !names.contains(&n.to_ascii_lowercase()))
        .collect();
    Ok(Baseline {
        schema_version: 1,
        profile: "nv-original",
        installation: root,
        content_fingerprint: format!("{:x}", set.finalize()),
        files,
        missing_required,
        unverified: vec![
            "runtime language",
            "effective INI settings",
            "difficulty",
            "input bindings",
            "active plugin load order",
            "retail behavior traces",
        ],
    })
}
