//! Authored scalar inputs over exact template origins, never evaluated actor values.
use crate::{World, identity::CampaignId};
use fallout_data::{
    actors::{
        self,
        dependencies::{self, TemplateCategory},
    },
    identity::FormKey,
    inventory,
    store::SourceReceipt,
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub template: dependencies::TemplateLimits,
    pub max_scalar_fields: usize,
    pub max_decoded_bytes: usize,
    pub max_components: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            template: dependencies::TemplateLimits::default(),
            max_scalar_fields: 131_072,
            max_decoded_bytes: 32 * 1024 * 1024,
            max_components: 16_384,
            max_visits: 2_000_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor stat request source cohort or campaign changed")]
    ContextChanged,
    #[error("actor stat source unavailable or inconsistent: {0}")]
    Source(&'static str),
    #[error("actor stat request {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Data(#[from] fallout_data::Error),
    #[error(transparent)]
    Projection(#[from] serde_json::Error),
}

/// This describes a physical declaration, not a formula or engine result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AutomaticCalculation {
    NotDeclaredForComponent,
    NpcDeclaration { enabled: bool },
    ConfigurationUnavailable,
}
#[derive(Serialize)]
pub struct Component {
    pub name: &'static str,
    pub template_category: TemplateCategory,
    pub template_mask: u16,
    pub template_flag_present: Option<bool>,
    /// The root declaration is selectable only when unique and not inherited.
    /// This never permits using its authored value as current runtime state.
    pub root_declaration_available: bool,
    pub automatic_calculation: AutomaticCalculation,
    pub evaluated_value: Option<serde_json::Value>,
}
#[derive(Serialize)]
pub struct FieldRequest<'a> {
    pub scalar_field_index: usize,
    pub field: &'a actors::fields::Field,
    pub ambiguous_source: bool,
    pub components: Vec<Component>,
}
#[derive(Serialize)]
pub struct CandidateScalars<'a> {
    pub template_source_index: usize,
    /// Complete existing physical fields and findings; no new decoder.
    pub definition: &'a actors::Definition<'a>,
}
#[derive(Serialize)]
pub struct MissingField {
    pub kind: [u8; 4],
    pub code: &'static str,
}
pub struct Requests<'a> {
    campaign: CampaignId,
    cohort: String,
    actor: &'a actors::Definition<'a>,
    template: dependencies::TemplateManifest<'a>,
    candidates: Vec<CandidateScalars<'a>>,
    fields: Vec<FieldRequest<'a>>,
    missing: Vec<MissingField>,
    scalar_fields: usize,
    decoded_bytes: usize,
    components: usize,
    visits: usize,
}
#[derive(Serialize)]
pub struct Observation<'a> {
    pub campaign: CampaignId,
    pub source_cohort_sha256: &'a str,
    pub state_revision: u64,
    pub actor_key: &'a FormKey,
    pub actor_kind: [u8; 4],
    pub actor_source: &'a inventory::Source,
    pub template_request: &'a dependencies::TemplateManifest<'a>,
    pub candidate_scalars: &'a [CandidateScalars<'a>],
    pub fields: &'a [FieldRequest<'a>],
    pub missing_fields: &'a [MissingField],
    pub scalar_fields: usize,
    pub decoded_bytes: usize,
    pub component_requests: usize,
    pub preparation_visits: usize,
    pub current_actor_values: Option<serde_json::Value>,
    pub actor_reference_bound: bool,
    pub initialization_supported: bool,
    pub automatic_calculation_supported: bool,
    pub original_behavior_verified: bool,
    pub scope: &'static str,
}

fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
fn same_sources(left: &[SourceReceipt], right: &[SourceReceipt]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(a, b)| {
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
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor stat projection byte budget"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> Requests<'a> {
    pub fn prepare(
        world: &World<'_>,
        actors: &'a actors::Catalogue<'a>,
        dependencies: &'a dependencies::Catalogue<'a>,
        actor_key: &FormKey,
        limits: Limits,
    ) -> Result<Self, Error> {
        for (sources, digest) in [
            (actors.sources(), actors.winning_content_sha256()),
            (
                dependencies.sources(),
                dependencies.winning_content_sha256(),
            ),
        ] {
            if !same_sources(sources, &world.catalogue().sources)
                || digest != world.catalogue().winning_content_sha256()
            {
                return Err(Error::ContextChanged);
            }
        }
        let actor = actors
            .get(actor_key)
            .filter(|actor| !actor.deleted)
            .ok_or(Error::Source("chosen actor winner missing or deleted"))?;
        let template = dependencies.template_manifest(actor_key, limits.template)?;
        let mut result = Self {
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
            actor,
            template,
            candidates: Vec::new(),
            fields: Vec::new(),
            missing: Vec::new(),
            scalar_fields: 0,
            decoded_bytes: 0,
            components: 0,
            visits: 0,
        };
        result.visits = result.template.field_visits;
        admit(result.visits, limits.max_visits, "visit")?;
        for (index, source) in result.template.candidate_sources.iter().enumerate() {
            let definition = actors
                .get(source.key)
                .filter(|actor| !actor.deleted)
                .ok_or(Error::Source(
                    "template candidate actor scalars unavailable",
                ))?;
            let left = definition.source;
            let right = source.source;
            if definition.kind != source.header.kind
                || definition.record_version != Some(source.header.version)
                || left.plugin != right.plugin
                || left.sha256 != right.sha256
                || left.record_file_offset != right.record_file_offset
                || left.record_flags != right.record_flags
                || left.decoded_record_sha256 != right.decoded_record_sha256
            {
                return Err(Error::Source("template candidate scalar origin differs"));
            }
            let record = definition
                .record()
                .ok_or(Error::Source("candidate body unavailable"))?;
            result.scalar_fields = result
                .scalar_fields
                .checked_add(definition.fields.len())
                .ok_or(Error::Capacity("scalar field"))?;
            result.decoded_bytes = result
                .decoded_bytes
                .checked_add(record.payload.len())
                .ok_or(Error::Capacity("decoded byte"))?;
            result.visits = result
                .visits
                .checked_add(definition.fields.len())
                .ok_or(Error::Capacity("visit"))?;
            admit(
                result.scalar_fields,
                limits.max_scalar_fields,
                "scalar field",
            )?;
            admit(
                result.decoded_bytes,
                limits.max_decoded_bytes,
                "decoded byte",
            )?;
            admit(result.visits, limits.max_visits, "visit")?;
            if let Some(configuration) = &source.configuration {
                let inventory_field = definition
                    .inventory_definition()
                    .fields
                    .get(configuration.inventory_field_index)
                    .ok_or(Error::Source("configuration field index differs"))?;
                if !definition.fields.iter().any(|field| {
                    field.kind == *b"ACBS"
                        && field.decoded_offset == inventory_field.decoded_offset
                        && field.bytes == inventory_field.bytes
                        && field.sha256 == inventory_field.sha256
                }) {
                    return Err(Error::Source("configuration scalar field origin differs"));
                }
                result.visits = result
                    .visits
                    .checked_add(definition.fields.len())
                    .ok_or(Error::Capacity("visit"))?;
                admit(result.visits, limits.max_visits, "visit")?;
            }
            result.candidates.push(CandidateScalars {
                template_source_index: index,
                definition,
            });
        }
        let root = result
            .template
            .candidate_sources
            .iter()
            .find(|source| source.key == actor_key)
            .ok_or(Error::Source("template root candidate absent"))?;
        let auto = if actor.kind == *b"NPC_" {
            root.configuration.as_ref().map_or(
                AutomaticCalculation::ConfigurationUnavailable,
                |configuration| AutomaticCalculation::NpcDeclaration {
                    enabled: configuration.flags & 0x10 != 0,
                },
            )
        } else {
            AutomaticCalculation::NotDeclaredForComponent
        };
        let mut occurrences = [0usize; 3];
        for field in &actor.fields {
            let index = match &field.value {
                actors::fields::Value::Configuration { .. } => Some(0),
                actors::fields::Value::NpcData { .. }
                | actors::fields::Value::CreatureData { .. } => Some(1),
                actors::fields::Value::NpcSkills { .. } => Some(2),
                _ => None,
            };
            if let Some(index) = index {
                occurrences[index] += 1;
            }
        }
        result.visits = result
            .visits
            .checked_add(actor.fields.len().saturating_mul(2))
            .ok_or(Error::Capacity("visit"))?;
        admit(result.visits, limits.max_visits, "visit")?;
        for (index, field) in actor.fields.iter().enumerate() {
            use AutomaticCalculation::NotDeclaredForComponent as Plain;
            use TemplateCategory::{AiData, Stats, Traits};
            use actors::fields::Value;
            let (group, names): (usize, Vec<_>) = match &field.value {
                Value::Configuration { .. } => (
                    0,
                    vec![
                        ("fatigue", Stats, Plain),
                        ("barter_gold", AiData, Plain),
                        ("level_word", Stats, Plain),
                        ("player_level_multiplier_flag", Stats, Plain),
                        ("calc_min", Stats, Plain),
                        ("calc_max", Stats, Plain),
                        ("speed_multiplier", Stats, Plain),
                        ("karma_bits", Traits, Plain),
                        ("disposition_base", Traits, Plain),
                    ],
                ),
                Value::NpcData { .. } => (
                    1,
                    vec![("base_health", Stats, Plain), ("attributes", Stats, auto)],
                ),
                Value::NpcSkills { .. } => (
                    2,
                    vec![
                        ("skill_values", Stats, auto),
                        ("skill_offsets", Stats, auto),
                    ],
                ),
                Value::CreatureData { .. } => (
                    1,
                    vec![
                        ("creature_type", Traits, Plain),
                        ("combat_skill", Stats, Plain),
                        ("magic_skill", Stats, Plain),
                        ("stealth_skill", Stats, Plain),
                        ("health", Stats, Plain),
                        ("damage", Stats, Plain),
                        ("attributes", Stats, Plain),
                    ],
                ),
                Value::Opaque => continue,
            };
            result.components = result
                .components
                .checked_add(names.len())
                .ok_or(Error::Capacity("component"))?;
            admit(result.components, limits.max_components, "component")?;
            let components = names
                .into_iter()
                .map(|(name, category, automatic)| {
                    let declaration = result
                        .template
                        .categories
                        .iter()
                        .find(|request| request.category == category)
                        .expect("fixed template categories");
                    Component {
                        name,
                        template_category: category,
                        template_mask: category.mask(),
                        template_flag_present: declaration.template_flag_present,
                        root_declaration_available: occurrences[group] == 1
                            && matches!(
                                declaration.declaration,
                                dependencies::DeclarationSelection::AuthoredSource { .. }
                            )
                            && !matches!(
                                automatic,
                                AutomaticCalculation::NpcDeclaration { enabled: true }
                                    | AutomaticCalculation::ConfigurationUnavailable
                            ),
                        automatic_calculation: automatic,
                        evaluated_value: None,
                    }
                })
                .collect();
            result.fields.push(FieldRequest {
                scalar_field_index: index,
                field,
                ambiguous_source: occurrences[group] > 1,
                components,
            });
        }
        for (index, (kind, code)) in [
            (*b"ACBS", "configuration_unavailable"),
            (*b"DATA", "actor_data_unavailable"),
            (*b"DNAM", "npc_skills_unavailable"),
        ]
        .into_iter()
        .enumerate()
        {
            if occurrences[index] == 0 && (index != 2 || actor.kind == *b"NPC_") {
                result.missing.push(MissingField { kind, code });
            }
        }
        Ok(result)
    }
    pub fn observe<'b>(
        &'b self,
        world: &World<'_>,
        limits: Limits,
    ) -> Result<Observation<'b>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        admit(
            self.template.candidate_sources.len(),
            limits.template.max_sources,
            "template source",
        )?;
        admit(
            self.template.links.len(),
            limits.template.max_links,
            "template link",
        )?;
        admit(
            self.template.issues.len(),
            limits.template.max_issues,
            "template issue",
        )?;
        admit(
            self.template.field_visits,
            limits.template.max_field_visits,
            "template field visit",
        )?;
        admit(
            self.template.structural_closure.nodes.len(),
            limits.template.closure.max_nodes,
            "closure node",
        )?;
        admit(
            self.template.structural_closure.edge_indices.len(),
            limits.template.closure.max_edges,
            "closure edge",
        )?;
        admit(self.scalar_fields, limits.max_scalar_fields, "scalar field")?;
        admit(self.decoded_bytes, limits.max_decoded_bytes, "decoded byte")?;
        admit(self.components, limits.max_components, "component")?;
        admit(self.visits, limits.max_visits, "visit")?;
        let observation = Observation {
            campaign: self.campaign,
            source_cohort_sha256: &self.cohort,
            state_revision: world.revision(),
            actor_key: self.actor.key,
            actor_kind: self.actor.kind,
            actor_source: self.actor.source,
            template_request: &self.template,
            candidate_scalars: &self.candidates,
            fields: &self.fields,
            missing_fields: &self.missing,
            scalar_fields: self.scalar_fields,
            decoded_bytes: self.decoded_bytes,
            component_requests: self.components,
            preparation_visits: self.visits,
            current_actor_values: None,
            actor_reference_bound: false,
            initialization_supported: false,
            automatic_calculation_supported: false,
            original_behavior_verified: false,
            scope: "Exact authored ACBS/DATA/DNAM and template candidate origins; category declarations and NPC auto-calc source flag, no inherited/effective/current actor values, actor reference binding, initialization, level conversion or gameplay",
        };
        serde_json::to_writer(
            &mut ProjectionBudget {
                bytes: 0,
                maximum: limits.max_projection_bytes,
            },
            &observation,
        )?;
        Ok(observation)
    }
}
