//! Source-bound selected text, with explicit custom values and preflighted copies.
//! This is entity substitution, without tile/operator or localization semantics.
use super::{Document, Kind, Span, includes};
use crate::model::Result;
use fallout_data::vfs::AssetPath;
use quick_xml::{escape::resolve_xml_entity, events::BytesRef};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, mem::size_of, path::Path};

#[derive(Clone, Copy)]
pub struct Limits {
    pub document: super::Limits,
    pub request_bytes: usize,
    pub definitions: usize,
    pub environment_bytes: usize,
    pub pieces: usize,
    pub references: usize,
    pub input_bytes: usize,
    pub expanded_bytes: usize,
    pub metadata_bytes: usize,
    pub output_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            document: super::Limits::default(),
            request_bytes: 128 * 1024,
            definitions: 128,
            environment_bytes: 64 * 1024,
            pieces: 32768,
            references: 16384,
            input_bytes: 1024 * 1024,
            expanded_bytes: 1024 * 1024,
            metadata_bytes: 2 * 1024 * 1024,
            output_bytes: 12 * 1024 * 1024,
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Selection {
    ElementText {
        node: usize,
        span: Span,
    },
    Attribute {
        node: usize,
        name_span: Span,
        value_span: Span,
    },
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub name: String,
    pub value: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub source: includes::Source,
    pub selection: Selection,
    pub definitions: Vec<Definition>,
}
pub fn read_request(path: &Path, limits: Limits) -> Result<Request> {
    super::read_json(path, limits.request_bytes, "entity")
}
#[derive(Serialize)]
pub struct Unresolved {
    pub span: Span,
    pub name: String,
}
#[derive(Default, Serialize)]
pub struct Usage {
    pub environment_bytes: usize,
    pub input_bytes: usize,
    pub pieces: usize,
    pub references: usize,
    pub expanded_bytes: usize,
    pub copied_bytes: usize,
    pub metadata_bytes: usize,
}
#[derive(Serialize)]
pub struct Resolution {
    pub value: Option<String>,
    pub unresolved: Vec<Unresolved>,
    pub usage: Usage,
}
#[derive(Serialize)]
pub struct Report<'a> {
    pub schema_version: u32,
    pub source: super::Report,
    pub request: &'a Request,
    pub resolution: Resolution,
    pub interpretation: &'static str,
    pub original_display_ready: bool,
}

