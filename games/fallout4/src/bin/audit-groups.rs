use fallout_data::plugin::{self, Event};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Default, Serialize)]
struct KindCount {
    physical_headers: u64,
    empty_headers: u64,
}

#[derive(Default, Serialize)]
struct GroupSummary {
    physical_headers: u64,
    empty_headers: u64,
    maximum_nesting_depth: usize,
    by_depth: BTreeMap<String, u64>,
    by_kind_and_label_bytes: BTreeMap<String, KindCount>,
    #[serde(skip)]
    end_offsets: Vec<u64>,
}

impl GroupSummary {
    fn add(&mut self, offset: u64, size: u32, kind: i32, label: [u8; 4]) -> Result<(), String> {
        while self.end_offsets.last().is_some_and(|end| *end <= offset) {
            self.end_offsets.pop();
        }
        let end = offset
            .checked_add(u64::from(size))
            .ok_or_else(|| format!("group end overflows at {offset}"))?;
        let depth = self.end_offsets.len();
        let empty = size == 24;
        let key = format!("kind={kind}:label_hex={}", hex(label));
        self.physical_headers += 1;
        if empty {
            self.empty_headers += 1;
        }
        self.maximum_nesting_depth = self.maximum_nesting_depth.max(depth);
        *self.by_depth.entry(depth.to_string()).or_default() += 1;
        let entry = self.by_kind_and_label_bytes.entry(key).or_default();
        entry.physical_headers += 1;
        if empty {
            entry.empty_headers += 1;
        }
        self.end_offsets.push(end);
        Ok(())
    }
}

fn hex(bytes: [u8; 4]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Serialize)]
struct Manifest {
    schema: u32,
    status: &'static str,
    plugin: &'static str,
    source_census: String,
    source_census_sha256: String,
    source_sha256: String,
    source_bytes: u64,
    rust_records_including_tes4: u64,
    rust_records_excluding_tes4: u64,
    hedr_declared_count: u64,
    groups: GroupSummary,
    source_rechecked_after_walk: bool,
    retail_file_modified: bool,
    runtime_ready: bool,
    limits: Vec<&'static str>,
}

