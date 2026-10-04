use super::{TextureSourcePlan, plan::binding};
use crate::{
    cache::{self, CacheResult},
    resource_jobs::{self, Generation, JobError, JobHandle, JobResult, JobToken, ResourceJobs},
    terrain::TerrainReport,
    vfs::{AssetPath, AssetSource},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const POLL_WORK: usize = 8;

#[derive(Debug, Serialize)]
pub struct TextureReceipt {
    pub asset_index: usize,
    pub path: AssetPath,
    pub source: AssetSource,
    pub archive_sha256: String,
    pub decoded_bytes: usize,
    pub sha256: String,
    pub cache: Option<CacheResult>,
}
#[derive(Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    plan: TextureSourcePlan,
    pub generation: u64,
    pub textures: Vec<TextureReceipt>,
    pub all_requested_texture_sources_ready: bool,
    pub all_authored_texture_sources_resolved: bool,
    pub runtime_ready: bool,
}
impl Receipt {
    pub fn plan(&self) -> &super::PlanReceipt {
        self.plan.receipt()
    }
}

/// Selected completion metadata remains private until the entire current batch
/// is ready. Owner replacement/cancel/drop also revokes an already taken Ready.
pub struct Ready {
    plan: TextureSourcePlan,
    token: JobToken,
    textures: Vec<TextureReceipt>,
}
impl Ready {
    pub fn publish_for(self, terrain: &TerrainReport) -> JobResult<Receipt> {
        // Hash a bounded source sink before entering the short epoch commit gate.
        let actual = binding(terrain)?;
        self.token.commit(|| {
            if actual != self.plan.0.terrain_binding {
                return Err(JobError::Invalid(
                    "terrain publication source binding differs from sealed plan".into(),
                ));
            }
            let sources = &self.plan.receipt().texture_sources;
            let resolved = sources.failures == 0
                && sources.unapplied_default_layers == 0
                && terrain.link_failures == 0;
            Ok(Receipt {
                schema_version: 1,
                plan: self.plan,
                generation: self.token.generation(),
                textures: self.textures,
                all_requested_texture_sources_ready: true,
                all_authored_texture_sources_resolved: resolved,
                runtime_ready: false,
            })
        })
    }
}

pub struct TexturePreparation {
    plan: TextureSourcePlan,
    generation: Generation,
    token: JobToken,
    jobs: ResourceJobs,
    limits: resource_jobs::Limits,
    cache: Option<(PathBuf, PathBuf)>,
    pending: Vec<(usize, JobHandle)>,
    staged: Vec<Option<TextureReceipt>>,
    next: usize,
    failure: Option<String>,
    taken: bool,
    #[cfg(test)]
    pause: Option<std::sync::Arc<resource_jobs::tests::Pause>>,
}
impl TexturePreparation {
    pub fn new(
        plan: TextureSourcePlan,
        source_tree: &Path,
        cache_root: Option<&Path>,
        limits: resource_jobs::Limits,
    ) -> JobResult<Self> {
        validate_admission(&plan, limits)?;
        let cache = cache_root
            .map(|root| {
                cache::validate_root(root, source_tree)
                    .map(|root| (root, source_tree.to_path_buf()))
            })
            .transpose()?;
        let generation = Generation::new(plan.identity().to_owned())?;
        let token = generation.token()?;
        let jobs = ResourceJobs::new(limits, generation.clone())?;
        let staged = (0..plan.0.requests.len()).map(|_| None).collect();
        Ok(Self {
            plan,
            generation,
            token,
            jobs,
            limits,
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
    pub fn plan(&self) -> &TextureSourcePlan {
        &self.plan
    }
    pub fn generation(&self) -> u64 {
        self.token.generation()
    }
    pub fn usage(&self) -> resource_jobs::Usage {
        self.jobs.usage()
    }

    /// At most eight completions and eight admissions; BSA extraction retains
    /// the existing opaque member decode and before/after token checks.
    pub fn poll(&mut self) -> JobResult<bool> {
        self.token.check()?;
        if self.failure.is_some() || self.taken {
            return Err(JobError::Invalid(
                "terrain batch failed or Ready already taken; retry required".into(),
            ));
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
        let mut completed = 0;
        while index < self.pending.len() && completed < POLL_WORK {
            let Some(mut artifact) = self.pending[index].1.try_take()? else {
                index += 1;
                continue;
            };
            let (selected, handle) = self.pending.swap_remove(index);
            let request = &self.plan.receipt().requests[selected];
            let sha256 = format!("{:x}", Sha256::digest(artifact.bytes()));
            self.token.check()?;
            self.staged[selected] = Some(TextureReceipt {
                asset_index: self.plan.0.requests[selected].asset_index,
                path: request.path.clone(),
                source: request.source.clone(),
                archive_sha256: request.archive_sha256.clone(),
                decoded_bytes: artifact.bytes().len(),
                sha256,
                cache: artifact.take_cache_receipt(),
            });
            // Drop payload while its source/reservation pin is still attached.
            drop(artifact);
            drop(handle);
            completed += 1;
        }
        for _ in 0..POLL_WORK {
            if self.next == self.plan.0.requests.len() {
                break;
            }
            let member = self.plan.member(self.next)?;
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
                "terrain batch failed or Ready already taken".into(),
            ));
        }
        if self.next != self.staged.len()
            || !self.pending.is_empty()
            || self.staged.iter().any(Option::is_none)
        {
            return Ok(None);
        }
        self.token.commit(|| {
            self.taken = true;
            let textures = std::mem::take(&mut self.staged)
                .into_iter()
                .map(|value| value.expect("complete selected texture batch"))
                .collect();
            Ok(Some(Ready {
                plan: self.plan.clone(),
                token: self.token.clone(),
                textures,
            }))
        })
    }
    pub fn wait(&mut self) -> JobResult<Ready> {
        loop {
            if self.poll()? {
                return self.take_ready()?.ok_or_else(|| {
                    JobError::Invalid("completed terrain batch lacks Ready".into())
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn replace(&mut self, plan: TextureSourcePlan) -> JobResult<()> {
        validate_admission(&plan, self.limits)?;
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
impl Drop for TexturePreparation {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn validate_admission(plan: &TextureSourcePlan, limits: resource_jobs::Limits) -> JobResult<()> {
    if limits.outstanding > POLL_WORK {
        return Err(JobError::Invalid(
            "terrain preparation outstanding ceiling is eight".into(),
        ));
    }
    if plan
        .receipt()
        .requests
        .iter()
        .any(|request| request.decoded_bytes > limits.decoded_bytes)
    {
        return Err(JobError::ByteBudget);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
