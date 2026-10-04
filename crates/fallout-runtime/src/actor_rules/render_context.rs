//! Explicit source model occurrences qualified by a current placed-actor view.
use super::context;
use crate::{World, foreign::Content, reference_state::View};
use fallout_data::{
    actors::{self, dependencies, placements},
    assets::ArchiveAssets,
    identity::FormKey,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::{collections::BTreeMap, io::Write};

pub struct Sources<'a, 'source> {
    pub placements: &'a placements::Catalogue,
    pub actors: &'a actors::Catalogue<'source>,
    pub dependencies: &'a dependencies::Catalogue<'source>,
    pub assets: &'a ArchiveAssets,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Occurrence {
    pub source: FormKey,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub field_byte_offset: u32,
    #[serde(deserialize_with = "role_input")]
    pub role: dependencies::RenderRole,
}

// Request JSON mapping only; the published source-role producer stays unchanged.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum RoleInput {
    ActorModel {},
    CreatureModelList {},
    AnimationList {},
    RaceHead { part_index: u32 },
    RaceBody { part_index: u32 },
    HeadPart {},
    Hair {},
    Eyes {},
}
fn role_input<'de, D: Deserializer<'de>>(input: D) -> Result<dependencies::RenderRole, D::Error> {
    use dependencies::RenderRole as R;
    Ok(match RoleInput::deserialize(input)? {
        RoleInput::ActorModel {} => R::ActorModel,
        RoleInput::CreatureModelList {} => R::CreatureModelList,
        RoleInput::AnimationList {} => R::AnimationList,
        RoleInput::RaceHead { part_index } => R::RaceHead { part_index },
        RoleInput::RaceBody { part_index } => R::RaceBody { part_index },
        RoleInput::HeadPart {} => R::HeadPart,
        RoleInput::Hair {} => R::Hair,
        RoleInput::Eyes {} => R::Eyes,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub context: context::Limits,
    pub render: dependencies::RenderLimits,
    pub max_sources: usize,
    pub max_selected: usize,
    pub max_identity_bytes: usize,
    pub max_visits: usize,
    pub max_view_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            context: context::Limits::default(),
            render: dependencies::RenderLimits::default(),
            max_sources: 256,
            max_selected: 1024,
            max_identity_bytes: 1024 * 1024,
            max_visits: 2_000_000,
            max_view_bytes: 64 * 1024,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor render context canonical view or source cohort changed")]
    ContextChanged,
    #[error("actor render context source identity or role differs")]
    SourceChanged,
    #[error("actor render context selected occurrence is unavailable or ambiguous")]
    SelectionUnavailable,
    #[error("actor render context input identity is invalid")]
    InvalidIdentity,
    #[error("actor render context {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Context(#[from] context::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Source(#[from] fallout_data::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    ComponentUnavailable,
    Disabled,
    ScaleUnavailable,
    SourceUnavailable,
    NoSelection,
    SelectedPathIsNotMesh,
    Admitted,
}

#[derive(Debug, Serialize)]
pub struct Selected {
    pub render_request_index: usize,
    pub manifest_path_index: usize,
    pub admitted: bool,
}

#[derive(Serialize)]
pub struct Observation<'a> {
    context: context::Observation<'a>,
    render: dependencies::RenderManifest<'a>,
    selected: Vec<Selected>,
    outcome: Outcome,
    visits: usize,
    identity_bytes: usize,
    gpu_ready: bool,
    state_changed: bool,
    scope: &'static str,
}
impl<'a> Observation<'a> {
    pub fn context(&self) -> &context::Observation<'a> {
        &self.context
    }
    pub fn render(&self) -> &dependencies::RenderManifest<'a> {
        &self.render
    }
    pub fn selected(&self) -> &[Selected] {
        &self.selected
    }
    pub fn outcome(&self) -> Outcome {
        self.outcome
    }
    /// Explicit selected model-source requests only; no implicit other paths.
    pub fn admitted_paths(&self) -> impl Iterator<Item = &dependencies::PathRequest> {
        self.selected
            .iter()
            .filter(|s| s.admitted)
            .map(|s| &self.render.manifest.paths[s.manifest_path_index])
    }
}

fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
fn add(total: &mut usize, amount: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    *total = total.checked_add(amount).ok_or(Error::Capacity(label))?;
    admit(*total, maximum, label)
}
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor render context projection budget",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn project(value: &impl Serialize, maximum: usize, label: &'static str) -> Result<(), Error> {
    serde_json::to_writer(ProjectionBudget { bytes: 0, maximum }, value)
        .map_err(|_| Error::Capacity(label))
}

