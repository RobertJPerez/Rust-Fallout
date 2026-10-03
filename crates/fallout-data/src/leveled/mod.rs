//! Source-bound item, creature and NPC lists. No selections or random values.
pub mod fields;
pub mod graph;
use crate::{
    Result,
    identity::FormKey,
    inventory, plugin,
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
            max_fields: 2_000_000,
            max_entries: 1_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    Entry {
        level_bits: u16,
        level_padding: u16,
        item: inventory::Binding,
        count_bits: Option<u16>,
        count_padding: Option<u16>,
        schema_kind_allowed: Option<bool>,
    },
    ChanceNone {
        raw: u8,
    },
    Flags {
        raw: u8,
        all_lower_levels: bool,
        each_count: bool,
        use_all: Option<bool>,
    },
    Global {
        global: inventory::Binding,
    },
    Extra {
        owner: inventory::Binding,
        union_word: inventory::ExtraWord,
        condition_bits: u32,
    },
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
    pub kind: [u8; 4],
    pub source: inventory::Source,
    pub deleted: bool,
    pub fields: Vec<Field>,
    pub entries: Vec<fields::Entry>,
    pub findings: Vec<inventory::fields::Finding>,
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
    pub extra_fields: usize,
    pub absent_counts: usize,
    pub zero_counts: usize,
    pub high_bit_counts: usize,
    pub high_bit_levels: usize,
    pub source_findings: usize,
    pub record_kinds: BTreeMap<String, usize>,
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
        let mut locations = Vec::new();
        for (key, location) in store.winning_definitions() {
            if !matches!(
                &store.definition(location).header.kind,
                b"LVLI" | b"LVLC" | b"LVLN"
            ) {
                continue;
            }
            if locations.len() >= limits.max_records {
                return Err(crate::Error::Unsupported(
                    "leveled record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), location));
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
        for (key, location) in locations {
            let header = &store.definition(location).header;
            let kind = header.kind;
            let mut definition = Definition {
                key: key.clone(),
                kind,
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
                findings: Vec::new(),
                record: None,
            };
            if definition.deleted {
                result.counts.deleted_records += 1;
            } else {
                let record = Arc::new(store.read(location)?);
                result.counts.decoded_bytes = result
                    .counts
                    .decoded_bytes
                    .checked_add(record.payload.len())
                    .ok_or_else(|| {
                        crate::Error::Unsupported("leveled byte budget overflow".into())
                    })?;
                if result.counts.decoded_bytes > limits.max_decoded_bytes {
                    return Err(crate::Error::Unsupported(
                        "leveled decoded byte budget exceeded".into(),
                    ));
                }
                let document = fields::decode(
                    &record,
                    store.source_name(location),
                    fields::Limits {
                        max_record_bytes: 64 * 1024 * 1024,
                        max_fields: limits.max_fields.saturating_sub(result.counts.fields),
                        max_entries: limits.max_entries.saturating_sub(result.counts.entries),
                    },
                )?;
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                for field in document.fields {
                    let value = match field.value {
                        fields::Raw::Opaque => Value::Opaque,
                        fields::Raw::Entry {
                            level_bits,
                            level_padding,
                            raw_form,
                            count_bits,
                            count_padding,
                        } => {
                            result.counts.absent_counts += usize::from(count_bits.is_none());
                            result.counts.zero_counts += usize::from(count_bits == Some(0));
                            result.counts.high_bit_counts +=
                                usize::from(count_bits.is_some_and(|v| v & 0x8000 != 0));
                            result.counts.high_bit_levels += usize::from(level_bits & 0x8000 != 0);
                            let item =
                                inventory::binding(store, location, raw_form, &mut bindings)?;
                            let allowed = item.target.as_ref().map(|t| match &kind {
                                b"LVLC" => matches!(&t.kind, b"CREA" | b"LVLC"),
                                b"LVLN" => matches!(&t.kind, b"NPC_" | b"LVLN"),
                                _ => matches!(
                                    &t.kind,
                                    b"ALCH"
                                        | b"AMMO"
                                        | b"ARMO"
                                        | b"BOOK"
                                        | b"CCRD"
                                        | b"CHIP"
                                        | b"CMNY"
                                        | b"IMOD"
                                        | b"KEYM"
                                        | b"LVLI"
                                        | b"MISC"
                                        | b"NOTE"
                                        | b"WEAP"
                                ),
                            });
                            Value::Entry {
                                level_bits,
                                level_padding,
                                item,
                                count_bits,
                                count_padding,
                                schema_kind_allowed: allowed,
                            }
                        }
                        fields::Raw::ChanceNone { raw } => Value::ChanceNone { raw },
                        fields::Raw::Flags { raw } => Value::Flags {
                            raw,
                            all_lower_levels: raw & 1 != 0,
                            each_count: raw & 2 != 0,
                            use_all: (kind == *b"LVLI").then_some(raw & 4 != 0),
                        },
                        fields::Raw::Global { raw_form } => Value::Global {
                            global: inventory::binding(store, location, raw_form, &mut bindings)?,
                        },
                        fields::Raw::Extra {
                            owner,
                            union_word,
                            condition_bits,
                        } => {
                            result.counts.extra_fields += 1;
                            let extra = inventory::bind_value(
                                store,
                                location,
                                inventory::fields::Raw::Extra {
                                    owner,
                                    union_word,
                                    condition_bits,
                                },
                                &mut bindings,
                            )?;
                            let inventory::Value::Extra {
                                owner,
                                union_word,
                                condition_bits,
                            } = extra
                            else {
                                unreachable!("extra classifier");
                            };
                            Value::Extra {
                                owner,
                                union_word,
                                condition_bits,
                            }
                        }
                    };
                    definition.fields.push(Field {
                        kind: field.kind,
                        decoded_offset: field.decoded_offset,
                        bytes: field.bytes,
                        sha256: field.sha256,
                        value,
                    });
                }
                definition.entries = document.entries;
                definition.findings = document.findings;
                definition.record = Some(record);
            }
            result.counts.records += 1;
            result.counts.fields += definition.fields.len();
            result.counts.entries += definition.entries.len();
            result.counts.source_findings += definition.findings.len();
            *result
                .counts
                .record_kinds
                .entry(plugin::signature(kind))
                .or_default() += 1;
            result.definitions.insert(key, definition);
        }
        result.counts.binding_statuses = bindings.binding_statuses;
        Ok(result)
    }
}
