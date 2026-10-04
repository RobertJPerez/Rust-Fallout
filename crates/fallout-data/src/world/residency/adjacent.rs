//! Priority and hysteresis policy over the existing bounded CELL set.
//!
//! Grid, priority, and hysteresis values are supplied by the consumer. This
//! module does not infer activation rules from CELL records or XCLR fields.
use super::*;
use crate::world::cells::CellGridRequest;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_REGION_CANDIDATES: usize = 256;
const MAX_GRID_RADIUS: u16 = 256;

/// Caller-selected admission and retention windows, measured in grid cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RegionPolicy {
    admission_radius: u16,
    retention_radius: u16,
}
impl RegionPolicy {
    pub fn new(admission_radius: u16, retention_radius: u16) -> JobResult<Self> {
        if admission_radius > retention_radius || retention_radius > MAX_GRID_RADIUS {
            return Err(JobError::Invalid(
                "invalid adjacent-region hysteresis radii".into(),
            ));
        }
        Ok(Self {
            admission_radius,
            retention_radius,
        })
    }
    pub fn admission_radius(&self) -> u16 {
        self.admission_radius
    }
    pub fn retention_radius(&self) -> u16 {
        self.retention_radius
    }
}

/// A caller-qualified grid request and its already sealed source plan.
///
/// Higher priority values are admitted and polled first. The owner does not
/// infer travel direction or priority from record data.
pub struct RegionRequest<'a> {
    worldspace: &'a FormKey,
    grid: [i32; 2],
    cell: &'a FormKey,
    plan: &'a CellModelPlan,
    priority: u8,
}
impl<'a> RegionRequest<'a> {
    pub fn new(
        worldspace: &'a FormKey,
        grid: [i32; 2],
        cell: &'a FormKey,
        plan: &'a CellModelPlan,
        priority: u8,
    ) -> JobResult<Self> {
        if cell != plan.root() {
            return Err(JobError::Invalid(
                "region CELL differs from sealed model plan".into(),
            ));
        }
        Ok(Self {
            worldspace,
            grid,
            cell,
            plan,
            priority,
        })
    }
    pub fn from_grid(
        request: &'a CellGridRequest,
        plan: &'a CellModelPlan,
        priority: u8,
    ) -> JobResult<Self> {
        Self::new(
            request.world(),
            request.grid(),
            request.cell(),
            plan,
            priority,
        )
    }
}

/// Stable key for an exterior CELL within a qualified worldspace/grid.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RegionKey {
    worldspace: FormKey,
    grid: [i32; 2],
    cell: FormKey,
}
impl RegionKey {
    pub fn worldspace(&self) -> &FormKey {
        &self.worldspace
    }
    pub fn grid(&self) -> [i32; 2] {
        self.grid
    }
    pub fn cell(&self) -> &FormKey {
        &self.cell
    }
}

/// Full identity used by asynchronous scene consumers.
///
/// World generation is part of identity even when a replacement world reuses
/// the same worldspace, grid, CELL root, and source-plan digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RegionIdentity {
    key: RegionKey,
    world_generation: u64,
    region_generation: u64,
    source_generation: u64,
    source_identity: String,
}
impl RegionIdentity {
    pub fn key(&self) -> &RegionKey {
        &self.key
    }
    pub fn world_generation(&self) -> u64 {
        self.world_generation
    }
    pub fn region_generation(&self) -> u64 {
        self.region_generation
    }
    pub fn source_generation(&self) -> u64 {
        self.source_generation
    }
    pub fn source_identity(&self) -> &str {
        &self.source_identity
    }
}

/// Opaque ticket that binds the scene key, world generation, and existing
/// ResourceJobs owner ticket. Every manager operation checks all three.
#[derive(Clone)]
pub struct RegionTicket {
    identity: RegionIdentity,
    source: Ticket,
    world_generation: Arc<AtomicU64>,
}
impl RegionTicket {
    pub fn identity(&self) -> &RegionIdentity {
        &self.identity
    }
    pub fn check(&self) -> JobResult<()> {
        if self.world_generation.load(Ordering::Acquire) != self.identity.world_generation {
            return Err(JobError::Stale);
        }
        self.source.check()
    }
}

