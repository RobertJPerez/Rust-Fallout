mod common;
use common::*;
use fallout_data::loaded_scripts::Catalogue;
use fallout_runtime::{
    Error, Limits, World,
    events::{Context, Trigger},
    foreign::{Content, Failure, Request},
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    inventory::{Facts, OpaqueExtra},
    save::{Captured, Recovery, Repository, SaveWorker, format},
    snapshot::Snapshot,
    state::InstanceHandle,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

const OLD_BITS: u64 = 0x8000_0000_0000_0000;
const NEW_BITS: u64 = 0x7ff8_1234_5678_9abc;
const BUDGET: usize = 8192;
fn limits() -> Limits {
    Limits {
        max_snapshot_bytes: BUDGET,
        ..Default::default()
    }
}
fn fixture() -> (Option<tempfile::TempDir>, PathBuf) {
    fixture_with_evidence("FALLOUT_FOREIGN_LIFECYCLE_EVIDENCE")
}
fn fixture_with_evidence(variable: &str) -> (Option<tempfile::TempDir>, PathBuf) {
    let (temporary, root) = match std::env::var_os(variable) {
        Some(root) => {
            let root = PathBuf::from(root).join("authored");
            fs::create_dir(&root).unwrap();
            (None, root)
        }
        None => {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_path_buf();
            (Some(directory), root)
        }
    };
    fs::create_dir(root.join("Data")).unwrap();
    // Fixed compiled reads: set own float42 to SCRO1.float42, then SCRV2.float42.
    // No statement is executed; these are two real compiled foreign operand sites.
    let compiled = [
        0x10, 0, 6, 0, 0, 0, 34, 0, 0, 0, 0x15, 0, 11, 0, b'f', 42, 0, 6, 0, b'r', 1, 0, b'f', 42,
        0, 0x15, 0, 11, 0, b'f', 42, 0, 6, 0, b'r', 2, 0, b'f', 42, 0, 0x11, 0, 0, 0,
    ];
    let original = unit(&[(42, 0), (90, 0)], &[(b"SCRO", 0x101), (b"SCRV", 90)]);
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", &compiled));
    source.extend(&original[46..]);
    let bytes = [
        header(&[]),
        record(b"SCPT", 0x300, 0, &source),
        record(
            b"SCPT",
            0x301,
            0,
            &unit(&[(42, 0), (70, 0)], &[(b"SCRV", 70)]),
        ),
        record(b"SCPT", 0x302, 0, &unit(&[(42, 1)], &[])),
        record(b"REFR", 0x101, 0, &field(b"NAME", &0x102_u32.to_le_bytes())),
        record(b"ACTI", 0x102, 0, &field(b"SCRI", &0x301_u32.to_le_bytes())),
    ]
    .concat();
    fs::write(root.join("Data/FalloutNV.esm"), bytes).unwrap();
    fs::write(root.join("load-order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    (temporary, root)
}
fn load_content(root: &Path) -> (Arc<Catalogue>, Content) {
    let mut store = fallout_data::store::RecordStore::open_nv_headers(
        &root.join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn definition(catalogue: &Catalogue, id: u32) -> fallout_data::loaded_scripts::Handle {
    catalogue
        .record_scripts(&form(id))
        .next()
        .unwrap()
        .handle()
        .clone()
}
fn request(source: InstanceHandle, context: u16, player: ReferenceId) -> Request {
    Request {
        source,
        context_reference: context,
        local_index: 42,
        player: Some(player),
    }
}
fn reads(
    world: &World<'_>,
    content: &Content,
    source: InstanceHandle,
    player: ReferenceId,
) -> serde_json::Value {
    json!([
        world
            .read_foreign(content, request(source, 1, player))
            .unwrap(),
        world
            .read_foreign(content, request(source, 2, player))
            .unwrap()
    ])
}
fn cold(root: &Path, phase: &str, generation: u64) -> serde_json::Value {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_foreign_lifecycle_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_FOREIGN_LIFECYCLE_COLD_ROOT", root)
        .env("FALLOUT_FOREIGN_LIFECYCLE_PHASE", phase)
        .env(
            "FALLOUT_FOREIGN_LIFECYCLE_GENERATION",
            generation.to_string(),
        )
        .output()
        .unwrap();
    fs::write(
        root.join(format!("cold-{phase}.stdout.txt")),
        &output.stdout,
    )
    .unwrap();
    fs::write(
        root.join(format!("cold-{phase}.stderr.txt")),
        &output.stderr,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("FOREIGN_LIFECYCLE_COLD_RECEIPT "))
            .unwrap(),
    )
    .unwrap()
}
fn phase(
    root: &Path,
    name: &str,
    repository: &Repository,
    snapshot: &Snapshot,
) -> serde_json::Value {
    let bytes = fs::read(repository.path().join("current.frsv")).unwrap();
    assert_eq!(
        format::decode(&bytes, limits()).unwrap().snapshot,
        *snapshot
    );
    fs::write(root.join(format!("{name}.frsv")), &bytes).unwrap();
    fs::write(
        root.join(format!("{name}.snapshot.json")),
        snapshot.encode(BUDGET).unwrap(),
    )
    .unwrap();
    // Each phase has its own new marked repository, selected explicitly.
    let selected = Repository::create(&root.join(name), &[], snapshot.campaign).unwrap();
    fs::write(selected.path().join("current.frsv"), bytes).unwrap();
    let decoded = format::decode(
        &fs::read(root.join(format!("{name}.frsv"))).unwrap(),
        limits(),
    )
    .unwrap();
    cold(root, name, decoded.metadata.generation)
}

#[test]
fn foreign_owner_guards_unload_reattachment_and_cold_banks_preserve_exact_identity() {
    let (_temporary, root) = fixture();
    let (catalogue, content) = load_content(&root);
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        limits(),
        CampaignId::from_bytes([0x4c; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x101))).unwrap();
    let player = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Live { id: reference }],
    };
    let source = world
        .create_instance(
            &definition(&catalogue, 0x300),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            source,
            &[
                (42, Value::Number { bits: 1 }),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    },
                ),
            ],
        )
        .unwrap();
    let owner = Owner::Placed { reference };
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            owner.clone(),
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            target,
            &[
                (42, Value::Number { bits: OLD_BITS }),
                (
                    70,
                    Value::Reference {
                        value: ReferenceValue::Null,
                    },
                ),
            ],
        )
        .unwrap();
    let source_id = world.instance(source).unwrap().id();
    let target_id = world.instance(target).unwrap().id();
    world.initialize_inventory(reference).unwrap();
    let mut facts = Facts::unknown(form(0x102));
    facts.script_instance = Some(target_id);
    facts.extra_fields.push(OpaqueExtra {
        tag: *b"HOST",
        bytes: vec![0, 255, 1],
    });
    let item = world
        .add_item(reference, facts.clone(), 17.try_into().unwrap())
        .unwrap();
    let first_event = world
        .enqueue(
            target,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context.clone(),
        )
        .unwrap();
    let tail_event = world
        .enqueue(
            source,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context.clone(),
        )
        .unwrap();
    let live = world.snapshot();
    let live_reads = reads(&world, &content, source, player);
    for index in 1..=2 {
        let read = world
            .read_foreign(&content, request(source, index, player))
            .unwrap();
        assert_eq!(read.target.target_instance, target_id);
        assert_eq!(read.target.target_owner, owner);
        assert_eq!(read.value, Value::Number { bits: OLD_BITS });
    }
    let pending_error = world.remove_instance(target).unwrap_err().to_string();
    assert_eq!(
        pending_error,
        "runtime state is invalid: instance has pending events; acknowledge them explicitly before removal"
    );
    assert_eq!(world.snapshot(), live);
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start_with_budget(repository.clone(), 8, 2 * BUDGET).unwrap();
    let first = worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap()
        .wait()
        .unwrap();
    let live_cold = phase(&root, "live", &repository, &live);

    assert_eq!(world.acknowledge(first_event).unwrap().instance, target_id);
    let acknowledged = world.snapshot();
    let link_error = world.remove_instance(target).unwrap_err().to_string();
    assert_eq!(
        link_error,
        "runtime state is invalid: script instance is still linked by an inventory item"
    );
    assert_eq!(world.snapshot(), acknowledged);
    facts.script_instance = None;
    world.replace_item_facts(item, facts).unwrap();
    let detached = world.snapshot();
    world.remove_instance(target).unwrap();
    let unloaded = world.snapshot();
    let mut expected = detached;
    expected.state_revision += 1;
    expected
        .instances
        .retain(|instance| instance.id != target_id);
    assert_eq!(unloaded, expected);
    assert_eq!(unloaded.references, live.references);
    assert_eq!(unloaded.next_reference, live.next_reference);
    assert_eq!(world.authored_reference(&form(0x101)), Some(reference));
    assert_eq!(world.owner_instance(&owner), None);
    assert!(matches!(world.instance(target), Err(Error::StaleHandle)));
    assert!(matches!(
        world.handle(target_id),
        Err(Error::MissingInstance)
    ));
    assert_eq!(world.pending_events().len(), 1);
    assert_eq!(world.pending_events().next().unwrap().sequence, tail_event);
    assert_eq!(world.pending_events().next().unwrap().context, context);
    for index in 1..=2 {
        assert!(
            matches!(world.read_foreign(&content,request(source,index,player)),Err(Failure::MissingEventList(ref missing)) if *missing==owner)
        );
    }
    let second = worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap()
        .wait()
        .unwrap();
    let unloaded_cold = phase(&root, "unloaded", &repository, &unloaded);
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        fs::read(root.join("live.frsv")).unwrap()
    );

    let replacement_definition = definition(&catalogue, 0x302);
    let replacement = world
        .create_instance(&replacement_definition, owner.clone(), context)
        .unwrap();
    let replacement_id = world.instance(replacement).unwrap().id();
    assert_ne!(replacement_id, target_id);
    assert_eq!(replacement_id.0.get(), unloaded.next_instance);
    assert!(matches!(world.instance(target), Err(Error::StaleHandle)));
    world
        .assign(replacement, &[(42, Value::Number { bits: NEW_BITS })])
        .unwrap();
    let reattached = world.snapshot();
    assert_eq!(reattached.state_revision, unloaded.state_revision + 2);
    assert_eq!(reattached.next_instance, unloaded.next_instance + 1);
    assert_eq!(reattached.references, unloaded.references);
    assert_eq!(reattached.inventory_banks, unloaded.inventory_banks);
    assert_eq!(reattached.pending_events, unloaded.pending_events);
    let reattached_reads = reads(&world, &content, source, player);
    for index in 1..=2 {
        let read = world
            .read_foreign(&content, request(source, index, player))
            .unwrap();
        assert_eq!(read.target.target_instance, replacement_id);
        assert_eq!(read.target.target_definition, replacement_definition);
        assert_eq!(
            read.target.declaration.kind,
            fallout_runtime::schema::Kind::Integer
        );
        assert_eq!(read.value, Value::Number { bits: NEW_BITS });
    }
    let third = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    world
        .assign(source, &[(42, Value::Number { bits: 99 })])
        .unwrap();
    drop(world);
    drop(catalogue);
    drop(content);
    worker.finish().unwrap();
    let third = third.wait().unwrap();
    let reattached_cold = phase(&root, "reattached", &repository, &reattached);
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        fs::read(root.join("unloaded.frsv")).unwrap()
    );
    let (catalogue, content) = load_content(&root);
    let restored = repository
        .load(catalogue.as_ref(), limits(), Recovery::Strict)
        .unwrap()
        .0;
    assert!(matches!(restored.instance(source), Err(Error::StaleHandle)));
    assert!(matches!(
        restored.instance(replacement),
        Err(Error::StaleHandle)
    ));
    assert_eq!(
        reads(
            &restored,
            &content,
            restored.handle(source_id).unwrap(),
            player
        ),
        reattached_reads
    );
    fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&json!({"source_instance":source_id,"retired_instance":target_id,"replacement_instance":replacement_id,"reference":reference,"player_reference":player,"pending_removal_error":pending_error,"item_link_removal_error":link_error,"live_write":first,"unloaded_write":second,"reattached_write":third,"live_reads":live_reads,"reattached_reads":reattached_reads,"cold_live":live_cold,"cold_unloaded":unloaded_cold,"cold_reattached":reattached_cold,"old_handles_rejected":true,"reference_identity_unchanged":true,"source_tail_unchanged":true,"engineering_only":true})).unwrap()).unwrap();
}

