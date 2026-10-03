//! Resolve the authored LAND -> LTEX -> TXST chain without inventing defaults.
//! Archive bytes are verified separately from image, blend and shader semantics.
use super::{Fields, RecordEntry, TerrainReport, inspect::entry_bounded};
use crate::{
    Error, Result,
    assets::ArchiveAssets,
    cache,
    identity::{FormKey, ProfileId},
    store::RecordStore,
    vfs::{AssetPath, AssetSource, texture_path},
    world::Dependency,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

const MAX_TEXTURE_RECORD_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy)]
pub struct Limits {
    pub records: usize,
    pub layers: usize,
    pub assets: usize,
    pub decoded_asset_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            records: 128,
            layers: 4096,
            assets: 256,
            decoded_asset_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Keep archive policy behind an interface. Production rejects ambiguity; fixtures
/// can exercise dependency failures without manufacturing a retail archive.
pub trait AssetReader {
    fn source_tree(&self) -> &Path;
    fn candidates(&self, path: &AssetPath) -> Result<&[AssetSource]>;
    fn read(&mut self, path: &AssetPath, maximum: usize) -> Result<AssetRead>;
}
pub struct AssetRead {
    pub source: AssetSource,
    pub archive_sha256: String,
    pub bytes: Vec<u8>,
}
impl AssetReader for ArchiveAssets {
    fn source_tree(&self) -> &Path {
        &self.source_tree
    }
    fn candidates(&self, path: &AssetPath) -> Result<&[AssetSource]> {
        self.candidates(path)
    }
    fn read(&mut self, path: &AssetPath, maximum: usize) -> Result<AssetRead> {
        let (source, bytes) = self.read_unique_bounded(path, maximum as u64)?;
        let archive_sha256 = self.source_digest(&source)?.to_owned();
        Ok(AssetRead {
            source,
            archive_sha256,
            bytes,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Binding {
    pub land: FormKey,
    pub layer_index: usize,
    pub land_texture: Dependency,
    pub texture_set: Option<Dependency>,
    pub status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct PathUsage {
    pub texture_set: FormKey,
    pub slot: usize,
    pub decoded_offset: usize,
    pub raw_path: Vec<u8>,
    pub path: Option<AssetPath>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Asset {
    pub path: AssetPath,
    pub usages: Vec<usize>,
    pub candidates: Vec<AssetSource>,
    pub archive_sha256: Option<String>,
    pub decoded_bytes: Option<usize>,
    pub sha256: Option<String>,
    pub cache: Option<cache::CacheResult>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub records: Vec<RecordEntry>,
    pub bindings: Vec<Binding>,
    pub path_usages: Vec<PathUsage>,
    pub assets: Vec<Asset>,
    pub failures: usize,
    pub unapplied_default_layers: usize,
    pub decoded_asset_bytes: usize,
    pub runtime_ready: bool,
    pub scope: &'static str,
}

fn load_record(
    store: &mut RecordStore,
    records: &mut BTreeMap<FormKey, RecordEntry>,
    key: &FormKey,
    bodies: Option<(&Path, &Path)>,
    maximum: usize,
) -> Result<()> {
    if records.contains_key(key) {
        return Ok(());
    }
    if records.len() >= maximum {
        return Err(Error::Unsupported(
            "terrain texture record budget exceeded".into(),
        ));
    }
    let location = *store
        .winners
        .get(key)
        .ok_or_else(|| Error::Resolution("resolved texture record disappeared".into()))?;
    records.insert(
        key.clone(),
        entry_bounded(
            store,
            key.clone(),
            location,
            bodies,
            MAX_TEXTURE_RECORD_BYTES,
        )?,
    );
    Ok(())
}

pub fn inspect(
    store: &mut RecordStore,
    terrain: &TerrainReport,
    assets: &mut impl AssetReader,
    bodies: Option<(&Path, &Path)>,
    asset_cache: Option<&Path>,
    limits: Limits,
) -> Result<Report> {
    // Validate destinations before reading any additional body or texture asset.
    if let Some((root, source_tree)) = bodies {
        cache::validate_root(root, source_tree)?;
    }
    let asset_cache = asset_cache
        .map(|root| cache::validate_root(root, assets.source_tree()))
        .transpose()?;
    let mut records = BTreeMap::new();
    let mut bindings = Vec::new();
    let mut failures = 0;
    for land in &terrain.landscapes {
        let Some(Fields::Land(fields)) = &land.fields else {
            continue;
        };
        for i in 0..fields.layers.len() {
            if bindings.len() >= limits.layers {
                return Err(Error::Unsupported(
                    "terrain texture layer budget exceeded".into(),
                ));
            }
            let texture = land
                .links
                .get(&format!("layer[{i}].texture"))
                .ok_or_else(|| Error::Resolution("LAND layer lacks resolved provenance".into()))?
                .clone();
            let mut binding = Binding {
                land: land.key.clone(),
                layer_index: i,
                land_texture: texture,
                texture_set: None,
                status: "unresolved-LTEX",
            };
            // xEdit permits NULL in BTXT/ATXT. It denotes a default layer, not a
            // missing record. Keep that capability gap separate from bad links.
            if binding.land_texture.status == "null" {
                binding.status = "null-default-unapplied";
            }
            if binding.land_texture.status == "resolved" {
                let key = binding
                    .land_texture
                    .key
                    .as_ref()
                    .expect("resolved LTEX key");
                load_record(store, &mut records, key, bodies, limits.records)?;
                let texture_set = records[key].links.get("TNAM").cloned();
                if let Some(link) = &texture_set {
                    binding.status = "unresolved-TXST";
                    if link.status == "resolved" {
                        let key = link.key.as_ref().expect("resolved TXST key");
                        load_record(store, &mut records, key, bodies, limits.records)?;
                        let Some(Fields::TextureSet(fields)) = &records[key].fields else {
                            unreachable!("TXST decoder");
                        };
                        binding.status = if fields.paths[0]
                            .as_ref()
                            .is_some_and(|p| !p.value.is_empty())
                        {
                            "records-resolved"
                        } else {
                            "missing-diffuse-path"
                        };
                    }
                } else {
                    binding.status = "absent-TNAM";
                }
                binding.texture_set = texture_set;
            }
            failures += usize::from(
                !["records-resolved", "null-default-unapplied"].contains(&binding.status),
            );
            bindings.push(binding);
        }
    }
    let mut usages = Vec::new();
    let mut paths: BTreeMap<AssetPath, Vec<usize>> = BTreeMap::new();
    for (key, record) in &records {
        // Grass references are checked, but their payloads/assets and behavior are
        // outside this texture chain. Do not claim transitive grass closure.
        failures += record
            .links
            .values()
            .filter(|link| !["resolved", "null"].contains(&link.status))
            .count();
        let Some(Fields::TextureSet(fields)) = &record.fields else {
            continue;
        };
        for (slot, field) in fields.paths.iter().enumerate() {
            let Some(field) = field else {
                continue;
            };
            let mut usage = PathUsage {
                texture_set: key.clone(),
                slot,
                decoded_offset: field.decoded_offset,
                raw_path: field.value.clone(),
                path: None,
                error: None,
            };
            if !field.value.is_empty() {
                match texture_path(&field.value) {
                    Ok(path) => {
                        paths.entry(path.clone()).or_default().push(usages.len());
                        usage.path = Some(path);
                    }
                    Err(error) => {
                        failures += 1;
                        usage.error = Some(error.to_string());
                    }
                }
            }
            usages.push(usage);
        }
    }
    if paths.len() > limits.assets {
        return Err(Error::Unsupported(
            "terrain texture asset budget exceeded".into(),
        ));
    }
    let mut decoded = 0;
    let mut probes = Vec::new();
    for (path, usages) in paths {
        let candidates = assets.candidates(&path)?;
        if candidates.len() > 64 {
            return Err(Error::Unsupported(
                "terrain texture candidate budget exceeded".into(),
            ));
        }
        let mut probe = Asset {
            path,
            usages,
            candidates: candidates.to_vec(),
            archive_sha256: None,
            decoded_bytes: None,
            sha256: None,
            cache: None,
            error: None,
        };
        let result = (|| -> Result<()> {
            // Candidate multiplicity is a policy failure, even if bytes happen to match.
            if probe.candidates.len() != 1 {
                return Err(Error::Resolution(format!(
                    "texture has {} archive candidates; no verified precedence",
                    probe.candidates.len()
                )));
            }
            let remaining = limits.decoded_asset_bytes.saturating_sub(decoded);
            let data = assets.read(&probe.path, remaining)?;
            if data.bytes.len() > remaining {
                return Err(Error::Unsupported(
                    "terrain texture byte budget exceeded".into(),
                ));
            }
            if data.source.container != probe.candidates[0].container
                || data.source.entry_index != probe.candidates[0].entry_index
                || data.source.original_path != probe.candidates[0].original_path
            {
                return Err(Error::Resolution(
                    "texture source differs from candidate".into(),
                ));
            }
            decoded += data.bytes.len();
            probe.decoded_bytes = Some(data.bytes.len());
            probe.sha256 = Some(format!("{:x}", Sha256::digest(&data.bytes)));
            if let Some(root) = &asset_cache {
                probe.cache = Some(cache::publish(
                    root,
                    assets.source_tree(),
                    cache::ArtifactIdentity {
                        profile: ProfileId::NvOriginal,
                        source_sha256: data.archive_sha256.clone(),
                        path_bytes: probe.path.bytes().to_vec(),
                        transform_version: "nv-bsa104-decode-v1".into(),
                    },
                    &data.bytes,
                )?);
            }
            probe.archive_sha256 = Some(data.archive_sha256);
            Ok(())
        })();
        if let Err(error) = result {
            failures += 1;
            probe.error = Some(error.to_string());
        }
        probes.push(probe);
    }
    let unapplied_default_layers = bindings
        .iter()
        .filter(|b| b.status == "null-default-unapplied")
        .count();
    Ok(Report {
        records: records.into_values().collect(),
        bindings,
        path_usages: usages,
        assets: probes,
        failures,
        unapplied_default_layers,
        decoded_asset_bytes: decoded,
        runtime_ready: false,
        scope: "Authored nonnull LAND layers -> winning LTEX/TXST records -> nonempty TX00..TX05 archive bytes; NULL default layers remain explicit and unapplied; unique candidates only, no loose/retail precedence, image interpretation, shader/blend behavior or grass asset closure",
    })
}