#[derive(Serialize)]
pub struct RegionUpdate {
    pub worldspace: FormKey,
    pub center_grid: [i32; 2],
    pub world_generation: u64,
    pub active_regions: usize,
    pub admitted: usize,
    pub retained: usize,
    pub deferred: usize,
    pub retired: usize,
    pub residency: SetSnapshot,
}

struct ActiveRegion {
    ticket: RegionTicket,
    priority: u8,
    distance: u64,
    wait_age: u64,
}

/// Single bounded multi-CELL source owner with caller-supplied priority,
/// grid hysteresis, and world-generation admission gates.
pub struct AdjacentRegionResidency {
    owner: CellResidencySet,
    policy: RegionPolicy,
    current_worldspace: Option<FormKey>,
    world_generation: u64,
    region_generation: u64,
    generation_gate: Arc<AtomicU64>,
    active: Vec<ActiveRegion>,
    retiring: Vec<Ticket>,
}
impl AdjacentRegionResidency {
    pub fn new(
        source_tree: &Path,
        cache_root: Option<&Path>,
        cohort: &str,
        limits: SetLimits,
        policy: RegionPolicy,
    ) -> JobResult<Self> {
        RegionPolicy::new(policy.admission_radius, policy.retention_radius)?;
        Ok(Self {
            owner: CellResidencySet::new(source_tree, cache_root, cohort, limits)?,
            policy,
            current_worldspace: None,
            world_generation: 0,
            region_generation: 0,
            generation_gate: Arc::new(AtomicU64::new(0)),
            active: Vec::new(),
            retiring: Vec::new(),
        })
    }

