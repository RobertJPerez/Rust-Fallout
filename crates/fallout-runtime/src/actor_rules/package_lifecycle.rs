//! A source-bound Travel route candidate, never package scheduling or execution.
use crate::{
    World,
    actor_rules::{packages as actor_packages, route_requests},
    foreign::Content,
    navigation,
};
use fallout_data::{
    actors::{package_dependencies, packages as source_packages},
    identity::FormKey,
    inventory,
    loaded_scripts::{OwnerKind, ScriptKey},
    store::RecordStore,
};
use serde::Serialize;
use std::{collections::BTreeMap, io::Write};

const TRAVEL_PACKAGE_TYPE: u8 = 6;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub route: route_requests::Limits,
    pub max_event_markers: usize,
    pub max_event_fields: usize,
    pub max_script_units: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            route: route_requests::Limits::default(),
            max_event_markers: 4096,
            max_event_fields: 16_384,
            max_script_units: 4096,
            max_projection_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Explicit actor package occurrence and caller route selection.
pub struct Request<'a, 'source> {
    pub capability: &'a actor_packages::Capability<'source>,
    pub occurrence_index: usize,
    pub query: &'a route_requests::Query,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor package candidate source cohort changed")]
    ContextChanged,
    #[error("actor package candidate source identity changed")]
    SourceChanged,
    #[error("actor package occurrence {0} is unavailable")]
    OccurrenceUnavailable(usize),
    #[error("selected actor package occurrence is not a resolved PACK source")]
    PackageUnavailable,
    #[error("selected PACK source is incomplete")]
    IncompleteSource,
    #[error("selected PACK source has no unique supported PKDT type")]
    PackageTypeUnavailable,
    #[error("PACK type {0} is unsupported by the Travel candidate")]
    UnsupportedPackageType(u8),
    #[error("selected Travel destination is not an admitted literal CELL")]
    DestinationUnavailable,
    #[error("selected Travel route has no found path")]
    RouteUnavailable,
    #[error("actor package candidate {0} budget exceeded")]
    Capacity(&'static str),
    #[error("actor package candidate projection failed: {0}")]
    Projection(#[from] serde_json::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Route(#[from] route_requests::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhysicalEventKind {
    Poba,
    Poca,
    Poea,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct EventMarkerIdentity {
    pub kind: [u8; 4],
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventFieldIdentity {
    pub kind: [u8; 4],
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub sha256: String,
    pub physical_marker: Option<EventMarkerIdentity>,
    pub binding_status: Option<inventory::Status>,
    pub binding_key: Option<FormKey>,
    pub schema_kind_allowed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompiledSourceIdentity {
    pub sha256: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddedScriptIdentity {
    pub field_index: usize,
    pub physical_marker: Option<EventMarkerIdentity>,
    pub key: ScriptKey,
    pub version_sha256: String,
    pub source_plugin: String,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_record_sha256: String,
    pub metadata_sha256: String,
    pub compiled_source: Option<CompiledSourceIdentity>,
    pub owner_kind: OwnerKind,
    pub owner_schema_verified: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventDeclaration {
    /// Raw physical marker identity. It does not define dispatch or lifecycle order.
    pub marker: EventMarkerIdentity,
    pub fields: Vec<EventFieldIdentity>,
    pub scripts: Vec<EmbeddedScriptIdentity>,
    pub embedded_script_units_without_compiled_source: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordSourceIdentity {
    pub key: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_record_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackageOccurrenceIdentity {
    pub occurrence_index: usize,
    pub actor_field_index: usize,
    pub actor_field_decoded_offset: u32,
    pub actor_field_sha256: String,
    pub package_key: FormKey,
}

#[derive(Debug, Clone, Copy, Serialize, thiserror::Error)]
#[error(
    "faithful package eligibility, lifecycle dispatch, script execution, scheduling and actor movement are unsupported"
)]
pub struct ExecutionRefusal {
    dependencies: &'static [&'static str],
}

/// Read-only proposal for one explicit source Travel occurrence and CELL route.
#[derive(Serialize)]
pub struct Candidate<'a> {
    pub actor: RecordSourceIdentity,
    pub occurrence: PackageOccurrenceIdentity,
    pub package: RecordSourceIdentity,
    pub package_type: u8,
    pub destination_cell: FormKey,
    /// POBA/POCA/POEA in physical field order; this is not temporal dispatch order.
    pub event_declarations: Vec<EventDeclaration>,
    pub event_fields_without_marker: Vec<EventFieldIdentity>,
    pub scripts_without_event_marker: Vec<EmbeddedScriptIdentity>,
    pub event_source_complete: bool,
    pub route: route_requests::Observation<'a>,
    pub lifecycle_dispatch_order_verified: bool,
    pub schedule_supported: bool,
    pub script_execution_supported: bool,
    pub movement_supported: bool,
    pub execution_supported: bool,
    pub state_changed: bool,
    pub refusal: ExecutionRefusal,
    pub scope: &'static str,
}
impl Candidate<'_> {
    pub fn require_execution(&self) -> Result<(), ExecutionRefusal> {
        Err(self.refusal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MarkerKey {
    kind: [u8; 4],
    field_index: usize,
    field_decoded_offset: u32,
}
impl From<&package_dependencies::Marker> for MarkerKey {
    fn from(marker: &package_dependencies::Marker) -> Self {
        Self {
            kind: marker.kind,
            field_index: marker.field_index,
            field_decoded_offset: marker.field_decoded_offset,
        }
    }
}
fn event_kind(kind: [u8; 4]) -> Option<PhysicalEventKind> {
    match &kind {
        b"POBA" => Some(PhysicalEventKind::Poba),
        b"POCA" => Some(PhysicalEventKind::Poca),
        b"POEA" => Some(PhysicalEventKind::Poea),
        _ => None,
    }
}
fn marker_identity(
    marker: &package_dependencies::Marker,
    fields: &[package_dependencies::EventField],
) -> Result<EventMarkerIdentity, Error> {
    let source = fields
        .iter()
        .find(|field| field.field_index == marker.field_index)
        .ok_or(Error::SourceChanged)?;
    if source.field_kind != marker.kind
        || source.field_decoded_offset != marker.field_decoded_offset
    {
        return Err(Error::SourceChanged);
    }
    Ok(EventMarkerIdentity {
        kind: marker.kind,
        field_index: marker.field_index,
        field_decoded_offset: marker.field_decoded_offset,
        sha256: source.sha256.clone(),
    })
}
fn physical_field(
    field: &package_dependencies::EventField,
    all_fields: &[package_dependencies::EventField],
) -> Result<EventFieldIdentity, Error> {
    let (binding_status, binding_key, schema_kind_allowed) = match &field.value {
        package_dependencies::EventValue::Marker => (None, None, None),
        package_dependencies::EventValue::Link {
            binding,
            schema_kind_allowed,
        } => (
            Some(binding.status),
            binding.key.clone(),
            *schema_kind_allowed,
        ),
    };
    Ok(EventFieldIdentity {
        kind: field.field_kind,
        field_index: field.field_index,
        field_decoded_offset: field.field_decoded_offset,
        sha256: field.sha256.clone(),
        physical_marker: field
            .physical_marker
            .as_ref()
            .map(|marker| marker_identity(marker, all_fields))
            .transpose()?,
        binding_status,
        binding_key,
        schema_kind_allowed,
    })
}
fn script_identity(
    script: &package_dependencies::Embedded<'_>,
    fields: &[package_dependencies::EventField],
) -> Result<EmbeddedScriptIdentity, Error> {
    Ok(EmbeddedScriptIdentity {
        field_index: script.field_index,
        physical_marker: script
            .physical_marker
            .as_ref()
            .map(|marker| marker_identity(marker, fields))
            .transpose()?,
        key: script.handle.key.clone(),
        version_sha256: script.handle.version_sha256.clone(),
        source_plugin: script.version.source_plugin.clone(),
        source_sha256: script.version.source_sha256.clone(),
        record_file_offset: script.version.record_file_offset,
        record_flags: script.version.record_flags,
        decoded_record_sha256: script.version.decoded_record_sha256.clone(),
        metadata_sha256: script.version.metadata_sha256.clone(),
        compiled_source: script.version.compiled_sha256.as_ref().map(|sha256| {
            CompiledSourceIdentity {
                sha256: sha256.clone(),
                bytes: script.version.compiled_bytes.unwrap_or_default(),
            }
        }),
        owner_kind: script.owner.kind,
        owner_schema_verified: script.owner.schema_ownership_verified,
        issues: script.issues.to_vec(),
    })
}
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor package candidate projection budget",
            ));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Builds a read-only candidate from one exact actor PKID occurrence and a caller route.
/// Only raw PACK type 6 with an admitted literal CELL operand and a found route is returned.
pub fn observe<'request, 'source>(
    world: &World<'_>,
    content: &Content,
    store: &mut RecordStore,
    packages: &'request source_packages::Catalogue,
    request: Request<'request, 'source>,
    limits: Limits,
) -> Result<Candidate<'request>, Error> {
    let capability = request.capability;
    let occurrence_index = request.occurrence_index;
    let query = request.query;
    if capability.observation.source_cohort_sha256 != world.catalogue_fingerprint() {
        return Err(Error::ContextChanged);
    }
    content.validate_world(world)?;
    let actor_source = capability.observation.actor_source;
    let actor_form = content.source_form(world, capability.observation.actor_key)?;
    if !matches!(&actor_form.kind, b"NPC_" | b"CREA")
        || actor_form.flags != actor_source.record_flags
    {
        return Err(Error::SourceChanged);
    }
    let actor = RecordSourceIdentity {
        key: capability.observation.actor_key.clone(),
        source_plugin: actor_source.plugin.clone(),
        source_sha256: actor_source.sha256.clone(),
        record_file_offset: actor_source.record_file_offset,
        record_flags: actor_source.record_flags,
        decoded_record_sha256: actor_source.decoded_record_sha256.clone(),
    };

    let observation = capability
        .observation
        .packages
        .get(occurrence_index)
        .ok_or(Error::OccurrenceUnavailable(occurrence_index))?;
    let input = capability
        .package_inputs
        .get(occurrence_index)
        .filter(|input| input.occurrence_index == occurrence_index)
        .ok_or(Error::OccurrenceUnavailable(occurrence_index))?;
    if observation.association.role != fallout_data::actors::associations::Role::Package
        || observation.field.kind != *b"PKID"
        || observation.association.schema_kind_allowed != Some(true)
        || observation.association.binding.status != inventory::Status::Defined
        || input.source.is_none()
    {
        return Err(Error::PackageUnavailable);
    }
    let package_source = observation
        .package
        .as_ref()
        .ok_or(Error::PackageUnavailable)?;
    let package_definition = input.source.ok_or(Error::PackageUnavailable)?;
    if package_source.key != &package_definition.key
        || observation.association.binding.key.as_ref() != Some(package_source.key)
        || package_definition.deleted
        || package_definition.header.kind != *b"PACK"
        || !input.physical_source_complete
    {
        return Err(Error::IncompleteSource);
    }
    if query.package != *package_source.key
        || query.source_sha256 != package_source.source.sha256
        || query.record_file_offset != package_source.source.record_file_offset
    {
        return Err(Error::SourceChanged);
    }
    let package_form = content.source_form(world, package_source.key)?;
    if package_form.kind != *b"PACK" || package_form.flags != package_source.header.flags {
        return Err(Error::SourceChanged);
    }

    let mut pkdt = package_definition
        .fields
        .iter()
        .filter(|field| field.kind == *b"PKDT");
    let Some(source_packages::Field {
        value: source_packages::Value::General { package_type, .. },
        ..
    }) = pkdt.next()
    else {
        return Err(Error::PackageTypeUnavailable);
    };
    if pkdt.next().is_some() {
        return Err(Error::PackageTypeUnavailable);
    }
    if *package_type != TRAVEL_PACKAGE_TYPE {
        return Err(Error::UnsupportedPackageType(*package_type));
    }

    if input.event_fields.len() > limits.max_event_fields {
        return Err(Error::Capacity("event field"));
    }
    if input.scripts.len() > limits.max_script_units {
        return Err(Error::Capacity("script unit"));
    }
    let mut previous = None;
    let mut marker_count = 0usize;
    for field in input.event_fields {
        if previous.is_some_and(|at| at >= field.field_index) {
            return Err(Error::SourceChanged);
        }
        previous = Some(field.field_index);
        marker_count += usize::from(event_kind(field.field_kind).is_some());
    }
    if marker_count > limits.max_event_markers {
        return Err(Error::Capacity("event marker"));
    }
    let mut previous_script = None;
    for script in input.scripts {
        if previous_script.is_some_and(|at| at >= script.field_index) {
            return Err(Error::SourceChanged);
        }
        previous_script = Some(script.field_index);
        if script.version.source_sha256 != package_source.source.sha256
            || script.version.source_plugin != package_source.source.plugin
            || script.version.record_file_offset != package_source.source.record_file_offset
            || script.version.record_flags != package_source.header.flags
        {
            return Err(Error::SourceChanged);
        }
    }

    let mut events = Vec::with_capacity(marker_count);
    let mut event_by_marker = BTreeMap::new();
    for field in input.event_fields {
        if event_kind(field.field_kind).is_none() {
            continue;
        }
        let marker = package_dependencies::Marker {
            kind: field.field_kind,
            field_index: field.field_index,
            field_decoded_offset: field.field_decoded_offset,
        };
        let identity = marker_identity(&marker, input.event_fields)?;
        let index = events.len();
        if event_by_marker
            .insert(MarkerKey::from(&marker), index)
            .is_some()
        {
            return Err(Error::SourceChanged);
        }
        events.push(EventDeclaration {
            marker: identity,
            fields: Vec::new(),
            scripts: Vec::new(),
            embedded_script_units_without_compiled_source: 0,
        });
    }

    let mut event_fields_without_marker = Vec::new();
    for field in input.event_fields {
        if event_kind(field.field_kind).is_some() {
            continue;
        }
        let identity = physical_field(field, input.event_fields)?;
        if let Some(marker) = field.physical_marker.as_ref()
            && let Some(index) = event_by_marker.get(&MarkerKey::from(marker)).copied()
        {
            events[index].fields.push(identity);
        } else {
            event_fields_without_marker.push(identity);
        }
    }

    let mut scripts_without_event_marker = Vec::new();
    for script in input.scripts {
        let identity = script_identity(script, input.event_fields)?;
        if let Some(marker) = script.physical_marker.as_ref()
            && let Some(index) = event_by_marker.get(&MarkerKey::from(marker)).copied()
        {
            events[index].embedded_script_units_without_compiled_source +=
                usize::from(identity.compiled_source.is_none());
            events[index].scripts.push(identity);
        } else {
            scripts_without_event_marker.push(identity);
        }
    }

    let route =
        route_requests::observe(world, content, store, packages, query, None, limits.route)?;
    if route.outcome() != route_requests::Outcome::EngineeringProposal {
        return Err(Error::DestinationUnavailable);
    }
    let operand = route
        .destination()
        .operands
        .iter()
        .find(|operand| {
            operand.field_index == query.field_index
                && operand.field_decoded_offset == query.field_decoded_offset
        })
        .ok_or(Error::SourceChanged)?;
    if !operand.binding_admitted
        || operand.alternative != source_packages::destinations::Alternative::Cell
        || operand.schema_kind_allowed != Some(true)
        || operand
            .binding
            .as_ref()
            .and_then(|binding| binding.key.as_ref())
            != Some(&query.destination)
        || query.goal.cell != query.destination
    {
        return Err(Error::DestinationUnavailable);
    }
    if !matches!(route.route(), Some(navigation::Route::Found { .. })) {
        return Err(Error::RouteUnavailable);
    }

    let result = Candidate {
        actor,
        occurrence: PackageOccurrenceIdentity {
            occurrence_index,
            actor_field_index: observation.association.field_index,
            actor_field_decoded_offset: observation.field.decoded_offset,
            actor_field_sha256: observation.field.sha256.clone(),
            package_key: package_source.key.clone(),
        },
        package: RecordSourceIdentity {
            key: package_source.key.clone(),
            source_plugin: package_source.source.plugin.clone(),
            source_sha256: package_source.source.sha256.clone(),
            record_file_offset: package_source.source.record_file_offset,
            record_flags: package_source.header.flags,
            decoded_record_sha256: package_source.source.decoded_record_sha256.clone(),
        },
        package_type: *package_type,
        destination_cell: query.destination.clone(),
        event_declarations: events,
        event_fields_without_marker,
        scripts_without_event_marker,
        event_source_complete: input.physical_source_complete,
        route,
        lifecycle_dispatch_order_verified: false,
        schedule_supported: false,
        script_execution_supported: false,
        movement_supported: false,
        execution_supported: false,
        state_changed: false,
        refusal: ExecutionRefusal {
            dependencies: &[
                "faithful_package_eligibility",
                "package_lifecycle_dispatch_order",
                "schedule_interpretation",
                "embedded_script_ownership_and_execution",
                "canonical_actor_movement",
            ],
        },
        scope: "One exact physical PKID occurrence, raw PACK type 6, admitted literal CELL and caller-supplied found source route; POBA/POCA/POEA and embedded source identities in physical source order only; no inferred lifecycle dispatch, scheduling, script execution or canonical mutation",
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
