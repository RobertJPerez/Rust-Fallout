//! Explicit two-source engineering root/bone mapping, never automatic rig choice.
mod determinant;
pub mod sampled;
use super::{Data, binding, pose};
use crate::{Result, nif, nif_animation::clip, nif_scene};
use pose::{Affine, Budget};
use serde::Serialize;
use sha2::{Digest, Sha256};

const IDENTITY: Affine = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];

#[derive(Debug)]
pub struct BoneMapping {
    pub bone_ordinal: usize,
    pub rig_node: u32,
    pub expected_skin_bone_name_bytes: Vec<u8>,
    pub expected_rig_node_name_bytes: Vec<u8>,
}
#[derive(Debug)]
pub struct Request {
    pub expected_skin_sha256: [u8; 32],
    pub expected_rig_sha256: [u8; 32],
    pub geometry: u32,
    pub rig_root: u32,
    pub explicit_bone_mapping: Vec<BoneMapping>,
    /// Maps rig-root coordinates into the declared skin-root coordinates.
    pub explicit_root_space_mapping: Affine,
    pub weights: pose::WeightPolicy,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub skin: binding::Limits,
    pub rig: nif_scene::Limits,
    pub source_bytes: usize,
    pub mapping_bones: usize,
    pub raw_name_bytes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            skin: Default::default(),
            rig: Default::default(),
            source_bytes: 128 * 1024 * 1024,
            mapping_bones: 4096,
            raw_name_bytes: 1024 * 1024,
            decoder_array_admission_bytes: 640 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            array_bytes: 64 * 1024 * 1024,
            work_units: 16_000_000,
            ancestry_depth: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct SourceLocation {
    pub block: u32,
    pub offset: usize,
    pub bytes: usize,
}
#[derive(Debug, Serialize)]
pub struct MappedBone {
    pub bone_ordinal: usize,
    pub skin_node: SourceLocation,
    pub rig_node: SourceLocation,
    pub raw_skin_bone_name_bytes: Vec<u8>,
    pub raw_rig_node_name_bytes: Vec<u8>,
    pub rig_bone_to_root: Affine,
    pub authored_skin_to_bone: Affine,
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub contract: &'static str,
    pub skin_source_sha256: String,
    pub rig_source_sha256: String,
    pub rig_root: SourceLocation,
    pub unapplied_rig_root_local: Affine,
    pub unapplied_rig_root_source_world: Affine,
    pub explicit_root_space_mapping: Affine,
    pub mappings: Vec<MappedBone>,
    pub rig_unapplied_controllers: Vec<pose::UnappliedController>,
    pub skin: pose::Evaluation,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub skin_source_retained_bytes: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

struct Forest<'a> {
    scene: &'a nif_scene::Scene,
    objects: Vec<Option<usize>>,
    worlds: Vec<Option<usize>>,
}
/// Created only by the exact three-source adapter from the existing clip core.
struct SampledRig {
    index: nif::NifIndex,
    scene: nif_scene::Scene,
    sample: clip::Evaluation,
}
impl<'a> Forest<'a> {
    fn new(scene: &'a nif_scene::Scene, blocks: usize, budget: &mut Budget<'_>) -> Result<Self> {
        budget.reserve::<Option<usize>>(
            blocks
                .checked_mul(2)
                .ok_or_else(|| budget.fail("external forest map count overflow"))?,
        )?;
        budget.charge(scene.objects.len() + scene.world_transforms.len())?;
        let mut objects = vec![None; blocks];
        let mut worlds = vec![None; blocks];
        for (i, object) in scene.objects.iter().enumerate() {
            objects[object.block as usize] = Some(i);
        }
        for (i, world) in scene.world_transforms.iter().enumerate() {
            worlds[world.block as usize] = Some(i);
        }
        Ok(Self {
            scene,
            objects,
            worlds,
        })
    }
    fn object(&self, id: u32, budget: &Budget<'_>) -> Result<&nif_scene::Object> {
        self.objects
            .get(id as usize)
            .copied()
            .flatten()
            .map(|i| &self.scene.objects[i])
            .ok_or_else(|| budget.fail(&format!("external source object {id} is not decoded")))
    }
    fn world(&self, id: u32, budget: &Budget<'_>) -> Result<&nif_scene::WorldTransform> {
        self.worlds
            .get(id as usize)
            .copied()
            .flatten()
            .map(|i| &self.scene.world_transforms[i])
            .filter(|w| w.reachable_from_footer)
            .ok_or_else(|| {
                budget.fail(&format!(
                    "external source object {id} has no reachable ancestry"
                ))
            })
    }
    fn node(&self, id: u32, budget: &Budget<'_>) -> Result<&nif_scene::Object> {
        let object = self.object(id, budget)?;
        if !matches!(object.kind, nif_scene::ObjectKind::Node { .. }) {
            return Err(budget.fail("external selected bone/root is not a decoded node"));
        }
        Ok(object)
    }
}
fn location(index: &nif::NifIndex, id: u32) -> SourceLocation {
    let block = &index.blocks[id as usize];
    SourceLocation {
        block: id,
        offset: block.offset,
        bytes: block.bytes,
    }
}
fn verify_name<'a>(
    index: &'a nif::NifIndex,
    object: &nif_scene::Object,
    expected: &[u8],
    budget: &mut Budget<'_>,
) -> Result<&'a [u8]> {
    let id = object
        .name
        .ok_or_else(|| budget.fail("external mapped node has no authored raw name"))?;
    let name = index
        .strings
        .get(id as usize)
        .ok_or_else(|| budget.fail("external mapped name index unavailable"))?;
    budget.charge(expected.len())?;
    if name != expected {
        return Err(budget.fail(&format!(
            "external raw name differs at object {}",
            object.block
        )));
    }
    Ok(name)
}

