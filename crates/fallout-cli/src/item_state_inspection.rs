//! Mutable-item engineering proofs use disclosed host values and original keys.
use super::{Result, inspection_input::Order, script_state_inspection};
use fallout_data::{inventory, loaded_scripts};
use fallout_runtime::{
    Limits, World,
    foreign::{Content, SourceForm},
    identity::{CampaignId, ReferenceId},
    inventory::{
        Ammo, Condition, Facts, InventoryView, OpaqueExtra, Ownership, Page, PageLimits,
        PageRequest, TransferLimits, ViewLimits,
    },
    save::{Captured, Recovery, Repository},
    source_items::{Policy, Role, SourceInventoryLimits},
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
/// Actual bounded page consumer; the engineering view is only an independent
/// equality check, not the implementation used to discover each next lot.
fn pages(world: &World<'_>, owners: &[ReferenceId]) -> Result<Vec<Vec<Page>>> {
    let mut remaining = PageLimits {
        max_visited: 65_536,
        max_rows: 1,
        max_links: 1_000_000,
        max_extra_bytes: 16 * 1024 * 1024,
        max_copied_bytes: 32 * 1024 * 1024,
    };
    let mut page_count = 0_usize;
    let mut observations = Vec::new();
    for &owner in owners {
        let mut owner_pages = Vec::new();
        let mut after = None;
        loop {
            if page_count >= 65_536 {
                return Err("Inventory page consumer page bound".into());
            }
            let page = world.inventory_page(
                PageRequest {
                    owner,
                    after: after.as_ref(),
                    rows: 1,
                },
                remaining,
            )?;
            page_count += 1;
            let usage = page.usage();
            remaining.max_visited -= usage.visited;
            remaining.max_links -= usage.links;
            remaining.max_extra_bytes -= usage.extra_bytes;
            remaining.max_copied_bytes -= usage.copied_bytes;
            after = page.next_cursor().cloned();
            owner_pages.push(page);
            if after.is_none() {
                break;
            }
        }
        observations.push(owner_pages);
    }
    Ok(observations)
}
fn verify_pages(pages: &[Vec<Page>], views: &[InventoryView]) -> Result<()> {
    if pages.len() != views.len() {
        return Err("Inventory page owners differ".into());
    }
    for (owner_pages, view) in pages.iter().zip(views) {
        let mut items = Vec::new();
        let mut initialized = None;
        for page in owner_pages {
            if page.campaign() != view.campaign()
                || page.catalogue_fingerprint() != view.catalogue_fingerprint()
                || page.revision() != view.revision()
                || page.boundary() != view.boundary()
                || page.owner() != view.owner()
                || page.authored() != view.authored()
            {
                return Err("Inventory page binding differs".into());
            }
            let known = page.items().is_some();
            if initialized.is_some_and(|prior| prior != known) {
                return Err("Inventory page initialization changed".into());
            }
            initialized = Some(known);
            items.extend(page.items().unwrap_or_default().iter());
        }
        if initialized != Some(view.items().is_some())
            || items != view.items().unwrap_or_default().iter().collect::<Vec<_>>()
        {
            return Err("Inventory pages differ from exact canonical lots".into());
        }
    }
    Ok(())
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
    let content = Content::load(&mut store, &scripts, 1_000_000)?;
    let placed = store
        .winning_definitions()
        .find_map(|(key, location)| {
            let header = &store.definition(location).header;
            let source = SourceForm {
                kind: header.kind,
                flags: header.flags,
            };
            (source.is_placed() && header.flags & fallout_data::plugin::DELETED == 0)
                .then(|| key.clone())
        })
        .ok_or("Item initialization probe needs a nondeleted placed source owner")?;
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
    let transfer_after = engineering.world.snapshot();
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
        if total(&transfer_before) != total(&transfer_after) {
            return Err("Grouped transfer changed a per-base total".into());
        }
    }
    let placed_source = content.source_form(&engineering.world, &placed)?;
    let source_owner = engineering.world.register_reference(Some(placed.clone()))?;
    let mut starting_first = engineering.world.item(separate)?.facts().clone();
    starting_first.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abc,
    });
    let mut starting_second = starting_first.clone();
    starting_second.condition = Some(Condition::Float32 { bits: 0x8000_0000 });
    let starting_lots = [
        (starting_first, 11.try_into()?),
        (starting_second, 2.try_into()?),
    ];
    // These caller-supplied engineering rules admit the selected exact source
    // kinds for each used role. They make no claim about original item rules.
    let base_kind = content.source_form(&engineering.world, &keys[0])?.kind;
    let ammo_kind = content.source_form(&engineering.world, &keys[1])?.kind;
    let modification_kind = content.source_form(&engineering.world, &keys[2])?.kind;
    let source_policy = Policy::new(&[
        (Role::Base, &[base_kind]),
        (Role::Ammo, &[ammo_kind]),
        (Role::Modification, &[modification_kind]),
    ])?;
    let initialization_before = engineering.world.snapshot();
    let mut bad = starting_lots.clone();
    bad[1].0.base = placed.clone();
    if engineering
        .world
        .stage_source_inventory_initialization(
            &content,
            &source_policy,
            source_owner,
            &bad,
            SourceInventoryLimits::default(),
        )
        .is_ok()
        || engineering.world.snapshot() != initialization_before
    {
        return Err("Invalid final source lot partly initialized a bank".into());
    }
    let stage = engineering.world.stage_source_inventory_initialization(
        &content,
        &source_policy,
        source_owner,
        &starting_lots,
        SourceInventoryLimits::default(),
    )?;
    if engineering.world.snapshot() != initialization_before {
        return Err("Source inventory staging changed state".into());
    }
    let source_initialization = engineering.world.commit_source_inventory_initialization(
        &content,
        &source_policy,
        stage,
    )?;
    if source_initialization.before_revision() != initialization_before.state_revision
        || source_initialization.after_revision()
            != initialization_before
                .state_revision
                .checked_add(1)
                .ok_or("Source initialization revision exhausted")?
        || source_initialization.item_ids().len() != starting_lots.len()
    {
        return Err("Source inventory initialization boundary differs".into());
    }
    for (&id, (facts, count)) in source_initialization.item_ids().iter().zip(&starting_lots) {
        let item = engineering.world.item(id)?;
        if item.owner() != source_owner || item.facts() != facts || item.count() != count.get() {
            return Err("Source inventory starting lot differs".into());
        }
    }
    let expected = engineering.world.snapshot();
    let expected_traces = traces(&engineering.world, &[a, b, source_owner], &keys)?;
    let expected_views = views(&engineering.world, &[a, b, source_owner, absent])?;
    let expected_pages = pages(&engineering.world, &[a, b, source_owner, absent])?;
    verify_pages(&expected_pages, &expected_views)?;
    let restored = World::restore(&scripts, expected.clone(), Limits::default())?;
    if restored.snapshot() != expected
        || traces(&restored, &[a, b, source_owner], &keys)? != expected_traces
        || views(&restored, &[a, b, source_owner, absent])? != expected_views
        || serde_json::to_value(pages(&restored, &[a, b, source_owner, absent])?)?
            != serde_json::to_value(&expected_pages)?
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
    let before_world = World::restore(&scripts, initialization_before.clone(), Limits::default())?;
    let source_inventory_before_write = repository.commit(&Captured::at_boundary(&before_world))?;
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
        || traces(&loaded, &[a, b, source_owner], &keys)? != expected_traces
        || views(&loaded, &[a, b, source_owner, absent])? != expected_views
        || serde_json::to_value(pages(&loaded, &[a, b, source_owner, absent])?)?
            != serde_json::to_value(&expected_pages)?
    {
        return Err("Owned native item capture differs".into());
    }
    let bytes = expected.encode(Limits::default().max_snapshot_bytes)?;
    Ok(
        json!({"schema_version":1,"profile":"nv-original","sources":scripts.sources,"inventory_counts":base.counts,"source_item_inputs":selected.iter().map(|(_,value)|value).collect::<Vec<_>>(),
        "engineering_inputs":{"owners":[a,b,source_owner],"uninitialized_owner":absent,"item_keys":keys,"original_id":original,"separate_id":separate,"split_id":split,
        "counts":[17,3,5,2],"condition_bits":[0x7ff8_1234_5678_9abc_u64,0x7ff8_1234_5678_9abd_u64],"equipment_slots":[7,1],"opaque_extra":{"tag":"TEST","bytes":[0,255,1]}},
        "item_instances":expected.inventory_banks.iter().map(|b|b.items.len()).sum::<usize>(),"inventory_banks":expected.inventory_banks.len(),"script_instances":expected.instances.len(),
        "query_traces":expected_traces,"snapshot_bytes":bytes.len(),"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"state_schema":expected.schema_version,
        "inventory_views":expected_views,"all_inventory_views_equal_after_restore":true,
        "inventory_pages":expected_pages,"all_inventory_pages_equal_after_restore":true,
        "atomic_inventory_transfer":atomic_transfer,"atomic_inventory_transfer_invalid_last_preserved_state":true,
        "atomic_inventory_transfer_totals_and_facts_conserved":true,"native_before_write":native_before_write,
        "source_inventory_initialization":source_initialization,"source_inventory_before_write":source_inventory_before_write,
        "source_inventory_inputs":{"owner":source_owner,"authored":placed,"source":placed_source,"policy":source_policy,"lots":starting_lots},
        "source_inventory_invalid_last_preserved_uninitialized":true,"source_inventory_exact_lots_and_one_revision":true,
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
    let observations = views(&world, owners)?;
    let paged = pages(&world, owners)?;
    verify_pages(&paged, &observations)?;
    Ok(
        json!({"schema_version":1,"receipt":receipt,"query_traces":traces(&world,owners,keys)?,"snapshot_sha256":format!("{:x}",Sha256::digest(&bytes)),"snapshot_bytes":bytes.len(),
        "inventory_views":observations,"inventory_pages":paged,"all_inventory_pages_equal":true,
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
