//! Explicit LAND texture source leases in the existing CELL epoch/job pool.
use super::*;
use crate::terrain::{
    TerrainReport,
    preparation::{PlanReceipt as TerrainReceipt, TextureSourcePlan},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TerrainState {
    Unrequested,
    IoPending,
    Decoded,
    Unsupported,
}

// The caller's sealed source plan is admitted into this owner's logical quota.
// A job or consumer can keep it alive after unload, keeping its mappings charged.
struct TerrainPin {
    usage: Arc<Mutex<PlanUsage>>,
    metadata: usize,
    mapped: u64,
}
impl Drop for TerrainPin {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.usage.lock() {
            usage.metadata -= self.metadata;
            usage.mapped -= self.mapped;
        }
    }
}
struct Scope {
    plan: TextureSourcePlan,
    models: Arc<ResidentSources>,
    _pin: TerrainPin,
}

/// Borrowed provenance and raw texture bytes; no cloneable archive-bearing plan
/// can escape this lease. Terrain geometry, GPU materials and physics admission
/// remain separate consumer work.
///
/// ```compile_fail
/// use fallout_data::{terrain::preparation::TextureSourcePlan, world::residency::ResidentTerrain};
/// fn detach(sources: &ResidentTerrain) -> TextureSourcePlan {
///     sources.receipt().unwrap().clone()
/// }
/// ```
pub struct ResidentTerrain {
    scope: Arc<Scope>,
    textures: Vec<Artifact>,
}
impl ResidentTerrain {
    pub fn ticket(&self) -> &Ticket {
        &self.scope.models.ticket
    }
    pub fn receipt(&self) -> JobResult<&TerrainReceipt> {
        self.ticket().check()?;
        Ok(self.scope.plan.receipt())
    }
    pub fn terrain(&self) -> JobResult<&TerrainReport> {
        self.ticket().check()?;
        Ok(self.scope.plan.terrain())
    }
    pub fn texture(&self, index: usize) -> JobResult<&[u8]> {
        self.ticket().check()?;
        self.textures
            .get(index)
            .map(Artifact::bytes)
            .ok_or_else(|| JobError::Invalid("resident terrain texture index outside batch".into()))
    }
}

pub(super) struct Batch {
    scope: Arc<Scope>,
    pending: Vec<(usize, JobHandle)>,
    staged: Vec<Option<Artifact>>,
    sources: Option<Arc<ResidentTerrain>>,
    next: usize,
}
impl Batch {
    fn new(scope: Arc<Scope>) -> Self {
        let staged = (0..scope.plan.receipt().requests.len())
            .map(|_| None)
            .collect();
        let mut batch = Self {
            scope,
            pending: Vec::new(),
            staged,
            sources: None,
            next: 0,
        };
        batch.finish();
        batch
    }
    fn finish(&mut self) {
        if self.next == self.staged.len() && self.pending.is_empty() && self.sources.is_none() {
            self.sources = Some(Arc::new(ResidentTerrain {
                scope: self.scope.clone(),
                textures: std::mem::take(&mut self.staged)
                    .into_iter()
                    .map(|value| value.expect("complete terrain texture batch"))
                    .collect(),
            }));
        }
    }
    pub fn pending(&self) -> bool {
        self.sources.is_none()
    }
    pub fn ready(&self) -> bool {
        let receipt = self.scope.plan.receipt();
        self.sources.is_some()
            && receipt.texture_sources.failures == 0
            && receipt.texture_sources.unapplied_default_layers == 0
            && receipt.terrain.link_failures == 0
    }
    pub fn requested(&self) -> usize {
        self.scope.plan.receipt().requests.len()
    }
    pub fn bytes(&self) -> usize {
        self.scope.plan.receipt().usage.texture_bytes
    }
    pub fn identity(&self) -> &str {
        self.scope.plan.identity()
    }
    pub fn completed(&self) -> usize {
        self.sources.as_ref().map_or_else(
            || self.staged.iter().flatten().count(),
            |sources| sources.textures.len(),
        )
    }
    pub fn state(&self) -> TerrainState {
        if self.pending() {
            TerrainState::IoPending
        } else if self.ready() {
            TerrainState::Decoded
        } else {
            TerrainState::Unsupported
        }
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        for (_, handle) in &self.pending {
            handle.cancel();
        }
    }
}

