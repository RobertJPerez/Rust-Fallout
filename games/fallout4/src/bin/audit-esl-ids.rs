//! Counts raw record-header FormID patterns in physically ESL-flagged FO4 plugins.
use fallout_data::plugin::{self, Event};
use fallout4_prep::{Error, Result, census, formid};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

fn error(reason: &str) -> Error {
    Error::Unsupported(reason.into())
}

fn checked_local_output(requested: &Path, install: &Path, proof: &Path) -> Result<PathBuf> {
    let parent = fs::canonicalize(requested.parent().unwrap_or(Path::new(".")))?;
    let local = fs::canonicalize("local")?;
    if !parent.starts_with(&local) || parent.starts_with(install) || parent.starts_with(proof) {
        return Err(error(
            "new output must be private local/, outside the install and input proof",
        ));
    }
    Ok(parent.join(
        requested
            .file_name()
            .ok_or_else(|| error("output directory name missing"))?,
    ))
}

fn direct_data_file(install: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.components().count() != 2
        || path
            .components()
            .next()
            .is_none_or(|component| component.as_os_str() != "Data")
        || path.components().nth(1).is_none_or(|component| {
            component
                .as_os_str()
                .to_string_lossy()
                .contains(['/', '\\', ':'])
        })
    {
        return Err(error("ESL source path is outside top-level Data"));
    }
    let source = install.join(path);
    let metadata = fs::symlink_metadata(&source)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(error("ESL source is not a regular file"));
    }
    Ok(source)
}

#[derive(Default)]
struct Counts {
    records: u64,
    patterns: BTreeMap<String, u64>,
    raw_high_bytes: BTreeMap<u8, u64>,
    small_selector_candidates: BTreeMap<u16, u64>,
}

fn bump<K: Ord>(map: &mut BTreeMap<K, u64>, key: K) {
    *map.entry(key).or_default() += 1;
}

fn write_json_line(out: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}

