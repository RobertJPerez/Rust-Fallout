//! Bounded structural requests, retaining every occurrence and archive candidate.
use super::{Catalogue, Context, LinkRole, PathRole, Value};
use crate::{
    Error, Result,
    assets::ArchiveAssets,
    identity::FormKey,
    inventory, leveled,
    vfs::{self, AssetPath, AssetSource},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy)]
pub struct ManifestLimits {
    /// Combined inventory/list and model-source nodes retained by this root.
    pub max_nodes: usize,
    /// Combined physical inventory/list and model-source edges.
    pub max_edges: usize,
    pub max_field_visits: usize,
    pub max_paths: usize,
    pub max_path_bytes: usize,
    pub max_candidates: usize,
    pub max_candidate_bytes: usize,
}
impl Default for ManifestLimits {
    fn default() -> Self {
        Self {
            max_nodes: 65_536,
            max_edges: 1_000_000,
            max_field_visits: 2_000_000,
            max_paths: 100_000,
            max_path_bytes: 32 * 1024 * 1024,
            max_candidates: 100_000,
            max_candidate_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupStatus {
    EmptySourcePath,
    RelativeBaseUnresolved,
    LookupPathTooLong,
    UnsafeAssetPath,
    MissingArchiveCandidate,
    OneArchiveCandidate,
    ArchiveCollision,
}
impl LookupStatus {
    fn label(self) -> &'static str {
        match self {
            Self::EmptySourcePath => "empty_source_path",
            Self::RelativeBaseUnresolved => "relative_base_unresolved",
            Self::LookupPathTooLong => "lookup_path_too_long",
            Self::UnsafeAssetPath => "unsafe_asset_path",
            Self::MissingArchiveCandidate => "missing_archive_candidate",
            Self::OneArchiveCandidate => "one_archive_candidate",
            Self::ArchiveCollision => "archive_collision",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PathRequest {
    pub source: FormKey,
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub field_byte_offset: u32,
    pub role: PathRole,
    pub context: Context,
    pub raw: Vec<u8>,
    pub asset_path: Option<AssetPath>,
    pub lookup_status: LookupStatus,
    /// Physical mount candidates; none is asserted to be the effective winner.
    pub candidates: Vec<AssetSource>,
}

#[derive(Debug, Serialize)]
pub struct ManifestEdge {
    pub source: FormKey,
    pub field_decoded_offset: u32,
    pub field_byte_offset: u32,
    pub role: &'static str,
    pub binding: inventory::Binding,
    pub schema_kind_allowed: Option<bool>,
}

#[derive(Debug, Default, Serialize)]
pub struct ManifestCounts {
    pub nodes: usize,
    pub model_source_nodes: usize,
    pub inventory_edges: usize,
    pub model_edges: usize,
    pub field_visits: usize,
    pub paths: usize,
    pub path_bytes: usize,
    pub candidates: usize,
    pub candidate_bytes: usize,
    pub cyclic_components: usize,
    pub cyclic_nodes: usize,
    pub lookup_statuses: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub root: FormKey,
    pub inventory_closure: leveled::graph::Closure,
    /// Canonical key order. Non-model inventory/list nodes remain present.
    pub nodes: Vec<FormKey>,
    /// Indices in nodes for the expanded actor/race/head-part sources.
    pub model_source_node_indices: Vec<usize>,
    pub model_edges: Vec<ManifestEdge>,
    pub paths: Vec<PathRequest>,
    /// Components index nodes, over retained defined structural edges only.
    pub cyclic_components: Vec<Vec<usize>>,
    pub counts: ManifestCounts,
    pub scope: &'static str,
}

fn unsupported(reason: &str) -> Error {
    Error::Unsupported(format!("actor manifest {reason} budget exceeded"))
}
fn link_label(role: LinkRole) -> &'static str {
    match role {
        LinkRole::HeadPart => "head_part",
        LinkRole::ExtraHeadPart => "extra_head_part",
        LinkRole::Hair => "hair",
        LinkRole::Eyes => "eyes",
    }
}

struct Walk<'a, 'b> {
    catalogue: &'a Catalogue<'b>,
    nodes: BTreeSet<FormKey>,
    queue: VecDeque<FormKey>,
    edges: Vec<ManifestEdge>,
    inventory_edges: usize,
    limits: ManifestLimits,
}
impl Walk<'_, '_> {
    fn edge(
        &mut self,
        source: &FormKey,
        offset: u32,
        byte_offset: u32,
        role: &'static str,
        binding: &inventory::Binding,
        allowed: Option<bool>,
    ) -> Result<()> {
        if self.edges.len() >= self.limits.max_edges.saturating_sub(self.inventory_edges) {
            return Err(unsupported("edge"));
        }
        if binding.status == inventory::Status::Defined && allowed == Some(true) {
            let key = binding
                .key
                .as_ref()
                .ok_or_else(|| Error::Resolution("defined model dependency has no key".into()))?;
            let target = self.catalogue.get(key).ok_or_else(|| {
                Error::Resolution("defined model dependency lacks its source definition".into())
            })?;
            if target.deleted {
                return Err(Error::Resolution(
                    "defined model dependency is deleted".into(),
                ));
            }
            if !self.nodes.contains(key) {
                if self.nodes.len() >= self.limits.max_nodes {
                    return Err(unsupported("node"));
                }
                self.nodes.insert(key.clone());
                self.queue.push_back(key.clone());
            }
        }
        self.edges.push(ManifestEdge {
            source: source.clone(),
            field_decoded_offset: offset,
            field_byte_offset: byte_offset,
            role,
            binding: binding.clone(),
            schema_kind_allowed: allowed,
        });
        Ok(())
    }
}

fn path(
    assets: &ArchiveAssets,
    role: PathRole,
    raw: &[u8],
    counts: &mut ManifestCounts,
    limits: ManifestLimits,
) -> Result<(Option<AssetPath>, LookupStatus, Vec<AssetSource>)> {
    if raw.is_empty() {
        return Ok((None, LookupStatus::EmptySourcePath, Vec::new()));
    }
    if matches!(role, PathRole::ModelList | PathRole::AnimationList) {
        return Ok((None, LookupStatus::RelativeBaseUnresolved, Vec::new()));
    }
    // Bounded normalization follows existing model/texture lookup conventions.
    // Long authored values remain source facts but are not lookup requests.
    if raw.len() > 4096 {
        return Ok((None, LookupStatus::LookupPathTooLong, Vec::new()));
    }
    let lookup = match role {
        PathRole::Model => {
            let mut rooted = b"meshes/".to_vec();
            rooted.extend(raw);
            AssetPath::new(&rooted)
        }
        PathRole::Texture => vfs::texture_path(raw),
        _ => unreachable!("relative lists handled above"),
    };
    let lookup = match lookup {
        Ok(lookup) => lookup,
        Err(_) => return Ok((None, LookupStatus::UnsafeAssetPath, Vec::new())),
    };
    let candidates = assets.candidates(&lookup)?;
    if candidates.len() > limits.max_candidates.saturating_sub(counts.candidates) {
        return Err(unsupported("candidate"));
    }
    let bytes = candidates.iter().try_fold(0usize, |used, candidate| {
        used.checked_add(candidate.container.len())
            .and_then(|value| value.checked_add(candidate.original_path.len()))
            .ok_or_else(|| unsupported("candidate byte"))
    })?;
    if bytes
        > limits
            .max_candidate_bytes
            .saturating_sub(counts.candidate_bytes)
    {
        return Err(unsupported("candidate byte"));
    }
    let status = match candidates.len() {
        0 => LookupStatus::MissingArchiveCandidate,
        1 => LookupStatus::OneArchiveCandidate,
        _ => LookupStatus::ArchiveCollision,
    };
    counts.candidates += candidates.len();
    counts.candidate_bytes += bytes;
    Ok((Some(lookup), status, candidates.to_vec()))
}

impl Catalogue<'_> {
    pub fn manifest(
        &self,
        root: &FormKey,
        assets: &ArchiveAssets,
        limits: ManifestLimits,
    ) -> Result<Manifest> {
        let input = self.get(root).ok_or_else(|| {
            Error::Unsupported("actor manifest root is outside this source catalogue".into())
        })?;
        if !matches!(&input.header.kind, b"NPC_" | b"CREA") || input.deleted {
            return Err(Error::Unsupported(
                "actor manifest root must be a nondeleted NPC_/CREA winner".into(),
            ));
        }
        let closure = self.graph.closure(
            root,
            leveled::graph::Limits {
                max_nodes: limits.max_nodes,
                max_edges: limits.max_edges,
            },
        )?;
        let mut walk = Walk {
            catalogue: self,
            nodes: closure.nodes.iter().cloned().collect(),
            queue: closure
                .nodes
                .iter()
                .filter(|key| self.get(key).is_some())
                .cloned()
                .collect(),
            edges: Vec::new(),
            inventory_edges: closure.edge_indices.len(),
            limits,
        };
        let mut counts = ManifestCounts::default();
        let mut paths = Vec::new();
        while let Some(key) = walk.queue.pop_front() {
            let definition = self.get(&key).expect("queued source definition");
            for (field_index, field) in definition.fields.iter().enumerate() {
                if counts.field_visits >= limits.max_field_visits {
                    return Err(unsupported("field visit"));
                }
                counts.field_visits += 1;
                match &field.value {
                    Value::Paths {
                        role,
                        context,
                        strings,
                    } => {
                        for string in strings {
                            if counts.paths >= limits.max_paths {
                                return Err(unsupported("path"));
                            }
                            if string.raw.len()
                                > limits.max_path_bytes.saturating_sub(counts.path_bytes)
                            {
                                return Err(unsupported("path byte"));
                            }
                            let (asset_path, lookup_status, candidates) =
                                path(assets, *role, &string.raw, &mut counts, limits)?;
                            paths.push(PathRequest {
                                source: key.clone(),
                                field_index,
                                field_decoded_offset: field.decoded_offset,
                                field_byte_offset: string.field_byte_offset,
                                role: *role,
                                context: *context,
                                raw: string.raw.clone(),
                                asset_path,
                                lookup_status,
                                candidates,
                            });
                            counts.paths += 1;
                            counts.path_bytes += string.raw.len();
                            *counts
                                .lookup_statuses
                                .entry(lookup_status.label().into())
                                .or_default() += 1;
                        }
                    }
                    Value::Links { role, bindings } => {
                        for link in bindings {
                            walk.edge(
                                &key,
                                field.decoded_offset,
                                link.field_byte_offset,
                                link_label(*role),
                                &link.binding,
                                link.schema_kind_allowed,
                            )?;
                        }
                    }
                    _ => {}
                }
            }
            for link in &definition.race_links {
                walk.edge(
                    &key,
                    link.field_decoded_offset,
                    0,
                    "race",
                    &link.binding,
                    link.schema_kind_allowed,
                )?;
            }
        }
        walk.edges.sort_by(|a, b| {
            (
                &a.source,
                a.field_decoded_offset,
                a.field_byte_offset,
                a.role,
            )
                .cmp(&(
                    &b.source,
                    b.field_decoded_offset,
                    b.field_byte_offset,
                    b.role,
                ))
        });
        paths.sort_by(|a, b| {
            (&a.source, a.field_decoded_offset, a.field_byte_offset).cmp(&(
                &b.source,
                b.field_decoded_offset,
                b.field_byte_offset,
            ))
        });
        let nodes = walk.nodes.into_iter().collect::<Vec<_>>();
        let indices = nodes
            .iter()
            .enumerate()
            .map(|(i, key)| (key, i))
            .collect::<BTreeMap<_, _>>();
        let model_source_node_indices = nodes
            .iter()
            .enumerate()
            .filter_map(|(i, key)| self.get(key).map(|_| i))
            .collect::<Vec<_>>();
        let mut children = vec![Vec::new(); nodes.len()];
        // Preserve the existing inventory graph's structural cycle semantics.
        for &i in &closure.edge_indices {
            let edge = &self.graph.edges[i];
            if edge.status == inventory::Status::Defined
                && let Some(to) = edge.target.as_ref().and_then(|key| indices.get(key))
            {
                children[indices[&edge.source]].push(*to);
            }
        }
        for edge in &walk.edges {
            if edge.binding.status == inventory::Status::Defined
                && let Some(to) = edge.binding.key.as_ref().and_then(|key| indices.get(key))
            {
                children[indices[&edge.source]].push(*to);
            }
        }
        let cyclic_components = crate::graph::cyclic_components(&children);
        counts.nodes = nodes.len();
        counts.model_source_nodes = model_source_node_indices.len();
        counts.inventory_edges = closure.edge_indices.len();
        counts.model_edges = walk.edges.len();
        counts.cyclic_components = cyclic_components.len();
        counts.cyclic_nodes = cyclic_components.iter().map(Vec::len).sum();
        Ok(Manifest {
            root: root.clone(),
            inventory_closure: closure,
            nodes,
            model_source_node_indices,
            model_edges: walk.edges,
            paths,
            cyclic_components,
            counts,
            scope: "Authored structural dependencies and archive candidates; terminal inventory bodies and NIFZ/KFFZ relative bases remain unresolved; no inheritance, equipment, gender/part choice, playback or retail lookup precedence",
        })
    }
}
