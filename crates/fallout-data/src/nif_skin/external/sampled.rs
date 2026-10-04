//! Exact supported external packet supplies one rig local to the existing palette.
use super::*;

pub const CONTRACT: &str = "engineering-one-exact-external-clip-skin-v1";

#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub mapping: &'a super::Request,
    pub clip: clip::Request<'a>,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub external: super::Limits,
    pub clip: clip::Limits,
    pub source_bytes: usize,
    pub raw_name_bytes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            external: Default::default(),
            clip: Default::default(),
            source_bytes: 192 * 1024 * 1024,
            raw_name_bytes: 1024 * 1024,
            decoder_array_admission_bytes: 768 * 1024 * 1024,
            decoder_check_admission_units: 96_000_000,
            array_bytes: 96 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub sample: clip::Evaluation,
    pub external: super::Evaluation,
    pub rig_scene_decodes: usize,
    pub skin_scene_decodes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}
pub fn evaluate(
    skin_bytes: &[u8],
    rig_bytes: &[u8],
    clip_bytes: &[u8],
    source: &str,
    request: Request<'_>,
    limits: Limits,
) -> Result<Evaluation> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    let input = skin_bytes
        .len()
        .checked_add(rig_bytes.len())
        .and_then(|n| n.checked_add(clip_bytes.len()))
        .filter(|n| *n <= limits.source_bytes)
        .ok_or_else(|| {
            budget.fail("sampled external combined three-source input byte budget exceeded")
        })?;
    if skin_bytes.len() > limits.external.skin.partition.skin.scene.input_bytes
        || rig_bytes.len() > limits.external.rig.input_bytes
        || rig_bytes.len() > limits.clip.pose.scene.input_bytes
        || clip_bytes.len() > limits.clip.pose.keys.animation.input_bytes
        || skin_bytes
            .len()
            .checked_add(rig_bytes.len())
            .is_none_or(|n| n > limits.external.source_bytes)
        || rig_bytes
            .len()
            .checked_add(clip_bytes.len())
            .is_none_or(|n| n > limits.clip.combined_input_bytes)
    {
        return Err(budget.fail("sampled external individual/pair source input budget exceeded"));
    }
    if !request.clip.source_time.is_finite() {
        return Err(budget.fail("requested source time must be finite"));
    }
    if request.clip.expected_skeleton_sha256 != request.mapping.expected_rig_sha256 {
        return Err(budget.fail("clip skeleton identity differs from exact mapped rig request"));
    }
    if request.mapping.explicit_bone_mapping.len() > limits.external.mapping_bones {
        return Err(budget.fail("external mapping count budget exceeded"));
    }
    let names = request
        .mapping
        .explicit_bone_mapping
        .iter()
        .try_fold(request.clip.node_name_bytes.len(), |n, m| {
            n.checked_add(m.expected_skin_bone_name_bytes.len())?
                .checked_add(m.expected_rig_node_name_bytes.len())
        })
        .filter(|n| *n <= limits.raw_name_bytes)
        .ok_or_else(|| budget.fail("sampled external raw name byte budget exceeded"))?;
    budget.charge(names)?;
    budget.charge(
        input
            .checked_mul(8)
            .ok_or_else(|| budget.fail("sampled external source byte work overflow"))?,
    )?;
    for (bytes, expected, kind) in [
        (skin_bytes, request.mapping.expected_skin_sha256, "skin"),
        (rig_bytes, request.mapping.expected_rig_sha256, "rig"),
        (clip_bytes, request.clip.expected_clip_sha256, "clip"),
    ] {
        if <[u8; 32]>::from(Sha256::digest(bytes)) != expected {
            return Err(budget.fail(&format!("sampled external {kind} source SHA256 differs")));
        }
    }
    let external = limits.external;
    let animation = limits.clip.pose;
    let arrays = [
        external.skin.partition.skin.scene.array_bytes,
        external.skin.partition.skin.skin_array_bytes,
        external.skin.partition.array_bytes,
        external.skin.array_bytes,
        external.rig.array_bytes,
        animation.keys.max_combined_retained_bytes,
        animation.scene.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("sampled external decoder array admission exceeded"))?;
    let checks = [
        external.skin.partition.skin.weight_index_checks,
        external.skin.partition.index_checks,
        external.skin.graph_checks,
        animation.keys.animation.reference_checks,
        animation.keys.key_work,
        animation.sampling.validation_work,
        animation.sampling.sampling_work,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("sampled external decoder check admission exceeded"))?;
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<SampledRig>(1)?;
    let mut scene_limits = animation.scene;
    scene_limits.blocks = scene_limits.blocks.min(external.rig.blocks);
    scene_limits.array_bytes = scene_limits.array_bytes.min(external.rig.array_bytes);
    scene_limits.input_bytes = scene_limits.input_bytes.min(external.rig.input_bytes);
    let (index, scene, sample) = clip::evaluate_with_scene(
        rig_bytes,
        clip_bytes,
        source,
        request.clip,
        clip::Limits {
            pose: crate::nif_animation::pose::Limits {
                scene: scene_limits,
                array_bytes: animation.array_bytes.min(budget.storage),
                work_units: animation.work_units.min(budget.work),
                ..animation
            },
            ..limits.clip
        },
    )?;
    budget.reserve::<u8>(sample.retained_bytes)?;
    budget.charge(sample.work_units)?;
    let rig = SampledRig {
        index,
        scene,
        sample,
    };
    let result = super::evaluate_inner(
        skin_bytes,
        rig_bytes,
        source,
        request.mapping,
        super::Limits {
            rig: scene_limits,
            array_bytes: external.array_bytes.min(budget.storage),
            work_units: external.work_units.min(budget.work),
            ..external
        },
        Some(&rig),
    )?;
    budget.reserve::<u8>(result.retained_bytes)?;
    budget.charge(result.work_units)?;
    Ok(Evaluation {
        contract: CONTRACT,
        sample: rig.sample,
        external: result,
        rig_scene_decodes: 1,
        skin_scene_decodes: 1,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        retained_bytes: limits.array_bytes - budget.storage,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
