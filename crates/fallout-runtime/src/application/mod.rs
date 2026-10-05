//! Application commands compose the canonical runtime rather than owning a
//! second inventory or save format. Selections are observations, not effects.
use crate::{
    Error, World,
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    inventory::{ItemHandle, ItemId, TransferLimits, TransferReceipt},
    source_items::{self, Policy},
};
use fallout_data::identity::FormKey;
use std::{
    collections::BTreeMap,
    mem::size_of,
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

static NEXT_HOST: AtomicU64 = AtomicU64::new(1);

mod continuation;
mod persistence;
pub use continuation::{
    ContinueBoundary, ContinueReceipt, ContinueRequest, PreparedContinue, ScenePublisher,
};
pub use persistence::{SaveRequest, SaveSubmission};

pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error(transparent)]
    State(#[from] Error),
    #[error(transparent)]
    Source(#[from] crate::foreign::Failure),
    #[error(transparent)]
    ItemSource(#[from] source_items::Failure),
    #[error(transparent)]
    Restore(#[from] crate::save::RestoreError),
    #[error(transparent)]
    SaveSubmission(#[from] Box<crate::save::SubmitFailure>),
    #[error("application selection belongs to an expired host or scene")]
    ExpiredSelection,
    #[error("application selection revision differs from the canonical world")]
    RevisionChanged,
    #[error("application request identity was already used with another command")]
    RequestConflict,
    #[error("application command refused: {0}")]
    Refused(&'static str),
    #[error("application retention budget exceeded: {0}")]
    Capacity(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct HostLimits {
    pub max_accepted_requests: usize,
    /// Logical retained selections and transaction copies, not allocator overhead
    /// or observations retained by external consumers.
    pub max_receipt_bytes: usize,
    pub transfer: TransferLimits,
}
impl Default for HostLimits {
    fn default() -> Self {
        Self {
            max_accepted_requests: 256,
            max_receipt_bytes: 8 * 1024 * 1024,
            transfer: TransferLimits::default(),
        }
    }
}

/// Issued only after qualifying both owners and the selected lot against the
/// same immutable content. Private fields prevent a deserialized view minting
/// mutation authority. This first supported operation transfers a complete lot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    host: u64,
    scene: NonZeroU64,
    campaign: CampaignId,
    catalogue: String,
    revision: u64,
    source: ReferenceId,
    source_key: FormKey,
    target: ReferenceId,
    target_key: FormKey,
    item: ItemHandle,
    base: FormKey,
    quantity: u32,
}
impl Selection {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
    pub fn source_key(&self) -> &FormKey {
        &self.source_key
    }
    pub fn target_key(&self) -> &FormKey {
        &self.target_key
    }
    pub fn quantity(&self) -> u32 {
        self.quantity
    }
    pub fn command(self, request: NonZeroU64) -> TransferCommand {
        TransferCommand {
            request,
            selection: self,
        }
    }
    fn retained_bytes(&self) -> Result<usize> {
        selection_bytes(
            &self.catalogue,
            &self.source_key,
            &self.target_key,
            &self.base,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferCommand {
    request: NonZeroU64,
    selection: Selection,
}

/// Process-local publication identity. Native restore can reuse a saved numeric
/// revision, so acknowledgements also need the host identity that issued them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostIdentity(u64);

#[derive(Debug, Clone)]
pub struct TransferResult {
    pub request: NonZeroU64,
    pub replayed: bool,
    pub receipt: Arc<TransferReceipt>,
    host: HostIdentity,
    scene: NonZeroU64,
}
impl TransferResult {
    pub fn host_identity(&self) -> HostIdentity {
        self.host
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
}
struct Accepted {
    command: TransferCommand,
    receipt: Arc<TransferReceipt>,
}

pub struct Host<'a> {
    world: World<'a>,
    content: Arc<Content>,
    policy: Policy,
    epoch: u64,
    scene: NonZeroU64,
    limits: HostLimits,
    accepted: BTreeMap<NonZeroU64, Accepted>,
    retained_bytes: usize,
    pending_continue: Option<ContinueRequest>,
    last_continue_request: u64,
    last_save_request: u64,
}
impl<'a> Host<'a> {
    pub fn new(
        world: World<'a>,
        content: Arc<Content>,
        policy: Policy,
        scene: NonZeroU64,
        limits: HostLimits,
    ) -> Result<Self> {
        content.validate_world(&world)?;
        let epoch = NEXT_HOST
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Failure::Capacity("host identities"))?;
        Ok(Self {
            world,
            content,
            policy,
            epoch,
            scene,
            limits,
            accepted: BTreeMap::new(),
            retained_bytes: 0,
            pending_continue: None,
            last_continue_request: 0,
            last_save_request: 0,
        })
    }
    pub fn world(&self) -> &World<'a> {
        &self.world
    }
    pub fn identity(&self) -> HostIdentity {
        HostIdentity(self.epoch)
    }
    pub fn into_world(self) -> World<'a> {
        self.world
    }
    pub fn scene_generation(&self) -> NonZeroU64 {
        self.scene
    }
    /// Input/focus cancellation changes no canonical state. Old scene selections
    /// cannot execute or replay in a newer scene, even if its revision is equal.
    pub fn advance_scene(&mut self, scene: NonZeroU64) -> Result<()> {
        if scene <= self.scene {
            return Err(Failure::Refused("scene generation must advance"));
        }
        self.scene = scene;
        Ok(())
    }
    fn owner_key(&self, owner: ReferenceId) -> Result<&FormKey> {
        let key = self
            .world
            .reference_origin(owner)?
            .ok_or(Failure::Refused("owner has no source-qualified identity"))?;
        if !self.content.source_form(&self.world, key)?.is_placed() {
            return Err(Failure::Refused("owner source is not a placed reference"));
        }
        Ok(key)
    }
    pub fn select_transfer(
        &self,
        source: ReferenceId,
        item: ItemId,
        target: ReferenceId,
        quantity: u32,
    ) -> Result<Selection> {
        if source == target {
            return Err(Failure::Refused("source and destination are identical"));
        }
        let source_key = self.owner_key(source)?;
        let target_key = self.owner_key(target)?;
        let lot = self.world.item(item)?;
        if lot.owner() != source {
            return Err(Failure::Refused("lot does not belong to selected source"));
        }
        if quantity != lot.count() {
            return Err(Failure::Refused(
                "only the exact complete lot quantity is supported",
            ));
        }
        source_items::validate(&self.world, &self.content, &self.policy, lot.facts())?;
        // Count before cloning source names. The transaction separately admits
        // the initialized banks and all retained Facts using its existing bounds.
        let bytes = selection_bytes(
            self.world.catalogue_fingerprint(),
            source_key,
            target_key,
            &lot.facts().base,
        )?;
        if bytes > self.limits.max_receipt_bytes {
            return Err(Failure::Capacity("selection bytes"));
        }
        Ok(Selection {
            host: self.epoch,
            scene: self.scene,
            campaign: self.world.campaign(),
            catalogue: self.world.catalogue_fingerprint().into(),
            revision: self.world.revision(),
            source,
            source_key: source_key.clone(),
            target,
            target_key: target_key.clone(),
            item: self.world.item_handle(item)?,
            base: lot.facts().base.clone(),
            quantity,
        })
    }
    pub fn transfer(&mut self, command: TransferCommand) -> Result<TransferResult> {
        let selection = &command.selection;
        if selection.host != self.epoch || selection.scene != self.scene {
            return Err(Failure::ExpiredSelection);
        }
        if let Some(accepted) = self.accepted.get(&command.request) {
            if accepted.command != command {
                return Err(Failure::RequestConflict);
            }
            return Ok(TransferResult {
                request: command.request,
                replayed: true,
                receipt: Arc::clone(&accepted.receipt),
                host: self.identity(),
                scene: self.scene,
            });
        }
        if selection.campaign != self.world.campaign()
            || selection.catalogue != self.world.catalogue_fingerprint()
        {
            return Err(Failure::Refused("selection canonical identity changed"));
        }
        if selection.revision != self.world.revision() {
            return Err(Failure::RevisionChanged);
        }
        if self.accepted.len() >= self.limits.max_accepted_requests {
            return Err(Failure::Capacity("accepted requests"));
        }
        if self.owner_key(selection.source)? != &selection.source_key
            || self.owner_key(selection.target)? != &selection.target_key
        {
            return Err(Failure::Refused("selected owner source changed"));
        }
        let lot = self.world.item_by_handle(selection.item)?;
        if lot.owner() != selection.source
            || lot.count() != selection.quantity
            || lot.facts().base != selection.base
        {
            return Err(Failure::Refused("selected lot changed"));
        }
        source_items::validate(&self.world, &self.content, &self.policy, lot.facts())?;
        let stage = self
            .world
            .stage_inventory_transfers(&[(lot.id(), selection.target)], self.limits.transfer)?;
        let bytes = self
            .retained_bytes
            .checked_add(selection.retained_bytes()?)
            .and_then(|n| n.checked_add(size_of::<Accepted>()))
            .and_then(|n| n.checked_add(stage.usage().copied_bytes))
            .ok_or(Failure::Capacity("receipt bytes"))?;
        if bytes > self.limits.max_receipt_bytes {
            return Err(Failure::Capacity("receipt bytes"));
        }
        // Every fallible check precedes the canonical commit. Retention has no
        // eviction: capacity refusal cannot make an accepted identity reusable.
        let receipt = Arc::new(self.world.commit_inventory_transfers(stage)?);
        let result = TransferResult {
            request: command.request,
            replayed: false,
            receipt: Arc::clone(&receipt),
            host: self.identity(),
            scene: self.scene,
        };
        self.accepted
            .insert(command.request, Accepted { command, receipt });
        self.retained_bytes = bytes;
        Ok(result)
    }
}

fn selection_bytes(
    catalogue: &str,
    source: &FormKey,
    target: &FormKey,
    base: &FormKey,
) -> Result<usize> {
    [
        catalogue.len(),
        source.origin_plugin.len(),
        target.origin_plugin.len(),
        base.origin_plugin.len(),
    ]
    .into_iter()
    .try_fold(size_of::<Selection>(), |n, bytes| {
        n.checked_add(bytes)
            .ok_or(Failure::Capacity("selection bytes"))
    })
}
