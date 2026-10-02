//! Asset inspection for a cell report. Only unambiguous archive candidates are
//! examined; this does not substitute for a verified retail mount policy.
use crate::{
    Error, Result,
    archive::NvArchive,
    baseline::digest_file,
    cache::{self, ArtifactIdentity},
    identity::ProfileId,
    nif,
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

pub fn inspect_models(
    report: &mut CellReport,
    source_tree: &Path,
    cache_root: Option<&Path>,
) -> Result<()> {
    let mut selected = BTreeMap::new();
    for model in &report.models {
        if let (Some(path), [source]) = (&model.asset_path, model.candidates.as_slice()) {
            selected.insert(path.clone(), source.clone());
        }
    }
    let mut archives = BTreeMap::new();
    let mut digests = BTreeMap::new();
    let mut probes = Vec::new();
    for (path, source) in selected {
        if !archives.contains_key(&source.container) {
            let archive = NvArchive::open(Path::new(&source.container))?;
            if cache_root.is_some() {
                digests.insert(
                    source.container.clone(),
                    digest_file(Path::new(&source.container))?.1,
                );
            }
            archives.insert(source.container.clone(), archive);
        }
        let archive = &archives[&source.container];
        let mut probe = ModelProbe {
            path,
            source,
            decoded_bytes: None,
            sha256: None,
            nif: None,
            cache: None,
            error: None,
        };
        let result = (|| -> Result<()> {
            let id = archive
                .backend()
                .get_id(&probe.source.original_path)
                .ok_or_else(|| Error::Resolution("indexed archive member disappeared".into()))?;
            if id.index() != probe.source.entry_index {
                return Err(Error::Resolution("archive member identity changed".into()));
            }
            let bytes = archive.read(id)?;
            probe.decoded_bytes = Some(bytes.len());
            probe.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
            probe.nif = Some(nif::inspect(
                &bytes,
                &String::from_utf8_lossy(probe.path.bytes()),
            )?);
            if let Some(root) = cache_root {
                probe.cache = Some(cache::publish(
                    root,
                    source_tree,
                    ArtifactIdentity {
                        profile: ProfileId::NvOriginal,
                        source_sha256: digests[&probe.source.container].clone(),
                        path_bytes: probe.path.bytes().to_vec(),
                        transform_version: "nv-bsa104-decode-v1".into(),
                    },
                    &bytes,
                )?);
            }
            Ok(())
        })();
        if let Err(error) = result {
            probe.error = Some(error.to_string());
        }
        probes.push(probe);
    }
    report.model_probes = probes;
    Ok(())
}
