//! Explicit engineering activation of one exact embedded source unit.
//! Physical ownership receipts do not establish Original fragment triggers.
use super::{attachment_boot, local_copy};
use crate::{
    World, foreign::Content, identity::Owner, programs::PreparedSources, snapshot::Snapshot,
};
use fallout_data::{
    loaded_scripts::{Handle, LoadedScript, OwnerKind, Version},
    plugin,
};
use serde::Serialize;
use std::num::NonZeroU64;

pub type Request = attachment_boot::Request;
pub type BootResult = attachment_boot::BootResult;
pub use local_copy::Intent;

#[derive(Debug, Clone, Copy)]
pub struct Selection<'a> {
    pub definition: &'a Handle,
    pub activation: NonZeroU64,
    pub intent: Intent,
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub input: attachment_boot::Limits,
    /// Conservative selected input/source name copies, not peak heap usage.
    pub maximum_retained_variable_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input: Default::default(),
            maximum_retained_variable_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Counts {
    pub input: attachment_boot::Counts,
    pub retained_variable_bytes: usize,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("embedded fragment boot input is invalid: {0}")]
    Input(&'static str),
    #[error("embedded fragment boot budget exceeded: {0}")]
    Capacity(&'static str),
    #[error(transparent)]
    Initialization(#[from] attachment_boot::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}
pub enum Preparation<'p, 's> {
    Unsupported {
        reason: local_copy::Unsupported,
        detail: &'static str,
    },
    Ready(BootPlan<'p, 's>),
}
/// Borrowed immutable authority; serialized diagnostics never reconstruct it.
pub struct BootPlan<'p, 's> {
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    script: &'p LoadedScript,
    activation: NonZeroU64,
    request: &'p Request,
    counts: Counts,
}

pub fn prepare<'p, 's>(
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    selection: Selection<'p>,
    request: &'p Request,
    limits: Limits,
) -> Result<Preparation<'p, 's>, Error> {
    if selection.intent == Intent::Faithful {
        return Ok(Preparation::Unsupported {
            reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            detail: "Original embedded fragment activation, initialization and trigger semantics are unverified",
        });
    }
    let expected_bytes = selection
        .definition
        .key
        .record
        .origin_plugin
        .len()
        .checked_add(selection.definition.version_sha256.len())
        .filter(|&n| n <= limits.input.maximum_variable_bytes)
        .ok_or(Error::Capacity("input variable bytes"))?;
    let script = sources
        .catalogue()
        .get_handle(selection.definition)
        .ok_or(crate::Error::DefinitionChanged)?;
    if script.owner().kind == OwnerKind::Standalone {
        return Err(Error::Input(
            "selected definition is standalone rather than embedded",
        ));
    }
    let version = script.version();
    let actual_handle_bytes = script
        .handle()
        .key
        .record
        .origin_plugin
        .len()
        .checked_add(script.handle().version_sha256.len())
        .ok_or(Error::Capacity("retained variable bytes"))?;
    let source_bytes = [
        version.source_plugin.as_str(),
        version.source_sha256.as_str(),
        version.decoded_record_sha256.as_str(),
        version.metadata_sha256.as_str(),
        version.compiled_sha256.as_deref().unwrap_or(""),
    ]
    .into_iter()
    .try_fold(0usize, |n, value| n.checked_add(value.len()))
    .ok_or(Error::Capacity("retained variable bytes"))?;
    let fixed = actual_handle_bytes
        .checked_mul(8)
        .and_then(|n| source_bytes.checked_mul(4).and_then(|m| n.checked_add(m)))
        .and_then(|n| n.checked_add(sources.source_cohort_sha256().len()))
        .and_then(|n| n.checked_add(sources.decoder_sha256().len()))
        .and_then(|n| n.checked_add(256))
        .filter(|&n| n <= limits.maximum_retained_variable_bytes)
        .ok_or(Error::Capacity("retained variable bytes"))?;
    let admitted_input = limits
        .input
        .maximum_variable_bytes
        .min((limits.maximum_retained_variable_bytes - fixed) / 6);
    let remaining_input = admitted_input
        .checked_sub(expected_bytes)
        .ok_or(Error::Capacity("retained input variable bytes"))?;
    // Existing preflight charges initializer/context/receipt extents before
    // canonical name validation. Restrict it with the remaining global bound.
    let mut input = attachment_boot::prepare_input(
        sources,
        &sources.catalogue().sources,
        &selection.definition.key.record,
        request,
        attachment_boot::Limits {
            maximum_variable_bytes: remaining_input,
            ..limits.input
        },
    )?;
    input.variable_bytes = input
        .variable_bytes
        .checked_add(expected_bytes)
        .ok_or(Error::Capacity("input variable bytes"))?;
    input.declarations =
        attachment_boot::prepare_definition(sources, script.handle(), limits.input)?;
    let retained_variable_bytes = input
        .variable_bytes
        .checked_mul(6)
        .and_then(|n| n.checked_add(fixed))
        .ok_or(Error::Capacity("retained variable bytes"))?;
    let validation =
        World::with_campaign(sources.catalogue(), Default::default(), request.campaign)?;
    content.validate_world(&validation)?;
    let form = content.source_form(&validation, &script.handle().key.record)?;
    if form.flags & plugin::DELETED != 0
        || form.flags != version.record_flags
        || form.kind == *b"SCPT"
    {
        return Err(Error::Input(
            "selected embedded unit differs from current nondeleted containing form",
        ));
    }
    Ok(Preparation::Ready(BootPlan {
        sources,
        content,
        script,
        activation: selection.activation,
        request,
        counts: Counts {
            input,
            retained_variable_bytes,
        },
    }))
}
impl BootPlan<'_, '_> {
    pub fn definition(&self) -> &Handle {
        self.script.handle()
    }
    pub fn script_version(&self) -> &Version {
        self.script.version()
    }
    pub fn script_owner(&self) -> &fallout_data::loaded_scripts::Owner {
        self.script.owner()
    }
    pub fn counts(&self) -> Counts {
        self.counts
    }
    pub fn source_cohort_sha256(&self) -> &str {
        self.sources.source_cohort_sha256()
    }
    /// All changes occur in one private restored result; failure discards it.
    pub fn apply(&self, snapshot: Snapshot, limits: crate::Limits) -> Result<BootResult, Error> {
        if snapshot.campaign != self.request.campaign {
            return Err(Error::Input(
                "initializer campaign differs from the supplied snapshot",
            ));
        }
        let mut result = World::restore(self.sources.catalogue(), snapshot, limits)?;
        self.sources
            .validate_world(&result)
            .map_err(attachment_boot::Error::from)?;
        self.content.validate_world(&result)?;
        let owner = Owner::Fragment {
            activation: self.activation,
        };
        if result.owner_instance(&owner).is_some() {
            return Err(Error::Input(
                "selected fragment activation already has a script instance",
            ));
        }
        let instance =
            attachment_boot::initialize(&mut result, self.definition(), owner, self.request)?;
        Ok(BootResult {
            instance,
            snapshot: result.snapshot(),
        })
    }
}
