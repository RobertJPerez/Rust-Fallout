//! Metadata only: command execution is deliberately absent until NV bytecode and
//! native behavior have independent test evidence. See xEdit's pinned FNV schema.
use crate::{
    Result, malformed,
    plugin::{RecordHeader, Subrecord, signature},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct CallSite {
    pub form_id: u32,
    pub record_kind: String,
    pub file_offset: u64,
    pub decoded_payload_offset: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub occurrences: u64,
    pub examples: Vec<CallSite>,
}

#[derive(Debug, Default, Serialize)]
pub struct ScriptInventory {
    pub headers: u64,
    pub compiled_bodies: u64,
    pub compiled_bytes: u64,
    pub source_text_fields: u64,
    pub explicit_form_references: u64,
    pub local_variable_references: u64,
    pub script_types: BTreeMap<u16, u64>,
    pub condition_functions: BTreeMap<u16, Usage>,
    pub condition_lengths: BTreeMap<usize, u64>,
    pub script_header_lengths: BTreeMap<usize, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScriptReference {
    pub caller: CallSite,
    pub target_raw_form: u32,
}

impl ScriptInventory {
    pub fn observe(
        &mut self,
        header: &RecordHeader,
        sub: &Subrecord<'_>,
        source: &str,
    ) -> Result<Option<ScriptReference>> {
        let site = || CallSite {
            form_id: header.form_id,
            record_kind: signature(header.kind),
            file_offset: header.offset,
            decoded_payload_offset: sub.payload_offset,
        };
        let fail = |reason| malformed(source, header.offset, reason);
        match &sub.kind {
            b"SCHR" => {
                self.headers += 1;
                *self
                    .script_header_lengths
                    .entry(sub.data.len())
                    .or_default() += 1;
                if sub.data.len() != 20 {
                    return Err(fail("unsupported SCHR layout"));
                }
                let kind = u16::from_le_bytes([sub.data[16], sub.data[17]]);
                *self.script_types.entry(kind).or_default() += 1;
            }
            b"SCDA" => {
                self.compiled_bodies += 1;
                self.compiled_bytes += sub.data.len() as u64;
            }
            b"SCTX" => self.source_text_fields += 1,
            b"SCRO" => {
                if sub.data.len() != 4 {
                    return Err(fail("SCRO FormID is not four bytes"));
                }
                self.explicit_form_references += 1;
                return Ok(Some(ScriptReference {
                    caller: site(),
                    target_raw_form: u32::from_le_bytes(sub.data.try_into().expect("checked size")),
                }));
            }
            b"SCRV" => {
                if sub.data.len() != 4 {
                    return Err(fail("SCRV variable index is not four bytes"));
                }
                self.local_variable_references += 1;
            }
            b"CTDA" => {
                *self.condition_lengths.entry(sub.data.len()).or_default() += 1;
                if ![20, 24, 28].contains(&sub.data.len()) {
                    return Err(fail("unsupported CTDA layout"));
                }
                let id = u16::from_le_bytes([sub.data[8], sub.data[9]]);
                let usage = self.condition_functions.entry(id).or_default();
                usage.occurrences += 1;
                if usage.examples.len() < 3 {
                    usage.examples.push(site());
                }
            }
            _ => {}
        }
        Ok(None)
    }
}
