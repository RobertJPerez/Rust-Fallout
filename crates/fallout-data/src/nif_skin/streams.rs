//! Exact whole-geometry streams sharing one sealed source and skin owner.
use super::{Data, binding, influences, pose, storage};
use crate::{Result, nif, nif_scene};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub expected_source_sha256: [u8; 32],
    pub geometry: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source: binding::Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub source_metadata_array_bytes: usize,
    pub source_metadata_work_units: usize,
    pub influence_entries: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub max_combined_retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            decoder_array_admission_bytes: 512 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            source_metadata_array_bytes: 128 * 1024 * 1024,
            source_metadata_work_units: 16_000_000,
            influence_entries: 4_000_000,
            array_bytes: 64 * 1024 * 1024,
            work_units: 128_000_000,
            max_combined_retained_bytes: 768 * 1024 * 1024,
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
    pub skin_data: SourceSpan,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Presence {
    pub vertices: bool,
    pub normals: bool,
    pub colors: bool,
    pub tangents: bool,
    pub uv_sets: usize,
}
#[derive(Debug, Serialize)]
pub struct Attributes {
    pub positions_bits: Vec<[u32; 3]>,
    pub normals_bits: Vec<[u32; 3]>,
    pub tangents_bits: Vec<[u32; 3]>,
    pub bitangents_bits: Vec<[u32; 3]>,
    pub colors_bits: Vec<[u32; 4]>,
    pub uv_sets_bits: Vec<Vec<[u32; 2]>>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Metadata {
    pub group_id: i32,
    pub vertex_count: u16,
    pub keep_flags: u8,
    pub compress_flags: u8,
    pub data_flags: u16,
    pub bound_bits: [u32; 4],
    pub consistency_flags: u16,
    pub declared_triangles: u16,
    pub source_triangle_count_matches: bool,
    pub strip_degenerate_triangles: usize,
}
#[derive(Debug, Serialize)]
pub struct AdditionalGeometryData {
    pub link: u32,
    pub span: SourceSpan,
    pub block_type: String,
    pub payload_decoded: bool,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Vertex {
    pub source_vertex: u16,
    pub influence_start: usize,
    pub influence_count: usize,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topology {
    Triangles {
        declared_points: u32,
        present: bool,
        indices: Vec<[u16; 3]>,
        match_groups: Vec<Vec<u16>>,
    },
    Strips {
        lengths: Vec<u16>,
        present: bool,
        indices: Vec<Vec<u16>>,
    },
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Triangle {
    pub vertices: [u16; 3],
    /// Includes degenerate source strip slots preceding this expanded triangle.
    pub source_primitive_ordinal: usize,
    pub strip_ordinal: Option<usize>,
    pub strip_step_ordinal: Option<usize>,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub source_bytes: usize,
    pub binding_decodes: usize,
    pub scene_decodes: usize,
    pub full_source_sha256_traversals: usize,
    pub csr_builds: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub source_binding_retained_bytes: usize,
    pub source_metadata_retained_bytes: usize,
    pub source_metadata_work_units: usize,
    /// Includes existing conservative source charges and released CSR scratch.
    pub charged_bytes: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
}
struct Authority {
    index: nif::NifIndex,
    decoded: binding::Source,
    scene: nif_scene::Scene,
    digest: [u8; 32],
}
#[derive(Serialize)]
pub struct PreparedGeometryStreams {
    contract: &'static str,
    identity: Identity,
    presence: Presence,
    metadata: Metadata,
    attributes: Attributes,
    vertices: Vec<Vertex>,
    topology: Topology,
    triangles: Vec<Triangle>,
    influences: influences::Table,
    additional_geometry_data: Option<AdditionalGeometryData>,
    usage: Usage,
    faithful_renderer_ready: bool,
    retail_behavior_verified: bool,
    #[serde(skip)]
    authority: Authority,
}
impl fmt::Debug for PreparedGeometryStreams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedGeometryStreams")
            .field("identity", &self.identity)
            .field("usage", &self.usage)
            .finish()
    }
}
#[derive(Clone, Copy, Debug)]
pub struct EvaluationRequest {
    pub expected_source_sha256: [u8; 32],
    pub geometry: u32,
    pub instance: u32,
    pub weights: pose::WeightPolicy,
}
#[derive(Clone, Copy, Debug)]
pub struct EvaluationLimits {
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
    pub max_combined_retained_bytes: usize,
}
impl Default for EvaluationLimits {
    fn default() -> Self {
        Self {
            array_bytes: 64 * 1024 * 1024,
            work_units: 16_000_000,
            ancestry_depth: 1024,
            max_combined_retained_bytes: 832 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct EvaluationUsage {
    pub binding_decodes: usize,
    pub scene_decodes: usize,
    pub full_source_sha256_traversals: usize,
    pub csr_builds: usize,
    pub packet_retained_bytes: usize,
    pub charged_output_bytes: usize,
    pub combined_retained_bytes: usize,
    pub work_units: usize,
}
#[derive(Debug, Serialize)]
pub struct Evaluation {
    pub skin: pose::Evaluation,
    pub usage: EvaluationUsage,
}
impl PreparedGeometryStreams {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn presence(&self) -> Presence {
        self.presence
    }
    pub fn metadata(&self) -> Metadata {
        self.metadata
    }
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }
    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }
    pub fn topology(&self) -> &Topology {
        &self.topology
    }
    pub fn triangles(&self) -> &[Triangle] {
        &self.triangles
    }
    pub fn influences(&self) -> &influences::Table {
        &self.influences
    }
    pub fn additional_geometry_data(&self) -> Option<&AdditionalGeometryData> {
        self.additional_geometry_data.as_ref()
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
    pub fn evaluate_stored(
        &self,
        request: EvaluationRequest,
        limits: EvaluationLimits,
    ) -> Result<Evaluation> {
        let mut budget = pose::Budget {
            source: &self.identity.source_sha256,
            storage: limits.array_bytes,
            work: limits.work_units,
        };
        budget.charge(34)?;
        if self.authority.digest != request.expected_source_sha256 {
            return Err(budget.fail("stream packet source SHA256 differs"));
        }
        if self.identity.geometry.block != request.geometry {
            return Err(budget.fail("stream packet geometry differs"));
        }
        if self.identity.instance.block != request.instance {
            return Err(budget.fail("stream packet skin instance differs"));
        }
        let available = limits
            .max_combined_retained_bytes
            .checked_sub(self.usage.retained_bytes)
            .ok_or_else(|| budget.fail("stream packet/pose combined storage budget exceeded"))?;
        budget.storage = budget.storage.min(available);
        let admitted = budget.storage;
        budget.reserve::<Evaluation>(1)?;
        let skin = pose::evaluate_streams_stored(
            pose::StoredStreamsView {
                source_sha256: &self.identity.source_sha256,
                index: &self.authority.index,
                decoded: &self.authority.decoded,
                scene: &self.authority.scene,
                table: &self.influences,
            },
            pose::Request {
                geometry: request.geometry,
                weights: request.weights,
            },
            pose::Limits {
                array_bytes: budget.storage,
                work_units: budget.work,
                ancestry_depth: limits.ancestry_depth,
                ..Default::default()
            },
        )?;
        budget.reserve::<u8>(skin.retained_bytes)?;
        budget.charge(skin.work_units)?;
        let charged_output_bytes = admitted - budget.storage;
        Ok(Evaluation {
            skin,
            usage: EvaluationUsage {
                binding_decodes: 0,
                scene_decodes: 0,
                full_source_sha256_traversals: 0,
                csr_builds: 0,
                packet_retained_bytes: self.usage.retained_bytes,
                charged_output_bytes,
                combined_retained_bytes: self.usage.retained_bytes + charged_output_bytes,
                work_units: limits.work_units - budget.work,
            },
        })
    }
}
fn sum(
    values: impl IntoIterator<Item = usize>,
    budget: &pose::Budget<'_>,
    detail: &str,
) -> Result<usize> {
    values
        .into_iter()
        .try_fold(0usize, |a, b| a.checked_add(b))
        .ok_or_else(|| budget.fail(detail))
}
fn span(
    bytes: &[u8],
    index: &nif::NifIndex,
    id: u32,
    budget: &mut pose::Budget<'_>,
) -> Result<SourceSpan> {
    let block = index
        .blocks
        .get(id as usize)
        .ok_or_else(|| budget.fail("stream source span unavailable"))?;
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
fn words<const N: usize>(
    rows: &[[f32; N]],
    budget: &mut pose::Budget<'_>,
) -> Result<Vec<[u32; N]>> {
    budget.reserve::<[u32; N]>(rows.len())?;
    budget.charge(
        rows.len()
            .checked_mul(N)
            .ok_or_else(|| budget.fail("stream word work overflow"))?,
    )?;
    Ok(rows.iter().map(|row| row.map(f32::to_bits)).collect())
}
fn copy<T: Copy>(rows: &[T], budget: &mut pose::Budget<'_>) -> Result<Vec<T>> {
    budget.reserve::<T>(rows.len())?;
    budget.charge(rows.len())?;
    Ok(rows.to_vec())
}
fn nested<T: Copy>(rows: &[Vec<T>], budget: &mut pose::Budget<'_>) -> Result<Vec<Vec<T>>> {
    budget.reserve::<Vec<T>>(rows.len())?;
    budget.charge(rows.len())?;
    rows.iter().map(|row| copy(row, budget)).collect()
}

pub fn prepare(
    bytes: &[u8],
    source: &str,
    request: Request,
    limits: Limits,
) -> Result<PreparedGeometryStreams> {
    let mut budget = pose::Budget {
        source,
        storage: limits.array_bytes,
        work: limits.work_units,
    };
    if bytes.len() > limits.source.partition.skin.scene.input_bytes {
        return Err(budget.fail("stream source input byte budget exceeded"));
    }
    let arrays = sum(
        [
            limits.source.partition.skin.scene.array_bytes,
            limits.source.partition.skin.skin_array_bytes,
            limits.source.partition.array_bytes,
            limits.source.array_bytes,
        ],
        &budget,
        "stream decoder array admission overflow",
    )?;
    let checks = sum(
        [
            limits.source.partition.skin.weight_index_checks,
            limits.source.partition.index_checks,
            limits.source.graph_checks,
        ],
        &budget,
        "stream decoder check admission overflow",
    )?;
    if arrays > limits.decoder_array_admission_bytes
        || checks > limits.decoder_check_admission_units
    {
        return Err(budget.fail("stream decoder admission exceeded"));
    }
    if sum(
        [
            arrays,
            limits.source_metadata_array_bytes,
            limits.array_bytes,
        ],
        &budget,
        "stream preparation admission overflow",
    )? > limits.max_combined_retained_bytes
    {
        return Err(budget.fail("stream source/selected preparation admission exceeded"));
    }
    budget.reserve::<PreparedGeometryStreams>(1)?;
    budget.reserve::<u8>(64)?;
    budget.charge(bytes.len())?;
    let hash = Sha256::digest(bytes);
    let digest: [u8; 32] = hash.into();
    budget.charge(32)?;
    if digest != request.expected_source_sha256 {
        return Err(budget.fail("stream whole source SHA256 differs"));
    }
    let (index, decoded, scene) = binding::decode_with_scene(bytes, source, limits.source)?;
    let mut metadata_budget = pose::Budget {
        source,
        storage: limits.source_metadata_array_bytes,
        work: limits.source_metadata_work_units,
    };
    storage::admit_index(&index, &mut metadata_budget)?;
    storage::admit_scene(&scene, &mut metadata_budget)?;
    let source_metadata_retained_bytes =
        limits.source_metadata_array_bytes - metadata_budget.storage;
    let source_metadata_work_units = limits.source_metadata_work_units - metadata_budget.work;
    let source_binding_retained_bytes = sum(
        [
            decoded.skin.skin.retained_bytes,
            decoded.skin.partitions.retained_bytes,
            decoded.bindings.retained_bytes,
        ],
        &budget,
        "stream source retained sum overflow",
    )?;
    let csr_limits = influences::Limits {
        source: limits.source,
        entries: limits.influence_entries,
        array_bytes: budget.storage,
        work_units: budget.work,
        decoder_array_admission_bytes: limits.decoder_array_admission_bytes,
        decoder_check_admission_units: limits.decoder_check_admission_units,
    };
    let influences = influences::prepare_decoded(
        &decoded,
        &scene,
        influences::SourceDigest::Prepared(digest),
        request.geometry,
        csr_limits,
        influences::Budget {
            source,
            bytes: budget.storage,
            work: budget.work,
        },
        (arrays, checks),
    )?;
    budget.reserve::<u8>(influences.usage().array_bytes)?;
    budget.charge(influences.usage().work_units)?;
    budget.charge(
        decoded.skin.skin.owners.len() + decoded.skin.skin.blocks.len() + scene.meshes.len(),
    )?;
    let owner = decoded
        .skin
        .skin
        .owners
        .iter()
        .find(|o| o.geometry == request.geometry)
        .ok_or_else(|| budget.fail("stream selected owner unavailable"))?;
    let geometry_data = owner
        .geometry_data
        .ok_or_else(|| budget.fail("stream geometry data unavailable"))?;
    let skin_data = decoded
        .skin
        .skin
        .blocks
        .iter()
        .find(|b| b.block == owner.instance)
        .and_then(|b| match &b.data {
            Data::Instance { instance } => instance.data,
            _ => None,
        })
        .ok_or_else(|| budget.fail("stream skin data unavailable"))?;
    let mesh = scene
        .meshes
        .iter()
        .find(|m| m.block == geometry_data)
        .ok_or_else(|| budget.fail("stream mesh unavailable"))?;
    let identity = Identity {
        source_sha256: format!("{hash:x}"),
        geometry: span(bytes, &index, request.geometry, &mut budget)?,
        geometry_data: span(bytes, &index, geometry_data, &mut budget)?,
        instance: span(bytes, &index, owner.instance, &mut budget)?,
        skin_data: span(bytes, &index, skin_data, &mut budget)?,
    };
    budget.charge(18)?;
    let presence = Presence {
        vertices: mesh.has_vertices,
        normals: mesh.has_normals,
        colors: mesh.has_colors,
        tangents: mesh.has_normals && mesh.data_flags & 0x1000 != 0,
        uv_sets: mesh.uv_sets.len(),
    };
    budget.reserve::<Vec<[u32; 2]>>(mesh.uv_sets.len())?;
    budget.charge(mesh.uv_sets.len())?;
    let attributes = Attributes {
        positions_bits: words(&mesh.vertices, &mut budget)?,
        normals_bits: words(&mesh.normals, &mut budget)?,
        tangents_bits: words(&mesh.tangents, &mut budget)?,
        bitangents_bits: words(&mesh.bitangents, &mut budget)?,
        colors_bits: words(&mesh.colors, &mut budget)?,
        uv_sets_bits: mesh
            .uv_sets
            .iter()
            .map(|uv| words(uv, &mut budget))
            .collect::<Result<_>>()?,
    };
    let metadata = Metadata {
        group_id: mesh.group_id,
        vertex_count: mesh.vertex_count,
        keep_flags: mesh.keep_flags,
        compress_flags: mesh.compress_flags,
        data_flags: mesh.data_flags,
        bound_bits: mesh.bound.map(f32::to_bits),
        consistency_flags: mesh.consistency_flags,
        declared_triangles: mesh.declared_triangles,
        source_triangle_count_matches: mesh.source_triangle_count_matches,
        strip_degenerate_triangles: mesh.strip_degenerate_triangles,
    };
    budget.reserve::<Vertex>(mesh.vertices.len())?;
    budget.charge(mesh.vertices.len())?;
    let vertices = (0..mesh.vertices.len())
        .map(|vertex| Vertex {
            source_vertex: vertex as u16,
            influence_start: influences.vertex_offsets()[vertex],
            influence_count: influences.vertex_offsets()[vertex + 1]
                - influences.vertex_offsets()[vertex],
        })
        .collect();
    let (topology, triangles) = topology(mesh, &mut budget)?;
    let additional_geometry_data = mesh
        .additional_data
        .map(|link| -> Result<AdditionalGeometryData> {
            let block = &index.blocks[link as usize];
            let name = &index.block_types[block.type_index as usize];
            budget.reserve::<u8>(name.len())?;
            budget.charge(name.len())?;
            Ok(AdditionalGeometryData {
                link,
                span: span(bytes, &index, link, &mut budget)?,
                block_type: name.clone(),
                payload_decoded: false,
            })
        })
        .transpose()?;
    let own_charge = limits.array_bytes - budget.storage;
    let charged_bytes = sum(
        [
            source_binding_retained_bytes,
            source_metadata_retained_bytes,
            own_charge,
        ],
        &budget,
        "stream retained sum overflow",
    )?;
    let retained_bytes =
        charged_bytes - (influences.usage().array_bytes - influences.usage().output_bytes);
    Ok(PreparedGeometryStreams {
        contract: "engineering-exact-whole-geometry-skin-streams-v1",
        identity,
        presence,
        metadata,
        attributes,
        vertices,
        topology,
        triangles,
        influences,
        additional_geometry_data,
        usage: Usage {
            source_bytes: bytes.len(),
            binding_decodes: 1,
            scene_decodes: 1,
            full_source_sha256_traversals: 1,
            csr_builds: 1,
            decoder_array_admission_bytes: arrays,
            decoder_check_admission_units: checks,
            source_binding_retained_bytes,
            source_metadata_retained_bytes,
            source_metadata_work_units,
            charged_bytes,
            retained_bytes,
            work_units: limits.work_units - budget.work,
        },
        faithful_renderer_ready: false,
        retail_behavior_verified: false,
        authority: Authority {
            index,
            decoded,
            scene,
            digest,
        },
    })
}

fn topology(
    mesh: &nif_scene::MeshData,
    budget: &mut pose::Budget<'_>,
) -> Result<(Topology, Vec<Triangle>)> {
    budget.reserve::<Triangle>(mesh.triangles.len())?;
    let mut triangles = Vec::with_capacity(mesh.triangles.len());
    let topology = match &mesh.topology {
        nif_scene::Topology::Triangles {
            declared_points,
            present,
            indices,
            match_groups,
        } => {
            for (ordinal, &vertices) in mesh.triangles.iter().enumerate() {
                budget.charge(1)?;
                triangles.push(Triangle {
                    vertices,
                    source_primitive_ordinal: ordinal,
                    strip_ordinal: None,
                    strip_step_ordinal: None,
                });
            }
            Topology::Triangles {
                declared_points: *declared_points,
                present: *present,
                indices: copy(indices, budget)?,
                match_groups: nested(match_groups, budget)?,
            }
        }
        nif_scene::Topology::Strips {
            lengths,
            present,
            indices,
        } => {
            let mut primitive = 0usize;
            for (strip_ordinal, strip) in indices.iter().enumerate() {
                budget.charge(1)?;
                for (step, face) in strip.windows(3).enumerate() {
                    budget.charge(1)?;
                    if face[0] != face[1] && face[0] != face[2] && face[1] != face[2] {
                        let vertices = *mesh
                            .triangles
                            .get(triangles.len())
                            .ok_or_else(|| budget.fail("decoded strip triangle extent differs"))?;
                        triangles.push(Triangle {
                            vertices,
                            source_primitive_ordinal: primitive,
                            strip_ordinal: Some(strip_ordinal),
                            strip_step_ordinal: Some(step),
                        });
                    }
                    primitive = primitive
                        .checked_add(1)
                        .ok_or_else(|| budget.fail("strip primitive ordinal overflow"))?;
                }
            }
            Topology::Strips {
                lengths: copy(lengths, budget)?,
                present: *present,
                indices: nested(indices, budget)?,
            }
        }
    };
    if triangles.len() != mesh.triangles.len() {
        return Err(budget.fail("decoded triangle extent differs"));
    }
    Ok((topology, triangles))
}
