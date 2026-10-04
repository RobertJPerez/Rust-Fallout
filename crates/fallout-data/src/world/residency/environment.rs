//! Exact lighting/water source declarations retained beneath one CELL epoch.
#[cfg(test)]
mod tests;
use super::*;
use crate::{
    store::RecordStore,
    vfs::MountIndex,
    world::{
        lighting::{self, CellLightingSources},
        water::{self, CellWaterSources},
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct EnvironmentLimits {
    pub metadata_bytes: usize,
    pub mapped_bytes: u64,
    pub noise_bytes: usize,
}
impl Default for EnvironmentLimits {
    fn default() -> Self {
        Self {
            metadata_bytes: 32 * 1024 * 1024,
            mapped_bytes: 16 * 1024 * 1024 * 1024,
            noise_bytes: 64 * 1024 * 1024,
        }
    }
}
struct Pin {
    models: Arc<ResidentSources>,
    epoch: u64,
    metadata: usize,
    mapped: u64,
    limits: EnvironmentLimits,
}
impl Pin {
    fn new(models: Arc<ResidentSources>, limits: EnvironmentLimits) -> JobResult<Self> {
        models.ticket.check()?;
        let max = EnvironmentLimits::default();
        if limits.metadata_bytes == 0
            || limits.metadata_bytes > max.metadata_bytes
            || limits.mapped_bytes > max.mapped_bytes
            || limits.noise_bytes > max.noise_bytes
        {
            return Err(JobError::Invalid(
                "environment limits exceed source ceiling".into(),
            ));
        }
        let epoch = models.ticket.generation();
        {
            let mut usage = models
                ._plan_pin
                .usage
                .lock()
                .map_err(|_| JobError::Closed)?;
            if !usage.environment_epochs.insert(epoch) {
                return Err(JobError::Invalid(
                    "environment scope already retained for this CELL epoch".into(),
                ));
            }
        }
        let mut pin = Self {
            models,
            epoch,
            metadata: 0,
            mapped: 0,
            limits,
        };
        let base = 4096usize
            .checked_add(8 * pin.models.ticket.root().origin_plugin.len())
            .ok_or(JobError::ByteBudget)?;
        pin.charge(base, 0)?;
        Ok(pin)
    }
    fn remaining(&self) -> JobResult<(usize, u64)> {
        let usage = self
            .models
            ._plan_pin
            .usage
            .lock()
            .map_err(|_| JobError::Closed)?;
        Ok((
            self.limits
                .metadata_bytes
                .saturating_sub(self.metadata)
                .min(
                    self.models
                        .limits
                        .plan_metadata_bytes
                        .saturating_sub(usage.metadata),
                ),
            self.limits.mapped_bytes.saturating_sub(self.mapped).min(
                self.models
                    .limits
                    .mapped_source_bytes
                    .saturating_sub(usage.mapped),
            ),
        ))
    }
    fn charge(&mut self, metadata: usize, mapped: u64) -> JobResult<()> {
        self.models.ticket.check()?;
        let mut usage = self
            .models
            ._plan_pin
            .usage
            .lock()
            .map_err(|_| JobError::Closed)?;
        if metadata > self.limits.metadata_bytes.saturating_sub(self.metadata)
            || metadata
                > self
                    .models
                    .limits
                    .plan_metadata_bytes
                    .saturating_sub(usage.metadata)
            || mapped > self.limits.mapped_bytes.saturating_sub(self.mapped)
            || mapped
                > self
                    .models
                    .limits
                    .mapped_source_bytes
                    .saturating_sub(usage.mapped)
        {
            return Err(JobError::ByteBudget);
        }
        self.metadata += metadata;
        self.mapped += mapped;
        usage.metadata += metadata;
        usage.mapped += mapped;
        usage.environment_metadata += metadata;
        usage.environment_mapped += mapped;
        Ok(())
    }
}
impl Drop for Pin {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.models._plan_pin.usage.lock() {
            usage.metadata -= self.metadata;
            usage.mapped -= self.mapped;
            usage.environment_metadata -= self.metadata;
            usage.environment_mapped -= self.mapped;
            usage.environment_epochs.remove(&self.epoch);
        }
    }
}
struct Scope {
    lighting: CellLightingSources,
    water: CellWaterSources,
    identity: String,
    pin: Pin,
}
impl Scope {
    fn declarations_available(&self) -> bool {
        self.lighting
            .receipt()
            .template
            .as_ref()
            .is_none_or(|template| matches!(template.input_status, "resolved" | "null"))
            && self
                .water
                .receipt()
                .water_type
                .as_ref()
                .is_none_or(|water| matches!(water.target.status, "resolved" | "null"))
            && self
                .water
                .receipt()
                .noise
                .as_ref()
                .is_none_or(|noise| noise.request.is_some() || noise.status == "empty-declaration")
    }
}
/// Only the existing protected factories can create this scope. Clones share pins.
#[derive(Clone)]
pub struct EnvironmentPlan(Arc<Scope>);
impl EnvironmentPlan {
    pub fn load(
        models: Arc<ResidentSources>,
        store: &mut RecordStore,
        mounts: &MountIndex,
        limits: EnvironmentLimits,
        mut lighting_limits: lighting::Limits,
        mut water_limits: water::Limits,
    ) -> JobResult<Self> {
        lighting_limits = lighting_limits.validate()?;
        water_limits = water_limits.validate()?;
        let mut pin = Pin::new(models, limits)?;
        validate_store(&pin.models, store)?;
        lighting_limits.metadata_bytes = lighting_limits.metadata_bytes.min(pin.remaining()?.0);
        let lighting = CellLightingSources::load(store, pin.models.ticket.root(), lighting_limits)?;
        lighting.validate_sources(store)?;
        if lighting.receipt().source_cohort_sha256 != pin.models.plan.receipt().source_cohort_sha256
        {
            return Err(JobError::Invalid(
                "lighting cohort differs from resident CELL".into(),
            ));
        }
        pin.charge(lighting.receipt().usage.metadata_bytes, 0)?;
        let remaining = pin.remaining()?;
        water_limits.metadata_bytes = water_limits.metadata_bytes.min(remaining.0);
        water_limits.mapped_bytes = water_limits.mapped_bytes.min(remaining.1);
        water_limits.noise_bytes = water_limits.noise_bytes.min(limits.noise_bytes).min(
            pin.models
                .limits
                .source_bytes
                .saturating_sub(pin.models.plan.receipt().usage.model_decoded_bytes),
        );
        let water = CellWaterSources::load(store, pin.models.ticket.root(), mounts, water_limits)?;
        water.validate_sources(store)?;
        if water.receipt().source_cohort_sha256 != pin.models.plan.receipt().source_cohort_sha256 {
            return Err(JobError::Invalid(
                "water cohort differs from resident CELL".into(),
            ));
        }
        pin.charge(
            water.receipt().usage.metadata_bytes,
            water.receipt().usage.mapped_bytes,
        )?;
        pin.models.ticket.check()?;
        let mut hash = Sha256::new();
        hash.update(b"nv-resident-cell-environment-v1\0");
        hash.update(pin.models.ticket.identity().as_bytes());
        hash.update(lighting.identity().as_bytes());
        hash.update(water.identity().as_bytes());
        Ok(Self(Arc::new(Scope {
            lighting,
            water,
            identity: format!("{:x}", hash.finalize()),
            pin,
        })))
    }
    pub fn identity(&self) -> &str {
        &self.0.identity
    }
    pub fn ticket(&self) -> &Ticket {
        &self.0.pin.models.ticket
    }
}
fn validate_store(models: &ResidentSources, store: &mut RecordStore) -> JobResult<()> {
    models.ticket.check()?;
    let sources = &models.plan.graph().sources;
    if store.indices().len() != sources.len() {
        return Err(JobError::Invalid(
            "resident environment source count changed".into(),
        ));
    }
    let current = store.source_receipts()?;
    if current.iter().zip(sources).any(|(a, b)| {
        a.source_name != b.source_name
            || a.source_bytes != b.source_bytes
            || a.source_sha256 != b.source_sha256
    }) {
        return Err(JobError::Invalid(
            "resident environment ordered source cohort changed".into(),
        ));
    }
    Ok(())
}
pub struct ResidentEnvironment {
    scope: Arc<Scope>,
    noise: Option<Artifact>,
}
impl ResidentEnvironment {
    pub fn ticket(&self) -> &Ticket {
        &self.scope.pin.models.ticket
    }
    pub fn identity(&self) -> &str {
        &self.scope.identity
    }
    pub fn lighting(&self) -> JobResult<&lighting::Receipt> {
        self.ticket().check()?;
        Ok(self.scope.lighting.receipt())
    }
    pub fn water(&self) -> JobResult<&water::Receipt> {
        self.ticket().check()?;
        Ok(self.scope.water.receipt())
    }
    pub fn noise(&self) -> JobResult<Option<&[u8]>> {
        self.ticket().check()?;
        Ok(self.noise.as_ref().map(Artifact::bytes))
    }
    /// Whether every declared source is available; no defaults or runtime admission.
    pub fn source_declarations_available(&self) -> JobResult<bool> {
        self.ticket().check()?;
        Ok(self.scope.declarations_available())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EnvironmentState {
    Unrequested,
    IoPending,
    SourceAvailable,
}
#[derive(Serialize)]
pub struct EnvironmentSnapshot {
    pub state: EnvironmentState,
    pub identity: Option<String>,
    pub noise_requested: usize,
    pub noise_completed: usize,
    pub noise_status: Option<String>,
    pub retained_scopes: usize,
    pub metadata_bytes: usize,
    pub mapped_bytes: u64,
    pub runtime_ready: bool,
}
pub(super) struct Batch {
    plan: EnvironmentPlan,
    handle: Option<JobHandle>,
    submitted: bool,
    sources: Option<Arc<ResidentEnvironment>>,
}
impl Batch {
    fn new(plan: EnvironmentPlan) -> Self {
        let submitted = plan.0.water.receipt().usage.jobs == 0;
        let sources = if submitted {
            Some(Arc::new(ResidentEnvironment {
                scope: plan.0.clone(),
                noise: None,
            }))
        } else {
            None
        };
        Self {
            plan,
            handle: None,
            submitted,
            sources,
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.sources.is_none()
    }
    pub(super) fn ready(&self) -> bool {
        self.sources.is_some() && self.plan.0.declarations_available()
    }
    pub(super) fn requested(&self) -> usize {
        self.plan.0.water.receipt().usage.jobs
    }
    pub(super) fn bytes(&self) -> usize {
        self.plan.0.water.receipt().usage.noise_bytes
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.cancel();
        }
    }
}
impl CellResidency {
    pub fn request_environment(
        &mut self,
        ticket: &Ticket,
        plan: EnvironmentPlan,
        store: &mut RecordStore,
    ) -> JobResult<()> {
        self.validate(ticket)?;
        if self.environment.is_some() || self.render_published {
            return Err(JobError::Invalid(
                "environment admission already used".into(),
            ));
        }
        if !Arc::ptr_eq(
            &plan.0.pin.models,
            self.sources.as_ref().expect("decoded CELL source"),
        ) {
            return Err(JobError::Invalid(
                "environment belongs to another resident source lease".into(),
            ));
        }
        validate_store(&plan.0.pin.models, store)?;
        plan.0.lighting.validate_sources(store)?;
        plan.0.water.validate_sources(store)?;
        self.validate_payloads(
            plan.0.water.receipt().usage.jobs,
            plan.0.water.receipt().usage.noise_bytes,
        )?;
        self.environment = Some(Batch::new(plan));
        self.dependencies = Readiness::Pending;
        self.stage = Stage::Decoded;
        Ok(())
    }
    pub fn environment_sources(&self, ticket: &Ticket) -> JobResult<Arc<ResidentEnvironment>> {
        self.validate(ticket)?;
        self.environment
            .as_ref()
            .and_then(|batch| batch.sources.clone())
            .ok_or_else(|| {
                JobError::Invalid("environment noise source is still pending or unrequested".into())
            })
    }
    pub fn environment_snapshot(&self) -> EnvironmentSnapshot {
        let usage = self
            .plan_usage
            .lock()
            .expect("private environment accounting");
        EnvironmentSnapshot {
            state: self
                .environment
                .as_ref()
                .map_or(EnvironmentState::Unrequested, |b| {
                    if b.pending() {
                        EnvironmentState::IoPending
                    } else {
                        EnvironmentState::SourceAvailable
                    }
                }),
            identity: self.environment.as_ref().map(|b| b.plan.identity().into()),
            noise_requested: self.environment.as_ref().map_or(0, Batch::requested),
            noise_completed: self.environment.as_ref().map_or(0, |b| {
                usize::from(b.sources.as_ref().is_some_and(|s| s.noise.is_some()))
            }),
            noise_status: self.environment.as_ref().and_then(|b| {
                b.plan
                    .0
                    .water
                    .receipt()
                    .noise
                    .as_ref()
                    .map(|n| n.status.into())
            }),
            retained_scopes: usage.environment_epochs.len(),
            metadata_bytes: usage.environment_metadata,
            mapped_bytes: usage.environment_mapped,
            runtime_ready: false,
        }
    }
    pub(super) fn poll_environment(&mut self) -> JobResult<()> {
        let batch = self
            .environment
            .as_mut()
            .expect("pending environment batch");
        batch.plan.ticket().check()?;
        if let Some(handle) = &batch.handle {
            if let Some(noise) = handle.try_take()? {
                // The same parent epoch gates extraction, cache markers and completion.
                batch.plan.ticket().check()?;
                batch.sources = Some(Arc::new(ResidentEnvironment {
                    scope: batch.plan.0.clone(),
                    noise: Some(noise),
                }));
                batch.handle = None;
            }
        } else if !batch.submitted {
            let member = batch
                .plan
                .0
                .water
                .resident_member(&batch.plan.0.pin.models)?
                .ok_or_else(|| {
                    JobError::Invalid("sealed environment noise member disappeared".into())
                })?;
            let token = self.generation.token()?;
            match self.jobs.submit_scoped(
                member,
                token,
                self.cache.clone(),
                batch.plan.0.clone(),
                #[cfg(test)]
                self.pause.clone(),
            ) {
                Ok(handle) => {
                    batch.handle = Some(handle);
                    batch.submitted = true;
                    self.backpressured = false;
                }
                Err(JobError::QueueFull | JobError::ByteBudget) => self.backpressured = true,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}
