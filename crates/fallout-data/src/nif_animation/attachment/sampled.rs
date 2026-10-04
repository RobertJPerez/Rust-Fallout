//! One exact source sample drives a forward-only rigid attachment mapping.
use super::{Affine, Budget, Evaluation, Limits, Loaded, Request, compose, finite, scene_affine};
use crate::{Result, nif, nif_animation::pose, nif_scene};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct SampledRequest<'a> {
    pub binding: Request<'a>,
    pub sample: pose::Request,
}
#[derive(Clone, Copy, Debug)]
pub struct SampledLimits {
    pub binding: Limits,
    pub sample: pose::Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for SampledLimits {
    fn default() -> Self {
        Self {
            binding: Default::default(),
            sample: Default::default(),
            decoder_array_admission_bytes: 160 * 1024 * 1024,
            decoder_check_admission_units: 32_000_000,
            array_bytes: 16 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SampledEvaluation {
    pub contract: &'static str,
    pub sample: pose::ObjectPose,
    /// Explicit stored-only reference observations, including stored mapping.
    /// The two outer matrices below are the evaluated sampled attachment result.
    pub stored_binding: Evaluation,
    pub attachment_source_to_skeleton_source: Affine,
    pub root_to_skeleton_source: Affine,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}
fn check_descendants(
    scene: &nif_scene::Scene,
    index: &nif::NifIndex,
    root: u32,
    allowed: Option<(u32, u32)>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.reserve::<Option<usize>>(index.blocks.len())?;
    budget.reserve::<bool>(index.blocks.len())?;
    budget.reserve::<u32>(index.blocks.len())?;
    budget.charge(index.blocks.len())?;
    budget.charge(scene.objects.len())?;
    let mut objects = vec![None; index.blocks.len()];
    let mut seen = vec![false; index.blocks.len()];
    for (i, object) in scene.objects.iter().enumerate() {
        objects[object.block as usize] = Some(i);
    }
    let mut queue = Vec::with_capacity(index.blocks.len());
    queue.push(root);
    seen[root as usize] = true;
    let mut cursor = 0;
    while cursor < queue.len() {
        budget.charge(1)?;
        let block = queue[cursor];
        cursor += 1;
        let object = &scene.objects[objects[block as usize]
            .ok_or_else(|| budget.fail("sampled attachment descendant is not decoded"))?];
        if let Some(controller) = object.controller
            && allowed != Some((block, controller))
        {
            return Err(budget.fail(&format!(
                "sampled attachment descendant {block} controller {controller} is unapplied"
            )));
        }
        if let nif_scene::ObjectKind::Node { children, .. } = &object.kind {
            budget.charge(children.len())?;
            for &child in children.iter().flatten() {
                if !seen[child as usize] {
                    seen[child as usize] = true;
                    queue.push(child);
                }
            }
        }
    }
    Ok(())
}
pub fn evaluate_sampled(
    skeleton_bytes: &[u8],
    attachment_bytes: &[u8],
    source: &str,
    request: SampledRequest<'_>,
    limits: SampledLimits,
) -> Result<SampledEvaluation> {
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    let input = skeleton_bytes
        .len()
        .checked_add(attachment_bytes.len())
        .filter(|n| *n <= limits.binding.combined_input_bytes)
        .ok_or_else(|| budget.fail("sampled attachment combined input byte budget exceeded"))?;
    if skeleton_bytes.len() > limits.binding.scene.input_bytes
        || attachment_bytes.len() > limits.binding.scene.input_bytes
        || skeleton_bytes.len() > limits.sample.scene.input_bytes
        || skeleton_bytes.len() > limits.sample.keys.animation.input_bytes
    {
        return Err(budget.fail("sampled attachment source input byte budget exceeded"));
    }
    if request.sample.object != request.binding.node {
        return Err(budget.fail("sampled attachment object differs from selected node"));
    }
    if !request.sample.source_time.is_finite() {
        return Err(budget.fail("sampled attachment time must be finite"));
    }
    finite(request.binding.attachment_parent_to_node, &budget)?;
    budget.charge(input)?;
    if <[u8; 32]>::from(Sha256::digest(skeleton_bytes)) != request.binding.expected_skeleton_sha256
    {
        return Err(budget.fail("sampled skeleton source SHA256 differs"));
    }
    if <[u8; 32]>::from(Sha256::digest(attachment_bytes))
        != request.binding.expected_attachment_sha256
    {
        return Err(budget.fail("sampled attachment source SHA256 differs"));
    }
    let arrays = [
        limits.sample.keys.max_combined_retained_bytes,
        limits.sample.scene.array_bytes,
        limits.binding.scene.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("sampled attachment decoder array admission exceeded"))?;
    let checks = [
        limits.sample.keys.animation.reference_checks,
        limits.sample.keys.key_work,
        limits.sample.sampling.validation_work,
        limits.sample.sampling.sampling_work,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("sampled attachment decoder check admission exceeded"))?;
    budget.reserve::<SampledEvaluation>(1)?;
    let sample_limits = pose::Limits {
        array_bytes: limits.sample.array_bytes.min(budget.bytes),
        work_units: limits.sample.work_units.min(budget.work),
        ..limits.sample
    };
    let (skeleton_index, skeleton, sample) =
        pose::evaluate_with_scene(skeleton_bytes, source, request.sample, sample_limits)?;
    budget.reserve::<u8>(sample.retained_bytes)?;
    budget.charge(sample.work_units)?;
    let (attachment_index, attachment) =
        nif_scene::decode_with_limits(attachment_bytes, source, limits.binding.scene)?;
    let binding_limits = Limits {
        array_bytes: limits.binding.array_bytes.min(budget.bytes),
        work_units: limits.binding.work_units.min(budget.work),
        ..limits.binding
    };
    let stored_binding = super::evaluate_loaded(
        skeleton_bytes,
        attachment_bytes,
        request.binding,
        binding_limits,
        Loaded {
            skeleton_index: &skeleton_index,
            skeleton: &skeleton,
            attachment_index: &attachment_index,
            attachment: &attachment,
        },
        Budget {
            source,
            bytes: binding_limits.array_bytes,
            work: binding_limits.work_units,
        },
    )?;
    budget.reserve::<u8>(stored_binding.retained_bytes)?;
    budget.charge(stored_binding.work_units)?;
    if sample.source_sha256 != stored_binding.skeleton_sha256
        || sample.object.block != stored_binding.skeleton_path[0].source.block
        || stored_binding.skeleton_path[0].unapplied_controller != Some(sample.controller.block)
    {
        return Err(budget.fail("sampled attachment source/object/controller identity differs"));
    }
    check_descendants(
        &skeleton,
        &skeleton_index,
        request.binding.node,
        Some((request.sample.object, request.sample.controller)),
        &mut budget,
    )?;
    check_descendants(
        &attachment,
        &attachment_index,
        request.binding.attachment_root,
        None,
        &mut budget,
    )?;
    budget.charge(attachment.objects.len())?;
    let root = attachment
        .objects
        .iter()
        .find(|o| o.block == request.binding.attachment_root)
        .ok_or_else(|| budget.fail("sampled attachment root unavailable"))?;
    budget.charge(24)?;
    let mapping = finite(
        compose(
            sample.source_world,
            request.binding.attachment_parent_to_node,
        ),
        &budget,
    )?;
    let root_to = finite(compose(mapping, scene_affine(root.transform)), &budget)?;
    Ok(SampledEvaluation {
        contract: "engineering-one-source-sampled-rigid-attachment-v1",
        sample,
        stored_binding,
        attachment_source_to_skeleton_source: mapping,
        root_to_skeleton_source: root_to,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        retained_bytes: limits.array_bytes - budget.bytes,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
