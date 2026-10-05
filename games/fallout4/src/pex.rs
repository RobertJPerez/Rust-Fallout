//! Bounded Fallout 4 PEX 3.9 structural decoding; no VM or native execution.
//! Layout facts are pinned in sources.lock.json. All original bytes remain borrowed.
use crate::{Error, Result, bad};
use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub file_bytes: usize,
    pub nodes: usize,
    pub varargs: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: 16 * 1024 * 1024,
            nodes: 1_000_000,
            varargs: 4096,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Header<'a> {
    pub major: u8,
    pub minor: u8,
    pub game: u16,
    pub compilation_time: u64,
    pub source_file: &'a [u8],
    pub user: &'a [u8],
    pub machine: &'a [u8],
}
#[derive(Debug, Serialize)]
pub struct File<'a> {
    #[serde(skip)]
    pub raw: &'a [u8],
    pub header: Header<'a>,
    pub strings: Vec<&'a [u8]>,
    pub debug_range: Range<usize>,
    pub user_flags: Vec<(u16, u8)>,
    pub objects: Vec<Object>,
}
#[derive(Debug, Serialize)]
pub struct Object {
    pub range: Range<usize>,
    pub name: u16,
    pub declared_size: u32,
    pub size_convention: &'static str,
    pub parent: u16,
    pub documentation: u16,
    pub constant: u8,
    pub user_flags: u32,
    pub auto_state: u16,
    pub structs: u16,
    pub variables: u16,
    pub properties: u16,
    pub states: u16,
    pub struct_definitions: Vec<Struct>,
    pub variable_definitions: Vec<Variable>,
    pub property_definitions: Vec<Property>,
    pub state_definitions: Vec<State>,
    pub functions: Vec<Function>,
}
#[derive(Debug, Serialize)]
pub struct Struct {
    pub range: Range<usize>,
    pub name: u16,
    pub members: Vec<Variable>,
}
#[derive(Debug, Serialize)]
pub struct Variable {
    pub range: Range<usize>,
    pub name: u16,
    pub type_name: u16,
    pub user_flags: u32,
    pub initial_value: Value,
    pub constant: u8,
    /// Struct members carry documentation; object variables do not.
    pub documentation: Option<u16>,
}
#[derive(Debug, Serialize)]
pub struct Property {
    pub range: Range<usize>,
    pub name: u16,
    pub type_name: u16,
    pub documentation: u16,
    pub user_flags: u32,
    pub flags: u8,
    pub auto_variable: Option<u16>,
    /// Accessors index the owning object's `functions` vector.
    pub getter: Option<usize>,
    pub setter: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct State {
    pub range: Range<usize>,
    pub name: u16,
    pub functions: Range<usize>,
}
#[derive(Debug, Serialize)]
pub struct Function {
    pub range: Range<usize>,
    pub state: Option<u16>,
    pub name: u16,
    /// method, getter, or setter; accessors use the property's name.
    pub kind: &'static str,
    pub return_type: u16,
    pub documentation: u16,
    pub user_flags: u32,
    pub flags: u8,
    pub parameters: Vec<(u16, u16)>,
    pub locals: Vec<(u16, u16)>,
    pub instructions: Vec<Instruction>,
}
impl Function {
    pub fn native(&self) -> bool {
        self.flags & 2 != 0
    }
}
#[derive(Debug, Serialize)]
pub struct Instruction {
    pub offset: usize,
    pub opcode: u8,
    pub arguments: Vec<Value>,
    pub varargs: Vec<Value>,
}
#[derive(Debug, Serialize, PartialEq)]
pub enum Value {
    None,
    Identifier(u16),
    String(u16),
    Integer(i32),
    FloatBits(u32),
    BoolByte(u8),
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    end: usize,
    name: &'a str,
    strings: usize,
    nodes: usize,
    limits: Limits,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.end.saturating_sub(self.pos) {
            return Err(bad(self.name, self.pos, "truncated PEX field"));
        }
        let slice = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
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
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<&'a [u8]> {
        let n = self.u16()? as usize;
        self.take(n)
    }
    fn index(&mut self) -> Result<u16> {
        let i = self.u16()?;
        if i as usize >= self.strings {
            return Err(bad(
                self.name,
                self.pos - 2,
                "PEX string index out of range",
            ));
        }
        Ok(i)
    }
    fn charge(&mut self, n: usize, min_bytes: usize) -> Result<()> {
        if n > self.limits.nodes.saturating_sub(self.nodes) || n > (self.end - self.pos) / min_bytes
        {
            return Err(bad(
                self.name,
                self.pos,
                "PEX collection exceeds input or node budget",
            ));
        }
        self.nodes += n;
        Ok(())
    }
    fn count(&mut self, min_bytes: usize) -> Result<u16> {
        let n = self.u16()?;
        self.charge(n as usize, min_bytes)?;
        Ok(n)
    }
    fn value(&mut self) -> Result<Value> {
        Ok(match self.u8()? {
            0 => Value::None,
            1 => Value::Identifier(self.index()?),
            2 => Value::String(self.index()?),
            3 => Value::Integer(self.u32()? as i32),
            4 => Value::FloatBits(self.u32()?),
            5 => Value::BoolByte(self.u8()?),
            tag => {
                return Err(bad(
                    self.name,
                    self.pos - 1,
                    format!("unknown PEX value tag {tag}"),
                ));
            }
        })
    }
    fn typed_names(&mut self) -> Result<Vec<(u16, u16)>> {
        let n = self.count(4)?;
        (0..n).map(|_| Ok((self.index()?, self.index()?))).collect()
    }
    fn debug(&mut self) -> Result<Range<usize>> {
        let start = self.pos;
        match self.u8()? {
            0 => return Ok(start..self.pos),
            1 => {}
            _ => return Err(bad(self.name, start, "invalid PEX debug presence flag")),
        }
        self.u64()?;
        for _ in 0..self.count(9)? {
            self.index()?;
            self.index()?;
            self.index()?;
            let kind = self.u8()?;
            if kind > 2 {
                return Err(bad(self.name, self.pos - 1, "unknown debug function kind"));
            }
            let count = self.count(2)?;
            self.take(count as usize * 2)?;
        }
        for _ in 0..self.count(12)? {
            self.index()?;
            self.index()?;
            self.index()?;
            self.u32()?;
            for _ in 0..self.count(2)? {
                self.index()?;
            }
        }
        for _ in 0..self.count(6)? {
            self.index()?;
            self.index()?;
            for _ in 0..self.count(2)? {
                self.index()?;
            }
        }
        Ok(start..self.pos)
    }
    fn function(&mut self, state: Option<u16>, name: u16, kind: &'static str) -> Result<Function> {
        let start = self.pos;
        let return_type = self.index()?;
        let documentation = self.index()?;
        let user_flags = self.u32()?;
        let flags = self.u8()?;
        if flags & !3 != 0 {
            return Err(bad(self.name, self.pos - 1, "unknown PEX function flags"));
        }
        let parameters = self.typed_names()?;
        let locals = self.typed_names()?;
        let count = self.count(1)?;
        let mut instructions = Vec::new();
        for _ in 0..count {
            let offset = self.pos;
            let opcode = self.u8()?;
            let args = operand_count(opcode).ok_or_else(|| {
                bad(
                    self.name,
                    offset,
                    format!("unsupported FO4 opcode {opcode}"),
                )
            })?;
            self.charge(args, 1)?;
            let arguments = (0..args)
                .map(|_| self.value())
                .collect::<Result<Vec<_>>>()?;
            let mut varargs = Vec::new();
            if matches!(opcode, 23..=25) {
                let Value::Integer(n) = self.value()? else {
                    return Err(bad(self.name, self.pos, "vararg count must be integer"));
                };
                if n < 0 || n as usize > self.limits.varargs {
                    return Err(bad(self.name, self.pos, "vararg count exceeds budget"));
                }
                self.charge(n as usize, 1)?;
                for _ in 0..n {
                    varargs.push(self.value()?);
                }
            }
            instructions.push(Instruction {
                offset,
                opcode,
                arguments,
                varargs,
            });
        }
        // Relative branches count instructions, not encoded bytes. A target just
        // beyond the final instruction is retained for later control-flow linking.
        for (pc, instruction) in instructions.iter().enumerate() {
            let operand = match instruction.opcode {
                20 => Some(0),
                21 | 22 => Some(1),
                _ => None,
            };
            if let Some(i) = operand {
                let Value::Integer(delta) = instruction.arguments[i] else {
                    return Err(bad(
                        self.name,
                        instruction.offset,
                        "branch displacement must be integer",
                    ));
                };
                let target = pc as i64 + i64::from(delta);
                if target < 0 || target > instructions.len() as i64 {
                    return Err(bad(
                        self.name,
                        instruction.offset,
                        "branch target outside function",
                    ));
                }
            }
        }
        Ok(Function {
            range: start..self.pos,
            state,
            name,
            kind,
            return_type,
            documentation,
            user_flags,
            flags,
            parameters,
            locals,
            instructions,
        })
    }
    fn variable(&mut self, member: bool) -> Result<Variable> {
        let start = self.pos;
        let name = self.index()?;
        let type_name = self.index()?;
        let user_flags = self.u32()?;
        let initial_value = self.value()?;
        let constant = self.u8()?;
        let documentation = if member { Some(self.index()?) } else { None };
        Ok(Variable {
            range: start..self.pos,
            name,
            type_name,
            user_flags,
            initial_value,
            constant,
            documentation,
        })
    }
    fn object(&mut self) -> Result<Object> {
        let start = self.pos;
        let name = self.index()?;
        let declared_size = self.u32()?;
        let enclosing_end = self.end;
        // The retail compiler counts the size field; pinned Caprica writes body
        // bytes only. Bound reads by the larger of those two explicit layouts.
        self.end = enclosing_end.min(self.pos + declared_size as usize);
        let parent = self.index()?;
        let documentation = self.index()?;
        let constant = self.u8()?;
        let user_flags = self.u32()?;
        let auto_state = self.index()?;
        let structs = self.count(4)?;
        let mut struct_definitions = Vec::new();
        for _ in 0..structs {
            let start = self.pos;
            let name = self.index()?;
            let count = self.count(12)?;
            let members = (0..count)
                .map(|_| self.variable(true))
                .collect::<Result<_>>()?;
            struct_definitions.push(Struct {
                range: start..self.pos,
                name,
                members,
            });
        }
        let variables = self.count(10)?;
        let variable_definitions = (0..variables)
            .map(|_| self.variable(false))
            .collect::<Result<_>>()?;
        let properties = self.count(11)?;
        let mut property_definitions = Vec::new();
        let mut functions = Vec::new();
        for _ in 0..properties {
            let start = self.pos;
            let property_name = self.index()?;
            let type_name = self.index()?;
            let documentation = self.index()?;
            let user_flags = self.u32()?;
            let flags = self.u8()?;
            if flags & !7 != 0 {
                return Err(bad(self.name, self.pos - 1, "unknown PEX property flags"));
            }
            let mut getter = None;
            let mut setter = None;
            let auto_variable = if flags & 4 != 0 {
                Some(self.index()?)
            } else {
                if flags & 1 != 0 {
                    getter = Some(functions.len());
                    functions.push(self.function(None, property_name, "getter")?);
                }
                if flags & 2 != 0 {
                    setter = Some(functions.len());
                    functions.push(self.function(None, property_name, "setter")?);
                }
                None
            };
            property_definitions.push(Property {
                range: start..self.pos,
                name: property_name,
                type_name,
                documentation,
                user_flags,
                flags,
                auto_variable,
                getter,
                setter,
            });
        }
        let states = self.count(4)?;
        let mut state_definitions = Vec::new();
        for _ in 0..states {
            let start = self.pos;
            let state = self.index()?;
            let first_function = functions.len();
            for _ in 0..self.count(17)? {
                let method = self.index()?;
                functions.push(self.function(Some(state), method, "method")?);
            }
            state_definitions.push(State {
                range: start..self.pos,
                name: state,
                functions: first_function..functions.len(),
            });
        }
        let body_bytes = self.pos - start - 6;
        let size_convention = if declared_size as usize == body_bytes + 4 {
            "includes-size-field"
        } else if declared_size as usize == body_bytes {
            "body-only"
        } else {
            return Err(bad(
                self.name,
                start + 2,
                "PEX object size does not match either supported compiler layout",
            ));
        };
        self.end = enclosing_end;
        Ok(Object {
            range: start..self.pos,
            name,
            declared_size,
            size_convention,
            parent,
            documentation,
            constant,
            user_flags,
            auto_state,
            structs,
            variables,
            properties,
            states,
            struct_definitions,
            variable_definitions,
            property_definitions,
            state_definitions,
            functions,
        })
    }
}

