mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, InstanceId, Owner, ReferenceId, ReferenceValue, Value},
    inventory::{
        Ammo, Condition, Facts, ItemId, OpaqueExtra, Ownership, Page, PageLimits, PageRequest,
        RemovalLimits, TransferLimits, ViewLimits, ViewUsage,
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

fn paging_world(
    catalogue: &fallout_data::loaded_scripts::Catalogue,
) -> (World<'_>, [ReferenceId; 3], [ItemId; 3]) {
    let (mut world, owners, ids) = transfer_world(catalogue);
    world.transfer_item(ids[2], owners[0]).unwrap();
    let mut snapshot = world.snapshot();
    snapshot.references[0].authored = Some(form(0x100));
    (
        World::restore(catalogue, snapshot, Limits::default()).unwrap(),
        owners,
        ids,
    )
}
fn collect_pages(world: &World<'_>, owner: ReferenceId, rows: usize) -> Vec<Page> {
    let mut after = None;
    let mut pages = Vec::new();
    loop {
        let page = world
            .inventory_page(
                PageRequest {
                    owner,
                    after: after.as_ref(),
                    rows,
                },
                PageLimits::default(),
            )
            .unwrap();
        after = page.next_cursor().cloned();
        pages.push(page);
        if after.is_none() {
            break;
        }
        assert!(pages.len() < 64);
    }
    pages
}
fn literal_paging_items() -> serde_json::Value {
    let mut first = facts();
    first.script_instance = Some(InstanceId(1.try_into().unwrap()));
    let mut second = first.clone();
    second.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let mut third = first.clone();
    third.base = form(0x200);
    third.condition = Some(Condition::Float32 { bits: 0x8000_0000 });
    serde_json::json!([
        {"id":1,"owner":1,"count":17,"facts":first},
        {"id":2,"owner":1,"count":3,"facts":second},
        {"id":3,"owner":1,"count":5,"facts":third}])
}

#[test]
fn consecutive_inventory_pages_preserve_literal_ids_quantities_raw_bits_and_opaque_facts() {
    let (_dir, catalogue) = fixture();
    let (world, [owner, _, _], _) = paging_world(&catalogue);
    let before = world.snapshot();
    let pages = collect_pages(&world, owner, 1);
    assert_eq!(pages.len(), 3);
    let items = pages
        .iter()
        .flat_map(|page| page.items().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(serde_json::to_value(items).unwrap(), literal_paging_items());
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.campaign(), CampaignId::from_bytes([34; 16]).unwrap());
        assert_eq!(page.catalogue_fingerprint(), world.catalogue_fingerprint());
        assert_eq!(page.revision(), 12);
        assert_eq!(page.boundary(), before.clocks);
        assert_eq!(page.owner(), owner);
        assert_eq!(page.authored(), Some(&form(0x100)));
        assert_eq!(
            page.start_after(),
            if index == 0 {
                None
            } else {
                Some(ItemId((index as u64).try_into().unwrap()))
            }
        );
        assert_eq!(page.usage().visited, 1);
        assert_eq!(page.usage().returned, 1);
        assert_eq!(page.usage().links, 7);
        assert_eq!(page.usage().extra_bytes, 3);
        assert_eq!(page.is_complete(), index == 2);
        assert_eq!(page.next_cursor().is_none(), index == 2);
    }
    assert_eq!(world.snapshot(), before);
    assert_transfer_counts(&world);
}

