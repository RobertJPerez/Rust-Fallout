//! Exact simultaneous external packets over the existing required forest.
use super::*;
use crate::nif_skin::pose::scene_affine;

pub const CONTRACT: &str = "engineering-explicit-external-clip-pose-set-v1";
#[derive(Clone, Copy, Debug)]
pub struct SetLimits {
    pub scene: nif_scene::Limits,
    pub keys: keyframe::Limits,
    pub combined_input_bytes: usize,
    pub requests: usize,
    pub raw_name_bytes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub sampling: sampling::Limits,
    pub ancestry_depth: usize,
}
impl Default for SetLimits {
    fn default() -> Self {
        let source = pose::Limits::default();
        Self {
            scene: source.scene,
            keys: source.keys,
            combined_input_bytes: 128 * 1024 * 1024,
            requests: 256,
            raw_name_bytes: 1024 * 1024,
            decoder_array_admission_bytes: 128 * 1024 * 1024,
            decoder_check_admission_units: 32_000_000,
            array_bytes: 64 * 1024 * 1024,
            work_units: 128_000_000,
            sampling: source.sampling,
            ancestry_depth: 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct ClipLocalObservation {
    pub object: SourceSpan,
    pub sequence: SourceSpan,
    pub controlled_ordinal: usize,
    pub controlled_packet: ControlledBlock,
    pub node_name_bytes: Vec<u8>,
    pub interpolator: SourceSpan,
    pub data: SourceSpan,
    pub requested_time_f64_bits: u64,
    pub source_local: SourceLocal,
    pub object_flags: u32,
    pub unapplied_object_controller: Option<u32>,
    pub unapplied_sequence_fields: SequenceFields,
    pub unapplied_interpolator_fields: TransformInterpolator,
    pub translation: sampling::Diagnostic,
    pub scale: sampling::Diagnostic,
    pub local: Affine,
}
#[derive(Debug, Serialize)]
pub struct ClipSetObjectPose {
    pub channel: ClipLocalObservation,
    pub source_world: Affine,
    pub ancestors: Vec<pose::SetAncestor>,
}
#[derive(Debug, Serialize)]
pub struct ClipPoseSet {
    pub contract: &'static str,
    pub skeleton_sha256: String,
    pub clip_sha256: String,
    pub objects: Vec<ClipSetObjectPose>,
    pub propagated_objects: usize,
    pub scene_decodes: usize,
    pub animation_key_decodes: usize,
    pub whole_source_sha256_computations: usize,
    pub additional_span_hashes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub scene_array_admission_bytes: usize,
    pub clip_retained_bytes: usize,
    pub sampling_work_admission_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}

/// No implicit channel closure or packet priority: every controlled required
/// ancestor has an explicit admitted row. A later error drops the whole set.
pub fn evaluate_set(
    skeleton_bytes: &[u8],
    clip_bytes: &[u8],
    source: &str,
    requests: &[Request<'_>],
    limits: SetLimits,
) -> Result<ClipPoseSet> {
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if requests.is_empty() || requests.len() > limits.requests {
        return Err(budget.fail("clip pose set requires a nonempty bounded request list"));
    }
    if limits.ancestry_depth == 0 {
        return Err(budget.fail("ancestry depth budget exceeded"));
    }
    let input = skeleton_bytes
        .len()
        .checked_add(clip_bytes.len())
        .filter(|n| *n <= limits.combined_input_bytes)
        .ok_or_else(|| budget.fail("clip pose set combined input byte budget exceeded"))?;
    if skeleton_bytes.len() > limits.scene.input_bytes
        || clip_bytes.len() > limits.keys.animation.input_bytes
    {
        return Err(budget.fail("clip pose set source input byte budget exceeded"));
    }
    let names = requests
        .iter()
        .try_fold(0usize, |n, r| n.checked_add(r.node_name_bytes.len()))
        .filter(|n| *n <= limits.raw_name_bytes)
        .ok_or_else(|| budget.fail("clip pose set raw name byte budget exceeded"))?;
    if requests.iter().any(|r| !r.source_time.is_finite()) {
        return Err(budget.fail("requested source time must be finite"));
    }
    let byte_work = input
        .checked_mul(6)
        .and_then(|n| {
            skeleton_bytes
                .len()
                .checked_mul(requests.len())
                .and_then(|v| n.checked_add(v))
        })
        .and_then(|n| names.checked_mul(2).and_then(|v| n.checked_add(v)))
        .and_then(|n| {
            requests
                .len()
                .checked_mul(85)
                .and_then(|v| n.checked_add(v))
        })
        .ok_or_else(|| budget.fail("clip pose set source/name work overflow"))?;
    budget.charge(byte_work)?;
    budget.charge(
        requests
            .len()
            .checked_mul(requests.len())
            .ok_or_else(|| budget.fail("clip pose set duplicate work overflow"))?,
    )?;
    let skeleton_digest: [u8; 32] = Sha256::digest(skeleton_bytes).into();
    let clip_digest: [u8; 32] = Sha256::digest(clip_bytes).into();
    for (ordinal, request) in requests.iter().enumerate() {
        if request.expected_skeleton_sha256 != skeleton_digest {
            return Err(budget.fail("skeleton source SHA256 differs from request"));
        }
        if request.expected_clip_sha256 != clip_digest {
            return Err(budget.fail("clip source SHA256 differs from request"));
        }
        for prior in &requests[..ordinal] {
            if prior.object == request.object {
                return Err(budget.fail("duplicate clip pose set destination"));
            }
            if (prior.sequence, prior.controlled_ordinal)
                == (request.sequence, request.controlled_ordinal)
            {
                return Err(budget.fail("duplicate clip pose set controlled packet"));
            }
        }
    }
    let arrays = limits
        .keys
        .max_combined_retained_bytes
        .checked_add(limits.scene.array_bytes)
        .filter(|n| *n <= limits.decoder_array_admission_bytes)
        .ok_or_else(|| budget.fail("clip pose set decoder array admission exceeded"))?;
    let checks = [
        limits.keys.animation.reference_checks,
        limits.keys.key_work,
        limits.sampling.validation_work,
        limits.sampling.sampling_work,
    ]
    .into_iter()
    .try_fold(0usize, usize::checked_add)
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("clip pose set decoder check admission exceeded"))?;
    // The complete shared sampler allowance is charged before any channel;
    // actual sampler visits remain a separate observation, never hidden work.
    let sampler_admission = limits
        .sampling
        .validation_work
        .checked_add(limits.sampling.sampling_work)
        .ok_or_else(|| budget.fail("clip pose set sampling work admission overflow"))?;
    budget.charge(sampler_admission)?;
    budget.reserve::<ClipPoseSet>(1)?;
    budget.reserve::<ClipSetObjectPose>(requests.len())?;
    budget.reserve::<u8>(128)?;
    // Scene payload allowance is admitted in full before decoding; existing
    // source block caps independently bound index/object/graph tables.
    budget.reserve::<u8>(limits.scene.array_bytes)?;
    let (clip_index, decoded) = keyframe::decode_with_limits(
        clip_bytes,
        source,
        keyframe::Limits {
            max_combined_retained_bytes: limits.keys.max_combined_retained_bytes.min(budget.bytes),
            ..limits.keys
        },
    )?;
    let clip_retained = decoded
        .animation
        .retained_bytes
        .checked_add(decoded.keys.retained_bytes)
        .ok_or_else(|| budget.fail("clip pose set decoded retention overflow"))?;
    budget.reserve::<u8>(clip_retained)?;
    let (skeleton_index, scene) =
        nif_scene::decode_with_limits(skeleton_bytes, source, limits.scene)?;
    if !scene.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    let blocks = skeleton_index.blocks.len();
    let spans_count = blocks
        .checked_add(clip_index.blocks.len())
        .ok_or_else(|| budget.fail("clip pose set span count overflow"))?;
    budget.reserve::<SourceSpan>(spans_count)?;
    budget.reserve::<u8>(
        spans_count
            .checked_mul(64)
            .ok_or_else(|| budget.fail("clip pose set span digest storage overflow"))?,
    )?;
    budget.charge(spans_count)?;
    let skeleton_spans: Vec<_> = (0..blocks)
        .map(|id| span(skeleton_bytes, &skeleton_index, id as u32))
        .collect();
    let clip_spans: Vec<_> = (0..clip_index.blocks.len())
        .map(|id| span(clip_bytes, &clip_index, id as u32))
        .collect();
    budget.reserve::<Option<usize>>(
        blocks
            .checked_mul(3)
            .ok_or_else(|| budget.fail("clip pose set map count overflow"))?,
    )?;
    budget.reserve::<bool>(blocks)?;
    budget.reserve::<Option<Affine>>(blocks)?;
    budget.charge(
        scene
            .objects
            .len()
            .checked_add(scene.world_transforms.len())
            .ok_or_else(|| budget.fail("clip pose set map work overflow"))?,
    )?;
    let mut object_slots = vec![None; blocks];
    let mut world_slots = vec![None; blocks];
    let mut selected = vec![None; blocks];
    let mut required = vec![false; blocks];
    let mut worlds = vec![None; blocks];
    for (slot, object) in scene.objects.iter().enumerate() {
        object_slots[object.block as usize] = Some(slot);
    }
    for (slot, world) in scene.world_transforms.iter().enumerate() {
        world_slots[world.block as usize] = Some(slot);
    }
    for (ordinal, request) in requests.iter().enumerate() {
        budget.charge(1)?;
        *selected
            .get_mut(request.object as usize)
            .ok_or_else(|| budget.fail("selected object is not decoded"))? = Some(ordinal);
    }
    for request in requests {
        let mut node = Some(request.object);
        let mut depth = 0;
        while let Some(id) = node {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            let object = object_slots[id as usize]
                .map(|slot| &scene.objects[slot])
                .ok_or_else(|| budget.fail("selected object is not decoded"))?;
            let world = world_slots[id as usize]
                .map(|slot| scene.world_transforms[slot])
                .ok_or_else(|| budget.fail("missing selected object ancestry"))?;
            if !world.reachable_from_footer {
                return Err(budget.fail("selected object is not reachable from footer"));
            }
            if let Some(controller) = object.controller
                && selected[id as usize].is_none()
            {
                return Err(budget.fail(&format!(
                    "required ancestor {id} controller {controller} is not explicitly selected"
                )));
            }
            required[id as usize] = true;
            node = world.parent;
        }
    }
    let view = Sources {
        skeleton_index: &skeleton_index,
        clip_index: &clip_index,
        decoded: &decoded,
        scene: &scene,
    };
    let mut sampling_left = limits.sampling;
    let mut objects = Vec::with_capacity(requests.len());
    for &request in requests {
        let bound = bind(view, request.into(), &mut budget)?;
        budget.reserve::<ClipLocalObservation>(1)?;
        budget.reserve::<u8>(4 * 64)?;
        budget.reserve::<u8>(request.node_name_bytes.len())?;
        if let NoteLinks::Array { targets, .. } = &bound.sequence.notes {
            budget.reserve::<Option<u32>>(targets.len())?;
            budget.charge(targets.len())?;
        }
        // Each channel reports its own sampler visits. The remaining allowance
        // is shared across the set, without request-order cumulative receipts.
        let mut sampling_budget = sampling::Budget::new(sampling_left);
        let (translation, scale, local) = sample_local(
            bound.object,
            bound.data,
            request.source_time,
            &mut sampling_budget,
        )?;
        let used = sampling_budget.usage();
        sampling_left.validation_work = sampling_left
            .validation_work
            .checked_sub(used.validation_units)
            .ok_or_else(|| budget.fail("clip pose set shared validation work exceeded"))?;
        sampling_left.sampling_work = sampling_left
            .sampling_work
            .checked_sub(used.sampling_units)
            .ok_or_else(|| budget.fail("clip pose set shared sampling work exceeded"))?;
        if !local.iter().flatten().all(|v| v.is_finite()) {
            return Err(budget.fail("evaluated matrix overflow"));
        }
        objects.push(ClipSetObjectPose {
            channel: ClipLocalObservation {
                object: skeleton_spans[request.object as usize].clone(),
                sequence: clip_spans[request.sequence as usize].clone(),
                controlled_ordinal: request.controlled_ordinal,
                controlled_packet: bound.packet.clone(),
                node_name_bytes: request.node_name_bytes.to_vec(),
                interpolator: clip_spans[bound.interpolator_id as usize].clone(),
                data: clip_spans[bound.data_id as usize].clone(),
                requested_time_f64_bits: request.source_time.to_bits(),
                source_local: bound.object.transform.into(),
                object_flags: bound.object.flags,
                unapplied_object_controller: bound.object.controller,
                unapplied_sequence_fields: sequence_fields(bound.sequence),
                unapplied_interpolator_fields: bound.interpolator.clone(),
                translation,
                scale,
                local,
            },
            source_world: [[0.; 4]; 3],
            ancestors: Vec::new(),
        });
    }
    let forest = pose::ForestView {
        scene: &scene,
        object_slots: &object_slots,
        world_slots: &world_slots,
        required: &required,
    };
    let propagated_objects =
        pose::propagate_required(forest, &mut worlds, &mut [], None, &mut budget, |object| {
            selected[object.block as usize]
                .map(|i| objects[i].channel.local)
                .unwrap_or_else(|| scene_affine(object.transform))
        })?;
    for ordinal in 0..objects.len() {
        let id = objects[ordinal].channel.object.block;
        objects[ordinal].source_world =
            worlds[id as usize].ok_or_else(|| budget.fail("selected world not propagated"))?;
        let mut parent = scene.world_transforms
            [world_slots[id as usize].expect("selected reachable object validated")]
        .parent;
        while let Some(id) = parent {
            budget.charge(1)?;
            budget.reserve::<pose::SetAncestor>(1)?;
            budget.reserve::<u8>(64)?;
            let object =
                &scene.objects[object_slots[id as usize].expect("required object validated")];
            let world = scene.world_transforms
                [world_slots[id as usize].expect("required ancestry validated")];
            let applied = selected[id as usize];
            let effective_local = applied
                .map(|i| objects[i].channel.local)
                .unwrap_or_else(|| scene_affine(object.transform));
            objects[ordinal].ancestors.push(pose::SetAncestor {
                source: skeleton_spans[id as usize].clone(),
                source_local: object.transform.into(),
                flags: object.flags,
                parent: world.parent,
                applied_object: applied.map(|_| id),
                effective_local,
            });
            parent = world.parent;
        }
    }
    Ok(ClipPoseSet {
        contract: CONTRACT,
        skeleton_sha256: prepared::hex(skeleton_digest),
        clip_sha256: prepared::hex(clip_digest),
        objects,
        propagated_objects,
        scene_decodes: 1,
        animation_key_decodes: 1,
        whole_source_sha256_computations: 2,
        additional_span_hashes: spans_count,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        scene_array_admission_bytes: limits.scene.array_bytes,
        clip_retained_bytes: clip_retained,
        sampling_work_admission_units: sampler_admission,
        retained_bytes: limits.array_bytes - budget.bytes,
        work_units: limits.work_units - budget.work,
        sample_work: sampling::Usage {
            validation_units: limits.sampling.validation_work - sampling_left.validation_work,
            sampling_units: limits.sampling.sampling_work - sampling_left.sampling_work,
        },
        retail_behavior_verified: false,
    })
}
