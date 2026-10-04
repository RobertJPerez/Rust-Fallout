//! Bounded authored microfixture bytes. Existing decoders verify the framing;
//! neither generation nor a structural round trip proves original execution.
use super::{copy_probe, trace};
use crate::identity::{CampaignId, Value};
use fallout_data::{plugin, script_units};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Cursor, num::NonZeroU64};

pub const PLUGIN_NAME: &str = "RustFalloutProbe.esm";
pub const SCRIPT_ID: u32 = 0x800;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub purpose: trace::Operation,
    pub campaign: CampaignId,
    pub activation: NonZeroU64,
    pub input: Value,
    pub destination_before: Value,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("probe fixture request is invalid: {0}")]
    Invalid(&'static str),
    #[error("probe fixture byte budget exceeded")]
    Capacity,
    #[error(transparent)]
    Data(#[from] fallout_data::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
}
#[derive(Debug, Serialize)]
pub struct Shape {
    pub purpose: trace::Operation,
    pub event_id: u16,
    pub begin_scda_offset: u32,
    pub operation_scda_offset: u32,
    pub caller: trace::Caller,
    pub operand_bits: Vec<trace::Word>,
    pub required_output: &'static str,
    pub original_expected_output_generated: bool,
}
#[derive(Debug)]
pub struct Artifact {
    pub plugin: Vec<u8>,
    pub compiled: Vec<u8>,
    pub plugin_sha256: String,
    pub compiled_sha256: String,
    pub shape: Shape,
    pub copy_request: copy_probe::Request,
}
fn number(value: &Value) -> Result<u64, Error> {
    let Value::Number { bits } = value else {
        return Err(Error::Invalid("explicit Number inputs required"));
    };
    if !f64::from_bits(*bits).is_finite() {
        return Err(Error::Invalid("finite numeric inputs required"));
    }
    Ok(*bits)
}
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}

/// One fixed-size source carrier with no masters or references. Retail loading,
/// activation and variable initialization need independent recorder evidence.
pub fn generate(request: &Request, maximum_bytes: usize) -> Result<Artifact, Error> {
    if request.schema_version != 1 {
        return Err(Error::Invalid("schema version"));
    }
    if !matches!(
        request.purpose,
        trace::Operation::Assignment | trace::Operation::Conversion
    ) {
        return Err(Error::Invalid(
            "only assignment and conversion fixture purposes are implemented",
        ));
    }
    CampaignId::from_bytes(request.campaign.bytes())?;
    let input = number(&request.input)?;
    number(&request.destination_before)?;
    let conversion = request.purpose == trace::Operation::Conversion;
    // Existing source evidence: script-name preamble, BEGIN event/header distance,
    // one set-to envelope (destination index2, own read index1), then END.
    let compiled = vec![
        0x1d,
        0,
        0,
        0,
        0x10,
        0,
        6,
        0,
        0,
        0,
        16,
        0,
        0,
        0,
        0x15,
        0,
        8,
        0,
        if conversion { b's' } else { b'f' },
        2,
        0,
        3,
        0,
        b'f',
        1,
        0,
        0x11,
        0,
        0,
        0,
    ];
    let name = if conversion {
        b"RustFalloutProbeConversion\0".as_slice()
    } else {
        b"RustFalloutProbeCopy\0".as_slice()
    };
    let mut payload = field(b"EDID", name);
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    schr[12..16].copy_from_slice(&2_u32.to_le_bytes());
    payload.extend(field(b"SCHR", &schr));
    payload.extend(field(b"SCDA", &compiled));
    for (index, type_byte, name) in [
        (1_u32, 0, b"probe_input\0".as_slice()),
        (2, u8::from(conversion), b"probe_output\0".as_slice()),
    ] {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        declaration[16] = type_byte;
        payload.extend(field(b"SLSD", &declaration));
        payload.extend(field(b"SCVR", name));
    }
    let hedr = [
        1.34_f32.to_le_bytes().as_slice(),
        &1_u32.to_le_bytes(),
        &0x801_u32.to_le_bytes(),
    ]
    .concat();
    let header = record(b"TES4", 0, 1, &field(b"HEDR", &hedr));
    let script = record(b"SCPT", SCRIPT_ID, 0, &payload);
    let group = [
        b"GRUP".as_slice(),
        &((script.len() + 24) as u32).to_le_bytes(),
        b"SCPT",
        &[0; 12],
        &script,
    ]
    .concat();
    let bytes = header.len() + group.len();
    if bytes > maximum_bytes {
        return Err(Error::Capacity);
    }
    let plugin = [header, group].concat();
    let mut scripts = 0;
    plugin::visit(
        &mut Cursor::new(&plugin),
        plugin.len() as u64,
        PLUGIN_NAME,
        plugin::Limits {
            max_record_bytes: 4096,
            max_records: 2,
            max_decoded_bytes: 8192,
            ..Default::default()
        },
        |event| {
            if let plugin::Event::Record(record) = event
                && record.header.kind == *b"SCPT"
            {
                let units = script_units::decode(
                    record,
                    PLUGIN_NAME,
                    script_units::Limits {
                        max_fields: 16,
                        max_units: 1,
                        max_variables_per_unit: 2,
                        max_references_per_unit: 0,
                    },
                )?;
                if units.len() != 1
                    || units[0].declared_compiled_bytes() != 30
                    || units[0].declared_variables() != 2
                    || !units[0].references.is_empty()
                    || units[0].compiled.as_ref().map(|field| field.data)
                        != Some(compiled.as_slice())
                {
                    return Err(fallout_data::Error::Unsupported(
                        "generated source carrier failed existing decoder round trip".into(),
                    ));
                }
                scripts += 1;
            }
            Ok(())
        },
    )?;
    if scripts != 1 {
        return Err(Error::Invalid("one authored source script required"));
    }
    Ok(Artifact {
        plugin_sha256: format!("{:x}", Sha256::digest(&plugin)),
        compiled_sha256: format!("{:x}", Sha256::digest(&compiled)),
        plugin,
        compiled,
        shape: Shape {
            purpose: request.purpose,
            event_id: 0,
            begin_scda_offset: 4,
            operation_scda_offset: 14,
            caller: trace::Caller {
                calling_reference: None,
                containing_reference: None,
                target: None,
                activation: request.activation.get(),
            },
            operand_bits: vec![trace::Word::binary64(input)],
            required_output: "observed_local_write_or_error",
            original_expected_output_generated: false,
        },
        copy_request: copy_probe::Request {
            schema_version: 1,
            campaign: request.campaign,
            activation: request.activation,
            initializers: vec![
                copy_probe::Initializer {
                    index: 1,
                    value: request.input.clone(),
                },
                copy_probe::Initializer {
                    index: 2,
                    value: request.destination_before.clone(),
                },
            ],
        },
    })
}
