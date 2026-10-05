//! Source-local engineering skin evaluation. Stored locals need not be the bind
//! pose. Controller playback, external rigs and retail normal rules are separate.

use super::{Data, Transform, binding};
use crate::{Error, Result, nif, nif_animation, nif_scene};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod batch;
pub mod palette_packet;
pub mod partition;
mod set;
pub use batch::{
    BatchEvaluationLimits, BatchLimits, GeometryBatch, GeometryLimits, PreparationLimits,
    PreparationUsage, PreparedSkinSource, evaluate_many,
};
pub use set::{EvaluationWithSet, SetCombinedLimits, SetRequest, evaluate_set_sampled};

/// Column-vector affine rows, in original NIF coordinates and units.
pub type Affine = [[f64; 4]; 3];
const IDENTITY: Affine = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WeightPolicy {
    /// Preserve every nonnegative contribution, including duplicates/nonunit sums.
    PreserveRawNonnegative,
    /// Validate the raw sum; passing this policy still never normalizes it.
    RequireUnitSum { absolute_tolerance: f64 },
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub geometry: u32,
    pub weights: WeightPolicy,
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source: binding::Limits,
    /// Conservative element storage, including block maps and returned vectors.
    pub array_bytes: usize,
    /// Source scans, ancestry steps, palette entries, influences and vertex checks.
    pub work_units: usize,
    pub ancestry_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            array_bytes: 64 * 1024 * 1024,
            work_units: 16_000_000,
            ancestry_depth: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControllerPolicy {
    /// Every required controlled object besides the selected sample refuses.
    RefuseOtherRequired,
}

#[derive(Clone, Copy, Debug)]
pub struct SampledRequest {
    pub expected_source_sha256: [u8; 32],
    pub skin: Request,
    pub controller_policy: ControllerPolicy,
}

#[derive(Clone, Copy, Debug)]
pub struct CombinedLimits {
    pub skin: Limits,
    pub animation: nif_animation::pose::Limits,
    /// Sum of declared decoder array allowances, admitted before either decoder.
    /// Existing input/block caps separately bound index and scene graph tables.
    pub decoder_array_admission_bytes: usize,
    /// Sum of declared decoder/index/sampler check allowances, before evaluation.
    pub decoder_check_admission_units: usize,
    /// Aggregate charged helper/output elements, including released scratch.
    pub array_bytes: usize,
    /// Aggregate skin and animation pose traversal units (checks are above).
    pub work_units: usize,
}

impl Default for CombinedLimits {
    fn default() -> Self {
        Self {
            skin: Default::default(),
            animation: Default::default(),
            decoder_array_admission_bytes: 640 * 1024 * 1024,
            decoder_check_admission_units: 72_000_000,
            array_bytes: 72 * 1024 * 1024,
            work_units: 18_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EvaluationWithSample {
    pub contract: &'static str,
    pub controller_policy: ControllerPolicy,
    pub sample: nif_animation::pose::ObjectPose,
    pub skin: Evaluation,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

struct SampleOverride<'a> {
    sample: &'a nif_animation::pose::ObjectPose,
}
enum SourcePoseOverride<'a> {
    One(SampleOverride<'a>),
    Forest(&'a nif_animation::pose::EvaluatedForest),
}
impl SourcePoseOverride<'_> {
    fn sample(&self) -> Option<&nif_animation::pose::ObjectPose> {
        match self {
            Self::One(value) => Some(value.sample),
            Self::Forest(_) => None,
        }
    }
    fn forest(&self) -> Option<&nif_animation::pose::EvaluatedForest> {
        match self {
            Self::Forest(value) => Some(value),
            Self::One(_) => None,
        }
    }
    fn applies(&self, id: u32) -> bool {
        match self {
            Self::One(value) => value.sample.object.block == id,
            Self::Forest(value) => value.applies(id),
        }
    }
}

/// One explicit source-linked sample, never a public matrix or decoded catalogue
/// accepted as authority. All other required controllers refuse atomically.
pub fn evaluate_sampled(
    bytes: &[u8],
    source: &str,
    request: SampledRequest,
    animation_request: nif_animation::pose::Request,
    limits: CombinedLimits,
) -> Result<EvaluationWithSample> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    if bytes.len() > limits.skin.source.partition.skin.scene.input_bytes
        || bytes.len() > limits.animation.scene.input_bytes
        || bytes.len() > limits.animation.keys.animation.input_bytes
    {
        return Err(budget.fail("sampled skin source input byte budget exceeded"));
    }
    if <[u8; 32]>::from(Sha256::digest(bytes)) != request.expected_source_sha256 {
        return Err(budget.fail("sampled skin source SHA256 differs"));
    }
    // Admission reserves the complete independently bounded decoder allowances;
    // it is deliberately conservative, not observed retained heap usage.
    let decoder_arrays = [
        limits.skin.source.partition.skin.scene.array_bytes,
        limits.skin.source.partition.skin.skin_array_bytes,
        limits.skin.source.partition.array_bytes,
        limits.skin.source.array_bytes,
        limits.animation.keys.max_combined_retained_bytes,
        limits.animation.scene.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |sum, n| sum.checked_add(n))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("combined decoder array admission exceeded"))?;
    let decoder_checks = [
        limits.skin.source.partition.skin.weight_index_checks,
        limits.skin.source.partition.index_checks,
        limits.skin.source.graph_checks,
        limits.animation.keys.animation.reference_checks,
        limits.animation.keys.key_work,
        limits.animation.sampling.validation_work,
        limits.animation.sampling.sampling_work,
    ]
    .into_iter()
    .try_fold(0usize, |sum, n| sum.checked_add(n))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("combined decoder check admission exceeded"))?;
    budget.reserve::<EvaluationWithSample>(1)?;
    let sample = nif_animation::pose::evaluate(
        bytes,
        source,
        animation_request,
        nif_animation::pose::Limits {
            array_bytes: limits.animation.array_bytes.min(budget.storage),
            work_units: limits.animation.work_units.min(budget.work),
            ..limits.animation
        },
    )?;
    budget.reserve::<u8>(sample.retained_bytes)?;
    budget.charge(sample.work_units)?;
    let skin = evaluate_inner(
        bytes,
        source,
        request.skin,
        Limits {
            array_bytes: limits.skin.array_bytes.min(budget.storage),
            work_units: limits.skin.work_units.min(budget.work),
            ..limits.skin
        },
        Some(SampleOverride { sample: &sample }),
        None,
    )?;
    budget.reserve::<u8>(skin.retained_bytes)?;
    budget.charge(skin.work_units)?;
    Ok(EvaluationWithSample {
        contract: "engineering-one-linked-sample-skin-v1",
        controller_policy: request.controller_policy,
        sample,
        skin,
        decoder_array_admission_bytes: decoder_arrays,
        decoder_check_admission_units: decoder_checks,
        retained_bytes: limits.array_bytes - budget.storage,
        work_units: limits.work_units - budget.work,
        retail_behavior_verified: false,
    })
}

#[derive(Debug, Serialize)]
pub struct BonePalette {
    pub ordinal: usize,
    pub node: u32,
    /// SkinTransform * BoneToRoot * authored SkinToBone, without weight repair.
    pub matrix: Affine,
}

#[derive(Debug, Serialize)]
pub struct UnappliedController {
    pub object: u32,
    pub controller: u32,
}

#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub source_sha256: String,
    pub geometry: u32,
    pub geometry_data: u32,
    pub instance: u32,
    pub skin_data: u32,
    pub skeleton_root: u32,
    pub weights: WeightPolicy,
    /// Apply this to the returned vertices; do not also apply the geometry local.
    pub skin_to_source_world: Affine,
    pub palette: Vec<BonePalette>,
    pub positions: Vec<[f64; 3]>,
    /// Weighted linear directions, without inverse-transpose or renormalization.
    pub normals: Vec<[f64; 3]>,
    pub weight_sums: Vec<f64>,
    pub unapplied_controllers: Vec<UnappliedController>,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

pub(super) struct Budget<'a> {
    pub(super) source: &'a str,
    pub(super) storage: usize,
    pub(super) work: usize,
}

