//! Compare extraction independently from typed field interpretation.
use super::{Result, inspection_input::Order};
use fallout_data::{compressed_records, record_metadata};
use serde_json::{Value, json};
use std::path::Path;
pub(super) fn inspect(install: &Path, order_path: &Path, diagnostic: bool) -> Result<Value> {
    let order = Order::read(order_path)?;
    let store = order.store(install, None)?;
    let metadata = record_metadata::inspect(&store)?;
    let mut reports = Vec::new();
    let mut records = 0_u64;
    let mut decoded = 0_u64;
    let mut mismatches = 0_u64;
    for name in &order.names {
        eprintln!("Checking compressed records in {name}");
        let mut limits = compressed_records::Limits::default();
        limits.plugin.inspect_checksum_mismatches = diagnostic;
        let report = compressed_records::inspect(&install.join("Data").join(name), limits)?;
        records += report.counts.records;
        decoded += report.counts.decoded_bytes;
        mismatches += report.counts.checksum_mismatches;
        if records > 1_000_000 || decoded > 8 * 1024 * 1024 * 1024 {
            return Err("compressed record run exceeds record/decoded byte budget".into());
        }
        reports.push(report);
    }
    Ok(
        json!({"schema_version":1,"profile":"nv-original","explicit_load_order":order.names,"load_order_sha256":order.sha256,"metadata":metadata,
        "plugins":reports,"counts":{"records":records,"decoded_bytes":decoded,"checksum_mismatches":mismatches},"checksum_inspection_enabled":diagnostic,
        "tainted_payloads_runtime_eligible":false,"runtime_checksum_policy":"strict","retail_parity_accepted":false}),
    )
}
