//! Sealed owned source preparation and atomic multi-geometry evaluation.
mod job;
mod table;

pub use job::{EvaluationJob, EvaluationState, GeometryStepBudget, JobAdmission, Progress};

use super::{Budget, DecodedView, Evaluation, Limits, Request, SourceHash, binding};
use crate::{Result, nif, nif_scene};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug)]
pub struct PreparationLimits {
    pub source: binding::Limits,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    /// Additional source header, hash and geometry-owner map elements.
    pub array_bytes: usize,
    /// Source hash byte visits, owner admission and map initialization.
    pub work_units: usize,
}
impl Default for PreparationLimits {
    fn default() -> Self {
        Self {
            source: Default::default(),
            decoder_array_admission_bytes: 512 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            array_bytes: 4 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct PreparationUsage {
    pub source_bytes: usize,
    pub binding_decodes: usize,
    pub scene_decodes: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    /// Existing skin/partition/binding charged elements, excluding scene/index.
    pub source_binding_retained_bytes: usize,
    pub retained_bytes: usize,
    pub work_units: usize,
}

/// Owns existing decoder results without input borrows, public construction,
/// serialization, mutation or a global cache granting source authority.
#[derive(Debug)]
pub struct PreparedSkinSource {
    index: nif::NifIndex,
    decoded: binding::Source,
    scene: nif_scene::Scene,
    geometry_owners: Vec<Option<usize>>,
    digest: [u8; 32],
    source_sha256: String,
    usage: PreparationUsage,
}
impl PreparedSkinSource {
    pub fn prepare(bytes: &[u8], source: &str, limits: PreparationLimits) -> Result<Self> {
        Self::prepare_checked(bytes, source, limits, None)
    }
    fn prepare_checked(
        bytes: &[u8],
        source: &str,
        limits: PreparationLimits,
        expected: Option<[u8; 32]>,
    ) -> Result<Self> {
        let mut budget = Budget {
            source,
            storage: limits.array_bytes,
            work: limits.work_units,
        };
        if bytes.len() > limits.source.partition.skin.scene.input_bytes {
            return Err(budget.fail("shared skin source input byte budget exceeded"));
        }
        let arrays = [
            limits.source.partition.skin.scene.array_bytes,
            limits.source.partition.skin.skin_array_bytes,
            limits.source.partition.array_bytes,
            limits.source.array_bytes,
        ]
        .into_iter()
        .try_fold(0usize, |sum, n| sum.checked_add(n))
        .filter(|n| *n <= limits.decoder_array_admission_bytes)
        .ok_or_else(|| budget.fail("shared skin decoder array admission exceeded"))?;
        let checks = [
            limits.source.partition.skin.weight_index_checks,
            limits.source.partition.index_checks,
            limits.source.graph_checks,
        ]
        .into_iter()
        .try_fold(0usize, |sum, n| sum.checked_add(n))
        .filter(|n| *n <= limits.decoder_check_admission_units)
        .ok_or_else(|| budget.fail("shared skin decoder check admission exceeded"))?;
        budget.reserve::<Self>(1)?;
        budget.reserve::<u8>(64)?;
        budget.charge(bytes.len())?;
        let hash = Sha256::digest(bytes);
        let digest: [u8; 32] = hash.into();
        if expected.is_some_and(|expected| expected != digest) {
            return Err(budget.fail("shared skin source SHA256 differs"));
        }
        // Sole existing binding/Scene decode for the lifetime of this object.
        let (index, decoded, scene) = binding::decode_with_scene(bytes, source, limits.source)?;
        if !decoded.bindings.unsupported_scene_edges.is_empty() {
            return Err(budget.fail("unresolved scene ancestry"));
        }
        budget.reserve::<Option<usize>>(index.blocks.len())?;
        budget.charge(index.blocks.len())?;
        budget.charge(decoded.skin.skin.owners.len())?;
        let mut geometry_owners = vec![None; index.blocks.len()];
        for (ordinal, owner) in decoded.skin.skin.owners.iter().enumerate() {
            let slot = geometry_owners
                .get_mut(owner.geometry as usize)
                .ok_or_else(|| budget.fail("prepared skin geometry owner out of range"))?;
            if slot.replace(ordinal).is_some() {
                return Err(budget.fail("prepared skin duplicate geometry owner"));
            }
        }
        let retained = [
            decoded.skin.skin.retained_bytes,
            decoded.skin.partitions.retained_bytes,
            decoded.bindings.retained_bytes,
        ]
        .into_iter()
        .try_fold(0usize, |sum, n| sum.checked_add(n))
        .ok_or_else(|| budget.fail("shared skin source retained byte sum overflow"))?;
        let usage = PreparationUsage {
            source_bytes: bytes.len(),
            binding_decodes: 1,
            scene_decodes: 1,
            decoder_array_admission_bytes: arrays,
            decoder_check_admission_units: checks,
            source_binding_retained_bytes: retained,
            retained_bytes: limits.array_bytes - budget.storage,
            work_units: limits.work_units - budget.work,
        };
        Ok(Self {
            index,
            decoded,
            scene,
            geometry_owners,
            digest,
            source_sha256: format!("{hash:x}"),
            usage,
        })
    }
    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    pub fn usage(&self) -> PreparationUsage {
        self.usage
    }
    pub fn evaluate_many(
        &self,
        source: &str,
        expected_source_sha256: [u8; 32],
        requests: &[Request],
        limits: BatchEvaluationLimits,
    ) -> Result<GeometryBatch> {
        let mut budget = Budget {
            source,
            storage: limits.array_bytes,
            work: limits.work_units,
        };
        if requests.is_empty() || requests.len() > limits.geometries {
            return Err(budget.fail("shared skin requires a nonempty bounded geometry request set"));
        }
        budget.charge(32)?;
        if self.digest != expected_source_sha256 {
            return Err(budget.fail("shared skin source SHA256 differs"));
        }
        for (ordinal, request) in requests.iter().enumerate() {
            budget.charge(1 + ordinal)?;
            super::validate_weight_policy(request.weights, &budget)?;
            if requests[..ordinal]
                .iter()
                .any(|r| r.geometry == request.geometry)
            {
                return Err(budget.fail("shared skin duplicate geometry request"));
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
        }
        budget.reserve::<GeometryBatch>(1)?;
        budget.reserve::<Evaluation>(requests.len())?;
        budget.reserve::<u8>(64)?;
        let view = DecodedView {
            source,
            hash: SourceHash::Prepared(&self.source_sha256),
            index: &self.index,
            decoded: &self.decoded,
            scene: &self.scene,
        };
        let mut geometries = Vec::with_capacity(requests.len());
        for request in requests {
            let per_geometry = Limits {
                array_bytes: limits.geometry.array_bytes.min(budget.storage),
                work_units: limits.geometry.work_units.min(budget.work),
                ancestry_depth: limits.geometry.ancestry_depth,
                ..Default::default()
            };
            let value = super::evaluate_decoded(
                view,
                *request,
                per_geometry,
                None,
                None,
                Budget {
                    source,
                    storage: per_geometry.array_bytes,
                    work: per_geometry.work_units,
                },
            )?;
            budget.reserve::<u8>(value.retained_bytes)?;
            budget.charge(value.work_units)?;
            geometries.push(value);
        }
        Ok(GeometryBatch {
            contract: "engineering-shared-source-skin-batch-v1",
            source_sha256: self.source_sha256.clone(),
            geometries,
            preparation: self.usage,
            decoder_array_admission_bytes: self.usage.decoder_array_admission_bytes,
            decoder_check_admission_units: self.usage.decoder_check_admission_units,
            source_binding_retained_bytes: self.usage.source_binding_retained_bytes,
            retained_bytes: limits.array_bytes - budget.storage,
            work_units: limits.work_units - budget.work,
            retail_behavior_verified: false,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GeometryLimits {
    pub array_bytes: usize,
    pub work_units: usize,
    pub ancestry_depth: usize,
}
impl Default for GeometryLimits {
    fn default() -> Self {
        let pose = Limits::default();
        Self {
            array_bytes: pose.array_bytes,
            work_units: pose.work_units,
            ancestry_depth: pose.ancestry_depth,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct BatchEvaluationLimits {
    pub geometry: GeometryLimits,
    pub geometries: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for BatchEvaluationLimits {
    fn default() -> Self {
        Self {
            geometry: Default::default(),
            geometries: 64,
            array_bytes: 128 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct BatchLimits {
    /// Source limits apply once at preparation; remaining pose caps per geometry.
    pub pose: Limits,
    pub geometries: usize,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub preparation_array_bytes: usize,
    pub preparation_work_units: usize,
    pub array_bytes: usize,
    pub work_units: usize,
}
impl Default for BatchLimits {
    fn default() -> Self {
        Self {
            pose: Default::default(),
            geometries: 64,
            decoder_array_admission_bytes: 512 * 1024 * 1024,
            decoder_check_admission_units: 64_000_000,
            preparation_array_bytes: 4 * 1024 * 1024,
            preparation_work_units: 128_000_000,
            array_bytes: 128 * 1024 * 1024,
            work_units: 128_000_000,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct GeometryBatch {
    pub contract: &'static str,
    pub source_sha256: String,
    pub geometries: Vec<Evaluation>,
    pub preparation: PreparationUsage,
    pub decoder_array_admission_bytes: usize,
    pub decoder_check_admission_units: usize,
    pub source_binding_retained_bytes: usize,
    /// Evaluation only, including charged temporary elements and output headers.
    pub retained_bytes: usize,
    pub work_units: usize,
    pub retail_behavior_verified: bool,
}

pub fn evaluate_many(
    bytes: &[u8],
    source: &str,
    expected_source_sha256: [u8; 32],
    requests: &[Request],
    limits: BatchLimits,
) -> Result<GeometryBatch> {
    let prepared = PreparedSkinSource::prepare_checked(
        bytes,
        source,
        PreparationLimits {
            source: limits.pose.source,
            decoder_array_admission_bytes: limits.decoder_array_admission_bytes,
            decoder_check_admission_units: limits.decoder_check_admission_units,
            array_bytes: limits.preparation_array_bytes,
            work_units: limits.preparation_work_units,
        },
        Some(expected_source_sha256),
    )?;
    prepared.evaluate_many(
        source,
        expected_source_sha256,
        requests,
        BatchEvaluationLimits {
            geometry: GeometryLimits {
                array_bytes: limits.pose.array_bytes,
                work_units: limits.pose.work_units,
                ancestry_depth: limits.pose.ancestry_depth,
            },
            geometries: limits.geometries,
            array_bytes: limits.array_bytes,
            work_units: limits.work_units,
        },
    )
}
