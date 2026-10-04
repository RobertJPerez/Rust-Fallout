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

/// Verify and load an existing artifact without first rebuilding it. No marker
/// means no committed entry: a later builder may recover a verified orphan blob.
pub fn read_verified(
    root: &Path,
    source_tree: &Path,
    identity: &ArtifactIdentity,
    max_bytes: usize,
) -> Result<Option<(CacheResult, Vec<u8>)>> {
    let root = validate_root(root, source_tree)?;
    let key = identity.key()?;
    let marker = root.join(format!("{key}.json"));
    if !marker.try_exists().map_err(|e| io(&marker, e))? {
        return Ok(None);
    }
    require_plain_file(&marker)?;
    let mut bytes = Vec::new();
    open_source(&marker)?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| io(&marker, e))?;
    if bytes.len() > 65536 {
        return Err(Error::Resolution("cache manifest exceeds budget".into()));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Resolution(format!("cache manifest: {e}")))?;
    if manifest.schema_version != 1 || manifest.identity != *identity {
        return Err(Error::Resolution("cache manifest identity mismatch".into()));
    }
    if manifest.bytes > max_bytes as u64 {
        return Err(Error::Resolution(
            "cache artifact exceeds input budget".into(),
        ));
    }
    let blob = root.join(format!("{key}.blob"));
    require_plain_file(&blob)?;
    let mut file = open_source(&blob)?;
    if file.metadata().map_err(|e| io(&blob, e))?.len() != manifest.bytes {
        return Err(Error::Resolution("cache blob byte length mismatch".into()));
    }
    // Reserve the admitted size exactly instead of read_to_end's geometric
    // growth, so resource-job output reservations cover warm-cache payloads too.
    bytes = Vec::new();
    bytes
        .try_reserve_exact(manifest.bytes as usize)
        .map_err(|e| Error::Resolution(format!("cache payload allocation: {e}")))?;
    bytes.resize(manifest.bytes as usize, 0);
    file.read_exact(&mut bytes).map_err(|e| io(&blob, e))?;
    if bytes.len() as u64 != manifest.bytes
        || format!("{:x}", Sha256::digest(&bytes)) != manifest.sha256
    {
        return Err(Error::Resolution("cache blob failed digest check".into()));
    }
    Ok(Some((
        CacheResult {
            key,
            reused: true,
            manifest,
        },
        bytes,
    )))
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
    publish_guarded(root, source_tree, identity, bytes, stop_after_blob, None)
}

pub(crate) fn publish_cancellable(
    root: &Path,
    source_tree: &Path,
    identity: ArtifactIdentity,
    bytes: &[u8],
    token: &crate::resource_jobs::JobToken,
) -> Result<CacheResult> {
    publish_guarded(root, source_tree, identity, bytes, false, Some(token))
}

fn checkpoint(token: Option<&crate::resource_jobs::JobToken>) -> Result<()> {
    token.map_or(Ok(()), |token| token.checkpoint())
}

fn publish_guarded(
    root: &Path,
    source_tree: &Path,
    identity: ArtifactIdentity,
    bytes: &[u8],
    stop_after_blob: bool,
    token: Option<&crate::resource_jobs::JobToken>,
) -> Result<CacheResult> {
    checkpoint(token)?;
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
        checkpoint(token)?;
        return Ok(CacheResult {
            key,
            reused: true,
            manifest: found,
        });
    }
    if blob.try_exists().map_err(|e| io(&blob, e))? {
        verify_blob(&blob, &expected)?;
    } else {
        publish_file(&root, &blob, bytes, token)?;
        verify_blob(&blob, &expected)?;
    }
    if stop_after_blob {
        return Err(Error::Resolution(
            "injected interruption after blob publication".into(),
        ));
    }
    #[cfg(test)]
    pause_publication_test(&root, "blob-published")?;
    checkpoint(token)?;
    let manifest =
        serde_json::to_vec_pretty(&expected).map_err(|e| Error::Resolution(e.to_string()))?;
    publish_file(&root, &marker, &manifest, token)?;
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

