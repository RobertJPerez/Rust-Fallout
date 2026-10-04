//! Immutable source-bound links and one compiled required forest. Samples retain
//! independent observations; no time vector alters this plan or supplies matrices.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct ChannelBinding {
    pub object: u32,
    pub controller: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ExplicitChannelTime {
    pub expected_source_sha256: [u8; 32],
    pub object: u32,
    pub controller: u32,
    pub source_time: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SetPreparationLimits {
    pub channels: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
    pub sampling: sampling::Limits,
    /// Logical live source storage plus all admitted plan storage/temporaries.
    pub live_array_bytes: usize,
}
impl Default for SetPreparationLimits {
    fn default() -> Self {
        Self {
            channels: 256,
            array_bytes: 16 * 1024 * 1024,
            work_units: 2_000_000,
            ancestry_depth: 1024,
            sampling: Limits::default().sampling,
            live_array_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SetSampleLimits {
    pub array_bytes: usize,
    pub work_units: usize,
    pub sampling: sampling::Limits,
    pub live_array_bytes: usize,
}
impl Default for SetSampleLimits {
    fn default() -> Self {
        Self {
            array_bytes: 16 * 1024 * 1024,
            work_units: 2_000_000,
            sampling: Limits::default().sampling,
            live_array_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SetBatchLimits {
    pub samples: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub sampling: sampling::Limits,
    pub sample: SetSampleLimits,
}
impl Default for SetBatchLimits {
    fn default() -> Self {
        Self {
            samples: 64,
            array_bytes: 128 * 1024 * 1024,
            work_units: 128_000_000,
            sampling: sampling::Limits {
                validation_work: 16_000_000,
                sampling_work: 16_000_000,
            },
            sample: Default::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct SetPreparationUsage {
    pub source_preparation: PreparationUsage,
    pub required_forest_constructions: usize,
    pub admitted_channels: usize,
    pub required_objects: usize,
    pub source_storage_bytes: usize,
    /// Includes the discarded CSR construction scratch as a conservative charge.
    pub extra_retained_bytes: usize,
    pub live_retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
}

#[derive(Debug, Serialize)]
pub struct PreparedSetBatch {
    pub contract: &'static str,
    pub source_sha256: String,
    pub preparation: SetPreparationUsage,
    pub samples: Vec<PoseSet>,
    /// Source/plan admitted once, plus every full result and sampling temporary.
    pub retained_bytes: usize,
    pub work_units: usize,
    /// Includes preparation's key validation and every sample's full validation.
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}

/// Borrowed references originate only in a sealed PreparedSource. No Deserialize,
/// public map/order constructor, caller Scene or caller pose can create a plan.
pub struct PreparedPoseSet<'a> {
    source: &'a PreparedSource,
    expected_sha256: [u8; 32],
    bindings: Vec<ChannelBinding>,
    linked: Vec<LinkedChannels<'a>>,
    selected: Vec<Option<usize>>,
    required: Vec<bool>,
    order: Vec<u32>,
    usage: SetPreparationUsage,
}

impl PreparedSource {
    pub fn prepare_set(
        &self,
        bindings: &[ChannelBinding],
        limits: SetPreparationLimits,
    ) -> Result<PreparedPoseSet<'_>> {
        let mut budget = Budget {
            source: &self.source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if bindings.is_empty() || bindings.len() > limits.channels {
            return Err(budget.fail("prepared set channel count budget exceeded or empty"));
        }
        if limits.ancestry_depth == 0 || limits.array_bytes == 0 {
            return Err(budget.fail("prepared set storage or ancestry depth budget exceeded"));
        }
        // The source already exists; account for it before any new plan allocation.
        let source_storage_bytes = source_storage(self, limits.live_array_bytes, &mut budget)?;
        let remaining_live = limits
            .live_array_bytes
            .checked_sub(source_storage_bytes)
            .ok_or_else(|| budget.fail("prepared set live source storage budget exceeded"))?;
        let initial_bytes = limits.array_bytes.min(remaining_live);
        budget.bytes = initial_bytes;
        budget.reserve::<PreparedPoseSet<'_>>(1)?;
        budget.reserve::<ChannelBinding>(bindings.len())?;
        budget.reserve::<LinkedChannels<'_>>(bindings.len())?;
        let blocks = self.index.blocks.len();
        budget.reserve::<Option<usize>>(blocks)?;
        budget.reserve::<bool>(blocks)?;
        budget.charge(64)?;
        let mut expected_sha256 = [0u8; 32];
        for (i, pair) in self.sha256.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let digit = |byte| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => unreachable!("sealed source SHA256 is lowercase hex"),
            };
            expected_sha256[i] = digit(pair[0]) * 16 + digit(pair[1]);
        }
        let view = SourceView::prepared(self);
        let mut selected = vec![None; blocks];
        let mut required = vec![false; blocks];
        for (ordinal, binding) in bindings.iter().enumerate() {
            budget.charge(1)?;
            let slot = selected
                .get_mut(binding.object as usize)
                .ok_or_else(|| budget.fail("selected object is not decoded"))?;
            if slot.replace(ordinal).is_some() {
                return Err(budget.fail("duplicate prepared set object"));
            }
        }
        required_closure(
            self,
            &view,
            &selected,
            &mut required,
            bindings.iter().map(|binding| binding.object),
            limits.ancestry_depth,
            &mut budget,
        )?;
        let mut linked = Vec::with_capacity(bindings.len());
        let mut validation = sampling::Budget::new(limits.sampling);
        for binding in bindings {
            budget.charge(4)?;
            let channel = admit_binding(&view, binding.object, binding.controller, &budget)?;
            // Admission never invents a source time. Keep existing supported tag,
            // finite-key and ordering validation; each time sample repeats it.
            sampling::prepare(&channel.data.data.translations, &mut validation)?;
            sampling::prepare(&channel.data.data.scales, &mut validation)?;
            linked.push(channel);
        }
        let forest = ForestView {
            scene: &self.scene,
            object_slots: &self.objects,
            world_slots: &self.worlds,
            required: &required,
        };
        let RequiredTopology {
            offsets,
            children,
            mut order,
            required_count,
        } = construct_required(forest, &mut budget)?;
        let mut position = 0;
        while position < order.len() {
            budget.charge(1)?;
            let id = order[position] as usize;
            append_children(
                id,
                &offsets,
                &children,
                &mut order,
                required_count,
                &mut budget,
            )?;
            position += 1;
        }
        if position != required_count {
            return Err(budget.fail("required forest could not compile all objects"));
        }
        let extra_retained_bytes = initial_bytes - budget.bytes;
        let live_retained_bytes = source_storage_bytes
            .checked_add(extra_retained_bytes)
            .ok_or_else(|| budget.fail("prepared set live storage overflow"))?;
        Ok(PreparedPoseSet {
            source: self,
            expected_sha256,
            bindings: bindings.to_vec(),
            linked,
            selected,
            required,
            order,
            usage: SetPreparationUsage {
                source_preparation: self.usage(),
                required_forest_constructions: 1,
                admitted_channels: bindings.len(),
                required_objects: required_count,
                source_storage_bytes,
                extra_retained_bytes,
                live_retained_bytes,
                work_units: limits.work_units - budget.work,
                sample_work: validation.usage(),
            },
        })
    }
}

impl PreparedPoseSet<'_> {
    pub fn usage(&self) -> SetPreparationUsage {
        self.usage
    }
    pub fn source_sha256(&self) -> &str {
        self.source.source_sha256()
    }
    pub fn sample(
        &self,
        times: &[ExplicitChannelTime],
        limits: SetSampleLimits,
    ) -> Result<PoseSet> {
        let mut budget = Budget {
            source: &self.source.source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if self.usage.live_retained_bytes > limits.live_array_bytes {
            return Err(budget.fail("prepared set live storage budget exceeded"));
        }
        if times.len() != self.bindings.len() {
            return Err(budget.fail("prepared set time vector has missing or excess bindings"));
        }
        budget.reserve::<Option<f64>>(times.len())?;
        let mut ordered_times = vec![None; times.len()];
        for time in times {
            budget.charge(34)?;
            if time.expected_source_sha256 != self.expected_sha256 {
                return Err(budget.fail("prepared set time source SHA256 differs"));
            }
            if !time.source_time.is_finite() {
                return Err(budget.fail("requested source time must be finite"));
            }
            let ordinal = self
                .selected
                .get(time.object as usize)
                .copied()
                .flatten()
                .ok_or_else(|| budget.fail("foreign prepared set time object"))?;
            if self.bindings[ordinal].controller != time.controller {
                return Err(budget.fail("prepared set time controller differs"));
            }
            if ordered_times[ordinal].replace(time.source_time).is_some() {
                return Err(budget.fail("duplicate prepared set time object"));
            }
        }
        budget.reserve::<PoseSet>(1)?;
        budget.reserve::<u8>(64)?;
        budget.reserve::<SetObjectPose>(self.bindings.len())?;
        budget.reserve::<Option<Affine>>(self.selected.len())?;
        let mut worlds = vec![None; self.selected.len()];
        let view = SourceView::prepared(self.source);
        let mut objects = Vec::with_capacity(self.bindings.len());
        let mut sampling_left = limits.sampling;
        for (ordinal, binding) in self.bindings.iter().enumerate() {
            budget.charge(1)?;
            budget.reserve::<u8>(6 * 64)?;
            let mut sampling_budget = sampling::Budget::new(sampling_left);
            let object = observe_sample(
                &view,
                &self.linked[ordinal],
                Request {
                    object: binding.object,
                    controller: binding.controller,
                    source_time: ordered_times[ordinal]
                        .ok_or_else(|| budget.fail("missing prepared set time object"))?,
                },
                &mut sampling_budget,
                &budget,
            )?;
            debit_sampler(&mut sampling_left, sampling_budget.usage(), &budget)?;
            objects.push(object);
        }
        let forest = ForestView {
            scene: &self.source.scene,
            object_slots: &self.source.objects,
            world_slots: &self.source.worlds,
            required: &self.required,
        };
        for &id in &self.order {
            propagate_node(
                forest,
                id as usize,
                &mut worlds,
                &mut [],
                None,
                &mut budget,
                &mut |object| {
                    self.selected[object.block as usize]
                        .map(|i| objects[i].channel.local)
                        .unwrap_or_else(|| scene_affine(object.transform))
                },
            )?;
        }
        finish_objects(
            self.source,
            &view,
            &self.selected,
            &worlds,
            &mut objects,
            &mut budget,
        )?;
        Ok(PoseSet {
            contract: "engineering-explicit-linked-pose-set-v1",
            source_sha256: self.source.sha256.clone(),
            preparation: self.source.usage(),
            objects,
            propagated_objects: self.order.len(),
            retained_bytes: limits.array_bytes - budget.bytes,
            work_units: limits.work_units - budget.work,
            sample_work: sampling::Usage {
                validation_units: limits.sampling.validation_work - sampling_left.validation_work,
                sampling_units: limits.sampling.sampling_work - sampling_left.sampling_work,
            },
            retail_behavior_verified: false,
        })
    }

    pub fn sample_many(
        &self,
        time_vectors: &[Vec<ExplicitChannelTime>],
        limits: SetBatchLimits,
    ) -> Result<PreparedSetBatch> {
        let mut budget = Budget {
            source: &self.source.source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if time_vectors.is_empty() || time_vectors.len() > limits.samples {
            return Err(budget.fail("prepared set batch sample count budget exceeded or empty"));
        }
        budget.reserve::<PreparedSetBatch>(1)?;
        budget.reserve::<u8>(64)?;
        budget.reserve::<u8>(self.usage.live_retained_bytes)?;
        budget.reserve::<PoseSet>(time_vectors.len())?;
        budget.charge(self.usage.work_units)?;
        budget.charge(self.source.usage().work_units)?;
        budget.charge(self.source.decoded.keys.work_units)?;
        let mut sampling_left = limits.sampling;
        debit_sampler(&mut sampling_left, self.usage.sample_work, &budget)?;
        let mut samples = Vec::with_capacity(time_vectors.len());
        for (ordinal, times) in time_vectors.iter().enumerate() {
            let sample = self
                .sample(
                    times,
                    SetSampleLimits {
                        array_bytes: limits.sample.array_bytes.min(budget.bytes),
                        work_units: limits.sample.work_units.min(budget.work),
                        sampling: sampling::Limits {
                            validation_work: limits
                                .sample
                                .sampling
                                .validation_work
                                .min(sampling_left.validation_work),
                            sampling_work: limits
                                .sample
                                .sampling
                                .sampling_work
                                .min(sampling_left.sampling_work),
                        },
                        ..limits.sample
                    },
                )
                .map_err(|error| {
                    budget.fail(&format!("prepared set batch vector {ordinal}: {error}"))
                })?;
            budget.reserve::<u8>(sample.retained_bytes)?;
            budget.charge(sample.work_units)?;
            debit_sampler(&mut sampling_left, sample.sample_work, &budget)?;
            samples.push(sample);
        }
        Ok(PreparedSetBatch {
            contract: "engineering-prepared-source-pose-set-batch-v1",
            source_sha256: self.source.sha256.clone(),
            preparation: self.usage,
            samples,
            retained_bytes: limits.array_bytes - budget.bytes,
            work_units: limits.work_units - budget.work,
            sample_work: sampling::Usage {
                validation_units: limits.sampling.validation_work - sampling_left.validation_work,
                sampling_units: limits.sampling.sampling_work - sampling_left.sampling_work,
            },
            retail_behavior_verified: false,
        })
    }
}

fn debit_sampler(
    left: &mut sampling::Limits,
    used: sampling::Usage,
    budget: &Budget<'_>,
) -> Result<()> {
    left.validation_work = left
        .validation_work
        .checked_sub(used.validation_units)
        .ok_or_else(|| budget.fail("prepared set validation work exceeded"))?;
    left.sampling_work = left
        .sampling_work
        .checked_sub(used.sampling_units)
        .ok_or_else(|| budget.fail("prepared set sampling work exceeded"))?;
    Ok(())
}

/// Element accounting, not allocator/process memory. Payload allowances include
/// source Scene arrays; index strings and separately block-bounded tables are
/// charged explicitly. No source bytes, key data or hashes are walked by samples.
fn source_storage(source: &PreparedSource, limit: usize, work: &mut Budget<'_>) -> Result<usize> {
    let mut bytes = Budget {
        source: &source.source,
        bytes: limit,
        work: 0,
    };
    let usage = source.usage();
    bytes.reserve::<u8>(usage.extra_retained_bytes)?;
    bytes.reserve::<u8>(usage.animation_key_retained_bytes)?;
    bytes.reserve::<u8>(usage.scene_array_admission_bytes)?;
    let index = &source.index;
    bytes.reserve::<nif::NifIndex>(1)?;
    bytes.reserve::<nif::Block>(index.blocks.len())?;
    bytes.reserve::<u32>(index.groups.len())?;
    bytes.reserve::<Option<u32>>(index.roots.len())?;
    bytes.reserve::<Vec<u8>>(index.strings.len())?;
    bytes.reserve::<Vec<u8>>(index.export_strings.len())?;
    bytes.reserve::<String>(index.block_types.len())?;
    for value in index.strings.iter().chain(&index.export_strings) {
        work.charge(1)?;
        bytes.reserve::<u8>(value.len())?;
    }
    for value in &index.block_types {
        work.charge(1)?;
        bytes.reserve::<u8>(value.len())?;
    }
    // Map entry storage has explicit four-word logical node/link allowance.
    for name in index.block_counts.keys() {
        work.charge(1)?;
        bytes.reserve::<(String, usize, [usize; 4])>(1)?;
        bytes.reserve::<u8>(name.len())?;
    }
    let scene = &source.scene;
    bytes.reserve::<nif_scene::Scene>(1)?;
    bytes.reserve::<nif_scene::Object>(scene.objects.len())?;
    bytes.reserve::<nif_scene::WorldTransform>(scene.world_transforms.len())?;
    bytes.reserve::<nif_scene::MeshData>(scene.meshes.len())?;
    bytes.reserve::<nif_scene::material::MaterialBlock>(scene.materials.len())?;
    bytes.reserve::<nif_scene::material::TextureReference>(scene.textures.len())?;
    bytes.reserve::<nif_scene::ExtraFlags>(scene.extra_flags.len())?;
    for (name, ids) in &scene.unsupported_blocks {
        work.charge(1)?;
        bytes.reserve::<(String, Vec<u32>, [usize; 4])>(1)?;
        bytes.reserve::<u8>(name.len())?;
        bytes.reserve::<u32>(ids.len())?;
    }
    Ok(limit - bytes.bytes)
}
