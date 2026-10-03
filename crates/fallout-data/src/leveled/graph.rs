//! Bounded authored dependency planning. Cycles are facts, not expansion rules.
use super::{Catalogue, Value};
use crate::{Error, Result, identity::FormKey, inventory};
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
            max_nodes: 131_072,
            max_edges: 2_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Node {
    pub key: FormKey,
    pub kind: [u8; 4],
    pub deleted: bool,
}
#[derive(Debug, Serialize)]
pub struct Edge {
    pub source: FormKey,
    pub field_decoded_offset: u32,
    pub role: &'static str,
    pub target: Option<FormKey>,
    pub status: inventory::Status,
    pub schema_kind_allowed: Option<bool>,
}
#[derive(Debug, Serialize)]
pub struct Counts {
    pub nodes: usize,
    pub edges: usize,
    pub internal_edges: usize,
    pub terminal_edges: usize,
    pub unresolved_edges: usize,
    pub schema_mismatches: usize,
    pub cyclic_components: usize,
    pub cyclic_nodes: usize,
}
#[derive(Debug, Serialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub cycles: Vec<Vec<FormKey>>,
    pub counts: Counts,
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
    pub fn build(base: &inventory::Catalogue, lists: &Catalogue, limits: Limits) -> Result<Self> {
        if base.winning_content_sha256() != lists.winning_content_sha256()
            || !base
                .sources
                .iter()
                .map(|s| (&s.source_name, s.source_bytes, &s.source_sha256))
                .eq(lists
                    .sources
                    .iter()
                    .map(|s| (&s.source_name, s.source_bytes, &s.source_sha256)))
        {
            return Err(Error::Unsupported(
                "inventory/list source cohorts differ".into(),
            ));
        }
        let mut nodes = BTreeMap::new();
        for (key, kind, deleted) in base
            .iter()
            .map(|(k, d)| (k, d.kind, d.deleted))
            .chain(lists.iter().map(|(k, d)| (k, d.kind, d.deleted)))
        {
            if nodes.len() >= limits.max_nodes {
                return Err(Error::Unsupported(
                    "inventory graph node budget exceeded".into(),
                ));
            }
            if nodes
                .insert(
                    key.clone(),
                    Node {
                        key: key.clone(),
                        kind,
                        deleted,
                    },
                )
                .is_some()
            {
                return Err(Error::Unsupported(
                    "duplicate inventory graph identity".into(),
                ));
            }
        }
        let nodes = nodes.into_values().collect::<Vec<_>>();
        let index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let mut edges = Vec::new();
        let mut add = |source: &FormKey,
                       offset: u32,
                       role: &'static str,
                       binding: &inventory::Binding,
                       allowed: Option<bool>|
         -> Result<()> {
            if edges.len() >= limits.max_edges {
                return Err(Error::Unsupported(
                    "inventory graph edge budget exceeded".into(),
                ));
            }
            edges.push(Edge {
                source: source.clone(),
                field_decoded_offset: offset,
                role,
                target: binding.key.clone(),
                status: binding.status,
                schema_kind_allowed: allowed,
            });
            Ok(())
        };
        for (key, d) in base.iter() {
            for f in &d.fields {
                match &f.value {
                    inventory::Value::Item {
                        item,
                        schema_kind_allowed,
                        ..
                    } => add(
                        key,
                        f.decoded_offset,
                        "base-item",
                        item,
                        *schema_kind_allowed,
                    )?,
                    inventory::Value::Template { template } => {
                        let allowed = template.target.as_ref().map(|t| match &d.kind {
                            b"NPC_" => matches!(&t.kind, b"NPC_" | b"LVLN"),
                            b"CREA" => matches!(&t.kind, b"CREA" | b"LVLC"),
                            _ => false,
                        });
                        add(key, f.decoded_offset, "actor-template", template, allowed)?;
                    }
                    _ => {}
                }
            }
        }
        for (key, d) in lists.iter() {
            for f in &d.fields {
                if let Value::Entry {
                    item,
                    schema_kind_allowed,
                    ..
                } = &f.value
                {
                    add(
                        key,
                        f.decoded_offset,
                        "leveled-entry",
                        item,
                        *schema_kind_allowed,
                    )?;
                }
            }
        }
        // Keep physical duplicates and their positions; only traversal deduplicates visits.
        edges.sort_by(|a, b| {
            (&a.source, a.field_decoded_offset, a.role).cmp(&(
                &b.source,
                b.field_decoded_offset,
                b.role,
            ))
        });
        let mut children = vec![Vec::new(); nodes.len()];
        let mut outgoing = vec![Vec::new(); nodes.len()];
        let mut internal = 0;
        let mut terminal = 0;
        let mut unresolved = 0;
        let mut mismatches = 0;
        for (i, e) in edges.iter().enumerate() {
            let from = index[&e.source];
            outgoing[from].push(i);
            mismatches += usize::from(e.schema_kind_allowed == Some(false));
            if e.status != inventory::Status::Defined {
                unresolved += 1;
                continue;
            }
            if let Some(to) = e.target.as_ref().and_then(|k| index.get(k)).copied() {
                children[from].push(to);
                internal += 1;
            } else {
                terminal += 1;
            }
        }
        let components = cyclic_components(&children);
        let cycles = components
            .into_iter()
            .map(|component| {
                component
                    .into_iter()
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
            schema_mismatches: mismatches,
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
    /// Structural closure within inventory/list/template records. Terminal item
    /// bodies are dependencies, not silently loaded or interpreted by this plan.
    pub fn closure(&self, root: &FormKey, limits: Limits) -> Result<Closure> {
        let first = *self
            .index
            .get(root)
            .ok_or_else(|| Error::Unsupported("inventory closure root missing".into()))?;
        if self.nodes[first].deleted {
            return Err(Error::Unsupported("inventory closure root deleted".into()));
        }
        if limits.max_nodes == 0 {
            return Err(Error::Unsupported(
                "inventory closure node budget exceeded".into(),
            ));
        }
        let mut seen = BTreeSet::from([first]);
        let mut queue = VecDeque::from([first]);
        let mut selected = Vec::new();
        while let Some(from) = queue.pop_front() {
            for &i in &self.outgoing[from] {
                if selected.len() >= limits.max_edges {
                    return Err(Error::Unsupported(
                        "inventory closure edge budget exceeded".into(),
                    ));
                }
                selected.push(i);
                let e = &self.edges[i];
                if e.status != inventory::Status::Defined {
                    continue;
                }
                if let Some(to) = e.target.as_ref().and_then(|k| self.index.get(k)).copied()
                    && !seen.contains(&to)
                {
                    if seen.len() >= limits.max_nodes {
                        return Err(Error::Unsupported(
                            "inventory closure node budget exceeded".into(),
                        ));
                    }
                    seen.insert(to);
                    queue.push_back(to);
                }
            }
        }
        selected.sort_unstable();
        Ok(Closure {
            root: root.clone(),
            nodes: seen
                .into_iter()
                .map(|i| self.nodes[i].key.clone())
                .collect(),
            edge_indices: selected,
        })
    }
}
// Iterative Kosaraju keeps traversal memory proportional to the bounded graph.
// A deeply nested authored list cannot consume the native call stack.
fn cyclic_components(children: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut reverse = vec![Vec::new(); children.len()];
    for (from, targets) in children.iter().enumerate() {
        for &to in targets {
            reverse[to].push(from);
        }
    }
    let mut seen = vec![false; children.len()];
    let mut order = Vec::with_capacity(children.len());
    for root in 0..children.len() {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut stack = vec![(root, 0)];
        while let Some((node, next)) = stack.last_mut() {
            if *next < children[*node].len() {
                let to = children[*node][*next];
                *next += 1;
                if !seen[to] {
                    seen[to] = true;
                    stack.push((to, 0));
                }
            } else {
                let (node, _) = stack.pop().expect("active graph frame");
                order.push(node);
            }
        }
    }
    seen.fill(false);
    let mut result = Vec::new();
    for &root in order.iter().rev() {
        if seen[root] {
            continue;
        }
        seen[root] = true;
        let mut stack = vec![root];
        let mut component = Vec::new();
        while let Some(node) = stack.pop() {
            component.push(node);
            for &to in &reverse[node] {
                if !seen[to] {
                    seen[to] = true;
                    stack.push(to);
                }
            }
        }
        component.sort_unstable();
        if component.len() > 1 || children[root].contains(&root) {
            result.push(component);
        }
    }
    result.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycles_distinguish_shared_children_self_links_and_disconnected_components() {
        let graph = vec![
            vec![1, 1, 3],
            vec![2],
            vec![1, 3],
            vec![],
            vec![4],
            vec![6],
            vec![],
        ];
        assert_eq!(cyclic_components(&graph), vec![vec![1, 2], vec![4]]);
    }
    #[test]
    fn very_deep_graph_uses_no_recursive_call_stack() {
        let mut graph = (0..100_000)
            .map(|i| {
                if i + 1 < 100_000 {
                    vec![i + 1]
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>();
        assert!(cyclic_components(&graph).is_empty());
        graph[99_999].push(0);
        let cycles = cyclic_components(&graph);
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].len(), 100_000);
    }
}
