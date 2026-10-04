//! One bounded cell residency owner over sealed source plans and ResourceJobs.
//! Source bytes, GPU resources, collision and persistent existence have separate
//! owners. A decoded BSA member never implies simulation or render readiness.
mod terrain;
mod textures;
use super::{
    dependencies,
    preparation::{CellModelPlan, PlanReceipt},
};
use crate::{
    cache,
    identity::FormKey,
    resource_jobs::{
        self, Artifact, Generation, JobError, JobHandle, JobResult, JobToken, ResourceJobs,
    },
};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
pub use terrain::{ResidentTerrain, TerrainState};
pub use textures::{ResidentTextures, TextureLimits, TexturePlan, TextureReceipt, TextureState};

const POLL_WORK: usize = 8;
const EMPTY_IDENTITY: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Copy)]
pub struct Limits {
    pub workers: usize,
    pub models: usize,
    /// Combined queued/running/retained model and texture payload reservations,
    /// including explicitly requested terrain textures.
    pub resources: usize,
    pub source_bytes: usize,
    pub retained_plans: usize,
    pub plan_metadata_bytes: usize,
    pub mapped_source_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            workers: 2,
            models: 1024,
            resources: 1024,
            source_bytes: 256 * 1024 * 1024,
            retained_plans: 2,
            plan_metadata_bytes: 32 * 1024 * 1024,
            mapped_source_bytes: 16 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Default)]
struct PlanUsage {
    plans: usize,
    metadata: usize,
    mapped: u64,
}
struct PlanPin {
    usage: Arc<Mutex<PlanUsage>>,
    metadata: usize,
    mapped: u64,
}
impl Drop for PlanPin {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.usage.lock() {
            usage.plans -= 1;
            usage.metadata -= self.metadata;
            usage.mapped -= self.mapped;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Stage {
    Unrequested,
    IoPending,
    Decoded,
    DependenciesReady,
    RenderResident,
    Unloading,
    Failed,
}

/// An explicit consumer report, never a default assumption about source data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Readiness {
    Pending,
    Ready,
    Unsupported,
}

/// Unforgeable owner/epoch binding passed to render and collision consumers.
#[derive(Clone)]
pub struct Ticket {
    root: FormKey,
    token: JobToken,
}
impl Ticket {
    pub fn root(&self) -> &FormKey {
        &self.root
    }
    pub fn identity(&self) -> &str {
        self.token.source_identity()
    }
    pub fn generation(&self) -> u64 {
        self.token.generation()
    }
    pub fn check(&self) -> JobResult<()> {
        self.token.check()
    }
}

/// Immutable source payloads remain attached to their existing byte/queue pins.
/// Consumers decode outside the admission lock and recheck their ticket before
/// publishing. Keeping an old Arc across unload keeps its bytes charged, but
/// cannot make that old cell active or publish into a newer request.
pub struct ResidentSources {
    plan: CellModelPlan,
    ticket: Ticket,
    models: Vec<Artifact>,
    _plan_pin: Arc<PlanPin>,
    limits: Limits,
}
/// A source lease may expose borrowed provenance and placement data, but never
/// a cloneable resource-bearing plan. This view cannot outlive ResidentSources
/// or detach its archive mappings from the owner's retained-plan accounting.
///
/// ```compile_fail
/// use fallout_data::world::{preparation::CellModelPlan, residency::ResidentSources};
/// fn detach(sources: &ResidentSources) -> CellModelPlan {
///     sources.plan().unwrap().clone()
/// }
/// ```
pub struct ResidentPlan<'a> {
    plan: &'a CellModelPlan,
}
impl<'a> ResidentPlan<'a> {
    pub fn graph(&self) -> &'a dependencies::Report {
        self.plan.graph()
    }
    pub fn receipt(&self) -> &'a PlanReceipt {
        self.plan.receipt()
    }
    pub fn root(&self) -> &'a FormKey {
        self.plan.root()
    }
    pub fn identity(&self) -> &'a str {
        self.plan.identity()
    }
}
impl ResidentSources {
    pub fn ticket(&self) -> &Ticket {
        &self.ticket
    }
    pub fn plan(&self) -> JobResult<ResidentPlan<'_>> {
        self.ticket.check()?;
        Ok(ResidentPlan { plan: &self.plan })
    }
    pub fn model(&self, index: usize) -> JobResult<&[u8]> {
        self.ticket.check()?;
        self.models
            .get(index)
            .map(Artifact::bytes)
            .ok_or_else(|| JobError::Invalid("resident model index out of range".into()))
    }
}

