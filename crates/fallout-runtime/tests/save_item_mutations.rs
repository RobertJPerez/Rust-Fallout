mod common;
use common::*;
use fallout_data::{identity::FormKey, loaded_scripts::Catalogue, store::RecordStore};
use fallout_runtime::{
    Error, Limits, World,
    events::Context,
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    inventory::{Ammo, Condition, Facts, OpaqueExtra, Ownership, ViewLimits},
    query::{Entry, GET_ITEM_COUNT_COMMAND, GET_ITEM_COUNT_CONDITION, Request},
    save::{self, Captured, CompletionError, Recovery, Repository, SaveWorker, format},
    snapshot::Snapshot,
    source_items::{Policy, Role},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    num::NonZeroU32,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

const BUDGET: usize = 16_384;
fn limits() -> Limits {
    Limits {
        max_snapshot_bytes: BUDGET,
        ..Default::default()
    }
}
fn quantity(value: u32) -> NonZeroU32 {
    value.try_into().unwrap()
}
fn load_content(root: &Path) -> (Arc<Catalogue>, Content) {
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], Default::default()).unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn fixture() -> (Option<tempfile::TempDir>, PathBuf, Arc<Catalogue>, Content) {
    let (temporary, root) = match std::env::var_os("FALLOUT_ITEM_MUTATION_EVIDENCE") {
        Some(root) => {
            let root = PathBuf::from(root).join("item-mutations");
            fs::create_dir(&root).unwrap();
            (None, root)
        }
        None => {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().to_path_buf();
            (Some(directory), root)
        }
    };
    write_fixture(&root, false);
    let mut source = fs::read(root.join("FalloutNV.esm")).unwrap();
    let mut entries = Vec::new();
    for id in [0x500_u32, 0x501, 0x502] {
        source.extend(record(b"MISC", id, 0, &[]));
        entries.extend(field(
            b"CNTO",
            &[id.to_le_bytes(), 1_i32.to_le_bytes()].concat(),
        ));
    }
    source.extend(record(b"CONT", 0x600, 0, &entries));
    fs::write(root.join("FalloutNV.esm"), source).unwrap();
    let (catalogue, content) = load_content(&root);
    (temporary, root, catalogue, content)
}
fn requests(world: &World<'_>, owners: &[ReferenceId], keys: &[FormKey]) -> Vec<Request> {
    let mut result = Vec::new();
    for &owner in owners {
        for key in keys {
            for entry in [
                Entry::Native {
                    command_id: GET_ITEM_COUNT_COMMAND,
                },
                Entry::Condition {
                    function_id: GET_ITEM_COUNT_CONDITION,
                },
            ] {
                result.push(
                    Request::prepare(
                        world,
                        entry,
                        Some(owner),
                        &[Value::Reference {
                            value: ReferenceValue::Content { key: key.clone() },
                        }],
                    )
                    .unwrap(),
                );
            }
        }
    }
    result
}
fn observations(
    world: &World<'_>,
    content: &Content,
    owners: &[ReferenceId],
    keys: &[FormKey],
    requests: &[Request],
) -> serde_json::Value {
    let snapshot = world.snapshot();
    let mut counts = Vec::new();
    for &owner in owners {
        for key in keys {
            // Compute the oracle from canonical rows, independently of both derived indices.
            let bank = snapshot
                .inventory_banks
                .iter()
                .find(|bank| bank.owner == owner);
            if let Some(bank) = bank {
                let contributions = bank
                    .items
                    .iter()
                    .filter(|item| item.facts().base == *key)
                    .map(|item| (item.id(), item.count()))
                    .collect::<Vec<_>>();
                let sum: u64 = contributions
                    .iter()
                    .map(|(_, count)| u64::from(*count))
                    .sum();
                let trace = world.inventory_count_trace_bounded(owner, key, 8).unwrap();
                assert_eq!(trace.result, sum);
                assert_eq!(world.inventory_count(owner, key).unwrap(), sum);
                assert_eq!(trace.contributions, contributions);
                counts.push(json!({"query":trace}));
            } else {
                assert_eq!(
                    world.inventory_count(owner, key).unwrap_err().to_string(),
                    "runtime state is invalid: inventory has no explicit initialization"
                );
                counts.push(json!({"error":"inventory has no explicit initialization"}));
            }
        }
    }
    let calls = requests
        .iter()
        .map(|request| match request.evaluate(world, content, 8) {
            Ok(trace) => {
                assert!(trace.original_numeric_return.is_none());
                assert!(!trace.original_behavior_verified);
                json!({"trace":trace})
            }
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    "runtime state is invalid: inventory has no explicit initialization"
                );
                json!({"error":error.to_string()})
            }
        })
        .collect::<Vec<_>>();
    for (index, pair) in calls.as_chunks::<2>().0.iter().enumerate() {
        if counts[index].get("query").is_some() {
            assert_eq!(pair[0]["trace"]["query"], counts[index]["query"]);
            assert_eq!(pair[1]["trace"]["query"], counts[index]["query"]);
        } else {
            assert_eq!(pair[0], pair[1]);
        }
    }
    let views = owners
        .iter()
        .map(|&owner| {
            let view = world
                .inventory_view(
                    owner,
                    ViewLimits {
                        max_items: 8,
                        max_links: 128,
                        max_extra_bytes: 4096,
                    },
                )
                .unwrap();
            let bank = snapshot
                .inventory_banks
                .iter()
                .find(|bank| bank.owner == owner);
            assert_eq!(view.items(), bank.map(|bank| bank.items.as_slice()));
            assert_eq!(view.revision(), snapshot.state_revision);
            assert_eq!(view.campaign(), snapshot.campaign);
            assert_eq!(view.catalogue_fingerprint(), snapshot.catalogue_sha256);
            view
        })
        .collect::<Vec<_>>();
    json!({"counts":counts,"calls":calls,"inventory_views":views})
}
fn archive(
    root: &Path,
    name: &str,
    bytes: &[u8],
    expected: &Snapshot,
    observed: &serde_json::Value,
) -> serde_json::Value {
    let decoded = format::decode(bytes, limits()).unwrap();
    assert_eq!(decoded.snapshot, *expected);
    let directory = root.join(name);
    fs::create_dir(&directory).unwrap();
    fs::write(
        directory.join("expected.json"),
        expected.encode(BUDGET).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.join("observations.json"),
        serde_json::to_vec_pretty(observed).unwrap(),
    )
    .unwrap();
    let repository = Repository::create(&directory.join("native"), &[], expected.campaign).unwrap();
    fs::write(repository.path().join("current.frsv"), bytes).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_item_mutations_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_ITEM_MUTATION_COLD_ROOT", root)
        .env("FALLOUT_ITEM_MUTATION_COLD_PHASE", name)
        .output()
        .unwrap();
    fs::write(directory.join("cold.stdout.txt"), &output.stdout).unwrap();
    fs::write(directory.join("cold.stderr.txt"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("ITEM_MUTATIONS_COLD "))
            .unwrap(),
    )
    .unwrap()
}
struct Matrix<'a> {
    root: &'a Path,
    repository: Repository,
    catalogue: Arc<Catalogue>,
    content: &'a Content,
    owners: Vec<ReferenceId>,
    keys: Vec<FormKey>,
    requests: Vec<Request>,
    previous: Option<(Snapshot, serde_json::Value)>,
    receipts: Vec<serde_json::Value>,
}
impl Matrix<'_> {
    fn save(&mut self, name: &str, world: &World<'_>) {
        let snapshot = world.snapshot();
        let observed = observations(
            world,
            self.content,
            &self.owners,
            &self.keys,
            &self.requests,
        );
        let mut worker = SaveWorker::start_with_budget(self.repository.clone(), 1, BUDGET).unwrap();
        let ticket = worker.try_submit(Captured::at_boundary(world)).unwrap();
        worker.finish().unwrap();
        let write = ticket.wait().unwrap();
        let (restored, receipt) = self
            .repository
            .load(self.catalogue.as_ref(), limits(), Recovery::Strict)
            .unwrap();
        assert_eq!(restored.snapshot(), snapshot);
        assert_eq!(
            observations(
                &restored,
                self.content,
                &self.owners,
                &self.keys,
                &self.requests
            ),
            observed
        );
        for bank in &snapshot.inventory_banks {
            for item in &bank.items {
                assert!(matches!(
                    restored.item_by_handle(world.item_handle(item.id()).unwrap()),
                    Err(Error::StaleHandle)
                ));
                assert_eq!(
                    restored
                        .item_by_handle(restored.item_handle(item.id()).unwrap())
                        .unwrap(),
                    item
                );
            }
        }
        let current = archive(
            self.root,
            &format!("{name}-current"),
            &fs::read(self.repository.path().join("current.frsv")).unwrap(),
            &snapshot,
            &observed,
        );
        let previous = self.previous.as_ref().map(|(snapshot, observed)| {
            archive(
                self.root,
                &format!("{name}-previous"),
                &fs::read(self.repository.path().join("previous.frsv")).unwrap(),
                snapshot,
                observed,
            )
        });
        if previous.is_none() {
            assert!(!self.repository.path().join("previous.frsv").exists());
        }
        self.receipts.push(json!({"phase":name,"write":write,"load":receipt,"current":current,"previous":previous}));
        self.previous = Some((snapshot, observed));
    }
}

