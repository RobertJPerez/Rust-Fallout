//! Read-only source structure. Execution semantics remain separate capability gates.
use super::Result;
use fallout_data::{
    baseline,
    obscript::{control_flow_bundle, expression_plan_bundle::MAX_BUNDLE_BYTES},
};
use serde_json::{Value, json};
use std::{io::Read, path::Path};

pub(super) fn inspect(bundle: &Path, diagnostic: bool) -> Result<Value> {
    let mut source = baseline::open_source(bundle)?;
    let mut bytes = Vec::new();
    (&mut source)
        .take(MAX_BUNDLE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let report = if diagnostic {
        control_flow_bundle::inspect_diagnostic(&bytes)?
    } else {
        control_flow_bundle::inspect(&bytes)?
    };
    Ok(json!({"schema_version":1,
        "scope":"SCDA delimiter structure and raw distance relations; no VM successors or original branch semantics",
        "structure":report,"execution_ready":false,"retail_parity_accepted":false}))
}
