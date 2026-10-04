//! Join two sealed producers without source bytes, decoding or CSR reconstruction.
use super::{
    Budget, DecodedView, Evaluation, GeometryLimits, Limits, PreparedSkinSource, Request,
    SourceHash,
};
use crate::{Result, nif, nif_scene, nif_skin::influences};
use nif_scene::material::{
    MaterialBlock, MaterialData, ShaderTexture, TextureReference, TextureSlot,
};

impl PreparedSkinSource {
    /// Reuse an independently prepared exact influence table with this source.
    /// `array_bytes` admits the concurrently live source, table and charged pose
    /// elements. Returned storage/work include that admission and metadata visits;
    /// earlier one-shot and geometry-batch counter scopes are unchanged.
    pub fn evaluate_table(
        &self,
        expected_source_sha256: [u8; 32],
        request: Request,
        table: &influences::Table,
        limits: GeometryLimits,
    ) -> Result<Evaluation> {
        let source = self.source_sha256.as_str();
        let mut budget = Budget {
            source,
            storage: limits.array_bytes,
            work: limits.work_units,
        };
        super::super::validate_weight_policy(request.weights, &budget)?;
        budget.charge(32)?;
        if self.digest != expected_source_sha256 {
            return Err(budget.fail("prepared table source SHA256 differs"));
        }
        budget.charge(32)?;
        if !table.matches_source(&self.digest) {
            return Err(budget.fail("influence table source SHA256 differs"));
        }
        budget.charge(3)?;
        if table.geometry() != request.geometry {
            return Err(budget.fail("table geometry differs from requested geometry"));
        }
        if table.vertex_offsets().first() != Some(&0)
            || table.vertex_offsets().last() != Some(&table.entries().len())
        {
            return Err(budget.fail("influence table CSR extent differs"));
        }
        if self
            .geometry_owners
            .get(request.geometry as usize)
            .copied()
            .flatten()
            .is_none()
        {
            return Err(budget.fail("selected geometry has no decoded skin owner"));
        }
        // Existing receipts include headers, skin/binding/partition arrays and
        // conservative decoder scratch. Scene/index allocations are separate.
        budget.reserve::<u8>(self.usage.retained_bytes)?;
        budget.reserve::<u8>(self.usage.source_binding_retained_bytes)?;
        admit_index(&self.index, &mut budget)?;
        admit_scene(&self.scene, &mut budget)?;
        budget.reserve::<u8>(table.usage().output_bytes)?;
        // The existing deformer checks geometry-data, instance, skin-data, bone
        // count and vertex cardinality before allocating maps/palette/output.
        super::super::evaluate_decoded(
            DecodedView {
                source,
                hash: SourceHash::Prepared(source),
                index: &self.index,
                decoded: &self.decoded,
                scene: &self.scene,
            },
            request,
            Limits {
                array_bytes: limits.array_bytes,
                work_units: limits.work_units,
                ancestry_depth: limits.ancestry_depth,
                ..Default::default()
            },
            None,
            Some(table),
            budget,
        )
    }
}

