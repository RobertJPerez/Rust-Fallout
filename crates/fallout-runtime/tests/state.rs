mod common;
use common::*;
use fallout_runtime::{
    Error, Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{Owner, ReferenceValue, Value},
    schema::{self, Kind},
    snapshot::Snapshot,
    state::initialization,
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

fn initialization_world(
    catalogue: &fallout_data::loaded_scripts::Catalogue,
    limits: Limits,
) -> World<'_> {
    let mut world = World::with_campaign(
        catalogue,
        limits,
        fallout_runtime::identity::CampaignId::from_bytes([0x18; 16]).unwrap(),
    )
    .unwrap();
    world.register_reference(Some(form(0x100))).unwrap();
    world
}

fn initializer(world: &World<'_>) -> (Context, Vec<(u32, Value)>) {
    let reference = world.snapshot().references[0].id;
    (
        Context {
            calling_reference: Some(reference),
            containing_reference: Some(reference),
            target: Some(ReferenceValue::Live { id: reference }),
            arguments: vec![
                ReferenceValue::Null,
                ReferenceValue::Content { key: form(0x100) },
            ],
        },
        vec![
            (
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            ),
            (
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: reference },
                },
            ),
        ],
    )
}

#[test]
fn instance_initialization_is_one_owned_atomic_publication_with_omitted_locals_uninitialized() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let (mut context, mut assignments) = initializer(&world);
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &assignments,
            initialization::Limits::default(),
        )
        .unwrap();
    assert_eq!(stage.base_revision(), before.state_revision);
    assert_eq!(stage.definition(), &definition(&catalogue));
    assert_eq!(stage.owner(), &owner(1));
    assert_eq!(stage.context(), &context);
    assert_eq!(stage.locals().len(), 5);
    assert_eq!(stage.locals()[&0], Value::Uninitialized);
    assert_eq!(stage.locals()[&2], Value::Uninitialized);
    assert_eq!(stage.locals()[&99], Value::Uninitialized);
    assert_eq!(world.snapshot(), before);
    drop(stage);
    assert_eq!(world.snapshot(), before);
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &assignments,
            initialization::Limits::default(),
        )
        .unwrap();
    let captured_context = context.clone();
    let reference = before.references[0].id;
    assignments[0].1 = Value::Number { bits: 0 };
    context.arguments.clear();
    let (receipt, handle) = world.commit_instance_initialization(stage).unwrap();
    assert_eq!(receipt.campaign, before.campaign);
    assert_eq!(receipt.catalogue_sha256, before.catalogue_sha256);
    assert_eq!(receipt.owner, owner(1));
    assert_eq!(receipt.before_revision, before.state_revision);
    assert_eq!(receipt.after_revision, before.state_revision + 1);
    assert_eq!(receipt.instance.0.get(), before.next_instance);
    assert_eq!(receipt.next_instance_before, before.next_instance);
    assert_eq!(receipt.next_instance_after, before.next_instance + 1);
    assert_eq!(receipt.explicit_assignments, 2);
    assert_eq!(receipt.initialized_locals, 2);
    assert_eq!(receipt.uninitialized_locals, 3);
    assert_eq!(world.owner_instance(&owner(1)), Some(receipt.instance));
    let instance = world.instance(handle).unwrap();
    assert_eq!(instance.context(), &captured_context);
    let mut expected = before.clone();
    expected.next_instance += 1;
    expected.state_revision += 1;
    expected
        .instances
        .push(fallout_runtime::snapshot::ScriptInstance {
            id: receipt.instance,
            definition: definition(&catalogue),
            owner: owner(1),
            context: captured_context,
            locals: vec![
                (0, Value::Uninitialized),
                (2, Value::Uninitialized),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9abc,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    },
                ),
                (99, Value::Uninitialized),
            ]
            .into_iter()
            .map(|(index, value)| fallout_runtime::snapshot::Local { index, value })
            .collect(),
        });
    assert_eq!(world.snapshot(), expected);
    for index in [0, 2, 99] {
        assert!(
            matches!(world.instance(handle).unwrap().local(index), Err(Error::UninitializedLocal(i)) if i == index)
        );
    }
    world.enqueue(handle, block(), Context::default()).unwrap(); // shared source block schema remains usable
}

#[test]
fn invalid_initializers_owner_context_and_source_leave_no_instance_or_consumed_id() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let (context, values) = initializer(&world);
    let good = values[0].clone();
    for (batch, reason) in [
        (vec![good.clone(), (90, Value::Number { bits: 7 })], "kind"),
        (vec![good.clone(), good.clone()], "duplicate"),
        (
            vec![good.clone(), (500, Value::Number { bits: 7 })],
            "missing",
        ),
        (
            vec![good.clone(), (99, Value::Number { bits: 7 })],
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
    ] {
        let error = world
            .stage_instance_initialization(
                &definition(&catalogue),
                &owner(1),
                &context,
                &batch,
                initialization::Limits::default(),
            )
            .unwrap_err();
        match reason {
            "kind" => assert!(matches!(error, Error::IncompatibleLocal(90))),
            "duplicate" => {
                assert!(matches!(error, Error::Invalid(s) if s == "duplicate local assignment"))
            }
            "missing" => assert!(matches!(error, Error::MissingLocal(500))),
            "unsupported" => assert!(matches!(error, Error::UnsupportedLocal(99))),
            "reference" => assert!(matches!(error, Error::MissingReference)),
            _ => unreachable!(),
        }
        assert_eq!(world.snapshot(), before);
        assert!(world.owner_instance(&owner(1)).is_none());
    }
    let missing = fallout_runtime::identity::ReferenceId(999.try_into().unwrap());
    assert!(matches!(
        world.stage_instance_initialization(
            &definition(&catalogue),
            &Owner::Placed { reference: missing },
            &context,
            &values,
            initialization::Limits::default()
        ),
        Err(Error::MissingReference)
    ));
    let invalid_context = Context {
        calling_reference: Some(missing),
        ..Context::default()
    };
    assert!(matches!(
        world.stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &invalid_context,
            &values,
            initialization::Limits::default()
        ),
        Err(Error::MissingReference)
    ));
    let mut changed = definition(&catalogue);
    changed.version_sha256 = "0".repeat(64);
    assert!(matches!(
        world.stage_instance_initialization(
            &changed,
            &owner(1),
            &context,
            &values,
            initialization::Limits::default()
        ),
        Err(Error::DefinitionChanged)
    ));
    assert_eq!(world.snapshot(), before);
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits::default(),
        )
        .unwrap();
    world.commit_instance_initialization(stage).unwrap();
    let exact = world.snapshot();
    assert!(
        matches!(world.stage_instance_initialization(&definition(&catalogue), &owner(1), &context, &values, initialization::Limits::default()), Err(Error::Invalid(s)) if s == "owner already has a live script instance")
    );
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn initializer_admission_bounds_physical_source_and_owned_values_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let world = initialization_world(&catalogue, Limits::default());
    let (context, values) = initializer(&world);
    let before = world.snapshot();
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits::default(),
        )
        .unwrap();
    let exact = stage.charged_bytes();
    drop(stage);
    for (limits, reason) in [
        (
            initialization::Limits {
                max_assignments: 1,
                ..Default::default()
            },
            "instance initializer assignments",
        ),
        (
            initialization::Limits {
                max_source_declarations: 5,
                ..Default::default()
            },
            "instance source declarations",
        ), // six physical, five canonical
        (
            initialization::Limits {
                max_source_references: 5,
                ..Default::default()
            },
            "instance source references",
        ),
        (
            initialization::Limits {
                max_compiled_bytes: 13,
                ..Default::default()
            },
            "instance compiled bytes",
        ),
        (
            initialization::Limits {
                max_event_blocks: 0,
                ..Default::default()
            },
            "instance source event blocks",
        ),
        (
            initialization::Limits {
                max_copied_bytes: exact - 1,
                ..Default::default()
            },
            "instance initialization copied bytes",
        ),
    ] {
        assert!(
            matches!(world.stage_instance_initialization(&definition(&catalogue), &owner(1), &context, &values, limits), Err(Error::Capacity(code)) if code == reason)
        );
        assert_eq!(world.snapshot(), before);
    }
    let admitted = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits {
                max_copied_bytes: exact,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(admitted.charged_bytes(), exact);
    drop(admitted);
    assert_eq!(world.snapshot(), before);
    for (limits, reason) in [
        (
            Limits {
                max_instances: 0,
                ..Default::default()
            },
            "script instances",
        ),
        (
            Limits {
                max_locals: 4,
                ..Default::default()
            },
            "local variables",
        ),
        (
            Limits {
                max_event_blocks: 0,
                ..Default::default()
            },
            "compiled event blocks",
        ),
    ] {
        let restricted = initialization_world(&catalogue, limits);
        let exact = restricted.snapshot();
        assert!(
            matches!(restricted.stage_instance_initialization(&definition(&catalogue), &owner(1), &context, &values, initialization::Limits::default()), Err(Error::Capacity(code)) if code == reason)
        );
        assert_eq!(restricted.snapshot(), exact);
    }
}

