//! Exact raw per-vertex influences, bound privately to one selected source skin.
use super::{Data, binding, pose};
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source: binding::Limits,
    pub entries: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    /// Admit the complete declared source decoder allowances before decoding.
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            entries: 4_000_000,
            array_bytes: 32 * 1024 * 1024,
            work_units: 16_000_000,
            decoder_array_admission_bytes: 512 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub bone_ordinal: usize,
    pub weight_bits: u32,
    /// Physical ordinal inside this bone's original weight array.
    pub source_weight_ordinal: usize,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub entries: usize,
    /// Charged table elements and temporary counts/sums; excludes spare capacity.
    pub array_bytes: usize,
    /// Table headers/hash/CSR elements retained after preparation.
    pub output_bytes: usize,
    pub work_units: usize,
    pub source_retained_bytes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
}

/// Private identity and arrays; serialization is observation, never a constructor.
#[derive(Debug, Serialize)]
pub struct Table {
    contract: &'static str,
    source_sha256: String,
    #[serde(skip)]
    source_digest: [u8; 32],
    geometry: u32,
    geometry_data: u32,
    instance: u32,
    skin_data: u32,
    bone_count: usize,
    vertex_offsets: Vec<usize>,
    entries: Vec<Entry>,
    usage: Usage,
    retail_behavior_verified: bool,
}

