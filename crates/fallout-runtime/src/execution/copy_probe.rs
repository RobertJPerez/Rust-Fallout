//! Standalone engineering own-local copy observations through canonical APIs.
//! This does not establish original initialization, conversions or scheduling.
use super::{local_copy, trace};
use crate::{
    World,
    events::{Context, Trigger},
    foreign::Content,
    identity::{CampaignId, Owner, Value},
    programs::PreparedSources,
    snapshot::Snapshot,
    state::InstanceHandle,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Initializer {
    pub index: u32,
    pub value: Value,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub campaign: CampaignId,
    pub activation: std::num::NonZeroU64,
    pub initializers: Vec<Initializer>,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_steps: usize,
    pub maximum_initializers: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_steps: 64,
            maximum_initializers: 128,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("standalone copy probe input is invalid: {0}")]
    Input(&'static str),
    #[error("standalone copy probe budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Trace(#[from] trace::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Copy(#[from] local_copy::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}
#[derive(Debug, Serialize)]
pub struct Unsupported {
    pub step: Option<usize>,
    pub detail: String,
}
#[derive(Debug, Serialize)]
pub struct Observation {
    pub capture: trace::Capture,
    pub unsupported: Option<Unsupported>,
    pub committed: Vec<local_copy::CommittedCopy>,
    pub initial_snapshot: Snapshot,
    pub final_snapshot: Snapshot,
    pub canonical_restore_verified: bool,
    pub faithful_execution_admitted: bool,
}
struct Completed {
    observations: Vec<trace::Step>,
    committed: Vec<local_copy::CommittedCopy>,
}

/// Result of consuming an existing pending event. Unsupported is a refusal,
/// with no assignment, journal acknowledgement or replacement state.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PendingOutcome {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: String,
    },
    EngineeringCommitted {
        committed: Box<local_copy::CommittedCopy>,
    },
}

/// Consume only an already restored fragment's journal head. The host must
/// name its activation and intent explicitly. This never seeds storage/events;
/// the existing adapter supplies all source admission and commit authority.
pub fn commit_pending(
    world: &mut World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    sequence: u64,
    activation: std::num::NonZeroU64,
    intent: local_copy::Intent,
    limits: local_copy::Limits,
) -> Result<PendingOutcome, Error> {
    let pending = world
        .pending_events()
        .next()
        .filter(|event| event.sequence == sequence)
        .ok_or(Error::Input(
            "copy must name the existing pending journal head",
        ))?;
    let instance = world.instance(world.handle(pending.instance)?)?;
    if instance.owner() != &(Owner::Fragment { activation }) {
        return Err(Error::Input(
            "explicit fragment activation differs from the saved owner",
        ));
    }
    match world.stage_source_local_copy_with_sources(sequence, sources, content, intent, limits)? {
        local_copy::Preparation::Unsupported { reason, detail } => {
            Ok(PendingOutcome::Unsupported { reason, detail })
        }
        local_copy::Preparation::Staged(stage) => Ok(PendingOutcome::EngineeringCommitted {
            committed: Box::new(stage.commit(world)?),
        }),
    }
}

fn world_limits(limits: Limits) -> crate::Limits {
    crate::Limits {
        max_instances: 1,
        max_locals: limits.maximum_initializers,
        max_pending_events: 1,
        max_event_blocks: 64,
        max_snapshot_bytes: 1024 * 1024,
        ..Default::default()
    }
}
fn seeded<'a>(
    sources: &PreparedSources<'a>,
    manifest: &trace::Manifest,
    request: &Request,
    limits: Limits,
) -> Result<(World<'a>, InstanceHandle), Error> {
    let mut world =
        World::with_campaign(sources.catalogue(), world_limits(limits), request.campaign)?;
    let handle = world.create_instance(
        &manifest.identity.definition,
        Owner::Fragment {
            activation: request.activation,
        },
        Context::default(),
    )?;
    let assignments: Vec<_> = request
        .initializers
        .iter()
        .map(|entry| (entry.index, entry.value.clone()))
        .collect();
    world.assign(handle, &assignments)?;
    Ok((world, handle))
}

// This uses the same adapter for preview and committed observation. An entire
// private preview is discarded on failure; no partially executed snapshot is
// returned as the requested result and no caller-owned world is ever mutated.
fn steps(
    world: &mut World<'_>,
    handle: InstanceHandle,
    sources: &PreparedSources<'_>,
    content: &Content,
    manifest: &trace::Manifest,
) -> Result<Result<Completed, Unsupported>, Error> {
    let mut observations = Vec::with_capacity(manifest.steps.len());
    let mut committed = Vec::with_capacity(manifest.steps.len());
    for (index, input) in manifest.steps.iter().enumerate() {
        let sequence = world.enqueue(
            handle,
            Trigger::Block {
                event_id: input.event_id,
                begin_byte_offset: input.begin_scda_offset,
            },
            Context::default(),
        )?;
        let stage = match world.stage_source_local_copy_with_sources(
            sequence,
            sources,
            content,
            local_copy::Intent::Engineering,
            local_copy::Limits {
                maximum_event_instructions: 3,
                ..Default::default()
            },
        )? {
            local_copy::Preparation::Staged(stage) => stage,
            local_copy::Preparation::Unsupported { reason, detail } => {
                return Ok(Err(Unsupported {
                    step: Some(index),
                    detail: format!("{reason:?}: {detail}"),
                }));
            }
        };
        let source = stage.trace();
        let Value::Number { bits } = &source.copied_value else {
            return Err(Error::Input("copy adapter did not observe a Number"));
        };
        if input.scda_offset as usize != source.statement_scda_bytes.start
            || input.operands != [trace::Word::binary64(*bits)]
        {
            return Ok(Err(Unsupported{step:Some(index),detail:"Manifest source site/operand bits differ from the actual canonical copy observation".into()}));
        }
        let output = trace::StepOutput {
            return_value: None,
            successor_scda_offset: Some(
                source
                    .statement_scda_bytes
                    .end
                    .try_into()
                    .map_err(|_| Error::Capacity("source offsets"))?,
            ),
            writes: vec![trace::LocalWrite {
                index: source.destination_index,
                value: trace::Word::binary64(*bits),
            }],
            error: None,
        };
        committed.push(stage.commit(world)?);
        observations.push(trace::Step {
            input: input.clone(),
            output,
        });
    }
    Ok(Ok(Completed {
        observations,
        committed,
    }))
}

