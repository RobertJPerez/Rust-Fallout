//! Both entry adapters query explicit host state. No original handler executes.
use super::{Result, command_catalogue, inspection_input::Order, source_item_inspection};
use fallout_data::{identity::FormKey, loaded_scripts};
use fallout_runtime::{
    Limits, World,
    foreign::Content,
    identity::{ReferenceId, ReferenceValue, Value as LocalValue},
    query::{Entry, GET_ITEM_COUNT_COMMAND, GET_ITEM_COUNT_CONDITION, Request},
    save::{Recovery, Repository},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
fn calls(
    world: &World<'_>,
    content: &Content,
    owners: &[ReferenceId],
    keys: &[FormKey],
) -> Result<Value> {
    let mut traces = Vec::new();
    let mut remaining = 65_536;
    for &owner in owners {
        for key in keys {
            let arguments = [LocalValue::Reference {
                value: ReferenceValue::Content { key: key.clone() },
            }];
            let mut pair = Vec::new();
            for entry in [
                Entry::Native {
                    command_id: GET_ITEM_COUNT_COMMAND,
                },
                Entry::Condition {
                    function_id: GET_ITEM_COUNT_CONDITION,
                },
            ] {
                let trace = Request::prepare(world, entry, Some(owner), &arguments)?
                    .evaluate(world, content, remaining)?;
                remaining -= trace.query.contributions.len();
                pair.push(trace);
            }
            if serde_json::to_value(&pair[0].query)? != serde_json::to_value(&pair[1].query)? {
                return Err("Shared query adapters diverged".into());
            }
            traces.extend(pair);
        }
    }
    Ok(serde_json::to_value(traces)?)
}
fn inspect(
    install: &Path,
    order_path: &Path,
    repository_path: &Path,
    owners: &[ReferenceId],
    keys: &[FormKey],
) -> Result<Value> {
    eprintln!("Primitive queries: validating original descriptor and shared host traces...");
    let native = command_catalogue::inspect(&install.join("FalloutNV.exe"))?;
    let descriptor = native
        .script_commands
        .iter()
        .find(|d| d.id == u32::from(GET_ITEM_COUNT_COMMAND))
        .ok_or("GetItemCount descriptor missing")?;
    if descriptor.name != "GetItemCount"
        || descriptor.table_index != usize::from(GET_ITEM_COUNT_CONDITION)
        || descriptor.stored_opcode != u32::from(GET_ITEM_COUNT_COMMAND)
        || descriptor.needs_parent_word != 1
        || !descriptor.condition_handler_present
        || descriptor.parameters.len() != 1
        || descriptor.parameters[0].type_id != 50
        || descriptor.parameters[0].optional_word != 0
    {
        return Err("GetItemCount descriptor contract changed".into());
    }
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let repository = Repository::open(repository_path, &[install.into()])?;
    let (world, receipt) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    let before = world.snapshot();
    let traces = calls(&world, &content, owners, keys)?;
    if world.snapshot() != before {
        return Err("Read-only query changed canonical state".into());
    }
    let restored = World::restore(&catalogue, before.clone(), Limits::default())?;
    if calls(&restored, &content, owners, keys)? != traces {
        return Err("Shared query traces differ after restore".into());
    }
    let arguments = [LocalValue::Reference {
        value: ReferenceValue::Content {
            key: keys.first().ok_or("Missing query item")?.clone(),
        },
    }];
    if Request::prepare(
        &world,
        Entry::Native { command_id: 1 },
        owners.first().copied(),
        &arguments,
    )
    .is_ok()
        || Request::prepare(
            &world,
            Entry::Native {
                command_id: GET_ITEM_COUNT_COMMAND,
            },
            None,
            &arguments,
        )
        .is_ok()
    {
        return Err("Unsupported query or implicit subject accepted".into());
    }
    let bytes = before.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","executable_sha256":native.source_sha256,"descriptor":descriptor,
        "sources":catalogue.sources,"source_content":content.report(),"engineering_inputs":{"owners":owners,"item_keys":keys},"traces":traces,
        "all_adapter_core_traces_equal":true,"all_same_process_traces_equal":true,"canonical_state_unchanged":true,"unsupported_entry_and_implicit_subject_rejected":true,
        "receipt":receipt,"snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"scope":"Shared GetItemCount source entry IDs over explicit host state; argument binding/subject supplied by caller",
        "original_numeric_return_verified":false,"original_argument_coercion_verified":false,"original_handler_executed":false,"bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
pub(super) fn probe(install: &Path, order_path: &Path, destination: &Path) -> Result<Value> {
    let prior = source_item_inspection::probe(install, order_path, destination)?;
    let owners: Vec<ReferenceId> =
        serde_json::from_value(prior["engineering_inputs"]["owners"].clone())?;
    let keys: Vec<FormKey> =
        serde_json::from_value(prior["engineering_inputs"]["item_keys"].clone())?;
    inspect(install, order_path, destination, &owners, &keys)
}
pub(super) fn cold(
    install: &Path,
    order_path: &Path,
    repository_path: &Path,
    owners: &[ReferenceId],
    keys: &[FormKey],
) -> Result<Value> {
    inspect(install, order_path, repository_path, owners, keys)
}
