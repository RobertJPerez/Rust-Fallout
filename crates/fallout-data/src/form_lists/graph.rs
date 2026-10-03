//! Structural list dependencies. Closure collects source links; it does not
//! flatten lists, remove duplicate members or choose a GetItemCount policy.
use super::{Catalogue, Value, budget};
use crate::{Error, Result, identity::FormKey, inventory::Status};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_nodes: usize,
    pub max_edges: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_nodes: 65_536,
            max_edges: 1_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Node {
    pub key: FormKey,
    pub deleted: bool,
}
#[derive(Debug, Serialize)]
pub struct Edge {
    pub source: FormKey,
    pub field_decoded_offset: u32,
    pub target: Option<FormKey>,
    pub status: Status,
    pub target_is_list: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct Counts {
    pub nodes: usize,
    pub edges: usize,
    pub internal_edges: usize,
    pub terminal_edges: usize,
    pub unresolved_edges: usize,
    pub cyclic_components: usize,
    pub cyclic_nodes: usize,
}
#[derive(Debug, Serialize)]
pub struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    cycles: Vec<Vec<FormKey>>,
    counts: Counts,
    #[serde(skip)]
    index: BTreeMap<FormKey, usize>,
    #[serde(skip)]
    outgoing: Vec<Vec<usize>>,
}
#[derive(Debug, Serialize)]
pub struct Closure {
    pub root: FormKey,
    pub nodes: Vec<FormKey>,
    pub edge_indices: Vec<usize>,
}
impl Graph {
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    pub fn cycles(&self) -> &[Vec<FormKey>] {
        &self.cycles
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn build(lists: &Catalogue, limits: Limits) -> Result<Self> {
        if lists.definitions.len() > limits.max_nodes {
            return Err(budget("graph"));
        }
        // Budget actual immutable members, never the editable display counters.
        lists.definitions.values().try_fold(0_usize, |total, d| {
            total
                .checked_add(d.entries.len())
                .filter(|n| *n <= limits.max_edges)
                .ok_or_else(|| budget("graph edges"))
        })?;
        let nodes = lists
            .iter()
            .map(|(key, d)| Node {
                key: key.clone(),
                deleted: d.deleted,
            })
            .collect::<Vec<_>>();
        let index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let mut edges = Vec::new();
        let mut outgoing = vec![Vec::new(); nodes.len()];
        let mut children = vec![Vec::new(); nodes.len()];
        let (mut internal, mut terminal, mut unresolved) = (0, 0, 0);
        for (key, d) in lists.iter() {
            let from = index[key];
            for &field_index in &d.entries {
                let field = &d.fields[field_index];
                let Value::Member { form } = &field.value else {
                    return Err(Error::Unsupported(
                        "form list member index is invalid".into(),
                    ));
                };
                let target_is_list = form.target.as_ref().map(|t| t.kind == *b"FLST");
                outgoing[from].push(edges.len());
                edges.push(Edge {
                    source: key.clone(),
                    field_decoded_offset: field.decoded_offset,
                    target: form.key.clone(),
                    status: form.status,
                    target_is_list,
                });
                if form.status != Status::Defined {
                    unresolved += 1;
                    continue;
                }
                if let Some(to) = form.key.as_ref().and_then(|k| index.get(k)).copied() {
                    children[from].push(to);
                    internal += 1;
                } else {
                    terminal += 1;
                }
            }
        }
        let cycles = crate::graph::cyclic_components(&children)
            .into_iter()
            .map(|c| {
                c.into_iter()
                    .map(|i| nodes[i].key.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let counts = Counts {
            nodes: nodes.len(),
            edges: edges.len(),
            internal_edges: internal,
            terminal_edges: terminal,
            unresolved_edges: unresolved,
            cyclic_components: cycles.len(),
            cyclic_nodes: cycles.iter().map(Vec::len).sum(),
        };
        Ok(Self {
            nodes,
            edges,
            cycles,
            counts,
            index,
            outgoing,
        })
    }
    pub fn closure(&self, root: &FormKey, limits: Limits) -> Result<Closure> {
        let first = *self
            .index
            .get(root)
            .ok_or_else(|| Error::Unsupported("form list root is missing".into()))?;
        if self.nodes[first].deleted {
            return Err(Error::Unsupported("form list root is deleted".into()));
        }
        if limits.max_nodes == 0 {
            return Err(budget("closure nodes"));
        }
        let mut visited = BTreeSet::from([first]);
        let mut pending = VecDeque::from([first]);
        let mut selected = Vec::new();
        while let Some(from) = pending.pop_front() {
            for &e in &self.outgoing[from] {
                if selected.len() >= limits.max_edges {
                    return Err(budget("closure edges"));
                }
                selected.push(e);
                let edge = &self.edges[e];
                if edge.status != Status::Defined {
                    continue;
                }
                if let Some(to) = edge
                    .target
                    .as_ref()
                    .and_then(|k| self.index.get(k))
                    .copied()
                    && !visited.contains(&to)
                {
                    if visited.len() >= limits.max_nodes {
                        return Err(budget("closure nodes"));
                    }
                    visited.insert(to);
                    pending.push_back(to);
                }
            }
        }
        selected.sort_unstable();
        Ok(Closure {
            root: root.clone(),
            nodes: visited
                .into_iter()
                .map(|i| self.nodes[i].key.clone())
                .collect(),
            edge_indices: selected,
        })
    }
}
