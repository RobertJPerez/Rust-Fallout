//! Explicit ordered CELL model plans; construction quotas never imply activation.
use super::{CellModelPlan, Limits, plan::ArchivePool};
use crate::{
    Error, Result,
    identity::FormKey,
    store::{RecordStore, SourceReceipt},
    vfs::MountIndex,
    world::cells::CellGridRequest,
};
use serde::{Serialize, Serializer, ser::SerializeSeq};
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelSetLimits {
    pub grids: usize,
    pub sources: usize,
    pub winners_scanned: usize,
    pub nodes: usize,
    pub edges: usize,
    pub bases: usize,
    pub candidates: usize,
    pub models: usize,
    pub field_sites: usize,
    pub read_bytes: usize,
    pub decoded_bytes: usize,
    pub model_bytes: usize,
    pub probe_metadata_bytes: usize,
    pub metadata_bytes: usize,
    pub archives: usize,
    pub mapped_bytes: u64,
    /// Each existing factory also retains its independently lowerable ceilings.
    pub cell: Limits,
}
impl Default for ModelSetLimits {
    fn default() -> Self {
        Self {
            grids: 8,
            sources: 2048,
            winners_scanned: 8_000_000,
            nodes: 4096,
            edges: 16384,
            bases: 1024,
            candidates: 8192,
            models: 1024,
            field_sites: 262144,
            read_bytes: 64 * 1024 * 1024,
            decoded_bytes: 64 * 1024 * 1024,
            model_bytes: 64 * 1024 * 1024,
            probe_metadata_bytes: 512 * 1024 * 1024,
            metadata_bytes: 32 * 1024 * 1024,
            archives: 8,
            mapped_bytes: 16 * 1024 * 1024 * 1024,
            cell: Default::default(),
        }
    }
}
impl ModelSetLimits {
    pub(in crate::world) fn validate(self) -> Result<Self> {
        self.cell.validate()?;
        let max = Self::default();
        let values = [
            self.grids,
            self.sources,
            self.winners_scanned,
            self.nodes,
            self.edges,
            self.bases,
            self.candidates,
            self.models,
            self.field_sites,
            self.read_bytes,
            self.decoded_bytes,
            self.model_bytes,
            self.probe_metadata_bytes,
            self.metadata_bytes,
            self.archives,
        ];
        let maxima = [
            max.grids,
            max.sources,
            max.winners_scanned,
            max.nodes,
            max.edges,
            max.bases,
            max.candidates,
            max.models,
            max.field_sites,
            max.read_bytes,
            max.decoded_bytes,
            max.model_bytes,
            max.probe_metadata_bytes,
            max.metadata_bytes,
            max.archives,
        ];
        if values.iter().zip(maxima).any(|(value, max)| *value > max)
            || self.mapped_bytes > max.mapped_bytes
        {
            return Err(failure("limit exceeds ceiling"));
        }
        Ok(self)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ModelSetUsage {
    pub grids: usize,
    pub sources: usize,
    pub winners_scanned: usize,
    pub nodes: usize,
    pub edges: usize,
    pub bases: usize,
    pub candidates: usize,
    /// Requests count per CELL, even when their source payloads coincide.
    pub models: usize,
    pub field_sites: usize,
    /// Actual max(stored, decoded) plugin-body reads; source hashing is separate.
    pub read_bytes: usize,
    pub decoded_bytes: usize,
    pub model_bytes: usize,
    pub probe_metadata_bytes: usize,
    pub metadata_bytes: usize,
    /// Distinct protected inputs actually reused by the returned plans.
    pub archives: usize,
    pub mapped_bytes: u64,
}

/// A caller-owned immutable source set, not a residency owner or process quota.
/// Cloned individual plans share their admitted immutable metadata/mappings.
pub struct CellModelPlanSet {
    world: FormKey,
    cohort: String,
    identity: String,
    requests: Vec<CellGridRequest>,
    plans: Vec<CellModelPlan>,
    usage: ModelSetUsage,
    limits: ModelSetLimits,
}
impl CellModelPlanSet {
    pub fn world(&self) -> &FormKey {
        &self.world
    }
    pub fn source_cohort_sha256(&self) -> &str {
        &self.cohort
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn requests(&self) -> &[CellGridRequest] {
        &self.requests
    }
    pub fn plan(&self, index: usize) -> Option<&CellModelPlan> {
        self.plans.get(index)
    }
    pub fn usage(&self) -> &ModelSetUsage {
        &self.usage
    }
    pub fn limits(&self) -> &ModelSetLimits {
        &self.limits
    }

    pub(in crate::world) fn preflight(
        world: &FormKey,
        requests: &[CellGridRequest],
        sources: &[SourceReceipt],
        limits: ModelSetLimits,
    ) -> Result<ModelSetUsage> {
        let limits = limits.validate()?;
        if requests.is_empty() || requests.len() > limits.grids {
            return Err(failure("explicit grid count bound"));
        }
        let mut usage = ModelSetUsage {
            grids: requests.len(),
            ..Default::default()
        };
        add(
            &mut usage.metadata_bytes,
            4096 + 4 * world.origin_plugin.len(),
            limits.metadata_bytes,
            "metadata",
        )?;
        for request in requests {
            // Includes selection copies, plan vector, identity sealing and source
            // validation scratch before calling the existing receipt helper.
            add(
                &mut usage.metadata_bytes,
                1024 + 8 * request.world().origin_plugin.len()
                    + 8 * request.cell().origin_plugin.len(),
                limits.metadata_bytes,
                "request metadata",
            )?;
        }
        for source in sources {
            add(
                &mut usage.metadata_bytes,
                512 + 2 * source.source_name.len(),
                limits.metadata_bytes,
                "source validation metadata",
            )?;
        }
        if sources
            .len()
            .checked_mul(requests.len())
            .is_none_or(|n| n > limits.sources)
        {
            return Err(failure("source count bound"));
        }
        Ok(usage)
    }

    pub(in crate::world) fn load(
        store: &mut RecordStore,
        world: &FormKey,
        cohort: &str,
        requests: &[CellGridRequest],
        mounts: &MountIndex,
        limits: ModelSetLimits,
        mut usage: ModelSetUsage,
    ) -> Result<Self> {
        let mut pool = ArchivePool::new(limits.mapped_bytes, limits.archives);
        let mut plans = Vec::with_capacity(requests.len());
        for request in requests {
            let mut cell = limits.cell;
            let graph = &mut cell.dependencies;
            graph.max_sources = graph.max_sources.min(limits.sources - usage.sources);
            graph.max_winners_scanned = graph
                .max_winners_scanned
                .min(limits.winners_scanned - usage.winners_scanned);
            graph.max_nodes = graph.max_nodes.min(limits.nodes - usage.nodes);
            graph.max_edges = graph.max_edges.min(limits.edges - usage.edges);
            cell.max_bases = cell.max_bases.min(limits.bases - usage.bases);
            cell.max_candidates = cell
                .max_candidates
                .min(limits.candidates - usage.candidates);
            cell.max_requests = cell.max_requests.min(limits.models - usage.models);
            cell.max_field_sites = cell
                .max_field_sites
                .min(limits.field_sites - usage.field_sites);
            cell.max_record_decoded_bytes = cell
                .max_record_decoded_bytes
                .min(limits.decoded_bytes - usage.decoded_bytes);
            cell.max_model_decoded_bytes = cell
                .max_model_decoded_bytes
                .min(limits.model_bytes - usage.model_bytes);
            cell.max_probe_metadata_bytes = cell
                .max_probe_metadata_bytes
                .min(limits.probe_metadata_bytes - usage.probe_metadata_bytes);
            cell.max_metadata_bytes = cell
                .max_metadata_bytes
                .min(limits.metadata_bytes - usage.metadata_bytes);
            let plan = CellModelPlan::load_with_archive_pool(
                store,
                request.cell(),
                mounts,
                cell,
                limits.read_bytes - usage.read_bytes,
                &mut pool,
            )?;
            if plan.receipt().source_cohort_sha256 != cohort {
                return Err(failure("plan source cohort differs"));
            }
            let graph = &plan.graph().usage;
            let used = &plan.receipt().usage;
            for (target, amount, maximum, name) in [
                (&mut usage.sources, graph.sources, limits.sources, "sources"),
                (
                    &mut usage.winners_scanned,
                    graph.winners_scanned,
                    limits.winners_scanned,
                    "winning scans",
                ),
                (&mut usage.nodes, graph.nodes, limits.nodes, "nodes"),
                (&mut usage.edges, graph.edges, limits.edges, "edges"),
                (&mut usage.bases, used.bases, limits.bases, "bases"),
                (
                    &mut usage.candidates,
                    used.candidates,
                    limits.candidates,
                    "candidates",
                ),
                (
                    &mut usage.models,
                    plan.receipt().requests.len(),
                    limits.models,
                    "models",
                ),
                (
                    &mut usage.field_sites,
                    used.field_sites,
                    limits.field_sites,
                    "field sites",
                ),
                (
                    &mut usage.read_bytes,
                    used.source_read_bytes,
                    limits.read_bytes,
                    "read bytes",
                ),
                (
                    &mut usage.decoded_bytes,
                    used.record_decoded_bytes,
                    limits.decoded_bytes,
                    "decoded bytes",
                ),
                (
                    &mut usage.model_bytes,
                    used.model_decoded_bytes,
                    limits.model_bytes,
                    "model bytes",
                ),
                (
                    &mut usage.probe_metadata_bytes,
                    used.probe_metadata_bytes,
                    limits.probe_metadata_bytes,
                    "probe metadata",
                ),
                (
                    &mut usage.metadata_bytes,
                    used.metadata_bytes,
                    limits.metadata_bytes,
                    "metadata",
                ),
            ] {
                add(target, amount, maximum, name)?;
            }
            // The factory was constrained by every aggregate remainder before
            // reading, collecting or mapping; only a fully admitted plan moves in.
            plans.push(plan);
        }
        usage.archives = pool.len();
        usage.mapped_bytes = pool.mapped_bytes;
        let mut writer = HashWriter(Sha256::new());
        writer.0.update(b"nv-cell-model-set-v1\0");
        serde_json::to_writer(
            &mut writer,
            &(world, cohort, requests, PlanIdentities(&plans)),
        )
        .map_err(|error| failure(&error.to_string()))?;
        Ok(Self {
            world: world.clone(),
            cohort: cohort.to_owned(),
            identity: format!("{:x}", writer.0.finalize()),
            requests: requests.to_vec(),
            plans,
            usage,
            limits,
        })
    }
}
struct PlanIdentities<'a>(&'a [CellModelPlan]);
impl Serialize for PlanIdentities<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for plan in self.0 {
            sequence.serialize_element(plan.identity())?;
        }
        sequence.end()
    }
}
struct PlanReceipts<'a>(&'a [CellModelPlan]);
impl Serialize for PlanReceipts<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for plan in self.0 {
            sequence.serialize_element(plan.receipt())?;
        }
        sequence.end()
    }
}
impl Serialize for CellModelPlanSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            schema_version: u32,
            world: &'a FormKey,
            source_cohort_sha256: &'a str,
            identity: &'a str,
            requests: &'a [CellGridRequest],
            plans: PlanReceipts<'a>,
            usage: &'a ModelSetUsage,
            limits: &'a ModelSetLimits,
            runtime_ready: bool,
        }
        View {
            schema_version: 1,
            world: &self.world,
            source_cohort_sha256: &self.cohort,
            identity: &self.identity,
            requests: &self.requests,
            plans: PlanReceipts(&self.plans),
            usage: &self.usage,
            limits: &self.limits,
            runtime_ready: false,
        }
        .serialize(serializer)
    }
}
struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn failure(message: &str) -> Error {
    Error::Resolution(format!("CELL model set: {message}"))
}
fn add(used: &mut usize, bytes: usize, maximum: usize, name: &str) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|value| *value <= maximum)
        .ok_or_else(|| failure(&format!("{name} bound exceeded")))?;
    Ok(())
}
