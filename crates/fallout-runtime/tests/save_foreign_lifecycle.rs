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
    let (temporary, root) = match std::env::var_os("FALLOUT_FOREIGN_LIFECYCLE_EVIDENCE") {
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
