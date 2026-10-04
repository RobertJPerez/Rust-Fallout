//! Explicit engineering cubic component math over borrowed compact source.
//! This knot/time model does not establish retail interpolation or poses.
use super::{Block, Data, components};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CONTRACT: &str = "engineering-open-uniform-cubic-components-v1";
const MAX_COUNT: usize = 2_000_000;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub validation_units: usize,
    pub sampling_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            validation_units: 16_000_000,
            sampling_units: 16_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub validation_units: usize,
    pub sampling_units: usize,
}
pub struct Budget {
    limits: Limits,
    validation_left: usize,
    sampling_left: usize,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            validation_left: limits.validation_units,
            sampling_left: limits.sampling_units,
        }
    }
    pub fn usage(&self) -> Usage {
        Usage {
            validation_units: self.limits.validation_units - self.validation_left,
            sampling_units: self.limits.sampling_units - self.sampling_left,
        }
    }
    fn validate(&mut self, units: usize) -> Result<()> {
        self.validation_left = self
            .validation_left
            .checked_sub(units)
            .ok_or_else(|| unsupported("validation work budget exceeded"))?;
        Ok(())
    }
    fn sample(&mut self, units: usize) -> Result<()> {
        self.sampling_left = self
            .sampling_left
            .checked_sub(units)
            .ok_or_else(|| unsupported("sampling work budget exceeded"))?;
        Ok(())
    }
}
fn unsupported(reason: &str) -> Error {
    Error::Unsupported(format!("engineering spline sampling: {reason}"))
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Translation,
    Scale,
    Float,
    Point3,
    /// Four independent raw WXYZ components; no quaternion/orientation policy.
    RotationComponents,
}
impl Channel {
    fn width(self) -> usize {
        match self {
            Self::Scale | Self::Float => 1,
            Self::Translation | Self::Point3 => 3,
            Self::RotationComponents => 4,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Identity {
    pub block: u32,
    pub block_type: &'static str,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
}
struct BorrowedIdentity<'a> {
    block: u32,
    block_type: &'static str,
    offset: usize,
    bytes: usize,
    sha256: &'a str,
}
impl BorrowedIdentity<'_> {
    fn validate(&self) -> Result<()> {
        if self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.offset.checked_add(self.bytes).is_none()
        {
            return Err(unsupported("source identity/span/hash is contradictory"));
        }
        Ok(())
    }
    fn owned(&self) -> Identity {
        Identity {
            block: self.block,
            block_type: self.block_type,
            offset: self.offset,
            bytes: self.bytes,
            sha256: self.sha256.into(),
        }
    }
}
impl<'a> From<&'a Block> for BorrowedIdentity<'a> {
    fn from(block: &'a Block) -> Self {
        Self {
            block: block.block,
            block_type: block.block_type,
            offset: block.offset,
            bytes: block.bytes,
            sha256: &block.sha256,
        }
    }
}

/// Constructed only after shape, links, interval and work admission. It borrows
/// exactly one compact window and never expands controls or allocates knots.
pub struct Window<'a> {
    identities: [BorrowedIdentity<'a>; 3],
    channel: Channel,
    controls: &'a [i16],
    count: usize,
    handle: u32,
    window_offset: usize,
    start_bits: u32,
    stop_bits: u32,
    offset_bits: u32,
    half_range_bits: u32,
    window_sha256: String,
}
#[derive(Debug, Serialize)]
pub struct Sample {
    pub requested_time_f64_bits: u64,
    pub parameter_f64_bits: u64,
    pub source_control_indices: [usize; 4],
    /// At most four scalar results; WXYZ remains raw components.
    pub evaluated_f64_bits: Vec<u64>,
}
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub contract: &'static str,
    pub source_blocks: [Identity; 3],
    pub channel: Channel,
    pub basis_count: usize,
    pub source_handle: u32,
    pub window_offset: usize,
    pub window_scalars: usize,
    pub window_sha256: String,
    pub start_bits: u32,
    pub stop_bits: u32,
    pub offset_bits: u32,
    pub half_range_bits: u32,
    pub sample: Sample,
    pub work: Usage,
    pub runtime_ready: bool,
    pub retail_behavior_verified: bool,
}

