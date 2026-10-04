//! Byte paths for archive members. A mount lists candidates; it does not guess NV's
//! archive invalidation rules or silently choose a winner on a collision.
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct AssetPath(Vec<u8>);

/// Texture fields may be relative to the textures directory or include it.
/// Preserve authored bytes at the caller; this only creates a safe lookup key.
pub fn texture_path(raw: &[u8]) -> Result<AssetPath> {
    if raw.len() > 4096 {
        return Err(Error::Unsupported("texture path exceeds 4096 bytes".into()));
    }
    let path = AssetPath::new(raw)?;
    if path.bytes().starts_with(b"textures/") {
        return Ok(path);
    }
    let mut rooted = b"textures/".to_vec();
    rooted.extend(path.bytes());
    AssetPath::new(&rooted)
}

impl AssetPath {
    pub fn new(raw: &[u8]) -> Result<Self> {
        if raw.is_empty()
            || matches!(raw[0], b'/' | b'\\')
            || raw.iter().any(|b| *b < 32 || *b == b':')
        {
            return Err(Error::Resolution(format!("unsafe asset path: {raw:?}")));
        }
        let mut normalized = Vec::with_capacity(raw.len());
        for component in raw.split(|b| matches!(b, b'/' | b'\\')) {
            if component.is_empty() || component == b"." || component == b".." {
                return Err(Error::Resolution(format!(
                    "invalid asset path component: {raw:?}"
                )));
            }
            if !normalized.is_empty() {
                normalized.push(b'/');
            }
            normalized.extend(component.iter().map(u8::to_ascii_lowercase));
        }
        Ok(Self(normalized))
    }
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetSource {
    pub container: String,
    pub entry_index: usize,
    pub original_path: Vec<u8>,
}

#[derive(Default)]
pub struct MountIndex {
    entries: BTreeMap<AssetPath, Vec<AssetSource>>,
}

impl MountIndex {
    pub fn insert(&mut self, source: AssetSource) -> Result<()> {
        let path = AssetPath::new(&source.original_path)?;
        self.entries.entry(path).or_default().push(source);
        Ok(())
    }
    pub fn candidates(&self, raw: &[u8]) -> Result<&[AssetSource]> {
        Ok(self
            .entries
            .get(&AssetPath::new(raw)?)
            .map(Vec::as_slice)
            .unwrap_or(&[]))
    }
    pub fn unique(&self, raw: &[u8]) -> Result<Option<&AssetSource>> {
        match self.candidates(raw)? {
            [] => Ok(None),
            [only] => Ok(Some(only)),
            many => Err(Error::Unsupported(format!(
                "asset has {} candidates; profile precedence is not verified",
                many.len()
            ))),
        }
    }
    pub fn collisions(&self) -> impl Iterator<Item = (&AssetPath, &[AssetSource])> {
        self.entries
            .iter()
            .filter(|(_, sources)| sources.len() > 1)
            .map(|(p, s)| (p, s.as_slice()))
    }
}

/// Source observations for explicit NV configuration roots. This never selects
/// runtime settings, active plugins, archive winners or invalidation behavior.
pub mod profile {
    use crate::{Error, Result, baseline::open_source, identity::ProfileId, io, malformed};
    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

