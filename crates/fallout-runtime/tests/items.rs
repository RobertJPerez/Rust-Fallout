mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, InstanceId, Owner, ReferenceId, ReferenceValue, Value},
    inventory::{
        Ammo, Condition, Facts, ItemId, OpaqueExtra, Ownership, TransferLimits, ViewLimits,
        ViewUsage,
    },
    save::{Captured, Recovery, Repository},
    snapshot::Snapshot,
    state::initialization,
};
use std::num::NonZeroU32;
fn quantity(n: u32) -> NonZeroU32 {
    n.try_into().unwrap()
}
fn fixture() -> (tempfile::TempDir, fallout_data::loaded_scripts::Catalogue) {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let c = load(dir.path(), &["FalloutNV.esm"]);
    (dir, c)
}
fn facts() -> Facts {
    let mut f = Facts::unknown(form(0x100));
    f.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abc,
    });
    f.ownership = Some(Ownership::Faction {
        key: form(0x123),
        rank: -7,
    });
    f.equipped_slots = Some(vec![7, 1]);
    f.ammo = Some(Ammo {
        base: form(0x321),
        count: 0,
    });
    f.modifications = Some(vec![form(0x456)]);
    f.quest_item = Some(false);
    f.extra_fields = vec![OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 255, 1],
    }];
    f
}
#[test]
fn missing_initialization_differs_from_an_explicit_empty_bank() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    let before = w.snapshot();
    assert!(w.inventory_count(owner, &form(0x100)).is_err());
    assert!(w.add_item(owner, facts(), quantity(1)).is_err());
    assert_eq!(before, w.snapshot());
    w.initialize_inventory(owner).unwrap();
    assert_eq!(w.inventory_count(owner, &form(0x100)).unwrap(), 0);
    let before = w.snapshot();
    assert!(w.initialize_inventory(owner).is_err());
    assert_eq!(before, w.snapshot());
}

