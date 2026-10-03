//! Operand associations are metadata. Unknown foreign declaration/value behavior
//! remains visible, and comparison records stay outside the original install.
use super::{Result, command_catalogue, data_files, protected_tree, script_profile};
use fallout_data::{baseline, operand_bindings};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

pub(super) fn inspect(install: &Path, focused: bool, bundle_path: Option<&Path>) -> Result<Value> {
    let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&catalogue)?;
    let signatures = script_profile::signatures(&catalogue);
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("operand binding bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FRUNIT01")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let mut bundle_bytes = 8_u64;
    let mut reports = Vec::new();
    let mut total_uses = 0_u64;
    for path in data_files(install, &["esm", "esp"])? {
        eprintln!("Inspecting operand bindings in {}", path.display());
        let report =
            operand_bindings::inspect(&path, focused, &operators, &signatures, |record, _| {
                if let Some(bundle) = &mut bundle {
                    bundle_bytes += 20 + record.payload.len() as u64;
                    if bundle_bytes > 512 * 1024 * 1024 {
                        return Err(fallout_data::Error::Unsupported(
                            "operand binding bundle exceeds 512 MiB".into(),
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
        total_uses += report
            .compiled_units
            .iter()
            .map(|unit| unit.counts.uses)
            .sum::<u64>();
        if total_uses > 2_000_000 {
            return Err("operand binding run exceeds two million uses".into());
        }
        reports.push(report);
    }
    let receipt = if let Some(mut bundle) = bundle {
        bundle.flush()?;
        bundle.get_ref().sync_all()?;
        drop(bundle);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("opened bundle"))?;
        Some(json!({"format":"FRUNIT01","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    let table_issues: usize = reports
        .iter()
        .map(|report| report.table_units_with_issues)
        .sum();
    let decode_issues: usize = reports.iter().map(|report| report.decode_issues).sum();
    let missing_bindings: u64 = reports.iter().map(|report| report.missing_bindings).sum();
    let foreign: u64 = reports
        .iter()
        .map(|report| report.deferred_foreign_locals)
        .sum();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","executable_source_sha256":catalogue.source_sha256,
        "scope":"authored operand/table associations; foreign declarations, loaded values and execution remain unverified",
        "plugins":reports,"table_units_with_issues":table_issues,"decode_issues":decode_issues,"missing_bindings":missing_bindings,
        "deferred_foreign_locals":foreign,"comparison_bundle":receipt,"execution_ready":false,"retail_parity_accepted":false}),
    )
}
