//! Source inventory declarations, without live item creation or list expansion.
use super::{Result, inspection_input::Order};
use fallout_data::{inventory, record_metadata};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn inspect(install: &Path, order_path: &Path, cache: Option<&Path>) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    eprintln!("Loading winning base inventory declarations");
    let metadata = record_metadata::inspect(&store)?;
    let catalogue = inventory::Catalogue::load(&mut store, inventory::Limits::default())?;
    let definitions = catalogue
        .iter()
        .map(|(_, definition)| definition)
        .collect::<Vec<_>>();
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources,"metadata":metadata,
        "counts":catalogue.counts,"definitions":definitions,"index_cache":store.index_cache_report(),
        "scope":"Authored winning CONT/NPC_/CREA declarations; no template or leveled-list expansion, live inventory, ownership enforcement or respawn behavior",
        "live_inventory_initialized":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
