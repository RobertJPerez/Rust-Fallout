//! Full archive NIF/KF container inventory, with every rejected member retained
//! in the report. Geometry and runtime behavior remain separate acceptance gates.
use crate::{Result, archive::NvArchive, baseline::digest_file, nif, nif_scene};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Serialize)]
pub struct Failure {
    pub path_bytes: Vec<u8>,
    pub file_offset: u32,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct NifCensus {
    pub archive: String,
    pub archive_sha256: String,
    pub nif_members: usize,
    pub kf_members: usize,
    pub containers_decoded: usize,
    pub decoded_bytes: u64,
    pub blocks: BTreeMap<String, u64>,
    pub stream_versions: BTreeMap<u32, usize>,
    pub files_with_block: BTreeMap<String, u64>,
    pub failures: Vec<Failure>,
    pub behavior_status: &'static str,
    pub scene_payloads: Option<SceneCensus>,
}

#[derive(Debug, Default, Serialize)]
pub struct SceneCensus {
    pub files_decoded: usize,
    pub objects: usize,
    pub meshes: usize,
    pub materials: usize,
    pub texture_references: usize,
    pub unsafe_texture_paths: usize,
    /// Keep a bounded diagnostic sample; the count above always includes every failure.
    pub texture_path_failures: Vec<TexturePathFailure>,
    pub vertices: usize,
    pub triangles: usize,
    pub strip_degenerate_triangles: usize,
    pub decoded_blocks: BTreeMap<String, usize>,
    pub unsupported_blocks: BTreeMap<String, usize>,
    pub unsupported_scene_edges: usize,
    pub missing_geometry_arrays: usize,
    pub source_triangle_count_mismatches: usize,
    pub failures: Vec<Failure>,
}

#[derive(Debug, Serialize)]
pub struct TexturePathFailure {
    pub model_path_bytes: Vec<u8>,
    pub block: u32,
    pub slot: usize,
    pub raw_path_preview: Vec<u8>,
    pub raw_path_bytes: usize,
    pub reason: String,
}

impl SceneCensus {
    fn record(&mut self, scene: nif_scene::Scene, index: &nif::NifIndex, path: &[u8]) {
        self.files_decoded += 1;
        self.objects += scene.objects.len();
        self.meshes += scene.meshes.len();
        self.materials += scene.materials.len();
        self.texture_references += scene.textures.len();
        self.unsafe_texture_paths += scene.textures.iter().filter(|t| t.error.is_some()).count();
        for texture in &scene.textures {
            if let Some(reason) = &texture.error
                && self.texture_path_failures.len() < 100
            {
                self.texture_path_failures.push(TexturePathFailure {
                    model_path_bytes: path.to_vec(),
                    block: texture.block,
                    slot: texture.slot,
                    raw_path_preview: texture.raw_path.iter().take(512).copied().collect(),
                    raw_path_bytes: texture.raw_path.len(),
                    reason: reason.clone(),
                });
            }
        }
        self.unsupported_scene_edges += scene.unsupported_scene_edges.len();
        for (kind, ids) in scene.unsupported_blocks {
            *self.unsupported_blocks.entry(kind).or_default() += ids.len();
        }
        for id in scene
            .objects
            .iter()
            .map(|o| o.block)
            .chain(scene.meshes.iter().map(|m| m.block))
            .chain(scene.materials.iter().map(|m| m.block))
        {
            let kind = &index.block_types[index.blocks[id as usize].type_index as usize];
            *self.decoded_blocks.entry(kind.clone()).or_default() += 1;
        }
        for mesh in scene.meshes {
            self.vertices += mesh.vertices.len();
            self.triangles += mesh.triangles.len();
            self.strip_degenerate_triangles += mesh.strip_degenerate_triangles;
            self.missing_geometry_arrays +=
                usize::from(!mesh.has_vertices && mesh.vertex_count != 0);
            self.source_triangle_count_mismatches +=
                usize::from(!mesh.source_triangle_count_matches);
        }
    }
}

pub fn scan(path: &Path) -> Result<NifCensus> {
    scan_with_scenes(path, false)
}

pub fn scan_with_scenes(path: &Path, inspect_scenes: bool) -> Result<NifCensus> {
    let archive = NvArchive::open(path)?;
    let mut report = NifCensus {
        archive: path.display().to_string(),
        archive_sha256: digest_file(path)?.1,
        nif_members: 0,
        kf_members: 0,
        containers_decoded: 0,
        decoded_bytes: 0,
        blocks: BTreeMap::new(),
        stream_versions: BTreeMap::new(),
        files_with_block: BTreeMap::new(),
        failures: Vec::new(),
        behavior_status: "unknown; byte decoding does not establish runtime behavior",
        scene_payloads: inspect_scenes.then(SceneCensus::default),
    };
    for (id, entry) in archive.backend().entries_with_ids() {
        let Some(name) = entry.path() else {
            continue;
        };
        let raw: &[u8] = name.as_ref();
        let suffix = raw.rsplit(|v| *v == b'.').next().unwrap_or_default();
        if suffix.eq_ignore_ascii_case(b"nif") {
            report.nif_members += 1;
        } else if suffix.eq_ignore_ascii_case(b"kf") {
            report.kf_members += 1;
        } else {
            continue;
        }
        let result = archive.read(id).and_then(|bytes| {
            report.decoded_bytes += bytes.len() as u64;
            let source = String::from_utf8_lossy(raw);
            let index = nif::inspect(&bytes, &source)?;
            if let Some(census) = &mut report.scene_payloads {
                match nif_scene::decode(&bytes, &source) {
                    Ok((_, scene)) => census.record(scene, &index, raw),
                    Err(error) => census.failures.push(Failure {
                        path_bytes: raw.to_vec(),
                        file_offset: entry.file().data_offset,
                        reason: error.to_string(),
                    }),
                }
            }
            Ok(index)
        });
        match result {
            Ok(index) => {
                report.containers_decoded += 1;
                *report
                    .stream_versions
                    .entry(index.bethesda_version)
                    .or_default() += 1;
                for (kind, count) in index.block_counts {
                    *report.files_with_block.entry(kind.clone()).or_default() += 1;
                    *report.blocks.entry(kind).or_default() += count as u64;
                }
            }
            Err(error) => report.failures.push(Failure {
                path_bytes: raw.to_vec(),
                file_offset: entry.file().data_offset,
                reason: error.to_string(),
            }),
        }
    }
    Ok(report)
}
