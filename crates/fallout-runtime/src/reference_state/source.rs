//! Header-admitted, explicit host initialization. The world caller still owns
//! containing-cell membership and source enable-parent semantics.
use super::{Pose, State, View};
use crate::{
    Error, Result, World,
    foreign::{Content, SourceForm},
    identity::{CampaignId, ReferenceId},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::{mem::size_of, num::NonZeroU64};

#[derive(Debug, Clone, Copy)]
pub struct SourceReferenceRequest<'a> {
    pub authored: &'a FormKey,
    pub allowed_kinds: &'a [[u8; 4]],
    pub cell: &'a FormKey,
    pub pose: &'a Pose,
    pub enabled: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct SourceReferenceLimits {
    pub max_allowed_kinds: usize,
    /// Fixed owned stage and UTF-8 strings. Excludes allocator overhead and
    /// commit peak memory; no source bodies or allowed-kind array are cloned.
    pub max_copied_bytes: usize,
}
impl Default for SourceReferenceLimits {
    fn default() -> Self {
        Self {
            max_allowed_kinds: 6,
            max_copied_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceReferenceFailure {
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    State(#[from] Error),
    #[error("admitted source reference kind is not a placed kind: {0:?}")]
    InvalidAllowedKind([u8; 4]),
    #[error("source reference kind is not explicitly admitted: {0:?}")]
    ReferenceKind([u8; 4]),
    #[error("explicit source cell has a different kind: {0:?}")]
    CellKind([u8; 4]),
}

#[derive(Debug)]
#[must_use = "staging reserves no identity; commit or drop the proposal"]
pub struct StagedSourceReference {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    next_reference: u64,
    authored: FormKey,
    state: State,
    placed_source: SourceForm,
    cell_source: SourceForm,
    existing: Option<View>,
    charged_bytes: usize,
}
impl StagedSourceReference {
    pub fn authored(&self) -> &FormKey {
        &self.authored
    }
    pub fn requested_state(&self) -> &State {
        &self.state
    }
    pub fn existing(&self) -> Option<&View> {
        self.existing.as_ref()
    }
    pub fn charged_bytes(&self) -> usize {
        self.charged_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceReferenceOutcome {
    Created,
    Reused,
}

#[derive(Debug, Serialize)]
pub struct SourceReferenceReceipt {
    outcome: SourceReferenceOutcome,
    before_revision: u64,
    after_revision: u64,
    view: View,
    requested_cell: FormKey,
    placed_source: SourceForm,
    cell_source: SourceForm,
}
impl SourceReferenceReceipt {
    pub fn outcome(&self) -> SourceReferenceOutcome {
        self.outcome
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn view(&self) -> &View {
        &self.view
    }
    /// Validated caller selection. A reused state's retained cell may differ.
    pub fn requested_cell(&self) -> &FormKey {
        &self.requested_cell
    }
    pub fn placed_source(&self) -> SourceForm {
        self.placed_source
    }
    pub fn cell_source(&self) -> SourceForm {
        self.cell_source
    }
}

impl World<'_> {
    pub fn stage_source_reference(
        &self,
        request: SourceReferenceRequest<'_>,
        content: &Content,
        limits: SourceReferenceLimits,
    ) -> std::result::Result<StagedSourceReference, SourceReferenceFailure> {
        content.validate_world(self)?;
        if request.allowed_kinds.is_empty() {
            return Err(Error::Invalid("empty source reference kind set".into()).into());
        }
        if request.allowed_kinds.len() > limits.max_allowed_kinds {
            return Err(Error::Capacity("source reference kinds").into());
        }
        for (index, kind) in request.allowed_kinds.iter().enumerate() {
            if !(SourceForm {
                kind: *kind,
                flags: 0,
            })
            .is_placed()
            {
                return Err(SourceReferenceFailure::InvalidAllowedKind(*kind));
            }
            if request.allowed_kinds[..index].contains(kind) {
                return Err(Error::Invalid("duplicate source reference kind".into()).into());
            }
        }
        let placed_source = content.source_form(self, request.authored)?;
        if !placed_source.is_placed() || !request.allowed_kinds.contains(&placed_source.kind) {
            return Err(SourceReferenceFailure::ReferenceKind(placed_source.kind));
        }
        let cell_source = content.source_form(self, request.cell)?;
        if cell_source.kind != *b"CELL" {
            return Err(SourceReferenceFailure::CellKind(cell_source.kind));
        }
        request.pose.validate()?;
        let existing = self.authored_reference(request.authored);
        let mut charged_bytes = size_of::<StagedSourceReference>();
        let observed = existing.map(|id| self.reference_origin(id)).transpose()?;
        let old_state = existing.and_then(|id| self.reference_states.get(&id));
        for charge in [
            self.cohort.len(),
            request.authored.origin_plugin.len(),
            request.cell.origin_plugin.len(),
            if existing.is_some() {
                self.cohort.len()
            } else {
                0
            },
            observed.flatten().map_or(0, |key| key.origin_plugin.len()),
            old_state.map_or(0, |state| state.cell().origin_plugin.len()),
        ] {
            charged_bytes = charged_bytes
                .checked_add(charge)
                .ok_or(Error::Capacity("source reference copied bytes"))?;
        }
        if charged_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("source reference copied bytes").into());
        }
        Ok(StagedSourceReference {
            epoch: self.epoch,
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            next_reference: self.next_reference,
            authored: request.authored.clone(),
            state: State::new(request.cell.clone(), request.pose.clone(), request.enabled)?,
            placed_source,
            cell_source,
            existing: existing.map(|id| self.reference_view(id)).transpose()?,
            charged_bytes,
        })
    }

    pub fn commit_source_reference(
        &mut self,
        stage: StagedSourceReference,
    ) -> Result<SourceReferenceReceipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision {
            return Err(Error::Invalid("source reference revision changed".into()));
        }
        stage.state.validate()?;
        let registered = self.authored_reference(&stage.authored);
        if let Some(view) = stage.existing {
            self.validate_reference_view(&view)?;
            if registered != Some(view.reference()) {
                return Err(Error::Invalid("source reference binding changed".into()));
            }
            // This outcome reports existing identity and retained state. It is
            // explicitly an observation, never a successful initialization write.
            return Ok(SourceReferenceReceipt {
                outcome: SourceReferenceOutcome::Reused,
                before_revision: self.revision,
                after_revision: self.revision,
                view,
                requested_cell: stage.state.cell().clone(),
                placed_source: stage.placed_source,
                cell_source: stage.cell_source,
            });
        }
        if registered.is_some() || stage.next_reference != self.next_reference {
            return Err(Error::Invalid("source reference binding changed".into()));
        }
        if self.references.len() >= self.limits.max_references {
            return Err(Error::Capacity("live references"));
        }
        let next = self
            .next_reference
            .checked_add(1)
            .ok_or(Error::Capacity("reference identities"))?;
        let reference = ReferenceId(
            NonZeroU64::new(self.next_reference)
                .ok_or_else(|| Error::Invalid("zero reference allocator".into()))?,
        );
        let revision = self.next_revision()?;
        let receipt = SourceReferenceReceipt {
            outcome: SourceReferenceOutcome::Created,
            before_revision: self.revision,
            after_revision: revision,
            requested_cell: stage.state.cell().clone(),
            placed_source: stage.placed_source,
            cell_source: stage.cell_source,
            view: View {
                campaign: stage.campaign,
                catalogue_sha256: stage.catalogue_sha256,
                revision,
                reference,
                authored: Some(stage.authored.clone()),
                state: Some(stage.state.clone()),
                epoch: self.epoch,
            },
        };
        // No fallible validation or arithmetic follows the first canonical write.
        self.authored_references
            .insert(stage.authored.clone(), reference);
        self.references.insert(reference, Some(stage.authored));
        self.reference_states.insert(reference, stage.state);
        self.next_reference = next;
        self.revision = revision;
        Ok(receipt)
    }
}
