//! Skyrim VMAD v4/v5 attachments, including record-specific fragments and aliases.
//! Layout: xEdit 9fb0168, Core/wbDefinitionsTES5.pas, wbScriptEntry/PropertyObject.
use crate::{Result, bad};
use serde::Serialize;

mod fragments;
pub use fragments::{Alias, Extension, Fragment, Selector};

#[derive(Debug, Serialize)]
pub struct RecordAttachment<'a> {
    pub primary: Attachment<'a>,
    pub tail: Tail<'a>,
}
#[derive(Debug, Serialize)]
pub enum Tail<'a> {
    Absent,
    Decoded(Extension<'a>),
    /// Preserve the entire original tail if any part cannot be interpreted.
    Unsupported {
        offset: usize,
        bytes: &'a [u8],
        reason: String,
    },
}

#[derive(Debug, Serialize)]
pub struct Attachment<'a> {
    pub version: u16,
    pub object_format: u16,
    pub scripts: Vec<Script<'a>>,
    pub tail_offset: usize,
    pub undecoded_tail: &'a [u8],
}
#[derive(Debug, Serialize)]
pub struct Script<'a> {
    pub offset: usize,
    pub name: &'a [u8],
    pub status: u8,
    pub properties: Vec<Property<'a>>,
}
#[derive(Debug, Serialize)]
pub struct Property<'a> {
    pub offset: usize,
    pub name: &'a [u8],
    pub status: u8,
    pub value: Value<'a>,
}
#[derive(Debug, PartialEq, Serialize)]
pub struct Object {
    pub form_id: u32,
    pub alias: i16,
    pub unused: [u8; 2],
}
#[derive(Debug, PartialEq, Serialize)]
pub enum Value<'a> {
    None,
    Object(Object),
    String(&'a [u8]),
    Int(i32),
    FloatBits(u32),
    BoolByte(u8),
    Array {
        element_type: u8,
        values: Vec<Value<'a>>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bytes: usize,
    /// Aggregate scripts, properties and array elements, before allocating.
    pub max_items: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 16 * 1024 * 1024,
            max_items: 250_000,
        }
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    name: &'a str,
    remaining_items: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| bad(self.name, self.at, "truncated VMAD"))?;
        let bytes = &self.bytes[self.at..end];
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<&'a [u8]> {
        let len = self.u16()? as usize;
        self.take(len)
    }
    fn admit(&mut self, count: usize, minimum_bytes: usize) -> Result<()> {
        if count > self.remaining_items || count > (self.bytes.len() - self.at) / minimum_bytes {
            return Err(bad(
                self.name,
                self.at,
                "VMAD count exceeds byte or item budget",
            ));
        }
        self.remaining_items -= count;
        Ok(())
    }
    fn object(&mut self, format: u16) -> Result<Object> {
        let bytes = self.take(8)?;
        let (form, alias, unused) = if format == 1 { (0, 4, 6) } else { (4, 2, 0) };
        Ok(Object {
            form_id: u32::from_le_bytes(bytes[form..form + 4].try_into().unwrap()),
            alias: i16::from_le_bytes(bytes[alias..alias + 2].try_into().unwrap()),
            unused: bytes[unused..unused + 2].try_into().unwrap(),
        })
    }
    fn scripts(&mut self, version: u16, object_format: u16) -> Result<Vec<Script<'a>>> {
        let count = self.u16()? as usize;
        self.admit(count, 5)?;
        let mut scripts = Vec::with_capacity(count);
        for _ in 0..count {
            let offset = self.at;
            let name = self.string()?;
            let status = self.u8()?;
            let count = self.u16()? as usize;
            self.admit(count, 4)?;
            let mut properties = Vec::with_capacity(count);
            for _ in 0..count {
                let offset = self.at;
                let name = self.string()?;
                let tag = self.u8()?;
                let status = self.u8()?;
                if version < 5 && tag >= 11 {
                    return Err(bad(self.name, self.at - 2, "VMAD arrays require version 5"));
                }
                let value = self.value(tag, object_format)?;
                properties.push(Property {
                    offset,
                    name,
                    status,
                    value,
                });
            }
            scripts.push(Script {
                offset,
                name,
                status,
                properties,
            });
        }
        Ok(scripts)
    }
    fn value(&mut self, tag: u8, format: u16) -> Result<Value<'a>> {
        Ok(match tag {
            0 => Value::None,
            1 => Value::Object(self.object(format)?),
            2 => Value::String(self.string()?),
            3 => Value::Int(self.u32()? as i32),
            4 => Value::FloatBits(self.u32()?),
            5 => Value::BoolByte(self.u8()?),
            11..=15 => {
                let count = self.u32()? as usize;
                let element_type = tag - 10;
                let minimum = match element_type {
                    1 => 8,
                    2 => 2,
                    3 | 4 => 4,
                    _ => 1,
                };
                self.admit(count, minimum)?;
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(self.value(element_type, format)?);
                }
                Value::Array {
                    element_type,
                    values,
                }
            }
            _ => {
                return Err(bad(
                    self.name,
                    self.at,
                    format!("unsupported Skyrim VMAD property type {tag}"),
                ));
            }
        })
    }
}

fn reader<'a>(bytes: &'a [u8], name: &'a str, limits: Limits) -> Result<Reader<'a>> {
    if bytes.len() > limits.max_bytes {
        return Err(bad(name, 0, "VMAD byte budget exceeded"));
    }
    Ok(Reader {
        bytes,
        at: 0,
        name,
        remaining_items: limits.max_items,
    })
}

