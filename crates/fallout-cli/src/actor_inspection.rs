//! Immutable source scalars over the existing inventory production load path.
use super::{Result, inspection_input::Order};
use fallout_data::{actors, baseline, inventory, record_metadata};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    include_associations: bool,
) -> Result<Value> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let inventory = inventory::Catalogue::load(&mut store, inventory::Limits::default())?;
    let catalogue = actors::Catalogue::load(&inventory, actors::Limits::default())?;
    let definitions = catalogue
        .iter()
        .map(|(_, definition)| definition)
        .collect::<Vec<_>>();
    let mut report = json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources(),
        "metadata":metadata,"winning_content_sha256":catalogue.winning_content_sha256(),
        "counts":catalogue.counts(),"definitions":definitions,"index_cache":store.index_cache_report(),
        "scope":"Exact authored NPC_/CREA scalar source fields joined to existing inventory provenance; no actor initialization, inheritance, automatic statistics or runtime conversion",
        "actors_initialized":false,"retail_parity_accepted":false,"accepted_scenarios":[]});
    if include_associations {
        let associations = actors::associations::Catalogue::load(
            &mut store,
            &catalogue,
            actors::associations::Limits::default(),
        )?;
        report["actor_associations"] = json!({"counts":associations.counts(),"definitions":associations.iter().map(|(_,definition)|definition).collect::<Vec<_>>()});
        report["scope"] = json!(
            "Exact authored NPC_/CREA scalar fields and ordered source associations; no inheritance, initialization, effect, faction or AI execution"
        );
    }
    Ok(report)
}

/// Compare complete projections with an external direct-source reader. Every
/// field (including opaque hashes), occurrence and provenance must agree.
pub(super) fn compare(report: &mut Value, oracle_path: &Path) -> Result<()> {
    let mut source = baseline::open_source(oracle_path)?;
    let mut bytes = Vec::new();
    (&mut source)
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err("actor oracle report exceeds 256 MiB".into());
    }
    let oracle: Value = serde_json::from_slice(&bytes)?;
    for key in [
        "schema_version",
        "profile",
        "sources",
        "metadata",
        "winning_content_sha256",
        "counts",
        "definitions",
    ] {
        if oracle.get(key).is_none() || report.get(key) != oracle.get(key) {
            return Err(format!("independent actor source comparison differs in {key}").into());
        }
    }
    if report.get("actor_associations").is_some()
        && report.get("actor_associations") != oracle.get("actor_associations")
    {
        return Err("independent actor source comparison differs in actor_associations".into());
    }
    let (oracle_bytes, oracle_sha256) = baseline::digest_file(oracle_path)?;
    report["independent_comparison"] = json!({"equal":true,"oracle_bytes":oracle_bytes,
        "oracle_sha256":oracle_sha256,"records_checked":report["counts"]["records"],
        "fields_checked":report["counts"]["fields"],"scalar_fields_checked":report["counts"]["scalar_fields"],
        "scope":"Complete source projection against a separate direct plugin reader; no retail behavior acceptance"});
    if report.get("actor_associations").is_some() {
        report["independent_comparison"]["association_bindings_checked"] =
            report["actor_associations"]["counts"]["bindings"].clone();
    }
    Ok(())
}