    #[derive(Clone, Copy)]
    pub struct Limits {
        pub files: usize,
        pub file_bytes: usize,
        pub total_bytes: usize,
        pub lines: usize,
        pub keys: usize,
        /// Includes attempted normalized section/key copies for duplicate diagnostics.
        pub identifier_bytes: usize,
    }
    impl Default for Limits {
        fn default() -> Self {
            Self {
                files: 7,
                file_bytes: 1024 * 1024,
                total_bytes: 2 * 1024 * 1024,
                lines: 16_384,
                keys: 8_192,
                identifier_bytes: 2 * 1024 * 1024,
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
    pub struct Span {
        pub offset: usize,
        pub bytes: usize,
    }
    impl Span {
        pub fn read(self, source: &Source) -> Option<&[u8]> {
            source
                .raw_bytes
                .get(self.offset..self.offset.checked_add(self.bytes)?)
        }
    }

    #[derive(Debug, PartialEq, Eq, Serialize)]
    #[serde(tag = "kind", rename_all = "kebab-case")]
    pub enum LineKind {
        Blank,
        Comment,
        Section {
            name: Span,
        },
        Setting {
            section: Option<Span>,
            key: Span,
            value: Span,
            /// Earliest matching ASCII-folded section/key in this file, never a winner.
            duplicate_of: Option<usize>,
        },
        ListEntry {
            value: Span,
        },
        Unparsed,
    }
    #[derive(Debug, Serialize)]
    pub struct Line {
        pub number: usize,
        /// Includes its original LF/CRLF terminator when present.
        pub span: Span,
        pub content: LineKind,
    }
    #[derive(Debug, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "kebab-case")]
    pub enum State {
        Missing,
        Empty,
        Present,
    }
    #[derive(Debug, Serialize)]
    pub struct Source {
        /// Logical root/relative name; absolute private input paths are omitted.
        pub name: &'static str,
        pub state: State,
        pub bytes: Option<usize>,
        pub sha256: Option<String>,
        pub lines: Vec<Line>,
        raw_bytes: Vec<u8>,
    }
    impl Source {
        pub fn raw_bytes(&self) -> &[u8] {
            &self.raw_bytes
        }
    }

    #[derive(Debug, Serialize)]
    pub struct Snapshot {
        pub schema_version: u32,
        pub profile: ProfileId,
        pub sources: Vec<Source>,
        pub unverified: Vec<&'static str>,
        pub runtime_ready: bool,
        #[serde(skip)]
        _pins: Vec<File>,
    }

    struct Budget {
        limits: Limits,
        bytes: usize,
        lines: usize,
        keys: usize,
        identifiers: usize,
    }
    fn charge(counter: &mut usize, amount: usize, maximum: usize, what: &str) -> Result<()> {
        *counter = counter
            .checked_add(amount)
            .ok_or_else(|| Error::Unsupported(format!("profile {what} budget exceeded")))?;
        if *counter > maximum {
            return Err(Error::Unsupported(format!(
                "profile {what} budget exceeded"
            )));
        }
        Ok(())
    }

    /// Documents is the explicit Windows Known Folder root supplied by the caller,
    /// not an inferred USERPROFILE/Documents directory. Sources stay pinned until
    /// this snapshot is dropped. Missing paths are observations at inspection time.
    pub fn observe(
        install: &Path,
        documents: &Path,
        local_appdata: &Path,
        limits: Limits,
    ) -> Result<Snapshot> {
        if [install, documents, local_appdata]
            .iter()
            .any(|root| root.as_os_str().is_empty())
        {
            return Err(Error::Unsupported(
                "profile roots must be explicit nonempty paths".into(),
            ));
        }
        let ceilings = Limits::default();
        if limits.files > ceilings.files
            || limits.file_bytes > ceilings.file_bytes
            || limits.total_bytes > ceilings.total_bytes
            || limits.lines > ceilings.lines
            || limits.keys > ceilings.keys
            || limits.identifier_bytes > ceilings.identifier_bytes
        {
            return Err(Error::Unsupported(
                "profile limits exceed supported ceilings".into(),
            ));
        }
        let candidates = [
            (
                "installation/Fallout_default.ini",
                install.join("Fallout_default.ini"),
                true,
            ),
            (
                "documents/My Games/FalloutNV/Fallout.ini",
                documents.join("My Games/FalloutNV/Fallout.ini"),
                true,
            ),
            (
                "documents/My Games/FalloutNV/FalloutPrefs.ini",
                documents.join("My Games/FalloutNV/FalloutPrefs.ini"),
                true,
            ),
            (
                "local-appdata/FalloutNV/plugins.txt",
                local_appdata.join("FalloutNV/plugins.txt"),
                false,
            ),
            (
                "local-appdata/FalloutNV/NVDLCList.txt",
                local_appdata.join("FalloutNV/NVDLCList.txt"),
                false,
            ),
            (
                "local-appdata/FalloutNV/loadorder.txt",
                local_appdata.join("FalloutNV/loadorder.txt"),
                false,
            ),
            // A candidate location only; SInvalidationFile is never followed or applied.
            (
                "installation/Data/ArchiveInvalidation.txt",
                install.join("Data/ArchiveInvalidation.txt"),
                false,
            ),
        ];
        if candidates.len() > limits.files {
            return Err(Error::Unsupported("profile file budget exceeded".into()));
        }
        let mut budget = Budget {
            limits,
            bytes: 0,
            lines: 0,
            keys: 0,
            identifiers: 0,
        };
        let mut snapshot = Snapshot {
            schema_version: 1,
            profile: ProfileId::NvOriginal,
            sources: Vec::new(),
            unverified: vec![
                "runtime consumption and cross-file setting precedence",
                "active plugin/DLC order and archive auto-mounting",
                "archive/loose precedence and invalidation behavior",
                "binding interpretation and retail behavior",
            ],
            runtime_ready: false,
            _pins: Vec::new(),
        };
        for (name, path, ini) in candidates {
            let mut file = match open_source(&path) {
                Ok(file) => file,
                Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                    snapshot.sources.push(Source {
                        name,
                        state: State::Missing,
                        bytes: None,
                        sha256: None,
                        lines: vec![],
                        raw_bytes: vec![],
                    });
                    continue;
                }
                Err(error) => return Err(error),
            };
            let metadata = file.metadata().map_err(|e| io(&path, e))?;
            if !metadata.is_file() || metadata.len() > limits.file_bytes as u64 {
                return Err(Error::Unsupported(format!(
                    "{name}: profile file type/byte budget exceeded"
                )));
            }
            let length = metadata.len() as usize;
            charge(
                &mut budget.bytes,
                length,
                limits.total_bytes,
                "aggregate byte",
            )?;
            let mut raw_bytes = Vec::new();
            raw_bytes
                .try_reserve_exact(length)
                .map_err(|e| Error::Resolution(format!("profile source allocation: {e}")))?;
            raw_bytes.resize(length, 0);
            file.read_exact(&mut raw_bytes).map_err(|e| io(&path, e))?;
            let mut extra = [0u8];
            if file.read(&mut extra).map_err(|e| io(&path, e))? != 0 {
                return Err(malformed(
                    name,
                    length as u64,
                    "profile source length changed",
                ));
            }
            let sha256 = format!("{:x}", Sha256::digest(&raw_bytes));
            let lines = physical_lines(name, &raw_bytes, ini, &mut budget)?;
            snapshot.sources.push(Source {
                name,
                state: if length == 0 {
                    State::Empty
                } else {
                    State::Present
                },
                bytes: Some(length),
                sha256: Some(sha256),
                lines,
                raw_bytes,
            });
            snapshot._pins.push(file);
        }
        Ok(snapshot)
    }

    fn trim(raw: &[u8], offset: usize, bytes: usize) -> Span {
        let mut start = offset;
        let mut end = offset + bytes;
        while start < end && matches!(raw[start], b' ' | b'\t') {
            start += 1;
        }
        while end > start && matches!(raw[end - 1], b' ' | b'\t') {
            end -= 1;
        }
        Span {
            offset: start,
            bytes: end - start,
        }
    }
    fn physical_lines(name: &str, raw: &[u8], ini: bool, budget: &mut Budget) -> Result<Vec<Line>> {
        if raw.starts_with(&[0xff, 0xfe])
            || raw.starts_with(&[0xfe, 0xff])
            || raw.starts_with(&[0, 0, 0xfe, 0xff])
        {
            return Err(Error::Unsupported(format!(
                "{name}: UTF-16/32 profile encoding unsupported"
            )));
        }
        if let Some(offset) = raw
            .iter()
            .position(|b| *b < 32 && !matches!(*b, b'\t' | b'\n' | b'\r'))
        {
            return Err(malformed(
                name,
                offset as u64,
                "unsupported profile control byte",
            ));
        }
        let mut lines = Vec::new();
        let mut section: Option<Span> = None;
        let mut seen = BTreeMap::new();
        let mut offset = 0;
        for physical in raw.split_inclusive(|b| *b == b'\n') {
            charge(&mut budget.lines, 1, budget.limits.lines, "physical line")?;
            let mut length = physical.len();
            if physical.last() == Some(&b'\n') {
                length -= 1;
            }
            if length > 0 && physical[length - 1] == b'\r' {
                length -= 1;
            }
            let bom = if offset == 0 && physical.starts_with(&[0xef, 0xbb, 0xbf]) {
                3
            } else {
                0
            };
            let text = trim(raw, offset + bom, length.saturating_sub(bom));
            let value = &raw[text.offset..text.offset + text.bytes];
            let number = lines.len() + 1;
            let content = if value.is_empty() {
                LineKind::Blank
            } else if matches!(value[0], b';' | b'#') {
                LineKind::Comment
            } else if !ini {
                charge(&mut budget.keys, 1, budget.limits.keys, "key/list entry")?;
                LineKind::ListEntry { value: text }
            } else if value[0] == b'[' {
                // Keep malformed/unknown syntax visible and stop assigning its
                // following keys to the previous section.
                section = if value.len() > 2 && value.last() == Some(&b']') {
                    Some(trim(raw, text.offset + 1, text.bytes - 2))
                } else {
                    None
                };
                section.map_or(LineKind::Unparsed, |name| LineKind::Section { name })
            } else if let Some(equal) = value.iter().position(|b| *b == b'=') {
                let key = trim(raw, text.offset, equal);
                if key.bytes == 0 {
                    LineKind::Unparsed
                } else {
                    charge(&mut budget.keys, 1, budget.limits.keys, "key/list entry")?;
                    charge(
                        &mut budget.identifiers,
                        section.map_or(0, |v| v.bytes) + key.bytes,
                        budget.limits.identifier_bytes,
                        "identifier byte",
                    )?;
                    let normalized_section = section.map(|v| {
                        raw[v.offset..v.offset + v.bytes]
                            .iter()
                            .map(u8::to_ascii_lowercase)
                            .collect::<Vec<_>>()
                    });
                    let normalized_key: Vec<_> = raw[key.offset..key.offset + key.bytes]
                        .iter()
                        .map(u8::to_ascii_lowercase)
                        .collect();
                    let identity = (normalized_section, normalized_key);
                    let duplicate_of = seen.get(&identity).copied();
                    if duplicate_of.is_none() {
                        seen.insert(identity, number);
                    }
                    LineKind::Setting {
                        section,
                        key,
                        value: trim(raw, text.offset + equal + 1, text.bytes - equal - 1),
                        duplicate_of,
                    }
                }
            } else {
                LineKind::Unparsed
            };
            lines
                .try_reserve(1)
                .map_err(|e| Error::Resolution(format!("profile line allocation: {e}")))?;
            lines.push(Line {
                number,
                span: Span {
                    offset,
                    bytes: physical.len(),
                },
                content,
            });
            offset += physical.len();
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_ascii_without_destroying_legacy_bytes() {
        assert_eq!(
            AssetPath::new(b"Meshes\\A.NIF").unwrap(),
            AssetPath::new(b"meshes/a.nif").unwrap()
        );
        assert_eq!(
            AssetPath::new(b"textures/\xE9.dds").unwrap().bytes(),
            b"textures/\xE9.dds"
        );
        for bad in [
            b"../bad".as_slice(),
            b"x/../bad",
            b"C:\\bad",
            b"/bad",
            b"x//bad",
            b"x\0",
        ] {
            assert!(AssetPath::new(bad).is_err(), "{bad:?}");
        }
    }
    #[test]
    fn ambiguous_mount_never_silently_wins() {
        let mut mounts = MountIndex::default();
        for name in ["base.bsa", "patch.bsa"] {
            mounts
                .insert(AssetSource {
                    container: name.into(),
                    entry_index: 0,
                    original_path: b"mesh.nif".to_vec(),
                })
                .unwrap();
        }
        assert_eq!(mounts.candidates(b"MESH.NIF").unwrap().len(), 2);
        assert!(mounts.unique(b"mesh.nif").is_err());
    }
}
