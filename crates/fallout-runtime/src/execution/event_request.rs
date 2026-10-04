//! Explicit engineering append of one exact source block to an existing owner.
//! This host operation neither executes the block nor infers retail dispatch.
use super::local_copy;
use crate::{
    World,
    events::{Clocks, Context, Trigger},
    foreign::Content,
    identity::{CampaignId, InstanceId, Owner, ReferenceValue},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use fallout_data::{
    loaded_scripts::{Handle, Version},
    store::SourceReceipt,
};
pub use local_copy::Intent;
use serde::Serialize;
use std::{io::Write, ops::Range};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Selection<'a> {
    pub instance: InstanceId,
    pub expected_owner: &'a Owner,
    pub definition: &'a Handle,
    pub begin_scda_offset: u32,
    pub event_id: u16,
    pub intent: Intent,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_source_bytes: usize,
    pub maximum_source_instructions: usize,
    pub maximum_context_arguments: usize,
    pub maximum_variable_bytes: usize,
    pub maximum_source_receipts: usize,
    pub maximum_trace_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_source_bytes: 1024 * 1024,
            maximum_source_instructions: 4096,
            maximum_context_arguments: 64,
            maximum_variable_bytes: 1024 * 1024,
            maximum_source_receipts: 256,
            maximum_trace_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Counts {
    pub source_bytes: usize,
    pub instructions: usize,
    pub context_arguments: usize,
    pub variable_bytes: usize,
    pub source_receipts: usize,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("saved event request is invalid: {0}")]
    Input(&'static str),
    #[error("saved event request budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Source(#[from] crate::programs::LookupError),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
}
#[derive(Debug, Serialize)]
pub struct Trace<'a> {
    pub campaign: CampaignId,
    pub before_revision: u64,
    pub clocks: Clocks,
    pub existing_pending_rows: usize,
    pub selection: Selection<'a>,
    pub event_scda_bytes: Range<usize>,
    pub context: &'a Context,
    pub version: &'a Version,
    pub source_cohort_sha256: &'a str,
    pub decoder_sha256: &'a str,
    pub source_receipts: &'a [SourceReceipt],
    pub counts: Counts,
    pub original_dispatch_verified: bool,
}
/// Borrows explicit bounded inputs and immutable source. No deserializer,
/// constructor, live mutable World access or persistent continuation token.
pub struct Request<'p, 's> {
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    trace: Trace<'p>,
}
pub enum Preparation<'p, 's> {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'static str,
    },
    Ready(Box<Request<'p, 's>>),
}
#[derive(Debug, Serialize)]
pub struct QueuedResult {
    pub sequence: u64,
    pub snapshot: Snapshot,
}
fn charge(used: &mut usize, size: usize, maximum: usize, kind: &'static str) -> Result<(), Error> {
    *used = used
        .checked_add(size)
        .filter(|&n| n <= maximum)
        .ok_or(Error::Capacity(kind))?;
    Ok(())
}
fn string_bytes(value: &ReferenceValue) -> usize {
    match value {
        ReferenceValue::Content { key } => key.origin_plugin.len(),
        _ => 0,
    }
}
struct TraceAdmission {
    bytes: usize,
    maximum: usize,
    exceeded: bool,
}
impl Write for TraceAdmission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            self.exceeded = true;
            return Err(std::io::Error::other(
                "saved event trace byte budget exceeded",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn prepare<'p, 's>(
    world: &World<'_>,
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    selection: Selection<'p>,
    context: &'p Context,
    limits: Limits,
) -> Result<Preparation<'p, 's>, Error> {
    if selection.intent == Intent::Faithful {
        return Ok(Preparation::Unsupported {
            reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            detail: "original event dispatch and event-list scheduling are unverified",
        });
    }
    let mut counts = Counts {
        context_arguments: context.arguments.len(),
        source_receipts: sources.catalogue().sources.len(),
        ..Default::default()
    };
    if counts.context_arguments > limits.maximum_context_arguments {
        return Err(Error::Capacity("context arguments"));
    }
    if counts.source_receipts > limits.maximum_source_receipts {
        return Err(Error::Capacity("source receipts"));
    }
    let owner_name = match selection.expected_owner {
        Owner::Quest { key } => key.origin_plugin.len(),
        _ => 0,
    };
    for size in [
        owner_name,
        selection.definition.key.record.origin_plugin.len(),
        selection.definition.version_sha256.len(),
        sources.source_cohort_sha256().len(),
        sources.decoder_sha256().len(),
    ] {
        charge(
            &mut counts.variable_bytes,
            size,
            limits.maximum_variable_bytes,
            "variable bytes",
        )?;
    }
    for receipt in &sources.catalogue().sources {
        for size in [receipt.source_name.len(), receipt.source_sha256.len(), 8] {
            charge(
                &mut counts.variable_bytes,
                size,
                limits.maximum_variable_bytes,
                "variable bytes",
            )?;
        }
    }
    for value in context.target.iter().chain(&context.arguments) {
        charge(
            &mut counts.variable_bytes,
            string_bytes(value),
            limits.maximum_variable_bytes,
            "variable bytes",
        )?;
    }
    sources.validate_world(world)?;
    content.validate_world(world)?;
    let instance = world.instance(world.handle(selection.instance)?)?;
    if instance.owner() != selection.expected_owner || instance.definition() != selection.definition
    {
        return Err(Error::Input(
            "explicit owner or definition differs from current instance",
        ));
    }
    let source = sources.get(selection.definition)?.plan();
    counts.source_bytes = source.control().bytes().len();
    if counts.source_bytes > limits.maximum_source_bytes {
        return Err(Error::Capacity("source bytes"));
    }
    let version = source.source().version();
    for size in [
        version.source_plugin.len(),
        version.source_sha256.len(),
        version.decoded_record_sha256.len(),
        version.metadata_sha256.len(),
        version.compiled_sha256.as_ref().map_or(0, String::len),
    ] {
        charge(
            &mut counts.variable_bytes,
            size,
            limits.maximum_variable_bytes,
            "variable bytes",
        )?;
    }
    let event = source
        .event_at_scda_offset(selection.begin_scda_offset as usize)
        .filter(|e| e.event_id == selection.event_id)
        .ok_or(Error::Input(
            "exact source event offset and descriptor do not match",
        ))?;
    counts.instructions = event
        .end_instruction
        .checked_sub(event.begin_instruction)
        .and_then(|n| n.checked_add(1))
        .ok_or(Error::Input("invalid source event span"))?;
    if counts.instructions > limits.maximum_source_instructions {
        return Err(Error::Capacity("source instructions"));
    }
    let instructions = source.control().instructions();
    let trace = Trace {
        campaign: world.campaign(),
        before_revision: world.revision(),
        clocks: world.clocks(),
        existing_pending_rows: world.pending_events().len(),
        selection,
        event_scda_bytes: instructions[event.begin_instruction].bytes.start
            ..instructions[event.end_instruction].bytes.end,
        context,
        version,
        source_cohort_sha256: sources.source_cohort_sha256(),
        decoder_sha256: sources.decoder_sha256(),
        source_receipts: &sources.catalogue().sources,
        counts,
        original_dispatch_verified: false,
    };
    let mut admission = TraceAdmission {
        bytes: 0,
        maximum: limits.maximum_trace_bytes,
        exceeded: false,
    };
    let encoded = serde_json::to_writer(&mut admission, &trace);
    if admission.exceeded {
        return Err(Error::Capacity("trace bytes"));
    }
    encoded?;
    // Existing canonical validation may allocate canonical form names. All
    // strings and complete trace work were admitted before this boundary.
    world.validate_owner(selection.expected_owner)?;
    world.validate_context(context)?;
    Ok(Preparation::Ready(Box::new(Request {
        sources,
        content,
        trace,
    })))
}
impl Request<'_, '_> {
    pub fn trace(&self) -> &Trace<'_> {
        &self.trace
    }
    /// No caller-owned live World is mutated. Even enqueue capacity failure
    /// discards this consumed input's restored private result.
    pub fn apply(&self, snapshot: Snapshot, limits: crate::Limits) -> Result<QueuedResult, Error> {
        if snapshot.campaign != self.trace.campaign
            || snapshot.state_revision != self.trace.before_revision
            || snapshot.clocks != self.trace.clocks
            || snapshot.pending_events.len() != self.trace.existing_pending_rows
        {
            return Err(Error::Input(
                "supplied snapshot changed since event request admission",
            ));
        }
        let mut result = World::restore(self.sources.catalogue(), snapshot, limits)?;
        self.sources.validate_world(&result)?;
        self.content.validate_world(&result)?;
        let selection = self.trace.selection;
        let handle = result.handle(selection.instance)?;
        let instance = result.instance(handle)?;
        if instance.owner() != selection.expected_owner
            || instance.definition() != selection.definition
        {
            return Err(Error::Input(
                "private current instance owner or definition changed",
            ));
        }
        let sequence = result.enqueue(
            handle,
            Trigger::Block {
                event_id: selection.event_id,
                begin_byte_offset: selection.begin_scda_offset,
            },
            self.trace.context.clone(),
        )?;
        Ok(QueuedResult {
            sequence,
            snapshot: result.snapshot(),
        })
    }
}