#[test]
fn initializer_stages_reject_mutation_restore_campaign_and_changed_source_exactly() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    for mutation in 0..3 {
        let mut world = initialization_world(&catalogue, Limits::default());
        let (context, values) = initializer(&world);
        let stage = world
            .stage_instance_initialization(
                &definition(&catalogue),
                &owner(1),
                &context,
                &values,
                initialization::Limits::default(),
            )
            .unwrap();
        match mutation {
            0 => {
                world.register_reference(None).unwrap();
            }
            1 => {
                world
                    .advance_clocks(Clocks {
                        tick: 1,
                        ..Clocks::default()
                    })
                    .unwrap();
            }
            _ => {
                world
                    .initialize_inventory(world.snapshot().references[0].id)
                    .unwrap();
            }
        }
        let exact = world.snapshot();
        assert!(world.commit_instance_initialization(stage).is_err());
        assert_eq!(world.snapshot(), exact);
    }
    let mut world = initialization_world(&catalogue, Limits::default());
    let (context, values) = initializer(&world);
    let before = world.snapshot();
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits::default(),
        )
        .unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(matches!(
        world.commit_instance_initialization(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), before);
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits::default(),
        )
        .unwrap();
    let mut other_campaign = before.clone();
    other_campaign.campaign =
        fallout_runtime::identity::CampaignId::from_bytes([0x19; 16]).unwrap();
    let mut other = World::restore(&catalogue, other_campaign.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.commit_instance_initialization(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), other_campaign);
    let changed = load(dir.path(), &["FalloutNV.esm", "Other.esm"]);
    let mut other = initialization_world(&changed, Limits::default());
    let exact = other.snapshot();
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits::default(),
        )
        .unwrap();
    assert!(matches!(
        other.commit_instance_initialization(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), exact);
}

#[test]
fn empty_initializer_creates_uninitialized_storage_and_competing_stages_have_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), true);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let mut world = initialization_world(&catalogue, Limits::default());
    let before = world.snapshot();
    let a = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &Context::default(),
            &[],
            initialization::Limits::default(),
        )
        .unwrap();
    let b = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(2),
            &Context::default(),
            &[],
            initialization::Limits::default(),
        )
        .unwrap();
    let (receipt, handle) = world.commit_instance_initialization(a).unwrap();
    let exact = world.snapshot();
    assert_eq!(receipt.after_revision, before.state_revision + 1);
    assert_eq!(receipt.explicit_assignments, 0);
    assert_eq!(receipt.initialized_locals, 0);
    assert_eq!(receipt.uninitialized_locals, 5);
    assert!(
        world
            .instance(handle)
            .unwrap()
            .locals()
            .values()
            .all(|value| *value == Value::Uninitialized)
    );
    assert!(world.commit_instance_initialization(b).is_err());
    assert_eq!(world.snapshot(), exact);
    world.remove_instance(handle).unwrap();
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(2),
            &Context::default(),
            &[],
            initialization::Limits::default(),
        )
        .unwrap();
    let (next, new) = world.commit_instance_initialization(stage).unwrap();
    assert!(next.instance > receipt.instance);
    assert!(matches!(world.instance(handle), Err(Error::StaleHandle)));
    assert_eq!(world.instance(new).unwrap().id(), next.instance);
}

#[test]
fn exhausted_initializer_allocator_or_revision_cannot_publish_owner_local_or_schema_authority() {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    for exhausted_id in [false, true] {
        let mut world = initialization_world(&catalogue, Limits::default());
        let mut exact = world.snapshot();
        if exhausted_id {
            exact.next_instance = u64::MAX;
        } else {
            exact.state_revision = u64::MAX;
        }
        world.replace_from_snapshot(exact.clone()).unwrap();
        let (context, values) = initializer(&world);
        let stage = world
            .stage_instance_initialization(
                &definition(&catalogue),
                &owner(1),
                &context,
                &values,
                initialization::Limits::default(),
            )
            .unwrap();
        assert!(
            matches!(world.commit_instance_initialization(stage), Err(Error::Capacity(code)) if code == if exhausted_id { "instance identities" } else { "state revisions" })
        );
        assert_eq!(world.snapshot(), exact);
        assert!(world.owner_instance(&owner(1)).is_none());
    }
}

