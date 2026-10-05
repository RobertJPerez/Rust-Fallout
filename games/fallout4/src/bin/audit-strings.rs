//! Bounded physical audit of Fallout 4 localization tables inside direct Data BA2s.
//! Extracted table bytes and detailed key indexes are written only under ignored local/.
use dream_archive::ByteSlice;
use fallout4_prep::{
    archive::{Ba2, Limits},
    census,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufWriter, Write},
    path::{Component, Path, PathBuf},
};

const MAX_TABLE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TABLE_ENTRIES: usize = 2_000_000;

#[derive(Debug, Deserialize)]
struct Census {
    files: Vec<CensusFile>,
    archives: Vec<CensusArchive>,
}

#[derive(Debug, Deserialize)]
struct CensusFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct CensusArchive {
    name: String,
    version: u32,
    format: String,
    entries: usize,
    extensions: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize)]
struct TableRow {
    table_id: usize,
    archive: String,
    archive_sha256: String,
    archive_version: u32,
    archive_format: String,
    member_index: usize,
    member_name_bytes_hex: String,
    extension: String,
    extracted_path: String,
    extracted_bytes: usize,
    extracted_sha256: String,
    keys: usize,
    data_bytes: usize,
    trailing_bytes: usize,
    entries_digest_sha256: String,
    missing_terminators: usize,
}

#[derive(Debug, Serialize)]
struct KeyRow {
    table_id: usize,
    key: u32,
    offset: u32,
    value_bytes: usize,
    value_sha256: String,
    terminator_found: bool,
}

#[derive(Debug, Serialize)]
struct ArchiveRow {
    archive: String,
    expected_sha256: String,
    before_sha256: String,
    after_sha256: String,
    expected_bytes: u64,
    actual_bytes: u64,
    archive_version: u32,
    archive_format: String,
    entries: usize,
    localization_tables: usize,
}