fn record(
    id: u32,
    forest: &Forest<'_>,
    seen: &mut [bool],
    out: &mut Vec<pose::UnappliedController>,
    budget: &mut Budget<'_>,
    applied: Option<u32>,
) -> Result<()> {
    if !seen[id as usize] {
        seen[id as usize] = true;
        if let Some(controller) = forest.object(id, budget)?.controller {
            if let Some(applied) = applied {
                if id != applied {
                    return Err(budget.fail("other required rig controller is unapplied"));
                }
                return Ok(());
            }
            budget.reserve::<pose::UnappliedController>(1)?;
            out.push(pose::UnappliedController {
                object: id,
                controller,
            });
        }
    }
    Ok(())
}

pub fn evaluate(
    skin_bytes: &[u8],
    rig_bytes: &[u8],
    source: &str,
    request: &Request,
    limits: Limits,
) -> Result<Evaluation> {
    evaluate_inner(skin_bytes, rig_bytes, source, request, limits, None)
}

fn evaluate_inner(
    skin_bytes: &[u8],
    rig_bytes: &[u8],
    source: &str,
    request: &Request,
    limits: Limits,
    sampled: Option<&SampledRig>,
) -> Result<Evaluation> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    if skin_bytes.len() > limits.skin.partition.skin.scene.input_bytes
        || rig_bytes.len() > limits.rig.input_bytes
        || skin_bytes
            .len()
            .checked_add(rig_bytes.len())
            .is_none_or(|n| n > limits.source_bytes)
    {
        return Err(budget.fail("external combined source input byte budget exceeded"));
    }
    if request.explicit_bone_mapping.len() > limits.mapping_bones {
        return Err(budget.fail("external mapping count budget exceeded"));
    }
    if limits.ancestry_depth == 0 {
        return Err(budget.fail("external ancestry depth budget exceeded"));
    }
    let name_bytes = request
        .explicit_bone_mapping
        .iter()
        .try_fold(0usize, |sum, m| {
            sum.checked_add(m.expected_skin_bone_name_bytes.len())?
                .checked_add(m.expected_rig_node_name_bytes.len())
        })
        .filter(|n| *n <= limits.raw_name_bytes)
        .ok_or_else(|| budget.fail("external raw name byte budget exceeded"))?;
    if let pose::WeightPolicy::RequireUnitSum { absolute_tolerance } = request.weights
        && (!absolute_tolerance.is_finite() || !(0. ..=1.).contains(&absolute_tolerance))
    {
        return Err(budget.fail("weight tolerance must be finite in [0,1]"));
    }
    if !request
        .explicit_root_space_mapping
        .iter()
        .flatten()
        .all(|v| v.is_finite())
    {
        return Err(budget.fail("external root-space mapping must be finite"));
    }
    // Invertibility is required by this explicit root-space relationship even
    // though forward palette composition itself does not use its inverse.
    determinant::certify(request.explicit_root_space_mapping, &mut budget)?;
    pose::inverse(request.explicit_root_space_mapping, &budget)
        .map_err(|_| budget.fail("external root-space mapping is singular or overflowing"))?;
    let skin_digest = Sha256::digest(skin_bytes);
    let rig_digest = Sha256::digest(rig_bytes);
    if <[u8; 32]>::from(skin_digest) != request.expected_skin_sha256 {
        return Err(budget.fail("external skin source SHA256 differs"));
    }
    if <[u8; 32]>::from(rig_digest) != request.expected_rig_sha256 {
        return Err(budget.fail("external rig source SHA256 differs"));
    }
    let arrays = [
        limits.skin.partition.skin.scene.array_bytes,
        limits.skin.partition.skin.skin_array_bytes,
        limits.skin.partition.array_bytes,
        limits.skin.array_bytes,
        limits.rig.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |s, n| s.checked_add(n))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("external combined decoder array admission exceeded"))?;
    let checks = [
        limits.skin.partition.skin.weight_index_checks,
        limits.skin.partition.index_checks,
        limits.skin.graph_checks,
    ]
    .into_iter()
    .try_fold(0usize, |s, n| s.checked_add(n))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("external combined decoder check admission exceeded"))?;
    let (skin_index, decoded, skin_scene) =
        binding::decode_with_scene(skin_bytes, source, limits.skin)?;
    let decoded_rig;
    let (rig_index, rig_scene) = match sampled {
        Some(rig) => (&rig.index, &rig.scene),
        None => {
            decoded_rig = nif_scene::decode_with_limits(rig_bytes, source, limits.rig)?;
            (&decoded_rig.0, &decoded_rig.1)
        }
    };
    if !skin_scene.unsupported_scene_edges.is_empty()
        || !rig_scene.unsupported_scene_edges.is_empty()
    {
        return Err(budget.fail("external unresolved scene ancestry"));
    }
    budget.charge(
        decoded.skin.skin.owners.len()
            + decoded.skin.skin.blocks.len()
            + decoded.bindings.instances.len()
            + skin_scene.meshes.len(),
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
        .ok_or_else(|| budget.fail("external selected skin instance unresolved"))?;
    let skin_root = instance
        .skeleton_root
        .ok_or_else(|| budget.fail("external missing skin root"))?;
    let data_id = instance
        .data
        .ok_or_else(|| budget.fail("external missing NiSkinData"))?;
    budget.charge(decoded.skin.skin.blocks.len())?;
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
    if bones.is_empty()
        || bones.len() != instance.bones.len()
        || bones.len() != request.explicit_bone_mapping.len()
    {
        return Err(budget.fail("external mapping must cover every nonempty skin bone ordinal"));
    }
    let geometry_data = owner
        .geometry_data
        .ok_or_else(|| budget.fail("geometry data unavailable"))?;
    let mesh = skin_scene
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
    let skin_forest = Forest::new(&skin_scene, skin_index.blocks.len(), &mut budget)?;
    let rig_forest = Forest::new(rig_scene, rig_index.blocks.len(), &mut budget)?;
    skin_forest.node(skin_root, &budget)?;
    let skin_root_world = skin_forest.world(skin_root, &budget)?.matrix;
    let bound = decoded
        .bindings
        .instances
        .iter()
        .find(|b| b.instance == owner.instance)
        .ok_or_else(|| budget.fail("external skin binding unavailable"))?;
    budget.charge(bound.owners.len())?;
    if bound
        .owners
        .iter()
        .find(|o| o.geometry == request.geometry)
        .is_none_or(|o| !o.reachable_from_footer || o.decoded_root_contains != Some(true))
    {
        return Err(budget.fail("external skin owner is outside its declared root"));
    }
    let rig_root_object = rig_forest.node(request.rig_root, &budget)?;
    let rig_root_world = rig_forest.world(request.rig_root, &budget)?.matrix;
    budget.reserve::<Option<usize>>(bones.len())?;
    budget.reserve::<bool>(rig_index.blocks.len())?;
    budget.reserve::<bool>(skin_index.blocks.len() + rig_index.blocks.len())?;
    budget.reserve::<Affine>(bones.len())?;
    let mut ordinals = vec![None; bones.len()];
    let mut targets = vec![false; rig_index.blocks.len()];
    let mut skin_seen = vec![false; skin_index.blocks.len()];
    let mut rig_seen = vec![false; rig_index.blocks.len()];
    let mut relative = vec![IDENTITY; bones.len()];
    let mut selected_on_mapped_path = false;
    // Complete map/type/name admission precedes output palette construction.
    for (i, mapping) in request.explicit_bone_mapping.iter().enumerate() {
        budget.charge(1)?;
        let slot = ordinals
            .get_mut(mapping.bone_ordinal)
            .ok_or_else(|| budget.fail("external bone ordinal out of range"))?;
        if slot.replace(i).is_some() {
            return Err(budget.fail("external duplicate skin bone ordinal"));
        }
        let target = targets
            .get_mut(mapping.rig_node as usize)
            .ok_or_else(|| budget.fail("external rig node out of range"))?;
        if std::mem::replace(target, true) {
            return Err(budget.fail("external duplicate rig node target"));
        }
        let skin_node = instance.bones[mapping.bone_ordinal]
            .ok_or_else(|| budget.fail("external missing source skin bone node"))?;
        let skin_object = skin_forest.node(skin_node, &budget)?;
        let rig_object = rig_forest.node(mapping.rig_node, &budget)?;
        verify_name(
            &skin_index,
            skin_object,
            &mapping.expected_skin_bone_name_bytes,
            &mut budget,
        )?;
        verify_name(
            rig_index,
            rig_object,
            &mapping.expected_rig_node_name_bytes,
            &mut budget,
        )?;
        let mut node = mapping.rig_node;
        let mut matrix = IDENTITY;
        let mut depth = 0;
        while node != request.rig_root {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("external ancestry depth budget exceeded"));
            }
            let object = rig_forest.object(node, &budget)?;
            let local = if let Some(rig) = sampled {
                budget.charge(1)?;
                if node == rig.sample.object.block {
                    selected_on_mapped_path = true;
                    rig.sample.local
                } else if object.controller.is_some() {
                    return Err(budget.fail("other required rig controller is unapplied"));
                } else {
                    pose::scene_affine(object.transform)
                }
            } else {
                pose::scene_affine(object.transform)
            };
            matrix = pose::finite(pose::compose(local, matrix), &budget)?;
            node = rig_forest.world(node, &budget)?.parent.ok_or_else(|| {
                budget.fail("external mapped bone does not reach chosen rig root")
            })?;
        }
        relative[mapping.bone_ordinal] = matrix;
    }
    if ordinals.iter().any(Option::is_none) {
        return Err(budget.fail("external missing skin bone ordinal"));
    }
    if sampled.is_some() && !selected_on_mapped_path {
        return Err(budget.fail("sampled rig node is outside strict mapped bone-to-root paths"));
    }
    let skin_matrix = pose::skin_affine(skin_transform);
    let display = pose::finite(
        pose::compose(skin_root_world, pose::inverse(skin_matrix, &budget)?),
        &budget,
    )?;
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<pose::Evaluation>(1)?;
    budget.reserve::<u8>(3 * 64 + name_bytes)?;
    budget.reserve::<MappedBone>(bones.len())?;
    budget.reserve::<pose::BonePalette>(bones.len())?;
    budget.reserve::<[f64; 3]>(mesh.vertices.len() + mesh.normals.len())?;
    budget.reserve::<f64>(mesh.vertices.len())?;
    let mut skin = pose::Evaluation {
        contract: if sampled.is_some() {
            sampled::CONTRACT
        } else {
            "engineering-exact-external-rig-skin-v1"
        },
        source_sha256: format!("{skin_digest:x}"),
        geometry: request.geometry,
        geometry_data,
        instance: owner.instance,
        skin_data: data_id,
        skeleton_root: skin_root,
        weights: request.weights,
        skin_to_source_world: display,
        palette: Vec::with_capacity(bones.len()),
        positions: vec![[0.; 3]; mesh.vertices.len()],
        normals: vec![[0.; 3]; mesh.normals.len()],
        weight_sums: vec![0.; mesh.vertices.len()],
        unapplied_controllers: Vec::new(),
        retained_bytes: 0,
        work_units: 0,
        retail_behavior_verified: false,
    };
    let mut rig_controllers = Vec::new();
    let mut mappings = Vec::with_capacity(bones.len());
    for start in [skin_root, request.geometry] {
        let mut node = Some(start);
        let mut depth = 0;
        while let Some(id) = node {
            budget.charge(1)?;
            depth += 1;
            if depth > limits.ancestry_depth {
                return Err(budget.fail("external ancestry depth budget exceeded"));
            }
            record(
                id,
                &skin_forest,
                &mut skin_seen,
                &mut skin.unapplied_controllers,
                &mut budget,
                None,
            )?;
            node = skin_forest.world(id, &budget)?.parent;
        }
    }
    record(
        request.rig_root,
        &rig_forest,
        &mut rig_seen,
        &mut rig_controllers,
        &mut budget,
        sampled.map(|rig| rig.sample.object.block),
    )?;
    for (ordinal, bone) in bones.iter().enumerate() {
        budget.charge(1)?;
        let mapping = &request.explicit_bone_mapping
            [ordinals[ordinal].expect("complete ordinal map admitted")];
        let bind = pose::skin_affine(&bone.transform);
        let matrix = pose::finite(
            pose::compose(
                skin_matrix,
                pose::compose(
                    request.explicit_root_space_mapping,
                    pose::compose(relative[ordinal], bind),
                ),
            ),
            &budget,
        )?;
        skin.palette.push(pose::BonePalette {
            ordinal,
            node: mapping.rig_node,
            matrix,
        });
        mappings.push(MappedBone {
            bone_ordinal: ordinal,
            skin_node: location(
                &skin_index,
                instance.bones[ordinal].expect("source node admitted"),
            ),
            rig_node: location(rig_index, mapping.rig_node),
            raw_skin_bone_name_bytes: mapping.expected_skin_bone_name_bytes.clone(),
            raw_rig_node_name_bytes: mapping.expected_rig_node_name_bytes.clone(),
            rig_bone_to_root: relative[ordinal],
            authored_skin_to_bone: bind,
        });
        let mut node = mapping.rig_node;
        while node != request.rig_root {
            budget.charge(1)?;
            record(
                node,
                &rig_forest,
                &mut rig_seen,
                &mut rig_controllers,
                &mut budget,
                sampled.map(|rig| rig.sample.object.block),
            )?;
            node = rig_forest
                .world(node, &budget)?
                .parent
                .expect("rig-root path already admitted");
        }
        for weight in &bone.weights {
            pose::accumulate_weight(
                matrix,
                weight.weight_bits,
                usize::from(weight.vertex),
                mesh,
                &mut skin,
                &mut budget,
            )?;
        }
    }
    budget.charge(skin.positions.len() + skin.normals.len())?;
    if !skin
        .positions
        .iter()
        .chain(&skin.normals)
        .flatten()
        .all(|v| v.is_finite())
    {
        return Err(budget.fail("deformed vertex or normal overflow"));
    }
    for (vertex, sum) in skin.weight_sums.iter().copied().enumerate() {
        if let Some(detail) = super::weight_sum_error(vertex, sum, request.weights) {
            return Err(budget.fail(&detail));
        }
    }
    let source_retained = [
        decoded.skin.skin.retained_bytes,
        decoded.skin.partitions.retained_bytes,
        decoded.bindings.retained_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |s, n| s.checked_add(n))
    .ok_or_else(|| budget.fail("external source retained byte sum overflow"))?;
    let retained = limits.array_bytes - budget.storage;
    let work = limits.work_units - budget.work;
    // The nested geometry shares this aggregate producer receipt, not a second
    // separately executed source-local evaluator or independent memory claim.
    skin.retained_bytes = retained;
    skin.work_units = work;
    Ok(Evaluation {
        contract: if sampled.is_some() {
            sampled::CONTRACT
        } else {
            "engineering-exact-external-rig-skin-v1"
        },
        skin_source_sha256: format!("{skin_digest:x}"),
        rig_source_sha256: format!("{rig_digest:x}"),
        rig_root: location(rig_index, request.rig_root),
        unapplied_rig_root_local: pose::scene_affine(rig_root_object.transform),
        unapplied_rig_root_source_world: rig_root_world,
        explicit_root_space_mapping: request.explicit_root_space_mapping,
        mappings,
        rig_unapplied_controllers: rig_controllers,
        skin,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        skin_source_retained_bytes: source_retained,
        retained_bytes: retained,
        work_units: work,
        retail_behavior_verified: false,
    })
}
