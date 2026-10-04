//! Complete explicit required source forest feeds the existing skin evaluator.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct SetRequest {
    pub expected_source_sha256: [u8; 32],
    pub skin: Request,
}
#[derive(Clone, Copy, Debug)]
pub struct SetCombinedLimits {
    pub skin: Limits,
    pub animation: nif_animation::pose::SetLimits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for SetCombinedLimits {
    fn default() -> Self {
        Self {
            skin: Default::default(),
            animation: Default::default(),
            decoder_array_admission_bytes: 640 * 1024 * 1024,
            decoder_check_admission_units: 72_000_000,
            array_bytes: 96 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct EvaluationWithSet {
    pub contract: &'static str,
    pub pose_set: nif_animation::pose::PoseSet,
    pub skin: Evaluation,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub preparation_array_admission_bytes: usize,
    pub preparation_work_admission_units: usize,
    pub skin_scene_decodes: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}
pub fn evaluate_set_sampled(
    bytes: &[u8],
    source: &str,
    request: SetRequest,
    channels: &[nif_animation::pose::Request],
    limits: SetCombinedLimits,
) -> Result<EvaluationWithSet> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    if channels.len() > limits.animation.requests {
        return Err(budget.fail("pose set request count budget exceeded"));
    }
    if bytes.len() > limits.skin.source.partition.skin.scene.input_bytes
        || bytes.len() > limits.animation.source.scene.input_bytes
        || bytes.len() > limits.animation.source.keys.animation.input_bytes
    {
        return Err(budget.fail("pose set skin source input byte budget exceeded"));
    }
    budget.charge(
        bytes
            .len()
            .checked_mul(6)
            .ok_or_else(|| budget.fail("pose set source byte work product overflow"))?,
    )?;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if digest != request.expected_source_sha256 {
        return Err(budget.fail("pose set skin source SHA256 differs"));
    }
    validate_weight_policy(request.skin.weights, &budget)?;
    let source_limits = limits.animation.source;
    let decoder_arrays = [
        limits.skin.source.partition.skin.scene.array_bytes,
        limits.skin.source.partition.skin.skin_array_bytes,
        limits.skin.source.partition.array_bytes,
        limits.skin.source.array_bytes,
        source_limits.keys.max_combined_retained_bytes,
        source_limits.scene.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("combined decoder array admission exceeded"))?;
    let decoder_checks = [
        limits.skin.source.partition.skin.weight_index_checks,
        limits.skin.source.partition.index_checks,
        limits.skin.source.graph_checks,
        source_limits.keys.animation.reference_checks,
        source_limits.keys.key_work,
        limits.animation.sampling.validation_work,
        limits.animation.sampling.sampling_work,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("combined decoder check admission exceeded"))?;
    budget.reserve::<EvaluationWithSet>(1)?;
    // Preparation and set evaluation have independently bounded budgets. Admit
    // the full preparation allowance before either starts, then pass the actual
    // aggregate remainder into set and skin outputs. These are logical caps.
    budget.reserve::<u8>(source_limits.array_bytes)?;
    budget.charge(source_limits.work_units)?;
    let mut skin_source_limits = limits.skin.source;
    let scene_limits = &mut skin_source_limits.partition.skin.scene;
    scene_limits.blocks = scene_limits.blocks.min(source_limits.scene.blocks);
    scene_limits.array_bytes = scene_limits
        .array_bytes
        .min(source_limits.scene.array_bytes);
    scene_limits.input_bytes = scene_limits
        .input_bytes
        .min(source_limits.scene.input_bytes);
    let (index, decoded, scene) = binding::decode_with_scene(bytes, source, skin_source_limits)?;
    budget.charge(decoded.skin.skin.owners.len())?;
    budget.charge(decoded.skin.skin.blocks.len())?;
    let owner = decoded
        .skin
        .skin
        .owners
        .iter()
        .find(|o| o.geometry == request.skin.geometry)
        .ok_or_else(|| budget.fail("selected geometry has no decoded skin owner"))?;
    let instance = decoded
        .skin
        .skin
        .blocks
        .iter()
        .find(|b| b.block == owner.instance)
        .and_then(|b| match &b.data {
            Data::Instance { instance } => Some(instance),
            _ => None,
        })
        .ok_or_else(|| budget.fail("selected skin instance unresolved"))?;
    let root = instance
        .skeleton_root
        .ok_or_else(|| budget.fail("missing skeleton root"))?;
    let seed_count = instance
        .bones
        .len()
        .checked_add(2)
        .ok_or_else(|| budget.fail("required skin seed count overflow"))?;
    budget.reserve::<u32>(seed_count)?;
    budget.charge(seed_count)?;
    let mut seeds = Vec::with_capacity(seed_count);
    seeds.extend([root, request.skin.geometry]);
    for &node in &instance.bones {
        seeds.push(node.ok_or_else(|| budget.fail("missing bone"))?);
    }
    let forest = nif_animation::pose::evaluate_required(
        bytes,
        source,
        channels,
        scene,
        &seeds,
        root,
        nif_animation::pose::SetLimits {
            array_bytes: limits.animation.array_bytes.min(budget.storage),
            work_units: limits.animation.work_units.min(budget.work),
            ..limits.animation
        },
    )?;
    budget.reserve::<u8>(forest.observation().retained_bytes)?;
    budget.charge(forest.observation().work_units)?;
    let skin = evaluate_decoded(
        DecodedView {
            source,
            hash: SourceHash::Prepared(&forest.observation().source_sha256),
            index: &index,
            decoded: &decoded,
            scene: forest.scene(),
        },
        request.skin,
        Limits {
            array_bytes: limits.skin.array_bytes.min(budget.storage),
            work_units: limits.skin.work_units.min(budget.work),
            source: skin_source_limits,
            ..limits.skin
        },
        Some(SourcePoseOverride::Forest(&forest)),
        None,
        Budget {
            source,
            storage: limits.skin.array_bytes.min(budget.storage),
            work: limits.skin.work_units.min(budget.work),
        },
    )?;
    budget.reserve::<u8>(skin.retained_bytes)?;
    budget.charge(skin.work_units)?;
    Ok(EvaluationWithSet {
        contract: "engineering-complete-required-pose-set-skin-v1",
        pose_set: forest.into_observation(),
        skin,
        decoder_array_admission_bytes: decoder_arrays,
        decoder_check_admission_units: decoder_checks,
        preparation_array_admission_bytes: source_limits.array_bytes,
        preparation_work_admission_units: source_limits.work_units,
        skin_scene_decodes: 1,
        retained_bytes: limits.array_bytes - budget.storage,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}
