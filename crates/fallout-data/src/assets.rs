//! Immutable archive candidates shared by inspection and presentation consumers.
//! Until a retail mount policy is verified, only an unambiguous member can be read.
use crate::{
    Error, Result,
    archive::NvArchive,
    baseline, io,
    vfs::{AssetPath, AssetSource, MountIndex},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub struct ArchiveAssets {
    archives: BTreeMap<String, NvArchive>,
    digests: BTreeMap<String, String>,
    mounts: MountIndex,
    pub source_tree: PathBuf,
}

impl ArchiveAssets {
    pub fn open_nv(install: &Path) -> Result<Self> {
        let directory = install.join("Data");
        let mut paths = Vec::new();
        for entry in fs::read_dir(&directory).map_err(|e| io(&directory, e))? {
            let entry = entry.map_err(|e| io(&directory, e))?;
            if entry
                .file_type()
                .map_err(|e| io(entry.path(), e))?
                .is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("bsa"))
            {
                paths.push(entry.path());
            }
        }
        paths.sort();
        let mut mounts = MountIndex::default();
        let mut archives = BTreeMap::new();
        for path in paths {
            let archive = NvArchive::open(&path)?;
            archive.census(&mut mounts)?;
            archives.insert(path.display().to_string(), archive);
        }
        Ok(Self {
            archives,
            mounts,
            digests: BTreeMap::new(),
            source_tree: install.to_path_buf(),
        })
    }

    pub fn candidates(&self, path: &AssetPath) -> Result<&[AssetSource]> {
        self.mounts.candidates(path.bytes())
    }

    pub fn mounts(&self) -> &MountIndex {
        &self.mounts
    }

    pub fn read_unique(&self, path: &AssetPath) -> Result<(AssetSource, Vec<u8>)> {
        let source = self.mounts.unique(path.bytes())?.ok_or_else(|| {
            Error::Resolution(format!(
                "missing archive asset: {:?}",
                String::from_utf8_lossy(path.bytes())
            ))
        })?;
        let archive = &self.archives[&source.container];
        let id = archive
            .backend()
            .get_id(&source.original_path)
            .ok_or_else(|| Error::Resolution("indexed member disappeared".into()))?;
        if id.index() != source.entry_index {
            return Err(Error::Resolution("archive member identity changed".into()));
        }
        Ok((source.clone(), archive.read(id)?))
    }

    pub fn source_digest(&mut self, source: &AssetSource) -> Result<&str> {
        if !self.archives.contains_key(&source.container) {
            return Err(Error::Resolution("archive is not mounted".into()));
        }
        if !self.digests.contains_key(&source.container) {
            self.digests.insert(
                source.container.clone(),
                baseline::digest_file(Path::new(&source.container))?.1,
            );
        }
        Ok(&self.digests[&source.container])
    }
}
