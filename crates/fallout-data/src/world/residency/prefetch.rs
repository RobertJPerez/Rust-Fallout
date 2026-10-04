//! Exact door destination consumption through existing model and texture hosts.
use super::*;
use crate::{
    store::RecordStore,
    vfs::MountIndex,
    world::{
        doors::{DoorDestination, Metadata as DoorMetadata},
        preparation::Limits as ModelLimits,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Clone, Copy)]
pub struct DoorPrefetchLimits {
    pub residency: Limits,
    pub retained_requests: usize,
    /// Conservative sum of retained door graph metadata and decoded source bodies.
    pub retained_source_bytes: usize,
}

#[cfg(test)]
mod tests;
impl Default for DoorPrefetchLimits {
    fn default() -> Self {
        Self {
            residency: Limits::default(),
            retained_requests: 2,
            retained_source_bytes: 128 * 1024 * 1024,
        }
    }
}
#[derive(Default)]
struct ScopeUsage {
    requests: usize,
    bytes: usize,
}
struct ScopePin {
    usage: Arc<Mutex<ScopeUsage>>,
    bytes: usize,
}
impl Drop for ScopePin {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.usage.lock() {
            usage.requests -= 1;
            usage.bytes -= self.bytes;
        }
    }
}
struct Scope {
    destination: DoorDestination,
    identity: String,
    ticket: Ticket,
    _pin: Arc<ScopePin>,
}
/// One cached immutable lease; cloned Arcs retain original graph/payload pins.
pub struct DoorPrefetchSources {
    scope: Arc<Scope>,
    models: Arc<ResidentSources>,
    textures: Arc<ResidentTextures>,
}
impl DoorPrefetchSources {
    pub fn ticket(&self) -> &Ticket {
        &self.scope.ticket
    }
    pub fn identity(&self) -> &str {
        &self.scope.identity
    }
    pub fn destination(&self) -> JobResult<&DoorMetadata> {
        self.ticket().check()?;
        Ok(self.scope.destination.metadata())
    }
    pub fn models(&self) -> JobResult<&ResidentSources> {
        self.ticket().check()?;
        Ok(&self.models)
    }
    pub fn texture_receipt(&self) -> JobResult<&TextureReceipt> {
        self.ticket().check()?;
        self.textures.receipt()
    }
    pub fn texture(&self, index: usize) -> JobResult<&[u8]> {
        self.ticket().check()?;
        self.textures.texture(index)
    }
}
#[derive(Serialize)]
pub struct DoorPrefetchSnapshot {
    pub residency: Snapshot,
    pub identity: Option<String>,
    pub source_door: Option<FormKey>,
    pub destination_cell: Option<FormKey>,
    pub retained_requests: usize,
    pub retained_source_bytes: usize,
    pub source_lease_available: bool,
    pub source_error: Option<String>,
    pub current_cell_changed: bool,
    pub authored_pose_applied: bool,
    pub runtime_ready: bool,
}
pub struct DoorPrefetcher {
    owner: CellResidency,
    limits: DoorPrefetchLimits,
    usage: Arc<Mutex<ScopeUsage>>,
    scope: Option<Arc<Scope>>,
    sources: Option<Arc<DoorPrefetchSources>>,
    failure: Option<String>,
}
struct IdentityWriter {
    digest: Sha256,
    written: usize,
}
impl Write for IdentityWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.written = self
            .written
            .checked_add(bytes.len())
            .filter(|n| *n <= 512 * 1024 * 1024)
            .ok_or_else(|| std::io::Error::other("door prefetch identity ceiling exceeded"))?;
        self.digest.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl DoorPrefetcher {
    pub fn new(
        source_tree: &Path,
        cache_root: Option<&Path>,
        limits: DoorPrefetchLimits,
    ) -> JobResult<Self> {
        let max = DoorPrefetchLimits::default();
        if limits.retained_requests == 0
            || limits.retained_requests > max.retained_requests
            || limits.retained_source_bytes == 0
            || limits.retained_source_bytes > max.retained_source_bytes
        {
            return Err(JobError::Invalid(
                "invalid door prefetch source limits".into(),
            ));
        }
        Ok(Self {
            owner: CellResidency::new(source_tree, cache_root, limits.residency)?,
            limits,
            usage: Arc::new(Mutex::new(ScopeUsage::default())),
            scope: None,
            sources: None,
            failure: None,
        })
    }
    fn reserve(&self, destination: &DoorDestination) -> JobResult<Arc<ScopePin>> {
        let graph = destination.graph();
        let bytes = graph
            .usage
            .metadata_bytes
            .checked_add(graph.usage.decoded_bytes)
            .and_then(|n| n.checked_add(4096))
            .ok_or(JobError::ByteBudget)?;
        let mut usage = self.usage.lock().map_err(|_| JobError::Closed)?;
        if usage.requests >= self.limits.retained_requests {
            return Err(JobError::QueueFull);
        }
        if bytes
            > self
                .limits
                .retained_source_bytes
                .saturating_sub(usage.bytes)
        {
            return Err(JobError::ByteBudget);
        }
        usage.requests += 1;
        usage.bytes += bytes;
        Ok(Arc::new(ScopePin {
            usage: self.usage.clone(),
            bytes,
        }))
    }
    /// Factory and retention refusal preserve an already live prefetch.
    pub fn request(
        &mut self,
        store: &mut RecordStore,
        destination: DoorDestination,
        mounts: &MountIndex,
        model_limits: ModelLimits,
    ) -> JobResult<Ticket> {
        let metadata = destination.metadata();
        if !metadata.source_destination_resolved
            || metadata.destination.is_none()
            || metadata.source_cycle_component.is_some()
            || metadata.destination_cycle_component.is_some()
            || destination.graph().integrity_failures != 0
        {
            return Err(JobError::Invalid(
                "door destination unresolved, cyclic or tainted".into(),
            ));
        }
        let pin = self.reserve(&destination)?;
        // Reuses exact cohort validation, destination persistent parent and protected model factory.
        let plan = destination.prepare_cell(store, mounts, model_limits)?;
        let mut writer = IdentityWriter {
            digest: Sha256::new(),
            written: 0,
        };
        writer.digest.update(b"nv-door-source-prefetch-v1\0");
        serde_json::to_writer(&mut writer, &(metadata, plan.identity()))
            .map_err(|error| JobError::Invalid(error.to_string()))?;
        let identity = format!("{:x}", writer.digest.finalize());
        let ticket = self.owner.request(plan)?;
        self.sources = None;
        self.scope = Some(Arc::new(Scope {
            destination,
            identity,
            ticket: ticket.clone(),
            _pin: pin,
        }));
        self.failure = None;
        Ok(ticket)
    }
    fn validate(&self, ticket: &Ticket) -> JobResult<&Arc<Scope>> {
        if !self.owner.generation.owns(&ticket.token) {
            return Err(JobError::Invalid("foreign door prefetch owner".into()));
        }
        ticket.check()?;
        let scope = self.scope.as_ref().ok_or(JobError::Stale)?;
        if scope.ticket.root != ticket.root
            || scope.ticket.identity() != ticket.identity()
            || scope.ticket.generation() != ticket.generation()
        {
            return Err(JobError::Stale);
        }
        Ok(scope)
    }
    /// Explicit bounded source planning may block; IO completion remains in poll().
    /// The future host should call this on its existing source preparation worker.
    pub fn prepare_textures(
        &mut self,
        ticket: &Ticket,
        mounts: &MountIndex,
        limits: TextureLimits,
    ) -> JobResult<()> {
        self.validate(ticket)?;
        if self.failure.is_some() || self.owner.textures.is_some() {
            return Err(JobError::Invalid(
                "prefetch texture admission unavailable or already used".into(),
            ));
        }
        let prepared = (|| {
            let models = self.owner.sources(ticket)?;
            if !self.owner.snapshot().complete_model_coverage {
                return Err(JobError::Invalid(
                    "destination model source coverage incomplete".into(),
                ));
            }
            let plan = TexturePlan::load(models, mounts, limits)?;
            if plan.receipt().missing_or_ambiguous != 0
                || !plan.receipt().reference_coverage_verified
            {
                return Err(JobError::Invalid(
                    "destination texture source coverage unavailable".into(),
                ));
            }
            self.owner.request_textures(ticket, plan)?;
            self.finish()
        })();
        if let Err(error) = &prepared {
            self.failure = Some(error.to_string().chars().take(4096).collect());
            self.sources = None;
        }
        prepared
    }
    fn finish(&mut self) -> JobResult<()> {
        if self.sources.is_some() || self.failure.is_some() {
            return Ok(());
        }
        let Some(scope) = &self.scope else {
            return Ok(());
        };
        let state = self.owner.snapshot();
        if state.texture_state != TextureState::Decoded {
            return Ok(());
        }
        if !state.complete_model_coverage
            || !state.complete_texture_coverage
            || !state.texture_reference_coverage_verified
        {
            return Err(JobError::Invalid(
                "door prefetch source coverage incomplete".into(),
            ));
        }
        scope.ticket.check()?;
        let models = self.owner.sources(&scope.ticket)?;
        let textures = self.owner.texture_sources(&scope.ticket)?;
        self.sources = Some(Arc::new(DoorPrefetchSources {
            scope: scope.clone(),
            models,
            textures,
        }));
        Ok(())
    }
    pub fn poll(&mut self) -> JobResult<DoorPrefetchSnapshot> {
        let advanced = self.owner.poll().and_then(|_| self.finish());
        if let Err(error) = advanced {
            self.failure = Some(error.to_string().chars().take(4096).collect());
            self.sources = None;
            return Err(error);
        }
        Ok(self.snapshot())
    }
    pub fn sources(&self, ticket: &Ticket) -> JobResult<Arc<DoorPrefetchSources>> {
        self.validate(ticket)?;
        self.sources.as_ref().cloned().ok_or_else(|| {
            JobError::Invalid("door destination sources are not completely prefetched".into())
        })
    }
    pub fn cancel(&mut self) -> JobResult<()> {
        self.owner.unload()?;
        self.sources = None;
        self.scope = None;
        self.failure = None;
        Ok(())
    }
    pub fn snapshot(&self) -> DoorPrefetchSnapshot {
        let usage = self.usage.lock().expect("private door scope accounting");
        DoorPrefetchSnapshot {
            residency: self.owner.snapshot(),
            identity: self.scope.as_ref().map(|scope| scope.identity.clone()),
            source_door: self
                .scope
                .as_ref()
                .map(|scope| scope.destination.metadata().source_door.clone()),
            destination_cell: self.scope.as_ref().and_then(|scope| {
                scope
                    .destination
                    .metadata()
                    .destination
                    .as_ref()
                    .map(|destination| destination.cell.clone())
            }),
            retained_requests: usage.requests,
            retained_source_bytes: usage.bytes,
            source_lease_available: self.sources.is_some(),
            source_error: self.failure.clone(),
            current_cell_changed: false,
            authored_pose_applied: false,
            runtime_ready: false,
        }
    }
}
