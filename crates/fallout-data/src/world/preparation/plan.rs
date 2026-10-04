use super::super::{SourceField, dependencies, model_path};
use crate::{
    Error, Result, baseline,
    identity::FormKey,
    plugin::{self, RecordHeader},
    resource_jobs::{ArchiveInput, Member},
    store::RecordStore,
    vfs::{AssetPath, AssetSource, MountIndex},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::Path,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub dependencies: dependencies::Limits,
    pub max_bases: usize,
    pub max_requests: usize,
    pub max_candidates: usize,
    pub max_archives: usize,
    pub max_record_bytes: usize,
    pub max_record_decoded_bytes: usize,
    pub max_field_sites: usize,
    pub max_path_bytes: usize,
    pub max_model_decoded_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_probe_metadata_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            dependencies: Default::default(),
            max_bases: 1024,
            max_requests: 1024,
            max_candidates: 8192,
            max_archives: 8,
            max_record_bytes: 4 * 1024 * 1024,
            max_record_decoded_bytes: 64 * 1024 * 1024,
            max_field_sites: 262144,
            max_path_bytes: 4096,
            max_model_decoded_bytes: 64 * 1024 * 1024,
            max_metadata_bytes: 16 * 1024 * 1024,
            max_probe_metadata_bytes: 512 * 1024 * 1024,
        }
    }
}

