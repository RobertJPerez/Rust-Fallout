//! Explicit host groups over existing registered identities. No authored group,
//! enable-parent relationship or initial pose/default is selected here.
use super::{Staged, State, View};
use crate::{
    Error, Result, World,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::{collections::BTreeSet, mem::size_of};

#[derive(Debug, Clone, Copy)]
pub struct BatchLimits {
    pub max_rows: usize,
    /// Fixed staged values, owned UTF-8 strings and duplicate-check IDs. Excludes
    /// allocator overhead; bounds staging copies, not process peak memory.
    pub max_copied_bytes: usize,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            max_rows: 256,
            max_copied_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug)]
#[must_use = "staging has no effects; commit the batch or drop it"]
pub struct StagedReferenceBatch {
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    epoch: u64,
    rows: Vec<Staged>,
    charged_bytes: usize,
}
impl StagedReferenceBatch {
    pub fn rows(&self) -> &[Staged] {
        &self.rows
    }
    pub fn charged_bytes(&self) -> usize {
        self.charged_bytes
    }
}

/// Report values only; deserialization cannot create commit authority.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct BatchChange {
    reference: ReferenceId,
    authored: Option<FormKey>,
    state: State,
}
impl BatchChange {
    pub fn reference(&self) -> ReferenceId {
        self.reference
    }
    pub fn authored(&self) -> Option<&FormKey> {
        self.authored.as_ref()
    }
    pub fn state(&self) -> &State {
        &self.state
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct BatchReceipt {
    campaign: CampaignId,
    catalogue_sha256: String,
    before_revision: u64,
    after_revision: u64,
    changes: Vec<BatchChange>,
}
impl BatchReceipt {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn before_revision(&self) -> u64 {
        self.before_revision
    }
    pub fn after_revision(&self) -> u64 {
        self.after_revision
    }
    /// Preserves explicit request order; no implicit group ordering is invented.
    pub fn changes(&self) -> &[BatchChange] {
        &self.changes
    }
}

impl World<'_> {
    /// Rejects empty requests. Even identical explicit writes commit once, as in
    /// the existing single-reference API; staging/dropping never commits.
    pub fn stage_reference_batch(
        &self,
        changes: &[(View, State)],
        limits: BatchLimits,
    ) -> Result<StagedReferenceBatch> {
        if changes.is_empty() {
            return Err(Error::Invalid("empty reference batch".into()));
        }
        if changes.len() > limits.max_rows {
            return Err(Error::Capacity("reference batch rows"));
        }
        let mut charged_bytes = size_of::<StagedReferenceBatch>()
            .checked_add(self.cohort.len())
            .ok_or(Error::Capacity("reference batch copied bytes"))?;
        // Admit every proposed owned copy before allocating any rows or IDs.
        for (view, state) in changes {
            self.validate_reference_view(view)?;
            state.validate()?;
            for charge in [
                size_of::<Staged>(),
                size_of::<ReferenceId>(),
                view.catalogue_sha256.len(),
                view.authored
                    .as_ref()
                    .map_or(0, |key| key.origin_plugin.len()),
                view.state
                    .as_ref()
                    .map_or(0, |old| old.cell().origin_plugin.len()),
                state.cell().origin_plugin.len(),
            ] {
                charged_bytes = charged_bytes
                    .checked_add(charge)
                    .ok_or(Error::Capacity("reference batch copied bytes"))?;
            }
            if charged_bytes > limits.max_copied_bytes {
                return Err(Error::Capacity("reference batch copied bytes"));
            }
        }
        let mut ids = BTreeSet::new();
        for (view, _) in changes {
            if !ids.insert(view.reference) {
                return Err(Error::Invalid("duplicate reference batch identity".into()));
            }
        }
        drop(ids);
        Ok(StagedReferenceBatch {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            epoch: self.epoch,
            rows: changes
                .iter()
                .map(|(view, state)| Staged {
                    base: view.clone(),
                    state: state.clone(),
                })
                .collect(),
            charged_bytes,
        })
    }

    pub fn commit_reference_batch(&mut self, stage: StagedReferenceBatch) -> Result<BatchReceipt> {
        if stage.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if stage.campaign != self.campaign || stage.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if stage.revision != self.revision {
            return Err(Error::Invalid("reference batch revision changed".into()));
        }
        for row in &stage.rows {
            self.validate_reference_view(&row.base)?;
            row.state.validate()?;
        }
        let revision = self.next_revision()?;
        let receipt = BatchReceipt {
            campaign: stage.campaign,
            catalogue_sha256: stage.catalogue_sha256,
            before_revision: self.revision,
            after_revision: revision,
            changes: stage
                .rows
                .iter()
                .map(|row| BatchChange {
                    reference: row.base.reference,
                    authored: row.base.authored.clone(),
                    state: row.state.clone(),
                })
                .collect(),
        };
        // All fallible validation and revision arithmetic precede these effects.
        for row in stage.rows {
            self.reference_states.insert(row.base.reference, row.state);
        }
        self.revision = revision;
        Ok(receipt)
    }
}
