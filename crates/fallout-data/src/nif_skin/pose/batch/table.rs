//! Join two sealed producers without source bytes, decoding or CSR reconstruction.
use super::{
    Budget, DecodedView, Evaluation, GeometryLimits, Limits, PreparedSkinSource, Request,
    SourceHash,
};
use crate::{
    Result,
    nif_skin::{
        influences,
        storage::{admit_index, admit_scene},
    },
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
