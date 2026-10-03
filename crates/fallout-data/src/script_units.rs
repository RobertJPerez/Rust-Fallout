//! Script metadata belongs to one SCHR, not to the containing record as a whole.
//! INFO and PACK can contain several independent scripts. Keep their tables in
//! authored order; SCDA reference numbers select the table starting at one.
//!
//! Layout: pinned xEdit FNV definitions. Lookup: pinned xNVSE GetRefFromRefList
//! and GetVariableInfo. No source text, inferred type or runtime value is needed
//! to associate a table entry with its original form or variable declaration.

use crate::{Result, malformed, plugin};
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_fields: usize,
    pub max_units: usize,
    pub max_variables_per_unit: usize,
    pub max_references_per_unit: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_fields: 1_048_576,
            max_units: 65_536,
            max_variables_per_unit: 65_536,
            max_references_per_unit: 65_536,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Field<'a> {
    pub kind: [u8; 4],
    /// Header offset within the decoded record. The data begins six bytes later.
    pub offset: usize,
    pub data: &'a [u8],
}

#[derive(Debug)]
pub struct Variable<'a> {
    pub declaration: Field<'a>,
    pub name: Field<'a>,
    pub index: u32,
    /// Zero also represents references in vanilla. This byte alone cannot tell
    /// a numeric variable from a reference, and is never used to invent a type.
    pub type_byte: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reference {
    Form(u32),
    Variable(u32),
}

#[derive(Debug, Clone, Copy)]
pub struct ReferenceEntry<'a> {
    pub field: Field<'a>,
    pub target: Reference,
}

#[derive(Debug)]
pub struct Unit<'a> {
    pub header: Field<'a>,
    pub compiled: Option<Field<'a>>,
    pub source: Option<Field<'a>>,
    pub variables: Vec<Variable<'a>>,
    pub references: Vec<ReferenceEntry<'a>>,
    /// All script fields in original order, including unused header bytes and
    /// source text. Inspection reports hash these bytes rather than publish them.
    pub fields: Vec<Field<'a>>,
    variable_positions: BTreeMap<u32, usize>,
}

impl Unit<'_> {
    pub fn declared_references(&self) -> u32 {
        word(self.header.data, 4)
    }

    pub fn declared_compiled_bytes(&self) -> u32 {
        word(self.header.data, 8)
    }

    pub fn declared_variables(&self) -> u32 {
        word(self.header.data, 12)
    }

    pub fn script_type(&self) -> u16 {
        u16::from_le_bytes(self.header.data[16..18].try_into().expect("checked SCHR"))
    }

    pub fn flags(&self) -> u16 {
        u16::from_le_bytes(self.header.data[18..20].try_into().expect("checked SCHR"))
    }

    pub fn variable(&self, index: u32) -> Option<&Variable<'_>> {
        self.variable_positions
            .get(&index)
            .map(|&position| &self.variables[position])
    }

    /// Zero deliberately returns None. It must not alias the first entry.
    pub fn reference(&self, index: u32) -> Option<&ReferenceEntry<'_>> {
        self.references.get(index.checked_sub(1)? as usize)
    }
}

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().expect("validated field extent"))
}

pub fn script_field(kind: [u8; 4]) -> bool {
    matches!(
        &kind,
        b"SCHR" | b"SCDA" | b"SCTX" | b"SLSD" | b"SCVR" | b"SCRO" | b"SCRV"
    )
}