fn legacy_foreign_envelope(snapshot: &Snapshot) -> Vec<u8> {
    legacy_foreign_envelope_at(snapshot, 2)
}
fn legacy_foreign_envelope_at(snapshot: &Snapshot, schema_version: u32) -> Vec<u8> {
    assert!(matches!(schema_version, 2 | 3));
    assert!(snapshot.reference_states.is_empty());
    let mut value = serde_json::to_value(snapshot).unwrap();
    let object = value.as_object_mut().unwrap();
    if schema_version == 2 {
        // Existing documented schema 2 has no item banks; never discard items.
        assert!(snapshot.inventory_banks.is_empty());
        assert_eq!(snapshot.next_item, 1);
        object.remove("inventory_banks");
        object.remove("next_item");
    }
    object.remove("reference_states");
    object.insert("schema_version".into(), schema_version.into());
    let body = serde_json::to_vec(&value).unwrap();
    assert!(body.len() < BUDGET);
    let mut metadata = Vec::new();
    metadata.extend(1_u32.to_le_bytes());
    metadata.extend(schema_version.to_le_bytes());
    metadata.extend(9_u64.to_le_bytes());
    metadata.extend(snapshot.clocks.tick.to_le_bytes());
    for index in (0..64).step_by(2) {
        metadata
            .push(u8::from_str_radix(&snapshot.catalogue_sha256[index..index + 2], 16).unwrap());
    }
    metadata.extend((body.len() as u64).to_le_bytes());
    metadata.extend(snapshot.campaign.bytes());
    metadata.extend(snapshot.state_revision.to_le_bytes());
    let mut bytes = b"FRSAVE01".to_vec();
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(0_u16.to_le_bytes());
    bytes.extend(2_u32.to_le_bytes());
    for (tag, payload) in [(b"META", metadata.as_slice()), (b"STAT", body.as_slice())] {
        bytes.extend(tag);
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend((payload.len() as u64).to_le_bytes());
        bytes.extend(Sha256::digest(payload));
        bytes.extend(payload);
    }
    bytes.extend(Sha256::digest(&bytes));
    bytes
}

