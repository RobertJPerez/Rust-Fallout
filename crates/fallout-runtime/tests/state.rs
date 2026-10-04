mod common;
use common::*;
use fallout_runtime::{
    Error, Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{Owner, ReferenceValue, Value},
    schema::{self, Kind},
    snapshot::Snapshot,
};

fn owner(n: u64) -> Owner {
    Owner::Fragment {
        activation: n.try_into().unwrap(),
    }
}
fn block() -> Trigger {
    Trigger::Block {
        event_id: 0,
        begin_byte_offset: 0,
    }
}

#[test]
fn source_less_schema_keeps_first_declarations_and_reference_membership() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let script = catalogue.iter().next().unwrap().1;
    let locals = schema::locals(script);
    assert_eq!(locals[&2].kind, Kind::Integer);
    assert_eq!(locals[&42].kind, Kind::Float);
    assert_eq!(locals[&90].kind, Kind::Reference);
    assert_eq!(locals[&99].kind, Kind::Unsupported { type_byte: 7 });
    assert_eq!(locals[&0].kind, Kind::UnverifiedZeroIndex { type_byte: 0 });
    assert_eq!(locals.len(), 5);
    assert_eq!(
        locals[&42].declaration_decoded_offset,
        script.declaration(42).unwrap().decoded_offset
    );
}

#[test]
fn assignments_are_atomic_typed_and_do_not_invent_initial_values() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let handle = world
        .create_instance(&definition(&catalogue), owner(1), Context::default())
        .unwrap();
    assert!(matches!(
        world.instance(handle).unwrap().local(2),
        Err(Error::UninitializedLocal(2))
    ));
    let before = world.snapshot();
    assert!(matches!(
        world.assign(
            handle,
            &[
                (2, Value::Number { bits: 123 }),
                (90, Value::Number { bits: 456 })
            ]
        ),
        Err(Error::IncompatibleLocal(90))
    ));
    assert_eq!(before, world.snapshot());
    assert!(
        world
            .assign(
                handle,
                &[
                    (2, Value::Number { bits: 1 }),
                    (2, Value::Number { bits: 2 })
                ]
            )
            .is_err()
    );
    assert_eq!(before, world.snapshot());
    assert!(
        world
            .assign(handle, &[(8, Value::Number { bits: 0 })])
            .is_err()
    );
    assert_eq!(world.reset_locals(handle).unwrap(), 3);
    assert_eq!(
        world.instance(handle).unwrap().local(90).unwrap(),
        &Value::Reference {
            value: ReferenceValue::Null
        }
    );
}

#[test]
fn unknown_declarations_make_reset_fail_without_partial_changes() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let handle = world
        .create_instance(&definition(&catalogue), owner(1), Context::default())
        .unwrap();
    world
        .assign(
            handle,
            &[(
                42,
                Value::Number {
                    bits: 0x8000000000000000,
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    assert!(matches!(
        world.reset_locals(handle),
        Err(Error::UnsupportedLocal(0))
    ));
    assert!(matches!(
        world.assign(handle, &[(99, Value::Number { bits: 0 })]),
        Err(Error::UnsupportedLocal(99))
    ));
    assert_eq!(before, world.snapshot());
}

#[test]
fn recycled_slots_and_restored_worlds_cannot_accept_old_handles() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let old = world
        .create_instance(&definition(&catalogue), owner(1), Context::default())
        .unwrap();
    let old_id = world.instance(old).unwrap().id();
    world.remove_instance(old).unwrap();
    let new = world
        .create_instance(&definition(&catalogue), owner(2), Context::default())
        .unwrap();
    let new_id = world.instance(new).unwrap().id();
    assert!(new_id > old_id);
    assert!(matches!(world.instance(old), Err(Error::StaleHandle)));
    let unrelated = World::new(&catalogue, Limits::default()).unwrap();
    assert!(matches!(unrelated.instance(new), Err(Error::StaleHandle)));
    world.replace_from_snapshot(world.snapshot()).unwrap();
    assert!(matches!(world.instance(new), Err(Error::StaleHandle)));
    let restored = world.handle(new_id).unwrap();
    assert_eq!(world.instance(restored).unwrap().id(), new_id);
    world.remove_instance(restored).unwrap();
    let next = world
        .create_instance(&definition(&catalogue), owner(3), Context::default())
        .unwrap();
    assert!(world.instance(next).unwrap().id() > new_id);
}

#[test]
fn reference_resolution_uses_typed_locals_and_explicit_player_binding() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let player = world.register_reference(None).unwrap();
    let handle = world
        .create_instance(
            &definition(&catalogue),
            Owner::Placed { reference: player },
            Context::default(),
        )
        .unwrap();
    assert!(matches!(
        world.resolve_script_reference(handle, 1, None),
        Err(Error::UninitializedLocal(90))
    ));
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: player },
                },
            )],
        )
        .unwrap();
    assert_eq!(
        world.resolve_script_reference(handle, 1, None).unwrap(),
        ReferenceValue::Live { id: player }
    );
    assert_eq!(
        world.resolve_script_reference(handle, 2, None).unwrap(),
        ReferenceValue::Content { key: form(0x100) }
    );
    assert!(world.resolve_script_reference(handle, 3, None).is_err());
    assert_eq!(
        world
            .resolve_script_reference(handle, 3, Some(player))
            .unwrap(),
        ReferenceValue::Live { id: player }
    );
    assert_eq!(
        world.resolve_script_reference(handle, 4, None).unwrap(),
        ReferenceValue::Null
    );
    for index in [0, 5, 6, 7] {
        assert!(
            world
                .resolve_script_reference(handle, index, Some(player))
                .is_err()
        );
    }
    assert!(world.register_reference(Some(form(0x500))).is_ok());
    let before = world.snapshot();
    assert!(world.register_reference(Some(form(0x500))).is_err());
    assert_eq!(before, world.snapshot());
}

