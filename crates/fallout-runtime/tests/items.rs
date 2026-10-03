mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::Context,
    identity::{InstanceId, Owner, ReferenceId},
    inventory::{Ammo, Condition, Facts, ItemId, OpaqueExtra, Ownership},
    save::{Captured, Recovery, Repository},
    snapshot::Snapshot,
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