/// Wire operand arities, restricted to Fallout 4 (later-game opcodes fail).
pub fn operand_count(opcode: u8) -> Option<usize> {
    Some(match opcode {
        0 => 0,
        1..=9 | 15..=19 | 23 | 25 | 27..=29 | 32..=33 | 36 | 38..=39 | 42..=43 | 45 => 3,
        10..=14 | 21..=22 | 24 | 30..=31 => 2,
        20 | 26 | 37 | 44 | 46 => 1,
        34..=35 => 4,
        40..=41 => 5,
        _ => return None,
    })
}

pub fn parse<'a>(bytes: &'a [u8], source_name: &'a str, limits: Limits) -> Result<File<'a>> {
    if bytes.len() > limits.file_bytes {
        return Err(bad(source_name, 0, "PEX file exceeds byte budget"));
    }
    let mut r = Reader {
        bytes,
        pos: 0,
        end: bytes.len(),
        name: source_name,
        strings: 0,
        nodes: 0,
        limits,
    };
    if r.take(4)? != [0xde, 0xc0, 0x57, 0xfa] {
        return Err(Error::Unsupported(
            "expected little-endian Fallout 4 PEX magic".into(),
        ));
    }
    let major = r.u8()?;
    let minor = r.u8()?;
    let game = r.u16()?;
    if (major, minor, game) != (3, 9, 2) {
        return Err(Error::Unsupported(format!(
            "PEX dialect {major}.{minor} game {game}; expected FO4 3.9 game 2"
        )));
    }
    let header = Header {
        major,
        minor,
        game,
        compilation_time: r.u64()?,
        source_file: r.string()?,
        user: r.string()?,
        machine: r.string()?,
    };
    let count = r.count(2)?;
    r.strings = count as usize;
    let strings = (0..count).map(|_| r.string()).collect::<Result<Vec<_>>>()?;
    let debug_range = r.debug()?;
    let count = r.count(3)?;
    let user_flags = (0..count)
        .map(|_| Ok((r.index()?, r.u8()?)))
        .collect::<Result<Vec<_>>>()?;
    let count = r.count(25)?;
    let objects = (0..count).map(|_| r.object()).collect::<Result<Vec<_>>>()?;
    if r.pos != bytes.len() {
        return Err(bad(source_name, r.pos, "trailing PEX bytes"));
    }
    Ok(File {
        raw: bytes,
        header,
        strings,
        debug_range,
        user_flags,
        objects,
    })
}