#[derive(Serialize)]
pub struct Snapshot {
    pub root: Option<FormKey>,
    pub identity: Option<String>,
    pub generation: u64,
    pub stage: Stage,
    pub completed_models: usize,
    pub completed_textures: usize,
    pub requested_textures: usize,
    pub texture_state: TextureState,
    pub texture_identity: Option<String>,
    pub complete_texture_coverage: bool,
    pub texture_reference_coverage_verified: bool,
    pub completed_terrain_textures: usize,
    pub requested_terrain_textures: usize,
    pub terrain_state: TerrainState,
    pub terrain_identity: Option<String>,
    pub complete_terrain_coverage: bool,
    pub requested_models: usize,
    pub outstanding: usize,
    pub pinned_source_bytes: usize,
    pub retained_plans: usize,
    pub plan_metadata_bytes: usize,
    pub mapped_source_bytes: u64,
    pub backpressured: bool,
    pub complete_model_coverage: bool,
    pub dependencies: Readiness,
    pub collision: Readiness,
    pub behavior: Readiness,
    pub simulation_ready: bool,
    pub render_published: bool,
    pub failure: Option<String>,
}

pub struct CellResidency {
    generation: Generation,
    jobs: ResourceJobs,
    limits: Limits,
    plan_usage: Arc<Mutex<PlanUsage>>,
    plan_pin: Option<Arc<PlanPin>>,
    cache: Option<(PathBuf, PathBuf)>,
    plan: Option<CellModelPlan>,
    ticket: Option<Ticket>,
    pending: Vec<(usize, JobHandle)>,
    staged: Vec<Option<Artifact>>,
    sources: Option<Arc<ResidentSources>>,
    next: usize,
    stage: Stage,
    dependencies: Readiness,
    collision: Readiness,
    behavior: Readiness,
    failure: Option<String>,
    backpressured: bool,
    render_published: bool,
    dependencies_admitted: bool,
    textures: Option<textures::Batch>,
    terrain: Option<terrain::Batch>,
    terrain_turn: bool,
    #[cfg(test)]
    pause: Option<Arc<resource_jobs::tests::Pause>>,
}
impl CellResidency {
    pub fn new(source_tree: &Path, cache_root: Option<&Path>, limits: Limits) -> JobResult<Self> {
        let ceiling = Limits::default();
        if limits.workers == 0
            || limits.workers > ceiling.workers
            || limits.models == 0
            || limits.models > ceiling.models
            || limits.resources == 0
            || limits.resources > ceiling.resources
            || limits.models > limits.resources
            || limits.source_bytes == 0
            || limits.source_bytes > ceiling.source_bytes
            || limits.retained_plans == 0
            || limits.retained_plans > ceiling.retained_plans
            || limits.plan_metadata_bytes == 0
            || limits.plan_metadata_bytes > ceiling.plan_metadata_bytes
            || limits.mapped_source_bytes == 0
            || limits.mapped_source_bytes > ceiling.mapped_source_bytes
        {
            return Err(JobError::Invalid("invalid cell residency limits".into()));
        }
        let cache = cache_root
            .map(|root| {
                cache::validate_root(root, source_tree)
                    .map(|root| (root, source_tree.to_path_buf()))
            })
            .transpose()?;
        let generation = Generation::new(EMPTY_IDENTITY.into())?;
        // Outstanding includes retained outputs, so admit the whole bounded cell.
        // An eight-request queue would deadlock after retaining its first eight
        // models. Each poll still performs at most eight admissions/completions.
        let jobs = ResourceJobs::new(
            resource_jobs::Limits {
                workers: limits.workers,
                outstanding: limits.resources,
                decoded_bytes: limits.source_bytes,
            },
            generation.clone(),
        )?;
        Ok(Self {
            generation,
            jobs,
            limits,
            plan_usage: Arc::new(Mutex::new(PlanUsage::default())),
            plan_pin: None,
            cache,
            plan: None,
            ticket: None,
            pending: Vec::new(),
            staged: Vec::new(),
            sources: None,
            next: 0,
            stage: Stage::Unrequested,
            dependencies: Readiness::Pending,
            collision: Readiness::Pending,
            behavior: Readiness::Pending,
            failure: None,
            backpressured: false,
            render_published: false,
            dependencies_admitted: false,
            textures: None,
            terrain: None,
            terrain_turn: false,
            #[cfg(test)]
            pause: None,
        })
    }

