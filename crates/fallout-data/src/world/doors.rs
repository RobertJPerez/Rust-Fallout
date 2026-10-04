//! One authored door destination is a source request, not an activation decision.
//! Reuse the bounded winning dependency graph; never compose XTEL links or apply
//! target-door DATA as the source door's authored destination transform.
use super::{
    Transform,
    dependencies::{self, Decoded, Edge, Limits, Report},
    preparation::{CellModelPlan, Limits as ModelLimits},
};
use crate::{Error, Result, identity::FormKey, plugin, store::RecordStore, vfs::MountIndex};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Issue {
    pub role: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Destination {
    pub cell: FormKey,
    pub door: FormKey,
    /// The source door's XTEL position/rotation, in original source units.
    pub authored_transform: Transform,
    /// Exact XTEL f32 words: position XYZ, then rotation XYZ. These preserve
    /// signed zero even when a downstream numeric display normalizes it.
    pub authored_transform_words: [u32; 6],
    pub raw_flags: u32,
}

/// Node/edge indices address the retained immutable dependency graph. The
/// destination CELL is header-only until prepare_cell uses its existing reader.
#[derive(Debug, Serialize)]
pub struct Metadata {
    pub schema_version: u32,
    pub source_cell: FormKey,
    pub source_door: FormKey,
    pub source_cohort_sha256: String,
    pub source_node: Option<usize>,
    pub source_base_edge: Option<usize>,
    pub teleport_edge: Option<usize>,
    pub destination_node: Option<usize>,
    pub destination_base_edge: Option<usize>,
    pub destination_cell_edge: Option<usize>,
    pub destination_cell_node: Option<usize>,
    pub source_cycle_component: Option<usize>,
    pub destination_cycle_component: Option<usize>,
    pub destination: Option<Destination>,
    pub issues: Vec<Issue>,
    pub source_destination_resolved: bool,
    pub runtime_ready: bool,
}

/// Only protected winning source reads can construct this request. Borrowed
/// receipt data is not an activation token or a canonical persistent mutation.
pub struct DoorDestination {
    graph: Report,
    metadata: Metadata,
}

fn issue(issues: &mut Vec<Issue>, role: &'static str, status: &'static str) {
    issues.push(Issue { role, status });
}

fn edge<'a>(graph: &'a Report, owner: &FormKey, role: &str) -> Option<(usize, &'a Edge)> {
    graph
        .edges
        .iter()
        .enumerate()
        .find(|(_, edge)| edge.owner == *owner && edge.role == role)
}

fn target<'a>(
    graph: &'a Report,
    owner: &FormKey,
    role: &'static str,
    kind: &str,
    issues: &mut Vec<Issue>,
) -> Option<(usize, &'a FormKey)> {
    let Some((index, link)) = edge(graph, owner, role) else {
        issue(issues, role, "no-authored-link");
        return None;
    };
    if link.target.status != "resolved" {
        issue(issues, role, link.target.status);
        return None;
    }
    if link.target.kind.as_deref() != Some(kind) {
        issue(issues, role, "wrong-requested-target-kind");
        return None;
    }
    link.target.key.as_ref().map(|key| (index, key))
}

fn cycle(graph: &Report, node: Option<usize>) -> Option<usize> {
    node.and_then(|node| {
        graph
            .cyclic_link_components
            .iter()
            .position(|part| part.contains(&node))
    })
}

