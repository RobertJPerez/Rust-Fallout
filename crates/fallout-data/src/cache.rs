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

const MAX_CACHE_SET_MEMBERS: usize = 1024;
const MAX_CACHE_SET_METADATA_BYTES: usize = 1024 * 1024;
const MAX_CACHE_SET_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;

/// One caller-declared required resource and its pinned decoded member length.
#[derive(Debug, Clone)]
pub struct RequiredCacheArtifact {
    pub identity: ArtifactIdentity,
    pub decoded_bytes: u64,
}

/// Lowerable limits for an ephemeral required-cache-set observation.
#[derive(Debug, Clone, Copy)]
pub struct CacheSetLimits {
    pub max_members: usize,
    pub max_metadata_bytes: usize,
    pub max_artifact_bytes: usize,
}

impl Default for CacheSetLimits {
    fn default() -> Self {
        Self {
            max_members: MAX_CACHE_SET_MEMBERS,
            max_metadata_bytes: MAX_CACHE_SET_METADATA_BYTES,
            max_artifact_bytes: MAX_CACHE_SET_ARTIFACT_BYTES,
        }
    }
}

/// Verification observed for one explicit required set; it is not a durable
/// snapshot and does not imply scene, simulation, or GPU readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheSetReceipt {
    pub source_options_sha256: String,
    pub fingerprint_sha256: String,
    pub member_count: usize,
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn identity_metadata_bound(identity: &ArtifactIdentity) -> Result<usize> {
    if !is_sha256_hex(&identity.source_sha256) {
        return Err(Error::Resolution(
            "cache identity needs a SHA-256 source digest".into(),
        ));
    }
    // JSON byte arrays use at most four characters per path byte; string
    // escapes use at most six bytes per UTF-8 byte. The fixed allowance covers
    // field names, profile, delimiters, and the enclosing object.
    256usize
        .checked_add(identity.source_sha256.len())
        .and_then(|size| size.checked_add(identity.path_bytes.len().checked_mul(4)?))
        .and_then(|size| size.checked_add(identity.transform_version.len().checked_mul(6)?))
        .ok_or_else(|| Error::Resolution("cache set metadata size overflow".into()))
}