    pub fn request(&mut self, plan: CellModelPlan) -> JobResult<Ticket> {
        if plan.receipt().requests.len() > self.limits.models {
            return Err(JobError::QueueFull);
        }
        if plan.receipt().usage.model_decoded_bytes > self.limits.source_bytes {
            return Err(JobError::ByteBudget);
        }
        let pin = self.reserve_plan(&plan)?;
        // Validate before revoking a working cell. Old consumer pins remain in
        // this same pool's accounting across replacement, unload and retry.
        self.generation.advance(plan.identity().to_owned())?;
        self.clear_work();
        let ticket = Ticket {
            root: plan.root().clone(),
            token: self.generation.token()?,
        };
        self.staged = (0..plan.receipt().requests.len()).map(|_| None).collect();
        self.plan = Some(plan);
        self.plan_pin = Some(pin);
        self.ticket = Some(ticket.clone());
        self.next = 0;
        self.stage = Stage::IoPending;
        self.dependencies = Readiness::Pending;
        self.collision = Readiness::Pending;
        self.behavior = Readiness::Pending;
        self.failure = None;
        self.backpressured = false;
        Ok(ticket)
    }

    fn reserve_plan(&self, plan: &CellModelPlan) -> JobResult<Arc<PlanPin>> {
        let metadata = plan.receipt().usage.metadata_bytes;
        let mapped = plan
            .receipt()
            .archives
            .iter()
            .try_fold(0u64, |sum, archive| {
                sum.checked_add(archive.source_bytes)
                    .ok_or(JobError::ByteBudget)
            })?;
        let mut usage = self.plan_usage.lock().map_err(|_| JobError::Closed)?;
        if usage.plans >= self.limits.retained_plans {
            return Err(JobError::QueueFull);
        }
        if metadata
            > self
                .limits
                .plan_metadata_bytes
                .saturating_sub(usage.metadata)
            || mapped > self.limits.mapped_source_bytes.saturating_sub(usage.mapped)
        {
            return Err(JobError::ByteBudget);
        }
        usage.plans += 1;
        usage.metadata += metadata;
        usage.mapped += mapped;
        Ok(Arc::new(PlanPin {
            usage: self.plan_usage.clone(),
            metadata,
            mapped,
        }))
    }

    pub fn retry(&mut self) -> JobResult<Ticket> {
        let plan = self
            .plan
            .as_ref()
            .ok_or_else(|| JobError::Invalid("no cell to retry".into()))?
            .clone();
        self.request(plan)
    }