// Logical element storage, excluding Vec capacity and allocator bookkeeping.
// No payload contents are scanned. Each length/variant visit is work charged.
fn payload<T>(count: usize, budget: &mut Budget<'_>) -> Result<()> {
    budget.charge(1)?;
    budget.reserve::<T>(count)
}
fn nested<T>(arrays: &[Vec<T>], budget: &mut Budget<'_>) -> Result<()> {
    payload::<Vec<T>>(arrays.len(), budget)?;
    for array in arrays {
        payload::<T>(array.len(), budget)?;
    }
    Ok(())
}
fn admit_index(index: &nif::NifIndex, budget: &mut Budget<'_>) -> Result<()> {
    nested(&index.export_strings, budget)?;
    payload::<String>(index.block_types.len(), budget)?;
    for name in &index.block_types {
        payload::<u8>(name.len(), budget)?;
    }
    payload::<nif::Block>(index.blocks.len(), budget)?;
    // Four machine words per logical map entry conservatively charge links.
    payload::<(String, usize, [usize; 4])>(index.block_counts.len(), budget)?;
    for name in index.block_counts.keys() {
        payload::<u8>(name.len(), budget)?;
    }
    nested(&index.strings, budget)?;
    payload::<u32>(index.groups.len(), budget)?;
    payload::<Option<u32>>(index.roots.len(), budget)
}
fn admit_scene(scene: &nif_scene::Scene, budget: &mut Budget<'_>) -> Result<()> {
    payload::<nif_scene::Object>(scene.objects.len(), budget)?;
    for object in &scene.objects {
        budget.charge(1)?;
        payload::<Option<u32>>(object.extra_data.len(), budget)?;
        payload::<Option<u32>>(object.properties.len(), budget)?;
        match &object.kind {
            nif_scene::ObjectKind::Node { children, effects } => {
                payload::<Option<u32>>(children.len(), budget)?;
                payload::<Option<u32>>(effects.len(), budget)?;
            }
            nif_scene::ObjectKind::Mesh {
                material_names,
                material_extra,
                ..
            } => {
                payload::<Option<u32>>(material_names.len(), budget)?;
                payload::<i32>(material_extra.len(), budget)?;
            }
        }
    }
    payload::<nif_scene::MeshData>(scene.meshes.len(), budget)?;
    for mesh in &scene.meshes {
        budget.charge(1)?;
        for arrays in [
            &mesh.vertices,
            &mesh.normals,
            &mesh.tangents,
            &mesh.bitangents,
        ] {
            payload::<[f32; 3]>(arrays.len(), budget)?;
        }
        payload::<[f32; 4]>(mesh.colors.len(), budget)?;
        nested(&mesh.uv_sets, budget)?;
        payload::<[u16; 3]>(mesh.triangles.len(), budget)?;
        match &mesh.topology {
            nif_scene::Topology::Triangles {
                indices,
                match_groups,
                ..
            } => {
                payload::<[u16; 3]>(indices.len(), budget)?;
                nested(match_groups, budget)?;
            }
            nif_scene::Topology::Strips {
                lengths, indices, ..
            } => {
                payload::<u16>(lengths.len(), budget)?;
                nested(indices, budget)?;
            }
        }
    }
    payload::<MaterialBlock>(scene.materials.len(), budget)?;
    for material in &scene.materials {
        budget.charge(1)?;
        if let Some(header) = &material.object {
            payload::<Option<u32>>(header.extra_data.len(), budget)?;
        }
        match &material.data {
            MaterialData::NoLighting { texture, .. } => payload::<u8>(texture.len(), budget)?,
            MaterialData::TextureSet { textures } => nested(textures, budget)?,
            MaterialData::Texturing {
                slots,
                shader_textures,
                ..
            } => {
                payload::<Option<TextureSlot>>(slots.len(), budget)?;
                payload::<Option<ShaderTexture>>(shader_textures.len(), budget)?;
            }
            MaterialData::Material { .. }
            | MaterialData::Alpha { .. }
            | MaterialData::Stencil { .. }
            | MaterialData::Shade { .. }
            | MaterialData::PerPixelLighting { .. }
            | MaterialData::SourceTexture { .. } => {}
        }
    }
    payload::<TextureReference>(scene.textures.len(), budget)?;
    for texture in &scene.textures {
        budget.charge(1)?;
        payload::<u8>(texture.raw_path.len(), budget)?;
        if let Some(path) = &texture.asset_path {
            payload::<u8>(path.bytes().len(), budget)?;
        }
        if let Some(error) = &texture.error {
            payload::<u8>(error.len(), budget)?;
        }
    }
    payload::<nif_scene::ExtraFlags>(scene.extra_flags.len(), budget)?;
    payload::<nif_scene::WorldTransform>(scene.world_transforms.len(), budget)?;
    payload::<(String, Vec<u32>, [usize; 4])>(scene.unsupported_blocks.len(), budget)?;
    for (name, ids) in &scene.unsupported_blocks {
        payload::<u8>(name.len(), budget)?;
        payload::<u32>(ids.len(), budget)?;
    }
    payload::<nif_scene::UnsupportedEdge>(scene.unsupported_scene_edges.len(), budget)?;
    for edge in &scene.unsupported_scene_edges {
        payload::<u8>(edge.block_type.len(), budget)?;
    }
    Ok(())
}
