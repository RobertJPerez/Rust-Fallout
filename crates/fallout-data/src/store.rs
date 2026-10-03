//! On-demand record access over immutable, indexed plugin files. The index owns
//! headers and provenance; decoded bodies are read only when a consumer needs them.
use crate::{
    Error, Result,
    baseline::open_source,
    content::{self, Definition, PluginIndex},
    identity::{FormKey, ProfileId, plugin_name, resolve_form},
    index_cache,
    plugin::{self, Limits, Record},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub(crate) plugin: usize,
    pub(crate) record: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct SourceReceipt {
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
}

pub struct RecordStore {
    // Windows write-denying handles stay open from indexing through the last read.
    files: Vec<File>,
    pub(crate) indices: Vec<PluginIndex>,
    pub(crate) winners: BTreeMap<FormKey, Location>,
    limits: Limits,
    index_cache: Option<index_cache::Report>,
    source_digests: BTreeMap<usize, String>,
}

impl RecordStore {
    pub fn winner(&self, key: &FormKey) -> Option<Location> {
        self.winners.get(key).copied()
    }

    pub fn winning_definitions(&self) -> impl Iterator<Item = (&FormKey, Location)> {
        self.winners.iter().map(|(key, location)| (key, *location))
    }

    pub fn indices(&self) -> &[PluginIndex] {
        &self.indices
    }

    pub fn open_nv(data: &Path, names: &[String], limits: Limits) -> Result<Self> {
        Self::open_selected(data, names, limits, false, None)
    }

    /// Startup validates framing, identities and CELL metadata. Every later read
    /// still checks the exact indexed header and strictly validates its payload.
    pub fn open_nv_headers(data: &Path, names: &[String], limits: Limits) -> Result<Self> {
        Self::open_selected(data, names, limits, true, None)
    }

    /// Hash every source under its retained read-only handle before cache lookup.
    /// A hit reuses metadata only; strict payload access and winner resolution stay
    /// identical to the uncached path.
    pub fn open_nv_headers_cached(
        data: &Path,
        names: &[String],
        limits: Limits,
        root: &Path,
    ) -> Result<Self> {
        Self::open_selected(data, names, limits, true, Some(root))
    }

    pub fn index_cache_report(&self) -> Option<&index_cache::Report> {
        self.index_cache.as_ref()
    }

    fn open_selected(
        data: &Path,
        names: &[String],
        limits: Limits,
        headers_only: bool,
        index_root: Option<&Path>,
    ) -> Result<Self> {
        if names.is_empty() {
            return Err(Error::Resolution("load order is empty".into()));
        }
        if index_root.is_some() && limits.inspect_checksum_mismatches {
            return Err(Error::Resolution(
                "index caching requires strict header indexing".into(),
            ));
        }
        let source_tree = if data
            .file_name()
            .is_some_and(|v| v.eq_ignore_ascii_case("Data"))
        {
            data.parent()
                .ok_or_else(|| Error::Resolution("Data has no installation parent".into()))?
        } else {
            data
        };
        let cache_root = index_root
            .map(|root| {
                let root = crate::cache::validate_root(root, source_tree)?;
                // Protect the actual data directory too if the supplied Data path
                // resolves through a junction outside its installation parent.
                crate::cache::validate_root(&root, data)
            })
            .transpose()?;
        let mut files = Vec::new();
        let mut indices = Vec::new();
        let mut receipts = Vec::new();
        let mut source_bytes_hashed = 0u64;
        for name in names {
            plugin_name(name)?;
            let path = data.join(name);
            let mut file = open_source(&path)?;
            let index = if let Some(root) = &cache_root {
                let (bytes, digest) = crate::baseline::digest_reader(&mut file)
                    .map_err(|error| crate::io(&path, error))?;
                source_bytes_hashed = source_bytes_hashed
                    .checked_add(bytes)
                    .ok_or_else(|| Error::Resolution("hashed byte count overflow".into()))?;
                let (index, receipt) = index_cache::load_or_build(
                    root,
                    source_tree,
                    &path,
                    index_cache::identity(name, digest, limits),
                    bytes,
                    limits,
                )?;
                receipts.push(receipt);
                index
            } else if headers_only {
                content::index_plugin_headers(&path, limits)?
            } else {
                content::index_plugin_with_limits(&path, limits)?
            };
            files.push(file);
            indices.push(index);
        }
        // Keep the same missing-master, duplicate-identity and taint rules as the
        // headless resolver. Inspection can carry taint, never erase it.
        content::resolve_structure(
            &indices,
            ProfileId::NvOriginal,
            limits.inspect_checksum_mismatches,
        )?;
        let mut winners = BTreeMap::new();
        for (plugin, index) in indices.iter().enumerate() {
            for (record, definition) in index.records.iter().enumerate() {
                let key = resolve_form(
                    ProfileId::NvOriginal,
                    &index.census.name,
                    &index.census.masters,
                    definition.header.form_id,
                )?
                .ok_or_else(|| Error::Resolution("null definition".into()))?;
                winners.insert(key, Location { plugin, record });
            }
        }
        let index_cache = if cache_root.is_some() {
            let ordered: Vec<_> = receipts
                .iter()
                .map(|row| (plugin_name(&row.plugin), &row.source_sha256, &row.key))
                .map(|(name, source, key)| name.map(|name| (name, source, key)))
                .collect::<Result<_>>()?;
            Some(index_cache::Report {
                format: "nv-header-index-v2",
                source_bytes_hashed,
                ordered_source_sha256: format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&ordered)
                            .map_err(|error| Error::Resolution(error.to_string()))?
                    )
                ),
                plugins: receipts,
                scope: "source-bound TES4/CELL metadata; other payloads remain deferred and validate strictly on access; winners rebuilt for supplied order",
            })
        } else {
            None
        };
        let source_digests = index_cache
            .as_ref()
            .map(|report| {
                report
                    .plugins
                    .iter()
                    .enumerate()
                    .map(|(i, row)| (i, row.source_sha256.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            files,
            indices,
            winners,
            limits,
            index_cache,
            source_digests,
        })
    }

    pub fn integrity_failures(&self) -> usize {
        self.indices
            .iter()
            .map(|p| p.census.integrity_issues.len())
            .sum()
    }

    pub fn deferred_payloads(&self) -> u64 {
        self.indices
            .iter()
            .map(|p| p.census.record_payloads_deferred)
            .sum()
    }

    pub fn definition(&self, location: Location) -> &Definition {
        &self.indices[location.plugin].records[location.record]
    }

    pub fn source_name(&self, location: Location) -> &str {
        &self.indices[location.plugin].census.name
    }

    pub fn key_for(&self, location: Location, raw: u32) -> Result<Option<FormKey>> {
        let index = &self.indices[location.plugin];
        resolve_form(
            ProfileId::NvOriginal,
            &index.census.name,
            &index.census.masters,
            raw,
        )
    }

    pub fn read(&mut self, location: Location) -> Result<Record> {
        self.read_bounded(location, self.limits.max_record_bytes)
    }

    /// A consumer may tighten the existing strict limits, never relax them.
    pub fn read_bounded(&mut self, location: Location, maximum: usize) -> Result<Record> {
        let mut limits = self.limits;
        limits.max_record_bytes = limits.max_record_bytes.min(maximum);
        limits.max_decoded_bytes = limits.max_decoded_bytes.min(maximum as u64);
        let index = &self.indices[location.plugin];
        plugin::read_indexed(
            &mut self.files[location.plugin],
            index.census.source_bytes,
            &index.records[location.record].header,
            &index.census.name,
            limits,
        )
    }

    /// Provenance uses the same retained, write-denying handle as indexed reads.
    /// Hash at most once per source, reusing a digest already obtained for caching.
    pub fn source_receipts(&mut self) -> Result<Vec<SourceReceipt>> {
        let mut receipts = Vec::with_capacity(self.indices.len());
        for index in 0..self.indices.len() {
            let hash = self.plugin_digest(index)?;
            let source = &self.indices[index].census;
            receipts.push(SourceReceipt {
                source_name: source.name.clone(),
                source_bytes: source.source_bytes,
                source_sha256: hash,
            });
        }
        Ok(receipts)
    }

    pub fn source_digest(&mut self, location: Location) -> Result<String> {
        self.plugin_digest(location.plugin)
    }

    fn plugin_digest(&mut self, plugin_index: usize) -> Result<String> {
        if let Some(digest) = self.source_digests.get(&plugin_index) {
            return Ok(digest.clone());
        }
        let file = &mut self.files[plugin_index];
        let name = &self.indices[plugin_index].census.name;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| crate::io(name, error))?;
        let (_, digest) =
            crate::baseline::digest_reader(file).map_err(|error| crate::io(name, error))?;
        self.source_digests.insert(plugin_index, digest.clone());
        Ok(digest)
    }

    pub fn cell_by_editor_id(&self, name: &[u8]) -> Result<(FormKey, Location)> {
        let mut found = None;
        for (key, location) in &self.winners {
            let definition = self.definition(*location);
            if definition.header.kind == *b"CELL"
                && definition
                    .editor_id
                    .as_ref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(name))
                && definition.header.flags & plugin::DELETED == 0
            {
                if found.is_some() {
                    return Err(Error::Resolution("ambiguous CELL editor ID".into()));
                }
                found = Some((key.clone(), *location));
            }
        }
        found.ok_or_else(|| {
            Error::Resolution(format!(
                "CELL editor ID not found: {}",
                String::from_utf8_lossy(name)
            ))
        })
    }
}
