//! On-demand record access over immutable, indexed plugin files. The index owns
//! headers and provenance; decoded bodies are read only when a consumer needs them.
use crate::{
    Error, Result,
    baseline::open_source,
    content::{self, Definition, PluginIndex},
    identity::{FormKey, ProfileId, plugin_name, resolve_form},
    plugin::{self, Limits, Record},
};
use std::{collections::BTreeMap, fs::File, path::Path};

#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub(crate) plugin: usize,
    pub(crate) record: usize,
}

pub struct RecordStore {
    // Windows write-denying handles stay open from indexing through the last read.
    files: Vec<File>,
    pub(crate) indices: Vec<PluginIndex>,
    pub(crate) winners: BTreeMap<FormKey, Location>,
    limits: Limits,
}

impl RecordStore {
    pub fn indices(&self) -> &[PluginIndex] {
        &self.indices
    }

    pub fn open_nv(data: &Path, names: &[String], limits: Limits) -> Result<Self> {
        Self::open_selected(data, names, limits, false)
    }

    /// Startup validates framing, identities and CELL metadata. Every later read
    /// still checks the exact indexed header and strictly validates its payload.
    pub fn open_nv_headers(data: &Path, names: &[String], limits: Limits) -> Result<Self> {
        Self::open_selected(data, names, limits, true)
    }

    fn open_selected(
        data: &Path,
        names: &[String],
        limits: Limits,
        headers_only: bool,
    ) -> Result<Self> {
        if names.is_empty() {
            return Err(Error::Resolution("load order is empty".into()));
        }
        let mut files = Vec::new();
        let mut indices = Vec::new();
        for name in names {
            plugin_name(name)?;
            let path = data.join(name);
            files.push(open_source(&path)?);
            indices.push(if headers_only {
                content::index_plugin_headers(&path, limits)?
            } else {
                content::index_plugin_with_limits(&path, limits)?
            });
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
        Ok(Self {
            files,
            indices,
            winners,
            limits,
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
        let index = &self.indices[location.plugin];
        plugin::read_indexed(
            &mut self.files[location.plugin],
            index.census.source_bytes,
            &index.records[location.record].header,
            &index.census.name,
            self.limits,
        )
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
