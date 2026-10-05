mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::source_items::{SourceInventoryGroupLimits, SourceInventoryRequest};
use fallout_runtime::{
    Limits, World,
    application::{self, ContinueBoundary, HostIdentity, ScenePublisher},
    events::{Clocks, Context, Trigger},
    foreign::Content,
    identity::{CampaignId, InstanceId, Owner, ReferenceId, ReferenceValue, Value},
    inventory::{Ammo, Condition, Facts, ItemHandle, ItemId, OpaqueExtra, Ownership, ViewLimits},
    save::{
        Captured, Recovery, Repository, RestorePoll, RestoreTask, RestoredCandidate, SaveWorker,
    },
    snapshot::Snapshot,
    source_items::{self, Failure, Policy, Role, SourceFactsLimits, SourceInventoryLimits},
    state::initialization,
};
use std::{
    num::{NonZeroU32, NonZeroU64},
    path::Path,
    sync::Arc,
};

#[test]
fn campaign_lifecycle_package_preserves_all_canonical_boundaries_through_native_and_fresh_cold_consumers()
 {
    use std::{fs, path::PathBuf, process::Command};
    let scratch = tempfile::tempdir().unwrap();
    let root = std::env::var_os("FALLOUT_RUNTIME_PACKAGE_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.path().join("package"));
    fs::create_dir(&root).unwrap();
    let (catalogue, content) = initialization_fixture_at(&root);
    // Authoritative source identities for the named actor/container owners are
    // supplied explicitly. This is still an engineering initialization policy.
    let mut bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    bytes.extend(record(
        b"ACHR",
        0x701,
        0,
        &field(b"NAME", &0x112u32.to_le_bytes()),
    ));
    bytes.extend(record(b"CONT", 0x600, 0, &[]));
    bytes.extend(record(
        b"REFR",
        0x703,
        0,
        &field(b"NAME", &0x600u32.to_le_bytes()),
    ));
    fs::write(root.join("FalloutNV.esm"), bytes).unwrap();
    drop(content);
    drop(catalogue);
    let mut store =
        RecordStore::open_nv_headers(&root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    let (mut world, unrelated, lots) = initialization_world(&catalogue, Limits::default());
    world.initialize_inventory(unrelated).unwrap();
    let (old_item, _) = world
        .add_source_item(&content, &policy(), unrelated, lots[0].0.clone(), lots[0].1)
        .unwrap();
    let actor = world.register_reference(Some(form(0x701))).unwrap();
    let container = world.register_reference(Some(form(0x703))).unwrap();
    let empty = world.register_reference(None).unwrap();
    for activation in [2, 3] {
        let stage = world
            .stage_instance_initialization(
                &definition(&catalogue),
                &Owner::Fragment {
                    activation: activation.try_into().unwrap(),
                },
                &Context::default(),
                &[
                    (2, Value::Number { bits: activation }),
                    (
                        42,
                        Value::Number {
                            bits: u64::MAX - activation,
                        },
                    ),
                    (
                        90,
                        Value::Reference {
                            value: ReferenceValue::Null,
                        },
                    ),
                ],
                initialization::Limits::default(),
            )
            .unwrap();
        world.commit_instance_initialization(stage).unwrap();
    }
    let before = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let mut snapshots = vec![("before", before.clone())];
    let mut wires = Vec::new();
    let saved = worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(saved.metadata.generation, 1);
    wires.push(fs::read(repository.path().join("current.frsv")).unwrap());
    let mut bad = lots.clone();
    bad[1].0.base = form(0x777);
    let bad_requests = [
        SourceInventoryRequest {
            owner: actor,
            lots: &lots,
        },
        SourceInventoryRequest {
            owner: container,
            lots: &bad,
        },
        SourceInventoryRequest {
            owner: empty,
            lots: &[],
        },
    ];
    assert!(
        world
            .stage_source_inventory_group(
                &content,
                &policy(),
                &bad_requests,
                SourceInventoryGroupLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    let requests = [
        SourceInventoryRequest {
            owner: actor,
            lots: &lots,
        },
        SourceInventoryRequest {
            owner: container,
            lots: &lots[1..],
        },
        SourceInventoryRequest {
            owner: empty,
            lots: &[],
        },
    ];
    let stage = world
        .stage_source_inventory_group(
            &content,
            &policy(),
            &requests,
            SourceInventoryGroupLimits::default(),
        )
        .unwrap();
    let boot = world
        .commit_source_inventory_group(&content, &policy(), stage)
        .unwrap();
    assert_eq!(boot.after_revision(), before.state_revision + 1);
    let boot_snapshot = world.snapshot();
    assert_eq!(boot_snapshot.instances, before.instances);
    assert_eq!(boot_snapshot.pending_events, before.pending_events);
    assert_eq!(boot_snapshot.clocks, before.clocks);
    assert_eq!(world.item(old_item).unwrap().facts(), &lots[0].0);
    snapshots.push(("boot", boot_snapshot.clone()));
    assert_eq!(
        worker
            .try_submit(Captured::at_boundary(&world))
            .unwrap()
            .wait()
            .unwrap()
            .metadata
            .generation,
        2
    );
    wires.push(fs::read(repository.path().join("current.frsv")).unwrap());
    assert!(
        world
            .stage_source_inventory_additions(
                &content,
                &policy(),
                actor,
                &bad,
                SourceInventoryLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), boot_snapshot);
    let stage = world
        .stage_source_inventory_additions(
            &content,
            &policy(),
            actor,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let additions = world
        .commit_source_inventory_additions(&content, &policy(), stage)
        .unwrap();
    assert_eq!(additions.after_revision(), boot.after_revision() + 1);
    assert_eq!(world.inventory_count(actor, &form(0x100)).unwrap(), 26);
    let addition_snapshot = world.snapshot();
    snapshots.push(("addition", addition_snapshot.clone()));
    assert_eq!(
        worker
            .try_submit(Captured::at_boundary(&world))
            .unwrap()
            .wait()
            .unwrap()
            .metadata
            .generation,
        3
    );
    wires.push(fs::read(repository.path().join("current.frsv")).unwrap());
    let selected = [
        world.handle(InstanceId(3.try_into().unwrap())).unwrap(),
        world.handle(InstanceId(2.try_into().unwrap())).unwrap(),
    ];
    let survivor = world.handle(InstanceId(1.try_into().unwrap())).unwrap();
    assert!(
        world
            .stage_instance_retirement_group(
                &[selected[0], survivor],
                fallout_runtime::state::retirement::Limits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), addition_snapshot);
    let stage = world
        .stage_instance_retirement_group(
            &selected,
            fallout_runtime::state::retirement::Limits::default(),
        )
        .unwrap();
    let retirement = world.commit_instance_retirement_group(stage).unwrap();
    let after = world.snapshot();
    let mut expected = addition_snapshot;
    expected.instances.truncate(1);
    expected.state_revision += 1;
    assert_eq!(after, expected);
    snapshots.push(("retirement", after));
    assert_eq!(
        worker
            .try_submit(Captured::at_boundary(&world))
            .unwrap()
            .wait()
            .unwrap()
            .metadata
            .generation,
        4
    );
    wires.push(fs::read(repository.path().join("current.frsv")).unwrap());
    worker.finish().unwrap();
    fs::write(
        root.join("receipts.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"boot":boot,"additions":additions,"retirement":retirement}),
        )
        .unwrap(),
    )
    .unwrap();
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(world);
    drop(content);
    drop(catalogue);
    for ((phase, snapshot), wire) in snapshots.iter().zip(&wires) {
        let phase_root = root.join(phase);
        fs::create_dir(&phase_root).unwrap();
        fs::write(
            root.join(format!("expected.{phase}.json")),
            snapshot.encode(1 << 20).unwrap(),
        )
        .unwrap();
        let copy = Repository::create(&phase_root.join("native"), &[], snapshot.campaign).unwrap();
        fs::write(copy.path().join("current.frsv"), wire).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_campaign_lifecycle_package_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_RUNTIME_PACKAGE_COLD_ROOT", &root)
            .env("FALLOUT_RUNTIME_PACKAGE_COLD_PHASE", phase)
            .output()
            .unwrap();
        fs::write(phase_root.join("cold.stdout.txt"), &child.stdout).unwrap();
        fs::write(phase_root.join("cold.stderr.txt"), &child.stderr).unwrap();
        assert!(
            child.status.success(),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(fs::read(copy.path().join("current.frsv")).unwrap(), *wire);
    }
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        wires[3]
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        wires[2]
    );
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}
#[test]
#[ignore = "fresh connected campaign consumer invoked by parent"]
fn cold_campaign_lifecycle_package_helper() {
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::var_os("FALLOUT_RUNTIME_PACKAGE_COLD_ROOT").unwrap());
    let phase = std::env::var("FALLOUT_RUNTIME_PACKAGE_COLD_PHASE").unwrap();
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join(&phase).join("native"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join(format!("expected.{phase}.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    let views = expected
        .references
        .iter()
        .map(|reference| {
            world
                .inventory_view(
                    reference.id,
                    ViewLimits {
                        max_items: 256,
                        max_links: 32_768,
                        max_extra_bytes: 2 * 1024 * 1024,
                    },
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    fs::write(
        root.join(&phase).join("cold.restored.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.views.json"),
        serde_json::to_vec_pretty(&views).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn source_inventory_group_boots_actor_container_and_empty_owner_in_request_order_once() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, container, lots) = initialization_world(&catalogue, Limits::default());
    let actor = world.register_reference(Some(form(0x112))).unwrap();
    let empty = world.register_reference(None).unwrap();
    let requests = [
        SourceInventoryRequest {
            owner: actor,
            lots: &lots,
        },
        SourceInventoryRequest {
            owner: container,
            lots: &lots[1..],
        },
        SourceInventoryRequest {
            owner: empty,
            lots: &[],
        },
    ];
    let before = world.snapshot();
    let stage = world
        .stage_source_inventory_group(
            &content,
            &policy(),
            &requests,
            SourceInventoryGroupLimits::default(),
        )
        .unwrap();
    assert_eq!(
        stage.owners().collect::<Vec<_>>(),
        [actor, container, empty]
    );
    assert_eq!(stage.usage().owners, 3);
    assert_eq!(stage.usage().inventory.lots, 3);
    assert_eq!(world.snapshot(), before);
    let receipt = world
        .commit_source_inventory_group(&content, &policy(), stage)
        .unwrap();
    assert_eq!(receipt.before_revision(), before.state_revision);
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    assert_eq!(
        receipt
            .owners()
            .iter()
            .map(|row| row.owner())
            .collect::<Vec<_>>(),
        [actor, container, empty]
    );
    assert_eq!(
        receipt
            .owners()
            .iter()
            .map(|row| row
                .item_ids()
                .iter()
                .map(|id| id.0.get())
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec![1, 2], vec![3], vec![]]
    );
    assert_eq!(world.inventory_count(actor, &form(0x100)).unwrap(), 13);
    assert_eq!(world.inventory_count(container, &form(0x100)).unwrap(), 2);
    assert_eq!(world.inventory_count(empty, &form(0x100)).unwrap(), 0);
    assert_eq!(world.inventory_items(empty).unwrap().count(), 0);
    for (row, request) in receipt.owners().iter().zip(&requests) {
        for (&id, (facts, count)) in row.item_ids().iter().zip(request.lots) {
            assert_eq!(world.item(id).unwrap().facts(), facts);
            assert_eq!(world.item(id).unwrap().count(), count.get());
        }
    }
    let after = world.snapshot();
    assert_eq!(after.next_item, 4);
    assert_eq!(after.references, before.references);
    assert_eq!(after.instances, before.instances);
    assert_eq!(after.pending_events, before.pending_events);
    assert_eq!(after.clocks, before.clocks);
    assert!(
        world
            .stage_source_inventory_group(
                &content,
                &policy(),
                &[SourceInventoryRequest {
                    owner: empty,
                    lots: &[]
                }],
                SourceInventoryGroupLimits::default()
            )
            .is_err()
    );
    let stage = world
        .stage_source_inventory_group(
            &content,
            &policy(),
            &[],
            SourceInventoryGroupLimits::default(),
        )
        .unwrap();
    let receipt = world
        .commit_source_inventory_group(&content, &policy(), stage)
        .unwrap();
    assert!(receipt.owners().is_empty());
    assert_eq!(receipt.before_revision(), receipt.after_revision());
    assert_eq!(world.snapshot(), after);
}
#[test]
fn source_inventory_group_invalid_final_owner_or_lot_and_duplicate_owner_leave_all_banks_unknown() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, first, lots) = initialization_world(&catalogue, Limits::default());
    let last = world.register_reference(None).unwrap();
    let before = world.snapshot();
    let missing = ReferenceId(999.try_into().unwrap());
    let mut bad = lots.clone();
    bad[1].0.modifications = Some(vec![form(0x113), form(0x777)]);
    let mut legacy = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    let stage = legacy
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            first,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    legacy
        .commit_source_inventory_initialization(&content, &policy(), stage)
        .unwrap();
    assert!(
        legacy
            .stage_source_inventory_initialization(
                &content,
                &policy(),
                last,
                &bad,
                SourceInventoryLimits::default()
            )
            .is_err()
    );
    assert_ne!(legacy.snapshot(), before);
    assert!(legacy.inventory_items(first).is_ok());
    assert!(legacy.inventory_items(last).is_err());
    for requests in [
        vec![
            SourceInventoryRequest {
                owner: first,
                lots: &lots,
            },
            SourceInventoryRequest {
                owner: missing,
                lots: &[],
            },
        ],
        vec![
            SourceInventoryRequest {
                owner: first,
                lots: &lots,
            },
            SourceInventoryRequest {
                owner: first,
                lots: &[],
            },
        ],
        vec![
            SourceInventoryRequest {
                owner: first,
                lots: &lots,
            },
            SourceInventoryRequest {
                owner: last,
                lots: &bad,
            },
        ],
    ] {
        assert!(
            world
                .stage_source_inventory_group(
                    &content,
                    &policy(),
                    &requests,
                    SourceInventoryGroupLimits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
        assert!(world.inventory_items(first).is_err());
        assert!(world.inventory_items(last).is_err());
    }
    world.initialize_inventory(last).unwrap();
    let before = world.snapshot();
    assert!(
        world
            .stage_source_inventory_group(
                &content,
                &policy(),
                &[
                    SourceInventoryRequest {
                        owner: first,
                        lots: &lots
                    },
                    SourceInventoryRequest {
                        owner: last,
                        lots: &[]
                    }
                ],
                SourceInventoryGroupLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
}
#[test]
fn source_inventory_group_cumulative_exact_bounds_and_stale_commit_are_atomic() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, first, lots) = initialization_world(&catalogue, Limits::default());
    let second = world.register_reference(None).unwrap();
    let requests = [
        SourceInventoryRequest {
            owner: first,
            lots: &lots,
        },
        SourceInventoryRequest {
            owner: second,
            lots: &lots,
        },
    ];
    let before = world.snapshot();
    let usage = world
        .stage_source_inventory_group(
            &content,
            &policy(),
            &requests,
            SourceInventoryGroupLimits::default(),
        )
        .unwrap()
        .usage();
    let exact = SourceInventoryGroupLimits {
        max_owners: 2,
        max_lots: 4,
        max_source_checks: usage.inventory.source_checks,
        max_copied_bytes: usage.inventory.copied_bytes,
    };
    assert!(
        world
            .stage_source_inventory_group(&content, &policy(), &requests, exact)
            .is_ok()
    );
    for limits in [
        SourceInventoryGroupLimits {
            max_owners: 1,
            ..exact
        },
        SourceInventoryGroupLimits {
            max_lots: 3,
            ..exact
        },
        SourceInventoryGroupLimits {
            max_source_checks: usage.inventory.source_checks - 1,
            ..exact
        },
        SourceInventoryGroupLimits {
            max_copied_bytes: usage.inventory.copied_bytes - 1,
            ..exact
        },
    ] {
        assert!(
            world
                .stage_source_inventory_group(&content, &policy(), &requests, limits)
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let stage = world
        .stage_source_inventory_group(&content, &policy(), &requests, exact)
        .unwrap();
    world.initialize_inventory(second).unwrap();
    let after = world.snapshot();
    assert!(
        world
            .commit_source_inventory_group(&content, &policy(), stage)
            .is_err()
    );
    assert_eq!(world.snapshot(), after);
    assert!(world.inventory_items(first).is_err());
    for limits in [
        Limits {
            max_inventory_banks: 1,
            ..Limits::default()
        },
        Limits {
            max_item_instances: 3,
            ..Limits::default()
        },
        Limits {
            max_total_item_links: 31,
            ..Limits::default()
        },
        Limits {
            max_total_item_bytes: 11,
            ..Limits::default()
        },
    ] {
        let (mut world, first, lots) = initialization_world(&catalogue, limits);
        let second = world.register_reference(None).unwrap();
        let before = world.snapshot();
        let requests = [
            SourceInventoryRequest {
                owner: first,
                lots: &lots,
            },
            SourceInventoryRequest {
                owner: second,
                lots: &lots,
            },
        ];
        assert!(
            world
                .stage_source_inventory_group(&content, &policy(), &requests, exact)
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let limits = Limits {
        max_inventory_banks: 2,
        max_item_instances: 4,
        max_total_item_links: 32,
        max_total_item_bytes: 12,
        ..Default::default()
    };
    let (mut world, first, lots) = initialization_world(&catalogue, limits);
    let second = world.register_reference(None).unwrap();
    let requests = [
        SourceInventoryRequest {
            owner: first,
            lots: &lots,
        },
        SourceInventoryRequest {
            owner: second,
            lots: &lots,
        },
    ];
    let stage = world
        .stage_source_inventory_group(&content, &policy(), &requests, exact)
        .unwrap();
    world
        .commit_source_inventory_group(&content, &policy(), stage)
        .unwrap();
    assert_eq!(world.snapshot().next_item, 5);
}

#[test]
fn source_additions_preserve_original_lots_and_publish_distinct_same_base_ids_once() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    world.initialize_inventory(owner).unwrap();
    let (original, _) = world
        .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
        .unwrap();
    let before = world.snapshot();
    let stage = world
        .stage_source_inventory_additions(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    assert_eq!(stage.owner(), owner);
    assert_eq!(stage.lots(), lots.as_slice());
    assert_eq!(stage.usage().lots, 2);
    assert_eq!(stage.usage().source_checks, 10);
    assert_eq!(world.snapshot(), before);
    let receipt = world
        .commit_source_inventory_additions(&content, &policy(), stage)
        .unwrap();
    assert_eq!(receipt.before_revision(), before.state_revision);
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    assert_eq!(
        receipt
            .item_ids()
            .iter()
            .map(|id| id.0.get())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(world.item(original).unwrap().facts(), &lots[0].0);
    assert_eq!(world.item(original).unwrap().count(), 11);
    for (id, (facts, count)) in receipt.item_ids().iter().zip(&lots) {
        assert_eq!(world.item(*id).unwrap().facts(), facts);
        assert_eq!(world.item(*id).unwrap().count(), count.get());
    }
    assert_eq!(world.inventory_count(owner, &form(0x100)).unwrap(), 24);
    let after = world.snapshot();
    assert_eq!(after.next_item, 4);
    assert_eq!(after.references, before.references);
    assert_eq!(after.instances, before.instances);
    assert_eq!(after.pending_events, before.pending_events);
    assert_eq!(after.clocks, before.clocks);
    let empty = world
        .stage_source_inventory_additions(
            &content,
            &policy(),
            owner,
            &[],
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let receipt = world
        .commit_source_inventory_additions(&content, &policy(), empty)
        .unwrap();
    assert!(receipt.item_ids().is_empty());
    assert_eq!(receipt.before_revision(), receipt.after_revision());
    assert_eq!(world.snapshot(), after);
}
#[test]
fn source_additions_bad_last_source_role_live_link_and_equipment_leave_existing_bank_exact() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let unknown = world.snapshot();
    assert!(
        world
            .stage_source_inventory_additions(
                &content,
                &policy(),
                owner,
                &lots,
                SourceInventoryLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), unknown);
    world.initialize_inventory(owner).unwrap();
    world
        .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
        .unwrap();
    let before = world.snapshot();
    let mut variants = Vec::new();
    let mut legacy = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    legacy
        .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
        .unwrap();
    assert!(
        legacy
            .add_source_item(
                &content,
                &policy(),
                owner,
                Facts::unknown(form(0x777)),
                lots[1].1
            )
            .is_err()
    );
    assert_ne!(legacy.snapshot(), before);
    for key in [form(0x777), form(0x110), form(0x200)] {
        let mut row = lots[1].0.clone();
        row.base = key;
        variants.push(row);
    }
    let mut row = lots[1].0.clone();
    row.modifications = Some(vec![form(0x113), form(0x777)]);
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.ownership = Some(Ownership::Live {
        reference: ReferenceId(999.try_into().unwrap()),
    });
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.equipped_slots = Some(vec![7, 7]);
    variants.push(row);
    for facts in variants {
        let bad = [lots[0].clone(), (facts, lots[1].1)];
        assert!(
            world
                .stage_source_inventory_additions(
                    &content,
                    &policy(),
                    owner,
                    &bad,
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let stage = world
        .stage_source_inventory_additions(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 3,
            ..world.clocks()
        })
        .unwrap();
    let changed = world.snapshot();
    assert!(
        world
            .commit_source_inventory_additions(&content, &policy(), stage)
            .is_err()
    );
    assert_eq!(world.snapshot(), changed);
}
#[test]
fn source_additions_aggregate_limits_and_policy_bindings_refuse_before_publication() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    world.initialize_inventory(owner).unwrap();
    world
        .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
        .unwrap();
    let before = world.snapshot();
    let usage = world
        .stage_source_inventory_additions(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap()
        .usage();
    let exact = SourceInventoryLimits {
        max_lots: usage.lots,
        max_source_checks: usage.source_checks,
        max_copied_bytes: usage.copied_bytes,
    };
    assert!(
        world
            .stage_source_inventory_additions(&content, &policy(), owner, &lots, exact)
            .is_ok()
    );
    for limits in [
        SourceInventoryLimits {
            max_lots: 1,
            ..exact
        },
        SourceInventoryLimits {
            max_source_checks: usage.source_checks - 1,
            ..exact
        },
        SourceInventoryLimits {
            max_copied_bytes: usage.copied_bytes - 1,
            ..exact
        },
    ] {
        assert!(
            world
                .stage_source_inventory_additions(&content, &policy(), owner, &lots, limits)
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let stage = world
        .stage_source_inventory_additions(&content, &policy(), owner, &lots, exact)
        .unwrap();
    let other = Policy::new(&[(Role::Base, &[*b"ACTI"])]).unwrap();
    assert!(
        world
            .commit_source_inventory_additions(&content, &other, stage)
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    for limits in [
        Limits {
            max_item_instances: 2,
            ..Limits::default()
        },
        Limits {
            max_total_item_links: 23,
            ..Limits::default()
        },
        Limits {
            max_total_item_bytes: 8,
            ..Limits::default()
        },
    ] {
        let (mut world, owner, lots) = initialization_world(&catalogue, limits);
        world.initialize_inventory(owner).unwrap();
        world
            .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
            .unwrap();
        let before = world.snapshot();
        assert!(
            world
                .stage_source_inventory_additions(&content, &policy(), owner, &lots, exact)
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
}
fn load_fixture() -> (tempfile::TempDir, Catalogue, Content) {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let mut bytes = std::fs::read(dir.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"AMMO", 0x110, 0, &[]));
    bytes.extend(record(b"FACT", 0x111, 0, &[]));
    bytes.extend(record(b"NPC_", 0x112, 0, &[]));
    bytes.extend(record(b"IMOD", 0x113, 0, &[]));
    std::fs::write(dir.path().join("FalloutNV.esm"), bytes).unwrap();
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let c = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &c, 100).unwrap();
    (dir, c, content)
}
fn policy() -> Policy {
    Policy::new(&[
        (Role::Base, &[*b"ACTI"]),
        (Role::ActorOwner, &[*b"NPC_"]),
        (Role::FactionOwner, &[*b"FACT"]),
        (Role::Ammo, &[*b"AMMO"]),
        (Role::Modification, &[*b"IMOD"]),
    ])
    .unwrap()
}
#[test]
fn source_checks_every_supplied_role_and_keeps_duplicate_links_in_order() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let mut facts = Facts::unknown(form(0x100));
    facts.ownership = Some(Ownership::Faction {
        key: form(0x111),
        rank: -3,
    });
    facts.ammo = Some(Ammo {
        base: form(0x110),
        count: 0,
    });
    facts.modifications = Some(vec![form(0x113), form(0x113)]);
    let before = w.revision();
    let (id, proof) = w
        .add_source_item(
            &content,
            &policy(),
            owner,
            facts.clone(),
            7.try_into().unwrap(),
        )
        .unwrap();
    assert_eq!(proof.state_revision, before);
    assert_eq!(w.revision(), before + 1);
    assert_eq!(w.item(id).unwrap().facts(), &facts);
    assert_eq!(
        proof.forms.iter().map(|f| f.role).collect::<Vec<_>>(),
        [
            Role::Base,
            Role::FactionOwner,
            Role::Ammo,
            Role::Modification,
            Role::Modification
        ]
    );
    assert_eq!(proof.forms[1].source.kind, *b"FACT");
    assert_eq!(proof.forms[3].key, proof.forms[4].key);
    facts.ownership = Some(Ownership::Actor { key: form(0x112) });
    w.replace_source_item_facts(&content, &policy(), id, facts)
        .unwrap();
    assert_eq!(w.inventory_count(owner, &form(0x100)).unwrap(), 7);
}
#[test]
fn missing_deleted_and_wrong_kind_reject_without_canonical_mutation() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let before = w.snapshot();
    for key in [form(0x777), form(0x200), form(0x110)] {
        assert!(
            w.add_source_item(
                &content,
                &policy(),
                owner,
                Facts::unknown(key),
                1.try_into().unwrap()
            )
            .is_err()
        );
        assert_eq!(w.snapshot(), before);
    }
    let (id, _) = w
        .add_source_item(
            &content,
            &policy(),
            owner,
            Facts::unknown(form(0x100)),
            1.try_into().unwrap(),
        )
        .unwrap();
    let before = w.snapshot();
    let mut bad = Facts::unknown(form(0x100));
    bad.modifications = Some(vec![form(0x113), form(0x777)]);
    assert!(
        w.replace_source_item_facts(&content, &policy(), id, bad)
            .is_err()
    );
    assert_eq!(w.snapshot(), before);
}
#[test]
fn absent_policy_is_not_a_permissive_rule_and_unknown_facts_stay_unknown() {
    let (_dir, c, content) = load_fixture();
    let w = World::new(&c, Limits::default()).unwrap();
    let p = Policy::new(&[(Role::Base, &[*b"ACTI"])]).unwrap();
    let f = Facts::unknown(form(0x100));
    let proof = source_items::validate(&w, &content, &p, &f).unwrap();
    assert_eq!(proof.forms.len(), 1);
    let mut f = f;
    f.ammo = Some(Ammo {
        base: form(0x110),
        count: 0,
    });
    assert!(matches!(
        source_items::validate(&w, &content, &p, &f),
        Err(Failure::MissingRule(Role::Ammo))
    ));
}
#[test]
fn source_index_is_bound_to_world_and_restored_state() {
    let (dir, c, content) = load_fixture();
    let w = World::new(&c, Limits::default()).unwrap();
    let proof =
        source_items::validate(&w, &content, &policy(), &Facts::unknown(form(0x100))).unwrap();
    let restored = World::restore(&c, w.snapshot(), Limits::default()).unwrap();
    let again =
        source_items::validate(&restored, &content, &policy(), &Facts::unknown(form(0x100)))
            .unwrap();
    assert_eq!(
        serde_json::to_value(proof).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    let mut bytes = std::fs::read(dir.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"MISC", 0x114, 0, &[]));
    std::fs::write(dir.path().join("FalloutNV.esm"), bytes).unwrap();
    let changed = load(dir.path(), &["FalloutNV.esm"]);
    let other = World::new(&changed, Limits::default()).unwrap();
    assert!(matches!(
        source_items::validate(&other, &content, &policy(), &Facts::unknown(form(0x100))),
        Err(Failure::Source(
            fallout_runtime::foreign::Failure::ContentChanged
        ))
    ));
}
#[test]
fn explicit_policy_is_bounded_unique_and_order_independent() {
    assert!(Policy::new(&[]).is_err());
    assert!(Policy::new(&[(Role::Ammo, &[*b"AMMO"])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[*b"ACTI", *b"ACTI"])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[*b"ACTI"]), (Role::Base, &[*b"MISC"])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[[0; 4]])]).is_err());
    let kinds = (0_u32..65)
        .map(|n| format!("{n:04}").as_bytes().try_into().unwrap())
        .collect::<Vec<[u8; 4]>>();
    assert!(Policy::new(&[(Role::Base, &kinds)]).is_err());
    let a = Policy::new(&[
        (Role::Base, &[*b"MISC", *b"ACTI"]),
        (Role::Ammo, &[*b"AMMO"]),
    ])
    .unwrap();
    let b = Policy::new(&[
        (Role::Ammo, &[*b"AMMO"]),
        (Role::Base, &[*b"ACTI", *b"MISC"]),
    ])
    .unwrap();
    assert_eq!(a.sha256(), b.sha256());
}
#[test]
fn canonical_budgets_and_missing_banks_still_apply_to_source_validated_mutations() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(
        &c,
        Limits {
            max_item_links: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    let owner = w.register_reference(None).unwrap();
    let before = w.snapshot();
    assert!(
        w.add_source_item(
            &content,
            &policy(),
            owner,
            Facts::unknown(form(0x100)),
            1.try_into().unwrap()
        )
        .is_err()
    );
    assert_eq!(w.snapshot(), before);
    w.initialize_inventory(owner).unwrap();
    let before = w.snapshot();
    let mut f = Facts::unknown(form(0x100));
    f.modifications = Some(vec![form(0x113)]);
    assert!(
        w.add_source_item(&content, &policy(), owner, f, 1.try_into().unwrap())
            .is_err()
    );
    assert_eq!(w.snapshot(), before);
}

#[test]
fn winning_kind_tombstones_and_master_namespaces_control_source_checks() {
    let (dir, _, _) = load_fixture();
    let mut other = header(&["FalloutNV.esm"]);
    other.extend(record(b"AMMO", 0x100, 0, &[]));
    other.extend(record(b"WEAP", 0x0100_0100, 0, &[]));
    other.extend(record(b"ACTI", 0x110, plugin::DELETED, &[]));
    std::fs::write(dir.path().join("Other.esm"), other).unwrap();
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into(), "Other.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let c = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &c, 100).unwrap();
    let w = World::new(&c, Limits::default()).unwrap();
    assert_eq!(
        content.source_form(&w, &form(0x100)).unwrap().kind,
        *b"AMMO"
    );
    assert!(source_items::validate(&w, &content, &policy(), &Facts::unknown(form(0x100))).is_err());
    let p = Policy::new(&[(Role::Base, &[*b"AMMO", *b"WEAP"])]).unwrap();
    source_items::validate(&w, &content, &p, &Facts::unknown(form(0x100))).unwrap();
    let mut own = form(0x100);
    own.origin_plugin = "other.esm".into();
    assert_eq!(
        source_items::validate(&w, &content, &p, &Facts::unknown(own))
            .unwrap()
            .forms[0]
            .source
            .kind,
        *b"WEAP"
    );
    assert!(matches!(
        source_items::validate(&w, &content, &p, &Facts::unknown(form(0x110))),
        Err(Failure::Source(
            fallout_runtime::foreign::Failure::DeletedForm(_)
        ))
    ));
}

#[test]
fn every_optional_content_role_rejects_wrong_kinds_atomically() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let (id, _) = w
        .add_source_item(
            &content,
            &policy(),
            owner,
            Facts::unknown(form(0x100)),
            1.try_into().unwrap(),
        )
        .unwrap();
    let before = w.snapshot();
    let mut actor = Facts::unknown(form(0x100));
    actor.ownership = Some(Ownership::Actor { key: form(0x111) });
    let mut faction = Facts::unknown(form(0x100));
    faction.ownership = Some(Ownership::Faction {
        key: form(0x112),
        rank: 0,
    });
    let mut ammo = Facts::unknown(form(0x100));
    ammo.ammo = Some(Ammo {
        base: form(0x113),
        count: 0,
    });
    let mut modification = Facts::unknown(form(0x100));
    modification.modifications = Some(vec![form(0x110)]);
    for facts in [actor, faction, ammo, modification] {
        assert!(matches!(
            w.replace_source_item_facts(&content, &policy(), id, facts),
            Err(Failure::Kind { .. })
        ));
        assert_eq!(w.snapshot(), before);
    }
}

fn initialization_fixture_at(root: &std::path::Path) -> (Catalogue, Content) {
    write_fixture(root, false);
    let mut bytes = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    for (kind, id) in [
        (*b"AMMO", 0x110),
        (*b"FACT", 0x111),
        (*b"NPC_", 0x112),
        (*b"IMOD", 0x113),
    ] {
        bytes.extend(record(&kind, id, 0, &[]));
    }
    bytes.extend(record(
        b"REFR",
        0x700,
        0,
        &field(b"NAME", &0x100_u32.to_le_bytes()),
    ));
    std::fs::write(root.join("FalloutNV.esm"), bytes).unwrap();
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn initialization_fixture() -> (tempfile::TempDir, Catalogue, Content) {
    let directory = tempfile::tempdir().unwrap();
    let (catalogue, content) = initialization_fixture_at(directory.path());
    (directory, catalogue, content)
}
fn initialization_world(
    catalogue: &Catalogue,
    limits: Limits,
) -> (World<'_>, ReferenceId, [(Facts, NonZeroU32); 2]) {
    let mut world =
        World::with_campaign(catalogue, limits, CampaignId::from_bytes([35; 16]).unwrap()).unwrap();
    let owner = world.register_reference(Some(form(0x700))).unwrap();
    let peer = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(owner),
        containing_reference: Some(peer),
        target: Some(ReferenceValue::Null),
        arguments: vec![ReferenceValue::Live { id: peer }],
    };
    let stage = world
        .stage_instance_initialization(
            &definition(catalogue),
            &Owner::Fragment {
                activation: 9.try_into().unwrap(),
            },
            &context,
            &[
                (2, Value::Number { bits: 1 << 63 }),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9abc,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: peer },
                    },
                ),
            ],
            initialization::Limits::default(),
        )
        .unwrap();
    let (_, handle) = world.commit_instance_initialization(stage).unwrap();
    world
        .enqueue(handle, Trigger::ObjectEvent { mask: 0x8000_0002 }, context)
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 2,
            game_nanoseconds: 11,
            menu_nanoseconds: 13,
            real_nanoseconds: 17,
        })
        .unwrap();
    let mut first = Facts::unknown(form(0x100));
    first.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abc,
    });
    first.ownership = Some(Ownership::Faction {
        key: form(0x111),
        rank: -3,
    });
    first.equipped_slots = Some(vec![7, 1]);
    first.ammo = Some(Ammo {
        base: form(0x110),
        count: 0,
    });
    first.modifications = Some(vec![form(0x113), form(0x113)]);
    first.quest_item = Some(false);
    first.script_instance = Some(world.instance(handle).unwrap().id());
    first.extra_fields = vec![OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![255, 0, 128],
    }];
    let mut second = first.clone();
    second.condition = Some(Condition::Float32 { bits: 0x8000_0000 });
    (
        world,
        owner,
        [
            (first, 11.try_into().unwrap()),
            (second, 2.try_into().unwrap()),
        ],
    )
}
fn assert_uninitialized_exact(world: &World<'_>, owner: ReferenceId, before: &Snapshot) {
    assert_eq!(&world.snapshot(), before);
    assert!(
        world
            .inventory_view(
                owner,
                ViewLimits {
                    max_items: 2,
                    max_links: 16,
                    max_extra_bytes: 6
                }
            )
            .unwrap()
            .items()
            .is_none()
    );
    assert!(world.inventory_count(owner, &form(0x100)).is_err());
}

#[test]
fn sequential_bank_boot_gap_is_reproduced_and_invalid_last_staged_source_lot_preserves_unknown_bank()
 {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut legacy, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = legacy.snapshot();
    legacy.initialize_inventory(owner).unwrap();
    legacy
        .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
        .unwrap();
    assert!(
        legacy
            .add_source_item(
                &content,
                &policy(),
                owner,
                Facts::unknown(form(0x777)),
                1.try_into().unwrap()
            )
            .is_err()
    );
    assert_eq!(legacy.inventory_count(owner, &form(0x100)).unwrap(), 11);
    assert_ne!(legacy.snapshot(), before);
    assert_eq!(legacy.snapshot().next_item, 2);
    let (world, owner, mut bad) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    bad[1].0.base = form(0x777);
    assert!(
        world
            .stage_source_inventory_initialization(
                &content,
                &policy(),
                owner,
                &bad,
                SourceInventoryLimits::default()
            )
            .is_err()
    );
    assert_uninitialized_exact(&world, owner, &before);
    if let Some(root) = std::env::var_os("FALLOUT_SOURCE_INVENTORY_EVIDENCE") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("legacy.before.json"),
            before.encode(1 << 20).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("legacy.partial.json"),
            legacy.snapshot().encode(1 << 20).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("stage.refused.json"),
            world.snapshot().encode(1 << 20).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn atomic_source_bank_publishes_ordered_distinct_lots_ids_roles_and_one_revision() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    assert_eq!(stage.owner(), owner);
    assert_eq!(stage.lots(), lots.as_slice());
    assert_eq!(stage.usage().lots, 2);
    assert_eq!(stage.usage().source_checks, 10);
    assert_eq!(stage.usage().links, 16);
    assert_eq!(stage.usage().extra_bytes, 6);
    assert_uninitialized_exact(&world, owner, &before);
    let receipt = world
        .commit_source_inventory_initialization(&content, &policy(), stage)
        .unwrap();
    assert_eq!(receipt.before_revision(), 5);
    assert_eq!(receipt.after_revision(), 6);
    assert_eq!(receipt.owner(), owner);
    assert_eq!(receipt.campaign(), world.campaign());
    assert_eq!(
        receipt.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    assert_eq!(receipt.policy_sha256(), policy().sha256());
    assert_eq!(
        receipt
            .item_ids()
            .iter()
            .map(|id| id.0.get())
            .collect::<Vec<_>>(),
        [1, 2]
    );
    for (index, (&id, proof)) in receipt.item_ids().iter().zip(receipt.proofs()).enumerate() {
        assert_eq!(world.item(id).unwrap().facts(), &lots[index].0);
        assert_eq!(world.item(id).unwrap().count(), lots[index].1.get());
        assert_eq!(proof.state_revision, 5);
        assert_eq!(
            proof.forms.iter().map(|row| row.role).collect::<Vec<_>>(),
            [
                Role::Base,
                Role::FactionOwner,
                Role::Ammo,
                Role::Modification,
                Role::Modification
            ]
        );
        assert_eq!(proof.forms[3].key, proof.forms[4].key);
    }
    assert_eq!(world.inventory_count(owner, &form(0x100)).unwrap(), 13);
    let after = world.snapshot();
    assert_eq!(after.next_item, 3);
    assert_eq!(after.instances, before.instances);
    assert_eq!(after.pending_events, before.pending_events);
    assert_eq!(after.clocks, before.clocks);
    assert_eq!(after.references, before.references);
    assert!(after.reference_states.is_empty());
    let trace = world.inventory_count_trace(owner, &form(0x100)).unwrap();
    assert_eq!(
        trace.contributions,
        receipt
            .item_ids()
            .iter()
            .copied()
            .zip([11, 2])
            .collect::<Vec<_>>()
    );
}

#[test]
fn every_bad_last_source_role_fact_or_live_link_leaves_bank_and_allocators_uninitialized() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let mut variants = Vec::new();
    for base in [form(0x110), form(0x200), form(0x777)] {
        let mut row = lots[1].0.clone();
        row.base = base;
        variants.push(row);
    }
    let mut row = lots[1].0.clone();
    row.ownership = Some(Ownership::Faction {
        key: form(0x112),
        rank: 0,
    });
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.ammo = Some(Ammo {
        base: form(0x113),
        count: 0,
    });
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.modifications = Some(vec![form(0x113), form(0x777)]);
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.equipped_slots = Some(vec![7, 7]);
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.ownership = Some(Ownership::Live {
        reference: ReferenceId(999.try_into().unwrap()),
    });
    variants.push(row);
    let mut row = lots[1].0.clone();
    row.script_instance = Some(InstanceId(999.try_into().unwrap()));
    variants.push(row);
    for bad in variants {
        assert!(
            world
                .stage_source_inventory_initialization(
                    &content,
                    &policy(),
                    owner,
                    &[(lots[0].0.clone(), lots[0].1), (bad, lots[1].1)],
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_uninitialized_exact(&world, owner, &before);
    }
    let missing = Policy::new(&[(Role::Base, &[*b"ACTI"])]).unwrap();
    let rows = [
        (Facts::unknown(form(0x100)), lots[0].1),
        (lots[1].0.clone(), lots[1].1),
    ];
    assert!(matches!(
        world.stage_source_inventory_initialization(
            &content,
            &missing,
            owner,
            &rows,
            SourceInventoryLimits::default()
        ),
        Err(Failure::MissingRule(Role::FactionOwner))
    ));
    assert_uninitialized_exact(&world, owner, &before);
}

#[test]
fn exact_and_one_under_lot_source_role_and_copy_limits_are_admitted_before_source_proofs() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let usage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap()
        .usage();
    let exact = SourceInventoryLimits {
        max_lots: 2,
        max_source_checks: 10,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(
        world
            .stage_source_inventory_initialization(&content, &policy(), owner, &lots, exact)
            .unwrap()
            .usage(),
        usage
    );
    for (limits, label) in [
        (
            SourceInventoryLimits {
                max_lots: 1,
                ..exact
            },
            "source inventory lots",
        ),
        (
            SourceInventoryLimits {
                max_source_checks: 9,
                ..exact
            },
            "source inventory source checks",
        ),
        (
            SourceInventoryLimits {
                max_copied_bytes: usage.copied_bytes - 1,
                ..exact
            },
            "source inventory copied bytes",
        ),
        (
            SourceInventoryLimits {
                max_copied_bytes: 0,
                ..exact
            },
            "source inventory copied bytes",
        ),
    ] {
        assert!(
            matches!(world.stage_source_inventory_initialization(&content,&policy(),owner,&lots,limits),Err(Failure::State(fallout_runtime::Error::Capacity(actual))) if actual==label)
        );
        assert_uninitialized_exact(&world, owner, &before);
    }
    let mut bad = lots;
    bad[1].0.base = form(0x777);
    assert!(matches!(
        world.stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &bad,
            SourceInventoryLimits {
                max_source_checks: 0,
                ..exact
            }
        ),
        Err(Failure::State(fallout_runtime::Error::Capacity(
            "source inventory source checks"
        )))
    ));
    assert_uninitialized_exact(&world, owner, &before);
}

#[test]
fn cumulative_canonical_caps_exhausted_ids_and_revision_refuse_without_partial_initialization() {
    let (_dir, catalogue, content) = initialization_fixture();
    for limits in [
        Limits {
            max_inventory_banks: 0,
            ..Limits::default()
        },
        Limits {
            max_item_instances: 1,
            ..Limits::default()
        },
        Limits {
            max_total_item_links: 15,
            ..Limits::default()
        },
        Limits {
            max_total_item_bytes: 5,
            ..Limits::default()
        },
        Limits {
            max_item_links: 7,
            ..Limits::default()
        },
        Limits {
            max_item_bytes: 2,
            ..Limits::default()
        },
    ] {
        let (world, owner, lots) = initialization_world(&catalogue, limits);
        let before = world.snapshot();
        assert!(
            world
                .stage_source_inventory_initialization(
                    &content,
                    &policy(),
                    owner,
                    &lots,
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_uninitialized_exact(&world, owner, &before);
    }
    let (world, owner, lots) = initialization_world(&catalogue, Limits::default());
    for (next, revision) in [(u64::MAX - 1, 5), (1, u64::MAX)] {
        let mut before = world.snapshot();
        before.next_item = next;
        before.state_revision = revision;
        let world = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
        assert!(
            world
                .stage_source_inventory_initialization(
                    &content,
                    &policy(),
                    owner,
                    &lots,
                    SourceInventoryLimits::default()
                )
                .is_err()
        );
        assert_uninitialized_exact(&world, owner, &before);
    }
}

#[test]
fn explicit_empty_initialization_is_real_and_existing_empty_or_nonempty_banks_are_never_reset() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &[],
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let receipt = world
        .commit_source_inventory_initialization(&content, &policy(), stage)
        .unwrap();
    assert!(receipt.item_ids().is_empty());
    assert!(receipt.proofs().is_empty());
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    assert_eq!(world.snapshot().next_item, before.next_item);
    assert_eq!(world.inventory_count(owner, &form(0x100)).unwrap(), 0);
    let empty = world.snapshot();
    assert!(
        world
            .stage_source_inventory_initialization(
                &content,
                &policy(),
                owner,
                &lots,
                SourceInventoryLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), empty);
    world
        .add_source_item(&content, &policy(), owner, lots[0].0.clone(), lots[0].1)
        .unwrap();
    let filled = world.snapshot();
    assert!(
        world
            .stage_source_inventory_initialization(
                &content,
                &policy(),
                owner,
                &[],
                SourceInventoryLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), filled);
}

#[test]
fn dropped_stale_restored_changed_policy_or_content_stages_never_publish_a_bank() {
    let (_dir, catalogue, content) = initialization_fixture();
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    drop(stage);
    assert_uninitialized_exact(&world, owner, &before);
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let changed_policy = Policy::new(&[(Role::Base, &[*b"ACTI", *b"MISC"])]).unwrap();
    assert!(matches!(
        world.commit_source_inventory_initialization(&content, &changed_policy, stage),
        Err(Failure::Policy("initialization policy changed"))
    ));
    assert_uninitialized_exact(&world, owner, &before);
    let other_dir = tempfile::tempdir().unwrap();
    let (_, other_content) = initialization_fixture_at(other_dir.path());
    // Append a distinct winning source before loading its new content index.
    let mut bytes = std::fs::read(other_dir.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"MISC", 0x114, 0, &[]));
    std::fs::write(other_dir.path().join("FalloutNV.esm"), bytes).unwrap();
    let mut store = RecordStore::open_nv_headers(
        other_dir.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let changed = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let changed_content = Content::load(&mut store, &changed, 100).unwrap();
    drop(other_content);
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    assert!(matches!(
        world.commit_source_inventory_initialization(&changed_content, &policy(), stage),
        Err(Failure::Source(
            fallout_runtime::foreign::Failure::ContentChanged
        ))
    ));
    assert_uninitialized_exact(&world, owner, &before);
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(matches!(
        world.commit_source_inventory_initialization(&content, &policy(), stage),
        Err(Failure::State(fallout_runtime::Error::StaleHandle))
    ));
    assert_uninitialized_exact(&world, owner, &before);
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let mut foreign = before.clone();
    foreign.campaign = CampaignId::from_bytes([36; 16]).unwrap();
    let mut other = World::restore(&catalogue, foreign.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.commit_source_inventory_initialization(&content, &policy(), stage),
        Err(Failure::State(fallout_runtime::Error::StaleHandle))
    ));
    assert_eq!(other.snapshot(), foreign);
    assert_uninitialized_exact(&world, owner, &before);
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    world.register_reference(None).unwrap();
    let after = world.snapshot();
    assert!(
        world
            .commit_source_inventory_initialization(&content, &policy(), stage)
            .is_err()
    );
    assert_uninitialized_exact(&world, owner, &after);
    let first = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let loser = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    world
        .commit_source_inventory_initialization(&content, &policy(), first)
        .unwrap();
    let winner = world.snapshot();
    assert!(
        world
            .commit_source_inventory_initialization(&content, &policy(), loser)
            .is_err()
    );
    assert_eq!(world.snapshot(), winner);
}

#[test]
fn source_inventory_before_current_native_and_two_fresh_cold_consumers_match_whole_snapshots() {
    use std::{fs, process::Command};
    let temporary = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_SOURCE_INVENTORY_EVIDENCE").map(std::path::PathBuf::from);
    let parent = retained.as_deref().unwrap_or(temporary.path());
    let root = parent.join("native-boundary");
    fs::create_dir_all(&root).unwrap();
    let (catalogue, content) = initialization_fixture_at(&root);
    let (mut world, owner, lots) = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let stage = world
        .stage_source_inventory_initialization(
            &content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let initialization = world
        .commit_source_inventory_initialization(&content, &policy(), stage)
        .unwrap();
    let after = world.snapshot();
    let second = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    worker.finish().unwrap();
    assert_eq!(first.wait().unwrap().metadata.generation, 1);
    assert_eq!(second.wait().unwrap().metadata.generation, 2);
    fs::write(
        root.join("expected.before.json"),
        before.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.after.json"),
        after.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("initialization.receipt.json"),
        serde_json::to_vec_pretty(&initialization).unwrap(),
    )
    .unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(world);
    drop(catalogue);
    drop(content);
    for (phase, snapshot, wire) in [("before", &before, &previous), ("after", &after, &current)] {
        let phase_root = root.join(phase);
        fs::create_dir(&phase_root).unwrap();
        let copied =
            Repository::create(&phase_root.join("native"), &[], snapshot.campaign).unwrap();
        fs::write(copied.path().join("current.frsv"), wire).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_source_inventory_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_SOURCE_INVENTORY_COLD_ROOT", &root)
            .env("FALLOUT_SOURCE_INVENTORY_COLD_PHASE", phase)
            .output()
            .unwrap();
        fs::write(phase_root.join("cold.stdout.txt"), &child.stdout).unwrap();
        fs::write(phase_root.join("cold.stderr.txt"), &child.stderr).unwrap();
        assert!(
            child.status.success(),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(fs::read(copied.path().join("current.frsv")).unwrap(), *wire);
    }
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        current
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}

#[test]
#[ignore = "fresh source inventory consumer invoked by parent"]
fn cold_source_inventory_helper() {
    use std::fs;
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_SOURCE_INVENTORY_COLD_ROOT").unwrap());
    let phase = std::env::var("FALLOUT_SOURCE_INVENTORY_COLD_PHASE").unwrap();
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join(&phase).join("native"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join(format!("expected.{phase}.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    let owner = ReferenceId(1.try_into().unwrap());
    let view = world
        .inventory_view(
            owner,
            ViewLimits {
                max_items: 2,
                max_links: 16,
                max_extra_bytes: 6,
            },
        )
        .unwrap();
    if phase == "before" {
        assert!(view.items().is_none());
        assert!(world.inventory_count(owner, &form(0x100)).is_err());
    } else {
        assert_eq!(world.inventory_count(owner, &form(0x100)).unwrap(), 13);
        assert_eq!(
            view.items()
                .unwrap()
                .iter()
                .map(|item| (item.id().0.get(), item.count()))
                .collect::<Vec<_>>(),
            [(1, 11), (2, 2)]
        );
    }
    fs::write(
        root.join(&phase).join("cold.restored.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.view.json"),
        serde_json::to_vec_pretty(&view).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

fn facts_fixture_at(root: &std::path::Path) -> (Catalogue, Content) {
    drop(initialization_fixture_at(root));
    let mut bytes = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"ACTI", 0x101, 0, &[]));
    std::fs::write(root.join("FalloutNV.esm"), bytes).unwrap();
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn facts_fixture() -> (tempfile::TempDir, Catalogue, Content) {
    let root = tempfile::tempdir().unwrap();
    let (catalogue, content) = facts_fixture_at(root.path());
    (root, catalogue, content)
}
fn facts_world<'a>(
    catalogue: &'a Catalogue,
    content: &Content,
    limits: Limits,
) -> (World<'a>, [(ItemHandle, Facts); 2]) {
    let (mut world, owner, lots) = initialization_world(catalogue, limits);
    let stage = world
        .stage_source_inventory_initialization(
            content,
            &policy(),
            owner,
            &lots,
            SourceInventoryLimits::default(),
        )
        .unwrap();
    let receipt = world
        .commit_source_inventory_initialization(content, &policy(), stage)
        .unwrap();
    let peer = ReferenceId(2.try_into().unwrap());
    world.initialize_inventory(peer).unwrap();
    let mut third = lots[0].0.clone();
    third.base = form(0x101);
    let third = world
        .add_source_item(content, &policy(), peer, third, 5.try_into().unwrap())
        .unwrap()
        .0;
    let mut first = Facts::unknown(form(0x101));
    first.condition = Some(Condition::Float32 { bits: 0x7fc0_1234 });
    first.equipped_slots = Some(Vec::new());
    first.modifications = Some(Vec::new());
    first.extra_fields = vec![OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 128, 255, 0],
    }];
    let mut second = Facts::unknown(form(0x100));
    second.condition = Some(Condition::Float64 {
        bits: 0x8000_0000_0000_0000,
    });
    second.ownership = Some(Ownership::Unowned);
    let handles = [
        world.item_handle(receipt.item_ids()[0]).unwrap(),
        world.item_handle(third).unwrap(),
    ];
    (world, [(handles[0], first), (handles[1], second)])
}
fn assert_facts_before(world: &World<'_>, before: &Snapshot) {
    assert_eq!(&world.snapshot(), before);
    for bank in &before.inventory_banks {
        for base in [form(0x100), form(0x101)] {
            let expected = bank
                .items
                .iter()
                .filter(|item| item.facts().base == base)
                .map(|item| u64::from(item.count()))
                .sum::<u64>();
            let trace = world.inventory_count_trace(bank.owner, &base).unwrap();
            assert_eq!(trace.result, expected);
            assert_eq!(
                trace.contributions,
                bank.items
                    .iter()
                    .filter(|item| item.facts().base == base)
                    .map(|item| (item.id(), item.count()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn source_facts_sequential_bad_last_gap_and_batch_refusal_preserve_the_whole_bank() {
    let (_root, catalogue, content) = facts_fixture();
    let (mut world, mut edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    edits[1].1.base = form(0x777);
    assert!(
        world
            .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
            .is_err()
    );
    assert_facts_before(&world, &before);
    let id = world.item_id(edits[0].0).unwrap();
    world
        .replace_source_item_facts(&content, &policy(), id, edits[0].1.clone())
        .unwrap();
    assert!(
        world
            .replace_source_item_facts(
                &content,
                &policy(),
                world.item_id(edits[1].0).unwrap(),
                edits[1].1.clone()
            )
            .is_err()
    );
    assert_ne!(world.snapshot(), before);
    assert_eq!(world.revision(), 9);
    if let Some(root) = std::env::var_os("FALLOUT_SOURCE_FACTS_EVIDENCE") {
        let root = std::path::PathBuf::from(root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("legacy.before.json"),
            before.encode(1 << 20).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("legacy.partial.json"),
            world.snapshot().encode(1 << 20).unwrap(),
        )
        .unwrap();
    }
}
#[test]
fn source_facts_cross_owner_edit_keeps_lot_ids_quantities_and_other_lots_exact() {
    let (_root, catalogue, content) = facts_fixture();
    let (mut world, edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    assert_eq!(stage.rows().len(), 2);
    assert_eq!(stage.usage().lots, 2);
    assert_eq!(stage.usage().links, 18);
    assert_eq!(stage.usage().extra_bytes, 10);
    assert_eq!(stage.usage().source_checks, 2);
    for (row, (_, facts)) in stage.rows().iter().zip(&edits) {
        assert_eq!(row.replacement(), facts);
    }
    assert_facts_before(&world, &before);
    let receipt = world
        .commit_source_item_facts(&content, &policy(), stage)
        .unwrap();
    assert_eq!(receipt.before_revision(), 8);
    assert_eq!(receipt.after_revision(), 9);
    assert_eq!(receipt.campaign(), world.campaign());
    assert_eq!(
        receipt.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    assert_eq!(receipt.policy_sha256(), policy().sha256());
    assert_eq!(
        receipt
            .item_ids()
            .iter()
            .map(|id| id.0.get())
            .collect::<Vec<_>>(),
        [1, 3]
    );
    assert_eq!(receipt.proofs().len(), 2);
    for proof in receipt.proofs() {
        assert_eq!(proof.state_revision, 8);
        assert_eq!(proof.forms.len(), 1);
        assert_eq!(proof.forms[0].role, Role::Base);
    }
    let mut expected = serde_json::to_value(&before).unwrap();
    expected["state_revision"] = 9.into();
    for (handle, facts) in &edits {
        let id = world.item_id(*handle).unwrap();
        let old = before
            .inventory_banks
            .iter()
            .flat_map(|bank| &bank.items)
            .find(|item| item.id() == id)
            .unwrap();
        let item = world.item(id).unwrap();
        assert_eq!(item.owner(), old.owner());
        assert_eq!(item.count(), old.count());
        assert_eq!(item.facts(), facts);
        let row = expected["inventory_banks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .flat_map(|bank| bank["items"].as_array_mut().unwrap())
            .find(|item| item["id"] == id.0.get())
            .unwrap();
        row["facts"] = serde_json::to_value(facts).unwrap();
    }
    let expected: Snapshot = serde_json::from_value(expected).unwrap();
    assert_facts_before(&world, &expected);
    let owner = ReferenceId(1.try_into().unwrap());
    let peer = ReferenceId(2.try_into().unwrap());
    assert_eq!(
        receipt
            .count_changes()
            .iter()
            .map(|row| (
                row.owner().0.get(),
                row.base().local_id,
                row.before(),
                row.after()
            ))
            .collect::<Vec<_>>(),
        [
            (1, 0x100, 13, 2),
            (1, 0x101, 0, 11),
            (2, 0x100, 0, 5),
            (2, 0x101, 5, 0)
        ]
    );
    assert_eq!(world.inventory_count(owner, &form(0x101)).unwrap(), 11);
    assert_eq!(world.inventory_count(peer, &form(0x101)).unwrap(), 0);
}
#[test]
fn source_facts_same_owner_swap_and_net_link_byte_capacity_are_order_independent() {
    let (_root, catalogue, content) = facts_fixture();
    let limits = Limits {
        max_total_item_links: 24,
        max_total_item_bytes: 9,
        ..Limits::default()
    };
    let (mut world, edits) = facts_world(&catalogue, &content, limits);
    let before = world.snapshot();
    let second = world.item_handle(ItemId(2.try_into().unwrap())).unwrap();
    let mut bigger = world.item_by_handle(edits[0].0).unwrap().facts().clone();
    bigger.base = form(0x101);
    bigger.extra_fields[0].bytes = vec![0, 255, 128, 1, 2, 3];
    bigger.equipped_slots = Some(vec![7, 1, 9, 10]);
    let smaller = Facts::unknown(form(0x100));
    let edits = [(edits[0].0, bigger), (second, smaller)];
    // Sequential first replacement exceeds both aggregate byte and link room;
    // replacing the second lot in the same operation supplies that room.
    assert!(
        world
            .replace_source_item_facts(
                &content,
                &policy(),
                ItemId(1.try_into().unwrap()),
                edits[0].1.clone()
            )
            .is_err()
    );
    assert_facts_before(&world, &before);
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    world
        .commit_source_item_facts(&content, &policy(), stage)
        .unwrap();
    assert_eq!(world.revision(), before.state_revision + 1);
    assert_eq!(world.snapshot().next_item, before.next_item);
    assert_eq!(
        world
            .inventory_count(ReferenceId(1.try_into().unwrap()), &form(0x100))
            .unwrap(),
        2
    );
    assert_eq!(
        world
            .inventory_count(ReferenceId(1.try_into().unwrap()), &form(0x101))
            .unwrap(),
        11
    );
    let mut alternate = World::restore(&catalogue, before, limits).unwrap();
    let reversed = edits
        .iter()
        .rev()
        .map(|(handle, facts)| {
            (
                alternate
                    .item_handle(world.item_id(*handle).unwrap())
                    .unwrap(),
                facts.clone(),
            )
        })
        .collect::<Vec<_>>();
    let stage = alternate
        .stage_source_item_facts(&content, &policy(), &reversed, SourceFactsLimits::default())
        .unwrap();
    alternate
        .commit_source_item_facts(&content, &policy(), stage)
        .unwrap();
    assert_eq!(alternate.snapshot(), world.snapshot());
}
#[test]
fn source_facts_bad_last_source_or_fact_and_duplicate_item_refuse_before_publication() {
    let (_root, catalogue, content) = facts_fixture();
    let (world, edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    let mut variants = Vec::new();
    for base in [form(0x110), form(0x200), form(0x777)] {
        let mut facts = edits[1].1.clone();
        facts.base = base;
        variants.push(facts);
    }
    let mut facts = edits[1].1.clone();
    facts.equipped_slots = Some(vec![7, 7]);
    variants.push(facts);
    let mut facts = edits[1].1.clone();
    facts.ownership = Some(Ownership::Live {
        reference: ReferenceId(999.try_into().unwrap()),
    });
    variants.push(facts);
    let mut facts = edits[1].1.clone();
    facts.script_instance = Some(InstanceId(999.try_into().unwrap()));
    variants.push(facts);
    let mut facts = edits[1].1.clone();
    facts.modifications = Some(vec![form(0x113), form(0x777)]);
    variants.push(facts);
    let mut facts = edits[1].1.clone();
    facts.ammo = Some(Ammo {
        base: form(0x113),
        count: 0,
    });
    variants.push(facts);
    for facts in variants {
        let bad = [edits[0].clone(), (edits[1].0, facts)];
        assert!(
            world
                .stage_source_item_facts(&content, &policy(), &bad, SourceFactsLimits::default())
                .is_err()
        );
        assert_facts_before(&world, &before);
    }
    let duplicate = [edits[0].clone(), edits[0].clone()];
    assert!(matches!(
        world.stage_source_item_facts(
            &content,
            &policy(),
            &duplicate,
            SourceFactsLimits::default()
        ),
        Err(Failure::State(fallout_runtime::Error::Invalid(_)))
    ));
    assert_facts_before(&world, &before);
    let mut bad = edits;
    bad[1].1.ownership = Some(Ownership::Faction {
        key: form(0x111),
        rank: -3,
    });
    let missing = Policy::new(&[(Role::Base, &[*b"ACTI"])]).unwrap();
    assert!(matches!(
        world.stage_source_item_facts(&content, &missing, &bad, SourceFactsLimits::default()),
        Err(Failure::MissingRule(Role::FactionOwner))
    ));
    assert_facts_before(&world, &before);
}
#[test]
fn source_facts_exact_one_under_copy_lot_link_source_and_net_canonical_bounds() {
    let (_root, catalogue, content) = facts_fixture();
    let (world, edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    let usage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap()
        .usage();
    let exact = SourceFactsLimits {
        max_lots: 2,
        max_links: 18,
        max_source_checks: 2,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(
        world
            .stage_source_item_facts(&content, &policy(), &edits, exact)
            .unwrap()
            .usage(),
        usage
    );
    for limits in [
        SourceFactsLimits {
            max_lots: 1,
            ..exact
        },
        SourceFactsLimits {
            max_links: 17,
            ..exact
        },
        SourceFactsLimits {
            max_source_checks: 1,
            ..exact
        },
        SourceFactsLimits {
            max_copied_bytes: usage.copied_bytes - 1,
            ..exact
        },
        SourceFactsLimits {
            max_copied_bytes: 0,
            ..exact
        },
    ] {
        assert!(
            world
                .stage_source_item_facts(&content, &policy(), &edits, limits)
                .is_err()
        );
        assert_facts_before(&world, &before);
    }
    for limits in [
        Limits {
            max_total_item_links: 24,
            ..Limits::default()
        },
        Limits {
            max_total_item_bytes: 9,
            ..Limits::default()
        },
        Limits {
            max_item_links: 8,
            ..Limits::default()
        },
        Limits {
            max_item_bytes: 6,
            ..Limits::default()
        },
    ] {
        let (world, edits) = facts_world(&catalogue, &content, limits);
        let before = world.snapshot();
        let mut bad = edits.clone();
        bad[0].1 = world.item_by_handle(edits[0].0).unwrap().facts().clone();
        bad[1].1 = world.item_by_handle(edits[1].0).unwrap().facts().clone();
        bad[0].1.equipped_slots = Some((0..10).collect());
        bad[0].1.extra_fields[0].bytes = vec![255; 7];
        assert!(
            world
                .stage_source_item_facts(&content, &policy(), &bad, SourceFactsLimits::default())
                .is_err()
        );
        assert_facts_before(&world, &before);
    }
}
#[test]
fn source_facts_empty_identical_exhausted_revision_and_drop_semantics_are_explicit() {
    let (_root, catalogue, content) = facts_fixture();
    let (mut world, edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    assert!(
        world
            .stage_source_item_facts(&content, &policy(), &[], SourceFactsLimits::default())
            .is_err()
    );
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    drop(stage);
    assert_facts_before(&world, &before);
    let identical = edits
        .iter()
        .map(|(handle, _)| {
            (
                *handle,
                world.item_by_handle(*handle).unwrap().facts().clone(),
            )
        })
        .collect::<Vec<_>>();
    let stage = world
        .stage_source_item_facts(
            &content,
            &policy(),
            &identical,
            SourceFactsLimits::default(),
        )
        .unwrap();
    assert!(stage.count_changes().is_empty());
    let receipt = world
        .commit_source_item_facts(&content, &policy(), stage)
        .unwrap();
    assert!(receipt.count_changes().is_empty());
    let mut expected = before;
    expected.state_revision += 1;
    assert_facts_before(&world, &expected);
    let ids = edits
        .iter()
        .map(|(handle, _)| world.item_id(*handle).unwrap())
        .collect::<Vec<_>>();
    expected.state_revision = u64::MAX;
    world.replace_from_snapshot(expected.clone()).unwrap();
    let current = edits
        .iter()
        .zip(ids)
        .map(|((_, facts), id)| (world.item_handle(id).unwrap(), facts.clone()))
        .collect::<Vec<_>>();
    assert!(
        world
            .stage_source_item_facts(&content, &policy(), &current, SourceFactsLimits::default())
            .is_err()
    );
    assert_facts_before(&world, &expected);
}
#[test]
fn source_facts_stale_handles_stages_policy_content_and_peer_commit_never_partly_edit() {
    let (_root, catalogue, content) = facts_fixture();
    let (mut world, edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    let changed = Policy::new(&[(Role::Base, &[*b"ACTI", *b"MISC"])]).unwrap();
    assert!(matches!(
        world.commit_source_item_facts(&content, &changed, stage),
        Err(Failure::Policy("facts edit policy changed"))
    ));
    assert_facts_before(&world, &before);
    let other = tempfile::tempdir().unwrap();
    drop(facts_fixture_at(other.path()));
    let mut bytes = std::fs::read(other.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"MISC", 0x114, 0, &[]));
    std::fs::write(other.path().join("FalloutNV.esm"), bytes).unwrap();
    let mut store = RecordStore::open_nv_headers(
        other.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let other_catalogue =
        Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let other_content = Content::load(&mut store, &other_catalogue, 100).unwrap();
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    assert!(
        world
            .commit_source_item_facts(&other_content, &policy(), stage)
            .is_err()
    );
    assert_facts_before(&world, &before);
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    let mut foreign = before.clone();
    foreign.campaign = CampaignId::from_bytes([36; 16]).unwrap();
    let mut foreign_world = World::restore(&catalogue, foreign.clone(), Limits::default()).unwrap();
    assert!(
        foreign_world
            .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
            .is_err()
    );
    assert!(
        foreign_world
            .commit_source_item_facts(&content, &policy(), stage)
            .is_err()
    );
    assert_facts_before(&foreign_world, &foreign);
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(
        world
            .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
            .is_err()
    );
    assert!(
        world
            .commit_source_item_facts(&content, &policy(), stage)
            .is_err()
    );
    assert_facts_before(&world, &before);
    let edits = [
        (
            world.item_handle(ItemId(1.try_into().unwrap())).unwrap(),
            edits[0].1.clone(),
        ),
        (
            world.item_handle(ItemId(3.try_into().unwrap())).unwrap(),
            edits[1].1.clone(),
        ),
    ];
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    world
        .remove_item_quantity(ItemId(3.try_into().unwrap()), 1.try_into().unwrap())
        .unwrap();
    let after = world.snapshot();
    assert!(
        world
            .commit_source_item_facts(&content, &policy(), stage)
            .is_err()
    );
    assert_facts_before(&world, &after);
    let winner = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    let loser = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    world
        .commit_source_item_facts(&content, &policy(), winner)
        .unwrap();
    let after = world.snapshot();
    assert!(
        world
            .commit_source_item_facts(&content, &policy(), loser)
            .is_err()
    );
    assert_facts_before(&world, &after);
}
#[test]
fn source_facts_before_current_native_and_two_fresh_cold_consumers_are_exact() {
    use std::{fs, process::Command};
    let temporary = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("FALLOUT_SOURCE_FACTS_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained
        .as_deref()
        .unwrap_or(temporary.path())
        .join("native-boundary");
    fs::create_dir_all(&root).unwrap();
    let (catalogue, content) = facts_fixture_at(&root);
    let (mut world, edits) = facts_world(&catalogue, &content, Limits::default());
    let before = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let stage = world
        .stage_source_item_facts(&content, &policy(), &edits, SourceFactsLimits::default())
        .unwrap();
    let receipt = world
        .commit_source_item_facts(&content, &policy(), stage)
        .unwrap();
    let after = world.snapshot();
    let second = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    worker.finish().unwrap();
    assert_eq!(first.wait().unwrap().metadata.generation, 1);
    assert_eq!(second.wait().unwrap().metadata.generation, 2);
    fs::write(
        root.join("expected.before.json"),
        before.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.after.json"),
        after.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("facts.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(world);
    drop(catalogue);
    drop(content);
    for (phase, snapshot, wire) in [("before", &before, &previous), ("after", &after, &current)] {
        let phase_root = root.join(phase);
        fs::create_dir(&phase_root).unwrap();
        let copy = Repository::create(&phase_root.join("native"), &[], snapshot.campaign).unwrap();
        fs::write(copy.path().join("current.frsv"), wire).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_source_facts_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_SOURCE_FACTS_COLD_ROOT", &root)
            .env("FALLOUT_SOURCE_FACTS_COLD_PHASE", phase)
            .output()
            .unwrap();
        fs::write(phase_root.join("cold.stdout.txt"), &child.stdout).unwrap();
        fs::write(phase_root.join("cold.stderr.txt"), &child.stderr).unwrap();
        assert!(
            child.status.success(),
            "{}{}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        assert_eq!(fs::read(copy.path().join("current.frsv")).unwrap(), *wire);
    }
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        current
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}
#[test]
#[ignore = "fresh source facts consumer invoked by parent"]
fn cold_source_facts_helper() {
    use std::fs;
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_SOURCE_FACTS_COLD_ROOT").unwrap());
    let phase = std::env::var("FALLOUT_SOURCE_FACTS_COLD_PHASE").unwrap();
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join(&phase).join("native"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join(format!("expected.{phase}.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_facts_before(&world, &expected);
    let views = [1, 2]
        .iter()
        .map(|id| {
            world
                .inventory_view(
                    ReferenceId((*id).try_into().unwrap()),
                    ViewLimits {
                        max_items: 2,
                        max_links: 16,
                        max_extra_bytes: 7,
                    },
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    fs::write(
        root.join(&phase).join("cold.restored.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.views.json"),
        serde_json::to_vec_pretty(&views).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

fn continue_id(value: u64) -> NonZeroU64 {
    value.try_into().unwrap()
}

fn application_sources(root: &Path) -> (Arc<Catalogue>, Arc<Content>) {
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap());
    let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
    (catalogue, content)
}

struct ContinueSourceScene {
    host_identity: HostIdentity,
    generation: NonZeroU64,
    displayed: Snapshot,
    content: Arc<Content>,
    publishes: usize,
}

impl ContinueSourceScene {
    fn current(host: &application::Host<'_>, content: Arc<Content>) -> Self {
        Self {
            host_identity: host.identity(),
            generation: host.scene_generation(),
            displayed: host.world().snapshot(),
            content,
            publishes: 0,
        }
    }
}

impl ScenePublisher for ContinueSourceScene {
    type Stage = Snapshot;

    fn prepare(
        &self,
        candidate: &World<'_>,
        boundary: &ContinueBoundary,
    ) -> application::Result<Snapshot> {
        if self.host_identity != boundary.prior_host_identity()
            || self.generation != boundary.scene_generation()
            || self.displayed.state_revision != boundary.prior_revision()
        {
            return Err(application::Failure::Refused("scene boundary changed"));
        }
        // Revalidate through the real authored source index at the scene gate.
        // A refusal flows back through Host::publish_continue before mutation.
        self.content.validate_world(candidate)?;
        Ok(candidate.snapshot())
    }

    fn publish(&mut self, stage: Snapshot, boundary: &ContinueBoundary) {
        self.displayed = stage;
        self.host_identity = boundary.candidate_host_identity();
        self.publishes += 1;
    }
}

fn application_candidate(
    repository: &Repository,
    catalogue: Arc<Catalogue>,
    request: &application::ContinueRequest,
) -> (RestoreTask, RestoredCandidate) {
    let mut task = RestoreTask::start(
        repository.clone(),
        catalogue,
        Limits::default(),
        Recovery::Strict,
        request.identity().clone(),
    )
    .unwrap();
    task.finish().unwrap();
    let RestorePoll::Ready(candidate) = task.try_poll() else {
        panic!("expected native restore candidate")
    };
    (task, *candidate)
}

#[test]
fn changed_authored_sources_refuse_continue_before_host_scene_and_next_save_publication() {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), false);
    let source_path = root.path().join("FalloutNV.esm");
    let mut source_bytes = std::fs::read(&source_path).unwrap();
    source_bytes.extend(record(b"REFR", 0x500, 0, &[]));
    source_bytes.extend(record(b"REFR", 0x501, 0, &[]));
    std::fs::write(&source_path, &source_bytes).unwrap();
    let original_source = source_bytes;
    let (catalogue, content) = application_sources(root.path());

    let mut world = World::new(Arc::clone(&catalogue), Limits::default()).unwrap();
    let source = world.register_reference(Some(form(0x500))).unwrap();
    let target = world.register_reference(Some(form(0x501))).unwrap();
    world.initialize_inventory(source).unwrap();
    world.initialize_inventory(target).unwrap();
    let (item, _) = world
        .add_source_item(
            &content,
            &policy(),
            source,
            Facts::unknown(form(0x100)),
            8.try_into().unwrap(),
        )
        .unwrap();
    let saved = world.snapshot();
    let repository =
        Repository::create(&root.path().join("native"), &[], world.campaign()).unwrap();
    let mut initial_worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let initial_ticket = initial_worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap();
    initial_worker.finish().unwrap();
    assert_eq!(initial_ticket.wait().unwrap().metadata.generation, 1);

    let mut host = application::Host::new(
        world,
        Arc::clone(&content),
        policy(),
        continue_id(7),
        application::HostLimits::default(),
    )
    .unwrap();
    let old_transfer = host
        .select_transfer(source, item, target, 8)
        .unwrap()
        .command(continue_id(1));
    host.transfer(old_transfer.clone()).unwrap();
    let active = host.world().snapshot();
    assert_ne!(active, saved);
    let prior_host = host.identity();
    let mut scene = ContinueSourceScene::current(&host, Arc::clone(&content));

    let request = host.begin_continue(continue_id(1)).unwrap();
    let (_task, candidate) = application_candidate(&repository, Arc::clone(&catalogue), &request);
    let prepared = host.prepare_continue(request, candidate).unwrap();
    assert_eq!(prepared.world().snapshot(), saved);
    assert_eq!(host.world().snapshot(), active);

    let mut changed_source = original_source.clone();
    changed_source.extend(record(b"MISC", 0x114, 0, &[]));
    std::fs::write(&source_path, &changed_source).unwrap();
    assert_ne!(std::fs::read(&source_path).unwrap(), original_source);
    let (_changed_catalogue, changed_content) = application_sources(root.path());
    scene.content = changed_content;

    assert!(matches!(
        host.publish_continue(prepared, &mut scene),
        Err(application::Failure::Source(
            fallout_runtime::foreign::Failure::ContentChanged
        ))
    ));
    assert_eq!(host.world().snapshot(), active);
    assert_eq!(host.identity(), prior_host);
    assert_eq!(scene.displayed, active);
    assert_eq!(scene.publishes, 0);
    assert!(host.transfer(old_transfer.clone()).unwrap().replayed);

    // The failed scene admission consumed no save identity. The existing save
    // worker accepts the same next request and captures the still-current host.
    let save_request = host.select_save(continue_id(1)).unwrap();
    let mut save_worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let submission = host.submit_save(save_request, &mut save_worker).unwrap();
    assert_eq!(submission.boundary().request_id(), continue_id(1));
    assert!(submission.matches_current_boundary(&host));
    save_worker.finish().unwrap();
    assert_eq!(submission.wait().unwrap().metadata.generation, 2);
    assert_eq!(host.world().snapshot(), active);
    assert!(host.select_save(continue_id(1)).is_err());
    assert!(host.select_save(continue_id(2)).is_ok());

    // With the exact original source restored, a same-revision Continue
    // publishes once and expires the previously selected source transaction.
    std::fs::write(&source_path, &original_source).unwrap();
    let mut scene = ContinueSourceScene::current(&host, Arc::clone(&content));
    let request = host.begin_continue(continue_id(2)).unwrap();
    let (_task, candidate) = application_candidate(&repository, Arc::clone(&catalogue), &request);
    let prepared = host.prepare_continue(request, candidate).unwrap();
    assert_eq!(prepared.world().snapshot(), active);
    let receipt = host.publish_continue(prepared, &mut scene).unwrap();
    assert_eq!(receipt.boundary.prior_revision(), active.state_revision);
    assert_eq!(receipt.boundary.candidate_revision(), active.state_revision);
    assert_eq!(host.world().snapshot(), active);
    assert_eq!(scene.displayed, active);
    assert_eq!(scene.publishes, 1);
    assert_ne!(host.identity(), prior_host);
    assert!(matches!(
        host.transfer(old_transfer),
        Err(application::Failure::ExpiredSelection)
    ));
}
