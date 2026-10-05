//! Immutable CPU terrain patches from exact private source-grid requests.
//! Existing evaluators define the engineering models; no GPU or collision admission.
use super::{
    Fields, Surface,
    blends::{self, BlendMaps},
    compare_neighbor,
    heights::{EdgeComparison, EdgeMismatch, SAMPLE_COUNT},
    mesh::{self, SurfaceMesh},
    preparation::{Limits as SourceLimits, PlanReceipt, TextureSourcePlan},
    reconstruct_cell,
};
use crate::{
    Error, Result,
    identity::FormKey,
    store::{RecordStore, SourceReceipt},
    vfs::MountIndex,
    world::cells::{CellGridRequest, CellGridSources},
};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Write, mem::size_of, sync::Arc};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub patches: usize,
    pub lands: usize,
    pub source_records: usize,
    pub source_read_bytes: usize,
    pub source_field_sites: usize,
    pub source_metadata_bytes: usize,
    /// Conservative sum of every plan's protected archive extents, without deduplication.
    pub mapped_source_bytes: u64,
    pub vertices: usize,
    pub indices: usize,
    pub layers: usize,
    pub weights: usize,
    pub output_bytes: usize,
    pub metadata_bytes: usize,
    pub seams: usize,
    pub seam_mismatch_slots: usize,
    pub source: SourceLimits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            patches: 8,
            lands: 8,
            source_records: 1024,
            source_read_bytes: 64 * 1024 * 1024,
            source_field_sites: 8 * 262144,
            source_metadata_bytes: 256 * 1024 * 1024,
            mapped_source_bytes: 16 * 1024 * 1024 * 1024,
            vertices: 8 * SAMPLE_COUNT,
            indices: 8 * 6144,
            layers: 8 * 256,
            weights: 8 * 256 * blends::SAMPLES,
            output_bytes: 16 * 1024 * 1024,
            metadata_bytes: 4 * 1024 * 1024,
            seams: 8,
            seam_mismatch_slots: 8 * 33,
            source: SourceLimits::default(),
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let max = Self::default();
        for (value, cap) in [
            (self.patches, max.patches),
            (self.lands, max.lands),
            (self.source_records, max.source_records),
            (self.source_read_bytes, max.source_read_bytes),
            (self.source_field_sites, max.source_field_sites),
            (self.source_metadata_bytes, max.source_metadata_bytes),
            (self.vertices, max.vertices),
            (self.indices, max.indices),
            (self.layers, max.layers),
            (self.weights, max.weights),
            (self.output_bytes, max.output_bytes),
            (self.metadata_bytes, max.metadata_bytes),
            (self.seams, max.seams),
            (self.seam_mismatch_slots, max.seam_mismatch_slots),
        ] {
            if value > cap {
                return Err(failure("limit exceeds ceiling"));
            }
        }
        if self.mapped_source_bytes > max.mapped_source_bytes {
            return Err(failure("mapping limit exceeds ceiling"));
        }
        Ok(self)
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub patches: usize,
    pub lands: usize,
    pub source_records: usize,
    pub source_read_bytes: usize,
    pub source_field_sites: usize,
    pub source_metadata_bytes: usize,
    pub mapped_source_bytes: u64,
    pub vertices: usize,
    pub indices: usize,
    pub layers: usize,
    pub weights: usize,
    /// Conservative retained element storage, including the mesh index capacity.
    pub output_bytes: usize,
    pub metadata_bytes: usize,
    pub seams: usize,
    /// Reserve all 33 possible mismatch entries per requested seam before comparison.
    pub seam_mismatch_slots: usize,
}
#[derive(Serialize)]
pub struct Patch {
    pub identity: String,
    source_plan: TextureSourcePlan,
    pub surface: Surface,
    pub mesh: SurfaceMesh,
    pub blends: BlendMaps,
    /// Complete XCLC flag word, including unused high bytes; absence is explicit.
    pub raw_quadrant_flags: Option<u32>,
    pub source_materials_prepared: bool,
    pub runtime_ready: bool,
}
impl Patch {
    pub fn source(&self) -> &PlanReceipt {
        self.source_plan.receipt()
    }
}
#[derive(Debug, Serialize)]
pub struct Seam {
    pub first_patch: usize,
    pub second_patch: usize,
    pub comparison: EdgeComparison,
}
#[derive(Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub identity: String,
    pub world: FormKey,
    pub source_cohort_sha256: String,
    pub patches: Vec<Patch>,
    pub seams: Vec<Seam>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
