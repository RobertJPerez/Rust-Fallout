//! Source-backed placed actor inputs for a gameplay host or scene consumer.
//! It joins existing declarations without applying initialization behavior.
use crate::{
    World,
    actor_rules::{context, initialization_inputs, stats},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::{
    actors::{
        self, associations, classes, dependencies, initialization_inputs as source_initialization,
        placements, races,
    },
    identity::FormKey,
    inventory as source_inventory,
    store::RecordStore,
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub context: context::Limits,
    pub source_initialization: source_initialization::Limits,
    pub initialization: initialization_inputs::Limits,
    pub stats: stats::Limits,
    pub max_inventory_fields: usize,
    pub max_inventory_items: usize,
    pub max_extra_fields: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            context: context::Limits::default(),
            source_initialization: source_initialization::Limits::default(),
            initialization: initialization_inputs::Limits::default(),
            stats: stats::Limits::default(),
            max_inventory_fields: 65_536,
            max_inventory_items: 16_384,
            max_extra_fields: 16_384,
            max_visits: 262_144,
            max_projection_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor spawn-state request source cohort or campaign changed")]
    ContextChanged,
    #[error("actor spawn-state source is unavailable or inconsistent: {0}")]
    Source(&'static str),
    #[error("actor spawn-state {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Context(#[from] context::Error),
    #[error(transparent)]
    Initialization(#[from] initialization_inputs::Error),
    #[error(transparent)]
    Stats(#[from] stats::Error),
    #[error(transparent)]
    Data(#[from] fallout_data::Error),
    #[error(transparent)]
    Runtime(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Projection(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ItemResolution {
    DirectItemDefinition,
    LeveledSelectionRequired { target_kind: [u8; 4] },
    Unavailable { reason: &'static str },
}

#[derive(Debug, Serialize)]
pub struct InventoryItem<'a> {
    pub occurrence_index: usize,
    pub source_item: &'a source_inventory::fields::Item,
    pub source_field: &'a source_inventory::Field,
    pub item: &'a source_inventory::Binding,
    pub signed_source_count: i32,
    pub schema_kind_allowed: Option<bool>,
    pub coed_fields: Vec<&'a source_inventory::Field>,
    pub resolution: ItemResolution,
    /// A record-kind candidate only; this does not say the actor equips it.
    pub equipment_candidate_kind: Option<[u8; 4]>,
}

pub struct Requests<'a> {
    reference: ReferenceId,
    actor_key: FormKey,
    campaign: CampaignId,
    cohort: String,
    placements: &'a placements::Catalogue,
    actors: &'a actors::Catalogue<'a>,
    initialization: initialization_inputs::Requests<'a>,
    statistics: stats::Requests<'a>,
    inventory_fields: &'a [source_inventory::Field],
    inventory_findings: &'a [source_inventory::fields::Finding],
    inventory_items: Vec<InventoryItem<'a>>,
}

#[derive(Serialize)]
pub struct Observation<'request, 'source> {
    pub reference_context: context::Observation<'source>,
    pub initialization_inputs: initialization_inputs::Observation<'request, 'source>,
    pub statistics: stats::Observation<'request>,
    pub actor_source_cohort_sha256: &'request str,
    pub state_revision: u64,
    pub inventory_fields: &'source [source_inventory::Field],
    pub inventory_findings: &'source [source_inventory::fields::Finding],
    pub inventory_items: &'request [InventoryItem<'source>],
    pub actor_reference_bound: bool,
    pub actor_initialization_supported: bool,
    pub inventory_initialization_supported: bool,
    pub equipment_selection_supported: bool,
    pub current_actor_values: Option<serde_json::Value>,
    pub original_behavior_verified: bool,
    pub scope: &'static str,
}

struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor spawn-state projection budget"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn admit(value: usize, maximum: usize, label: &'static str) -> Result<(), Error> {
    if value > maximum {
        Err(Error::Capacity(label))
    } else {
        Ok(())
    }
}
fn projection(value: &impl Serialize, maximum: usize) -> Result<(), Error> {
    serde_json::to_writer(&mut ProjectionBudget { bytes: 0, maximum }, value)?;
    Ok(())
}

fn inventory_items<'a>(
    world: &World<'_>,
    content: &Content,
    definition: &'a source_inventory::Definition,
    limits: Limits,
) -> Result<Vec<InventoryItem<'a>>, Error> {
    admit(
        definition.fields.len(),
        limits.max_inventory_fields,
        "inventory field",
    )?;
    admit(
        definition.items.len(),
        limits.max_inventory_items,
        "inventory item",
    )?;
    let mut visits = definition.fields.len();
    let mut extra_fields = 0usize;
    let mut result = Vec::with_capacity(definition.items.len());
    for (occurrence_index, source_item) in definition.items.iter().enumerate() {
        visits = visits.checked_add(1).ok_or(Error::Capacity("visit"))?;
        let source_field = definition
            .fields
            .get(source_item.cnto_field)
            .ok_or(Error::Source("CNTO field index unavailable"))?;
        if source_field.kind != *b"CNTO" {
            return Err(Error::Source("inventory occurrence is not a CNTO field"));
        }
        let (item, signed_source_count, schema_kind_allowed) = match &source_field.value {
            source_inventory::Value::Item {
                item,
                count,
                schema_kind_allowed,
            } => (item, *count, *schema_kind_allowed),
            _ => return Err(Error::Source("CNTO field has no item declaration")),
        };
        extra_fields = extra_fields
            .checked_add(source_item.coed_fields.len())
            .ok_or(Error::Capacity("extra field"))?;
        admit(extra_fields, limits.max_extra_fields, "extra field")?;
        let mut coed = Vec::with_capacity(source_item.coed_fields.len());
        for &field_index in &source_item.coed_fields {
            visits = visits.checked_add(1).ok_or(Error::Capacity("visit"))?;
            let field = definition
                .fields
                .get(field_index)
                .ok_or(Error::Source("COED field index unavailable"))?;
            if field.kind != *b"COED"
                || !matches!(&field.value, source_inventory::Value::Extra { .. })
            {
                return Err(Error::Source("inventory extra field index differs"));
            }
            coed.push(field);
        }
        admit(visits, limits.max_visits, "visit")?;

        let (resolution, equipment_candidate_kind) = match item.status {
            source_inventory::Status::Null => (
                ItemResolution::Unavailable {
                    reason: "null_item_base",
                },
                None,
            ),
            source_inventory::Status::Missing => (
                ItemResolution::Unavailable {
                    reason: "missing_item_base",
                },
                None,
            ),
            source_inventory::Status::Deleted => (
                ItemResolution::Unavailable {
                    reason: "deleted_item_base",
                },
                None,
            ),
            source_inventory::Status::Defined => {
                let Some(target) = item.target.as_ref() else {
                    return Err(Error::Source("defined item binding has no target"));
                };
                if schema_kind_allowed == Some(false) {
                    (
                        ItemResolution::Unavailable {
                            reason: "item_kind_not_allowed",
                        },
                        None,
                    )
                } else if schema_kind_allowed != Some(true) {
                    (
                        ItemResolution::Unavailable {
                            reason: "item_kind_unavailable",
                        },
                        None,
                    )
                } else {
                    let key = item
                        .key
                        .as_ref()
                        .ok_or(Error::Source("defined item binding has no form key"))?;
                    let canonical = content.source_form(world, key)?;
                    if canonical.kind != target.kind || canonical.flags != target.record_flags {
                        return Err(Error::Source("item winner identity differs"));
                    }
                    if matches!(&target.kind, b"LVLI" | b"LVLC" | b"LVLN") {
                        (
                            ItemResolution::LeveledSelectionRequired {
                                target_kind: target.kind,
                            },
                            None,
                        )
                    } else {
                        let candidate = matches!(&target.kind, b"ARMO" | b"ARMA" | b"WEAP")
                            .then_some(target.kind);
                        (ItemResolution::DirectItemDefinition, candidate)
                    }
                }
            }
        };
        result.push(InventoryItem {
            occurrence_index,
            source_item,
            source_field,
            item,
            signed_source_count,
            schema_kind_allowed,
            coed_fields: coed,
            resolution,
            equipment_candidate_kind,
        });
    }
    Ok(result)
}

impl<'a> Requests<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        store: &mut RecordStore,
        world: &World<'_>,
        content: &Content,
        placements: &'a placements::Catalogue,
        actors: &'a actors::Catalogue<'a>,
        associations: &'a associations::Catalogue<'a>,
        races: &'a races::Catalogue,
        classes: &'a classes::Catalogue,
        dependencies: &'a dependencies::Catalogue<'a>,
        reference: ReferenceId,
        limits: Limits,
    ) -> Result<Self, Error> {
        let joined = context::observe(
            world,
            content,
            placements,
            actors,
            reference,
            limits.context,
        )?;
        let actor_key = joined.actor.key.clone();
        let actor = actors
            .get(&actor_key)
            .filter(|actor| !actor.deleted)
            .ok_or(Error::Source("placed actor base is unavailable"))?;
        if actor.kind != joined.actor.kind
            || actor.source.plugin != joined.actor.source.plugin
            || actor.source.sha256 != joined.actor.source.sha256
            || actor.source.record_file_offset != joined.actor.source.record_file_offset
            || actor.source.record_flags != joined.actor.source.record_flags
        {
            return Err(Error::Source("placed actor base provenance differs"));
        }
        let inventory = actor.inventory_definition();
        let items = inventory_items(world, content, inventory, limits)?;
        let source_manifest = source_initialization::request(
            store,
            actors,
            associations,
            races,
            classes,
            &actor_key,
            limits.source_initialization,
        )?;
        let initialization = initialization_inputs::Requests::prepare(
            world,
            content,
            source_manifest,
            limits.initialization,
        )?;
        let statistics =
            stats::Requests::prepare(world, actors, dependencies, &actor_key, limits.stats)?;
        let result = Self {
            reference,
            actor_key,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
            placements,
            actors,
            initialization,
            statistics,
            inventory_fields: &inventory.fields,
            inventory_findings: &inventory.findings,
            inventory_items: items,
        };
        result.observe(world, content, limits)?;
        Ok(result)
    }

    pub fn observe<'request>(
        &'request self,
        world: &World<'_>,
        content: &Content,
        limits: Limits,
    ) -> Result<Observation<'request, 'a>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        admit(
            self.inventory_fields.len(),
            limits.max_inventory_fields,
            "inventory field",
        )?;
        admit(
            self.inventory_items.len(),
            limits.max_inventory_items,
            "inventory item",
        )?;
        let extra_fields = self
            .inventory_items
            .iter()
            .try_fold(0usize, |total, item| {
                total
                    .checked_add(item.coed_fields.len())
                    .ok_or(Error::Capacity("extra field"))
            })?;
        admit(extra_fields, limits.max_extra_fields, "extra field")?;
        let visits = self
            .inventory_fields
            .len()
            .checked_add(self.inventory_items.len())
            .and_then(|total| total.checked_add(extra_fields))
            .ok_or(Error::Capacity("visit"))?;
        admit(visits, limits.max_visits, "visit")?;

        let reference_context = context::observe(
            world,
            content,
            self.placements,
            self.actors,
            self.reference,
            limits.context,
        )?;
        if reference_context.actor.key != &self.actor_key {
            return Err(Error::ContextChanged);
        }
        let initialization_inputs =
            self.initialization
                .observe(world, content, limits.initialization)?;
        let statistics = self.statistics.observe(world, limits.stats)?;
        let revision = world.revision();
        if reference_context.reference.revision() != revision
            || initialization_inputs.state_revision != revision
            || statistics.state_revision != revision
        {
            return Err(Error::ContextChanged);
        }
        let observation = Observation {
            reference_context,
            initialization_inputs,
            statistics,
            actor_source_cohort_sha256: &self.cohort,
            state_revision: revision,
            inventory_fields: self.inventory_fields,
            inventory_findings: self.inventory_findings,
            inventory_items: &self.inventory_items,
            actor_reference_bound: true,
            actor_initialization_supported: false,
            inventory_initialization_supported: false,
            equipment_selection_supported: false,
            current_actor_values: None,
            original_behavior_verified: false,
            scope: "Source-bound placed ACHR/ACRE and NPC_/CREA base joined to exact race/class declarations, template/stat candidate sources and physical CNTO/COED inventory entries. Direct item winners and record-kind equipment candidates are source identities only. No inherited selection, leveled roll, host count/facts, inventory creation, equipped-state inference, current actor values or faithful spawn initialization",
        };
        projection(&observation, limits.max_projection_bytes)?;
        Ok(observation)
    }
}
