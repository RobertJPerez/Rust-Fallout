//! Source-bound base inventory declarations. Duplicate entries, signed counts,
//! extra-data ambiguity and template inputs remain authored facts, not live state.
pub mod fields;
use crate::{
    Result,
    identity::FormKey,
    plugin,
    store::{Location, RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_items: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
            max_items: 1_000_000,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Null,
    Missing,
    Deleted,
    Defined,
}
#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub kind: [u8; 4],
    pub source_plugin: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct Binding {
    pub raw_form: u32,
    pub key: Option<FormKey>,
    pub status: Status,
    pub target: Option<Target>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtraWord {
    Unused { raw_word: u32 },
    Global { binding: Binding },
    RequiredRank { raw_word: u32, value: i32 },
    UnresolvedOwner { raw_word: u32 },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    Item {
        item: Binding,
        count: i32,
        schema_kind_allowed: Option<bool>,
    },
    Extra {
        owner: Binding,
        union_word: ExtraWord,
        condition_bits: u32,
    },
    ActorBase {
        flags: u32,
        template_flags: u16,
        inventory_template_flag: bool,
    },
    Template {
        template: Binding,
    },
    ContainerData {
        flags: u8,
        weight_bits: u32,
        respawn_flag: bool,
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
pub struct Source {
    pub plugin: String,
    pub sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_record_sha256: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Definition {
    pub key: FormKey,
    pub kind: [u8; 4],
    pub source: Source,
    pub deleted: bool,
    pub fields: Vec<Field>,
    pub items: Vec<fields::Item>,
    pub findings: Vec<fields::Finding>,
    #[serde(skip)]
    record: Option<Arc<plugin::Record>>,
}
impl Definition {
    /// Unknown and not-yet-decoded fields still have their original bytes.
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
    pub items: usize,
    pub extra_fields: usize,
    pub template_fields: usize,
    pub non_positive_counts: usize,
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
                b"CONT" | b"NPC_" | b"CREA"
            ) {
                continue;
            }
            if locations.len() >= limits.max_records {
                return Err(crate::Error::Unsupported(
                    "inventory record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), location));
        }
        let mut result = Self {
            definitions: BTreeMap::new(),
            winning_content_sha256,
            sources,
            counts: Counts::default(),
        };
        let source_digests = result
            .sources
            .iter()
            .map(|source| (source.source_name.clone(), source.source_sha256.clone()))
            .collect::<BTreeMap<_, _>>();
        for (key, location) in locations {
            let header = &store.definition(location).header;
            let mut definition = Definition {
                key: key.clone(),
                kind: header.kind,
                source: Source {
                    plugin: store.source_name(location).into(),
                    sha256: source_digests[store.source_name(location)].clone(),
                    record_file_offset: header.offset,
                    record_flags: header.flags,
                    decoded_record_sha256: None,
                },
                deleted: header.flags & plugin::DELETED != 0,
                fields: Vec::new(),
                items: Vec::new(),
                findings: Vec::new(),
                record: None,
            };
            if !definition.deleted {
                let record = Arc::new(store.read(location)?);
                result.counts.decoded_bytes = result
                    .counts
                    .decoded_bytes
                    .checked_add(record.payload.len())
                    .ok_or_else(|| {
                        crate::Error::Unsupported("inventory decoded byte budget overflow".into())
                    })?;
                if result.counts.decoded_bytes > limits.max_decoded_bytes {
                    return Err(crate::Error::Unsupported(
                        "inventory decoded byte budget exceeded".into(),
                    ));
                }
                let document = fields::decode(
                    &record,
                    store.source_name(location),
                    fields::Limits {
                        max_record_bytes: 64 * 1024 * 1024,
                        max_fields: limits.max_fields.saturating_sub(result.counts.fields),
                        max_items: limits.max_items.saturating_sub(result.counts.items),
                    },
                )?;
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                for field in document.fields {
                    let value = bind_value(store, location, field.value, &mut result.counts)?;
                    definition.fields.push(Field {
                        kind: field.kind,
                        decoded_offset: field.decoded_offset,
                        bytes: field.bytes,
                        sha256: field.sha256,
                        value,
                    });
                }
                definition.items = document.items;
                definition.findings = document.findings;
                definition.record = Some(record);
            } else {
                result.counts.deleted_records += 1;
            }
            result.counts.records += 1;
            result.counts.fields += definition.fields.len();
            result.counts.items += definition.items.len();
            result.counts.source_findings += definition.findings.len();
            *result
                .counts
                .record_kinds
                .entry(plugin::signature(definition.kind))
                .or_default() += 1;
            result.definitions.insert(key, definition);
        }
        Ok(result)
    }
}
pub(crate) fn binding(
    store: &RecordStore,
    location: Location,
    raw_form: u32,
    counts: &mut Counts,
) -> Result<Binding> {
    let key = store.key_for(location, raw_form)?;
    let target = key
        .as_ref()
        .and_then(|key| store.winner(key))
        .map(|location| {
            let header = &store.definition(location).header;
            Target {
                kind: header.kind,
                source_plugin: store.source_name(location).into(),
                record_file_offset: header.offset,
                record_flags: header.flags,
            }
        });
    let status = if key.is_none() {
        Status::Null
    } else if let Some(target) = &target {
        if target.record_flags & plugin::DELETED != 0 {
            Status::Deleted
        } else {
            Status::Defined
        }
    } else {
        Status::Missing
    };
    let name = match status {
        Status::Null => "null",
        Status::Missing => "missing",
        Status::Deleted => "deleted",
        Status::Defined => "defined",
    };
    *counts.binding_statuses.entry(name.into()).or_default() += 1;
    Ok(Binding {
        raw_form,
        key,
        status,
        target,
    })
}
pub(crate) fn bind_value(
    store: &RecordStore,
    location: Location,
    raw: fields::Raw,
    counts: &mut Counts,
) -> Result<Value> {
    Ok(match raw {
        fields::Raw::Opaque => Value::Opaque,
        fields::Raw::Item { raw_form, count } => {
            counts.non_positive_counts += usize::from(count <= 0);
            let item = binding(store, location, raw_form, counts)?;
            let allowed = item.target.as_ref().map(|target| {
                matches!(
                    &target.kind,
                    b"ARMO"
                        | b"AMMO"
                        | b"MISC"
                        | b"WEAP"
                        | b"BOOK"
                        | b"LVLI"
                        | b"KEYM"
                        | b"ALCH"
                        | b"NOTE"
                        | b"IMOD"
                        | b"CMNY"
                        | b"CCRD"
                        | b"LIGH"
                        | b"CHIP"
                )
            });
            Value::Item {
                item,
                count,
                schema_kind_allowed: allowed,
            }
        }
        fields::Raw::Extra {
            owner,
            union_word,
            condition_bits,
        } => {
            counts.extra_fields += 1;
            let owner = binding(store, location, owner, counts)?;
            let word = if owner.status == Status::Null {
                ExtraWord::Unused {
                    raw_word: union_word,
                }
            } else if owner.status != Status::Defined {
                ExtraWord::UnresolvedOwner {
                    raw_word: union_word,
                }
            } else {
                match owner.target.as_ref().expect("defined owner header").kind {
                    kind if kind == *b"NPC_" => ExtraWord::Global {
                        binding: binding(store, location, union_word, counts)?,
                    },
                    kind if kind == *b"FACT" => ExtraWord::RequiredRank {
                        raw_word: union_word,
                        value: union_word as i32,
                    },
                    _ => ExtraWord::UnresolvedOwner {
                        raw_word: union_word,
                    },
                }
            };
            Value::Extra {
                owner,
                union_word: word,
                condition_bits,
            }
        }
        fields::Raw::ActorBase {
            flags,
            template_flags,
        } => Value::ActorBase {
            flags,
            template_flags,
            inventory_template_flag: template_flags & 0x100 != 0,
        },
        fields::Raw::Template { raw_form } => {
            counts.template_fields += 1;
            Value::Template {
                template: binding(store, location, raw_form, counts)?,
            }
        }
        fields::Raw::ContainerData { flags, weight_bits } => Value::ContainerData {
            flags,
            weight_bits,
            respawn_flag: flags & 2 != 0,
        },
    })
}
