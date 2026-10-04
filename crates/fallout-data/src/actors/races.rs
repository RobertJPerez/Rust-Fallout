//! Winning RACE scalar source words. No race or FaceGen defaults are applied.
use super::fields::Finding;
use crate::{
    Result,
    identity::FormKey,
    inventory, malformed, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
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
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_records: 65_536,
            max_record_bytes: 64 * 1024 * 1024,
            max_decoded_bytes: 256 * 1024 * 1024,
            max_fields: 2_000_000,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SkillBoost {
    pub skill: i8,
    pub boost: i8,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    RaceData {
        skill_boosts: [SkillBoost; 7],
        unused: [u8; 2],
        height_bits: [u32; 2],
        weight_bits: [u32; 2],
        flags: u32,
    },
    MainClamp {
        bits: u32,
    },
    FaceClamp {
        bits: u32,
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
    pub scalar_fields: usize,
    pub source_findings: usize,
    pub record_versions: BTreeMap<String, usize>,
    pub scalar_layouts: BTreeMap<String, usize>,
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
            if store.definition(at).header.kind != *b"RACE" {
                continue;
            }
            if locations.len() >= limits.max_records {
                return Err(crate::Error::Unsupported(
                    "race record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), at));
        }
        let mut catalogue = Self {
            sources: store.source_receipts()?,
            winning_content_sha256: record_metadata::inspect(store)?.winning_definitions_sha256,
            definitions: BTreeMap::new(),
            counts: Counts::default(),
        };
        for (key, at) in locations {
            let header = store.definition(at).header.clone();
            let mut definition = Definition {
                key: key.clone(),
                source: inventory::Source {
                    plugin: store.source_name(at).into(),
                    sha256: store.source_digest(at)?,
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
                catalogue.counts.deleted_records += 1;
            } else {
                if definition.header.version != 15 {
                    return Err(crate::Error::Unsupported(format!(
                        "RACE record version {} at {}:0x{:X}",
                        definition.header.version,
                        definition.source.plugin,
                        definition.header.offset
                    )));
                }
                let maximum = limits.max_record_bytes.min(
                    limits
                        .max_decoded_bytes
                        .saturating_sub(catalogue.counts.decoded_bytes),
                );
                let record = store.read_bounded(at, maximum)?;
                catalogue.counts.decoded_bytes += record.payload.len();
                let (fields, findings) = decode(
                    &record,
                    &definition.source.plugin,
                    limits.max_fields.saturating_sub(catalogue.counts.fields),
                )?;
                definition.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                definition.fields = fields;
                definition.findings = findings;
                definition.record = Some(record);
                *catalogue
                    .counts
                    .record_versions
                    .entry(definition.header.version.to_string())
                    .or_default() += 1;
            }
            catalogue.counts.records += 1;
            catalogue.counts.fields += definition.fields.len();
            catalogue.counts.source_findings += definition.findings.len();
            for field in &definition.fields {
                if field.value != Value::Opaque {
                    catalogue.counts.scalar_fields += 1;
                    *catalogue
                        .counts
                        .scalar_layouts
                        .entry(format!("{}:{}", plugin::signature(field.kind), field.bytes))
                        .or_default() += 1;
                }
            }
            catalogue.definitions.insert(key, definition);
        }
        Ok(catalogue)
    }
}
fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("checked race word"),
    )
}
fn decode(
    record: &plugin::Record,
    name: &str,
    maximum_fields: usize,
) -> Result<(Vec<Field>, Vec<Finding>)> {
    if record.integrity_issue.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            "tainted record cannot supply race source inputs",
        ));
    }
    let mut fields = Vec::new();
    let mut findings = Vec::new();
    let mut seen = [0usize; 3];
    plugin::visit_subrecords(record, name, |field| {
        if fields.len() >= maximum_fields {
            return Err(crate::Error::Unsupported(
                "race field budget exceeded".into(),
            ));
        }
        let offset = u32::try_from(field.payload_offset)
            .map_err(|_| crate::Error::Unsupported("race decoded offset exceeds u32".into()))?;
        let selected = match &field.kind {
            b"DATA" => Some((0, 36)),
            b"PNAM" => Some((1, 4)),
            b"UNAM" => Some((2, 4)),
            _ => None,
        };
        if let Some((index, size)) = selected {
            if field.data.len() != size {
                return Err(malformed(
                    name,
                    record.header.offset,
                    format!(
                        "RACE {} at decoded +0x{offset:X} needs {size} bytes, found {}",
                        plugin::signature(field.kind),
                        field.data.len()
                    ),
                ));
            }
            seen[index] += 1;
            if seen[index] > 1 {
                findings.push(Finding {
                    field_decoded_offset: Some(offset),
                    code: [
                        "multiple_race_data_fields",
                        "multiple_race_main_clamp_fields",
                        "multiple_race_face_clamp_fields",
                    ][index],
                });
            }
        }
        let value = match &field.kind {
            b"DATA" => Value::RaceData {
                skill_boosts: std::array::from_fn(|index| SkillBoost {
                    skill: field.data[index * 2] as i8,
                    boost: field.data[index * 2 + 1] as i8,
                }),
                unused: [field.data[14], field.data[15]],
                height_bits: [word(field.data, 16), word(field.data, 20)],
                weight_bits: [word(field.data, 24), word(field.data, 28)],
                flags: word(field.data, 32),
            },
            b"PNAM" => Value::MainClamp {
                bits: word(field.data, 0),
            },
            b"UNAM" => Value::FaceClamp {
                bits: word(field.data, 0),
            },
            _ => Value::Opaque,
        };
        fields.push(Field {
            kind: field.kind,
            decoded_offset: offset,
            bytes: field.data.len(),
            sha256: format!("{:x}", Sha256::digest(field.data)),
            value,
        });
        Ok(())
    })?;
    for (index, count) in seen.into_iter().enumerate() {
        if count == 0 {
            findings.push(Finding {
                field_decoded_offset: None,
                code: [
                    "missing_race_data_field",
                    "missing_race_main_clamp_field",
                    "missing_race_face_clamp_field",
                ][index],
            });
        }
    }
    Ok((fields, findings))
}