impl Budget<'_> {
    pub(super) fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!("{}: source-local skin pose: {detail}", self.source))
    }
    pub(super) fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.storage = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|n| self.storage.checked_sub(n))
            .ok_or_else(|| self.fail("array storage budget exceeded"))?;
        Ok(())
    }
    pub(super) fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(count)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
}

pub(crate) fn compose(parent: Affine, local: Affine) -> Affine {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3).map(|k| parent[r][k] * local[k][c]).sum::<f64>()
                + if c == 3 { parent[r][3] } else { 0. }
        })
    })
}

pub(super) fn skin_affine(t: &Transform) -> Affine {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c == 3 {
                f64::from(f32::from_bits(t.translation_bits[r]))
            } else {
                f64::from(f32::from_bits(t.rotation_bits[r][c]))
                    * f64::from(f32::from_bits(t.scale_bits))
            }
        })
    })
}

pub(crate) fn scene_affine(t: nif_scene::Transform) -> Affine {
    std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c == 3 {
                f64::from(t.translation[r])
            } else {
                f64::from(t.rotation[r][c]) * f64::from(t.scale)
            }
        })
    })
}

pub(super) fn finite(matrix: Affine, budget: &Budget<'_>) -> Result<Affine> {
    if matrix.iter().flatten().all(|v| v.is_finite()) {
        Ok(matrix)
    } else {
        Err(budget.fail("matrix overflow"))
    }
}

