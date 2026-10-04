//! Physical source text-key observations over a closed source-time interval.
//! No event dispatch, names, playback clocks or cycle semantics are inferred.
use super::{Data, pose};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-source-text-key-interval-v1";

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub expected_sha256: [u8; 32],
    pub sequence: u32,
    pub source_start: f64,
    pub source_end: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source: super::Limits,
    pub array_bytes: usize,
    pub work_units: usize,
    /// Decoded source plus returned logical elements/strings, concurrently.
    pub max_combined_retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: super::Limits {
                blocks: 16_384,
                array_bytes: 32 * 1024 * 1024,
                reference_checks: 1_000_000,
                ..Default::default()
            },
            array_bytes: 4 * 1024 * 1024,
            work_units: 1_000_000,
            max_combined_retained_bytes: 36 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub source_key_ordinal: usize,
    pub time_bits: u32,
    pub string_index: u32,
    pub raw_string_bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct ClockFields {
    pub cycle_type: u32,
    pub frequency_bits: u32,
    pub start_bits: u32,
    pub stop_bits: u32,
    pub weight_bits: u32,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub contract: &'static str,
    pub source_sha256: String,
    pub sequence: pose::SourceSpan,
    pub text_keys: pose::SourceSpan,
    pub source_start_f64_bits: u64,
    pub source_end_f64_bits: u64,
    pub unapplied_sequence_clock_fields: ClockFields,
    pub text_keys_name_index: Option<u32>,
    pub declared_source_keys: u32,
    pub entries: Vec<Entry>,
    pub retained_bytes: usize,
    pub decoded_source_retained_bytes: usize,
    pub combined_retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

struct Budget<'a> {
    source: &'a str,
    bytes: usize,
    work: usize,
}
impl Budget<'_> {
    fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!(
            "{}: source text-key interval: {detail}",
            self.source
        ))
    }
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|size| self.bytes.checked_sub(size))
            .ok_or_else(|| self.fail("array storage budget exceeded"))?;
        Ok(())
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(count)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
}

/// All keys of the exact selected text-key block are validated, even outside
/// the requested interval. Physical order and duplicate times are preserved.
pub fn query(bytes: &[u8], source: &str, request: Request, limits: Limits) -> Result<Observation> {
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if !request.source_start.is_finite()
        || !request.source_end.is_finite()
        || request.source_start > request.source_end
    {
        return Err(budget.fail("request interval must have finite ordered bounds"));
    }
    if bytes.len() > limits.source.input_bytes {
        return Err(budget.fail("source input byte budget exceeded"));
    }
    if <[u8; 32]>::from(Sha256::digest(bytes)) != request.expected_sha256 {
        return Err(budget.fail("source SHA256 differs"));
    }
    let (index, decoded) = super::decode_with_limits(
        bytes,
        source,
        super::Limits {
            array_bytes: limits
                .source
                .array_bytes
                .min(limits.max_combined_retained_bytes),
            ..limits.source
        },
    )?;
    let available = limits
        .max_combined_retained_bytes
        .checked_sub(decoded.retained_bytes)
        .ok_or_else(|| budget.fail("combined source/output storage budget exceeded"))?;
    budget.bytes = budget.bytes.min(available);
    let admitted_output_bytes = budget.bytes;
    budget.charge(decoded.blocks.len() * 2)?;
    let sequence = decoded
        .blocks
        .iter()
        .find(|block| block.block == request.sequence)
        .and_then(|block| match &block.data {
            Data::ControllerSequence { sequence } => Some(sequence),
            _ => None,
        })
        .ok_or_else(|| {
            budget.fail(&format!(
                "sequence {} is not decoded NiControllerSequence",
                request.sequence
            ))
        })?;
    let text_id = sequence.text_keys.ok_or_else(|| {
        budget.fail(&format!(
            "sequence {} has no text_keys link",
            request.sequence
        ))
    })?;
    let text = decoded
        .blocks
        .iter()
        .find(|block| block.block == text_id)
        .and_then(|block| match &block.data {
            Data::TextKeyExtraData { text_keys } => Some(text_keys),
            _ => None,
        })
        .ok_or_else(|| {
            budget.fail(&format!(
                "sequence {} text_keys {text_id} is not decoded NiTextKeyExtraData",
                request.sequence
            ))
        })?;
    let inside = |time: u32| {
        let time = f64::from(f32::from_bits(time));
        time >= request.source_start && time <= request.source_end
    };
    budget.reserve::<Observation>(1)?;
    budget.reserve::<u8>(3 * 64)?;
    let mut count = 0usize;
    let mut copied_bytes = 0usize;
    for (ordinal, key) in text.keys.iter().enumerate() {
        budget.charge(1)?;
        if !f32::from_bits(key.time_bits).is_finite() {
            return Err(budget.fail(&format!(
                "text_keys {text_id} key {ordinal} has nonfinite source time"
            )));
        }
        let string_id = key.value.ok_or_else(|| {
            budget.fail(&format!(
                "text_keys {text_id} key {ordinal} has no authored string"
            ))
        })?;
        let raw = index.strings.get(string_id as usize).ok_or_else(|| {
            budget.fail(&format!(
                "text_keys {text_id} key {ordinal} string {string_id} is missing"
            ))
        })?;
        if inside(key.time_bits) {
            count = count
                .checked_add(1)
                .ok_or_else(|| budget.fail("entry count overflow"))?;
            copied_bytes = copied_bytes.checked_add(raw.len()).ok_or_else(|| {
                budget.fail(&format!(
                    "text_keys {text_id} key {ordinal} string byte count overflow"
                ))
            })?;
            // Charge each selected contribution before cloning any raw strings.
            budget.reserve::<Entry>(1).map_err(|_| {
                budget.fail(&format!(
                    "text_keys {text_id} key {ordinal} entry storage budget exceeded"
                ))
            })?;
            budget.reserve::<u8>(raw.len()).map_err(|_| {
                budget.fail(&format!(
                    "text_keys {text_id} key {ordinal} string {string_id} storage budget exceeded"
                ))
            })?;
        }
    }
    budget.charge(text.keys.len())?;
    budget.charge(copied_bytes)?;
    let mut entries = Vec::with_capacity(count);
    for (ordinal, key) in text.keys.iter().enumerate() {
        if inside(key.time_bits) {
            let string_id = key
                .value
                .expect("all selected source keys validated before allocation");
            entries.push(Entry {
                source_key_ordinal: ordinal,
                time_bits: key.time_bits,
                string_index: string_id,
                raw_string_bytes: index.strings[string_id as usize].clone(),
            });
        }
    }
    let retained_bytes = admitted_output_bytes - budget.bytes;
    Ok(Observation {
        contract: CONTRACT,
        source_sha256: format!("{:x}", Sha256::digest(bytes)),
        sequence: pose::span(bytes, &index, request.sequence),
        text_keys: pose::span(bytes, &index, text_id),
        source_start_f64_bits: request.source_start.to_bits(),
        source_end_f64_bits: request.source_end.to_bits(),
        unapplied_sequence_clock_fields: ClockFields {
            cycle_type: sequence.cycle_type,
            frequency_bits: sequence.frequency_bits,
            start_bits: sequence.start_bits,
            stop_bits: sequence.stop_bits,
            weight_bits: sequence.weight_bits,
        },
        text_keys_name_index: text.name,
        declared_source_keys: text.declared_keys,
        entries,
        retained_bytes,
        decoded_source_retained_bytes: decoded.retained_bytes,
        combined_retained_bytes: decoded.retained_bytes + retained_bytes,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