fn migration_world(
    catalogue: Arc<Catalogue>,
) -> (
    World<'static>,
    InstanceHandle,
    InstanceHandle,
    ReferenceId,
    ReferenceId,
) {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        limits(),
        CampaignId::from_bytes([0x4f; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x101))).unwrap();
    let player = world.register_reference(None).unwrap();
    let static_value = ReferenceValue::Content { key: form(0x102) };
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(player),
        target: Some(static_value.clone()),
        arguments: vec![
            ReferenceValue::Live { id: reference },
            static_value.clone(),
            ReferenceValue::Null,
        ],
    };
    let source = world
        .create_instance(
            &definition(&catalogue, 0x300),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            context.clone(),
        )
        .unwrap();
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Placed { reference },
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            source,
            &[
                (42, Value::Number { bits: OLD_BITS }),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    },
                ),
            ],
        )
        .unwrap();
    world
        .assign(
            target,
            &[
                (42, Value::Number { bits: NEW_BITS }),
                (
                    70,
                    Value::Reference {
                        value: static_value,
                    },
                ),
            ],
        )
        .unwrap();
    world
        .advance_clocks(fallout_runtime::events::Clocks {
            tick: 7,
            game_nanoseconds: 11,
            menu_nanoseconds: 13,
            real_nanoseconds: 17,
        })
        .unwrap();
    for handle in [source, target] {
        world
            .enqueue(
                handle,
                Trigger::Block {
                    event_id: 0,
                    begin_byte_offset: 0,
                },
                context.clone(),
            )
            .unwrap();
    }
    (world, source, target, reference, player)
}

