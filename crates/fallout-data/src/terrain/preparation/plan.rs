use super::budget::{Budget, Limits, Usage, charge};
use crate::{
    Error, Result,
    content::ParentContext,
    identity::FormKey,
    resource_jobs::{ArchiveInput, Member},
    store::{RecordStore, SourceReceipt},
    terrain::{TerrainReport, inspect, textures},
    vfs::MountIndex,
    world::preparation::{ArchiveReceipt, RequestReceipt},
};
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, path::Path, sync::Arc};

#[derive(Debug, Serialize)]
pub struct PhysicalContext {
    pub key: FormKey,
    pub parent: ParentContext,
}
#[derive(Debug, Serialize)]
pub struct PlanReceipt {
    pub schema_version: u32,
    pub root: FormKey,
    pub identity: String,
    pub sources: Vec<SourceReceipt>,
    pub source_cohort_sha256: String,
    pub terrain: TerrainReport,
    pub physical_contexts: Vec<PhysicalContext>,
    pub texture_sources: textures::Report,
    pub archives: Vec<ArchiveReceipt>,
    pub requests: Vec<RequestReceipt>,
    pub usage: Usage,
    pub limits: Limits,
    pub runtime_ready: bool,
}
pub(super) struct Planned {
    pub asset_index: usize,
    pub input: Arc<ArchiveInput>,
}
pub(super) struct Inner {
    pub receipt: PlanReceipt,
    pub requests: Vec<Planned>,
    pub terrain_binding: String,
}
/// Only protected winning source reads can construct a plan; inspector JSON
/// cannot manufacture requests. Clones share immutable metadata and source pins.
#[derive(Clone)]
pub struct TextureSourcePlan(pub(super) Arc<Inner>);
impl Serialize for TextureSourcePlan {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.receipt().serialize(serializer)
    }
}
impl TextureSourcePlan {
    pub fn receipt(&self) -> &PlanReceipt {
        &self.0.receipt
    }
    pub fn terrain(&self) -> &TerrainReport {
        &self.0.receipt.terrain
    }
    pub fn identity(&self) -> &str {
        &self.0.receipt.identity
    }
    pub fn root(&self) -> &FormKey {
        &self.0.receipt.root
    }
    pub(crate) fn member(&self, index: usize) -> Result<Member> {
        let request = &self.receipt().requests[index];
        self.0.requests[index]
            .input
            .member(&request.path, &request.source)
            .map_err(|error| Error::Resolution(error.to_string()))
    }
    pub fn load(
        store: &mut RecordStore,
        root: &FormKey,
        mounts: &MountIndex,
        limits: Limits,
    ) -> Result<Self> {
        let mut budget = Budget::new(limits, store)?;
        let sources = store.source_receipts()?;
        let mut cohort = Sha256::new();
        cohort.update(b"nv-world-source-cohort-v1\0");
        for (ordinal, source) in sources.iter().enumerate() {
            cohort.update((ordinal as u64).to_le_bytes());
            cohort.update((source.source_name.len() as u64).to_le_bytes());
            cohort.update(source.source_name.as_bytes());
            cohort.update(source.source_bytes.to_le_bytes());
            cohort.update(source.source_sha256.as_bytes());
        }
        let terrain = inspect::inspect_cell_key_admitted(store, root, None, Some(&mut budget))?;
        if terrain.integrity_failures != 0 {
            return Err(Error::Resolution(
                "terrain preparation refuses tainted source cohort".into(),
            ));
        }
        let texture_limits = textures::Limits {
            records: limits.records,
            layers: limits.layers,
            assets: limits.assets,
            decoded_asset_bytes: limits.texture_bytes,
        };
        let mut texture_sources = textures::collect_sources(
            store,
            &terrain,
            |path| mounts.candidates(path.bytes()),
            None,
            texture_limits,
            Some(&mut budget),
        )?;
        texture_sources.scope = "Immutable LAND/LTEX/TXST physical source plan; selected member bytes not yet prepared; unresolved/default/ambiguous coverage remains explicit; no image/material or gameplay readiness";
        let mut physical_contexts = Vec::new();
        for entry in std::iter::once(&terrain.cell)
            .chain(&terrain.world_chain)
            .chain(&terrain.landscapes)
            .chain(&texture_sources.records)
        {
            budget.metadata(512 + 2 * entry.key.origin_plugin.len())?;
            let location = store
                .winner(&entry.key)
                .expect("source-derived winning entry");
            physical_contexts.push(PhysicalContext {
                key: entry.key.clone(),
                parent: store.definition(location).parent.clone(),
            });
        }
        let mut archives: BTreeMap<String, Arc<ArchiveInput>> = BTreeMap::new();
        let mut requests = Vec::new();
        let mut planned = Vec::new();
        for (asset_index, asset) in texture_sources.assets.iter_mut().enumerate() {
            if asset.candidates.len() != 1 {
                texture_sources.failures += 1;
                asset.error = Some(
                    Error::Resolution(format!(
                        "texture has {} archive candidates; no verified precedence",
                        asset.candidates.len(),
                    ))
                    .to_string(),
                );
                continue;
            }
            let source = &asset.candidates[0];
            if !archives.contains_key(&source.container) {
                charge(&mut budget.usage.archives, 1, limits.archives, "archives")?;
                budget.metadata(1024 + 4 * source.container.len())?;
                archives.insert(
                    source.container.clone(),
                    ArchiveInput::open(Path::new(&source.container))
                        .map_err(|error| Error::Resolution(error.to_string()))?,
                );
            }
            budget.metadata(
                2048 + 4 * source.container.len()
                    + 4 * source.original_path.len()
                    + 4 * asset.path.bytes().len(),
            )?;
            let input = archives[&source.container].clone();
            let member = input
                .member(&asset.path, source)
                .map_err(|error| Error::Resolution(error.to_string()))?;
            charge(
                &mut budget.usage.texture_bytes,
                member.bytes,
                limits.texture_bytes,
                "texture bytes",
            )?;
            requests.push(RequestReceipt {
                path: asset.path.clone(),
                source: source.clone(),
                archive_sha256: input.source_sha256().to_owned(),
                decoded_bytes: member.bytes,
            });
            planned.push(Planned { asset_index, input });
        }
        let archives = archives
            .iter()
            .map(|(container, input)| ArchiveReceipt {
                container: container.clone(),
                source_bytes: input.source_bytes(),
                source_sha256: input.source_sha256().to_owned(),
            })
            .collect();
        let terrain_binding = binding(&terrain)?;
        let mut receipt = PlanReceipt {
            schema_version: 1,
            root: root.clone(),
            identity: String::new(),
            sources,
            source_cohort_sha256: format!("{:x}", cohort.finalize()),
            terrain,
            physical_contexts,
            texture_sources,
            archives,
            requests,
            usage: budget.usage,
            limits,
            runtime_ready: false,
        };
        receipt.identity = digest(
            b"nv-terrain-texture-plan-v1\0",
            &(
                &receipt.root,
                &receipt.source_cohort_sha256,
                &receipt.terrain.cell,
                &receipt.terrain.world_chain,
                &receipt.terrain.landscapes,
                &receipt.physical_contexts,
                &receipt.texture_sources,
                &receipt.archives,
                &receipt.requests,
            ),
        )?;
        Ok(Self(Arc::new(Inner {
            receipt,
            requests: planned,
            terrain_binding,
        })))
    }
}

pub(super) fn binding(report: &TerrainReport) -> Result<String> {
    if report.runtime_ready
        || report.integrity_failures != 0
        || report.texture_dependencies.is_some()
    {
        return Err(Error::Resolution(
            "terrain publication source sink is not a pristine source diagnostic".into(),
        ));
    }
    digest(
        b"nv-terrain-source-binding-v1\0",
        &(
            &report.cell,
            &report.world_chain,
            &report.landscapes,
            report.link_failures,
        ),
    )
}
fn digest(domain: &[u8], value: &impl Serialize) -> Result<String> {
    let mut hash = HashWriter {
        hash: Sha256::new(),
        bytes: 0,
    };
    hash.hash.update(domain);
    serde_json::to_writer(&mut hash, value)
        .map_err(|error| Error::Resolution(error.to_string()))?;
    Ok(format!("{:x}", hash.hash.finalize()))
}
struct HashWriter {
    hash: Sha256,
    bytes: usize,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("terrain identity size overflow"))?;
        if self.bytes > 512 * 1024 * 1024 {
            return Err(std::io::Error::other(
                "terrain identity serialization ceiling exceeded",
            ));
        }
        self.hash.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
