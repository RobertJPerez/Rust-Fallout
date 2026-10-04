//! Exact authored partition rows and topology, without draw/shader semantics.
use super::{Dependency, decode_with_scene};
use crate::{Error, Result, nif, nif_skin::Data};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub expected_source_sha256: [u8; 32],
    pub geometry: u32,
    pub partition_block: u32,
    pub partition_ordinal: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source: super::Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub influence_rows: usize,
    pub draw_indices: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            decoder_array_admission_bytes: 448 * 1024 * 1024,
            decoder_check_admission_units: 32_000_000,
            influence_rows: 4_000_000,
            draw_indices: 4_000_000,
            array_bytes: 32 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SourceSpan {
    pub block: u32,
    pub offset: usize,
    pub bytes: usize,
    pub sha256: String,
}
#[derive(Debug, Serialize)]
pub struct Identity {
    pub source_sha256: String,
    pub geometry: SourceSpan,
    pub geometry_data: SourceSpan,
    pub instance: SourceSpan,
    pub partition: SourceSpan,
    pub partition_ordinal: usize,
    pub source_vertex_count: u16,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Presence {
    pub vertex_map: u8,
    pub vertex_weights: u8,
    pub faces: u8,
    pub bone_indices: u8,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct PaletteEntry {
    pub local_bone: u16,
    pub global_bone_ordinal: u16,
    pub source_bone_node: u32,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Vertex {
    pub local_vertex: u16,
    pub source_vertex: u16,
    pub influence_start: usize,
    pub influence_count: u16,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Influence {
    pub source_weight_ordinal: usize,
    pub local_vertex: u16,
    pub source_vertex: u16,
    pub slot: u16,
    pub local_bone: u8,
    pub global_bone_ordinal: u16,
    pub source_bone_node: u32,
    pub weight_bits: u32,
}
#[derive(Debug, Serialize)]
pub struct Triangle {
    pub local_vertices: [u16; 3],
    pub source_vertices: [u16; 3],
}
#[derive(Debug, Serialize)]
pub struct Strip {
    pub local_vertices: Vec<u16>,
    pub source_vertices: Vec<u16>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topology {
    Triangles {
        triangles: Vec<Triangle>,
    },
    Strips {
        lengths: Vec<u16>,
        strips: Vec<Strip>,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub source_retained_bytes: usize,
    pub influence_rows: usize,
    pub draw_indices: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
}
/// Source authority is sealed here; serialization and public row types are
/// observations, never constructors accepted by another source evaluator.
#[derive(Debug, Serialize)]
pub struct Streams {
    contract: &'static str,
    identity: Identity,
    presence: Presence,
    declared_vertices: u16,
    declared_triangles: u16,
    declared_bones: u16,
    declared_strips: u16,
    weights_per_vertex: u16,
    palette: Vec<PaletteEntry>,
    vertices: Vec<Vertex>,
    influences: Vec<Influence>,
    topology: Topology,
    usage: Usage,
    retail_behavior_verified: bool,
}
impl Streams {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn presence(&self) -> Presence {
        self.presence
    }
    pub fn palette(&self) -> &[PaletteEntry] {
        &self.palette
    }
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    pub fn influences(&self) -> &[Influence] {
        &self.influences
    }
    pub fn topology(&self) -> &Topology {
        &self.topology
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}
struct Budget<'a> {
    source: &'a str,
    storage: usize,
    work: usize,
}
impl Budget<'_> {
    fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!("{}: partition streams: {detail}", self.source))
    }
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.storage = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|n| self.storage.checked_sub(n))
            .ok_or_else(|| self.fail("array storage budget exceeded"))?;
        Ok(())
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self
            .work
            .checked_sub(count)
            .ok_or_else(|| self.fail("work budget exceeded"))?;
        Ok(())
    }
}
fn span(
    bytes: &[u8],
    index: &nif::NifIndex,
    id: u32,
    budget: &mut Budget<'_>,
) -> Result<SourceSpan> {
    let block = index
        .blocks
        .get(id as usize)
        .ok_or_else(|| budget.fail("source span block unavailable"))?;
    budget.reserve::<u8>(64)?;
    budget.charge(block.bytes)?;
    Ok(SourceSpan {
        block: id,
        offset: block.offset,
        bytes: block.bytes,
        sha256: format!(
            "{:x}",
            Sha256::digest(&bytes[block.offset..block.offset + block.bytes])
        ),
    })
}
pub fn prepare(bytes: &[u8], source: &str, request: Request, limits: Limits) -> Result<Streams> {
    let mut budget = Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    if bytes.len() > limits.source.skin.scene.input_bytes {
        return Err(budget.fail("source input byte budget exceeded"));
    }
    budget.charge(bytes.len())?;
    let digest = Sha256::digest(bytes);
    if <[u8; 32]>::from(digest) != request.expected_source_sha256 {
        return Err(budget.fail("whole source SHA256 differs"));
    }
    let arrays = [
        limits.source.skin.scene.array_bytes,
        limits.source.skin.skin_array_bytes,
        limits.source.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |sum, n| sum.checked_add(n))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("decoder array admission exceeded"))?;
    let checks = [
        limits.source.skin.weight_index_checks,
        limits.source.index_checks,
    ]
    .into_iter()
    .try_fold(0usize, |sum, n| sum.checked_add(n))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("decoder check admission exceeded"))?;
    let (index, decoded, scene) = decode_with_scene(bytes, source, limits.source)?;
    budget.charge(decoded.skin.owners.len())?;
    budget.charge(decoded.skin.blocks.len())?;
    budget.charge(decoded.partitions.blocks.len())?;
    let owner = decoded
        .skin
        .owners
        .iter()
        .find(|o| o.geometry == request.geometry)
        .ok_or_else(|| budget.fail("selected geometry has no decoded skin owner"))?;
    let instance = decoded
        .skin
        .blocks
        .iter()
        .find(|b| b.block == owner.instance)
        .and_then(|b| match &b.data {
            Data::Instance { instance } => Some(instance),
            _ => None,
        })
        .ok_or_else(|| budget.fail("selected skin instance unavailable"))?;
    if instance.partition != Some(request.partition_block) {
        return Err(budget.fail("selected instance partition link differs"));
    }
    let vertex_count = owner
        .vertex_count
        .ok_or_else(|| budget.fail("selected geometry vertex domain unavailable"))?;
    let geometry_data = owner
        .geometry_data
        .ok_or_else(|| budget.fail("selected geometry data link unavailable"))?;
    let block = decoded
        .partitions
        .blocks
        .iter()
        .find(|b| b.block == request.partition_block)
        .ok_or_else(|| budget.fail("selected partition block unavailable"))?;
    let part = block
        .partitions
        .get(request.partition_ordinal)
        .ok_or_else(|| budget.fail("selected partition ordinal unavailable"))?;
    budget.charge(decoded.partitions.dependencies.len())?;
    if decoded.partitions.dependencies.iter().any(
        |d| matches!(d,Dependency::BodyPartCountMismatch{instance,..} if *instance==owner.instance),
    ) {
        return Err(budget.fail("selected dismember body-part association unavailable"));
    }
    if part.has_vertex_map != 1
        || part.has_vertex_weights != 1
        || part.has_faces != 1
        || part.has_bone_indices != 1
    {
        return Err(budget.fail(
            "selected partition requires authored vertex-map/weight/face/bone-index arrays",
        ));
    }
    let vertices = usize::from(part.num_vertices);
    let width = usize::from(part.weights_per_vertex);
    let rows = vertices
        .checked_mul(width)
        .filter(|n| *n <= limits.influence_rows)
        .ok_or_else(|| budget.fail("influence row product budget exceeded"))?;
    if (vertices != 0 && width != 4)
        || part.vertex_map.len() != vertices
        || part.weight_bits.len() != rows
        || part.bone_indices.len() != rows
        || part.bone_palette.len() != usize::from(part.num_bones)
    {
        return Err(budget.fail("selected partition declared widths or array lengths differ"));
    }
    budget.charge(part.strip_lengths.len())?;
    budget.charge(part.strips.len())?;
    let indices = if part.num_strips == 0 {
        part.triangles.len().checked_mul(3)
    } else {
        part.strips
            .iter()
            .try_fold(0usize, |sum, s| sum.checked_add(s.len()))
    }
    .filter(|n| *n <= limits.draw_indices)
    .ok_or_else(|| budget.fail("draw index count budget exceeded"))?;
    if part.num_strips == 0 {
        if part.triangles.len() != usize::from(part.num_triangles) || !part.strips.is_empty() {
            return Err(budget.fail("selected triangle topology lengths differ"));
        }
        budget.reserve::<Triangle>(part.triangles.len())?;
    } else {
        if part.strips.len() != usize::from(part.num_strips)
            || part.strip_lengths.len() != part.strips.len()
            || !part.triangles.is_empty()
        {
            return Err(budget.fail("selected strip topology lengths differ"));
        }
        budget.reserve::<Strip>(part.strips.len())?;
        budget.reserve::<u16>(part.strip_lengths.len())?;
        budget.reserve::<u16>(
            indices
                .checked_mul(2)
                .ok_or_else(|| budget.fail("draw index output product overflow"))?,
        )?;
    }
    budget.reserve::<Streams>(1)?;
    budget.reserve::<u8>(64)?;
    budget.reserve::<PaletteEntry>(part.bone_palette.len())?;
    budget.reserve::<Vertex>(vertices)?;
    budget.reserve::<Influence>(rows)?;
    budget.reserve::<bool>(index.blocks.len())?;
    budget.charge(index.blocks.len())?;
    budget.charge(scene.objects.len())?;
    let mut decoded_nodes = vec![false; index.blocks.len()];
    for object in &scene.objects {
        if matches!(object.kind, crate::nif_scene::ObjectKind::Node { .. }) {
            decoded_nodes[object.block as usize] = true;
        }
    }
    budget.charge(part.bone_palette.len())?;
    let mut palette = Vec::with_capacity(part.bone_palette.len());
    for (local, &global) in part.bone_palette.iter().enumerate() {
        let node = instance
            .bones
            .get(usize::from(global))
            .copied()
            .flatten()
            .ok_or_else(|| budget.fail("partition palette member has no source bone link"))?;
        if !decoded_nodes.get(node as usize).copied().unwrap_or(false) {
            return Err(budget.fail("partition palette member bone link is not a decoded node"));
        }
        palette.push(PaletteEntry {
            local_bone: local as u16,
            global_bone_ordinal: global,
            source_bone_node: node,
        });
    }
    let identity = Identity {
        source_sha256: format!("{digest:x}"),
        geometry: span(bytes, &index, request.geometry, &mut budget)?,
        geometry_data: span(bytes, &index, geometry_data, &mut budget)?,
        instance: span(bytes, &index, owner.instance, &mut budget)?,
        partition: span(bytes, &index, request.partition_block, &mut budget)?,
        partition_ordinal: request.partition_ordinal,
        source_vertex_count: vertex_count,
    };
    budget.charge(vertices)?;
    budget.charge(rows)?;
    let mut vertex_rows = Vec::with_capacity(vertices);
    let mut influences = Vec::with_capacity(rows);
    for (local, &source_vertex) in part.vertex_map.iter().enumerate() {
        if source_vertex >= vertex_count {
            return Err(budget.fail("vertex map is outside source geometry domain"));
        }
        let start = local
            .checked_mul(width)
            .ok_or_else(|| budget.fail("vertex row offset product overflow"))?;
        vertex_rows.push(Vertex {
            local_vertex: local as u16,
            source_vertex,
            influence_start: start,
            influence_count: part.weights_per_vertex,
        });
        for slot in 0..width {
            let ordinal = start + slot;
            let local_bone = part.bone_indices[ordinal];
            let bone = palette
                .get(usize::from(local_bone))
                .ok_or_else(|| budget.fail("local bone index is outside partition palette"))?;
            influences.push(Influence {
                source_weight_ordinal: ordinal,
                local_vertex: local as u16,
                source_vertex,
                slot: slot as u16,
                local_bone,
                global_bone_ordinal: bone.global_bone_ordinal,
                source_bone_node: bone.source_bone_node,
                weight_bits: part.weight_bits[ordinal],
            });
        }
    }
    budget.charge(indices)?;
    let source_vertex = |local: u16| {
        part.vertex_map
            .get(usize::from(local))
            .copied()
            .ok_or_else(|| budget.fail("draw index is outside local vertex map"))
    };
    let topology = if part.num_strips == 0 {
        let triangles = part
            .triangles
            .iter()
            .map(|&local| {
                Ok(Triangle {
                    local_vertices: local,
                    source_vertices: [
                        source_vertex(local[0])?,
                        source_vertex(local[1])?,
                        source_vertex(local[2])?,
                    ],
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Topology::Triangles { triangles }
    } else {
        let mut strips = Vec::with_capacity(part.strips.len());
        for (strip, &length) in part.strips.iter().zip(&part.strip_lengths) {
            if strip.len() != usize::from(length) {
                return Err(budget.fail("authored strip length differs"));
            }
            strips.push(Strip {
                local_vertices: strip.clone(),
                source_vertices: strip
                    .iter()
                    .map(|&v| source_vertex(v))
                    .collect::<Result<Vec<_>>>()?,
            });
        }
        Topology::Strips {
            lengths: part.strip_lengths.clone(),
            strips,
        }
    };
    let retained = decoded
        .skin
        .retained_bytes
        .checked_add(decoded.partitions.retained_bytes)
        .ok_or_else(|| budget.fail("source retention sum overflow"))?;
    Ok(Streams {
        contract: "source-qualified-authored-partition-streams-v1",
        identity,
        presence: Presence {
            vertex_map: part.has_vertex_map,
            vertex_weights: part.has_vertex_weights,
            faces: part.has_faces,
            bone_indices: part.has_bone_indices,
        },
        declared_vertices: part.num_vertices,
        declared_triangles: part.num_triangles,
        declared_bones: part.num_bones,
        declared_strips: part.num_strips,
        weights_per_vertex: part.weights_per_vertex,
        palette,
        vertices: vertex_rows,
        influences,
        topology,
        usage: Usage {
            decoder_array_admission_bytes: arrays,
            decoder_check_admission_units: checks,
            source_retained_bytes: retained,
            influence_rows: rows,
            draw_indices: indices,
            retained_bytes: limits.array_bytes - budget.storage,
            work_units: limits.work_units - budget.work,
        },
        retail_behavior_verified: false,
    })
}