impl Limits {
    pub(super) fn validate(self) -> Result<Self> {
        self.dependencies.validate()?;
        let ceiling = Self::default();
        for (value, maximum, name) in [
            (self.max_bases, ceiling.max_bases, "bases"),
            (self.max_requests, ceiling.max_requests, "requests"),
            (self.max_candidates, ceiling.max_candidates, "candidates"),
            (self.max_archives, ceiling.max_archives, "archives"),
            (
                self.max_record_bytes,
                ceiling.max_record_bytes,
                "record bytes",
            ),
            (
                self.max_record_decoded_bytes,
                ceiling.max_record_decoded_bytes,
                "record decoded bytes",
            ),
            (self.max_field_sites, ceiling.max_field_sites, "field sites"),
            (self.max_path_bytes, ceiling.max_path_bytes, "path bytes"),
            (
                self.max_model_decoded_bytes,
                ceiling.max_model_decoded_bytes,
                "model decoded bytes",
            ),
            (
                self.max_metadata_bytes,
                ceiling.max_metadata_bytes,
                "metadata bytes",
            ),
            (
                self.max_probe_metadata_bytes,
                ceiling.max_probe_metadata_bytes,
                "probe metadata bytes",
            ),
        ] {
            if value > maximum {
                return Err(budget(name, "ceiling exceeded"));
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Serialize)]
pub struct ModelCoverage {
    pub base_key: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: RecordHeader,
    pub decoded_sha256: String,
    pub model_field: Option<SourceField<Vec<u8>>>,
    pub asset_path: Option<AssetPath>,
    pub candidates: Vec<AssetSource>,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ArchiveReceipt {
    pub container: String,
    pub source_bytes: u64,
    pub source_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct RequestReceipt {
    pub path: AssetPath,
    pub source: AssetSource,
    pub archive_sha256: String,
    pub decoded_bytes: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub bases: usize,
    pub candidates: usize,
    pub archives: usize,
    pub record_decoded_bytes: usize,
    /// Actual aggregate max(stored, decoded) strict plugin body reads.
    pub source_read_bytes: usize,
    pub field_sites: usize,
    pub model_decoded_bytes: usize,
    pub metadata_bytes: usize,
    pub probe_metadata_bytes: usize,
}

#[derive(Debug, Serialize)]
pub struct PlanReceipt {
    pub schema_version: u32,
    pub root: FormKey,
    pub identity: String,
    pub source_cohort_sha256: String,
    pub coverage: Vec<ModelCoverage>,
    pub archives: Vec<ArchiveReceipt>,
    pub requests: Vec<RequestReceipt>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}

pub(super) struct Planned {
    pub receipt: RequestReceipt,
    pub input: Arc<ArchiveInput>,
}

impl Planned {
    pub fn member(&self) -> Result<Member> {
        self.input
            .member(&self.receipt.path, &self.receipt.source)
            .map_err(|error| Error::Resolution(error.to_string()))
    }
}

pub(super) struct Inner {
    pub graph: dependencies::Report,
    pub receipt: PlanReceipt,
    pub requests: Vec<Planned>,
}

/// Construction derives every selection from protected source records. Public
/// report JSON and mutable inspector structures cannot manufacture a sealed plan.
#[derive(Clone)]
pub struct CellModelPlan(pub(super) Arc<Inner>);

/// Only existing protected ArchiveInput instances are reused. A container lookup
/// retains its exact immutable fingerprint/extent and still checks every member
/// against that input's physical container/table; aliases are not guessed equal.
pub(in crate::world) struct ArchivePool {
    inputs: BTreeMap<String, Arc<ArchiveInput>>,
    pub mapped_bytes: u64,
    max_mapped_bytes: u64,
    max_archives: usize,
}
impl ArchivePool {
    pub fn new(max_mapped_bytes: u64, max_archives: usize) -> Self {
        Self {
            inputs: BTreeMap::new(),
            mapped_bytes: 0,
            max_mapped_bytes,
            max_archives,
        }
    }
    pub fn len(&self) -> usize {
        self.inputs.len()
    }
    fn get(&mut self, container: &str) -> Result<Arc<ArchiveInput>> {
        if let Some(input) = self.inputs.get(container) {
            return Ok(input.clone());
        }
        if self.inputs.len() >= self.max_archives {
            return Err(budget("shared archives", "exceeded"));
        }
        // Hold the protected file from extent admission through importer mapping.
        let protected = baseline::open_source(Path::new(container))?;
        let bytes = protected
            .metadata()
            .map_err(|error| crate::io(container, error))?
            .len();
        let mapped = self
            .mapped_bytes
            .checked_add(bytes)
            .filter(|n| *n <= self.max_mapped_bytes)
            .ok_or_else(|| budget("mapped source bytes", "exceeded"))?;
        let input = ArchiveInput::open(Path::new(container))
            .map_err(|error| Error::Resolution(error.to_string()))?;
        if input.source_bytes() != bytes {
            return Err(budget("mapped source bytes", "source extent changed"));
        }
        self.inputs.insert(container.to_owned(), input.clone());
        self.mapped_bytes = mapped;
        Ok(input)
    }
}

impl CellModelPlan {
    pub fn graph(&self) -> &dependencies::Report {
        &self.0.graph
    }
    pub fn receipt(&self) -> &PlanReceipt {
        &self.0.receipt
    }
    pub fn root(&self) -> &FormKey {
        &self.0.receipt.root
    }
    pub fn identity(&self) -> &str {
        &self.0.receipt.identity
    }

    /// The residency consumer uses the same sealed selections and importer as
    /// diagnostic preparation; receipts alone cannot manufacture an input.
    pub(crate) fn member(&self, index: usize) -> Result<Member> {
        self.0
            .requests
            .get(index)
            .ok_or_else(|| {
                Error::Resolution("cell model request index is outside sealed plan".into())
            })?
            .member()
    }

    pub fn load(
        store: &mut RecordStore,
        root: &FormKey,
        mounts: &MountIndex,
        limits: Limits,
    ) -> Result<Self> {
        Self::load_with_archive_pool(
            store,
            root,
            mounts,
            limits,
            usize::MAX,
            &mut ArchivePool::new(u64::MAX, usize::MAX),
        )
    }

    pub(in crate::world) fn load_with_archive_pool(
        store: &mut RecordStore,
        root: &FormKey,
        mounts: &MountIndex,
        limits: Limits,
        max_read_bytes: usize,
        archives: &mut ArchivePool,
    ) -> Result<Self> {
        let limits = limits.validate()?;
        let mut graph_limits = limits.dependencies;
        graph_limits.max_record_bytes = graph_limits.max_record_bytes.min(limits.max_record_bytes);
        graph_limits.max_decoded_bytes = graph_limits
            .max_decoded_bytes
            .min(limits.max_record_decoded_bytes);
        graph_limits.max_metadata_bytes = graph_limits
            .max_metadata_bytes
            .min(limits.max_metadata_bytes);
        graph_limits.max_field_sites = graph_limits.max_field_sites.min(limits.max_field_sites);
        let graph =
            dependencies::inspect_cell_key_read_bounded(store, root, graph_limits, max_read_bytes)?;
        let source_read_bytes = graph
            .nodes
            .iter()
            .filter_map(|node| {
                node.decoded_body
                    .as_ref()
                    .map(|body| body.len().max(node.header.stored_size as usize))
            })
            .try_fold(0usize, |sum, bytes| {
                sum.checked_add(bytes)
                    .ok_or_else(|| budget("source read bytes", "overflow"))
            })?;
        let mut usage = Usage {
            record_decoded_bytes: graph.usage.decoded_bytes,
            source_read_bytes,
            field_sites: graph.usage.field_sites,
            metadata_bytes: graph.usage.metadata_bytes,
            ..Default::default()
        };
        // Borrow graph keys while selecting; each new retained key is admitted first.
        charge(
            &mut usage.metadata_bytes,
            graph
                .root_members
                .checked_mul(128)
                .ok_or_else(|| budget("metadata bytes", "overflow"))?,
            limits.max_metadata_bytes,
            "metadata bytes",
        )?;
        let members: BTreeSet<_> = graph
            .edges
            .iter()
            .filter(|edge| edge.role == "member" && edge.target.status == "resolved")
            .filter_map(|edge| edge.target.key.as_ref())
            .collect();
        let mut bases = BTreeSet::new();
        for edge in &graph.edges {
            if edge.role == "NAME"
                && edge.target.status == "resolved"
                && members.contains(&edge.owner)
            {
                let key = edge.target.key.as_ref().expect("resolved base identity");
                if !bases.contains(key) {
                    charge(&mut usage.bases, 1, limits.max_bases, "bases")?;
                    charge(
                        &mut usage.metadata_bytes,
                        128 + key.origin_plugin.len(),
                        limits.max_metadata_bytes,
                        "metadata bytes",
                    )?;
                    bases.insert(key.clone());
                }
            }
        }
        let mut coverage = Vec::with_capacity(bases.len());
        let mut selected = BTreeMap::new();
        for key in bases {
            let location = store.winner(&key).expect("source-derived resolved base");
            charge(
                &mut usage.metadata_bytes,
                1024 + key.origin_plugin.len() + store.source_name(location).len(),
                limits.max_metadata_bytes,
                "metadata bytes",
            )?;
            let maximum = limits
                .max_record_bytes
                .min(limits.max_record_decoded_bytes - usage.record_decoded_bytes)
                .min(max_read_bytes - usage.source_read_bytes);
            if maximum == 0
                && (max_read_bytes == usize::MAX || record_header_nonempty(store, location))
            {
                return Err(budget("record decoded bytes", "no allowance remains"));
            }
            let record = store.read_bounded(location, maximum)?;
            charge(
                &mut usage.source_read_bytes,
                record.payload.len().max(record.header.stored_size as usize),
                max_read_bytes,
                "source read bytes",
            )?;
            if record.integrity_issue.is_some() {
                return Err(Error::Resolution(
                    "cell model plan refuses tainted base body".into(),
                ));
            }
            charge(
                &mut usage.record_decoded_bytes,
                record.payload.len(),
                limits.max_record_decoded_bytes,
                "record decoded bytes",
            )?;
            plugin::visit_subrecords(&record, store.source_name(location), |sub| {
                charge(
                    &mut usage.field_sites,
                    1,
                    limits.max_field_sites,
                    "field sites",
                )?;
                if sub.kind == *b"MODL" {
                    let bytes = sub.data.len().saturating_sub(1) + b"meshes/".len();
                    if bytes > limits.max_path_bytes {
                        return Err(budget("path bytes", "MODL lookup exceeds allowance"));
                    }
                    // Typed field, normalization scratch, selection and receipt copies.
                    charge(
                        &mut usage.metadata_bytes,
                        8 * bytes,
                        limits.max_metadata_bytes,
                        "metadata bytes",
                    )?;
                }
                Ok(())
            })?;
            let model_field = model_path(&record, store.source_name(location))?;
            let asset_path = model_field
                .as_ref()
                .map(|field| {
                    let mut raw = b"meshes/".to_vec();
                    raw.extend(&field.value);
                    AssetPath::new(&raw)
                })
                .transpose()?;
            let candidates = asset_path
                .as_ref()
                .map(|path| mounts.candidates(path.bytes()))
                .transpose()?
                .unwrap_or(&[]);
            charge(
                &mut usage.candidates,
                candidates.len(),
                limits.max_candidates,
                "candidates",
            )?;
            for source in candidates {
                if source.original_path.len() > limits.max_path_bytes {
                    return Err(budget("path bytes", "candidate path exceeded"));
                }
                charge(
                    &mut usage.metadata_bytes,
                    512 + 4 * source.container.len() + 4 * source.original_path.len(),
                    limits.max_metadata_bytes,
                    "metadata bytes",
                )?;
            }
            let status = match (&asset_path, candidates) {
                (None, _) => "no-modl-field; model-selection-deferred",
                (Some(_), []) => "missing-archive-source; lookup-policy-deferred",
                (Some(_), [_]) => "one-archive-source; retail-precedence-unverified",
                _ => "ambiguous-archive-sources; precedence-deferred",
            };
            if let (Some(path), [source]) = (&asset_path, candidates)
                && !selected.contains_key(path)
            {
                if selected.len() >= limits.max_requests {
                    return Err(budget("requests", "exceeded"));
                }
                selected.insert(path.clone(), source.clone());
            }
            coverage.push(ModelCoverage {
                base_key: key,
                source_plugin: store.source_name(location).to_owned(),
                source_sha256: store.source_digest(location)?,
                header: record.header,
                decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
                model_field,
                asset_path,
                candidates: candidates.to_vec(),
                status,
            });
        }
        let mut plan_archives: BTreeMap<String, Arc<ArchiveInput>> = BTreeMap::new();
        let mut planned = Vec::with_capacity(selected.len());
        let mut requests = Vec::with_capacity(selected.len());
        for (path, source) in selected {
            if !plan_archives.contains_key(&source.container) {
                charge(&mut usage.archives, 1, limits.max_archives, "archives")?;
                charge(
                    &mut usage.metadata_bytes,
                    1024 + 4 * source.container.len(),
                    limits.max_metadata_bytes,
                    "metadata bytes",
                )?;
                let input = archives.get(&source.container)?;
                plan_archives.insert(source.container.clone(), input);
            }
            let input = plan_archives[&source.container].clone();
            charge(
                &mut usage.metadata_bytes,
                1024 + 4 * path.bytes().len() + 4 * source.container.len(),
                limits.max_metadata_bytes,
                "metadata bytes",
            )?;
            let member = input
                .member(&path, &source)
                .map_err(|error| Error::Resolution(error.to_string()))?;
            charge(
                &mut usage.model_decoded_bytes,
                member.bytes,
                limits.max_model_decoded_bytes,
                "model decoded bytes",
            )?;
            let metadata = member
                .bytes
                .checked_mul(32)
                .and_then(|bytes| bytes.checked_add(4096))
                .ok_or_else(|| budget("probe metadata bytes", "overflow"))?;
            charge(
                &mut usage.probe_metadata_bytes,
                metadata,
                limits.max_probe_metadata_bytes,
                "probe metadata bytes",
            )?;
            let receipt = RequestReceipt {
                path,
                source,
                archive_sha256: input.source_sha256().to_owned(),
                decoded_bytes: member.bytes,
            };
            requests.push(RequestReceipt {
                path: receipt.path.clone(),
                source: receipt.source.clone(),
                archive_sha256: receipt.archive_sha256.clone(),
                decoded_bytes: receipt.decoded_bytes,
            });
            planned.push(Planned { receipt, input });
        }
        let archives = plan_archives
            .iter()
            .map(|(container, input)| ArchiveReceipt {
                container: container.clone(),
                source_bytes: input.source_bytes(),
                source_sha256: input.source_sha256().to_owned(),
            })
            .collect();
        let mut receipt = PlanReceipt {
            schema_version: 1,
            root: root.clone(),
            identity: String::new(),
            source_cohort_sha256: graph.source_cohort_sha256.clone(),
            coverage,
            archives,
            requests,
            usage,
            limits,
            runtime_ready: false,
        };
        let mut hash = HashWriter(Sha256::new());
        hash.0.update(b"nv-cell-model-plan-v1\0");
        serde_json::to_writer(
            &mut hash,
            &(
                &receipt.root,
                &receipt.source_cohort_sha256,
                &receipt.coverage,
                &receipt.archives,
                &receipt.requests,
            ),
        )
        .map_err(|error| Error::Resolution(error.to_string()))?;
        receipt.identity = format!("{:x}", hash.0.finalize());
        Ok(Self(Arc::new(Inner {
            graph,
            receipt,
            requests: planned,
        })))
    }
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn budget(name: &str, reason: &str) -> Error {
    Error::Resolution(format!("cell model plan {name} budget: {reason}"))
}
fn record_header_nonempty(store: &RecordStore, location: crate::store::Location) -> bool {
    store.definition(location).header.stored_size != 0
}
fn charge(current: &mut usize, add: usize, maximum: usize, name: &str) -> Result<()> {
    let value = current
        .checked_add(add)
        .ok_or_else(|| budget(name, "overflow"))?;
    if value > maximum {
        return Err(budget(name, "exceeded"));
    }
    *current = value;
    Ok(())
}
