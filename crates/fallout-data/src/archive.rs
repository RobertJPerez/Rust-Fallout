use crate::{
    Error, Result,
    baseline::open_source,
    io, malformed,
    vfs::{AssetPath, AssetSource, MountIndex},
};
use dream_archive::bsa::tes4::{Archive, EntryId};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_ENTRIES: u32 = 1_000_000;
const MAX_NAME_BYTES: u32 = 64 * 1024 * 1024;
pub const MAX_ASSET_BYTES: u64 = 256 * 1024 * 1024;

pub struct NvArchive {
    archive: Archive,
    path: PathBuf,
    // Keep the write-denying handle alive longer than the map owned by the backend.
    _source_guard: File,
}

#[derive(Debug, Serialize)]
pub struct ArchiveCensus {
    pub source: PathBuf,
    pub version: u32,
    pub entries: usize,
    pub compressed_entries: usize,
    pub stored_bytes: u64,
    pub decoded_bytes: u64,
    pub extensions: BTreeMap<String, u64>,
    pub non_ascii_paths: usize,
    pub path_collisions: usize,
    pub status: &'static str,
}

impl NvArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let mut guard = open_source(path)?;
        let len = guard.metadata().map_err(|e| io(path, e))?.len();
        let mut header = [0; 36];
        guard.read_exact(&mut header).map_err(|e| io(path, e))?;
        preflight(&header, len, &path.to_string_lossy())?;
        let archive = Archive::open_path(path)
            .map_err(|e| malformed(&path.to_string_lossy(), 0, e.to_string()))?;
        Ok(Self {
            archive,
            path: path.to_owned(),
            _source_guard: guard,
        })
    }
    pub fn backend(&self) -> &Archive {
        &self.archive
    }
    pub fn read(&self, id: EntryId) -> Result<Vec<u8>> {
        let name = self.path.to_string_lossy();
        let size = self
            .archive
            .extracted_len_by_id(id)
            .map_err(|e| malformed(&name, 0, e.to_string()))?;
        if size > MAX_ASSET_BYTES {
            return Err(Error::Unsupported(format!(
                "{} entry {} exceeds the {} byte asset budget",
                name,
                id.index(),
                MAX_ASSET_BYTES
            )));
        }
        let entry = self
            .archive
            .entry_by_id_required(id)
            .map_err(|e| malformed(&name, 0, e.to_string()))?;
        self.archive
            .read_entry(entry)
            .map_err(|e| malformed(&name, u64::from(entry.file().data_offset), e.to_string()))
    }
    pub fn census(&self, mounts: &mut MountIndex) -> Result<ArchiveCensus> {
        let mut report = ArchiveCensus {
            source: self.path.clone(),
            version: self.archive.info().version as u32,
            entries: self.archive.len(),
            compressed_entries: 0,
            stored_bytes: 0,
            decoded_bytes: 0,
            extensions: BTreeMap::new(),
            non_ascii_paths: 0,
            path_collisions: 0,
            status: "index-decoded; asset semantics unimplemented",
        };
        let mut local_paths = BTreeMap::new();
        for (id, entry) in self.archive.entries_with_ids() {
            let raw = entry.path().ok_or_else(|| {
                Error::Unsupported(format!(
                    "{}: hash-only entry {}",
                    self.path.display(),
                    id.index()
                ))
            })?;
            let bytes: &[u8] = raw.as_ref();
            let normalized = AssetPath::new(bytes)?;
            if local_paths.insert(normalized.clone(), id.index()).is_some() {
                report.path_collisions += 1;
            }
            if !bytes.is_ascii() {
                report.non_ascii_paths += 1;
            }
            let extension = normalized
                .bytes()
                .rsplit(|b| *b == b'.')
                .next()
                .unwrap_or(b"");
            *report
                .extensions
                .entry(String::from_utf8_lossy(extension).into_owned())
                .or_default() += 1;
            report.stored_bytes += u64::from(entry.file().stored_size);
            report.decoded_bytes += self.archive.extracted_len(entry).map_err(|e| {
                malformed(
                    &self.path.to_string_lossy(),
                    u64::from(entry.file().data_offset),
                    e.to_string(),
                )
            })?;
            if entry
                .file()
                .is_compressed(self.archive.info().archive_flags)
            {
                report.compressed_entries += 1;
            }
            mounts.insert(AssetSource {
                container: self.path.display().to_string(),
                entry_index: id.index(),
                original_path: bytes.to_vec(),
            })?;
        }
        Ok(report)
    }
}

fn preflight(h: &[u8; 36], length: u64, name: &str) -> Result<()> {
    let field = |p| u32::from_le_bytes(h[p..p + 4].try_into().expect("fixed header"));
    if &h[..4] != b"BSA\0" {
        return Err(malformed(name, 0, "expected BSA magic"));
    }
    if field(4) != 104 {
        return Err(Error::Unsupported(format!(
            "{name}: NV adapter requires BSA 104, found {}",
            field(4)
        )));
    }
    if field(8) != 36 {
        return Err(malformed(name, 8, "invalid folder table offset"));
    }
    if field(16) > MAX_ENTRIES
        || field(20) > MAX_ENTRIES
        || field(24) > MAX_NAME_BYTES
        || field(28) > MAX_NAME_BYTES
    {
        return Err(malformed(name, 16, "archive index budget exceeded"));
    }
    let minimum = 36 + u64::from(field(16)) * 16 + u64::from(field(20)) * 16;
    if minimum > length {
        return Err(malformed(name, 16, "archive index cannot fit in file"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_allocation_bombs_before_backend() {
        let mut h = [0; 36];
        h[..4].copy_from_slice(b"BSA\0");
        h[4..8].copy_from_slice(&104u32.to_le_bytes());
        h[8..12].copy_from_slice(&36u32.to_le_bytes());
        h[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(preflight(&h, u64::MAX, "bomb").is_err());
    }
}
