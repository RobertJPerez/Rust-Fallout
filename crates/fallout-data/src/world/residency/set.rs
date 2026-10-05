//! Fixed, independently cancelled CELL source hosts under one static allowance.
use super::*;
use serde::Serialize;

/// Global allowances are partitioned once; retired hosts retain their partition.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SetLimits {
    pub slots: usize,
    pub workers: usize,
    pub models: usize,
    pub resources: usize,
    pub source_bytes: usize,
    pub retained_plans: usize,
    pub plan_metadata_bytes: usize,
    pub mapped_source_bytes: u64,
    /// Logical manager, admission and snapshot copies; host plans have their own pool.
    pub metadata_bytes: usize,
}
impl Default for SetLimits {
    fn default() -> Self {
        Self {
            slots: 4,
            workers: 4,
            models: 1024,
            resources: 1024,
            source_bytes: 256 * 1024 * 1024,
            retained_plans: 8,
            plan_metadata_bytes: 32 * 1024 * 1024,
            mapped_source_bytes: 16 * 1024 * 1024 * 1024,
            metadata_bytes: 4 * 1024 * 1024,
        }
    }
}
/// Both identities are checked before cloning a sealed plan or revoking a host.
pub struct Admission<'a> {
    pub root: &'a FormKey,
    pub plan: &'a CellModelPlan,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SlotState {
    Free,
    Active,
    Retired,
}
#[derive(Serialize)]
pub struct SlotSnapshot {
    pub slot: usize,
    pub state: SlotState,
    pub root: Option<FormKey>,
    pub workers: usize,
    pub source: Option<Snapshot>,
}
#[derive(Default, Serialize)]
pub struct SetUsage {
    pub active_slots: usize,
    pub retired_slots: usize,
    pub workers: usize,
    pub outstanding: usize,
    pub pinned_source_bytes: usize,
    pub retained_plans: usize,
    pub plan_metadata_bytes: usize,
    pub mapped_source_bytes: u64,
    pub metadata_bytes: usize,
}
#[derive(Serialize)]
pub struct SetSnapshot {
    pub source_cohort_sha256: String,
    pub limits: SetLimits,
    pub usage: SetUsage,
    pub slots: Vec<SlotSnapshot>,
    pub last_polled_slots: Vec<usize>,
    pub simulation_ready: bool,
    pub runtime_ready: bool,
}
struct Slot {
    limits: Limits,
    root: Option<FormKey>,
    host: Option<CellResidency>,
    active: bool,
    key_bytes: usize,
}
struct Candidate {
    slot: usize,
    root: FormKey,
    host: CellResidency,
    ticket: Ticket,
    key_bytes: usize,
}
pub struct CellResidencySet {
    source_tree: PathBuf,
    cache_root: Option<PathBuf>,
    cohort: String,
    limits: SetLimits,
    slots: Vec<Slot>,
    cursor: usize,
    poll_wait_age: Vec<u64>,
    last_polled: Vec<usize>,
    base_metadata: usize,
}
fn invalid(message: &str) -> JobError {
    JobError::Invalid(message.into())
}
fn part(total: usize, count: usize, index: usize) -> usize {
    total / count + usize::from(index < total % count)
}
fn mapped_part(total: u64, count: usize, index: usize) -> u64 {
    total / count as u64 + u64::from((index as u64) < total % count as u64)
}
impl CellResidencySet {
    pub fn new(
        source_tree: &Path,
        cache_root: Option<&Path>,
        cohort: &str,
        limits: SetLimits,
    ) -> JobResult<Self> {
        let max = SetLimits::default();
        if limits.slots == 0
            || limits.slots > max.slots
            || limits.workers < limits.slots
            || limits.workers > max.workers
            || limits.workers > 2 * limits.slots
            || limits.models < limits.slots
            || limits.models > max.models
            || limits.resources < limits.models
            || limits.resources > max.resources
            || limits.source_bytes < limits.slots
            || limits.source_bytes > max.source_bytes
            || limits.retained_plans < limits.slots
            || limits.retained_plans > max.retained_plans
            || limits.plan_metadata_bytes < limits.slots
            || limits.plan_metadata_bytes > max.plan_metadata_bytes
            || limits.mapped_source_bytes < limits.slots as u64
            || limits.mapped_source_bytes > max.mapped_source_bytes
            || limits.metadata_bytes == 0
            || limits.metadata_bytes > max.metadata_bytes
        {
            return Err(invalid("invalid residency set limits"));
        }
        if cohort.len() != 64 || !cohort.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("residency set needs an exact source cohort digest"));
        }
        let path_bytes = source_tree
            .as_os_str()
            .len()
            .checked_add(cache_root.map_or(0, |path| path.as_os_str().len()))
            .and_then(|n| n.checked_mul(16))
            .and_then(|n| n.checked_add(4096 + limits.slots * 4096))
            .ok_or(JobError::ByteBudget)?;
        if path_bytes > limits.metadata_bytes {
            return Err(JobError::ByteBudget);
        }
        // Validate paths before any pool creation. Existing cache validation remains authoritative.
        let cache_root = cache_root
            .map(|path| cache::validate_root(path, source_tree))
            .transpose()?;
        let mut slots = Vec::with_capacity(limits.slots);
        for index in 0..limits.slots {
            slots.push(Slot {
                limits: Limits {
                    workers: part(limits.workers, limits.slots, index),
                    models: part(limits.models, limits.slots, index),
                    resources: part(limits.resources, limits.slots, index),
                    source_bytes: part(limits.source_bytes, limits.slots, index),
                    retained_plans: part(limits.retained_plans, limits.slots, index).min(2),
                    plan_metadata_bytes: part(limits.plan_metadata_bytes, limits.slots, index),
                    mapped_source_bytes: mapped_part(
                        limits.mapped_source_bytes,
                        limits.slots,
                        index,
                    ),
                },
                root: None,
                host: None,
                active: false,
                key_bytes: 0,
            });
        }
        Ok(Self {
            source_tree: source_tree.to_path_buf(),
            cache_root,
            cohort: cohort.to_owned(),
            limits,
            slots,
            cursor: 0,
            poll_wait_age: vec![0; limits.slots],
            last_polled: Vec::with_capacity(limits.slots),
            base_metadata: path_bytes,
        })
    }
    fn metadata_bytes(&self) -> usize {
        self.base_metadata + self.slots.iter().map(|slot| slot.key_bytes).sum::<usize>()
    }
    fn preflight(&self, slot: usize, request: &Admission<'_>) -> JobResult<usize> {
        if request.root != request.plan.root() {
            return Err(invalid("requested CELL differs from sealed model plan"));
        }
        if request.plan.receipt().source_cohort_sha256 != self.cohort {
            return Err(invalid("CELL plan belongs to another source cohort"));
        }
        let limits = self.slots[slot].limits;
        let receipt = request.plan.receipt();
        if receipt.requests.len() > limits.models {
            return Err(JobError::QueueFull);
        }
        if receipt.usage.model_decoded_bytes > limits.source_bytes
            || receipt.usage.metadata_bytes > limits.plan_metadata_bytes
        {
            return Err(JobError::ByteBudget);
        }
        let mapped = receipt.archives.iter().try_fold(0u64, |sum, archive| {
            sum.checked_add(archive.source_bytes)
                .ok_or(JobError::ByteBudget)
        })?;
        if mapped > limits.mapped_source_bytes {
            return Err(JobError::ByteBudget);
        }
        request
            .root
            .origin_plugin
            .len()
            .checked_mul(16)
            .and_then(|n| n.checked_add(4096))
            .ok_or(JobError::ByteBudget)
    }
    fn candidate(
        &self,
        slot: usize,
        request: &Admission<'_>,
        key_bytes: usize,
    ) -> JobResult<Candidate> {
        let root = request.root.clone();
        let mut host = CellResidency::new(
            &self.source_tree,
            self.cache_root.as_deref(),
            self.slots[slot].limits,
        )?;
        let ticket = host.request(request.plan.clone())?;
        Ok(Candidate {
            slot,
            root,
            host,
            ticket,
            key_bytes,
        })
    }
    fn commit(&mut self, candidate: Candidate) {
        self.poll_wait_age[candidate.slot] = 0;
        let slot = &mut self.slots[candidate.slot];
        debug_assert!(slot.host.is_none());
        slot.root = Some(candidate.root);
        slot.host = Some(candidate.host);
        slot.active = true;
        slot.key_bytes = candidate.key_bytes;
    }
    /// No returned tickets or successful prefix on a last-candidate refusal.
    /// Tentative pools occupy distinct free partitions and receive no IO poll.
    pub fn admit(&mut self, requests: &[Admission<'_>]) -> JobResult<Vec<Ticket>> {
        if requests.is_empty() || requests.len() > self.limits.slots {
            return Err(invalid("residency set admission count out of bounds"));
        }
        let free: Vec<_> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.host.is_none().then_some(index))
            .collect();
        if requests.len() > free.len() {
            return Err(JobError::QueueFull);
        }
        let mut metadata = self.metadata_bytes();
        let mut charges = Vec::with_capacity(requests.len());
        for (index, request) in requests.iter().enumerate() {
            if requests[..index]
                .iter()
                .any(|other| other.root == request.root)
                || self
                    .slots
                    .iter()
                    .any(|slot| slot.root.as_ref() == Some(request.root))
            {
                return Err(invalid("duplicate active or retired CELL root"));
            }
            let bytes = self.preflight(free[index], request)?;
            metadata = metadata
                .checked_add(bytes)
                .filter(|n| *n <= self.limits.metadata_bytes)
                .ok_or(JobError::ByteBudget)?;
            charges.push(bytes);
        }
        let mut candidates = Vec::with_capacity(requests.len());
        for (index, request) in requests.iter().enumerate() {
            candidates.push(self.candidate(free[index], request, charges[index])?);
        }
        // All returned-key copies and vector storage were admitted before candidates.
        let tickets = candidates
            .iter()
            .map(|candidate| candidate.ticket.clone())
            .collect();
        for candidate in candidates {
            self.commit(candidate);
        }
        Ok(tickets)
    }
    fn current_slot(&self, ticket: &Ticket) -> JobResult<usize> {
        let index = self
            .slots
            .iter()
            .position(|slot| slot.active && slot.root.as_ref() == Some(ticket.root()))
            .ok_or(JobError::Stale)?;
        let host = self.slots[index]
            .host
            .as_ref()
            .expect("active private host");
        if !host.generation.owns(&ticket.token) {
            return Err(invalid("foreign residency set host ticket"));
        }
        let current = host.ticket.as_ref().ok_or(JobError::Stale)?;
        if current.identity() != ticket.identity() || current.generation() != ticket.generation() {
            return Err(JobError::Stale);
        }
        Ok(index)
    }
    /// Requires a free partition while the old live host remains charged.
    pub fn replace(&mut self, old: &Ticket, request: Admission<'_>) -> JobResult<Ticket> {
        let previous = self.current_slot(old)?;
        old.check()?;
        let next = self
            .slots
            .iter()
            .position(|slot| slot.host.is_none())
            .ok_or(JobError::QueueFull)?;
        if self
            .slots
            .iter()
            .enumerate()
            .any(|(index, slot)| index != previous && slot.root.as_ref() == Some(request.root))
        {
            return Err(invalid("replacement CELL duplicates another slot"));
        }
        let bytes = self.preflight(next, &request)?;
        if bytes
            > self
                .limits
                .metadata_bytes
                .saturating_sub(self.metadata_bytes())
        {
            return Err(JobError::ByteBudget);
        }
        let candidate = self.candidate(next, &request, bytes)?;
        let ticket = candidate.ticket.clone();
        // Existing unload validates epoch advance before changing this old host.
        self.slots[previous]
            .host
            .as_mut()
            .expect("active private host")
            .unload()?;
        self.slots[previous].active = false;
        self.commit(candidate);
        Ok(ticket)
    }
    pub fn sources(&self, ticket: &Ticket) -> JobResult<Arc<ResidentSources>> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_ref()
            .expect("active private host")
            .sources(ticket)
    }
    #[cfg(test)]
    pub(super) fn pause_test_work(
        &mut self,
        ticket: &Ticket,
        pause: Arc<crate::resource_jobs::tests::Pause>,
    ) -> JobResult<()> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .pause = Some(pause);
        Ok(())
    }
    pub fn request_textures(&mut self, ticket: &Ticket, plan: TexturePlan) -> JobResult<()> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .request_textures(ticket, plan)
    }
    pub fn texture_sources(&self, ticket: &Ticket) -> JobResult<Arc<ResidentTextures>> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_ref()
            .expect("active private host")
            .texture_sources(ticket)
    }
    pub fn report_dependencies(&mut self, ticket: &Ticket, readiness: Readiness) -> JobResult<()> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .report_dependencies(ticket, readiness)
    }
    pub fn report_collision(&mut self, ticket: &Ticket, readiness: Readiness) -> JobResult<()> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .report_collision(ticket, readiness)
    }
    pub fn report_behavior(&mut self, ticket: &Ticket, readiness: Readiness) -> JobResult<()> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .report_behavior(ticket, readiness)
    }
    pub fn publish_render<T>(
        &mut self,
        ticket: &Ticket,
        publish: impl FnOnce() -> JobResult<T>,
    ) -> JobResult<T> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .publish_render(ticket, publish)
    }
    /// Owned current failures are removable too; stale/foreign tickets cannot remove a host.
    pub fn remove(&mut self, ticket: &Ticket) -> JobResult<()> {
        let index = self.current_slot(ticket)?;
        self.slots[index]
            .host
            .as_mut()
            .expect("active private host")
            .unload()?;
        self.slots[index].active = false;
        Ok(())
    }
    /// Visits bounded slots in round-robin order, including free slots.
    /// Each existing IO poll has up to eight inspections and eight submissions.
    pub fn poll(&mut self, slot_budget: usize) -> JobResult<SetSnapshot> {
        self.poll_ordered(&[], slot_budget)
    }
    /// Ages every occupied slot so active preferences cannot starve retired
    /// hosts. Preference order breaks equal-age ties; each visit still uses the
    /// existing bounded poll and ResourceJobs queues.
    pub fn poll_ordered(
        &mut self,
        preferred: &[Ticket],
        slot_budget: usize,
    ) -> JobResult<SetSnapshot> {
        if slot_budget == 0 || slot_budget > self.limits.slots {
            return Err(invalid("residency set poll slot bound"));
        }
        let mut preferred_rank = vec![usize::MAX; self.slots.len()];
        for (rank, ticket) in preferred.iter().enumerate() {
            let index = self.current_slot(ticket)?;
            if preferred_rank[index] == usize::MAX {
                preferred_rank[index] = rank;
            }
        }
        let preference_scale = preferred.len() as u64 + 1;
        let slot_count = self.slots.len();
        let mut order: Vec<_> = (0..slot_count)
            .filter(|index| self.slots[*index].host.is_some())
            .collect();
        order.sort_by(|left, right| {
            let score = |index: usize| {
                let rank = preferred_rank[index];
                let preference_bonus = if rank == usize::MAX {
                    0
                } else {
                    (preferred.len() - rank) as u64
                };
                self.poll_wait_age[index]
                    .saturating_mul(preference_scale)
                    .saturating_add(preference_bonus)
            };
            let cursor_distance = |index: usize| (index + slot_count - self.cursor) % slot_count;
            score(*right)
                .cmp(&score(*left))
                .then_with(|| cursor_distance(*left).cmp(&cursor_distance(*right)))
        });
        order.truncate(slot_budget);
        self.last_polled.clone_from(&order);
        if let Some(last) = order.last() {
            self.cursor = (last + 1) % self.slots.len();
        }
        for &index in &order {
            let slot = &mut self.slots[index];
            if let Some(host) = &mut slot.host {
                // A host retains its own Failed/error snapshot; other cells still advance.
                let _ = host.poll();
                let state = host.snapshot();
                if !slot.active
                    && state.stage == Stage::Unrequested
                    && state.outstanding == 0
                    && state.pinned_source_bytes == 0
                    && state.retained_plans == 0
                    && state.plan_metadata_bytes == 0
                    && state.mapped_source_bytes == 0
                {
                    // No owned work or escaped source pin remains; join only this drained pool.
                    slot.host = None;
                    slot.root = None;
                    slot.key_bytes = 0;
                }
            }
        }
        for index in 0..self.slots.len() {
            if self.slots[index].host.is_none() || order.contains(&index) {
                self.poll_wait_age[index] = 0;
            } else {
                self.poll_wait_age[index] = self.poll_wait_age[index].saturating_add(1);
            }
        }
        Ok(self.snapshot())
    }
    pub fn snapshot(&self) -> SetSnapshot {
        let mut usage = SetUsage {
            metadata_bytes: self.metadata_bytes(),
            ..Default::default()
        };
        let slots = self
            .slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let source = slot.host.as_ref().map(CellResidency::snapshot);
                let state = if source.is_none() {
                    SlotState::Free
                } else if slot.active {
                    SlotState::Active
                } else {
                    SlotState::Retired
                };
                let workers = if source.is_some() {
                    slot.limits.workers
                } else {
                    0
                };
                usage.workers += workers;
                usage.active_slots += usize::from(state == SlotState::Active);
                usage.retired_slots += usize::from(state == SlotState::Retired);
                if let Some(source) = &source {
                    usage.outstanding += source.outstanding;
                    usage.pinned_source_bytes += source.pinned_source_bytes;
                    usage.retained_plans += source.retained_plans;
                    usage.plan_metadata_bytes += source.plan_metadata_bytes;
                    usage.mapped_source_bytes += source.mapped_source_bytes;
                }
                SlotSnapshot {
                    slot: index,
                    state,
                    root: slot.root.clone(),
                    workers,
                    source,
                }
            })
            .collect();
        SetSnapshot {
            source_cohort_sha256: self.cohort.clone(),
            limits: self.limits,
            usage,
            slots,
            last_polled_slots: self.last_polled.clone(),
            simulation_ready: false,
            runtime_ready: false,
        }
    }
}

#[cfg(test)]
mod tests;
