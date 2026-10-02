//! Follow authored NIF texture paths to immutable archive members. This does not
//! choose ambiguous overrides or claim the image pixels have been interpreted.
use crate::{
    Result,
    assets::ArchiveAssets,
    baseline,
    cache::{self, ArtifactIdentity},
    identity::ProfileId,
    io, nif_scene,
    vfs::{AssetPath, AssetSource},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct ModelTextures {
    pub file: PathBuf,
    pub sha256: Option<String>,
    pub material_blocks: usize,
    pub textures: Vec<nif_scene::material::TextureReference>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub model: usize,
    pub block: u32,
    pub slot: usize,
}

#[derive(Debug, Serialize)]
pub struct TextureProbe {
    pub path: AssetPath,
    pub usages: Vec<Usage>,
    pub candidates: Vec<AssetSource>,
    pub decoded_bytes: Option<usize>,
    pub sha256: Option<String>,
    pub cache: Option<cache::CacheResult>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TextureReport {
    pub schema_version: u32,
    pub profile: ProfileId,
    pub models: Vec<ModelTextures>,
    pub textures: Vec<TextureProbe>,
    pub failures: usize,
    pub runtime_ready: bool,
    pub scope: &'static str,
}

pub fn inspect(
    input: &Path,
    assets: &mut ArchiveAssets,
    cache_root: Option<&Path>,
) -> Result<TextureReport> {
    let cache_root = cache_root
        .map(|root| cache::validate_root(root, &assets.source_tree))
        .transpose()?;
    let mut paths = Vec::new();
    let metadata = fs::symlink_metadata(input).map_err(|e| io(input, e))?;
    if metadata.is_dir() {
        for entry in fs::read_dir(input).map_err(|e| io(input, e))? {
            let entry = entry.map_err(|e| io(input, e))?;
            if entry
                .file_type()
                .map_err(|e| io(entry.path(), e))?
                .is_file()
                && entry.path().extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("nif") || e.eq_ignore_ascii_case("blob")
                })
            {
                paths.push(entry.path());
            }
        }
    } else {
        paths.push(input.to_path_buf());
    }
    paths.sort();
    if paths.is_empty() {
        return Err(crate::Error::Resolution("no NIF/blob inputs found".into()));
    }
    let mut models = Vec::new();
    let mut selected: BTreeMap<AssetPath, Vec<Usage>> = BTreeMap::new();
    let mut failures = 0;
    for path in paths {
        let mut model = ModelTextures {
            file: path,
            sha256: None,
            material_blocks: 0,
            textures: Vec::new(),
            error: None,
        };
        let result = (|| -> Result<()> {
            let mut bytes = Vec::new();
            baseline::open_source(&model.file)?
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| io(&model.file, e))?;
            model.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
            let (_, scene) = nif_scene::decode(&bytes, &model.file.display().to_string())?;
            model.material_blocks = scene.materials.len();
            model.textures = scene.textures;
            for texture in &model.textures {
                if let Some(path) = &texture.asset_path {
                    selected.entry(path.clone()).or_default().push(Usage {
                        model: models.len(),
                        block: texture.block,
                        slot: texture.slot,
                    });
                } else {
                    failures += 1;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            failures += 1;
            model.error = Some(error.to_string());
        }
        models.push(model);
    }
    let mut textures = Vec::new();
    for (path, usages) in selected {
        let candidates = assets.candidates(&path)?.to_vec();
        let mut probe = TextureProbe {
            path,
            usages,
            candidates,
            decoded_bytes: None,
            sha256: None,
            cache: None,
            error: None,
        };
        let result = (|| -> Result<()> {
            let (source, bytes) = assets.read_unique(&probe.path)?;
            probe.decoded_bytes = Some(bytes.len());
            probe.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
            if let Some(root) = &cache_root {
                let identity = ArtifactIdentity {
                    profile: ProfileId::NvOriginal,
                    source_sha256: assets.source_digest(&source)?.into(),
                    path_bytes: probe.path.bytes().to_vec(),
                    transform_version: "nv-bsa104-decode-v1".into(),
                };
                probe.cache = Some(cache::publish(root, &assets.source_tree, identity, &bytes)?);
            }
            Ok(())
        })();
        if let Err(error) = result {
            failures += 1;
            probe.error = Some(error.to_string());
        }
        textures.push(probe);
    }
    Ok(TextureReport {
        schema_version: 1,
        profile: ProfileId::NvOriginal,
        models,
        textures,
        failures,
        runtime_ready: false,
        scope: "authored external texture references in decoded material blocks; archive candidates only; no pixel/shader parity",
    })
}
