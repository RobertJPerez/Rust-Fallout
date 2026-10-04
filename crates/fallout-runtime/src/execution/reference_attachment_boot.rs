//! Explicit engineering boot of one existing authored reference's script.
//! Its source attachment is never inferred to be a retail current event list.
use super::{attachment_boot, local_copy};
use crate::{
    World,
    foreign::Content,
    identity::{Owner, ReferenceId},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use fallout_data::{
    identity::FormKey,
    loaded_scripts::{Handle, Version},
    plugin, script_reference_attachment,
};
pub use local_copy::Intent;
pub type Limits = attachment_boot::Limits;
pub type Counts = attachment_boot::Counts;
pub type BootResult = attachment_boot::BootResult;

#[derive(Debug, Clone, Copy)]
pub struct Selection<'a> {
    pub reference: ReferenceId,
    pub expected_authored_key: &'a FormKey,
    pub intent: Intent,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("placed script boot input is invalid: {0}")]
    Input(&'static str),
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
/// Borrows a sealed fresh source request and explicitly bounded initialization.
/// Mutation remains confined to a consumed snapshot's private restored World.
pub struct BootPlan<'p, 's> {
    sources: &'p PreparedSources<'s>,
    attachment: &'p script_reference_attachment::Request<'s>,
    content: &'p Content,
    selection: Selection<'p>,
    request: &'p attachment_boot::Request,
    counts: Counts,
}
pub fn prepare<'p, 's>(
    sources: &'p PreparedSources<'s>,
    attachment: &'p script_reference_attachment::Request<'s>,
    content: &'p Content,
    selection: Selection<'p>,
    request: &'p attachment_boot::Request,
    limits: Limits,
) -> Result<Preparation<'p, 's>, Error> {
    if selection.intent == Intent::Faithful {
        return Ok(Preparation::Unsupported {
            reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            detail: "original placed-script activation and event-list lifetime are unverified",
        });
    }
    let mut counts = attachment_boot::prepare_input(
        sources,
        attachment.source_receipts(),
        selection.expected_authored_key,
        request,
        limits,
    )?;
    if selection.expected_authored_key != &attachment.proof().placement.key {
        return Err(Error::Input(
            "selected authored key differs from the sealed placement",
        ));
    }
    if attachment.catalogue().winning_content_sha256()
        != sources.catalogue().winning_content_sha256()
    {
        return Err(Error::Input(
            "sealed attachment winning headers differ from prepared sources",
        ));
    }
    let validation =
        World::with_campaign(sources.catalogue(), Default::default(), request.campaign)?;
    content.validate_world(&validation)?;
    for proof in [
        &attachment.proof().placement,
        &attachment.proof().base,
        &attachment.proof().script,
    ] {
        let form = content.source_form(&validation, &proof.key)?;
        if form.kind != proof.header.kind
            || form.flags != proof.header.flags
            || form.flags & plugin::DELETED != 0
        {
            return Err(Error::Input(
                "sealed attachment differs from current source content",
            ));
        }
    }
    counts.declarations =
        attachment_boot::prepare_definition(sources, attachment.definition(), limits)?;
    let current = sources
        .catalogue()
        .get_handle(attachment.definition())
        .ok_or(crate::Error::DefinitionChanged)?;
    // Full-source and winning-header checks plus the exact immutable handle bind
    // the metadata/compiled bytes; these comparisons bind its retained source.
    let version = current.version();
    let sealed = attachment.script_version();
    if version.source_plugin != sealed.source_plugin
        || version.source_sha256 != sealed.source_sha256
        || version.record_file_offset != sealed.record_file_offset
        || version.record_flags != sealed.record_flags
        || version.decoded_record_sha256 != sealed.decoded_record_sha256
        || version.metadata_sha256 != sealed.metadata_sha256
        || version.compiled_sha256 != sealed.compiled_sha256
        || version.compiled_bytes != sealed.compiled_bytes
    {
        return Err(Error::Input(
            "selected retained script version differs from sealed attachment",
        ));
    }
    Ok(Preparation::Ready(BootPlan {
        sources,
        attachment,
        content,
        selection,
        request,
        counts,
    }))
}
impl BootPlan<'_, '_> {
    pub fn attachment(&self) -> &script_reference_attachment::Request<'_> {
        self.attachment
    }
    pub fn definition(&self) -> &Handle {
        self.attachment.definition()
    }
    pub fn script_version(&self) -> &Version {
        self.attachment.script_version()
    }
    pub fn counts(&self) -> Counts {
        self.counts
    }
    pub fn source_cohort_sha256(&self) -> &str {
        self.sources.source_cohort_sha256()
    }
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
        if result.reference_origin(self.selection.reference)?
            != Some(self.selection.expected_authored_key)
            || result.authored_reference(self.selection.expected_authored_key)
                != Some(self.selection.reference)
        {
            return Err(Error::Input(
                "selected reference origin and inverse authored mapping differ",
            ));
        }
        let owner = Owner::Placed {
            reference: self.selection.reference,
        };
        if result.owner_instance(&owner).is_some() {
            return Err(Error::Input(
                "selected placed owner already has a script instance",
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
