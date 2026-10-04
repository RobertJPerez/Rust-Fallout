//! Explicit finite f32 transport; no GPU layout or source-frame change.
use super::{
    Budget, DecodedView, Limits as PoseLimits, Request as PoseRequest, SourceHash, WeightPolicy,
    binding,
};
use crate::{Result, nif_skin::storage};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Precision {
    FiniteNearestF32 { maximum_absolute_error: f64 },
}
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub expected_source_sha256: [u8; 32],
    pub geometry: u32,
    pub weights: WeightPolicy,
    pub precision: Precision,
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
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub pose: PoseLimits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub source_metadata_array_bytes: usize,
    pub source_metadata_work_units: usize,
    pub palette_entries: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub max_combined_retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pose: Default::default(),
            decoder_array_admission_bytes: 512 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            source_metadata_array_bytes: 128 * 1024 * 1024,
            source_metadata_work_units: 16_000_000,
            palette_entries: 65_536,
            array_bytes: 16 * 1024 * 1024,
            work_units: 128_000_000,
            max_combined_retained_bytes: 896 * 1024 * 1024,
        }
    }
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
pub struct Usage {
    pub source_bytes: usize,
    pub source_decodes: usize,
    pub full_source_sha256_traversals: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub initial_concurrent_admission_bytes: usize,
    pub source_metadata_retained_bytes: usize,
    pub source_metadata_work_units: usize,
    /// Existing cold deformer charge includes released source/scratch storage.
    pub deformation_charged_bytes: usize,
    pub deformation_work_units: usize,
    pub scalar_conversions: usize,
    pub retained_bytes: usize,
    pub conservative_concurrent_charged_bytes: usize,
    pub work_units: usize,
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
#[derive(Debug, Serialize)]
pub struct Packet {
    contract: &'static str,
    source_sha256: String,
    geometry: u32,
    geometry_data: u32,
    instance: u32,
    skin_data: u32,
    skeleton_root: u32,
    weights: WeightPolicy,
    precision: Precision,
    palette: Vec<Bone>,
    skin_to_source_world: Matrix,
    usage: Usage,
    retail_behavior_verified: bool,
}
impl Packet {
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    pub fn palette(&self) -> &[Bone] {
        &self.palette
    }
    pub fn skin_to_source_world(&self) -> Matrix {
        self.skin_to_source_world
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
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
pub fn prepare(bytes: &[u8], source: &str, request: Request, limits: Limits) -> Result<Packet> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    let Precision::FiniteNearestF32 {
        maximum_absolute_error: tolerance,
    } = request.precision;
    if !tolerance.is_finite() || tolerance < 0. {
        return Err(budget.fail("palette precision tolerance must be finite and nonnegative"));
    }
    super::validate_weight_policy(request.weights, &budget)?;
    if bytes.len() > limits.pose.source.partition.skin.scene.input_bytes {
        return Err(budget.fail("palette source input byte budget exceeded"));
    }
    let source = limits.pose.source;
    let arrays = sum(
        [
            source.partition.skin.scene.array_bytes,
            source.partition.skin.skin_array_bytes,
            source.partition.array_bytes,
            source.array_bytes,
        ],
        &budget,
        "palette decoder array admission overflow",
    )?;
    let checks = sum(
        [
            source.partition.skin.weight_index_checks,
            source.partition.index_checks,
            source.graph_checks,
        ],
        &budget,
        "palette decoder check admission overflow",
    )?;
    if arrays > limits.decoder_array_admission_bytes
        || checks > limits.decoder_check_admission_units
    {
        return Err(budget.fail("palette decoder admission exceeded"));
    }
    let admission = sum(
        [
            bytes.len(),
            arrays,
            limits.source_metadata_array_bytes,
            limits.pose.array_bytes,
            limits.array_bytes,
        ],
        &budget,
        "palette concurrent admission overflow",
    )?;
    if admission > limits.max_combined_retained_bytes {
        return Err(budget.fail("palette concurrent admission exceeded"));
    }
    budget.reserve::<Packet>(1)?;
    budget.reserve::<u8>(64)?;
    budget.charge(bytes.len())?;
    let digest = Sha256::digest(bytes);
    budget.charge(32)?;
    if <[u8; 32]>::from(digest) != request.expected_source_sha256 {
        return Err(budget.fail("palette source SHA256 differs"));
    }
    let sha = format!("{digest:x}");
    let (index, decoded, scene) = binding::decode_with_scene(bytes, budget.source, source)?;
    let mut meta = Budget {
        source: budget.source,
        storage: limits.source_metadata_array_bytes,
        work: limits.source_metadata_work_units,
    };
    storage::admit_index(&index, &mut meta)?;
    storage::admit_scene(&scene, &mut meta)?;
    let metadata_retained = limits.source_metadata_array_bytes - meta.storage;
    let metadata_work = limits.source_metadata_work_units - meta.work;
    let skin = super::evaluate_decoded(
        DecodedView {
            source: budget.source,
            hash: SourceHash::Prepared(&sha),
            index: &index,
            decoded: &decoded,
            scene: &scene,
        },
        PoseRequest {
            geometry: request.geometry,
            weights: request.weights,
        },
        limits.pose,
        None,
        None,
        Budget {
            source: budget.source,
            storage: limits.pose.array_bytes,
            work: limits.pose.work_units.min(budget.work),
        },
    )?;
    budget.charge(skin.work_units)?;
    if skin.palette.len() > limits.palette_entries {
        return Err(budget.fail("palette transport entry limit exceeded"));
    }
    budget.reserve::<Bone>(skin.palette.len())?;
    let retained = limits.array_bytes - budget.storage;
    let concurrent = sum(
        [
            bytes.len(),
            metadata_retained,
            skin.retained_bytes,
            retained,
        ],
        &budget,
        "palette concurrent retention overflow",
    )?;
    if concurrent > limits.max_combined_retained_bytes {
        return Err(budget.fail("palette concurrent retention exceeded"));
    }
    let conversions = skin
        .palette
        .len()
        .checked_add(1)
        .and_then(|n| n.checked_mul(12))
        .ok_or_else(|| budget.fail("palette scalar conversion count overflow"))?;
    // Source structures stay private and can be released before conversion.
    drop(scene);
    drop(decoded);
    drop(index);
    let mut palette = Vec::with_capacity(skin.palette.len());
    for bone in &skin.palette {
        budget.charge(2)?;
        palette.push(Bone {
            ordinal: bone.ordinal,
            node: bone.node,
            matrix: matrix(bone.matrix, tolerance, &mut budget)?,
        });
    }
    // Placement is deliberately last: failure here also discards every bone row.
    let placement = matrix(skin.skin_to_source_world, tolerance, &mut budget)?;
    Ok(Packet {
        contract: "engineering-finite-nearest-f32-skin-palette-v1",
        source_sha256: sha,
        geometry: skin.geometry,
        geometry_data: skin.geometry_data,
        instance: skin.instance,
        skin_data: skin.skin_data,
        skeleton_root: skin.skeleton_root,
        weights: request.weights,
        precision: request.precision,
        palette,
        skin_to_source_world: placement,
        usage: Usage {
            source_bytes: bytes.len(),
            source_decodes: 1,
            full_source_sha256_traversals: 1,
            decoder_array_admission_bytes: arrays,
            decoder_check_admission_units: checks,
            initial_concurrent_admission_bytes: admission,
            source_metadata_retained_bytes: metadata_retained,
            source_metadata_work_units: metadata_work,
            deformation_charged_bytes: skin.retained_bytes,
            deformation_work_units: skin.work_units,
            scalar_conversions: conversions,
            retained_bytes: retained,
            conservative_concurrent_charged_bytes: concurrent,
            work_units: limits.work_units - budget.work,
        },
        retail_behavior_verified: false,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn certified_rounding_words_and_exact_errors() {
        let mut b = Budget {
            source: "rounding",
            storage: 0,
            work: 1000,
        };
        let half = 2f64.powi(-24);
        for (x, word, error) in [
            (1. + half, 0x3f80_0000, half),
            (1. + 3. * half, 0x3f80_0002, half),
            (-0., 0x8000_0000, 0.),
            (2f64.powi(-150), 0, 2f64.powi(-150)),
            (-2f64.powi(-150), 0x8000_0000, 2f64.powi(-150)),
            (f64::from_bits(1), 0, f64::from_bits(1)),
        ] {
            assert_eq!(scalar(x, error, &mut b).unwrap(), (word, error));
            if error > 0. {
                assert!(scalar(x, error.next_down(), &mut b).is_err());
            }
        }
        assert!(scalar(f64::MAX, f64::MAX, &mut b).is_err());
        assert!(scalar(f64::NAN, 1., &mut b).is_err());
        assert!(scalar(f64::INFINITY, 1., &mut b).is_err());
    }
}
