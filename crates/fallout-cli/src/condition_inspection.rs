//! Source condition metadata and vanilla descriptor links, without evaluation.
use super::{Result, command_catalogue, data_files, protected_tree};
use fallout_data::{baseline, condition_census};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

pub(super) fn inspect(install: &Path, focused: bool, bundle_path: Option<&Path>) -> Result<Value> {
    let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("condition bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FRCOND01")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let mut bundle_bytes = 8_u64;
    let mut reports = Vec::new();
    let mut functions = BTreeMap::<u16, u64>::new();
    let mut total = 0;
    for path in data_files(install, &["esm", "esp"])? {
        eprintln!("Inspecting conditions in {}", path.display());
        let report = condition_census::inspect(&path, focused, |record| {
            if let Some(bundle) = &mut bundle {
                bundle_bytes += 20 + record.payload.len() as u64;
                if bundle_bytes > 256 * 1024 * 1024 {
                    return Err(fallout_data::Error::Unsupported(
                        "condition bundle exceeds 256 MiB".into(),
                    ));
                }
                let fail =
                    |error: std::io::Error| fallout_data::Error::Resolution(error.to_string());
                bundle.write_all(&record.header.kind).map_err(fail)?;
                bundle
                    .write_all(&record.header.form_id.to_le_bytes())
                    .map_err(fail)?;
                bundle
                    .write_all(&record.header.offset.to_le_bytes())
                    .map_err(fail)?;
                bundle
                    .write_all(&(record.payload.len() as u32).to_le_bytes())
                    .map_err(fail)?;
                bundle.write_all(&record.payload).map_err(fail)?;
            }
            Ok(())
        })?;
        total += report.counts.conditions;
        if total > 1_000_000 {
            return Err("condition run exceeds one million fields".into());
        }
        for (id, count) in &report.counts.functions {
            *functions.entry(*id).or_default() += count;
        }
        reports.push(report);
    }
    let mut links = Vec::new();
    let mut unresolved = Vec::new();
    for (id, count) in functions {
        // This is the explicitly verified vanilla identifier mapping from the
        // catalogue checkpoint. Condition parameter words are not native args.
        if let Some(command) = catalogue
            .script_commands
            .iter()
            .find(|row| row.id == u32::from(id) + 0x1000 && row.condition_handler_present)
        {
            links.push(json!({"condition_function_id":id,"occurrences":count,"script_command_id":command.id,
                "name":command.name,"descriptor_file_offset":command.descriptor_file_offset,"behavior_status":"unimplemented"}));
        } else {
            unresolved.push(id);
        }
    }
    let receipt = if let Some(mut bundle) = bundle {
        bundle.flush()?;
        bundle.get_ref().sync_all()?;
        drop(bundle);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("opened bundle"))?;
        Some(json!({"format":"FRCOND01","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    Ok(
        json!({"schema_version":1,"profile":"nv-original","executable_source_sha256":catalogue.source_sha256,
        "scope":"CTDA source layouts, comparison/subject metadata and original handler links; parameters remain raw; no evaluation/grouping or default migration",
        "plugins":reports,"condition_function_links":links,"unresolved_condition_function_ids":unresolved,
        "comparison_bundle":receipt,"evaluation_ready":false,"retail_parity_accepted":false}),
    )
}
