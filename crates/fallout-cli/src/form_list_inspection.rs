//! Inspect physical form-list members without inventing live expansion rules.
use super::{Result, inspection_input::Order};
use fallout_data::{
    form_lists,
    identity::{FormKey, ProfileId, plugin_name},
    record_metadata,
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
    eprintln!("Loading winning source form lists");
    let metadata = record_metadata::inspect(&store)?;
    let lists = form_lists::Catalogue::load(&mut store, form_lists::Limits::default())?;
    let graph = form_lists::graph::Graph::build(&lists, form_lists::graph::Limits::default())?;
    let closure = root
        .map(|(name, id)| -> Result<_> {
            if id == 0 || id > 0x00ff_ffff {
                return Err("List root must have a nonzero canonical local ID".into());
            }
            Ok(graph.closure(
                &FormKey {
                    profile: ProfileId::NvOriginal,
                    origin_plugin: plugin_name(name)?,
                    local_id: id,
                },
                form_lists::graph::Limits::default(),
            )?)
        })
        .transpose()?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":lists.sources,"metadata":metadata,"counts":lists.counts,
        "definitions":lists.iter().map(|(_,d)|d).collect::<Vec<_>>(),"dependency_graph":graph,"closure":closure,"index_cache":store.index_cache_report(),
        "scope":"Physical FLST fields, ordered member bindings and structural closure; no live list mutation or count expansion",
        "live_list_state_initialized":false,"get_item_count_list_expansion_implemented":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