#[test]
fn legacy_foreign_migration_preserves_typed_identity_before_explicit_item_initialization() {
    let (_temporary, root) = fixture_with_evidence("FALLOUT_FOREIGN_MIGRATION_EVIDENCE");
    let (catalogue, content) = load_content(&root);
    let (world, source, target, reference, player) = migration_world(Arc::clone(&catalogue));
    let source_id = world.instance(source).unwrap().id();
    let target_id = world.instance(target).unwrap().id();
    let before = world.snapshot();
    let original_reads = reads(&world, &content, source, player);
    let legacy = legacy_foreign_envelope(&before);
    fs::write(root.join("legacy-schema2.frsv"), &legacy).unwrap();
    assert!(format::decode(&legacy, limits()).is_err());
    let migration = format::migrate_v2(&legacy, limits()).unwrap();
    assert_eq!(migration.source_metadata.generation, 9);
    assert_eq!(migration.snapshot, before);
    let mut restored =
        World::restore(Arc::clone(&catalogue), migration.snapshot, limits()).unwrap();
    assert_eq!(restored.snapshot(), before);
    for handle in [source, target] {
        assert!(matches!(restored.instance(handle), Err(Error::StaleHandle)));
    }
    assert_eq!(restored.authored_reference(&form(0x101)), Some(reference));
    assert_eq!(
        reads(
            &restored,
            &content,
            restored.handle(source_id).unwrap(),
            player
        ),
        original_reads
    );
    assert!(restored.inventory_count(reference, &form(0x102)).is_err());
    assert!(restored.snapshot().inventory_banks.is_empty());
    assert_eq!(restored.snapshot().next_item, 1);
    let repository =
        Repository::create(&root.join("native-migration"), &[], restored.campaign()).unwrap();
    let mut worker = SaveWorker::start_with_budget(repository.clone(), 2, 2 * BUDGET).unwrap();
    let prewrite = worker
        .try_submit(Captured::at_boundary(&restored))
        .unwrap()
        .wait()
        .unwrap();
    let legacy_cold = phase(&root, "legacy-current", &repository, &before);
    restored.initialize_inventory(reference).unwrap();
    let mut facts = Facts::unknown(form(0x102));
    facts.ownership = Some(fallout_runtime::inventory::Ownership::Live { reference: player });
    facts.condition = Some(fallout_runtime::inventory::Condition::Float64 { bits: NEW_BITS });
    facts.script_instance = Some(target_id);
    facts.extra_fields.push(OpaqueExtra {
        tag: *b"HOST",
        bytes: vec![0, 255, 1],
    });
    let item_id = restored
        .add_item(reference, facts, 17.try_into().unwrap())
        .unwrap();
    let item = restored.item_handle(item_id).unwrap();
    assert_eq!(item_id.0.get(), 1);
    assert_eq!(restored.snapshot().next_item, 2);
    let after = restored.snapshot();
    let mut expected = before.clone();
    expected.state_revision += 2;
    expected.next_item = 2;
    expected.inventory_banks = after.inventory_banks.clone();
    assert_eq!(after, expected);
    let postwrite = worker.try_submit(Captured::at_boundary(&restored)).unwrap();
    restored
        .assign(
            restored.handle(source_id).unwrap(),
            &[(42, Value::Number { bits: 99 })],
        )
        .unwrap();
    drop(restored);
    drop(world);
    worker.finish().unwrap();
    let postwrite = postwrite.wait().unwrap();
    assert_eq!(prewrite.metadata.generation, 1);
    assert_eq!(postwrite.metadata.generation, 2);
    let items_cold = phase(&root, "items-current", &repository, &after);
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    assert_eq!(
        previous,
        fs::read(root.join("legacy-current.frsv")).unwrap()
    );
    let previous_repository =
        Repository::create(&root.join("previous-selection"), &[], before.campaign).unwrap();
    fs::write(previous_repository.path().join("current.frsv"), &previous).unwrap();
    let previous_cold = phase(&root, "items-previous", &previous_repository, &before);
    let current = repository
        .load(catalogue.as_ref(), limits(), Recovery::Strict)
        .unwrap()
        .0;
    assert_eq!(current.snapshot(), after);
    assert!(current.item_by_handle(item).is_err());
    assert_eq!(
        current
            .item_id(current.item_handle(item_id).unwrap())
            .unwrap(),
        item_id
    );
    assert_eq!(current.item(item_id).unwrap().owner(), reference);
    assert_eq!(
        current.item(item_id).unwrap().facts().ownership,
        Some(fallout_runtime::inventory::Ownership::Live { reference: player })
    );
    assert_eq!(
        current.item(item_id).unwrap().facts().script_instance,
        Some(target_id)
    );
    assert_eq!(current.authored_reference(&form(0x101)), Some(reference));
    let mut item_reads = original_reads.clone();
    for read in item_reads.as_array_mut().unwrap() {
        read["target"]["state_revision"] = after.state_revision.into();
    }
    assert_eq!(
        reads(
            &current,
            &content,
            current.handle(source_id).unwrap(),
            player
        ),
        item_reads
    );
    for handle in [source, target] {
        assert!(matches!(current.instance(handle), Err(Error::StaleHandle)));
    }
    assert_eq!(fs::read(root.join("legacy-schema2.frsv")).unwrap(), legacy);
    fs::write(root.join("migration-identity-receipt.json"),serde_json::to_vec_pretty(&json!({
        "source_instance":source_id,"target_instance":target_id,"reference":reference,"player_reference":player,"item":item_id,
        "legacy_metadata":migration.source_metadata,"prewrite":prewrite,"postwrite":postwrite,
        "legacy_cold":legacy_cold,"items_cold":items_cold,"previous_cold":previous_cold,
        "reads":original_reads,"item_reads":item_reads,"full_migration_fields_equal":true,"old_instance_and_item_handles_rejected":true,
        "inventory_unknown_until_explicit_initialization":true,"typed_origins_and_links_equal":true,"engineering_only":true
    })).unwrap()).unwrap();
}