#[test]
fn immutable_views_preserve_unknown_empty_and_campaign_source_revision() {
    let (_dir, catalogue) = fixture();
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let owner = world.register_reference(Some(form(0x100))).unwrap();
    let zero = ViewLimits {
        max_items: 0,
        max_links: 0,
        max_extra_bytes: 0,
    };
    let before = world.snapshot();
    let unknown = world.inventory_view(owner, zero).unwrap();
    assert_eq!(unknown.items(), None);
    assert_eq!(unknown.usage(), ViewUsage::default());
    assert_eq!(unknown.campaign(), world.campaign());
    assert_eq!(
        unknown.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    assert_eq!(unknown.revision(), world.revision());
    assert_eq!(unknown.boundary(), world.clocks());
    assert_eq!(unknown.owner(), owner);
    assert_eq!(unknown.authored(), Some(&form(0x100)));
    assert_eq!(world.snapshot(), before);
    assert!(matches!(
        world.inventory_view(ReferenceId(999.try_into().unwrap()), zero),
        Err(fallout_runtime::Error::MissingReference)
    ));
    assert_eq!(world.snapshot(), before);
    world.initialize_inventory(owner).unwrap();
    let empty = world.inventory_view(owner, zero).unwrap();
    assert_eq!(empty.items(), Some([].as_slice()));
    assert!(empty.revision() > unknown.revision());
    assert!(unknown.items().is_none());
}

#[test]
fn owned_lot_views_survive_mutation_world_drop_and_thread_transfer_without_merging() {
    let (_dir, catalogue) = fixture();
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let a = world.register_reference(None).unwrap();
    let b = world.register_reference(None).unwrap();
    world.initialize_inventory(a).unwrap();
    world.initialize_inventory(b).unwrap();
    let first = world.add_item(a, facts(), quantity(u32::MAX)).unwrap();
    let mut second_facts = facts();
    second_facts.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let second = world
        .add_item(a, second_facts.clone(), quantity(u32::MAX))
        .unwrap();
    let limits = ViewLimits {
        max_items: 2,
        max_links: 12,
        max_extra_bytes: 6,
    };
    let old = world.inventory_view(a, limits).unwrap();
    assert_eq!(
        old.items()
            .unwrap()
            .iter()
            .map(|item| item.id())
            .collect::<Vec<_>>(),
        [first, second]
    );
    assert_eq!(old.items().unwrap()[0].facts(), &facts());
    assert_eq!(old.items().unwrap()[1].facts(), &second_facts);
    assert_eq!(
        old.items()
            .unwrap()
            .iter()
            .map(|item| u64::from(item.count()))
            .sum::<u64>(),
        2 * u64::from(u32::MAX)
    );
    world.transfer_item(first, b).unwrap();
    world.remove_item_quantity(second, quantity(1)).unwrap();
    let current = world.inventory_view(a, limits).unwrap();
    assert!(current.revision() > old.revision());
    assert_eq!(current.items().unwrap().len(), 1);
    assert_eq!(current.items().unwrap()[0].count(), u32::MAX - 1);
    drop(world);
    std::thread::spawn(move || {
        assert_eq!(old.items().unwrap().len(), 2);
        assert_eq!(old.items().unwrap()[0].owner(), a);
        assert_eq!(old.items().unwrap()[1].count(), u32::MAX);
        assert_eq!(old.items().unwrap()[1].facts(), &second_facts);
    })
    .join()
    .unwrap();
}

#[test]
fn view_admission_accepts_exact_limits_and_refuses_one_over_without_mutation() {
    let (_dir, catalogue) = fixture();
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let owner = world.register_reference(None).unwrap();
    world.initialize_inventory(owner).unwrap();
    world.add_item(owner, facts(), quantity(2)).unwrap();
    world.add_item(owner, facts(), quantity(3)).unwrap();
    let exact = ViewLimits {
        max_items: 2,
        max_links: 12,
        max_extra_bytes: 6,
    };
    let view = world.inventory_view(owner, exact).unwrap();
    assert_eq!(
        view.usage(),
        ViewUsage {
            items: 2,
            links: 12,
            extra_bytes: 6
        }
    );
    let before = world.snapshot();
    for (limits, expected) in [
        (
            ViewLimits {
                max_items: 1,
                ..exact
            },
            "inventory view items",
        ),
        (
            ViewLimits {
                max_links: 11,
                ..exact
            },
            "inventory view links",
        ),
        (
            ViewLimits {
                max_extra_bytes: 5,
                ..exact
            },
            "inventory view extra bytes",
        ),
    ] {
        assert!(
            matches!(world.inventory_view(owner, limits), Err(fallout_runtime::Error::Capacity(label)) if label == expected)
        );
        assert_eq!(world.snapshot(), before);
    }
}
#[test]
fn split_transfer_and_removal_conserve_counts_and_keep_exact_facts() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let a = w.register_reference(None).unwrap();
    let b = w.register_reference(None).unwrap();
    w.initialize_inventory(a).unwrap();
    w.initialize_inventory(b).unwrap();
    let id = w.add_item(a, facts(), quantity(17)).unwrap();
    let revision = w.revision();
    let other = w.split_item(id, quantity(5)).unwrap();
    assert_ne!(id, other);
    assert_eq!(w.revision(), revision + 1);
    assert_eq!(w.item(id).unwrap().facts(), w.item(other).unwrap().facts());
    assert_eq!(w.inventory_count(a, &form(0x100)).unwrap(), 17);
    w.transfer_item(other, b).unwrap();
    assert_eq!(w.item(other).unwrap().owner(), b);
    assert_eq!(w.item(other).unwrap().facts(), &facts());
    assert_eq!(w.inventory_count(a, &form(0x100)).unwrap(), 12);
    assert_eq!(w.inventory_count(b, &form(0x100)).unwrap(), 5);
    w.remove_item_quantity(other, quantity(2)).unwrap();
    assert_eq!(w.inventory_count(b, &form(0x100)).unwrap(), 3);
    w.remove_item_quantity(other, quantity(3)).unwrap();
    assert!(w.item(other).is_err());
    assert_eq!(w.inventory_count(b, &form(0x100)).unwrap(), 0);
    let next = w.add_item(b, facts(), quantity(1)).unwrap();
    assert!(next > other);
    assert_eq!(
        w.inventory_count_trace(a, &form(0x100))
            .unwrap()
            .contributions,
        vec![(id, 12)]
    );
}
#[test]
fn equal_unknown_or_different_nan_stacks_never_merge() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let a = w
        .add_item(owner, Facts::unknown(form(0x100)), quantity(u32::MAX))
        .unwrap();
    let b = w
        .add_item(owner, Facts::unknown(form(0x100)), quantity(u32::MAX))
        .unwrap();
    let x = w.add_item(owner, facts(), quantity(1)).unwrap();
    let mut different = facts();
    different.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let y = w.add_item(owner, different, quantity(1)).unwrap();
    assert_ne!(a, b);
    assert_ne!(x, y);
    assert_eq!(w.inventory_items(owner).unwrap().count(), 4);
    assert_eq!(
        w.inventory_count(owner, &form(0x100)).unwrap(),
        2 * u64::from(u32::MAX) + 2
    );
    assert_ne!(w.item(x).unwrap().facts(), w.item(y).unwrap().facts());
}
#[test]
fn failed_mutations_leave_items_indices_revision_and_allocators_unchanged() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    let uninitialized = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let id = w.add_item(owner, facts(), quantity(5)).unwrap();
    let before = w.snapshot();
    assert!(w.split_item(id, quantity(5)).is_err());
    assert!(w.split_item(id, quantity(6)).is_err());
    assert!(w.remove_item_quantity(id, quantity(6)).is_err());
    assert!(w.transfer_item(id, uninitialized).is_err());
    assert!(
        w.transfer_item(ItemId(999.try_into().unwrap()), owner)
            .is_err()
    );
    let mut invalid = facts();
    invalid.base.origin_plugin = "FalloutNV.esm".into();
    assert!(w.replace_item_facts(id, invalid).is_err());
    assert_eq!(before, w.snapshot());
    assert_eq!(w.inventory_count(owner, &form(0x100)).unwrap(), 5);
}
#[test]
fn fact_replacement_updates_count_index_without_reidentifying_the_item() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let id = w.add_item(owner, facts(), quantity(3)).unwrap();
    let mut new = facts();
    new.base = form(0x200);
    new.condition = Some(Condition::Float32 { bits: 0x8000_0000 });
    w.replace_item_facts(id, new.clone()).unwrap();
    assert_eq!(w.item(id).unwrap().facts(), &new);
    assert_eq!(w.inventory_count(owner, &form(0x100)).unwrap(), 0);
    assert_eq!(w.inventory_count(owner, &form(0x200)).unwrap(), 3);
    let before = w.snapshot();
    w.transfer_item(id, owner).unwrap();
    assert_eq!(before, w.snapshot());
}
#[test]
fn live_ownership_and_script_links_must_exist_and_prevent_script_removal() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let h = w
        .create_instance(
            &definition(&c),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let mut f = facts();
    f.script_instance = Some(w.instance(h).unwrap().id());
    f.ownership = Some(Ownership::Live { reference: owner });
    let id = w.add_item(owner, f.clone(), quantity(1)).unwrap();
    let before = w.snapshot();
    assert!(w.remove_instance(h).is_err());
    assert_eq!(before, w.snapshot());
    f.ownership = Some(Ownership::Live {
        reference: ReferenceId(999.try_into().unwrap()),
    });
    assert!(w.replace_item_facts(id, f.clone()).is_err());
    f.ownership = None;
    f.script_instance = Some(InstanceId(999.try_into().unwrap()));
    assert!(w.replace_item_facts(id, f).is_err());
    assert_eq!(before, w.snapshot());
    w.replace_item_facts(id, facts()).unwrap();
    w.remove_instance(h).unwrap();
}
#[test]
fn capacities_and_revision_or_identity_exhaustion_fail_atomically() {
    let (_dir, c) = fixture();
    let mut w = World::new(
        &c,
        Limits {
            max_inventory_banks: 1,
            max_item_instances: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let owner = w.register_reference(None).unwrap();
    let other = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    assert!(w.initialize_inventory(other).is_err());
    let id = w.add_item(owner, facts(), quantity(2)).unwrap();
    let before = w.snapshot();
    assert!(w.add_item(owner, facts(), quantity(1)).is_err());
    assert!(w.split_item(id, quantity(1)).is_err());
    assert_eq!(before, w.snapshot());
    let mut full = before.clone();
    full.state_revision = u64::MAX;
    let mut w = World::restore(&c, full, Limits::default()).unwrap();
    let before = w.snapshot();
    assert!(w.remove_item_quantity(id, quantity(1)).is_err());
    assert!(w.replace_item_facts(id, facts()).is_err());
    assert!(w.split_item(id, quantity(1)).is_err());
    assert_eq!(before, w.snapshot());
    let mut full = before;
    full.state_revision = 100;
    full.next_item = u64::MAX;
    let mut w = World::restore(&c, full, Limits::default()).unwrap();
    let before = w.snapshot();
    assert!(w.add_item(owner, facts(), quantity(1)).is_err());
    assert!(w.split_item(id, quantity(1)).is_err());
    assert_eq!(before, w.snapshot());
}
#[test]
fn item_extra_budgets_check_replacement_and_split_before_mutation() {
    let (_dir, c) = fixture();
    let mut w = World::new(
        &c,
        Limits {
            max_item_links: 6,
            max_total_item_links: 6,
            max_item_bytes: 3,
            max_total_item_bytes: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let id = w.add_item(owner, facts(), quantity(2)).unwrap();
    let before = w.snapshot();
    assert!(w.split_item(id, quantity(1)).is_err());
    let mut f = facts();
    f.extra_fields[0].bytes.push(9);
    assert!(w.replace_item_facts(id, f).is_err());
    assert_eq!(before, w.snapshot());
    w.replace_item_facts(id, Facts::unknown(form(0x100)))
        .unwrap();
    assert!(w.split_item(id, quantity(1)).is_ok());
}
#[test]
fn complete_item_state_round_trips_and_indices_rebuild_after_restore() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let a = w.register_reference(None).unwrap();
    let b = w.register_reference(None).unwrap();
    w.initialize_inventory(a).unwrap();
    w.initialize_inventory(b).unwrap();
    let id = w.add_item(a, facts(), quantity(3)).unwrap();
    let before = w.snapshot();
    let bytes = before.encode(usize::MAX).unwrap();
    let decoded = Snapshot::decode(&bytes, Limits::default()).unwrap();
    let mut restored = World::restore(&c, decoded, Limits::default()).unwrap();
    assert_eq!(restored.snapshot(), before);
    assert_eq!(restored.inventory_count(a, &form(0x100)).unwrap(), 3);
    assert_eq!(restored.inventory_count(b, &form(0x100)).unwrap(), 0);
    assert_eq!(restored.item(id).unwrap().facts(), &facts());
    let later = restored.add_item(a, facts(), quantity(1)).unwrap();
    assert!(later > id);
}
#[test]
fn invalid_saved_item_links_duplicates_quantities_and_budgets_reject_transactionally() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    w.add_item(owner, facts(), quantity(1)).unwrap();
    let before = w.snapshot();
    let mut cases = Vec::new();
    let valid = serde_json::to_value(&before).unwrap();
    for path in ["id", "owner"] {
        let mut v = valid.clone();
        v["inventory_banks"][0]["items"][0][path] = 999.into();
        cases.push(v);
    }
    let mut v = valid.clone();
    v["inventory_banks"][0]["items"][0]["count"] = 0.into();
    cases.push(v);
    let mut v = valid.clone();
    let duplicate = v["inventory_banks"][0]["items"][0].clone();
    v["inventory_banks"][0]["items"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    cases.push(v);
    let mut v = valid.clone();
    let duplicate = v["inventory_banks"][0].clone();
    v["inventory_banks"].as_array_mut().unwrap().push(duplicate);
    cases.push(v);
    let mut v = valid.clone();
    v["inventory_banks"][0]["items"][0]["facts"]["script_instance"] = 999.into();
    cases.push(v);
    let mut v = valid.clone();
    v["inventory_banks"][0]["items"][0]["facts"]["unknown"] = true.into();
    cases.push(v);
    let mut v = valid.clone();
    v["inventory_banks"][0]["items"][0]["facts"]["equipped_slots"] = serde_json::json!([1, 1]);
    cases.push(v);
    let mut v = valid;
    v["next_item"] = 1.into();
    cases.push(v);
    for value in cases {
        let decoded = Snapshot::decode(&serde_json::to_vec(&value).unwrap(), Limits::default());
        if let Ok(snapshot) = decoded {
            assert!(w.replace_from_snapshot(snapshot).is_err());
        }
        assert_eq!(before, w.snapshot());
    }
    assert!(
        Snapshot::decode(
            &before.encode(usize::MAX).unwrap(),
            Limits {
                max_item_instances: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn schema_two_migration_preserves_all_old_state_and_leaves_inventory_unknown() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    let expected = w.snapshot();
    let mut old = serde_json::to_value(&expected).unwrap();
    let object = old.as_object_mut().unwrap();
    object.remove("next_item");
    object.remove("inventory_banks");
    object.remove("reference_states");
    object.insert("schema_version".into(), 2.into());
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(Snapshot::decode(&bytes, Limits::default()).is_err());
    let migrated = Snapshot::migrate_v2(&bytes, Limits::default()).unwrap();
    assert_eq!(migrated, expected);
    w.replace_from_snapshot(migrated).unwrap();
    assert!(w.inventory_count(owner, &form(0x100)).is_err());
    old["unknown"] = true.into();
    assert!(Snapshot::migrate_v2(&serde_json::to_vec(&old).unwrap(), Limits::default()).is_err());
}
#[test]
fn native_repository_preserves_items_and_rejects_allocator_rewind() {
    let (dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let id = w.add_item(owner, facts(), quantity(2)).unwrap();
    let repository =
        Repository::create(&dir.path().join("native-items"), &[], w.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&w)).unwrap();
    let (loaded, _) = repository
        .load(&c, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(loaded.snapshot(), w.snapshot());
    assert_eq!(loaded.inventory_count(owner, &form(0x100)).unwrap(), 2);
    let before = std::fs::read(dir.path().join("native-items/current.frsv")).unwrap();
    let mut stale = w.snapshot();
    stale.state_revision += 1;
    stale.next_item = 1;
    stale.inventory_banks[0].items.clear();
    let rewind = World::restore(&c, stale, Limits::default()).unwrap();
    assert!(repository.commit(&Captured::at_boundary(&rewind)).is_err());
    assert_eq!(
        before,
        std::fs::read(dir.path().join("native-items/current.frsv")).unwrap()
    );
    assert_eq!(loaded.item(id).unwrap().facts(), &facts());
}

#[test]
fn indexed_counts_match_canonical_items_across_long_mutation_and_restore_sequences() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owners = (0..8)
        .map(|_| w.register_reference(None).unwrap())
        .collect::<Vec<_>>();
    for &owner in &owners {
        w.initialize_inventory(owner).unwrap();
    }
    let mut seed = 0x33_9876_1234_u64;
    for step in 0..2_000 {
        // Deterministic engineering inputs, unrelated to the game's loot RNG.
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let owner = owners[(seed as usize >> 8) % owners.len()];
        let base = form(0x500 + (seed as u32 % 5));
        let snapshot = w.snapshot();
        let ids = snapshot
            .inventory_banks
            .iter()
            .flat_map(|b| b.items.iter().map(|i| i.id()))
            .collect::<Vec<_>>();
        if ids.is_empty() || seed.is_multiple_of(5) {
            w.add_item(
                owner,
                Facts::unknown(base),
                quantity((seed as u32 % 31) + 1),
            )
            .unwrap();
        } else {
            let id = ids[(seed as usize >> 16) % ids.len()];
            match seed % 5 {
                1 => w.transfer_item(id, owner).unwrap(),
                2 if w.item(id).unwrap().count() > 1 => {
                    w.split_item(id, quantity(1)).unwrap();
                }
                3 => w.remove_item_quantity(id, quantity(1)).unwrap(),
                _ => w.replace_item_facts(id, Facts::unknown(base)).unwrap(),
            }
        }
        if step % 97 == 0 {
            let state = w.snapshot();
            w = World::restore(&c, state, Limits::default()).unwrap();
        }
        let state = w.snapshot();
        for bank in &state.inventory_banks {
            for local in 0x500..0x505 {
                let base = form(local);
                let expected = bank
                    .items
                    .iter()
                    .filter(|i| i.facts().base == base)
                    .map(|i| u64::from(i.count()))
                    .sum::<u64>();
                assert_eq!(
                    w.inventory_count(bank.owner, &base).unwrap(),
                    expected,
                    "step {step}"
                );
            }
        }
    }
}

#[test]
fn transient_item_handles_expire_after_removal_or_restoration_and_traces_are_bounded() {
    let (_dir, c) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let id = w.add_item(owner, facts(), quantity(1)).unwrap();
    let handle = w.item_handle(id).unwrap();
    assert_eq!(w.item_id(handle).unwrap(), id);
    assert!(
        w.inventory_count_trace_bounded(owner, &form(0x100), 0)
            .is_err()
    );
    assert_eq!(
        w.inventory_count_trace_bounded(owner, &form(0x100), 1)
            .unwrap()
            .result,
        1
    );
    let snapshot = w.snapshot();
    w.remove_item_quantity(id, quantity(1)).unwrap();
    assert!(w.item_by_handle(handle).is_err());
    w.replace_from_snapshot(snapshot).unwrap();
    assert!(w.item_by_handle(handle).is_err());
    assert_eq!(
        w.item_by_handle(w.item_handle(id).unwrap())
            .unwrap()
            .facts(),
        &facts()
    );
}

fn transfer_world(
    catalogue: &fallout_data::loaded_scripts::Catalogue,
) -> (World<'_>, [ReferenceId; 3], [ItemId; 3]) {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x22; 16]).unwrap(),
    )
    .unwrap();
    let owners = [
        world.register_reference(None).unwrap(),
        world.register_reference(None).unwrap(),
        world.register_reference(None).unwrap(),
    ];
    world.initialize_inventory(owners[0]).unwrap();
    world.initialize_inventory(owners[1]).unwrap();
    let context = Context {
        calling_reference: Some(owners[0]),
        containing_reference: Some(owners[1]),
        target: Some(ReferenceValue::Live { id: owners[2] }),
        arguments: vec![ReferenceValue::Null],
    };
    let stage = world
        .stage_instance_initialization(
            &definition(catalogue),
            &Owner::Fragment {
                activation: 7.try_into().unwrap(),
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
                        value: ReferenceValue::Live { id: owners[0] },
                    },
                ),
            ],
            initialization::Limits::default(),
        )
        .unwrap();
    let (_, handle) = world.commit_instance_initialization(stage).unwrap();
    world
        .enqueue(handle, Trigger::ObjectEvent { mask: 0x8000_0001 }, context)
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 3,
            menu_nanoseconds: 5,
            real_nanoseconds: 7,
        })
        .unwrap();
    let mut first = facts();
    first.script_instance = Some(world.instance(handle).unwrap().id());
    let id1 = world
        .add_item(owners[0], first.clone(), quantity(17))
        .unwrap();
    let mut second = first.clone();
    second.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let id2 = world.add_item(owners[0], second, quantity(3)).unwrap();
    let mut third = first;
    third.base = form(0x200);
    third.condition = Some(Condition::Float32 { bits: 0x8000_0000 });
    let id3 = world.add_item(owners[1], third, quantity(5)).unwrap();
    (world, owners, [id1, id2, id3])
}
fn assert_transfer_counts(world: &World<'_>) {
    let snapshot = world.snapshot();
    for bank in &snapshot.inventory_banks {
        for base in [form(0x100), form(0x200)] {
            let rows = bank
                .items
                .iter()
                .filter(|item| item.facts().base == base)
                .map(|item| (item.id(), item.count()))
                .collect::<Vec<_>>();
            let total = rows.iter().map(|(_, count)| u64::from(*count)).sum::<u64>();
            let trace = world.inventory_count_trace(bank.owner, &base).unwrap();
            assert_eq!(trace.result, total);
            assert_eq!(trace.contributions, rows);
            assert_eq!(world.inventory_count(bank.owner, &base).unwrap(), total);
        }
    }
}

#[test]
fn explicit_multi_lot_transfer_publishes_once_with_exact_facts_and_counts_without_source_item_policy()
 {
    let (_directory, catalogue) = fixture();
    let (mut world, [a, b, absent], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let original = ids.map(|id| world.item(id).unwrap().clone());
    // The stored ACTI/deleted base keys would not constitute new source-item
    // admission. Ownership changes retain already-admitted canonical facts.
    let changes = [(ids[0], b), (ids[1], b), (ids[2], a)];
    let stage = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap();
    assert_eq!(stage.usage().rows, 3);
    assert_eq!(stage.usage().moved_rows, 3);
    assert_eq!(stage.usage().links, 21);
    assert_eq!(world.snapshot(), before);
    let receipt = world.commit_inventory_transfers(stage).unwrap();
    assert_eq!(receipt.before_revision(), before.state_revision);
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    assert_eq!(receipt.campaign(), world.campaign());
    assert_eq!(
        receipt.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    for (index, &(id, target)) in changes.iter().enumerate() {
        assert_eq!(world.item(id).unwrap().owner(), target);
        assert_eq!(world.item(id).unwrap().facts(), original[index].facts());
        assert_eq!(world.item(id).unwrap().count(), original[index].count());
        assert_eq!(receipt.changes()[index].original(), &original[index]);
        assert_eq!(receipt.changes()[index].target(), target);
    }
    assert_eq!(world.inventory_count(a, &form(0x100)).unwrap(), 0);
    assert_eq!(world.inventory_count(b, &form(0x100)).unwrap(), 20);
    assert_eq!(world.inventory_count(a, &form(0x200)).unwrap(), 5);
    assert_eq!(world.inventory_count(b, &form(0x200)).unwrap(), 0);
    assert!(world.inventory_items(absent).is_err());
    let after = world.snapshot();
    assert_eq!(before.next_item, after.next_item);
    assert_eq!(before.instances, after.instances);
    assert_eq!(before.pending_events, after.pending_events);
    assert_eq!(before.clocks, after.clocks);
    assert_transfer_counts(&world);
}

#[test]
fn invalid_last_repeated_missing_and_uninitialized_batch_rows_leave_all_canonical_state_exact() {
    let (_directory, catalogue) = fixture();
    let (world, [a, b, absent], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    for changes in [
        vec![],
        vec![(ids[0], b), (ids[1], ReferenceId(999.try_into().unwrap()))],
        vec![(ids[0], b), (ids[1], absent)],
        vec![(ids[0], b), (ids[0], a)],
        vec![(ids[0], b), (ItemId(999.try_into().unwrap()), a)],
    ] {
        assert!(
            world
                .stage_inventory_transfers(&changes, TransferLimits::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
        assert_transfer_counts(&world);
    }
    let stage = world
        .stage_inventory_transfers(&[(ids[0], b), (ids[2], a)], TransferLimits::default())
        .unwrap();
    drop(stage);
    assert_eq!(world.snapshot(), before);
    assert_transfer_counts(&world);
}

#[test]
fn batch_copy_row_and_link_limits_accept_exact_and_refuse_one_under_before_effects() {
    let (_directory, catalogue) = fixture();
    let (world, [a, b, _], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let changes = [(ids[0], b), (ids[1], b), (ids[2], a)];
    let usage = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap()
        .usage();
    let exact = TransferLimits {
        max_rows: 3,
        max_links: 21,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(
        world
            .stage_inventory_transfers(&changes, exact)
            .unwrap()
            .usage(),
        usage
    );
    for (limits, label) in [
        (
            TransferLimits {
                max_rows: 2,
                ..exact
            },
            "inventory transfer rows",
        ),
        (
            TransferLimits {
                max_links: 20,
                ..exact
            },
            "inventory transfer links",
        ),
        (
            TransferLimits {
                max_copied_bytes: usage.copied_bytes - 1,
                ..exact
            },
            "inventory transfer copied bytes",
        ),
        (
            TransferLimits {
                max_copied_bytes: 0,
                ..exact
            },
            "inventory transfer copied bytes",
        ),
    ] {
        assert!(
            matches!(world.stage_inventory_transfers(&changes,limits),Err(fallout_runtime::Error::Capacity(actual)) if actual==label)
        );
        assert_eq!(world.snapshot(), before);
    }
    assert_transfer_counts(&world);
}

#[test]
fn same_target_rows_keep_existing_noop_semantics_even_at_exhausted_revision() {
    let (_directory, catalogue) = fixture();
    let (world, [a, b, _], ids) = transfer_world(&catalogue);
    let mut full = world.snapshot();
    full.state_revision = u64::MAX;
    let mut world = World::restore(&catalogue, full.clone(), Limits::default()).unwrap();
    let stage = world
        .stage_inventory_transfers(&[(ids[0], a), (ids[2], b)], TransferLimits::default())
        .unwrap();
    assert_eq!(stage.usage().moved_rows, 0);
    assert!(stage.count_changes().is_empty());
    let receipt = world.commit_inventory_transfers(stage).unwrap();
    assert_eq!(receipt.before_revision(), u64::MAX);
    assert_eq!(receipt.after_revision(), u64::MAX);
    assert_eq!(world.snapshot(), full);
    let stage = world
        .stage_inventory_transfers(&[(ids[0], b), (ids[2], a)], TransferLimits::default())
        .unwrap();
    assert!(matches!(
        world.commit_inventory_transfers(stage),
        Err(fallout_runtime::Error::Capacity("state revisions"))
    ));
    assert_eq!(world.snapshot(), full);
    assert_transfer_counts(&world);
}

#[test]
fn stale_restore_other_world_and_changed_last_lot_refuse_batch_without_partial_transfers() {
    let (_directory, catalogue) = fixture();
    let (mut world, [a, b, _], ids) = transfer_world(&catalogue);
    let changes = [(ids[0], b), (ids[2], a)];
    let stage = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap();
    world.remove_item_quantity(ids[2], quantity(1)).unwrap();
    let after = world.snapshot();
    assert!(world.commit_inventory_transfers(stage).is_err());
    assert_eq!(world.snapshot(), after);
    assert_transfer_counts(&world);
    let stage = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap();
    world.replace_from_snapshot(after.clone()).unwrap();
    assert!(matches!(
        world.commit_inventory_transfers(stage),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), after);
    let stage = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap();
    let mut foreign = after.clone();
    foreign.campaign = CampaignId::from_bytes([0x23; 16]).unwrap();
    let mut other = World::restore(&catalogue, foreign.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.commit_inventory_transfers(stage),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), foreign);
    assert_eq!(world.snapshot(), after);
    let first = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap();
    let loser = world
        .stage_inventory_transfers(&changes, TransferLimits::default())
        .unwrap();
    world.commit_inventory_transfers(first).unwrap();
    let winner = world.snapshot();
    assert!(world.commit_inventory_transfers(loser).is_err());
    assert_eq!(world.snapshot(), winner);
    let stage = world
        .stage_inventory_transfers(&[(ids[0], a), (ids[2], b)], TransferLimits::default())
        .unwrap();
    let mut facts = world.item(ids[2]).unwrap().facts().clone();
    facts.condition = None;
    world.replace_item_facts(ids[2], facts).unwrap();
    let changed = world.snapshot();
    assert!(world.commit_inventory_transfers(stage).is_err());
    assert_eq!(world.snapshot(), changed);
    let stage = world
        .stage_inventory_transfers(&[(ids[0], a), (ids[2], b)], TransferLimits::default())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 2,
            game_nanoseconds: 3,
            menu_nanoseconds: 5,
            real_nanoseconds: 7,
        })
        .unwrap();
    let unrelated = world.snapshot();
    assert!(world.commit_inventory_transfers(stage).is_err());
    assert_eq!(world.snapshot(), unrelated);
    assert_transfer_counts(&world);
}

#[test]
fn opposing_same_base_lot_moves_and_mixed_noops_conserve_totals_independent_of_row_order() {
    let (_directory, catalogue) = fixture();
    let (world, [a, b, _], ids) = transfer_world(&catalogue);
    let mut before = world.snapshot();
    // Existing canonical syntax permits explicit facts replacement; no game
    // rule or source admission is introduced by choosing a shared base here.
    let mut working = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    let mut third = working.item(ids[2]).unwrap().facts().clone();
    third.base = form(0x100);
    working.replace_item_facts(ids[2], third).unwrap();
    before = working.snapshot();
    let rows = [(ids[0], b), (ids[2], a), (ids[1], a)];
    let stage = working
        .stage_inventory_transfers(&rows, TransferLimits::default())
        .unwrap();
    assert_eq!(stage.usage().moved_rows, 2);
    working.commit_inventory_transfers(stage).unwrap();
    let expected = working.snapshot();
    let mut opposite = World::restore(&catalogue, before, Limits::default()).unwrap();
    let stage = opposite
        .stage_inventory_transfers(&[rows[2], rows[1], rows[0]], TransferLimits::default())
        .unwrap();
    opposite.commit_inventory_transfers(stage).unwrap();
    assert_eq!(opposite.snapshot(), expected);
    assert_eq!(working.inventory_count(a, &form(0x100)).unwrap(), 8);
    assert_eq!(working.inventory_count(b, &form(0x100)).unwrap(), 17);
    assert_transfer_counts(&working);
    assert_transfer_counts(&opposite);
}

#[test]
fn native_multi_lot_batch_cold_restores_complete_before_and_after_boundaries() {
    use std::{fs, process::Command};
    let temporary = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_INVENTORY_TRANSFER_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temporary.path());
    fs::create_dir_all(root).unwrap();
    write_fixture(root, false);
    let catalogue = load(root, &["FalloutNV.esm"]);
    let (mut world, [a, b, _], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = fallout_runtime::save::SaveWorker::start(repository.clone(), 2).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let stage = world
        .stage_inventory_transfers(
            &[(ids[0], b), (ids[1], b), (ids[2], a)],
            TransferLimits::default(),
        )
        .unwrap();
    let receipt = world.commit_inventory_transfers(stage).unwrap();
    let after = world.snapshot();
    let second = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    worker.finish().unwrap();
    assert_eq!(first.wait().unwrap().metadata.generation, 1);
    assert_eq!(second.wait().unwrap().metadata.generation, 2);
    fs::write(
        root.join("batch.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
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
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(world);
    drop(catalogue);
    for (phase, snapshot, wire) in [("before", &before, &previous), ("after", &after, &current)] {
        let phase_root = root.join(phase);
        fs::create_dir(&phase_root).unwrap();
        let cold_repo =
            Repository::create(&phase_root.join("native"), &[], snapshot.campaign).unwrap();
        fs::write(cold_repo.path().join("current.frsv"), wire).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_inventory_transfer_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_INVENTORY_TRANSFER_COLD_ROOT", root)
            .env("FALLOUT_INVENTORY_TRANSFER_COLD_PHASE", phase)
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
        assert_eq!(
            fs::read(cold_repo.path().join("current.frsv")).unwrap(),
            *wire
        );
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
#[ignore = "fresh before/after inventory consumer invoked by its parent"]
fn cold_inventory_transfer_helper() {
    use std::fs;
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_INVENTORY_TRANSFER_COLD_ROOT").unwrap());
    let phase = std::env::var("FALLOUT_INVENTORY_TRANSFER_COLD_PHASE").unwrap();
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join(&phase).join("native"), &[]).unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join(format!("expected.{phase}.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(world.snapshot(), expected);
    assert_eq!(world.pending_events().len(), 1);
    assert_eq!(
        receipt.metadata.generation,
        if phase == "before" { 1 } else { 2 }
    );
    assert_transfer_counts(&world);
    let observations = [
        ReferenceId(1.try_into().unwrap()),
        ReferenceId(2.try_into().unwrap()),
        ReferenceId(3.try_into().unwrap()),
    ]
    .map(|owner| {
        world
            .inventory_view(
                owner,
                ViewLimits {
                    max_items: 3,
                    max_links: 21,
                    max_extra_bytes: 9,
                },
            )
            .unwrap()
    });
    assert!(observations[2].items().is_none());
    fs::write(
        root.join(&phase).join("cold.restored.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.views.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join(&phase).join("cold.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
