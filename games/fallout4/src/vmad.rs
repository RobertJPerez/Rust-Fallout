//! Bounded FO4 VMAD attachments. Raw IDs are plugin-local; no load order is assumed.
use crate::{Result, bad};
use serde::Serialize;
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub bytes: usize,
    pub nodes: usize,
    pub depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 16 * 1024 * 1024,
            nodes: 1_000_000,
            depth: 32,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Adapter<'a> {
    pub version: u16,
    pub object_format: u16,
    pub scripts: Vec<Script<'a>>,
    pub fragments: Option<Fragments<'a>>,
    #[serde(skip)]
    pub raw: &'a [u8],
}
#[derive(Debug, Serialize)]
pub struct Script<'a> {
    pub range: Range<usize>,
    pub name: &'a [u8],
    /// An empty script name has no flags or property count on disk.
    pub flags: Option<u8>,
    pub properties: Vec<Property<'a>>,
}
#[derive(Debug, Serialize)]
pub struct Property<'a> {
    pub range: Range<usize>,
    pub name: &'a [u8],
    pub flags: u8,
    pub value: Value<'a>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Object {
    pub form_id: u32,
    pub alias: i16,
    pub unused: u16,
}
#[derive(Debug, Serialize)]
pub enum Value<'a> {
    None,
    Object(Object),
    String(&'a [u8]),
    Int(i32),
    FloatBits(u32),
    BoolByte(u8),
    Struct(Vec<Property<'a>>),
    Objects(Vec<Object>),
    Strings(Vec<&'a [u8]>),
    Ints(Vec<i32>),
    Floats(Vec<u32>),
    Bools(Vec<u8>),
    Structs(Vec<Vec<Property<'a>>>),
}
impl Value<'_> {
    pub fn type_code(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::Object(_) => 1,
            Self::String(_) => 2,
            Self::Int(_) => 3,
            Self::FloatBits(_) => 4,
            Self::BoolByte(_) => 5,
            Self::Struct(_) => 7,
            Self::Objects(_) => 11,
            Self::Strings(_) => 12,
            Self::Ints(_) => 13,
            Self::Floats(_) => 14,
            Self::Bools(_) => 15,
            Self::Structs(_) => 17,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Fragment<'a> {
    pub range: Range<usize>,
    /// INFO/PACK/SCEN event bit, PERK/TERM packed index, or QUST packed stage.
    /// Kept as bits: independent readers disagree on high-half semantics.
    pub index_bits: u32,
    pub stage_index_bits: Option<u32>,
    pub unknown: u8,
    pub script: &'a [u8],
    pub function: &'a [u8],
}
#[derive(Debug, Serialize)]
pub struct Phase<'a> {
    pub range: Range<usize>,
    pub flags: u8,
    pub index_bits: u32,
    pub unknown: u8,
    pub script: &'a [u8],
    pub function: &'a [u8],
}
#[derive(Debug, Serialize)]
pub struct Alias<'a> {
    pub range: Range<usize>,
    pub object: Object,
    pub version: u16,
    pub object_format: u16,
    pub scripts: Vec<Script<'a>>,
}
#[derive(Debug, Serialize)]
pub struct Fragments<'a> {
    pub range: Range<usize>,
    pub extra_bind_version: u8,
    pub flags: Option<u8>,
    pub script: Script<'a>,
    pub fragments: Vec<Fragment<'a>>,
    pub phases: Vec<Phase<'a>>,
    pub aliases: Vec<Alias<'a>>,
}

