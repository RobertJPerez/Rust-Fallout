//! Authored faction declarations, preserving legacy optional bytes and ordered
//! links. No effective rank, crime state or combat reaction is initialized.
use super::fields::Finding;
use crate::{
    Result,
    identity::FormKey,
    inventory, malformed, plugin, record_metadata,
    store::{Location, RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_records: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_fields: usize,
    pub max_bindings: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
            max_bindings: 1_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    Flags {
        flags_1: u8,
        flags_2: Option<u8>,
        unused: Option<[u8; 2]>,
    },
    UnusedFloat {
        bits: u32,
    },
    RankNumber {
        rank: i32,
    },
    Relation {
        faction: inventory::Binding,
        modifier: i32,
        group_combat_reaction: u32,
        schema_kind_allowed: Option<bool>,
    },
    Reputation {
        reputation: inventory::Binding,
        schema_kind_allowed: Option<bool>,
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
    pub source: inventory::Source,
    pub header: plugin::RecordHeader,
    pub deleted: bool,
    pub fields: Vec<Field>,
    pub findings: Vec<Finding>,
    #[serde(skip)]
    record: Option<plugin::Record>,
}
impl Definition {
    pub fn record(&self) -> Option<&plugin::Record> {
        self.record.as_ref()
    }
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub records: usize,
    pub deleted_records: usize,
    pub decoded_bytes: usize,
    pub fields: usize,
    pub selected_fields: usize,
    pub bindings: usize,
    pub source_findings: usize,
    pub record_versions: BTreeMap<String, usize>,
    pub selected_layouts: BTreeMap<String, usize>,
    pub binding_statuses: BTreeMap<String, usize>,
}
pub struct Catalogue {
    sources: Vec<SourceReceipt>,
    winning_content_sha256: String,
    definitions: BTreeMap<FormKey, Definition>,
    counts: Counts,
}
impl Catalogue {
    pub fn sources(&self) -> &[SourceReceipt] {
        &self.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        &self.winning_content_sha256
    }
    pub fn counts(&self) -> &Counts {
        &self.counts
    }
    pub fn get(&self, key: &FormKey) -> Option<&Definition> {
        self.definitions.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Definition)> {
        self.definitions.iter()
    }
    pub fn load(store: &mut RecordStore, limits: Limits) -> Result<Self> {
        let mut locations = Vec::new();
        for (key, at) in store.winning_definitions() {
            if store.definition(at).header.kind != *b"FACT" {
                continue;
            }
            if locations.len() >= limits.max_records {
                return Err(crate::Error::Unsupported(
                    "faction record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), at));
        }
        let mut result = Self {
            sources: store.source_receipts()?,
            winning_content_sha256: record_metadata::inspect(store)?.winning_definitions_sha256,
            definitions: BTreeMap::new(),
            counts: Counts::default(),
        };
        let mut binding_counts = inventory::Counts::default();
        for (key, location) in locations {
            let header = store.definition(location).header.clone();
            let mut definition = Definition {
                key: key.clone(),
                source: inventory::Source {
                    plugin: store.source_name(location).into(),
                    sha256: store.source_digest(location)?,
                    record_file_offset: header.offset,
                    record_flags: header.flags,
                    decoded_record_sha256: None,
                },
                deleted: header.flags & plugin::DELETED != 0,
                header,
                fields: Vec::new(),
                findings: Vec::new(),
                record: None,
            };
            if definition.deleted {
                result.counts.deleted_records += 1;
            } else {
                let maximum = limits.max_record_bytes.min(
                    limits
                        .max_decoded_bytes
                        .saturating_sub(result.counts.decoded_bytes),
                );
                let record = store.read_bounded(location, maximum)?;
                result.counts.decoded_bytes += record.payload.len();
                // Admit observed record versions with the pinned selected schema.
                // Layout absence is read directly, never inferred from a version.
                if !matches!(
                    record.header.version,
                    1 | 2 | 3 | 4 | 8 | 9 | 10 | 11 | 13 | 14 | 15
                ) {
                    return Err(crate::Error::Unsupported(format!(
                        "FACT record version {} at {}:0x{:X}",
                        record.header.version, definition.source.plugin, record.header.offset
                    )));
                }
                let mut seen = [0usize; 3];
                plugin::visit_subrecords(&record, &definition.source.plugin, |field| {
                    if result.counts.fields + definition.fields.len() >= limits.max_fields {
                        return Err(crate::Error::Unsupported(
                            "faction field budget exceeded".into(),
                        ));
                    }
                    let offset = u32::try_from(field.payload_offset).map_err(|_| {
                        crate::Error::Unsupported("faction decoded offset exceeds u32".into())
                    })?;
                    let selected = match &field.kind {
                        b"DATA" => Some((0, None)),
                        b"CNAM" => Some((1, Some(4))),
                        b"WMI1" => Some((2, Some(4))),
                        b"XNAM" => Some((3, Some(12))),
                        b"RNAM" => Some((4, Some(4))),
                        _ => None,
                    };
                    if let Some((index, length)) = selected {
                        if length.is_some_and(|length| field.data.len() != length)
                            || (index == 0 && !matches!(field.data.len(), 1 | 4))
                        {
                            return Err(malformed(
                                &definition.source.plugin,
                                record.header.offset,
                                format!(
                                    "unsupported FACT {} length {} at decoded +0x{offset:X}",
                                    plugin::signature(field.kind),
                                    field.data.len()
                                ),
                            ));
                        }
                        if index < 3 {
                            seen[index] += 1;
                            if seen[index] > 1 {
                                definition.findings.push(Finding {
                                    field_decoded_offset: Some(offset),
                                    code: [
                                        "multiple_faction_data_fields",
                                        "multiple_faction_unused_float_fields",
                                        "multiple_faction_reputation_fields",
                                    ][index],
                                });
                            }
                        }
                    }
                    let value = match &field.kind {
                        b"DATA" => Value::Flags {
                            flags_1: field.data[0],
                            flags_2: (field.data.len() == 4).then(|| field.data[1]),
                            unused: (field.data.len() == 4).then(|| [field.data[2], field.data[3]]),
                        },
                        b"CNAM" => Value::UnusedFloat {
                            bits: word(field.data, 0),
                        },
                        b"RNAM" => Value::RankNumber {
                            rank: word(field.data, 0) as i32,
                        },
                        b"XNAM" | b"WMI1" => {
                            if result.counts.bindings >= limits.max_bindings {
                                return Err(crate::Error::Unsupported(
                                    "faction binding budget exceeded".into(),
                                ));
                            }
                            let relation = field.kind == *b"XNAM";
                            let (binding, allowed) = bind(
                                store,
                                location,
                                word(field.data, 0),
                                relation,
                                &mut binding_counts,
                                &mut definition.findings,
                                offset,
                            )?;
                            result.counts.bindings += 1;
                            if relation {
                                Value::Relation {
                                    faction: binding,
                                    modifier: word(field.data, 4) as i32,
                                    group_combat_reaction: word(field.data, 8),
                                    schema_kind_allowed: allowed,
                                }
                            } else {
                                Value::Reputation {
                                    reputation: binding,
                                    schema_kind_allowed: allowed,
                                }
                            }
                        }
                        _ => Value::Opaque,
                    };
                    definition.fields.push(Field {
                        kind: field.kind,
                        decoded_offset: offset,
                        bytes: field.data.len(),
                        sha256: format!("{:x}", Sha256::digest(field.data)),
                        value,
                    });
                    Ok(())
                })?;
                if seen[0] == 0 {
                    definition.findings.push(Finding {
                        field_decoded_offset: None,
                        code: "missing_faction_data_field",
                    });
                }
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                definition.record = Some(record);
                *result
                    .counts
                    .record_versions
                    .entry(definition.header.version.to_string())
                    .or_default() += 1;
            }
            result.counts.records += 1;
            result.counts.fields += definition.fields.len();
            result.counts.source_findings += definition.findings.len();
            for field in &definition.fields {
                if !matches!(&field.value, Value::Opaque) {
                    result.counts.selected_fields += 1;
                    *result
                        .counts
                        .selected_layouts
                        .entry(format!("{}:{}", plugin::signature(field.kind), field.bytes))
                        .or_default() += 1;
                }
            }
            result.definitions.insert(key, definition);
        }
        result.counts.binding_statuses = binding_counts.binding_statuses;
        Ok(result)
    }
}
fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("checked faction word"),
    )
}
fn bind(
    store: &RecordStore,
    location: Location,
    raw: u32,
    relation: bool,
    counts: &mut inventory::Counts,
    findings: &mut Vec<Finding>,
    offset: u32,
) -> Result<(inventory::Binding, Option<bool>)> {
    let binding = inventory::binding(store, location, raw, counts)?;
    let allowed = binding.target.as_ref().map(|target| {
        if relation {
            matches!(&target.kind, b"FACT" | b"RACE")
        } else {
            target.kind == *b"REPU"
        }
    });
    let code = match binding.status {
        inventory::Status::Missing => Some("faction_target_missing"),
        inventory::Status::Deleted => Some("faction_target_deleted"),
        _ if allowed == Some(false) => Some("faction_target_wrong_kind"),
        _ => None,
    };
    if let Some(code) = code {
        findings.push(Finding {
            field_decoded_offset: Some(offset),
            code,
        });
    }
    Ok((binding, allowed))
}