pub(super) fn inverse(matrix: Affine, budget: &Budget<'_>) -> Result<Affine> {
    // General 3x3 inverse: source rotations are not silently orthogonalized.
    let cofactors: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            let a = (r + 1) % 3;
            let b = (r + 2) % 3;
            let x = (c + 1) % 3;
            let y = (c + 2) % 3;
            matrix[a][x] * matrix[b][y] - matrix[a][y] * matrix[b][x]
        })
    });
    let determinant = (0..3).map(|c| matrix[0][c] * cofactors[0][c]).sum::<f64>();
    if determinant == 0. || !determinant.is_finite() {
        return Err(budget.fail("singular or overflowing SkinTransform"));
    }
    let mut result = IDENTITY;
    for (r, row) in result.iter_mut().enumerate() {
        for (c, value) in row[..3].iter_mut().enumerate() {
            *value = cofactors[c][r] / determinant;
        }
        row[3] = -(0..3).map(|c| row[c] * matrix[c][3]).sum::<f64>();
    }
    finite(result, budget)
}

fn apply(matrix: Affine, value: [f32; 3], point: bool) -> [f64; 3] {
    std::array::from_fn(|r| {
        (0..3)
            .map(|c| matrix[r][c] * f64::from(value[c]))
            .sum::<f64>()
            + if point { matrix[r][3] } else { 0. }
    })
}

/// Decode once through the existing skin/scene pipeline. The caller supplies an
/// exact block and an explicit raw-weight policy, never a bone-name guess.
pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
) -> Result<Evaluation> {
    evaluate_inner(bytes, source, request, limits, None, None)
}

pub(super) fn evaluate_table(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
    table: &super::influences::Table,
) -> Result<Evaluation> {
    evaluate_inner(bytes, source, request, limits, None, Some(table))
}

fn evaluate_inner(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
    selected: Option<SampleOverride<'_>>,
    table: Option<&super::influences::Table>,
) -> Result<Evaluation> {
    let budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    validate_weight_policy(request.weights, &budget)?;
    let (index, decoded, scene) = binding::decode_with_scene(bytes, source, limits.source)?;
    evaluate_decoded(
        DecodedView {
            hash: SourceHash::Input(bytes),
            source,
            index: &index,
            decoded: &decoded,
            scene: &scene,
        },
        request,
        limits,
        selected.map(SourcePoseOverride::One),
        table,
        budget,
    )
}

fn validate_weight_policy(weights: WeightPolicy, budget: &Budget<'_>) -> Result<()> {
    if let WeightPolicy::RequireUnitSum { absolute_tolerance } = weights
        && (!absolute_tolerance.is_finite() || !(0. ..=1.).contains(&absolute_tolerance))
    {
        return Err(budget.fail("weight tolerance must be finite in [0,1]"));
    }
    Ok(())
}

/// Private borrowed authority created only by the existing source decoder.
#[derive(Clone, Copy)]
enum SourceHash<'a> {
    Input(&'a [u8]),
    Prepared(&'a str),
}
#[derive(Clone, Copy)]
struct DecodedView<'a> {
    source: &'a str,
    hash: SourceHash<'a>,
    index: &'a nif::NifIndex,
    decoded: &'a binding::Source,
    scene: &'a nif_scene::Scene,
}