impl DoorDestination {
    pub fn load(
        store: &mut RecordStore,
        cell: &FormKey,
        door: &FormKey,
        limits: Limits,
    ) -> Result<Self> {
        if door.profile != cell.profile {
            return Err(Error::Resolution(
                "door source profile differs from CELL".into(),
            ));
        }
        // Preflight externally supplied identities before any owned clones. The
        // graph then enforces its existing source/body/node/edge admission limits.
        if door.origin_plugin.len() > limits.max_metadata_bytes {
            return Err(Error::Resolution(
                "door source identity exceeds metadata bound".into(),
            ));
        }
        let mut graph = dependencies::inspect_cell_key(store, cell, limits)?;
        let maximum_origin = graph
            .nodes
            .iter()
            .map(|node| node.key.origin_plugin.len())
            .chain([cell.origin_plugin.len(), door.origin_plugin.len()])
            .max()
            .unwrap_or(0);
        // Four cloned keys, bounded diagnostics/indices and transform storage.
        // Charge before allocating this producer's owned receipt, in the same
        // metadata allowance as the graph; no second unconstrained source tree.
        let extra = maximum_origin
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(4096))
            .ok_or_else(|| Error::Resolution("door metadata charge overflow".into()))?;
        graph.usage.metadata_bytes = graph
            .usage
            .metadata_bytes
            .checked_add(extra)
            .filter(|bytes| *bytes <= limits.max_metadata_bytes)
            .ok_or_else(|| Error::Resolution("door destination metadata bound exceeded".into()))?;
        let mut metadata = Metadata {
            schema_version: 1,
            source_cell: cell.clone(),
            source_door: door.clone(),
            source_cohort_sha256: graph.source_cohort_sha256.clone(),
            source_node: None,
            source_base_edge: None,
            teleport_edge: None,
            destination_node: None,
            destination_base_edge: None,
            destination_cell_edge: None,
            destination_cell_node: None,
            source_cycle_component: None,
            destination_cycle_component: None,
            destination: None,
            issues: Vec::new(),
            source_destination_resolved: false,
            runtime_ready: false,
        };
        let source = graph.nodes.iter().position(|node| node.key == *door);
        metadata.source_node = source;
        let member = graph.edges.iter().find(|edge| {
            edge.owner == *cell && edge.role == "member" && edge.target.key.as_ref() == Some(door)
        });
        if member.is_none_or(|member| member.target.status != "resolved") {
            issue(
                &mut metadata.issues,
                "source-door",
                "not-live-winning-source-cell-member",
            );
        }
        let source = source.and_then(|index| {
            let node = &graph.nodes[index];
            if node.header.kind != *b"REFR" || node.header.flags & plugin::DELETED != 0 {
                issue(&mut metadata.issues, "source-door", "deleted-or-not-REFR");
                return None;
            }
            match node.fields.as_ref() {
                Some(Decoded::Placement(placement)) => Some(placement),
                _ => {
                    issue(
                        &mut metadata.issues,
                        "source-door",
                        "placement-body-unavailable",
                    );
                    None
                }
            }
        });
        if source.is_none() && metadata.issues.is_empty() {
            issue(
                &mut metadata.issues,
                "source-door",
                "winning-source-unavailable",
            );
        }
        if let Some(source) = source {
            metadata.source_base_edge =
                target(&graph, door, "NAME", "DOOR", &mut metadata.issues).map(|(index, _)| index);
            metadata.teleport_edge = edge(&graph, door, "XTEL").map(|(index, _)| index);
            if let Some((_, destination_key)) =
                target(&graph, door, "XTEL", "REFR", &mut metadata.issues)
            {
                metadata.destination_node = graph
                    .nodes
                    .iter()
                    .position(|node| node.key == *destination_key);
                metadata.destination_base_edge = target(
                    &graph,
                    destination_key,
                    "NAME",
                    "DOOR",
                    &mut metadata.issues,
                )
                .map(|(index, _)| index);
                if let Some((cell_edge, cell_key)) = target(
                    &graph,
                    destination_key,
                    "group.cell",
                    "CELL",
                    &mut metadata.issues,
                ) {
                    metadata.destination_cell_edge = Some(cell_edge);
                    metadata.destination_cell_node =
                        graph.nodes.iter().position(|node| node.key == *cell_key);
                    if let Some(teleport) = &source.teleport {
                        let pose = &teleport.value.destination;
                        metadata.destination = Some(Destination {
                            cell: cell_key.clone(),
                            door: destination_key.clone(),
                            authored_transform: teleport.value.destination.clone(),
                            authored_transform_words: [
                                pose.position[0].to_bits(),
                                pose.position[1].to_bits(),
                                pose.position[2].to_bits(),
                                pose.rotation[0].to_bits(),
                                pose.rotation[1].to_bits(),
                                pose.rotation[2].to_bits(),
                            ],
                            raw_flags: teleport.value.flags,
                        });
                    }
                }
            }
        }
        metadata.source_cycle_component = cycle(&graph, metadata.source_node);
        metadata.destination_cycle_component = cycle(&graph, metadata.destination_node);
        metadata.source_destination_resolved =
            metadata.issues.is_empty() && metadata.destination.is_some();
        // An unresolved target can retain partial diagnostics in the graph but
        // cannot expose an actionable destination request through this producer.
        if !metadata.source_destination_resolved {
            metadata.destination = None;
        }
        Ok(Self { graph, metadata })
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    pub fn graph(&self) -> &Report {
        &self.graph
    }

    /// Feed the exact authored target CELL into the existing model-plan consumer.
    /// This does not change the current cell or apply the authored transform.
    pub fn prepare_cell(
        &self,
        store: &mut RecordStore,
        mounts: &MountIndex,
        limits: ModelLimits,
    ) -> Result<CellModelPlan> {
        let destination =
            self.metadata.destination.as_ref().ok_or_else(|| {
                Error::Resolution("door destination source links unresolved".into())
            })?;
        let current = store.source_receipts()?;
        if current.len() != self.graph.sources.len()
            || current.iter().zip(&self.graph.sources).any(|(a, b)| {
                a.source_name != b.source_name
                    || a.source_bytes != b.source_bytes
                    || a.source_sha256 != b.source_sha256
            })
        {
            return Err(Error::Resolution(
                "door destination ordered source cohort changed".into(),
            ));
        }
        let plan = CellModelPlan::load(store, &destination.cell, mounts, limits)?;
        if plan.receipt().source_cohort_sha256 != self.metadata.source_cohort_sha256 {
            return Err(Error::Resolution(
                "destination CELL plan has another source cohort".into(),
            ));
        }
        Ok(plan)
    }
}
