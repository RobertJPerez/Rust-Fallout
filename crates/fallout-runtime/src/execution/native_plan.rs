//! Private source-bound engineering query authority created from current native
//! admission. Historical diagnostic data alone is never accepted as a plan.
use super::native;
use crate::{
    World,
    foreign::Content,
    identity::{ReferenceId, ReferenceValue, Value},
    preparation,
    programs::PreparedSources,
    query,
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub native: native::Limits,
    pub source_projection: preparation::ObservationLimits,
    /// Retained decoder/cohort strings and both copies of the resolved item key.
    pub maximum_query_variable_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            native: native::Limits {
                maximum_event_instructions: 4096,
                maximum_calls: 128,
                maximum_argument_bytes: 65_539,
            },
            source_projection: preparation::ObservationLimits::default(),
            maximum_query_variable_bytes: 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Native(#[from] native::Error),
    #[error(transparent)]
    Projection(#[from] preparation::ObservationError),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error("native query plan context changed: {0}")]
    ContextChanged(&'static str),
    #[error("native query plan budget exceeded: {0}")]
    Capacity(&'static str),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallProof {
    pub command_id: u16,
    pub instruction_index: usize,
    pub scda_bytes: Range<usize>,
    pub instruction_scda_bytes: Range<usize>,
    pub argument_scda_bytes: Range<usize>,
    pub calling_reference_index: Option<u16>,
    pub location: native::Location,
}
impl CallProof {
    fn capture(call: &native::Call<'_>) -> Self {
        Self {
            command_id: call.command_id,
            instruction_index: call.instruction_index,
            scda_bytes: call.scda_bytes.clone(),
            instruction_scda_bytes: call.instruction_scda_bytes.clone(),
            argument_scda_bytes: call.argument_scda_offset
                ..call.argument_scda_offset + call.raw_arguments.len(),
            calling_reference_index: call.calling_reference_index,
            location: call.location.clone(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Counts {
    pub source: preparation::ObservationCounts,
    pub query_variable_bytes: usize,
}
pub enum Preparation {
    Unsupported {
        reason: native::Unsupported,
        detail: String,
    },
    Ready(Box<Plan>),
}
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub sequence: u64,
    pub occurrence: usize,
    pub inputs: native::Inputs,
    pub intent: native::Intent,
}
/// Neither Serialize nor Deserialize, no public fields/constructor/commit. Its
/// retained query request owns persistent identities, never World/epoch borrows.
pub struct Plan {
    source: preparation::EventObservation,
    decoder_sha256: String,
    call: CallProof,
    occurrence: usize,
    inputs: native::Inputs,
    subject: ReferenceId,
    item: FormKey,
    query: query::Request,
    limits: Limits,
    counts: Counts,
}
impl Plan {
    pub fn source(&self) -> &preparation::EventObservation {
        &self.source
    }
    pub fn call(&self) -> &CallProof {
        &self.call
    }
    pub fn subject(&self) -> ReferenceId {
        self.subject
    }
    pub fn item(&self) -> &FormKey {
        &self.item
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn occurrence(&self) -> usize {
        self.occurrence
    }
    pub fn supplied_subject(&self) -> Option<ReferenceId> {
        self.inputs.supplied_subject
    }
    pub fn explicit_player(&self) -> Option<ReferenceId> {
        self.inputs.player
    }

    pub fn observe(
        &self,
        world: &World<'_>,
        sources: &PreparedSources<'_>,
        content: &Content,
        maximum_contributions: usize,
    ) -> Result<native::Outcome, Error> {
        if world.campaign() != self.source.campaign
            || world.catalogue_fingerprint() != self.source.catalogue_sha256
            || sources.decoder_sha256() != self.decoder_sha256
        {
            return Err(Error::ContextChanged("campaign, full source or decoder"));
        }
        if world.pending_events().next() != Some(&self.source.pending) {
            return Err(Error::ContextChanged("existing journal head"));
        }
        content.validate_world(world)?;
        let calls = world.prepare_native_calls_with_sources(
            self.source.pending.sequence,
            sources,
            self.limits.native,
        )?;
        let frame = calls.frame();
        if frame.source().handle() != &self.source.definition
            || frame.binding_sha256().as_ref() != self.source.full_definition_binding_sha256
            || frame.selected() != &self.source.selected
            || frame.instance().owner() != &self.source.instance_owner
            || frame.instance().context() != &self.source.instance_context
        {
            return Err(Error::ContextChanged(
                "definition, binding, event or owner context",
            ));
        }
        let call = calls
            .calls()
            .get(self.occurrence)
            .ok_or(native::Error::MissingCall(self.occurrence))?;
        if CallProof::capture(call) != self.call {
            return Err(Error::ContextChanged("exact source call"));
        }
        let argument = self.call.argument_scda_bytes.start - self.source.begin_scda_offset
            ..self.call.argument_scda_bytes.end - self.source.begin_scda_offset;
        if call.raw_arguments != &self.source.source_bytes[argument] {
            return Err(Error::ContextChanged("source argument bytes"));
        }
        match calls.admit_occurrence(
            self.occurrence,
            self.inputs,
            native::Intent::EngineeringObservation,
            self.limits.maximum_query_variable_bytes,
        )? {
            native::Admission::Unsupported { reason, detail } => {
                return Ok(native::Outcome::Unsupported { reason, detail });
            }
            native::Admission::Resolved(resolved) => {
                let Value::Reference {
                    value: ReferenceValue::Content { key },
                } = &resolved.argument
                else {
                    return Err(Error::ContextChanged("resolved argument kind"));
                };
                if resolved.subject != self.subject || key != &self.item {
                    return Err(Error::ContextChanged("fresh resolved caller or item"));
                }
            }
        }
        Ok(
            match self.query.evaluate(world, content, maximum_contributions) {
                Ok(trace) => native::Outcome::EngineeringObservation {
                    trace: Box::new(trace),
                },
                Err(query::Failure::UnverifiedFormList) => native::Outcome::Unsupported {
                    reason: native::Unsupported::UnverifiedFormList,
                    detail: "Original GetItemCount form-list expansion is unverified".into(),
                },
                Err(query::Failure::State(crate::Error::Capacity(_))) => {
                    return Err(Error::Capacity("query contributions"));
                }
                Err(error) => native::Outcome::Unsupported {
                    reason: native::Unsupported::HostQueryUnavailable,
                    detail: error.to_string(),
                },
            },
        )
    }
}

/// Only current source-native admission creates a reusable live query plan.
/// No query executes here; a missing inventory can become available later.
pub fn prepare(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    selection: Selection,
    limits: Limits,
) -> Result<Preparation, Error> {
    let Selection {
        sequence,
        occurrence,
        inputs,
        intent,
    } = selection;
    if world
        .pending_events()
        .next()
        .is_none_or(|pending| pending.sequence != sequence)
    {
        return Err(Error::ContextChanged("existing journal head"));
    }
    content.validate_world(world)?;
    let calls = world.prepare_native_calls_with_sources(sequence, sources, limits.native)?;
    let resolved = match calls.admit_occurrence(
        occurrence,
        inputs,
        intent,
        limits.maximum_query_variable_bytes,
    )? {
        native::Admission::Unsupported { reason, detail } => {
            return Ok(Preparation::Unsupported { reason, detail });
        }
        native::Admission::Resolved(resolved) => resolved,
    };
    let Value::Reference {
        value: ReferenceValue::Content { key },
    } = &resolved.argument
    else {
        return Err(Error::ContextChanged("resolved argument kind"));
    };
    let query_variable_bytes = key
        .origin_plugin
        .len()
        .checked_mul(2)
        .and_then(|size| size.checked_add(world.catalogue_fingerprint().len()))
        .and_then(|size| size.checked_add(sources.decoder_sha256().len()))
        .filter(|&size| size <= limits.maximum_query_variable_bytes)
        .ok_or(Error::Capacity("query variable bytes"))?;
    // Shared admission bounded the resolver's transient key clone. Charge both
    // retained query/item copies and decoder/cohort strings before retaining.
    let source =
        preparation::EventObservation::capture(calls.frame(), &[], limits.source_projection)?;
    let query = match query::Request::prepare(
        world,
        query::Entry::Native {
            command_id: calls.calls()[occurrence].command_id,
        },
        Some(resolved.subject),
        std::slice::from_ref(&resolved.argument),
    ) {
        Ok(query) => query,
        Err(error) => {
            return Ok(Preparation::Unsupported {
                reason: native::Unsupported::HostQueryUnavailable,
                detail: error.to_string(),
            });
        }
    };
    let counts = Counts {
        source: source.counts,
        query_variable_bytes,
    };
    Ok(Preparation::Ready(Box::new(Plan {
        source,
        decoder_sha256: sources.decoder_sha256().into(),
        call: CallProof::capture(&calls.calls()[occurrence]),
        occurrence,
        inputs,
        subject: resolved.subject,
        item: key.clone(),
        query,
        limits,
        counts,
    })))
}
