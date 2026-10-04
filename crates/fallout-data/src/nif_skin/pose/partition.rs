//! One existing geometry deformation, projected through its authored partition.
use super::{
    Affine, BonePalette, Budget, DecodedView, SourceHash, UnappliedController, WeightPolicy,
};
use crate::{
    Result,
    nif_skin::{Data, partition as raw},
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub expected_source_sha256: [u8; 32],
    pub skin: super::Request,
    pub partition_block: u32,
    pub partition_ordinal: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub skin: super::Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub vertices: usize,
    pub draw_indices: usize,
    pub subset_array_bytes: usize,
    pub subset_work_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            skin: Default::default(),
            decoder_array_admission_bytes: 512 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            vertices: 65535,
            draw_indices: 4_000_000,
            subset_array_bytes: 32 * 1024 * 1024,
            subset_work_units: 128_000_000,
            array_bytes: 96 * 1024 * 1024,
            work_units: 144_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct BodyPart {
    pub flags: u16,
    pub body_part: u16,
}
#[derive(Debug, Serialize)]
pub struct PartitionSpan {
    pub block: u32,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
    pub ordinal: usize,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Presence {
    pub vertex_map: u8,
    pub vertex_weights: u8,
    pub faces: u8,
    pub bone_indices: u8,
}
#[derive(Debug, Serialize)]
pub struct PartitionBone {
    pub local_bone: u16,
    pub global_bone_ordinal: u16,
    pub source_bone_node: u32,
}
#[derive(Debug, Serialize)]
pub struct Vertex {
    pub partition_vertex: u16,
    pub source_vertex: u16,
    pub position: [f64; 3],
    pub normal: Option<[f64; 3]>,
    pub weight_sum: f64,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topology {
    Triangles {
        triangles: Vec<[u16; 3]>,
    },
    Strips {
        lengths: Vec<u16>,
        strips: Vec<Vec<u16>>,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub binding_decodes: usize,
    pub scene_decodes: usize,
    pub geometry_deformations: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub source_binding_retained_bytes: usize,
    pub full_pose_retained_bytes: usize,
    pub full_pose_work_units: usize,
    pub subset_retained_bytes: usize,
    pub subset_work_units: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
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
    pub skin_to_source_world: Affine,
    pub partition: PartitionSpan,
    pub body_part: Option<BodyPart>,
    pub presence: Presence,
    pub declared_triangles: u16,
    pub weights_per_vertex: u16,
    pub source_vertex_count: usize,
    pub palette: Vec<BonePalette>,
    pub partition_palette: Vec<PartitionBone>,
    pub vertices: Vec<Vertex>,
    /// CSR: each source vertex's ordered partition-vertex occurrences.
    pub source_to_partition_offsets: Vec<usize>,
    pub source_to_partition_vertices: Vec<u16>,
    pub topology: Topology,
    pub unapplied_controllers: Vec<UnappliedController>,
    pub usage: Usage,
    pub retail_behavior_verified: bool,
}
struct ProjectionBudget<'a> {
    total: Budget<'a>,
    subset: Budget<'a>,
}
impl ProjectionBudget<'_> {
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.total.reserve::<T>(count)?;
        self.subset.reserve::<T>(count)
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.total.charge(count)?;
        self.subset.charge(count)
    }
}
pub fn evaluate(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
) -> Result<Evaluation> {
    let mut budget = ProjectionBudget {
        total: Budget {
            source,
            storage: limits.array_bytes,
            work: limits.work_units,
        },
        subset: Budget {
            source,
            storage: limits.subset_array_bytes,
            work: limits.subset_work_units,
        },
    };
    super::validate_weight_policy(request.skin.weights, &budget.total)?;
    if bytes.len() > limits.skin.source.partition.skin.scene.input_bytes {
        return Err(budget
            .total
            .fail("partition pose source input byte budget exceeded"));
    }
    budget.charge(bytes.len())?;
    let digest = Sha256::digest(bytes);
    if <[u8; 32]>::from(digest) != request.expected_source_sha256 {
        return Err(budget.total.fail("partition pose source SHA256 differs"));
    }
    let arrays = [
        limits.skin.source.partition.skin.scene.array_bytes,
        limits.skin.source.partition.skin.skin_array_bytes,
        limits.skin.source.partition.array_bytes,
        limits.skin.source.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| {
        budget
            .total
            .fail("partition pose decoder array admission exceeded")
    })?;
    let checks = [
        limits.skin.source.partition.skin.weight_index_checks,
        limits.skin.source.partition.index_checks,
        limits.skin.source.graph_checks,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| {
        budget
            .total
            .fail("partition pose decoder check admission exceeded")
    })?;
    budget.reserve::<Evaluation>(1)?;
    budget.reserve::<u8>(64)?;
    let source_sha256 = format!("{digest:x}");
    let (index, decoded, scene) =
        super::binding::decode_with_scene(bytes, source, limits.skin.source)?;
    budget.charge(decoded.skin.skin.owners.len())?;
    budget.charge(decoded.skin.skin.blocks.len())?;
    budget.charge(decoded.skin.partitions.blocks.len())?;
    budget.charge(decoded.skin.partitions.dependencies.len())?;
    let owner = decoded
        .skin
        .skin
        .owners
        .iter()
        .find(|o| o.geometry == request.skin.geometry)
        .ok_or_else(|| {
            budget
                .total
                .fail("partition pose geometry has no decoded skin owner")
        })?;
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
        .ok_or_else(|| {
            budget
                .total
                .fail("partition pose skin instance unavailable")
        })?;
    if instance.partition != Some(request.partition_block) {
        return Err(budget
            .total
            .fail("partition pose instance partition link differs"));
    }
    let block = decoded
        .skin
        .partitions
        .blocks
        .iter()
        .find(|b| b.block == request.partition_block)
        .ok_or_else(|| budget.total.fail("partition pose block unavailable"))?;
    let part = block
        .partitions
        .get(request.partition_ordinal)
        .ok_or_else(|| budget.total.fail("partition pose ordinal unavailable"))?;
    if decoded.skin.partitions.dependencies.iter().any(|d|matches!(d,raw::Dependency::BodyPartCountMismatch{instance,..} if *instance==owner.instance)) {
        return Err(budget.total.fail("partition pose body-part association unavailable"));
    }
    if part.has_vertex_map != 1
        || part.has_faces != 1
        || part.vertex_map.len() != usize::from(part.num_vertices)
    {
        return Err(budget
            .total
            .fail("partition pose requires authored vertex map and faces"));
    }
    if part.vertex_map.len() > limits.vertices {
        return Err(budget
            .total
            .fail("partition pose subset vertex count budget exceeded"));
    }
    let body_part = if let Some(parts) = &instance.body_parts {
        let p = parts.get(request.partition_ordinal).ok_or_else(|| {
            budget
                .total
                .fail("partition pose body-part ordinal unavailable")
        })?;
        Some(BodyPart {
            flags: p.flags,
            body_part: p.body_part,
        })
    } else {
        None
    };
    budget.charge(part.strips.len())?;
    budget.charge(part.strip_lengths.len())?;
    let draw_indices = if part.num_strips == 0 {
        part.triangles.len().checked_mul(3)
    } else {
        part.strips
            .iter()
            .try_fold(0usize, |sum, s| sum.checked_add(s.len()))
    }
    .filter(|n| *n <= limits.draw_indices)
    .ok_or_else(|| {
        budget
            .total
            .fail("partition pose draw index budget exceeded")
    })?;
    budget.reserve::<u8>(64)?;
    let partition = PartitionSpan {
        block: block.block,
        offset: block.offset,
        bytes: block.bytes,
        sha256: block.sha256.clone(),
        ordinal: request.partition_ordinal,
    };
    let full_limits = super::Limits {
        array_bytes: limits.skin.array_bytes.min(budget.total.storage),
        work_units: limits.skin.work_units.min(budget.total.work),
        ..limits.skin
    };
    let full = super::evaluate_decoded(
        DecodedView {
            source,
            hash: SourceHash::Prepared(&source_sha256),
            index: &index,
            decoded: &decoded,
            scene: &scene,
        },
        request.skin,
        full_limits,
        None,
        None,
        Budget {
            source,
            storage: full_limits.array_bytes,
            work: full_limits.work_units,
        },
    )?;
    budget.total.reserve::<u8>(full.retained_bytes)?;
    budget.total.charge(full.work_units)?;
    if full.geometry != owner.geometry
        || full.instance != owner.instance
        || Some(full.geometry_data) != owner.geometry_data
        || owner.vertex_count.map(usize::from) != Some(full.positions.len())
    {
        return Err(budget
            .total
            .fail("partition pose geometry/instance/vertex identity differs"));
    }
    budget.reserve::<PartitionBone>(part.bone_palette.len())?;
    budget.charge(part.bone_palette.len())?;
    let mut partition_palette = Vec::with_capacity(part.bone_palette.len());
    for (local, &global) in part.bone_palette.iter().enumerate() {
        let bone = full.palette.get(usize::from(global)).ok_or_else(|| {
            budget
                .total
                .fail("partition pose global bone outside evaluated palette")
        })?;
        if bone.ordinal != usize::from(global)
            || instance.bones.get(usize::from(global)).copied().flatten() != Some(bone.node)
        {
            return Err(budget
                .total
                .fail("partition pose palette source bone differs"));
        }
        partition_palette.push(PartitionBone {
            local_bone: local as u16,
            global_bone_ordinal: global,
            source_bone_node: bone.node,
        });
    }
    let source_vertices = full.positions.len();
    let selected = part.vertex_map.len();
    let offsets_count = source_vertices.checked_add(1).ok_or_else(|| {
        budget
            .total
            .fail("partition pose source-offset count overflow")
    })?;
    budget.reserve::<Vertex>(selected)?;
    budget.reserve::<usize>(offsets_count)?;
    budget.reserve::<usize>(source_vertices)?;
    budget.reserve::<u16>(selected)?;
    let map_work = source_vertices
        .checked_mul(3)
        .and_then(|n| selected.checked_mul(3).and_then(|m| n.checked_add(m)))
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| {
            budget
                .total
                .fail("partition pose source-offset work overflow")
        })?;
    budget.charge(map_work)?;
    let mut offsets = vec![0usize; offsets_count];
    let mut vertices = Vec::with_capacity(selected);
    for (local, &source_vertex) in part.vertex_map.iter().enumerate() {
        let v = usize::from(source_vertex);
        if v >= source_vertices {
            return Err(budget
                .total
                .fail("partition pose vertex map outside deformed geometry"));
        }
        offsets[v + 1] = offsets[v + 1].checked_add(1).ok_or_else(|| {
            budget
                .total
                .fail("partition pose occurrence count overflow")
        })?;
        vertices.push(Vertex {
            partition_vertex: local as u16,
            source_vertex,
            position: full.positions[v],
            normal: if full.normals.is_empty() {
                None
            } else {
                Some(full.normals[v])
            },
            weight_sum: full.weight_sums[v],
        });
    }
    for i in 0..source_vertices {
        offsets[i + 1] = offsets[i + 1]
            .checked_add(offsets[i])
            .ok_or_else(|| budget.total.fail("partition pose offset prefix overflow"))?;
    }
    let mut cursors = offsets[..source_vertices].to_vec();
    let mut occurrences = vec![0u16; selected];
    for (local, &source_vertex) in part.vertex_map.iter().enumerate() {
        let v = usize::from(source_vertex);
        occurrences[cursors[v]] = local as u16;
        cursors[v] += 1;
    }
    budget.charge(draw_indices)?;
    let topology = if part.num_strips == 0 {
        budget.reserve::<[u16; 3]>(part.triangles.len())?;
        if part.triangles.len() != usize::from(part.num_triangles) || !part.strips.is_empty() {
            return Err(budget
                .total
                .fail("partition pose triangle topology differs"));
        }
        Topology::Triangles {
            triangles: part.triangles.clone(),
        }
    } else {
        budget.reserve::<Vec<u16>>(part.strips.len())?;
        budget.reserve::<u16>(part.strip_lengths.len())?;
        budget.reserve::<u16>(draw_indices)?;
        if part.strips.len() != usize::from(part.num_strips)
            || part.strip_lengths.len() != part.strips.len()
            || !part.triangles.is_empty()
        {
            return Err(budget.total.fail("partition pose strip topology differs"));
        }
        for (strip, &length) in part.strips.iter().zip(&part.strip_lengths) {
            if strip.len() != usize::from(length) {
                return Err(budget.total.fail("partition pose strip length differs"));
            }
        }
        Topology::Strips {
            lengths: part.strip_lengths.clone(),
            strips: part.strips.clone(),
        }
    };
    let source_retained = [
        decoded.skin.skin.retained_bytes,
        decoded.skin.partitions.retained_bytes,
        decoded.bindings.retained_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |n, v| n.checked_add(v))
    .ok_or_else(|| {
        budget
            .total
            .fail("partition pose source retention sum overflow")
    })?;
    let usage = Usage {
        binding_decodes: 1,
        scene_decodes: 1,
        geometry_deformations: 1,
        decoder_array_admission_bytes: arrays,
        decoder_check_admission_units: checks,
        source_binding_retained_bytes: source_retained,
        full_pose_retained_bytes: full.retained_bytes,
        full_pose_work_units: full.work_units,
        subset_retained_bytes: limits.subset_array_bytes - budget.subset.storage,
        subset_work_units: limits.subset_work_units - budget.subset.work,
        retained_bytes: limits.array_bytes - budget.total.storage,
        work_units: limits.work_units - budget.total.work,
    };
    Ok(Evaluation {
        contract: "engineering-source-geometry-partition-subset-v1",
        source_sha256,
        geometry: full.geometry,
        geometry_data: full.geometry_data,
        instance: full.instance,
        skin_data: full.skin_data,
        skeleton_root: full.skeleton_root,
        weights: full.weights,
        skin_to_source_world: full.skin_to_source_world,
        partition,
        body_part,
        presence: Presence {
            vertex_map: part.has_vertex_map,
            vertex_weights: part.has_vertex_weights,
            faces: part.has_faces,
            bone_indices: part.has_bone_indices,
        },
        declared_triangles: part.num_triangles,
        weights_per_vertex: part.weights_per_vertex,
        source_vertex_count: source_vertices,
        palette: full.palette,
        partition_palette,
        vertices,
        source_to_partition_offsets: offsets,
        source_to_partition_vertices: occurrences,
        topology,
        unapplied_controllers: full.unapplied_controllers,
        usage,
        retail_behavior_verified: false,
    })
}
