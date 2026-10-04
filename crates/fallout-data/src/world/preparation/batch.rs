use super::CellModelPlan;
use crate::{
    cache,
    model_probe::{self, ModelProbe},
    resource_jobs::{self, Generation, JobError, JobHandle, JobResult, JobToken, ResourceJobs},
    world::{CellReport, SourceField, dependencies::Decoded},
};
use serde::{Serialize, Serializer};
use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const POLL_WORK: usize = 8;

impl Serialize for CellModelPlan {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.receipt().serialize(serializer)
    }
}

#[derive(Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    plan: CellModelPlan,
    pub generation: u64,
    pub all_requested_model_sources_ready: bool,
    pub runtime_ready: bool,
}
impl Receipt {
    pub fn plan(&self) -> &super::PlanReceipt {
        self.plan.receipt()
    }
}

/// A completed diagnostic batch still belongs to its live preparation owner.
/// Replacement, cancellation or owner drop revokes publication, including an
/// already extracted Ready. No probes are exposed before the aggregate gate.
pub struct Ready {
    plan: CellModelPlan,
    token: JobToken,
    probes: Vec<ModelProbe>,
}
impl Ready {
    pub fn publish_into(self, report: &mut CellReport) -> JobResult<Receipt> {
        self.token.commit(|| {
            validate_sink(&self.plan, report)?;
            report.model_probes = self.probes;
            Ok(Receipt {
                schema_version: 1,
                plan: self.plan,
                generation: self.token.generation(),
                all_requested_model_sources_ready: true,
                runtime_ready: false,
            })
        })
    }
}

pub struct CellPreparation {
    plan: CellModelPlan,
    generation: Generation,
    token: JobToken,
    jobs: ResourceJobs,
    job_limits: resource_jobs::Limits,
    cache: Option<(PathBuf, PathBuf)>,
    pending: Vec<(usize, JobHandle)>,
    staged: Vec<Option<ModelProbe>>,
    next: usize,
    failure: Option<String>,
    taken: bool,
    #[cfg(test)]
    pause: Option<std::sync::Arc<resource_jobs::tests::Pause>>,
}
impl CellPreparation {
    pub fn new(
        plan: CellModelPlan,
        source_tree: &Path,
        cache_root: Option<&Path>,
        job_limits: resource_jobs::Limits,
    ) -> JobResult<Self> {
        // A poll has a fixed work ceiling, independent of a caller's queue size.
        if job_limits.outstanding > POLL_WORK {
            return Err(JobError::Invalid(
                "cell preparation outstanding ceiling is eight".into(),
            ));
        }
        validate_admission(&plan, job_limits)?;
        let cache = cache_root
            .map(|root| {
                cache::validate_root(root, source_tree)
                    .map(|root| (root, source_tree.to_path_buf()))
            })
            .transpose()?;
        let generation = Generation::new(plan.identity().to_owned())?;
        let token = generation.token()?;
        let jobs = ResourceJobs::new(job_limits, generation.clone())?;
        let staged = (0..plan.0.requests.len()).map(|_| None).collect();
        Ok(Self {
            plan,
            generation,
            token,
            jobs,
            job_limits,
            cache,
            pending: Vec::new(),
            staged,
            next: 0,
            failure: None,
            taken: false,
            #[cfg(test)]
            pause: None,
        })
    }

    pub fn plan(&self) -> &CellModelPlan {
        &self.plan
    }
    pub fn generation(&self) -> u64 {
        self.token.generation()
    }
    pub fn usage(&self) -> resource_jobs::Usage {
        self.jobs.usage()
    }

    /// Consume at most eight completions and submit at most eight requests.
    /// One existing BSA extraction/NIF inspection is opaque within its admitted
    /// size; epoch checks before and after inspection prevent its publication.
    pub fn poll(&mut self) -> JobResult<bool> {
        self.token.check()?;
        if let Some(error) = &self.failure {
            return Err(JobError::Invalid(format!(
                "cell preparation failed; retry required: {error}"
            )));
        }
        if self.taken {
            return Err(JobError::Invalid("cell Ready was already taken".into()));
        }
        let result = self.poll_inner();
        if let Err(error) = &result {
            self.failure = Some(error.to_string());
            self.clear_work();
        }
        result
    }

    fn poll_inner(&mut self) -> JobResult<bool> {
        let mut index = 0;
        let mut inspected = 0;
        while index < self.pending.len() && inspected < POLL_WORK {
            let Some(mut artifact) = self.pending[index].1.try_take()? else {
                index += 1;
                continue;
            };
            let (request, handle) = self.pending.swap_remove(index);
            let selected = &self.plan.0.requests[request].receipt;
            let mut probe = ModelProbe::empty(selected.path.clone(), selected.source.clone());
            model_probe::inspect_artifact(&mut probe, &mut artifact)?;
            self.token.check()?;
            self.staged[request] = Some(probe);
            drop(artifact);
            drop(handle);
            inspected += 1;
        }
        for _ in 0..POLL_WORK {
            if self.next == self.plan.0.requests.len() {
                break;
            }
            let request = &self.plan.0.requests[self.next];
            let member = request.member()?;
            let token = self.generation.token()?;
            #[cfg(test)]
            let submitted = if let Some(pause) = &self.pause {
                self.jobs
                    .submit_paused(member, token, self.cache.clone(), pause.clone())
            } else {
                self.jobs.submit(member, token, self.cache.clone())
            };
            #[cfg(not(test))]
            let submitted = self.jobs.submit(member, token, self.cache.clone());
            match submitted {
                Ok(handle) => {
                    self.pending.push((self.next, handle));
                    self.next += 1;
                }
                Err(JobError::QueueFull | JobError::ByteBudget) => break,
                Err(error) => return Err(error),
            }
        }
        self.token.check()?;
        Ok(self.next == self.staged.len() && self.pending.is_empty())
    }