fn sha256(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut input = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn confined_local(path: &Path, repo: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("evidence paths must be relative paths under local/".into());
    }
    let resolved = repo.join(path);
    let local = repo.join("local").canonicalize()?;
    let parent = resolved.parent().ok_or("evidence path has no parent")?;
    let parent = parent.canonicalize()?;
    if !parent.starts_with(local) {
        return Err("evidence paths must remain under local/".into());
    }
    Ok(resolved)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let install = args
        .next()
        .ok_or("usage: audit-groups <install-root> <proof-dir> <new-local-output-dir>")?;
    let proof = args
        .next()
        .ok_or("usage: audit-groups <install-root> <proof-dir> <new-local-output-dir>")?;
    let output = args
        .next()
        .ok_or("usage: audit-groups <install-root> <proof-dir> <new-local-output-dir>")?;
    if args.next().is_some() {
        return Err("usage: audit-groups <install-root> <proof-dir> <new-local-output-dir>".into());
    }
    let repo = std::env::current_dir()?;
    let install = PathBuf::from(install).canonicalize()?;
    let proof = confined_local(Path::new(&proof), &repo)?;
    let output = confined_local(Path::new(&output), &repo)?;
    if output.exists() {
        return Err("output directory already exists; choose a fresh local/ path".into());
    }
    if output.starts_with(&proof) || proof.starts_with(&output) {
        return Err("proof and output directories must remain separate".into());
    }

    let complete: serde_json::Value =
        serde_json::from_slice(&fs::read(proof.join("complete.json"))?)?;
    if complete.get("status").and_then(serde_json::Value::as_str) != Some("matched") {
        return Err("frozen proof completion status is not matched".into());
    }
    let census_bytes = fs::read(proof.join("census.json"))?;
    let census_hash = format!("{:x}", sha2::Sha256::digest(&census_bytes));
    if complete
        .get("census_sha256")
        .and_then(serde_json::Value::as_str)
        != Some(&census_hash)
    {
        return Err("frozen proof completion marker does not bind its census".into());
    }
    let census: serde_json::Value = serde_json::from_slice(&census_bytes)?;
    let plugin_row = census["plugins"]
        .as_array()
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|row| row["name"].as_str() == Some("Fallout4.esm"))
        })
        .ok_or("Fallout4.esm is absent from the frozen census")?;
    let declared = plugin_row["declared_records_and_groups"]
        .as_u64()
        .ok_or("Fallout4.esm has no HEDR count")?;
    let record_count = plugin_row["records"]
        .as_u64()
        .ok_or("Fallout4.esm has no raw record count")?;
    let expected = census["files"]
        .as_array()
        .and_then(|files| {
            files
                .iter()
                .find(|row| row["path"].as_str() == Some("Data/Fallout4.esm"))
        })
        .ok_or("Fallout4.esm is absent from frozen file fingerprints")?;
    let expected_hash = expected["sha256"]
        .as_str()
        .ok_or("frozen Fallout4.esm hash is missing")?;
    let expected_bytes = expected["bytes"]
        .as_u64()
        .ok_or("frozen Fallout4.esm size is missing")?;
    let source = install.join("Data").join("Fallout4.esm");
    let metadata = fs::symlink_metadata(&source)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != expected_bytes
    {
        return Err("Fallout4.esm is not a regular file matching its frozen length".into());
    }
    let before = sha256(&source)?;
    if before != expected_hash {
        return Err("Fallout4.esm differs from its frozen source census".into());
    }

    let file = File::open(&source)?;
    let mut groups = GroupSummary::default();
    let mut records_seen = 0_u64;
    plugin::visit(
        &mut BufReader::new(file),
        expected_bytes,
        "Fallout4.esm",
        plugin::Limits::default(),
        |event| {
            match event {
                Event::Group(group) => groups
                    .add(group.offset, group.size, group.kind, group.label)
                    .map_err(fallout_data::Error::Unsupported)?,
                Event::Record(_) => records_seen += 1,
            }
            Ok(())
        },
    )?;
    if records_seen != record_count {
        return Err(format!(
            "revisited {records_seen} records; frozen census counted {record_count}"
        )
        .into());
    }
    if groups.physical_headers
        != plugin_row["groups"]
            .as_u64()
            .ok_or("Fallout4.esm has no raw group count")?
    {
        return Err("physical GRUP count differs from the frozen plugin census".into());
    }
    let after = sha256(&source)?;
    if after != before {
        return Err("Fallout4.esm changed during group census".into());
    }
    let output_manifest = Manifest {
        schema: 1,
        status: "complete-fo4-physical-group-structure-not-record-count-semantics",
        plugin: "Fallout4.esm",
        source_census: "local/proof-fo4-002/census.json".into(),
        source_census_sha256: census_hash,
        source_sha256: after,
        source_bytes: expected_bytes,
        rust_records_including_tes4: records_seen,
        rust_records_excluding_tes4: records_seen.saturating_sub(1),
        hedr_declared_count: declared,
        groups,
        source_rechecked_after_walk: true,
        retail_file_modified: false,
        runtime_ready: false,
        limits: vec![
            "A 24-byte GRUP header is classified as physically empty; this does not establish the game's HEDR count rule.",
            "Group type and label are retained as raw numeric/hex observations, without interpreting record-family semantics.",
            "The scan verifies structural group extents only; it does not prove runtime loading, native APIs, gameplay or save compatibility.",
        ],
    };
    fs::create_dir_all(&output)?;
    let manifest_path = output.join("manifest.json");
    let bytes = serde_json::to_vec_pretty(&output_manifest)?;
    let mut writer = File::create(&manifest_path)?;
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    let manifest_hash = sha256(&manifest_path)?;
    fs::write(
        output.join("complete.json"),
        format!(
            "{{\"schema\":1,\"manifest_sha256\":\"{manifest_hash}\",\"complete\":true,\"runtime_ready\":false}}\n"
        ),
    )?;
    println!(
        "records={records_seen}; groups={}; empty_groups={}; max_group_depth={}; manifest_sha256={manifest_hash}",
        output_manifest.groups.physical_headers,
        output_manifest.groups.empty_headers,
        output_manifest.groups.maximum_nesting_depth
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FO4 group census failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_empty_groups_and_tracks_nested_siblings_by_extent() {
        let mut summary = GroupSummary::default();
        summary.add(0, 200, 0, *b"CELL").unwrap();
        summary.add(24, 64, 1, [1, 2, 3, 4]).unwrap();
        summary.add(88, 24, 1, [1, 2, 3, 4]).unwrap();
        summary.add(200, 24, 0, *b"CELL").unwrap();
        assert_eq!(summary.physical_headers, 4);
        assert_eq!(summary.empty_headers, 2);
        assert_eq!(summary.maximum_nesting_depth, 1);
        assert_eq!(summary.by_depth.get("0"), Some(&2));
        assert_eq!(summary.by_depth.get("1"), Some(&2));
    }

    #[test]
    fn group_extent_overflow_is_diagnostic() {
        let mut summary = GroupSummary::default();
        assert!(summary.add(u64::MAX - 4, 24, 0, *b"CELL").is_err());
    }
}
