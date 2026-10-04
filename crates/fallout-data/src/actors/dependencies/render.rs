//! Source role requests for one actor, over the existing structural manifest.
//! This selects declarations, not effective equipment, FaceGen or template state.
use super::{Catalogue, Manifest, ManifestLimits, PathRole, Value};
use crate::{Error, Result, assets::ArchiveAssets, identity::FormKey, inventory, plugin};
use serde::Serialize;
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy)]
pub struct RenderLimits {
    pub manifest: ManifestLimits,
    pub max_sources: usize,
    pub max_requests: usize,
    pub max_issues: usize,
    pub max_visits: usize,
}
impl Default for RenderLimits {
    fn default() -> Self {
        Self {
            manifest: ManifestLimits::default(),
            max_sources: 4096,
            max_requests: 16_384,
            max_issues: 16_384,
            max_visits: 2_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sex {
    Male,
    Female,
}

#[derive(Debug, Serialize)]
pub struct ConfigurationOrigin {
    /// Index in the existing inventory definition, not the model field array.
    pub inventory_field_index: usize,
    pub field_decoded_offset: u32,
    pub flags: u32,
    pub template_flags: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderRole {
    ActorModel,
    CreatureModelList,
    AnimationList,
    RaceHead { part_index: u32 },
    RaceBody { part_index: u32 },
    HeadPart,
    Hair,
    Eyes,
}

#[derive(Debug, Serialize)]
pub struct RenderRequest {
    /// All path bytes, source offsets and lookup candidates stay in Manifest.
    pub manifest_path_index: usize,
    pub role: RenderRole,
    /// Repeated singleton/part declarations are never resolved by first/last wins.
    pub ambiguous_source: bool,
}

#[derive(Debug, Serialize)]
pub struct RenderSource<'a> {
    pub key: &'a FormKey,
    pub source: &'a inventory::Source,
    pub header: &'a plugin::RecordHeader,
}

#[derive(Debug, Serialize)]
pub struct RenderIssue {
    pub code: &'static str,
    pub source: FormKey,
    pub manifest_edge_index: Option<usize>,
    pub manifest_path_index: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct RenderManifest<'a> {
    pub winning_content_sha256: &'a str,
    pub manifest: Manifest,
    pub configuration: Option<ConfigurationOrigin>,
    pub sex: Option<Sex>,
    /// Only reached actor/RACE/explicit head-part/hair/eye winners, in key order.
    pub sources: Vec<RenderSource<'a>>,
    /// Physical link indices retain missing/deleted/null/wrong-kind provenance.
    pub selected_edge_indices: Vec<usize>,
    pub requests: Vec<RenderRequest>,
    pub issues: Vec<RenderIssue>,
    pub visits: usize,
    pub equipment_selection_supported: bool,
    pub scope: &'static str,
}

fn budget(label: &str) -> Error {
    Error::Unsupported(format!("actor render {label} budget exceeded"))
}

struct Selection {
    requests: Vec<RenderRequest>,
    issues: Vec<RenderIssue>,
    edges: Vec<usize>,
    visits: usize,
    limits: RenderLimits,
}
impl Selection {
    fn visit(&mut self) -> Result<()> {
        if self.visits >= self.limits.max_visits {
            return Err(budget("visit"));
        }
        self.visits += 1;
        Ok(())
    }
    fn issue(
        &mut self,
        code: &'static str,
        source: &FormKey,
        edge: Option<usize>,
        path: Option<usize>,
    ) -> Result<()> {
        if self.issues.len() >= self.limits.max_issues {
            return Err(budget("issue"));
        }
        self.issues.push(RenderIssue {
            code,
            source: source.clone(),
            manifest_edge_index: edge,
            manifest_path_index: path,
        });
        Ok(())
    }
}

impl Catalogue<'_> {
    pub fn render_manifest(
        &self,
        root: &FormKey,
        assets: &ArchiveAssets,
        limits: RenderLimits,
    ) -> Result<RenderManifest<'_>> {
        // Construct internally: public mutable Manifest fields cannot forge a
        // source join or substitute paths from a different winning cohort.
        let manifest = self.manifest(root, assets, limits.manifest)?;
        let input = self.inventory.get(root).ok_or_else(|| {
            Error::Resolution("render root lacks its joined inventory source".into())
        })?;
        let mut selection = Selection {
            requests: Vec::new(),
            issues: Vec::new(),
            edges: Vec::new(),
            visits: 0,
            limits,
        };
        let mut configuration = None;
        let mut configurations = 0usize;
        for (index, field) in input.fields.iter().enumerate() {
            selection.visit()?;
            if let inventory::Value::ActorBase {
                flags,
                template_flags,
                ..
            } = field.value
            {
                configurations += 1;
                configuration = Some(ConfigurationOrigin {
                    inventory_field_index: index,
                    field_decoded_offset: field.decoded_offset,
                    flags,
                    template_flags,
                });
            }
        }
        let mut sex = None;
        let mut queue = VecDeque::new();
        let mut selected = BTreeSet::new();
        if limits.max_sources == 0 {
            return Err(budget("source"));
        }
        selected.insert(&manifest.root);
        if configurations != 1 {
            configuration = None;
            selection.issue(
                if configurations == 0 {
                    "missing_actor_configuration"
                } else {
                    "ambiguous_actor_configuration"
                },
                root,
                None,
                None,
            )?;
        } else {
            let config = configuration.as_ref().expect("one configuration");
            // FNV xEdit's category masks are source classification evidence.
            // Its editor visibility callbacks do not prove retail inheritance.
            if config.template_flags & 0x40 != 0 {
                selection.issue("model_template_selection_unsupported", root, None, None)?;
            } else {
                queue.push_back(&manifest.root);
            }
            if input.kind == *b"NPC_" {
                if config.template_flags & 1 != 0 {
                    selection.issue("traits_template_selection_unsupported", root, None, None)?;
                } else {
                    sex = Some(if config.flags & 1 == 0 {
                        Sex::Male
                    } else {
                        Sex::Female
                    });
                }
            }
        }
        while let Some(key) = queue.pop_front() {
            let definition = self.get(key).ok_or_else(|| {
                Error::Resolution("selected render source lacks a winning definition".into())
            })?;
            let kind = definition.header.kind;
            let start = manifest.paths.partition_point(|path| &path.source < key);
            let end = manifest.paths.partition_point(|path| &path.source <= key);
            let first_request = selection.requests.len();
            for path_index in start..end {
                selection.visit()?;
                let path = &manifest.paths[path_index];
                let role = match (&kind, path.role) {
                    (b"NPC_" | b"CREA", PathRole::Model) => RenderRole::ActorModel,
                    (b"CREA", PathRole::ModelList) => RenderRole::CreatureModelList,
                    (b"NPC_" | b"CREA", PathRole::AnimationList) => RenderRole::AnimationList,
                    (b"HDPT", PathRole::Model) => RenderRole::HeadPart,
                    (b"HAIR", PathRole::Model | PathRole::Texture) => RenderRole::Hair,
                    (b"EYES", PathRole::Texture) => RenderRole::Eyes,
                    (b"RACE", PathRole::Model | PathRole::Texture) => {
                        let context = path.context;
                        let (Some(region), Some(marker), Some(part)) =
                            (context.region, context.sex, context.part)
                        else {
                            selection.issue(
                                "race_path_without_part_context",
                                key,
                                None,
                                Some(path_index),
                            )?;
                            continue;
                        };
                        let expected_sex = match sex {
                            Some(Sex::Male) => *b"MNAM",
                            Some(Sex::Female) => *b"FNAM",
                            None => continue,
                        };
                        if marker.kind != expected_sex {
                            continue;
                        }
                        match (region.kind, part.raw_index) {
                            (tag, Some(index)) if tag == *b"NAM0" && index < 8 => {
                                RenderRole::RaceHead { part_index: index }
                            }
                            (tag, Some(index)) if tag == *b"NAM1" && index < 4 => {
                                RenderRole::RaceBody { part_index: index }
                            }
                            _ => {
                                selection.issue(
                                    "unsupported_race_part_index",
                                    key,
                                    None,
                                    Some(path_index),
                                )?;
                                continue;
                            }
                        }
                    }
                    _ => continue,
                };
                if selection.requests.len() >= limits.max_requests {
                    return Err(budget("request"));
                }
                selection.requests.push(RenderRequest {
                    manifest_path_index: path_index,
                    role,
                    ambiguous_source: false,
                });
            }
            // Duplicate keys include the physical part marker: duplicate MODL
            // within one part and repeated declarations of a part are ambiguous.
            // Different KFFZ/NIFZ frames in the same field remain ordinary lists.
            let mut groups = std::collections::BTreeMap::new();
            for field in &definition.fields {
                selection.visit()?;
                if let Value::Paths { role, context, .. } = &field.value {
                    let group = (
                        *role as u8,
                        context.region.map(|m| m.kind),
                        context.sex.map(|m| m.kind),
                        context.part.and_then(|m| m.raw_index),
                    );
                    *groups.entry(group).or_insert(0usize) += 1;
                }
            }
            for request_index in first_request..selection.requests.len() {
                selection.visit()?;
                let request = &mut selection.requests[request_index];
                let path = &manifest.paths[request.manifest_path_index];
                let group = (
                    path.role as u8,
                    path.context.region.map(|m| m.kind),
                    path.context.sex.map(|m| m.kind),
                    path.context.part.and_then(|m| m.raw_index),
                );
                request.ambiguous_source = groups[&group] > 1;
            }
            if !selection.requests[first_request..].iter().any(|request| {
                let path = &manifest.paths[request.manifest_path_index];
                path.role
                    == if kind == *b"EYES" {
                        PathRole::Texture
                    } else {
                        PathRole::Model
                    }
            }) {
                selection.issue("missing_selected_model_or_texture", key, None, None)?;
            }
            let start = manifest
                .model_edges
                .partition_point(|edge| &edge.source < key);
            let end = manifest
                .model_edges
                .partition_point(|edge| &edge.source <= key);
            let links = &manifest.model_edges[start..end];
            let mut singleton_counts = [0usize; 3];
            for edge in links {
                selection.visit()?;
                match edge.role {
                    "race" => singleton_counts[0] += 1,
                    "hair" => singleton_counts[1] += 1,
                    "eyes" => singleton_counts[2] += 1,
                    _ => {}
                }
            }
            for (local_index, edge) in links.iter().enumerate() {
                selection.visit()?;
                let allowed_role = match &kind {
                    b"NPC_" => {
                        matches!(edge.role, "head_part" | "hair" | "eyes")
                            || (edge.role == "race" && sex.is_some())
                    }
                    b"HDPT" => edge.role == "extra_head_part",
                    _ => false,
                };
                if !allowed_role {
                    continue;
                }
                let edge_index = start + local_index;
                selection.edges.push(edge_index);
                let singleton_count = match edge.role {
                    "race" => singleton_counts[0],
                    "hair" => singleton_counts[1],
                    "eyes" => singleton_counts[2],
                    _ => 0,
                };
                if singleton_count > 1 {
                    selection.issue("ambiguous_actor_render_link", key, Some(edge_index), None)?;
                    continue;
                }
                if edge.binding.status != inventory::Status::Defined
                    || edge.schema_kind_allowed != Some(true)
                {
                    selection.issue(
                        "unavailable_actor_render_link",
                        key,
                        Some(edge_index),
                        None,
                    )?;
                    continue;
                }
                let target = edge.binding.key.as_ref().ok_or_else(|| {
                    Error::Resolution("defined selected render link has no key".into())
                })?;
                if !selected.contains(target) {
                    if selected.len() >= limits.max_sources {
                        return Err(budget("source"));
                    }
                    selected.insert(target);
                    queue.push_back(target);
                }
            }
            if kind == *b"NPC_" && sex.is_some() && singleton_counts[0] == 0 {
                selection.issue("missing_actor_race_link", key, None, None)?;
            }
        }
        selection.edges.sort_unstable();
        let sources = selected
            .into_iter()
            .map(|key| {
                let definition = self.get(key).expect("selected source was checked");
                RenderSource {
                    key: &definition.key,
                    source: &definition.source,
                    header: &definition.header,
                }
            })
            .collect();
        Ok(RenderManifest {
            winning_content_sha256: self.winning_content_sha256(),
            sources,
            manifest,
            configuration,
            sex,
            selected_edge_indices: selection.edges,
            requests: selection.requests,
            issues: selection.issues,
            visits: selection.visits,
            equipment_selection_supported: false,
            scope: "Authored actor model/animation and explicit head-part/hair/eye links, sex-bound RACE declarations; no template inheritance, effective equipment, FaceGen composition, relative list base, animation playback or retail precedence",
        })
    }
}
