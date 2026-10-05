//! FO4 policy and resource admission around the shared archive backend.
use crate::{Error, Result, bad};
use dream_archive::{
    ByteSlice,
    ba2::{Archive, Entry, FileHeader},
};
use fallout_data::vfs::AssetPath;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub entries: u32,
    pub chunks: u64,
    pub name_bytes: u64,
    pub member_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 500_000,
            chunks: 1_000_000,
            name_bytes: 64 * 1024 * 1024,
            member_bytes: 64 * 1024 * 1024,
        }
    }
}

pub struct Ba2 {
    archive: Archive,
    names: BTreeMap<AssetPath, Vec<usize>>,
    limits: Limits,
}
impl Ba2 {
    /// Files must stay immutable for the lifetime of this mmap-backed reader.
    pub fn open(path: &Path, limits: Limits) -> Result<Self> {
        let metadata = admit(&mut File::open(path)?, &path.display().to_string(), limits)?;
        let archive = Archive::open_path(path)?;
        Self::index(archive, limits, metadata)
    }
    pub fn from_bytes(bytes: Vec<u8>, limits: Limits) -> Result<Self> {
        let metadata = admit(&mut std::io::Cursor::new(&bytes), "BA2", limits)?;
        Self::index(Archive::from_vec(bytes)?, limits, metadata)
    }
    fn index(archive: Archive, limits: Limits, metadata: MetadataRanges) -> Result<Self> {
        let mut names: BTreeMap<AssetPath, Vec<usize>> = BTreeMap::new();
        for (i, entry) in archive.entries().iter().enumerate() {
            for chunk in entry.file().chunks() {
                let start = chunk.offset();
                let end = start + u64::from(chunk.stored_size());
                if start < end
                    && (start < metadata.index_end
                        || (start < metadata.names_end && end > metadata.names_start))
                {
                    return Err(bad("BA2", i, "member payload overlaps archive metadata"));
                }
            }
            if !entry.name().is_empty() {
                names
                    .entry(AssetPath::new(entry.name().as_bytes())?)
                    .or_default()
                    .push(i);
            }
        }
        Ok(Self {
            archive,
            names,
            limits,
        })
    }
    pub fn info(&self) -> dream_archive::ba2::ArchiveInfo {
        self.archive.info()
    }
    pub fn entries(&self) -> &[Entry] {
        self.archive.entries()
    }
    pub fn collisions(&self) -> usize {
        self.names.values().filter(|v| v.len() > 1).count()
    }
    pub fn find(&self, name: &[u8]) -> Result<Option<usize>> {
        match self.names.get(&AssetPath::new(name)?).map(Vec::as_slice) {
            None => Ok(None),
            Some([i]) => Ok(Some(*i)),
            Some(_) => Err(Error::Unsupported("ambiguous BA2 member name".into())),
        }
    }
    /// Entry indices are local to this immutable archive, never global identities.
    pub fn read(&self, index: usize) -> Result<Vec<u8>> {
        let entry = self
            .entries()
            .get(index)
            .ok_or_else(|| bad("BA2", index, "member index out of bounds"))?;
        let expected = self.archive.extracted_len(entry)?;
        if expected > self.limits.member_bytes {
            return Err(bad("BA2", index, "member exceeds decompression budget"));
        }
        let mut out = Vec::new();
        self.archive
            .open_entry(entry)?
            .take(self.limits.member_bytes.saturating_add(1))
            .read_to_end(&mut out)?;
        if out.len() as u64 != expected {
            return Err(bad("BA2", index, "extracted length mismatch"));
        }
        Ok(out)
    }
    pub fn texture(&self, index: usize) -> Option<dream_archive::ba2::TextureHeader> {
        match self.entries().get(index)?.file().header {
            FileHeader::DX10(header) => Some(header),
            _ => None,
        }
    }
}

/// Validate allocation-driving counts before the dependency allocates its index.
/// Full decoding and range/mip/compression validation remains in dream_archive.
struct MetadataRanges {
    index_end: u64,
    names_start: u64,
    names_end: u64,
}
fn admit(reader: &mut (impl Read + Seek), name: &str, limits: Limits) -> Result<MetadataRanges> {
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut h = [0; 24];
    reader.read_exact(&mut h)?;
    if &h[..4] != b"BTDX" {
        return Err(bad(name, 0, "expected BTDX"));
    }
    let version = u32::from_le_bytes(h[4..8].try_into().unwrap());
    if !matches!(version, 1 | 7 | 8) {
        return Err(Error::Unsupported(format!("FO4 BA2 version {version}")));
    }
    let (header_len, chunk_len) = match &h[8..12] {
        b"GNRL" => (16u64, 20u64),
        b"DX10" => (24, 24),
        _ => return Err(Error::Unsupported("FO4 BA2 payload type".into())),
    };
    let count = u32::from_le_bytes(h[12..16].try_into().unwrap());
    let names = u64::from_le_bytes(h[16..24].try_into().unwrap());
    if count > limits.entries || 24 + u64::from(count) * header_len > length {
        return Err(bad(name, 12, "BA2 entry count exceeds input or budget"));
    }
    let mut pos = 24u64;
    let mut chunks = 0u64;
    for _ in 0..count {
        let mut entry = [0; 16];
        reader.read_exact(&mut entry)?;
        chunks += u64::from(entry[13]);
        if chunks > limits.chunks {
            return Err(bad(name, pos as usize, "BA2 chunk budget exceeded"));
        }
        if u64::from(u16::from_le_bytes([entry[14], entry[15]])) != header_len {
            return Err(bad(name, pos as usize, "BA2 file header size mismatch"));
        }
        pos += header_len + u64::from(entry[13]) * chunk_len;
        if pos > length {
            return Err(bad(name, pos as usize, "BA2 table exceeds file"));
        }
        reader.seek(SeekFrom::Start(pos))?;
    }
    let mut names_end = names;
    if names != 0 {
        if names < pos || names > length {
            return Err(bad(
                name,
                16,
                "BA2 name table overlaps index or exceeds file",
            ));
        }
        reader.seek(SeekFrom::Start(names))?;
        let mut end = names;
        let mut total = 0;
        for _ in 0..count {
            let mut b = [0; 2];
            reader.read_exact(&mut b)?;
            let n = u64::from(u16::from_le_bytes(b));
            end += 2 + n;
            total += n;
            if end > length || total > limits.name_bytes {
                return Err(bad(
                    name,
                    end as usize,
                    "BA2 name bytes exceed input or budget",
                ));
            }
            reader.seek(SeekFrom::Start(end))?;
        }
        names_end = end;
    }
    Ok(MetadataRanges {
        index_end: pos,
        names_start: names,
        names_end,
    })
}
