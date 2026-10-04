//! Exact caller-bound include dependencies, without a relative-path or template
//! expansion policy. Every edge retains its original source-value span.
use super::{Document, Kind, Span, parse};
use crate::model::Result;
use fallout_data::{
    assets::ArchiveAssets,
    baseline,
    vfs::{AssetPath, AssetSource},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    mem::size_of,
    path::Path,
};

#[derive(Clone, Copy)]
pub struct Limits {
    pub document: super::Limits,
    pub files: usize,
    pub edges: usize,
    pub depth: usize,
    pub source_bytes: usize,
    pub events: usize,
    pub nodes: usize,
    pub metadata_bytes: usize,
    pub request_bytes: usize,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            document: super::Limits::default(),
            files: 64,
            edges: 256,
            depth: 16,
            source_bytes: 4 * 1024 * 1024,
            events: 131072,
            nodes: 65536,
            metadata_bytes: 16 * 1024 * 1024,
            request_bytes: 128 * 1024,
            output_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: String,
    pub archive_sha256: String,
    pub payload_sha256: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub source_path: String,
    pub source_payload_sha256: String,
    pub src_span: Span,
    pub raw_src: String,
    pub target: Source,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub root: Source,
    pub bindings: Vec<Binding>,
}

pub fn read_request(path: &Path, limits: Limits) -> Result<Request> {
    let file = baseline::open_source(path)?;
    if file.metadata()?.len() > limits.request_bytes as u64 {
        return Err("Menu include request byte budget exceeded".into());
    }
    let mut bytes = Vec::new();
    let maximum = u64::try_from(limits.request_bytes)?
        .checked_add(1)
        .ok_or("Menu include request byte limit overflow")?;
    file.take(maximum).read_to_end(&mut bytes)?;
    if bytes.len() > limits.request_bytes {
        return Err("Menu include request byte budget exceeded".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Serialize)]
pub struct File {
    pub path: AssetPath,
    pub source: AssetSource,
    pub archive_sha256: String,
    pub payload_sha256: String,
    pub document: Document,
}
#[derive(Serialize)]
pub struct Edge {
    pub source_file: usize,
    pub include_node: usize,
    pub src_span: Span,
    pub target_file: usize,
    pub request_binding: usize,
}
#[derive(Default, Serialize)]
pub struct Usage {
    pub source_bytes: usize,
    pub events: usize,
    pub nodes: usize,
    pub metadata_bytes: usize,
    pub maximum_depth: usize,
}
#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub files: Vec<File>,
    pub edges: Vec<Edge>,
    pub usage: Usage,
    pub selected_root_element: Option<usize>,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}

fn path(raw: &str) -> Result<AssetPath> {
    if raw.len() > 4096 {
        return Err("Menu include path exceeds 4096 bytes".into());
    }
    Ok(AssetPath::new(raw.as_bytes())?)
}
fn hash(raw: &str) -> Result<()> {
    if raw.len() != 64
        || !raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Menu include requires exact lowercase SHA-256".into());
    }
    Ok(())
}
fn charge(total: &mut usize, amount: usize, maximum: usize, what: &str) -> Result<()> {
    let next = total
        .checked_add(amount)
        .filter(|n| *n <= maximum)
        .ok_or_else(|| format!("Menu include {what} budget exceeded"))?;
    *total = next;
    Ok(())
}

