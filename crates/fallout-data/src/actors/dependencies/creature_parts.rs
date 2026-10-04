//! Explicit directory requests over the existing physical CREA NIFZ frames.
use super::{
    Catalogue, LookupStatus, ManifestCounts, PathRole, RenderLimits, RenderManifest, manifest,
};
use crate::{
    Error, Result,
    assets::ArchiveAssets,
    identity::FormKey,
    vfs::{AssetPath, AssetSource},
};
use serde::Serialize;
use std::{collections::BTreeMap, io::Write};

#[derive(Debug, Clone, Copy)]
pub struct CreaturePartsLimits {
    pub render: RenderLimits,
    pub max_requests: usize,
    pub max_lookup_path_bytes: usize,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for CreaturePartsLimits {
    fn default() -> Self {
        Self {
            render: RenderLimits::default(),
            max_requests: 16_384,
            max_lookup_path_bytes: 32 * 1024 * 1024,
            max_visits: 2_000_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CreaturePartSelection {
    AuthoredSource,
    ActorConfigurationUnavailable,
    ModelTemplateSelectionUnsupported,
    AmbiguousSource,
}

#[derive(Debug, Serialize)]
pub struct CreaturePartRequest {
    /// Raw bytes, frame offset, physical field and winning source remain here.
    pub manifest_path_index: usize,
    pub selection: CreaturePartSelection,
    pub lookup_status: Option<LookupStatus>,
    pub asset_path: Option<AssetPath>,
    /// Physical candidates under the caller's directory, without retail precedence.
    pub candidates: Vec<AssetSource>,
}

#[derive(Debug, Default, Serialize)]
pub struct CreaturePartsCounts {
    pub requests: usize,
    pub lookup_attempts: usize,
    pub lookup_path_bytes: usize,
    /// Includes candidates already retained by the structural render manifest.
    pub aggregate_candidates: usize,
    pub aggregate_candidate_bytes: usize,
    pub visits: usize,
}

#[derive(Debug, Serialize)]
pub struct CreaturePartsManifest<'a> {
    pub render: RenderManifest<'a>,
    pub explicit_mesh_directory: AssetPath,
    pub directory_authority: &'static str,
    pub requests: Vec<CreaturePartRequest>,
    pub counts: CreaturePartsCounts,
    /// Only unambiguous, nonempty physical source requests with unique candidates.
    pub part_requests_admitted: bool,
    pub effective_part_selection_supported: bool,
    pub rig_playback_supported: bool,
    pub scope: &'static str,
}

fn budget(label: &str) -> Error {
    Error::Unsupported(format!("creature parts {label} budget exceeded"))
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
            .filter(|next| *next <= self.maximum)
            .ok_or_else(|| std::io::Error::other("creature parts projection byte budget"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn visit(counts: &mut CreaturePartsCounts, limits: CreaturePartsLimits) -> Result<()> {
    if counts.visits >= limits.max_visits {
        return Err(budget("visit"));
    }
    counts.visits += 1;
    Ok(())
}

impl Catalogue<'_> {
    pub fn creature_parts_manifest(
        &self,
        root: &FormKey,
        explicit_mesh_directory: &AssetPath,
        assets: &ArchiveAssets,
        limits: CreaturePartsLimits,
    ) -> Result<CreaturePartsManifest<'_>> {
        let directory = explicit_mesh_directory.bytes();
        let relative_directory = directory
            .strip_prefix(b"meshes/")
            .filter(|part| !part.is_empty())
            .ok_or_else(|| {
                Error::Unsupported("creature parts directory must be rooted beneath meshes/".into())
            })?;
        if directory.len() > 4096 {
            return Err(budget("directory byte"));
        }
        let input = self.get(root).ok_or_else(|| {
            Error::Unsupported("creature parts root is outside this source catalogue".into())
        })?;
        if input.header.kind != *b"CREA" || input.deleted {
            return Err(Error::Unsupported(
                "creature parts root must be a nondeleted CREA winner".into(),
            ));
        }
        let render = self.render_manifest(root, assets, limits.render)?;
        let mut result = CreaturePartsManifest {
            counts: CreaturePartsCounts {
                aggregate_candidates: render.manifest.counts.candidates,
                aggregate_candidate_bytes: render.manifest.counts.candidate_bytes,
                ..Default::default()
            },
            render,
            explicit_mesh_directory: explicit_mesh_directory.clone(),
            directory_authority: "explicit_caller_directory",
            requests: Vec::new(),
            part_requests_admitted: false,
            effective_part_selection_supported: false,
            rig_playback_supported: false,
            scope: "Physical CREA NIFZ frames requested under an explicit caller mesh directory; original structural/render requests unchanged, no inferred relative base, effective part assembly, NIF decoding, rig timing or retail selection",
        };
        let mut selected = BTreeMap::new();
        for request in &result.render.requests {
            visit(&mut result.counts, limits)?;
            selected.insert(request.manifest_path_index, request.ambiguous_source);
        }
        let mut candidate_counts = ManifestCounts {
            candidates: result.counts.aggregate_candidates,
            candidate_bytes: result.counts.aggregate_candidate_bytes,
            ..Default::default()
        };
        for (index, source_path) in result.render.manifest.paths.iter().enumerate() {
            visit(&mut result.counts, limits)?;
            if source_path.source != *root || source_path.role != PathRole::ModelList {
                continue;
            }
            if result.requests.len() >= limits.max_requests {
                return Err(budget("request"));
            }
            let selection = match result.render.configuration.as_ref() {
                None => CreaturePartSelection::ActorConfigurationUnavailable,
                Some(config) if config.template_flags & 0x40 != 0 => {
                    CreaturePartSelection::ModelTemplateSelectionUnsupported
                }
                Some(_) => match selected.get(&index) {
                    Some(false) => CreaturePartSelection::AuthoredSource,
                    Some(true) => CreaturePartSelection::AmbiguousSource,
                    None => {
                        return Err(Error::Resolution(
                            "creature model frame lacks its selected render request".into(),
                        ));
                    }
                },
            };
            let mut request = CreaturePartRequest {
                manifest_path_index: index,
                selection,
                lookup_status: None,
                asset_path: None,
                candidates: Vec::new(),
            };
            if selection == CreaturePartSelection::AuthoredSource {
                result.counts.lookup_attempts += 1;
                let raw = &source_path.raw;
                let (path, status, candidates) = if raw.is_empty() {
                    (None, LookupStatus::EmptySourcePath, Vec::new())
                } else if raw.len() > 4096 {
                    (None, LookupStatus::LookupPathTooLong, Vec::new())
                } else if AssetPath::new(raw).is_err() {
                    // Validate before prefixing so absolute/drive/traversal input
                    // cannot become a safe relative path by adding a directory.
                    (None, LookupStatus::UnsafeAssetPath, Vec::new())
                } else if relative_directory
                    .len()
                    .saturating_add(1)
                    .saturating_add(raw.len())
                    > 4096
                {
                    (None, LookupStatus::LookupPathTooLong, Vec::new())
                } else {
                    let mut joined = relative_directory.to_vec();
                    joined.push(b'/');
                    joined.extend(raw);
                    // The existing Model path helper adds meshes/, normalizes,
                    // and charges every archive candidate against one budget.
                    manifest::path(
                        assets,
                        PathRole::Model,
                        &joined,
                        &mut candidate_counts,
                        limits.render.manifest,
                    )?
                };
                if let Some(path) = &path {
                    if path.bytes().len()
                        > limits
                            .max_lookup_path_bytes
                            .saturating_sub(result.counts.lookup_path_bytes)
                    {
                        return Err(budget("lookup path byte"));
                    }
                    result.counts.lookup_path_bytes += path.bytes().len();
                }
                request.asset_path = path;
                request.lookup_status = Some(status);
                request.candidates = candidates;
            }
            result.requests.push(request);
        }
        result.counts.requests = result.requests.len();
        result.counts.aggregate_candidates = candidate_counts.candidates;
        result.counts.aggregate_candidate_bytes = candidate_counts.candidate_bytes;
        result.part_requests_admitted = !result.requests.is_empty()
            && result.render.issues.is_empty()
            && result.render.selected_source_cycles.is_empty()
            && result.requests.iter().all(|request| {
                request.selection == CreaturePartSelection::AuthoredSource
                    && request.lookup_status == Some(LookupStatus::OneArchiveCandidate)
            });
        serde_json::to_writer(
            ProjectionBudget {
                bytes: 0,
                maximum: limits.max_projection_bytes,
            },
            &result,
        )
        .map_err(|_| budget("projection byte"))?;
        Ok(result)
    }
}
