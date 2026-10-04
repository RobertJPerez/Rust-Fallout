//! Cooperative execution between indivisible bounded geometry evaluations.
use super::{
    BatchEvaluationLimits, Budget, DecodedView, Evaluation, GeometryBatch, Limits,
    PreparationUsage, PreparedSkinSource, Request, SourceHash,
};
use crate::Result;
use serde::Serialize;

#[derive(Clone, Copy, Debug)]
pub struct GeometryStepBudget {
    pub geometries: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationState {
    Running,
    Complete,
    Cancelled,
    Failed,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct JobAdmission {
    pub source_preparation: PreparationUsage,
    /// Job/result headers, request copy, output capacity and result hash bytes.
    pub retained_bytes: usize,
    /// Digest check, whole-set validation and request copying.
    pub work_units: usize,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Progress {
    pub state: EvaluationState,
    pub total_geometries: usize,
    /// Historical successful evaluations, even after outputs are discarded.
    pub completed_geometries: usize,
    pub advanced_geometries: usize,
    pub step_retained_bytes: usize,
    pub step_work_units: usize,
    /// Complete charged evaluation elements, including released temporaries.
    pub evaluation_retained_bytes: usize,
    pub evaluation_work_units: usize,
}

/// Borrows sealed source authority and owns bounded requests/partial results.
/// No arrays are accessible before successful consuming `finish`.
pub struct EvaluationJob<'a> {
    source: &'a PreparedSkinSource,
    requests: Vec<Request>,
    outputs: Vec<Evaluation>,
    limits: BatchEvaluationLimits,
    storage_left: usize,
    work_left: usize,
    admission: JobAdmission,
    completed: usize,
    state: EvaluationState,
}
impl std::fmt::Debug for EvaluationJob<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvaluationJob")
            .field("source_sha256", &self.source.source_sha256)
            .field("admission", &self.admission)
            .field("progress", &self.progress())
            .finish_non_exhaustive()
    }
}
impl PreparedSkinSource {
    pub fn begin_evaluation(
        &self,
        expected_source_sha256: [u8; 32],
        requests: &[Request],
        limits: BatchEvaluationLimits,
    ) -> Result<EvaluationJob<'_>> {
        let mut budget = Budget {
            source: &self.source_sha256,
            storage: limits.array_bytes,
            work: limits.work_units,
        };
        if requests.is_empty() || requests.len() > limits.geometries {
            return Err(budget.fail("skin job requires a nonempty bounded geometry request set"));
        }
        budget.charge(32)?;
        if self.digest != expected_source_sha256 {
            return Err(budget.fail("skin job source SHA256 differs"));
        }
        for (ordinal, request) in requests.iter().enumerate() {
            budget.charge(1 + ordinal)?;
            super::super::validate_weight_policy(request.weights, &budget)?;
            if requests[..ordinal]
                .iter()
                .any(|r| r.geometry == request.geometry)
            {
                return Err(budget.fail("skin job duplicate geometry request"));
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
        budget.reserve::<EvaluationJob<'_>>(1)?;
        budget.reserve::<GeometryBatch>(1)?;
        budget.reserve::<Request>(requests.len())?;
        budget.reserve::<Evaluation>(requests.len())?;
        budget.reserve::<u8>(64)?;
        budget.charge(requests.len())?;
        let admission = JobAdmission {
            source_preparation: self.usage,
            retained_bytes: limits.array_bytes - budget.storage,
            work_units: limits.work_units - budget.work,
        };
        Ok(EvaluationJob {
            source: self,
            requests: requests.to_vec(),
            outputs: Vec::with_capacity(requests.len()),
            limits,
            storage_left: budget.storage,
            work_left: budget.work,
            admission,
            completed: 0,
            state: EvaluationState::Running,
        })
    }
}
impl EvaluationJob<'_> {
    pub fn admission(&self) -> JobAdmission {
        self.admission
    }
    pub fn progress(&self) -> Progress {
        Progress {
            state: self.state,
            total_geometries: self.requests.len(),
            completed_geometries: self.completed,
            advanced_geometries: 0,
            step_retained_bytes: 0,
            step_work_units: 0,
            evaluation_retained_bytes: self.limits.array_bytes
                - self.storage_left
                - self.admission.retained_bytes,
            evaluation_work_units: self.limits.work_units
                - self.work_left
                - self.admission.work_units,
        }
    }
    pub fn advance(&mut self, step: GeometryStepBudget) -> Result<Progress> {
        if self.state != EvaluationState::Running {
            return Err(self.failure("skin job is terminal; cannot advance"));
        }
        let before = self.progress();
        let count = step.geometries.min(self.requests.len() - self.completed);
        for _ in 0..count {
            let per_geometry = Limits {
                array_bytes: self.limits.geometry.array_bytes.min(self.storage_left),
                work_units: self.limits.geometry.work_units.min(self.work_left),
                ancestry_depth: self.limits.geometry.ancestry_depth,
                ..Default::default()
            };
            let source = self.source.source_sha256.as_str();
            let mut budget = Budget {
                source,
                storage: per_geometry.array_bytes,
                work: per_geometry.work_units,
            };
            let evaluated = super::super::evaluate_decoded_with_budget(
                DecodedView {
                    source,
                    hash: SourceHash::Prepared(source),
                    index: &self.source.index,
                    decoded: &self.source.decoded,
                    scene: &self.source.scene,
                },
                self.requests[self.completed],
                per_geometry,
                None,
                None,
                &mut budget,
            );
            // Budget borrowing preserves actual existing charges on refusal.
            self.storage_left -= per_geometry.array_bytes - budget.storage;
            self.work_left -= per_geometry.work_units - budget.work;
            match evaluated {
                Ok(value) => {
                    self.outputs.push(value);
                    self.completed += 1;
                }
                Err(error) => {
                    self.outputs = Vec::new();
                    self.state = EvaluationState::Failed;
                    return Err(error);
                }
            }
        }
        if self.completed == self.requests.len() {
            self.state = EvaluationState::Complete;
        }
        let mut progress = self.progress();
        progress.advanced_geometries = self.completed - before.completed_geometries;
        progress.step_retained_bytes =
            progress.evaluation_retained_bytes - before.evaluation_retained_bytes;
        progress.step_work_units = progress.evaluation_work_units - before.evaluation_work_units;
        Ok(progress)
    }
    /// Idempotent terminal cancellation also discards a completed/failed job.
    pub fn cancel(&mut self) -> Progress {
        self.outputs = Vec::new();
        self.state = EvaluationState::Cancelled;
        self.progress()
    }
    pub fn finish(self) -> Result<GeometryBatch> {
        if self.state != EvaluationState::Complete {
            return Err(self.failure("skin job has no complete result"));
        }
        Ok(GeometryBatch {
            contract: "engineering-shared-source-skin-batch-v1",
            source_sha256: self.source.source_sha256.clone(),
            geometries: self.outputs,
            preparation: self.source.usage,
            decoder_array_admission_bytes: self.source.usage.decoder_array_admission_bytes,
            decoder_check_admission_units: self.source.usage.decoder_check_admission_units,
            source_binding_retained_bytes: self.source.usage.source_binding_retained_bytes,
            retained_bytes: self.limits.array_bytes - self.storage_left,
            work_units: self.limits.work_units - self.work_left,
            retail_behavior_verified: false,
        })
    }
    fn failure(&self, detail: &str) -> crate::Error {
        Budget {
            source: &self.source.source_sha256,
            storage: 0,
            work: 0,
        }
        .fail(detail)
    }
}
