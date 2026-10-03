//! Load winning script definitions and publish metadata without source text.
use super::{Result, protected_tree};
use fallout_data::{baseline, loaded_scripts, plugin, record_metadata, store::RecordStore};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Read, Write},
    path::Path,
};

#[derive(Serialize)]
struct Row<'a> {
    handle: &'a loaded_scripts::Handle,
    version: &'a loaded_scripts::Version,
    owner: &'a loaded_scripts::Owner,
    script_type: u16,
    flags: u16,
    declarations: &'a [loaded_scripts::Declaration],
    references: &'a [loaded_scripts::Reference],
    issues: &'a [String],
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    bundle_path: Option<&Path>,
) -> Result<Value> {
    let mut order_file = baseline::open_source(order_path)?;
    let mut order_bytes = Vec::new();
    (&mut order_file)
        .take(65_537)
        .read_to_end(&mut order_bytes)?;
    if order_bytes.len() > 65_536 {
        return Err("script load order exceeds 64 KiB".into());
    }
    let names: Vec<String> = serde_json::from_slice(&order_bytes)?;
    if names.is_empty() || names.len() > 254 {
        return Err("script load order requires 1..=254 plugins".into());
    }
    let mut store = if let Some(root) = cache {
        RecordStore::open_nv_headers_cached(
            &install.join("Data"),
            &names,
            plugin::Limits::default(),
            root,
        )?
    } else {
        RecordStore::open_nv_headers(&install.join("Data"), &names, plugin::Limits::default())?
    };
    let metadata = record_metadata::inspect(&store)?;
    let mut bundle = bundle_path
        .map(|path| -> Result<_> {
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .canonicalize()?;
            if parent.starts_with(protected_tree(install)?) {
                return Err("script catalogue bundle must be outside the installation".into());
            }
            let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(b"FRCAT001")?;
            Ok(BufWriter::new(file))
        })
        .transpose()?;
    let mut bundle_bytes = 8_u64;
    eprintln!(
        "Loading winning script definitions from {} plugins",
        names.len()
    );
    let catalogue = loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |source_index, record| {
            if let Some(bundle) = &mut bundle {
                bundle_bytes += 25 + record.payload.len() as u64;
                if bundle_bytes > 256 * 1024 * 1024 {
                    return Err(fallout_data::Error::Unsupported(
                        "script catalogue bundle exceeds 256 MiB".into(),
                    ));
                }
                let fail =
                    |error: std::io::Error| fallout_data::Error::Resolution(error.to_string());
                bundle.write_all(&[source_index as u8]).map_err(fail)?;
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
        },
    )?;
    let rows = catalogue
        .iter()
        .map(|(_, script)| Row {
            handle: script.handle(),
            version: script.version(),
            owner: script.owner(),
            script_type: script.script_type(),
            flags: script.flags(),
            declarations: script.declarations(),
            references: script.references(),
            issues: script.issues(),
        })
        .collect::<Vec<_>>();
    let receipt = if let Some(mut bundle) = bundle {
        bundle.flush()?;
        bundle.get_ref().sync_all()?;
        drop(bundle);
        let (bytes, sha256) = baseline::digest_file(bundle_path.expect("opened bundle"))?;
        Some(json!({"format":"FRCAT001","bytes":bytes,"sha256":sha256}))
    } else {
        None
    };
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":names,
        "load_order_sha256":format!("{:x}", Sha256::digest(&order_bytes)),"plugins":catalogue.sources,
        "metadata":metadata,"counts":catalogue.counts,"scripts":rows,"comparison_bundle":receipt,
        "index_cache":store.index_cache_report(),"index_payloads_deferred":store.deferred_payloads(),
        "execution_ready":false,"live_event_lists_loaded":false,"retail_parity_accepted":false}),
    )
}
