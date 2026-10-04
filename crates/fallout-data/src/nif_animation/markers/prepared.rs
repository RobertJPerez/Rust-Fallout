//! Immutable selected text keys with an index into their physical source order.
use super::{Budget, ClockFields, Entry, Observation, pose};
use crate::{
    Result,
    nif_animation::{self, Data},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{cmp::Ordering, fmt};

#[derive(Clone, Copy, Debug)]
pub struct PreparationLimits {
    pub source: nif_animation::Limits,
    pub keys: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    /// Complete declared decoder allowance plus this preparation allowance.
    /// Borrowed input bytes have the separate source input cap.
    pub max_combined_retained_bytes: usize,
}
impl Default for PreparationLimits {
    fn default() -> Self {
        Self {
            source: super::Limits::default().source,
            keys: 65_536,
            array_bytes: 16 * 1024 * 1024,
            work_units: 128_000_000,
            max_combined_retained_bytes: 48 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct PreparationUsage {
    pub source_bytes: usize,
    pub animation_decodes: usize,
    pub full_source_sha256_traversals: usize,
    pub validated_keys: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    /// Historical source/index charge; these objects are dropped at preparation.
    pub decoded_source_retained_bytes: usize,
    pub retained_bytes: usize,
    pub combined_preparation_bytes: usize,
    pub sort_comparisons: usize,
    /// Own hash/metadata/key/copy/sort visits; framing/reference caps are separate.
    pub work_units: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct IntervalRequest {
    pub expected_sha256: [u8; 32],
    pub sequence: u32,
    pub source_start: f64,
    pub source_end: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct QueryLimits {
    pub entries: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    /// Live prepared selection plus charged output and ordinal scratch.
    pub max_combined_retained_bytes: usize,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            entries: 65_536,
            array_bytes: 4 * 1024 * 1024,
            work_units: 1_000_000,
            max_combined_retained_bytes: 20 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct QueryUsage {
    pub animation_decodes: usize,
    pub full_source_sha256_traversals: usize,
    pub full_key_validations: usize,
    pub boundary_probes: usize,
    pub matching_index_visits: usize,
    pub ordinal_sort_comparisons: usize,
    /// Includes released ordinal scratch, excluding allocator/spare capacity.
    pub charged_bytes: usize,
    pub output_bytes: usize,
    pub prepared_retained_bytes: usize,
    pub combined_retained_bytes: usize,
    pub work_units: usize,
}
#[derive(Debug, Serialize)]
pub struct IndexedObservation {
    pub observation: Observation,
    pub usage: QueryUsage,
}
#[derive(Clone, Copy, Debug)]
pub struct BatchLimits {
    pub query: QueryLimits,
    pub intervals: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub max_combined_retained_bytes: usize,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            query: Default::default(),
            intervals: 64,
            array_bytes: 64 * 1024 * 1024,
            work_units: 64_000_000,
            max_combined_retained_bytes: 80 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct IndexedBatch {
    pub contract: &'static str,
    pub source_sha256: String,
    pub sequence: u32,
    pub preparation: PreparationUsage,
    pub intervals: Vec<IndexedObservation>,
    pub charged_bytes: usize,
    pub output_bytes: usize,
    pub combined_retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

/// Only exact source preparation can create this authority. No input borrow,
/// mutable arrays, deserialization, public predecoded constructor or cursor.
pub struct PreparedSequence {
    digest: [u8; 32],
    source_sha256: String,
    sequence: pose::SourceSpan,
    text_keys: pose::SourceSpan,
    clock: ClockFields,
    name: Option<u32>,
    declared_keys: u32,
    keys: Vec<Entry>,
    time_order: Vec<usize>,
    usage: PreparationUsage,
}
impl fmt::Debug for PreparedSequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedSequence")
            .field("source_sha256", &self.source_sha256)
            .field("sequence", &self.sequence.block)
            .field("usage", &self.usage)
            .finish()
    }
}
impl PreparedSequence {
    pub fn prepare(
        bytes: &[u8],
        source: &str,
        expected_sha256: [u8; 32],
        sequence: u32,
        limits: PreparationLimits,
    ) -> Result<Self> {
        let mut budget = Budget {
            source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if bytes.len() > limits.source.input_bytes {
            return Err(budget.fail("source input byte budget exceeded"));
        }
        // Both independently bounded allowances coexist during construction.
        limits
            .source
            .array_bytes
            .checked_add(limits.array_bytes)
            .filter(|n| *n <= limits.max_combined_retained_bytes)
            .ok_or_else(|| budget.fail("prepared source/output admission exceeded"))?;
        budget.reserve::<Self>(1)?;
        budget.reserve::<u8>(3 * 64)?;
        budget.charge(bytes.len())?;
        let hash = Sha256::digest(bytes);
        let digest: [u8; 32] = hash.into();
        budget.charge(32)?;
        if digest != expected_sha256 {
            return Err(budget.fail("source SHA256 differs"));
        }
        let (index, decoded) = nif_animation::decode_with_limits(bytes, source, limits.source)?;
        budget.charge(
            decoded
                .blocks
                .len()
                .checked_mul(2)
                .ok_or_else(|| budget.fail("source metadata work overflow"))?,
        )?;
        // Decoder block hashes already identify the exact admitted spans.
        for block in &decoded.blocks {
            budget.charge(block.bytes)?;
        }
        let sequence_block = decoded
            .blocks
            .iter()
            .find(|b| b.block == sequence)
            .ok_or_else(|| {
                budget.fail(&format!(
                    "sequence {sequence} is not decoded NiControllerSequence"
                ))
            })?;
        let Data::ControllerSequence { sequence: selected } = &sequence_block.data else {
            return Err(budget.fail(&format!(
                "sequence {sequence} is not decoded NiControllerSequence"
            )));
        };
        let text_id = selected
            .text_keys
            .ok_or_else(|| budget.fail(&format!("sequence {sequence} has no text_keys link")))?;
        let text_block = decoded
            .blocks
            .iter()
            .find(|b| b.block == text_id)
            .ok_or_else(|| {
                budget.fail(&format!(
                    "sequence {sequence} text_keys {text_id} is not decoded NiTextKeyExtraData"
                ))
            })?;
        let Data::TextKeyExtraData { text_keys: text } = &text_block.data else {
            return Err(budget.fail(&format!(
                "sequence {sequence} text_keys {text_id} is not decoded NiTextKeyExtraData"
            )));
        };
        if text.keys.len() > limits.keys {
            return Err(budget.fail("prepared key count budget exceeded"));
        }
        budget.reserve::<Entry>(text.keys.len())?;
        budget.reserve::<usize>(text.keys.len())?;
        // Keep the index alive until every key/string is admitted and cloned.
        // It is returned by the existing decoder, never supplied by a caller.
        for (ordinal, key) in text.keys.iter().enumerate() {
            budget.charge(1)?;
            if !f32::from_bits(key.time_bits).is_finite() {
                return Err(budget.fail(&format!(
                    "text_keys {text_id} key {ordinal} has nonfinite source time"
                )));
            }
            let string = key.value.ok_or_else(|| {
                budget.fail(&format!(
                    "text_keys {text_id} key {ordinal} has no authored string"
                ))
            })?;
            let raw = index.strings.get(string as usize).ok_or_else(|| {
                budget.fail(&format!(
                    "text_keys {text_id} key {ordinal} string {string} is missing"
                ))
            })?;
            budget.reserve::<u8>(raw.len())?;
        }
        let mut keys = Vec::with_capacity(text.keys.len());
        for (ordinal, key) in text.keys.iter().enumerate() {
            let string_index = key
                .value
                .expect("every key string validated before cloning");
            let raw = &index.strings[string_index as usize];
            budget.charge(1)?;
            budget.charge(raw.len())?;
            keys.push(Entry {
                source_key_ordinal: ordinal,
                time_bits: key.time_bits,
                string_index,
                raw_string_bytes: raw.clone(),
            });
        }
        budget.charge(keys.len())?;
        let mut time_order = (0..keys.len()).collect::<Vec<_>>();
        let sort_comparisons = sort_ordinals(&mut time_order, &mut budget, |a, b| {
            f32::from_bits(keys[*a].time_bits)
                .partial_cmp(&f32::from_bits(keys[*b].time_bits))
                .expect("finite source times validated")
                .then_with(|| a.cmp(b))
        })?;
        let span = |block: &nif_animation::Block| pose::SourceSpan {
            block: block.block,
            offset: block.offset,
            bytes: block.bytes,
            sha256: block.sha256.clone(),
        };
        let retained_bytes = limits.array_bytes - budget.bytes;
        let combined_preparation_bytes = decoded
            .retained_bytes
            .checked_add(retained_bytes)
            .filter(|n| *n <= limits.max_combined_retained_bytes)
            .ok_or_else(|| budget.fail("prepared source/output storage budget exceeded"))?;
        Ok(Self {
            digest,
            source_sha256: format!("{hash:x}"),
            sequence: span(sequence_block),
            text_keys: span(text_block),
            clock: ClockFields {
                cycle_type: selected.cycle_type,
                frequency_bits: selected.frequency_bits,
                start_bits: selected.start_bits,
                stop_bits: selected.stop_bits,
                weight_bits: selected.weight_bits,
            },
            name: text.name,
            declared_keys: text.declared_keys,
            keys,
            time_order,
            usage: PreparationUsage {
                source_bytes: bytes.len(),
                animation_decodes: 1,
                full_source_sha256_traversals: 1,
                validated_keys: text.keys.len(),
                decoder_array_admission_bytes: limits.source.array_bytes,
                decoder_check_admission_units: limits.source.reference_checks,
                decoded_source_retained_bytes: decoded.retained_bytes,
                retained_bytes,
                combined_preparation_bytes,
                sort_comparisons,
                work_units: limits.work_units - budget.work,
            },
        })
    }

    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    pub fn sequence(&self) -> u32 {
        self.sequence.block
    }
    pub fn usage(&self) -> PreparationUsage {
        self.usage
    }

    fn validate(&self, request: IntervalRequest, budget: &mut Budget<'_>) -> Result<()> {
        budget.charge(33)?;
        if self.digest != request.expected_sha256 {
            return Err(budget.fail("prepared source SHA256 differs"));
        }
        if self.sequence.block != request.sequence {
            return Err(budget.fail("prepared sequence differs"));
        }
        if !request.source_start.is_finite()
            || !request.source_end.is_finite()
            || request.source_start > request.source_end
        {
            return Err(budget.fail("request interval must have finite ordered bounds"));
        }
        Ok(())
    }
    pub fn query(
        &self,
        request: IntervalRequest,
        limits: QueryLimits,
    ) -> Result<IndexedObservation> {
        let mut budget = Budget {
            source: &self.source_sha256,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        self.validate(request, &mut budget)?;
        let available = limits
            .max_combined_retained_bytes
            .checked_sub(self.usage.retained_bytes)
            .ok_or_else(|| budget.fail("prepared/output combined storage budget exceeded"))?;
        budget.bytes = budget.bytes.min(available);
        let admitted = budget.bytes;
        let (lower, lower_probes) = self.boundary(request.source_start, false, &mut budget)?;
        let (upper, upper_probes) = self.boundary(request.source_end, true, &mut budget)?;
        let count = upper - lower;
        if count > limits.entries {
            return Err(budget.fail("query entry count budget exceeded"));
        }
        budget.reserve::<IndexedObservation>(1)?;
        budget.reserve::<u8>(3 * 64)?;
        budget.reserve::<usize>(count)?;
        budget.reserve::<Entry>(count)?;
        budget.charge(count)?;
        for &ordinal in &self.time_order[lower..upper] {
            budget.charge(1)?;
            budget.reserve::<u8>(self.keys[ordinal].raw_string_bytes.len())?;
        }
        let mut ordinals = self.time_order[lower..upper].to_vec();
        let comparisons = sort_ordinals(&mut ordinals, &mut budget, usize::cmp)?;
        let mut entries = Vec::with_capacity(count);
        for ordinal in ordinals {
            let key = &self.keys[ordinal];
            budget.charge(1)?;
            budget.charge(key.raw_string_bytes.len())?;
            entries.push(Entry {
                source_key_ordinal: ordinal,
                time_bits: key.time_bits,
                string_index: key.string_index,
                raw_string_bytes: key.raw_string_bytes.clone(),
            });
        }
        let charged_bytes = admitted - budget.bytes;
        let output_bytes = charged_bytes - count * std::mem::size_of::<usize>();
        let combined_retained_bytes = self.usage.retained_bytes + charged_bytes;
        let work_units = limits.work_units - budget.work;
        Ok(IndexedObservation {
            observation: Observation {
                contract: super::CONTRACT,
                source_sha256: self.source_sha256.clone(),
                sequence: self.sequence.clone(),
                text_keys: self.text_keys.clone(),
                source_start_f64_bits: request.source_start.to_bits(),
                source_end_f64_bits: request.source_end.to_bits(),
                unapplied_sequence_clock_fields: copy_clock(&self.clock),
                text_keys_name_index: self.name,
                declared_source_keys: self.declared_keys,
                entries,
                retained_bytes: charged_bytes,
                decoded_source_retained_bytes: 0,
                combined_retained_bytes,
                work_units,
                retail_behavior_verified: false,
            },
            usage: QueryUsage {
                animation_decodes: 0,
                full_source_sha256_traversals: 0,
                full_key_validations: 0,
                boundary_probes: lower_probes + upper_probes,
                matching_index_visits: count * 2,
                ordinal_sort_comparisons: comparisons,
                charged_bytes,
                output_bytes,
                prepared_retained_bytes: self.usage.retained_bytes,
                combined_retained_bytes,
                work_units,
            },
        })
    }
    fn boundary(
        &self,
        endpoint: f64,
        upper: bool,
        budget: &mut Budget<'_>,
    ) -> Result<(usize, usize)> {
        let (mut left, mut right, mut probes) = (0, self.time_order.len(), 0);
        while left < right {
            budget.charge(1)?;
            probes += 1;
            let middle = left + (right - left) / 2;
            let time = f64::from(f32::from_bits(self.keys[self.time_order[middle]].time_bits));
            if time < endpoint || (upper && time == endpoint) {
                left = middle + 1;
            } else {
                right = middle;
            }
        }
        Ok((left, probes))
    }
    pub fn query_many(
        &self,
        requests: &[IntervalRequest],
        limits: BatchLimits,
    ) -> Result<IndexedBatch> {
        let mut budget = Budget {
            source: &self.source_sha256,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if requests.is_empty() || requests.len() > limits.intervals {
            return Err(budget.fail("indexed markers require a nonempty bounded interval list"));
        }
        for &request in requests {
            self.validate(request, &mut budget)?;
        }
        let available = limits
            .max_combined_retained_bytes
            .checked_sub(self.usage.retained_bytes)
            .ok_or_else(|| budget.fail("prepared/batch combined storage budget exceeded"))?;
        budget.bytes = budget.bytes.min(available);
        let admitted = budget.bytes;
        budget.reserve::<IndexedBatch>(1)?;
        // Per-query charges also include their headers: deliberately conservative.
        budget.reserve::<IndexedObservation>(requests.len())?;
        budget.reserve::<u8>(64)?;
        let mut intervals = Vec::with_capacity(requests.len());
        let mut scratch = 0usize;
        for &request in requests {
            let allowance = limits.query.array_bytes.min(budget.bytes);
            let result = self.query(
                request,
                QueryLimits {
                    array_bytes: allowance,
                    work_units: limits.query.work_units.min(budget.work),
                    max_combined_retained_bytes: limits
                        .query
                        .max_combined_retained_bytes
                        .min(self.usage.retained_bytes + allowance),
                    ..limits.query
                },
            )?;
            budget.reserve::<u8>(result.usage.charged_bytes)?;
            budget.charge(result.usage.work_units)?;
            scratch += result.usage.charged_bytes - result.usage.output_bytes;
            intervals.push(result);
        }
        let charged_bytes = admitted - budget.bytes;
        Ok(IndexedBatch {
            contract: "engineering-prepared-text-key-interval-batch-v1",
            source_sha256: self.source_sha256.clone(),
            sequence: self.sequence.block,
            preparation: self.usage,
            intervals,
            charged_bytes,
            output_bytes: charged_bytes - scratch,
            combined_retained_bytes: self.usage.retained_bytes + charged_bytes,
            work_units: limits.work_units - budget.work,
            retail_behavior_verified: false,
        })
    }
}
fn copy_clock(clock: &ClockFields) -> ClockFields {
    ClockFields {
        cycle_type: clock.cycle_type,
        frequency_bits: clock.frequency_bits,
        start_bits: clock.start_bits,
        stop_bits: clock.stop_bits,
        weight_bits: clock.weight_bits,
    }
}

// An in-place heap sort charges each comparison and swap before doing it.
// Refusal discards only the local index/scratch; prepared keys never mutate.
fn sort_ordinals(
    values: &mut [usize],
    budget: &mut Budget<'_>,
    compare: impl Fn(&usize, &usize) -> Ordering,
) -> Result<usize> {
    fn sift(
        values: &mut [usize],
        mut root: usize,
        end: usize,
        budget: &mut Budget<'_>,
        compare: &impl Fn(&usize, &usize) -> Ordering,
        comparisons: &mut usize,
    ) -> Result<()> {
        while root < end / 2 {
            let mut child = root * 2 + 1;
            if child + 1 < end {
                budget.charge(1)?;
                *comparisons += 1;
                if compare(&values[child], &values[child + 1]).is_lt() {
                    child += 1;
                }
            }
            budget.charge(1)?;
            *comparisons += 1;
            if !compare(&values[root], &values[child]).is_lt() {
                break;
            }
            budget.charge(1)?;
            values.swap(root, child);
            root = child;
        }
        Ok(())
    }
    let mut comparisons = 0;
    let length = values.len();
    for root in (0..length / 2).rev() {
        sift(values, root, length, budget, &compare, &mut comparisons)?;
    }
    for end in (1..values.len()).rev() {
        budget.charge(1)?;
        values.swap(0, end);
        sift(values, 0, end, budget, &compare, &mut comparisons)?;
    }
    Ok(comparisons)
}
