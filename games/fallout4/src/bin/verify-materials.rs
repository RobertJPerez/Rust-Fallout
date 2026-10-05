//! Differentially checks the bounded Rust BGSM/BGEM texture decoder against
//! the pinned offline reference output. It does not produce runtime assets.
use fallout4_prep::{
    Error, Result, census,
    material::{self, FieldValue, Kind, MaterialField},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

const REFERENCE_REVISION: &str = "21411873f17454a2442d6785d533499e14c63adb";
const MAX_MATERIALS: usize = 100_000;
const MAX_MATERIAL_BYTES: u64 = 1024 * 1024;
const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DIAGNOSTICS: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ExtractManifest {
    schema_version: u32,
    profile: String,
    observation_only: bool,
    archives_scanned: usize,
    archives_with_materials: usize,
    bgsm: usize,
    bgem: usize,
    decoded_material_bytes: u64,
    materials: Vec<ExtractMaterial>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ExtractMaterial {
    archive: String,
    archive_sha256: String,
    entry_index: usize,
    member_name_bytes_hex: String,
    kind: String,
    bytes: usize,
    sha256: String,
    extracted_file: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ExtractCompletion {
    schema_version: u32,
    manifest_sha256: String,
    archives_scanned: usize,
    archives_with_materials: usize,
    bgsm: usize,
    bgem: usize,
    decoded_material_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleSummary {
    reference_revision: String,
    manifest_sha256: String,
    results_sha256: String,
    bgsm: usize,
    bgem: usize,
    parsed: usize,
    json_parsed: usize,
    trailing_bytes: usize,
    errors: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleRow {
    archive: String,
    archive_sha256: String,
    entry_index: usize,
    member_name_bytes_hex: String,
    kind: String,
    bytes: usize,
    sha256: String,
    extracted_file: String,
    state: String,
    error: Option<String>,
    version: Option<u32>,
    consumed: usize,
    extracted_sha256: String,
    textures: Vec<OracleTexture>,
    fields: Vec<OracleField>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleTexture {
    field: String,
    value: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OracleField {
    name: String,
    kind: String,
    value: Value,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u32,
    reference_revision: String,
    extraction_manifest_sha256: String,
    oracle_results_sha256: String,
    materials_compared: usize,
    binary_version_two: usize,
    json_material_documents: usize,
    texture_slots_compared: usize,
    scalar_fields_compared: usize,
    scalar_field_kinds: BTreeMap<String, usize>,
    field_ledger_sha256: String,
    noncanonical_boolean_bytes: usize,
    mismatch_count: usize,
    diagnostics: Vec<String>,
    matches_reference: bool,
    runtime_ready: bool,
    scope: &'static str,
}

#[derive(Debug, Serialize)]
struct Completion {
    schema_version: u32,
    report_sha256: String,
    extraction_manifest_sha256: String,
    oracle_results_sha256: String,
    materials_compared: usize,
    scalar_fields_compared: usize,
    field_ledger_sha256: String,
    mismatch_count: usize,
    matches_reference: bool,
    runtime_ready: bool,
}

#[derive(Serialize)]
struct FieldLedgerRow<'a> {
    extracted_file: &'a str,
    source_sha256: &'a str,
    fields: &'a [MaterialField],
}

fn read_stable(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let before = fs::metadata(path)?;
    if !before.is_file() || before.len() > limit {
        return Err(Error::Unsupported(format!(
            "{}: input type or byte budget exceeded",
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

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn field_matches(rust: &MaterialField, reference: &OracleField) -> bool {
    if rust.name != reference.name {
        return false;
    }
    match &rust.value {
        FieldValue::Bool { decoded, .. } => {
            reference.kind == "bool" && reference.value.as_bool() == Some(*decoded)
        }
        FieldValue::U8(value) => {
            reference.kind == "u8" && reference.value.as_u64() == Some(u64::from(*value))
        }
        FieldValue::F32Bits(bits) => {
            reference.kind == "f32_bits" && reference.value.as_u64() == Some(u64::from(*bits))
        }
        FieldValue::String(value) => {
            reference.kind == "string" && reference.value.as_str() == Some(value)
        }
        FieldValue::Enum { decoded, .. } => {
            reference.kind == "enum" && reference.value.as_str() == Some(decoded)
        }
        FieldValue::ColorRgb { packed_rgb, .. } => {
            reference.kind == "color_rgb"
                && reference.value.as_u64() == Some(u64::from(*packed_rgb))
        }
    }
}

fn field_kind(field: &MaterialField) -> &'static str {
    match &field.value {
        FieldValue::Bool { .. } => "bool",
        FieldValue::U8(_) => "u8",
        FieldValue::F32Bits(_) => "f32_bits",
        FieldValue::String(_) => "string",
        FieldValue::Enum { .. } => "enum",
        FieldValue::ColorRgb { .. } => "color_rgb",
    }
}

fn safe_filename(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

fn output_directory(install: &Path, inputs: &[&Path], requested: &Path) -> Result<PathBuf> {
    let install = fs::canonicalize(install)?;
    let local = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("local"))?;
    let mut protected = Vec::new();
    for input in inputs {
        protected.push(fs::canonicalize(input)?);
    }
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)?;
    let name = requested
        .file_name()
        .ok_or_else(|| Error::Unsupported("new verification directory name required".into()))?;
    let output = parent.join(name);
    if output.starts_with(&install)
        || !output.starts_with(&local)
        || protected.iter().any(|root| output.starts_with(root))
    {
        return Err(Error::Unsupported(
            "verification output must be inside ignored local/ and outside the retail install and input evidence directories".into(),
        ));
    }
    Ok(output)
}

fn verify(
    install: &Path,
    extraction: &Path,
    oracle: &Path,
    output_requested: &Path,
) -> Result<(Report, Completion)> {
    let extraction = fs::canonicalize(extraction)?;
    let oracle = fs::canonicalize(oracle)?;
    let output = output_directory(install, &[&extraction, &oracle], output_requested)?;

    let manifest_bytes = read_stable(&extraction.join("manifest.json"), MAX_EVIDENCE_BYTES)?;
    let extract_marker_bytes = read_stable(&extraction.join("complete.json"), 1024 * 1024)?;
    let manifest_hash = hash(&manifest_bytes);
    let manifest: ExtractManifest = serde_json::from_slice(&manifest_bytes)?;
    let extract_marker: ExtractCompletion = serde_json::from_slice(&extract_marker_bytes)?;
    if manifest.schema_version != 1
        || manifest.profile != "fo4-original"
        || !manifest.observation_only
        || manifest.materials.len() > MAX_MATERIALS
        || manifest.materials.len() != manifest.bgsm + manifest.bgem
        || extract_marker.schema_version != 1
        || extract_marker.manifest_sha256 != manifest_hash
        || extract_marker.archives_scanned != manifest.archives_scanned
        || extract_marker.archives_with_materials != manifest.archives_with_materials
        || extract_marker.bgsm != manifest.bgsm
        || extract_marker.bgem != manifest.bgem
        || extract_marker.decoded_material_bytes != manifest.decoded_material_bytes
    {
        return Err(Error::Unsupported(
            "material extraction manifest and completion marker disagree".into(),
        ));
    }

    let oracle_summary_bytes = read_stable(&oracle.join("complete.json"), 1024 * 1024)?;
    let oracle_rows_bytes =
        read_stable(&oracle.join("material-reference.jsonl"), MAX_EVIDENCE_BYTES)?;
    let summary: OracleSummary = serde_json::from_slice(&oracle_summary_bytes)?;
    let results_hash = hash(&oracle_rows_bytes);
    if summary.reference_revision != REFERENCE_REVISION
        || summary.manifest_sha256 != manifest_hash
        || summary.results_sha256 != results_hash
        || summary.bgsm != manifest.bgsm
        || summary.bgem != manifest.bgem
        || summary.parsed + summary.json_parsed != manifest.materials.len()
        || summary.trailing_bytes != 0
        || summary.errors != 0
    {
        return Err(Error::Unsupported(
            "oracle summary does not match this complete extraction".into(),
        ));
    }

    let mut oracle_by_file = BTreeMap::<String, OracleRow>::new();
    for line in oracle_rows_bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let row: OracleRow = serde_json::from_slice(line)?;
        if !safe_filename(&row.extracted_file)
            || oracle_by_file
                .insert(row.extracted_file.clone(), row)
                .is_some()
        {
            return Err(Error::Unsupported(
                "oracle file identity is unsafe or duplicated".into(),
            ));
        }
    }
    if oracle_by_file.len() != manifest.materials.len() {
        return Err(Error::Unsupported(
            "oracle row count differs from extracted material count".into(),
        ));
    }

    let mut binary_version_two = 0;
    let mut json_material_documents = 0;
    let mut texture_slots_compared = 0;
    let mut scalar_fields_compared = 0;
    let mut scalar_field_kinds = BTreeMap::<String, usize>::new();
    let mut field_ledger = Vec::new();
    let mut noncanonical_boolean_bytes = 0;
    let mut diagnostics = Vec::new();
    let mut mismatch_count = 0usize;
    for source in &manifest.materials {
        let mut mismatch = |detail: String| {
            mismatch_count += 1;
            if diagnostics.len() < MAX_DIAGNOSTICS {
                diagnostics.push(format!("{}: {detail}", source.extracted_file));
            }
        };
        if !safe_filename(&source.extracted_file) {
            mismatch("unsafe extracted filename".into());
            continue;
        }
        let Some(reference) = oracle_by_file.remove(&source.extracted_file) else {
            mismatch("oracle row is absent".into());
            continue;
        };
        if reference.archive != source.archive
            || reference.archive_sha256 != source.archive_sha256
            || reference.entry_index != source.entry_index
            || reference.member_name_bytes_hex != source.member_name_bytes_hex
            || reference.kind != source.kind
            || reference.bytes != source.bytes
            || reference.sha256 != source.sha256
            || reference.state != "parsed"
            || reference.error.is_some()
            || reference.extracted_sha256 != source.sha256
        {
            mismatch("archive/member identity, hash, or reference state differs".into());
            continue;
        }
        let bytes = match read_stable(&extraction.join(&source.extracted_file), MAX_MATERIAL_BYTES)
        {
            Ok(bytes) => bytes,
            Err(error) => {
                mismatch(error.to_string());
                continue;
            }
        };
        if bytes.len() != source.bytes || hash(&bytes) != source.sha256 {
            mismatch("extracted payload hash differs from manifest".into());
            continue;
        }
        let Some(kind) = Kind::from_extension(&source.kind) else {
            mismatch("unsupported material kind".into());
            continue;
        };
        let decoded = match material::parse(&bytes, kind, &source.extracted_file) {
            Ok(material) => material,
            Err(error) => {
                mismatch(format!("Rust decoder: {error}"));
                continue;
            }
        };
        if decoded.version.is_some() {
            binary_version_two += 1;
        } else {
            json_material_documents += 1;
        }
        noncanonical_boolean_bytes += decoded.noncanonical_boolean_bytes;
        texture_slots_compared += decoded.textures.len();
        scalar_fields_compared += decoded.fields.len();
        for field in &decoded.fields {
            *scalar_field_kinds
                .entry(field_kind(field).to_owned())
                .or_default() += 1;
        }
        if decoded.fields.len() != reference.fields.len()
            || decoded
                .fields
                .iter()
                .zip(&reference.fields)
                .any(|(rust, expected)| !field_matches(rust, expected))
        {
            mismatch(format!(
                "typed source fields differ: Rust {} fields vs reference {} fields",
                decoded.fields.len(),
                reference.fields.len()
            ));
        }
        serde_json::to_writer(
            &mut field_ledger,
            &FieldLedgerRow {
                extracted_file: &source.extracted_file,
                source_sha256: &source.sha256,
                fields: &decoded.fields,
            },
        )?;
        field_ledger.push(b'\n');
        if field_ledger.len() as u64 > MAX_EVIDENCE_BYTES {
            return Err(Error::Unsupported(
                "material field ledger exceeds evidence budget".into(),
            ));
        }
        let rust_textures: Vec<_> = decoded
            .textures
            .iter()
            .map(|texture| (texture.field, texture.value.as_str()))
            .collect();
        let reference_textures: Vec<_> = reference
            .textures
            .iter()
            .map(|texture| (texture.field.as_str(), texture.value.as_str()))
            .collect();
        if reference.consumed != decoded.bytes_consumed
            || reference.version != decoded.version
            || rust_textures != reference_textures
        {
            mismatch(format!(
                "reference mismatch: consumed {} vs {}, version {:?} vs {:?}, texture slots {} vs {}",
                reference.consumed,
                decoded.bytes_consumed,
                reference.version,
                decoded.version,
                reference_textures.len(),
                rust_textures.len()
            ));
        }
    }
    if !oracle_by_file.is_empty() {
        mismatch_count += oracle_by_file.len();
        if diagnostics.len() < MAX_DIAGNOSTICS {
            diagnostics.push(format!(
                "{} oracle rows have no extraction member",
                oracle_by_file.len()
            ));
        }
    }
    let matches_reference = mismatch_count == 0
        && binary_version_two == summary.parsed
        && json_material_documents == summary.json_parsed;
    if !matches_reference && mismatch_count == 0 {
        mismatch_count = 1;
        diagnostics.push("aggregate binary/JSON format counts differ from the reference".into());
    }
    let field_ledger_hash = hash(&field_ledger);
    let report = Report {
        schema_version: 1,
        reference_revision: summary.reference_revision,
        extraction_manifest_sha256: manifest_hash,
        oracle_results_sha256: results_hash,
        materials_compared: manifest.materials.len(),
        binary_version_two,
        json_material_documents,
        texture_slots_compared,
        scalar_fields_compared,
        scalar_field_kinds,
        field_ledger_sha256: field_ledger_hash.clone(),
        noncanonical_boolean_bytes,
        mismatch_count,
        diagnostics,
        matches_reference,
        runtime_ready: false,
        scope: "Binary v2 Rust material framing and named source fields compared with the pinned offline parser. Float bit patterns, bool source bytes and RGB source bits are retained; the reference RGB packed value is compared. This does not establish shader interpretation, rendering, active source selection, or gameplay.",
    };
    let report_bytes = serde_json::to_vec_pretty(&report)?;
    fs::create_dir(&output)?;
    fs::write(output.join("fields.jsonl"), &field_ledger)?;
    fs::write(output.join("report.json"), &report_bytes)?;
    let completion = Completion {
        schema_version: 1,
        report_sha256: census::sha256(&report_bytes),
        extraction_manifest_sha256: report.extraction_manifest_sha256.clone(),
        oracle_results_sha256: report.oracle_results_sha256.clone(),
        materials_compared: report.materials_compared,
        scalar_fields_compared: report.scalar_fields_compared,
        field_ledger_sha256: report.field_ledger_sha256.clone(),
        mismatch_count: report.mismatch_count,
        matches_reference: report.matches_reference,
        runtime_ready: false,
    };
    fs::write(
        output.join("complete.json"),
        serde_json::to_vec_pretty(&completion)?,
    )?;
    Ok((report, completion))
}

fn main() {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--help") || args.is_empty() {
        println!(
            "verify-materials <install-root> <material-extraction-directory> <oracle-evidence-directory> <new-local-directory>\nExit 2 means Rust/reference material observations differ."
        );
        return;
    }
    if args.len() != 4 {
        eprintln!("invalid arguments; use --help");
        std::process::exit(1);
    }
    match verify(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
    ) {
        Ok((report, completion)) => {
            match serde_json::to_writer_pretty(std::io::stdout().lock(), &completion) {
                Ok(()) => println!(),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
            if !report.matches_reference {
                std::process::exit(2);
            }
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

    #[test]
    fn evidence_member_names_cannot_escape_the_extraction_root() {
        for name in [
            "",
            "..",
            "../outside.bgsm",
            "nested/file.bgsm",
            "C:\\outside.bgsm",
        ] {
            assert!(!safe_filename(name), "{name:?}");
        }
        assert!(safe_filename("archive-000-member-000001.bgsm"));
    }

    #[test]
    fn comparison_output_is_confined_to_local_and_outside_inputs() {
        let root =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("local")).unwrap();
        let install = root.path().join("game");
        let extraction = root.path().join("extract");
        let oracle = root.path().join("oracle");
        fs::create_dir_all(install.join("Data")).unwrap();
        fs::create_dir(&extraction).unwrap();
        fs::create_dir(&oracle).unwrap();
        assert!(
            output_directory(&install, &[&extraction, &oracle], &install.join("report")).is_err()
        );
        assert!(
            output_directory(&install, &[&extraction, &oracle], &oracle.join("report")).is_err()
        );
        assert!(
            output_directory(
                &install,
                &[&extraction, &oracle],
                &root.path().join("report")
            )
            .is_ok()
        );
    }
}