fn scan_plugin(
    install: &Path,
    relative_path: &str,
    plugin_row: &Value,
    expected_sha256: &str,
    expected_bytes: u64,
    source_census_sha256: &str,
    out: &mut impl Write,
) -> Result<Value> {
    let path = direct_data_file(install, relative_path)?;
    let name = plugin_row["name"]
        .as_str()
        .ok_or_else(|| error("plugin name missing from frozen census"))?;
    let master_names = plugin_row["masters"]
        .as_array()
        .ok_or_else(|| error("plugin master table missing from frozen census"))?;
    let header_flags = plugin_row["header_flags"]
        .as_u64()
        .ok_or_else(|| error("plugin header flags missing from frozen census"))?
        as u32;
    if !formid::is_small_master_flag(header_flags) {
        return Err(error(
            "selected plugin no longer has the Fallout 4 small-master flag",
        ));
    }
    let before_hash = census::hash_file(&path)?;
    if before_hash != expected_sha256 {
        return Err(error(
            "ESL source fingerprint differs from the frozen census",
        ));
    }
    let bytes = fs::metadata(&path)?.len();
    if bytes != expected_bytes {
        return Err(error("ESL source size differs from the frozen census"));
    }
    let mut counts = Counts::default();
    let source_file = File::open(&path)?;
    let mut callback_error = None;
    let parse_result = plugin::visit(
        &mut BufReader::new(source_file),
        bytes,
        name,
        plugin::Limits::default(),
        |event| {
            if let Event::Record(record) = event {
                let observation = formid::observe(record.header.form_id, master_names.len());
                counts.records += 1;
                bump(&mut counts.patterns, format!("{:?}", observation.pattern));
                bump(&mut counts.raw_high_bytes, observation.raw_high_byte);
                if let Some(index) = observation.small_selector_candidate {
                    bump(&mut counts.small_selector_candidates, index);
                }
                if let Err(error) = write_json_line(
                    out,
                    &json!({
                        "source_census_sha256":source_census_sha256,
                        "plugin":name,
                        "plugin_sha256":expected_sha256,
                        "plugin_header_flags":header_flags,
                        "plugin_is_small_master":true,
                        "listed_master_count":master_names.len(),
                        "record_offset":record.header.offset,
                        "record_kind":plugin::signature(record.header.kind),
                        "record_version":record.header.version,
                        "record_flags":record.header.flags,
                        "form_id":observation
                    }),
                ) {
                    callback_error = Some(error);
                    return Err(fallout_data::Error::Unsupported(
                        "FormID inventory output failed".into(),
                    ));
                }
            }
            Ok(())
        },
    );
    if let Some(error) = callback_error {
        return Err(error);
    }
    parse_result?;
    let expected_records = plugin_row["records"]
        .as_u64()
        .ok_or_else(|| error("plugin record count missing from frozen census"))?;
    if counts.records != expected_records {
        return Err(error("ESL record count differs from the frozen census"));
    }
    let after_hash = census::hash_file(&path)?;
    if after_hash != before_hash {
        return Err(error("retail plugin changed during read-only FormID audit"));
    }
    Ok(json!({
        "plugin":name,
        "plugin_sha256":before_hash,
        "plugin_bytes":bytes,
        "header_flags":header_flags,
        "listed_masters":master_names,
        "record_headers":counts.records,
        "raw_pattern_counts":counts.patterns,
        "raw_high_byte_histogram":counts.raw_high_bytes,
        "small_selector_candidates":counts.small_selector_candidates,
        "source_hash_unchanged":before_hash==after_hash
    }))
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(error(
            "audit-esl-ids <Fallout-4-install> <completed-content-proof> <new-local-output>",
        ));
    }
    let install = fs::canonicalize(&args[0])?;
    let proof = fs::canonicalize(&args[1])?;
    let complete: Value = serde_json::from_reader(File::open(proof.join("complete.json"))?)?;
    let census_path = proof.join("census.json");
    let census_sha256 = census::hash_file(&census_path)?;
    if complete["status"] != "matched" || complete["census_sha256"] != census_sha256 {
        return Err(error("completed content proof required"));
    }
    let corpus: Value = serde_json::from_reader(File::open(census_path)?)?;
    let output = checked_local_output(Path::new(&args[2]), &install, &proof)?;
    fs::create_dir(&output)?;
    let files = corpus["files"]
        .as_array()
        .ok_or_else(|| error("frozen file fingerprint list missing"))?;
    let mut selected = Vec::new();
    for plugin_row in corpus["plugins"]
        .as_array()
        .ok_or_else(|| error("frozen plugin census missing"))?
    {
        let flags = plugin_row["header_flags"]
            .as_u64()
            .ok_or_else(|| error("plugin header flags missing"))? as u32;
        if !formid::is_small_master_flag(flags) {
            continue;
        }
        let name = plugin_row["name"]
            .as_str()
            .ok_or_else(|| error("plugin name missing"))?;
        let relative_path = format!("Data/{name}");
        let file = files
            .iter()
            .find(|candidate| candidate["path"] == relative_path)
            .ok_or_else(|| error("ESL has no fingerprint in the frozen content census"))?;
        selected.push((plugin_row, relative_path, file));
    }
    if selected.is_empty() {
        return Err(error(
            "frozen content census has no FO4 small-master plugins",
        ));
    }
    let mut records = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("record-ids.jsonl"))?,
    );
    let mut plugin_summaries = Vec::with_capacity(selected.len());
    let mut total_records = 0u64;
    for (plugin_row, relative_path, fingerprint) in selected {
        let sha256 = fingerprint["sha256"]
            .as_str()
            .ok_or_else(|| error("ESL source hash missing from frozen content census"))?;
        let bytes = fingerprint["bytes"]
            .as_u64()
            .ok_or_else(|| error("ESL source size missing from frozen content census"))?;
        let summary = scan_plugin(
            &install,
            &relative_path,
            plugin_row,
            sha256,
            bytes,
            &census_sha256,
            &mut records,
        )?;
        total_records = total_records
            .checked_add(
                summary["record_headers"]
                    .as_u64()
                    .ok_or_else(|| error("ESL record count missing after scan"))?,
            )
            .ok_or_else(|| error("record count overflow"))?;
        plugin_summaries.push(summary);
    }
    records.flush()?;
    let report_path = output.join("plugins.json");
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&report_path)?,
        &json!({"schema":1,"plugins":plugin_summaries}),
    )?;
    let rows_path = output.join("record-ids.jsonl");
    let result = json!({
        "schema":1,
        "status":"raw_small_master_record_header_id_observation",
        "source_census_sha256":census_sha256,
        "source_proof_complete_sha256":census::hash_file(&proof.join("complete.json"))?,
        "small_master_flag":"0x00000200",
        "selected_plugins":plugin_summaries.len(),
        "record_headers":total_records,
        "plugins_sha256":census::hash_file(&report_path)?,
        "record_ids_sha256":census::hash_file(&rows_path)?,
        "limits":[
            "Record-header FormID fields only; VMAD object IDs and other reference subrecords are outside this scan",
            "Raw values and bit-pattern candidates are retained; no self-slot, ESL runtime slot, FormKey or override winner is assigned",
            "Small/medium selector fields are pattern observations based on the pinned source layout, not active load-order identities",
            "Physical plugins are read-only and must match the frozen source fingerprints before and after scanning"
        ]
    });
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("complete.json"))?,
        &result,
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn output_must_be_new_local_and_outside_source_inputs() {
        let install = tempdir().unwrap();
        let proof = tempdir().unwrap();
        let requested = fs::canonicalize("local").unwrap().join("audit-test");
        assert_eq!(
            checked_local_output(&requested, install.path(), proof.path()).unwrap(),
            requested
        );
        assert!(checked_local_output(install.path(), install.path(), proof.path()).is_err());
        assert!(checked_local_output(proof.path(), install.path(), proof.path()).is_err());
    }

    #[test]
    fn source_path_is_confined_to_direct_data_members() {
        let install = tempdir().unwrap();
        fs::create_dir(install.path().join("Data")).unwrap();
        let source = install.path().join("Data/item.esl");
        fs::write(&source, b"fixture").unwrap();
        assert!(direct_data_file(install.path(), "Data/item.esl").is_ok());
        assert!(direct_data_file(install.path(), "Data/../outside.esl").is_err());
        assert!(direct_data_file(install.path(), "Data/nested/item.esl").is_err());
    }
}
