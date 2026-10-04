//! Immutable authored actor inputs. Scalars borrow their exact inventory
//! catalogue; separate prerequisite catalogues retain complete source receipts
//! and winning-content identity. No live actor state is initialized.
pub mod associations;
pub mod classes;
pub mod dependencies;
pub mod factions;
pub mod fields;
pub mod package_dependencies;
pub mod packages;
pub mod placements;
pub mod races;
pub mod script_attachment;
pub mod voices;
use crate::{Result, identity::FormKey, inventory, plugin, store::SourceReceipt};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Definition<'a> {
    pub key: &'a FormKey,
    pub kind: [u8; 4],
    pub source: &'a inventory::Source,
    pub deleted: bool,
    /// Deleted inventory definitions intentionally have no decoded body/header.
    pub record_version: Option<u16>,
    pub fields: Vec<fields::Field>,
    pub findings: Vec<fields::Finding>,
    #[serde(skip)]
    inventory_definition: &'a inventory::Definition,
}
impl<'a> Definition<'a> {
    pub fn inventory_definition(&self) -> &'a inventory::Definition {
        self.inventory_definition
    }
    pub fn record(&self) -> Option<&plugin::Record> {
        self.inventory_definition.record()
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub deleted_records: usize,
    pub decoded_bytes: usize,
    pub fields: usize,
    pub scalar_fields: usize,
    pub source_findings: usize,
    pub record_kinds: BTreeMap<String, usize>,
    pub record_versions: BTreeMap<String, usize>,
    pub scalar_layouts: BTreeMap<String, usize>,
}
pub struct Catalogue<'a> {
    inventory: &'a inventory::Catalogue,
    definitions: BTreeMap<FormKey, Definition<'a>>,
    counts: Counts,
}
impl<'a> Catalogue<'a> {
    pub fn sources(&self) -> &[SourceReceipt] {
        &self.inventory.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        self.inventory.winning_content_sha256()
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
    pub fn load(inventory: &'a inventory::Catalogue, limits: Limits) -> Result<Self> {
        let mut result = Self {
            inventory,
            definitions: BTreeMap::new(),
            counts: Counts::default(),
        };
        for (key, input) in inventory.iter() {
            if !matches!(&input.kind, b"NPC_" | b"CREA") {
                continue;
            }
            if result.counts.records >= limits.max_records {
                return Err(crate::Error::Unsupported(
                    "actor record budget exceeded".into(),
                ));
            }
            let mut definition = Definition {
                key,
                kind: input.kind,
                source: &input.source,
                deleted: input.deleted,
                record_version: None,
                fields: Vec::new(),
                findings: Vec::new(),
                inventory_definition: input,
            };
            if !input.deleted {
                let record = input.record().ok_or_else(|| {
                    crate::Error::Resolution(
                        "nondeleted actor inventory definition lacks retained source".into(),
                    )
                })?;
                result.counts.decoded_bytes = result
                    .counts
                    .decoded_bytes
                    .checked_add(record.payload.len())
                    .ok_or_else(|| {
                        crate::Error::Unsupported("actor decoded byte budget overflow".into())
                    })?;
                if result.counts.decoded_bytes > limits.max_decoded_bytes {
                    return Err(crate::Error::Unsupported(
                        "actor decoded byte budget exceeded".into(),
                    ));
                }
                let document = fields::decode(
                    record,
                    &input.source.plugin,
                    fields::Limits {
                        max_fields: limits.max_fields.saturating_sub(result.counts.fields),
                        ..Default::default()
                    },
                )?;
                definition.record_version = Some(record.header.version);
                definition.fields = document.fields;
                definition.findings = document.findings;
                *result
                    .counts
                    .record_versions
                    .entry(format!(
                        "{}:{}",
                        plugin::signature(input.kind),
                        record.header.version
                    ))
                    .or_default() += 1;
            } else {
                result.counts.deleted_records += 1;
            }
            result.counts.records += 1;
            result.counts.fields += definition.fields.len();
            result.counts.source_findings += definition.findings.len();
            *result
                .counts
                .record_kinds
                .entry(plugin::signature(input.kind))
                .or_default() += 1;
            for field in &definition.fields {
                if field.value != fields::Value::Opaque {
                    result.counts.scalar_fields += 1;
                    *result
                        .counts
                        .scalar_layouts
                        .entry(format!(
                            "{}:{}:{}",
                            plugin::signature(input.kind),
                            plugin::signature(field.kind),
                            field.bytes
                        ))
                        .or_default() += 1;
                }
            }
            result.definitions.insert(key.clone(), definition);
        }
        Ok(result)
    }
}