fn charge(total: &mut usize, amount: usize, maximum: usize, what: &str) -> Result<()> {
    let next = total
        .checked_add(amount)
        .filter(|next| *next <= maximum)
        .ok_or_else(|| format!("Menu entity {what} budget exceeded"))?;
    *total = next;
    Ok(())
}
enum Piece<'a> {
    Text(&'a str),
    Character(char),
    Missing { span: Span, name: &'a str },
}
struct Planner<'a> {
    document: &'a Document,
    environment: BTreeMap<&'a str, &'a str>,
    limits: Limits,
    usage: Usage,
    pieces: Vec<Piece<'a>>,
}
impl<'a> Planner<'a> {
    fn text(&self, span: Span) -> Result<&'a str> {
        self.document
            .source_utf8
            .get(span.start..span.end)
            .ok_or_else(|| "Menu selected source span is invalid".into())
    }
    fn piece(&mut self, piece: Piece<'a>) -> Result<()> {
        charge(&mut self.usage.pieces, 1, self.limits.pieces, "piece")?;
        charge(
            &mut self.usage.metadata_bytes,
            size_of::<Piece>(),
            self.limits.metadata_bytes,
            "metadata",
        )?;
        let bytes = match piece {
            Piece::Text(value) => value.len(),
            Piece::Character(value) => value.len_utf8(),
            Piece::Missing { name, .. } => {
                charge(
                    &mut self.usage.metadata_bytes,
                    size_of::<Unresolved>() + name.len(),
                    self.limits.metadata_bytes,
                    "metadata",
                )?;
                0
            }
        };
        charge(
            &mut self.usage.expanded_bytes,
            bytes,
            self.limits.expanded_bytes,
            "expanded byte",
        )?;
        self.pieces.push(piece);
        Ok(())
    }
    fn reference(&mut self, span: Span, name: &'a str) -> Result<()> {
        charge(
            &mut self.usage.references,
            1,
            self.limits.references,
            "reference work",
        )?;
        if name.is_empty() || name.len() > 256 {
            return Err("Menu entity reference name requires 1..256 source bytes".into());
        }
        if let Some(character) = BytesRef::new(name).resolve_char_ref()? {
            self.piece(Piece::Character(character))
        } else if let Some(value) = resolve_xml_entity(name) {
            self.piece(Piece::Text(value))
        } else if let Some(value) = self.environment.get(name) {
            self.piece(Piece::Text(value))
        } else {
            self.piece(Piece::Missing { span, name })
        }
    }
    fn escaped(&mut self, span: Span) -> Result<()> {
        let raw = self.text(span)?;
        let mut cursor = 0;
        while let Some(start) = raw[cursor..].find('&').map(|offset| cursor + offset) {
            if start > cursor {
                self.piece(Piece::Text(&raw[cursor..start]))?;
            }
            let Some(end) = raw[start + 1..].find(';').map(|offset| start + 1 + offset) else {
                return Err("Menu selected value has an unterminated entity reference".into());
            };
            let name = &raw[start + 1..end];
            if name.contains('&') {
                return Err("Menu selected value has an unterminated entity reference".into());
            }
            self.reference(
                Span {
                    start: span.start + start,
                    end: span.start + end + 1,
                },
                name,
            )?;
            cursor = end + 1;
        }
        if cursor < raw.len() {
            self.piece(Piece::Text(&raw[cursor..]))?;
        }
        Ok(())
    }
    fn input(&mut self, span: Span) -> Result<()> {
        let bytes = self.text(span)?.len();
        charge(
            &mut self.usage.input_bytes,
            bytes,
            self.limits.input_bytes,
            "selected input byte",
        )
    }
}

