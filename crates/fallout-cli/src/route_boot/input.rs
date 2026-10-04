//! File admission for the route compositor. These helpers never create outputs.
use super::Result;
use fallout_data::baseline;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
};

pub(super) const REQUEST_BYTES: usize = 1024 * 1024;
pub(super) const SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;
pub(super) const REPORT_BYTES: usize = 8 * 1024 * 1024;

pub(super) struct RequestFile {
    pub bytes: Vec<u8>,
    pub sha256: String,
    _source: File,
}
impl RequestFile {
    pub fn read(path: &Path) -> Result<Self> {
        let mut source = baseline::open_source(path)?;
        if source.metadata()?.len() > REQUEST_BYTES as u64 {
            return Err("route boot request exceeds 1 MiB".into());
        }
        let mut bytes = Vec::new();
        (&mut source)
            .take(REQUEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > REQUEST_BYTES {
            return Err("route boot request exceeds 1 MiB".into());
        }
        Ok(Self {
            sha256: digest(&bytes),
            bytes,
            _source: source,
        })
    }
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Count serialized bytes before retaining a report or fingerprint projection.
struct Counter {
    bytes: usize,
    maximum: usize,
    hash: Sha256,
}
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(io::Error::other(
                "route boot serialized byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn admit(value: &impl Serialize, maximum: usize) -> Result<(usize, String)> {
    let mut counter = Counter {
        bytes: 0,
        maximum,
        hash: Sha256::new(),
    };
    serde_json::to_writer(&mut counter, value)?;
    Ok((counter.bytes, format!("{:x}", counter.hash.finalize())))
}

/// This is an application guard, not an atomic filesystem namespace lock.
/// Inspect the supplied ancestry before resolving it, and repeat just before
/// creation. Repository::create still owns create-new and publication.
pub(super) struct Destination {
    supplied: PathBuf,
    pub path: PathBuf,
    protected: Vec<PathBuf>,
}
impl Destination {
    pub fn inspect(path: &Path, protected: &[PathBuf]) -> Result<Self> {
        if path.components().any(|part| part == Component::ParentDir) {
            return Err("route boot destination must not contain parent traversal".into());
        }
        let supplied = std::path::absolute(path)?;
        let name = supplied
            .file_name()
            .ok_or("route boot destination requires a fresh directory name")?;
        let parent = supplied
            .parent()
            .ok_or("route boot destination requires an existing parent")?;
        for ancestor in parent.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)?;
            if !metadata.is_dir() || is_link(&metadata) {
                return Err("route boot destination ancestry must be plain directories".into());
            }
        }
        let path = parent.canonicalize()?.join(name);
        let protected = protected
            .iter()
            .map(|path| path.canonicalize())
            .collect::<io::Result<Vec<_>>>()?;
        if protected.iter().any(|root| path.starts_with(root)) {
            return Err("route boot destination aliases a protected input".into());
        }
        match fs::symlink_metadata(&path) {
            Ok(_) => return Err("route boot destination must be a new repository".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        Ok(Self {
            supplied,
            path,
            protected,
        })
    }
    pub fn recheck(&self) -> Result<()> {
        let current = Self::inspect(&self.supplied, &self.protected)?;
        if current.path != self.path {
            return Err("route boot destination parent changed".into());
        }
        Ok(())
    }
    pub fn protected(&self) -> &[PathBuf] {
        &self.protected
    }
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 || metadata.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

struct ProtectedFile {
    path: PathBuf,
    source: File,
    bytes: u64,
    sha256: String,
}
impl ProtectedFile {
    fn inspect(path: PathBuf, maximum: usize) -> Result<Self> {
        let mut source = baseline::open_source(&path)?;
        if source.metadata()?.len() > maximum as u64 {
            return Err("route boot input repository file exceeds byte budget".into());
        }
        let (bytes, sha256) = baseline::digest_reader(&mut (&mut source).take(maximum as u64 + 1))?;
        if bytes > maximum as u64 {
            return Err("route boot input repository file exceeds byte budget".into());
        }
        Ok(Self {
            path,
            source,
            bytes,
            sha256,
        })
    }
    fn recheck(&mut self) -> Result<()> {
        self.source.seek(SeekFrom::Start(0))?;
        let current = baseline::digest_reader(&mut (&mut self.source).take(self.bytes + 1))?;
        if current != (self.bytes, self.sha256.clone()) {
            return Err(format!("route boot input changed: {}", self.path.display()).into());
        }
        Ok(())
    }
}

/// Keep native slot/marker handles protected across asynchronous restoration.
/// The existing writer lock needs a read/write open, so hash it between stages
/// rather than holding a write-denying handle that would prevent strict load.
pub(super) struct RepositoryInputs {
    root: PathBuf,
    names: Vec<std::ffi::OsString>,
    files: Vec<ProtectedFile>,
    lock_identity: (u64, String),
}
fn names(root: &Path) -> Result<Vec<std::ffi::OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        if names.len() == 64 {
            return Err("route boot input repository entry budget exceeded".into());
        }
        names.push(entry?.file_name());
    }
    names.sort();
    Ok(names)
}
fn lock_identity(root: &Path) -> Result<(u64, String)> {
    let file = ProtectedFile::inspect(root.join("writer.lock"), 8)?;
    Ok((file.bytes, file.sha256))
}
impl RepositoryInputs {
    pub fn inspect(root: &Path) -> Result<Self> {
        let mut files = vec![
            ProtectedFile::inspect(root.join(".rust-fallout-saves"), 26)?,
            ProtectedFile::inspect(
                root.join("current.frsv"),
                SNAPSHOT_BYTES + fallout_runtime::save::format::OVERHEAD,
            )?,
        ];
        match fs::symlink_metadata(root.join("previous.frsv")) {
            Ok(_) => files.push(ProtectedFile::inspect(
                root.join("previous.frsv"),
                SNAPSHOT_BYTES + fallout_runtime::save::format::OVERHEAD,
            )?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        Ok(Self {
            root: root.into(),
            names: names(root)?,
            files,
            lock_identity: lock_identity(root)?,
        })
    }
    pub fn recheck(&mut self) -> Result<()> {
        if names(&self.root)? != self.names || lock_identity(&self.root)? != self.lock_identity {
            return Err("route boot input repository entries or lock changed".into());
        }
        for file in &mut self.files {
            file.recheck()?;
        }
        Ok(())
    }
}
