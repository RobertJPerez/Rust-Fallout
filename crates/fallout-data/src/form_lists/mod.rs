//! Immutable authored form lists. Physical order and repeated members are source
//! facts; editor sorting and the live game's list mutations are separate behavior.
pub mod graph;
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, malformed, plugin,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_decoded_bytes: 128 * 1024 * 1024,
            max_fields: 1_000_000,
            max_entries: 1_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    Member { form: inventory::Binding },
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub bytes: usize,
    pub sha256: String,
    pub value: Value,
}
#[derive(Debug, Serialize)]
pub struct Definition {
    pub key: FormKey,
    pub source: inventory::Source,
    pub deleted: bool,
    pub fields: Vec<Field>,
    /// Indices into fields, in physical source order. Repeated LNAMs stay repeated.
    pub entries: Vec<usize>,
    #[serde(skip)]
    record: Option<Arc<plugin::Record>>,
}
impl Definition {
    pub fn record(&self) -> Option<&plugin::Record> {
        self.record.as_deref()
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub deleted_records: usize,
    pub decoded_bytes: usize,
    pub fields: usize,
    pub entries: usize,
    pub binding_statuses: BTreeMap<String, usize>,
}
pub struct Catalogue {
    definitions: BTreeMap<FormKey, Definition>,
    winning_content_sha256: String,
    pub sources: Vec<SourceReceipt>,
    pub counts: Counts,
}
impl Catalogue {
    pub fn winning_content_sha256(&self) -> &str {
        &self.winning_content_sha256
    }
    pub fn get(&self, key: &FormKey) -> Option<&Definition> {
        self.definitions.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Definition)> {
        self.definitions.iter()
    }
    pub fn load(store: &mut RecordStore, limits: Limits) -> Result<Self> {
        let sources = store.source_receipts()?;
        let winning_content_sha256 =
            crate::record_metadata::inspect(store)?.winning_definitions_sha256;
        let mut selected = Vec::new();
        for (key, location) in store.winning_definitions() {
            if store.definition(location).header.kind != *b"FLST" {
                continue;
            }
            if selected.len() >= limits.max_records {
                return Err(budget("records"));
            }
            selected.push((key.clone(), location));
        }
        let digests = sources
            .iter()
            .map(|s| (s.source_name.clone(), s.source_sha256.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut result = Self {
            definitions: BTreeMap::new(),
            winning_content_sha256,
            sources,
            counts: Counts::default(),
        };
        let mut bindings = inventory::Counts::default();
        for (key, location) in selected {
            let header = &store.definition(location).header;
            let mut definition = Definition {
                key: key.clone(),
                source: inventory::Source {
                    plugin: store.source_name(location).into(),
                    sha256: digests[store.source_name(location)].clone(),
                    record_file_offset: header.offset,
                    record_flags: header.flags,
                    decoded_record_sha256: None,
                },
                deleted: header.flags & plugin::DELETED != 0,
                fields: Vec::new(),
                entries: Vec::new(),
                record: None,
            };
            if definition.deleted {
                // A winning tombstone never falls back to an older list body.
                result.counts.deleted_records += 1;
            } else {
                let remaining = limits.max_decoded_bytes - result.counts.decoded_bytes;
                let record =
                    Arc::new(store.read_bounded(location, remaining.min(64 * 1024 * 1024))?);
                if record.integrity_issue.is_some() {
                    return Err(malformed(
                        store.source_name(location),
                        record.header.offset,
                        "tainted form list",
                    ));
                }
                result.counts.decoded_bytes = result
                    .counts
                    .decoded_bytes
                    .checked_add(record.payload.len())
                    .ok_or_else(|| budget("decoded bytes"))?;
                if result.counts.decoded_bytes > limits.max_decoded_bytes {
                    return Err(budget("decoded bytes"));
                }
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                plugin::visit_subrecords(&record, store.source_name(location), |field| {
                    if result.counts.fields >= limits.max_fields {
                        return Err(budget("fields"));
                    }
                    let value = if field.kind == *b"LNAM" {
                        if field.data.len() != 4 {
                            return Err(malformed(
                                store.source_name(location),
                                record.header.offset,
                                "FLST LNAM needs exactly four bytes",
                            ));
                        }
                        if result.counts.entries >= limits.max_entries {
                            return Err(budget("entries"));
                        }
                        let raw =
                            u32::from_le_bytes(field.data.try_into().expect("checked LNAM extent"));
                        let form = inventory::binding(store, location, raw, &mut bindings)?;
                        definition.entries.push(definition.fields.len());
                        result.counts.entries += 1;
                        Value::Member { form }
                    } else {
                        Value::Opaque
                    };
                    definition.fields.push(Field {
                        kind: field.kind,
                        decoded_offset: u32::try_from(field.payload_offset)
                            .map_err(|_| budget("field offset"))?,
                        bytes: field.data.len(),
                        sha256: format!("{:x}", Sha256::digest(field.data)),
                        value,
                    });
                    result.counts.fields += 1;
                    Ok(())
                })?;
                definition.record = Some(record);
            }
            result.counts.records += 1;
            result.definitions.insert(key, definition);
        }
        result.counts.binding_statuses = bindings.binding_statuses;
        Ok(result)
    }
}
fn budget(name: &str) -> Error {
    Error::Unsupported(format!("form list {name} budget exceeded"))
}

#[cfg(test)]
mod tests;
