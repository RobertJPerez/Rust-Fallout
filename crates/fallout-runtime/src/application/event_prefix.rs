//! Prepare existing journal effects in a private world. This grants no active
//! world or scene publication authority and advances no application clocks.
use super::{Host, HostIdentity};
use crate::{
    World,
    execution::{local_copy, pending_batch},
    identity::CampaignId,
    programs::{LookupError, PreparedSources},
};
use std::num::NonZeroU64;

#[derive(Debug, thiserror::Error)]
pub enum EventPrefixError {
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Sources(#[from] LookupError),
    #[error(transparent)]
    Batch(#[from] pending_batch::Error),
}

#[derive(Debug)]
pub enum EventPrefixPreparation {
    Unsupported {
        event_index: Option<usize>,
        reason: local_copy::Unsupported,
        detail: String,
    },
    EngineeringPrepared(PreparedEventPrefix),
}

/// An immutable observation of private engineering effects. It cannot be
/// deserialized or submitted as an application mutation acknowledgement.
#[derive(Debug)]
pub struct PreparedEventPrefix {
    host: HostIdentity,
    scene: NonZeroU64,
    campaign: CampaignId,
    catalogue: String,
    revision: u64,
    decoder: String,
    result: Box<pending_batch::Completed>,
}
impl PreparedEventPrefix {
    pub fn host_identity(&self) -> HostIdentity {
        self.host
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
    pub fn input_revision(&self) -> u64 {
        self.revision
    }
    pub fn decoder_sha256(&self) -> &str {
        &self.decoder
    }
    pub fn observation(&self) -> &pending_batch::Completed {
        &self.result
    }
    /// Current input identity is necessary for a later consumer, but is not
    /// evidence that these engineering effects may be published to gameplay.
    pub fn matches_current_boundary(&self, host: &Host<'_>, sources: &PreparedSources<'_>) -> bool {
        self.host == host.identity()
            && self.scene == host.scene_generation()
            && self.campaign == host.world().campaign()
            && self.catalogue == host.world().catalogue_fingerprint()
            && self.revision == host.world().revision()
            && self.decoder == sources.decoder_sha256()
            && sources.validate_world(host.world()).is_ok()
    }
}

impl Host<'_> {
    /// Run off the render/input frame: this copies the bounded canonical
    /// snapshot and prepares an explicit existing prefix. The retained source
    /// catalogue (including decoded schemas) and World limits are reused.
    /// Existing adapters own instruction/order/conversion refusals; Faithful
    /// intent remains unsupported. Every private effect is dropped on refusal.
    pub fn prepare_event_prefix(
        &self,
        sources: &PreparedSources<'_>,
        requests: &[pending_batch::OwnerRequest],
        intent: local_copy::Intent,
        limits: pending_batch::Limits,
    ) -> Result<EventPrefixPreparation, EventPrefixError> {
        if requests.is_empty() {
            return Err(
                pending_batch::Error::Input("an explicit nonempty prefix is required").into(),
            );
        }
        if requests.len() > limits.maximum_events {
            return Err(pending_batch::Error::Capacity("events").into());
        }
        sources.validate_world(&self.world)?;
        let input = self.world.snapshot();
        // Reuse the existing serialized cap as well as restore's relationship
        // and source-schema admission. No new persistence format is introduced.
        input.encode(self.world.limits.max_snapshot_bytes)?;
        let private = World::restore(self.world.catalogue.clone(), input, self.world.limits)?;
        match pending_batch::consume_ordered(
            private,
            sources,
            &self.content,
            requests,
            intent,
            limits,
        )? {
            pending_batch::Outcome::Unsupported {
                event_index,
                reason,
                detail,
            } => Ok(EventPrefixPreparation::Unsupported {
                event_index,
                reason,
                detail,
            }),
            pending_batch::Outcome::EngineeringCommitted { result } => {
                result
                    .snapshot
                    .encode(self.world.limits.max_snapshot_bytes)?;
                Ok(EventPrefixPreparation::EngineeringPrepared(
                    PreparedEventPrefix {
                        host: self.identity(),
                        scene: self.scene,
                        campaign: self.world.campaign(),
                        catalogue: self.world.catalogue_fingerprint().into(),
                        revision: self.world.revision(),
                        decoder: sources.decoder_sha256().into(),
                        result,
                    },
                ))
            }
        }
    }
}
