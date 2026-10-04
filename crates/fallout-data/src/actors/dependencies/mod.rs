//! Source-bound actor model dependencies. No inheritance or part/clip selection.
mod creature_parts;
pub mod equipment;
mod fields;
mod manifest;
pub mod material_overrides;
mod render;
mod templates;
use super::{Catalogue as Actors, associations, fields::Finding};
use crate::{
    Error, Result,
    identity::{self, FormKey},
    inventory, leveled, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
pub use creature_parts::{
    CreaturePartRequest, CreaturePartSelection, CreaturePartsCounts, CreaturePartsLimits,
    CreaturePartsManifest,
};
pub use fields::{ByteString, Context, Field, Link, LinkRole, Marker, PathRole, Value};
pub use manifest::{
    LookupStatus, Manifest, ManifestCounts, ManifestEdge, ManifestLimits, PathRequest,
};
pub use render::{
    ConfigurationOrigin, RenderIssue, RenderLimits, RenderManifest, RenderRequest, RenderRole,
    RenderSource, Sex,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
pub use templates::{
    CategoryRequest, DeclarationSelection, TemplateCategory, TemplateIssue, TemplateLimits,
    TemplateLink, TemplateManifest, TemplateSource,
};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_strings: usize,
    pub max_path_bytes: usize,
    pub max_bindings: usize,
    pub max_graph_nodes: usize,
    pub max_graph_edges: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
            max_strings: 1_000_000,
            max_path_bytes: 32 * 1024 * 1024,
            max_bindings: 1_000_000,
            max_graph_nodes: 131_072,
            max_graph_edges: 2_000_000,
        }
    }
}
enum Body<'a> {
    Borrowed(&'a plugin::Record),
    Owned(plugin::Record),
}
impl Body<'_> {
    fn record(&self) -> &plugin::Record {
        match self {
            Self::Borrowed(record) => record,
            Self::Owned(record) => record,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RaceLink {
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub binding: inventory::Binding,
    pub schema_kind_allowed: Option<bool>,
}

#[derive(Serialize)]
pub struct Definition<'a> {
    pub key: FormKey,
    pub source: inventory::Source,
    pub header: plugin::RecordHeader,
    pub deleted: bool,
    pub fields: Vec<Field>,
    /// Exact RNAM occurrences from the existing actor association decoder.
    pub race_links: Vec<RaceLink>,
    pub findings: Vec<Finding>,
    #[serde(skip)]
    body: Option<Body<'a>>,
}
impl Definition<'_> {
    pub fn record(&self) -> Option<&plugin::Record> {
        self.body.as_ref().map(Body::record)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub deleted_records: usize,
    pub decoded_bytes: usize,
    pub fields: usize,
    pub marker_fields: usize,
    pub path_fields: usize,
    pub strings: usize,
    pub empty_strings: usize,
    pub path_bytes: usize,
    pub link_fields: usize,
    pub bindings: usize,
    pub race_links: usize,
    pub source_findings: usize,
    pub record_kinds: BTreeMap<String, usize>,
    pub record_versions: BTreeMap<String, usize>,
}
pub struct Catalogue<'a> {
    inventory: &'a inventory::Catalogue,
    sources: Vec<SourceReceipt>,
    winning_content_sha256: String,
    definitions: BTreeMap<FormKey, Definition<'a>>,
    graph: leveled::graph::Graph,
    counts: Counts,
}