/// Reports cannot construct a bundle or escape a clone of its protected source plans.
pub struct TerrainPatchBundle(Arc<Receipt>);
impl TerrainPatchBundle {
    pub fn receipt(&self) -> &Receipt {
        &self.0
    }
    pub fn patches(&self) -> &[Patch] {
        &self.0.patches
    }
    pub fn identity(&self) -> &str {
        &self.0.identity
    }
    pub fn validate_sources(&self, store: &mut RecordStore) -> Result<()> {
        let sources = &self.patches()[0].source().sources;
        validate_sources(store, sources)
    }
    pub fn load(
        directory: &CellGridSources,
        store: &mut RecordStore,
        requests: &[CellGridRequest],
        mounts: &MountIndex,
        seams: &[[usize; 2]],
        limits: Limits,
    ) -> Result<Self> {
        let mut budget = Budget {
            limits: limits.validate()?,
            usage: Usage::default(),
        };
        // Check original nested ceilings before remaining-budget tightening can mask them.
        let source_limits = super::preparation::budget::Budget::new(limits.source, store)?.limits;
        if requests.is_empty() {
            return Err(failure("explicit patch set is empty"));
        }
        charge(
            &mut budget.usage.patches,
            requests.len(),
            limits.patches,
            "patches",
        )?;
        charge(&mut budget.usage.seams, seams.len(), limits.seams, "seams")?;
        charge(
            &mut budget.usage.seam_mismatch_slots,
            mul(seams.len(), 33)?,
            limits.seam_mismatch_slots,
            "seam mismatch slots",
        )?;
        budget.metadata(4096)?;
        // Validate source names/count before any new source-plan allocation.
        validate_sources(store, &directory.metadata().sources)?;
        let mut cells = BTreeSet::new();
        for request in requests {
            if request.world() != &directory.metadata().world
                || directory.request(request.grid())?.cell() != request.cell()
            {
                return Err(failure("request belongs to another source directory"));
            }
            budget.metadata(512 + 4 * request.cell().origin_plugin.len())?;
            if !cells.insert(request.cell().clone()) {
                return Err(failure("duplicate selected CELL"));
            }
        }
        for pair in seams {
            if pair[0] >= requests.len() || pair[1] >= requests.len() || pair[0] == pair[1] {
                return Err(failure("invalid explicit seam indices"));
            }
            // Both surfaces are passed untouched to the existing cardinal comparison.
            budget.metadata(1024 + 64 * size_of::<EdgeMismatch>())?;
        }
        let mut plans = Vec::with_capacity(requests.len());
        for request in requests {
            let mut source_limits = source_limits;
            source_limits.plugin_bytes = source_limits
                .plugin_bytes
                .min(limits.source_read_bytes - budget.usage.source_read_bytes);
            source_limits.metadata_bytes = source_limits
                .metadata_bytes
                .min(limits.source_metadata_bytes - budget.usage.source_metadata_bytes);
            source_limits.field_sites = source_limits
                .field_sites
                .min(limits.source_field_sites - budget.usage.source_field_sites);
            source_limits.layers = source_limits
                .layers
                .min(limits.layers - budget.usage.layers)
                .min(256);
            let remaining_records = limits.source_records - budget.usage.source_records;
            if remaining_records < 3 {
                return Err(failure("source records budget exceeded"));
            }
            source_limits.worlds = source_limits.worlds.min(remaining_records - 2);
            source_limits.records = source_limits.records.min(remaining_records);
            // Existing protected factory checks the sealed request/cohort and strict LAND branch.
            let plan = directory.prepare_terrain(store, request, mounts, source_limits)?;
            let r = plan.receipt();
            if r.root != *request.cell()
                || r.source_cohort_sha256 != directory.metadata().source_cohort_sha256
            {
                return Err(failure("terrain source plan identity differs"));
            }
            charge(
                &mut budget.usage.source_read_bytes,
                r.usage.plugin_bytes,
                limits.source_read_bytes,
                "source reads",
            )?;
            charge(
                &mut budget.usage.source_field_sites,
                r.usage.field_sites,
                limits.source_field_sites,
                "source field sites",
            )?;
            charge(
                &mut budget.usage.source_metadata_bytes,
                r.usage.metadata_bytes,
                limits.source_metadata_bytes,
                "source metadata",
            )?;
            charge(
                &mut budget.usage.source_records,
                r.physical_contexts.len(),
                limits.source_records,
                "source records",
            )?;
            for archive in &r.archives {
                budget.usage.mapped_source_bytes = budget
                    .usage
                    .mapped_source_bytes
                    .checked_add(archive.source_bytes)
                    .filter(|n| *n <= limits.mapped_source_bytes)
                    .ok_or_else(|| failure("retained mapping budget exceeded"))?;
            }
            let [land_entry] = r.terrain.landscapes.as_slice() else {
                return Err(failure("requires exactly one winning present LAND"));
            };
            let Some(Fields::Land(land)) = &land_entry.fields else {
                return Err(failure("LAND fields absent"));
            };
            if land.heights.is_none() {
                return Err(failure("LAND lacks required VHGT"));
            }
            let Some(Fields::Cell(cell)) = &r.terrain.cell.fields else {
                return Err(failure("CELL fields absent"));
            };
            if cell.grid.as_ref().map(|f| f.value) != Some(request.grid()) {
                return Err(failure("CELL source grid differs"));
            }
            // This is the existing engineering model's absent-byte convention, not a retail default.
            let hidden = cell.land_flags().unwrap_or(0);
            if hidden & !15 != 0 {
                return Err(failure("unknown source land hide bits"));
            }
            charge(&mut budget.usage.lands, 1, limits.lands, "LAND records")?;
            charge(
                &mut budget.usage.vertices,
                SAMPLE_COUNT,
                limits.vertices,
                "vertices",
            )?;
            let indices = (4 - hidden.count_ones() as usize) * 1536;
            charge(
                &mut budget.usage.indices,
                indices,
                limits.indices,
                "indices",
            )?;
            charge(
                &mut budget.usage.layers,
                land.layers.len(),
                limits.layers,
                "layers",
            )?;
            let weights = mul(land.layers.len(), blends::SAMPLES)?;
            charge(
                &mut budget.usage.weights,
                weights,
                limits.weights,
                "weights",
            )?;
            let sample_bytes = 4
                + 24
                + if land.normals.is_some() { 12 } else { 0 }
                + if land.colors.is_some() { 3 } else { 0 };
            let output = mul(SAMPLE_COUNT, sample_bytes)?
                .checked_add(6144 * 4)
                .and_then(|n| n.checked_add(weights))
                .ok_or_else(|| failure("output size overflow"))?;
            charge(
                &mut budget.usage.output_bytes,
                output,
                limits.output_bytes,
                "output bytes",
            )?;
            budget.metadata(
                4096 + mul(land.layers.len(), 256)? + 4 * request.cell().origin_plugin.len(),
            )?;
            // All cumulative admission is complete before retaining this plan or building CPU outputs.
            plans.push(plan);
        }
        let mut patches = Vec::with_capacity(plans.len());
        for plan in plans {
            let report = plan.terrain();
            let Some(Fields::Land(land)) = &report.landscapes[0].fields else {
                unreachable!("admitted LAND");
            };
            let Some(Fields::Cell(cell)) = &report.cell.fields else {
                unreachable!("admitted CELL");
            };
            let raw_quadrant_flags = cell.quadrant_flags.as_ref().map(|f| f.value);
            let surface = reconstruct_cell(report)?;
            let mesh = mesh::build(land, cell.land_flags().unwrap_or(0))?;
            let blends = blends::build(land)?;
            let identity = digest(
                b"nv-terrain-cpu-patch-v1\0",
                &(
                    plan.identity(),
                    &surface,
                    &mesh,
                    &blends,
                    raw_quadrant_flags,
                ),
            )?;
            patches.push(Patch {
                identity,
                source_plan: plan,
                surface,
                mesh,
                blends,
                raw_quadrant_flags,
                source_materials_prepared: false,
                runtime_ready: false,
            });
        }
        let mut comparisons = Vec::with_capacity(seams.len());
        for pair in seams {
            comparisons.push(Seam {
                first_patch: pair[0],
                second_patch: pair[1],
                comparison: compare_neighbor(&patches[pair[0]].surface, &patches[pair[1]].surface)?,
            });
        }
        let identity = digest(
            b"nv-terrain-patch-bundle-v1\0",
            &(
                &directory.metadata().world,
                &directory.metadata().source_cohort_sha256,
                patches.iter().map(|p| &p.identity).collect::<Vec<_>>(),
                &comparisons,
            ),
        )?;
        Ok(Self(Arc::new(Receipt {
            schema_version: 1,
            identity,
            world: directory.metadata().world.clone(),
            source_cohort_sha256: directory.metadata().source_cohort_sha256.clone(),
            patches,
            seams: comparisons,
            usage: budget.usage,
            limits,
            runtime_ready: false,
        })))
    }
}
impl Serialize for TerrainPatchBundle {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.receipt().serialize(serializer)
    }
}
struct Budget {
    limits: Limits,
    usage: Usage,
}
impl Budget {
    fn metadata(&mut self, bytes: usize) -> Result<()> {
        charge(
            &mut self.usage.metadata_bytes,
            bytes,
            self.limits.metadata_bytes,
            "metadata",
        )
    }
}
fn validate_sources(store: &mut RecordStore, sources: &[SourceReceipt]) -> Result<()> {
    if store.indices().len() != sources.len()
        || store
            .indices()
            .iter()
            .zip(sources)
            .any(|(a, b)| a.census.name != b.source_name)
    {
        return Err(failure("ordered source names/count changed"));
    }
    if store
        .source_receipts()?
        .iter()
        .zip(sources)
        .any(|(a, b)| a.source_bytes != b.source_bytes || a.source_sha256 != b.source_sha256)
    {
        return Err(failure("ordered source bytes changed"));
    }
    Ok(())
}
fn charge(used: &mut usize, amount: usize, limit: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(amount)
        .filter(|n| *n <= limit)
        .ok_or_else(|| failure(&format!("{name} budget exceeded")))?;
    Ok(())
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b)
        .ok_or_else(|| failure("element size overflow"))
}
fn failure(message: &str) -> Error {
    Error::Resolution(format!("terrain source patches: {message}"))
}
fn digest(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let mut writer = HashWriter {
        hash: Sha256::new(),
        bytes: 0,
    };
    writer.hash.update(domain);
    serde_json::to_writer(&mut writer, value).map_err(|e| failure(&e.to_string()))?;
    Ok(format!("{:x}", writer.hash.finalize()))
}
struct HashWriter {
    hash: Sha256,
    bytes: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= 512 * 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("terrain patch identity ceiling exceeded"))?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