pub fn observe(
    sources: &PreparedSources<'_>,
    content: &Content,
    manifest: &trace::Manifest,
    request: &Request,
    producer_executable_sha256: &str,
    transport_receipt_sha256: &str,
    limits: Limits,
) -> Result<Observation, Error> {
    trace::validate_manifest(sources, manifest, Default::default())?;
    if request.schema_version != 1 {
        return Err(Error::Input("schema version"));
    }
    if manifest.steps.len() > limits.maximum_steps {
        return Err(Error::Capacity("steps"));
    }
    if request.initializers.len() > limits.maximum_initializers {
        return Err(Error::Capacity("initializers"));
    }
    let mut assigned = BTreeSet::new();
    for entry in &request.initializers {
        if !assigned.insert(entry.index) {
            return Err(Error::Input("duplicate local initializer"));
        }
        let Value::Number { bits } = &entry.value else {
            return Err(Error::Input(
                "only explicit numeric initializer bits are supported",
            ));
        };
        if !f64::from_bits(*bits).is_finite() {
            return Err(Error::Input("initializer must be finite"));
        }
    }
    if producer_executable_sha256 == manifest.identity.executable_sha256 {
        return Err(Error::Input(
            "replacement producer cannot be the original executable",
        ));
    }
    let mut capture=trace::Capture{schema_version:1,identity:manifest.identity.clone(),producer:trace::Producer::Replacement,
        producer_executable_sha256:producer_executable_sha256.into(),transport_receipt_sha256:transport_receipt_sha256.into(),
        instrumentation:"Standalone engineering own-local bit-copy through canonical staging/commit. Preview uses a discarded private world. No original scheduling, conversion, initialization or behavior claim.".into(),
        finish:trace::Finish::Unsupported,steps:Vec::new()};
    // Validate all supplied provenance before initializing either private world.
    trace::compare(sources, manifest, None, Some(&capture), Default::default())?;
    let (mut world, handle) = seeded(sources, manifest, request, limits)?;
    content.validate_world(&world)?;
    let initial = world.snapshot();
    let unsupported = if manifest.purpose != trace::Operation::Assignment
        || manifest.steps.iter().any(|input| {
            input.operation != trace::Operation::Assignment
                || input.caller.activation != request.activation.get()
                || input.caller.calling_reference.is_some()
                || input.caller.containing_reference.is_some()
                || input.caller.target.is_some()
        }) {
        Some(Unsupported{step:None,detail:"Only explicit own-scope engineering assignment copies are supported; conversion, branch, native and external caller behavior are unmeasured".into()})
    } else if let Some(step) = manifest
        .steps
        .iter()
        .enumerate()
        .position(|(index, input)| input.event_ordinal as usize != index)
    {
        Some(Unsupported {
            step: Some(step),
            detail: "Standalone copy steps execute complete events and require consecutive distinct event ordinals starting at zero".into(),
        })
    } else {
        let (mut preview, preview_handle) = seeded(sources, manifest, request, limits)?;
        steps(&mut preview, preview_handle, sources, content, manifest)?.err()
    };
    let completed = if unsupported.is_none() {
        steps(&mut world, handle, sources, content, manifest)?
            .map_err(|_| Error::Input("fixed-source preview and execution diverged"))?
    } else {
        Completed {
            observations: Vec::new(),
            committed: Vec::new(),
        }
    };
    let final_snapshot = world.snapshot();
    let restored = World::restore(
        sources.catalogue(),
        final_snapshot.clone(),
        world_limits(limits),
    )?;
    let canonical_restore_verified = restored.snapshot() == final_snapshot;
    if !canonical_restore_verified {
        return Err(Error::Input("canonical restore changed probe state"));
    }
    capture.finish = if unsupported.is_some() {
        trace::Finish::Unsupported
    } else {
        trace::Finish::Completed
    };
    capture.steps = completed.observations;
    // Validate receipt/word structure even when an original capture is absent.
    trace::compare(sources, manifest, None, Some(&capture), Default::default())?;
    Ok(Observation {
        capture,
        unsupported,
        committed: completed.committed,
        initial_snapshot: initial,
        final_snapshot,
        canonical_restore_verified,
        faithful_execution_admitted: false,
    })
}