#[test]
fn numeric_bits_and_reference_identity_round_trip_exactly_through_json() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let reference = world.register_reference(Some(form(0x888))).unwrap();
    let handle = world
        .create_instance(&definition(&catalogue), owner(1), Context::default())
        .unwrap();
    let id = world.instance(handle).unwrap().id();
    for bits in [
        0,
        1,
        u64::MAX,
        0x8000000000000000,
        0x7ff0000000000000,
        0x7ff8123456789abc,
        0xfff0123456789abc,
        0x4340000000000001,
    ]
    .into_iter()
    .chain((0..64).map(|shift| 1_u64 << shift))
    {
        world
            .assign(
                handle,
                &[
                    (42, Value::Number { bits }),
                    (
                        90,
                        Value::Reference {
                            value: ReferenceValue::Live { id: reference },
                        },
                    ),
                ],
            )
            .unwrap();
        let snapshot = world.snapshot();
        let bytes = snapshot
            .encode(Limits::default().max_snapshot_bytes)
            .unwrap();
        let decoded = Snapshot::decode(&bytes, Limits::default()).unwrap();
        assert_eq!(snapshot, decoded);
        let restored = World::restore(&catalogue, decoded, Limits::default()).unwrap();
        assert_eq!(
            restored
                .instance(restored.handle(id).unwrap())
                .unwrap()
                .local(42)
                .unwrap(),
            &Value::Number { bits }
        );
        assert_eq!(
            restored
                .snapshot()
                .encode(Limits::default().max_snapshot_bytes)
                .unwrap(),
            bytes
        );
    }
}

#[test]
fn events_preserve_context_clocks_and_distinct_id_domains() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let reference = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Content { key: form(0x100) }),
        arguments: vec![ReferenceValue::Live { id: reference }],
    };
    let handle = world
        .create_instance(&definition(&catalogue), owner(1), context.clone())
        .unwrap();
    let clocks = Clocks {
        tick: 1,
        game_nanoseconds: 100,
        menu_nanoseconds: 0,
        real_nanoseconds: 120,
    };
    world.advance_clocks(clocks).unwrap();
    let a = world.enqueue(handle, block(), context.clone()).unwrap();
    let b = world
        .enqueue(
            handle,
            Trigger::ObjectEvent { mask: 0x80000001 },
            context.clone(),
        )
        .unwrap();
    let before = world.snapshot();
    assert!(
        world
            .enqueue(
                handle,
                Trigger::Block {
                    event_id: 1,
                    begin_byte_offset: 0
                },
                Context::default()
            )
            .is_err()
    );
    assert!(
        world
            .enqueue(
                handle,
                Trigger::Block {
                    event_id: 0,
                    begin_byte_offset: 1
                },
                Context::default()
            )
            .is_err()
    );
    assert!(
        world
            .enqueue(handle, Trigger::ObjectEvent { mask: 0 }, Context::default())
            .is_err()
    );
    assert!(world.advance_clocks(clocks).is_err());
    assert!(world.acknowledge(b).is_err());
    assert!(world.remove_instance(handle).is_err());
    assert_eq!(before, world.snapshot());
    let mut restored = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    assert_eq!(restored.snapshot(), before);
    let event = restored.acknowledge(a).unwrap();
    assert_eq!(event.context, context);
    assert_eq!(event.arrived, clocks);
    assert_eq!(event.trigger, block());
    assert_eq!(
        restored.acknowledge(b).unwrap().trigger,
        Trigger::ObjectEvent { mask: 0x80000001 }
    );
    assert_eq!(restored.pending_events().len(), 0);
}