    /// Reconcile the bounded desired window. Existing overlapping tickets are
    /// retained; cells outside the retention radius retire immediately. New
    /// candidates within the admission radius are admitted in priority order.
    /// A retired reused root remains deferred until its old host drains.
    pub fn reconcile(
        &mut self,
        worldspace: &FormKey,
        center_grid: [i32; 2],
        requests: &[RegionRequest<'_>],
    ) -> JobResult<RegionUpdate> {
        if requests.len() > MAX_REGION_CANDIDATES {
            return Err(JobError::Invalid(
                "adjacent-region candidate count exceeds bound".into(),
            ));
        }
        for (index, request) in requests.iter().enumerate() {
            if request.worldspace != worldspace || request.cell != request.plan.root() {
                return Err(JobError::Invalid(
                    "region request belongs to another worldspace or CELL plan".into(),
                ));
            }
            if requests[..index]
                .iter()
                .any(|other| other.grid == request.grid || other.cell == request.cell)
            {
                return Err(JobError::Invalid(
                    "duplicate adjacent-region grid or CELL root".into(),
                ));
            }
        }

        if self.current_worldspace.as_ref() == Some(worldspace) {
            for active in &self.active {
                if let Some(request) = requests.iter().find(|request| {
                    request.grid == active.ticket.identity.key.grid
                        && request.cell == &active.ticket.identity.key.cell
                }) {
                    if request.plan.identity() != active.ticket.identity.source_identity {
                        return Err(JobError::Invalid(
                            "active region source changed; replace the world generation".into(),
                        ));
                    }
                }
            }
        }

        let mut retired = self.select_worldspace(worldspace)?;
        retired += self.drain_retiring()?;

        let mut index = 0;
        while index < self.active.len() {
            let distance = grid_distance(self.active[index].ticket.identity.key.grid, center_grid);
            if distance > u64::from(self.policy.retention_radius) {
                self.owner.remove(&self.active[index].ticket.source)?;
                self.active.remove(index);
                retired += 1;
            } else {
                self.active[index].distance = distance;
                if let Some(request) = requests.iter().find(|request| {
                    request.grid == self.active[index].ticket.identity.key.grid
                        && request.cell == &self.active[index].ticket.identity.key.cell
                }) {
                    self.active[index].priority = request.priority;
                }
                index += 1;
            }
        }
        let retained = self.active.len();

        let mut candidates: Vec<usize> = requests
            .iter()
            .enumerate()
            .filter_map(|(index, request)| {
                let distance = grid_distance(request.grid, center_grid);
                (distance <= u64::from(self.policy.admission_radius)
                    && !self.active.iter().any(|active| {
                        active.ticket.identity.key.grid == request.grid
                            && active.ticket.identity.key.cell == *request.cell
                    }))
                .then_some(index)
            })
            .collect();
        candidates.sort_by(|left, right| {
            let left = &requests[*left];
            let right = &requests[*right];
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| {
                    grid_distance(left.grid, center_grid)
                        .cmp(&grid_distance(right.grid, center_grid))
                })
                .then_with(|| left.grid.cmp(&right.grid))
                .then_with(|| left.cell.cmp(right.cell))
        });

        let retired_roots: Vec<FormKey> = self
            .owner
            .snapshot()
            .slots
            .into_iter()
            .filter(|slot| slot.state == SlotState::Retired)
            .filter_map(|slot| slot.root)
            .collect();
        let mut admitted = 0;
        let mut deferred = 0;
        for candidate in candidates {
            let request = &requests[candidate];
            if self.active.iter().any(|active| {
                active.ticket.identity.key.cell == *request.cell
                    && active.ticket.identity.key.grid != request.grid
            }) {
                return Err(JobError::Invalid(
                    "active CELL root is already assigned to another grid".into(),
                ));
            }
            if retired_roots.iter().any(|root| root == request.cell) {
                deferred += 1;
                continue;
            }
            let region_generation = self
                .region_generation
                .checked_add(1)
                .ok_or(JobError::Closed)?;
            let admission = Admission {
                root: request.cell,
                plan: request.plan,
            };
            let source = match self.owner.admit(std::slice::from_ref(&admission)) {
                Ok(mut tickets) => tickets.pop().expect("single region admission ticket"),
                Err(JobError::QueueFull | JobError::ByteBudget) => {
                    deferred += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            self.region_generation = region_generation;
            let identity = RegionIdentity {
                key: RegionKey {
                    worldspace: worldspace.clone(),
                    grid: request.grid,
                    cell: request.cell.clone(),
                },
                world_generation: self.world_generation,
                region_generation,
                source_generation: source.generation(),
                source_identity: source.identity().to_owned(),
            };
            self.active.push(ActiveRegion {
                ticket: RegionTicket {
                    identity,
                    source,
                    world_generation: self.generation_gate.clone(),
                },
                priority: request.priority,
                distance: grid_distance(request.grid, center_grid),
                wait_age: 0,
            });
            admitted += 1;
        }

        Ok(RegionUpdate {
            worldspace: worldspace.clone(),
            center_grid,
            world_generation: self.world_generation,
            active_regions: self.active.len(),
            admitted,
            retained,
            deferred,
            retired,
            residency: self.owner.snapshot(),
        })
    }

    fn select_worldspace(&mut self, worldspace: &FormKey) -> JobResult<usize> {
        match self
            .current_worldspace
            .as_ref()
            .map(|current| current == worldspace)
        {
            None => self.replace_worldspace(worldspace),
            Some(true) => Ok(0),
            Some(false) => self.replace_worldspace(worldspace),
        }
    }

    /// Start a new world instance, including a reload that reuses the same
    /// worldspace FormKey. The generation gate closes old tickets first; their
    /// set hosts retire through the existing owner and remain budgeted to drain.
    pub fn replace_worldspace(&mut self, worldspace: &FormKey) -> JobResult<usize> {
        let next = if self.current_worldspace.is_none() {
            1
        } else {
            self.world_generation
                .checked_add(1)
                .ok_or(JobError::Closed)?
        };
        self.generation_gate.store(next, Ordering::Release);
        for active in self.active.drain(..) {
            self.retiring.push(active.ticket.source);
        }
        self.current_worldspace = Some(worldspace.clone());
        self.world_generation = next;
        self.drain_retiring()
    }

    pub fn world_generation(&self) -> u64 {
        self.world_generation
    }

    fn drain_retiring(&mut self) -> JobResult<usize> {
        let mut retired = 0;
        while let Some(ticket) = self.retiring.pop() {
            if let Err(error) = self.owner.remove(&ticket) {
                self.retiring.push(ticket);
                return Err(error);
            }
            retired += 1;
        }
        Ok(retired)
    }

    fn validate(&self, ticket: &RegionTicket) -> JobResult<Ticket> {
        ticket.check()?;
        if ticket.identity.world_generation != self.world_generation
            || self.current_worldspace.as_ref() != Some(&ticket.identity.key.worldspace)
        {
            return Err(JobError::Stale);
        }
        let active = self
            .active
            .iter()
            .find(|active| active.ticket.identity.key == ticket.identity.key)
            .ok_or(JobError::Stale)?;
        if active.ticket.identity != ticket.identity {
            return Err(JobError::Stale);
        }
        Ok(ticket.source.clone())
    }

    pub fn active_tickets(&self) -> Vec<RegionTicket> {
        self.active
            .iter()
            .map(|active| active.ticket.clone())
            .collect()
    }
    pub fn is_current(&self, ticket: &RegionTicket) -> bool {
        self.validate(ticket).is_ok()
    }
    pub fn sources(&self, ticket: &RegionTicket) -> JobResult<Arc<ResidentSources>> {
        let source = self.validate(ticket)?;
        self.owner.sources(&source)
    }
    pub fn request_textures(&mut self, ticket: &RegionTicket, plan: TexturePlan) -> JobResult<()> {
        let source = self.validate(ticket)?;
        self.owner.request_textures(&source, plan)
    }
    pub fn texture_sources(&self, ticket: &RegionTicket) -> JobResult<Arc<ResidentTextures>> {
        let source = self.validate(ticket)?;
        self.owner.texture_sources(&source)
    }
    pub fn report_dependencies(
        &mut self,
        ticket: &RegionTicket,
        readiness: Readiness,
    ) -> JobResult<()> {
        let source = self.validate(ticket)?;
        self.owner.report_dependencies(&source, readiness)
    }
    pub fn report_collision(
        &mut self,
        ticket: &RegionTicket,
        readiness: Readiness,
    ) -> JobResult<()> {
        let source = self.validate(ticket)?;
        self.owner.report_collision(&source, readiness)
    }
    pub fn report_behavior(
        &mut self,
        ticket: &RegionTicket,
        readiness: Readiness,
    ) -> JobResult<()> {
        let source = self.validate(ticket)?;
        self.owner.report_behavior(&source, readiness)
    }
    pub fn publish_render<T>(
        &mut self,
        ticket: &RegionTicket,
        publish: impl FnOnce() -> JobResult<T>,
    ) -> JobResult<T> {
        let source = self.validate(ticket)?;
        self.owner.publish_render(&source, publish)
    }
    pub fn remove(&mut self, ticket: &RegionTicket) -> JobResult<()> {
        let source = self.validate(ticket)?;
        self.owner.remove(&source)?;
        let index = self
            .active
            .iter()
            .position(|active| active.ticket.identity == ticket.identity)
            .expect("validated active region ticket");
        self.active.remove(index);
        Ok(())
    }

    /// Higher-priority new work starts first, then waiting active cells receive
    /// fair turns before priority breaks ties. Retired owners still drain.
    pub fn poll(&mut self, slot_budget: usize) -> JobResult<SetSnapshot> {
        let mut order: Vec<usize> = (0..self.active.len()).collect();
        order.sort_by(|left, right| {
            let left = &self.active[*left];
            let right = &self.active[*right];
            right
                .wait_age
                .cmp(&left.wait_age)
                .then_with(|| right.priority.cmp(&left.priority))
                .then_with(|| left.distance.cmp(&right.distance))
                .then_with(|| {
                    left.ticket
                        .identity
                        .key
                        .grid
                        .cmp(&right.ticket.identity.key.grid)
                })
        });
        let preferred: Vec<_> = order
            .iter()
            .map(|index| self.active[*index].ticket.source.clone())
            .collect();
        let snapshot = self.owner.poll_ordered(&preferred, slot_budget)?;
        let polled_roots: Vec<_> = snapshot
            .last_polled_slots
            .iter()
            .filter_map(|index| snapshot.slots[*index].root.as_ref())
            .collect();
        for active in &mut self.active {
            if polled_roots.contains(&&active.ticket.identity.key.cell) {
                active.wait_age = 0;
            } else {
                active.wait_age = active.wait_age.saturating_add(1);
            }
        }
        Ok(snapshot)
    }
    pub fn snapshot(&self) -> SetSnapshot {
        self.owner.snapshot()
    }
}

fn grid_distance(left: [i32; 2], right: [i32; 2]) -> u64 {
    let dx = (i64::from(left[0]) - i64::from(right[0])).abs() as u64;
    let dy = (i64::from(left[1]) - i64::from(right[1])).abs() as u64;
    dx.max(dy)
}

#[cfg(test)]
mod tests;
