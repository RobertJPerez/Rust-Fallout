//! Resumable single-asset jobs. The manifest is the commit marker; an orphaned
//! blob after interruption must pass a digest check before it can be published.
use crate::{
    Error, Result,
    baseline::{digest_file, open_source},
    identity::ProfileId,
    io,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactIdentity {
    pub profile: ProfileId,
    pub source_sha256: String,
    pub path_bytes: Vec<u8>,
    pub transform_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub identity: ArtifactIdentity,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct CacheResult {
    pub key: String,
    pub reused: bool,
    pub manifest: Manifest,
}

impl ArtifactIdentity {
    pub fn key(&self) -> Result<String> {
        if self.source_sha256.len() != 64
            || !self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Resolution(
                "cache identity needs a SHA-256 source digest".into(),
            ));
        }
        let bytes = serde_json::to_vec(self).map_err(|e| Error::Resolution(e.to_string()))?;
        let mut hash = Sha256::new();
        hash.update(b"fallout-asset-job-v1\0");
        hash.update(bytes);
        Ok(format!("{:x}", hash.finalize()))
    }
}

/// Files are synced before publication. This rebuildable cache does not promise
/// the power-loss durability required for saves or cross-campaign transactions.
pub fn publish(
    root: &Path,
    source_tree: &Path,
    identity: ArtifactIdentity,
    bytes: &[u8],
) -> Result<CacheResult> {
    publish_at(root, source_tree, identity, bytes, false)
}

/// Resolve the destination before starting a batch so a bad cache path is one
/// configuration error, rather than a failure attributed to every source asset.
pub fn validate_root(root: &Path, source_tree: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().map_err(|e| io(root, e))?;
    let source_tree = source_tree.canonicalize().map_err(|e| io(source_tree, e))?;
    if !root.is_dir() || root.starts_with(&source_tree) {
        return Err(Error::Resolution(
            "cache must be an existing directory outside the installation".into(),
        ));
    }
    Ok(root)
}

fn publish_at(
    root: &Path,
    source_tree: &Path,
    identity: ArtifactIdentity,
    bytes: &[u8],
    stop_after_blob: bool,
) -> Result<CacheResult> {
    let root = validate_root(root, source_tree)?;
    let key = identity.key()?;
    let blob = root.join(format!("{key}.blob"));
    let marker = root.join(format!("{key}.json"));
    let expected = Manifest {
        schema_version: 1,
        identity,
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    };
    if marker.try_exists().map_err(|e| io(&marker, e))? {
        require_plain_file(&marker)?;
        let mut input = Vec::new();
        open_source(&marker)?
            .take(65537)
            .read_to_end(&mut input)
            .map_err(|e| io(&marker, e))?;
        if input.len() > 65536 {
            return Err(Error::Resolution("cache manifest exceeds budget".into()));
        }
        let found: Manifest = serde_json::from_slice(&input)
            .map_err(|e| Error::Resolution(format!("cache manifest: {e}")))?;
        if found.schema_version != 1
            || found.identity != expected.identity
            || found.bytes != expected.bytes
            || found.sha256 != expected.sha256
        {
            return Err(Error::Resolution(
                "cache manifest identity or digest mismatch".into(),
            ));
        }
        verify_blob(&blob, &found)?;
        return Ok(CacheResult {
            key,
            reused: true,
            manifest: found,
        });
    }
    if blob.try_exists().map_err(|e| io(&blob, e))? {
        verify_blob(&blob, &expected)?;
    } else {
        publish_file(&root, &blob, bytes)?;
        verify_blob(&blob, &expected)?;
    }
    if stop_after_blob {
        return Err(Error::Resolution(
            "injected interruption after blob publication".into(),
        ));
    }
    let manifest =
        serde_json::to_vec_pretty(&expected).map_err(|e| Error::Resolution(e.to_string()))?;
    publish_file(&root, &marker, &manifest)?;
    Ok(CacheResult {
        key,
        reused: false,
        manifest: expected,
    })
}

fn require_plain_file(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::Resolution("cache reparse point refused".into()));
        }
    }
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::Resolution(
            "cache entry must be a regular file".into(),
        ));
    }
    Ok(())
}

fn verify_blob(path: &Path, manifest: &Manifest) -> Result<()> {
    require_plain_file(path)?;
    let (bytes, digest) = digest_file(path)?;
    if bytes != manifest.bytes || digest != manifest.sha256 {
        return Err(Error::Resolution(format!(
            "cache blob failed digest check: {}",
            path.display()
        )));
    }
    Ok(())
}

fn publish_file(root: &Path, path: &Path, bytes: &[u8]) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(root).map_err(|e| io(root, e))?;
    staged.write_all(bytes).map_err(|e| io(staged.path(), e))?;
    staged
        .as_file()
        .sync_all()
        .map_err(|e| io(staged.path(), e))?;
    match staged.persist_noclobber(path) {
        Ok(_) => Ok(()),
        // A concurrent builder can win, but its published bytes must still agree.
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            require_plain_file(path)?;
            let (size, digest) = digest_file(path)?;
            if size == bytes.len() as u64 && digest == format!("{:x}", Sha256::digest(bytes)) {
                Ok(())
            } else {
                Err(Error::Resolution(
                    "concurrent cache publication conflicts".into(),
                ))
            }
        }
        Err(e) => Err(io(path, e.error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> ArtifactIdentity {
        ArtifactIdentity {
            profile: ProfileId::NvOriginal,
            source_sha256: "a".repeat(64),
            path_bytes: b"meshes/a.nif".to_vec(),
            transform_version: "archive-decode-v1".into(),
        }
    }
    #[test]
    fn interrupted_publication_resumes_and_corrupt_reuse_fails() {
        let cache = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let id = identity();
        assert!(
            publish_at(
                cache.path(),
                source.path(),
                id.clone(),
                b"original fixture",
                true
            )
            .is_err()
        );
        let key = id.key().unwrap();
        assert!(!cache.path().join(format!("{key}.json")).exists());
        publish(cache.path(), source.path(), id.clone(), b"original fixture").unwrap();
        assert!(
            publish(cache.path(), source.path(), id.clone(), b"original fixture")
                .unwrap()
                .reused
        );
        fs::write(cache.path().join(format!("{key}.blob")), b"damaged").unwrap();
        assert!(publish(cache.path(), source.path(), id, b"original fixture").is_err());
    }
    #[test]
    fn profiles_and_transform_revisions_cannot_share_a_cache_identity() {
        let original = identity();
        let mut changed = original.clone();
        changed.profile = ProfileId::Fo3Original;
        assert_ne!(original.key().unwrap(), changed.key().unwrap());
        changed = original.clone();
        changed.transform_version = "archive-decode-v2".into();
        assert_ne!(original.key().unwrap(), changed.key().unwrap());
    }
    #[test]
    fn cannot_publish_inside_source_tree() {
        let source = tempfile::tempdir().unwrap();
        assert!(publish(source.path(), source.path(), identity(), b"x").is_err());
        assert_eq!(fs::read_dir(source.path()).unwrap().count(), 0);
    }
}
