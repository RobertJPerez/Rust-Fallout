//! Retained source menu structure for the original tile consumer. This uses the
//! existing archive importer and audited tokenizer. Bounded source consumers
//! live in the owned submodules.
use crate::model::Result;
use fallout_data::{
    assets::ArchiveAssets,
    vfs::{AssetPath, AssetSource},
};
use quick_xml::{events::Event, reader::Reader};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    mem::size_of,
    path::Path,
};

#[derive(Clone, Copy)]
pub struct Limits {
    pub source_bytes: usize,
    pub events: usize,
    pub nodes: usize,
    pub depth: usize,
    pub attributes_per_element: usize,
    pub metadata_bytes: usize,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 1024 * 1024,
            events: 32768,
            nodes: 16384,
            depth: 128,
            attributes_per_element: 64,
            metadata_bytes: 4 * 1024 * 1024,
            output_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Half-open offsets into the exact UTF-8 source, including its optional BOM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}
#[derive(Debug, Serialize)]
pub struct Attribute {
    pub name: Span,
    pub raw_value: Span,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Element,
    Text,
    EntityReference,
    Cdata,
    Comment,
    Declaration,
    ProcessingInstruction,
    Doctype,
}
#[derive(Debug, Serialize)]
pub struct Node {
    pub kind: Kind,
    /// Whole element span after its closing event, or this leaf's event span.
    pub span: Span,
    pub open: Span,
    pub close: Option<Span>,
    pub name: Option<Span>,
    pub value: Option<Span>,
    pub attributes: Vec<Attribute>,
    pub parent: Option<usize>,
    /// All child nodes in source order, including whitespace and references.
    pub children: Vec<usize>,
    pub empty_element: bool,
}
#[derive(Debug, Serialize)]
pub struct Document {
    pub source_utf8: String,
    pub utf8_bom_bytes: usize,
    pub roots: Vec<usize>,
    pub nodes: Vec<Node>,
    pub event_count: usize,
    /// Declared logical retained metadata, not allocator peak memory.
    pub charged_metadata_bytes: usize,
}

impl Document {
    pub fn text(&self, span: Span) -> &str {
        &self.source_utf8[span.start..span.end]
    }
    pub fn named_element(&self, name: &str) -> Result<usize> {
        let mut matches = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.kind == Kind::Element
                    && node.attributes.iter().any(|attribute| {
                        self.text(attribute.name) == "name"
                            && self.text(attribute.raw_value) == name
                    })
            })
            .map(|(id, _)| id);
        let first = matches
            .next()
            .ok_or("Requested authored menu name is unavailable")?;
        if matches.next().is_some() {
            return Err("Requested authored menu name is ambiguous".into());
        }
        Ok(first)
    }
}

#[derive(Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub source: AssetSource,
    pub archive_sha256: String,
    pub payload_sha256: String,
    pub document: Document,
    pub selected_element: Option<usize>,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}

pub fn inspect(
    install: &Path,
    path: &AssetPath,
    selected: Option<&str>,
    limits: Limits,
) -> Result<Report> {
    let mut assets = ArchiveAssets::open_nv(install)?;
    let (source, bytes) = assets.read_unique_bounded(path, limits.source_bytes as u64)?;
    let archive_sha256 = assets.source_digest(&source)?.to_owned();
    let payload_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let document = parse(bytes, limits)?;
    let selected_element = selected
        .map(|name| document.named_element(name))
        .transpose()?;
    Ok(Report {
        schema_version: 1,
        source,
        archive_sha256,
        payload_sha256,
        document,
        selected_element,
        interpretation: "Source observations only: includes/templates/custom entities/traits/operators/fonts/navigation/action dispatch unevaluated; no default layout or original tile display",
        original_display_ready: false,
    })
}

fn charge(total: &mut usize, bytes: usize, limit: usize) -> Result<()> {
    let next = total
        .checked_add(bytes)
        .ok_or("Menu metadata charge overflow")?;
    if next > limit {
        return Err("Menu retained metadata budget exceeded".into());
    }
    *total = next;
    Ok(())
}

fn borrowed_span(source: &str, text: &str) -> Result<Span> {
    let start = (text.as_ptr() as usize)
        .checked_sub(source.as_ptr() as usize)
        .ok_or("Tokenizer text is outside admitted source")?;
    let end = start.checked_add(text.len()).ok_or("Menu span overflow")?;
    if source.get(start..end) != Some(text) {
        return Err("Tokenizer text is not a retained raw source span".into());
    }
    Ok(Span { start, end })
}

