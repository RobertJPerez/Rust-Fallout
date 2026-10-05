//! A selected INFO speaker joined to its exact current canonical actor context.
//! This prepares dialogue inputs; it does not select or accept responses.
use super::{context, faction_pair, stats};
use crate::{
    World,
    foreign::Content,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::{
    actors::{self, associations, dependencies, factions, placements},
    identity::FormKey,
    narrative, plugin,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_info_record_bytes: usize,
    pub narrative: narrative::Limits,
    pub context: context::Limits,
    pub statistics: stats::Limits,
    pub relationships: faction_pair::Limits,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_info_record_bytes: 4 * 1024 * 1024,
            narrative: narrative::Limits::default(),
            context: context::Limits::default(),
            statistics: stats::Limits::default(),
            relationships: faction_pair::Limits::default(),
            max_projection_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Choice {
    /// The dialogue host selects one INFO source explicitly.
    pub info: FormKey,
    /// The host binds the source speaker to an existing current actor reference.
    pub speaker_reference: ReferenceId,
    /// Relationship inputs use the host's explicit dialogue target reference.
    pub target_reference: ReferenceId,
}

#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    pub placements: &'a placements::Catalogue,
    pub actors: &'a actors::Catalogue<'a>,
    pub associations: &'a associations::Catalogue<'a>,
    pub dependencies: &'a dependencies::Catalogue<'a>,
    pub factions: &'a factions::Catalogue,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("dialogue actor context source cohort changed")]
    ContextChanged,
    #[error("dialogue actor context canonical revision changed; prepare a current selection")]
    RevisionChanged,
    #[error("selected dialogue INFO is unavailable: {0}")]
    InfoUnavailable(&'static str),
    #[error("selected dialogue INFO speaker is unavailable: {0}")]
    SpeakerUnavailable(&'static str),
    #[error("selected dialogue INFO contains multiple physical speaker fields")]
    AmbiguousSpeaker,
    #[error("selected dialogue INFO speaker does not match the canonical actor reference")]
    SpeakerReferenceMismatch,
    #[error("dialogue actor context {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Context(#[from] context::Error),
    #[error(transparent)]
    Statistics(#[from] stats::Error),
    #[error(transparent)]
    Relationships(#[from] faction_pair::Error),
    #[error(transparent)]
    Data(#[from] fallout_data::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Projection(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InfoSource {
    pub key: FormKey,
    pub source_plugin: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_record_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpeakerField {
    pub field_index: usize,
    pub decoded_offset: usize,
    pub raw_form: u32,
    pub actor_key: FormKey,
    pub actor_kind: [u8; 4],
    pub field_sha256: String,
}

pub struct Requests<'a> {
    choice: Choice,
    campaign: CampaignId,
    cohort: String,
    revision: u64,
    sources: Sources<'a>,
    info: InfoSource,
    speaker: SpeakerField,
    statistics: stats::Requests<'a>,
}

#[derive(Serialize)]
pub struct Observation<'request, 'source> {
    pub info: InfoSource,
    pub speaker: SpeakerField,
    pub speaker_context: context::Observation<'source>,
    pub statistics: stats::Observation<'request>,
    pub relationships: faction_pair::DirectedPairInputs<'source>,
    pub speaker_reference_matches_info: bool,
    pub response_selection_supported: bool,
    pub dialogue_conditions_supported: bool,
    pub current_actor_values_supported: bool,
    pub effective_faction_membership_supported: bool,
    pub relationship_evaluation_supported: bool,
    pub dialogue_eligibility_supported: bool,
    pub original_behavior_verified: bool,
    pub scope: &'static str,
}

struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|size| *size <= self.maximum)
            .ok_or_else(|| std::io::Error::other("dialogue actor projection budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn same_sources(left: &[SourceReceipt], right: &[SourceReceipt]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.source_name == right.source_name
                && left.source_bytes == right.source_bytes
                && left.source_sha256 == right.source_sha256
        })
}

fn admitted_receipts(
    store: &mut RecordStore,
    world: &World<'_>,
    limits: Limits,
) -> Result<Vec<SourceReceipt>, Error> {
    let receipts = store.source_receipts()?;
    if receipts.len() > limits.max_sources {
        return Err(Error::Capacity("source"));
    }
    if !same_sources(&receipts, &world.catalogue().sources) {
        return Err(Error::ContextChanged);
    }
    Ok(receipts)
}

fn selected_info(
    store: &mut RecordStore,
    world: &World<'_>,
    content: &Content,
    actors: &actors::Catalogue<'_>,
    key: &FormKey,
    limits: Limits,
) -> Result<(InfoSource, SpeakerField), Error> {
    content.validate_world(world)?;
    let receipts = admitted_receipts(store, world, limits)?;
    let location = store
        .winner(key)
        .ok_or(Error::InfoUnavailable("winning_record_missing"))?;
    let (header, source_plugin) = {
        let definition = store.definition(location);
        (
            definition.header.clone(),
            store.source_name(location).to_owned(),
        )
    };
    if header.kind != *b"INFO" {
        return Err(Error::InfoUnavailable("winning_record_is_not_info"));
    }
    if header.flags & plugin::DELETED != 0 {
        return Err(Error::InfoUnavailable("winning_info_is_deleted"));
    }
    let content_form = content.source_form(world, key)?;
    if content_form.kind != *b"INFO" || content_form.flags != header.flags {
        return Err(Error::InfoUnavailable("content_winner_differs"));
    }
    let record = store.read_bounded(location, limits.max_info_record_bytes)?;
    if record.header != header {
        return Err(Error::InfoUnavailable("indexed_header_changed"));
    }
    let document = narrative::decode(&record, &source_plugin, limits.narrative)?;
    let mut speaker = None;
    for (field_index, field) in document.fields.iter().enumerate() {
        if field.kind != *b"ANAM" {
            continue;
        }
        if speaker.is_some() {
            return Err(Error::AmbiguousSpeaker);
        }
        if field.owner != Some(0) {
            return Err(Error::SpeakerUnavailable("speaker_field_owner_unavailable"));
        }
        let raw_form = match &field.value {
            narrative::Value::RawForm(raw_form) => *raw_form,
            _ => return Err(Error::SpeakerUnavailable("speaker_field_not_typed")),
        };
        if raw_form == 0 {
            return Err(Error::SpeakerUnavailable("null_speaker"));
        }
        let actor_key = store
            .key_for(location, raw_form)?
            .ok_or(Error::SpeakerUnavailable("speaker_form_unresolved"))?;
        let actor = actors
            .get(&actor_key)
            .filter(|actor| !actor.deleted)
            .ok_or(Error::SpeakerUnavailable(
                "speaker_actor_missing_or_deleted",
            ))?;
        if !matches!(&actor.kind, b"NPC_" | b"CREA") {
            return Err(Error::SpeakerUnavailable("speaker_actor_wrong_kind"));
        }
        speaker = Some(SpeakerField {
            field_index,
            decoded_offset: field.offset,
            raw_form,
            actor_key,
            actor_kind: actor.kind,
            field_sha256: format!("{:x}", Sha256::digest(field.data)),
        });
    }
    let speaker = speaker.ok_or(Error::SpeakerUnavailable("missing_anam"))?;
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.source_name == source_plugin)
        .ok_or(Error::InfoUnavailable("source_receipt_missing"))?;
    Ok((
        InfoSource {
            key: key.clone(),
            source_plugin,
            source_bytes: receipt.source_bytes,
            source_sha256: receipt.source_sha256.clone(),
            record_file_offset: header.offset,
            record_flags: header.flags,
            decoded_record_sha256: format!("{:x}", Sha256::digest(&record.payload)),
        },
        speaker,
    ))
}

impl<'a> Requests<'a> {
    pub fn prepare(
        store: &mut RecordStore,
        world: &World<'_>,
        content: &Content,
        sources: Sources<'a>,
        choice: Choice,
        limits: Limits,
    ) -> Result<Self, Error> {
        let (info, speaker) =
            selected_info(store, world, content, sources.actors, &choice.info, limits)?;
        let speaker_context = context::observe(
            world,
            content,
            sources.placements,
            sources.actors,
            choice.speaker_reference,
            limits.context,
        )?;
        if speaker_context.actor.key != &speaker.actor_key {
            return Err(Error::SpeakerReferenceMismatch);
        }
        let statistics = stats::Requests::prepare(
            world,
            sources.actors,
            sources.dependencies,
            &speaker.actor_key,
            limits.statistics,
        )?;
        let result = Self {
            choice,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
            revision: world.revision(),
            sources,
            info,
            speaker,
            statistics,
        };
        result.observe(store, world, content, limits)?;
        Ok(result)
    }

    pub fn observe<'request>(
        &'request self,
        store: &mut RecordStore,
        world: &World<'_>,
        content: &Content,
        limits: Limits,
    ) -> Result<Observation<'request, 'a>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        if self.revision != world.revision() {
            return Err(Error::RevisionChanged);
        }
        let (info, speaker) = selected_info(
            store,
            world,
            content,
            self.sources.actors,
            &self.choice.info,
            limits,
        )?;
        if info != self.info || speaker != self.speaker {
            return Err(Error::InfoUnavailable("selected_speaker_source_changed"));
        }
        let speaker_context = context::observe(
            world,
            content,
            self.sources.placements,
            self.sources.actors,
            self.choice.speaker_reference,
            limits.context,
        )?;
        if speaker_context.actor.key != &self.speaker.actor_key {
            return Err(Error::SpeakerReferenceMismatch);
        }
        let statistics = self.statistics.observe(world, limits.statistics)?;
        let relationships = faction_pair::observe_pair(
            world,
            content,
            self.sources.placements,
            self.sources.actors,
            self.sources.associations,
            self.sources.factions,
            self.choice.speaker_reference,
            self.choice.target_reference,
            limits.relationships,
        )?;
        if relationships.from.context.actor.key != &self.speaker.actor_key {
            return Err(Error::SpeakerReferenceMismatch);
        }
        let result = Observation {
            info,
            speaker,
            speaker_context,
            statistics,
            relationships,
            speaker_reference_matches_info: true,
            response_selection_supported: false,
            dialogue_conditions_supported: false,
            current_actor_values_supported: false,
            effective_faction_membership_supported: false,
            relationship_evaluation_supported: false,
            dialogue_eligibility_supported: false,
            original_behavior_verified: false,
            scope: "One explicitly selected INFO with one physical ANAM speaker linked to a fresh canonical placed-actor reference, exact authored stat/template candidates and an explicit directed actor pair. No response ordering, condition truth, current actor values, effective faction membership, relationship evaluation, dialogue eligibility, scripts or gameplay behavior",
        };
        serde_json::to_writer(
            &mut ProjectionBudget {
                bytes: 0,
                maximum: limits.max_projection_bytes,
            },
            &result,
        )?;
        Ok(result)
    }
}
