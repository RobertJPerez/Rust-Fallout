//! Immutable two-source packet binding and atomic ordered sample batches.
use super::*;
use crate::{
    nif,
    nif_skin::pose::{compose, scene_affine},
};

#[derive(Clone, Copy, Debug)]
pub struct BindingRequest<'a> {
    pub expected_skeleton_sha256: [u8; 32],
    pub expected_clip_sha256: [u8; 32],
    pub object: u32,
    pub node_name_bytes: &'a [u8],
    pub sequence: u32,
    pub controlled_ordinal: usize,
}
impl<'a> From<Request<'a>> for BindingRequest<'a> {
    fn from(r: Request<'a>) -> Self {
        Self {
            expected_skeleton_sha256: r.expected_skeleton_sha256,
            expected_clip_sha256: r.expected_clip_sha256,
            object: r.object,
            node_name_bytes: r.node_name_bytes,
            sequence: r.sequence,
            controlled_ordinal: r.controlled_ordinal,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PreparationLimits {
    pub source: Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for PreparationLimits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            decoder_array_admission_bytes: 128 * 1024 * 1024,
            decoder_check_admission_units: 32_000_000,
            array_bytes: 8 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct PreparationUsage {
    pub skeleton_bytes: usize,
    pub clip_bytes: usize,
    pub animation_key_decodes: usize,
    pub scene_decodes: usize,
    pub target_bindings: usize,
    pub whole_source_sha256_computations: usize,
    pub additional_span_hashes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub clip_retained_bytes: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
}
struct PreparedAncestor {
    observation: Ancestor,
    matrix: Affine,
}
pub struct PreparedClipSource {
    _skeleton_index: nif::NifIndex,
    _clip_index: nif::NifIndex,
    scene: nif_scene::Scene,
    decoded: keyframe::Source,
    skeleton_digest: [u8; 32],
    clip_digest: [u8; 32],
    pub(super) skeleton_sha256: String,
    pub(super) clip_sha256: String,
    object: u32,
    sequence: u32,
    controlled_ordinal: usize,
    node_name_bytes: Vec<u8>,
    object_position: usize,
    sequence_position: usize,
    interpolator_position: usize,
    data_position: usize,
    interpolator_id: u32,
    data_id: u32,
    pub(super) object_span: SourceSpan,
    sequence_span: SourceSpan,
    interpolator_span: SourceSpan,
    data_span: SourceSpan,
    ancestors: Vec<PreparedAncestor>,
    usage: PreparationUsage,
}
fn copy_local(value: &SourceLocal) -> SourceLocal {
    SourceLocal {
        translation_bits: value.translation_bits,
        rotation_bits: value.rotation_bits,
        scale_bits: value.scale_bits,
    }
}
fn copy_ancestor(value: &Ancestor) -> Ancestor {
    Ancestor {
        source: value.source.clone(),
        local: copy_local(&value.local),
        flags: value.flags,
        parent: value.parent,
    }
}
impl PreparedClipSource {
    pub fn prepare(
        skeleton_bytes: &[u8],
        clip_bytes: &[u8],
        source: &str,
        request: BindingRequest<'_>,
        limits: PreparationLimits,
    ) -> Result<Self> {
        let mut budget = Budget {
            source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if limits.source.pose.ancestry_depth == 0 {
            return Err(budget.fail("prepared clip ancestry depth budget exceeded"));
        }
        let input = skeleton_bytes
            .len()
            .checked_add(clip_bytes.len())
            .filter(|n| *n <= limits.source.combined_input_bytes)
            .ok_or_else(|| {
                budget.fail("prepared combined clip/skeleton input byte budget exceeded")
            })?;
        if skeleton_bytes.len() > limits.source.pose.scene.input_bytes
            || clip_bytes.len() > limits.source.pose.keys.animation.input_bytes
        {
            return Err(budget.fail("prepared clip source input byte budget exceeded"));
        }
        // Conservative byte-work admission covers both whole-source hashes,
        // selected disjoint spans, and bounded raw string comparisons once.
        budget.charge(
            input
                .checked_mul(4)
                .ok_or_else(|| budget.fail("prepared clip source byte work overflow"))?,
        )?;
        let skeleton_digest: [u8; 32] = Sha256::digest(skeleton_bytes).into();
        let clip_digest: [u8; 32] = Sha256::digest(clip_bytes).into();
        if skeleton_digest != request.expected_skeleton_sha256 {
            return Err(budget.fail("prepared skeleton source SHA256 differs"));
        }
        if clip_digest != request.expected_clip_sha256 {
            return Err(budget.fail("prepared clip source SHA256 differs"));
        }
        let arrays = [
            limits.source.pose.keys.max_combined_retained_bytes,
            limits.source.pose.scene.array_bytes,
        ]
        .into_iter()
        .try_fold(0usize, |n, v| n.checked_add(v))
        .filter(|n| *n <= limits.decoder_array_admission_bytes)
        .ok_or_else(|| budget.fail("prepared clip decoder array admission exceeded"))?;
        let checks = [
            limits.source.pose.keys.animation.reference_checks,
            limits.source.pose.keys.key_work,
        ]
        .into_iter()
        .try_fold(0usize, |n, v| n.checked_add(v))
        .filter(|n| *n <= limits.decoder_check_admission_units)
        .ok_or_else(|| budget.fail("prepared clip decoder check admission exceeded"))?;
        budget.reserve::<Self>(1)?;
        budget.reserve::<u8>(2 * 64)?;
        let (clip_index, decoded) =
            keyframe::decode_with_limits(clip_bytes, source, limits.source.pose.keys)?;
        let (skeleton_index, scene) =
            nif_scene::decode_with_limits(skeleton_bytes, source, limits.source.pose.scene)?;
        let bound = super::bind(
            Sources {
                skeleton_index: &skeleton_index,
                clip_index: &clip_index,
                decoded: &decoded,
                scene: &scene,
            },
            request,
            &mut budget,
        )?;
        let mapping = SceneMapping::prepare(&scene, &skeleton_index, request.object, &mut budget)?;
        let (_, ancestors) = mapping.compose(
            skeleton_bytes,
            &skeleton_index,
            scene_affine(bound.object.transform),
            &mut budget,
            limits.source.pose.ancestry_depth,
        )?;
        budget.reserve::<PreparedAncestor>(ancestors.len())?;
        budget.reserve::<SourceSpan>(4)?;
        budget.reserve::<u8>(4 * 64)?;
        budget.reserve::<u8>(request.node_name_bytes.len())?;
        budget.charge(scene.objects.len())?;
        budget.charge(
            decoded
                .animation
                .blocks
                .len()
                .checked_mul(2)
                .ok_or_else(|| budget.fail("prepared clip lookup work overflow"))?,
        )?;
        budget.charge(decoded.keys.blocks.len())?;
        budget.charge(ancestors.len())?;
        let object_position = scene
            .objects
            .iter()
            .position(|o| o.block == request.object)
            .ok_or_else(|| budget.fail("prepared clip selected object unavailable"))?;
        let sequence_position = decoded
            .animation
            .blocks
            .iter()
            .position(|b| b.block == request.sequence)
            .ok_or_else(|| budget.fail("prepared clip selected sequence unavailable"))?;
        let interpolator_id = bound.interpolator_id;
        let data_id = bound.data_id;
        let interpolator_position = decoded
            .animation
            .blocks
            .iter()
            .position(|b| b.block == interpolator_id)
            .ok_or_else(|| budget.fail("prepared clip interpolator unavailable"))?;
        let data_position = decoded
            .keys
            .blocks
            .iter()
            .position(|b| b.block == data_id)
            .ok_or_else(|| budget.fail("prepared clip data unavailable"))?;
        let object_span = span(skeleton_bytes, &skeleton_index, request.object);
        let sequence_span = span(clip_bytes, &clip_index, request.sequence);
        let interpolator_span = span(clip_bytes, &clip_index, interpolator_id);
        let data_span = span(clip_bytes, &clip_index, data_id);
        let additional_span_hashes = 4 + ancestors.len();
        let ancestors = ancestors
            .into_iter()
            .map(|observation| {
                let matrix = scene_affine(nif_scene::Transform {
                    translation: observation.local.translation_bits.map(f32::from_bits),
                    rotation: observation
                        .local
                        .rotation_bits
                        .map(|r| r.map(f32::from_bits)),
                    scale: f32::from_bits(observation.local.scale_bits),
                });
                PreparedAncestor {
                    observation,
                    matrix,
                }
            })
            .collect();
        let usage = PreparationUsage {
            skeleton_bytes: skeleton_bytes.len(),
            clip_bytes: clip_bytes.len(),
            animation_key_decodes: 1,
            scene_decodes: 1,
            target_bindings: 1,
            whole_source_sha256_computations: 2,
            additional_span_hashes,
            decoder_array_admission_bytes: arrays,
            decoder_check_admission_units: checks,
            clip_retained_bytes: decoded
                .animation
                .retained_bytes
                .checked_add(decoded.keys.retained_bytes)
                .ok_or_else(|| budget.fail("prepared clip source retention overflow"))?,
            retained_bytes: limits.array_bytes - budget.bytes,
            work_units: limits.work_units - budget.work,
        };
        Ok(Self {
            _skeleton_index: skeleton_index,
            _clip_index: clip_index,
            scene,
            decoded,
            skeleton_digest,
            clip_digest,
            skeleton_sha256: hex(skeleton_digest),
            clip_sha256: hex(clip_digest),
            object: request.object,
            sequence: request.sequence,
            controlled_ordinal: request.controlled_ordinal,
            node_name_bytes: request.node_name_bytes.to_vec(),
            object_position,
            sequence_position,
            interpolator_position,
            data_position,
            interpolator_id,
            data_id,
            object_span,
            sequence_span,
            interpolator_span,
            data_span,
            ancestors,
            usage,
        })
    }
    pub fn skeleton_sha256(&self) -> &str {
        &self.skeleton_sha256
    }
    pub fn clip_sha256(&self) -> &str {
        &self.clip_sha256
    }
    pub fn usage(&self) -> PreparationUsage {
        self.usage
    }
    pub(super) fn clip_span(&self, id: u32) -> SourceSpan {
        if id == self.sequence {
            self.sequence_span.clone()
        } else if id == self.interpolator_id {
            self.interpolator_span.clone()
        } else {
            debug_assert_eq!(id, self.data_id);
            self.data_span.clone()
        }
    }
    pub(super) fn compose(
        &self,
        local: Affine,
        budget: &mut Budget<'_>,
        depth: usize,
    ) -> Result<(Affine, Vec<Ancestor>)> {
        if self.ancestors.len() >= depth {
            return Err(budget.fail("ancestry depth budget exceeded"));
        }
        budget.reserve::<Ancestor>(self.ancestors.len())?;
        budget.reserve::<u8>(
            self.ancestors
                .len()
                .checked_mul(64)
                .ok_or_else(|| budget.fail("prepared clip ancestor span product overflow"))?,
        )?;
        budget.charge(self.ancestors.len())?;
        let mut world = local;
        let mut ancestors = Vec::with_capacity(self.ancestors.len());
        for ancestor in &self.ancestors {
            world = compose(ancestor.matrix, world);
            ancestors.push(copy_ancestor(&ancestor.observation));
        }
        if !local.iter().chain(&world).flatten().all(|v| v.is_finite()) {
            return Err(budget.fail("evaluated matrix overflow"));
        }
        Ok((world, ancestors))
    }
    fn bound(&self) -> Bound<'_> {
        let sequence = match &self.decoded.animation.blocks[self.sequence_position].data {
            Data::ControllerSequence { sequence } => sequence,
            _ => unreachable!("sealed clip sequence changed"),
        };
        let interpolator = match &self.decoded.animation.blocks[self.interpolator_position].data {
            Data::TransformInterpolator { interpolator } => interpolator,
            _ => unreachable!("sealed clip interpolator changed"),
        };
        Bound {
            object: &self.scene.objects[self.object_position],
            sequence,
            packet: &sequence.controlled_blocks[self.controlled_ordinal],
            interpolator,
            data: &self.decoded.keys.blocks[self.data_position],
            interpolator_id: self.interpolator_id,
            data_id: self.data_id,
        }
    }
    pub fn sample_many(
        &self,
        source: &str,
        expected_skeleton_sha256: [u8; 32],
        expected_clip_sha256: [u8; 32],
        times: &[f64],
        limits: BatchLimits,
    ) -> Result<ClipBatch> {
        let mut budget = Budget {
            source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if times.is_empty() || times.len() > limits.samples {
            return Err(budget.fail("prepared clip requires a nonempty bounded time list"));
        }
        budget.charge(64)?;
        if self.skeleton_digest != expected_skeleton_sha256 {
            return Err(budget.fail("prepared skeleton source SHA256 differs"));
        }
        if self.clip_digest != expected_clip_sha256 {
            return Err(budget.fail("prepared clip source SHA256 differs"));
        }
        budget.reserve::<ClipBatch>(1)?;
        budget.reserve::<Evaluation>(times.len())?;
        budget.reserve::<u8>(128)?;
        let mut samples = Vec::with_capacity(times.len());
        let mut validation = limits.sampling.validation_work;
        let mut sampling = limits.sampling.sampling_work;
        for &time in times {
            budget.charge(1)?;
            if !time.is_finite() {
                return Err(budget.fail("requested source time must be finite"));
            }
            if limits.sample.ancestry_depth == 0 {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            let sample_limits = pose::SampleLimits {
                array_bytes: limits.sample.array_bytes.min(budget.bytes),
                work_units: limits.sample.work_units.min(budget.work),
                sampling: super::sampling::Limits {
                    validation_work: limits.sample.sampling.validation_work.min(validation),
                    sampling_work: limits.sample.sampling.sampling_work.min(sampling),
                },
                ..limits.sample
            };
            let mut sample_budget = Budget {
                source,
                bytes: sample_limits.array_bytes,
                work: sample_limits.work_units,
            };
            sample_budget.charge(4)?;
            let sample = super::evaluate_bound(
                self.bound(),
                Request {
                    expected_skeleton_sha256: self.skeleton_digest,
                    expected_clip_sha256: self.clip_digest,
                    object: self.object,
                    node_name_bytes: &self.node_name_bytes,
                    sequence: self.sequence,
                    controlled_ordinal: self.controlled_ordinal,
                    source_time: time,
                },
                sample_limits,
                sample_budget,
                Observation::Prepared(self),
            )?;
            budget.reserve::<u8>(sample.retained_bytes)?;
            budget.charge(sample.work_units)?;
            validation = validation
                .checked_sub(sample.sample_work.validation_units)
                .ok_or_else(|| budget.fail("prepared clip aggregate validation work exceeded"))?;
            sampling = sampling
                .checked_sub(sample.sample_work.sampling_units)
                .ok_or_else(|| budget.fail("prepared clip aggregate sampling work exceeded"))?;
            samples.push(sample);
        }
        Ok(ClipBatch {
            contract: "engineering-prepared-two-source-clip-batch-v1",
            skeleton_sha256: self.skeleton_sha256.clone(),
            clip_sha256: self.clip_sha256.clone(),
            preparation: self.usage,
            samples,
            retained_bytes: limits.array_bytes - budget.bytes,
            work_units: limits.work_units - budget.work,
            sample_work: super::sampling::Usage {
                validation_units: limits.sampling.validation_work - validation,
                sampling_units: limits.sampling.sampling_work - sampling,
            },
            retail_behavior_verified: false,
        })
    }
}
fn hex(bytes: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 15) as usize] as char);
    }
    output
}
#[derive(Clone, Copy, Debug)]
pub struct BatchLimits {
    pub samples: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub sampling: sampling::Limits,
    pub sample: pose::SampleLimits,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            samples: 64,
            array_bytes: 64 * 1024 * 1024,
            work_units: 64_000_000,
            sampling: pose::SampleLimits::default().sampling,
            sample: Default::default(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ClipBatch {
    pub contract: &'static str,
    pub skeleton_sha256: String,
    pub clip_sha256: String,
    pub preparation: PreparationUsage,
    pub samples: Vec<Evaluation>,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}