#[test]
fn schema_three_foreign_migration_retains_existing_item_links_and_refuses_other_identity() {
    let (_temporary, root) = fixture_with_evidence("FALLOUT_FOREIGN_MIGRATION_V3_EVIDENCE");
    let (catalogue, content) = load_content(&root);
    let (mut world, source, target, reference, player) = migration_world(Arc::clone(&catalogue));
    let source_id = world.instance(source).unwrap().id();
    let target_id = world.instance(target).unwrap().id();
    world.initialize_inventory(reference).unwrap();
    world.initialize_inventory(player).unwrap();
    let unknown = world.register_reference(None).unwrap();
    let mut facts = Facts::unknown(form(0x102));
    facts.ownership = Some(fallout_runtime::inventory::Ownership::Live { reference: player });
    facts.condition = Some(fallout_runtime::inventory::Condition::Float64 { bits: NEW_BITS });
    facts.script_instance = Some(target_id);
    facts.extra_fields.push(OpaqueExtra {
        tag: *b"HOST",
        bytes: vec![0, 255, 1],
    });
    let item_id = world
        .add_item(reference, facts.clone(), 17.try_into().unwrap())
        .unwrap();
    let old_item = world.item_handle(item_id).unwrap();
    let before = world.snapshot();
    let original_reads = reads(&world, &content, source, player);
    let legacy = legacy_foreign_envelope_at(&before, 3);
    let legacy_path = root.join("legacy-schema3.frsv");
    fs::write(&legacy_path, &legacy).unwrap();
    assert!(format::decode(&legacy, limits()).is_err());
    assert!(format::migrate_v2(&legacy, limits()).is_err());
    let migration = format::migrate_v3(&legacy, limits()).unwrap();
    assert_eq!(migration.source_state_schema, 3);
    assert_eq!(migration.source_metadata.generation, 9);
    assert_eq!(
        migration.source_metadata.container_sha256,
        format!("{:x}", Sha256::digest(&legacy))
    );
    assert_eq!(migration.snapshot, before);
    let mut restored =
        World::restore(Arc::clone(&catalogue), migration.snapshot, limits()).unwrap();
    let restored_source = restored.handle(source_id).unwrap();
    assert_eq!(
        reads(&restored, &content, restored_source, player),
        original_reads
    );
    for handle in [source, target] {
        assert!(matches!(restored.instance(handle), Err(Error::StaleHandle)));
    }
    assert!(matches!(
        restored.read_foreign(&content, request(source, 1, player)),
        Err(Failure::State(Error::StaleHandle))
    ));
    assert!(restored.item_by_handle(old_item).is_err());
    assert_eq!(restored.item(item_id).unwrap().facts(), &facts);
    assert_eq!(
        restored.inventory_count(reference, &form(0x102)).unwrap(),
        17
    );
    assert_eq!(restored.inventory_count(player, &form(0x102)).unwrap(), 0);
    assert!(restored.inventory_count(unknown, &form(0x102)).is_err());
    for id in [reference, player, unknown] {
        assert!(restored.reference_view(id).unwrap().state().is_none());
    }
    let repository = Repository::create(
        &root.join("native-schema3-migration"),
        &[],
        restored.campaign(),
    )
    .unwrap();
    let mut worker = SaveWorker::start_with_budget(repository.clone(), 2, 2 * BUDGET).unwrap();
    let prewrite = worker
        .try_submit(Captured::at_boundary(&restored))
        .unwrap()
        .wait()
        .unwrap();
    let migrated_cold = phase(&root, "schema3-current", &repository, &before);
    restored
        .assign(restored_source, &[(42, Value::Number { bits: 99 })])
        .unwrap();
    let after = restored.snapshot();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected
        .instances
        .iter_mut()
        .find(|instance| instance.id == source_id)
        .unwrap()
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = Value::Number { bits: 99 };
    assert_eq!(after, expected);
    let postwrite = worker.try_submit(Captured::at_boundary(&restored)).unwrap();
    restored
        .assign(restored_source, &[(42, Value::Number { bits: 100 })])
        .unwrap();
    drop(restored);
    drop(world);
    worker.finish().unwrap();
    let postwrite = postwrite.wait().unwrap();
    assert_eq!(prewrite.metadata.generation, 1);
    assert_eq!(postwrite.metadata.generation, 2);
    let changed_cold = phase(&root, "schema3-changed", &repository, &after);
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    assert_eq!(
        previous,
        fs::read(root.join("schema3-current.frsv")).unwrap()
    );
    let selected = Repository::create(
        &root.join("schema3-previous-selection"),
        &[],
        before.campaign,
    )
    .unwrap();
    fs::write(selected.path().join("current.frsv"), &previous).unwrap();
    let previous_cold = phase(&root, "schema3-previous", &selected, &before);
    let current_bytes = fs::read(repository.path().join("current.frsv")).unwrap();
    let current = repository
        .load(catalogue.as_ref(), limits(), Recovery::Strict)
        .unwrap()
        .0;
    assert_eq!(current.snapshot(), after);
    assert_eq!(current.item(item_id).unwrap().facts(), &facts);
    assert!(current.item_by_handle(old_item).is_err());
    let current_source = current.handle(source_id).unwrap();
    let mut current_reads = original_reads.clone();
    for read in current_reads.as_array_mut().unwrap() {
        read["target"]["state_revision"] = after.state_revision.into();
    }
    assert_eq!(
        reads(&current, &content, current_source, player),
        current_reads
    );

    // A copied canonical state can be deliberately given a new campaign, but
    // old foreign handles and an existing campaign's repository cannot accept it.
    let mut other_snapshot = after.clone();
    other_snapshot.campaign = CampaignId::from_bytes([0x50; 16]).unwrap();
    let other = World::restore(Arc::clone(&catalogue), other_snapshot.clone(), limits()).unwrap();
    assert!(matches!(
        other.read_foreign(&content, request(current_source, 1, player)),
        Err(Failure::State(Error::StaleHandle))
    ));
    assert_eq!(other.snapshot(), other_snapshot);
    let mut stages = Vec::new();
    assert!(
        matches!(repository.commit_observing(&Captured::at_boundary(&other), |stage| stages.push(stage)),
        Err(fallout_runtime::save::Error::Format(reason)) if reason == "captured campaign differs from repository identity")
    );
    assert!(stages.is_empty());

    let original_source = fs::read(root.join("Data/FalloutNV.esm")).unwrap();
    for changed_body in [true, false] {
        let changed = root.join(if changed_body {
            "changed-body"
        } else {
            "changed-plugins"
        });
        fs::create_dir(&changed).unwrap();
        fs::create_dir(changed.join("Data")).unwrap();
        let mut bytes = original_source.clone();
        let order = if changed_body {
            // Change only the final ACTI SCRI payload, preserving all winning
            // headers and compiled script records. Exact version handles also
            // bind the whole-source hash and must change with this payload.
            let end = bytes.len();
            assert_eq!(&bytes[end - 4..], &0x301_u32.to_le_bytes());
            bytes[end - 4..].copy_from_slice(&0x302_u32.to_le_bytes());
            vec!["FalloutNV.esm"]
        } else {
            fs::write(changed.join("Data/Other.esm"), header(&[])).unwrap();
            vec!["FalloutNV.esm", "Other.esm"]
        };
        fs::write(changed.join("Data/FalloutNV.esm"), bytes).unwrap();
        let mut store = fallout_data::store::RecordStore::open_nv_headers(
            &changed.join("Data"),
            &order.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
            Default::default(),
        )
        .unwrap();
        let changed_catalogue =
            Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
        let changed_content = Content::load(&mut store, &changed_catalogue, 100).unwrap();
        for instance in &after.instances {
            let original = catalogue.get_handle(&instance.definition).unwrap();
            let changed_script = changed_catalogue.get(&instance.definition.key).unwrap();
            assert_eq!(changed_script.compiled(), original.compiled());
            assert_eq!(
                changed_script.version().decoded_record_sha256,
                original.version().decoded_record_sha256
            );
            assert_eq!(
                changed_script.version().metadata_sha256,
                original.version().metadata_sha256
            );
            if changed_body {
                assert_ne!(
                    changed_script.version().source_sha256,
                    original.version().source_sha256
                );
                assert!(changed_catalogue.get_handle(&instance.definition).is_none());
            } else {
                assert!(changed_catalogue.get_handle(&instance.definition).is_some());
            }
        }
        if changed_body {
            assert_eq!(
                changed_content.report().winning_headers_sha256,
                content.report().winning_headers_sha256
            );
        }
        assert!(matches!(
            current.read_foreign(&changed_content, request(current_source, 1, player)),
            Err(Failure::ContentChanged)
        ));
        assert!(matches!(
            repository.load(&changed_catalogue, limits(), Recovery::Strict),
            Err(fallout_runtime::save::Error::State(
                Error::DefinitionChanged
            ))
        ));
        assert_eq!(current.snapshot(), after);
    }
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        current_bytes
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    assert_eq!(
        fs::read(root.join("Data/FalloutNV.esm")).unwrap(),
        original_source
    );
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy);
    fs::write(root.join("schema3-migration-identity-receipt.json"), serde_json::to_vec_pretty(&json!({
        "source_instance": source_id, "target_instance": target_id, "reference": reference,
        "player_reference": player, "unknown_reference": unknown, "item": item_id,
        "legacy_metadata": migration.source_metadata, "prewrite": prewrite, "postwrite": postwrite,
        "migrated_cold": migrated_cold, "changed_cold": changed_cold, "previous_cold": previous_cold,
        "reads": original_reads, "current_reads": current_reads, "full_migration_fields_equal": true,
        "unknown_and_empty_banks_distinct": true, "reference_components_unavailable": true,
        "old_handles_other_campaign_and_changed_content_refused": true, "input_files_unchanged": true,
        "engineering_only": true
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "parent launches fresh native phase restoration with authored inputs"]
fn cold_foreign_lifecycle_helper() {
    let root = PathBuf::from(std::env::var_os("FALLOUT_FOREIGN_LIFECYCLE_COLD_ROOT").unwrap());
    let phase = std::env::var("FALLOUT_FOREIGN_LIFECYCLE_PHASE").unwrap();
    let (catalogue, content) = load_content(&root);
    let repository = Repository::open(&root.join(&phase), &[]).unwrap();
    let before = fs::read(repository.path().join("current.frsv")).unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join(format!("{phase}.snapshot.json"))).unwrap(),
        limits(),
    )
    .unwrap();
    let (world, receipt) = repository
        .load(catalogue.as_ref(), limits(), Recovery::Strict)
        .unwrap();
    assert_eq!(world.snapshot(), expected);
    assert_eq!(
        receipt.metadata.generation,
        std::env::var("FALLOUT_FOREIGN_LIFECYCLE_GENERATION")
            .unwrap()
            .parse::<u64>()
            .unwrap()
    );
    assert_eq!(
        before,
        fs::read(repository.path().join("current.frsv")).unwrap()
    );
    let source = world
        .handle(
            expected
                .instances
                .iter()
                .find(|instance| matches!(instance.owner, Owner::Fragment { .. }))
                .unwrap()
                .id,
        )
        .unwrap();
    let reference = world.authored_reference(&form(0x101)).unwrap();
    assert_eq!(reference.0.get(), 1);
    let player = ReferenceId(2.try_into().unwrap());
    world.reference_origin(player).unwrap();
    if phase.starts_with("schema3-") {
        for reference in &expected.references {
            assert!(
                world
                    .reference_view(reference.id)
                    .unwrap()
                    .state()
                    .is_none()
            );
        }
        assert_eq!(world.inventory_count(reference, &form(0x102)).unwrap(), 17);
        assert_eq!(world.inventory_count(player, &form(0x102)).unwrap(), 0);
        assert!(
            world
                .inventory_count(ReferenceId(3.try_into().unwrap()), &form(0x102))
                .is_err()
        );
    }
    let mut outcomes = Vec::new();
    for index in 1..=2 {
        if phase == "unloaded" {
            let failure = world
                .read_foreign(&content, request(source, index, player))
                .unwrap_err();
            assert!(
                matches!(failure,Failure::MissingEventList(Owner::Placed {reference:found}) if found==reference)
            );
            outcomes.push(json!({"status":failure.code()}));
        } else {
            let read = world
                .read_foreign(&content, request(source, index, player))
                .unwrap();
            assert_eq!(
                read.value,
                Value::Number {
                    bits: if phase == "live" { OLD_BITS } else { NEW_BITS }
                }
            );
            outcomes.push(json!({"status":"resolved","read":read}));
        }
    }
    let bytes = world.snapshot().encode(BUDGET).unwrap();
    let hash = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(hash, receipt.metadata.snapshot_sha256);
    println!(
        "FOREIGN_LIFECYCLE_COLD_RECEIPT {}",
        json!({"receipt":receipt,"snapshot_sha256":hash,"reference":reference,"lookups":outcomes,"cold_files_unchanged":true})
    );
}