#[test]
fn owner_and_event_budgets_reject_without_dropping_state() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut no_blocks = World::new(
        &catalogue,
        Limits {
            max_event_blocks: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(
        no_blocks
            .create_instance(&definition(&catalogue), owner(1), Context::default())
            .is_err()
    );
    assert_eq!(no_blocks.instance_count(), 0);
    let limits = Limits {
        max_instances: 1,
        max_references: 1,
        max_locals: 3,
        max_pending_events: 1,
        max_event_arguments: 1,
        ..Limits::default()
    };
    let mut world = World::new(&catalogue, limits).unwrap();
    world.register_reference(None).unwrap();
    assert!(world.register_reference(None).is_err());
    let handle = world
        .create_instance(&definition(&catalogue), owner(1), Context::default())
        .unwrap();
    assert!(
        world
            .create_instance(&definition(&catalogue), owner(1), Context::default())
            .is_err()
    );
    assert!(
        world
            .create_instance(&definition(&catalogue), owner(2), Context::default())
            .is_err()
    );
    world.enqueue(handle, block(), Context::default()).unwrap();
    let before = world.snapshot();
    assert!(world.enqueue(handle, block(), Context::default()).is_err());
    assert!(
        world
            .enqueue(
                handle,
                block(),
                Context {
                    arguments: vec![ReferenceValue::Null, ReferenceValue::Null],
                    ..Context::default()
                }
            )
            .is_err()
    );
    assert_eq!(before, world.snapshot());
    assert!(
        World::restore(
            &catalogue,
            before.clone(),
            Limits {
                max_locals: 2,
                ..limits
            }
        )
        .is_err()
    );
    assert!(before.encode(20).is_err());
    assert!(
        Snapshot::decode(
            &before.encode(10000).unwrap(),
            Limits {
                max_snapshot_bytes: 20,
                ..limits
            }
        )
        .is_err()
    );
}

#[test]
fn unchanged_source_set_with_different_nonscript_winners_cannot_restore() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    for (name, kind) in [("A.esp", b"ACTI"), ("B.esp", b"STAT")] {
        std::fs::write(
            dir.path().join(name),
            [header(&["FalloutNV.esm"]), record(kind, 0x999, 0, &[])].concat(),
        )
        .unwrap();
    }
    let a = load(dir.path(), &["FalloutNV.esm", "A.esp", "B.esp"]);
    let b = load(dir.path(), &["FalloutNV.esm", "B.esp", "A.esp"]);
    assert_eq!(definition(&a), definition(&b));
    let world = World::new(&a, Limits::default()).unwrap();
    assert!(matches!(
        World::restore(&b, world.snapshot(), Limits::default()),
        Err(Error::DefinitionChanged)
    ));
}

#[test]
fn restores_reject_changed_sources_and_allow_same_source_reordering() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let a = load(dir.path(), &["FalloutNV.esm", "Other.esm"]);
    let b = load(dir.path(), &["Other.esm", "FalloutNV.esm"]);
    let mut world = World::new(&a, Limits::default()).unwrap();
    world
        .create_instance(&definition(&a), owner(1), Context::default())
        .unwrap();
    let snapshot = world.snapshot();
    assert_eq!(
        World::restore(&b, snapshot.clone(), Limits::default())
            .unwrap()
            .snapshot(),
        snapshot
    );
    std::fs::write(
        dir.path().join("Other.esm"),
        [header(&[]), record(b"ACTI", 0x999, 0, &[])].concat(),
    )
    .unwrap();
    let changed = load(dir.path(), &["FalloutNV.esm", "Other.esm"]);
    assert!(matches!(
        World::restore(&changed, snapshot, Limits::default()),
        Err(Error::DefinitionChanged)
    ));
}