    pub fn take_ready(&mut self) -> JobResult<Option<Ready>> {
        self.token.check()?;
        if self.failure.is_some() || self.taken {
            return Err(JobError::Invalid(
                "cell batch failed or Ready already taken".into(),
            ));
        }
        if self.next != self.staged.len() || !self.pending.is_empty() {
            return Ok(None);
        }
        if self.staged.iter().any(Option::is_none) {
            return Ok(None);
        }
        self.token.commit(|| {
            self.taken = true;
            let probes = std::mem::take(&mut self.staged)
                .into_iter()
                .map(|probe| probe.expect("complete source batch"))
                .collect();
            Ok(Some(Ready {
                plan: self.plan.clone(),
                token: self.token.clone(),
                probes,
            }))
        })
    }

    pub fn wait(&mut self) -> JobResult<Ready> {
        loop {
            if self.poll()? {
                return self
                    .take_ready()?
                    .ok_or_else(|| JobError::Invalid("completed cell batch missing Ready".into()));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Advance even for identical bytes: residency/retry is a new owner epoch.
    pub fn replace(&mut self, plan: CellModelPlan) -> JobResult<()> {
        validate_admission(&plan, self.job_limits)?;
        self.generation.advance(plan.identity().to_owned())?;
        self.clear_work();
        self.token = self.generation.token()?;
        self.staged = (0..plan.0.requests.len()).map(|_| None).collect();
        self.plan = plan;
        self.next = 0;
        self.failure = None;
        self.taken = false;
        Ok(())
    }
    pub fn retry(&mut self) -> JobResult<()> {
        self.replace(self.plan.clone())
    }
    pub fn cancel(&mut self) {
        self.token.cancel();
        self.clear_work();
    }
    fn clear_work(&mut self) {
        for (_, handle) in self.pending.drain(..) {
            handle.cancel();
        }
        self.staged.clear();
    }
}
impl Drop for CellPreparation {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn validate_admission(plan: &CellModelPlan, limits: resource_jobs::Limits) -> JobResult<()> {
    if plan
        .0
        .requests
        .iter()
        .any(|request| request.receipt.decoded_bytes > limits.decoded_bytes)
    {
        return Err(JobError::ByteBudget);
    }
    Ok(())
}

fn same_field<T: PartialEq>(left: &Option<SourceField<T>>, right: &Option<SourceField<T>>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.decoded_offset == right.decoded_offset && left.value == right.value
        }
        _ => false,
    }
}
fn validate_sink(plan: &CellModelPlan, report: &CellReport) -> JobResult<()> {
    let root = plan
        .graph()
        .nodes
        .iter()
        .find(|node| &node.key == plan.root())
        .expect("sealed CELL root");
    let Some(Decoded::Cell(cell)) = &root.fields else {
        return Err(JobError::Invalid(
            "sealed plan lacks decoded CELL root".into(),
        ));
    };
    if &report.key != plan.root()
        || report.source_plugin != root.source_plugin
        || report.record_offset != root.header.offset
        || report.integrity_failures != 0
        || report.runtime_ready
        || report.cell.flags.decoded_offset != cell.flags.decoded_offset
        || report.cell.flags.value != cell.flags.value
        || !same_field(&report.cell.grid, &cell.grid)
        || !same_field(&report.cell.full_name, &cell.full_name)
        || report.models.len() != plan.receipt().coverage.len()
    {
        return Err(JobError::Invalid(
            "cell publication root/source binding differs from sealed plan".into(),
        ));
    }
    for expected in &plan.receipt().coverage {
        let Some(model) = report
            .models
            .iter()
            .find(|model| model.base_key == expected.base_key)
        else {
            return Err(JobError::Invalid(
                "cell publication base binding differs from sealed plan".into(),
            ));
        };
        if model.source_plugin != expected.source_plugin
            || model.record_offset != expected.header.offset
            || model.base_kind != crate::plugin::signature(expected.header.kind)
            || !same_field(&model.model_field, &expected.model_field)
            || model.asset_path != expected.asset_path
            || model.candidates.len() != expected.candidates.len()
            || model
                .candidates
                .iter()
                .zip(&expected.candidates)
                .any(|(left, right)| {
                    left.container != right.container
                        || left.entry_index != right.entry_index
                        || left.original_path != right.original_path
                })
        {
            return Err(JobError::Invalid(
                "cell publication MODL/archive binding differs from sealed plan".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
