//! Skyrim SE/AE BSA 105 admission and script inventory, using the shared backend.
use crate::{Result, bad, pex_header};
use dream_archive::bsa::tes4::{Archive, EntryId};
use fallout_data::{
    baseline::{digest_reader, open_source},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub struct SkyrimArchive {
    archive: Archive,
    _guard: File,
    name: String,
}
#[derive(Debug, Serialize)]
pub struct ArchiveReport {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub version: u32,
    pub entries: usize,
    pub compressed_entries: usize,
    pub extensions: BTreeMap<String, u64>,
    pub duplicate_paths: usize,
    pub scripts: Vec<ScriptAsset>,
    pub script_header_counts: BTreeMap<String, u64>,
    pub script_header_findings: usize,
    pub payload_scope: &'static str,
}
#[derive(Debug, Serialize)]
pub struct ScriptAsset {
    pub path: Vec<u8>,
    pub bytes: u64,
    pub sha256: String,
    pub dialect_prefix: pex_header::Observation,
}
#[derive(Debug, Serialize)]
pub struct NifArchiveIndex {
    pub file: String,
    pub version: u32,
    pub prefix: Option<Vec<u8>>,
    pub offset: usize,
    pub matching_members: u64,
    pub returned_members: Vec<Vec<u8>>,
    pub duplicate_normalized_paths: u64,
    pub hash_only_entries: u64,
    pub invalid_asset_paths: u64,
    pub truncated: bool,
    pub scope: &'static str,
}
#[derive(Debug, Serialize)]
pub struct ArchivePathHit {
    pub normalized_path: Vec<u8>,
    pub actual_member_paths: Vec<Vec<u8>>,
    pub duplicate_entries: u64,
}
#[derive(Debug, Serialize)]
pub struct ArchivePathQuery {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub version: u32,
    pub entries: usize,
    pub hash_only_entries: u64,
    pub invalid_asset_paths: u64,
    pub matches: Vec<ArchivePathHit>,
    pub scope: &'static str,
}

fn admit(header: &[u8; 36], length: u64, name: &str) -> Result<()> {
    let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    if &header[..4] != b"BSA\0" || word(4) != 105 {
        return Err(bad(
            name,
            0,
            "Skyrim SE/AE requires BSA 105; other dialects are unsupported",
        ));
    }
    if word(8) != 36 {
        return Err(bad(name, 8, "invalid folder table offset"));
    }
    if word(16) > 1_000_000
        || word(20) > 1_000_000
        || word(24) > 64 * 1024 * 1024
        || word(28) > 64 * 1024 * 1024
    {
        return Err(bad(name, 16, "BSA index allocation budget exceeded"));
    }
    // v105 folder records are 24 bytes (NV v104 uses 16).
    let minimum = 36
        + u64::from(word(16)) * 24
        + u64::from(word(20)) * 16
        + u64::from(word(24))
        + u64::from(word(28));
    if minimum > length {
        return Err(bad(name, 16, "BSA index cannot fit in file"));
    }
    Ok(())
}
impl SkyrimArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let name = path.display().to_string();
        let mut guard = open_source(path)?;
        let mut header = [0; 36];
        guard.read_exact(&mut header)?;
        admit(&header, guard.metadata()?.len(), &name)?;
        let archive = Archive::open_path(path).map_err(|e| bad(&name, 0, e.to_string()))?;
        Ok(Self {
            archive,
            _guard: guard,
            name,
        })
    }
    pub fn read_bounded(&self, id: EntryId, maximum: u64) -> Result<Vec<u8>> {
        let entry = self
            .archive
            .entry_by_id_required(id)
            .map_err(|e| bad(&self.name, 0, e.to_string()))?;
        self.read_entry_bounded(entry, maximum)
    }
    pub fn read_path_bounded(&self, path: &[u8], maximum: u64) -> Result<(Vec<u8>, Vec<u8>)> {
        let normalized = AssetPath::new(path)?;
        let mut match_id = None;
        for (id, entry) in self.archive.entries_with_ids() {
            let Some(raw) = entry.path() else {
                continue;
            };
            let Ok(candidate) = AssetPath::new(raw.as_ref()) else {
                continue;
            };
            if candidate == normalized && match_id.replace(id).is_some() {
                return Err(bad(
                    &self.name,
                    0,
                    "duplicate normalized archive member path; entry identity is ambiguous",
                ));
            }
        }
        let id = match_id.ok_or_else(|| bad(&self.name, 0, "archive member path not found"))?;
        let entry = self
            .archive
            .entry_by_id_required(id)
            .map_err(|e| bad(&self.name, 0, e.to_string()))?;
        let raw_path = entry
            .path()
            .ok_or_else(|| bad(&self.name, 0, "hash-only BSA paths unsupported"))?;
        let raw_bytes: &[u8] = raw_path.as_ref();
        let actual_path = raw_bytes.to_vec();
        let data = self.read_entry_bounded(entry, maximum)?;
        Ok((actual_path, data))
    }
    pub fn list_nif_paths(
        &self,
        prefix: Option<&[u8]>,
        offset: usize,
        limit: usize,
    ) -> Result<NifArchiveIndex> {
        if !(1..=512).contains(&limit) {
            return Err(crate::Error::Unsupported(
                "NIF archive index limit must be between 1 and 512".into(),
            ));
        }
        if offset > 1_000_000 {
            return Err(crate::Error::Unsupported(
                "NIF archive index offset exceeds the 1,000,000 path budget".into(),
            ));
        }
        let prefix = prefix.map(AssetPath::new).transpose()?;
        let mut report = NifArchiveIndex {
            file: self.name.clone(),
            version: 105,
            prefix: prefix.as_ref().map(|path| path.bytes().to_vec()),
            offset,
            matching_members: 0,
            returned_members: Vec::new(),
            duplicate_normalized_paths: 0,
            hash_only_entries: 0,
            invalid_asset_paths: 0,
            truncated: false,
            scope: "archive index paths only; no member payloads decompressed; returned order follows the archive table",
        };
        let mut seen = BTreeSet::new();
        for (_, entry) in self.archive.entries_with_ids() {
            let Some(raw) = entry.path() else {
                report.hash_only_entries += 1;
                continue;
            };
            let path = match AssetPath::new(raw.as_ref()) {
                Ok(path) => path,
                Err(_) => {
                    report.invalid_asset_paths += 1;
                    continue;
                }
            };
            let bytes = path.bytes();
            if !bytes.ends_with(b".nif")
                || prefix
                    .as_ref()
                    .is_some_and(|prefix| !bytes.starts_with(prefix.bytes()))
            {
                continue;
            }
            report.matching_members += 1;
            if !seen.insert(bytes.to_vec()) {
                report.duplicate_normalized_paths += 1;
            }
            if report.matching_members - 1 < offset as u64 {
                continue;
            }
            if report.returned_members.len() < limit {
                report.returned_members.push(bytes.to_vec());
            } else {
                report.truncated = true;
            }
        }
        Ok(report)
    }
    /// Match caller-supplied normalized candidates against the archive's
    /// names-only index. No member payload is selected or decompressed.
    pub fn match_paths(&mut self, requested: &BTreeSet<AssetPath>) -> Result<ArchivePathQuery> {
        self._guard.seek(SeekFrom::Start(0))?;
        let (bytes, sha256) = digest_reader(&mut self._guard)?;
        let mut matches: BTreeMap<AssetPath, Vec<Vec<u8>>> = BTreeMap::new();
        let mut hash_only_entries = 0;
        let mut invalid_asset_paths = 0;
        for (_, entry) in self.archive.entries_with_ids() {
            let Some(raw) = entry.path() else {
                hash_only_entries += 1;
                continue;
            };
            let path = match AssetPath::new(raw.as_ref()) {
                Ok(path) => path,
                Err(_) => {
                    invalid_asset_paths += 1;
                    continue;
                }
            };
            if requested.contains(&path) {
                let raw_bytes: &[u8] = raw.as_ref();
                matches.entry(path).or_default().push(raw_bytes.to_vec());
            }
        }
        let matches = matches
            .into_iter()
            .map(|(path, actual_member_paths)| ArchivePathHit {
                normalized_path: path.bytes().to_vec(),
                duplicate_entries: actual_member_paths.len().saturating_sub(1) as u64,
                actual_member_paths,
            })
            .collect();
        Ok(ArchivePathQuery {
            file: Path::new(&self.name)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&self.name)
                .to_owned(),
            bytes,
            sha256,
            version: 105,
            entries: self.archive.len(),
            hash_only_entries,
            invalid_asset_paths,
            matches,
            scope: "requested paths matched against names-only BSA index; no member payloads decompressed or load winners resolved",
        })
    }
    fn read_entry_bounded(
        &self,
        entry: &dream_archive::bsa::tes4::Entry,
        maximum: u64,
    ) -> Result<Vec<u8>> {
        let size = self
            .archive
            .extracted_len(entry)
            .map_err(|e| bad(&self.name, 0, e.to_string()))?;
        if size > maximum.min(64 * 1024 * 1024) {
            return Err(bad(&self.name, 0, "asset decompression budget exceeded"));
        }
        self.archive
            .read_entry(entry)
            .map_err(|e| bad(&self.name, entry.file().data_offset as usize, e.to_string()))
    }
    pub fn inspect(&mut self) -> Result<ArchiveReport> {
        self._guard.seek(SeekFrom::Start(0))?;
        let (bytes, sha256) = digest_reader(&mut self._guard)?;
        let mut report = ArchiveReport {
            file: self.name.clone(),
            bytes,
            sha256,
            version: 105,
            entries: self.archive.len(),
            compressed_entries: 0,
            extensions: BTreeMap::new(),
            duplicate_paths: 0,
            scripts: Vec::new(),
            script_header_counts: BTreeMap::new(),
            script_header_findings: 0,
            payload_scope: "all index entries; only scripts/*.pex decompressed, hashed, and observed through the first eight PEX header bytes",
        };
        let mut paths = BTreeSet::new();
        let mut script_bytes = 0u64;
        for (id, entry) in self.archive.entries_with_ids() {
            let raw = entry
                .path()
                .ok_or_else(|| bad(&self.name, 0, "hash-only BSA paths unsupported"))?;
            let path = AssetPath::new(raw.as_ref())?;
            if !paths.insert(path.clone()) {
                report.duplicate_paths += 1;
            }
            let bytes = path.bytes();
            let extension = bytes.rsplit(|b| *b == b'.').next().unwrap_or_default();
            *report
                .extensions
                .entry(String::from_utf8_lossy(extension).into())
                .or_default() += 1;
            if entry
                .file()
                .is_compressed(self.archive.info().archive_flags)
            {
                report.compressed_entries += 1;
            }
            if bytes.starts_with(b"scripts/") && bytes.ends_with(b".pex") {
                let data =
                    self.read_bounded(id, (512 * 1024 * 1024u64).saturating_sub(script_bytes))?;
                let (length, sha256) = digest_reader(&mut &data[..])?;
                script_bytes += length;
                let dialect_prefix = pex_header::observe(&data);
                *report
                    .script_header_counts
                    .entry(dialect_prefix.census_key())
                    .or_default() += 1;
                if !dialect_prefix.is_skyrim() {
                    report.script_header_findings += 1;
                }
                report.scripts.push(ScriptAsset {
                    path: bytes.to_vec(),
                    bytes: length,
                    sha256,
                    dialect_prefix,
                });
            }
        }
        report.scripts.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dream_archive::bsa::tes4::Builder;
    #[test]
    fn se_lz4_script_payload_is_bounded_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bsa");
        let mut builder = Builder::skyrim_se();
        builder.set_compressed(true);
        builder
            .add_bytes(
                b"Scripts\\Example.pex",
                [0xFA, 0x57, 0xC0, 0xDE, 3, 2, 0, 1, 0xAA],
            )
            .unwrap();
        builder.write_path(&path).unwrap();
        let mut archive = SkyrimArchive::open(&path).unwrap();
        let (id, _) = archive.archive.entries_with_ids().next().unwrap();
        assert!(archive.read_bounded(id, 2).is_err());
        assert_eq!(
            archive.read_bounded(id, 30).unwrap(),
            [0xFA, 0x57, 0xC0, 0xDE, 3, 2, 0, 1, 0xAA]
        );
        let report = archive.inspect().unwrap();
        assert_eq!(report.scripts.len(), 1);
        assert_eq!(report.scripts[0].path, b"scripts/example.pex");
        assert_eq!(
            report.scripts[0].dialect_prefix.census_key(),
            "big-endian/3.2/game-1"
        );
        assert_eq!(
            report.scripts[0].dialect_prefix.dialect_prefix_bytes.len(),
            8
        );
        assert_eq!(report.script_header_counts["big-endian/3.2/game-1"], 1);
        assert_eq!(report.compressed_entries, 1);
    }
    #[test]
    fn bounded_path_lookup_uses_shared_archive_normalization() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bsa");
        let mut builder = Builder::skyrim_se();
        builder.set_compressed(true);
        builder
            .add_bytes(b"Meshes\\Actors\\Example.NIF", b"synthetic mesh bytes")
            .unwrap();
        builder.write_path(&path).unwrap();
        let archive = SkyrimArchive::open(&path).unwrap();
        let (member, bytes) = archive
            .read_path_bounded(b"meshes/actors/example.nif", 64)
            .unwrap();
        assert_eq!(member, b"meshes\\actors\\example.nif");
        assert_eq!(bytes, b"synthetic mesh bytes");
        assert!(
            archive
                .read_path_bounded(b"meshes/actors/example.nif", 2)
                .is_err()
        );
        assert!(
            archive
                .read_path_bounded(b"meshes/actors/missing.nif", 64)
                .is_err()
        );
        assert!(archive.list_nif_paths(None, 0, 0).is_err());
        assert!(archive.list_nif_paths(None, 1_000_001, 1).is_err());
    }
    #[test]
    fn wrong_dialect_and_index_bomb_are_rejected_before_backend() {
        let mut h = [0u8; 36];
        h[..4].copy_from_slice(b"BSA\0");
        h[4..8].copy_from_slice(&104u32.to_le_bytes());
        assert!(admit(&h, u64::MAX, "nv").is_err());
        h[4..8].copy_from_slice(&105u32.to_le_bytes());
        h[8..12].copy_from_slice(&36u32.to_le_bytes());
        h[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(admit(&h, u64::MAX, "bomb").is_err());
        h[20..24].copy_from_slice(&1u32.to_le_bytes());
        assert!(admit(&h, 36, "truncated").is_err());
    }
}
