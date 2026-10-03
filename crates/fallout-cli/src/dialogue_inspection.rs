//! Membership follows the supplied order and original parent labels. Header-only
//! inspection does not validate deferred INFO bodies or establish dialogue order.
use super::{Result, inspection_input::Order};
use fallout_data::{dialogue_membership::MembershipIndex, record_metadata};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn inspect(install: &Path, order_path: &Path, cache: Option<&Path>) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let membership = MembershipIndex::build(&store, 1_000_000)?;
    let metadata = record_metadata::inspect(&store)?;
    let sources = store.source_receipts()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,
        "load_order_sha256":order.sha256,"plugins":sources,"metadata":metadata,"membership":membership.report(),
        "index_cache":store.index_cache_report(),"index_payloads_deferred":store.deferred_payloads(),
        "runtime_ready":false,"retail_parity_accepted":false}),
    )
}