fn evaluate_decoded(
    view: DecodedView<'_>,
    request: Request,
    limits: Limits,
    selected: Option<SourcePoseOverride<'_>>,
    table: Option<&super::influences::Table>,
    mut budget: Budget<'_>,
) -> Result<Evaluation> {
    let DecodedView {
        hash,
        source,
        index,
        decoded,
        scene,
    } = view;
    // An unknown node may contain a hidden link to a selected bone. The old
    // catalogue deliberately certifies only decoded ancestry, so refuse here.
    if !decoded.bindings.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.charge(
        scene.objects.len()
            + scene.world_transforms.len()
            + decoded.skin.skin.owners.len()
            + decoded.skin.skin.blocks.len()
            + decoded.bindings.instances.len()
            + scene.meshes.len(),
    )?;
    let owner = decoded
        .skin
        .skin
        .owners
        .iter()
        .find(|o| o.geometry == request.geometry)
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
    let bound = decoded
        .bindings
        .instances
        .iter()
        .find(|i| i.instance == owner.instance)
        .ok_or_else(|| budget.fail("missing selected binding"))?;
    budget.charge(bound.bones.len() + bound.owners.len() + decoded.skin.skin.blocks.len())?;
    if !bound.skeleton_root.decoded_node
        || bound.skeleton_root.reachable_from_footer != Some(true)
        || bound.bones.iter().any(|b| {
            !b.node.decoded_node
                || b.node.reachable_from_footer != Some(true)
                || b.decoded_root_contains != Some(true)
        })
        || bound
            .owners
            .iter()
            .find(|o| o.geometry == request.geometry)
            .is_none_or(|o| !o.reachable_from_footer || o.decoded_root_contains != Some(true))
    {
        return Err(budget.fail("unresolved root, bone or owner ancestry"));
    }
    let data_id = instance
        .data
        .ok_or_else(|| budget.fail("missing NiSkinData"))?;
    let (skin_transform, bones) = decoded
        .skin
        .skin
        .blocks
        .iter()
        .find(|b| b.block == data_id)
        .and_then(|b| match &b.data {
            Data::SkinData {
                transform,
                has_vertex_weights,
                bones,
            } if *has_vertex_weights != 0 => Some((transform, bones)),
            _ => None,
        })
        .ok_or_else(|| budget.fail("NiSkinData vertex weights unavailable"))?;
    let geometry_data = owner
        .geometry_data
        .ok_or_else(|| budget.fail("geometry data unavailable"))?;
    let mesh = scene
        .meshes
        .iter()
        .find(|m| m.block == geometry_data)
        .ok_or_else(|| budget.fail("geometry data not decoded"))?;
    if !mesh.has_vertices
        || mesh.vertices.is_empty()
        || mesh.vertices.len() != usize::from(mesh.vertex_count)
    {
        return Err(budget.fail("vertex positions unavailable"));
    }
    if bones.is_empty() || bones.len() != instance.bones.len() {
        return Err(budget.fail("empty or mismatched bone palette"));
    }
    if table.is_some_and(|t| {
        !t.matches(
            geometry_data,
            owner.instance,
            data_id,
            bones.len(),
            mesh.vertices.len(),
        )
    }) {
        return Err(budget.fail("influence table selected source binding differs"));
    }
    budget.reserve::<Option<usize>>(index.blocks.len() * 2)?;
    budget.reserve::<bool>(index.blocks.len())?;
    let mut objects = vec![None; index.blocks.len()];
    let mut worlds = vec![None; index.blocks.len()];
    let mut controller_seen = vec![false; index.blocks.len()];
    for (i, object) in scene.objects.iter().enumerate() {
        objects[object.block as usize] = Some(i);
    }
    for (i, world) in scene.world_transforms.iter().enumerate() {
        worlds[world.block as usize] = Some(i);
    }
    let world = |node: u32| -> Result<&nif_scene::WorldTransform> {
        worlds
            .get(node as usize)
            .copied()
            .flatten()
            .map(|i| &scene.world_transforms[i])
            .ok_or_else(|| {
                Error::Unsupported(format!(
                    "{source}: source-local skin pose: missing node {node}"
                ))
            })
    };
    let mut root_world = world(root)?.matrix;
    if let Some(sample) = selected.as_ref().and_then(SourcePoseOverride::sample) {
        // Verify membership before palette/output allocation. Owner locals are
        // deliberately excluded: geometry transforms are not applied twice.
        let mut affects_skin = false;
        for start in std::iter::once(root).chain(instance.bones.iter().flatten().copied()) {
            let mut cursor = Some(start);
            let mut depth = 0;
            while let Some(id) = cursor {
                budget.charge(1)?;
                depth += 1;
                if depth > limits.ancestry_depth {
                    return Err(budget.fail("ancestry depth budget exceeded"));
                }
                affects_skin |= id == sample.object.block;
                if start != root && id == root {
                    break;
                }
                cursor = world(id)?.parent;
            }
        }
        if !affects_skin {
            return Err(budget.fail(&format!(
                "sampled node {} is outside selected skin root/bone ancestry",
                sample.object.block
            )));
        }
        let mut cursor = Some(root);
        let mut relative = IDENTITY;
        let mut depth = 0;
        while let Some(id) = cursor {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            if id == sample.object.block {
                root_world = finite(compose(sample.source_world, relative), &budget)?;
                break;
            }
            let object = &scene.objects
                [objects[id as usize].ok_or_else(|| budget.fail("undecoded root ancestor"))?];
            relative = finite(compose(scene_affine(object.transform), relative), &budget)?;
            cursor = world(id)?.parent;
        }
    } else if let Some(forest) = selected.as_ref().and_then(SourcePoseOverride::forest) {
        budget.charge(1)?;
        root_world = forest
            .world(root)
            .ok_or_else(|| budget.fail("required skin root world unavailable"))?;
    }
    let skin = skin_affine(skin_transform);
    let skin_to_source_world = finite(compose(root_world, inverse(skin, &budget)?), &budget)?;
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<u8>(64)?;
    budget.reserve::<BonePalette>(bones.len())?;
    budget.reserve::<[f64; 3]>(mesh.vertices.len() + mesh.normals.len())?;
    budget.reserve::<f64>(mesh.vertices.len())?;
    let mut result = Evaluation {
        contract: if selected
            .as_ref()
            .and_then(SourcePoseOverride::forest)
            .is_some()
        {
            "engineering-complete-required-pose-set-skin-v1"
        } else if selected.is_some() {
            "engineering-one-linked-sample-skin-v1"
        } else if table.is_some() {
            "engineering-exact-csr-source-local-skin-v1"
        } else {
            "engineering-source-local-skin-v1"
        },
        source_sha256: match hash {
            SourceHash::Prepared(digest) => digest.to_owned(),
            SourceHash::Input(bytes) => format!("{:x}", Sha256::digest(bytes)),
        },
        geometry: request.geometry,
        geometry_data,
        instance: owner.instance,
        skin_data: data_id,
        skeleton_root: root,
        weights: request.weights,
        skin_to_source_world,
        palette: Vec::with_capacity(bones.len()),
        positions: vec![[0.; 3]; mesh.vertices.len()],
        normals: vec![[0.; 3]; mesh.normals.len()],
        weight_sums: vec![0.; mesh.vertices.len()],
        unapplied_controllers: Vec::new(),
        retained_bytes: 0,
        work_units: 0,
        retail_behavior_verified: false,
    };
    // Record root/owner ancestry as well as palette paths, since all source
    // transforms are static even when an ancestor carries a controller.
    for start in [root, request.geometry] {
        let mut node = Some(start);
        let mut depth = 0;
        while let Some(id) = node {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            record_controller(
                id,
                scene,
                &objects,
                &mut controller_seen,
                &mut result,
                &mut budget,
                selected.as_ref(),
            )?;
            node = world(id)?.parent;
        }
    }
    for (ordinal, bone) in bones.iter().enumerate() {
        budget.charge(1)?;
        let node = instance.bones[ordinal].ok_or_else(|| budget.fail("missing bone"))?;
        let mut cursor = node;
        let forest = selected.as_ref().and_then(SourcePoseOverride::forest);
        let mut relative = if let Some(forest) = forest {
            budget.charge(1)?;
            forest
                .relative(node)
                .ok_or_else(|| budget.fail("required root-relative bone world unavailable"))?
        } else {
            IDENTITY
        };
        let mut depth = 0;
        while cursor != root {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("ancestry depth budget exceeded"));
            }
            record_controller(
                cursor,
                scene,
                &objects,
                &mut controller_seen,
                &mut result,
                &mut budget,
                selected.as_ref(),
            )?;
            let object = &scene.objects
                [objects[cursor as usize].ok_or_else(|| budget.fail("undecoded bone node"))?];
            if forest.is_none() {
                let local = match selected.as_ref().and_then(SourcePoseOverride::sample) {
                    Some(sample) if cursor == sample.object.block => sample.local,
                    _ => scene_affine(object.transform),
                };
                relative = finite(compose(local, relative), &budget)?;
            }
            cursor = world(cursor)?
                .parent
                .ok_or_else(|| budget.fail("bone chain does not reach root"))?;
        }
        let matrix = finite(
            compose(skin, compose(relative, skin_affine(&bone.transform))),
            &budget,
        )?;
        result.palette.push(BonePalette {
            ordinal,
            node,
            matrix,
        });
        if table.is_none() {
            for weight in &bone.weights {
                accumulate_weight(
                    matrix,
                    weight.weight_bits,
                    usize::from(weight.vertex),
                    mesh,
                    &mut result,
                    &mut budget,
                )?;
            }
        }
    }
    if let Some(table) = table {
        for vertex in 0..mesh.vertices.len() {
            budget.charge(1)?;
            let entries = table
                .vertex(vertex)
                .ok_or_else(|| budget.fail("influence table vertex range unavailable"))?;
            for entry in entries {
                let matrix = result
                    .palette
                    .get(entry.bone_ordinal)
                    .ok_or_else(|| budget.fail("influence table bone ordinal out of range"))?
                    .matrix;
                accumulate_weight(
                    matrix,
                    entry.weight_bits,
                    vertex,
                    mesh,
                    &mut result,
                    &mut budget,
                )?;
            }
        }
    }
    budget.charge(result.positions.len() + result.normals.len())?;
    if !result
        .positions
        .iter()
        .chain(&result.normals)
        .flatten()
        .all(|v| v.is_finite())
    {
        return Err(budget.fail("deformed vertex or normal overflow"));
    }
    for (vertex, &sum) in result.weight_sums.iter().enumerate() {
        if let Some(detail) = super::weight_sum_error(vertex, sum, request.weights) {
            return Err(budget.fail(&detail));
        }
    }
    result.retained_bytes = limits.array_bytes - budget.storage;
    result.work_units = limits.work_units - budget.work;
    Ok(result)
}

