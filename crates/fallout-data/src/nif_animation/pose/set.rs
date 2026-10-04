//! Explicit simultaneous channels; no priority, blending or playback clock.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct SetLimits {
    pub source: Limits,
    pub requests: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub sampling: sampling::Limits,
    pub ancestry_depth: usize,
}
impl Default for SetLimits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            requests: 256,
            array_bytes: 16 * 1024 * 1024,
            work_units: 2_000_000,
            sampling: Limits::default().sampling,
            ancestry_depth: 1024,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LocalObservation {
    pub object: SourceSpan,
    pub controller: SourceSpan,
    pub interpolator: SourceSpan,
    pub data: SourceSpan,
    pub requested_time_f64_bits: u64,
    pub source_local: SourceLocal,
    pub object_flags: u32,
    pub unapplied_controller_fields: Controller,
    pub unapplied_interpolator_fields: TransformInterpolator,
    pub translation: sampling::Diagnostic,
    pub scale: sampling::Diagnostic,
    pub local: Affine,
}

#[derive(Debug, Serialize)]
pub struct SetAncestor {
    pub source: SourceSpan,
    pub source_local: SourceLocal,
    pub flags: u32,
    pub parent: Option<u32>,
    /// Exact source object selected by a request, never a request-order priority.
    pub applied_object: Option<u32>,
    pub effective_local: Affine,
}

#[derive(Debug, Serialize)]
pub struct SetObjectPose {
    pub channel: LocalObservation,
    pub source_world: Affine,
    pub ancestors: Vec<SetAncestor>,
}

#[derive(Debug, Serialize)]
pub struct PoseSet {
    pub contract: &'static str,
    pub source_sha256: String,
    pub preparation: PreparationUsage,
    pub objects: Vec<SetObjectPose>,
    pub propagated_objects: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub sample_work: sampling::Usage,
    pub retail_behavior_verified: bool,
}

const IDENTITY: Affine = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];

/// Internal source authority. Only the existing evaluated CSR forest constructs
/// these maps; no public PoseSet, Scene or caller matrix can substitute them.
pub(crate) struct EvaluatedForest {
    prepared: PreparedSource,
    observation: PoseSet,
    selected: Vec<Option<usize>>,
    worlds: Vec<Option<Affine>>,
    relative: Vec<Option<Affine>>,
}
impl EvaluatedForest {
    pub(crate) fn observation(&self) -> &PoseSet {
        &self.observation
    }
    pub(crate) fn scene(&self) -> &nif_scene::Scene {
        &self.prepared.scene
    }
    pub(crate) fn world(&self, id: u32) -> Option<Affine> {
        self.worlds.get(id as usize).copied().flatten()
    }
    pub(crate) fn relative(&self, id: u32) -> Option<Affine> {
        self.relative.get(id as usize).copied().flatten()
    }
    pub(crate) fn applies(&self, id: u32) -> bool {
        self.selected.get(id as usize).is_some_and(Option::is_some)
    }
    pub(crate) fn into_observation(self) -> PoseSet {
        self.observation
    }
}
#[derive(Clone, Copy)]
struct RequiredScope<'a> {
    seeds: &'a [u32],
    anchor: u32,
}

fn validate_requests(requests: &[Request], limits: SetLimits, budget: &Budget<'_>) -> Result<()> {
    if requests.len() > limits.requests {
        return Err(budget.fail("pose set request count budget exceeded"));
    }
    if limits.ancestry_depth == 0 {
        return Err(budget.fail("ancestry depth budget exceeded"));
    }
    for request in requests {
        if !request.source_time.is_finite() {
            return Err(budget.fail("requested source time must be finite"));
        }
    }
    Ok(())
}

/// One request per controlled selected object; every controlled ancestor needed
/// by a selected result must also have an exact admitted request. No fallback.
pub fn evaluate_set(
    bytes: &[u8],
    source: &str,
    requests: &[Request],
    limits: SetLimits,
) -> Result<PoseSet> {
    let budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    validate_requests(requests, limits, &budget)?;
    let prepared = PreparedSource::prepare(bytes, source, limits.source)?;
    evaluate_prepared(prepared, requests, limits, budget, None)
        .map(EvaluatedForest::into_observation)
}

/// The skin decoder transfers one already bounded same-source Scene. Seed IDs
/// and anchor come from its exact decoded instance, never a public pose receipt.
pub(crate) fn evaluate_required(
    bytes: &[u8],
    source: &str,
    requests: &[Request],
    scene: nif_scene::Scene,
    seeds: &[u32],
    anchor: u32,
    limits: SetLimits,
) -> Result<EvaluatedForest> {
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    validate_requests(requests, limits, &budget)?;
    if seeds.is_empty() {
        return Err(budget.fail("required skin forest has no exact seeds"));
    }
    budget.reserve::<EvaluatedForest>(1)?;
    let prepared = PreparedSource::prepare_with_scene(bytes, source, limits.source, scene)?;
    evaluate_prepared(
        prepared,
        requests,
        limits,
        budget,
        Some(RequiredScope { seeds, anchor }),
    )
}

