//! Read-only join from canonical reference identity to exact placed actor source.
use crate::{World, foreign::Content, identity::ReferenceId, reference_state};
use fallout_data::{
    actors::{self, placements},
    identity::FormKey,
    inventory,
    store::SourceReceipt,
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_fields: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_fields: 65_536,
            max_visits: 2_000_000,
            max_projection_bytes: 8 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor context source cohort changed")]
    ContextChanged,
    #[error("actor context source unavailable: {0}")]
    Unavailable(&'static str),
    #[error("actor context source join is inconsistent: {0}")]
    SourceMismatch(&'static str),
    #[error("actor context {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}
#[derive(Debug, Serialize)]
pub struct ActorSource<'a> {
    pub key: &'a FormKey,
    pub source: &'a inventory::Source,
    pub kind: [u8; 4],
    pub record_version: u16,
    pub fields: &'a [actors::fields::Field],
    pub findings: &'a [actors::fields::Finding],
}
/// Borrowed source declarations and a freshly acquired canonical observation.
/// This cannot be deserialized into authority or used to register/mutate state.
#[derive(Debug, Serialize)]
pub struct Observation<'a> {
    pub reference: reference_state::View,
    pub placement: &'a placements::Definition,
    pub actor: ActorSource<'a>,
    pub base_field_index: usize,
    pub transform_field_index: usize,
    pub fields: usize,
    pub visits: usize,
    pub actor_initialization_supported: bool,
    pub authored_enable_evaluated: bool,
    pub scope: &'static str,
}
fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
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
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= self.maximum)
            .ok_or_else(|| std::io::Error::other("actor context projection byte budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn observe<'a>(
    world: &World<'_>,
    content: &Content,
    placements: &'a placements::Catalogue,
    actors: &'a actors::Catalogue<'_>,
    reference: ReferenceId,
    limits: Limits,
) -> Result<Observation<'a>, Error> {
    content.validate_world(world)?;
    let mut visits = 0usize;
    for (sources, digest) in [
        (placements.sources(), placements.winning_content_sha256()),
        (actors.sources(), actors.winning_content_sha256()),
    ] {
        admit(sources.len(), limits.max_sources, "source")?;
        visits = visits
            .checked_add(sources.len())
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;
        if !same_sources(sources, &world.catalogue().sources)
            || digest != world.catalogue().winning_content_sha256()
        {
            return Err(Error::ContextChanged);
        }
    }
    let view = world.reference_view(reference)?;
    let origin = view
        .authored()
        .ok_or(Error::Unavailable("reference_has_no_authored_origin"))?;
    let placed_form = content.source_form(world, origin)?;
    let expected = match placed_form.kind {
        kind if kind == *b"ACHR" => *b"NPC_",
        kind if kind == *b"ACRE" => *b"CREA",
        _ => {
            return Err(Error::Unavailable(
                "authored_origin_is_not_an_actor_placement",
            ));
        }
    };
    let placement = placements
        .get(origin)
        .filter(|p| !p.deleted)
        .ok_or(Error::Unavailable("placed_actor_winner_unavailable"))?;
    if placement.header.kind != placed_form.kind || placement.header.flags != placed_form.flags {
        return Err(Error::SourceMismatch("placement_header"));
    }
    let core = placement
        .core
        .as_ref()
        .ok_or(Error::Unavailable("placed_actor_core_unavailable"))?;
    let binding = placement
        .base
        .as_ref()
        .filter(|b| b.status == inventory::Status::Defined)
        .ok_or(Error::Unavailable("placed_actor_base_unavailable"))?;
    if placement.base_schema_kind_allowed != Some(true) {
        return Err(Error::Unavailable("placed_actor_base_wrong_kind"));
    }
    let key = binding
        .key
        .as_ref()
        .ok_or(Error::SourceMismatch("base_binding_key"))?;
    let target = binding
        .target
        .as_ref()
        .ok_or(Error::SourceMismatch("base_binding_target"))?;
    let actor = actors
        .get(key)
        .filter(|a| !a.deleted)
        .ok_or(Error::Unavailable("actor_base_winner_unavailable"))?;
    let actor_form = content.source_form(world, key)?;
    if actor.kind != expected || actor_form.kind != expected || target.kind != expected {
        return Err(Error::Unavailable("actor_base_wrong_kind"));
    }
    if core.base.value != binding.raw_form
        || target.source_plugin != actor.source.plugin
        || target.record_file_offset != actor.source.record_file_offset
        || target.record_flags != actor.source.record_flags
        || actor_form.flags != actor.source.record_flags
    {
        return Err(Error::SourceMismatch("actor_base_winning_source"));
    }
    let version = actor
        .record_version
        .ok_or(Error::Unavailable("actor_base_body_unavailable"))?;
    let fields = placement
        .fields
        .len()
        .checked_add(actor.fields.len())
        .ok_or(Error::Capacity("field"))?;
    admit(fields, limits.max_fields, "field")?;
    visits = visits
        .checked_add(fields)
        .and_then(|n| n.checked_add(7))
        .ok_or(Error::Capacity("visit"))?;
    admit(visits, limits.max_visits, "visit")?;
    let mut base_indices = Vec::new();
    let mut transform_indices = Vec::new();
    for (index, field) in placement.fields.iter().enumerate() {
        if field.kind == *b"NAME" {
            base_indices.push(index);
        }
        if field.kind == *b"DATA" {
            transform_indices.push(index);
        }
    }
    if base_indices.len() != 1 || transform_indices.len() != 1 {
        return Err(Error::Unavailable("ambiguous_placement_core_declarations"));
    }
    let base_field_index = base_indices[0];
    let transform_field_index = transform_indices[0];
    if placement.fields[base_field_index].decoded_offset as usize != core.base.decoded_offset
        || placement.fields[transform_field_index].decoded_offset as usize
            != core.transform_decoded_offset
    {
        return Err(Error::SourceMismatch("placement_core_origins"));
    }
    // Source disabled flags and source DATA do not supply canonical enable/pose.
    let result = Observation {
        reference: view,
        placement,
        actor: ActorSource {
            key: actor.key,
            source: actor.source,
            kind: actor.kind,
            record_version: version,
            fields: &actor.fields,
            findings: &actor.findings,
        },
        base_field_index,
        transform_field_index,
        fields,
        visits,
        actor_initialization_supported: false,
        authored_enable_evaluated: false,
        scope: "Exact authored ACHR/ACRE placement and winning NPC_/CREA base joined to one existing canonical reference; source transform/flags separate from optional current pose/enable, no registration, actor initialization, defaults, template evaluation or state mutation",
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
