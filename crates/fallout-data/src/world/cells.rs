//! Explicit source-world/XCLC queries. Persistent reference groups are not
//! spatial cells, and a grid lookup does not establish a retail activation rule.
use super::{
    Cell, decode_cell,
    dependencies::source_cohort,
    preparation::{CellModelPlan, CellModelPlanSet, Limits as ModelLimits, ModelSetLimits},
};
use crate::{
    Error, Result,
    identity::{FormKey, ProfileId},
    plugin::{self, RecordHeader},
    store::{RecordStore, SourceReceipt},
    terrain::preparation::{Limits as TerrainLimits, TextureSourcePlan},
    vfs::MountIndex,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub sources: usize,
    pub winners_scanned: usize,
    pub cells: usize,
    pub record_bytes: usize,
    pub read_bytes: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            sources: 256,
            winners_scanned: 1_000_000,
            cells: 65_536,
            record_bytes: 4 * 1024 * 1024,
            read_bytes: 64 * 1024 * 1024,
            metadata_bytes: 32 * 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let ceiling = Self::default();
        for (value, maximum) in [
            (self.sources, ceiling.sources),
            (self.winners_scanned, ceiling.winners_scanned),
            (self.cells, ceiling.cells),
            (self.record_bytes, ceiling.record_bytes),
            (self.read_bytes, ceiling.read_bytes),
            (self.metadata_bytes, ceiling.metadata_bytes),
        ] {
            if value > maximum {
                return Err(failure("limit exceeds source directory ceiling"));
            }
        }
        Ok(self)
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub sources: usize,
    pub winners_scanned: usize,
    pub cells: usize,
    /// Aggregate max(stored, decoded) body extents, excluding source hashing.
    pub read_bytes: usize,
    /// Conservative retained metadata including temporary key and grid-index storage.
    pub metadata_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    GridCell,
    PersistentGroup,
    Interior,
    NoGrid,
    Deleted,
}
#[derive(Debug, Serialize)]
pub struct Entry {
    pub key: FormKey,
    pub source_ordinal: usize,
    pub header: RecordHeader,
    pub parent_world_raw: u32,
    pub role: Role,
    pub decoded_sha256: Option<String>,
    /// Deleted winners stay header-only; no older grid/body is resurrected.
    pub fields: Option<Cell>,
}
#[derive(Debug, Serialize)]
pub struct Metadata {
    pub schema_version: u32,
    pub world: FormKey,
    pub world_source_ordinal: usize,
    /// Requested world payload and inherited settings remain unvalidated here.
    pub world_header: RecordHeader,
    pub sources: Vec<SourceReceipt>,
    pub source_cohort_sha256: String,
    /// Supplied source ordinal then physical winning-header order.
    pub entries: Vec<Entry>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct CellGridRequest {
    world: FormKey,
    grid: [i32; 2],
    cell: FormKey,
    source_cohort_sha256: String,
}
impl CellGridRequest {
    pub fn world(&self) -> &FormKey {
        &self.world
    }
    pub fn grid(&self) -> [i32; 2] {
        self.grid
    }
    pub fn cell(&self) -> &FormKey {
        &self.cell
    }
}
pub struct CellGridSources {
    metadata: Metadata,
    grids: BTreeMap<[i32; 2], Vec<usize>>,
}
/// Explicit caller order, sealed against the same directory/source cohort.
#[derive(Debug, Clone, Serialize)]
pub struct CellGridSetRequest {
    world: FormKey,
    source_cohort_sha256: String,
    requests: Vec<CellGridRequest>,
}
impl CellGridSetRequest {
    pub fn world(&self) -> &FormKey {
        &self.world
    }
    pub fn requests(&self) -> &[CellGridRequest] {
        &self.requests
    }
}
fn failure(message: &str) -> Error {
    Error::Resolution(format!("CELL grid sources: {message}"))
}
fn charge(used: &mut usize, amount: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(amount)
        .filter(|value| *value <= maximum)
        .ok_or_else(|| failure(&format!("{name} bound exceeded")))?;
    Ok(())
}

impl CellGridSources {
    pub fn load(store: &mut RecordStore, world: &FormKey, limits: Limits) -> Result<Self> {
        let limits = limits.validate()?;
        if world.profile != ProfileId::NvOriginal {
            return Err(failure("requires nv-original world identity"));
        }
        let location = store
            .winner(world)
            .ok_or_else(|| failure("requested world missing"))?;
        let header = &store.definition(location).header;
        if header.kind != *b"WRLD" || header.flags & plugin::DELETED != 0 {
            return Err(failure("requested world deleted or wrong kind"));
        }
        let mut usage = Usage::default();
        charge(
            &mut usage.metadata_bytes,
            4096 + 2 * world.origin_plugin.len(),
            limits.metadata_bytes,
            "metadata",
        )?;
        charge(
            &mut usage.sources,
            store.indices().len(),
            limits.sources,
            "sources",
        )?;
        charge(
            &mut usage.winners_scanned,
            store.winners.len(),
            limits.winners_scanned,
            "winning scan",
        )?;
        for source in store.indices() {
            charge(
                &mut usage.metadata_bytes,
                512 + 2 * source.census.name.len(),
                limits.metadata_bytes,
                "source metadata",
            )?;
        }
        let world_header = header.clone();
        let world_source_ordinal = location.plugin;
        let sources = store.source_receipts()?;
        let cohort = source_cohort(&sources);
        let mut selected = Vec::new();
        for (key, location) in store.winning_definitions() {
            let definition = store.definition(location);
            if definition.header.kind != *b"CELL" {
                continue;
            }
            let Some(raw) = definition.parent.world else {
                continue;
            };
            // key_for's temporary normalized origin is bounded before resolution.
            let census = &store.indices()[location.plugin].census;
            let origin = census
                .masters
                .iter()
                .map(String::len)
                .chain([census.name.len()])
                .max()
                .unwrap_or(0);
            if origin > limits.metadata_bytes.saturating_sub(usage.metadata_bytes) {
                return Err(failure(
                    "temporary world identity exceeds metadata remainder",
                ));
            }
            if store.key_for(location, raw)?.as_ref() != Some(world) {
                continue;
            }
            charge(&mut usage.cells, 1, limits.cells, "CELL count")?;
            charge(
                &mut usage.metadata_bytes,
                512 + 2 * key.origin_plugin.len(),
                limits.metadata_bytes,
                "CELL metadata",
            )?;
            selected.push((key.clone(), location, raw));
        }
        selected.sort_unstable_by_key(|(_, location, _)| {
            (location.plugin, store.definition(*location).header.offset)
        });
        let mut entries = Vec::with_capacity(selected.len());
        let mut grids: BTreeMap<[i32; 2], Vec<usize>> = BTreeMap::new();
        for (key, location, parent_world_raw) in selected {
            let header = store.definition(location).header.clone();
            let (role, fields, decoded_sha256) = if header.flags & plugin::DELETED != 0 {
                (Role::Deleted, None, None)
            } else {
                let maximum = limits
                    .record_bytes
                    .min(limits.read_bytes.saturating_sub(usage.read_bytes));
                if maximum == 0 {
                    return Err(failure("no source read allowance remains"));
                }
                let record = store.read_bounded(location, maximum)?;
                if record.integrity_issue.is_some() {
                    return Err(failure("tainted requested CELL body"));
                }
                charge(
                    &mut usage.read_bytes,
                    record.payload.len().max(header.stored_size as usize),
                    limits.read_bytes,
                    "read bytes",
                )?;
                // Existing CELL decoder copies at most one body into typed fields.
                charge(
                    &mut usage.metadata_bytes,
                    record.payload.len(),
                    limits.metadata_bytes,
                    "decoded CELL metadata",
                )?;
                let cell = decode_cell(&record, store.source_name(location))?;
                let role = if header.flags & plugin::PERSISTENT != 0 {
                    Role::PersistentGroup
                } else if cell.flags.value & 1 != 0 {
                    Role::Interior
                } else if cell.grid.is_some() {
                    Role::GridCell
                } else {
                    Role::NoGrid
                };
                if role == Role::GridCell {
                    grids
                        .entry(cell.grid.as_ref().expect("grid role requires XCLC").value)
                        .or_default()
                        .push(entries.len());
                }
                (
                    role,
                    Some(cell),
                    Some(format!("{:x}", Sha256::digest(&record.payload))),
                )
            };
            entries.push(Entry {
                key,
                source_ordinal: location.plugin,
                header,
                parent_world_raw,
                role,
                decoded_sha256,
                fields,
            });
        }
        Ok(Self {
            metadata: Metadata {
                schema_version: 1,
                world: world.clone(),
                world_source_ordinal,
                world_header,
                sources,
                source_cohort_sha256: cohort,
                entries,
                usage,
                limits,
                runtime_ready: false,
            },
            grids,
        })
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn request(&self, grid: [i32; 2]) -> Result<CellGridRequest> {
        let candidates = self.grids.get(&grid).map(Vec::as_slice).unwrap_or(&[]);
        let [entry] = candidates else {
            return Err(failure(if candidates.is_empty() {
                "explicit grid has no live exterior CELL"
            } else {
                "explicit grid has ambiguous winning CELLs"
            }));
        };
        Ok(CellGridRequest {
            world: self.metadata.world.clone(),
            grid,
            cell: self.metadata.entries[*entry].key.clone(),
            source_cohort_sha256: self.metadata.source_cohort_sha256.clone(),
        })
    }

    pub fn prepare_cell(
        &self,
        store: &mut RecordStore,
        request: &CellGridRequest,
        mounts: &MountIndex,
        limits: ModelLimits,
    ) -> Result<CellModelPlan> {
        self.validate_request_sources(store, request)?;
        let plan = CellModelPlan::load(store, &request.cell, mounts, limits)?;
        if plan.receipt().source_cohort_sha256 != request.source_cohort_sha256 {
            return Err(failure("CELL plan has another source cohort"));
        }
        Ok(plan)
    }

    pub fn request_set(&self, grids: &[[i32; 2]]) -> Result<CellGridSetRequest> {
        if grids.is_empty() || grids.len() > ModelSetLimits::default().grids {
            return Err(failure("explicit grid set count bound"));
        }
        let mut seen = BTreeSet::new();
        let mut requests = Vec::with_capacity(grids.len());
        let mut metadata = 4096 + 4 * self.metadata.world.origin_plugin.len();
        for grid in grids {
            if !seen.insert(*grid) {
                return Err(failure("duplicate explicit grid"));
            }
            let candidates = self.grids.get(grid).map(Vec::as_slice).unwrap_or(&[]);
            let [index] = candidates else {
                return Err(failure("explicit grid set has missing or ambiguous CELL"));
            };
            let cell = &self.metadata.entries[*index].key;
            charge(
                &mut metadata,
                1024 + 8 * self.metadata.world.origin_plugin.len() + 8 * cell.origin_plugin.len(),
                ModelSetLimits::default().metadata_bytes,
                "set selection metadata",
            )?;
            requests.push(self.request(*grid)?);
        }
        Ok(CellGridSetRequest {
            world: self.metadata.world.clone(),
            source_cohort_sha256: self.metadata.source_cohort_sha256.clone(),
            requests,
        })
    }

    /// No successful partial set escapes selection, source or aggregate admission.
    pub fn prepare_cells(
        &self,
        store: &mut RecordStore,
        request: &CellGridSetRequest,
        mounts: &MountIndex,
        limits: ModelSetLimits,
    ) -> Result<CellModelPlanSet> {
        let limits = limits.validate()?;
        if request.world != self.metadata.world
            || request.source_cohort_sha256 != self.metadata.source_cohort_sha256
        {
            return Err(failure("set belongs to another source directory"));
        }
        let usage = CellModelPlanSet::preflight(
            &request.world,
            &request.requests,
            &self.metadata.sources,
            limits,
        )?;
        for cell in &request.requests {
            self.validate_request_sources(store, cell)?;
        }
        CellModelPlanSet::load(
            store,
            &request.world,
            &request.source_cohort_sha256,
            &request.requests,
            mounts,
            limits,
            usage,
        )
    }

    /// Prepare existing strict LAND/world/layer/texture sources for the sealed
    /// explicit CELL. This does not admit inheritance or a terrain surface.
    pub fn prepare_terrain(
        &self,
        store: &mut RecordStore,
        request: &CellGridRequest,
        mounts: &MountIndex,
        limits: TerrainLimits,
    ) -> Result<TextureSourcePlan> {
        self.validate_request_sources(store, request)?;
        let plan = TextureSourcePlan::load(store, &request.cell, mounts, limits)?;
        if plan.receipt().source_cohort_sha256 != request.source_cohort_sha256 {
            return Err(failure("terrain plan has another source cohort"));
        }
        Ok(plan)
    }

    fn validate_request_sources(
        &self,
        store: &mut RecordStore,
        request: &CellGridRequest,
    ) -> Result<()> {
        if request.world != self.metadata.world
            || request.source_cohort_sha256 != self.metadata.source_cohort_sha256
            || self.request(request.grid)?.cell != request.cell
        {
            return Err(failure("request belongs to another source directory"));
        }
        // Reject added sources before receipt allocation or hashing their bodies.
        if store.indices().len() != self.metadata.sources.len() {
            return Err(failure("source cohort count changed"));
        }
        let current = store.source_receipts()?;
        if current.iter().zip(&self.metadata.sources).any(|(a, b)| {
            a.source_name != b.source_name
                || a.source_bytes != b.source_bytes
                || a.source_sha256 != b.source_sha256
        }) {
            return Err(failure("ordered source cohort changed"));
        }
        Ok(())
    }
}
