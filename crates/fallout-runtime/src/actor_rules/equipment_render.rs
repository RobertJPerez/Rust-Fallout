//! One current canonical lot joined to an explicitly chosen source model role.
use super::equipment;
use crate::{
    World,
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    inventory::ItemHandle,
};
use fallout_data::{
    actors::{self, dependencies::equipment as model},
    assets::ArchiveAssets,
    identity::FormKey,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use std::io::Write;

pub struct Choice {
    pub owner: ReferenceId,
    pub item: ItemHandle,
    /// Explicit source context; this does not infer the owner's actor origin.
    pub actor: FormKey,
    pub role: model::Role,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub selection: equipment::Limits,
    pub model: model::Limits,
    pub max_sources: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            selection: equipment::Limits::default(),
            model: model::Limits::default(),
            max_sources: 256,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("equipment model request campaign or source cohort changed")]
    ContextChanged,
    #[error("equipment model request canonical revision changed; prepare a current selection")]
    RevisionChanged,
    #[error("equipment model request actor source kind is unsupported")]
    ActorUnsupported,
    #[error("equipment model request canonical source header differs")]
    SourceChanged,
    #[error("equipment model request {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Selection(#[from] equipment::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Source(#[from] fallout_data::Error),
}

/// A current-epoch intent, never deserializable item or model authority.
pub struct Requests {
    choice: Choice,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
}

#[derive(Serialize)]
pub struct Observation<'a> {
    selection: equipment::Selection,
    model: model::Manifest<'a>,
    state_revision: u64,
    equipped_state_verified: bool,
    actor_reference_bound: bool,
    scope: &'static str,
}
impl<'a> Observation<'a> {
    pub fn selection(&self) -> &equipment::Selection {
        &self.selection
    }
    pub fn model(&self) -> &model::Manifest<'a> {
        &self.model
    }
}

fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}

fn validate_actor(world: &World<'_>, content: &Content, actor: &FormKey) -> Result<(), Error> {
    let source = content.source_form(world, actor)?;
    if !matches!(&source.kind, b"NPC_" | b"CREA") {
        return Err(Error::ActorUnsupported);
    }
    Ok(())
}

struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("equipment model projection budget"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Requests {
    pub fn prepare(
        world: &World<'_>,
        content: &Content,
        choice: Choice,
        limits: Limits,
    ) -> Result<Self, Error> {
        if world.catalogue().sources.len() > limits.max_sources {
            return Err(Error::Capacity("source"));
        }
        content.validate_world(world)?;
        validate_actor(world, content, &choice.actor)?;
        // Admit the exact owner/lot and bounded facts before retaining intent.
        equipment::observe_handle(world, content, choice.owner, choice.item, limits.selection)?;
        Ok(Self {
            choice,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
            revision: world.revision(),
        })
    }

    pub fn observe<'a>(
        &self,
        world: &World<'_>,
        content: &Content,
        store: &mut RecordStore,
        actors: &'a actors::Catalogue<'a>,
        assets: &ArchiveAssets,
        limits: Limits,
    ) -> Result<Observation<'a>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        // Check epoch/current membership before the revision fence, including a
        // cold reload with the same campaign, saved IDs, facts and revision.
        world.item_id(self.choice.item)?;
        if self.revision != world.revision() {
            return Err(Error::RevisionChanged);
        }
        if actors.sources().len() > limits.max_sources {
            return Err(Error::Capacity("source"));
        }
        if !same_sources(actors.sources(), &world.catalogue().sources)
            || actors.winning_content_sha256() != world.catalogue().winning_content_sha256()
        {
            return Err(Error::ContextChanged);
        }
        validate_actor(world, content, &self.choice.actor)?;
        let selection = equipment::observe_handle(
            world,
            content,
            self.choice.owner,
            self.choice.item,
            limits.selection,
        )?;
        let model = model::request(
            store,
            actors,
            &self.choice.actor,
            model::Choice {
                equipment: selection.base().clone(),
                role: self.choice.role,
            },
            assets,
            limits.model,
        )?;
        let actor = content.source_form(world, model.actor.key)?;
        let selected = model.source_records.first().ok_or(Error::SourceChanged)?;
        let canonical = selection.source_form();
        if actor.kind != model.actor.header.kind
            || actor.flags != model.actor.header.flags
            || selected.key != *selection.base()
            || selected.header.kind != canonical.kind
            || selected.header.flags != canonical.flags
        {
            return Err(Error::SourceChanged);
        }
        let result = Observation {
            selection,
            model,
            state_revision: self.revision,
            equipped_state_verified: false,
            actor_reference_bound: false,
            scope: "Exact current canonical item handle/owner/lot joined to explicit caller source actor/model role; unknown/raw equipped slots and modifications do not choose equipped state, sex, role or mod mask; no actor-origin inference, equip mutation, attachment, texture swap or original behavior admission",
        };
        serde_json::to_writer(
            ProjectionBudget {
                bytes: 0,
                maximum: limits.max_projection_bytes,
            },
            &result,
        )
        .map_err(|_| Error::Capacity("projection byte"))?;
        Ok(result)
    }
}