pub fn observe<'a>(
    world: &World<'_>,
    content: &Content,
    sources: Sources<'a, '_>,
    view: &View,
    occurrences: &[Occurrence],
    limits: Limits,
) -> Result<Observation<'a>, Error> {
    admit(occurrences.len(), limits.max_selected, "selection")?;
    project(view, limits.max_view_bytes, "view byte")?;
    if view.campaign() != world.campaign()
        || view.catalogue_fingerprint() != world.catalogue_fingerprint()
        || view.revision() != world.revision()
        || view.authored() != world.reference_origin(view.reference())?
    {
        return Err(Error::ContextChanged);
    }
    content.validate_world(world)?;
    // Reuse the canonical private epoch/state validator. Staging has no effects:
    // drop the private proposal immediately and never call a commit method.
    if let Some(state) = view.state() {
        drop(world.stage_reference_state(view, state.clone())?);
    }
    admit(
        sources.dependencies.sources().len(),
        limits.max_sources,
        "source",
    )?;
    let expected = &world.catalogue().sources;
    let mut visits = sources.dependencies.sources().len();
    admit(visits, limits.max_visits, "visit")?;
    if sources.dependencies.sources().len() != expected.len()
        || !sources
            .dependencies
            .sources()
            .iter()
            .zip(expected)
            .all(|(a, b)| {
                a.source_name == b.source_name
                    && a.source_bytes == b.source_bytes
                    && a.source_sha256 == b.source_sha256
            })
        || sources.dependencies.winning_content_sha256()
            != world.catalogue().winning_content_sha256()
    {
        return Err(Error::ContextChanged);
    }
    let mut identity_bytes = 0;
    for occurrence in occurrences {
        add(&mut visits, 1, limits.max_visits, "visit")?;
        add(
            &mut identity_bytes,
            occurrence.source.origin_plugin.len(),
            limits.max_identity_bytes,
            "identity byte",
        )?;
        add(
            &mut identity_bytes,
            occurrence.source_sha256.len(),
            limits.max_identity_bytes,
            "identity byte",
        )?;
        crate::identity::valid_form(&occurrence.source)?;
        if occurrence.source_sha256.len() != 64
            || !occurrence
                .source_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::InvalidIdentity);
        }
    }
    let mut context_limits = limits.context;
    context_limits.max_sources = context_limits.max_sources.min(limits.max_sources);
    context_limits.max_visits = context_limits
        .max_visits
        .min(limits.max_visits.saturating_sub(visits));
    let context = context::observe(
        world,
        content,
        sources.placements,
        sources.actors,
        view.reference(),
        context_limits,
    )?;
    if context.reference.state() != view.state() {
        return Err(Error::ContextChanged);
    }
    add(&mut visits, context.visits, limits.max_visits, "visit")?;
    let mut render_limits = limits.render;
    render_limits.max_visits = render_limits
        .max_visits
        .min(limits.max_visits.saturating_sub(visits));
    let render =
        sources
            .dependencies
            .render_manifest(context.actor.key, sources.assets, render_limits)?;
    add(&mut visits, render.visits, limits.max_visits, "visit")?;
    for source in &render.sources {
        add(&mut visits, 1, limits.max_visits, "visit")?;
        let canonical = content.source_form(world, source.key)?;
        if canonical.kind != source.header.kind || canonical.flags != source.header.flags {
            return Err(Error::SourceChanged);
        }
    }
    // Index each already bounded producer request once. Selected repetitions
    // retain only indices, so repeating a long path does not clone it repeatedly.
    add(
        &mut visits,
        render.requests.len(),
        limits.max_visits,
        "visit",
    )?;
    let mut index = BTreeMap::new();
    for (request_index, request) in render.requests.iter().enumerate() {
        let path = &render.manifest.paths[request.manifest_path_index];
        if index
            .insert(
                (&path.source, path.field_index, path.field_byte_offset),
                request_index,
            )
            .is_some()
        {
            return Err(Error::SelectionUnavailable);
        }
    }
    add(&mut visits, occurrences.len(), limits.max_visits, "visit")?;
    let mut selected = Vec::with_capacity(occurrences.len());
    for occurrence in occurrences {
        let &request_index = index
            .get(&(
                &occurrence.source,
                occurrence.field_index,
                occurrence.field_byte_offset,
            ))
            .ok_or(Error::SelectionUnavailable)?;
        let request = &render.requests[request_index];
        let path = &render.manifest.paths[request.manifest_path_index];
        let source = sources
            .dependencies
            .get(&occurrence.source)
            .ok_or(Error::SourceChanged)?;
        if source.source.sha256 != occurrence.source_sha256
            || source.header.offset != occurrence.record_file_offset
            || path.field_decoded_offset != occurrence.field_decoded_offset
            || request.role != occurrence.role
        {
            return Err(Error::SourceChanged);
        }
        selected.push(Selected {
            render_request_index: request_index,
            manifest_path_index: request.manifest_path_index,
            admitted: false,
        });
    }
    add(
        &mut visits,
        selected
            .len()
            .checked_mul(2)
            .ok_or(Error::Capacity("visit"))?,
        limits.max_visits,
        "visit",
    )?;
    let outcome = match context.reference.state() {
        None => Outcome::ComponentUnavailable,
        Some(state) if !state.enabled() => Outcome::Disabled,
        Some(state) if state.pose().source_scale().is_none() => Outcome::ScaleUnavailable,
        Some(_) if !render.selected_requests_admitted => Outcome::SourceUnavailable,
        Some(_) if selected.is_empty() => Outcome::NoSelection,
        Some(_)
            if selected.iter().any(|s| {
                render.manifest.paths[s.manifest_path_index].role != dependencies::PathRole::Model
            }) =>
        {
            Outcome::SelectedPathIsNotMesh
        }
        Some(_) => Outcome::Admitted,
    };
    for item in &mut selected {
        item.admitted = outcome == Outcome::Admitted;
    }
    let result = Observation {
        context,
        render,
        selected,
        outcome,
        visits,
        identity_bytes,
        gpu_ready: false,
        state_changed: false,
        scope: "Exact caller-selected source model occurrences joined to fresh canonical actor placement/base and existing saved pose/enable; admitted only with explicit enabled state/positive scale and existing render source admission; no implicit paths, authored-pose reset, equipment, transform conversion, NIF/GPU readiness or canonical mutation",
    };
    project(&result, limits.max_projection_bytes, "projection byte")?;
    Ok(result)
}
