use super::{JobError, JobResult};
use crate::{
    archive::NvArchive,
    baseline,
    cache::ArtifactIdentity,
    identity::ProfileId,
    vfs::{AssetPath, AssetSource},
};
use dream_archive::bsa::tes4::EntryId;
use std::{path::Path, sync::Arc};

/// One existing production archive reader, its write-denying handle and digest.
/// POSIX callers must honor the existing immutable-source contract.
pub struct ArchiveInput {
    pub(crate) archive: NvArchive,
    pub(crate) container: String,
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
}

pub struct Member {
    pub(crate) input: Arc<ArchiveInput>,
    pub(crate) id: EntryId,
    pub(crate) bytes: usize,
    pub(crate) identity: ArtifactIdentity,
}

impl ArchiveInput {
    pub fn open(path: &Path) -> JobResult<Arc<Self>> {
        // Open the protected reader before hashing, so the two see one source.
        let archive = NvArchive::open(path)?;
        let (bytes, sha256) = baseline::digest_file(path)?;
        if bytes != archive.backend().archive_size() as u64 {
            return Err(JobError::Invalid(
                "archive length changed during fingerprinting".into(),
            ));
        }
        Ok(Arc::new(Self {
            archive,
            container: path.display().to_string(),
            sha256,
            bytes,
        }))
    }

    pub fn source_sha256(&self) -> &str {
        &self.sha256
    }
    pub fn source_bytes(&self) -> u64 {
        self.bytes
    }

    pub fn member(self: &Arc<Self>, path: &AssetPath, source: &AssetSource) -> JobResult<Member> {
        if source.container != self.container
            || source.original_path.len() > 4096
            || AssetPath::new(&source.original_path)? != *path
        {
            return Err(JobError::Invalid(
                "archive member source/path mismatch".into(),
            ));
        }
        let id = EntryId::from_index(source.entry_index);
        let entry = self
            .archive
            .backend()
            .entry_by_id(id)
            .ok_or_else(|| JobError::Invalid("indexed archive member disappeared".into()))?;
        if entry.path().map(|p| p.as_ref()) != Some(source.original_path.as_slice()) {
            return Err(JobError::Invalid("archive member identity changed".into()));
        }
        let bytes = self
            .archive
            .backend()
            .extracted_len_by_id(id)
            .map_err(|e| JobError::Invalid(e.to_string()))?;
        if bytes > crate::archive::MAX_ASSET_BYTES {
            return Err(JobError::ByteBudget);
        }
        Ok(Member {
            input: self.clone(),
            id,
            bytes: bytes as usize,
            identity: ArtifactIdentity {
                profile: ProfileId::NvOriginal,
                source_sha256: self.sha256.clone(),
                path_bytes: path.bytes().to_vec(),
                transform_version: "nv-bsa104-decode-v1".into(),
            },
        })
    }
}