#[test]
fn malformed_snapshots_never_replace_the_existing_world() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = World::new(&catalogue, Limits::default()).unwrap();
    let reference = world.register_reference(None).unwrap();
    let handle = world
        .create_instance(&definition(&catalogue), owner(1), Context::default())
        .unwrap();
    world.enqueue(handle, block(), Context::default()).unwrap();
    let good = world.snapshot();
    let mut cases = Vec::new();
    let mut bad = good.clone();
    bad.schema_version += 1;
    cases.push(bad);
    let mut bad = good.clone();
    bad.profile = fallout_data::identity::ProfileId::Fo3Original;
    cases.push(bad);
    let mut bad = good.clone();
    bad.instances[0].definition.version_sha256 = "0".repeat(64);
    cases.push(bad);
    let mut bad = good.clone();
    bad.references.push(bad.references[0].clone());
    cases.push(bad);
    let mut bad = good.clone();
    bad.instances.push(bad.instances[0].clone());
    cases.push(bad);
    let mut bad = good.clone();
    bad.instances[0].locals[0].index = 500;
    cases.push(bad);
    let mut bad = good.clone();
    bad.instances[0].locals[1] = bad.instances[0].locals[0].clone();
    cases.push(bad);
    let mut bad = good.clone();
    bad.instances[0].locals.remove(0);
    cases.push(bad);
    let mut bad = good.clone();
    bad.instances[0].locals[2].value = Value::Number { bits: 0 };
    cases.push(bad);
    let mut bad = good.clone();
    bad.next_instance = 1;
    cases.push(bad);
    let mut bad = good.clone();
    bad.next_reference = reference.0.get();
    cases.push(bad);
    let mut bad = good.clone();
    bad.next_event_sequence = 1;
    cases.push(bad);
    let mut bad = good.clone();
    bad.pending_events[0].sequence = 0;
    cases.push(bad);
    let mut bad = good.clone();
    bad.pending_events[0].arrived.tick = 10;
    cases.push(bad);
    let mut bad = good.clone();
    bad.references.clear();
    cases.push(bad);
    // Bind the instance context to the registry so deleting it must fail.
    cases.last_mut().unwrap().instances[0]
        .context
        .calling_reference = Some(reference);
    for bad in cases {
        assert!(world.replace_from_snapshot(bad).is_err());
        assert_eq!(good, world.snapshot());
        assert!(world.instance(handle).is_ok());
    }
    let bytes = good.encode(10000).unwrap();
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    json["unknown_field"] = true.into();
    assert!(Snapshot::decode(&serde_json::to_vec(&json).unwrap(), Limits::default()).is_err());
    json.as_object_mut().unwrap().remove("unknown_field");
    json["instances"][0]["id"] = 0.into();
    assert!(Snapshot::decode(&serde_json::to_vec(&json).unwrap(), Limits::default()).is_err());
}

fn staged_world(catalogue: &fallout_data::loaded_scripts::Catalogue) -> World<'_> {
    let mut world = World::new(catalogue, Limits::default()).unwrap();
    let reference = world.register_reference(Some(form(0x100))).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Live { id: reference }],
        ..Context::default()
    };
    let handle = world
        .create_instance(&definition(catalogue), owner(1), context.clone())
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 1 })])
        .unwrap();
    world.enqueue(handle, block(), context).unwrap();
    world
}

#[test]
fn staged_changes_are_owned_and_dropping_or_staging_has_no_effect() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = staged_world(&catalogue);
    let before = world.snapshot();
    let mut inputs = vec![
        (42, Value::Number { bits: u64::MAX }),
        (
            90,
            Value::Reference {
                value: ReferenceValue::Content { key: form(0x100) },
            },
        ),
    ];
    let discarded = world.stage_event_changes(1, &inputs, true).unwrap();
    assert_eq!(discarded.base_revision(), before.state_revision);
    assert_eq!(discarded.sequence(), 1);
    assert_eq!(discarded.instance(), before.instances[0].id);
    assert_eq!(discarded.definition(), &before.instances[0].definition);
    assert_eq!(discarded.assignments(), inputs);
    assert!(discarded.acknowledges());
    assert_eq!(world.snapshot(), before);
    drop(discarded);
    assert_eq!(world.snapshot(), before);
    let stage = world.stage_event_changes(1, &inputs, false).unwrap();
    inputs[0].1 = Value::Number { bits: 0 };
    if let Value::Reference {
        value: ReferenceValue::Content { key },
    } = &mut inputs[1].1
    {
        key.origin_plugin = "changed.esm".into();
    }
    let receipt = world.commit_event_changes(stage).unwrap();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = Value::Number { bits: u64::MAX };
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 90)
        .unwrap()
        .value = Value::Reference {
        value: ReferenceValue::Content { key: form(0x100) },
    };
    assert_eq!(world.snapshot(), expected);
    assert!(receipt.acknowledged.is_none());
    assert_eq!(receipt.assignments, 2);
}

