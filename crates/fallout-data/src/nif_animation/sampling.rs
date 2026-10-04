//! Explicit-time engineering math over unmodified source keys. This is not an
//! original interpolation, clock, pose, event or retail playback contract.
use super::keyframe::{Block, Group, Key};
use crate::{Error, Result};
use serde::Serialize;

pub const CONTRACT: &str = "engineering-linear-constant-components-v1";
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub validation_work: usize,
    pub sampling_work: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            validation_work: 16_000_000,
            sampling_work: 16_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub validation_units: usize,
    pub sampling_units: usize,
}
pub struct Budget {
    initial: Limits,
    validation_left: usize,
    sampling_left: usize,
}
impl Budget {
    pub fn new(limits: Limits) -> Self {
        Self {
            initial: limits,
            validation_left: limits.validation_work,
            sampling_left: limits.sampling_work,
        }
    }
    fn validate(&mut self, count: usize) -> Result<()> {
        self.validation_left = self
            .validation_left
            .checked_sub(count)
            .ok_or_else(|| unsupported("sampling validation-work budget exceeded"))?;
        Ok(())
    }
    fn sample(&mut self, count: usize) -> Result<()> {
        self.sampling_left = self
            .sampling_left
            .checked_sub(count)
            .ok_or_else(|| unsupported("sampling request-work budget exceeded"))?;
        Ok(())
    }
    /// Consumer catalogue lookup visits share the validation allowance.
    pub fn source_block_visit(&mut self) -> Result<()> {
        self.validate(1)
    }
    pub fn usage(&self) -> Usage {
        Usage {
            validation_units: self.initial.validation_work - self.validation_left,
            sampling_units: self.initial.sampling_work - self.sampling_left,
        }
    }
}
fn unsupported(reason: &str) -> Error {
    Error::Unsupported(format!("engineering component sampling: {reason}"))
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Linear,
    Constant,
}
/// A validated borrowed view cannot outlive or modify the source keys. No
/// sorting, normalization, time deduplication or key/value substitution occurs.
pub struct ValidatedGroup<'a, const N: usize> {
    keys: &'a [Key<N>],
    interpolation: Interpolation,
}
pub fn prepare<'a, const N: usize>(
    group: &'a Group<N>,
    budget: &mut Budget,
) -> Result<Option<ValidatedGroup<'a, N>>> {
    budget.validate(1)?;
    if N != 1 && N != 3 {
        return Err(unsupported("only scalar/vector source groups are admitted"));
    }
    if group.keys.len() > 2_000_000 || group.declared_keys as usize != group.keys.len() {
        return Err(unsupported(
            "source key cardinality is contradictory or exceeds count budget",
        ));
    }
    if group.keys.is_empty() {
        if group.key_type.is_some() {
            return Err(unsupported(
                "empty source group has a contradictory present tag",
            ));
        }
        return Ok(None);
    }
    let interpolation = match group.key_type {
        Some(1) => Interpolation::Linear,
        Some(5) => Interpolation::Constant,
        _ => return Err(unsupported("source interpolation tag is unadmitted")),
    };
    // Admit every examined time/value word before the complete validation walk.
    budget.validate(group.keys.len() * (1 + N))?;
    let mut previous = None;
    for key in &group.keys {
        if key.forward_bits.is_some() || key.backward_bits.is_some() || key.tbc_bits.is_some() {
            return Err(unsupported(
                "source key fields contradict linear/constant layout",
            ));
        }
        let time = f32::from_bits(key.time_bits);
        if !time.is_finite()
            || key
                .value_bits
                .iter()
                .any(|&bits| !f32::from_bits(bits).is_finite())
        {
            return Err(unsupported("nonfinite source key time/value"));
        }
        if previous.is_some_and(|old| time <= old) {
            return Err(unsupported("source key times are not strictly increasing"));
        }
        previous = Some(time);
    }
    Ok(Some(ValidatedGroup {
        keys: &group.keys,
        interpolation,
    }))
}
#[derive(Debug, Serialize)]
#[serde(bound(serialize = "[u64; N]: Serialize, [u32; N]: Serialize"))]
pub struct Sample<const N: usize> {
    pub source_key_indices: [usize; 2],
    pub alpha_f64_bits: u64,
    pub evaluated_f64_bits: [u64; N],
    /// Present at an exact source key or for a held constant source value.
    pub source_value_bits: Option<[u32; N]>,
    pub interpolation: Interpolation,
}
impl<const N: usize> ValidatedGroup<'_, N> {
    pub fn sample(&self, time: f64, budget: &mut Budget) -> Result<Sample<N>> {
        budget.sample(1)?;
        if !time.is_finite() {
            return Err(unsupported("nonfinite requested source time"));
        }
        let key_time = |id: usize| f64::from(f32::from_bits(self.keys[id].time_bits));
        let last = self.keys.len() - 1;
        budget.sample(2)?;
        let first_time = key_time(0);
        let last_time = key_time(last);
        if time < first_time || time > last_time {
            return Err(unsupported(
                "requested time would extrapolate outside source key range",
            ));
        }
        let (mut left, mut right) = (0, last);
        if time == first_time {
            right = 0;
        } else if time == last_time {
            left = last;
        } else {
            while left + 1 < right {
                budget.sample(1)?;
                let middle = left + (right - left) / 2;
                let middle_time = key_time(middle);
                if time == middle_time {
                    left = middle;
                    right = middle;
                    break;
                }
                if time < middle_time {
                    right = middle;
                } else {
                    left = middle;
                }
            }
        }
        // Fixed result/alpha work is admitted before evaluating any component.
        budget.sample(1 + N)?;
        let alpha = if left == right {
            0.0
        } else {
            (time - key_time(left)) / (key_time(right) - key_time(left))
        };
        let exact = left == right || matches!(self.interpolation, Interpolation::Constant);
        let mut values = [0; N];
        for (id, value) in values.iter_mut().enumerate() {
            let a = f64::from(f32::from_bits(self.keys[left].value_bits[id]));
            *value = if exact {
                a
            } else {
                let b = f64::from(f32::from_bits(self.keys[right].value_bits[id]));
                (1.0 - alpha) * a + alpha * b
            }
            .to_bits();
        }
        Ok(Sample {
            source_key_indices: [left, right],
            alpha_f64_bits: alpha.to_bits(),
            evaluated_f64_bits: values,
            source_value_bits: exact.then_some(self.keys[left].value_bits),
            interpolation: self.interpolation,
        })
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Translation,
    Scale,
}
#[derive(Debug, Serialize)]
#[serde(tag = "channel", rename_all = "snake_case")]
pub enum Evaluated {
    Translation { sample: Option<Sample<3>> },
    Scale { sample: Option<Sample<1>> },
}
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub contract: &'static str,
    pub source_block: u32,
    pub source_block_type: &'static str,
    pub source_offset: usize,
    pub source_bytes: usize,
    pub source_sha256: String,
    pub requested_time_f64_bits: u64,
    pub evaluation: Evaluated,
    pub work: Usage,
    pub runtime_ready: bool,
}
pub fn evaluate(
    block: &Block,
    channel: Channel,
    time: f64,
    budget: &mut Budget,
) -> Result<Diagnostic> {
    // Empty groups still validate the explicit request; absence is not zero.
    budget.sample(1)?;
    if !time.is_finite() {
        return Err(unsupported("nonfinite requested source time"));
    }
    if block.block_type != "NiTransformData"
        || block.sha256.len() != 64
        || !block.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || block.offset.checked_add(block.bytes).is_none()
    {
        return Err(unsupported(
            "source block identity/span/hash is contradictory",
        ));
    }
    let evaluation = match channel {
        Channel::Translation => Evaluated::Translation {
            sample: prepare(&block.data.translations, budget)?
                .map(|group| group.sample(time, budget))
                .transpose()?,
        },
        Channel::Scale => Evaluated::Scale {
            sample: prepare(&block.data.scales, budget)?
                .map(|group| group.sample(time, budget))
                .transpose()?,
        },
    };
    Ok(Diagnostic {
        contract: CONTRACT,
        source_block: block.block,
        source_block_type: block.block_type,
        source_offset: block.offset,
        source_bytes: block.bytes,
        source_sha256: block.sha256.clone(),
        requested_time_f64_bits: time.to_bits(),
        evaluation,
        work: budget.usage(),
        runtime_ready: false,
    })
}
