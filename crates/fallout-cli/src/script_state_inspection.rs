//! Inspect source-less local declarations and exercise canonical runtime state
//! using explicit synthetic values. No retail event list is captured or run.
use super::{Result, inspection_input::Order};
use fallout_data::{
    loaded_scripts::{self, Catalogue},
    record_metadata,
};
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{Owner, ReferenceValue, Value},
    schema::{self, Kind},
    snapshot::Snapshot,
};
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

fn probe(catalogue: &Catalogue) -> Result<Json> {
    let limits = Limits::default();
    let mut world = World::new(catalogue, limits)?;
    let reference = world.register_reference(None)?;
    world.advance_clocks(Clocks {
        tick: 1,
        game_nanoseconds: 100,
        menu_nanoseconds: 20,
        real_nanoseconds: 120,
    })?;
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Null],
    };
    let mut handles = Vec::new();
    let mut numbers = 0_u64;
    let mut references = 0_u64;
    let mut unknown = 0_u64;
    let patterns = [
        0,
        1,
        u64::MAX,
        0x8000000000000000,
        0x7ff0000000000000,
        0x7ff8123456789abc,
        0xfff0123456789abc,
        0x4340000000000001,
    ];
    for (_, script) in catalogue.iter() {
        let locals = schema::locals(script);
        if locals.is_empty() {
            continue;
        }
        let activation = (handles.len() as u64 + 1).try_into()?;
        let handle = world.create_instance(
            script.handle(),
            Owner::Fragment { activation },
            context.clone(),
        )?;
        let mut assignments = Vec::with_capacity(locals.len());
        for local in locals.values() {
            let value = match local.kind {
                Kind::Float | Kind::Integer => {
                    let bits = patterns[numbers as usize % patterns.len()];
                    numbers += 1;
                    Value::Number { bits }
                }
                Kind::Reference => {
                    references += 1;
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    }
                }
                _ => {
                    unknown += 1;
                    continue;
                }
            };
            assignments.push((local.index, value));
        }
        world.assign(handle, &assignments)?;
        if let Some(instruction) = script
            .program()?
            .iter()
            .flat_map(|program| program.instructions.iter())
            .find(|instruction| instruction.event.is_some())
        {
            let event = instruction.event.expect("selected event");
            world.enqueue(
                handle,
                Trigger::Block {
                    event_id: event.id,
                    begin_byte_offset: instruction.bytes.start as u32,
                },
                context.clone(),
            )?;
        }
        handles.push((world.instance(handle)?.id(), handle));
    }
    let snapshot = world.snapshot();
    let bytes = snapshot.encode(limits.max_snapshot_bytes)?;
    let restored = World::restore(catalogue, Snapshot::decode(&bytes, limits)?, limits)?;
    if restored.snapshot() != snapshot
        || restored.snapshot().encode(limits.max_snapshot_bytes)? != bytes
    {
        return Err("Canonical state round trip differs".into());
    }
    for (id, old) in &handles {
        if restored.instance(*old).is_ok() || restored.instance(restored.handle(*id)?)?.id() != *id
        {
            return Err("Restored runtime handle identity checks failed".into());
        }
    }
    Ok(
        json!({"scope":"Engineering inputs on original compiled declaration schemas; no original running event lists or initialization defaults",
        "instances":world.instance_count(),"numeric_values":numbers,"typed_reference_values":references,
        "unsupported_slots_retained_uninitialized":unknown,"pending_events":world.pending_events().len(),
        "snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),
        "catalogue_sha256":world.catalogue_fingerprint(),"canonical_bytes_equal":true,
        "old_handles_rejected":true,"persistent_ids_preserved":true,
        "original_state_captured":false,"bytecode_executed":false,"retail_parity_accepted":false}),
    )
}

pub(super) fn inspect(install: &Path, order_path: &Path, cache: Option<&Path>) -> Result<Json> {
    let order = Order::read(order_path)?;
    let mut store = order.store(install, cache)?;
    let metadata = record_metadata::inspect(&store)?;
    let mut payloads = BTreeMap::new();
    eprintln!(
        "Inspecting compiled local schemas and canonical state across {} plugins",
        order.names.len()
    );
    let catalogue = Catalogue::load(
        &mut store,
        loaded_scripts::Limits::default(),
        |source, record| {
            payloads.insert(
                (order.names[source].clone(), record.header.offset),
                (record.header.kind, record.payload.len()),
            );
            Ok(())
        },
    )?;
    let mut records = Vec::new();
    let mut kinds = BTreeMap::<String, u64>::new();
    let mut count = 0_u64;
    for (key, script) in catalogue.iter() {
        let locals = schema::locals(script).into_values().collect::<Vec<_>>();
        for local in &locals {
            let name = serde_json::to_value(local.kind)?["kind"]
                .as_str()
                .ok_or("Local kind name")?
                .to_string();
            *kinds.entry(name).or_default() += 1;
            count += 1;
        }
        let unit = json!({"header_decoded_offset":key.header_decoded_offset,"locals":locals});
        if records
            .last()
            .is_some_and(|record: &Json| record["key"] == json!(key.record))
        {
            records.last_mut().expect("matching record")["units"]
                .as_array_mut()
                .expect("unit rows")
                .push(unit);
        } else {
            let version = script.version();
            let &(kind, bytes) = payloads
                .get(&(version.source_plugin.clone(), version.record_file_offset))
                .ok_or("Retained script payload provenance")?;
            records.push(json!({"key":key.record,"source_name":version.source_plugin,
                "record_kind":std::str::from_utf8(&kind)?,"record_file_offset":version.record_file_offset,
                "record_flags":version.record_flags,"decoded_bytes":bytes,"decoded_sha256":version.decoded_record_sha256,"units":[unit]}));
        }
    }
    let state_probe = probe(&catalogue)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources,"metadata":metadata,
        "counts":{"candidate_records":catalogue.counts.candidate_records_read+catalogue.counts.deleted_candidates_skipped,
            "deleted_candidate_records":catalogue.counts.deleted_candidates_skipped,"decoded_candidate_bytes":catalogue.counts.payload_bytes_scanned,
            "scripts":catalogue.counts.scripts,"unique_locals":count,"duplicate_declarations":catalogue.counts.duplicate_variable_indices,"local_kinds":kinds},
        "records":records,"state_probe":state_probe,"explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "index_cache":store.index_cache_report(),"catalogue_source_findings":catalogue.counts.scripts_with_issues,
        "constructor_defaults_verified":false,"retail_parity_accepted":false}),
    )
}
