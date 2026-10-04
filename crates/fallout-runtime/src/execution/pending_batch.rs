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
#[derive(Debug, Serialize)]
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
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}
#[derive(Debug, Serialize)]
pub struct Completed {
    pub snapshot: Snapshot,
    pub committed: Vec<local_copy::CommittedCopy>,
    pub counts: Counts,
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
    mut world: World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    requests: &[Request],
    intent: local_copy::Intent,
    limits: Limits,
) -> Result<Outcome, Error> {
    if requests.is_empty() {
        return Err(Error::Input("an explicit nonempty prefix is required"));
    }
    if requests.len() > limits.maximum_events {
        return Err(Error::Capacity("events"));
    }
    if intent == local_copy::Intent::Faithful {
        return Ok(Outcome::Unsupported { event_index: None, reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            detail: "Original assignment conversions, dispatch order and event-list lifecycle are unverified".into() });
    }
    content.validate_world(&world)?;
    if world.pending_events().len() < requests.len() {
        return Err(Error::Input("prefix exceeds the existing journal"));
    }
    // Validate all sequence/owner identities before any private commit.
    for (pending, request) in world.pending_events().zip(requests) {
        if pending.sequence != request.sequence.get() {
            return Err(Error::Input(
                "requests must name the exact existing ordered journal prefix",
            ));
        }
        let instance = world.instance(world.handle(pending.instance)?)?;
        if instance.owner()
            != &(Owner::Fragment {
                activation: request.activation,
            })
        {
            return Err(Error::Input(
                "explicit fragment activation differs from the saved owner",
            ));
        }
    }
    let mut counts = Counts {
        events: 0,
        source_instructions: 0,
        statement_bytes: 0,
        trace_projection: preparation::ObservationCounts {
            source_bytes: 0,
            rows: 0,
            variable_bytes: 0,
            binding_uses: 0,
        },
    };
    let mut remaining = limits.trace_projection;
    let mut committed = Vec::with_capacity(requests.len());
    for (event_index, request) in requests.iter().enumerate() {
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
        let projection = preparation::EventObservation::capture(&frame, &selected, remaining)?;
        let admitted = projection.counts;
        let instructions = frame.instructions().len();
        // Temporary projection is not retained alongside the old trace. Its
        // source/context/value admission is a conservative superset of that
        // trace's accepted own-number payload, not a peak-heap measurement.
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
        match copy_probe::commit_pending(
            &mut world,
            sources,
            content,
            request.sequence.get(),
            request.activation,
            intent,
            local_copy::Limits {
                maximum_event_instructions: instructions,
                maximum_operand_uses: 2,
                maximum_statement_bytes: limits.maximum_statement_bytes,
            },
        )? {
            copy_probe::PendingOutcome::EngineeringCommitted { committed: copy } => {
                committed.push(*copy)
            }
            copy_probe::PendingOutcome::Unsupported { reason, detail } => {
                return Ok(Outcome::Unsupported {
                    event_index: Some(event_index),
                    reason,
                    detail,
                });
            }
        }
        counts.events += 1;
    }
    Ok(Outcome::EngineeringCommitted {
        result: Box::new(Completed {
            snapshot: world.snapshot(),
            committed,
            counts,
        }),
    })
}
