//! Bind live preparation to a separately restored engineering journal, fresh
//! source plans and an independent reader of every exported event-window header.
use super::{Result, digest, json_file, run_logged, run_logged_status, write_new};
use fallout_data::obscript;
use fallout_runtime::{Limits, events::Trigger, save::format};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const FOREIGN: &str = "source-item-regression/native-migration-regression/item-state-regression/leveled-source-regression/base-inventory-regression/foreign-runtime-regression";
fn key(value: &Value) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}
fn bodies(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    if bytes.len() > 66 * 1024 * 1024 || bytes.get(..8) != Some(b"FROBS001") {
        return Err("Invalid winning source bundle".into());
    }
    let mut cursor = 8;
    let mut rows = Vec::new();
    while cursor < bytes.len() {
        let length = u32::from_le_bytes(
            bytes
                .get(cursor..cursor + 4)
                .ok_or("Truncated winning body length")?
                .try_into()?,
        ) as usize;
        cursor += 4;
        let body = bytes
            .get(cursor..cursor + length)
            .ok_or("Truncated winning source body")?;
        rows.push(body);
        cursor += length;
        if rows.len() > 65_536 {
            return Err("Winning body count budget exceeded".into());
        }
    }
    Ok(rows)
}
pub(super) fn run(
    root: &Path,
    run: &Path,
    cli: &Path,
    oracle: &Path,
    install: &Path,
    sources: &Value,
) -> Result<Value> {
    let path = run.join("event-frames-rust.json");
    let bundle = run.join("event-frames.bin");
    let mut command = Command::new(cli);
    command
        .current_dir(root)
        .arg("event-frames")
        .arg("--install")
        .arg(install)
        .arg("--load-order")
        .arg(root.join("profiles/nv-inspection-order.json"))
        .arg("--comparison-bundle")
        .arg(&bundle)
        .arg("--output")
        .arg(&path);
    run_logged_status(command, &run.join("event-frames-rust.log"), 1)?;
    let report = json_file(&path)?;
    if report["sources"] != *sources
        || report["canonical_state_unchanged"] != true
        || report["bytecode_executed"] != false
        || report["retail_parity_accepted"] != false
        || report["comparison_bundle"]["sha256"] != digest(&bundle)?
    {
        return Err("Event-frame source identities or capability scope differs".into());
    }
    let source_path = run.join("source-plans-rust.json");
    let source = json_file(&source_path)?;
    if source["sources"] != *sources
        || source["executable_source_sha256"] != report["executable_source_sha256"]
    {
        return Err("Event-frame source or metadata profiles differ".into());
    }
    let mut definitions = BTreeMap::new();
    for row in source["definitions"]
        .as_array()
        .ok_or("Missing fresh source plans")?
    {
        if definitions.insert(key(&row["handle"])?, row).is_some() {
            return Err("Duplicate source-plan handle".into());
        }
    }
    let winning_bytes = fs::read(run.join("winning-compiled.bin"))?;
    if format!("{:x}", Sha256::digest(&winning_bytes)) != source["comparison_bundle"]["sha256"] {
        return Err("Winning body bytes changed".into());
    }
    let winning = bodies(&winning_bytes)?;
    let save_path = run
        .join(FOREIGN)
        .join("native-save-regression/native-repository/golden-previous.frsv");
    let saved = format::decode(&fs::read(&save_path)?, Limits::default())?;
    if report["snapshot_sha256"] != saved.metadata.snapshot_sha256
        || report["catalogue_sha256"] != saved.metadata.catalogue_sha256
    {
        return Err(
            "Event preparation differs from the separately persisted engineering journal".into(),
        );
    }
    let instances = saved
        .snapshot
        .instances
        .iter()
        .map(|instance| (instance.id, &instance.definition))
        .collect::<BTreeMap<_, _>>();
    let rows = report["events"]
        .as_array()
        .ok_or("Missing pending event rows")?;
    if rows.len() != saved.snapshot.pending_events.len()
        || report["pending_events_checked"] != rows.len()
    {
        return Err("Event preparation dropped journal entries".into());
    }
    let mut command = Command::new(oracle);
    command.current_dir(root).arg(&bundle);
    let output = run_logged(command, &run.join("event-frames-native.log"))?;
    let native_path = run.join("event-frames-native.json");
    write_new(&native_path, &output.stdout)?;
    let native = json_file(&native_path)?;
    if native["bundle_sha256"] != report["comparison_bundle"]["sha256"]
        || native["execution_ready"] != false
    {
        return Err("Independent frame bundle identity differs".into());
    }
    let native_rows = native["bodies"]
        .as_array()
        .ok_or("Missing independent frame headers")?;
    let mut cursor = 0;
    let mut instructions = 0;
    let mut operand_uses = 0_u64;
    let mut attempted_source_bytes = 0;
    let mut findings = Vec::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for (row, pending) in rows.iter().zip(&saved.snapshot.pending_events) {
        let definition = instances
            .get(&pending.instance)
            .ok_or("Pending instance is missing from native snapshot")?;
        if row["pending"] != json!(pending) || row["definition"] != json!(definition) {
            return Err("Event frame substituted a journal context or live instance".into());
        }
        let source = definitions
            .get(&key(&row["definition"])?)
            .ok_or("Pending definition is missing from fresh source plans")?;
        if let Some(index) = source["compiled_bundle_index"].as_u64() {
            attempted_source_bytes += winning
                .get(index as usize)
                .ok_or("Missing attempted winning body")?
                .len();
        }
        if !source["finding"].is_null() {
            if row["finding"] != source["finding"] || !row["prepared"].is_null() {
                return Err("Event frame erased an unresolved source finding".into());
            }
            *counts
                .entry(
                    row["finding"]["kind"]
                        .as_str()
                        .ok_or("Missing event finding kind")?
                        .into(),
                )
                .or_default() += 1;
            findings.push(json!({"pending_sequence":pending.sequence,"definition":row["definition"],"finding":row["finding"]}));
            continue;
        }
        let prepared = &row["prepared"];
        let Trigger::Block {
            event_id,
            begin_byte_offset,
        } = pending.trigger
        else {
            return Err(
                "Engineering journal unexpectedly includes an object-event observation".into(),
            );
        };
        if !row["finding"].is_null()
            || prepared.is_null()
            || prepared["begin_scda_offset"] != begin_byte_offset
            || prepared["selected"]["event_id"] != event_id
            || !source["prepared"]["control"]["events"]
                .as_array()
                .ok_or("Missing source event blocks")?
                .contains(&prepared["selected"])
            || prepared["binding_sha256"] != source["prepared"]["binding_sha256"]
        {
            return Err("Live event identity, source group or owning tables differ".into());
        }
        let body_index = source["compiled_bundle_index"]
            .as_u64()
            .ok_or("Prepared definition has no compiled body")? as usize;
        let program = obscript::decode(
            winning
                .get(body_index)
                .ok_or("Missing winning compiled body")?,
            obscript::Limits::default(),
        )?;
        let begin = prepared["selected"]["begin_instruction"]
            .as_u64()
            .ok_or("Missing event begin index")? as usize;
        let end = prepared["selected"]["end_instruction"]
            .as_u64()
            .ok_or("Missing event end index")? as usize;
        let start_offset = program
            .instructions
            .get(begin)
            .ok_or("Invalid event begin index")?
            .bytes
            .start;
        let end_offset = program
            .instructions
            .get(end)
            .ok_or("Invalid event end index")?
            .bytes
            .end;
        let window = program
            .bytes
            .get(start_offset..end_offset)
            .ok_or("Invalid event byte window")?;
        if end < begin
            || prepared["instructions"] != end - begin + 1
            || prepared["begin_scda_offset"] != start_offset
            || prepared["end_scda_offset"] != end_offset
            || prepared["bytes"] != window.len()
            || prepared["sha256"] != format!("{:x}", Sha256::digest(window))
            || prepared["frame_bundle_index"] != cursor
        {
            return Err("Event frame bytes do not belong to its winning source region".into());
        }
        let native = native_rows
            .get(cursor)
            .ok_or("Missing independent prepared frame")?;
        for key in [
            "bytes",
            "sha256",
            "framing_sha256",
            "instructions",
            "reference_calls",
            "event_blocks",
        ] {
            if native[key] != prepared[key] {
                return Err(format!("Independent event header comparison differs: {key}").into());
            }
        }
        *counts.entry("prepared_source_frame".into()).or_default() += 1;
        operand_uses += source["prepared"]["binding_counts"]["uses"]
            .as_u64()
            .ok_or("Missing full source binding count")?;
        instructions += end - begin + 1;
        cursor += 1;
    }
    if cursor != native_rows.len()
        || report["prepared_frames"] != cursor
        || report["prepared_instructions"] != instructions
        || report["counts"] != json!(counts)
        || report["attempted_source_bytes"] != attempted_source_bytes
        || report["prepared_operand_uses"] != operand_uses
    {
        return Err("Event-frame aggregate coverage differs".into());
    }
    Ok(json!({"schema_version":1,"profile":"nv-original",
        "scope":"Bounded event source frames joined to exact separately persisted engineering journal contexts, immutable winning plans and independent window headers; no VM execution",
        "sources":sources,"pending_events_checked":rows.len(),"prepared_frames":cursor,"prepared_instructions":instructions,"counts":counts,"source_findings":findings,
        "attempted_source_bytes":attempted_source_bytes,"prepared_operand_uses":operand_uses,
        "all_journal_identities_source_regions_and_independent_headers_equal":true,"canonical_state_unchanged":true,
        "rust_report_sha256":digest(&path)?,"native_report_sha256":digest(&native_path)?,"event_bundle_sha256":digest(&bundle)?,
        "source_plan_report_sha256":digest(&source_path)?,"native_journal_container_sha256":digest(&save_path)?,"native_journal_snapshot_sha256":saved.metadata.snapshot_sha256,
        "bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[],
        "known_gaps":["Original event argument/lifecycle/dispatch rules and object-event mapping","Live operand readiness and native form/type capabilities","Branch/arithmetic/native effects, execution continuations and campaign scenarios"]}))
}