#[test]
fn initialized_source_instance_native_worker_and_fresh_consumer_preserve_all_explicit_state() {
    use fallout_runtime::save::{Captured, Recovery, Repository, SaveStatus, SaveWorker};
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_INSTANCE_INITIALIZATION_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    write_fixture(root, true);
    let source_bytes = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    let catalogue = load(root, &["FalloutNV.esm"]);
    let mut world = initialization_world(&catalogue, Limits::default());
    let (context, values) = initializer(&world);
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &owner(1),
            &context,
            &values,
            initialization::Limits::default(),
        )
        .unwrap();
    let (receipt, handle) = world.commit_instance_initialization(stage).unwrap();
    world.enqueue(handle, block(), context).unwrap();
    let expected = world.snapshot();
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let done = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    world
        .assign(handle, &[(42, Value::Number { bits: 0 })])
        .unwrap();
    worker.finish().unwrap();
    assert_eq!(done.wait().unwrap().metadata.generation, 1);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected
    );
    std::fs::write(
        root.join("expected.snapshot.json"),
        expected.encode(1 << 20).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("initialization.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_instance_initialization_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_INSTANCE_INITIALIZATION_COLD_ROOT", root)
        .output()
        .unwrap();
    std::fs::write(root.join("cold.stdout.txt"), &result.stdout).unwrap();
    std::fs::write(root.join("cold.stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read(root.join("FalloutNV.esm")).unwrap(),
        source_bytes
    );
}

#[test]
#[ignore = "fresh child source-initialization consumer selected explicitly by parent"]
fn cold_instance_initialization_helper() {
    use fallout_runtime::save::{Recovery, Repository};
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_INSTANCE_INITIALIZATION_COLD_ROOT").unwrap(),
    );
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join("saved"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.metadata.generation, 1);
    let expected = Snapshot::decode(
        &std::fs::read(root.join("expected.snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    let id = expected.instances[0].id;
    assert_eq!(world.owner_instance(&owner(1)), Some(id));
    let instance = world.instance(world.handle(id).unwrap()).unwrap();
    assert_eq!(
        instance.local(42).unwrap(),
        &Value::Number {
            bits: 0x7ff8_1234_5678_9abc
        }
    );
    for index in [0, 2, 99] {
        assert_eq!(instance.locals()[&index], Value::Uninitialized);
    }
    assert_eq!(
        world
            .resolve_script_reference(world.handle(id).unwrap(), 1, None)
            .unwrap(),
        ReferenceValue::Live {
            id: expected.references[0].id
        }
    );
    assert_eq!(
        world.pending_events().next().unwrap().context,
        expected.instances[0].context
    );
    std::fs::write(
        root.join("cold.snapshot.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    println!(
        "atomic explicit source initializer: exact owner/IDs/typed bits/uninitialized omitted fields/context/compiled block and pending journal restored before consumer"
    );
}

fn assignment_group_fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("FalloutNV.esm"),
        [
            header(&[]),
            record(
                b"SCPT",
                0x300,
                0,
                &unit(
                    &[
                        (42, 0),
                        (2, 1),
                        (90, 0),
                        (91, 0),
                        (92, 0),
                        (93, 0),
                        (94, 1),
                        (42, 1),
                        (99, 7),
                        (0, 0),
                    ],
                    &[(b"SCRV", 90), (b"SCRV", 91), (b"SCRV", 92)],
                ),
            ),
            record(
                b"SCPT",
                0x301,
                0,
                &unit(
                    &[
                        (7, 1),
                        (15, 0),
                        (20, 0),
                        (21, 0),
                        (22, 0),
                        (23, 1),
                        (99, 9),
                        (0, 1),
                    ],
                    &[(b"SCRV", 20), (b"SCRV", 21), (b"SCRV", 22)],
                ),
            ),
            record(b"ACTI", 0x100, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn assignment_group_host(
    catalogue: &fallout_data::loaded_scripts::Catalogue,
) -> (World<'_>, [fallout_runtime::state::InstanceHandle; 2]) {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        fallout_runtime::identity::CampaignId::from_bytes([43; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(None).unwrap();
    let mut handles = Vec::new();
    for (position, (_, script)) in catalogue.iter().enumerate() {
        let context = Context {
            calling_reference: Some(reference),
            containing_reference: Some(reference),
            target: Some(if position == 0 {
                ReferenceValue::Content { key: form(0x100) }
            } else {
                ReferenceValue::Live { id: reference }
            }),
            arguments: vec![
                ReferenceValue::Null,
                ReferenceValue::Content {
                    key: form(if position == 0 { 0x300 } else { 0x100 }),
                },
                ReferenceValue::Live { id: reference },
            ],
        };
        let handle = world
            .create_instance(script.handle(), owner(position as u64 + 1), context.clone())
            .unwrap();
        world.enqueue(handle, block(), context).unwrap();
        handles.push(handle);
    }
    (world, handles.try_into().unwrap())
}
fn assignment_group_values(
    reference: fallout_runtime::identity::ReferenceId,
) -> [Vec<(u32, Value)>; 2] {
    [
        vec![
            (
                2,
                Value::Number {
                    bits: 0x8000000000000000,
                },
            ),
            (
                42,
                Value::Number {
                    bits: 0x7ff8123456789abc,
                },
            ),
            (
                90,
                Value::Reference {
                    value: ReferenceValue::Null,
                },
            ),
            (
                91,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x100) },
                },
            ),
            (
                92,
                Value::Reference {
                    value: ReferenceValue::Live { id: reference },
                },
            ),
            (93, Value::Uninitialized),
            (99, Value::Uninitialized),
            (0, Value::Uninitialized),
        ],
        vec![
            (
                7,
                Value::Number {
                    bits: 0x7ff8123456789abd,
                },
            ),
            (
                15,
                Value::Number {
                    bits: 0x8000000000000000,
                },
            ),
            (
                20,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x300) },
                },
            ),
            (
                21,
                Value::Reference {
                    value: ReferenceValue::Live { id: reference },
                },
            ),
            (
                22,
                Value::Reference {
                    value: ReferenceValue::Null,
                },
            ),
            (99, Value::Uninitialized),
            (0, Value::Uninitialized),
        ],
    ]
}
fn assignment_group_requests<'a>(
    handles: [fallout_runtime::state::InstanceHandle; 2],
    values: &'a [Vec<(u32, Value)>; 2],
) -> [fallout_runtime::state::assignment_group::Request<'a>; 2] {
    // Deliberately retain caller order distinct from slot/persistent-id order.
    [
        fallout_runtime::state::assignment_group::Request {
            instance: handles[1],
            assignments: &values[1],
        },
        fallout_runtime::state::assignment_group::Request {
            instance: handles[0],
            assignments: &values[0],
        },
    ]
}