impl ArtifactIdentity {
    pub fn key(&self) -> Result<String> {
        if !is_sha256_hex(&self.source_sha256) {
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

/// Verify every artifact in a caller-supplied required set, one payload at a
/// time. The caller owns the required-resource policy and supplies the digest
/// of its sealed source/options plan; this function never infers membership.
///
/// The returned fingerprint is deterministic over the explicit digest, sorted
/// artifact keys, and pinned decoded lengths. Verification is an observation at
/// the time each entry is read, not an atomic cache snapshot or readiness gate.
pub fn verify_required_cache_set(
    root: &Path,
    source_tree: &Path,
    source_options_sha256: &str,
    required: &[RequiredCacheArtifact],
    limits: CacheSetLimits,
) -> Result<CacheSetReceipt> {
    if !is_sha256_hex(source_options_sha256) {
        return Err(Error::Resolution(
            "cache set needs a SHA-256 source/options digest".into(),
        ));
    }
    if limits.max_members == 0
        || limits.max_members > MAX_CACHE_SET_MEMBERS
        || limits.max_metadata_bytes == 0
        || limits.max_metadata_bytes > MAX_CACHE_SET_METADATA_BYTES
        || limits.max_artifact_bytes == 0
        || limits.max_artifact_bytes > MAX_CACHE_SET_ARTIFACT_BYTES
    {
        return Err(Error::Resolution(
            "invalid cache set member, metadata, or artifact limit".into(),
        ));
    }
    if required.is_empty() {
        return Err(Error::Resolution(
            "required cache set cannot be empty".into(),
        ));
    }
    if required.len() > limits.max_members {
        return Err(Error::Resolution(
            "required cache set exceeds member limit".into(),
        ));
    }

    // Bound all identity/key bookkeeping before serializing identities or
    // allocating the verification list. This metadata cap is independently
    // lowerable from the hard member ceiling.
    let mut metadata_bytes = 128usize
        .checked_add(source_options_sha256.len())
        .ok_or_else(|| Error::Resolution("cache set metadata size overflow".into()))?;
    for artifact in required {
        if artifact.decoded_bytes > limits.max_artifact_bytes as u64 {
            return Err(Error::Resolution(
                "required cache artifact exceeds byte limit".into(),
            ));
        }
        let member_bytes = identity_metadata_bound(&artifact.identity)?
            .checked_add(72)
            .ok_or_else(|| Error::Resolution("cache set metadata size overflow".into()))?;
        metadata_bytes = metadata_bytes
            .checked_add(member_bytes)
            .ok_or_else(|| Error::Resolution("cache set metadata size overflow".into()))?;
        if metadata_bytes > limits.max_metadata_bytes {
            return Err(Error::Resolution(
                "required cache set exceeds metadata budget".into(),
            ));
        }
    }

    struct Entry<'a> {
        key: String,
        identity: &'a ArtifactIdentity,
        decoded_bytes: u64,
    }
    let mut entries = Vec::with_capacity(required.len());
    for artifact in required {
        entries.push(Entry {
            key: artifact.identity.key()?,
            identity: &artifact.identity,
            decoded_bytes: artifact.decoded_bytes,
        });
    }
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    for pair in entries.windows(2) {
        if pair[0].key == pair[1].key {
            let reason = if pair[0].identity == pair[1].identity
                && pair[0].decoded_bytes == pair[1].decoded_bytes
            {
                "required cache set contains a duplicate member"
            } else {
                "required cache set contains inconsistent duplicate identity or length"
            };
            return Err(Error::Resolution(reason.into()));
        }
    }

    let mut fingerprint = Sha256::new();
    fingerprint.update(b"fallout-required-cache-set-v1\0");
    fingerprint.update(source_options_sha256.as_bytes());
    let count = u32::try_from(entries.len())
        .map_err(|_| Error::Resolution("required cache set member count overflow".into()))?;
    fingerprint.update(count.to_le_bytes());

    for entry in &entries {
        let max_bytes = usize::try_from(entry.decoded_bytes)
            .map_err(|_| Error::Resolution("required cache artifact size overflow".into()))?;
        let Some((cache_result, payload)) =
            read_verified(root, source_tree, entry.identity, max_bytes)?
        else {
            return Err(Error::Resolution(format!(
                "required cache member is missing: {}",
                entry.key
            )));
        };
        if cache_result.key != entry.key
            || cache_result.manifest.bytes != entry.decoded_bytes
            || payload.len() as u64 != entry.decoded_bytes
        {
            return Err(Error::Resolution(format!(
                "required cache member identity or decoded length mismatch: {}",
                entry.key
            )));
        }
        drop(payload);

        fingerprint.update(entry.key.as_bytes());
        fingerprint.update(entry.decoded_bytes.to_le_bytes());
    }

    Ok(CacheSetReceipt {
        source_options_sha256: source_options_sha256.to_owned(),
        fingerprint_sha256: format!("{:x}", fingerprint.finalize()),
        member_count: entries.len(),
    })
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
    #[cfg(test)]
    pause_publication_test(&root, "marker-published")?;
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
    fn source_and_transform_changes_leave_unrelated_cache_entries_readable() {
        let cache = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let original = identity();
        let mut unrelated = identity();
        unrelated.profile = ProfileId::Fo3Original;
        unrelated.source_sha256 = "b".repeat(64);
        unrelated.path_bytes = b"textures/independent.dds".to_vec();

        publish(cache.path(), source.path(), original.clone(), b"source-v1").unwrap();
        publish(
            cache.path(),
            source.path(),
            unrelated.clone(),
            b"other-source",
        )
        .unwrap();

        let mut changed_source = original.clone();
        changed_source.source_sha256 = "c".repeat(64);
        let mut changed_transform = original.clone();
        changed_transform.transform_version = "archive-decode-v2".into();
        assert_ne!(original.key().unwrap(), changed_source.key().unwrap());
        assert_ne!(original.key().unwrap(), changed_transform.key().unwrap());
        assert!(
            read_verified(cache.path(), source.path(), &changed_source, 64)
                .unwrap()
                .is_none()
        );
        assert!(
            read_verified(cache.path(), source.path(), &changed_transform, 64)
                .unwrap()
                .is_none()
        );

        publish(
            cache.path(),
            source.path(),
            changed_source.clone(),
            b"source-v2",
        )
        .unwrap();
        publish(
            cache.path(),
            source.path(),
            changed_transform.clone(),
            b"transformed-v2",
        )
        .unwrap();

        for (id, expected) in [
            (&changed_source, b"source-v2".as_slice()),
            (&changed_transform, b"transformed-v2".as_slice()),
            (&unrelated, b"other-source".as_slice()),
        ] {
            let (_, bytes) = read_verified(cache.path(), source.path(), id, 64)
                .unwrap()
                .unwrap();
            assert_eq!(bytes, expected);
        }
    }

    fn required_artifact(identity: ArtifactIdentity, decoded_bytes: u64) -> RequiredCacheArtifact {
        RequiredCacheArtifact {
            identity,
            decoded_bytes,
        }
    }

    #[test]
    fn required_cache_set_fingerprint_binds_members_options_and_is_order_independent() {
        let cache = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let first = identity();
        let mut second = identity();
        second.source_sha256 = "b".repeat(64);
        second.path_bytes = b"textures/independent.dds".to_vec();
        publish(cache.path(), source.path(), first.clone(), b"first").unwrap();
        publish(cache.path(), source.path(), second.clone(), b"second").unwrap();

        let requirements = [required_artifact(first, 5), required_artifact(second, 6)];
        let options = "c".repeat(64);
        let forward = verify_required_cache_set(
            cache.path(),
            source.path(),
            &options,
            &requirements,
            CacheSetLimits::default(),
        )
        .unwrap();
        let reversed = [requirements[1].clone(), requirements[0].clone()];
        let reverse = verify_required_cache_set(
            cache.path(),
            source.path(),
            &options,
            &reversed,
            CacheSetLimits::default(),
        )
        .unwrap();
        assert_eq!(forward.member_count, 2);
        assert_eq!(forward.fingerprint_sha256, reverse.fingerprint_sha256);

        let changed_options = verify_required_cache_set(
            cache.path(),
            source.path(),
            &"d".repeat(64),
            &requirements,
            CacheSetLimits::default(),
        )
        .unwrap();
        assert_ne!(
            forward.fingerprint_sha256,
            changed_options.fingerprint_sha256
        );

        let one_member = verify_required_cache_set(
            cache.path(),
            source.path(),
            &options,
            &requirements[..1],
            CacheSetLimits::default(),
        )
        .unwrap();
        assert_ne!(forward.fingerprint_sha256, one_member.fingerprint_sha256);
    }

    #[test]
    fn required_cache_set_refuses_missing_duplicate_inconsistent_and_unbounded_inputs() {
        let cache = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let options = "d".repeat(64);
        let present = identity();
        publish(cache.path(), source.path(), present.clone(), b"whole").unwrap();
        let required = required_artifact(present.clone(), 5);

        let mut missing = present.clone();
        missing.path_bytes = b"meshes/mandatory-missing.nif".to_vec();
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[required_artifact(missing, 9)],
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("required cache member is missing")
        ));

        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[required.clone(), required.clone()],
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("duplicate member")
        ));

        let lower_member_limit = CacheSetLimits {
            max_members: 1,
            ..CacheSetLimits::default()
        };
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[required.clone(), required.clone()],
                lower_member_limit,
            ),
            Err(Error::Resolution(reason)) if reason.contains("exceeds member limit")
        ));

        let inconsistent_length = required_artifact(present.clone(), 4);
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[required.clone(), inconsistent_length],
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("inconsistent duplicate")
        ));

        let lower_limit = CacheSetLimits {
            max_metadata_bytes: 64,
            ..CacheSetLimits::default()
        };
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                std::slice::from_ref(&required),
                lower_limit,
            ),
            Err(Error::Resolution(reason)) if reason.contains("metadata budget")
        ));

        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[],
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("cannot be empty")
        ));
    }

    #[test]
    fn required_cache_set_rejects_member_length_and_payload_digest_mismatches() {
        let cache = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let id = identity();
        publish(cache.path(), source.path(), id.clone(), b"whole").unwrap();
        let options = "e".repeat(64);

        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[required_artifact(id.clone(), 6)],
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("decoded length mismatch")
        ));

        fs::write(
            cache.path().join(format!("{}.blob", id.key().unwrap())),
            b"wrong",
        )
        .unwrap();
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &options,
                &[required_artifact(id, 5)],
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("digest")
        ));
    }

    #[test]
    fn required_cache_set_rejects_invalid_options_digest_and_member_limit() {
        let cache = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let required = [required_artifact(identity(), 5)];
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                "not-a-digest",
                &required,
                CacheSetLimits::default(),
            ),
            Err(Error::Resolution(reason)) if reason.contains("source/options digest")
        ));
        let no_members = CacheSetLimits {
            max_members: 0,
            ..CacheSetLimits::default()
        };
        assert!(matches!(
            verify_required_cache_set(
                cache.path(),
                source.path(),
                &"f".repeat(64),
                &required,
                no_members,
            ),
            Err(Error::Resolution(reason)) if reason.contains("invalid cache set")
        ));
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
    fn killed_publication_processes_recover_at_each_marker_boundary() {
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
        for phase in [
            "blob-staged",
            "blob-published",
            "marker-staged",
            "marker-published",
        ] {
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
            let marker = root.path().join(format!("{}.json", id.key().unwrap()));
            if phase == "marker-published" {
                assert!(marker.is_file());
                assert_eq!(
                    read_verified(root.path(), source.path(), &id, 64)
                        .unwrap()
                        .unwrap()
                        .1,
                    b"original fixture"
                );
                assert!(
                    publish(root.path(), source.path(), id.clone(), b"original fixture")
                        .unwrap()
                        .reused
                );
            } else {
                assert!(!marker.exists());
                assert!(
                    read_verified(root.path(), source.path(), &id, 64)
                        .unwrap()
                        .is_none()
                );
                assert!(
                    !publish(root.path(), source.path(), id.clone(), b"original fixture")
                        .unwrap()
                        .reused
                );
                assert_eq!(
                    read_verified(root.path(), source.path(), &id, 64)
                        .unwrap()
                        .unwrap()
                        .1,
                    b"original fixture"
                );
            }
            assert_eq!(fs::read(&source_path).unwrap(), b"read-only source fixture");
        }
    }
}
