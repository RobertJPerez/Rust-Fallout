//! Exact selected tile fields, with caller-chosen literal conversion policies.
use super::{Document, Kind, Span, entities, includes};
use crate::model::Result;
use fallout_data::vfs::AssetPath;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, mem::size_of, path::Path};

#[derive(Clone, Copy)]
pub struct Limits {
    pub document: super::Limits,
    pub request_bytes: usize,
    pub conversions: usize,
    pub rows: usize,
    pub copy_bytes: usize,
    pub metadata_bytes: usize,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            document: super::Limits::default(),
            request_bytes: 128 * 1024,
            conversions: 128,
            rows: 256,
            copy_bytes: 2 * 1024 * 1024,
            metadata_bytes: 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConversionKind {
    String,
    FiniteF32,
    #[serde(rename = "boolean-01")]
    Boolean01,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Conversion {
    pub name: String,
    pub kind: ConversionKind,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    pub name: String,
    pub node: usize,
    pub span: Span,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub source: includes::Source,
    pub tile: Tile,
    pub conversions: Vec<Conversion>,
}
pub fn read_request(path: &Path, limits: Limits) -> Result<Request> {
    super::read_json(path, limits.request_bytes, "literal-trait")
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Literal {
    String {
        value: String,
    },
    FiniteF32 {
        value: f32,
        bits: u32,
    },
    #[serde(rename = "boolean-01")]
    Boolean01 {
        value: bool,
    },
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    DuplicateDeclaration,
    AttributedDeclaration,
    NestedSource,
    ConversionUnsupplied,
    CustomEntity,
    MalformedNumber,
    Overflow,
    NonFiniteNumber,
    Underflow,
    BooleanNotZeroOrOne,
}
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Absent,
    Structural,
    Empty,
    Value {
        value: Literal,
    },
    Unresolved {
        reason: Reason,
        references: Vec<entities::Unresolved>,
    },
}
#[derive(Serialize)]
pub struct Row {
    pub name: String,
    pub node: Option<usize>,
    pub name_span: Option<Span>,
    pub span: Option<Span>,
    pub inner_span: Option<Span>,
    pub source_spelling: Option<String>,
    pub conversion: Option<ConversionKind>,
    pub outcome: Outcome,
}
#[derive(Default, Serialize)]
pub struct Usage {
    pub rows: usize,
    pub reserved_copy_bytes: usize,
    pub metadata_bytes: usize,
}
#[derive(Serialize)]
pub struct Projection {
    pub tile_node: usize,
    /// Complete source order, including whitespace, comments and other leaves.
    pub direct_children: Vec<usize>,
    pub rows: Vec<Row>,
    pub usage: Usage,
}
#[derive(Serialize)]
pub struct Report<'a> {
    pub schema_version: u32,
    pub source: super::Report,
    pub request: &'a Request,
    pub projection: Projection,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}
fn charge(total: &mut usize, amount: usize, maximum: usize, what: &str) -> Result<()> {
    *total = total
        .checked_add(amount)
        .filter(|next| *next <= maximum)
        .ok_or_else(|| format!("Menu literal-trait {what} budget exceeded"))?;
    Ok(())
}
fn structural(name: &str) -> bool {
    matches!(
        name,
        "menu"
            | "rect"
            | "image"
            | "text"
            | "nif"
            | "3d"
            | "hotrect"
            | "window"
            | "radial"
            | "template"
            | "include"
    )
}
fn unresolved(reason: Reason) -> Outcome {
    Outcome::Unresolved {
        reason,
        references: Vec::new(),
    }
}
fn convert(value: String, kind: ConversionKind) -> Outcome {
    if kind == ConversionKind::String {
        return Outcome::Value {
            value: Literal::String { value },
        };
    }
    // Numeric/boolean conversion strips XML ASCII whitespace only. Strings do not.
    let value = value.trim_matches([' ', '\t', '\r', '\n']);
    if value.is_empty() {
        return Outcome::Empty;
    }
    if kind == ConversionKind::Boolean01 {
        return match value {
            "0" => Outcome::Value {
                value: Literal::Boolean01 { value: false },
            },
            "1" => Outcome::Value {
                value: Literal::Boolean01 { value: true },
            },
            _ => unresolved(Reason::BooleanNotZeroOrOne),
        };
    }
    let Ok(number) = value.parse::<f32>() else {
        return unresolved(Reason::MalformedNumber);
    };
    if !number.is_finite() {
        return unresolved(
            if value
                .trim_start_matches(['+', '-'])
                .eq_ignore_ascii_case("nan")
                || value
                    .trim_start_matches(['+', '-'])
                    .eq_ignore_ascii_case("inf")
                || value
                    .trim_start_matches(['+', '-'])
                    .eq_ignore_ascii_case("infinity")
            {
                Reason::NonFiniteNumber
            } else {
                Reason::Overflow
            },
        );
    }
    let mantissa = value.split(['e', 'E']).next().unwrap_or(value);
    if number == 0.0 && mantissa.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
        return unresolved(Reason::Underflow);
    }
    Outcome::Value {
        value: Literal::FiniteF32 {
            value: number,
            bits: number.to_bits(),
        },
    }
}
struct Plan<'a> {
    name: &'a str,
    node: Option<usize>,
    inner: Option<Span>,
    kind: Option<ConversionKind>,
}

