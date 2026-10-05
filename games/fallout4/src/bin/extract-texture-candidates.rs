//! Extract a bounded, source-verified sample of unambiguous direct-BA2 DDS
//! candidates named by parsed FO4 materials. This does not select runtime files.
use dream_archive::ByteSlice;
use fallout_data::vfs::AssetPath;
use fallout4_prep::{
    Error, Result,
    archive::{Ba2, Limits},
    census,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

const REFERENCE_REVISION: &str = "21411873f17454a2442d6785d533499e14c63adb";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SELECTIONS: usize = 128;
const DEFAULT_SELECTIONS: usize = 24;
const MAX_MEMBER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct AnalysisCompletion {
    schema_version: u32,
    manifest_sha256: String,
    observation_complete: bool,
    runtime_ready: bool,
    archives_scanned: usize,
    archives_with_errors: usize,
    unique_texture_paths: usize,
    texture_reference_slots: usize,
    missing_unique_paths: usize,
    ambiguous_unique_paths: usize,
    candidate_members: usize,
    invalid_texture_references: usize,
}

#[derive(Debug, Deserialize)]
struct AnalysisManifest {
    schema_version: u32,
    reference_revision: String,
    observation_complete: bool,
    runtime_ready: bool,
    archives_scanned: usize,
    archives_with_errors: usize,
    candidate_members: usize,
    invalid_texture_references: Vec<serde_json::Value>,
    archives: Vec<ArchiveObservation>,
    textures: Vec<TextureRequest>,
}

#[derive(Debug, Deserialize)]
struct ArchiveObservation {
    archive: String,
    archive_sha256: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TextureRequest {
    canonical_path: String,
    references: Vec<TextureUse>,
    candidates: Vec<Candidate>,
}

#[derive(Clone, Debug, Deserialize)]
struct TextureUse {
    archive: String,
    material_entry_index: usize,
    material_member_name_bytes_hex: String,
    material_kind: String,
    material_sha256: String,
    field: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Candidate {
    archive: String,
    archive_sha256: String,
    entry_index: usize,
    member_name_bytes_hex: String,
}

#[derive(Clone, Debug)]
struct Selection {
    texture: TextureRequest,
    fields: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TextureHeaderEvidence {
    width: u16,
    height: u16,
    mip_count: u8,
    dxgi_format: u8,
    flags: u8,
    tile_mode: u8,
}

#[derive(Debug, Serialize)]
struct MaterialSourceExample {
    archive: String,
    entry_index: usize,
    member_name_bytes_hex: String,
    kind: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct ExtractedTexture {
    id: usize,
    canonical_path: String,
    material_fields: Vec<String>,
    material_reference_count: usize,
    material_sources: Vec<MaterialSourceExample>,
    candidate_archive: String,
    candidate_archive_sha256: String,
    candidate_entry_index: usize,
    candidate_member_name_bytes_hex: String,
    texture_header: TextureHeaderEvidence,
    dds_file: String,
    dds_bytes: u64,
    dds_sha256: String,
}

#[derive(Debug, Serialize)]
struct StableArchive {
    archive: String,
    sha256: String,
    unchanged_before_and_after_read: bool,
}

#[derive(Debug, Serialize)]
struct ExtractionReport {
    schema_version: u32,
    analysis_manifest_sha256: String,
    reference_revision: String,
    requested_limit: usize,
    selected_count: usize,
    extracted_bytes: u64,
    texture_sources: Vec<StableArchive>,
    textures: Vec<ExtractedTexture>,
    scope: &'static str,
}

#[derive(Debug, Serialize)]
struct ExtractionCompletion {
    schema_version: u32,
    report_sha256: String,
    selected_count: usize,
    extracted_bytes: u64,
    texture_sources: usize,
    complete: bool,
    runtime_ready: bool,
}

fn read_stable(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let before = fs::metadata(path)?;
    if !before.is_file() || before.len() > limit {
        return Err(Error::Unsupported(format!(
            "{}: source type or byte budget exceeded",
            path.display()
        )));
    }
    let bytes = fs::read(path)?;
    let after = fs::metadata(path)?;
    if bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(Error::Unsupported(format!(
            "{} changed during evidence read",
            path.display()
        )));
    }
    Ok(bytes)
}

fn analysis_manifest(evidence: &Path) -> Result<(AnalysisManifest, String)> {
    let completion_bytes = read_stable(&evidence.join("complete.json"), 1024 * 1024)?;
    let completion: AnalysisCompletion = serde_json::from_slice(&completion_bytes)?;
    let manifest_bytes = read_stable(&evidence.join("manifest.json"), MAX_MANIFEST_BYTES)?;
    let manifest_sha256 = census::sha256(&manifest_bytes);
    if completion.schema_version != 1 || completion.manifest_sha256 != manifest_sha256 {
        return Err(Error::Unsupported(
            "texture analysis completion does not bind its manifest".into(),
        ));
    }
    let manifest: AnalysisManifest = serde_json::from_slice(&manifest_bytes)?;
    let candidate_count: usize = manifest.textures.iter().map(|t| t.candidates.len()).sum();
    if manifest.schema_version != 1
        || manifest.reference_revision != REFERENCE_REVISION
        || !manifest.observation_complete
        || manifest.runtime_ready
        || manifest.archives_scanned != completion.archives_scanned
        || manifest.archives_with_errors != completion.archives_with_errors
        || manifest.candidate_members != completion.candidate_members
        || candidate_count != manifest.candidate_members
        || manifest.archives_scanned != manifest.archives.len()
        || manifest.archives_with_errors != 0
        || !manifest.invalid_texture_references.is_empty()
        || !completion.observation_complete
        || completion.runtime_ready
        || completion.unique_texture_paths != manifest.textures.len()
        || completion.invalid_texture_references != 0
    {
        return Err(Error::Unsupported(
            "texture analysis is incomplete, inconsistent, or uses an unsupported schema".into(),
        ));
    }
    let reference_slots: usize = manifest.textures.iter().map(|t| t.references.len()).sum();
    if reference_slots != completion.texture_reference_slots
        || manifest.textures.len() != completion.unique_texture_paths
        || manifest
            .textures
            .iter()
            .filter(|texture| texture.candidates.is_empty())
            .count()
            != completion.missing_unique_paths
        || manifest
            .textures
            .iter()
            .filter(|texture| texture.candidates.len() > 1)
            .count()
            != completion.ambiguous_unique_paths
    {
        return Err(Error::Unsupported(
            "texture analysis completion counts do not match its manifest".into(),
        ));
    }
    Ok((manifest, manifest_sha256))
}

fn selection_fields(request: &TextureRequest) -> Vec<String> {
    let fields: BTreeSet<_> = request
        .references
        .iter()
        .map(|reference| reference.field.clone())
        .collect();
    fields.into_iter().collect()
}

fn select_textures(manifest: &AnalysisManifest, limit: usize) -> Result<Vec<Selection>> {
    if limit == 0 || limit > MAX_SELECTIONS {
        return Err(Error::Unsupported(format!(
            "selection limit must be between 1 and {MAX_SELECTIONS}"
        )));
    }
    let eligible: Vec<_> = manifest
        .textures
        .iter()
        .filter(|texture| {
            texture.candidates.len() == 1
                && texture
                    .canonical_path
                    .to_ascii_lowercase()
                    .ends_with(".dds")
                && !texture.references.is_empty()
        })
        .collect();
    let field_priority = [
        "DiffuseTexture",
        "BaseTexture",
        "NormalTexture",
        "SmoothSpecTexture",
        "SpecularTexture",
        "GrayscaleTexture",
        "GreyscaleTexture",
        "EnvmapTexture",
        "EnvmapMaskTexture",
        "GlowTexture",
        "LightingTexture",
        "InnerLayerTexture",
        "WrinklesTexture",
        "DisplacementTexture",
        "FlowTexture",
        "DistanceFieldAlphaTexture",
    ];
    let mut selected = BTreeMap::<String, Selection>::new();
    for field in field_priority {
        let mut admitted = 0usize;
        for texture in &eligible {
            if !texture
                .references
                .iter()
                .any(|use_site| use_site.field == field)
                || selected.contains_key(&texture.canonical_path)
            {
                continue;
            }
            selected.insert(
                texture.canonical_path.clone(),
                Selection {
                    texture: (*texture).clone(),
                    fields: selection_fields(texture),
                },
            );
            admitted += 1;
            if admitted == 2 || selected.len() == limit {
                break;
            }
        }
        if selected.len() == limit {
            break;
        }
    }
    if selected.len() < limit {
        for texture in eligible {
            if selected.len() == limit {
                break;
            }
            selected
                .entry(texture.canonical_path.clone())
                .or_insert_with(|| Selection {
                    texture: texture.clone(),
                    fields: selection_fields(texture),
                });
        }
    }
    if selected.is_empty() {
        return Err(Error::Unsupported(
            "no uniquely matched direct-BA2 DDS candidates are available".into(),
        ));
    }
    Ok(selected.into_values().collect())
}

fn validate_archive_name(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || Path::new(name)
            .extension()
            .is_none_or(|ext| !ext.eq_ignore_ascii_case("ba2"))
    {
        return Err(Error::Unsupported(format!(
            "unsafe or non-BA2 candidate archive name {name:?}"
        )));
    }
    Ok(())
}

fn output_directory(install: &Path, evidence: &Path, requested: &Path) -> Result<PathBuf> {
    let install = fs::canonicalize(install)?;
    let evidence = fs::canonicalize(evidence)?;
    let local = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("local"))?;
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)?;
    let name = requested
        .file_name()
        .ok_or_else(|| Error::Unsupported("new texture output directory name required".into()))?;
    let output = parent.join(name);
    if output.starts_with(&install) || output.starts_with(&evidence) || !output.starts_with(&local)
    {
        return Err(Error::Unsupported(
            "output must be inside ignored local/ and outside install/evidence inputs".into(),
        ));
    }
    Ok(output)
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("bounded DDS header"),
    )
}

fn extract(
    install_root: &Path,
    evidence: &Path,
    requested_output: &Path,
    limit: usize,
) -> Result<(ExtractionReport, ExtractionCompletion)> {
    let install = fs::canonicalize(install_root)?;
    let evidence = fs::canonicalize(evidence)?;
    let output = output_directory(&install, &evidence, requested_output)?;
    let (manifest, manifest_sha256) = analysis_manifest(&evidence)?;
    let selected = select_textures(&manifest, limit)?;
    let data = fs::canonicalize(install.join("Data"))?;
    fs::create_dir(&output)?;
    fs::create_dir(output.join("raw"))?;

    let archives: BTreeMap<_, _> = manifest
        .archives
        .iter()
        .map(|row| (row.archive.as_str(), row))
        .collect();
    let mut grouped = BTreeMap::<String, Vec<usize>>::new();
    for (index, selection) in selected.iter().enumerate() {
        let candidate = &selection.texture.candidates[0];
        validate_archive_name(&candidate.archive)?;
        let source = archives.get(candidate.archive.as_str()).ok_or_else(|| {
            Error::Unsupported(format!(
                "candidate archive {} is absent from the checked analysis manifest",
                candidate.archive
            ))
        })?;
        if source.error.is_some()
            || source.archive_sha256.as_deref() != Some(&candidate.archive_sha256)
        {
            return Err(Error::Unsupported(format!(
                "candidate archive {} has inconsistent analysis provenance",
                candidate.archive
            )));
        }
        grouped
            .entry(candidate.archive.clone())
            .or_default()
            .push(index);
    }

    let mut report_rows: Vec<Option<ExtractedTexture>> =
        (0..selected.len()).map(|_| None).collect();
    let mut stable_archives = Vec::with_capacity(grouped.len());
    let mut total_bytes = 0u64;
    for (archive_name, indices) in grouped {
        let path = data.join(&archive_name);
        let source = archives[archive_name.as_str()];
        let expected_sha = source.archive_sha256.as_deref().ok_or_else(|| {
            Error::Unsupported(format!("archive {archive_name} has no source fingerprint"))
        })?;
        let digest_before = census::hash_file(&path)?;
        if digest_before != expected_sha {
            return Err(Error::Unsupported(format!(
                "archive {archive_name} differs from its frozen candidate fingerprint"
            )));
        }
        let archive = Ba2::open(&path, Limits::default())?;
        for index in indices {
            let selection = &selected[index];
            let candidate = &selection.texture.candidates[0];
            let entry = archive
                .entries()
                .get(candidate.entry_index)
                .ok_or_else(|| {
                    Error::Unsupported(format!(
                        "{}: candidate entry index {} is out of range",
                        archive_name, candidate.entry_index
                    ))
                })?;
            if candidate.member_name_bytes_hex != hex(entry.name().as_bytes()) {
                return Err(Error::Unsupported(format!(
                    "{}: candidate member name changed at index {}",
                    archive_name, candidate.entry_index
                )));
            }
            let actual_path = AssetPath::new(entry.name().as_bytes())?;
            let expected_path = AssetPath::new(selection.texture.canonical_path.as_bytes())?;
            if actual_path != expected_path {
                return Err(Error::Unsupported(format!(
                    "{}: candidate path does not match authored material path {}",
                    archive_name, selection.texture.canonical_path
                )));
            }
            let header = archive.texture(candidate.entry_index).ok_or_else(|| {
                Error::Unsupported(format!(
                    "{}: candidate {} lacks the admitted BA2 DX10 texture header",
                    archive_name, selection.texture.canonical_path
                ))
            })?;
            let bytes = archive.read(candidate.entry_index)?;
            if bytes.len() as u64 > MAX_MEMBER_BYTES || bytes.len() < 128 || &bytes[..4] != b"DDS "
            {
                return Err(Error::Unsupported(format!(
                    "{}: candidate {} is not a bounded reconstructed DDS",
                    archive_name, selection.texture.canonical_path
                )));
            }
            if read_u32(&bytes, 4) != 124
                || read_u32(&bytes, 12) != u32::from(header.height)
                || read_u32(&bytes, 16) != u32::from(header.width)
            {
                return Err(Error::Unsupported(format!(
                    "{}: DDS dimensions or fixed header disagree with the BA2 DX10 header",
                    selection.texture.canonical_path
                )));
            }
            total_bytes = total_bytes
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| Error::Unsupported("texture sample byte count overflow".into()))?;
            if total_bytes > MAX_TOTAL_BYTES {
                return Err(Error::Unsupported(format!(
                    "texture sample exceeds {MAX_TOTAL_BYTES} bytes"
                )));
            }
            let file_name = format!("{:04}.dds", index + 1);
            let file_path = output.join("raw").join(&file_name);
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&file_path)?;
            file.write_all(&bytes)?;
            let dds_sha256 = census::sha256(&bytes);
            let material_sources = selection
                .texture
                .references
                .iter()
                .take(4)
                .map(|use_site| MaterialSourceExample {
                    archive: use_site.archive.clone(),
                    entry_index: use_site.material_entry_index,
                    member_name_bytes_hex: use_site.material_member_name_bytes_hex.clone(),
                    kind: use_site.material_kind.clone(),
                    sha256: use_site.material_sha256.clone(),
                })
                .collect();
            report_rows[index] = Some(ExtractedTexture {
                id: index + 1,
                canonical_path: selection.texture.canonical_path.clone(),
                material_fields: selection.fields.clone(),
                material_reference_count: selection.texture.references.len(),
                material_sources,
                candidate_archive: archive_name.clone(),
                candidate_archive_sha256: candidate.archive_sha256.clone(),
                candidate_entry_index: candidate.entry_index,
                candidate_member_name_bytes_hex: candidate.member_name_bytes_hex.clone(),
                texture_header: TextureHeaderEvidence {
                    width: header.width,
                    height: header.height,
                    mip_count: header.mip_count,
                    dxgi_format: header.format,
                    flags: header.flags,
                    tile_mode: header.tile_mode,
                },
                dds_file: format!("raw/{file_name}"),
                dds_bytes: bytes.len() as u64,
                dds_sha256,
            });
        }
        drop(archive);
        let digest_after = census::hash_file(&path)?;
        if digest_after != digest_before {
            return Err(Error::Unsupported(format!(
                "archive {archive_name} changed during texture extraction"
            )));
        }
        stable_archives.push(StableArchive {
            archive: archive_name,
            sha256: digest_before,
            unchanged_before_and_after_read: true,
        });
    }

    let textures: Vec<_> = report_rows
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| Error::Unsupported("texture extraction left an incomplete row".into()))?;
    let report = ExtractionReport {
        schema_version: 1,
        analysis_manifest_sha256: manifest_sha256,
        reference_revision: manifest.reference_revision,
        requested_limit: limit,
        selected_count: textures.len(),
        extracted_bytes: total_bytes,
        texture_sources: stable_archives,
        textures,
        scope: "A deterministic sample of uniquely matched DDS members from physical Data BA2 archives. These files are material texture candidates only; the extraction does not choose a runtime winner, verify loose/VFS inputs, interpret shaders, or claim pixel/rendering parity.",
    };
    let report_bytes = serde_json::to_vec_pretty(&report)?;
    fs::write(output.join("report.json"), &report_bytes)?;
    let completion = ExtractionCompletion {
        schema_version: 1,
        report_sha256: census::sha256(&report_bytes),
        selected_count: report.selected_count,
        extracted_bytes: report.extracted_bytes,
        texture_sources: report.texture_sources.len(),
        complete: true,
        runtime_ready: false,
    };
    fs::write(
        output.join("complete.json"),
        serde_json::to_vec_pretty(&completion)?,
    )?;
    Ok((report, completion))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--help") || args.is_empty() {
        println!(
            "extract-texture-candidates <install-root> <texture-analysis-directory> <new-local-directory> [limit]\nExtracts only uniquely matched direct-BA2 DDS candidates; default limit is {DEFAULT_SELECTIONS}."
        );
        return;
    }
    if !(3..=4).contains(&args.len()) {
        eprintln!("invalid arguments; use --help");
        std::process::exit(1);
    }
    let limit = match args.get(3) {
        Some(value) => value.to_string_lossy().parse::<usize>().unwrap_or(0),
        None => DEFAULT_SELECTIONS,
    };
    match extract(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        limit,
    ) {
        Ok((report, completion)) => {
            if let Err(error) = serde_json::to_writer_pretty(std::io::stdout().lock(), &completion)
            {
                eprintln!("{error}");
                std::process::exit(1);
            }
            println!();
            eprintln!(
                "extracted {} candidate DDS textures ({} bytes) from {} stable BA2 files",
                report.selected_count,
                report.extracted_bytes,
                report.texture_sources.len()
            );
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str, field: &str, candidates: usize) -> TextureRequest {
        TextureRequest {
            canonical_path: path.into(),
            references: vec![TextureUse {
                archive: "Materials.ba2".into(),
                material_entry_index: 1,
                material_member_name_bytes_hex: "00".into(),
                material_kind: "bgsm".into(),
                material_sha256: "0".repeat(64),
                field: field.into(),
            }],
            candidates: (0..candidates)
                .map(|entry_index| Candidate {
                    archive: "Textures.ba2".into(),
                    archive_sha256: "1".repeat(64),
                    entry_index,
                    member_name_bytes_hex: "00".into(),
                })
                .collect(),
        }
    }

    fn manifest(textures: Vec<TextureRequest>) -> AnalysisManifest {
        AnalysisManifest {
            schema_version: 1,
            reference_revision: REFERENCE_REVISION.into(),
            observation_complete: true,
            runtime_ready: false,
            archives_scanned: 0,
            archives_with_errors: 0,
            candidate_members: 0,
            invalid_texture_references: vec![],
            archives: vec![],
            textures,
        }
    }

    #[test]
    fn selection_prioritizes_material_slots_and_skips_ambiguous_or_missing_paths() {
        let rows = manifest(vec![
            request("textures/a_diffuse.dds", "DiffuseTexture", 1),
            request("textures/b_normal.dds", "NormalTexture", 1),
            request("textures/c_ambiguous.dds", "DiffuseTexture", 2),
            request("textures/d_missing.dds", "DiffuseTexture", 0),
            request("textures/e_not_dds.tga", "DiffuseTexture", 1),
        ]);
        let chosen = select_textures(&rows, 10).unwrap();
        let paths: Vec<_> = chosen
            .iter()
            .map(|selection| selection.texture.canonical_path.as_str())
            .collect();
        assert_eq!(paths, ["textures/a_diffuse.dds", "textures/b_normal.dds"]);
        assert!(chosen[0].fields.contains(&"DiffuseTexture".into()));
    }

    #[test]
    fn selection_limit_is_bounded_before_any_archive_work() {
        let rows = manifest(vec![request("textures/a.dds", "DiffuseTexture", 1)]);
        assert!(select_textures(&rows, 0).is_err());
        assert!(select_textures(&rows, MAX_SELECTIONS + 1).is_err());
    }

    #[test]
    fn archive_names_must_be_single_direct_ba2_filenames() {
        for name in [
            "Textures.ba2",
            "../Textures.ba2",
            "dir\\Textures.ba2",
            "C:\\Textures.ba2",
        ] {
            assert_eq!(validate_archive_name(name).is_ok(), name == "Textures.ba2");
        }
        assert!(validate_archive_name("Textures.txt").is_err());
    }

    #[test]
    fn output_is_local_new_and_separate_from_analysis_and_install_inputs() {
        let root =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("local")).unwrap();
        let install = root.path().join("game");
        let evidence = root.path().join("evidence");
        fs::create_dir_all(install.join("Data")).unwrap();
        fs::create_dir(&evidence).unwrap();
        assert!(output_directory(&install, &evidence, &install.join("out")).is_err());
        assert!(output_directory(&install, &evidence, &evidence.join("out")).is_err());
        assert!(output_directory(&install, &evidence, &root.path().join("out")).is_ok());
        assert!(output_directory(&install, &evidence, &PathBuf::from("out")).is_err());
    }

    #[test]
    fn dds_header_validation_keeps_width_height_and_magic_explicit() {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(b"DDS ");
        bytes[4..8].copy_from_slice(&124u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&256u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&512u32.to_le_bytes());
        assert_eq!(read_u32(&bytes, 12), 256);
        assert_eq!(read_u32(&bytes, 16), 512);
        bytes[0] = b'X';
        assert_ne!(&bytes[..4], b"DDS ");
    }
}
