use super::*;
use crate::save::test_source as common;
use crate::{
    Limits as WorldLimits,
    events::{Clocks, Context, Trigger},
    identity::ReferenceValue,
    inventory::Facts,
    state::initialization,
};
use common::{form, load, record, unit, write_fixture};
use std::sync::Arc;

fn fixture() -> (
    tempfile::TempDir,
    fallout_data::loaded_scripts::Catalogue,
    [Handle; 2],
) {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let path = dir.path().join("FalloutNV.esm");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend(record(b"SCPT", 0x301, 0, &unit(&[(42, 0), (43, 0)], &[])));
    std::fs::write(path, bytes).unwrap();
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let handles = catalogue
        .iter()
        .map(|(_, script)| script.handle().clone())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    (dir, catalogue, handles)
}
fn owners() -> [Owner; 3] {
    [
        Owner::Fragment {
            activation: 1.try_into().unwrap(),
        },
        Owner::Quest { key: form(0x600) },
        Owner::Placed {
            reference: crate::identity::ReferenceId(1.try_into().unwrap()),
        },
    ]
}
fn setup<'a>(
    catalogue: &'a fallout_data::loaded_scripts::Catalogue,
    definitions: &[Handle; 2],
    world_limits: WorldLimits,
) -> (World<'a>, [InstanceHandle; 3]) {
    let mut world = World::new(catalogue, world_limits).unwrap();
    let reference = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Null],
    };
    let owners = owners();
    let handles = std::array::from_fn(|index| {
        world
            .create_instance(
                &definitions[usize::from(index == 2)],
                owners[index].clone(),
                context.clone(),
            )
            .unwrap()
    });
    world
        .enqueue(
            handles[0],
            Trigger::ObjectEvent { mask: 0x8000_0001 },
            context,
        )
        .unwrap();
    world.initialize_inventory(reference).unwrap();
    let mut facts = Facts::unknown(form(0x100));
    facts.script_instance = Some(world.instance(handles[0]).unwrap().id());
    world
        .add_item(reference, facts, 8.try_into().unwrap())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 3,
            menu_nanoseconds: 5,
            real_nanoseconds: 7,
        })
        .unwrap();
    (world, handles)
}
fn unchanged(world: &World<'_>) -> impl PartialEq + std::fmt::Debug + use<> {
    (
        world.snapshot(),
        world.free.clone(),
        world
            .slots
            .iter()
            .map(|slot| {
                (
                    slot.generation,
                    slot.value.as_ref().map(super::super::Instance::id),
                )
            })
            .collect::<Vec<_>>(),
        world.local_count,
        world.block_count,
        world
            .definitions
            .iter()
            .map(|(key, schema)| {
                (
                    key.clone(),
                    Arc::as_ptr(schema) as usize,
                    Arc::strong_count(schema),
                )
            })
            .collect::<Vec<_>>(),
    )
}
#[test]
fn caller_order_retirement_releases_only_instances_locals_and_reuses_slots_without_cache_eviction()
{
    let (_dir, catalogue, definitions) = fixture();
    let bounds = WorldLimits {
        max_instances: 3,
        max_locals: 8,
        max_event_blocks: 2,
        ..WorldLimits::default()
    };
    let (mut world, handles) = setup(&catalogue, &definitions, bounds);
    let before = unchanged(&world);
    let pointers = world
        .definitions
        .values()
        .map(Arc::as_ptr)
        .collect::<Vec<_>>();
    let stage = world
        .stage_instance_retirement_group(&[handles[2], handles[1]], Limits::default())
        .unwrap();
    assert_eq!(
        stage.rows().iter().map(Row::instance).collect::<Vec<_>>(),
        vec![
            InstanceId(3.try_into().unwrap()),
            InstanceId(2.try_into().unwrap())
        ]
    );
    assert_eq!(stage.usage().removed_locals, 5);
    assert_eq!(stage.usage().visited_events, 1);
    assert_eq!(stage.usage().visited_items, 1);
    assert_eq!(unchanged(&world), before);
    let revision = world.revision();
    let receipt = world.commit_instance_retirement_group(stage).unwrap();
    assert_eq!(receipt.before_revision(), revision);
    assert_eq!(receipt.after_revision(), revision + 1);
    assert_eq!(world.local_count, 3);
    assert_eq!(world.instance_count(), 1);
    assert_eq!(world.next_instance, 4);
    assert_eq!(world.free, vec![handles[2].slot, handles[1].slot]);
    assert_eq!(world.block_count, 2);
    assert_eq!(world.definitions.len(), 2);
    assert_eq!(
        world
            .definitions
            .values()
            .map(Arc::as_ptr)
            .collect::<Vec<_>>(),
        pointers
    );
    assert_eq!(
        world
            .definitions
            .values()
            .map(Arc::strong_count)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(
        world.pending.front().unwrap().instance,
        world.instance(handles[0]).unwrap().id()
    );
    assert_eq!(world.items.len(), 1);
    assert_eq!(
        world.items.values().next().unwrap().facts.script_instance,
        Some(InstanceId(1.try_into().unwrap()))
    );
    let owner_values = owners();
    for (position, old) in [(1, handles[1]), (2, handles[2])] {
        let stage = world
            .stage_instance_initialization(
                &definitions[usize::from(position == 2)],
                &owner_values[position],
                &Context::default(),
                &[],
                initialization::Limits::default(),
            )
            .unwrap();
        let (_, current) = world.commit_instance_initialization(stage).unwrap();
        assert_eq!(current.slot, old.slot);
        assert_eq!(current.generation, old.generation + 1);
        assert!(matches!(world.instance(old), Err(Error::StaleHandle)));
    }
    assert_eq!(world.local_count, 8);
    assert_eq!(world.instance_count(), 3);
    assert_eq!(world.next_instance, 6);
    assert!(world.free.is_empty());
    assert_eq!(world.block_count, 2);
}
#[test]
fn exact_metadata_visited_and_removed_local_limits_refuse_one_under_without_any_private_effect() {
    let (_dir, catalogue, definitions) = fixture();
    let (world, handles) = setup(&catalogue, &definitions, WorldLimits::default());
    let usage = world
        .stage_instance_retirement_group(&handles[1..], Limits::default())
        .unwrap()
        .usage();
    let exact = Limits {
        max_instances: 2,
        max_visited_events: 1,
        max_visited_items: 1,
        max_removed_locals: 5,
        max_copied_bytes: usage.copied_bytes,
    };
    let before = unchanged(&world);
    for limits in [
        Limits {
            max_instances: 1,
            ..exact
        },
        Limits {
            max_visited_events: 0,
            ..exact
        },
        Limits {
            max_visited_items: 0,
            ..exact
        },
        Limits {
            max_removed_locals: 4,
            ..exact
        },
        Limits {
            max_copied_bytes: usage.copied_bytes - 1,
            ..exact
        },
    ] {
        assert!(
            world
                .stage_instance_retirement_group(&handles[1..], limits)
                .is_err()
        );
        assert_eq!(unchanged(&world), before);
    }
    assert!(
        world
            .stage_instance_retirement_group(&handles[1..], exact)
            .is_ok()
    );
    assert_eq!(unchanged(&world), before);
    let mut bytes = 1;
    assert!(charge(&mut bytes, usize::MAX, 2).is_err());
    assert_eq!(bytes, 1);
    assert!(charge(&mut bytes, 1, usize::MAX).is_err());
    assert_eq!(bytes, 1);
}
#[test]
fn every_private_binding_original_row_and_commit_admission_refuses_before_owner_teardown() {
    let (_dir, catalogue, definitions) = fixture();
    let (mut world, handles) = setup(&catalogue, &definitions, WorldLimits::default());
    let before = unchanged(&world);
    for changed in 0..17 {
        let mut stage = world
            .stage_instance_retirement_group(&handles[1..], Limits::default())
            .unwrap();
        match changed {
            0 => stage.epoch += 1,
            1 => stage.campaign = CampaignId::from_bytes([2; 16]).unwrap(),
            2 => stage.cohort = "f".repeat(64),
            3 => stage.revision += 1,
            4 => stage.next_instance += 1,
            5 => stage.instance_count += 1,
            6 => stage.local_count += 1,
            7 => stage.free_count += 1,
            8 => stage.rows[1].instance = InstanceId(99.try_into().unwrap()),
            9 => {
                stage.rows[1].owner = Owner::Fragment {
                    activation: 99.try_into().unwrap(),
                }
            }
            10 => stage.rows[1].definition.version_sha256 = "f".repeat(64),
            11 => stage.rows[1].locals += 1,
            12 => stage.rows[1].next_generation += 1,
            13 => stage.rows[1].handle.generation += 1,
            14 => stage.usage.removed_locals += 1,
            15 => stage.limits.max_visited_events = 0,
            _ => stage.limits.max_visited_items = 0,
        }
        assert!(world.commit_instance_retirement_group(stage).is_err());
        assert_eq!(unchanged(&world), before);
    }
}
#[test]
fn pending_and_item_links_keep_original_single_removal_policy_and_reject_the_entire_selection() {
    let (_dir, catalogue, definitions) = fixture();
    let (mut world, handles) = setup(&catalogue, &definitions, WorldLimits::default());
    let (mut legacy, legacy_handles) = setup(&catalogue, &definitions, WorldLimits::default());
    let legacy_before = legacy.snapshot();
    legacy.remove_instance(legacy_handles[1]).unwrap();
    assert!(legacy.remove_instance(legacy_handles[0]).is_err());
    assert_ne!(legacy.snapshot(), legacy_before);
    assert_eq!(legacy.instance_count(), 2);
    let before = unchanged(&world);
    let group = world
        .stage_instance_retirement_group(&[handles[1], handles[0]], Limits::default())
        .unwrap_err();
    let single = world.remove_instance(handles[0]).unwrap_err();
    assert_eq!(group.to_string(), single.to_string());
    assert_eq!(unchanged(&world), before);
    world.acknowledge(1).unwrap();
    let before = unchanged(&world);
    let group = world
        .stage_instance_retirement_group(&[handles[1], handles[0]], Limits::default())
        .unwrap_err();
    let single = world.remove_instance(handles[0]).unwrap_err();
    assert_eq!(group.to_string(), single.to_string());
    assert_eq!(unchanged(&world), before);
    assert!(
        world
            .stage_instance_retirement_group(&[handles[1], handles[1]], Limits::default())
            .is_err()
    );
    assert_eq!(unchanged(&world), before);
}
#[test]
fn final_generation_revision_and_local_arithmetic_refusals_preserve_slots_and_empty_noop() {
    let (_dir, catalogue, definitions) = fixture();
    let (mut world, handles) = setup(&catalogue, &definitions, WorldLimits::default());
    world.slots[handles[2].slot].generation = u64::MAX;
    let final_handle = world.handle(InstanceId(3.try_into().unwrap())).unwrap();
    let before = unchanged(&world);
    assert!(matches!(
        world.stage_instance_retirement_group(&[handles[1], final_handle], Limits::default()),
        Err(Error::Capacity("slot generations"))
    ));
    assert_eq!(unchanged(&world), before);
    world.slots[handles[2].slot].generation = handles[2].generation;
    world.revision = u64::MAX;
    let before = unchanged(&world);
    assert!(
        world
            .stage_instance_retirement_group(&handles[1..], Limits::default())
            .is_err()
    );
    assert_eq!(unchanged(&world), before);
    let empty = world
        .stage_instance_retirement_group(
            &[],
            Limits {
                max_instances: 0,
                max_visited_events: 0,
                max_visited_items: 0,
                max_removed_locals: 0,
                ..Limits::default()
            },
        )
        .unwrap();
    assert_eq!(empty.usage().visited_events, 0);
    assert_eq!(empty.usage().visited_items, 0);
    let receipt = world.commit_instance_retirement_group(empty).unwrap();
    assert_eq!(receipt.before_revision(), u64::MAX);
    assert_eq!(receipt.after_revision(), u64::MAX);
    assert_eq!(unchanged(&world), before);
    world.revision = 8;
    world.local_count = 1;
    let before = unchanged(&world);
    assert!(
        world
            .stage_instance_retirement_group(&handles[1..], Limits::default())
            .is_err()
    );
    assert_eq!(unchanged(&world), before);
    assert_eq!(next_generation(u64::MAX - 1).unwrap(), u64::MAX);
    assert!(next_generation(u64::MAX).is_err());
}
#[test]
fn unrelated_cache_warming_without_revision_does_not_evict_or_invalidate_retirement() {
    let (_dir, catalogue, definitions) = fixture();
    let mut world = World::new(
        &catalogue,
        WorldLimits {
            max_locals: 9,
            max_event_blocks: 2,
            ..WorldLimits::default()
        },
    )
    .unwrap();
    let context = Context::default();
    let handles = [1_u64, 2, 3].map(|id| {
        world
            .create_instance(
                &definitions[0],
                Owner::Fragment {
                    activation: id.try_into().unwrap(),
                },
                context.clone(),
            )
            .unwrap()
    });
    let stage = world
        .stage_instance_retirement_group(&handles[1..], Limits::default())
        .unwrap();
    let before = world.snapshot();
    assert!(
        world
            .create_instance(
                &definitions[1],
                Owner::Fragment {
                    activation: 4.try_into().unwrap()
                },
                context
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    assert_eq!(world.definitions.len(), 2);
    assert_eq!(world.block_count, 2);
    let pointers = world
        .definitions
        .values()
        .map(Arc::as_ptr)
        .collect::<Vec<_>>();
    world.commit_instance_retirement_group(stage).unwrap();
    assert_eq!(
        world
            .definitions
            .values()
            .map(Arc::as_ptr)
            .collect::<Vec<_>>(),
        pointers
    );
    assert_eq!(world.block_count, 2);
    assert_eq!(world.local_count, 3);
}