impl CellResidency {
    /// Explicitly add terrain source work before dependency admission. A CELL
    /// request alone never infers terrain scope or a current-cell transition.
    /// Source planning/fingerprinting occurs on the caller's source worker.
    pub fn request_terrain(&mut self, ticket: &Ticket, plan: TextureSourcePlan) -> JobResult<()> {
        self.validate(ticket)?;
        if self.terrain.is_some() || self.dependencies_admitted || self.render_published {
            return Err(JobError::Invalid(
                "terrain admission already used or dependencies admitted".into(),
            ));
        }
        let models = self.sources.as_ref().expect("decoded model lease");
        let graph = models.plan.graph();
        let receipt = plan.receipt();
        if plan.root() != ticket.root()
            || receipt.source_cohort_sha256 != graph.source_cohort_sha256
            || receipt.sources.len() != graph.sources.len()
            || receipt.sources.iter().zip(&graph.sources).any(|(a, b)| {
                a.source_name != b.source_name
                    || a.source_bytes != b.source_bytes
                    || a.source_sha256 != b.source_sha256
            })
        {
            return Err(JobError::Invalid(
                "terrain plan CELL/source cohort differs from cell lease".into(),
            ));
        }
        self.validate_payloads(receipt.requests.len(), receipt.usage.texture_bytes)?;
        let metadata = receipt.usage.metadata_bytes;
        let mapped = receipt.archives.iter().try_fold(0u64, |sum, archive| {
            sum.checked_add(archive.source_bytes)
                .ok_or(JobError::ByteBudget)
        })?;
        ticket.token.commit(|| {
            let mut usage = self.plan_usage.lock().map_err(|_| JobError::Closed)?;
            if metadata
                > self
                    .limits
                    .plan_metadata_bytes
                    .saturating_sub(usage.metadata)
                || mapped > self.limits.mapped_source_bytes.saturating_sub(usage.mapped)
            {
                return Err(JobError::ByteBudget);
            }
            usage.metadata += metadata;
            usage.mapped += mapped;
            Ok(())
        })?;
        let scope = Arc::new(Scope {
            plan,
            models: models.clone(),
            _pin: TerrainPin {
                usage: self.plan_usage.clone(),
                metadata,
                mapped,
            },
        });
        self.terrain = Some(Batch::new(scope));
        self.dependencies = Readiness::Pending;
        self.stage = Stage::Decoded;
        Ok(())
    }
    pub fn terrain_sources(&self, ticket: &Ticket) -> JobResult<Arc<ResidentTerrain>> {
        self.validate(ticket)?;
        self.terrain
            .as_ref()
            .and_then(|batch| batch.sources.as_ref())
            .cloned()
            .ok_or_else(|| JobError::Invalid("terrain sources are not decoded".into()))
    }
    pub(super) fn poll_terrain(&mut self) -> JobResult<()> {
        self.ticket
            .as_ref()
            .expect("active terrain ticket")
            .check()?;
        let batch = self.terrain.as_mut().expect("pending terrain batch");
        let mut index = 0;
        let mut inspected = 0;
        while index < batch.pending.len() && inspected < POLL_WORK {
            inspected += 1;
            if let Some(artifact) = batch.pending[index].1.try_take()? {
                let (request, _) = batch.pending.swap_remove(index);
                batch.staged[request] = Some(artifact);
            } else {
                index += 1;
            }
        }
        self.backpressured = false;
        for _ in 0..POLL_WORK {
            if batch.next == batch.staged.len() {
                break;
            }
            let member = batch.scope.plan.member(batch.next)?;
            match self.jobs.submit_scoped(
                member,
                self.generation.token()?,
                self.cache.clone(),
                batch.scope.clone(),
                #[cfg(test)]
                self.pause.clone(),
            ) {
                Ok(handle) => {
                    batch.pending.push((batch.next, handle));
                    batch.next += 1;
                }
                Err(JobError::QueueFull | JobError::ByteBudget) => {
                    self.backpressured = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        batch.finish();
        Ok(())
    }
}
