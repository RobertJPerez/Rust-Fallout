//! Structural plans for a hash-bound offline SCDA bundle; no game execution.
use super::{Result, command_catalogue, script_profile};
use fallout_data::{
    baseline,
    obscript::{expression_plan, expression_plan_bundle},
};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

pub(super) fn inspect(install: &Path, bundle: &Path, diagnostic: bool) -> Result<Value> {
    let catalogue = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let operators = script_profile::operators(&catalogue)?;
    let model = expression_plan::Model::vanilla(&operators)?;
    let mut source = baseline::open_source(bundle)?;
    let mut bytes = Vec::new();
    (&mut source)
        .take(expression_plan_bundle::MAX_BUNDLE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let report = if diagnostic {
        expression_plan_bundle::inspect_diagnostic(&bytes, &model)?
    } else {
        expression_plan_bundle::inspect(&bytes, &model)?
    };
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"Vanilla postfix structure from an offline SCDA bundle; extraction provenance belongs to its source receipt",
        "executable_source_sha256":catalogue.source_sha256,"operator_descriptors":catalogue.operators,
        "plans":report,"execution_ready":false,"retail_parity_accepted":false}))
}