fn publish_file(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    token: Option<&crate::resource_jobs::JobToken>,
) -> Result<()> {
    let mut staged = tempfile::NamedTempFile::new_in(root).map_err(|e| io(root, e))?;
    for chunk in bytes.chunks(1024 * 1024) {
        checkpoint(token)?;
        staged.write_all(chunk).map_err(|e| io(staged.path(), e))?;
    }
    staged
        .as_file()
        .sync_all()
        .map_err(|e| io(staged.path(), e))?;
    #[cfg(test)]
    pause_publication_test(
        root,
        if path.extension().is_some_and(|v| v == "blob") {
            "blob-staged"
        } else {
            "marker-staged"
        },
    )?;
    #[cfg(test)]
    crate::resource_jobs::tests::pause_cache(root, path.extension().is_some_and(|v| v == "json"));
    checkpoint(token)?;
    // Generation/cancellation and the final rename share one critical section.
    // Staging/sync/hash work stays outside it. A completed marker remains a valid
    // source-addressed artifact even if a later residency generation no longer uses it.
    if let Some(token) = token {
        token
            .commit(|| {
                persist_file(staged, path, bytes).map_err(crate::resource_jobs::JobError::from)
            })
            .map_err(|error| match error {
                crate::resource_jobs::JobError::Failed(error) => error,
                error => Error::Resolution(error.to_string()),
            })
    } else {
        persist_file(staged, path, bytes)
    }
}

fn persist_file(staged: tempfile::NamedTempFile, path: &Path, bytes: &[u8]) -> Result<()> {
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

// Compiled only into the unit-test executable. A child announces a precise
// publication point and waits for its parent to terminate it.
#[cfg(test)]
fn pause_publication_test(root: &Path, phase: &str) -> Result<()> {
    if std::env::var("FALLOUT_CACHE_TEST_PHASE").ok().as_deref() == Some(phase) {
        let ready = root.join("publication.ready");
        let mut signal = tempfile::NamedTempFile::new_in(root).map_err(|error| io(root, error))?;
        signal
            .write_all(phase.as_bytes())
            .map_err(|error| io(signal.path(), error))?;
        signal
            .persist_noclobber(&ready)
            .map_err(|error| io(&ready, error.error))?;
        loop {
            std::thread::park();
        }
    }
    Ok(())
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

    #[test]
    fn verified_lookup_checks_payload_budget_and_digest_before_reuse() {
        let source = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let id = identity();
        assert!(
            read_verified(root.path(), source.path(), &id, 64)
                .unwrap()
                .is_none()
        );
        publish(root.path(), source.path(), id.clone(), b"authored payload").unwrap();
        assert!(read_verified(root.path(), source.path(), &id, 1).is_err());
        let (_, bytes) = read_verified(root.path(), source.path(), &id, 64)
            .unwrap()
            .unwrap();
        assert_eq!(bytes, b"authored payload");
        fs::write(
            root.path().join(format!("{}.blob", id.key().unwrap())),
            b"damaged payload!",
        )
        .unwrap();
        assert!(read_verified(root.path(), source.path(), &id, 64).is_err());
    }

    #[test]
    #[ignore = "helper executed by the process-termination test"]
    fn publication_child_process() {
        let root = PathBuf::from(std::env::var_os("FALLOUT_CACHE_TEST_ROOT").unwrap());
        let source = PathBuf::from(std::env::var_os("FALLOUT_CACHE_TEST_SOURCE").unwrap());
        publish(&root, &source, identity(), b"original fixture").unwrap();
    }

    #[test]
    fn killed_publication_processes_leave_no_committed_partial_artifact() {
        use std::{
            process::{Child, Command, Stdio},
            time::{Duration, Instant},
        };
        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        for phase in ["blob-staged", "blob-published", "marker-staged"] {
            let root = tempfile::tempdir().unwrap();
            let source = tempfile::tempdir().unwrap();
            let source_path = source.path().join("untouched.txt");
            fs::write(&source_path, b"read-only source fixture").unwrap();
            let mut child = ChildGuard(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "cache::tests::publication_child_process",
                        "--ignored",
                    ])
                    .env("FALLOUT_CACHE_TEST_ROOT", root.path())
                    .env("FALLOUT_CACHE_TEST_SOURCE", source.path())
                    .env("FALLOUT_CACHE_TEST_PHASE", phase)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let ready = root.path().join("publication.ready");
            let started = Instant::now();
            while !ready.exists() {
                assert!(
                    started.elapsed() < Duration::from_secs(15),
                    "child did not reach {phase}"
                );
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "child exited before {phase}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(fs::read_to_string(&ready).unwrap(), phase);
            child.0.kill().unwrap();
            assert!(!child.0.wait().unwrap().success());
            let id = identity();
            assert!(
                !root
                    .path()
                    .join(format!("{}.json", id.key().unwrap()))
                    .exists()
            );
            assert!(
                read_verified(root.path(), source.path(), &id, 64)
                    .unwrap()
                    .is_none()
            );
            publish(root.path(), source.path(), id.clone(), b"original fixture").unwrap();
            assert_eq!(
                read_verified(root.path(), source.path(), &id, 64)
                    .unwrap()
                    .unwrap()
                    .1,
                b"original fixture"
            );
            assert_eq!(fs::read(&source_path).unwrap(), b"read-only source fixture");
        }
    }
}
