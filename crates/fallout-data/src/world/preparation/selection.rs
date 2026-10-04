//! An explicitly selected, sealed reference set over the existing protected graph.
#[cfg(test)]
mod tests;
use super::plan::{self, ArchivePool, CellModelPlan, Limits as ModelLimits};
use crate::{
    Error, Result, identity::FormKey, plugin, store::RecordStore, vfs::MountIndex,
    world::dependencies,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, Serialize)]
pub struct SelectionLimits {
    pub references: usize,
    pub metadata_bytes: usize,
}
impl Default for SelectionLimits {
    fn default() -> Self {
        Self {
            references: 1024,
            metadata_bytes: 4 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SelectionState {
    Selected,
    NotSelected,
}
#[derive(Debug, Serialize)]
pub struct ReferenceSelection {
    pub key: FormKey,
    pub source_node: usize,
    pub state: SelectionState,
    pub base_edge: Option<usize>,
    pub base: Option<FormKey>,
}
#[derive(Debug, Serialize)]
pub struct SelectionReceipt {
    pub identity: String,
    pub requested: Vec<FormKey>,
    pub references: Vec<ReferenceSelection>,
    pub metadata_bytes: usize,
    pub limits: SelectionLimits,
    pub full_cell_coverage: bool,
}
/// No caller-built Report can create this request; preparation rechecks its source seal.
pub struct CellModelSelection {
    graph: dependencies::Report,
    receipt: SelectionReceipt,
    model_limits: ModelLimits,
}
fn failure(reason: &str) -> Error {
    Error::Resolution(format!("selected CELL model sources: {reason}"))
}
fn charge(used: &mut usize, bytes: usize, maximum: usize) -> Result<()> {
    *used = used
        .checked_add(bytes)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| failure("selection metadata bound exceeded"))?;
    Ok(())
}
impl CellModelSelection {
    pub fn load(
        store: &mut RecordStore,
        root: &FormKey,
        keys: &[FormKey],
        model_limits: ModelLimits,
        limits: SelectionLimits,
    ) -> Result<Self> {
        let max = SelectionLimits::default();
        let model_limits = model_limits.validate()?;
        if limits.references == 0
            || limits.references > max.references
            || limits.metadata_bytes == 0
            || limits.metadata_bytes > max.metadata_bytes
            || keys.is_empty()
            || keys.len() > limits.references
        {
            return Err(failure("selected reference bounds exceeded"));
        }
        let mut metadata = 0;
        charge(
            &mut metadata,
            4096 + 8 * root.origin_plugin.len(),
            limits.metadata_bytes,
        )?;
        let mut seen = BTreeSet::new();
        for key in keys {
            charge(
                &mut metadata,
                1024 + 8 * key.origin_plugin.len(),
                limits.metadata_bytes,
            )?;
            if key.profile != root.profile || !seen.insert(key) {
                return Err(failure("duplicate reference or different profile"));
            }
        }
        let graph = plan::model_graph(store, root, model_limits, usize::MAX)?;
        // Validate all caller-selected references before cloning keys or any base/MODL read.
        for key in keys {
            let member = graph
                .edges
                .iter()
                .find(|e| {
                    e.owner == *root && e.role == "member" && e.target.key.as_ref() == Some(key)
                })
                .ok_or_else(|| failure("selected reference is outside the exact CELL"))?;
            let node = graph
                .nodes
                .iter()
                .find(|n| n.key == *key)
                .ok_or_else(|| failure("selected source node missing"))?;
            if member.target.status != "resolved"
                || node.header.flags & plugin::DELETED != 0
                || !matches!(&node.header.kind, b"REFR" | b"ACHR" | b"ACRE")
                || !matches!(node.fields, Some(dependencies::Decoded::Placement(_)))
            {
                return Err(failure(
                    "selected reference is deleted, wrongly typed or unavailable",
                ));
            }
            let base = graph
                .edges
                .iter()
                .find(|e| e.owner == *key && e.role == "NAME")
                .ok_or_else(|| failure("selected reference has no source base edge"))?;
            if base.target.status != "resolved" || base.target.key.is_none() {
                return Err(failure("selected base is unresolved or deleted"));
            }
        }
        let mut references = Vec::new();
        for (source_node, node) in graph.nodes.iter().enumerate() {
            if !matches!(&node.header.kind, b"REFR" | b"ACHR" | b"ACRE") {
                continue;
            }
            if !graph.edges.iter().any(|e| {
                e.owner == *root && e.role == "member" && e.target.key.as_ref() == Some(&node.key)
            }) {
                continue;
            }
            charge(
                &mut metadata,
                1024 + 8 * node.key.origin_plugin.len(),
                limits.metadata_bytes,
            )?;
            let selected = seen.contains(&node.key);
            let link = if selected {
                graph
                    .edges
                    .iter()
                    .enumerate()
                    .find(|(_, e)| e.owner == node.key && e.role == "NAME")
            } else {
                None
            };
            let base = link.and_then(|(_, e)| e.target.key.as_ref());
            if let Some(key) = base {
                charge(
                    &mut metadata,
                    512 + 8 * key.origin_plugin.len(),
                    limits.metadata_bytes,
                )?;
            }
            references.push(ReferenceSelection {
                key: node.key.clone(),
                source_node,
                state: if selected {
                    SelectionState::Selected
                } else {
                    SelectionState::NotSelected
                },
                base_edge: link.map(|(index, _)| index),
                base: base.cloned(),
            });
        }
        let requested = keys.to_vec();
        let mut hash = Sha256::new();
        hash.update(b"nv-cell-model-selection-v1\0");
        // All strings here come from the already admitted source/selection copies.
        hash.update(graph.source_cohort_sha256.as_bytes());
        hash.update((root.origin_plugin.len() as u64).to_le_bytes());
        hash.update(root.origin_plugin.as_bytes());
        hash.update(root.local_id.to_le_bytes());
        for key in &requested {
            hash.update((key.origin_plugin.len() as u64).to_le_bytes());
            hash.update(key.origin_plugin.as_bytes());
            hash.update(key.local_id.to_le_bytes());
        }
        let receipt = SelectionReceipt {
            identity: format!("{:x}", hash.finalize()),
            requested,
            references,
            metadata_bytes: metadata,
            limits,
            full_cell_coverage: false,
        };
        Ok(Self {
            graph,
            receipt,
            model_limits,
        })
    }
    pub fn receipt(&self) -> &SelectionReceipt {
        &self.receipt
    }
    pub fn graph(&self) -> &dependencies::Report {
        &self.graph
    }
    pub fn prepare(self, store: &mut RecordStore, mounts: &MountIndex) -> Result<CellModelPlan> {
        if store.indices().len() != self.graph.sources.len()
            || store
                .indices()
                .iter()
                .zip(&self.graph.sources)
                .any(|(i, s)| i.census.name != s.source_name)
        {
            return Err(failure(
                "selected request ordered source names/count changed",
            ));
        }
        let current = store.source_receipts()?;
        if current
            .iter()
            .zip(&self.graph.sources)
            .any(|(a, b)| a.source_bytes != b.source_bytes || a.source_sha256 != b.source_sha256)
        {
            return Err(failure("selected request ordered source bytes changed"));
        }
        let root = self.graph.root.clone();
        CellModelPlan::prepare_graph(
            store,
            &root,
            mounts,
            self.model_limits,
            usize::MAX,
            &mut ArchivePool::new(u64::MAX, usize::MAX),
            self.graph,
            Some(self.receipt),
        )
    }
}