pub(super) fn accumulate_weight(
    matrix: Affine,
    bits: u32,
    vertex: usize,
    mesh: &nif_scene::MeshData,
    result: &mut Evaluation,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.charge(1)?;
    let value = super::raw_weight(bits).map_err(|detail| budget.fail(detail))?;
    let position = apply(matrix, mesh.vertices[vertex], true);
    for (out, component) in result.positions[vertex].iter_mut().zip(position) {
        *out += value * component;
    }
    if mesh.has_normals {
        let normal = apply(matrix, mesh.normals[vertex], false);
        for (out, component) in result.normals[vertex].iter_mut().zip(normal) {
            *out += value * component;
        }
    }
    result.weight_sums[vertex] += value;
    Ok(())
}

fn record_controller(
    block: u32,
    scene: &nif_scene::Scene,
    objects: &[Option<usize>],
    seen: &mut [bool],
    result: &mut Evaluation,
    budget: &mut Budget<'_>,
    selected: Option<&SourcePoseOverride<'_>>,
) -> Result<()> {
    if !seen[block as usize] {
        seen[block as usize] = true;
        let object = &scene.objects
            [objects[block as usize].ok_or_else(|| budget.fail("unresolved controlled object"))?];
        if let Some(controller) = object.controller {
            if let Some(selected) = selected {
                if selected.applies(block) {
                    return Ok(());
                }
                return Err(budget.fail(&format!(
                    "required object {block} controller {controller} is unapplied"
                )));
            }
            budget.reserve::<UnappliedController>(1)?;
            result.unapplied_controllers.push(UnappliedController {
                object: block,
                controller,
            });
        }
    }
    Ok(())
}