// Fixed copied metadata only, with no control-point copies or repaired links.
struct Fields<'a> {
    identity: BorrowedIdentity<'a>,
    start: u32,
    stop: u32,
    data: Option<u32>,
    basis: Option<u32>,
    handle: u32,
    offset: u32,
    half_range: u32,
}
fn selected<'a>(
    source: &'a components::Source,
    id: u32,
    channel: Channel,
    budget: &mut Budget,
) -> Result<Fields<'a>> {
    let mut found = None;
    for block in &source.source.splines.blocks {
        budget.validate(1)?;
        if block.block != id {
            continue;
        }
        let Data::CompactTransform {
            start_bits,
            stop_bits,
            spline_data,
            basis_data,
            translation_handle,
            rotation_handle,
            scale_handle,
            translation_offset_bits,
            translation_half_range_bits,
            rotation_offset_bits,
            rotation_half_range_bits,
            scale_offset_bits,
            scale_half_range_bits,
            ..
        } = &block.data
        else {
            return Err(unsupported("selected block is not a compact interpolator"));
        };
        if block.block_type != "NiBSplineCompTransformInterpolator" || block.bytes != 84 {
            return Err(unsupported(
                "compact transform identity/span is contradictory",
            ));
        }
        let (handle, offset, half_range) = match channel {
            Channel::Translation => (
                *translation_handle,
                *translation_offset_bits,
                *translation_half_range_bits,
            ),
            Channel::Scale => (*scale_handle, *scale_offset_bits, *scale_half_range_bits),
            Channel::RotationComponents => (
                *rotation_handle,
                *rotation_offset_bits,
                *rotation_half_range_bits,
            ),
            _ => return Err(unsupported("requested channel does not match interpolator")),
        };
        if found.is_some() {
            return Err(unsupported("duplicate selected block identity"));
        }
        found = Some(Fields {
            identity: block.into(),
            start: *start_bits,
            stop: *stop_bits,
            data: *spline_data,
            basis: *basis_data,
            handle,
            offset,
            half_range,
        });
    }
    for block in &source.components.blocks {
        budget.validate(1)?;
        if block.block != id {
            continue;
        }
        if found.is_some() {
            return Err(unsupported("duplicate selected block identity"));
        }
        let (start, stop, data, basis, handle, offset, half_range, kind, bytes) =
            match (&block.data, channel) {
                (
                    components::Data::CompactFloat {
                        start_bits,
                        stop_bits,
                        spline_data,
                        basis_data,
                        handle,
                        float_offset_bits,
                        float_half_range_bits,
                        ..
                    },
                    Channel::Float,
                ) => (
                    *start_bits,
                    *stop_bits,
                    *spline_data,
                    *basis_data,
                    *handle,
                    *float_offset_bits,
                    *float_half_range_bits,
                    "NiBSplineCompFloatInterpolator",
                    32,
                ),
                (
                    components::Data::CompactPoint3 {
                        start_bits,
                        stop_bits,
                        spline_data,
                        basis_data,
                        handle,
                        position_offset_bits,
                        position_half_range_bits,
                        ..
                    },
                    Channel::Point3,
                ) => (
                    *start_bits,
                    *stop_bits,
                    *spline_data,
                    *basis_data,
                    *handle,
                    *position_offset_bits,
                    *position_half_range_bits,
                    "NiBSplineCompPoint3Interpolator",
                    40,
                ),
                _ => return Err(unsupported("requested channel does not match interpolator")),
            };
        if block.block_type != kind || block.bytes != bytes {
            return Err(unsupported(
                "compact component identity/span is contradictory",
            ));
        }
        found = Some(Fields {
            identity: BorrowedIdentity {
                block: block.block,
                block_type: block.block_type,
                offset: block.offset,
                bytes: block.bytes,
                sha256: &block.sha256,
            },
            start,
            stop,
            data,
            basis,
            handle,
            offset,
            half_range,
        });
    }
    found
        .ok_or_else(|| unsupported("selected block is not in the decoded compact source catalogue"))
}
fn target<'a>(blocks: &'a [Block], id: Option<u32>, budget: &mut Budget) -> Result<&'a Block> {
    let id = id.ok_or_else(|| unsupported("missing data or basis link"))?;
    let mut found = None;
    for block in blocks {
        budget.validate(1)?;
        if block.block == id {
            if found.is_some() {
                return Err(unsupported("duplicate target block identity"));
            }
            found = Some(block);
        }
    }
    found.ok_or_else(|| unsupported("data or basis link is unresolved in decoded source"))
}
pub fn prepare<'a>(
    source: &'a components::Source,
    block: u32,
    channel: Channel,
    budget: &mut Budget,
) -> Result<Window<'a>> {
    budget.validate(16)?;
    let fields = selected(source, block, channel, budget)?;
    fields.identity.validate()?;
    let start = f64::from(f32::from_bits(fields.start));
    let stop = f64::from(f32::from_bits(fields.stop));
    if !start.is_finite() || !stop.is_finite() || stop <= start {
        return Err(unsupported(
            "source interval must be finite and strictly increasing",
        ));
    }
    if !f32::from_bits(fields.offset).is_finite() || !f32::from_bits(fields.half_range).is_finite()
    {
        return Err(unsupported("nonfinite compact scaling parameters"));
    }
    // XML's ushort-valued invalid handle is stored in a uint source field.
    // u32::MAX is preserved and checked as a window offset, never reinterpreted.
    if fields.handle == 65535 {
        return Err(unsupported("absent compact handle 65535"));
    }
    let data = target(&source.source.splines.blocks, fields.data, budget)?;
    let basis = target(&source.source.splines.blocks, fields.basis, budget)?;
    let identities = [fields.identity, data.into(), basis.into()];
    for identity in &identities {
        identity.validate()?;
    }
    let Data::Basis { num_control_points } = basis.data else {
        return Err(unsupported("basis target has wrong decoded kind"));
    };
    if basis.block_type != "NiBSplineBasisData" || basis.bytes != 4 {
        return Err(unsupported("basis identity/span is contradictory"));
    }
    let count = num_control_points as usize;
    if !(4..=MAX_COUNT).contains(&count) {
        return Err(unsupported(
            "cubic basis count must be between 4 and 2000000",
        ));
    }
    let Data::ControlPoints {
        declared_float_count,
        float_bits,
        declared_compact_count,
        compact,
    } = &data.data
    else {
        return Err(unsupported("data target has wrong decoded kind"));
    };
    if data.block_type != "NiBSplineData"
        || *declared_float_count as usize != float_bits.len()
        || *declared_compact_count as usize != compact.len()
        || float_bits.len() > MAX_COUNT
        || compact.len() > MAX_COUNT
    {
        return Err(unsupported(
            "control array cardinality is contradictory or exceeds limit",
        ));
    }
    let compact_start = float_bits
        .len()
        .checked_mul(4)
        .and_then(|v| v.checked_add(8))
        .ok_or_else(|| unsupported("control array span overflow"))?;
    if compact
        .len()
        .checked_mul(2)
        .and_then(|v| v.checked_add(compact_start))
        != Some(data.bytes)
    {
        return Err(unsupported("control array span is contradictory"));
    }
    let scalars = count
        .checked_mul(channel.width())
        .ok_or_else(|| unsupported("compact window size overflow"))?;
    let begin = fields.handle as usize;
    let end = begin
        .checked_add(scalars)
        .ok_or_else(|| unsupported("compact window end overflow"))?;
    let controls = compact
        .get(begin..end)
        .ok_or_else(|| unsupported("compact window exceeds source array"))?;
    let window_offset = begin
        .checked_mul(2)
        .and_then(|v| v.checked_add(compact_start))
        .and_then(|v| v.checked_add(data.offset))
        .ok_or_else(|| unsupported("compact window offset overflow"))?;
    // Admission precedes hashing or allocating the fixed 64-byte digest.
    budget.validate(scalars)?;
    let mut hash = Sha256::new();
    for control in controls {
        hash.update(control.to_le_bytes());
    }
    Ok(Window {
        identities,
        channel,
        controls,
        count,
        handle: fields.handle,
        window_offset,
        start_bits: fields.start,
        stop_bits: fields.stop,
        offset_bits: fields.offset,
        half_range_bits: fields.half_range,
        window_sha256: format!("{hash:x}", hash = hash.finalize()),
    })
}
fn knot(index: usize, count: usize) -> f64 {
    index.saturating_sub(3).min(count - 3) as f64
}
impl Window<'_> {
    /// A source adapter may copy only this already admitted immutable window.
    pub(crate) fn compact_controls(&self) -> &[i16] {
        self.controls
    }

    pub fn sample(&self, time: f64, budget: &mut Budget) -> Result<Sample> {
        let width = self.channel.width();
        // All time/parameter, four unpack and six blend operations admitted
        // before evaluating any component or allocating the <=4 result scalars.
        budget.sample(17 + 10 * width)?;
        let start = f64::from(f32::from_bits(self.start_bits));
        let stop = f64::from(f32::from_bits(self.stop_bits));
        if !time.is_finite() {
            return Err(unsupported("nonfinite requested source time"));
        }
        if time < start || time > stop {
            return Err(unsupported("requested time would extrapolate"));
        }
        let end = (self.count - 3) as f64;
        let parameter = if time == start {
            0.0
        } else if time == stop {
            end
        } else {
            ((time - start) / (stop - start)) * end
        };
        let span = (parameter.floor() as usize + 3).min(self.count - 1);
        let indices = [span - 3, span - 2, span - 1, span];
        let offset = f64::from(f32::from_bits(self.offset_bits));
        let range = f64::from(f32::from_bits(self.half_range_bits));
        let mut scratch = [[0.0; 4]; 4];
        for (row, index) in scratch.iter_mut().zip(indices) {
            for (component, value) in row.iter_mut().enumerate().take(width) {
                *value = offset
                    + (f64::from(self.controls[index * width + component]) / 32767.0) * range;
            }
        }
        // Open uniform clamped degree3 local de Boor. Endpoint values use the
        // source endpoint directly; interior rounding never repairs source time.
        let values = if time == start {
            scratch[0]
        } else if time == stop {
            scratch[3]
        } else {
            for level in 1..=3 {
                for j in (level..=3).rev() {
                    let index = span - 3 + j;
                    let left = knot(index, self.count);
                    let right = knot(index + 4 - level, self.count);
                    let alpha = (parameter - left) / (right - left);
                    let previous = scratch[j - 1];
                    for (value, prior) in scratch[j].iter_mut().zip(previous).take(width) {
                        *value = (1.0 - alpha) * prior + alpha * *value;
                    }
                }
            }
            scratch[3]
        };
        if values[..width].iter().any(|value| !value.is_finite()) {
            return Err(unsupported("nonfinite engineering result"));
        }
        Ok(Sample {
            requested_time_f64_bits: time.to_bits(),
            parameter_f64_bits: parameter.to_bits(),
            source_control_indices: indices,
            evaluated_f64_bits: values[..width].iter().map(|v| v.to_bits()).collect(),
        })
    }
}
pub fn evaluate(
    source: &components::Source,
    block: u32,
    channel: Channel,
    time: f64,
    budget: &mut Budget,
) -> Result<Diagnostic> {
    let window = prepare(source, block, channel, budget)?;
    window.observe(time, budget)
}

impl Window<'_> {
    /// Preserve the existing diagnostic and work scope while a private source
    /// adapter separately admits copied controls and composed pose output.
    pub(crate) fn observe(self, time: f64, budget: &mut Budget) -> Result<Diagnostic> {
        let sample = self.sample(time, budget)?;
        Ok(Diagnostic {
            contract: CONTRACT,
            source_blocks: self.identities.each_ref().map(|v| v.owned()),
            channel: self.channel,
            basis_count: self.count,
            source_handle: self.handle,
            window_offset: self.window_offset,
            window_scalars: self.controls.len(),
            window_sha256: self.window_sha256,
            start_bits: self.start_bits,
            stop_bits: self.stop_bits,
            offset_bits: self.offset_bits,
            half_range_bits: self.half_range_bits,
            sample,
            work: budget.usage(),
            runtime_ready: false,
            retail_behavior_verified: false,
        })
    }
}