pub fn resolve<'a>(
    document: &'a Document,
    expected_payload: &str,
    selection: &Selection,
    definitions: &'a [Definition],
    limits: Limits,
) -> Result<Resolution> {
    if document.source_utf8.len() > limits.document.source_bytes {
        return Err("Menu entity source document byte budget exceeded".into());
    }
    includes::hash(expected_payload)?;
    if format!("{:x}", Sha256::digest(document.source_utf8.as_bytes())) != expected_payload {
        return Err("Menu entity environment payload SHA differs from selected source".into());
    }
    if definitions.len() > limits.definitions {
        return Err("Menu entity definition budget exceeded".into());
    }
    let mut planner = Planner {
        document,
        environment: BTreeMap::new(),
        limits,
        usage: Usage::default(),
        pieces: Vec::new(),
    };
    for definition in definitions {
        let name = definition.name.as_str();
        if name.is_empty()
            || name.len() > 128
            || name.starts_with('#')
            || resolve_xml_entity(name).is_some()
            || name
                .chars()
                .any(|c| c.is_whitespace() || "&;<>'\"".contains(c))
        {
            return Err("Menu custom entity name is invalid or reserved".into());
        }
        // Supplied values are literal strings, not another XML/entity source.
        // A literal ampersand is allowed; entity-shaped nested input is refused.
        charge(
            &mut planner.usage.environment_bytes,
            name.len() + definition.value.len(),
            limits.environment_bytes,
            "environment byte",
        )?;
        if definition.value.contains('\0')
            || definition
                .value
                .find('&')
                .zip(definition.value.rfind(';'))
                .is_some_and(|(amp, semi)| amp < semi)
        {
            return Err(
                "Menu custom entity value contains NUL or nested entity-shaped input".into(),
            );
        }
        charge(
            &mut planner.usage.metadata_bytes,
            size_of::<(&str, &str)>(),
            limits.metadata_bytes,
            "metadata",
        )?;
        if planner
            .environment
            .insert(name, &definition.value)
            .is_some()
        {
            return Err("Menu custom entity definition duplicated".into());
        }
    }
    match selection {
        Selection::ElementText { node, span } => {
            let node = document
                .nodes
                .get(*node)
                .filter(|node| node.kind == Kind::Element && node.span == *span)
                .ok_or("Menu selected element identity/span differs")?;
            for child in &node.children {
                let child = &document.nodes[*child];
                planner.input(child.span)?;
                match child.kind {
                    Kind::Text => {
                        planner.escaped(child.value.ok_or("Menu text source span missing")?)?
                    }
                    Kind::Cdata => planner.piece(Piece::Text(
                        planner.text(child.value.ok_or("Menu CDATA source span missing")?)?,
                    ))?,
                    Kind::EntityReference => planner.reference(
                        child.span,
                        planner.text(child.value.ok_or("Menu reference source span missing")?)?,
                    )?,
                    Kind::Comment => {}
                    _ => {
                        return Err(
                            "Menu selected element contains unevaluated child markup/operators"
                                .into(),
                        );
                    }
                }
            }
        }
        Selection::Attribute {
            node,
            name_span,
            value_span,
        } => {
            let node = document
                .nodes
                .get(*node)
                .filter(|node| node.kind == Kind::Element)
                .ok_or("Menu selected attribute owner is not an element")?;
            if !node
                .attributes
                .iter()
                .any(|attribute| attribute.name == *name_span && attribute.raw_value == *value_span)
            {
                return Err("Menu selected attribute identity/spans differ".into());
            }
            planner.input(*value_span)?;
            planner.escaped(*value_span)?;
        }
    }
    let available = planner
        .pieces
        .iter()
        .all(|piece| !matches!(piece, Piece::Missing { .. }));
    // Complete preflight precedes every selected output/name copy. Missing values
    // cannot publish a partially expanded string or silently substitute empty.
    let mut value = available.then(|| String::with_capacity(planner.usage.expanded_bytes));
    let mut unresolved = Vec::new();
    for piece in planner.pieces {
        match piece {
            Piece::Text(text) => {
                if let Some(value) = &mut value {
                    value.push_str(text)
                }
            }
            Piece::Character(character) => {
                if let Some(value) = &mut value {
                    value.push(character)
                }
            }
            Piece::Missing { span, name } => unresolved.push(Unresolved {
                span,
                name: name.to_owned(),
            }),
        }
    }
    planner.usage.copied_bytes = value.as_ref().map_or(0, String::len);
    Ok(Resolution {
        value,
        unresolved,
        usage: planner.usage,
    })
}

pub fn inspect<'a>(
    install: &Path,
    path: &AssetPath,
    request: &'a Request,
    limits: Limits,
) -> Result<Report<'a>> {
    if request.schema_version != 1 || includes::path(&request.source.path)? != *path {
        return Err("Menu entity request schema/member differs".into());
    }
    includes::hash(&request.source.archive_sha256)?;
    let source = super::inspect(install, path, None, limits.document)?;
    if source.archive_sha256 != request.source.archive_sha256
        || source.payload_sha256 != request.source.payload_sha256
    {
        return Err("Menu entity request source archive/payload SHA differs".into());
    }
    let resolution = resolve(
        &source.document,
        &request.source.payload_sha256,
        &request.selection,
        &request.definitions,
        limits,
    )?;
    Ok(Report {
        schema_version: 1,
        source,
        request,
        resolution,
        interpretation: "Only exact selected source text/attribute entity substitution with literal caller values; missing values unavailable, no DTD/template/operator/layout/localization/action or original tile display",
        original_display_ready: false,
    })
}
pub fn write_report(writer: impl Write, report: &Report<'_>, limit: usize) -> Result<()> {
    super::write_json(writer, report, limit)
}

#[cfg(test)]
mod tests;