#[test]
fn staged_locals_and_exact_head_acknowledge_commit_in_one_revision() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = staged_world(&catalogue);
    let other = world
        .create_instance(&definition(&catalogue), owner(2), Context::default())
        .unwrap();
    world.enqueue(other, block(), Context::default()).unwrap();
    let before = world.snapshot();
    let stage = world
        .stage_event_changes(
            1,
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
            true,
        )
        .unwrap();
    let receipt = world.commit_event_changes(stage).unwrap();
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 42)
        .unwrap()
        .value = Value::Number {
        bits: 0x7ff8_1234_5678_9abc,
    };
    let acknowledged = expected.pending_events.remove(0);
    assert_eq!(world.snapshot(), expected);
    assert_eq!(receipt.acknowledged, Some(acknowledged));
    assert_eq!(receipt.campaign, before.campaign);
    assert_eq!(receipt.catalogue_sha256, before.catalogue_sha256);
    assert_eq!(receipt.definition, before.instances[0].definition);
    assert_eq!(receipt.instance, before.instances[0].id);
    assert_eq!(receipt.sequence, 1);
    assert_eq!(receipt.before_revision, before.state_revision);
    assert_eq!(receipt.after_revision, before.state_revision + 1);
    assert_eq!(receipt.boundary, before.clocks);
    assert_eq!(receipt.assignments, 1);
    assert_eq!(world.pending_events().next().unwrap().sequence, 2);
}

#[test]
fn staged_empty_commits_count_once_and_replay_always_rejects() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    for acknowledge in [false, true] {
        let mut world = staged_world(&catalogue);
        let before = world.snapshot();
        let stage = world.stage_event_changes(1, &[], acknowledge).unwrap();
        let equivalent = world.stage_event_changes(1, &[], acknowledge).unwrap();
        let receipt = world.commit_event_changes(stage).unwrap();
        let mut expected = before.clone();
        expected.state_revision += 1;
        if acknowledge {
            expected.pending_events.remove(0);
        }
        assert_eq!(world.snapshot(), expected);
        assert_eq!(receipt.assignments, 0);
        assert_eq!(receipt.acknowledged.is_some(), acknowledge);
        assert!(
            matches!(world.commit_event_changes(equivalent), Err(Error::Invalid(message)) if message == "staged event revision changed")
        );
        assert_eq!(world.snapshot(), expected);
    }
}

