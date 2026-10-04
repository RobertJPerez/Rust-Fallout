//! Explicit source CELL sets. One winner scan and one global admission ledger.
use super::{Limits, SourceMesh, load::load_one_bounded};
use crate::{
    Error, Result,
    identity::{FormKey, plugin_name},
    plugin,
    store::RecordStore,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub struct CellSetLimits {
    pub cells: usize,
    pub meshes: usize,
    pub index_visits: usize,
    /// Whole indexed plugin cohort, admitted before any new digest reads.
    pub source_bytes: u64,
    /// Total selected CELL/NAVM payload bytes, fields and source elements.
    pub records: Limits,
    pub identity_metadata_bytes: usize,
}
impl Default for CellSetLimits {
    fn default() -> Self {
        Self {
            cells: 64,
            meshes: 10_000,
            index_visits: 4_000_000,
            source_bytes: 4 * 1024 * 1024 * 1024,
            records: Limits::default(),
            identity_metadata_bytes: 16 * 1024 * 1024,
        }
    }
}
impl CellSetLimits {
    fn validate(self) -> Result<Self> {
        let max = Self::default();
        for (value, ceiling) in [
            (self.cells, max.cells),
            (self.meshes, max.meshes),
            (self.index_visits, max.index_visits),
            (self.records.record_bytes, max.records.record_bytes),
            (self.records.fields, max.records.fields),
            (self.records.elements, max.records.elements),
            (self.identity_metadata_bytes, max.identity_metadata_bytes),
        ] {
            if value > ceiling {
                return Err(Error::Unsupported("navigation cell-set ceiling".into()));
            }
        }
        if self.source_bytes > max.source_bytes {
            return Err(Error::Unsupported("navigation source byte ceiling".into()));
        }
        Ok(self)
    }
}
pub(super) fn charge(left: &mut usize, count: usize, name: &str) -> Result<()> {
    *left = left
        .checked_sub(count)
        .ok_or_else(|| Error::Unsupported(name.into()))?;
    Ok(())
}
fn identity_charge(left: &mut usize, name: &str) -> Result<()> {
    let bytes = name
        .len()
        .checked_mul(4)
        .and_then(|n| n.checked_add(1024))
        .ok_or_else(|| Error::Unsupported("navigation identity metadata overflow".into()))?;
    charge(left, bytes, "navigation identity metadata")
}
#[derive(Debug, Serialize)]
pub struct CellSource {
    pub key: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: plugin::RecordHeader,
    pub decoded_bytes: usize,
    pub decoded_sha256: String,
    pub fields: usize,
}
#[derive(Debug, Serialize)]
pub struct CellSetUsage {
    pub source_bytes: u64,
    pub index_visits: usize,
    pub cells: usize,
    pub meshes: usize,
    pub record_bytes: usize,
    pub fields: usize,
    pub elements: usize,
    pub identity_metadata_bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct CellSet {
    /// Stable source-key order, independent of caller selection order.
    pub cells: Vec<CellSource>,
    pub meshes: Vec<SourceMesh>,
    pub usage: CellSetUsage,
}
pub fn load_cells(
    store: &mut RecordStore,
    cells: &[FormKey],
    limits: CellSetLimits,
) -> Result<CellSet> {
    let limits = limits.validate()?;
    if cells.is_empty() || cells.len() > limits.cells {
        return Err(Error::Unsupported("navigation selected cell budget".into()));
    }
    let mut visits = limits.index_visits;
    let mut metadata = limits.identity_metadata_bytes;
    charge(&mut metadata, 4096, "navigation identity metadata")?;
    let mut source_bytes = 0u64;
    let mut longest_name = 0;
    for index in store.indices() {
        charge(&mut visits, 1, "navigation index visits")?;
        source_bytes = source_bytes
            .checked_add(index.census.source_bytes)
            .ok_or_else(|| Error::Unsupported("navigation source byte overflow".into()))?;
        if source_bytes > limits.source_bytes {
            return Err(Error::Unsupported("navigation source bytes".into()));
        }
        for name in std::iter::once(&index.census.name).chain(&index.census.masters) {
            charge(&mut visits, 1, "navigation index visits")?;
            identity_charge(&mut metadata, name)?;
            longest_name = longest_name.max(name.len());
        }
    }
    let mut selected_cells = BTreeSet::new();
    for cell in cells {
        identity_charge(&mut metadata, &cell.origin_plugin)?;
        if plugin_name(&cell.origin_plugin)? != cell.origin_plugin
            || !selected_cells.insert(cell.clone())
        {
            return Err(Error::Resolution(
                "navigation CELL identities must be exact and unique".into(),
            ));
        }
    }
    let mut records = limits.records;
    let mut cell_sources = Vec::with_capacity(cells.len());
    // Validate and read every selected live CELL before retaining any NAVM.
    for cell in &selected_cells {
        let at = store
            .winner(cell)
            .ok_or_else(|| Error::Resolution("navigation CELL winner is missing".into()))?;
        let header = &store.definition(at).header;
        if header.kind != *b"CELL" || header.flags & plugin::DELETED != 0 {
            return Err(Error::Resolution(
                "navigation target must be a live CELL".into(),
            ));
        }
        let record = store.read_bounded(at, records.record_bytes)?;
        if record.integrity_issue.is_some() {
            return Err(Error::Resolution(
                "navigation CELL source is tainted".into(),
            ));
        }
        let mut fields = 0;
        plugin::visit_subrecords(&record, store.source_name(at), |_| {
            charge(&mut records.fields, 1, "navigation aggregate field budget")?;
            fields += 1;
            Ok(())
        })?;
        charge(
            &mut records.record_bytes,
            record.payload.len(),
            "navigation aggregate record byte budget",
        )?;
        identity_charge(&mut metadata, &cell.origin_plugin)?;
        identity_charge(&mut metadata, store.source_name(at))?;
        cell_sources.push(CellSource {
            key: cell.clone(),
            source_plugin: store.source_name(at).to_owned(),
            source_sha256: store.source_digest(at)?,
            header: record.header,
            decoded_bytes: record.payload.len(),
            decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
            fields,
        });
    }
    let mut selected = Vec::new();
    let mut identities = BTreeSet::new();
    // Scan all winners once, rather than renewing scan/record limits per CELL.
    for (key, at) in store.winning_definitions() {
        charge(&mut visits, 1, "navigation index visits")?;
        let def = store.definition(at);
        if def.header.kind != *b"NAVM" || def.header.flags & plugin::DELETED != 0 {
            continue;
        }
        let Some(raw) = def.parent.cell else {
            continue;
        };
        let Some(cell) = store.key_for(at, raw)? else {
            continue;
        };
        if !selected_cells.contains(&cell) {
            continue;
        }
        if selected.len() >= limits.meshes {
            return Err(Error::Unsupported("navigation selected mesh budget".into()));
        }
        identity_charge(&mut metadata, &key.origin_plugin)?;
        identity_charge(&mut metadata, &cell.origin_plugin)?;
        if !identities.insert(key.clone()) {
            return Err(Error::Resolution(
                "duplicate navigation mesh identity".into(),
            ));
        }
        selected.push((key.clone(), at, cell));
    }
    let mut meshes = Vec::with_capacity(selected.len());
    for (key, at, cell) in selected {
        let source = load_one_bounded(store, key, at, records, &mut metadata, longest_name)?;
        if source.cell.as_ref() != Some(&cell) {
            return Err(Error::Resolution(
                "NAVM DATA cell differs from source GRUP ancestry".into(),
            ));
        }
        let count = source.mesh.vertices.len()
            + source.mesh.triangles.len()
            + source.mesh.edge_links.len()
            + source.mesh.cover_triangles.len()
            + source.mesh.door_links.len();
        charge(
            &mut records.elements,
            count,
            "navigation aggregate element budget",
        )?;
        charge(
            &mut records.record_bytes,
            source.mesh.decoded_bytes,
            "navigation aggregate record byte budget",
        )?;
        charge(
            &mut records.fields,
            source.mesh.fields.len(),
            "navigation aggregate field budget",
        )?;
        meshes.push(source);
    }
    let usage = CellSetUsage {
        source_bytes,
        index_visits: limits.index_visits - visits,
        cells: cell_sources.len(),
        meshes: meshes.len(),
        record_bytes: limits.records.record_bytes - records.record_bytes,
        fields: limits.records.fields - records.fields,
        elements: limits.records.elements - records.elements,
        identity_metadata_bytes: limits.identity_metadata_bytes - metadata,
    };
    Ok(CellSet {
        cells: cell_sources,
        meshes,
        usage,
    })
}
