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
    identity::{CampaignId, Owner, ReferenceValue, Value},
    schema::{self, Kind},
    snapshot::Snapshot,
};
use serde::Deserialize;
use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};

/// A bounded explicit inspector request, not a persisted script continuation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EngineeringCommit {
    sequence: u64,
    assignments: Vec<fallout_runtime::snapshot::Local>,
    acknowledge: bool,
}
impl EngineeringCommit {
    pub(super) fn read(path: &Path) -> Result<Self> {
        const MAXIMUM_BYTES: u64 = 64 * 1024;
        let file = fallout_data::baseline::open_source(path)?;
        if file.metadata()?.len() > MAXIMUM_BYTES {
            return Err("Engineering event-commit input byte budget exceeded".into());
        }
        let mut bytes = Vec::new();
        file.take(MAXIMUM_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAXIMUM_BYTES {
            return Err("Engineering event-commit input byte budget exceeded".into());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub(super) fn stage(
        self,
        world: &World<'_>,
    ) -> fallout_runtime::Result<fallout_runtime::state::event_commit::StagedEventChanges> {
        let assignments: Vec<_> = self
            .assignments
            .into_iter()
            .map(|local| (local.index, local.value))
            .collect();
        world.stage_event_changes(self.sequence, &assignments, self.acknowledge)
    }
}

pub(super) struct EngineeringWorld<'a> {
    pub world: World<'a>,
    pub handles: Vec<(
        fallout_runtime::identity::InstanceId,
        fallout_runtime::state::InstanceHandle,
    )>,
    pub numbers: u64,
    pub references: u64,
    pub unknown: u64,
}

/// Shared deterministic harness inputs. This does not initialize a game session.
pub(super) fn engineering_world(catalogue: &Catalogue) -> Result<EngineeringWorld<'_>> {
    let limits = Limits::default();
    let mut world = World::with_campaign(catalogue, limits, CampaignId::from_bytes([0x28; 16])?)?;
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
    Ok(EngineeringWorld {
        world,
        handles,
        numbers,
        references,
        unknown,
    })
}
fn probe(catalogue: &Catalogue) -> Result<Json> {
    let EngineeringWorld {
        world,
        handles,
        numbers,
        references,
        unknown,
    } = engineering_world(catalogue)?;
    let limits = Limits::default();
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

pub(super) fn event_commit_probe(
    world: &mut World<'_>,
    request: EngineeringCommit,
) -> Result<Json> {
    let before = world.snapshot();
    let stage = request.stage(world)?;
    let assignments = stage.assignments().to_vec();
    let acknowledge = stage.acknowledges();
    if world.snapshot() != before {
        return Err("Staging changed canonical state".into());
    }
    let instance_id = stage.instance();
    let mut expected = before.clone();
    let instance = expected
        .instances
        .iter_mut()
        .find(|instance| instance.id == instance_id)
        .ok_or("Staged instance missing from snapshot")?;
    for (index, value) in &assignments {
        instance
            .locals
            .iter_mut()
            .find(|local| local.index == *index)
            .ok_or("Staged local missing from snapshot")?
            .value = value.clone();
    }
    expected.state_revision = expected
        .state_revision
        .checked_add(1)
        .ok_or("State revision exhausted")?;
    if acknowledge {
        expected.pending_events.remove(0);
    }
    let receipt = world.commit_event_changes(stage)?;
    let after = world.snapshot();
    if after != expected {
        return Err("Staged event commit differs from requested canonical changes".into());
    }
    let limits = Limits::default();
    for snapshot in [&before, &after] {
        let bytes = snapshot.encode(limits.max_snapshot_bytes)?;
        let restored =
            World::restore(world.catalogue(), Snapshot::decode(&bytes, limits)?, limits)?;
        if restored.snapshot() != *snapshot {
            return Err("Staged event boundary restoration differs".into());
        }
    }
    Ok(json!({
        "scope":"Explicit engineering typed-local/head transaction; no bytecode execution or original timing claim",
        "receipt":receipt,"requested_assignments":assignments,
        "before_snapshot_sha256":format!("{:x}",Sha256::digest(before.encode(limits.max_snapshot_bytes)?)),
        "after_snapshot_sha256":format!("{:x}",Sha256::digest(after.encode(limits.max_snapshot_bytes)?)),
        "pending_before":before.pending_events.len(),"pending_after":after.pending_events.len(),
        "staging_changed_state":false,"only_requested_changes":true,
        "before_after_restoration_equal":true,"bytecode_executed":false,"retail_parity_accepted":false
    }))
}

pub(super) fn inspect(
    install: &Path,
    order_path: &Path,
    cache: Option<&Path>,
    engineering_event_commit: Option<&Path>,
) -> Result<Json> {
    // Bound and validate the opt-in request before loading the content corpus.
    let request = engineering_event_commit
        .map(EngineeringCommit::read)
        .transpose()?;
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
    let mut report = json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources,"metadata":metadata,
        "counts":{"candidate_records":catalogue.counts.candidate_records_read+catalogue.counts.deleted_candidates_skipped,
            "deleted_candidate_records":catalogue.counts.deleted_candidates_skipped,"decoded_candidate_bytes":catalogue.counts.payload_bytes_scanned,
            "scripts":catalogue.counts.scripts,"unique_locals":count,"duplicate_declarations":catalogue.counts.duplicate_variable_indices,"local_kinds":kinds},
        "records":records,"state_probe":state_probe,"explicit_load_order":order.names,"load_order_sha256":order.sha256,
        "index_cache":store.index_cache_report(),"catalogue_source_findings":catalogue.counts.scripts_with_issues,
        "constructor_defaults_verified":false,"retail_parity_accepted":false});
    if let Some(request) = request {
        let mut harness = engineering_world(&catalogue)?;
        report["engineering_event_commit"] = event_commit_probe(&mut harness.world, request)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static NEXT_INPUT: AtomicU64 = AtomicU64::new(1);
    struct InputFile(PathBuf);
    impl InputFile {
        fn new() -> Self {
            let root = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("target"));
            std::fs::create_dir_all(&root).unwrap();
            let path = root.join(format!(
                "runtime-event-commit-input-{}-{}-{}.json",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_INPUT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap();
            Self(path)
        }
    }
    impl Drop for InputFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn explicit_event_commit_input_preserves_bits_and_requires_every_field() {
        let file = InputFile::new();
        let path = &file.0;
        std::fs::write(path, br#"{"sequence":1,"assignments":[{"index":42,"value":{"kind":"number","bits":18446744073709551615}}],"acknowledge":true}"#).unwrap();
        let input = EngineeringCommit::read(path).unwrap();
        assert_eq!(input.sequence, 1);
        assert_eq!(input.assignments[0].value, Value::Number { bits: u64::MAX });
        assert!(input.acknowledge);
        for bytes in [
            br#"{"sequence":1,"assignments":[],"acknowledge":false,"unknown":true}"#.as_slice(),
            br#"{"sequence":1,"sequence":2,"assignments":[],"acknowledge":false}"#,
            br#"{"sequence":1,"assignments":[]}"#,
            br#"{"sequence":1,"assignments":[],"acknowledge":null}"#,
            br#"{"sequence":1,"assignments":[{"index":42,"value":{"kind":"number","bits":18446744073709551616}}],"acknowledge":true}"#,
        ] {
            std::fs::write(path, bytes).unwrap();
            assert!(EngineeringCommit::read(path).is_err());
        }
    }

    #[test]
    fn explicit_event_commit_input_byte_budget_fails_for_the_intended_reason() {
        let file = InputFile::new();
        let path = &file.0;
        std::fs::write(path, vec![b' '; 65_537]).unwrap();
        assert_eq!(
            EngineeringCommit::read(path).err().unwrap().to_string(),
            "Engineering event-commit input byte budget exceeded"
        );
    }
}
