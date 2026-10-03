//! Immutable authored inventory fields. Counts and condition words remain exact;
//! this decoder does not roll leveled lists or initialize a live inventory.
use crate::{Result, malformed, plugin};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_record_bytes: usize,
    pub max_fields: usize,
    pub max_items: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_record_bytes: 64 * 1024 * 1024,
            max_fields: 1_000_000,
            max_items: 262_144,
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Raw {
    Opaque,
    Item {
        raw_form: u32,
        count: i32,
    },
    Extra {
        owner: u32,
        union_word: u32,
        condition_bits: u32,
    },
    ActorBase {
        flags: u32,
        template_flags: u16,
    },
    Template {
        raw_form: u32,
    },
    ContainerData {
        flags: u8,
        weight_bits: u32,
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
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub cnto_field: usize,
    pub coed_fields: Vec<usize>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub field_decoded_offset: u32,
    pub code: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Document {
    pub fields: Vec<Field>,
    pub items: Vec<Item>,
    pub findings: Vec<Finding>,
}
fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("validated inventory field"),
    )
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
            "inventory record byte budget exceeded".into(),
        ));
    }
    if record.integrity_issue.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            "tainted record cannot supply inventory inputs",
        ));
    }
    if !matches!(&record.header.kind, b"CONT" | b"NPC_" | b"CREA") {
        return Err(malformed(
            name,
            record.header.offset,
            "inventory fields require CONT, NPC_ or CREA",
        ));
    }
    let actor = record.header.kind != *b"CONT";
    let mut result = Document {
        fields: Vec::new(),
        items: Vec::new(),
        findings: Vec::new(),
    };
    let mut pending = None;
    let mut actor_headers = 0;
    let mut templates = 0;
    plugin::visit_subrecords(record, name, |field| {
        if result.fields.len() >= limits.max_fields {
            return Err(crate::Error::Unsupported(
                "inventory field budget exceeded".into(),
            ));
        }
        let offset = u32::try_from(field.payload_offset).map_err(|_| {
            crate::Error::Unsupported("inventory decoded offset exceeds u32".into())
        })?;
        let value = match &field.kind {
            b"CNTO" => {
                extent(record, name, &field, 8)?;
                if result.items.len() >= limits.max_items {
                    return Err(crate::Error::Unsupported(
                        "inventory item budget exceeded".into(),
                    ));
                }
                pending = Some(result.items.len());
                result.items.push(Item {
                    cnto_field: result.fields.len(),
                    coed_fields: Vec::new(),
                });
                Raw::Item {
                    raw_form: word(field.data, 0),
                    count: word(field.data, 4) as i32,
                }
            }
            b"COED" => {
                extent(record, name, &field, 12)?;
                if let Some(item) = pending {
                    let item = &mut result.items[item];
                    if !item.coed_fields.is_empty() {
                        result.findings.push(Finding {
                            field_decoded_offset: offset,
                            code: "multiple_item_extra_fields",
                        });
                    }
                    item.coed_fields.push(result.fields.len());
                } else {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "orphan_item_extra_field",
                    });
                }
                Raw::Extra {
                    owner: word(field.data, 0),
                    union_word: word(field.data, 4),
                    condition_bits: word(field.data, 8),
                }
            }
            b"ACBS" if actor => {
                extent(record, name, &field, 24)?;
                pending = None;
                actor_headers += 1;
                if actor_headers > 1 {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "multiple_actor_base_fields",
                    });
                }
                Raw::ActorBase {
                    flags: word(field.data, 0),
                    template_flags: u16::from_le_bytes(
                        field.data[22..24].try_into().expect("checked ACBS"),
                    ),
                }
            }
            b"TPLT" if actor => {
                extent(record, name, &field, 4)?;
                pending = None;
                templates += 1;
                if templates > 1 {
                    result.findings.push(Finding {
                        field_decoded_offset: offset,
                        code: "multiple_actor_template_fields",
                    });
                }
                Raw::Template {
                    raw_form: word(field.data, 0),
                }
            }
            b"DATA" if !actor => {
                extent(record, name, &field, 5)?;
                pending = None;
                Raw::ContainerData {
                    flags: field.data[0],
                    weight_bits: word(field.data, 1),
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