#[test]
fn pages_distinguish_unknown_uninitialized_explicit_empty_and_after_last() {
    let (_dir, catalogue) = fixture();
    let (world, [owner, empty, absent], ids) = paging_world(&catalogue);
    let before = world.snapshot();
    let uninitialized = world
        .inventory_page(
            PageRequest {
                owner: absent,
                after: None,
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap();
    assert!(uninitialized.items().is_none());
    assert!(uninitialized.is_complete());
    let explicit = world
        .inventory_page(
            PageRequest {
                owner: empty,
                after: None,
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap();
    assert_eq!(explicit.items(), Some([].as_slice()));
    assert!(explicit.is_complete());
    assert_eq!(explicit.usage().visited, 0);
    assert_eq!(explicit.start_after(), None);
    let pages = collect_pages(&world, owner, 2);
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].items().unwrap().len(), 2);
    assert_eq!(pages[1].items().unwrap().len(), 1);
    let tail = world
        .inventory_page(
            PageRequest {
                owner,
                after: Some(pages[1].cursor()),
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap();
    assert_eq!(tail.items(), Some([].as_slice()));
    assert_eq!(tail.start_after(), Some(ids[2]));
    assert_eq!(tail.usage().visited, 0);
    assert!(tail.next_cursor().is_none());
    assert!(
        world
            .inventory_page(
                PageRequest {
                    owner: ReferenceId(999.try_into().unwrap()),
                    after: None,
                    rows: 1
                },
                PageLimits::default()
            )
            .is_err()
    );
    assert!(
        world
            .inventory_page(
                PageRequest {
                    owner,
                    after: None,
                    rows: 0
                },
                PageLimits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn page_required_slice_exact_limits_and_one_under_refuse_whole_observation() {
    let (_dir, catalogue) = fixture();
    let (world, [owner, _, _], _) = paging_world(&catalogue);
    let before = world.snapshot();
    let request = PageRequest {
        owner,
        after: None,
        rows: 2,
    };
    let usage = world
        .inventory_page(request, PageLimits::default())
        .unwrap()
        .usage();
    let exact = PageLimits {
        max_visited: 2,
        max_rows: 2,
        max_links: 14,
        max_extra_bytes: 6,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(world.inventory_page(request, exact).unwrap().usage(), usage);
    for (limits, label) in [
        (
            PageLimits {
                max_visited: 1,
                ..exact
            },
            "inventory page visited",
        ),
        (
            PageLimits {
                max_rows: 1,
                ..exact
            },
            "inventory page rows",
        ),
        (
            PageLimits {
                max_links: 13,
                ..exact
            },
            "inventory page links",
        ),
        (
            PageLimits {
                max_extra_bytes: 5,
                ..exact
            },
            "inventory page extra bytes",
        ),
        (
            PageLimits {
                max_copied_bytes: usage.copied_bytes - 1,
                ..exact
            },
            "inventory page copied bytes",
        ),
        (
            PageLimits {
                max_copied_bytes: 0,
                ..exact
            },
            "inventory page copied bytes",
        ),
    ] {
        assert!(
            matches!(world.inventory_page(request,limits),Err(fallout_runtime::Error::Capacity(actual)) if actual==label)
        );
        assert_eq!(world.snapshot(), before);
    }
    assert_eq!(usage.visited, 2);
    assert_eq!(usage.returned, 2);
    assert_transfer_counts(&world);
}

#[test]
fn bounded_page_does_not_precharge_or_clone_large_unselected_suffix_and_resumes_at_id_gap() {
    let (_dir, catalogue) = fixture();
    let (mut world, [owner, other, _], ids) = paging_world(&catalogue);
    let initial = world
        .inventory_page(
            PageRequest {
                owner,
                after: None,
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap()
        .usage();
    let mut large = world.item(ids[2]).unwrap().facts().clone();
    large.extra_fields[0].bytes = vec![0xff; 16_384];
    world.replace_item_facts(ids[2], large).unwrap();
    world.transfer_item(ids[1], other).unwrap();
    let before = world.snapshot();
    let limits = PageLimits {
        max_visited: 1,
        max_rows: 1,
        max_links: 7,
        max_extra_bytes: 3,
        max_copied_bytes: initial.copied_bytes,
    };
    let first = world
        .inventory_page(
            PageRequest {
                owner,
                after: None,
                rows: 1,
            },
            limits,
        )
        .unwrap();
    assert_eq!(first.items().unwrap()[0].id(), ids[0]);
    assert_eq!(first.usage(), initial);
    assert!(!first.is_complete());
    assert!(matches!(
        world.inventory_page(
            PageRequest {
                owner,
                after: first.next_cursor(),
                rows: 1
            },
            limits
        ),
        Err(fallout_runtime::Error::Capacity(
            "inventory page extra bytes"
        ))
    ));
    let next = world
        .inventory_page(
            PageRequest {
                owner,
                after: first.next_cursor(),
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap();
    assert_eq!(next.items().unwrap()[0].id(), ids[2]);
    assert_eq!(
        next.items().unwrap()[0].facts().extra_fields[0].bytes,
        vec![0xff; 16_384]
    );
    assert!(next.is_complete());
    assert_eq!(world.snapshot(), before);
}

#[test]
fn mutated_restored_cross_world_and_changed_owner_cursors_refuse_before_zero_copy_budget() {
    let (_dir, catalogue) = fixture();
    let (mut world, [owner, other, _], ids) = paging_world(&catalogue);
    let zero = PageLimits {
        max_copied_bytes: 0,
        ..PageLimits::default()
    };
    let cursor = world
        .inventory_page(
            PageRequest {
                owner,
                after: None,
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap()
        .cursor()
        .clone();
    let before = world.snapshot();
    assert!(matches!(
        world.inventory_page(
            PageRequest {
                owner: other,
                after: Some(&cursor),
                rows: 1
            },
            zero
        ),
        Err(fallout_runtime::Error::Invalid(_))
    ));
    let restored = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    assert!(matches!(
        restored.inventory_page(
            PageRequest {
                owner,
                after: Some(&cursor),
                rows: 1
            },
            zero
        ),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    let mut foreign = before.clone();
    foreign.campaign = CampaignId::from_bytes([35; 16]).unwrap();
    let foreign = World::restore(&catalogue, foreign, Limits::default()).unwrap();
    assert!(matches!(
        foreign.inventory_page(
            PageRequest {
                owner,
                after: Some(&cursor),
                rows: 1
            },
            zero
        ),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    world.transfer_item(ids[2], other).unwrap();
    let after = world.snapshot();
    assert!(matches!(
        world.inventory_page(
            PageRequest {
                owner,
                after: Some(&cursor),
                rows: 1
            },
            zero
        ),
        Err(fallout_runtime::Error::Invalid(_))
    ));
    assert_eq!(world.snapshot(), after);
    let cursor = world
        .inventory_page(
            PageRequest {
                owner,
                after: None,
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap()
        .cursor()
        .clone();
    let mut facts = world.item(ids[1]).unwrap().facts().clone();
    facts.condition = None;
    world.replace_item_facts(ids[1], facts).unwrap();
    let after = world.snapshot();
    assert!(matches!(
        world.inventory_page(
            PageRequest {
                owner,
                after: Some(&cursor),
                rows: 1
            },
            zero
        ),
        Err(fallout_runtime::Error::Invalid(_))
    ));
    assert_eq!(world.snapshot(), after);
    let cursor = world
        .inventory_page(
            PageRequest {
                owner,
                after: None,
                rows: 1,
            },
            PageLimits::default(),
        )
        .unwrap()
        .cursor()
        .clone();
    world.replace_from_snapshot(after.clone()).unwrap();
    assert!(matches!(
        world.inventory_page(
            PageRequest {
                owner,
                after: Some(&cursor),
                rows: 1
            },
            zero
        ),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), after);
    assert_transfer_counts(&world);
}

#[test]
fn native_inventory_page_consumer_cold_restores_same_literal_complete_bank_without_writes() {
    use std::{fs, process::Command};
    let temporary = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_INVENTORY_PAGE_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temporary.path());
    fs::create_dir_all(root).unwrap();
    write_fixture(root, false);
    let catalogue = load(root, &["FalloutNV.esm"]);
    let (world, owners, _) = paging_world(&catalogue);
    let expected = world.snapshot();
    let pages = owners.map(|owner| collect_pages(&world, owner, 1));
    assert_eq!(
        serde_json::to_value(
            pages[0]
                .iter()
                .flat_map(|page| page.items().unwrap())
                .collect::<Vec<_>>()
        )
        .unwrap(),
        literal_paging_items()
    );
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let receipt = repository.commit(&Captured::at_boundary(&world)).unwrap();
    fs::write(
        root.join("expected.json"),
        expected.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("expected.pages.json"),
        serde_json::to_vec_pretty(&pages).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("write.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let native = fs::read(repository.path().join("current.frsv")).unwrap();
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(world);
    drop(catalogue);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_inventory_page_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_INVENTORY_PAGE_COLD_ROOT", root)
        .output()
        .unwrap();
    fs::write(root.join("cold.stdout.txt"), &child.stdout).unwrap();
    fs::write(root.join("cold.stderr.txt"), &child.stderr).unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        native
    );
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source);
    assert!(!repository.path().join("previous.frsv").exists());
}

#[test]
#[ignore = "fresh inventory page consumer invoked by parent"]
fn cold_inventory_page_helper() {
    use std::fs;
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_INVENTORY_PAGE_COLD_ROOT").unwrap());
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join("expected.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    let pages =
        [1_u64, 2, 3].map(|id| collect_pages(&world, ReferenceId(id.try_into().unwrap()), 1));
    let observed = serde_json::to_value(&pages).unwrap();
    assert_eq!(
        observed,
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(root.join("expected.pages.json")).unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        serde_json::to_value(
            pages[0]
                .iter()
                .flat_map(|page| page.items().unwrap())
                .collect::<Vec<_>>()
        )
        .unwrap(),
        literal_paging_items()
    );
    assert_transfer_counts(&world);
    fs::write(
        root.join("cold.restored.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("cold.pages.json"),
        serde_json::to_vec_pretty(&pages).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("cold.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

fn removal_inputs(
    world: &World<'_>,
    ids: [ItemId; 3],
) -> [(fallout_runtime::inventory::ItemHandle, NonZeroU32); 2] {
    [
        (world.item_handle(ids[0]).unwrap(), quantity(2)),
        (world.item_handle(ids[2]).unwrap(), quantity(5)),
    ]
}
fn expected_removal_state(before: &Snapshot, removed: &[(ItemId, u32)]) -> Snapshot {
    let mut value = serde_json::to_value(before).unwrap();
    value["state_revision"] = (before.state_revision + 1).into();
    for bank in value["inventory_banks"].as_array_mut().unwrap() {
        let items = bank["items"].as_array_mut().unwrap();
        for item in items.iter_mut() {
            if let Some((_, quantity)) = removed.iter().find(|(id, _)| item["id"] == id.0.get()) {
                let count = item["count"].as_u64().unwrap();
                item["count"] = (count - u64::from(*quantity)).into();
            }
        }
        items.retain(|item| item["count"] != 0);
    }
    serde_json::from_value(value).unwrap()
}
#[test]
fn multi_lot_removal_reproduces_sequential_bad_last_gap_and_preserves_batch_state() {
    let (_root, catalogue) = fixture();
    let (mut world, _, ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let mut inputs = removal_inputs(&world, ids);
    inputs[1].1 = quantity(6);
    assert!(
        world
            .stage_inventory_removals(&inputs, RemovalLimits::default())
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    assert_transfer_counts(&world);
    world.remove_item_quantity(ids[0], quantity(2)).unwrap();
    assert!(world.remove_item_quantity(ids[2], quantity(6)).is_err());
    assert_eq!(
        world.snapshot(),
        expected_removal_state(&before, &[(ids[0], 2)])
    );
    if let Some(root) = std::env::var_os("FALLOUT_INVENTORY_REMOVAL_EVIDENCE") {
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
fn explicit_partial_and_complete_removal_publish_once_and_keep_surviving_facts_exact() {
    let (_root, catalogue) = fixture();
    let (mut world, [a, b, unknown], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let original = world.item(ids[0]).unwrap().facts().clone();
    let stage = world
        .stage_inventory_removals(&removal_inputs(&world, ids), RemovalLimits::default())
        .unwrap();
    assert_eq!(world.snapshot(), before);
    assert_eq!(
        stage
            .rows()
            .iter()
            .map(|row| (
                row.id(),
                row.owner(),
                row.before(),
                row.requested(),
                row.remaining(),
                row.links(),
                row.extra_bytes()
            ))
            .collect::<Vec<_>>(),
        [(ids[0], a, 17, 2, 15, 7, 3), (ids[2], b, 5, 5, 0, 7, 3)]
    );
    assert_eq!(stage.rows()[0].base(), &form(0x100));
    assert_eq!(stage.rows()[1].base(), &form(0x200));
    let receipt = world.commit_inventory_removals(stage).unwrap();
    assert_eq!(receipt.before_revision(), 11);
    assert_eq!(receipt.after_revision(), 12);
    assert_eq!(receipt.campaign(), world.campaign());
    assert_eq!(
        receipt.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    assert_eq!(receipt.usage().rows, 2);
    assert_eq!(receipt.usage().removed_lots, 1);
    assert_eq!(receipt.usage().quantity, 7);
    assert_eq!(receipt.usage().released_links, 7);
    assert_eq!(receipt.usage().released_extra_bytes, 3);
    assert_eq!(
        receipt
            .count_changes()
            .iter()
            .map(|row| (row.owner(), row.base().local_id, row.before(), row.after()))
            .collect::<Vec<_>>(),
        [(a, 0x100, 20, 18), (b, 0x200, 5, 0)]
    );
    assert_eq!(
        world.snapshot(),
        expected_removal_state(&before, &[(ids[0], 2), (ids[2], 5)])
    );
    assert_eq!(world.item(ids[0]).unwrap().facts(), &original);
    assert!(world.item(ids[2]).is_err());
    assert_transfer_counts(&world);
    let limits = ViewLimits {
        max_items: 2,
        max_links: 14,
        max_extra_bytes: 6,
    };
    assert_eq!(
        world.inventory_view(b, limits).unwrap().items(),
        Some([].as_slice())
    );
    assert!(
        world
            .inventory_view(unknown, limits)
            .unwrap()
            .items()
            .is_none()
    );
    assert!(world.inventory_count(unknown, &form(0x100)).is_err());
}
#[test]
fn same_owner_removals_aggregate_counts_independent_of_row_order() {
    let (_root, catalogue) = fixture();
    let (mut world, [a, _, _], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let inputs = [
        (world.item_handle(ids[0]).unwrap(), quantity(2)),
        (world.item_handle(ids[1]).unwrap(), quantity(3)),
    ];
    let stage = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    assert_eq!(stage.count_changes().len(), 1);
    world.commit_inventory_removals(stage).unwrap();
    assert_eq!(world.inventory_count(a, &form(0x100)).unwrap(), 15);
    assert_eq!(
        world.snapshot(),
        expected_removal_state(&before, &[(ids[0], 2), (ids[1], 3)])
    );
    let mut reversed = World::restore(&catalogue, before, Limits::default()).unwrap();
    let inputs = [
        (reversed.item_handle(ids[1]).unwrap(), quantity(3)),
        (reversed.item_handle(ids[0]).unwrap(), quantity(2)),
    ];
    let stage = reversed
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    let receipt = reversed.commit_inventory_removals(stage).unwrap();
    assert_eq!(receipt.rows()[0].id(), ids[1]);
    assert_eq!(reversed.snapshot(), world.snapshot());
    assert_transfer_counts(&reversed);
}
#[test]
fn removal_exact_and_one_under_metadata_rows_precharge_without_copying_opaque_payloads() {
    let (_root, catalogue) = fixture();
    let (mut world, _, ids) = transfer_world(&catalogue);
    let inputs = removal_inputs(&world, ids);
    let before = world.snapshot();
    let usage = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap()
        .usage();
    let exact = RemovalLimits {
        max_rows: 2,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(
        world
            .stage_inventory_removals(&inputs, exact)
            .unwrap()
            .usage(),
        usage
    );
    for limits in [
        RemovalLimits {
            max_rows: 1,
            ..exact
        },
        RemovalLimits {
            max_rows: 0,
            ..exact
        },
        RemovalLimits {
            max_copied_bytes: usage.copied_bytes - 1,
            ..exact
        },
        RemovalLimits {
            max_copied_bytes: 0,
            ..exact
        },
    ] {
        assert!(world.stage_inventory_removals(&inputs, limits).is_err());
        assert_eq!(world.snapshot(), before);
        assert_transfer_counts(&world);
    }
    let mut large = world.item(ids[0]).unwrap().facts().clone();
    large.extra_fields[0].bytes = vec![255; 32 * 1024];
    world.replace_item_facts(ids[0], large.clone()).unwrap();
    let before = world.snapshot();
    let stage = world.stage_inventory_removals(&inputs, exact).unwrap();
    assert_eq!(stage.usage(), usage);
    assert!(stage.usage().copied_bytes < large.extra_fields[0].bytes.len());
    assert_eq!(world.snapshot(), before);
    world.commit_inventory_removals(stage).unwrap();
    assert_eq!(world.item(ids[0]).unwrap().facts(), &large);
}
#[test]
fn empty_duplicate_overdraw_missing_stale_or_changed_removal_rows_never_partly_mutate() {
    let (_root, catalogue) = fixture();
    let (mut world, _, ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let inputs = removal_inputs(&world, ids);
    for rows in [
        Vec::new(),
        vec![inputs[0], inputs[0]],
        vec![inputs[0], (inputs[1].0, quantity(6))],
    ] {
        assert!(
            world
                .stage_inventory_removals(&rows, RemovalLimits::default())
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let stage = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    drop(stage);
    assert_eq!(world.snapshot(), before);
    let stage = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    let mut foreign = before.clone();
    foreign.campaign = CampaignId::from_bytes([0x23; 16]).unwrap();
    let mut other = World::restore(&catalogue, foreign.clone(), Limits::default()).unwrap();
    assert!(
        other
            .stage_inventory_removals(&inputs, RemovalLimits::default())
            .is_err()
    );
    assert!(other.commit_inventory_removals(stage).is_err());
    assert_eq!(other.snapshot(), foreign);
    let stage = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(
        world
            .stage_inventory_removals(&inputs, RemovalLimits::default())
            .is_err()
    );
    assert!(world.commit_inventory_removals(stage).is_err());
    assert_eq!(world.snapshot(), before);
    let inputs = removal_inputs(&world, ids);
    let stage = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    world.remove_item_quantity(ids[2], quantity(5)).unwrap();
    let after = world.snapshot();
    assert!(
        world
            .stage_inventory_removals(&inputs, RemovalLimits::default())
            .is_err()
    );
    assert!(world.commit_inventory_removals(stage).is_err());
    assert_eq!(world.snapshot(), after);
    for change in 0..4 {
        let mut world = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
        let stage = world
            .stage_inventory_removals(&removal_inputs(&world, ids), RemovalLimits::default())
            .unwrap();
        match change {
            0 => world.remove_item_quantity(ids[0], quantity(1)).unwrap(),
            1 => {
                let mut facts = world.item(ids[0]).unwrap().facts().clone();
                facts.condition = None;
                world.replace_item_facts(ids[0], facts).unwrap();
            }
            2 => world
                .transfer_item(ids[0], ReferenceId(2.try_into().unwrap()))
                .unwrap(),
            _ => {
                world.register_reference(None).unwrap();
            }
        }
        let after = world.snapshot();
        assert!(world.commit_inventory_removals(stage).is_err());
        assert_eq!(world.snapshot(), after);
        assert_transfer_counts(&world);
    }
    let mut world = World::restore(&catalogue, before, Limits::default()).unwrap();
    let inputs = removal_inputs(&world, ids);
    let winner = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    let loser = world
        .stage_inventory_removals(&inputs, RemovalLimits::default())
        .unwrap();
    world.commit_inventory_removals(winner).unwrap();
    let after = world.snapshot();
    assert!(world.commit_inventory_removals(loser).is_err());
    assert_eq!(world.snapshot(), after);
}
#[test]
fn removal_revision_exhaustion_refuses_but_exhausted_item_allocator_needs_no_new_id() {
    let (_root, catalogue) = fixture();
    let (world, _, ids) = transfer_world(&catalogue);
    let mut exhausted = world.snapshot();
    exhausted.state_revision = u64::MAX;
    let world = World::restore(&catalogue, exhausted.clone(), Limits::default()).unwrap();
    assert!(
        world
            .stage_inventory_removals(&removal_inputs(&world, ids), RemovalLimits::default())
            .is_err()
    );
    assert_eq!(world.snapshot(), exhausted);
    exhausted.state_revision = 11;
    exhausted.next_item = u64::MAX;
    let mut world = World::restore(&catalogue, exhausted.clone(), Limits::default()).unwrap();
    let stage = world
        .stage_inventory_removals(&removal_inputs(&world, ids), RemovalLimits::default())
        .unwrap();
    world.commit_inventory_removals(stage).unwrap();
    assert_eq!(
        world.snapshot(),
        expected_removal_state(&exhausted, &[(ids[0], 2), (ids[2], 5)])
    );
}
#[test]
fn only_complete_removal_reclaims_exact_item_link_and_opaque_capacity_for_a_real_add() {
    let (_root, catalogue) = fixture();
    let (world, [_, b, _], ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let replacement = world.item(ids[2]).unwrap().facts().clone();
    for limits in [
        Limits {
            max_item_instances: 3,
            ..Limits::default()
        },
        Limits {
            max_total_item_links: 21,
            ..Limits::default()
        },
        Limits {
            max_total_item_bytes: 9,
            ..Limits::default()
        },
        Limits {
            max_item_instances: 3,
            max_total_item_links: 21,
            max_total_item_bytes: 9,
            ..Limits::default()
        },
    ] {
        let mut partial = World::restore(&catalogue, before.clone(), limits).unwrap();
        let inputs = [(partial.item_handle(ids[0]).unwrap(), quantity(2))];
        let stage = partial
            .stage_inventory_removals(&inputs, RemovalLimits::default())
            .unwrap();
        let receipt = partial.commit_inventory_removals(stage).unwrap();
        assert_eq!(receipt.usage().released_links, 0);
        assert_eq!(receipt.usage().released_extra_bytes, 0);
        assert!(
            partial
                .add_item(b, replacement.clone(), quantity(1))
                .is_err()
        );
        let mut world = World::restore(&catalogue, before.clone(), limits).unwrap();
        assert!(world.add_item(b, replacement.clone(), quantity(1)).is_err());
        assert_eq!(world.snapshot(), before);
        let stage = world
            .stage_inventory_removals(&removal_inputs(&world, ids), RemovalLimits::default())
            .unwrap();
        world.commit_inventory_removals(stage).unwrap();
        let boundary = world.snapshot();
        assert_eq!(
            boundary,
            expected_removal_state(&before, &[(ids[0], 2), (ids[2], 5)])
        );
        let new = world.add_item(b, replacement.clone(), quantity(1)).unwrap();
        assert_eq!(new.0.get(), 4);
        assert_eq!(world.item(new).unwrap().facts(), &replacement);
        assert_eq!(world.inventory_count(b, &form(0x200)).unwrap(), 1);
        assert_eq!(world.snapshot().next_item, 5);
        assert_eq!(world.revision(), 13);
        assert_transfer_counts(&world);
        let after = world.snapshot();
        assert!(world.add_item(b, replacement.clone(), quantity(1)).is_err());
        assert_eq!(world.snapshot(), after);
    }
}
#[test]
fn native_removal_before_current_and_two_fresh_cold_consumers_preserve_whole_state() {
    use std::{fs, process::Command};
    let temporary = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_INVENTORY_REMOVAL_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained
        .as_deref()
        .unwrap_or(temporary.path())
        .join("native-boundary");
    fs::create_dir_all(&root).unwrap();
    write_fixture(&root, false);
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let (mut world, _, ids) = transfer_world(&catalogue);
    let before = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let mut worker = fallout_runtime::save::SaveWorker::start(repository.clone(), 2).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let stage = world
        .stage_inventory_removals(&removal_inputs(&world, ids), RemovalLimits::default())
        .unwrap();
    let receipt = world.commit_inventory_removals(stage).unwrap();
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
        root.join("removal.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    let source = fs::read(root.join("FalloutNV.esm")).unwrap();
    drop(world);
    drop(catalogue);
    for (phase, snapshot, wire) in [("before", &before, &previous), ("after", &after, &current)] {
        let phase_root = root.join(phase);
        fs::create_dir(&phase_root).unwrap();
        let copy = Repository::create(&phase_root.join("native"), &[], snapshot.campaign).unwrap();
        fs::write(copy.path().join("current.frsv"), wire).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_inventory_removal_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_INVENTORY_REMOVAL_COLD_ROOT", &root)
            .env("FALLOUT_INVENTORY_REMOVAL_COLD_PHASE", phase)
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
#[ignore = "fresh inventory removal consumer invoked by parent"]
fn cold_inventory_removal_helper() {
    use std::fs;
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_INVENTORY_REMOVAL_COLD_ROOT").unwrap());
    let phase = std::env::var("FALLOUT_INVENTORY_REMOVAL_COLD_PHASE").unwrap();
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
    assert_transfer_counts(&world);
    let views = [1, 2, 3]
        .iter()
        .map(|id| {
            world
                .inventory_view(
                    ReferenceId((*id).try_into().unwrap()),
                    ViewLimits {
                        max_items: 2,
                        max_links: 14,
                        max_extra_bytes: 6,
                    },
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(views[2].items().is_none());
    if phase == "after" {
        assert_eq!(views[1].items(), Some([].as_slice()));
    }
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