#[test]
fn staged_invalid_batches_and_nonhead_requests_preserve_all_state() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = staged_world(&catalogue);
    let handle = world.handle(world.snapshot().instances[0].id).unwrap();
    world.enqueue(handle, block(), Context::default()).unwrap();
    let before = world.snapshot();
    for sequence in [0, 2, u64::MAX] {
        assert!(
            matches!(world.stage_event_changes(sequence, &[], true), Err(Error::Invalid(message)) if message == "staging must name the first pending event")
        );
        assert_eq!(world.snapshot(), before);
    }
    let good = (42, Value::Number { bits: 7 });
    for (batch, expected) in [
        (vec![good.clone(), (90, Value::Number { bits: 8 })], "kind"),
        (vec![good.clone(), good.clone()], "duplicate"),
        (
            vec![good.clone(), (500, Value::Number { bits: 1 })],
            "missing",
        ),
        (
            vec![good.clone(), (99, Value::Number { bits: 1 })],
            "unsupported",
        ),
        (
            vec![
                good.clone(),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live {
                            id: fallout_runtime::identity::ReferenceId(999.try_into().unwrap()),
                        },
                    },
                ),
            ],
            "reference",
        ),
        (vec![good.clone(); 6], "capacity"),
    ] {
        let error = world.stage_event_changes(1, &batch, true).unwrap_err();
        match expected {
            "kind" => assert!(matches!(error, Error::IncompatibleLocal(90))),
            "duplicate" => assert!(
                matches!(error, Error::Invalid(message) if message == "duplicate local assignment")
            ),
            "missing" => assert!(matches!(error, Error::MissingLocal(500))),
            "unsupported" => assert!(matches!(error, Error::UnsupportedLocal(99))),
            "reference" => assert!(matches!(error, Error::MissingReference)),
            "capacity" => assert!(matches!(error, Error::Capacity("assignment batch"))),
            _ => unreachable!(),
        }
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn staged_changes_reject_every_intervening_mutation_without_overwriting_it() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    for mutation in 0..6 {
        let mut world = staged_world(&catalogue);
        let handle = world.handle(world.snapshot().instances[0].id).unwrap();
        let stage = world
            .stage_event_changes(1, &[(42, Value::Number { bits: 8 })], true)
            .unwrap();
        match mutation {
            0 => {
                world
                    .assign(handle, &[(42, Value::Number { bits: 2 })])
                    .unwrap();
            }
            1 => {
                world.enqueue(handle, block(), Context::default()).unwrap();
            }
            2 => {
                world
                    .advance_clocks(Clocks {
                        tick: 1,
                        ..Clocks::default()
                    })
                    .unwrap();
            }
            3 => {
                world.register_reference(None).unwrap();
            }
            4 => {
                world.acknowledge(1).unwrap();
            }
            5 => {
                world
                    .initialize_inventory(world.snapshot().references[0].id)
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let after_mutation = world.snapshot();
        assert!(
            matches!(world.commit_event_changes(stage), Err(Error::Invalid(message)) if message == "staged event revision changed")
        );
        assert_eq!(world.snapshot(), after_mutation);
    }
}

#[test]
fn staged_changes_reject_an_equal_restored_or_other_world_epoch() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = staged_world(&catalogue);
    let before = world.snapshot();
    let for_other = world.stage_event_changes(1, &[], true).unwrap();
    let mut other = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.commit_event_changes(for_other),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), before);
    let for_restore = world.stage_event_changes(1, &[], true).unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(matches!(
        world.commit_event_changes(for_restore),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), before);
    world
        .commit_event_changes(world.stage_event_changes(1, &[], true).unwrap())
        .unwrap();
    assert!(world.pending_events().next().is_none());
}

#[test]
fn staged_revision_exhaustion_fails_before_locals_or_acknowledgment() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let world = staged_world(&catalogue);
    let mut before = world.snapshot();
    before.state_revision = u64::MAX;
    let mut exhausted = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    let stage = exhausted
        .stage_event_changes(1, &[(42, Value::Number { bits: 2 })], true)
        .unwrap();
    assert!(matches!(
        exhausted.commit_event_changes(stage),
        Err(Error::Capacity("state revisions"))
    ));
    assert_eq!(exhausted.snapshot(), before);
}

#[test]
fn staged_exact_numeric_and_reference_values_restore_with_pending_head_retained() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = staged_world(&catalogue);
    let reference = world.snapshot().references[0].id;
    for bits in [0, u64::MAX, 0x8000_0000_0000_0000, 0x7ff8_1234_5678_9abc] {
        let stage = world
            .stage_event_changes(
                1,
                &[
                    (42, Value::Number { bits }),
                    (
                        90,
                        Value::Reference {
                            value: ReferenceValue::Live { id: reference },
                        },
                    ),
                ],
                false,
            )
            .unwrap();
        world.commit_event_changes(stage).unwrap();
        let snapshot = world.snapshot();
        let bytes = snapshot
            .encode(Limits::default().max_snapshot_bytes)
            .unwrap();
        world = World::restore(
            &catalogue,
            Snapshot::decode(&bytes, Limits::default()).unwrap(),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(world.snapshot(), snapshot);
        assert_eq!(world.pending_events().next().unwrap().sequence, 1);
        let handle = world.handle(snapshot.instances[0].id).unwrap();
        assert_eq!(
            world.instance(handle).unwrap().local(42).unwrap(),
            &Value::Number { bits }
        );
        assert_eq!(
            world.instance(handle).unwrap().local(90).unwrap(),
            &Value::Reference {
                value: ReferenceValue::Live { id: reference }
            }
        );
    }
}
