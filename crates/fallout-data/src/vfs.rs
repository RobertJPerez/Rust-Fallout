//! Byte paths for archive members. A mount lists candidates; it does not guess NV's
//! archive invalidation rules or silently choose a winner on a collision.
use crate::{Error, Result};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct AssetPath(Vec<u8>);

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