struct Loader<'a> {
    assets: &'a mut ArchiveAssets,
    limits: Limits,
    files: Vec<File>,
    usage: Usage,
}
impl Loader<'_> {
    fn load(&mut self, expected: &Source) -> Result<usize> {
        if self.files.len() >= self.limits.files {
            return Err("Menu include file budget exceeded".into());
        }
        let key = path(&expected.path)?;
        let candidates = self.assets.candidates(&key)?;
        let [candidate] = candidates else {
            return Err(format!(
                "Menu include member {:?} has {} candidates",
                expected.path,
                candidates.len()
            )
            .into());
        };
        // Charge fixed records, retained paths/hash strings and the lookup entry
        // before source read. Parser metadata is separately admitted below.
        let fixed = size_of::<File>()
            + size_of::<AssetPath>()
            + size_of::<usize>()
            + key.bytes().len() * 2
            + candidate.container.len()
            + candidate.original_path.len()
            + 128;
        charge(
            &mut self.usage.metadata_bytes,
            fixed,
            self.limits.metadata_bytes,
            "metadata",
        )?;
        let document_limits = super::Limits {
            source_bytes: self.limits.document.source_bytes.min(
                self.limits
                    .source_bytes
                    .saturating_sub(self.usage.source_bytes),
            ),
            events: self
                .limits
                .document
                .events
                .min(self.limits.events.saturating_sub(self.usage.events)),
            nodes: self
                .limits
                .document
                .nodes
                .min(self.limits.nodes.saturating_sub(self.usage.nodes)),
            metadata_bytes: self.limits.document.metadata_bytes.min(
                self.limits
                    .metadata_bytes
                    .saturating_sub(self.usage.metadata_bytes),
            ),
            ..self.limits.document
        };
        let (source, bytes) = self
            .assets
            .read_unique_bounded(&key, document_limits.source_bytes as u64)?;
        let archive_sha256 = self.assets.source_digest(&source)?;
        if archive_sha256 != expected.archive_sha256 {
            return Err(format!("Menu include archive SHA differs for {:?}", expected.path).into());
        }
        let archive_sha256 = archive_sha256.to_owned();
        let payload_sha256 = format!("{:x}", Sha256::digest(&bytes));
        if payload_sha256 != expected.payload_sha256 {
            return Err(format!("Menu include payload SHA differs for {:?}", expected.path).into());
        }
        charge(
            &mut self.usage.source_bytes,
            bytes.len(),
            self.limits.source_bytes,
            "source bytes",
        )?;
        let document = parse(bytes, document_limits)?;
        charge(
            &mut self.usage.events,
            document.event_count,
            self.limits.events,
            "events",
        )?;
        charge(
            &mut self.usage.nodes,
            document.nodes.len(),
            self.limits.nodes,
            "nodes",
        )?;
        charge(
            &mut self.usage.metadata_bytes,
            document.charged_metadata_bytes,
            self.limits.metadata_bytes,
            "metadata",
        )?;
        let index = self.files.len();
        self.files.push(File {
            path: key,
            source,
            archive_sha256,
            payload_sha256,
            document,
        });
        Ok(index)
    }
}

struct Frame {
    file: usize,
    next_node: usize,
    via: Option<usize>,
}

pub fn inspect(
    install: &Path,
    root: &AssetPath,
    selected: Option<&str>,
    request: Request,
    limits: Limits,
) -> Result<Report> {
    let mut assets = ArchiveAssets::open_nv(install)?;
    inspect_assets(&mut assets, root, selected, request, limits)
}

