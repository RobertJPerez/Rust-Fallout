//! Physical model/link inputs. This decoder never chooses an actor part or clip.
use crate::{
    Result,
    actors::fields::Finding,
    inventory, malformed, plugin,
    store::{Location, RecordStore},
    world,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub max_fields: usize,
    pub max_strings: usize,
    pub max_path_bytes: usize,
    pub max_bindings: usize,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct Counts {
    pub fields: usize,
    pub strings: usize,
    pub path_bytes: usize,
    pub bindings: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Marker {
    pub field_index: usize,
    pub decoded_offset: u32,
    pub kind: [u8; 4],
    pub raw_index: Option<u32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Context {
    pub region: Option<Marker>,
    pub sex: Option<Marker>,
    pub part: Option<Marker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRole {
    Model,
    Texture,
    ModelList,
    AnimationList,
}

#[derive(Debug, Serialize)]
pub struct ByteString {
    /// Offset within this field's data, before its terminating NUL byte.
    pub field_byte_offset: u32,
    /// Empty physical frames remain empty, including array terminal frames.
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRole {
    HeadPart,
    ExtraHeadPart,
    Hair,
    Eyes,
    FirstPersonModel,
}
impl LinkRole {
    fn allowed(self, kind: &[u8; 4]) -> bool {
        match self {
            Self::HeadPart | Self::ExtraHeadPart => kind == b"HDPT",
            Self::Hair => kind == b"HAIR",
            Self::Eyes => kind == b"EYES",
            Self::FirstPersonModel => kind == b"STAT",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Link {
    pub field_byte_offset: u32,
    pub binding: inventory::Binding,
    pub schema_kind_allowed: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Opaque,
    Marker {
        marker: Marker,
    },
    Paths {
        role: PathRole,
        context: Context,
        strings: Vec<ByteString>,
    },
    Links {
        role: LinkRole,
        bindings: Vec<Link>,
    },
    BipedSlots {
        flags: u32,
        general_flags: u8,
        unused: [u8; 3],
    },
    EquipmentType {
        raw: i32,
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

pub(super) struct Document {
    pub fields: Vec<Field>,
    pub findings: Vec<Finding>,
    pub counts: Counts,
}

fn offset(value: usize) -> Result<u32> {
    u32::try_from(value)
        .map_err(|_| crate::Error::Unsupported("actor dependency offset exceeds u32".into()))
}

fn string(
    document: &mut Document,
    strings: &mut Vec<ByteString>,
    at: usize,
    raw: &[u8],
    limits: Limits,
) -> Result<()> {
    if document.counts.strings >= limits.max_strings {
        return Err(crate::Error::Unsupported(
            "actor dependency string budget exceeded".into(),
        ));
    }
    if raw.len()
        > limits
            .max_path_bytes
            .saturating_sub(document.counts.path_bytes)
    {
        return Err(crate::Error::Unsupported(
            "actor dependency path byte budget exceeded".into(),
        ));
    }
    strings.push(ByteString {
        field_byte_offset: offset(at)?,
        raw: raw.to_vec(),
    });
    document.counts.strings += 1;
    document.counts.path_bytes += raw.len();
    Ok(())
}

pub(super) fn decode(
    store: &RecordStore,
    location: Location,
    record: &plugin::Record,
    limits: Limits,
) -> Result<Document> {
    let name = store.source_name(location);
    if record.integrity_issue.is_some() {
        return Err(malformed(
            name,
            record.header.offset,
            "tainted record cannot supply actor dependency inputs",
        ));
    }
    let kind = record.header.kind;
    let mut document = Document {
        fields: Vec::new(),
        findings: Vec::new(),
        counts: Counts::default(),
    };
    let mut context = Context::default();
    let mut singletons = [0usize; 6];
    let mut binding_counts = inventory::Counts::default();
    plugin::visit_subrecords(record, name, |field| {
        if document.counts.fields >= limits.max_fields {
            return Err(crate::Error::Unsupported(
                "actor dependency field budget exceeded".into(),
            ));
        }
        let field_index = document.fields.len();
        let decoded_offset = offset(field.payload_offset)?;
        let mut value = Value::Opaque;
        let mut singleton = None;
        if kind == *b"RACE" {
            if matches!(&field.kind, b"HNAM" | b"ENAM" | b"FGGS" | b"FGGA" | b"FGTS") {
                // These delimit the part declarations. Later FaceGen sex markers
                // must not be carried into an earlier body/head model context.
                context = Context::default();
            }
            if matches!(&field.kind, b"NAM0" | b"NAM1" | b"MNAM" | b"FNAM" | b"INDX") {
                let expected = if field.kind == *b"INDX" { 4 } else { 0 };
                if field.data.len() != expected {
                    return Err(malformed(
                        name,
                        record.header.offset,
                        format!(
                            "RACE {} at decoded +0x{decoded_offset:X} needs {expected} bytes, found {}",
                            plugin::signature(field.kind),
                            field.data.len()
                        ),
                    ));
                }
                let marker = Marker {
                    field_index,
                    decoded_offset,
                    kind: field.kind,
                    raw_index: (field.kind == *b"INDX").then(|| {
                        u32::from_le_bytes(field.data.try_into().expect("checked part index"))
                    }),
                };
                match &field.kind {
                    b"NAM0" | b"NAM1" => {
                        context = Context {
                            region: Some(marker),
                            ..Default::default()
                        };
                    }
                    b"MNAM" | b"FNAM" if context.region.is_some() => {
                        context.sex = Some(marker);
                        context.part = None;
                    }
                    b"INDX" if context.region.is_some() => context.part = Some(marker),
                    _ => {}
                }
                value = Value::Marker { marker };
            }
        }
        let path_role = match (&kind, &field.kind) {
            (b"NPC_" | b"CREA" | b"RACE" | b"HDPT" | b"HAIR", b"MODL") => Some(PathRole::Model),
            (b"RACE" | b"HAIR" | b"EYES", b"ICON") => Some(PathRole::Texture),
            (b"CREA", b"NIFZ") => Some(PathRole::ModelList),
            (b"NPC_" | b"CREA", b"KFFZ") => Some(PathRole::AnimationList),
            (b"ARMO" | b"ARMA", b"MODL" | b"MOD2" | b"MOD3" | b"MOD4") => Some(PathRole::Model),
            (
                b"WEAP",
                b"MODL" | b"MOD2" | b"MOD3" | b"MOD4" | b"MWD1" | b"MWD2" | b"MWD3" | b"MWD4"
                | b"MWD5" | b"MWD6" | b"MWD7",
            ) => Some(PathRole::Model),
            (b"STAT", b"MODL") => Some(PathRole::Model),
            _ => None,
        };
        if let Some(role) = path_role {
            let mut strings = Vec::new();
            match role {
                PathRole::Model | PathRole::Texture => {
                    let raw = world::terminated(field.data, name, record.header.offset)?;
                    string(&mut document, &mut strings, 0, raw, limits)?;
                    if matches!(&kind, b"ARMO" | b"ARMA" | b"WEAP") {
                        // Equipment singleton roles are distinguished by their
                        // exact field kind in the explicit equipment consumer.
                    } else if kind != *b"RACE" {
                        singleton = Some(usize::from(role == PathRole::Texture));
                    } else if context.region.is_none()
                        || context.sex.is_none()
                        || context.part.is_none()
                    {
                        document.findings.push(Finding {
                            field_decoded_offset: Some(decoded_offset),
                            code: "race_path_without_part_context",
                        });
                    }
                }
                PathRole::ModelList | PathRole::AnimationList => {
                    if !field.data.is_empty() && field.data.last() != Some(&0) {
                        return Err(malformed(
                            name,
                            record.header.offset,
                            format!(
                                "{} {} at decoded +0x{decoded_offset:X} has an unterminated string frame",
                                plugin::signature(kind),
                                plugin::signature(field.kind)
                            ),
                        ));
                    }
                    let mut at = 0;
                    for frame in field.data.split_inclusive(|byte| *byte == 0) {
                        let raw = world::terminated(frame, name, record.header.offset)?;
                        string(&mut document, &mut strings, at, raw, limits)?;
                        at += frame.len();
                    }
                    singleton = Some(if role == PathRole::ModelList { 4 } else { 5 });
                }
            }
            value = Value::Paths {
                role,
                context: if kind == *b"RACE" {
                    context
                } else {
                    Context::default()
                },
                strings,
            };
        }
        let selected_link = match (&kind, &field.kind) {
            (b"NPC_", b"PNAM") => Some((LinkRole::HeadPart, false)),
            (b"NPC_", b"HNAM") => {
                singleton = Some(2);
                Some((LinkRole::Hair, false))
            }
            (b"NPC_", b"ENAM") => {
                singleton = Some(3);
                Some((LinkRole::Eyes, false))
            }
            (b"HDPT", b"HNAM") => Some((LinkRole::ExtraHeadPart, false)),
            (b"RACE", b"HNAM") => Some((LinkRole::Hair, true)),
            (b"RACE", b"ENAM") => Some((LinkRole::Eyes, true)),
            (
                b"WEAP",
                b"WNAM" | b"WNM1" | b"WNM2" | b"WNM3" | b"WNM4" | b"WNM5" | b"WNM6" | b"WNM7",
            ) => Some((LinkRole::FirstPersonModel, false)),
            _ => None,
        };
        if let Some((role, array)) = selected_link {
            if (!array && field.data.len() != 4) || field.data.len() % 4 != 0 {
                return Err(malformed(
                    name,
                    record.header.offset,
                    format!(
                        "{} {} at decoded +0x{decoded_offset:X} needs {}, found {} bytes",
                        plugin::signature(kind),
                        plugin::signature(field.kind),
                        if array {
                            "complete 4-byte words"
                        } else {
                            "4 bytes"
                        },
                        field.data.len()
                    ),
                ));
            }
            let mut bindings = Vec::new();
            for (index, word) in field.data.as_chunks::<4>().0.iter().enumerate() {
                if document.counts.bindings >= limits.max_bindings {
                    return Err(crate::Error::Unsupported(
                        "actor dependency binding budget exceeded".into(),
                    ));
                }
                let raw = u32::from_le_bytes(*word);
                let binding = inventory::binding(store, location, raw, &mut binding_counts)?;
                let allowed = binding
                    .target
                    .as_ref()
                    .map(|target| role.allowed(&target.kind));
                bindings.push(Link {
                    field_byte_offset: offset(index * 4)?,
                    binding,
                    schema_kind_allowed: allowed,
                });
                document.counts.bindings += 1;
            }
            value = Value::Links { role, bindings };
        }
        if matches!(&kind, b"ARMO" | b"ARMA") && field.kind == *b"BMDT" {
            if field.data.len() != 8 {
                return Err(malformed(
                    name,
                    record.header.offset,
                    "equipment BMDT needs 8 bytes",
                ));
            }
            value = Value::BipedSlots {
                flags: u32::from_le_bytes(field.data[..4].try_into().expect("checked slots")),
                general_flags: field.data[4],
                unused: field.data[5..].try_into().expect("checked slots"),
            };
        }
        if matches!(&kind, b"ARMO" | b"ARMA" | b"WEAP") && field.kind == *b"ETYP" {
            if field.data.len() != 4 {
                return Err(malformed(
                    name,
                    record.header.offset,
                    "equipment ETYP needs 4 bytes",
                ));
            }
            value = Value::EquipmentType {
                raw: i32::from_le_bytes(field.data.try_into().expect("checked equipment type")),
            };
        }
        if let Some(index) = singleton {
            singletons[index] += 1;
            if singletons[index] > 1 {
                document.findings.push(Finding {
                    field_decoded_offset: Some(decoded_offset),
                    code: [
                        "multiple_actor_model_fields",
                        "multiple_actor_texture_fields",
                        "multiple_actor_hair_fields",
                        "multiple_actor_eyes_fields",
                        "multiple_actor_model_list_fields",
                        "multiple_actor_animation_list_fields",
                    ][index],
                });
            }
        }
        document.fields.push(Field {
            kind: field.kind,
            decoded_offset,
            bytes: field.data.len(),
            sha256: format!("{:x}", Sha256::digest(field.data)),
            value,
        });
        document.counts.fields += 1;
        Ok(())
    })?;
    Ok(document)
}