#[test]
fn local_assignment_group_closes_partial_cross_instance_writes_and_preserves_journal() {
    use fallout_runtime::state::assignment_group as group;
    let dir = tempfile::tempdir().unwrap();
    assignment_group_fixture(dir.path());
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let (mut legacy, handles) = assignment_group_host(&catalogue);
    let before = legacy.snapshot();
    legacy
        .assign(handles[0], &[(2, Value::Number { bits: 123 })])
        .unwrap();
    assert!(matches!(
        legacy.assign(
            handles[1],
            &[(
                7,
                Value::Reference {
                    value: ReferenceValue::Null
                }
            )]
        ),
        Err(Error::IncompatibleLocal(7))
    ));
    assert_ne!(legacy.snapshot(), before); // The old host loop already published row one.
    assert_eq!(legacy.snapshot().instances[1], before.instances[1]);
    let (mut world, handles) = assignment_group_host(&catalogue);
    let before = world.snapshot();
    let first = [(2, Value::Number { bits: 123 })];
    let bad = [(
        7,
        Value::Reference {
            value: ReferenceValue::Null,
        },
    )];
    assert!(
        world
            .stage_local_assignments(
                &[
                    group::Request {
                        instance: handles[0],
                        assignments: &first
                    },
                    group::Request {
                        instance: handles[1],
                        assignments: &bad
                    },
                ],
                group::Limits::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    let values = assignment_group_values(before.references[0].id);
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default(),
        )
        .unwrap();
    assert_eq!(stage.rows()[0].instance(), before.instances[1].id);
    assert_eq!(stage.rows()[1].instance(), before.instances[0].id);
    assert_eq!(stage.rows()[0].assignments(), values[1]);
    assert_eq!(world.snapshot(), before);
    let receipt = world.commit_local_assignments(stage).unwrap();
    assert_eq!(receipt.before_revision(), 5);
    assert_eq!(receipt.after_revision(), 6);
    assert_eq!(receipt.usage().instances, 2);
    assert_eq!(receipt.usage().assignments, 15);
    let mut expected = before.clone();
    expected.state_revision = 6;
    for (instance, values) in expected.instances.iter_mut().zip(&values) {
        for (index, value) in values {
            instance
                .locals
                .iter_mut()
                .find(|row| row.index == *index)
                .unwrap()
                .value = value.clone();
        }
    }
    assert_eq!(world.snapshot(), expected);
    assert_eq!(
        world.pending_events().collect::<Vec<_>>(),
        before.pending_events.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        world.instance(handles[0]).unwrap().locals()[&94],
        Value::Uninitialized
    );
    assert_eq!(
        world.instance(handles[1]).unwrap().locals()[&23],
        Value::Uninitialized
    );
    assert_eq!(
        receipt.rows()[0].definition(),
        world.instance(handles[1]).unwrap().definition()
    );
}

#[test]
fn local_assignment_group_refuses_bad_final_rows_and_aggregate_caps_before_effects() {
    use fallout_runtime::state::assignment_group as group;
    let dir = tempfile::tempdir().unwrap();
    assignment_group_fixture(dir.path());
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let (mut world, handles) = assignment_group_host(&catalogue);
    let before = world.snapshot();
    let good = [(2, Value::Number { bits: 123 })];
    let bad_keys = [
        (777, Value::Uninitialized),
        (
            7,
            Value::Reference {
                value: ReferenceValue::Null,
            },
        ),
        (
            20,
            Value::Reference {
                value: ReferenceValue::Live {
                    id: fallout_runtime::identity::ReferenceId(999.try_into().unwrap()),
                },
            },
        ),
        (
            20,
            Value::Reference {
                value: ReferenceValue::Content {
                    key: fallout_data::identity::FormKey {
                        origin_plugin: "BAD.ESM".into(),
                        ..form(0x100)
                    },
                },
            },
        ),
        (99, Value::Number { bits: 1 }),
        (0, Value::Number { bits: 1 }),
    ];
    for bad in &bad_keys {
        assert!(
            world
                .stage_local_assignments(
                    &[
                        group::Request {
                            instance: handles[0],
                            assignments: &good
                        },
                        group::Request {
                            instance: handles[1],
                            assignments: std::slice::from_ref(bad)
                        },
                    ],
                    group::Limits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let duplicate = [
        (7, Value::Number { bits: 1 }),
        (7, Value::Number { bits: 2 }),
    ];
    assert!(
        world
            .stage_local_assignments(
                &[
                    group::Request {
                        instance: handles[0],
                        assignments: &good
                    },
                    group::Request {
                        instance: handles[1],
                        assignments: &duplicate
                    },
                ],
                group::Limits::default()
            )
            .is_err()
    );
    assert!(
        world
            .stage_local_assignments(
                &[
                    group::Request {
                        instance: handles[0],
                        assignments: &[]
                    },
                    group::Request {
                        instance: handles[0],
                        assignments: &[]
                    },
                ],
                group::Limits::default()
            )
            .is_err()
    );
    let mut values = assignment_group_values(before.references[0].id);
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default(),
        )
        .unwrap();
    let usage = stage.usage();
    drop(stage);
    assert_eq!(world.snapshot(), before);
    for limits in [
        group::Limits {
            max_instances: 1,
            ..Default::default()
        },
        group::Limits {
            max_assignments: 14,
            ..Default::default()
        },
        group::Limits {
            max_copied_bytes: usage.copied_bytes - 1,
            ..Default::default()
        },
    ] {
        assert!(
            world
                .stage_local_assignments(&assignment_group_requests(handles, &values), limits)
                .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits {
                max_instances: 2,
                max_assignments: 15,
                max_copied_bytes: usage.copied_bytes,
            },
        )
        .unwrap();
    values[0][0].1 = Value::Number { bits: 0 }; // Owned staged values are independent of caller storage.
    world.commit_local_assignments(stage).unwrap();
    assert_eq!(
        world.instance(handles[0]).unwrap().local(2).unwrap(),
        &Value::Number {
            bits: 0x8000000000000000
        }
    );
}

#[test]
fn local_assignment_group_expires_on_mutation_restore_and_recycled_handles() {
    use fallout_runtime::state::assignment_group as group;
    let dir = tempfile::tempdir().unwrap();
    assignment_group_fixture(dir.path());
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    for mutation in 0..5 {
        let (mut world, handles) = assignment_group_host(&catalogue);
        let values = assignment_group_values(world.snapshot().references[0].id);
        let stage = world
            .stage_local_assignments(
                &assignment_group_requests(handles, &values),
                group::Limits::default(),
            )
            .unwrap();
        match mutation {
            0 => world
                .assign(handles[0], &[(2, Value::Number { bits: 17 })])
                .unwrap(),
            1 => {
                world.register_reference(None).unwrap();
            }
            2 => world
                .advance_clocks(Clocks {
                    tick: 1,
                    ..Default::default()
                })
                .unwrap(),
            3 => {
                world
                    .enqueue(
                        handles[1],
                        block(),
                        world.instance(handles[1]).unwrap().context().clone(),
                    )
                    .unwrap();
            }
            _ => {
                world.acknowledge(1).unwrap();
            }
        }
        let after = world.snapshot();
        assert!(world.commit_local_assignments(stage).is_err());
        assert_eq!(world.snapshot(), after);
    }
    let (mut world, handles) = assignment_group_host(&catalogue);
    let before = world.snapshot();
    let values = assignment_group_values(before.references[0].id);
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default(),
        )
        .unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(matches!(
        world.commit_local_assignments(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), before);
    assert!(matches!(
        world.stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default()
        ),
        Err(Error::StaleHandle)
    ));
    let current = [
        world.handle(before.instances[0].id).unwrap(),
        world.handle(before.instances[1].id).unwrap(),
    ];
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(current, &values),
            group::Limits::default(),
        )
        .unwrap();
    let mut other = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.commit_local_assignments(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), before);
    world.acknowledge(1).unwrap();
    world.acknowledge(2).unwrap();
    world.remove_instance(current[1]).unwrap();
    world
        .create_instance(
            &before.instances[1].definition,
            owner(2),
            Context::default(),
        )
        .unwrap();
    let exact = world.snapshot();
    assert!(matches!(
        world.stage_local_assignments(
            &assignment_group_requests(current, &values),
            group::Limits::default()
        ),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn local_assignment_group_empty_write_set_preserves_revision_at_exhaustion() {
    use fallout_runtime::state::assignment_group as group;
    let dir = tempfile::tempdir().unwrap();
    assignment_group_fixture(dir.path());
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    let (world, _) = assignment_group_host(&catalogue);
    let mut snapshot = world.snapshot();
    snapshot.state_revision = u64::MAX - 1;
    let mut world = World::restore(&catalogue, snapshot, Limits::default()).unwrap();
    let handles = [
        world.handle(world.snapshot().instances[0].id).unwrap(),
        world.handle(world.snapshot().instances[1].id).unwrap(),
    ];
    let values = assignment_group_values(world.snapshot().references[0].id);
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default(),
        )
        .unwrap();
    let receipt = world.commit_local_assignments(stage).unwrap();
    assert_eq!(receipt.after_revision(), u64::MAX);
    let before = world.snapshot();
    assert!(matches!(
        world.stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default()
        ),
        Err(Error::Capacity("state revisions"))
    ));
    for requests in [
        vec![],
        vec![
            group::Request {
                instance: handles[1],
                assignments: &[],
            },
            group::Request {
                instance: handles[0],
                assignments: &[],
            },
        ],
    ] {
        let stage = world
            .stage_local_assignments(&requests, group::Limits::default())
            .unwrap();
        let receipt = world.commit_local_assignments(stage).unwrap();
        assert_eq!(receipt.before_revision(), u64::MAX);
        assert_eq!(receipt.after_revision(), u64::MAX);
        assert_eq!(receipt.usage().assignments, 0);
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn local_assignment_group_native_boundaries_and_two_fresh_consumers_preserve_exact_state() {
    use fallout_runtime::{
        save::{Captured, Repository},
        state::assignment_group as group,
    };
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_LOCAL_ASSIGNMENT_GROUP_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    assignment_group_fixture(root);
    let source = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    let catalogue = load(root, &["FalloutNV.esm"]);
    let (mut world, handles) = assignment_group_host(&catalogue);
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let before = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let values = assignment_group_values(before.references[0].id);
    let stage = world
        .stage_local_assignments(
            &assignment_group_requests(handles, &values),
            group::Limits::default(),
        )
        .unwrap();
    let receipt = world.commit_local_assignments(stage).unwrap();
    let current = world.snapshot();
    let captured = Captured::at_boundary(&world);
    world
        .assign(handles[0], &[(2, Value::Number { bits: 17 })])
        .unwrap();
    repository.commit(&captured).unwrap();
    for (name, snapshot) in [
        ("before.snapshot.json", &before),
        ("current.snapshot.json", &current),
    ] {
        std::fs::write(root.join(name), snapshot.encode(1 << 20).unwrap()).unwrap();
    }
    std::fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("assignments.json"),
        serde_json::to_vec_pretty(&values).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    for mode in ["previous", "current"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_local_assignment_group_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_LOCAL_ASSIGNMENT_GROUP_COLD_ROOT", root)
            .env("FALLOUT_LOCAL_ASSIGNMENT_GROUP_COLD_MODE", mode)
            .output()
            .unwrap();
        std::fs::write(root.join(format!("cold-{mode}.stdout.txt")), &result.stdout).unwrap();
        std::fs::write(root.join(format!("cold-{mode}.stderr.txt")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(std::fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}

#[test]
#[ignore = "fresh exact local-assignment boundary selected by parent or actual CLI proof"]
fn cold_local_assignment_group_helper() {
    use fallout_runtime::save::{Captured, Recovery, Repository, format};
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_LOCAL_ASSIGNMENT_GROUP_COLD_ROOT").unwrap(),
    );
    let mode = std::env::var("FALLOUT_LOCAL_ASSIGNMENT_GROUP_COLD_MODE").unwrap();
    let before = mode == "previous" || mode.ends_with("before");
    let phase = if before { "before" } else { "current" };
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let expected = Snapshot::decode(
        &std::fs::read(root.join(format!("{phase}.snapshot.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let native = root.join(if mode.starts_with("cli-") {
        "cold-native"
    } else {
        "saved"
    });
    let world = if mode.starts_with("cli-save-") {
        let world = World::restore(&catalogue, expected.clone(), Limits::default()).unwrap();
        let repository = if before {
            Repository::create(&native, &[], world.campaign()).unwrap()
        } else {
            Repository::open(&native, &[]).unwrap()
        };
        repository.commit(&Captured::at_boundary(&world)).unwrap();
        world
    } else if before {
        World::restore(
            &catalogue,
            format::decode(
                &std::fs::read(native.join("previous.frsv")).unwrap(),
                Limits::default(),
            )
            .unwrap()
            .snapshot,
            Limits::default(),
        )
        .unwrap()
    } else {
        Repository::open(&native, &[])
            .unwrap()
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
    };
    assert_eq!(world.snapshot(), expected);
    std::fs::write(
        root.join(format!("cold-{mode}.snapshot.json")),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
}

fn journal_host(
    catalogue: &fallout_data::loaded_scripts::Catalogue,
) -> (World<'_>, fallout_runtime::state::InstanceHandle) {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        fallout_runtime::identity::CampaignId::from_bytes([44; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(None).unwrap();
    let a = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Content { key: form(0x300) }),
        arguments: vec![
            ReferenceValue::Null,
            ReferenceValue::Content { key: form(0x300) },
            ReferenceValue::Live { id: reference },
        ],
    };
    let b = Context {
        calling_reference: None,
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![
            ReferenceValue::Content { key: form(0x300) },
            ReferenceValue::Null,
            ReferenceValue::Live { id: reference },
            ReferenceValue::Content { key: form(0x300) },
        ],
    };
    let c = Context {
        calling_reference: Some(reference),
        containing_reference: None,
        target: Some(ReferenceValue::Null),
        arguments: vec![
            ReferenceValue::Live { id: reference },
            ReferenceValue::Content { key: form(0x300) },
            ReferenceValue::Null,
        ],
    };
    let handle = world
        .create_instance(&definition(catalogue), owner(1), a.clone())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 7,
            game_nanoseconds: 100,
            menu_nanoseconds: 200,
            real_nanoseconds: 300,
        })
        .unwrap();
    world
        .enqueue(handle, Trigger::ObjectEvent { mask: 0x80000001 }, a.clone())
        .unwrap();
    world.enqueue(handle, block(), b).unwrap();
    world
        .advance_clocks(Clocks {
            tick: 9,
            game_nanoseconds: 101,
            menu_nanoseconds: 202,
            real_nanoseconds: 303,
        })
        .unwrap();
    world
        .enqueue(handle, Trigger::ObjectEvent { mask: 0xdeadbeef }, c)
        .unwrap();
    world.enqueue(handle, block(), a).unwrap();
    (world, handle)
}
fn collect_journal_pages(
    world: &World<'_>,
    rows: usize,
) -> Vec<fallout_runtime::state::journal::Page> {
    use fallout_runtime::state::journal;
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = world
            .pending_page(
                journal::Request {
                    after: cursor.as_ref(),
                    start_after: None,
                    rows,
                },
                journal::Limits::default(),
            )
            .unwrap();
        let complete = page.is_complete();
        cursor = Some(page.cursor().clone());
        pages.push(page);
        if complete {
            return pages;
        }
    }
}

#[test]
fn journal_pages_retain_literal_mixed_fifo_contexts_clocks_and_owned_history() {
    use fallout_runtime::state::journal;
    let root = tempfile::tempdir().unwrap();
    assignment_group_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let (mut world, handle) = journal_host(&catalogue);
    let before = world.snapshot();
    let pages = collect_journal_pages(&world, 2);
    assert_eq!(pages.len(), 2);
    assert_eq!(
        pages
            .iter()
            .flat_map(|page| page.events().iter())
            .cloned()
            .collect::<Vec<_>>(),
        before.pending_events
    );
    assert_eq!(pages[0].head(), Some(1));
    assert_eq!(pages[0].start_after(), None);
    assert_eq!(pages[1].start_after(), Some(2));
    assert_eq!(pages[0].campaign().bytes(), [44; 16]);
    assert_eq!(pages[0].revision(), 8);
    assert_eq!(pages[0].boundary().tick, 9);
    assert_eq!(
        pages[0].events()[0].trigger,
        Trigger::ObjectEvent { mask: 0x80000001 }
    );
    assert_eq!(
        pages[1].events()[0].trigger,
        Trigger::ObjectEvent { mask: 0xdeadbeef }
    );
    assert_eq!(pages[0].events()[1].trigger, block());
    assert_eq!(pages[1].events()[1].trigger, block());
    assert_eq!(pages[0].events()[0].arrived.tick, 7);
    assert_eq!(pages[1].events()[0].arrived.tick, 9);
    assert_eq!(pages[0].usage().arguments, 7);
    assert_eq!(pages[0].usage().source_keys, 4);
    assert_eq!(pages[0].usage().source_key_bytes, 180);
    assert_eq!(pages[1].usage().arguments, 6);
    assert_eq!(pages[1].usage().source_keys, 3);
    assert_eq!(pages[1].usage().source_key_bytes, 135);
    assert!(pages[0].next_cursor().is_some());
    assert!(pages[1].next_cursor().is_none());
    assert_eq!(world.snapshot(), before);
    let serialized = serde_json::to_value(&pages).unwrap();
    assert!(serialized[0].get("cursor").is_none());
    world
        .assign(handle, &[(2, Value::Number { bits: 1 })])
        .unwrap();
    assert_eq!(serde_json::to_value(&pages).unwrap(), serialized);
    assert!(
        world
            .pending_page(
                journal::Request {
                    after: Some(pages[0].cursor()),
                    start_after: None,
                    rows: 2
                },
                journal::Limits::default()
            )
            .is_err()
    );
}

#[test]
fn journal_pages_admit_whole_requested_slice_and_bound_sequence_search_and_copies() {
    use fallout_runtime::state::journal;
    let root = tempfile::tempdir().unwrap();
    assignment_group_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let (world, _) = journal_host(&catalogue);
    let before = world.snapshot();
    let request = journal::Request {
        after: None,
        start_after: None,
        rows: 2,
    };
    let first = world
        .pending_page(request, journal::Limits::default())
        .unwrap();
    let usage = first.usage();
    for limits in [
        journal::Limits {
            max_visited: 1,
            ..Default::default()
        },
        journal::Limits {
            max_rows: 1,
            ..Default::default()
        },
        journal::Limits {
            max_arguments: 6,
            ..Default::default()
        },
        journal::Limits {
            max_source_keys: 3,
            ..Default::default()
        },
        journal::Limits {
            max_source_key_bytes: 179,
            ..Default::default()
        },
        journal::Limits {
            max_copied_bytes: usage.copied_bytes - 1,
            ..Default::default()
        },
    ] {
        assert!(world.pending_page(request, limits).is_err());
        assert_eq!(world.snapshot(), before);
    }
    let exact = world
        .pending_page(
            request,
            journal::Limits {
                max_visited: 2,
                max_rows: 2,
                max_arguments: 7,
                max_source_keys: 4,
                max_source_key_bytes: 180,
                max_copied_bytes: usage.copied_bytes,
            },
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&exact).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    let anchored = world
        .pending_page(
            journal::Request {
                after: None,
                start_after: Some(2),
                rows: 2,
            },
            journal::Limits::default(),
        )
        .unwrap();
    assert_eq!(anchored.events(), &before.pending_events[2..]);
    assert_eq!(anchored.usage().visited, 4);
    assert!(anchored.is_complete());
    assert!(
        world
            .pending_page(
                journal::Request {
                    after: None,
                    start_after: Some(2),
                    rows: 2
                },
                journal::Limits {
                    max_visited: 3,
                    ..Default::default()
                }
            )
            .is_err()
    );
    for sequence in [0, 5, u64::MAX] {
        assert!(
            world
                .pending_page(
                    journal::Request {
                        after: None,
                        start_after: Some(sequence),
                        rows: 1
                    },
                    journal::Limits::default()
                )
                .is_err()
        );
    }
    assert!(
        world
            .pending_page(
                journal::Request {
                    after: Some(first.cursor()),
                    start_after: Some(2),
                    rows: 1
                },
                journal::Limits::default()
            )
            .is_err()
    );
    assert!(
        world
            .pending_page(
                journal::Request {
                    after: None,
                    start_after: None,
                    rows: 0
                },
                journal::Limits::default()
            )
            .is_err()
    );
    assert!(
        world
            .pending_page(
                journal::Request {
                    after: None,
                    start_after: None,
                    rows: usize::MAX
                },
                journal::Limits::default()
            )
            .is_err()
    );
    let all = world
        .pending_page(
            journal::Request {
                after: None,
                start_after: None,
                rows: 8,
            },
            journal::Limits::default(),
        )
        .unwrap();
    assert_eq!(all.events(), before.pending_events);
    assert_eq!(all.usage().returned, 4);
    let tail = world
        .pending_page(
            journal::Request {
                after: Some(all.cursor()),
                start_after: None,
                rows: 2,
            },
            journal::Limits::default(),
        )
        .unwrap();
    assert!(tail.events().is_empty());
    assert!(tail.is_complete());
    assert_eq!(tail.usage().visited, 0);
    assert_eq!(tail.start_after(), Some(4));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn journal_cursors_expire_on_every_mutation_restore_and_cross_world() {
    use fallout_runtime::state::journal;
    let root = tempfile::tempdir().unwrap();
    assignment_group_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    for mutation in 0..5 {
        let (mut world, handle) = journal_host(&catalogue);
        let first = world
            .pending_page(
                journal::Request {
                    after: None,
                    start_after: None,
                    rows: 2,
                },
                journal::Limits::default(),
            )
            .unwrap();
        match mutation {
            0 => {
                world.acknowledge(1).unwrap();
            }
            1 => {
                world
                    .enqueue(handle, Trigger::ObjectEvent { mask: 1 }, Context::default())
                    .unwrap();
            }
            2 => world
                .advance_clocks(Clocks {
                    tick: 10,
                    game_nanoseconds: 101,
                    menu_nanoseconds: 202,
                    real_nanoseconds: 303,
                })
                .unwrap(),
            3 => world
                .assign(handle, &[(2, Value::Number { bits: 1 })])
                .unwrap(),
            _ => {
                world.register_reference(None).unwrap();
            }
        }
        let current = world.snapshot();
        assert!(
            world
                .pending_page(
                    journal::Request {
                        after: Some(first.cursor()),
                        start_after: None,
                        rows: 2
                    },
                    journal::Limits::default()
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), current);
    }
    let (mut world, _) = journal_host(&catalogue);
    let before = world.snapshot();
    let first = world
        .pending_page(
            journal::Request {
                after: None,
                start_after: None,
                rows: 2,
            },
            journal::Limits::default(),
        )
        .unwrap();
    let other = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    assert!(matches!(
        other.pending_page(
            journal::Request {
                after: Some(first.cursor()),
                start_after: None,
                rows: 2
            },
            journal::Limits::default()
        ),
        Err(Error::StaleHandle)
    ));
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(matches!(
        world.pending_page(
            journal::Request {
                after: Some(first.cursor()),
                start_after: None,
                rows: 2
            },
            journal::Limits::default()
        ),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), before);
    for sequence in 1..=4 {
        world.acknowledge(sequence).unwrap();
    }
    let empty = world.snapshot();
    let page = world
        .pending_page(
            journal::Request {
                after: None,
                start_after: None,
                rows: 1,
            },
            journal::Limits::default(),
        )
        .unwrap();
    assert!(page.events().is_empty() && page.is_complete());
    assert_eq!(page.head(), None);
    assert_eq!(page.start_after(), None);
    assert_eq!(page.usage().returned, 0);
    let tail = world
        .pending_page(
            journal::Request {
                after: Some(page.cursor()),
                start_after: None,
                rows: 1,
            },
            journal::Limits::default(),
        )
        .unwrap();
    assert!(tail.events().is_empty());
    assert_eq!(world.snapshot(), empty);
}

#[test]
fn journal_pages_keep_wrapped_fifo_and_maximal_sequence_words_without_arithmetic() {
    use fallout_runtime::state::journal;
    let root = tempfile::tempdir().unwrap();
    assignment_group_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let (mut world, handle) = journal_host(&catalogue);
    for sequence in 1..=3 {
        world.acknowledge(sequence).unwrap();
    }
    for mask in [0x2000, 0x4000, 0x8000] {
        world
            .enqueue(handle, Trigger::ObjectEvent { mask }, Context::default())
            .unwrap();
    }
    let before = world.snapshot();
    assert_eq!(
        before
            .pending_events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![4, 5, 6, 7]
    );
    let pages = collect_journal_pages(&world, 2);
    assert_eq!(
        pages
            .iter()
            .flat_map(|page| page.events())
            .cloned()
            .collect::<Vec<_>>(),
        before.pending_events
    );
    assert_eq!(world.snapshot(), before);
    let mut maximal = before;
    maximal.next_event_sequence = u64::MAX;
    for (position, event) in maximal.pending_events.iter_mut().enumerate() {
        event.sequence = u64::MAX - 4 + position as u64;
    }
    let world = World::restore(&catalogue, maximal.clone(), Limits::default()).unwrap();
    let page = world
        .pending_page(
            journal::Request {
                after: None,
                start_after: Some(u64::MAX - 2),
                rows: 1,
            },
            journal::Limits::default(),
        )
        .unwrap();
    assert_eq!(page.events()[0].sequence, u64::MAX - 1);
    assert!(page.is_complete());
    assert_eq!(world.snapshot(), maximal);
    assert!(
        world
            .pending_page(
                journal::Request {
                    after: None,
                    start_after: Some(u64::MAX),
                    rows: 1
                },
                journal::Limits::default()
            )
            .is_err()
    );
}

#[test]
fn journal_pages_native_and_two_fresh_readers_preserve_complete_saved_journal() {
    use fallout_runtime::save::{Captured, Repository};
    let temp = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("FALLOUT_JOURNAL_PAGE_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    assignment_group_fixture(root);
    let source = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    let catalogue = load(root, &["FalloutNV.esm"]);
    let (world, _) = journal_host(&catalogue);
    let snapshot = world.snapshot();
    let pages = collect_journal_pages(&world, 2);
    assert_eq!(world.snapshot(), snapshot);
    Repository::create(&root.join("saved"), &[], world.campaign())
        .unwrap()
        .commit(&Captured::at_boundary(&world))
        .unwrap();
    std::fs::write(
        root.join("current.snapshot.json"),
        snapshot.encode(1 << 20).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("pages.json"),
        serde_json::to_vec_pretty(&pages).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    for mode in ["unit-current", "unit-anchor"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_journal_page_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_JOURNAL_PAGE_COLD_ROOT", root)
            .env("FALLOUT_JOURNAL_PAGE_COLD_MODE", mode)
            .output()
            .unwrap();
        std::fs::write(root.join(format!("cold-{mode}.stdout.txt")), &result.stdout).unwrap();
        std::fs::write(root.join(format!("cold-{mode}.stderr.txt")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(std::fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}

#[test]
#[ignore = "fresh source/native exact journal observation selected by parent or CLI proof"]
fn cold_journal_page_helper() {
    use fallout_runtime::{
        save::{Captured, Recovery, Repository},
        state::journal,
    };
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_JOURNAL_PAGE_COLD_ROOT").unwrap());
    let mode = std::env::var("FALLOUT_JOURNAL_PAGE_COLD_MODE").unwrap();
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let expected = Snapshot::decode(
        &std::fs::read(root.join("current.snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let native = root.join(if mode.starts_with("cli-") {
        "cold-native"
    } else {
        "saved"
    });
    let world = if mode == "cli-save" {
        let world = World::restore(&catalogue, expected.clone(), Limits::default()).unwrap();
        Repository::create(&native, &[], world.campaign())
            .unwrap()
            .commit(&Captured::at_boundary(&world))
            .unwrap();
        world
    } else {
        Repository::open(&native, &[])
            .unwrap()
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
    };
    let pages = collect_journal_pages(&world, 2);
    assert_eq!(world.snapshot(), expected);
    assert_eq!(
        serde_json::to_value(&pages).unwrap(),
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(root.join("pages.json")).unwrap()
        )
        .unwrap()
    );
    if mode == "unit-anchor" {
        let anchored = world
            .pending_page(
                journal::Request {
                    after: None,
                    start_after: Some(2),
                    rows: 2,
                },
                journal::Limits::default(),
            )
            .unwrap();
        assert_eq!(anchored.events(), &expected.pending_events[2..]);
        std::fs::write(
            root.join("cold-anchor.page.json"),
            serde_json::to_vec_pretty(&anchored).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        root.join(format!("cold-{mode}.snapshot.json")),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join(format!("cold-{mode}.pages.json")),
        serde_json::to_vec_pretty(&pages).unwrap(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
}

fn observation_fixture(root: &std::path::Path) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(
        root.join("FalloutNV.esm"),
        [
            header(&[]),
            record(
                b"SCPT",
                0x300,
                0,
                &unit(
                    &[
                        (42, 0),
                        (2, 1),
                        (90, 0),
                        (91, 0),
                        (92, 0),
                        (93, 0),
                        (94, 1),
                        (42, 1),
                        (99, 7),
                        (0, 0),
                    ],
                    &[(b"SCRV", 90), (b"SCRV", 91), (b"SCRV", 92)],
                ),
            ),
            record(b"ACTI", 0x100, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn observation_host(
    catalogue: &fallout_data::loaded_scripts::Catalogue,
) -> (World<'_>, fallout_runtime::state::InstanceHandle) {
    use fallout_runtime::identity::CampaignId;
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([42; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(None).unwrap();
    let handle = world
        .create_instance(
            &definition(catalogue),
            Owner::Quest { key: form(0x100) },
            Context {
                calling_reference: Some(reference),
                containing_reference: Some(reference),
                target: Some(ReferenceValue::Content { key: form(0x300) }),
                arguments: vec![
                    ReferenceValue::Null,
                    ReferenceValue::Content { key: form(0x100) },
                    ReferenceValue::Live { id: reference },
                ],
            },
        )
        .unwrap();
    (world, handle)
}
fn observation_assignments(reference: fallout_runtime::identity::ReferenceId) -> Vec<(u32, Value)> {
    vec![
        (
            2,
            Value::Number {
                bits: 0x8000000000000000,
            },
        ),
        (
            42,
            Value::Number {
                bits: 0x7ff8123456789abc,
            },
        ),
        (
            90,
            Value::Reference {
                value: ReferenceValue::Null,
            },
        ),
        (
            91,
            Value::Reference {
                value: ReferenceValue::Content { key: form(0x100) },
            },
        ),
        (
            92,
            Value::Reference {
                value: ReferenceValue::Live { id: reference },
            },
        ),
        (
            93,
            Value::Number {
                bits: 0x7ff8123456789abd,
            },
        ),
    ]
}
const OBSERVATION_INDICES: [u32; 7] = [94, 92, 2, 42, 91, 90, 93];

#[test]
fn selected_observation_retains_literal_requested_order_raw_bits_and_source_declarations() {
    use fallout_runtime::state::observation;
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    observation_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let (mut world, handle) = observation_host(&catalogue);
    let reference = world
        .instance(handle)
        .unwrap()
        .context()
        .calling_reference
        .unwrap();
    world
        .assign(handle, &observation_assignments(reference))
        .unwrap();
    let before = world.snapshot();
    let observed = world
        .observe_locals(handle, &OBSERVATION_INDICES, observation::Limits::default())
        .unwrap();
    assert_eq!(
        observed
            .rows()
            .iter()
            .map(|row| row.index())
            .collect::<Vec<_>>(),
        OBSERVATION_INDICES
    );
    assert_eq!(observed.revision(), 3);
    assert_eq!(observed.campaign(), world.campaign());
    assert_eq!(observed.definition(), &definition(&catalogue));
    assert_eq!(observed.owner(), &Owner::Quest { key: form(0x100) });
    assert_eq!(
        observed.context(),
        world.instance(handle).unwrap().context()
    );
    assert_eq!(observed.rows()[0].value(), &Value::Uninitialized);
    assert!(matches!(
        world.instance(handle).unwrap().local(94),
        Err(Error::UninitializedLocal(94))
    ));
    let rows = serde_json::to_value(observed.rows()).unwrap();
    assert_eq!(
        rows,
        json!([
            {"declaration":{"index":94,"declaration_decoded_offset":315,"kind":{"kind":"integer"}},"value":{"kind":"uninitialized"}},
            {"declaration":{"index":92,"declaration_decoded_offset":225,"kind":{"kind":"reference"}},"value":{"kind":"reference","value":{"kind":"live","id":1}}},
            {"declaration":{"index":2,"declaration_decoded_offset":91,"kind":{"kind":"integer"}},"value":{"kind":"number","bits":0x8000000000000000_u64}},
            {"declaration":{"index":42,"declaration_decoded_offset":46,"kind":{"kind":"float"}},"value":{"kind":"number","bits":0x7ff8123456789abc_u64}},
            {"declaration":{"index":91,"declaration_decoded_offset":180,"kind":{"kind":"reference"}},"value":{"kind":"reference","value":{"kind":"content","key":form(0x100)}}},
            {"declaration":{"index":90,"declaration_decoded_offset":135,"kind":{"kind":"reference"}},"value":{"kind":"reference","value":{"kind":"null"}}},
            {"declaration":{"index":93,"declaration_decoded_offset":270,"kind":{"kind":"float"}},"value":{"kind":"number","bits":0x7ff8123456789abd_u64}}
        ])
    );
    assert_eq!(world.snapshot(), before);
    let held = observed.clone();
    world
        .assign(handle, &[(42, Value::Number { bits: 0 })])
        .unwrap();
    assert_eq!(
        held.rows()[3].value(),
        &Value::Number {
            bits: 0x7ff8123456789abc
        }
    );
    assert_eq!(held.revision(), 3);
    assert_eq!(
        world
            .observe_locals(handle, &[42], observation::Limits::default())
            .unwrap()
            .revision(),
        4
    );
}

#[test]
fn selected_observation_refuses_missing_duplicate_unsupported_and_every_one_under_budget() {
    use fallout_runtime::state::observation;
    let root = tempfile::tempdir().unwrap();
    observation_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let (mut world, handle) = observation_host(&catalogue);
    world
        .assign(
            handle,
            &observation_assignments(
                1.try_into()
                    .map(fallout_runtime::identity::ReferenceId)
                    .unwrap(),
            ),
        )
        .unwrap();
    let before = world.snapshot();
    let usage = world
        .observe_locals(handle, &OBSERVATION_INDICES, observation::Limits::default())
        .unwrap()
        .usage();
    assert_eq!(usage.indices, 7);
    assert_eq!(usage.source_keys, 5);
    assert_eq!(usage.context_arguments, 3);
    assert_eq!(
        usage.source_key_bytes,
        5 * (std::mem::size_of::<fallout_data::identity::FormKey>() + 13)
    );
    let exact = observation::Limits {
        max_indices: 7,
        max_source_keys: 5,
        max_source_key_bytes: usage.source_key_bytes,
        max_context_arguments: 3,
        max_copied_bytes: usage.copied_bytes,
    };
    assert_eq!(
        world
            .observe_locals(handle, &OBSERVATION_INDICES, exact)
            .unwrap()
            .usage(),
        usage
    );
    for (indices, error) in [
        (vec![2, 42, 0], Error::UnsupportedLocal(0)),
        (vec![2, 42, 99], Error::UnsupportedLocal(99)),
        (vec![2, 42, 888], Error::MissingLocal(888)),
    ] {
        assert_eq!(
            world
                .observe_locals(handle, &indices, exact)
                .unwrap_err()
                .to_string(),
            error.to_string()
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(matches!(
        world.observe_locals(handle, &[42, 2, 42], exact),
        Err(Error::Invalid(_))
    ));
    for (limits, reason) in [
        (
            observation::Limits {
                max_indices: 6,
                ..exact
            },
            "local observation indices",
        ),
        (
            observation::Limits {
                max_source_keys: 4,
                ..exact
            },
            "local observation source keys",
        ),
        (
            observation::Limits {
                max_source_key_bytes: usage.source_key_bytes - 1,
                ..exact
            },
            "local observation source key bytes",
        ),
        (
            observation::Limits {
                max_context_arguments: 2,
                ..exact
            },
            "local observation context arguments",
        ),
        (
            observation::Limits {
                max_copied_bytes: usage.copied_bytes - 1,
                ..exact
            },
            "local observation copied bytes",
        ),
    ] {
        assert!(
            matches!(world.observe_locals(handle,&OBSERVATION_INDICES,limits),Err(Error::Capacity(actual))if actual==reason)
        );
        assert_eq!(world.snapshot(), before);
    }
    let empty = world
        .observe_locals(handle, &[], observation::Limits::default())
        .unwrap();
    assert!(empty.rows().is_empty());
    assert_eq!(empty.usage().source_keys, 4);
    assert_eq!(world.snapshot(), before);
}

#[test]
fn restored_recycled_and_other_source_handles_cannot_observe_an_instance() {
    use fallout_runtime::state::observation;
    let root = tempfile::tempdir().unwrap();
    observation_fixture(root.path());
    let catalogue = load(root.path(), &["FalloutNV.esm"]);
    let (mut world, handle) = observation_host(&catalogue);
    let initial = world.snapshot();
    let observation = world
        .observe_locals(handle, &OBSERVATION_INDICES, observation::Limits::default())
        .unwrap();
    let id = observation.instance();
    world.replace_from_snapshot(initial.clone()).unwrap();
    assert!(matches!(
        world.observe_locals(handle, &[42], observation::Limits::default()),
        Err(Error::StaleHandle)
    ));
    let current = world.handle(id).unwrap();
    assert_eq!(
        world
            .observe_locals(
                current,
                &OBSERVATION_INDICES,
                observation::Limits::default()
            )
            .unwrap(),
        observation
    );
    world.remove_instance(current).unwrap();
    let replacement = world
        .create_instance(&definition(&catalogue), owner(99), Context::default())
        .unwrap();
    let exact = world.snapshot();
    assert!(matches!(
        world.observe_locals(current, &[42], observation::Limits::default()),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), exact);
    let other = tempfile::tempdir().unwrap();
    write_fixture(other.path(), false);
    let other_source = load(other.path(), &["FalloutNV.esm"]);
    let other_world = World::new(&other_source, Limits::default()).unwrap();
    let before = other_world.snapshot();
    assert!(matches!(
        other_world.observe_locals(replacement, &[42], observation::Limits::default()),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other_world.snapshot(), before);
    assert!(matches!(
        World::restore(&other_source, initial, Limits::default()),
        Err(Error::DefinitionChanged)
    ));
}

#[test]
fn selected_local_observations_survive_native_before_current_and_two_fresh_reads() {
    use fallout_runtime::{
        save::{Captured, Repository},
        state::observation,
    };
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_LOCAL_OBSERVATION_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    observation_fixture(root);
    let source_bytes = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    let catalogue = load(root, &["FalloutNV.esm"]);
    let (mut world, handle) = observation_host(&catalogue);
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let before = world.snapshot();
    let first = world
        .observe_locals(handle, &OBSERVATION_INDICES, observation::Limits::default())
        .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    world
        .assign(
            handle,
            &observation_assignments(
                world
                    .instance(handle)
                    .unwrap()
                    .context()
                    .calling_reference
                    .unwrap(),
            ),
        )
        .unwrap();
    let current = world.snapshot();
    let second = world
        .observe_locals(handle, &OBSERVATION_INDICES, observation::Limits::default())
        .unwrap();
    let capture = Captured::at_boundary(&world);
    world
        .assign(handle, &[(42, Value::Number { bits: 0 })])
        .unwrap();
    repository.commit(&capture).unwrap();
    for (name, value) in [
        ("before.snapshot.json", &before),
        ("current.snapshot.json", &current),
    ] {
        std::fs::write(root.join(name), value.encode(1 << 20).unwrap()).unwrap();
    }
    for (name, value) in [
        ("before.observation.json", &first),
        ("current.observation.json", &second),
    ] {
        std::fs::write(root.join(name), serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }
    drop(world);
    drop(catalogue);
    for mode in ["previous", "current"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cold_local_observation_helper",
                "--ignored",
                "--nocapture",
            ])
            .env("FALLOUT_LOCAL_OBSERVATION_COLD_ROOT", root)
            .env("FALLOUT_LOCAL_OBSERVATION_COLD_MODE", mode)
            .output()
            .unwrap();
        std::fs::write(root.join(format!("cold-{mode}.stdout.txt")), &result.stdout).unwrap();
        std::fs::write(root.join(format!("cold-{mode}.stderr.txt")), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        std::fs::read(root.join("FalloutNV.esm")).unwrap(),
        source_bytes
    );
}

#[test]
#[ignore = "fresh bounded local observation child selected by parent"]
fn cold_local_observation_helper() {
    use fallout_runtime::{
        save::{Captured, Recovery, Repository, format},
        state::observation,
    };
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_LOCAL_OBSERVATION_COLD_ROOT").unwrap());
    let mode = std::env::var("FALLOUT_LOCAL_OBSERVATION_COLD_MODE").unwrap();
    let phase = if mode == "previous" {
        "before"
    } else {
        "current"
    };
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let expected = Snapshot::decode(
        &std::fs::read(root.join(format!("{phase}.snapshot.json"))).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let world = if mode == "current" || mode == "cli-native" {
        Repository::open(
            &root.join(if mode == "cli-native" {
                "cold-native"
            } else {
                "saved"
            }),
            &[],
        )
        .unwrap()
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap()
        .0
    } else if mode == "previous" {
        World::restore(
            &catalogue,
            format::decode(
                &std::fs::read(root.join("saved/previous.frsv")).unwrap(),
                Limits::default(),
            )
            .unwrap()
            .snapshot,
            Limits::default(),
        )
        .unwrap()
    } else {
        assert_eq!(mode, "cli-snapshot");
        World::restore(&catalogue, expected.clone(), Limits::default()).unwrap()
    };
    assert_eq!(world.snapshot(), expected);
    let indices = if mode.starts_with("cli-") {
        serde_json::from_str::<Vec<u32>>(
            &std::env::var("FALLOUT_LOCAL_OBSERVATION_COLD_INDICES").unwrap(),
        )
        .unwrap()
    } else {
        OBSERVATION_INDICES.to_vec()
    };
    let observed = world
        .observe_locals(
            world.handle(expected.instances[0].id).unwrap(),
            &indices,
            observation::Limits::default(),
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&observed).unwrap(),
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(root.join(format!("{phase}.observation.json"))).unwrap()
        )
        .unwrap()
    );
    assert_eq!(world.snapshot(), expected);
    if mode == "cli-snapshot" {
        Repository::create(&root.join("cold-native"), &[], world.campaign())
            .unwrap()
            .commit(&Captured::at_boundary(&world))
            .unwrap();
    }
    std::fs::write(
        root.join(format!("cold-{mode}.snapshot.json")),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join(format!("cold-{mode}.observation.json")),
        serde_json::to_vec_pretty(&observed).unwrap(),
    )
    .unwrap();
}
