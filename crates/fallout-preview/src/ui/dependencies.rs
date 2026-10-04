//! Explicit source endpoint/operand bindings and bounded downstream invalidation.
//! XML reference paths and expression values are deliberately not evaluated here.
use super::{Document, Kind, Span, includes};
use crate::model::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    io::Write,
    mem::size_of,
    path::Path,
};

#[derive(Clone, Copy)]
pub struct Limits {
    pub source: includes::Limits,
    pub request_bytes: usize,
    pub graph_nodes: usize,
    pub bindings: usize,
    pub inputs: usize,
    pub value_bytes: usize,
    pub value_bytes_per_input: usize,
    pub steps: usize,
    pub change_fields: usize,
    pub validation_work: usize,
    pub validation_bytes: usize,
    pub propagation_work: usize,
    pub metadata_bytes: usize,
    pub report_rows: usize,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: includes::Limits::default(),
            request_bytes: 256 * 1024,
            graph_nodes: 1024,
            bindings: 4096,
            inputs: 1024,
            value_bytes: 1024 * 1024,
            value_bytes_per_input: 64 * 1024,
            steps: 128,
            change_fields: 128,
            validation_work: 32768,
            validation_bytes: 8 * 1024 * 1024,
            propagation_work: 16384,
            metadata_bytes: 4 * 1024 * 1024,
            report_rows: 16384,
            output_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub source: usize,
    pub node: usize,
    pub span: Span,
    pub name_span: Span,
}
impl Endpoint {
    fn key(self) -> (usize, usize) {
        (self.source, self.node)
    }
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operand {
    pub name_span: Span,
    pub value_span: Span,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operator {
    pub node: usize,
    pub span: Span,
    pub src: Operand,
    pub trait_operand: Operand,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub from: Endpoint,
    pub to: Endpoint,
    pub operator: Operator,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub endpoint: Endpoint,
    pub value: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub cohort_sha256: String,
    pub expected_revision: u64,
    pub updates: Vec<Input>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub sources: Vec<includes::Source>,
    pub bindings: Vec<Binding>,
    pub inputs: Vec<Input>,
    pub changes: Vec<Change>,
}
pub fn read_request(path: &Path, limits: Limits) -> Result<Request> {
    super::read_json(path, limits.request_bytes, "dependency")
}
/// Domain-separated, ordered, length-prefixed exact path/archive/payload receipt.
pub fn cohort_sha256(sources: &[includes::Source]) -> String {
    let mut hash = Sha256::new();
    hash.update(b"fallout-preview-ui-dependency-cohort-v1\0");
    hash.update((sources.len() as u64).to_le_bytes());
    for source in sources {
        for text in [&source.path, &source.archive_sha256, &source.payload_sha256] {
            hash.update((text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        }
    }
    format!("{:x}", hash.finalize())
}
#[derive(Default, Serialize)]
pub struct SourceUsage {
    pub files: usize,
    pub bytes: usize,
    pub events: usize,
    pub nodes: usize,
    pub metadata_bytes: usize,
}
#[derive(Default, Serialize)]
pub struct GraphUsage {
    pub source: SourceUsage,
    pub bindings: usize,
    pub graph_nodes: usize,
    pub unique_edges: usize,
    pub initial_value_bytes: usize,
    pub validation_work: usize,
    pub validation_bytes: usize,
    pub metadata_bytes: usize,
}
#[derive(Default, Serialize)]
pub struct ChangeUsage {
    pub visits: usize,
    pub copied_value_bytes: usize,
}
#[derive(Serialize)]
pub struct DirtyReport {
    pub cohort_sha256: String,
    pub previous_revision: u64,
    pub revision: u64,
    pub changed_inputs: Vec<Endpoint>,
    pub affected: Vec<Endpoint>,
    pub usage: ChangeUsage,
}
#[derive(Debug)]
pub struct Cycle {
    pub witness: Vec<Endpoint>,
}
impl fmt::Display for Cycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Menu dependency cycle witness")?;
        for node in &self.witness {
            write!(
                f,
                " -> source{} node{} span{}..{}",
                node.source, node.node, node.span.start, node.span.end
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for Cycle {}
fn charge(total: &mut usize, amount: usize, maximum: usize, what: &str) -> Result<()> {
    *total = total
        .checked_add(amount)
        .filter(|next| *next <= maximum)
        .ok_or_else(|| format!("Menu dependency {what} budget exceeded"))?;
    Ok(())
}
fn text(document: &Document, span: Span) -> Result<&str> {
    document
        .source_utf8
        .get(span.start..span.end)
        .ok_or_else(|| "Menu dependency invalid source span".into())
}
fn tile(name: &str) -> bool {
    matches!(
        name,
        "menu" | "rect" | "image" | "text" | "nif" | "3d" | "hotrect" | "window" | "radial"
    )
}
fn operator(name: &str) -> bool {
    matches!(
        name,
        "copy"
            | "add"
            | "sub"
            | "mul"
            | "mult"
            | "div"
            | "min"
            | "max"
            | "mod"
            | "floor"
            | "ceil"
            | "abs"
            | "round"
            | "gt"
            | "gte"
            | "eq"
            | "neq"
            | "lt"
            | "lte"
            | "and"
            | "or"
            | "not"
            | "onlyif"
            | "onlyifnot"
            | "ref"
            | "begin"
            | "end"
    )
}
fn source_admit(
    usage: &mut SourceUsage,
    document: &Document,
    limits: includes::Limits,
) -> Result<()> {
    let d = limits.document;
    if document.source_utf8.len() > d.source_bytes
        || document.nodes.len() > d.nodes
        || document.event_count > d.events
        || document.charged_metadata_bytes > d.metadata_bytes
    {
        return Err("Menu dependency per-document budget exceeded".into());
    }
    charge(&mut usage.files, 1, limits.files, "source file")?;
    charge(
        &mut usage.bytes,
        document.source_utf8.len(),
        limits.source_bytes,
        "source byte",
    )?;
    charge(
        &mut usage.events,
        document.event_count,
        limits.events,
        "source event",
    )?;
    charge(
        &mut usage.nodes,
        document.nodes.len(),
        limits.nodes,
        "source node",
    )?;
    charge(
        &mut usage.metadata_bytes,
        document.charged_metadata_bytes,
        limits.metadata_bytes,
        "source metadata",
    )
}
fn step(usage: &mut GraphUsage, limits: Limits) -> Result<()> {
    charge(
        &mut usage.validation_work,
        1,
        limits.validation_work,
        "validation work",
    )
}
fn endpoint<'a>(
    documents: &[&'a Document],
    endpoint: Endpoint,
    usage: &mut GraphUsage,
    limits: Limits,
) -> Result<&'a str> {
    step(usage, limits)?;
    let document = documents
        .get(endpoint.source)
        .ok_or("Menu dependency endpoint source missing")?;
    let node = document
        .nodes
        .get(endpoint.node)
        .filter(|n| {
            n.kind == Kind::Element && n.span == endpoint.span && n.name == Some(endpoint.name_span)
        })
        .ok_or("Menu dependency endpoint node/name/spans differ")?;
    let name = text(document, endpoint.name_span)?;
    charge(
        &mut usage.validation_bytes,
        name.len(),
        limits.validation_bytes,
        "validation byte",
    )?;
    if tile(name) || operator(name) || matches!(name, "template" | "include") {
        return Err("Menu dependency endpoint is not a source trait".into());
    }
    step(usage, limits)?;
    let parent = node
        .parent
        .and_then(|id| document.nodes.get(id))
        .filter(|n| n.kind == Kind::Element)
        .ok_or("Menu dependency trait tile parent missing")?;
    if !tile(text(
        document,
        parent.name.ok_or("Menu dependency tile name missing")?,
    )?) {
        return Err("Menu dependency endpoint is not a direct tile trait".into());
    }
    Ok(name)
}
fn operand<'a>(
    document: &'a Document,
    node: &super::Node,
    operand: Operand,
    name: &str,
    usage: &mut GraphUsage,
    limits: Limits,
) -> Result<&'a str> {
    step(usage, limits)?;
    if !node
        .attributes
        .iter()
        .any(|a| a.name == operand.name_span && a.raw_value == operand.value_span)
        || text(document, operand.name_span)? != name
    {
        return Err("Menu dependency operator operand name/spans differ".into());
    }
    let value = text(document, operand.value_span)?;
    charge(
        &mut usage.validation_bytes,
        value.len(),
        limits.validation_bytes,
        "validation byte",
    )?;
    Ok(value)
}
fn validate_binding(
    documents: &[&Document],
    binding: &Binding,
    usage: &mut GraphUsage,
    limits: Limits,
) -> Result<()> {
    let name = endpoint(documents, binding.from, usage, limits)?;
    endpoint(documents, binding.to, usage, limits)?;
    step(usage, limits)?;
    let document = documents[binding.to.source];
    let node = document
        .nodes
        .get(binding.operator.node)
        .filter(|n| n.kind == Kind::Element && n.span == binding.operator.span)
        .ok_or("Menu dependency operator identity/span differs")?;
    if !operator(text(
        document,
        node.name.ok_or("Menu dependency operator name missing")?,
    )?) || node.attributes.len() != 2
    {
        return Err("Menu dependency operator/attributes unsupported".into());
    }
    operand(document, node, binding.operator.src, "src", usage, limits)?;
    if operand(
        document,
        node,
        binding.operator.trait_operand,
        "trait",
        usage,
        limits,
    )? != name
    {
        return Err("Menu dependency bound trait name differs from source operand".into());
    }
    let mut parent = node.parent;
    while parent != Some(binding.to.node) {
        step(usage, limits)?;
        let ancestor = parent
            .and_then(|id| document.nodes.get(id))
            .filter(|n| n.kind == Kind::Element)
            .ok_or("Menu dependency operator not inside bound trait")?;
        if !operator(text(
            document,
            ancestor
                .name
                .ok_or("Menu dependency ancestor name missing")?,
        )?) {
            return Err("Menu dependency operator crosses another trait/tile".into());
        }
        parent = ancestor.parent;
    }
    Ok(())
}

pub struct Session<'a> {
    sources: &'a [includes::Source],
    cohort: String,
    revision: u64,
    nodes: Vec<Endpoint>,
    index: BTreeMap<(usize, usize), usize>,
    dependents: Vec<Vec<usize>>,
    topological: Vec<usize>,
    values: Vec<Option<String>>,
    value_bytes: usize,
    limits: Limits,
    pub graph: GraphUsage,
}
impl<'a> Session<'a> {
    pub fn new(documents: &[&Document], request: &'a Request, limits: Limits) -> Result<Self> {
        if request.schema_version != 1
            || request.sources.is_empty()
            || documents.len() != request.sources.len()
        {
            return Err("Menu dependency request schema/source cohort differs".into());
        }
        if request.sources.len() > limits.source.files
            || request.bindings.len() > limits.bindings
            || request.inputs.len() > limits.inputs
            || request.changes.len() > limits.steps
        {
            return Err("Menu dependency request count budget exceeded".into());
        }
        let mut graph = GraphUsage {
            bindings: request.bindings.len(),
            ..GraphUsage::default()
        };
        let mut paths = BTreeSet::new();
        for (source, document) in request.sources.iter().zip(documents) {
            let path = includes::path(&source.path)?;
            includes::hash(&source.archive_sha256)?;
            includes::hash(&source.payload_sha256)?;
            if !paths.insert(path) {
                return Err("Menu dependency source member duplicated".into());
            }
            source_admit(&mut graph.source, document, limits.source)?;
            if format!("{:x}", Sha256::digest(document.source_utf8.as_bytes()))
                != source.payload_sha256
            {
                return Err("Menu dependency source payload SHA differs".into());
            }
        }
        let mut endpoints = BTreeMap::new();
        let mut resolved_operands = BTreeMap::new();
        // Declared logical storage for endpoint/index/adjacency/value and bounded
        // DFS/change scratch. Allocator nodes/capacity peak are not this metric.
        let node_bytes = 3 * size_of::<Endpoint>()
            + 3 * size_of::<((usize, usize), usize)>()
            + 2 * size_of::<Vec<usize>>()
            + size_of::<Option<String>>()
            + 9 * size_of::<usize>()
            + 2;
        for binding in &request.bindings {
            validate_binding(documents, binding, &mut graph, limits)?;
            let slot = (binding.to.source, binding.operator.node);
            let resolution = (binding.from, binding.to);
            if let Some(previous) = resolved_operands.get(&slot) {
                if *previous != resolution {
                    return Err("Menu dependency operand bound to multiple source traits".into());
                }
            } else {
                charge(
                    &mut graph.metadata_bytes,
                    size_of::<((usize, usize), (Endpoint, Endpoint))>(),
                    limits.metadata_bytes,
                    "metadata",
                )?;
                resolved_operands.insert(slot, resolution);
            }
            for endpoint in [binding.from, binding.to] {
                if !endpoints.contains_key(&endpoint.key()) {
                    charge(&mut graph.graph_nodes, 1, limits.graph_nodes, "graph node")?;
                    charge(
                        &mut graph.metadata_bytes,
                        node_bytes,
                        limits.metadata_bytes,
                        "metadata",
                    )?;
                }
                endpoints.insert(endpoint.key(), endpoint);
            }
        }
        let mut initial = BTreeMap::new();
        for input in &request.inputs {
            endpoint(documents, input.endpoint, &mut graph, limits)?;
            if input.value.len() > limits.value_bytes_per_input {
                return Err("Menu dependency input value byte budget exceeded".into());
            }
            charge(
                &mut graph.initial_value_bytes,
                input.value.len(),
                limits.value_bytes,
                "initial value byte",
            )?;
            if initial.insert(input.endpoint.key(), &input.value).is_some() {
                return Err("Menu dependency initial input duplicated".into());
            }
            if !endpoints.contains_key(&input.endpoint.key()) {
                charge(&mut graph.graph_nodes, 1, limits.graph_nodes, "graph node")?;
                charge(
                    &mut graph.metadata_bytes,
                    node_bytes,
                    limits.metadata_bytes,
                    "metadata",
                )?;
            }
            endpoints.insert(input.endpoint.key(), input.endpoint);
        }
        let nodes: Vec<_> = endpoints.into_values().collect();
        let index: BTreeMap<_, _> = nodes
            .iter()
            .enumerate()
            .map(|(id, node)| (node.key(), id))
            .collect();
        let mut edges = BTreeSet::new();
        for binding in &request.bindings {
            let edge = (index[&binding.from.key()], index[&binding.to.key()]);
            if !edges.contains(&edge) {
                charge(&mut graph.unique_edges, 1, limits.bindings, "unique edge")?;
                charge(
                    &mut graph.metadata_bytes,
                    3 * size_of::<usize>(),
                    limits.metadata_bytes,
                    "metadata",
                )?;
            }
            edges.insert(edge);
        }
        let mut dependents = vec![Vec::new(); nodes.len()];
        for (from, to) in edges {
            dependents[from].push(to);
        }
        let mut colors = vec![0u8; nodes.len()];
        let mut topological = Vec::new();
        for root in 0..nodes.len() {
            if colors[root] != 0 {
                continue;
            }
            let mut stack = vec![(root, 0)];
            colors[root] = 1;
            while let Some((node, next)) = stack.last_mut() {
                step(&mut graph, limits)?;
                if *next == dependents[*node].len() {
                    colors[*node] = 2;
                    topological.push(*node);
                    stack.pop();
                    continue;
                }
                let child = dependents[*node][*next];
                *next += 1;
                match colors[child] {
                    0 => {
                        colors[child] = 1;
                        stack.push((child, 0));
                    }
                    1 => {
                        let start = stack
                            .iter()
                            .position(|(id, _)| *id == child)
                            .expect("active cycle node");
                        charge(
                            &mut graph.metadata_bytes,
                            (stack.len() - start + 1) * size_of::<Endpoint>(),
                            limits.metadata_bytes,
                            "cycle witness metadata",
                        )?;
                        let mut witness: Vec<_> =
                            stack[start..].iter().map(|(id, _)| nodes[*id]).collect();
                        witness.push(nodes[child]);
                        return Err(Box::new(Cycle { witness }));
                    }
                    _ => {}
                }
            }
        }
        topological.reverse();
        let values: Vec<_> = nodes
            .iter()
            .map(|node| initial.get(&node.key()).map(|value| (*value).clone()))
            .collect();
        let cohort = cohort_sha256(&request.sources);
        Ok(Self {
            sources: &request.sources,
            cohort,
            revision: 0,
            nodes,
            index,
            dependents,
            topological,
            value_bytes: graph.initial_value_bytes,
            values,
            limits,
            graph,
        })
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn cohort(&self) -> &str {
        &self.cohort
    }
    pub fn snapshot(&self) -> Vec<Input> {
        self.nodes
            .iter()
            .zip(&self.values)
            .filter_map(|(endpoint, value)| {
                value.as_ref().map(|value| Input {
                    endpoint: *endpoint,
                    value: value.clone(),
                })
            })
            .collect()
    }
    pub fn change(&mut self, change: &Change) -> Result<DirtyReport> {
        self.change_bounded(change, self.limits.report_rows)
    }
    fn change_bounded(&mut self, change: &Change, report_rows: usize) -> Result<DirtyReport> {
        if change.cohort_sha256 != self.cohort || cohort_sha256(self.sources) != self.cohort {
            return Err("Menu dependency change source cohort differs".into());
        }
        if change.expected_revision != self.revision {
            return Err("Menu dependency change revision differs".into());
        }
        if change.updates.len() > self.limits.change_fields {
            return Err("Menu dependency change field budget exceeded".into());
        }
        let mut seen = BTreeSet::new();
        let mut changed = BTreeMap::new();
        let mut removed_bytes = 0;
        let mut replacement_bytes = 0;
        let mut update_bytes = 0;
        for input in &change.updates {
            let id = *self
                .index
                .get(&input.endpoint.key())
                .ok_or("Menu dependency change endpoint missing")?;
            if self.nodes[id] != input.endpoint || !seen.insert(id) {
                return Err("Menu dependency change endpoint spans differ or duplicate".into());
            }
            let old = self.values[id]
                .as_ref()
                .ok_or("Menu dependency change input was not explicitly initialized")?;
            if input.value.len() > self.limits.value_bytes_per_input {
                return Err("Menu dependency change value byte budget exceeded".into());
            }
            charge(
                &mut update_bytes,
                input.value.len(),
                self.limits.value_bytes,
                "update value byte",
            )?;
            if *old == input.value {
                continue;
            }
            removed_bytes += old.len();
            replacement_bytes += input.value.len();
            changed.insert(id, &input.value);
        }
        let mut value_bytes = self
            .value_bytes
            .checked_sub(removed_bytes)
            .ok_or("Menu dependency state value size inconsistent")?;
        charge(
            &mut value_bytes,
            replacement_bytes,
            self.limits.value_bytes,
            "state value byte",
        )?;
        let mut usage = ChangeUsage::default();
        let mut visited = vec![false; self.nodes.len()];
        let mut queue = VecDeque::new();
        for root in changed.keys() {
            charge(
                &mut usage.visits,
                1,
                self.limits.propagation_work,
                "propagation work",
            )?;
            visited[*root] = true;
            queue.push_back(*root);
        }
        while let Some(node) = queue.pop_front() {
            for child in &self.dependents[node] {
                charge(
                    &mut usage.visits,
                    1,
                    self.limits.propagation_work,
                    "propagation work",
                )?;
                if visited[*child] {
                    continue;
                }
                charge(
                    &mut usage.visits,
                    1,
                    self.limits.propagation_work,
                    "propagation work",
                )?;
                visited[*child] = true;
                queue.push_back(*child);
            }
        }
        let next = if changed.is_empty() {
            self.revision
        } else {
            self.revision
                .checked_add(1)
                .ok_or("Menu dependency session revision exhausted")?
        };
        let affected_count = self
            .topological
            .iter()
            .filter(|id| visited[**id] && !changed.contains_key(id))
            .count();
        let mut row_count = 0;
        charge(
            &mut row_count,
            changed.len() + affected_count,
            report_rows.min(self.limits.report_rows),
            "report row",
        )?;
        let affected = self
            .topological
            .iter()
            .filter(|id| visited[**id] && !changed.contains_key(id))
            .map(|id| self.nodes[*id])
            .collect();
        let changed_inputs = changed.keys().map(|id| self.nodes[*id]).collect();
        let previous_revision = self.revision;
        // Source identity, full change, state byte, traversal and revision admission
        // all finish before any replacement string is copied or revision changes.
        for (id, value) in changed {
            usage.copied_value_bytes += value.len();
            self.values[id] = Some(value.clone());
        }
        self.value_bytes = value_bytes;
        self.revision = next;
        Ok(DirtyReport {
            cohort_sha256: self.cohort.clone(),
            previous_revision,
            revision: next,
            changed_inputs,
            affected,
            usage,
        })
    }
}

#[derive(Serialize)]
pub struct Report<'a> {
    pub schema_version: u32,
    pub sources: Vec<super::Report>,
    pub request: &'a Request,
    pub cohort_sha256: String,
    pub graph: GraphUsage,
    pub changes: Vec<DirtyReport>,
    pub final_revision: u64,
    pub final_inputs: Vec<Input>,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}
pub fn inspect<'a>(install: &Path, request: &'a Request, limits: Limits) -> Result<Report<'a>> {
    if request.schema_version != 1
        || request.sources.is_empty()
        || request.sources.len() > limits.source.files
        || request.changes.len() > limits.steps
    {
        return Err("Menu dependency request schema/source/step budget differs".into());
    }
    let mut sources = Vec::new();
    let mut usage = SourceUsage::default();
    let mut paths = BTreeSet::new();
    for source in &request.sources {
        let path = includes::path(&source.path)?;
        includes::hash(&source.archive_sha256)?;
        includes::hash(&source.payload_sha256)?;
        if !paths.insert(path.clone()) {
            return Err("Menu dependency source member duplicated".into());
        }
        let d = limits.source.document;
        let remaining = super::Limits {
            source_bytes: d.source_bytes.min(limits.source.source_bytes - usage.bytes),
            events: d.events.min(limits.source.events - usage.events),
            nodes: d.nodes.min(limits.source.nodes - usage.nodes),
            metadata_bytes: d
                .metadata_bytes
                .min(limits.source.metadata_bytes - usage.metadata_bytes),
            ..d
        };
        let report = super::inspect(install, &path, None, remaining)?;
        if report.archive_sha256 != source.archive_sha256
            || report.payload_sha256 != source.payload_sha256
        {
            return Err("Menu dependency source archive/payload SHA differs".into());
        }
        source_admit(&mut usage, &report.document, limits.source)?;
        sources.push(report);
    }
    let documents: Vec<_> = sources.iter().map(|source| &source.document).collect();
    let mut session = Session::new(&documents, request, limits)?;
    let cohort_sha256 = session.cohort().to_owned();
    let mut rows = 0;
    let mut changes = Vec::new();
    for change in &request.changes {
        let report = if rows == 0 {
            session.change(change)?
        } else {
            session.change_bounded(change, limits.report_rows - rows)?
        };
        charge(
            &mut rows,
            report.changed_inputs.len() + report.affected.len(),
            limits.report_rows,
            "report row",
        )?;
        changes.push(report);
    }
    let final_revision = session.revision();
    let final_inputs = session.snapshot();
    let graph = session.graph;
    Ok(Report {
        schema_version: 1,
        sources,
        request,
        cohort_sha256,
        graph,
        changes,
        final_revision,
        final_inputs,
        interpretation: "Only caller-resolved exact source trait/operator/operand bindings and opaque host input invalidation; no XML path resolution, expression values, engine defaults, template/font/layout/action or original display",
        original_display_ready: false,
    })
}
pub fn write_report(writer: impl Write, report: &Report<'_>, limit: usize) -> Result<()> {
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