fn primary<'a>(r: &mut Reader<'a>) -> Result<Attachment<'a>> {
    let version = r.u16()?;
    let object_format = r.u16()?;
    if !matches!(version, 4 | 5) || !matches!(object_format, 1 | 2) {
        return Err(bad(
            r.name,
            0,
            format!("unsupported VMAD version {version}, object format {object_format}"),
        ));
    }
    let scripts = r.scripts(version, object_format)?;
    Ok(Attachment {
        version,
        object_format,
        scripts,
        tail_offset: r.at,
        undecoded_tail: &r.bytes[r.at..],
    })
}

/// Decode only primary attachments; retained for consumers that inspect raw tails.
pub fn decode<'a>(bytes: &'a [u8], name: &'a str, limits: Limits) -> Result<Attachment<'a>> {
    primary(&mut reader(bytes, name, limits)?)
}

/// A tail failure never discards valid primary attachments or becomes success.
pub fn decode_record<'a>(
    bytes: &'a [u8],
    kind: [u8; 4],
    name: &'a str,
    limits: Limits,
) -> Result<RecordAttachment<'a>> {
    let mut r = reader(bytes, name, limits)?;
    let primary = primary(&mut r)?;
    let tail = if primary.undecoded_tail.is_empty() {
        Tail::Absent
    } else {
        match fragments::read(&mut r, kind, primary.object_format) {
            Ok(extension) if r.at == bytes.len() => Tail::Decoded(extension),
            result => Tail::Unsupported {
                offset: primary.tail_offset,
                bytes: primary.undecoded_tail,
                reason: match result {
                    Err(error) => error.to_string(),
                    Ok(_) => format!(
                        "{} trailing bytes after VMAD extension at 0x{:X}",
                        bytes.len() - r.at,
                        r.at
                    ),
                },
            },
        }
    };
    Ok(RecordAttachment { primary, tail })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Independently authored byte layout, not a serializer shared with the decoder.
    const OBJECT: &[u8] = &[
        5, 0, 2, 0, 1, 0, 1, 0, b'S', 0, 1, 0, 1, 0, b'P', 1, 1, 0xAA, 0xBB, 0xFF, 0xFF, 0x78,
        0x56, 0x34, 0x12,
    ];
    #[test]
    fn object_format_two_preserves_alias_form_and_padding() {
        let a = decode(OBJECT, "fixture", Limits::default()).unwrap();
        assert_eq!(
            a.scripts[0].properties[0].value,
            Value::Object(Object {
                form_id: 0x12345678,
                alias: -1,
                unused: [0xAA, 0xBB],
            })
        );
        assert!(a.undecoded_tail.is_empty());
    }
    #[test]
    fn object_format_one_and_fragment_tail() {
        let mut bytes = OBJECT.to_vec();
        bytes[2] = 1;
        bytes[17..25].copy_from_slice(&[0x78, 0x56, 0x34, 0x12, 0xFE, 0xFF, 0xAA, 0xBB]);
        bytes.extend_from_slice(&[0xDE, 0xAD]);
        let a = decode(&bytes, "fixture", Limits::default()).unwrap();
        assert_eq!(
            a.scripts[0].properties[0].value,
            Value::Object(Object {
                form_id: 0x12345678,
                alias: -2,
                unused: [0xAA, 0xBB],
            })
        );
        assert_eq!(a.tail_offset, 25);
        assert_eq!(a.undecoded_tail, &[0xDE, 0xAD]);
    }
    #[test]
    fn all_truncations_fail() {
        for end in 0..OBJECT.len() {
            assert!(
                decode(&OBJECT[..end], "cut", Limits::default()).is_err(),
                "{end}"
            );
        }
    }
    #[test]
    fn array_bombs_and_unknown_types_fail() {
        let mut bytes = OBJECT[..17].to_vec();
        bytes[15] = 11;
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes, "bomb", Limits::default()).is_err());
        bytes[15] = 16;
        assert!(decode(&bytes, "tag", Limits::default()).is_err());
        assert!(
            decode(
                OBJECT,
                "budget",
                Limits {
                    max_items: 1,
                    ..Limits::default()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn version_four_supports_scalar_properties_but_refuses_version_five_arrays() {
        let mut bytes = OBJECT.to_vec();
        bytes[0] = 4;
        let a = decode(&bytes, "v4", Limits::default()).unwrap();
        assert_eq!(a.version, 4);
        assert_eq!(
            a.scripts[0].properties[0].value,
            Value::Object(Object {
                form_id: 0x12345678,
                alias: -1,
                unused: [0xAA, 0xBB],
            })
        );
        bytes.truncate(17);
        bytes[15] = 11;
        bytes.extend([0; 4]);
        assert!(decode(&bytes, "v4-array", Limits::default()).is_err());
        bytes[0] = 5;
        assert!(decode(&bytes, "v5-array", Limits::default()).is_ok());
        bytes[0] = 3;
        assert!(decode(&bytes, "unsupported-v3", Limits::default()).is_err());
    }
    #[test]
    fn scalar_array_preserves_nan_bits_and_non_boolean_bytes() {
        let mut bytes = OBJECT[..17].to_vec();
        bytes[15] = 14;
        bytes.extend_from_slice(&[1, 0, 0, 0, 0x34, 0x12, 0xC0, 0x7F]);
        let a = decode(&bytes, "float", Limits::default()).unwrap();
        assert_eq!(
            a.scripts[0].properties[0].value,
            Value::Array {
                element_type: 4,
                values: vec![Value::FloatBits(0x7FC01234)]
            }
        );
        bytes.truncate(17);
        bytes[15] = 5;
        bytes.push(0xFE);
        assert_eq!(
            decode(&bytes, "bool", Limits::default()).unwrap().scripts[0].properties[0].value,
            Value::BoolByte(0xFE)
        );
    }
}