#[test]
fn source_item_mutation_boundaries_keep_exact_counts_ids_and_cold_queries() {
    let (_temporary, root, catalogue, content) = fixture();
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        limits(),
        CampaignId::from_bytes([0x49; 16]).unwrap(),
    )
    .unwrap();
    let a = world.register_reference(None).unwrap();
    let b = world.register_reference(None).unwrap();
    let unknown = world.register_reference(None).unwrap();
    let handle = world
        .create_instance(
            &definition(&catalogue),
            Owner::Placed { reference: a },
            Context::default(),
        )
        .unwrap();
    let instance = world.instance(handle).unwrap().id();
    world
        .assign(
            handle,
            &[
                (
                    42,
                    Value::Number {
                        bits: 0x8000_0000_0000_0000,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: b },
                    },
                ),
            ],
        )
        .unwrap();
    let before = world.snapshot();
    let mut legacy = serde_json::to_value(&before).unwrap();
    legacy["schema_version"] = json!(2);
    legacy.as_object_mut().unwrap().remove("next_item");
    legacy.as_object_mut().unwrap().remove("inventory_banks");
    legacy.as_object_mut().unwrap().remove("reference_states");
    let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
    fs::write(root.join("legacy-v2.json"), &legacy_bytes).unwrap();
    let migrated = Snapshot::migrate_v2(&legacy_bytes, limits()).unwrap();
    assert_eq!(migrated, before);
    world = World::restore(Arc::clone(&catalogue), migrated, limits()).unwrap();
    assert!(matches!(world.instance(handle), Err(Error::StaleHandle)));
    let owners = vec![a, b, unknown];
    let keys = vec![form(0x500), form(0x501), form(0x502)];
    fs::write(
        root.join("queries.json"),
        serde_json::to_vec_pretty(&json!({"owners":[a,b],"item_keys":keys})).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("inputs.json"),
        serde_json::to_vec_pretty(&json!({"owners":owners,"item_keys":keys})).unwrap(),
    )
    .unwrap();
    let requests = requests(&world, &owners, &keys);
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut matrix = Matrix {
        root: &root,
        repository,
        catalogue: Arc::clone(&catalogue),
        content: &content,
        owners,
        keys: keys.clone(),
        requests,
        previous: None,
        receipts: Vec::new(),
    };
    matrix.save("unknown", &world);
    world.initialize_inventory(a).unwrap();
    world.initialize_inventory(b).unwrap();
    matrix.save("empty", &world);
    let policy = Policy::new(&[
        (Role::Base, &[*b"MISC"]),
        (Role::Ammo, &[*b"MISC"]),
        (Role::Modification, &[*b"MISC"]),
    ])
    .unwrap();
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
    facts.script_instance = Some(instance);
    facts.extra_fields = vec![OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 255, 1],
    }];
    let (original, proof) = world
        .add_source_item(&content, &policy, a, facts.clone(), quantity(17))
        .unwrap();
    let mut other_facts = facts.clone();
    other_facts.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let (separate, _) = world
        .add_source_item(&content, &policy, a, other_facts, quantity(3))
        .unwrap();
    assert_ne!(original, separate);
    assert_eq!(world.inventory_count(a, &keys[0]).unwrap(), 20);
    matrix.save("added", &world);
    let split = world.split_item(original, quantity(5)).unwrap();
    assert!(split > separate);
    assert_eq!(world.item(split).unwrap().facts(), &facts);
    assert_eq!(world.inventory_count(a, &keys[0]).unwrap(), 20);
    matrix.save("split", &world);
    world.transfer_item(split, b).unwrap();
    assert_eq!(world.item(split).unwrap().owner(), b);
    assert_eq!(world.item(split).unwrap().facts(), &facts);
    assert_eq!(world.inventory_count(a, &keys[0]).unwrap(), 15);
    assert_eq!(world.inventory_count(b, &keys[0]).unwrap(), 5);
    matrix.save("transfer", &world);
    world.remove_item_quantity(split, quantity(2)).unwrap();
    assert_eq!(world.item(split).unwrap().count(), 3);
    matrix.save("partial-remove", &world);
    let rejected = world.snapshot();
    assert_eq!(
        world.transfer_item(split, unknown).unwrap_err().to_string(),
        "runtime state is invalid: inventory has no explicit initialization"
    );
    assert_eq!(
        world
            .split_item(split, quantity(3))
            .unwrap_err()
            .to_string(),
        "runtime state is invalid: split needs a strict partial quantity"
    );
    assert_eq!(
        world
            .remove_item_quantity(split, quantity(4))
            .unwrap_err()
            .to_string(),
        "runtime state is invalid: removal exceeds item quantity"
    );
    assert!(matches!(
        world.inventory_count_trace_bounded(a, &keys[0], 1),
        Err(Error::Capacity("inventory query trace contributions"))
    ));
    world.transfer_item(split, b).unwrap();
    assert_eq!(world.snapshot(), rejected);
    let removed_handle = world.item_handle(split).unwrap();
    world.remove_item_quantity(split, quantity(3)).unwrap();
    assert_eq!(
        world
            .item_by_handle(removed_handle)
            .unwrap_err()
            .to_string(),
        "runtime state is invalid: item instance missing"
    );
    assert_eq!(world.inventory_count(b, &keys[0]).unwrap(), 0);
    matrix.save("full-remove", &world);
    let mut replacement = facts.clone();
    replacement.base = keys[2].clone();
    replacement.condition = Some(Condition::Float32 { bits: 0x8000_0000 });
    let replacement_proof = world
        .replace_source_item_facts(&content, &policy, original, replacement.clone())
        .unwrap();
    assert_eq!(world.item(original).unwrap().facts(), &replacement);
    assert_eq!(world.inventory_count(a, &keys[0]).unwrap(), 3);
    assert_eq!(world.inventory_count(a, &keys[2]).unwrap(), 12);
    matrix.save("replace-facts", &world);
    let (large_a, _) = world
        .add_source_item(
            &content,
            &policy,
            b,
            Facts::unknown(keys[1].clone()),
            quantity(u32::MAX),
        )
        .unwrap();
    let (large_b, _) = world
        .add_source_item(
            &content,
            &policy,
            b,
            Facts::unknown(keys[1].clone()),
            quantity(u32::MAX),
        )
        .unwrap();
    assert!(large_a > split && large_b > large_a);
    assert_eq!(
        world.inventory_count(b, &keys[1]).unwrap(),
        2 * u64::from(u32::MAX)
    );
    matrix.save("wide-count", &world);
    let max_ids = Snapshot {
        next_item: u64::MAX,
        ..world.snapshot()
    };
    let mut exhausted = World::restore(Arc::clone(&catalogue), max_ids.clone(), limits()).unwrap();
    assert!(matches!(
        exhausted.add_item(b, Facts::unknown(keys[1].clone()), quantity(1)),
        Err(Error::Capacity("item identities"))
    ));
    assert_eq!(exhausted.snapshot(), max_ids);
    let max_revision = Snapshot {
        state_revision: u64::MAX,
        ..world.snapshot()
    };
    let mut exhausted =
        World::restore(Arc::clone(&catalogue), max_revision.clone(), limits()).unwrap();
    assert!(matches!(
        exhausted.replace_item_facts(original, facts),
        Err(Error::Capacity("state revisions"))
    ));
    assert_eq!(exhausted.snapshot(), max_revision);
    let current = fs::read(matrix.repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(matrix.repository.path().join("previous.frsv")).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(matrix.repository.path().join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let mut worker = SaveWorker::start_with_budget(matrix.repository.clone(), 1, BUDGET).unwrap();
    assert!(matches!(
        worker
            .try_submit(Captured::at_boundary(&world))
            .unwrap()
            .wait(),
        Err(CompletionError::Save(save::Error::Busy))
    ));
    worker.finish().unwrap();
    assert_eq!(
        fs::read(matrix.repository.path().join("current.frsv")).unwrap(),
        current
    );
    assert_eq!(
        fs::read(matrix.repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    drop(lock);
    matrix.save("explicit-retry", &world);
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
    fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&json!({"boundaries":matrix.receipts,"source_sha256":format!("{:x}",Sha256::digest(&source)),"item_ids":{"original":original,"separate":separate,"split_retired":split,"large_a":large_a,"large_b":large_b},"addition_proof":proof,"replacement_proof":replacement_proof,"unknown_inventory_not_zero":true,"rejected_mutations_and_failed_save_unchanged":true,"allocator_and_revision_exhaustion_unchanged":true,"legacy_snapshot_all_fields_equal":true,"engineering_only":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "invoked in fresh child processes by the parent mutation matrix"]
fn cold_item_mutations_helper() {
    let root = PathBuf::from(std::env::var_os("FALLOUT_ITEM_MUTATION_COLD_ROOT").unwrap());
    let phase = root.join(std::env::var_os("FALLOUT_ITEM_MUTATION_COLD_PHASE").unwrap());
    let expected =
        Snapshot::decode(&fs::read(phase.join("expected.json")).unwrap(), limits()).unwrap();
    let expected_observations: serde_json::Value =
        serde_json::from_slice(&fs::read(phase.join("observations.json")).unwrap()).unwrap();
    let inputs: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("inputs.json")).unwrap()).unwrap();
    let owners: Vec<ReferenceId> = serde_json::from_value(inputs["owners"].clone()).unwrap();
    let keys: Vec<FormKey> = serde_json::from_value(inputs["item_keys"].clone()).unwrap();
    let before = fs::read(phase.join("native/current.frsv")).unwrap();
    let (catalogue, content) = load_content(&root);
    let repository = Repository::open(&phase.join("native"), &[]).unwrap();
    let (world, receipt) = repository
        .load(catalogue.as_ref(), limits(), Recovery::Strict)
        .unwrap();
    assert_eq!(world.snapshot(), expected);
    let observed = observations(
        &world,
        &content,
        &owners,
        &keys,
        &requests(&world, &owners, &keys),
    );
    assert_eq!(observed, expected_observations);
    let policy = Policy::new(&[
        (Role::Base, &[*b"MISC"]),
        (Role::Ammo, &[*b"MISC"]),
        (Role::Modification, &[*b"MISC"]),
    ])
    .unwrap();
    let mut proofs = Vec::new();
    for bank in &expected.inventory_banks {
        for item in &bank.items {
            let handle = world.item_handle(item.id()).unwrap();
            assert_eq!(world.item_id(handle).unwrap(), item.id());
            assert_eq!(world.item_by_handle(handle).unwrap(), item);
            proofs.push(
                fallout_runtime::source_items::validate(&world, &content, &policy, item.facts())
                    .unwrap(),
            );
        }
    }
    assert_eq!(world.snapshot(), expected);
    assert_eq!(fs::read(phase.join("native/current.frsv")).unwrap(), before);
    let result = json!({"receipt":receipt,"snapshot_sha256":format!("{:x}",Sha256::digest(expected.encode(BUDGET).unwrap())),"observations":observed,"source_proofs":proofs,"state_and_slot_unchanged":true});
    fs::write(
        phase.join("cold-receipt.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("ITEM_MUTATIONS_COLD {result}");
}
