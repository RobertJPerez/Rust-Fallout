//! Source-bound finite-f32 transport for a complete sampled skin pose set.
use super::{Budget, WeightPolicy};
use crate::Result;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Precision {
    FiniteNearestF32 { maximum_absolute_error: f64 },
}
/// One complete explicit controller set bound to the selected skinned geometry.
/// The source evaluator remains authoritative for all pose and joint matrices.
#[derive(Clone, Copy, Debug)]
pub struct SampledRequest {
    pub pose: super::SetRequest,
    pub precision: Precision,
}

#[derive(Clone, Copy, Debug)]
pub struct SampledLimits {
    pub pose: super::SetCombinedLimits,
    pub channels: usize,
    pub palette_entries: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub max_combined_retained_bytes: usize,
}
impl Default for SampledLimits {
    fn default() -> Self {
        Self {
            pose: Default::default(),
            channels: 256,
            palette_entries: 65_536,
            array_bytes: 16 * 1024 * 1024,
            work_units: 128_000_000,
            max_combined_retained_bytes: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct SampledChannelBinding {
    pub object: u32,
    pub controller: u32,
    pub requested_time_f64_bits: u64,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Matrix {
    pub row_major_3x4_bits: [[u32; 4]; 3],
    /// Exact absolute roundtrip differences under the certified subtraction.
    pub maximum_absolute_error_bound: f64,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Bone {
    pub ordinal: usize,
    pub node: u32,
    pub matrix: Matrix,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SampledUsage {
    pub source_bytes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub preparation_array_admission_bytes: usize,
    pub preparation_work_admission_units: usize,
    pub skin_scene_decodes: usize,
    pub pose_retained_bytes: usize,
    pub pose_work_units: usize,
    pub transport_retained_bytes: usize,
    pub transport_work_units: usize,
    pub scalar_conversions: usize,
    pub conservative_concurrent_charged_bytes: usize,
}
/// Source-bound finite-f32 palette for a complete explicitly sampled pose set.
/// Rows preserve source bone order and NIF-space placement is separate.
#[derive(Debug, Serialize)]
pub struct SampledPacket {
    contract: &'static str,
    source_sha256: String,
    geometry: u32,
    geometry_data: u32,
    instance: u32,
    skin_data: u32,
    skeleton_root: u32,
    weights: WeightPolicy,
    precision: Precision,
    channels: Vec<SampledChannelBinding>,
    palette: Vec<Bone>,
    skin_to_source_world: Matrix,
    usage: SampledUsage,
    retail_behavior_verified: bool,
}
impl SampledPacket {
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    /// `(geometry, geometry_data, instance, skin_data, skeleton_root)`.
    pub fn skin_binding(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.geometry,
            self.geometry_data,
            self.instance,
            self.skin_data,
            self.skeleton_root,
        )
    }
    pub fn channels(&self) -> &[SampledChannelBinding] {
        &self.channels
    }
    pub fn palette(&self) -> &[Bone] {
        &self.palette
    }
    pub fn skin_to_source_world(&self) -> Matrix {
        self.skin_to_source_world
    }
    pub fn usage(&self) -> SampledUsage {
        self.usage
    }
}
fn sum(
    values: impl IntoIterator<Item = usize>,
    budget: &Budget<'_>,
    detail: &str,
) -> Result<usize> {
    values
        .into_iter()
        .try_fold(0usize, |a, b| a.checked_add(b))
        .ok_or_else(|| budget.fail(detail))
}
fn scalar(value: f64, tolerance: f64, budget: &mut Budget<'_>) -> Result<(u32, f64)> {
    budget.charge(4)?;
    if !value.is_finite() {
        return Err(budget.fail("palette transport input is nonfinite"));
    }
    let rounded = value as f32;
    if !rounded.is_finite() {
        return Err(budget.fail("palette transport exceeds finite f32 range"));
    }
    let a = value.abs();
    let b = f64::from(rounded).abs();
    let error = if a == b {
        0.
    } else if b == 0. {
        a
    } else if a >= b / 2. && a <= b * 2. {
        // Sterbenz's lemma: these same-sign binary floats differ exactly when
        // within a factor of two. f32->f64 is exact, including all subnormals.
        (a - b).abs()
    } else {
        return Err(budget.fail("palette transport roundtrip error cannot be certified"));
    };
    if error > tolerance {
        return Err(budget.fail("palette transport precision tolerance exceeded"));
    }
    Ok((rounded.to_bits(), error))
}
fn matrix(value: super::Affine, tolerance: f64, budget: &mut Budget<'_>) -> Result<Matrix> {
    let mut words = [[0; 4]; 3];
    let mut maximum = 0f64;
    for (input, output) in value.iter().flatten().zip(words.iter_mut().flatten()) {
        let (bits, error) = scalar(*input, tolerance, budget)?;
        *output = bits;
        maximum = maximum.max(error);
    }
    Ok(Matrix {
        row_major_3x4_bits: words,
        maximum_absolute_error_bound: maximum,
    })
}
/// Evaluate and transport the selected skin against the complete admitted
/// controller set. The input byte hash and pose-set links are validated by the
/// existing source producer; no caller-provided matrix or joint table is used.
pub fn prepare_sampled_set(
    bytes: &[u8],
    source: &str,
    request: SampledRequest,
    channels: &[crate::nif_animation::pose::Request],
    limits: SampledLimits,
) -> Result<SampledPacket> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    let Precision::FiniteNearestF32 {
        maximum_absolute_error: tolerance,
    } = request.precision;
    if !tolerance.is_finite() || tolerance < 0. {
        return Err(
            budget.fail("sampled palette precision tolerance must be finite and nonnegative")
        );
    }
    super::validate_weight_policy(request.pose.skin.weights, &budget)?;
    if channels.len() > limits.channels || channels.len() > limits.pose.animation.requests {
        return Err(budget.fail("sampled palette channel limit exceeded"));
    }
    if channels.is_empty() {
        return Err(budget.fail("sampled palette requires an explicit controller channel"));
    }

    let evaluated =
        super::evaluate_set_sampled(bytes, source, request.pose, channels, limits.pose)?;
    let skin = &evaluated.skin;
    let pose_set = &evaluated.pose_set;
    if pose_set.source_sha256 != skin.source_sha256 {
        return Err(budget.fail("sampled pose and skin source SHA256 differ"));
    }
    if skin.palette.len() > limits.palette_entries {
        return Err(budget.fail("sampled palette entry limit exceeded"));
    }
    if pose_set.objects.len() != channels.len() {
        return Err(budget.fail("sampled pose set channel binding count differs"));
    }

    budget.reserve::<SampledPacket>(1)?;
    budget.reserve::<u8>(64)?;
    budget.reserve::<SampledChannelBinding>(channels.len())?;
    budget.reserve::<Bone>(skin.palette.len())?;
    let retained = limits.array_bytes - budget.storage;
    let concurrent = sum(
        [bytes.len(), evaluated.retained_bytes, retained],
        &budget,
        "sampled palette concurrent retention overflow",
    )?;
    if concurrent > limits.max_combined_retained_bytes {
        return Err(budget.fail("sampled palette concurrent retention exceeded"));
    }

    let mut channel_bindings = Vec::with_capacity(channels.len());
    for request in channels {
        budget.charge(1)?;
        let observation = pose_set.objects.iter().find(|object| {
            object.channel.object.block == request.object
                && object.channel.controller.block == request.controller
                && object.channel.requested_time_f64_bits == request.source_time.to_bits()
        });
        if observation.is_none() {
            return Err(budget.fail("sampled pose set channel binding differs"));
        }
        channel_bindings.push(SampledChannelBinding {
            object: request.object,
            controller: request.controller,
            requested_time_f64_bits: request.source_time.to_bits(),
        });
    }

    let mut palette = Vec::with_capacity(skin.palette.len());
    for (ordinal, bone) in skin.palette.iter().enumerate() {
        budget.charge(2)?;
        if bone.ordinal != ordinal {
            return Err(budget.fail("sampled palette joint order is not contiguous"));
        }
        palette.push(Bone {
            ordinal: bone.ordinal,
            node: bone.node,
            matrix: matrix(bone.matrix, tolerance, &mut budget)?,
        });
    }
    let placement = matrix(skin.skin_to_source_world, tolerance, &mut budget)?;
    let scalar_conversions = palette
        .len()
        .checked_add(1)
        .and_then(|n| n.checked_mul(12))
        .ok_or_else(|| budget.fail("sampled palette scalar conversion count overflow"))?;

    Ok(SampledPacket {
        contract: "engineering-finite-nearest-f32-sampled-skin-palette-set-v1",
        source_sha256: skin.source_sha256.clone(),
        geometry: skin.geometry,
        geometry_data: skin.geometry_data,
        instance: skin.instance,
        skin_data: skin.skin_data,
        skeleton_root: skin.skeleton_root,
        weights: request.pose.skin.weights,
        precision: request.precision,
        channels: channel_bindings,
        palette,
        skin_to_source_world: placement,
        usage: SampledUsage {
            source_bytes: bytes.len(),
            decoder_array_admission_bytes: evaluated.decoder_array_admission_bytes,
            decoder_check_admission_units: evaluated.decoder_check_admission_units,
            preparation_array_admission_bytes: evaluated.preparation_array_admission_bytes,
            preparation_work_admission_units: evaluated.preparation_work_admission_units,
            skin_scene_decodes: evaluated.skin_scene_decodes,
            pose_retained_bytes: evaluated.retained_bytes,
            pose_work_units: evaluated.work_units,
            transport_retained_bytes: retained,
            transport_work_units: limits.work_units - budget.work,
            scalar_conversions,
            conservative_concurrent_charged_bytes: concurrent,
        },
        retail_behavior_verified: false,
    })
}
