//! Explicit engineering consumption of an existing journal prefix. The owned
//! private World is discarded on failure; no partial snapshot/trace is returned.
use super::{copy_probe, local_copy};
use crate::{
    World, foreign::Content, identity::Owner, preparation, programs::PreparedSources,
    snapshot::Snapshot,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub sequence: NonZeroU64,
    pub activation: NonZeroU64,
}

/// One source-qualified event in an application tick. The owner is repeated
/// from the current canonical instance so a stale or reordered queue cannot
/// silently execute against a different script instance.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerRequest {
    pub sequence: NonZeroU64,
    pub expected_owner: Owner,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_events: usize,
    pub maximum_source_instructions: usize,
    pub maximum_statement_bytes: usize,
    /// Conservative projection admission for old copy traces, including both
    /// contexts and canonical values of own bound reads/destinations.
    pub trace_projection: preparation::ObservationLimits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_events: 64,
            maximum_source_instructions: 192,
            maximum_statement_bytes: 65_539,
            trace_projection: preparation::ObservationLimits::default(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub events: usize,
    pub source_instructions: usize,
    pub statement_bytes: usize,
    pub trace_projection: preparation::ObservationCounts,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved journal batch input is invalid: {0}")]
    Input(&'static str),
    #[error("saved journal batch budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Preparation(#[from] preparation::Error),
    #[error(transparent)]
    Observation(#[from] preparation::ObservationError),
    #[error(transparent)]
    Copy(#[from] copy_probe::Error),
    #[error(transparent)]
    Multi(#[from] local_copy::MultiError),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}
#[derive(Debug, Serialize)]
pub struct Completed {
    pub snapshot: Snapshot,
    pub committed: Vec<local_copy::CommittedCopy>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub committed_multi: Vec<local_copy::CommittedMultiCopy>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ordered_events: Vec<CommittedEvent>,
    pub counts: Counts,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommittedAdapter {
    FragmentCopy,
    OwnedMultiCopy,
}
#[derive(Debug, Serialize)]
pub struct CommittedEvent {
    pub sequence: u64,
    pub expected_owner: Owner,
    pub adapter: CommittedAdapter,
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Unsupported {
        event_index: Option<usize>,
        reason: local_copy::Unsupported,
        detail: String,
    },
    EngineeringCommitted {
        result: Box<Completed>,
    },
}

/// Call-site witnesses for cooperative work. These do not count immutable
/// source parsing performed earlier when PreparedSources was created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkCounts {
    pub source_frame_attempts: usize,
    pub copy_adapter_attempts: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Ready,
    Unsupported,
    Failed,
}
/// Counters and lifecycle only. Progress carries no state or commit authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub status: Status,
    pub counts: Counts,
    pub work: WorkCounts,
}
enum State {
    Pending,
    Ready,
    Unsupported {
        event_index: Option<usize>,
        reason: local_copy::Unsupported,
        detail: String,
    },
    Failed,
}
struct BatchRequest {
    sequence: NonZeroU64,
    expected_owner: Owner,
    identity_error: &'static str,
}
struct BatchProjection {
    owner_bytes: usize,
    include_ordered_events: bool,
}
enum StepOutcome {
    Single(local_copy::CommittedCopy),
    Multi(local_copy::CommittedMultiCopy),
    Unsupported {
        reason: local_copy::Unsupported,
        detail: String,
    },
}
/// Ephemeral private prefix execution. Dropping this job or finishing early
/// discards all private effects. Borrowed inputs cannot change between slices.
pub struct Job<'w, 'p, 's> {
    world: Option<World<'w>>,
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    requests: Vec<BatchRequest>,
    intent: local_copy::Intent,
    limits: Limits,
    counts: Counts,
    work: WorkCounts,
    remaining: preparation::ObservationLimits,
    committed: Vec<local_copy::CommittedCopy>,
    committed_multi: Vec<local_copy::CommittedMultiCopy>,
    ordered_events: Vec<CommittedEvent>,
    include_ordered_events: bool,
    state: State,
}

fn charge(
    used: &mut usize,
    amount: usize,
    maximum: usize,
    name: &'static str,
) -> Result<(), Error> {
    *used = used
        .checked_add(amount)
        .filter(|&sum| sum <= maximum)
        .ok_or(Error::Capacity(name))?;
    Ok(())
}

/// Move a strictly source-restored private World into this operation. Explicit
/// entries must cover its exact existing prefix; this creates no event/instance.
/// Every error/refusal consumes and discards that private World. Existing input
/// bytes/caller snapshots are never modified or returned as a partial success.
pub fn consume(
    world: World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    requests: &[Request],
    intent: local_copy::Intent,
    limits: Limits,
) -> Result<Outcome, Error> {
    let mut job = Job::new(world, sources, content, requests, intent, limits)?;
    job.advance(usize::MAX)?;
    job.finish()
}

impl<'w, 'p, 's> Job<'w, 'p, 's> {
    pub fn new(
        world: World<'w>,
        sources: &'p PreparedSources<'s>,
        content: &'p Content,
        requests: &'p [Request],
        intent: local_copy::Intent,
        limits: Limits,
    ) -> Result<Self, Error> {
        if requests.is_empty() {
            return Err(Error::Input("an explicit nonempty prefix is required"));
        }
        if requests.len() > limits.maximum_events {
            return Err(Error::Capacity("events"));
        }
        let requests = requests
            .iter()
            .map(|request| BatchRequest {
                sequence: request.sequence,
                expected_owner: Owner::Fragment {
                    activation: request.activation,
                },
                identity_error: "explicit fragment activation differs from the saved owner",
            })
            .collect();
        Self::new_inner(
            world,
            sources,
            content,
            requests,
            BatchProjection {
                owner_bytes: 0,
                include_ordered_events: false,
            },
            intent,
            limits,
        )
    }

    /// Admit an exact ordered prefix for a behavior tick. Quest and Placed
    /// owners use the existing bounded multi-copy adapter; Fragment owners use
    /// the existing single-copy adapter. No owner, event or dispatch is made up.
    pub fn new_ordered(
        world: World<'w>,
        sources: &'p PreparedSources<'s>,
        content: &'p Content,
        requests: &[OwnerRequest],
        intent: local_copy::Intent,
        limits: Limits,
    ) -> Result<Self, Error> {
        if requests.is_empty() {
            return Err(Error::Input("an explicit nonempty prefix is required"));
        }
        if requests.len() > limits.maximum_events {
            return Err(Error::Capacity("events"));
        }
        let mut owner_bytes = 0_usize;
        for request in requests {
            if let Owner::Quest { key } = &request.expected_owner {
                charge(
                    &mut owner_bytes,
                    key.origin_plugin.len(),
                    limits.trace_projection.maximum_variable_bytes,
                    "trace variable bytes",
                )?;
            }
        }
        let requests = requests
            .iter()
            .map(|request| BatchRequest {
                sequence: request.sequence,
                expected_owner: request.expected_owner.clone(),
                identity_error: "explicit source-qualified owner differs from the saved owner",
            })
            .collect();
        Self::new_inner(
            world,
            sources,
            content,
            requests,
            BatchProjection {
                owner_bytes,
                include_ordered_events: true,
            },
            intent,
            limits,
        )
    }

    fn new_inner(
        world: World<'w>,
        sources: &'p PreparedSources<'s>,
        content: &'p Content,
        requests: Vec<BatchRequest>,
        projection: BatchProjection,
        intent: local_copy::Intent,
        limits: Limits,
    ) -> Result<Self, Error> {
        let mut remaining = limits.trace_projection;
        remaining.maximum_variable_bytes = remaining
            .maximum_variable_bytes
            .checked_sub(projection.owner_bytes)
            .ok_or(Error::Capacity("trace variable bytes"))?;
        let mut job = Self {
            world: Some(world),
            sources,
            content,
            requests,
            intent,
            limits,
            counts: Counts {
                events: 0,
                source_instructions: 0,
                statement_bytes: 0,
                trace_projection: preparation::ObservationCounts {
                    source_bytes: 0,
                    rows: 0,
                    variable_bytes: projection.owner_bytes,
                    binding_uses: 0,
                },
            },
            work: WorkCounts {
                source_frame_attempts: 0,
                copy_adapter_attempts: 0,
            },
            remaining,
            committed: Vec::new(),
            committed_multi: Vec::new(),
            ordered_events: Vec::new(),
            include_ordered_events: projection.include_ordered_events,
            state: State::Pending,
        };
        if intent == local_copy::Intent::Faithful {
            job.discard();
            job.state = State::Unsupported {
                event_index: None,
                reason: local_copy::Unsupported::UnverifiedRetailSemantics,
                detail: "Original assignment conversions, dispatch order and event-list lifecycle are unverified".into(),
            };
            return Ok(job);
        }
        let world = job.world.as_ref().expect("private world present");
        content.validate_world(world)?;
        if world.pending_events().len() < job.requests.len() {
            return Err(Error::Input("prefix exceeds the existing journal"));
        }
        // Validate the complete prefix before the first private event commit.
        for (pending, request) in world.pending_events().zip(&job.requests) {
            if pending.sequence != request.sequence.get() {
                return Err(Error::Input(
                    "requests must name the exact existing ordered journal prefix",
                ));
            }
            world.validate_owner(&request.expected_owner)?;
            let instance = world.instance(world.handle(pending.instance)?)?;
            if instance.owner() != &request.expected_owner {
                return Err(Error::Input(request.identity_error));
            }
        }
        job.committed = Vec::with_capacity(job.requests.len());
        job.committed_multi = Vec::with_capacity(job.requests.len());
        if projection.include_ordered_events {
            job.ordered_events = Vec::with_capacity(job.requests.len());
        }
        Ok(job)
    }

    pub fn progress(&self) -> Progress {
        Progress {
            status: match self.state {
                State::Pending => Status::Pending,
                State::Ready => Status::Ready,
                State::Unsupported { .. } => Status::Unsupported,
                State::Failed => Status::Failed,
            },
            counts: self.counts,
            work: self.work,
        }
    }

    fn discard(&mut self) {
        self.world.take();
        self.committed.clear();
        self.committed_multi.clear();
        self.ordered_events.clear();
    }

    /// A complete source-admitted event is indivisible. Slice size bounds the
    /// number of events; all source/projection caps remain global to the job.
    pub fn advance(&mut self, maximum_complete_events: usize) -> Result<Progress, Error> {
        if maximum_complete_events == 0 {
            return Err(Error::Input("a positive complete-event slice is required"));
        }
        if matches!(self.state, State::Failed) {
            return Err(Error::Input("the private job has already failed"));
        }
        if !matches!(self.state, State::Pending) {
            return Ok(self.progress());
        }
        for _ in 0..maximum_complete_events.min(self.requests.len() - self.counts.events) {
            let event_index = self.counts.events;
            let outcome = match self.step() {
                Ok(outcome) => outcome,
                Err(error) => {
                    self.discard();
                    self.state = State::Failed;
                    return Err(error);
                }
            };
            match outcome {
                StepOutcome::Single(committed) => {
                    if self.include_ordered_events {
                        let request = &self.requests[event_index];
                        self.ordered_events.push(CommittedEvent {
                            sequence: request.sequence.get(),
                            expected_owner: request.expected_owner.clone(),
                            adapter: CommittedAdapter::FragmentCopy,
                        });
                    }
                    self.committed.push(committed);
                    self.counts.events += 1;
                }
                StepOutcome::Multi(committed) => {
                    if self.include_ordered_events {
                        let request = &self.requests[event_index];
                        self.ordered_events.push(CommittedEvent {
                            sequence: request.sequence.get(),
                            expected_owner: request.expected_owner.clone(),
                            adapter: CommittedAdapter::OwnedMultiCopy,
                        });
                    }
                    self.committed_multi.push(committed);
                    self.counts.events += 1;
                }
                StepOutcome::Unsupported { reason, detail } => {
                    self.discard();
                    self.state = State::Unsupported {
                        event_index: Some(event_index),
                        reason,
                        detail,
                    };
                    return Ok(self.progress());
                }
            }
        }
        if self.counts.events == self.requests.len() {
            self.state = State::Ready;
        }
        Ok(self.progress())
    }

    pub fn finish(mut self) -> Result<Outcome, Error> {
        match self.state {
            State::Ready => Ok(Outcome::EngineeringCommitted {
                result: Box::new(Completed {
                    snapshot: self
                        .world
                        .take()
                        .expect("complete private world")
                        .snapshot(),
                    committed: self.committed,
                    committed_multi: self.committed_multi,
                    ordered_events: self.ordered_events,
                    counts: self.counts,
                }),
            }),
            State::Unsupported {
                event_index,
                reason,
                detail,
            } => Ok(Outcome::Unsupported {
                event_index,
                reason,
                detail,
            }),
            State::Pending => Err(Error::Input(
                "the explicit prefix is not completely executed",
            )),
            State::Failed => Err(Error::Input("the private job has already failed")),
        }
    }

    fn step(&mut self) -> Result<StepOutcome, Error> {
        self.work.source_frame_attempts += 1;
        let world = self.world.as_mut().expect("pending private world");
        let request = &self.requests[self.counts.events];
        let sources = self.sources;
        let content = self.content;
        let intent = self.intent;
        let limits = self.limits;
        let counts = &mut self.counts;
        let remaining = &mut self.remaining;
        let frame = world.prepare_event_with_sources(
            request.sequence.get(),
            sources,
            limits
                .maximum_source_instructions
                .saturating_sub(counts.source_instructions),
        )?;
        charge(
            &mut counts.source_instructions,
            frame.instructions().len(),
            limits.maximum_source_instructions,
            "source instructions",
        )?;
        let statement_bytes_before = counts.statement_bytes;
        for instruction in &frame.instructions()[1..frame.instructions().len() - 1] {
            charge(
                &mut counts.statement_bytes,
                instruction.bytes.len(),
                limits.maximum_statement_bytes,
                "statement bytes",
            )?;
        }
        let begin = frame.instructions()[0].bytes.start;
        let end = frame.instructions().last().expect("checked end").bytes.end;
        let bindings = &frame.source().bindings().uses;
        if bindings.len() > remaining.maximum_binding_uses {
            return Err(Error::Capacity("trace binding uses"));
        }
        let event_observation_limits = *remaining;
        // Admission before allocating the temporary local-index rows. Repeated
        // own bindings preserve multiplicity; foreign scope is never resolved.
        let mut selected = Vec::new();
        for binding in bindings {
            if (begin..end).contains(&binding.scda_offset)
                && matches!(binding.role, 2 | 4)
                && binding.context_reference.is_none()
                && binding.local_declaration_decoded_offset.is_some()
            {
                if selected.len() >= remaining.maximum_rows {
                    return Err(Error::Capacity("trace local rows"));
                }
                selected.push(u32::from(binding.index));
            }
        }
        let projection = preparation::EventObservation::capture(&frame, &selected, *remaining)?;
        let admitted = projection.counts;
        let instructions = frame.instructions().len();
        drop(projection);
        drop(frame);
        remaining.maximum_source_bytes -= admitted.source_bytes;
        remaining.maximum_rows -= admitted.rows;
        remaining.maximum_variable_bytes -= admitted.variable_bytes;
        remaining.maximum_binding_uses -= admitted.binding_uses;
        charge(
            &mut counts.trace_projection.source_bytes,
            admitted.source_bytes,
            limits.trace_projection.maximum_source_bytes,
            "trace source bytes",
        )?;
        charge(
            &mut counts.trace_projection.rows,
            admitted.rows,
            limits.trace_projection.maximum_rows,
            "trace rows",
        )?;
        charge(
            &mut counts.trace_projection.variable_bytes,
            admitted.variable_bytes,
            limits.trace_projection.maximum_variable_bytes,
            "trace variable bytes",
        )?;
        charge(
            &mut counts.trace_projection.binding_uses,
            admitted.binding_uses,
            limits.trace_projection.maximum_binding_uses,
            "trace binding uses",
        )?;
        self.work.copy_adapter_attempts += 1;
        match &request.expected_owner {
            Owner::Fragment { activation } => match copy_probe::commit_pending(
                world,
                sources,
                content,
                request.sequence.get(),
                *activation,
                intent,
                local_copy::Limits {
                    maximum_event_instructions: instructions,
                    maximum_operand_uses: 2,
                    maximum_statement_bytes: limits
                        .maximum_statement_bytes
                        .saturating_sub(statement_bytes_before),
                },
            )? {
                copy_probe::PendingOutcome::EngineeringCommitted { committed } => {
                    Ok(StepOutcome::Single(*committed))
                }
                copy_probe::PendingOutcome::Unsupported { reason, detail } => {
                    Ok(StepOutcome::Unsupported { reason, detail })
                }
            },
            Owner::Quest { .. } | Owner::Placed { .. } => {
                let multi_limits = local_copy::MultiLimits {
                    maximum_event_instructions: instructions,
                    maximum_statement_bytes: limits
                        .maximum_statement_bytes
                        .saturating_sub(statement_bytes_before),
                    observation: event_observation_limits,
                    ..local_copy::MultiLimits::default()
                };
                match world.stage_source_multi_copy_with_sources(
                    request.sequence.get(),
                    sources,
                    content,
                    intent,
                    multi_limits,
                )? {
                    local_copy::MultiPreparation::Staged(stage) => {
                        Ok(StepOutcome::Multi(stage.commit(world)?))
                    }
                    local_copy::MultiPreparation::Unsupported { reason, detail } => {
                        Ok(StepOutcome::Unsupported { reason, detail })
                    }
                }
            }
        }
    }
}

/// Consume an exact source-qualified prefix as one private behavior-tick
/// candidate. A refusal exposes no snapshot, even when an earlier event staged
/// and committed to the private candidate.
pub fn consume_ordered(
    world: World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    requests: &[OwnerRequest],
    intent: local_copy::Intent,
    limits: Limits,
) -> Result<Outcome, Error> {
    let mut job = Job::new_ordered(world, sources, content, requests, intent, limits)?;
    job.advance(usize::MAX)?;
    job.finish()
}
