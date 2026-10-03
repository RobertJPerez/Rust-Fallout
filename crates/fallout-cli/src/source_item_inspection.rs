//! Caller-defined source rules stay separate from original admission behavior.
use super::{Result, inspection_input::Order, item_state_inspection};
use fallout_data::{identity::FormKey, loaded_scripts, plugin};
use fallout_runtime::{
    Limits, World,
    foreign::{Content, Failure as SourceFailure},
    identity::ReferenceId,
    inventory::Facts,
    save::{Captured, Recovery, Repository},
    source_items::{self, Policy, Role},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

fn policy(content: &Content, world: &World<'_>, keys: &[FormKey]) -> Result<Policy> {
    if keys.len() != 3 {
        return Err("Source item probe requires exactly three disclosed keys".into());
    }
    let kinds = keys
        .iter()
        .map(|k| content.source_form(world, k).map(|f| f.kind))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?
        .into_iter()
        .collect::<Vec<_>>();
    let ammo = [content.source_form(world, &keys[1])?.kind];
    let modification = [content.source_form(world, &keys[2])?.kind];
    Ok(Policy::new(&[
        (Role::Base, &kinds),
        (Role::Ammo, &ammo),
        (Role::Modification, &modification),
    ])?)
}
fn observations(
    world: &World<'_>,
    content: &Content,
    rules: &Policy,
    owners: &[ReferenceId],
    keys: &[FormKey],
) -> Result<Value> {
    let mut items = Vec::new();
    let mut queries = Vec::new();
    let mut remaining = 65_536;
    for &owner in owners {
        for item in world.inventory_items(owner)? {
            let proof = source_items::validate(world, content, rules, item.facts())?;
            if proof.forms.len() > remaining {
                return Err("Source item proof contribution budget".into());
            }
            remaining -= proof.forms.len();
            items.push(json!({"id":item.id(),"count":item.count(),"proof":proof}));
        }
        for key in keys {
            let trace = world.inventory_count_trace_bounded(owner, key, remaining)?;
            remaining -= trace.contributions.len();
            queries.push(trace);
        }
    }
    Ok(json!({"items":items,"queries":queries}))
}
pub(super) fn probe(install: &Path, order_path: &Path, destination: &Path) -> Result<Value> {
    let prior = item_state_inspection::probe(install, order_path, destination)?;
    eprintln!("Source item validation: checking winning headers and explicit host rules...");
    let keys: Vec<FormKey> =
        serde_json::from_value(prior["engineering_inputs"]["item_keys"].clone())?;
    let owners: Vec<ReferenceId> =
        serde_json::from_value(prior["engineering_inputs"]["owners"].clone())?;
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let repository = Repository::open(destination, &[install.into()])?;
    let (mut world, _) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    let rules = policy(&content, &world, &keys)?;
    let before = world.snapshot();
    let mut missing = None;
    for offset in 0..512 {
        let mut key = keys[0].clone();
        key.local_id = 0x00ff_ffff - offset;
        match content.source_form(&world, &key) {
            Err(SourceFailure::MissingForm(_)) => {
                missing = Some(key);
                break;
            }
            Ok(_) | Err(SourceFailure::DeletedForm(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let missing = missing.ok_or("No missing key within engineering search budget")?;
    let deleted = store
        .winning_definitions()
        .find(|(_, location)| store.definition(*location).header.flags & plugin::DELETED != 0)
        .map(|(key, _)| key.clone())
        .ok_or("Source probe requires a documented deleted winner")?;
    for key in [&missing, &deleted] {
        if world
            .add_source_item(
                &content,
                &rules,
                owners[0],
                Facts::unknown(key.clone()),
                1.try_into()?,
            )
            .is_ok()
        {
            return Err("Invalid source item accepted".into());
        }
        if world.snapshot() != before {
            return Err("Rejected source item changed canonical state".into());
        }
    }
    // QUST is an explicitly chosen rejecting test rule, not a retail item domain.
    let rejecting = Policy::new(&[(Role::Base, &[*b"QUST"])])?;
    if world
        .add_source_item(
            &content,
            &rejecting,
            owners[0],
            Facts::unknown(keys[0].clone()),
            1.try_into()?,
        )
        .is_ok()
        || world.snapshot() != before
    {
        return Err("Disallowed source kind changed canonical state".into());
    }
    let (id, addition) = world.add_source_item(
        &content,
        &rules,
        owners[0],
        Facts::unknown(keys[0].clone()),
        9.try_into()?,
    )?;
    let replacement =
        world.replace_source_item_facts(&content, &rules, id, Facts::unknown(keys[2].clone()))?;
    let expected = world.snapshot();
    let observed = observations(&world, &content, &rules, &owners, &keys)?;
    let write = repository.commit(&Captured::at_boundary(&world))?;
    let (restored, _) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    if restored.snapshot() != expected
        || observations(&restored, &content, &rules, &owners, &keys)? != observed
    {
        return Err("Source validated item restoration differs".into());
    }
    let bytes = expected.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":catalogue.sources,"source_content":content.report(),"policy":rules,
        "engineering_inputs":{"owners":owners,"item_keys":keys,"added_quantity":9,"missing_key":missing,"deleted_key":deleted,"rejecting_base_kind":[81,85,83,84]},
        "source_item_inputs":prior["source_item_inputs"],"mutation_proofs":[addition,replacement],"observations":observed,"all_same_process_observations_equal":true,"rejected_source_mutations_preserved_state":true,
        "native_write":write,"snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"state_schema":expected.schema_version,
        "scope":"Explicit host source-kind checks, immutable header identities and atomic item mutations; no original item admission behavior",
        "original_item_admission_verified":false,"original_live_values_captured":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
pub(super) fn cold(
    install: &Path,
    order_path: &Path,
    repository_path: &Path,
    owners: &[ReferenceId],
    keys: &[FormKey],
) -> Result<Value> {
    eprintln!("Source item restore: validating exact source cohort and item facts...");
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let catalogue =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let content = Content::load(&mut store, &catalogue, 1_000_000)?;
    let repository = Repository::open(repository_path, &[install.into()])?;
    let (world, receipt) = repository.load(&catalogue, Limits::default(), Recovery::Strict)?;
    let rules = policy(&content, &world, keys)?;
    let bytes = world
        .snapshot()
        .encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"receipt":receipt,"policy":rules,"source_content":content.report(),"observations":observations(&world,&content,&rules,owners,keys)?,
        "snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"source_bound_restore":true,"original_item_admission_verified":false,"retail_parity_accepted":false}),
    )
}