fn cohort(sources: &[SourceReceipt]) -> Result<BTreeMap<String, (u64, &str)>> {
    let mut result = BTreeMap::new();
    for source in sources {
        if result
            .insert(
                identity::plugin_name(&source.source_name)?,
                (source.source_bytes, source.source_sha256.as_str()),
            )
            .is_some()
        {
            return Err(Error::Resolution(
                "duplicate actor dependency source receipt".into(),
            ));
        }
    }
    Ok(result)
}
fn admitted(kind: &[u8; 4], version: u16) -> bool {
    match kind {
        b"NPC_" => matches!(version, 14 | 15),
        b"CREA" => matches!(version, 9 | 11 | 13 | 14 | 15),
        b"RACE" | b"HDPT" | b"HAIR" => version == 15,
        b"EYES" => matches!(version, 3 | 14 | 15),
        _ => false,
    }
}
impl<'a> Catalogue<'a> {
    pub fn sources(&self) -> &[SourceReceipt] {
        &self.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        &self.winning_content_sha256
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn get(&self, key: &FormKey) -> Option<&Definition<'a>> {
        self.definitions.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Definition<'a>)> {
        self.definitions.iter()
    }
    pub fn inventory_graph(&self) -> &leveled::graph::Graph {
        &self.graph
    }
    pub fn load(
        store: &mut RecordStore,
        actors: &'a Actors<'_>,
        associations: &associations::Catalogue<'_>,
        lists: &leveled::Catalogue,
        limits: Limits,
    ) -> Result<Self> {
        let mut locations = Vec::new();
        for (key, location) in store.winning_definitions() {
            if !matches!(
                &store.definition(location).header.kind,
                b"NPC_" | b"CREA" | b"RACE" | b"HDPT" | b"HAIR" | b"EYES"
            ) {
                continue;
            }
            if locations.len() >= limits.max_records {
                return Err(Error::Unsupported(
                    "actor dependency record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), location));
        }
        let sources = store.source_receipts()?;
        let winning_content_sha256 = record_metadata::inspect(store)?.winning_definitions_sha256;
        for (receipts, digest) in [
            (actors.sources(), actors.winning_content_sha256()),
            (
                associations.sources(),
                associations.winning_content_sha256(),
            ),
            (lists.sources.as_slice(), lists.winning_content_sha256()),
        ] {
            if cohort(receipts)? != cohort(&sources)? || digest != winning_content_sha256 {
                return Err(Error::Resolution(
                    "actor dependency source cohorts or winners differ".into(),
                ));
            }
        }
        let graph = leveled::graph::Graph::build(
            actors.inventory,
            lists,
            leveled::graph::Limits {
                max_nodes: limits.max_graph_nodes,
                max_edges: limits.max_graph_edges,
            },
        )?;
        let mut result = Self {
            inventory: actors.inventory,
            sources,
            winning_content_sha256,
            definitions: BTreeMap::new(),
            graph,
            counts: Counts::default(),
        };
        for (key, location) in locations {
            let header = store.definition(location).header.clone();
            let deleted = header.flags & plugin::DELETED != 0;
            let mut definition = Definition {
                key: key.clone(),
                source: inventory::Source {
                    plugin: store.source_name(location).into(),
                    sha256: store.source_digest(location)?,
                    record_file_offset: header.offset,
                    record_flags: header.flags,
                    decoded_record_sha256: None,
                },
                header,
                deleted,
                fields: Vec::new(),
                race_links: Vec::new(),
                findings: Vec::new(),
                body: None,
            };
            if deleted {
                result.counts.deleted_records += 1;
            } else {
                if !admitted(&definition.header.kind, definition.header.version) {
                    return Err(Error::Unsupported(format!(
                        "{} dependency record version {} at {}:0x{:X}",
                        plugin::signature(definition.header.kind),
                        definition.header.version,
                        definition.source.plugin,
                        definition.header.offset
                    )));
                }
                let maximum = limits.max_record_bytes.min(
                    limits
                        .max_decoded_bytes
                        .saturating_sub(result.counts.decoded_bytes),
                );
                let actor = actors.get(&key);
                let body = if matches!(&definition.header.kind, b"NPC_" | b"CREA") {
                    let input = actor.ok_or_else(|| {
                        Error::Resolution("joined dependency actor missing".into())
                    })?;
                    let record = input.record().ok_or_else(|| {
                        Error::Resolution("joined dependency actor body missing".into())
                    })?;
                    if record.header != definition.header
                        || input.source.sha256 != definition.source.sha256
                        || identity::plugin_name(&input.source.plugin)?
                            != identity::plugin_name(&definition.source.plugin)?
                    {
                        return Err(Error::Resolution(
                            "joined dependency actor provenance differs".into(),
                        ));
                    }
                    if record.header.stored_size as usize > maximum
                        || record.payload.len() > maximum
                    {
                        return Err(Error::Unsupported(
                            "actor dependency borrowed body budget exceeded".into(),
                        ));
                    }
                    Body::Borrowed(record)
                } else {
                    Body::Owned(store.read_bounded(location, maximum)?)
                };
                let record = body.record();
                let document = fields::decode(
                    store,
                    location,
                    record,
                    fields::Limits {
                        max_fields: limits.max_fields.saturating_sub(result.counts.fields),
                        max_strings: limits.max_strings.saturating_sub(result.counts.strings),
                        max_path_bytes: limits
                            .max_path_bytes
                            .saturating_sub(result.counts.path_bytes),
                        max_bindings: limits.max_bindings.saturating_sub(result.counts.bindings),
                    },
                )?;
                result.counts.decoded_bytes += record.payload.len();
                result.counts.fields += document.counts.fields;
                result.counts.strings += document.counts.strings;
                result.counts.path_bytes += document.counts.path_bytes;
                result.counts.bindings += document.counts.bindings;
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                definition.fields = document.fields;
                definition.findings = document.findings;
                if let Some(actor) = actor {
                    let input = associations.get(&key).ok_or_else(|| {
                        Error::Resolution("joined dependency associations missing".into())
                    })?;
                    for association in &input.associations {
                        if association.role != associations::Role::Race {
                            continue;
                        }
                        if result.counts.bindings >= limits.max_bindings {
                            return Err(Error::Unsupported(
                                "actor dependency binding budget exceeded".into(),
                            ));
                        }
                        let field = actor.fields.get(association.field_index).ok_or_else(|| {
                            Error::Resolution("joined race field index missing".into())
                        })?;
                        if field.kind != *b"RNAM" {
                            return Err(Error::Resolution("joined race field kind differs".into()));
                        }
                        definition.race_links.push(RaceLink {
                            field_index: association.field_index,
                            field_decoded_offset: field.decoded_offset,
                            binding: association.binding.clone(),
                            schema_kind_allowed: association.schema_kind_allowed,
                        });
                        result.counts.bindings += 1;
                        result.counts.race_links += 1;
                    }
                }
                definition.body = Some(body);
                *result
                    .counts
                    .record_versions
                    .entry(format!(
                        "{}:{}",
                        plugin::signature(definition.header.kind),
                        definition.header.version
                    ))
                    .or_default() += 1;
            }
            for field in &definition.fields {
                match &field.value {
                    Value::Marker { .. } => result.counts.marker_fields += 1,
                    Value::Paths { strings, .. } => {
                        result.counts.path_fields += 1;
                        result.counts.empty_strings +=
                            strings.iter().filter(|s| s.raw.is_empty()).count();
                    }
                    Value::Links { .. } => result.counts.link_fields += 1,
                    Value::Opaque | Value::BipedSlots { .. } | Value::EquipmentType { .. } => {}
                }
            }
            result.counts.records += 1;
            result.counts.source_findings += definition.findings.len();
            *result
                .counts
                .record_kinds
                .entry(plugin::signature(definition.header.kind))
                .or_default() += 1;
            result.definitions.insert(key, definition);
        }
        Ok(result)
    }
}