    /// Nonblocking IO polling; existing extraction runs on bounded workers.
    /// This stage means archive decoding completed, not NIF/physics/GPU readiness.
    pub fn poll(&mut self) -> JobResult<Snapshot> {
        if self.stage == Stage::Unloading {
            let usage = self.plan_usage.lock().map_err(|_| JobError::Closed)?;
            if self.jobs.usage().outstanding == 0
                && usage.plans == 0
                && usage.metadata == 0
                && usage.mapped == 0
            {
                self.stage = Stage::Unrequested;
            }
        } else if self.stage == Stage::IoPending
            || self.textures.as_ref().is_some_and(textures::Batch::pending)
            || self.terrain.as_ref().is_some_and(terrain::Batch::pending)
        {
            let result = if self.stage == Stage::IoPending {
                self.poll_io()
            } else {
                // One bounded batch per call preserves the existing eight
                // admissions/completions limit and prevents either texture
                // consumer from starving while they share one job pool.
                let terrain_pending = self.terrain.as_ref().is_some_and(terrain::Batch::pending);
                let textures_pending = self.textures.as_ref().is_some_and(textures::Batch::pending);
                if terrain_pending && (self.terrain_turn || !textures_pending) {
                    self.terrain_turn = false;
                    self.poll_terrain()
                } else {
                    self.terrain_turn = true;
                    self.poll_textures()
                }
            };
            if let Err(error) = result {
                self.ticket
                    .as_ref()
                    .expect("active cell ticket")
                    .token
                    .cancel();
                self.clear_work();
                self.stage = Stage::Failed;
                self.failure = Some(error.to_string().chars().take(4096).collect());
                return Err(error);
            }
        }
        Ok(self.snapshot())
    }

