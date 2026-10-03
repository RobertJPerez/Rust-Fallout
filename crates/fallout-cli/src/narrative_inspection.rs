//! Source quest/dialogue structure and ownership, without evaluation.
use super::{Result, data_files, protected_tree};
use fallout_data::{baseline, narrative_census};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::Path,
};

pub(super) fn inspect(install: &Path, focused: bool, bundle_path: Option<&Path>) -> Result<Value> {
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("narrative bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FRNARR01")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let mut bundle_bytes = 8_u64;
    let mut reports = Vec::new();
    let mut findings = 0;
    let mut total = 0;
    for path in data_files(install, &["esm", "esp"])? {
        eprintln!("Inspecting narratives in {}", path.display());
        let report = narrative_census::inspect(&path, focused, |record| {
            if let Some(bundle) = &mut bundle {
                bundle_bytes += 24 + record.payload.len() as u64;
                if bundle_bytes > 256 * 1024 * 1024 {
                    return Err(fallout_data::Error::Unsupported(
                        "narrative bundle exceeds 256 MiB".into(),
                    ));
                }
                let fail =
                    |error: std::io::Error| fallout_data::Error::Resolution(error.to_string());
                bundle.write_all(&record.header.kind).map_err(fail)?;
                bundle
                    .write_all(&record.header.form_id.to_le_bytes())
                    .map_err(fail)?;
                bundle
                    .write_all(&record.header.flags.to_le_bytes())
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
        total += report.counts.fields;
        if total > 8_000_000 {
            return Err("narrative run exceeds eight million fields".into());
        }
        findings += report.counts.findings;
        reports.push(report);
    }
    let receipt = if let Some(mut bundle) = bundle {
        bundle.flush()?;
        bundle.get_ref().sync_all()?;
        drop(bundle);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("opened bundle"))?;
        Some(json!({"format":"FRNARR01","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"authored QUST/INFO/DIAL fields, sections, condition ownership and embedded script ownership; no story text or execution",
        "plugins":reports,"findings":findings,"comparison_bundle":receipt,
        "execution_ready":false,"retail_parity_accepted":false}))
}
