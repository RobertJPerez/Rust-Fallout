//! Membership follows the supplied order and original parent labels. Header-only
//! inspection does not validate deferred INFO bodies or establish dialogue order.
use super::Result;
use fallout_data::{baseline, dialogue_membership::MembershipIndex, plugin, store::RecordStore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(super) fn inspect(install: &Path, order_path: &Path, cache: Option<&Path>) -> Result<Value> {
    let mut order_file = baseline::open_source(order_path)?;
    let mut order_bytes = Vec::new();
    (&mut order_file)
        .take(65_537)
        .read_to_end(&mut order_bytes)?;
    if order_bytes.len() > 65_536 {
        return Err("dialogue load order exceeds 64 KiB".into());
    }
    let names: Vec<String> = serde_json::from_slice(&order_bytes)?;
    if names.is_empty() || names.len() > 254 {
        return Err("dialogue load order requires 1..=254 plugins".into());
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
    let membership = MembershipIndex::build(&store, 1_000_000)?;
    let metadata = fallout_data::record_metadata::inspect(&store)?;
    let sources = store.source_receipts()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":names,
        "load_order_sha256":format!("{:x}", Sha256::digest(&order_bytes)),
        "plugins":sources,"metadata":metadata,"membership":membership.report(),"index_cache":store.index_cache_report(),
        "index_payloads_deferred":store.deferred_payloads(),"runtime_ready":false,"retail_parity_accepted":false}),
    )
}
