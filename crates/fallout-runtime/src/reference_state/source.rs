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

/// Logical stage storage and bounded source-header work. These limits exclude
/// allocator overhead, borrowed inputs and the receipt/canonical copies at commit.
#[derive(Debug, Clone, Copy)]
pub struct SourceReferenceGroupLimits {
    pub max_requests: usize,
    pub max_allowed_kinds_per_request: usize,
    pub max_allowed_kinds: usize,
    pub max_source_checks: usize,
    pub max_copied_bytes: usize,
}
impl Default for SourceReferenceGroupLimits {
    fn default() -> Self {
        Self {
            max_requests: 256,
            max_allowed_kinds_per_request: 6,
            max_allowed_kinds: 1536,
            max_source_checks: 512,
            max_copied_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourceReferenceGroupUsage {
    pub requests: usize,
    pub created: usize,
    pub reused: usize,
    pub allowed_kinds: usize,
    pub source_checks: usize,
    pub copied_bytes: usize,
}

#[derive(Debug)]
pub struct SourceReferenceGroupRow {
    authored: FormKey,
    state: State,
    placed_source: SourceForm,
    cell_source: SourceForm,
    existing: Option<View>,
}
impl SourceReferenceGroupRow {
    pub fn authored(&self) -> &FormKey {
        &self.authored
    }
    pub fn requested_state(&self) -> &State {
        &self.state
    }
    pub fn existing(&self) -> Option<&View> {
        self.existing.as_ref()
    }
}

#[derive(Debug)]
#[must_use = "staging reserves no identities; commit or drop the group"]
pub struct StagedSourceReferenceGroup {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    next_reference: u64,
    rows: Vec<SourceReferenceGroupRow>,
    usage: SourceReferenceGroupUsage,
}
impl StagedSourceReferenceGroup {
    pub fn rows(&self) -> &[SourceReferenceGroupRow] {
        &self.rows
    }
    pub fn usage(&self) -> SourceReferenceGroupUsage {
        self.usage
    }
    fn check(
        &self,
        epoch: u64,
        campaign: CampaignId,
        cohort: &str,
        revision: u64,
        next_reference: u64,
    ) -> Result<()> {
        if self.epoch != epoch {
            return Err(Error::StaleHandle);
        }
        if self.campaign != campaign || self.catalogue_sha256 != cohort {
            return Err(Error::DefinitionChanged);
        }
        if self.revision != revision || self.next_reference != next_reference {
            return Err(Error::Invalid(
                "source reference group binding changed".into(),
            ));
        }
        Ok(())
    }
}

fn group_add(before: usize, bytes: usize) -> Result<usize> {
    before
        .checked_add(bytes)
        .ok_or(Error::Capacity("source reference group copied bytes"))
}
fn group_table(rows: usize, bytes: usize) -> Result<usize> {
    rows.checked_mul(bytes)
        .ok_or(Error::Capacity("source reference group copied bytes"))
}

#[derive(Debug, Serialize)]
pub struct SourceReferenceGroupReceipt {
    before_revision: u64,
    after_revision: u64,
    rows: Vec<SourceReferenceReceipt>,
    usage: SourceReferenceGroupUsage,
}
impl SourceReferenceGroupReceipt {
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    pub fn rows(&self) -> &[SourceReferenceReceipt] {
        &self.rows
    }
    pub fn usage(&self) -> SourceReferenceGroupUsage {
        self.usage
    }
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
    // Both paths use the same borrowed source authority and validation. No body
    // decoder, source policy or state mutation is introduced by the group path.
    fn check_source_reference_request(
        &self,
        request: SourceReferenceRequest<'_>,
        content: &Content,
        max_allowed_kinds: usize,
    ) -> std::result::Result<(SourceForm, SourceForm), SourceReferenceFailure> {
        if request.allowed_kinds.is_empty() {
            return Err(Error::Invalid("empty source reference kind set".into()).into());
        }
        if request.allowed_kinds.len() > max_allowed_kinds {
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
        Ok((placed_source, cell_source))
    }

    pub fn stage_source_reference(
        &self,
        request: SourceReferenceRequest<'_>,
        content: &Content,
        limits: SourceReferenceLimits,
    ) -> std::result::Result<StagedSourceReference, SourceReferenceFailure> {
        content.validate_world(self)?;
        let (placed_source, cell_source) =
            self.check_source_reference_request(request, content, limits.max_allowed_kinds)?;
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

    fn source_reference_group_span(&self, created: usize) -> Result<(u64, u64)> {
        if self
            .references
            .len()
            .checked_add(created)
            .ok_or(Error::Capacity("live references"))?
            > self.limits.max_references
        {
            return Err(Error::Capacity("live references"));
        }
        if created == 0 {
            return Ok((self.next_reference, self.revision));
        }
        if self.next_reference == 0 {
            return Err(Error::Invalid("zero reference allocator".into()));
        }
        let count = u64::try_from(created).map_err(|_| Error::Capacity("reference identities"))?;
        let next = self
            .next_reference
            .checked_add(count)
            .ok_or(Error::Capacity("reference identities"))?;
        Ok((next, self.next_revision()?))
    }

    /// Admit ordered explicit selections atomically. Empty and all-reused
    /// groups are observations; they never advance the canonical revision.
    pub fn stage_source_reference_group(
        &self,
        requests: &[SourceReferenceRequest<'_>],
        content: &Content,
        limits: SourceReferenceGroupLimits,
    ) -> std::result::Result<StagedSourceReferenceGroup, SourceReferenceFailure> {
        content.validate_world(self)?;
        if requests.len() > limits.max_requests {
            return Err(Error::Capacity("source reference group requests").into());
        }
        let source_checks = requests
            .len()
            .checked_mul(2)
            .ok_or(Error::Capacity("source reference group source checks"))?;
        if source_checks > limits.max_source_checks {
            return Err(Error::Capacity("source reference group source checks").into());
        }
        let mut allowed_kinds = 0_usize;
        for request in requests {
            allowed_kinds = allowed_kinds
                .checked_add(request.allowed_kinds.len())
                .ok_or(Error::Capacity("source reference group kinds"))?;
            if allowed_kinds > limits.max_allowed_kinds {
                return Err(Error::Capacity("source reference group kinds").into());
            }
        }
        let mut copied_bytes = group_add(
            group_add(size_of::<StagedSourceReferenceGroup>(), self.cohort.len())?,
            group_add(
                size_of::<Vec<(SourceForm, SourceForm, Option<ReferenceId>)>>(),
                group_table(
                    requests.len(),
                    size_of::<SourceReferenceGroupRow>()
                        + size_of::<(SourceForm, SourceForm, Option<ReferenceId>)>(),
                )?,
            )?,
        )?;
        if copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("source reference group copied bytes").into());
        }
        let mut created = 0_usize;
        let mut checked = Vec::with_capacity(requests.len());
        // The whole validation/charge pass borrows inputs. No key, pose or
        // existing view is cloned before the final row and budgets pass.
        for (index, request) in requests.iter().copied().enumerate() {
            if requests[..index]
                .iter()
                .any(|earlier| earlier.authored == request.authored)
            {
                return Err(Error::Invalid("duplicate source reference group key".into()).into());
            }
            let (placed_source, cell_source) = self.check_source_reference_request(
                request,
                content,
                limits.max_allowed_kinds_per_request,
            )?;
            let existing = self.authored_reference(request.authored);
            let observed = existing.map(|id| self.reference_origin(id)).transpose()?;
            let old_state = existing.and_then(|id| self.reference_states.get(&id));
            for bytes in [
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
                copied_bytes = group_add(copied_bytes, bytes)?;
            }
            if existing.is_none() {
                created += 1;
            }
            checked.push((placed_source, cell_source, existing));
        }
        if copied_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("source reference group copied bytes").into());
        }
        self.source_reference_group_span(created)?;
        let rows = requests
            .iter()
            .copied()
            .zip(checked)
            .map(|(request, (placed_source, cell_source, existing))| {
                Ok(SourceReferenceGroupRow {
                    authored: request.authored.clone(),
                    state: State::new(request.cell.clone(), request.pose.clone(), request.enabled)?,
                    placed_source,
                    cell_source,
                    existing: existing.map(|id| self.reference_view(id)).transpose()?,
                })
            })
            .collect::<std::result::Result<Vec<_>, SourceReferenceFailure>>()?;
        Ok(StagedSourceReferenceGroup {
            epoch: self.epoch,
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            next_reference: self.next_reference,
            rows,
            usage: SourceReferenceGroupUsage {
                requests: requests.len(),
                created,
                reused: requests.len() - created,
                allowed_kinds,
                source_checks,
                copied_bytes,
            },
        })
    }

    pub fn commit_source_reference_group(
        &mut self,
        stage: StagedSourceReferenceGroup,
    ) -> Result<SourceReferenceGroupReceipt> {
        stage.check(
            self.epoch,
            self.campaign,
            &self.cohort,
            self.revision,
            self.next_reference,
        )?;
        let mut created = 0_usize;
        for (index, row) in stage.rows.iter().enumerate() {
            row.state.validate()?;
            if stage.rows[..index]
                .iter()
                .any(|earlier| earlier.authored == row.authored)
            {
                return Err(Error::Invalid(
                    "duplicate source reference group key".into(),
                ));
            }
            let registered = self.authored_reference(&row.authored);
            if let Some(view) = &row.existing {
                self.validate_reference_view(view)?;
                if registered != Some(view.reference()) {
                    return Err(Error::Invalid(
                        "source reference group binding changed".into(),
                    ));
                }
            } else {
                if registered.is_some() {
                    return Err(Error::Invalid(
                        "source reference group binding changed".into(),
                    ));
                }
                created += 1;
            }
        }
        if stage.usage.requests != stage.rows.len()
            || stage.usage.created != created
            || stage.usage.reused != stage.rows.len() - created
        {
            return Err(Error::Invalid(
                "source reference group counts changed".into(),
            ));
        }
        let (next_reference, revision) = self.source_reference_group_span(created)?;
        let mut cursor = self.next_reference;
        let mut pending = Vec::with_capacity(created);
        let mut rows = Vec::with_capacity(stage.rows.len());
        // Construct every receipt and identity before the first canonical write.
        for row in stage.rows {
            let (outcome, view) = if let Some(mut view) = row.existing {
                view.revision = revision;
                (SourceReferenceOutcome::Reused, view)
            } else {
                let reference = ReferenceId(
                    NonZeroU64::new(cursor)
                        .ok_or_else(|| Error::Invalid("zero reference allocator".into()))?,
                );
                cursor = cursor
                    .checked_add(1)
                    .ok_or(Error::Capacity("reference identities"))?;
                let view = View {
                    campaign: self.campaign,
                    catalogue_sha256: self.cohort.clone(),
                    revision,
                    reference,
                    authored: Some(row.authored.clone()),
                    state: Some(row.state.clone()),
                    epoch: self.epoch,
                };
                pending.push((reference, row.authored, row.state.clone()));
                (SourceReferenceOutcome::Created, view)
            };
            rows.push(SourceReferenceReceipt {
                outcome,
                before_revision: self.revision,
                after_revision: revision,
                view,
                requested_cell: row.state.cell().clone(),
                placed_source: row.placed_source,
                cell_source: row.cell_source,
            });
        }
        let receipt = SourceReferenceGroupReceipt {
            before_revision: self.revision,
            after_revision: revision,
            rows,
            usage: stage.usage,
        };
        // Every fallible check, arithmetic operation and receipt copy is complete.
        for (reference, authored, state) in pending {
            self.authored_references.insert(authored.clone(), reference);
            self.references.insert(reference, Some(authored));
            self.reference_states.insert(reference, state);
        }
        self.next_reference = next_reference;
        self.revision = revision;
        Ok(receipt)
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;
    #[test]
    fn logical_group_metadata_addition_and_table_lengths_never_wrap() {
        assert_eq!(group_add(usize::MAX - 1, 1).unwrap(), usize::MAX);
        assert!(group_add(1, usize::MAX).is_err());
        assert!(group_table(usize::MAX, 2).is_err());
    }
    #[test]
    fn every_group_authority_binding_is_checked_before_rows() {
        let campaign = CampaignId::from_bytes([1; 16]).unwrap();
        let stage = StagedSourceReferenceGroup {
            epoch: 7,
            campaign,
            catalogue_sha256: "a".repeat(64),
            revision: 9,
            next_reference: 4,
            rows: vec![],
            usage: SourceReferenceGroupUsage {
                requests: 0,
                created: 0,
                reused: 0,
                allowed_kinds: 0,
                source_checks: 0,
                copied_bytes: 0,
            },
        };
        assert!(stage.check(7, campaign, &"a".repeat(64), 9, 4).is_ok());
        assert!(matches!(
            stage.check(8, campaign, &"a".repeat(64), 9, 4),
            Err(Error::StaleHandle)
        ));
        assert!(matches!(
            stage.check(
                7,
                CampaignId::from_bytes([2; 16]).unwrap(),
                &"a".repeat(64),
                9,
                4
            ),
            Err(Error::DefinitionChanged)
        ));
        assert!(matches!(
            stage.check(7, campaign, &"b".repeat(64), 9, 4),
            Err(Error::DefinitionChanged)
        ));
        assert!(stage.check(7, campaign, &"a".repeat(64), 10, 4).is_err());
        assert!(stage.check(7, campaign, &"a".repeat(64), 9, 5).is_err());
    }
}
