//! Load winning script definitions and publish metadata without source text.
use super::{
    Result,
    inspection_input::{Order, RecordBundle},
};
use fallout_data::{loaded_scripts, record_metadata};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

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
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let mut bundle = bundle_path
        .map(|path| RecordBundle::create(path, install, b"FRCAT001"))
        .transpose()?;
    eprintln!(
        "Loading winning script definitions from {} plugins",
        order.names.len()
    );
    let catalogue = loaded_scripts::Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |source, record| {
            if let Some(bundle) = &mut bundle {
                bundle.write(source, record)?;
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
    let receipt = bundle.map(RecordBundle::finish).transpose()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,
        "load_order_sha256":order.sha256,"plugins":catalogue.sources,
        "metadata":metadata,"counts":catalogue.counts,"scripts":rows,"comparison_bundle":receipt,
        "index_cache":store.index_cache_report(),"index_payloads_deferred":store.deferred_payloads(),
        "execution_ready":false,"live_event_lists_loaded":false,"retail_parity_accepted":false}),
    )
}
