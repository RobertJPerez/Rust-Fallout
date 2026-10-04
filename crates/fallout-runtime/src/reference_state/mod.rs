//! Explicit canonical source-coordinate reference state. Residency and render
//! entities do not own identity. These host inputs do not evaluate authored
//! enable parents, select a player spawn or infer missing source fields.
use crate::{
    Error, Result, World,
    identity::{CampaignId, ReferenceId, valid_form},
};
use fallout_data::{identity::FormKey, world::Transform};
use serde::{Deserialize, Serialize};

mod batch;
mod paging;
mod source;
pub use batch::{BatchChange, BatchLimits, BatchReceipt, StagedReferenceBatch};
pub use paging::{Cursor, Page, PageLimits, PageRequest, PageUsage};
pub use source::{
    SourceReferenceFailure, SourceReferenceLimits, SourceReferenceOutcome, SourceReferenceReceipt,
    SourceReferenceRequest, StagedSourceReference,
};

pub const COMPONENT_VERSION: u32 = 1;

/// Exact source float bits keep signed zero and finite subnormal values intact.
/// Rotation uses the original DATA radians/convention; consumers use the shared
/// coordinates adapter. Missing source scale remains unavailable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    position_bits: [u32; 3],
    rotation_bits: [u32; 3],
    scale_bits: Option<u32>,
}
impl Pose {
    pub fn from_source(transform: &Transform, scale: Option<f32>) -> Result<Self> {
        let value = Self {
            position_bits: transform.position.map(f32::to_bits),
            rotation_bits: transform.rotation.map(f32::to_bits),
            scale_bits: scale.map(f32::to_bits),
        };
        value.validate()?;
        Ok(value)
    }
    pub fn source_transform(&self) -> Transform {
        Transform {
            position: self.position_bits.map(f32::from_bits),
            rotation: self.rotation_bits.map(f32::from_bits),
        }
    }
    pub fn source_scale(&self) -> Option<f32> {
        self.scale_bits.map(f32::from_bits)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self
            .position_bits
            .iter()
            .chain(&self.rotation_bits)
            .any(|&bits| !f32::from_bits(bits).is_finite())
        {
            return Err(Error::Invalid(
                "reference pose must be finite in source units".into(),
            ));
        }
        if self
            .source_scale()
            .is_some_and(|scale| !scale.is_finite() || scale <= 0.0)
        {
            return Err(Error::Invalid(
                "explicit reference scale must be finite and positive".into(),
            ));
        }
        Ok(())
    }
}

/// A source cell identifies residency; no cell offset, unit conversion or default
/// enable value is inferred. Callers must bind keys to verified source records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    schema_version: u32,
    cell: FormKey,
    pose: Pose,
    enabled: bool,
}
impl State {
    pub fn new(cell: FormKey, pose: Pose, enabled: bool) -> Result<Self> {
        let value = Self {
            schema_version: COMPONENT_VERSION,
            cell,
            pose,
            enabled,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn cell(&self) -> &FormKey {
        &self.cell
    }
    pub fn pose(&self) -> &Pose {
        &self.pose
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != COMPONENT_VERSION {
            return Err(Error::Invalid(
                "unsupported reference component schema".into(),
            ));
        }
        valid_form(&self.cell)?;
        self.pose.validate()
    }
}

/// An immutable owned observation. Private authority fields prevent callers
/// from forging a current view. Its transient epoch is never persisted.
#[derive(Debug, Clone, Serialize)]
pub struct View {
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    reference: ReferenceId,
    authored: Option<FormKey>,
    state: Option<State>,
    #[serde(skip)]
    epoch: u64,
}
impl View {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn reference(&self) -> ReferenceId {
        self.reference
    }
    pub fn authored(&self) -> Option<&FormKey> {
        self.authored.as_ref()
    }
    pub fn state(&self) -> Option<&State> {
        self.state.as_ref()
    }
}

#[derive(Debug)]
#[must_use = "staging has no effects; commit the proposal or drop it"]
pub struct Staged {
    base: View,
    state: State,
}
impl Staged {
    pub fn base(&self) -> &View {
        &self.base
    }
    pub fn state(&self) -> &State {
        &self.state
    }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub reference: ReferenceId,
    pub authored: Option<FormKey>,
    pub before_revision: u64,
    pub after_revision: u64,
    pub state: State,
}

impl World<'_> {
    pub fn reference_view(&self, reference: ReferenceId) -> Result<View> {
        let authored = self.reference_origin(reference)?.cloned();
        Ok(View {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            reference,
            authored,
            state: self.reference_states.get(&reference).cloned(),
            epoch: self.epoch,
        })
    }
    fn validate_reference_view(&self, view: &View) -> Result<()> {
        if view.epoch != self.epoch {
            return Err(Error::StaleHandle);
        }
        if view.campaign != self.campaign || view.catalogue_sha256 != self.cohort {
            return Err(Error::DefinitionChanged);
        }
        if view.revision != self.revision {
            return Err(Error::Invalid("reference view revision changed".into()));
        }
        if view.authored.as_ref() != self.reference_origin(view.reference)?
            || view.state.as_ref() != self.reference_states.get(&view.reference)
        {
            return Err(Error::Invalid("reference view state changed".into()));
        }
        Ok(())
    }
    /// Initial assignment and later changes both require an exact observation.
    /// Reload consumers read existing state instead of reassigning source pose.
    pub fn stage_reference_state(&self, view: &View, state: State) -> Result<Staged> {
        self.validate_reference_view(view)?;
        state.validate()?;
        Ok(Staged {
            base: view.clone(),
            state,
        })
    }
    pub fn commit_reference_state(&mut self, stage: Staged) -> Result<Receipt> {
        self.validate_reference_view(&stage.base)?;
        stage.state.validate()?;
        let revision = self.next_revision()?;
        let receipt = Receipt {
            campaign: self.campaign,
            catalogue_sha256: stage.base.catalogue_sha256,
            reference: stage.base.reference,
            authored: stage.base.authored,
            before_revision: self.revision,
            after_revision: revision,
            state: stage.state.clone(),
        };
        self.reference_states
            .insert(stage.base.reference, stage.state);
        self.revision = revision;
        Ok(receipt)
    }
}
