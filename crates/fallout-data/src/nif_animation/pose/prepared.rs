use super::*;

#[derive(Clone, Copy, Debug)]
pub struct SampleLimits {
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
    pub sampling: sampling::Limits,
}
impl From<Limits> for SampleLimits {
    fn from(value: Limits) -> Self {
        Self {
            array_bytes: value.array_bytes,
            work_units: value.work_units,
            ancestry_depth: value.ancestry_depth,
            sampling: value.sampling,
        }
    }
}
impl Default for SampleLimits {
    fn default() -> Self {
        Limits::default().into()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BatchLimits {
    pub samples: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub sampling: sampling::Limits,
    pub sample: SampleLimits,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            samples: 64,
            array_bytes: 64 * 1024 * 1024,
            work_units: 1_000_000,
            sampling: SampleLimits::default().sampling,
            sample: Default::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PoseBatch {
    pub contract: &'static str,
    pub source_sha256: String,
    pub preparation: PreparationUsage,
    pub samples: Vec<ObjectPose>,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PreparationUsage {
    pub animation_key_decodes: usize,
    pub scene_decodes: usize,
    pub map_constructions: usize,
    pub source_sha256_computations: usize,
    pub block_sha256_computations: usize,
    pub extra_retained_bytes: usize,
    pub animation_key_retained_bytes: usize,
    /// Scene decoder arrays are conservatively admitted by their full allowance.
    /// Index/scene graph tables retain their existing independent block cap.
    pub scene_array_admission_bytes: usize,
    pub work_units: usize,
}

/// Owns only catalogues/maps/spans admitted from these exact immutable bytes.
/// No caller catalogue constructor, Deserialize, raw-byte borrow or global cache.
pub struct PreparedSource {
    pub(super) source: String,
    pub(super) sha256: String,
    pub(super) index: nif::NifIndex,
    pub(super) decoded: keyframe::Source,
    pub(super) scene: nif_scene::Scene,
    pub(super) objects: Vec<Option<usize>>,
    pub(super) worlds: Vec<Option<usize>>,
    pub(super) animation: Vec<Option<usize>>,
    pub(super) keys: Vec<Option<usize>>,
    pub(super) spans: Vec<SourceSpan>,
    usage: PreparationUsage,
}

impl PreparedSource {
    pub fn prepare(bytes: &[u8], source: &str, limits: Limits) -> Result<Self> {
        let mut budget = Budget {
            source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        let (index, decoded) = keyframe::decode_with_limits(bytes, source, limits.keys)?;
        let (_, scene) = nif_scene::decode_with_limits(bytes, source, limits.scene)?;
        if !scene.unsupported_scene_edges.is_empty() {
            return Err(budget.fail("unresolved scene ancestry"));
        }
        budget.reserve::<Self>(1)?;
        let name_storage = source
            .len()
            .checked_add(64)
            .ok_or_else(|| budget.fail("prepared source label storage overflow"))?;
        budget.reserve::<u8>(name_storage)?;
        budget.reserve::<Option<usize>>(index.blocks.len() * 4)?;
        budget.reserve::<SourceSpan>(index.blocks.len())?;
        budget.reserve::<u8>(index.blocks.len() * 64)?;
        budget.charge(
            index.blocks.len()
                + scene.objects.len()
                + scene.world_transforms.len()
                + decoded.animation.blocks.len()
                + decoded.keys.blocks.len(),
        )?;
        let mut objects = vec![None; index.blocks.len()];
        let mut worlds = vec![None; index.blocks.len()];
        let mut animation = vec![None; index.blocks.len()];
        let mut keys = vec![None; index.blocks.len()];
        for (i, value) in scene.objects.iter().enumerate() {
            objects[value.block as usize] = Some(i);
        }
        for (i, value) in scene.world_transforms.iter().enumerate() {
            worlds[value.block as usize] = Some(i);
        }
        for (i, value) in decoded.animation.blocks.iter().enumerate() {
            animation[value.block as usize] = Some(i);
        }
        for (i, value) in decoded.keys.blocks.iter().enumerate() {
            keys[value.block as usize] = Some(i);
        }
        let spans = (0..index.blocks.len())
            .map(|block| span(bytes, &index, block as u32))
            .collect();
        let usage = PreparationUsage {
            animation_key_decodes: 1,
            scene_decodes: 1,
            map_constructions: 1,
            source_sha256_computations: 1,
            block_sha256_computations: index.blocks.len(),
            extra_retained_bytes: limits.array_bytes - budget.bytes,
            animation_key_retained_bytes: decoded.animation.retained_bytes
                + decoded.keys.retained_bytes,
            scene_array_admission_bytes: limits.scene.array_bytes,
            work_units: limits.work_units - budget.work,
        };
        Ok(Self {
            source: source.into(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
            index,
            decoded,
            scene,
            objects,
            worlds,
            animation,
            keys,
            spans,
            usage,
        })
    }
    pub fn usage(&self) -> PreparationUsage {
        self.usage
    }
    pub fn source_sha256(&self) -> &str {
        &self.sha256
    }
    pub fn sample(&self, request: Request, limits: SampleLimits) -> Result<ObjectPose> {
        evaluate_loaded(
            SourceView {
                source: &self.source,
                index: &self.index,
                decoded: &self.decoded,
                scene: &self.scene,
                storage: SourceStorage::Prepared(self),
            },
            request,
            limits,
        )
    }
    /// Explicit request order, including repeats/decreasing times. A later error
    /// drops all retained earlier observations; no completed batch can escape.
    pub fn sample_many(&self, requests: &[Request], limits: BatchLimits) -> Result<PoseBatch> {
        let mut budget = Budget {
            source: &self.source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if requests.len() > limits.samples {
            return Err(budget.fail("prepared batch sample count budget exceeded"));
        }
        budget.reserve::<PoseBatch>(1)?;
        budget.reserve::<u8>(64)?;
        // Reserve the complete result vector before its allocation. Individual
        // receipts also charge their ObjectPose header, conservatively twice.
        budget.reserve::<ObjectPose>(requests.len())?;
        let mut validation_left = limits.sampling.validation_work;
        let mut sampling_left = limits.sampling.sampling_work;
        let mut samples = Vec::with_capacity(requests.len());
        for (ordinal, &request) in requests.iter().enumerate() {
            let sample = self
                .sample(
                    request,
                    SampleLimits {
                        array_bytes: limits.sample.array_bytes.min(budget.bytes),
                        work_units: limits.sample.work_units.min(budget.work),
                        sampling: sampling::Limits {
                            validation_work: limits
                                .sample
                                .sampling
                                .validation_work
                                .min(validation_left),
                            sampling_work: limits.sample.sampling.sampling_work.min(sampling_left),
                        },
                        ..limits.sample
                    },
                )
                .map_err(|error| {
                    budget.fail(&format!("prepared batch request {ordinal}: {error}"))
                })?;
            budget.reserve::<u8>(sample.retained_bytes)?;
            budget.charge(sample.work_units)?;
            validation_left = validation_left
                .checked_sub(sample.sample_work.validation_units)
                .ok_or_else(|| budget.fail("prepared batch validation work exceeded"))?;
            sampling_left = sampling_left
                .checked_sub(sample.sample_work.sampling_units)
                .ok_or_else(|| budget.fail("prepared batch sampling work exceeded"))?;
            samples.push(sample);
        }
        Ok(PoseBatch {
            contract: "engineering-prepared-source-pose-batch-v1",
            source_sha256: self.sha256.clone(),
            preparation: self.usage,
            samples,
            retained_bytes: limits.array_bytes - budget.bytes,
            work_units: limits.work_units - budget.work,
            sample_work: sampling::Usage {
                validation_units: limits.sampling.validation_work - validation_left,
                sampling_units: limits.sampling.sampling_work - sampling_left,
            },
            retail_behavior_verified: false,
        })
    }
}
