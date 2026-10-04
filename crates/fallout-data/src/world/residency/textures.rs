//! Texture source closure over leased models and the existing NIF/BSA readers.
use super::*;
use crate::{
    baseline, nif_scene,
    resource_jobs::{ArchiveInput, Member},
    vfs::{AssetPath, AssetSource, MountIndex},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

#[derive(Clone, Copy, Debug)]
pub struct TextureLimits {
    pub references: usize,
    pub requests: usize,
    pub archives: usize,
    pub metadata_bytes: usize,
    pub nif_blocks: usize,
    pub nif_array_bytes: usize,
}
impl Default for TextureLimits {
    fn default() -> Self {
        Self {
            references: 8192,
            requests: 1024,
            archives: 8,
            metadata_bytes: 8 * 1024 * 1024,
            nif_blocks: 100_000,
            nif_array_bytes: 128 * 1024 * 1024,
        }
    }
}
impl TextureLimits {
    fn validate(self) -> JobResult<Self> {
        let ceiling = Self::default();
        if self.references > ceiling.references
            || self.requests > ceiling.requests
            || self.archives > ceiling.archives
            || self.metadata_bytes > ceiling.metadata_bytes
            || self.nif_blocks > ceiling.nif_blocks
            || self.nif_array_bytes > ceiling.nif_array_bytes
        {
            return Err(JobError::Invalid(
                "texture plan limits exceed ceiling".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Serialize)]
pub struct TextureUsage {
    pub model: usize,
    pub block: u32,
    pub slot: usize,
    pub raw_path: Vec<u8>,
    pub path: Option<AssetPath>,
    pub error: Option<String>,
    pub candidates: Vec<AssetSource>,
}
#[derive(Debug, Serialize)]
pub struct TextureModel {
    pub model: usize,
    pub sha256: String,
    pub unsupported_blocks: BTreeMap<String, Vec<u32>>,
    pub unsupported_scene_edges: Vec<nif_scene::UnsupportedEdge>,
}
#[derive(Debug, Serialize)]
pub struct TextureReceipt {
    pub model_identity: String,
    pub generation: u64,
    pub identity: String,
    pub models: Vec<TextureModel>,
    pub usages: Vec<TextureUsage>,
    pub requests: Vec<super::super::preparation::RequestReceipt>,
    pub archives: Vec<super::super::preparation::ArchiveReceipt>,
    pub missing_or_ambiguous: usize,
    pub reference_coverage_verified: bool,
    pub decoded_bytes: usize,
    pub metadata_bytes: usize,
    pub mapped_bytes: u64,
}

/// A sidecar quota pin charges mappings/metadata while planning, before mapping
/// or retaining them. Model leases keep the corresponding cell plan pin alive.
struct TexturePin {
    usage: Arc<Mutex<PlanUsage>>,
    ticket: Ticket,
    limits: Limits,
    maximum_metadata: usize,
    metadata: usize,
    mapped: u64,
}
impl TexturePin {
    fn charge(&mut self, metadata: usize, mapped: u64) -> JobResult<()> {
        self.ticket.token.commit(|| {
            let mut usage = self.usage.lock().map_err(|_| JobError::Closed)?;
            if metadata > self.maximum_metadata.saturating_sub(self.metadata)
                || metadata
                    > self
                        .limits
                        .plan_metadata_bytes
                        .saturating_sub(usage.metadata)
                || mapped > self.limits.mapped_source_bytes.saturating_sub(usage.mapped)
            {
                return Err(JobError::ByteBudget);
            }
            usage.metadata += metadata;
            usage.mapped += mapped;
            Ok(())
        })?;
        self.metadata += metadata;
        self.mapped += mapped;
        Ok(())
    }
}
impl Drop for TexturePin {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.usage.lock() {
            usage.metadata -= self.metadata;
            usage.mapped -= self.mapped;
        }
    }
}

struct PlannedTexture {
    input: Arc<ArchiveInput>,
    source: AssetSource,
    path: AssetPath,
}

// Stream bounded metadata to its seal rather than allocating a second JSON
// copy. Unknown block names and repeated unsupported edges are counted too.
struct JsonWriter {
    digest: Sha256,
    written: usize,
    maximum: usize,
    exceeded: bool,
}
impl JsonWriter {
    fn new(maximum: usize) -> Self {
        Self {
            digest: Sha256::new(),
            written: 0,
            maximum,
            exceeded: false,
        }
    }
    fn serialize(&mut self, value: &impl Serialize) -> JobResult<()> {
        serde_json::to_writer(&mut *self, value).map_err(|error| {
            if self.exceeded {
                JobError::ByteBudget
            } else {
                JobError::Invalid(error.to_string())
            }
        })
    }
}
impl Write for JsonWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.written) {
            self.exceeded = true;
            return Err(std::io::Error::other("texture metadata budget exceeded"));
        }
        self.written += bytes.len();
        self.digest.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct Inner {
    models: Arc<ResidentSources>,
    requests: Vec<PlannedTexture>,
    receipt: TextureReceipt,
    _pin: TexturePin,
}

/// Clones retain both the model source lease and the texture quota pin. No
/// resource-bearing archive input/member is exposed to callers.
#[derive(Clone)]
pub struct TexturePlan(Arc<Inner>);
impl TexturePlan {
    pub fn receipt(&self) -> &TextureReceipt {
        &self.0.receipt
    }
    fn member(&self, index: usize) -> JobResult<Member> {
        self.0.models.ticket.check()?;
        let request = self
            .0
            .requests
            .get(index)
            .ok_or_else(|| JobError::Invalid("texture request index outside plan".into()))?;
        request.input.member(&request.path, &request.source)
    }

    /// Existing bounded scene/material decoding and archive fingerprinting can
    /// block; run this on the host's source worker, retaining its model Arc.
    pub fn load(
        models: Arc<ResidentSources>,
        mounts: &MountIndex,
        limits: TextureLimits,
    ) -> JobResult<Self> {
        let limits = limits.validate()?;
        models.ticket.check()?;
        let mut pin = TexturePin {
            usage: models._plan_pin.usage.clone(),
            ticket: models.ticket.clone(),
            limits: models.limits,
            maximum_metadata: limits.metadata_bytes,
            metadata: 0,
            mapped: 0,
        };
        pin.charge(1024, 0)?;
        let mut model_rows = Vec::new();
        let mut usages = Vec::new();
        let mut selected = BTreeMap::new();
        let mut missing = 0;
        let mut reference_coverage_verified = true;
        for model in 0..models.models.len() {
            models.ticket.check()?;
            let bytes = models.model(model)?;
            let (_, scene) = nif_scene::decode_with_limits(
                bytes,
                "resident-model-textures",
                nif_scene::Limits {
                    input_bytes: 64 * 1024 * 1024,
                    blocks: limits.nif_blocks,
                    array_bytes: limits.nif_array_bytes,
                },
            )?;
            reference_coverage_verified &=
                scene.unsupported_blocks.is_empty() && scene.unsupported_scene_edges.is_empty();
            // Charge retained unknown metadata before moving it into the plan.
            // Existing decoder limits separately bound its temporary arrays.
            let mut count = JsonWriter::new(limits.metadata_bytes.saturating_sub(pin.metadata));
            count.serialize(&(&scene.unsupported_blocks, &scene.unsupported_scene_edges))?;
            pin.charge(
                count
                    .written
                    .checked_mul(2)
                    .and_then(|n| n.checked_add(1024))
                    .ok_or(JobError::ByteBudget)?,
                0,
            )?;
            model_rows.push(TextureModel {
                model,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                unsupported_blocks: scene.unsupported_blocks,
                unsupported_scene_edges: scene.unsupported_scene_edges,
            });
            for texture in scene.textures {
                if usages.len() >= limits.references {
                    return Err(JobError::QueueFull);
                }
                let candidates = texture
                    .asset_path
                    .as_ref()
                    .map(|path| mounts.candidates(path.bytes()))
                    .transpose()?
                    .unwrap_or(&[]);
                let candidate_bytes = candidates.iter().try_fold(0usize, |sum, candidate| {
                    sum.checked_add(
                        512 + 8 * candidate.container.len() + 8 * candidate.original_path.len(),
                    )
                    .ok_or(JobError::ByteBudget)
                })?;
                pin.charge(1024 + 16 * texture.raw_path.len() + candidate_bytes, 0)?;
                if let (Some(path), [source]) = (&texture.asset_path, candidates) {
                    if !selected.contains_key(path) {
                        if selected.len() >= limits.requests
                            || models.models.len() + selected.len() >= models.limits.resources
                        {
                            return Err(JobError::QueueFull);
                        }
                        selected.insert(path.clone(), source.clone());
                    }
                } else {
                    missing += 1;
                }
                usages.push(TextureUsage {
                    model,
                    block: texture.block,
                    slot: texture.slot,
                    raw_path: texture.raw_path,
                    path: texture.asset_path,
                    error: texture.error,
                    candidates: candidates.to_vec(),
                });
            }
        }
        let mut inputs = BTreeMap::<String, Arc<ArchiveInput>>::new();
        let mut requests = Vec::new();
        let mut request_rows = Vec::new();
        let mut decoded_bytes = 0usize;
        for (path, source) in selected {
            models.ticket.check()?;
            if !inputs.contains_key(&source.container) {
                if inputs.len() >= limits.archives {
                    return Err(JobError::QueueFull);
                }
                // Protect the same source before observing its length and hold
                // it until ArchiveInput has opened its own protected mapping.
                let protected = baseline::open_source(Path::new(&source.container))?;
                let bytes = protected
                    .metadata()
                    .map_err(|error| crate::io(&source.container, error))?
                    .len();
                pin.charge(1024 + 8 * source.container.len(), bytes)?;
                let input = ArchiveInput::open(Path::new(&source.container))?;
                if input.source_bytes() != bytes {
                    return Err(JobError::Invalid("texture archive length changed".into()));
                }
                inputs.insert(source.container.clone(), input);
            }
            let input = inputs[&source.container].clone();
            let member = input.member(&path, &source)?;
            decoded_bytes = decoded_bytes
                .checked_add(member.bytes)
                .ok_or(JobError::ByteBudget)?;
            if decoded_bytes
                > models
                    .limits
                    .source_bytes
                    .saturating_sub(models.plan.receipt().usage.model_decoded_bytes)
            {
                return Err(JobError::ByteBudget);
            }
            pin.charge(
                1024 + 8 * path.bytes().len() + 8 * source.container.len(),
                0,
            )?;
            request_rows.push(super::super::preparation::RequestReceipt {
                path: path.clone(),
                source: source.clone(),
                archive_sha256: input.source_sha256().into(),
                decoded_bytes: member.bytes,
            });
            requests.push(PlannedTexture {
                input,
                source,
                path,
            });
        }
        let archives = inputs
            .into_iter()
            .map(
                |(container, input)| super::super::preparation::ArchiveReceipt {
                    container,
                    source_bytes: input.source_bytes(),
                    source_sha256: input.source_sha256().into(),
                },
            )
            .collect();
        let mut receipt = TextureReceipt {
            model_identity: models.ticket.identity().into(),
            generation: models.ticket.generation(),
            identity: String::new(),
            models: model_rows,
            usages,
            requests: request_rows,
            archives,
            missing_or_ambiguous: missing,
            reference_coverage_verified,
            decoded_bytes,
            metadata_bytes: pin.metadata,
            mapped_bytes: pin.mapped,
        };
        let mut writer = JsonWriter::new(pin.metadata);
        writer.digest.update(b"nv-resident-texture-plan-v1\0");
        writer.serialize(&receipt)?;
        receipt.identity = format!("{:x}", writer.digest.finalize());
        models.ticket.check()?;
        Ok(Self(Arc::new(Inner {
            models,
            requests,
            receipt,
            _pin: pin,
        })))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TextureState {
    Unrequested,
    IoPending,
    Decoded,
    Unsupported,
}

pub struct ResidentTextures {
    plan: TexturePlan,
    textures: Vec<Artifact>,
}
impl ResidentTextures {
    pub fn ticket(&self) -> &Ticket {
        &self.plan.0.models.ticket
    }
    pub fn receipt(&self) -> JobResult<&TextureReceipt> {
        self.ticket().check()?;
        Ok(self.plan.receipt())
    }
    pub fn texture(&self, index: usize) -> JobResult<&[u8]> {
        self.ticket().check()?;
        self.textures
            .get(index)
            .map(Artifact::bytes)
            .ok_or_else(|| JobError::Invalid("resident texture index outside batch".into()))
    }
}
pub(super) struct Batch {
    pub plan: TexturePlan,
    pending: Vec<(usize, JobHandle)>,
    staged: Vec<Option<Artifact>>,
    sources: Option<Arc<ResidentTextures>>,
    next: usize,
}
impl Batch {
    fn new(plan: TexturePlan) -> Self {
        let staged = (0..plan.0.requests.len()).map(|_| None).collect();
        let mut batch = Self {
            plan,
            pending: Vec::new(),
            staged,
            sources: None,
            next: 0,
        };
        batch.finish();
        batch
    }
    fn finish(&mut self) {
        if self.next == self.staged.len() && self.pending.is_empty() && self.sources.is_none() {
            self.sources = Some(Arc::new(ResidentTextures {
                plan: self.plan.clone(),
                textures: std::mem::take(&mut self.staged)
                    .into_iter()
                    .map(|value| value.expect("complete texture batch"))
                    .collect(),
            }));
        }
    }
    pub fn pending(&self) -> bool {
        self.sources.is_none()
    }
    pub fn ready(&self) -> bool {
        self.sources.is_some() && self.plan.receipt().missing_or_ambiguous == 0
    }
    pub fn requested(&self) -> usize {
        self.plan.0.requests.len()
    }
    pub fn completed(&self) -> usize {
        self.sources.as_ref().map_or_else(
            || self.staged.iter().flatten().count(),
            |sources| sources.textures.len(),
        )
    }
    pub fn state(&self) -> TextureState {
        if self.pending() {
            TextureState::IoPending
        } else if self.ready() {
            TextureState::Decoded
        } else {
            TextureState::Unsupported
        }
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        for (_, handle) in &self.pending {
            handle.cancel();
        }
    }
}
impl CellResidency {
    pub fn request_textures(&mut self, ticket: &Ticket, plan: TexturePlan) -> JobResult<()> {
        self.validate(ticket)?;
        if !Arc::ptr_eq(
            &plan.0.models,
            self.sources.as_ref().expect("decoded model lease"),
        ) {
            return Err(JobError::Invalid(
                "texture plan belongs to another source lease".into(),
            ));
        }
        if self.textures.is_some() || self.render_published {
            return Err(JobError::Invalid(
                "texture admission already used for this generation".into(),
            ));
        }
        self.validate_payloads(plan.receipt().requests.len(), plan.receipt().decoded_bytes)?;
        self.textures = Some(Batch::new(plan));
        self.dependencies = Readiness::Pending;
        self.stage = Stage::Decoded;
        Ok(())
    }
    pub fn texture_sources(&self, ticket: &Ticket) -> JobResult<Arc<ResidentTextures>> {
        self.validate(ticket)?;
        self.textures
            .as_ref()
            .and_then(|batch| batch.sources.as_ref())
            .cloned()
            .ok_or_else(|| JobError::Invalid("texture sources are not decoded".into()))
    }
    pub(super) fn poll_textures(&mut self) -> JobResult<()> {
        self.ticket
            .as_ref()
            .expect("active texture ticket")
            .check()?;
        let batch = self.textures.as_mut().expect("pending texture batch");
        let mut index = 0;
        let mut inspected = 0;
        while index < batch.pending.len() && inspected < POLL_WORK {
            inspected += 1;
            if let Some(artifact) = batch.pending[index].1.try_take()? {
                let (request, _) = batch.pending.swap_remove(index);
                batch.staged[request] = Some(artifact);
            } else {
                index += 1;
            }
        }
        self.backpressured = false;
        for _ in 0..POLL_WORK {
            if batch.next == batch.staged.len() {
                break;
            }
            let member = batch.plan.member(batch.next)?;
            let token = self.generation.token()?;
            match self.jobs.submit_scoped(
                member,
                token,
                self.cache.clone(),
                batch.plan.0.clone(),
                #[cfg(test)]
                self.pause.clone(),
            ) {
                Ok(handle) => {
                    batch.pending.push((batch.next, handle));
                    batch.next += 1;
                }
                Err(JobError::QueueFull | JobError::ByteBudget) => {
                    self.backpressured = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        batch.finish();
        Ok(())
    }
}
