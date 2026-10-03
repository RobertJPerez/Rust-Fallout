//! Exact authored NPC_/CREA scalar words. Editor fixups, template inheritance
//! and runtime actor-value calculations are deliberately outside this decoder.
use crate::{Result, malformed, plugin};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_record_bytes: usize,
    pub max_fields: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_record_bytes: 64 * 1024 * 1024,
            max_fields: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    /// Flags and template flags remain in the joined inventory definition.
    Configuration {
        fatigue: u16,
        barter_gold: u16,
        level_word: u16,
        player_level_multiplier_flag: bool,
        calc_min: u16,
        calc_max: u16,
        speed_multiplier: u16,
        karma_bits: u32,
        disposition_base: i16,
    },
    NpcData {
        base_health: i32,
        attributes: [u8; 7],
        /// The pinned schema declares a variable byte array here; no migration
        /// or invented version threshold replaces these authored bytes.
        unused_tail: Vec<u8>,
    },
    NpcSkills {
        skill_values: [u8; 14],
        skill_offsets: [u8; 14],
    },
    CreatureData {
        creature_type: u8,
        combat_skill: u8,
        magic_skill: u8,
        stealth_skill: u8,
        health: i16,
        unused: [u8; 2],
        damage: i16,
        attributes: [u8; 7],
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub bytes: usize,
    pub sha256: String,
    pub value: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub field_decoded_offset: Option<u32>,
    pub code: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Document {
    pub fields: Vec<Field>,
    pub findings: Vec<Finding>,
}

fn u16_at(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().expect("checked scalar"))
}
fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("checked scalar"))
}
fn extent(
    record: &plugin::Record,
    name: &str,
    field: &plugin::Subrecord<'_>,
    size: usize,
) -> Result<()> {
    if field.data.len() != size {
        return Err(malformed(
            name,
            record.header.offset,
            format!(
                "{} at decoded +0x{:X} needs {size} bytes, found {}",
                plugin::signature(field.kind),
                field.payload_offset,
                field.data.len()
            ),
        ));
    }
    Ok(())
}

pub fn decode(record: &plugin::Record, name: &str, limits: Limits) -> Result<Document> {
    if record.payload.len() > limits.max_record_bytes {
        return Err(crate::Error::Unsupported(
            "actor record byte budget exceeded".into(),
        ));
    }
    if record.integrity_issue.is_some() || record.header.flags & plugin::DELETED != 0 {
        return Err(malformed(
            name,
            record.header.offset,
            "tainted or deleted record cannot supply actor scalar inputs",
        ));
    }
    let npc = match &record.header.kind {
        b"NPC_" => true,
        b"CREA" => false,
        _ => {
            return Err(malformed(
                name,
                record.header.offset,
                "actor scalar fields require NPC_ or CREA",
            ));
        }
    };
    // These header versions were independently observed in the pinned official
    // cohort. The selected disk schema is invariant across these versions.
    let supported = if npc {
        matches!(record.header.version, 14 | 15)
    } else {
        matches!(record.header.version, 9 | 11 | 13 | 14 | 15)
    };
    if !supported {
        return Err(crate::Error::Unsupported(format!(
            "{} actor record version {} at {name}:0x{:X}",
            plugin::signature(record.header.kind),
            record.header.version,
            record.header.offset
        )));
    }
    let mut document = Document {
        fields: Vec::new(),
        findings: Vec::new(),
    };
    let mut occurrences = [0usize; 3];
    plugin::visit_subrecords(record, name, |field| {
        if document.fields.len() >= limits.max_fields {
            return Err(crate::Error::Unsupported(
                "actor field budget exceeded".into(),
            ));
        }
        let offset = u32::try_from(field.payload_offset)
            .map_err(|_| crate::Error::Unsupported("actor decoded offset exceeds u32".into()))?;
        let selected = match &field.kind {
            b"ACBS" => Some(0),
            b"DATA" => Some(1),
            b"DNAM" if npc => Some(2),
            _ => None,
        };
        if let Some(index) = selected {
            occurrences[index] += 1;
            if occurrences[index] > 1 {
                document.findings.push(Finding {
                    field_decoded_offset: Some(offset),
                    code: [
                        "multiple_configuration_fields",
                        "multiple_actor_data_fields",
                        "multiple_npc_skill_fields",
                    ][index],
                });
            }
        }
        let value = match &field.kind {
            b"ACBS" => {
                extent(record, name, &field, 24)?;
                Value::Configuration {
                    fatigue: u16_at(field.data, 4),
                    barter_gold: u16_at(field.data, 6),
                    level_word: u16_at(field.data, 8),
                    player_level_multiplier_flag: u32_at(field.data, 0) & 0x80 != 0,
                    calc_min: u16_at(field.data, 10),
                    calc_max: u16_at(field.data, 12),
                    speed_multiplier: u16_at(field.data, 14),
                    karma_bits: u32_at(field.data, 16),
                    disposition_base: u16_at(field.data, 20) as i16,
                }
            }
            b"DATA" if npc => {
                if field.data.len() < 11 {
                    return Err(malformed(
                        name,
                        record.header.offset,
                        format!(
                            "NPC DATA at decoded +0x{offset:X} needs an 11-byte prefix, found {}",
                            field.data.len()
                        ),
                    ));
                }
                Value::NpcData {
                    base_health: u32_at(field.data, 0) as i32,
                    attributes: field.data[4..11].try_into().expect("checked NPC DATA"),
                    unused_tail: field.data[11..].to_vec(),
                }
            }
            b"DNAM" if npc => {
                extent(record, name, &field, 28)?;
                Value::NpcSkills {
                    skill_values: field.data[..14].try_into().expect("checked NPC DNAM"),
                    skill_offsets: field.data[14..].try_into().expect("checked NPC DNAM"),
                }
            }
            b"DATA" => {
                extent(record, name, &field, 17)?;
                Value::CreatureData {
                    creature_type: field.data[0],
                    combat_skill: field.data[1],
                    magic_skill: field.data[2],
                    stealth_skill: field.data[3],
                    health: u16_at(field.data, 4) as i16,
                    unused: field.data[6..8].try_into().expect("checked CREA DATA"),
                    damage: u16_at(field.data, 8) as i16,
                    attributes: field.data[10..].try_into().expect("checked CREA DATA"),
                }
            }
            _ => Value::Opaque,
        };
        document.fields.push(Field {
            kind: field.kind,
            decoded_offset: offset,
            bytes: field.data.len(),
            sha256: format!("{:x}", Sha256::digest(field.data)),
            value,
        });
        Ok(())
    })?;
    for (index, code) in ["missing_configuration_field", "missing_actor_data_field"]
        .into_iter()
        .enumerate()
    {
        if occurrences[index] == 0 {
            document.findings.push(Finding {
                field_decoded_offset: None,
                code,
            });
        }
    }
    Ok(document)
}
