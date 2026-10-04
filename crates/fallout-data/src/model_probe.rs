//! Asset inspection for a cell report. Only unambiguous archive candidates are
//! examined; this does not substitute for a verified retail mount policy.
use crate::{
    Error, Result, cache, nif,
    resource_jobs::{ArchiveInput, Artifact, Generation, Limits, ResourceJobs},
    vfs::{AssetPath, AssetSource},
    world::CellReport,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Serialize)]
pub struct ModelProbe {
    pub path: AssetPath,
    pub source: AssetSource,
    pub decoded_bytes: Option<usize>,
    pub sha256: Option<String>,
    pub nif: Option<nif::NifIndex>,
    pub cache: Option<cache::CacheResult>,
    pub error: Option<String>,
}

impl ModelProbe {
    pub(crate) fn empty(path: AssetPath, source: AssetSource) -> Self {
        Self {
            path,
            source,
            decoded_bytes: None,
            sha256: None,
            nif: None,
            cache: None,
            error: None,
        }
    }
}

pub(crate) fn inspect_artifact(probe: &mut ModelProbe, artifact: &mut Artifact) -> Result<()> {
    probe.decoded_bytes = Some(artifact.bytes().len());
    probe.sha256 = Some(format!("{:x}", Sha256::digest(artifact.bytes())));
    probe.nif = Some(nif::inspect(
        artifact.bytes(),
        &String::from_utf8_lossy(probe.path.bytes()),
    )?);
    probe.cache = artifact.take_cache_receipt();
    Ok(())
}

pub fn inspect_models(
    report: &mut CellReport,
    source_tree: &Path,
    cache_root: Option<&Path>,
) -> Result<()> {
    let cache_root = cache_root
        .map(|root| cache::validate_root(root, source_tree))
        .transpose()?;
    let mut selected = BTreeMap::new();
    for model in &report.models {
        if let (Some(path), [source]) = (&model.asset_path, model.candidates.as_slice()) {
            selected.insert(path.clone(), source.clone());
        }
    }
    let mut archives = BTreeMap::new();
    // Hash the immutable selection to identify this preparation request owner.
    // Each member separately binds actual archive digest/length and source index.
    let owner = serde_json::to_vec(&(&report.key, selected.iter().collect::<Vec<_>>()))
        .map_err(|error| Error::Resolution(error.to_string()))?;
    let generation = Generation::new(format!("{:x}", Sha256::digest(&owner)))
        .map_err(|error| Error::Resolution(error.to_string()))?;
    let jobs = ResourceJobs::new(Limits::default(), generation.clone())
        .map_err(|error| Error::Resolution(error.to_string()))?;
    let mut probes = Vec::new();
    for (path, source) in selected {
        if !archives.contains_key(&source.container) {
            let archive = ArchiveInput::open(Path::new(&source.container))
                .map_err(|error| Error::Resolution(error.to_string()))?;
            archives.insert(source.container.clone(), archive);
        }
        let archive = &archives[&source.container];
        let mut probe = ModelProbe::empty(path, source);
        let result = (|| -> Result<()> {
            let mut artifact = jobs
                .submit(
                    archive
                        .member(&probe.path, &probe.source)
                        .map_err(|error| Error::Resolution(error.to_string()))?,
                    generation
                        .token()
                        .map_err(|error| Error::Resolution(error.to_string()))?,
                    cache_root
                        .as_ref()
                        .map(|root| (root.clone(), source_tree.to_path_buf())),
                )
                .and_then(|handle| handle.wait())
                .map_err(|error| Error::Resolution(error.to_string()))?;
            inspect_artifact(&mut probe, &mut artifact)
        })();
        if let Err(error) = result {
            probe.error = Some(error.to_string());
        }
        probes.push(probe);
    }
    report.model_probes = probes;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        identity::{FormKey, ProfileId},
        resource_jobs::tests::Fixture,
        world::{Cell, ModelDependency, SourceField},
    };

    #[test]
    fn model_inspector_consumes_jobs_and_reuses_exact_cached_source() {
        let mut nif = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
        nif.extend(0x1402_0007u32.to_le_bytes());
        nif.push(1);
        for value in [11u32, 0, 34] {
            nif.extend(value.to_le_bytes());
        }
        nif.extend([0, 0, 0]); // three empty export strings
        nif.extend(0u16.to_le_bytes()); // no types or blocks
        for _ in 0..4 {
            nif.extend(0u32.to_le_bytes());
        } // strings, max length, groups, roots
        let fixture = Fixture::new(&nif, true);
        let key = FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "authored.esm".into(),
            local_id: 1,
        };
        let mut report = CellReport {
            schema_version: 1,
            key: key.clone(),
            editor_id: b"AuthoredCell".to_vec(),
            source_plugin: "authored.esm".into(),
            record_offset: 0,
            cell: Cell {
                flags: SourceField {
                    decoded_offset: 0,
                    value: 1,
                },
                grid: None,
                full_name: None,
            },
            integrity_failures: 0,
            index_payloads_deferred: 0,
            index_cache: None,
            link_failures: 0,
            references: vec![],
            models: vec![ModelDependency {
                base_key: key,
                base_kind: "STAT".into(),
                source_plugin: "authored.esm".into(),
                record_offset: 0,
                model_field: None,
                asset_path: Some(fixture.member_path.clone()),
                candidates: vec![fixture.source_member.clone()],
                status: "unique-candidate",
            }],
            model_probes: vec![],
            other_child_records: BTreeMap::new(),
            runtime_ready: false,
            unknown: vec![],
        };
        for reused in [false, true] {
            inspect_models(
                &mut report,
                fixture.source.path(),
                Some(fixture.cache.path()),
            )
            .unwrap();
            let probe = &report.model_probes[0];
            assert!(probe.error.is_none(), "{:?}", probe.error);
            assert_eq!(probe.decoded_bytes, Some(nif.len()));
            assert_eq!(probe.sha256, Some(format!("{:x}", Sha256::digest(&nif))));
            assert_eq!(probe.nif.as_ref().unwrap().bethesda_version, 34);
            assert_eq!(probe.cache.as_ref().unwrap().reused, reused);
            assert!(!report.runtime_ready);
        }
    }
}
