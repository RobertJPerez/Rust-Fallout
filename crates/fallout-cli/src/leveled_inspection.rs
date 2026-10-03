//! Authored list words and structural dependency closure, without loot selection.
use super::{Result, inspection_input::Order};
use fallout_data::{
    identity::{FormKey, ProfileId, plugin_name},
    inventory, leveled, record_metadata,
};
use serde_json::{Value, json};
use std::path::Path;
pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    root: Option<(&str, u32)>,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    eprintln!("Loading winning leveled lists and inventory dependencies");
    let metadata = record_metadata::inspect(&store)?;
    let lists = leveled::Catalogue::load(&mut store, leveled::Limits::default())?;
    let base = inventory::Catalogue::load(&mut store, inventory::Limits::default())?;
    let graph = leveled::graph::Graph::build(&base, &lists, leveled::graph::Limits::default())?;
    let closure = root
        .map(|(name, id)| -> Result<_> {
            if id > 0x00ff_ffff {
                return Err("Closure root must use a canonical local ID".into());
            }
            let key = FormKey {
                profile: ProfileId::NvOriginal,
                origin_plugin: plugin_name(name)?,
                local_id: id,
            };
            Ok(graph.closure(&key, leveled::graph::Limits::default())?)
        })
        .transpose()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":lists.sources,"metadata":metadata,"counts":lists.counts,
        "definitions":lists.iter().map(|(_,d)|d).collect::<Vec<_>>(),"inventory_counts":base.counts,"dependency_graph":graph,"closure":closure,
        "index_cache":store.index_cache_report(),"scope":"Authored LVLI/LVLC/LVLN fields and structural inventory/template/list links; no random draws, inheritance or live initialization",
        "random_draws":0,"live_inventory_initialized":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
