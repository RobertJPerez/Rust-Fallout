//! Category declaration requests over the existing inventory/template graph.
//! Editor category masks classify source inputs, not retail inheritance.
use super::{Catalogue, ConfigurationOrigin};
use crate::{Error, Result, identity::FormKey, inventory, leveled, plugin};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy)]
pub struct TemplateLimits {
    pub closure: leveled::graph::Limits,
    pub max_sources: usize,
    pub max_links: usize,
    pub max_field_visits: usize,
    pub max_issues: usize,
}
impl Default for TemplateLimits {
    fn default() -> Self {
        Self {
            closure: leveled::graph::Limits::default(),
            max_sources: 4096,
            max_links: 16_384,
            max_field_visits: 2_000_000,
            max_issues: 16_384,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCategory {
    Traits,
    Stats,
    Factions,
    ActorEffects,
    AiData,
    AiPackages,
    ModelAnimation,
    BaseData,
    Inventory,
    Script,
}
impl TemplateCategory {
    pub fn mask(self) -> u16 {
        1 << self as u16
    }
}
const CATEGORIES: [TemplateCategory; 10] = [
    TemplateCategory::Traits,
    TemplateCategory::Stats,
    TemplateCategory::Factions,
    TemplateCategory::ActorEffects,
    TemplateCategory::AiData,
    TemplateCategory::AiPackages,
    TemplateCategory::ModelAnimation,
    TemplateCategory::BaseData,
    TemplateCategory::Inventory,
    TemplateCategory::Script,
];

#[derive(Debug, Serialize)]
pub struct TemplateSource<'a> {
    pub key: &'a FormKey,
    pub source: &'a inventory::Source,
    pub header: &'a plugin::RecordHeader,
    pub configuration: Option<ConfigurationOrigin>,
}
#[derive(Debug, Serialize)]
pub struct TemplateLink<'a> {
    pub graph_edge_index: usize,
    pub source: &'a FormKey,
    pub inventory_field_index: usize,
    pub field: &'a inventory::Field,
    pub binding: &'a inventory::Binding,
    pub ambiguous_source: bool,
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeclarationSelection {
    AuthoredSource { source_index: usize },
    Unsupported { reason: &'static str },
}
#[derive(Debug, Serialize)]
pub struct CategoryRequest {
    pub category: TemplateCategory,
    pub mask: u16,
    pub template_flag_present: Option<bool>,
    pub declaration: DeclarationSelection,
    pub runtime_value_evaluated: bool,
}
#[derive(Debug, Serialize)]
pub struct TemplateIssue {
    pub code: &'static str,
    pub source: FormKey,
    pub graph_edge_index: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct TemplateManifest<'a> {
    pub root: FormKey,
    pub winning_content_sha256: &'a str,
    /// Existing broad inventory/list/template closure, without re-parsing it.
    pub structural_closure: leveled::graph::Closure,
    /// Defined singleton actor TPLT candidates, not effective inherited winners.
    pub candidate_sources: Vec<TemplateSource<'a>>,
    pub links: Vec<TemplateLink<'a>>,
    pub categories: Vec<CategoryRequest>,
    /// Indices in candidate_sources, over retained direct actor template links.
    pub template_cycles: Vec<Vec<usize>>,
    pub issues: Vec<TemplateIssue>,
    pub field_visits: usize,
    pub template_inheritance_supported: bool,
    pub leveled_template_selection_supported: bool,
    pub scope: &'static str,
}
fn budget(label: &str) -> Error {
    Error::Unsupported(format!("actor template {label} budget exceeded"))
}
fn issue(
    issues: &mut Vec<TemplateIssue>,
    limits: TemplateLimits,
    code: &'static str,
    source: &FormKey,
    graph_edge_index: Option<usize>,
) -> Result<()> {
    if issues.len() >= limits.max_issues {
        return Err(budget("issue"));
    }
    issues.push(TemplateIssue {
        code,
        source: source.clone(),
        graph_edge_index,
    });
    Ok(())
}

impl Catalogue<'_> {
    pub fn template_manifest(
        &self,
        root: &FormKey,
        limits: TemplateLimits,
    ) -> Result<TemplateManifest<'_>> {
        let first = self
            .inventory
            .get(root)
            .filter(|source| !source.deleted && matches!(&source.kind, b"NPC_" | b"CREA"))
            .ok_or_else(|| Error::Resolution("template root actor missing or deleted".into()))?;
        if limits.max_sources == 0 {
            return Err(budget("source"));
        }
        let structural_closure = self.graph.closure(root, limits.closure)?;
        let mut selected = BTreeSet::from([root.clone()]);
        let mut queue = VecDeque::from([root.clone()]);
        let mut sources = BTreeMap::new();
        let mut links = Vec::new();
        let mut issues = Vec::new();
        let mut field_visits = 0usize;
        while let Some(key) = queue.pop_front() {
            let source = self
                .inventory
                .get(&key)
                .filter(|source| !source.deleted)
                .ok_or_else(|| {
                    Error::Resolution("template candidate actor winner unavailable".into())
                })?;
            let definition = self.get(&key).ok_or_else(|| {
                Error::Resolution("template candidate source header unavailable".into())
            })?;
            if source.fields.len() > limits.max_field_visits.saturating_sub(field_visits) {
                return Err(budget("field visit"));
            }
            field_visits += source.fields.len();
            let mut configuration = None;
            let mut configuration_count = 0usize;
            let mut template_count = 0usize;
            for (index, field) in source.fields.iter().enumerate() {
                match &field.value {
                    inventory::Value::ActorBase {
                        flags,
                        template_flags,
                        ..
                    } => {
                        configuration_count += 1;
                        configuration = Some(ConfigurationOrigin {
                            inventory_field_index: index,
                            field_decoded_offset: field.decoded_offset,
                            flags: *flags,
                            template_flags: *template_flags,
                        });
                    }
                    inventory::Value::Template { .. } => template_count += 1,
                    _ => {}
                }
            }
            if configuration_count != 1 {
                configuration = None;
                issue(
                    &mut issues,
                    limits,
                    if configuration_count == 0 {
                        "missing_actor_configuration"
                    } else {
                        "ambiguous_actor_configuration"
                    },
                    &key,
                    None,
                )?;
            }
            if let Some(config) = &configuration {
                if config.template_flags & !0x3ff != 0 {
                    issue(
                        &mut issues,
                        limits,
                        "unknown_template_category_bits",
                        &key,
                        None,
                    )?;
                }
                if config.template_flags & 0x3ff != 0 && template_count == 0 {
                    issue(&mut issues, limits, "missing_template_link", &key, None)?;
                }
            }
            let edge_start = self.graph.edges.partition_point(|edge| edge.source < key);
            let edge_end = self.graph.edges.partition_point(|edge| edge.source <= key);
            // Charge the bounded edge join scan once, then match physical offsets.
            let edge_count = edge_end - edge_start;
            if edge_count > limits.max_field_visits.saturating_sub(field_visits) {
                return Err(budget("field visit"));
            }
            field_visits += edge_count;
            let edges: BTreeMap<_, _> = (edge_start..edge_end)
                .filter(|&index| self.graph.edges[index].role == "actor-template")
                .map(|index| (self.graph.edges[index].field_decoded_offset, index))
                .collect();
            for (index, field) in source.fields.iter().enumerate() {
                if field_visits >= limits.max_field_visits {
                    return Err(budget("field visit"));
                }
                field_visits += 1;
                let inventory::Value::Template { template } = &field.value else {
                    continue;
                };
                if links.len() >= limits.max_links {
                    return Err(budget("link"));
                }
                let edge_index = *edges.get(&field.decoded_offset).ok_or_else(|| {
                    Error::Resolution("template physical graph edge join differs".into())
                })?;
                let edge = &self.graph.edges[edge_index];
                links.push(TemplateLink {
                    graph_edge_index: edge_index,
                    source: &source.key,
                    inventory_field_index: index,
                    field,
                    binding: template,
                    ambiguous_source: template_count > 1,
                });
                if template_count > 1 {
                    issue(
                        &mut issues,
                        limits,
                        "ambiguous_template_link",
                        &key,
                        Some(edge_index),
                    )?;
                    continue;
                }
                if template.status != inventory::Status::Defined
                    || edge.schema_kind_allowed != Some(true)
                {
                    issue(
                        &mut issues,
                        limits,
                        "unavailable_template_link",
                        &key,
                        Some(edge_index),
                    )?;
                    continue;
                }
                let target = template
                    .key
                    .as_ref()
                    .ok_or_else(|| Error::Resolution("defined template link lacks key".into()))?;
                let target_kind = template
                    .target
                    .as_ref()
                    .ok_or_else(|| Error::Resolution("defined template link lacks target".into()))?
                    .kind;
                if matches!(&target_kind, b"LVLN" | b"LVLC") {
                    issue(
                        &mut issues,
                        limits,
                        "leveled_template_selection_unsupported",
                        &key,
                        Some(edge_index),
                    )?;
                    continue;
                }
                if !selected.contains(target) {
                    if selected.len() >= limits.max_sources {
                        return Err(budget("source"));
                    }
                    selected.insert(target.clone());
                    queue.push_back(target.clone());
                }
            }
            sources.insert(
                key,
                TemplateSource {
                    key: &definition.key,
                    source: &source.source,
                    header: &definition.header,
                    configuration,
                },
            );
        }
        let candidate_sources: Vec<_> = sources.into_values().collect();
        let indices: BTreeMap<_, _> = candidate_sources
            .iter()
            .enumerate()
            .map(|(index, source)| (source.key, index))
            .collect();
        let root_index = indices[&first.key];
        let config = &candidate_sources[root_index].configuration;
        let categories = CATEGORIES
            .into_iter()
            .map(|category| {
                let present = config
                    .as_ref()
                    .map(|config| config.template_flags & category.mask() != 0);
                let declaration = match present {
                    Some(false) => DeclarationSelection::AuthoredSource {
                        source_index: root_index,
                    },
                    Some(true) => DeclarationSelection::Unsupported {
                        reason: "unverified_template_inheritance",
                    },
                    None => DeclarationSelection::Unsupported {
                        reason: "actor_configuration_unavailable",
                    },
                };
                CategoryRequest {
                    category,
                    mask: category.mask(),
                    template_flag_present: present,
                    declaration,
                    runtime_value_evaluated: false,
                }
            })
            .collect();
        let mut children = vec![Vec::new(); candidate_sources.len()];
        for link in &links {
            if !link.ambiguous_source
                && link.binding.status == inventory::Status::Defined
                && self.graph.edges[link.graph_edge_index].schema_kind_allowed == Some(true)
                && let Some(target) = link.binding.key.as_ref().and_then(|key| indices.get(key))
            {
                children[indices[link.source]].push(*target);
            }
        }
        let template_cycles = crate::graph::cyclic_components(&children);
        for cycle in &template_cycles {
            for &index in cycle {
                issue(
                    &mut issues,
                    limits,
                    "cyclic_template_dependency",
                    candidate_sources[index].key,
                    None,
                )?;
            }
        }
        Ok(TemplateManifest {
            root: root.clone(),
            winning_content_sha256: self.winning_content_sha256(),
            structural_closure,
            candidate_sources,
            links,
            categories,
            template_cycles,
            issues,
            field_visits,
            template_inheritance_supported: false,
            leveled_template_selection_supported: false,
            scope: "Pinned editor category masks over exact authored ACBS/TPLT origins and structural actor candidates; no effective inherited fields, leveled template selection, auto-calculated statistics or runtime values",
        })
    }
}