pub fn parse(bytes: Vec<u8>, limits: Limits) -> Result<Document> {
    if bytes.len() > limits.source_bytes {
        return Err("Menu source byte budget exceeded".into());
    }
    let source = String::from_utf8(bytes)?;
    let bom = usize::from(source.starts_with('\u{feff}')) * 3;
    let mut reader = Reader::from_reader(&source.as_bytes()[bom..]);
    // The original menu uses divider comments with internal `--`. Retain these
    // as opaque bytes; XML end-name and attribute checks remain enabled.
    reader.config_mut().check_end_names = true;
    reader.config_mut().check_comments = false;
    reader.config_mut().trim_text(false);
    let mut nodes: Vec<Node> = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut events = 0usize;
    let mut metadata = 0usize;
    loop {
        // Bound event work before asking the tokenizer for another event.
        if events >= limits.events {
            return Err("Menu event budget exceeded".into());
        }
        let start = usize::try_from(reader.buffer_position())? + bom;
        let event = reader.read_event()?;
        events += 1;
        let end = usize::try_from(reader.buffer_position())? + bom;
        let span = Span { start, end };
        if source.get(start..end).is_none() {
            return Err("Menu event span invalid".into());
        }
        if matches!(event, Event::Eof) {
            break;
        }
        if let Event::End(_) = event {
            let id = stack.pop().ok_or("Menu unmatched closing element")?;
            nodes[id].span.end = end;
            nodes[id].close = Some(span);
            continue;
        }
        if nodes.len() >= limits.nodes {
            return Err("Menu node budget exceeded".into());
        }
        charge(
            &mut metadata,
            size_of::<Node>() + size_of::<usize>(),
            limits.metadata_bytes,
        )?;
        let parent = stack.last().copied();
        let empty = matches!(event, Event::Empty(_));
        let mut node = Node {
            kind: Kind::Text,
            span,
            open: span,
            close: None,
            name: None,
            value: None,
            attributes: Vec::new(),
            parent,
            children: Vec::new(),
            empty_element: empty,
        };
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                if stack.len() >= limits.depth {
                    return Err("Menu depth budget exceeded".into());
                }
                node.kind = Kind::Element;
                node.name = Some(borrowed_span(&source, element.name().as_ref())?);
                for attribute in element.attributes() {
                    if node.attributes.len() >= limits.attributes_per_element {
                        return Err("Menu attribute budget exceeded".into());
                    }
                    charge(&mut metadata, size_of::<Attribute>(), limits.metadata_bytes)?;
                    let attribute = attribute?;
                    node.attributes.push(Attribute {
                        name: borrowed_span(&source, attribute.key.as_ref())?,
                        raw_value: borrowed_span(&source, attribute.value.as_ref())?,
                    });
                }
                if !empty {
                    charge(&mut metadata, size_of::<usize>(), limits.metadata_bytes)?;
                }
            }
            Event::Text(ref text) => {
                node.value = Some(borrowed_span(&source, text.as_ref())?);
            }
            Event::GeneralRef(ref value) => {
                node.kind = Kind::EntityReference;
                node.value = Some(borrowed_span(&source, value.as_ref())?);
            }
            Event::CData(ref value) => {
                node.kind = Kind::Cdata;
                node.value = Some(borrowed_span(&source, value.as_ref())?);
            }
            Event::Comment(_) => {
                node.kind = Kind::Comment;
            }
            Event::PI(_) => {
                node.kind = Kind::ProcessingInstruction;
            }
            Event::DocType(_) => {
                node.kind = Kind::Doctype;
            }
            Event::Decl(ref declaration) => {
                node.kind = Kind::Declaration;
                if let Some(encoding) = declaration.encoding() {
                    let encoding = encoding?;
                    if !encoding.eq_ignore_ascii_case("utf-8")
                        && !(encoding.eq_ignore_ascii_case("us-ascii") && source.is_ascii())
                    {
                        return Err(
                            format!("Menu declared encoding {encoding} is unsupported").into()
                        );
                    }
                }
            }
            Event::End(_) | Event::Eof => unreachable!("handled before node admission"),
        }
        let id = nodes.len();
        if node.kind == Kind::Element && !empty {
            stack.push(id);
        }
        nodes.push(node);
        if let Some(parent) = parent {
            nodes[parent].children.push(id);
        } else {
            roots.push(id);
        }
    }
    if !stack.is_empty() {
        return Err("Menu source has unclosed elements".into());
    }
    if !nodes.iter().any(|node| node.kind == Kind::Element) {
        return Err("Menu source has no elements".into());
    }
    Ok(Document {
        source_utf8: source,
        utf8_bom_bytes: bom,
        roots,
        nodes,
        event_count: events,
        charged_metadata_bytes: metadata,
    })
}

struct BoundedOutput<W> {
    inner: W,
    written: usize,
    limit: usize,
}
impl<W: Write> Write for BoundedOutput<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.written) {
            return Err(io::Error::other("Menu report output budget exceeded"));
        }
        let written = self.inner.write(bytes)?;
        self.written += written;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
fn write_json(writer: impl Write, report: &impl Serialize, limit: usize) -> Result<()> {
    let mut writer = BoundedOutput {
        inner: writer,
        written: 0,
        limit,
    };
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn write_report(writer: impl Write, report: &Report, limit: usize) -> Result<()> {
    write_json(writer, report, limit)
}

fn read_json<T: DeserializeOwned>(path: &Path, limit: usize, kind: &str) -> Result<T> {
    let file = fallout_data::baseline::open_source(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(format!("Menu {kind} request byte budget exceeded").into());
    }
    let maximum = u64::try_from(limit)?
        .checked_add(1)
        .ok_or_else(|| format!("Menu {kind} request byte limit overflow"))?;
    let mut bytes = Vec::new();
    file.take(maximum).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(format!("Menu {kind} request byte budget exceeded").into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub mod dependencies;
pub mod entities;
pub mod fonts;
pub mod images;
pub mod includes;
pub mod rectangles;
pub mod traits;

#[cfg(test)]
mod tests;