fn inspect_assets(
    assets: &mut ArchiveAssets,
    root: &AssetPath,
    selected: Option<&str>,
    request: Request,
    limits: Limits,
) -> Result<Report> {
    if request.schema_version != 1 || path(&request.root.path)? != *root {
        return Err("Menu include request schema/root differs".into());
    }
    if request.bindings.len() > limits.edges || limits.depth == 0 {
        return Err("Menu include request edge/depth budget exceeded".into());
    }
    hash(&request.root.archive_sha256)?;
    hash(&request.root.payload_sha256)?;
    let mut expected = BTreeMap::new();
    expected.insert(root.clone(), &request.root);
    let mut bindings = BTreeMap::new();
    for (index, binding) in request.bindings.iter().enumerate() {
        hash(&binding.source_payload_sha256)?;
        hash(&binding.target.archive_sha256)?;
        hash(&binding.target.payload_sha256)?;
        path(&binding.raw_src)?;
        if binding.raw_src.contains('&') || binding.src_span.start >= binding.src_span.end {
            return Err("Menu include binding requires a nonempty literal source path/span".into());
        }
        let source = path(&binding.source_path)?;
        if bindings
            .insert(
                (source, binding.src_span.start, binding.src_span.end),
                index,
            )
            .is_some()
        {
            return Err("Menu include source-span binding duplicated".into());
        }
        let target = path(&binding.target.path)?;
        if let Some(previous) = expected.insert(target, &binding.target)
            && (previous.archive_sha256 != binding.target.archive_sha256
                || previous.payload_sha256 != binding.target.payload_sha256)
        {
            return Err("Menu include member has conflicting expected source hashes".into());
        }
    }
    let mut loader = Loader {
        assets,
        limits,
        files: Vec::new(),
        usage: Usage::default(),
    };
    loader.load(&request.root)?;
    let selected_root_element = selected
        .map(|name| loader.files[0].document.named_element(name))
        .transpose()?;
    let mut by_path = BTreeMap::from([(root.clone(), 0)]);
    let mut finished = vec![false];
    let mut heights = vec![1usize];
    let mut stack = vec![Frame {
        file: 0,
        next_node: 0,
        via: None,
    }];
    loader.usage.maximum_depth = 1;
    let mut edges = Vec::new();
    let mut used = vec![false; request.bindings.len()];
    while let Some(frame) = stack.last_mut() {
        let file = frame.file;
        let document = &loader.files[file].document;
        let include = (frame.next_node..document.nodes.len()).find(|id| {
            let node = &document.nodes[*id];
            node.kind == Kind::Element
                && node
                    .name
                    .is_some_and(|span| document.text(span) == "include")
        });
        let Some(node_id) = include else {
            finished[file] = true;
            stack.pop();
            if let Some(parent) = stack.last() {
                heights[parent.file] = heights[parent.file].max(heights[file] + 1);
            }
            continue;
        };
        frame.next_node = node_id + 1;
        let node = &document.nodes[node_id];
        if node.attributes.len() != 1
            || document.text(node.attributes[0].name) != "src"
            || node.children.iter().any(|id| {
                let child = &document.nodes[*id];
                child.kind != Kind::Comment
                    && !(child.kind == Kind::Text
                        && child
                            .value
                            .is_some_and(|span| document.text(span).trim().is_empty()))
            })
        {
            return Err(format!(
                "Unsupported menu include fields/children at file {file}, span {}..{}",
                node.span.start, node.span.end
            )
            .into());
        }
        let span = node.attributes[0].raw_value;
        let index = *bindings
            .get(&(loader.files[file].path.clone(), span.start, span.end))
            .ok_or_else(|| {
                format!(
                    "Unbound menu include src at file {file}, span {}..{}",
                    span.start, span.end
                )
            })?;
        let binding = &request.bindings[index];
        if binding.source_payload_sha256 != loader.files[file].payload_sha256
            || binding.raw_src != document.text(span)
        {
            return Err(format!(
                "Stale menu include source binding at file {file}, span {}..{}",
                span.start, span.end
            )
            .into());
        }
        if edges.len() >= limits.edges {
            return Err("Menu include edge budget exceeded".into());
        }
        charge(
            &mut loader.usage.metadata_bytes,
            size_of::<Edge>(),
            limits.metadata_bytes,
            "metadata",
        )?;
        used[index] = true;
        let target = path(&binding.target.path)?;
        let (target_file, fresh) = if let Some(id) = by_path.get(&target) {
            (*id, false)
        } else {
            if stack.len() >= limits.depth {
                return Err("Menu include depth budget exceeded".into());
            }
            let id = loader.load(&binding.target)?;
            by_path.insert(target, id);
            finished.push(false);
            heights.push(1);
            (id, true)
        };
        let edge_id = edges.len();
        edges.push(Edge {
            source_file: file,
            include_node: node_id,
            src_span: span,
            target_file,
            request_binding: index,
        });
        if !fresh && !finished[target_file] {
            let route = stack
                .iter()
                .filter_map(|frame| frame.via)
                .chain([edge_id])
                .map(|id| {
                    let edge = &edges[id];
                    format!(
                        "{:?}:{}..{} -> {:?}",
                        String::from_utf8_lossy(loader.files[edge.source_file].path.bytes()),
                        edge.src_span.start,
                        edge.src_span.end,
                        String::from_utf8_lossy(loader.files[edge.target_file].path.bytes())
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!("Menu include cycle: {route}").into());
        }
        if !fresh {
            // Reuse saves reads, but must not hide a deeper source dependency
            // path through an already visited subtree.
            let depth = stack
                .len()
                .checked_add(heights[target_file])
                .filter(|value| *value <= limits.depth)
                .ok_or("Menu include depth budget exceeded through reused source")?;
            loader.usage.maximum_depth = loader.usage.maximum_depth.max(depth);
            heights[file] = heights[file].max(heights[target_file] + 1);
        }
        if fresh {
            stack.push(Frame {
                file: target_file,
                next_node: 0,
                via: Some(edge_id),
            });
            loader.usage.maximum_depth = loader.usage.maximum_depth.max(stack.len());
        }
    }
    if used.iter().any(|used| !used) {
        return Err("Menu include request has unused source bindings".into());
    }
    Ok(Report {
        schema_version: 1,
        files: loader.files,
        edges,
        usage: loader.usage,
        selected_root_element,
        interpretation: "Exact explicit include-member dependencies only; immutable source Documents, no relative precedence/template expansion/entity/operator evaluation or original tile display",
        original_display_ready: false,
    })
}

pub fn write_report(writer: impl Write, report: &Report, limit: usize) -> Result<()> {
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