struct Reader<'a, 'n> {
    raw: &'a [u8],
    name: &'n str,
    pos: usize,
    remaining_nodes: usize,
    limits: Limits,
}
impl<'a> Reader<'a, '_> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.raw.len() - self.pos {
            return Err(bad(self.name, self.pos, "truncated VMAD"));
        }
        let bytes = &self.raw[self.pos..self.pos + n];
        self.pos += n;
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
        let n = self.u16()? as usize;
        self.take(n)
    }
    fn list<T>(
        &mut self,
        n: usize,
        min_bytes: usize,
        mut read: impl FnMut(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        if n > self.remaining_nodes || n > (self.raw.len() - self.pos) / min_bytes {
            return Err(bad(
                self.name,
                self.pos,
                "VMAD count exceeds remaining bytes or node budget",
            ));
        }
        self.remaining_nodes -= n;
        (0..n).map(|_| read(self)).collect()
    }
    fn dialect(&self, version: u16, format: u16) -> Result<()> {
        if !matches!(version, 5 | 6) || !matches!(format, 1 | 2) {
            return Err(bad(
                self.name,
                self.pos,
                format!("unsupported VMAD version {version}/object format {format}"),
            ));
        }
        Ok(())
    }
    fn object(&mut self, format: u16) -> Result<Object> {
        Ok(if format == 1 {
            Object {
                form_id: self.u32()?,
                alias: self.u16()? as i16,
                unused: self.u16()?,
            }
        } else {
            let unused = self.u16()?;
            let alias = self.u16()? as i16;
            Object {
                form_id: self.u32()?,
                alias,
                unused,
            }
        })
    }
    fn script(&mut self, version: u16, format: u16) -> Result<Script<'a>> {
        let start = self.pos;
        let name = self.string()?;
        let (flags, properties) = if name.is_empty() {
            (None, Vec::new())
        } else {
            let flags = self.u8()?;
            let count = self.u16()? as usize;
            (
                Some(flags),
                self.list(count, 4, |r| r.property(version, format, 0))?,
            )
        };
        Ok(Script {
            range: start..self.pos,
            name,
            flags,
            properties,
        })
    }
    fn scripts(&mut self, version: u16, format: u16) -> Result<Vec<Script<'a>>> {
        let n = self.u16()? as usize;
        self.list(n, 2, |r| r.script(version, format))
    }
    fn members(&mut self, version: u16, format: u16, depth: usize) -> Result<Vec<Property<'a>>> {
        let n = self.u32()? as usize;
        self.list(n, 4, |r| r.property(version, format, depth))
    }
    fn property(&mut self, version: u16, format: u16, depth: usize) -> Result<Property<'a>> {
        if depth > self.limits.depth {
            return Err(bad(self.name, self.pos, "VMAD nesting budget exceeded"));
        }
        let start = self.pos;
        let name = self.string()?;
        let code = self.u8()?;
        let flags = self.u8()?;
        if version < 6 && matches!(code, 6 | 7 | 16 | 17) {
            return Err(bad(
                self.name,
                start,
                "VMAD v5 cannot contain FO4 struct/variable types",
            ));
        }
        let value = match code {
            0 => Value::None,
            1 => Value::Object(self.object(format)?),
            2 => Value::String(self.string()?),
            3 => Value::Int(self.u32()? as i32),
            4 => Value::FloatBits(self.u32()?),
            5 => Value::BoolByte(self.u8()?),
            7 => Value::Struct(self.members(version, format, depth + 1)?),
            11 => {
                let n = self.u32()? as usize;
                Value::Objects(self.list(n, 8, |r| r.object(format))?)
            }
            12 => {
                let n = self.u32()? as usize;
                Value::Strings(self.list(n, 2, Self::string)?)
            }
            13 => {
                let n = self.u32()? as usize;
                Value::Ints(self.list(n, 4, |r| Ok(r.u32()? as i32))?)
            }
            14 => {
                let n = self.u32()? as usize;
                Value::Floats(self.list(n, 4, Self::u32)?)
            }
            15 => {
                let n = self.u32()? as usize;
                Value::Bools(self.list(n, 1, Self::u8)?)
            }
            17 => {
                let n = self.u32()? as usize;
                Value::Structs(self.list(n, 4, |r| r.members(version, format, depth + 1))?)
            }
            _ => {
                return Err(bad(
                    self.name,
                    start,
                    format!("unsupported VMAD property type {code}; no guessed payload length"),
                ));
            }
        };
        Ok(Property {
            range: start..self.pos,
            name,
            flags,
            value,
        })
    }
    fn fragment(
        &mut self,
        start: usize,
        index_bits: u32,
        stage_index_bits: Option<u32>,
    ) -> Result<Fragment<'a>> {
        let unknown = self.u8()?;
        let script = self.string()?;
        let function = self.string()?;
        Ok(Fragment {
            range: start..self.pos,
            index_bits,
            stage_index_bits,
            unknown,
            script,
            function,
        })
    }
    fn suffix(&mut self, kind: [u8; 4], version: u16, format: u16) -> Result<Fragments<'a>> {
        let start = self.pos;
        let extra_bind_version = self.u8()?;
        if extra_bind_version != 3 {
            return Err(bad(
                self.name,
                start,
                format!("unsupported extra bind version {extra_bind_version}"),
            ));
        }
        let mut flags = None;
        let mut phases = Vec::new();
        let mut aliases = Vec::new();
        let script;
        let fragments;
        match &kind {
            b"INFO" | b"PACK" | b"SCEN" => {
                let mask = if kind == *b"PACK" { 7 } else { 3 };
                let bits = self.u8()?;
                if bits & !mask != 0 {
                    return Err(bad(
                        self.name,
                        self.pos - 1,
                        "unknown fragment presence bits",
                    ));
                }
                flags = Some(bits);
                script = self.script(version, format)?;
                let mut items = Vec::new();
                for bit in 0..3 {
                    if bits & (1 << bit) != 0 {
                        items.push(self.fragment(self.pos, bit, None)?);
                    }
                }
                fragments = items;
                if kind == *b"SCEN" {
                    let n = self.u16()? as usize;
                    phases = self.list(n, 10, |r| {
                        let start = r.pos;
                        let flags = r.u8()?;
                        let index_bits = r.u32()?;
                        let unknown = r.u8()?;
                        let script = r.string()?;
                        let function = r.string()?;
                        Ok(Phase {
                            range: start..r.pos,
                            flags,
                            index_bits,
                            unknown,
                            script,
                            function,
                        })
                    })?;
                }
            }
            b"PERK" | b"TERM" => {
                script = self.script(version, format)?;
                let n = self.u16()? as usize;
                fragments = self.list(n, 9, |r| {
                    let start = r.pos;
                    let index = r.u32()?;
                    r.fragment(start, index, None)
                })?;
            }
            b"QUST" => {
                let n = self.u16()? as usize;
                script = self.script(version, format)?;
                fragments = self.list(n, 13, |r| {
                    let start = r.pos;
                    let stage = r.u32()?;
                    let index = r.u32()?;
                    r.fragment(start, stage, Some(index))
                })?;
                let n = self.u16()? as usize;
                aliases = self.list(n, 14, |r| {
                    let start = r.pos;
                    // The alias has its own format, declared after the eight-byte object.
                    let header = r.take(12)?;
                    let version = u16::from_le_bytes(header[8..10].try_into().unwrap());
                    let object_format = u16::from_le_bytes(header[10..12].try_into().unwrap());
                    r.dialect(version, object_format)?;
                    r.pos = start;
                    let object = r.object(object_format)?;
                    r.take(4)?;
                    let scripts = r.scripts(version, object_format)?;
                    Ok(Alias {
                        range: start..r.pos,
                        object,
                        version,
                        object_format,
                        scripts,
                    })
                })?;
            }
            _ => {
                return Err(bad(
                    self.name,
                    start,
                    "unexpected VMAD suffix for record kind",
                ));
            }
        }
        Ok(Fragments {
            range: start..self.pos,
            extra_bind_version,
            flags,
            script,
            fragments,
            phases,
            aliases,
        })
    }
}

/// Decode one complete VMAD payload. Offsets refer to the decompressed subrecord
/// payload, never the compressed plugin file. Unsupported layouts fail explicitly.
pub fn parse<'a>(raw: &'a [u8], kind: [u8; 4], name: &str, limits: Limits) -> Result<Adapter<'a>> {
    if raw.len() > limits.bytes {
        return Err(bad(name, 0, "VMAD byte budget exceeded"));
    }
    let mut r = Reader {
        raw,
        name,
        pos: 0,
        remaining_nodes: limits.nodes,
        limits,
    };
    let version = r.u16()?;
    let object_format = r.u16()?;
    r.dialect(version, object_format)?;
    let scripts = r.scripts(version, object_format)?;
    let fragments = if r.pos < raw.len() {
        Some(r.suffix(kind, version, object_format)?)
    } else {
        None
    };
    if r.pos != raw.len() {
        return Err(bad(name, r.pos, "unconsumed VMAD bytes"));
    }
    Ok(Adapter {
        version,
        object_format,
        scripts,
        fragments,
        raw,
    })
}