/// Collect checked ranges first so returned views borrow the record, not the
/// visitor's temporary Subrecord. XXXX is handled by the shared strict framing.
pub fn decode<'a>(record: &'a plugin::Record, name: &str, limits: Limits) -> Result<Vec<Unit<'a>>> {
    let mut ranges: Vec<([u8; 4], usize, Range<usize>)> = Vec::new();
    let fail = |at: usize, reason: String| {
        malformed(
            name,
            record.header.offset,
            format!("script field at decoded offset 0x{at:X}: {reason}"),
        )
    };
    let mut field_count = 0;
    plugin::visit_subrecords(record, name, |sub| {
        field_count += 1;
        if field_count > limits.max_fields {
            return Err(fail(sub.payload_offset, "field budget exceeded".into()));
        }
        if script_field(sub.kind) {
            let start = sub.payload_offset + 6;
            ranges.push((sub.kind, sub.payload_offset, start..start + sub.data.len()));
        }
        Ok(())
    })?;
    let mut units: Vec<Unit<'a>> = Vec::new();
    let mut pending_variable = None;
    for (kind, offset, range) in ranges {
        let field = Field {
            kind,
            offset,
            data: &record.payload[range],
        };
        if kind == *b"SCHR" {
            if pending_variable.is_some() {
                return Err(fail(offset, "SLSD has no following SCVR".into()));
            }
            if field.data.len() != 20 {
                return Err(fail(offset, "SCHR must contain 20 bytes".into()));
            }
            if units.len() >= limits.max_units {
                return Err(fail(offset, "script unit budget exceeded".into()));
            }
            units.push(Unit {
                header: field,
                compiled: None,
                source: None,
                variables: Vec::new(),
                references: Vec::new(),
                fields: vec![field],
                variable_positions: BTreeMap::new(),
            });
            continue;
        }
        let unit = units
            .last_mut()
            .ok_or_else(|| fail(offset, "script field has no SCHR owner".into()))?;
        if pending_variable.is_some() && kind != *b"SCVR" {
            return Err(fail(offset, "SLSD has no following SCVR".into()));
        }
        unit.fields.push(field);
        match &kind {
            b"SCDA" => {
                if unit.compiled.replace(field).is_some() {
                    return Err(fail(offset, "duplicate SCDA in one script unit".into()));
                }
            }
            b"SCTX" => {
                if unit.source.replace(field).is_some() {
                    return Err(fail(offset, "duplicate SCTX in one script unit".into()));
                }
            }
            b"SLSD" => {
                if field.data.len() != 24 {
                    return Err(fail(offset, "SLSD must contain 24 bytes".into()));
                }
                if unit.variables.len() >= limits.max_variables_per_unit {
                    return Err(fail(offset, "variable budget exceeded".into()));
                }
                pending_variable = Some(field);
            }
            b"SCVR" => {
                let declaration = pending_variable
                    .take()
                    .ok_or_else(|| fail(offset, "SCVR has no SLSD declaration".into()))?;
                if field.data.last() != Some(&0) || field.data[..field.data.len() - 1].contains(&0)
                {
                    return Err(fail(offset, "SCVR must contain one terminated name".into()));
                }
                let index = word(declaration.data, 0);
                // The retail corpus contains repeated declarations. xNVSE's
                // GetVariableInfo searches the authored list and returns its
                // first match. Preserve every entry and index the first one.
                unit.variable_positions
                    .entry(index)
                    .or_insert(unit.variables.len());
                unit.variables.push(Variable {
                    declaration,
                    name: field,
                    index,
                    type_byte: declaration.data[16],
                });
            }
            b"SCRO" | b"SCRV" => {
                if field.data.len() != 4 {
                    return Err(fail(
                        offset,
                        "reference entry must contain four bytes".into(),
                    ));
                }
                if unit.references.len() >= limits.max_references_per_unit {
                    return Err(fail(offset, "reference budget exceeded".into()));
                }
                let value = word(field.data, 0);
                let target = if kind == *b"SCRO" {
                    Reference::Form(value)
                } else {
                    Reference::Variable(value)
                };
                unit.references.push(ReferenceEntry { field, target });
            }
            _ => unreachable!("selected script fields"),
        }
    }
    if let Some(field) = pending_variable {
        return Err(fail(field.offset, "SLSD has no following SCVR".into()));
    }
    Ok(units)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn field(payload: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        payload.extend_from_slice(kind);
        payload.extend_from_slice(&(data.len() as u16).to_le_bytes());
        payload.extend_from_slice(data);
    }

    pub(crate) fn record(payload: Vec<u8>) -> plugin::Record {
        plugin::Record {
            header: plugin::RecordHeader {
                kind: *b"INFO",
                offset: 100,
                stored_size: payload.len() as u32,
                flags: 0,
                form_id: 0x123,
                revision: [0; 4],
                version: 15,
                trailing_bytes: [0; 2],
            },
            payload,
            integrity_issue: None,
        }
    }

    pub(crate) fn declaration(index: u32) -> [u8; 24] {
        let mut data = [0x7b; 24];
        data[..4].copy_from_slice(&index.to_le_bytes());
        data[16] = 0;
        data
    }

    #[test]
    fn embedded_tables_keep_authored_order_and_sparse_variable_indices() {
        let mut payload = Vec::new();
        field(&mut payload, b"SCHR", &[0; 20]);
        field(&mut payload, b"SLSD", &declaration(42));
        field(&mut payload, b"SCVR", b"reference_\xe9\0");
        field(&mut payload, b"SCRO", &0x05001234_u32.to_le_bytes());
        field(&mut payload, b"SCRV", &42_u32.to_le_bytes());
        // Repeating a form is allowed: indices refer to authored table positions.
        field(&mut payload, b"SCRO", &0x05001234_u32.to_le_bytes());
        // A conflicting duplicate also stays authored. Lookup returns the first
        // match, as in GetVariableInfo, without rewriting either declaration.
        field(&mut payload, b"SLSD", &declaration(42));
        field(&mut payload, b"SCVR", b"duplicate\0");
        field(&mut payload, b"SCHR", &[0; 20]);
        field(&mut payload, b"SLSD", &declaration(42));
        field(&mut payload, b"SCVR", b"another_script\0");
        field(&mut payload, b"SCRO", &7_u32.to_le_bytes());
        let record = record(payload);
        let units = decode(&record, "fixture.esm", Limits::default()).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].variables.len(), 2);
        assert_eq!(units[0].variables[1].name.data, b"duplicate\0");
        assert!(units[0].reference(0).is_none());
        assert_eq!(
            units[0].reference(1).unwrap().target,
            Reference::Form(0x05001234)
        );
        assert_eq!(
            units[0].reference(2).unwrap().target,
            Reference::Variable(42)
        );
        assert_eq!(
            units[0].reference(3).unwrap().target,
            Reference::Form(0x05001234)
        );
        assert!(units[0].reference(4).is_none());
        assert_eq!(units[1].reference(1).unwrap().target, Reference::Form(7));
        assert!(units[0].variable(1).is_none());
        let variable = units[0].variable(42).unwrap();
        assert_eq!(variable.name.data, b"reference_\xe9\0");
        assert_eq!(&variable.declaration.data[4..16], &[0x7b; 12]);
        assert!(std::ptr::eq(
            variable.name.data.as_ptr(),
            record.payload[variable.name.offset + 6..].as_ptr()
        ));
    }

    #[test]
    fn extended_fields_keep_the_actual_header_offset() {
        let mut payload = Vec::new();
        field(&mut payload, b"SCHR", &[0; 20]);
        let prefix = payload.len();
        field(&mut payload, b"XXXX", &4_u32.to_le_bytes());
        payload.extend_from_slice(b"SCRO\0\0");
        payload.extend_from_slice(&123_u32.to_le_bytes());
        let record = record(payload);
        let units = decode(&record, "fixture.esm", Limits::default()).unwrap();
        let reference = units[0].reference(1).unwrap();
        assert_eq!(reference.field.offset, prefix + 10);
        assert_eq!(reference.target, Reference::Form(123));
    }

    #[test]
    fn rejects_orphans_duplicates_truncated_names_and_resource_limits() {
        let bad: Vec<Vec<(&[u8; 4], Vec<u8>)>> = vec![
            vec![(b"SCRO", vec![0; 4])],
            vec![(b"SCHR", vec![0; 19])],
            vec![(b"SCHR", vec![0; 20]), (b"SCDA", vec![]), (b"SCDA", vec![])],
            vec![(b"SCHR", vec![0; 20]), (b"SCVR", b"orphan\0".to_vec())],
            vec![(b"SCHR", vec![0; 20]), (b"SLSD", vec![0; 23])],
            vec![(b"SCHR", vec![0; 20]), (b"SLSD", declaration(1).to_vec())],
            vec![
                (b"SCHR", vec![0; 20]),
                (b"SLSD", declaration(1).to_vec()),
                (b"SCHR", vec![0; 20]),
            ],
            vec![
                (b"SCHR", vec![0; 20]),
                (b"SLSD", declaration(1).to_vec()),
                (b"SCVR", vec![]),
            ],
            vec![
                (b"SCHR", vec![0; 20]),
                (b"SLSD", declaration(1).to_vec()),
                (b"SCVR", b"unclosed".to_vec()),
            ],
            vec![
                (b"SCHR", vec![0; 20]),
                (b"SLSD", declaration(1).to_vec()),
                (b"SCVR", b"a\0b\0".to_vec()),
            ],
            vec![(b"SCHR", vec![0; 20]), (b"SCRO", vec![0; 3])],
        ];
        for fields in bad {
            let mut payload = Vec::new();
            for (kind, data) in fields {
                field(&mut payload, kind, &data);
            }
            assert!(decode(&record(payload), "fixture.esm", Limits::default()).is_err());
        }
        let mut payload = Vec::new();
        field(&mut payload, b"SCHR", &[0; 20]);
        field(&mut payload, b"SLSD", &declaration(1));
        field(&mut payload, b"SCVR", b"a\0");
        field(&mut payload, b"SCRO", &[0; 4]);
        let record = record(payload);
        for limits in [
            Limits {
                max_fields: 1,
                ..Limits::default()
            },
            Limits {
                max_units: 0,
                ..Limits::default()
            },
            Limits {
                max_variables_per_unit: 0,
                ..Limits::default()
            },
            Limits {
                max_references_per_unit: 0,
                ..Limits::default()
            },
        ] {
            assert!(decode(&record, "fixture.esm", limits).is_err());
        }
        // Arbitrary source truncation must produce a result, never unwind.
        for end in 0..record.payload.len() {
            let short = self::record(record.payload[..end].to_vec());
            assert!(
                std::panic::catch_unwind(|| decode(&short, "fixture.esm", Limits::default()))
                    .is_ok()
            );
        }
    }
}
