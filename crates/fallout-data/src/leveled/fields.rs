//! Authored leveled-list words. Selection and random draws belong to the runtime.
use crate::{Result, inventory::fields::Finding, malformed, plugin};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_record_bytes: usize,
    pub max_fields: usize,
    pub max_entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_record_bytes: 64 * 1024 * 1024,
            max_fields: 1_000_000,
            max_entries: 262_144,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Raw {
    Opaque,
    Entry {
        level_bits: u16,
        level_padding: u16,
        raw_form: u32,
        count_bits: Option<u16>,
        count_padding: Option<u16>,
    },
    ChanceNone {
        raw: u8,
    },
    Flags {
        raw: u8,
    },
    Global {
        raw_form: u32,
    },
    Extra {
        owner: u32,
        union_word: u32,
        condition_bits: u32,
    },
}
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub bytes: usize,
    pub sha256: String,
    pub value: Raw,
}
#[derive(Debug, Serialize)]
pub struct Entry {
    pub lvlo_field: usize,
    pub coed_fields: Vec<usize>,
}
#[derive(Debug, Serialize)]
pub struct Document {
    pub fields: Vec<Field>,
    pub entries: Vec<Entry>,
    pub findings: Vec<Finding>,
}
fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("validated list word"))
}
fn short(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().expect("validated list short"))
}
pub fn decode(record: &plugin::Record, name: &str, limits: Limits) -> Result<Document> {
    if record.payload.len() > limits.max_record_bytes {
        return Err(crate::Error::Unsupported(
            "leveled record byte budget exceeded".into(),
        ));
    }
    if record.integrity_issue.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            "tainted record cannot supply leveled inputs",
        ));
    }
    if !matches!(&record.header.kind, b"LVLI" | b"LVLC" | b"LVLN") {
        return Err(malformed(
            name,
            record.header.offset,
            "leveled fields require LVLI/LVLC/LVLN",
        ));
    }
    let mut result = Document {
        fields: Vec::new(),
        entries: Vec::new(),
        findings: Vec::new(),
    };
    let mut pending = None;
    let mut chances = 0;
    let mut flags = 0;
    let mut globals = 0;
    plugin::visit_subrecords(record, name, |field| {
        if result.fields.len() >= limits.max_fields {
            return Err(crate::Error::Unsupported(
                "leveled field budget exceeded".into(),
            ));
        }
        let offset = u32::try_from(field.payload_offset)
            .map_err(|_| crate::Error::Unsupported("leveled offset exceeds u32".into()))?;
        let exact = |required: usize| -> Result<()> {
            if field.data.len() != required {
                return Err(malformed(
                    name,
                    record.header.offset,
                    format!(
                        "{} at decoded +0x{offset:X} needs {required} bytes, found {}",
                        plugin::signature(field.kind),
                        field.data.len()
                    ),
                ));
            }
            Ok(())
        };
        let value = match &field.kind {
            b"LVLO" => {
                // The common schema makes count and trailing padding optional.
                // Absence stays explicit; an editor's default is not inserted.
                if !matches!(field.data.len(), 8 | 10 | 12) {
                    return Err(malformed(
                        name,
                        record.header.offset,
                        "LVLO needs 8, 10 or 12 bytes",
                    ));
                }
                if result.entries.len() >= limits.max_entries {
                    return Err(crate::Error::Unsupported(
                        "leveled entry budget exceeded".into(),
                    ));
                }
                pending = Some(result.entries.len());
                result.entries.push(Entry {
                    lvlo_field: result.fields.len(),
                    coed_fields: Vec::new(),
                });
                Raw::Entry {
                    level_bits: short(field.data, 0),
                    level_padding: short(field.data, 2),
                    raw_form: word(field.data, 4),
                    count_bits: (field.data.len() >= 10).then(|| short(field.data, 8)),
                    count_padding: (field.data.len() == 12).then(|| short(field.data, 10)),
                }
            }
            b"COED" => {
                exact(12)?;
                if let Some(entry) = pending {
                    let entry = &mut result.entries[entry];
                    if !entry.coed_fields.is_empty() {
                        result.findings.push(Finding {
                            field_decoded_offset: offset,
                            code: "multiple_entry_extra_fields",
                        });
                    }
                    entry.coed_fields.push(result.fields.len());
                } else {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "orphan_entry_extra_field",
                    });
                }
                Raw::Extra {
                    owner: word(field.data, 0),
                    union_word: word(field.data, 4),
                    condition_bits: word(field.data, 8),
                }
            }
            b"LVLD" => {
                exact(1)?;
                pending = None;
                chances += 1;
                if chances > 1 {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "multiple_chance_none_fields",
                    });
                }
                Raw::ChanceNone { raw: field.data[0] }
            }
            b"LVLF" => {
                exact(1)?;
                pending = None;
                flags += 1;
                if flags > 1 {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "multiple_list_flag_fields",
                    });
                }
                Raw::Flags { raw: field.data[0] }
            }
            b"LVLG" if record.header.kind == *b"LVLI" => {
                exact(4)?;
                pending = None;
                globals += 1;
                if globals > 1 {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "multiple_chance_global_fields",
                    });
                }
                Raw::Global {
                    raw_form: word(field.data, 0),
                }
            }
            _ => {
                pending = None;
                Raw::Opaque
            }
        };
        result.fields.push(Field {
            kind: field.kind,
            decoded_offset: offset,
            bytes: field.data.len(),
            sha256: format!("{:x}", Sha256::digest(field.data)),
            value,
        });
        Ok(())
    })?;
    Ok(result)
}