fn evaluate_prepared(
    prepared: PreparedSource,
    requests: &[Request],
    limits: SetLimits,
    mut budget: Budget<'_>,
    scope: Option<RequiredScope<'_>>,
) -> Result<EvaluatedForest> {
    let view = SourceView {
        source: &prepared.source,
        index: &prepared.index,
        decoded: &prepared.decoded,
        scene: &prepared.scene,
        storage: SourceStorage::Prepared(&prepared),
    };
    let blocks = prepared.index.blocks.len();
    budget.reserve::<Option<usize>>(blocks)?;
    budget.reserve::<bool>(blocks)?;
    budget.reserve::<Option<Affine>>(blocks)?;
    budget.reserve::<PoseSet>(1)?;
    budget.reserve::<u8>(64)?;
    budget.reserve::<SetObjectPose>(requests.len())?;
    let mut selected = vec![None; blocks];
    let mut required = vec![false; blocks];
    let mut worlds = vec![None; blocks];
    let mut relative = if scope.is_some() {
        budget.reserve::<Option<Affine>>(blocks)?;
        vec![None; blocks]
    } else {
        Vec::new()
    };
    for (ordinal, request) in requests.iter().enumerate() {
        budget.charge(1)?;
        let slot = selected
            .get_mut(request.object as usize)
            .ok_or_else(|| budget.fail("selected object is not decoded"))?;
        if slot.replace(ordinal).is_some() {
            return Err(budget.fail(&format!("duplicate pose set object {}", request.object)));
        }
    }
    // All required controlling ancestors are checked before channel admission.
    let explicit_seeds = scope.map(|scope| scope.seeds).unwrap_or(&[]);
    let requested_seeds = requests
        .iter()
        .filter(|_| scope.is_none())
        .map(|request| request.object);
    let required_seeds = explicit_seeds.iter().copied().chain(requested_seeds);
    for seed in required_seeds {
        let mut node = Some(seed);
        let mut depth = 0;
        while let Some(id) = node {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            let object = view
                .object(id)
                .ok_or_else(|| budget.fail("selected object is not decoded"))?;
            let world = prepared
                .worlds
                .get(id as usize)
                .copied()
                .flatten()
                .map(|i| prepared.scene.world_transforms[i])
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
    if scope.is_some() {
        budget.charge(requests.len())?;
        if requests.iter().any(|r| !required[r.object as usize]) {
            return Err(budget.fail("pose set channel is outside required skin forest"));
        }
    }
    let mut sampling_left = limits.sampling;
    let mut objects = Vec::with_capacity(requests.len());
    for &request in requests {
        budget.charge(4)?;
        let linked = admit_channels(&view, request, &budget)?;
        budget.reserve::<u8>(6 * 64)?;
        let mut sampling_budget = sampling::Budget::new(sampling_left);
        let (translation, scale, local) =
            sample_channels(&linked, request.source_time, &mut sampling_budget)?;
        let used = sampling_budget.usage();
        sampling_left.validation_work = sampling_left
            .validation_work
            .checked_sub(used.validation_units)
            .ok_or_else(|| budget.fail("pose set validation work exceeded"))?;
        sampling_left.sampling_work = sampling_left
            .sampling_work
            .checked_sub(used.sampling_units)
            .ok_or_else(|| budget.fail("pose set sampling work exceeded"))?;
        if !local.iter().flatten().all(|v| v.is_finite()) {
            return Err(budget.fail("evaluated matrix overflow"));
        }
        objects.push(SetObjectPose {
            channel: LocalObservation {
                object: view.source_span(request.object),
                controller: view.source_span(request.controller),
                interpolator: view.source_span(linked.interpolator_id),
                data: view.source_span(linked.data_id),
                requested_time_f64_bits: request.source_time.to_bits(),
                source_local: linked.object.transform.into(),
                object_flags: linked.object.flags,
                unapplied_controller_fields: linked.controller.clone(),
                unapplied_interpolator_fields: linked.interpolator.clone(),
                translation,
                scale,
                local,
            },
            source_world: IDENTITY,
            ancestors: Vec::new(),
        });
    }
    // Scene validates the forest, then sorts its public worlds by block ID.
    // Build a private required-child CSR and root queue from validated parents;
    // source block order never determines whether a parent has propagated.
    budget.reserve::<usize>(blocks)?;
    let offset_count = blocks
        .checked_add(1)
        .ok_or_else(|| budget.fail("required forest offset count overflow"))?;
    budget.reserve::<usize>(offset_count)?;
    budget.charge(blocks)?;
    let required_count = required.iter().filter(|&&value| value).count();
    budget.reserve::<u32>(required_count)?;
    let mut counts = vec![0usize; blocks];
    let mut queue = Vec::with_capacity(required_count);
    let mut edges = 0usize;
    budget.charge(prepared.scene.world_transforms.len())?;
    for world in &prepared.scene.world_transforms {
        if !required[world.block as usize] {
            continue;
        }
        match world.parent {
            Some(parent) => {
                if !required[parent as usize] {
                    return Err(budget.fail("required forest parent is missing"));
                }
                counts[parent as usize] = counts[parent as usize]
                    .checked_add(1)
                    .ok_or_else(|| budget.fail("required forest child count overflow"))?;
                edges = edges
                    .checked_add(1)
                    .ok_or_else(|| budget.fail("required forest edge count overflow"))?;
            }
            None => queue.push(world.block),
        }
    }
    budget.reserve::<u32>(edges)?;
    budget.charge(blocks)?;
    let mut offsets = Vec::with_capacity(offset_count);
    offsets.push(0usize);
    for (id, count) in counts.iter_mut().enumerate() {
        let next = offsets[id]
            .checked_add(*count)
            .ok_or_else(|| budget.fail("required forest prefix sum overflow"))?;
        *count = offsets[id];
        offsets.push(next);
    }
    if offsets[blocks] != edges {
        return Err(budget.fail("required forest edge count differs"));
    }
    let mut children = vec![0u32; edges];
    budget.charge(prepared.scene.world_transforms.len())?;
    for world in &prepared.scene.world_transforms {
        if required[world.block as usize]
            && let Some(parent) = world.parent
        {
            let parent = parent as usize;
            let slot = counts[parent];
            if slot >= offsets[parent + 1] {
                return Err(budget.fail("required forest child fill exceeds range"));
            }
            children[slot] = world.block;
            counts[parent] += 1;
        }
    }
    let mut propagated_objects = 0;
    while propagated_objects < queue.len() {
        budget.charge(1)?;
        let id = queue[propagated_objects] as usize;
        let world = &prepared.scene.world_transforms
            [prepared.worlds[id].expect("required reachable world validated")];
        let object = view
            .object(world.block)
            .ok_or_else(|| budget.fail("unresolved required object"))?;
        let local = selected[id]
            .map(|i| objects[i].channel.local)
            .unwrap_or_else(|| scene_affine(object.transform));
        let matrix = match world.parent {
            Some(parent) => compose(
                worlds[parent as usize]
                    .ok_or_else(|| budget.fail("required forest parent not propagated"))?,
                local,
            ),
            None => local,
        };
        if !matrix.iter().flatten().all(|v| v.is_finite()) {
            return Err(budget.fail("evaluated matrix overflow"));
        }
        worlds[id] = Some(matrix);
        if let Some(scope) = scope {
            budget.charge(1)?;
            relative[id] = if world.block == scope.anchor {
                Some(IDENTITY)
            } else {
                match world.parent.and_then(|parent| relative[parent as usize]) {
                    Some(parent) => {
                        let value = compose(parent, local);
                        if !value.iter().flatten().all(|v| v.is_finite()) {
                            return Err(budget.fail("evaluated root-relative matrix overflow"));
                        }
                        Some(value)
                    }
                    None => None,
                }
            };
        }
        propagated_objects += 1;
        let descendants = &children[offsets[id]..offsets[id + 1]];
        budget.charge(descendants.len())?;
        if queue
            .len()
            .checked_add(descendants.len())
            .is_none_or(|n| n > required_count)
        {
            return Err(budget.fail("required forest queue exceeds admitted objects"));
        }
        queue.extend_from_slice(descendants);
    }
    if propagated_objects != required_count {
        return Err(budget.fail("required forest could not propagate all objects"));
    }
    for ordinal in 0..objects.len() {
        let id = objects[ordinal].channel.object.block;
        objects[ordinal].source_world =
            worlds[id as usize].ok_or_else(|| budget.fail("selected world not propagated"))?;
        let mut parent = prepared.scene.world_transforms
            [prepared.worlds[id as usize].expect("selected reachable object validated")]
        .parent;
        while let Some(id) = parent {
            budget.charge(1)?;
            budget.reserve::<SetAncestor>(1)?;
            budget.reserve::<u8>(64)?;
            let object = view
                .object(id)
                .ok_or_else(|| budget.fail("unresolved required ancestor"))?;
            let world = prepared.scene.world_transforms
                [prepared.worlds[id as usize].expect("required ancestry validated")];
            let applied = selected[id as usize];
            let effective_local = applied
                .map(|i| objects[i].channel.local)
                .unwrap_or_else(|| scene_affine(object.transform));
            objects[ordinal].ancestors.push(SetAncestor {
                source: view.source_span(id),
                source_local: object.transform.into(),
                flags: object.flags,
                parent: world.parent,
                applied_object: applied.map(|_| id),
                effective_local,
            });
            parent = world.parent;
        }
    }
    let observation = PoseSet {
        contract: "engineering-explicit-linked-pose-set-v1",
        source_sha256: prepared.source_sha256().into(),
        preparation: prepared.usage(),
        objects,
        propagated_objects,
        retained_bytes: limits.array_bytes - budget.bytes,
        work_units: limits.work_units - budget.work,
        sample_work: sampling::Usage {
            validation_units: limits.sampling.validation_work - sampling_left.validation_work,
            sampling_units: limits.sampling.sampling_work - sampling_left.sampling_work,
        },
        retail_behavior_verified: false,
    };
    Ok(EvaluatedForest {
        prepared,
        observation,
        selected,
        worlds,
        relative,
    })
}
