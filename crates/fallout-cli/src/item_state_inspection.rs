//! Mutable-item engineering proofs use disclosed host values and original keys.
use super::{Result, inspection_input::Order, script_state_inspection};
use fallout_data::{inventory, loaded_scripts};
use fallout_runtime::{
    Limits, World,
    identity::{CampaignId, ReferenceId},
    inventory::{
        Ammo, Condition, Facts, InventoryView, OpaqueExtra, Ownership, TransferLimits, ViewLimits,
    },
    save::{Captured, Recovery, Repository},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
fn traces(
    world: &World<'_>,
    owners: &[ReferenceId],
    items: &[fallout_data::identity::FormKey],
) -> Result<Value> {
    let mut values = Vec::new();
    let mut remaining = 65_536;
    for &owner in owners {
        for item in items {
            let trace = world.inventory_count_trace_bounded(owner, item, remaining)?;
            remaining -= trace.contributions.len();
            values.push(trace);
        }
    }
    Ok(serde_json::to_value(values)?)
}
fn views(world: &World<'_>, owners: &[ReferenceId]) -> Result<Vec<InventoryView>> {
    let mut remaining = ViewLimits {
        max_items: 65_536,
        max_links: 1_000_000,
        max_extra_bytes: 16 * 1024 * 1024,
    };
    let mut observations = Vec::new();
    for &owner in owners {
        let view = world.inventory_view(owner, remaining)?;
        let usage = view.usage();
        remaining.max_items -= usage.items;
        remaining.max_links -= usage.links;
        remaining.max_extra_bytes -= usage.extra_bytes;
        observations.push(view);
    }
    Ok(observations)
}
pub(super) fn probe(install: &Path, order_path: &Path, repository_path: &Path) -> Result<Value> {
    eprintln!("Item state: reading original script and inventory identities...");
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let base = inventory::Catalogue::load(&mut store, inventory::Limits::default())?;
    let mut candidates = BTreeMap::new();
    for (key, d) in base.iter() {
        for field in &d.fields {
            if let inventory::Value::Item {
                item,
                schema_kind_allowed: Some(true),
                ..
            } = &field.value
                && item.status == inventory::Status::Defined
                && item.target.as_ref().is_some_and(|t| t.kind != *b"LVLI")
                && let Some(target) = &item.key
            {
                candidates.entry(target.clone()).or_insert(json!({"parent":key,"field_decoded_offset":field.decoded_offset,"binding":item}));
            }
        }
    }
    let selected = candidates.into_iter().take(3).collect::<Vec<_>>();
    if selected.len() != 3 {
        return Err("Item probe needs three defined terminal source items".into());
    }
    let keys = selected
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let mut engineering = script_state_inspection::engineering_world(&scripts)?;
    // A distinct campaign keeps this probe's persistent counters separate.
    let mut snapshot = engineering.world.snapshot();
    snapshot.campaign = CampaignId::from_bytes([0x33; 16])?;
    engineering.world = World::restore(&scripts, snapshot, Limits::default())?;
    let a = engineering.world.register_reference(None)?;
    let b = engineering.world.register_reference(None)?;
    let absent = engineering.world.register_reference(None)?;
    if engineering.world.inventory_count(absent, &keys[0]).is_ok() {
        return Err("Uninitialized inventory manufactured a zero".into());
    }
    engineering.world.initialize_inventory(a)?;
    engineering.world.initialize_inventory(b)?;
    let mut facts = Facts::unknown(keys[0].clone());
    facts.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abc,
    });
    facts.ownership = Some(Ownership::Live { reference: a });
    facts.equipped_slots = Some(vec![7, 1]);
    facts.ammo = Some(Ammo {
        base: keys[1].clone(),
        count: 0,
    });
    facts.modifications = Some(vec![keys[2].clone()]);
    facts.quest_item = Some(false);
    facts.script_instance = engineering.handles.first().map(|(id, _)| *id);
    facts.extra_fields = vec![OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 255, 1],
    }];
    let original = engineering
        .world
        .add_item(a, facts.clone(), 17.try_into()?)?;
    let mut second = facts.clone();
    second.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let separate = engineering.world.add_item(a, second, 3.try_into()?)?;
    let split = engineering.world.split_item(original, 5.try_into()?)?;
    engineering.world.transfer_item(split, b)?;
    engineering
        .world
        .remove_item_quantity(split, 2.try_into()?)?;
    let unchanged = engineering.world.snapshot();
    if engineering.world.transfer_item(split, absent).is_ok()
        || engineering
            .world
            .split_item(original, 12.try_into()?)
            .is_ok()
    {
        return Err("Invalid item operation succeeded".into());
    }
    if engineering.world.snapshot() != unchanged {
        return Err("Rejected item operation mutated state".into());
    }
    facts.base = keys[2].clone();
    engineering.world.replace_item_facts(original, facts)?;
    let transfer_before = engineering.world.snapshot();
    let transfers = [(original, b), (separate, b), (split, a)];
    if engineering
        .world
        .stage_inventory_transfers(
            &[(original, b), (separate, absent)],
            TransferLimits::default(),
        )
        .is_ok()
        || engineering.world.snapshot() != transfer_before
    {
        return Err("Invalid final batch destination partly transferred inventory".into());
    }
    let stage = engineering
        .world
        .stage_inventory_transfers(&transfers, TransferLimits::default())?;
    if engineering.world.snapshot() != transfer_before {
        return Err("Inventory staging changed state".into());
    }
    let atomic_transfer = engineering.world.commit_inventory_transfers(stage)?;
    if atomic_transfer.before_revision() != transfer_before.state_revision
        || atomic_transfer.after_revision()
            != transfer_before
                .state_revision
                .checked_add(1)
                .ok_or("Transfer revision exhausted")?
        || atomic_transfer.usage().moved_rows != 3
    {
        return Err("Grouped inventory transfer did not publish exactly one revision".into());
    }
    for row in atomic_transfer.changes() {
        let current = engineering.world.item(row.original().id())?;
        if current.owner() != row.target()
            || current.count() != row.original().count()
            || current.facts() != row.original().facts()
        {
            return Err("Grouped transfer changed an original lot identity/quantity/fact".into());
        }
    }
    let expected = engineering.world.snapshot();
    for key in &keys {
        let total = |snapshot: &fallout_runtime::snapshot::Snapshot| {
            snapshot
                .inventory_banks
                .iter()
                .flat_map(|bank| &bank.items)
                .filter(|item| item.facts().base == *key)
                .map(|item| u64::from(item.count()))
                .sum::<u64>()
        };
        if total(&transfer_before) != total(&expected) {
            return Err("Grouped transfer changed a per-base total".into());
        }
    }
    let expected_traces = traces(&engineering.world, &[a, b], &keys)?;
    let expected_views = views(&engineering.world, &[a, b, absent])?;
    let restored = World::restore(&scripts, expected.clone(), Limits::default())?;
    if restored.snapshot() != expected
        || traces(&restored, &[a, b], &keys)? != expected_traces
        || views(&restored, &[a, b, absent])? != expected_views
    {
        return Err("Canonical item restoration differs".into());
    }
    let repository = Repository::create(
        repository_path,
        &[install.into()],
        engineering.world.campaign(),
    )?;
    let before_world = World::restore(&scripts, transfer_before.clone(), Limits::default())?;
    let native_before_write = repository.commit(&Captured::at_boundary(&before_world))?;
    drop(before_world);
    let capture = Captured::at_boundary(&engineering.world);
    let worker_repository = repository.clone();
    let worker = std::thread::spawn(move || worker_repository.commit(&capture));
    engineering
        .world
        .remove_item_quantity(original, 1.try_into()?)?;
    let write = worker.join().map_err(|_| "Item save worker panicked")??;
    let (loaded, receipt) = repository.load(&scripts, Limits::default(), Recovery::Strict)?;
    if loaded.snapshot() != expected
        || traces(&loaded, &[a, b], &keys)? != expected_traces
        || views(&loaded, &[a, b, absent])? != expected_views
    {
        return Err("Owned native item capture differs".into());
    }
    let bytes = expected.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":scripts.sources,"inventory_counts":base.counts,"source_item_inputs":selected.iter().map(|(_,value)|value).collect::<Vec<_>>(),
        "engineering_inputs":{"owners":[a,b],"uninitialized_owner":absent,"item_keys":keys,"original_id":original,"separate_id":separate,"split_id":split,
        "counts":[17,3,5,2],"condition_bits":[0x7ff8_1234_5678_9abc_u64,0x7ff8_1234_5678_9abd_u64],"equipment_slots":[7,1],"opaque_extra":{"tag":"TEST","bytes":[0,255,1]}},
        "item_instances":expected.inventory_banks.iter().map(|b|b.items.len()).sum::<usize>(),"inventory_banks":expected.inventory_banks.len(),"script_instances":expected.instances.len(),
        "query_traces":expected_traces,"snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"state_schema":expected.schema_version,
        "inventory_views":expected_views,"all_inventory_views_equal_after_restore":true,
        "atomic_inventory_transfer":atomic_transfer,"atomic_inventory_transfer_invalid_last_preserved_state":true,
        "atomic_inventory_transfer_totals_and_facts_conserved":true,"native_before_write":native_before_write,
        "canonical_state_round_trip_equal":true,"all_query_traces_equal_after_restore":true,"rejected_mutations_preserved_state":true,"uninitialized_inventory_rejected":true,"worker_capture_isolated":true,
        "native_write":write,"native_load":receipt,"original_live_values_captured":false,"original_item_admission_verified":false,"bytecode_executed":false,"retail_parity_accepted":false,"accepted_scenarios":[]}),
    )
}
pub(super) fn cold(
    install: &Path,
    order_path: &Path,
    repository_path: &Path,
    owners: &[ReferenceId],
    keys: &[fallout_data::identity::FormKey],
) -> Result<Value> {
    eprintln!("Item restore: binding original sources and loading canonical state...");
    let order = Order::read(order_path)?;
    let mut store = order.store(install, None)?;
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, loaded_scripts::Limits::default(), |_, _| {
            Ok(())
        })?;
    let repository = Repository::open(repository_path, &[install.into()])?;
    let (world, receipt) = repository.load(&scripts, Limits::default(), Recovery::Strict)?;
    let snapshot = world.snapshot();
    let bytes = snapshot.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"receipt":receipt,"query_traces":traces(&world,owners,keys)?,"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"snapshot_bytes":bytes.len(),
        "inventory_views":views(&world,owners)?,
        "item_instances":snapshot.inventory_banks.iter().map(|b|b.items.len()).sum::<usize>(),"inventory_banks":snapshot.inventory_banks.len(),"state_schema":snapshot.schema_version,"source_bound_restore":true,"retail_parity_accepted":false}),
    )
}
pub(super) fn decode_query_inputs(
    path: &Path,
) -> Result<(Vec<ReferenceId>, Vec<fallout_data::identity::FormKey>)> {
    use std::io::Read;
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Inputs {
        owners: Vec<ReferenceId>,
        item_keys: Vec<fallout_data::identity::FormKey>,
    }
    let source = fallout_data::baseline::open_source(path)?;
    let mut bytes = Vec::new();
    source.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err("Item query inputs exceed 64 KiB".into());
    }
    let inputs: Inputs = serde_json::from_slice(&bytes)?;
    if inputs.owners.is_empty()
        || inputs.item_keys.is_empty()
        || inputs.owners.len() > 16
        || inputs.item_keys.len() > 16
    {
        return Err("Item query input cardinality is outside 1..=16".into());
    }
    Ok((inputs.owners, inputs.item_keys))
}