#[derive(Debug, Serialize)]
struct Manifest {
    schema: u32,
    status: &'static str,
    installation_data_root: String,
    source_census_sha256: String,
    localized_archives: usize,
    archive_entries_scanned: usize,
    localization_tables: usize,
    localization_keys: usize,
    table_extensions: BTreeMap<String, usize>,
    table_rows_sha256: String,
    key_rows_sha256: String,
    archives: Vec<ArchiveRow>,
    limits: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct Completion {
    schema: u32,
    manifest_sha256: String,
    table_rows: usize,
    key_rows: usize,
    source_archives_rehashed_before_and_after: usize,
    localization_tables: usize,
    localization_keys: usize,
    complete: bool,
    runtime_ready: bool,
}

#[derive(Debug)]
struct ParsedTable {
    keys: Vec<KeyRow>,
    data_bytes: usize,
    trailing_bytes: usize,
    entries_digest_sha256: String,
    missing_terminators: usize,
}

fn bad(message: impl Into<String>) -> Box<dyn std::error::Error> {
    message.into().into()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn parse_localization_table(
    extension: &str,
    bytes: &[u8],
    table_id: usize,
) -> Result<ParsedTable, Box<dyn std::error::Error>> {
    if bytes.len() > MAX_TABLE_BYTES {
        return Err(bad(format!(
            "table {table_id} exceeds {MAX_TABLE_BYTES} bytes"
        )));
    }
    if bytes.len() < 8 {
        return Err(bad(format!("table {table_id} has a truncated header")));
    }
    let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let data_bytes = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if count > MAX_TABLE_ENTRIES {
        return Err(bad(format!(
            "table {table_id} exceeds the key-count budget"
        )));
    }
    let index_bytes = count
        .checked_mul(8)
        .ok_or_else(|| bad(format!("table {table_id} index length overflow")))?;
    let data_start = 8usize
        .checked_add(index_bytes)
        .ok_or_else(|| bad(format!("table {table_id} data offset overflow")))?;
    let declared_end = data_start
        .checked_add(data_bytes)
        .ok_or_else(|| bad(format!("table {table_id} declared data length overflow")))?;
    if data_start > bytes.len() || declared_end > bytes.len() {
        return Err(bad(format!(
            "table {table_id} index or declared data exceeds input"
        )));
    }
    let data = &bytes[data_start..declared_end];
    let trailing_bytes = bytes.len() - declared_end;
    let length_prepended = matches!(extension, "dlstrings" | "ilstrings");
    if extension != "strings" && !length_prepended {
        return Err(bad(format!(
            "table {table_id} has unsupported extension {extension}"
        )));
    }

    let mut seen = BTreeSet::new();
    let mut keys = Vec::with_capacity(count);
    let mut digest_rows = Vec::with_capacity(count);
    let mut missing_terminators = 0usize;
    for index in 0..count {
        let pos = 8 + index * 8;
        let key = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        let offset = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap());
        if !seen.insert(key) {
            return Err(bad(format!(
                "table {table_id} contains duplicate key {key}"
            )));
        }
        let start = usize::try_from(offset)?;
        if start >= data.len() {
            return Err(bad(format!(
                "table {table_id} key {key} offset exceeds string data"
            )));
        }
        let (value, terminator_found) = if length_prepended {
            let prefix_end = start
                .checked_add(4)
                .ok_or_else(|| bad(format!("table {table_id} key {key} length prefix overflow")))?;
            let prefix = data.get(start..prefix_end).ok_or_else(|| {
                bad(format!(
                    "table {table_id} key {key} has a truncated length prefix"
                ))
            })?;
            let declared = u32::from_le_bytes(prefix.try_into().unwrap()) as usize;
            let value_start = prefix_end;
            let value_end = value_start
                .checked_add(declared)
                .ok_or_else(|| bad(format!("table {table_id} key {key} string length overflow")))?;
            let framed_value = data
                .get(value_start..value_end)
                .ok_or_else(|| bad(format!("table {table_id} key {key} string exceeds data")))?;
            match framed_value.iter().position(|byte| *byte == 0) {
                Some(end) => (&framed_value[..end], true),
                None => (framed_value, false),
            }
        } else {
            let framed_value = &data[start..];
            match framed_value.iter().position(|byte| *byte == 0) {
                Some(end) => (&framed_value[..end], true),
                None => (framed_value, false),
            }
        };
        if !terminator_found {
            missing_terminators += 1;
        }
        let value_sha256 = census::sha256(value);
        digest_rows.push((key, offset, value.len() as u64, value_sha256.clone()));
        keys.push(KeyRow {
            table_id,
            key,
            offset,
            value_bytes: value.len(),
            value_sha256,
            terminator_found,
        });
    }
    digest_rows.sort_by_key(|row| row.0);
    let mut digest = Sha256::new();
    for (key, offset, length, value_hash) in digest_rows {
        digest.update(key.to_le_bytes());
        digest.update(offset.to_le_bytes());
        digest.update(length.to_le_bytes());
        digest.update(value_hash.as_bytes());
    }
    Ok(ParsedTable {
        keys,
        data_bytes,
        trailing_bytes,
        entries_digest_sha256: format!("{:x}", digest.finalize()),
        missing_terminators,
    })
}

fn ensure_local_output(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if path.components().next() != Some(Component::Normal(std::ffi::OsStr::new("local")))
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        return Err(bad("output path must be inside local/"));
    }
    if path.exists() {
        return Err(bad(
            "output directory already exists; choose a fresh local/ directory",
        ));
    }
    fs::create_dir_all(path)?;
    fs::canonicalize(path).map_err(Into::into)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let installation =
        PathBuf::from(args.next().ok_or(
            "usage: audit-strings <Fallout-4-root> <source-census.json> <local-output-dir>",
        )?);
    let census_path =
        PathBuf::from(args.next().ok_or(
            "usage: audit-strings <Fallout-4-root> <source-census.json> <local-output-dir>",
        )?);
    let output_arg =
        PathBuf::from(args.next().ok_or(
            "usage: audit-strings <Fallout-4-root> <source-census.json> <local-output-dir>",
        )?);
    if args.next().is_some() {
        return Err(bad("too many arguments"));
    }
    let installation = fs::canonicalize(installation)?;
    let data_root = installation.join("Data");
    if !data_root.is_dir() {
        return Err(bad(format!(
            "missing Data directory under {}",
            installation.display()
        )));
    }
    let census_bytes = fs::read(census_path)?;
    let source_census_sha256 = census::sha256(&census_bytes);
    let census: Census = serde_json::from_slice(&census_bytes)?;
    let expected_files: BTreeMap<_, _> = census
        .files
        .iter()
        .filter_map(|row| {
            Path::new(&row.path)
                .file_name()
                .map(|name| (name.to_string_lossy().to_lowercase(), row))
        })
        .collect();
    let expected_archives: BTreeMap<_, _> = census
        .archives
        .iter()
        .map(|row| (row.name.to_lowercase(), row))
        .collect();
    let expected_localization_tables: usize = census
        .archives
        .iter()
        .map(|row| {
            ["strings", "dlstrings", "ilstrings"]
                .iter()
                .map(|extension| row.extensions.get(*extension).copied().unwrap_or_default())
                .sum::<usize>()
        })
        .sum();
    let expected_localized_archives = census
        .archives
        .iter()
        .filter(|row| {
            ["strings", "dlstrings", "ilstrings"]
                .iter()
                .any(|extension| row.extensions.get(*extension).copied().unwrap_or_default() != 0)
        })
        .count();
    let mut archives: Vec<PathBuf> = fs::read_dir(&data_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ba2"))
        })
        .collect();
    archives.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });
    let output = ensure_local_output(&output_arg)?;
    let extracted_root = output.join("tables");
    fs::create_dir(&extracted_root)?;
    let mut table_writer = BufWriter::new(fs::File::create(output.join("tables.jsonl"))?);
    let mut key_writer = BufWriter::new(fs::File::create(output.join("keys.jsonl"))?);
    let mut archive_rows = Vec::new();
    let mut ext_counts = BTreeMap::<String, usize>::new();
    let mut archive_entries_scanned = 0usize;
    let mut key_count = 0usize;
    let mut table_id = 0usize;
    let mut localized_archives = 0usize;

    for archive_path in archives {
        let archive_name = archive_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let census_file = expected_files
            .get(&archive_name.to_lowercase())
            .ok_or_else(|| bad(format!("{} is missing from source census", archive_name)))?;
        let expected_archive = expected_archives
            .get(&archive_name.to_lowercase())
            .ok_or_else(|| bad(format!("{} is missing from archive census", archive_name)))?;
        let before_sha256 = census::hash_file(&archive_path)?;
        let actual_bytes = fs::metadata(&archive_path)?.len();
        if before_sha256 != census_file.sha256 || actual_bytes != census_file.bytes {
            return Err(bad(format!(
                "{} no longer matches the frozen physical census",
                archive_name
            )));
        }
        let ba2 = Ba2::open(&archive_path, Limits::default())?;
        let info = ba2.info();
        if info.version as u32 != expected_archive.version
            || format!("{:?}", info.format) != expected_archive.format
        {
            return Err(bad(format!(
                "{} archive version or format differs from census",
                archive_name
            )));
        }
        let mut local_table_count = 0usize;
        let mut local_ext_counts = BTreeMap::<String, usize>::new();
        for (member_index, entry) in ba2.entries().iter().enumerate() {
            archive_entries_scanned += 1;
            let member_name = entry.name().as_bytes();
            let extension = member_name
                .rsplit(|byte| *byte == b'.')
                .next()
                .unwrap_or_default()
                .iter()
                .map(u8::to_ascii_lowercase)
                .collect::<Vec<_>>();
            let extension = match extension.as_slice() {
                b"strings" => "strings",
                b"dlstrings" => "dlstrings",
                b"ilstrings" => "ilstrings",
                _ => continue,
            };
            let extracted = ba2.read(member_index)?;
            let parsed = parse_localization_table(extension, &extracted, table_id)?;
            if parsed.missing_terminators != 0 {
                return Err(bad(format!(
                    "table {table_id} has {} unterminated values",
                    parsed.missing_terminators
                )));
            }
            let extracted_sha256 = census::sha256(&extracted);
            let rel_path = format!("tables/{table_id:06}.bin");
            fs::write(output.join(&rel_path), &extracted)?;
            let row = TableRow {
                table_id,
                archive: archive_name.clone(),
                archive_sha256: before_sha256.clone(),
                archive_version: info.version as u32,
                archive_format: expected_archive.format.clone(),
                member_index,
                member_name_bytes_hex: hex(member_name),
                extension: extension.to_owned(),
                extracted_path: rel_path,
                extracted_bytes: extracted.len(),
                extracted_sha256,
                keys: parsed.keys.len(),
                data_bytes: parsed.data_bytes,
                trailing_bytes: parsed.trailing_bytes,
                entries_digest_sha256: parsed.entries_digest_sha256,
                missing_terminators: parsed.missing_terminators,
            };
            serde_json::to_writer(&mut table_writer, &row)?;
            table_writer.write_all(b"\n")?;
            for key in &parsed.keys {
                serde_json::to_writer(&mut key_writer, key)?;
                key_writer.write_all(b"\n")?;
            }
            key_count += parsed.keys.len();
            table_id += 1;
            local_table_count += 1;
            *local_ext_counts.entry(extension.to_owned()).or_default() += 1;
            *ext_counts.entry(extension.to_owned()).or_default() += 1;
        }
        for extension in ["strings", "dlstrings", "ilstrings"] {
            let actual = local_ext_counts.get(extension).copied().unwrap_or_default();
            let expected = expected_archive
                .extensions
                .get(extension)
                .copied()
                .unwrap_or_default();
            if actual != expected {
                return Err(bad(format!(
                    "{} {extension} count changed from census",
                    archive_name
                )));
            }
        }
        if local_table_count != 0 {
            localized_archives += 1;
            let after_sha256 = census::hash_file(&archive_path)?;
            if before_sha256 != after_sha256 {
                return Err(bad(format!("{} changed while being scanned", archive_name)));
            }
            archive_rows.push(ArchiveRow {
                archive: archive_name,
                expected_sha256: census_file.sha256.clone(),
                before_sha256,
                after_sha256,
                expected_bytes: census_file.bytes,
                actual_bytes,
                archive_version: info.version as u32,
                archive_format: expected_archive.format.clone(),
                entries: expected_archive.entries,
                localization_tables: local_table_count,
            });
        }
    }
    table_writer.flush()?;
    key_writer.flush()?;
    let table_rows_sha256 = census::hash_file(&output.join("tables.jsonl"))?;
    let key_rows_sha256 = census::hash_file(&output.join("keys.jsonl"))?;
    if table_id != expected_localization_tables {
        return Err(bad(format!(
            "found {table_id} localized tables; expected {expected_localization_tables} from the frozen census"
        )));
    }
    if archive_rows.len() != expected_localized_archives {
        return Err(bad(format!(
            "found {} archives with localization tables; frozen census expects {expected_localized_archives}",
            archive_rows.len()
        )));
    }
    let manifest = Manifest {
        schema: 1,
        status: "physical-localization-table-census-not-language-activation",
        installation_data_root: data_root.display().to_string(),
        source_census_sha256,
        localized_archives,
        archive_entries_scanned,
        localization_tables: table_id,
        localization_keys: key_count,
        table_extensions: ext_counts,
        table_rows_sha256,
        key_rows_sha256,
        archives: archive_rows,
        limits: vec![
            "archives are read-only physical Data files; this does not establish active plugins or archive precedence",
            "localization payload bytes stay under ignored local/; reports include hashes and lengths, not decoded text",
            "string tables are structurally parsed and compared with Mutagen; no encoding, translation correctness, or game lookup behavior is asserted",
            "loose files, VFS deployment, profile language, and runtime string fallback remain unobserved",
        ],
    };
    let manifest_path = output.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    let completion = Completion {
        schema: 1,
        manifest_sha256: census::hash_file(&manifest_path)?,
        table_rows: table_id,
        key_rows: key_count,
        source_archives_rehashed_before_and_after: localized_archives,
        localization_tables: table_id,
        localization_keys: key_count,
        complete: true,
        runtime_ready: false,
    };
    fs::write(
        output.join("complete.json"),
        serde_json::to_vec_pretty(&completion)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&completion)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(extension: &str, keys: &[(u32, u32)], data: &[u8], trailer: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend((keys.len() as u32).to_le_bytes());
        bytes.extend((data.len() as u32).to_le_bytes());
        for (key, offset) in keys {
            bytes.extend(key.to_le_bytes());
            bytes.extend(offset.to_le_bytes());
        }
        bytes.extend(data);
        bytes.extend(trailer);
        assert!(matches!(extension, "strings" | "dlstrings" | "ilstrings"));
        bytes
    }

    #[test]
    fn normal_table_reads_null_terminated_bytes_and_reports_trailer() {
        let bytes = table("strings", &[(17, 0)], b"abc\0", b"x");
        let parsed = parse_localization_table("strings", &bytes, 3).unwrap();
        assert_eq!(parsed.keys.len(), 1);
        assert_eq!(parsed.keys[0].key, 17);
        assert_eq!(parsed.keys[0].value_bytes, 3);
        assert!(parsed.keys[0].terminator_found);
        assert_eq!(parsed.trailing_bytes, 1);
    }

    #[test]
    fn length_prepended_table_trims_at_first_null_like_mutagen() {
        let mut data = Vec::new();
        data.extend(5u32.to_le_bytes());
        data.extend(b"xy\0z\0");
        let bytes = table("dlstrings", &[(9, 0)], &data, b"");
        let parsed = parse_localization_table("dlstrings", &bytes, 4).unwrap();
        assert_eq!(parsed.keys[0].value_bytes, 2);
        assert!(parsed.keys[0].terminator_found);
    }

    #[test]
    fn malformed_headers_indexes_duplicates_offsets_and_strings_fail_closed() {
        for bytes in [&[][..], &[0; 7][..]] {
            assert!(parse_localization_table("strings", bytes, 1).is_err());
        }
        let truncated_index = table("strings", &[(1, 0)], b"", b"")[..15].to_vec();
        assert!(parse_localization_table("strings", &truncated_index, 1).is_err());
        let duplicates = table("strings", &[(1, 0), (1, 0)], b"a\0", b"");
        assert!(parse_localization_table("strings", &duplicates, 1).is_err());
        let bad_offset = table("strings", &[(1, 4)], b"a\0", b"");
        assert!(parse_localization_table("strings", &bad_offset, 1).is_err());
        let missing_nul = table("strings", &[(1, 0)], b"abc", b"");
        assert_eq!(
            parse_localization_table("strings", &missing_nul, 1)
                .unwrap()
                .missing_terminators,
            1
        );
        let short_length = table("ilstrings", &[(1, 0)], &[9, 0, 0, 0, b'a'], b"");
        assert!(parse_localization_table("ilstrings", &short_length, 1).is_err());
    }
}