impl Table {
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    pub fn geometry(&self) -> u32 {
        self.geometry
    }
    pub fn instance(&self) -> u32 {
        self.instance
    }
    pub fn vertex_offsets(&self) -> &[usize] {
        &self.vertex_offsets
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// Reuse only this source/geometry. Palette construction and source checks
    /// remain the existing private pose pipeline; CSR supplies its influences.
    pub fn evaluate(
        &self,
        bytes: &[u8],
        source: &str,
        request: pose::Request,
        mut limits: pose::Limits,
    ) -> Result<pose::Evaluation> {
        let budget = Budget {
            source,
            bytes: limits.array_bytes,
            work: limits.work_units,
        };
        if bytes.len() > limits.source.partition.skin.scene.input_bytes {
            return Err(budget.fail("source input byte budget exceeded"));
        }
        if request.geometry != self.geometry {
            return Err(budget.fail("table geometry differs from requested geometry"));
        }
        if <[u8; 32]>::from(Sha256::digest(bytes)) != self.source_digest {
            return Err(budget.fail("table source SHA256 differs"));
        }
        // The live table and returned pose coexist; admit both logical outputs.
        limits.array_bytes = limits
            .array_bytes
            .checked_sub(self.usage.output_bytes)
            .ok_or_else(|| budget.fail("table plus pose array storage budget exceeded"))?;
        pose::evaluate_table(bytes, source, request, limits, self)
    }

    pub(super) fn matches(
        &self,
        geometry_data: u32,
        instance: u32,
        skin_data: u32,
        bones: usize,
        vertices: usize,
    ) -> bool {
        self.geometry_data == geometry_data
            && self.instance == instance
            && self.skin_data == skin_data
            && self.bone_count == bones
            && self.vertex_offsets.len().checked_sub(1) == Some(vertices)
    }
    pub(super) fn vertex(&self, vertex: usize) -> Option<&[Entry]> {
        self.entries.get(
            *self.vertex_offsets.get(vertex)?..*self.vertex_offsets.get(vertex.checked_add(1)?)?,
        )
    }
}

struct Budget<'a> {
    source: &'a str,
    bytes: usize,
    work: usize,
}
impl Budget<'_> {
    fn fail(&self, detail: &str) -> Error {
        Error::Unsupported(format!(
            "{}: exact source influences: {detail}",
            self.source
        ))
    }
    fn reserve<T>(&mut self, count: usize) -> Result<()> {
        self.bytes = count
            .checked_mul(std::mem::size_of::<T>())
            .and_then(|n| self.bytes.checked_sub(n))
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

fn prefix_next(previous: usize, count: usize, budget: &Budget<'_>) -> Result<usize> {
    previous
        .checked_add(count)
        .ok_or_else(|| budget.fail("influence prefix sum overflow"))
}

pub fn prepare(bytes: &[u8], source: &str, geometry: u32, limits: Limits) -> Result<Table> {
    let mut budget = Budget {
        source,
        bytes: limits.array_bytes,
        work: limits.work_units,
    };
    if bytes.len() > limits.source.partition.skin.scene.input_bytes {
        return Err(budget.fail("source input byte budget exceeded"));
    }
    let decoder_arrays = [
        limits.source.partition.skin.scene.array_bytes,
        limits.source.partition.skin.skin_array_bytes,
        limits.source.partition.array_bytes,
        limits.source.array_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |s, n| s.checked_add(n))
    .filter(|n| *n <= limits.decoder_array_admission_bytes)
    .ok_or_else(|| budget.fail("decoder array admission exceeded"))?;
    let decoder_checks = [
        limits.source.partition.skin.weight_index_checks,
        limits.source.partition.index_checks,
        limits.source.graph_checks,
    ]
    .into_iter()
    .try_fold(0usize, |s, n| s.checked_add(n))
    .filter(|n| *n <= limits.decoder_check_admission_units)
    .ok_or_else(|| budget.fail("decoder check admission exceeded"))?;
    let (_, decoded, scene) = binding::decode_with_scene(bytes, source, limits.source)?;
    if !decoded.bindings.unsupported_scene_edges.is_empty() {
        return Err(budget.fail("unresolved scene ancestry"));
    }
    budget.charge(
        decoded.skin.skin.owners.len()
            + decoded.skin.skin.blocks.len()
            + decoded.bindings.instances.len()
            + scene.meshes.len(),
    )?;
    let owner = decoded
        .skin
        .skin
        .owners
        .iter()
        .find(|o| o.geometry == geometry)
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
    let bound = decoded
        .bindings
        .instances
        .iter()
        .find(|b| b.instance == owner.instance)
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
            .find(|o| o.geometry == geometry)
            .is_none_or(|o| !o.reachable_from_footer || o.decoded_root_contains != Some(true))
    {
        return Err(budget.fail("unresolved root, bone or owner ancestry"));
    }
    let data_id = instance
        .data
        .ok_or_else(|| budget.fail("missing NiSkinData"))?;
    let bones = decoded
        .skin
        .skin
        .blocks
        .iter()
        .find(|b| b.block == data_id)
        .and_then(|b| match &b.data {
            Data::SkinData {
                has_vertex_weights,
                bones,
                ..
            } if *has_vertex_weights != 0 => Some(bones),
            _ => None,
        })
        .ok_or_else(|| budget.fail("NiSkinData vertex weights unavailable"))?;
    if bones.is_empty() || bones.len() != instance.bones.len() {
        return Err(budget.fail("empty or mismatched bone palette"));
    }
    let geometry_data = owner
        .geometry_data
        .ok_or_else(|| budget.fail("geometry data unavailable"))?;
    let mesh = scene
        .meshes
        .iter()
        .find(|m| m.block == geometry_data)
        .ok_or_else(|| budget.fail("geometry data not decoded"))?;
    let vertices = mesh.vertices.len();
    if !mesh.has_vertices || vertices == 0 || vertices != usize::from(mesh.vertex_count) {
        return Err(budget.fail("vertex positions unavailable"));
    }
    let offset_count = vertices
        .checked_add(1)
        .ok_or_else(|| budget.fail("vertex offset count overflow"))?;
    budget.reserve::<Table>(1)?;
    budget.reserve::<u8>(64)?;
    budget.reserve::<usize>(vertices)?;
    budget.reserve::<f64>(vertices)?;
    budget.reserve::<usize>(offset_count)?;
    let mut counts = vec![0usize; vertices];
    let mut sums = vec![0.; vertices];
    let mut total = 0usize;
    for bone in bones {
        budget.charge(1)?;
        for weight in &bone.weights {
            budget.charge(1)?;
            let value =
                super::raw_weight(weight.weight_bits).map_err(|detail| budget.fail(detail))?;
            let vertex = usize::from(weight.vertex);
            let count = counts
                .get_mut(vertex)
                .ok_or_else(|| budget.fail("weight vertex out of range"))?;
            *count = prefix_next(*count, 1, &budget)?;
            total = prefix_next(total, 1, &budget)?;
            if total > limits.entries {
                return Err(budget.fail("total influence entry budget exceeded"));
            }
            sums[vertex] += value;
        }
    }
    budget.charge(vertices)?;
    for (vertex, sum) in sums.iter().copied().enumerate() {
        if let Some(detail) =
            super::weight_sum_error(vertex, sum, pose::WeightPolicy::PreserveRawNonnegative)
        {
            return Err(budget.fail(&detail));
        }
    }
    budget.reserve::<Entry>(total)?;
    let mut offsets = Vec::with_capacity(offset_count);
    offsets.push(0);
    for (vertex, count) in counts.iter_mut().enumerate() {
        budget.charge(1)?;
        let next = prefix_next(offsets[vertex], *count, &budget)?;
        *count = offsets[vertex];
        offsets.push(next);
    }
    if offsets[vertices] != total {
        return Err(budget.fail("influence count differs from prefix sum"));
    }
    let mut entries = vec![
        Entry {
            bone_ordinal: 0,
            weight_bits: 0,
            source_weight_ordinal: 0
        };
        total
    ];
    for (bone_ordinal, bone) in bones.iter().enumerate() {
        budget.charge(1)?;
        for (source_weight_ordinal, weight) in bone.weights.iter().enumerate() {
            budget.charge(1)?;
            let vertex = usize::from(weight.vertex);
            let slot = counts[vertex];
            if slot >= offsets[vertex + 1] {
                return Err(budget.fail("influence fill exceeds counted vertex range"));
            }
            entries[slot] = Entry {
                bone_ordinal,
                weight_bits: weight.weight_bits,
                source_weight_ordinal,
            };
            counts[vertex] = prefix_next(slot, 1, &budget)?;
        }
    }
    let source_retained_bytes = [
        decoded.skin.skin.retained_bytes,
        decoded.skin.partitions.retained_bytes,
        decoded.bindings.retained_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |s, n| s.checked_add(n))
    .ok_or_else(|| budget.fail("source retained byte sum overflow"))?;
    let charged = limits.array_bytes - budget.bytes;
    let scratch = vertices
        .checked_mul(std::mem::size_of::<usize>() + std::mem::size_of::<f64>())
        .ok_or_else(|| budget.fail("scratch byte sum overflow"))?;
    let digest = Sha256::digest(bytes);
    Ok(Table {
        contract: "engineering-exact-raw-influence-csr-v1",
        source_sha256: format!("{digest:x}"),
        source_digest: digest.into(),
        geometry,
        geometry_data,
        instance: owner.instance,
        skin_data: data_id,
        bone_count: bones.len(),
        vertex_offsets: offsets,
        entries,
        usage: Usage {
            entries: total,
            array_bytes: charged,
            output_bytes: charged - scratch,
            work_units: limits.work_units - budget.work,
            source_retained_bytes,
            decoder_array_admission_bytes: decoder_arrays,
            decoder_check_admission_units: decoder_checks,
        },
        retail_behavior_verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_refuses_before_any_prefix_can_wrap() {
        let budget = Budget {
            source: "hostile declared counts",
            bytes: 0,
            work: 0,
        };
        assert_eq!(prefix_next(usize::MAX - 1, 1, &budget).unwrap(), usize::MAX);
        assert!(
            prefix_next(usize::MAX, 1, &budget)
                .unwrap_err()
                .to_string()
                .contains("prefix sum overflow")
        );
    }
}