    fn poll_io(&mut self) -> JobResult<()> {
        self.ticket.as_ref().expect("active cell ticket").check()?;
        let mut index = 0;
        let mut inspected = 0;
        while index < self.pending.len() && inspected < POLL_WORK {
            inspected += 1;
            if let Some(artifact) = self.pending[index].1.try_take()? {
                let (request, _) = self.pending.swap_remove(index);
                self.staged[request] = Some(artifact);
            } else {
                index += 1;
            }
        }
        self.backpressured = false;
        let plan = self.plan.as_ref().expect("active sealed plan");
        for _ in 0..POLL_WORK {
            if self.next == self.staged.len() {
                break;
            }
            let member = plan.member(self.next)?;
            let token = self.generation.token()?;
            let submitted = self.jobs.submit_scoped(
                member,
                token,
                self.cache.clone(),
                self.plan_pin.as_ref().expect("active plan pin").clone(),
                #[cfg(test)]
                self.pause.clone(),
            );
            match submitted {
                Ok(handle) => {
                    self.pending.push((self.next, handle));
                    self.next += 1;
                }
                Err(JobError::QueueFull | JobError::ByteBudget) => {
                    self.backpressured = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        if self.next == self.staged.len() && self.pending.is_empty() {
            self.sources = Some(Arc::new(ResidentSources {
                plan: plan.clone(),
                ticket: self.ticket.as_ref().expect("active ticket").clone(),
                models: std::mem::take(&mut self.staged)
                    .into_iter()
                    .map(|value| value.expect("all source completions admitted"))
                    .collect(),
                _plan_pin: self.plan_pin.as_ref().expect("active plan pin").clone(),
                limits: self.limits,
            }));
            self.stage = Stage::Decoded;
        }
        Ok(())
    }

    fn validate(&self, ticket: &Ticket) -> JobResult<()> {
        if !self.generation.owns(&ticket.token) {
            return Err(JobError::Invalid("foreign cell owner".into()));
        }
        ticket.check()?;
        let Some(current) = &self.ticket else {
            return Err(JobError::Stale);
        };
        if ticket.root != current.root
            || ticket.identity() != current.identity()
            || ticket.generation() != current.generation()
        {
            return Err(JobError::Stale);
        }
        if !matches!(
            self.stage,
            Stage::Decoded | Stage::DependenciesReady | Stage::RenderResident
        ) {
            return Err(JobError::Invalid("cell sources are not decoded".into()));
        }
        Ok(())
    }

    pub fn sources(&self, ticket: &Ticket) -> JobResult<Arc<ResidentSources>> {
        self.validate(ticket)?;
        Ok(self.sources.as_ref().expect("decoded cell sources").clone())
    }
    fn validate_payloads(&self, extra_requests: usize, extra_bytes: usize) -> JobResult<()> {
        let plan = self.plan.as_ref().expect("active model plan");
        let requests = plan
            .receipt()
            .requests
            .len()
            .checked_add(self.textures.as_ref().map_or(0, textures::Batch::requested))
            .and_then(|n| n.checked_add(self.terrain.as_ref().map_or(0, terrain::Batch::requested)))
            .and_then(|n| n.checked_add(extra_requests))
            .ok_or(JobError::QueueFull)?;
        if requests > self.limits.resources {
            return Err(JobError::QueueFull);
        }
        let bytes = plan
            .receipt()
            .usage
            .model_decoded_bytes
            .checked_add(
                self.textures
                    .as_ref()
                    .map_or(0, |b| b.plan.receipt().decoded_bytes),
            )
            .and_then(|n| n.checked_add(self.terrain.as_ref().map_or(0, terrain::Batch::bytes)))
            .and_then(|n| n.checked_add(extra_bytes))
            .ok_or(JobError::ByteBudget)?;
        if bytes > self.limits.source_bytes {
            return Err(JobError::ByteBudget);
        }
        Ok(())
    }
    pub fn report_dependencies(&mut self, ticket: &Ticket, readiness: Readiness) -> JobResult<()> {
        self.validate(ticket)?;
        if readiness == Readiness::Ready
            && !self.textures.as_ref().is_some_and(textures::Batch::ready)
        {
            return Err(JobError::Invalid(
                "cell texture plan/payloads are not ready".into(),
            ));
        }
        if readiness == Readiness::Ready
            && self.terrain.as_ref().is_some_and(|batch| !batch.ready())
        {
            return Err(JobError::Invalid(
                "requested terrain sources are not ready".into(),
            ));
        }
        self.dependencies = readiness;
        self.dependencies_admitted |= readiness == Readiness::Ready;
        self.stage = if readiness == Readiness::Ready {
            if self.render_published {
                Stage::RenderResident
            } else {
                Stage::DependenciesReady
            }
        } else {
            Stage::Decoded
        };
        Ok(())
    }
    pub fn report_collision(&mut self, ticket: &Ticket, readiness: Readiness) -> JobResult<()> {
        self.validate(ticket)?;
        self.collision = readiness;
        Ok(())
    }
    pub fn report_behavior(&mut self, ticket: &Ticket, readiness: Readiness) -> JobResult<()> {
        self.validate(ticket)?;
        self.behavior = readiness;
        Ok(())
    }

    /// Only final host admission runs under the cancellation gate. Decode and
    /// GPU staging happen outside it; keep this callback short and nonblocking.
    /// A failed callback leaves residency unchanged; the host must clean up its
    /// own partial GPU/entity work rather than treating this as a rollback API.
    /// A successful publication consumes the current epoch's admission, even
    /// if a later dependency report downgrades readiness. Retry uses a new epoch.
    pub fn publish_render<T>(
        &mut self,
        ticket: &Ticket,
        publish: impl FnOnce() -> JobResult<T>,
    ) -> JobResult<T> {
        self.validate(ticket)?;
        if self.render_published {
            return Err(JobError::Invalid(
                "cell render already published for this generation".into(),
            ));
        }
        if self.dependencies != Readiness::Ready {
            return Err(JobError::Invalid(
                "cell render dependencies are not ready".into(),
            ));
        }
        let value = ticket.token.commit(publish)?;
        self.render_published = true;
        self.stage = Stage::RenderResident;
        Ok(value)
    }

    pub fn unload(&mut self) -> JobResult<()> {
        self.generation.advance(EMPTY_IDENTITY.into())?;
        self.clear_work();
        self.plan = None;
        self.plan_pin = None;
        self.ticket = None;
        self.stage = Stage::Unloading;
        self.dependencies = Readiness::Pending;
        self.collision = Readiness::Pending;
        self.behavior = Readiness::Pending;
        self.failure = None;
        self.backpressured = false;
        Ok(())
    }
    fn clear_work(&mut self) {
        self.render_published = false;
        self.dependencies_admitted = false;
        self.textures = None;
        self.terrain = None;
        self.terrain_turn = false;
        for (_, handle) in self.pending.drain(..) {
            handle.cancel();
        }
        self.staged.clear();
        self.sources = None;
    }
    pub fn snapshot(&self) -> Snapshot {
        let usage = self.jobs.usage();
        let plan_usage = self.plan_usage.lock().expect("private plan accounting");
        let complete_model_coverage = self.plan.as_ref().is_some_and(|plan| {
            plan.receipt()
                .coverage
                .iter()
                .all(|model| model.asset_path.is_some() && model.candidates.len() == 1)
                && plan.graph().integrity_failures == 0
                && plan
                    .graph()
                    .edges
                    .iter()
                    .all(|edge| matches!(edge.target.status, "resolved" | "null"))
        });
        let decoded = matches!(
            self.stage,
            Stage::Decoded | Stage::DependenciesReady | Stage::RenderResident
        );
        Snapshot {
            root: self.ticket.as_ref().map(|ticket| ticket.root.clone()),
            identity: self
                .ticket
                .as_ref()
                .map(|ticket| ticket.identity().to_owned()),
            generation: self.ticket.as_ref().map_or(0, Ticket::generation),
            stage: self.stage,
            completed_models: self.sources.as_ref().map_or_else(
                || self.staged.iter().filter(|value| value.is_some()).count(),
                |sources| sources.models.len(),
            ),
            requested_models: self
                .plan
                .as_ref()
                .map_or(0, |plan| plan.receipt().requests.len()),
            completed_textures: self.textures.as_ref().map_or(0, textures::Batch::completed),
            requested_textures: self.textures.as_ref().map_or(0, textures::Batch::requested),
            texture_state: self
                .textures
                .as_ref()
                .map_or(TextureState::Unrequested, textures::Batch::state),
            texture_identity: self
                .textures
                .as_ref()
                .map(|batch| batch.plan.receipt().identity.clone()),
            complete_texture_coverage: self.textures.as_ref().is_some_and(textures::Batch::ready),
            texture_reference_coverage_verified: self
                .textures
                .as_ref()
                .is_some_and(|batch| batch.plan.receipt().reference_coverage_verified),
            completed_terrain_textures: self.terrain.as_ref().map_or(0, terrain::Batch::completed),
            requested_terrain_textures: self.terrain.as_ref().map_or(0, terrain::Batch::requested),
            terrain_state: self
                .terrain
                .as_ref()
                .map_or(TerrainState::Unrequested, terrain::Batch::state),
            terrain_identity: self
                .terrain
                .as_ref()
                .map(|batch| batch.identity().to_owned()),
            complete_terrain_coverage: self.terrain.as_ref().is_some_and(terrain::Batch::ready),
            outstanding: usage.outstanding,
            pinned_source_bytes: usage.decoded_bytes,
            retained_plans: plan_usage.plans,
            plan_metadata_bytes: plan_usage.metadata,
            mapped_source_bytes: plan_usage.mapped,
            backpressured: self.backpressured,
            complete_model_coverage,
            dependencies: self.dependencies,
            collision: self.collision,
            behavior: self.behavior,
            simulation_ready: decoded
                && complete_model_coverage
                && self.textures.as_ref().is_some_and(|batch| {
                    batch.ready() && batch.plan.receipt().reference_coverage_verified
                })
                && self.terrain.as_ref().is_none_or(terrain::Batch::ready)
                && self.dependencies == Readiness::Ready
                && self.collision == Readiness::Ready
                && self.behavior == Readiness::Ready,
            render_published: self.render_published,
            failure: self.failure.clone(),
        }
    }
}
impl Drop for CellResidency {
    fn drop(&mut self) {
        if let Some(ticket) = &self.ticket {
            ticket.token.cancel();
        }
        self.clear_work();
        // ResourceJobs closes the epoch, cancels obsolete work and joins only
        // this owner's bounded worker threads at the existing extraction boundary.
    }
}

#[cfg(test)]
mod tests;