pub fn project(
    document: &Document,
    expected_payload: &str,
    request: &Request,
    limits: Limits,
) -> Result<Projection> {
    if request.schema_version != 1 || request.source.payload_sha256 != expected_payload {
        return Err("Menu literal-trait request schema/payload differs".into());
    }
    includes::path(&request.source.path)?;
    includes::hash(&request.source.archive_sha256)?;
    includes::hash(expected_payload)?;
    if request.tile.name.is_empty() || request.tile.name.len() > 256 {
        return Err("Menu literal-trait tile name requires 1..256 bytes".into());
    }
    if document.named_element(&request.tile.name)? != request.tile.node {
        return Err("Menu literal-trait selected name/node differs".into());
    }
    project_exact(
        document,
        expected_payload,
        request.tile.node,
        request.tile.span,
        &request.conversions,
        limits,
    )
}

/// Exact source identity for an already selected subtree's children. The named
/// request entry point still requires a globally unique selected name.
pub(super) fn project_exact(
    document: &Document,
    expected_payload: &str,
    tile_node: usize,
    tile_span: Span,
    requested_conversions: &[Conversion],
    limits: Limits,
) -> Result<Projection> {
    includes::hash(expected_payload)?;
    if document.source_utf8.len() > limits.document.source_bytes {
        return Err("Menu literal-trait source byte budget exceeded".into());
    }
    if format!("{:x}", Sha256::digest(document.source_utf8.as_bytes())) != expected_payload {
        return Err("Menu literal-trait payload SHA differs from selected source".into());
    }
    let tile = document
        .nodes
        .get(tile_node)
        .filter(|node| node.kind == Kind::Element && node.span == tile_span)
        .ok_or("Menu literal-trait selected tile span differs")?;
    let tile_kind = document.text(tile.name.ok_or("Menu tile tag name missing")?);
    if !structural(tile_kind) || matches!(tile_kind, "include" | "template") {
        return Err("Menu literal-trait selection is not a supported tile tag".into());
    }
    if requested_conversions.len() > limits.conversions {
        return Err("Menu literal-trait conversion budget exceeded".into());
    }
    let mut usage = Usage::default();
    charge(
        &mut usage.metadata_bytes,
        tile.children.len() * size_of::<usize>(),
        limits.metadata_bytes,
        "metadata",
    )?;
    let mut conversions = BTreeMap::new();
    for conversion in requested_conversions {
        if conversion.name.is_empty()
            || conversion.name.len() > 128
            || conversion.name.chars().any(char::is_whitespace)
            || structural(&conversion.name)
        {
            return Err("Menu literal-trait conversion name invalid or structural".into());
        }
        charge(
            &mut usage.metadata_bytes,
            size_of::<(&str, ConversionKind)>(),
            limits.metadata_bytes,
            "metadata",
        )?;
        if conversions
            .insert(conversion.name.as_str(), conversion.kind)
            .is_some()
        {
            return Err("Menu literal-trait conversion duplicated".into());
        }
    }
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut plans = Vec::new();
    for child in &tile.children {
        let node = document
            .nodes
            .get(*child)
            .ok_or("Menu tile child node missing")?;
        if node.kind != Kind::Element {
            continue;
        }
        let name = document.text(node.name.ok_or("Menu source field name missing")?);
        let inner = Span {
            start: node.open.end,
            end: node.close.map_or(node.open.end, |span| span.start),
        };
        let spelling = document.text(inner);
        charge(&mut usage.rows, 1, limits.rows, "row")?;
        charge(
            &mut usage.metadata_bytes,
            size_of::<Plan>() + size_of::<Row>(),
            limits.metadata_bytes,
            "metadata",
        )?;
        // Missing references retain owned span/name rows inside the outcome.
        // Reserve every direct reference conservatively, including builtins.
        let references = node
            .children
            .iter()
            .filter(|id| document.nodes[**id].kind == Kind::EntityReference)
            .count();
        charge(
            &mut usage.metadata_bytes,
            references * size_of::<entities::Unresolved>(),
            limits.metadata_bytes,
            "metadata",
        )?;
        let copies = spelling
            .len()
            .checked_mul(2)
            .and_then(|n| n.checked_add(name.len()))
            .ok_or("Menu literal-trait copy reservation overflow")?;
        charge(
            &mut usage.reserved_copy_bytes,
            copies,
            limits.copy_bytes,
            "reserved copy byte",
        )?;
        if !counts.contains_key(name) {
            charge(
                &mut usage.metadata_bytes,
                size_of::<(&str, usize)>(),
                limits.metadata_bytes,
                "metadata",
            )?;
        }
        *counts.entry(name).or_default() += 1;
        plans.push(Plan {
            name,
            node: Some(*child),
            inner: Some(inner),
            kind: conversions.get(name).copied(),
        });
    }
    for conversion in requested_conversions {
        if counts.contains_key(conversion.name.as_str()) {
            continue;
        }
        charge(&mut usage.rows, 1, limits.rows, "row")?;
        charge(
            &mut usage.metadata_bytes,
            size_of::<Plan>() + size_of::<Row>(),
            limits.metadata_bytes,
            "metadata",
        )?;
        charge(
            &mut usage.reserved_copy_bytes,
            conversion.name.len(),
            limits.copy_bytes,
            "reserved copy byte",
        )?;
        plans.push(Plan {
            name: &conversion.name,
            node: None,
            inner: None,
            kind: Some(conversion.kind),
        });
    }
    // Complete row/metadata/copy admission precedes projection/name/value copies.
    let mut rows = Vec::with_capacity(plans.len());
    for plan in plans {
        let outcome = if let Some(id) = plan.node {
            let node = &document.nodes[id];
            if structural(plan.name) {
                Outcome::Structural
            } else if counts[plan.name] != 1 {
                unresolved(Reason::DuplicateDeclaration)
            } else if !node.attributes.is_empty() {
                unresolved(Reason::AttributedDeclaration)
            } else if node.children.iter().any(|id| {
                !matches!(
                    document.nodes[*id].kind,
                    Kind::Text | Kind::Cdata | Kind::EntityReference | Kind::Comment
                )
            }) {
                unresolved(Reason::NestedSource)
            } else if let Some(kind) = plan.kind {
                let resolution = entities::resolve(
                    document,
                    expected_payload,
                    &entities::Selection::ElementText {
                        node: id,
                        span: node.span,
                    },
                    &[],
                    entities::Limits {
                        document: limits.document,
                        ..entities::Limits::default()
                    },
                )?;
                match resolution.value {
                    Some(value) => convert(value, kind),
                    None => Outcome::Unresolved {
                        reason: Reason::CustomEntity,
                        references: resolution.unresolved,
                    },
                }
            } else {
                unresolved(Reason::ConversionUnsupplied)
            }
        } else {
            Outcome::Absent
        };
        rows.push(Row {
            name: plan.name.to_owned(),
            node: plan.node,
            name_span: plan.node.and_then(|id| document.nodes[id].name),
            span: plan.node.map(|id| document.nodes[id].span),
            inner_span: plan.inner,
            source_spelling: plan.inner.map(|span| document.text(span).to_owned()),
            conversion: plan.kind,
            outcome,
        });
    }
    Ok(Projection {
        tile_node,
        direct_children: tile.children.clone(),
        rows,
        usage,
    })
}
pub fn inspect<'a>(
    install: &Path,
    path: &AssetPath,
    request: &'a Request,
    limits: Limits,
) -> Result<Report<'a>> {
    if request.schema_version != 1 || includes::path(&request.source.path)? != *path {
        return Err("Menu literal-trait request schema/member differs".into());
    }
    includes::hash(&request.source.archive_sha256)?;
    let source = super::inspect(install, path, None, limits.document)?;
    if source.archive_sha256 != request.source.archive_sha256
        || source.payload_sha256 != request.source.payload_sha256
    {
        return Err("Menu literal-trait source archive/payload SHA differs".into());
    }
    let projection = project(
        &source.document,
        &request.source.payload_sha256,
        request,
        limits,
    )?;
    Ok(Report {
        schema_version: 1,
        source,
        request,
        projection,
        interpretation: "Exact direct source fields with caller-chosen literal policies; absent/empty/unresolved retained, no operators/template expansion/default layout/font/action or original display",
        original_display_ready: false,
    })
}
pub fn write_report(writer: impl Write, report: &Report<'_>, limit: usize) -> Result<()> {
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
