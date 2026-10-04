use super::*;
use crate::{Limits as WorldLimits, events::Clocks, identity::ReferenceId, save::Captured};
use fallout_data::{loaded_scripts::Catalogue, store::RecordStore};

#[test]
fn borrowed_group_bytes_refuse_malformed_final_quest_target_and_argument_before_name_validation() {
    use crate::identity::ReferenceValue;
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(dir.path());
    let world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let owners = owners();
    let context = Context::default();
    let valid = [
        Request {
            definition: &handles[0],
            owner: &owners[0],
            context: &context,
            assignments: &[],
        },
        Request {
            definition: &handles[1],
            owner: &owners[1],
            context: &context,
            assignments: &[],
        },
    ];
    let admitted = world
        .stage_instance_initialization_group(&valid, Limits::default())
        .unwrap()
        .usage()
        .copied_bytes;
    let before = unchanged(&world);
    let mut key = handles[0].key.record.clone();
    key.origin_plugin = "BAD/".to_string() + &"x".repeat(8192);
    let quest = Owner::Quest { key: key.clone() };
    let target = Context {
        target: Some(ReferenceValue::Content { key: key.clone() }),
        ..Default::default()
    };
    let arguments = Context {
        arguments: vec![ReferenceValue::Content { key }],
        ..Default::default()
    };
    for (owner, context) in [
        (&quest, &context),
        (&owners[1], &target),
        (&owners[1], &arguments),
    ] {
        let requests = [
            Request {
                definition: &handles[0],
                owner: &owners[0],
                context: valid[0].context,
                assignments: &[],
            },
            Request {
                definition: &handles[1],
                owner,
                context,
                assignments: &[],
            },
        ];
        let error = world
            .stage_instance_initialization_group(
                &requests,
                Limits {
                    max_copied_bytes: admitted,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error,
            Error::Capacity("instance initialization copied bytes")
        ));
        assert_eq!(unchanged(&world), before);
        // With complete admission, canonical validation still rejects the same
        // supplied malformed name under the original identity rules.
        assert!(matches!(
            world.stage_instance_initialization_group(&requests, Limits::default()),
            Err(Error::Invalid(_))
        ));
        assert_eq!(unchanged(&world), before);
    }
    assert!(world.definitions.is_empty());
    assert!(world.slots.is_empty());
    assert_eq!(world.next_instance, 1);
}
#[test]
fn aggregate_group_assignment_and_argument_caps_precede_malformed_element_traversal() {
    use crate::identity::ReferenceValue;
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(dir.path());
    let world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let owners = owners();
    let mut key = handles[0].key.record.clone();
    key.origin_plugin = "BAD/".to_string() + &"x".repeat(8192);
    let malformed = ReferenceValue::Content { key };
    let context = Context {
        arguments: vec![malformed.clone()],
        ..Default::default()
    };
    let empty = Context::default();
    let values = [(42, Value::Reference { value: malformed })];
    let first_values = [(
        42,
        Value::Number {
            bits: 0x8000_0000_0000_0000,
        },
    )];
    let requests = [
        Request {
            definition: &handles[0],
            owner: &owners[0],
            context: &empty,
            assignments: &first_values,
        },
        Request {
            definition: &handles[1],
            owner: &owners[1],
            context: &context,
            assignments: &values,
        },
    ];
    let before = unchanged(&world);
    assert!(matches!(
        world.stage_instance_initialization_group(
            &requests,
            Limits {
                max_assignments: 1,
                ..Default::default()
            }
        ),
        Err(Error::Capacity("instance initialization group assignments"))
    ));
    assert_eq!(unchanged(&world), before);
    assert!(matches!(
        world.stage_instance_initialization_group(
            &requests,
            Limits {
                max_context_arguments: 0,
                ..Default::default()
            }
        ),
        Err(Error::Capacity(
            "instance initialization group context arguments"
        ))
    ));
    assert_eq!(unchanged(&world), before);
}
#[test]
fn admitted_quest_target_and_argument_names_keep_exact_charges_and_one_revision() {
    use crate::identity::ReferenceValue;
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(dir.path());
    let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let owners = [
        owners()[0].clone(),
        Owner::Quest {
            key: handles[0].key.record.clone(),
        },
    ];
    let context = Context {
        target: Some(ReferenceValue::Content {
            key: handles[1].key.record.clone(),
        }),
        arguments: vec![
            ReferenceValue::Content {
                key: handles[0].key.record.clone(),
            },
            ReferenceValue::Null,
        ],
        ..Default::default()
    };
    let values = [(
        42,
        Value::Number {
            bits: 0x7ff8_1234_5678_9abc,
        },
    )];
    let requests = [
        Request {
            definition: &handles[0],
            owner: &owners[0],
            context: &context,
            assignments: &values,
        },
        Request {
            definition: &handles[1],
            owner: &owners[1],
            context: &context,
            assignments: &values,
        },
    ];
    let usage = world
        .stage_instance_initialization_group(&requests, Limits::default())
        .unwrap()
        .usage();
    let before = unchanged(&world);
    let exact = Limits {
        max_copied_bytes: usage.copied_bytes,
        ..Default::default()
    };
    assert!(matches!(
        world.stage_instance_initialization_group(
            &requests,
            Limits {
                max_copied_bytes: usage.copied_bytes - 1,
                ..exact
            }
        ),
        Err(Error::Capacity("instance initialization copied bytes"))
    ));
    assert_eq!(unchanged(&world), before);
    let stage = world
        .stage_instance_initialization_group(&requests, exact)
        .unwrap();
    assert_eq!(stage.usage(), usage);
    let (receipt, handles) = world.commit_instance_initialization_group(stage).unwrap();
    assert_eq!(receipt.after_revision, receipt.before_revision + 1);
    assert_eq!(world.instance(handles[1]).unwrap().owner(), &owners[1]);
    assert_eq!(world.instance(handles[1]).unwrap().context(), &context);
    assert_eq!(
        world.instance(handles[1]).unwrap().local(42).unwrap(),
        &values[0].1
    );
    assert!(matches!(
        world.instance(handles[1]).unwrap().local(43),
        Err(Error::UninitializedLocal(43))
    ));
}

fn fixture(root: &std::path::Path) -> (Catalogue, [Handle; 2]) {
    fn field(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
        [tag.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
    }
    fn record(tag: &[u8; 4], id: u32, bytes: &[u8]) -> Vec<u8> {
        [
            tag.as_slice(),
            &(bytes.len() as u32).to_le_bytes(),
            &[0; 4],
            &id.to_le_bytes(),
            &[0; 8],
            bytes,
        ]
        .concat()
    }
    fn unit(indices: &[u32]) -> Vec<u8> {
        let compiled = [0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0];
        let mut header = [0; 20];
        header[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
        header[12..16].copy_from_slice(&(indices.len() as u32).to_le_bytes());
        let mut bytes = field(b"SCHR", &header);
        bytes.extend(field(b"SCDA", &compiled));
        for index in indices {
            let mut declaration = [0; 24];
            declaration[..4].copy_from_slice(&index.to_le_bytes());
            bytes.extend(field(b"SLSD", &declaration));
            bytes.extend(field(b"SCVR", b"explicit\0"));
        }
        bytes
    }
    std::fs::write(
        root.join("FalloutNV.esm"),
        [
            record(
                b"TES4",
                0,
                &field(
                    b"HEDR",
                    &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                ),
            ),
            record(b"SCPT", 0x300, &unit(&[42])),
            record(b"SCPT", 0x301, &unit(&[42, 43])),
        ]
        .concat(),
    )
    .unwrap();
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], Default::default()).unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let handles = catalogue
        .iter()
        .map(|(_, script)| script.handle().clone())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    (catalogue, handles)
}
fn owners() -> [Owner; 3] {
    [1_u64, 2, 3].map(|id| Owner::Fragment {
        activation: id.try_into().unwrap(),
    })
}
fn requests<'a>(
    handles: &'a [Handle; 2],
    owners: &'a [Owner; 3],
    context: &'a Context,
    values: &'a [(u32, Value)],
) -> [Request<'a>; 3] {
    std::array::from_fn(|index| Request {
        definition: &handles[usize::from(index == 2)],
        owner: &owners[index],
        context,
        assignments: values,
    })
}
fn unchanged(world: &World<'_>) -> impl PartialEq + std::fmt::Debug + use<> {
    (
        world.snapshot(),
        world.free.clone(),
        world
            .slots
            .iter()
            .map(|slot| (slot.generation, slot.value.as_ref().map(Instance::id)))
            .collect::<Vec<_>>(),
        world.local_count,
        world.block_count,
        world
            .definitions
            .iter()
            .map(|(key, value)| (key.clone(), value.locals.clone(), value.blocks.clone()))
            .collect::<Vec<_>>(),
    )
}
#[test]
fn cold_exact_handle_dedup_drop_and_caller_order_single_revision_share_only_immutable_schema() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let owners = owners();
    let context = Context::default();
    let values = [(42, Value::Number { bits: 1 << 63 })];
    let requests = requests(&handles, &owners, &context, &values);
    let before = unchanged(&world);
    let stage = world
        .stage_instance_initialization_group(&requests, Limits::default())
        .unwrap();
    let first = Arc::downgrade(&stage.rows[0].schema);
    let other = Arc::downgrade(&stage.rows[2].schema);
    assert!(Arc::ptr_eq(&stage.rows[0].schema, &stage.rows[1].schema));
    assert!(!Arc::ptr_eq(&stage.rows[0].schema, &stage.rows[2].schema));
    assert_eq!(stage.usage.unique_definitions, 2);
    assert_eq!(stage.usage.source_declarations, 3);
    assert_eq!(stage.usage.compiled_bytes, 28);
    assert_eq!(stage.usage.event_blocks, 2);
    assert_eq!(stage.usage.locals, 4);
    assert_eq!(unchanged(&world), before);
    drop(stage);
    assert!(first.upgrade().is_none() && other.upgrade().is_none());
    let stage = world
        .stage_instance_initialization_group(&requests, Limits::default())
        .unwrap();
    let (receipt, runtime) = world.commit_instance_initialization_group(stage).unwrap();
    assert_eq!(
        (
            receipt.before_revision,
            receipt.after_revision,
            receipt.next_instance_before,
            receipt.next_instance_after
        ),
        (0, 1, 1, 4)
    );
    assert_eq!(
        receipt
            .rows
            .iter()
            .map(|row| row.instance.0.get())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(world.local_count, 4);
    assert_eq!(world.block_count, 2);
    assert_eq!(world.definitions.len(), 2);
    assert_eq!(Arc::strong_count(&world.definitions[&handles[0].key]), 3);
    assert_eq!(Arc::strong_count(&world.definitions[&handles[1].key]), 2);
    assert_eq!(
        world.instance(runtime[2]).unwrap().locals[&43],
        Value::Uninitialized
    );
    let capture = Captured::at_boundary(&world);
    drop(capture);
    assert_eq!(Arc::strong_count(&world.definitions[&handles[0].key]), 3);
}
#[test]
fn whole_bounds_and_canonical_capacities_refuse_without_cache_or_allocator_effects() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    let owners = owners();
    let context = Context {
        arguments: vec![crate::identity::ReferenceValue::Null],
        ..Context::default()
    };
    let values = [(42, Value::Number { bits: 7 })];
    let requests = requests(&handles, &owners, &context, &values);
    let world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let before = unchanged(&world);
    let bytes = world
        .stage_instance_initialization_group(&requests, Limits::default())
        .unwrap()
        .usage
        .copied_bytes;
    for limits in [
        Limits {
            max_instances: 2,
            ..Default::default()
        },
        Limits {
            max_source_declarations: 2,
            ..Default::default()
        },
        Limits {
            max_compiled_bytes: 27,
            ..Default::default()
        },
        Limits {
            max_event_blocks: 1,
            ..Default::default()
        },
        Limits {
            max_assignments: 2,
            ..Default::default()
        },
        Limits {
            max_context_arguments: 2,
            ..Default::default()
        },
        Limits {
            max_copied_bytes: bytes - 1,
            ..Default::default()
        },
    ] {
        assert!(
            world
                .stage_instance_initialization_group(&requests, limits)
                .is_err()
        );
        assert_eq!(unchanged(&world), before);
    }
    for limits in [
        WorldLimits {
            max_instances: 2,
            ..Default::default()
        },
        WorldLimits {
            max_locals: 3,
            ..Default::default()
        },
        WorldLimits {
            max_event_blocks: 1,
            ..Default::default()
        },
    ] {
        let world = World::new(&catalogue, limits).unwrap();
        let before = unchanged(&world);
        assert!(
            world
                .stage_instance_initialization_group(&requests, Limits::default())
                .is_err()
        );
        assert_eq!(unchanged(&world), before);
    }
}
#[test]
fn bad_final_source_owner_context_and_value_leave_free_slots_cache_and_all_counters_exact() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    for refusal in 0..6 {
        let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
        let old = world
            .create_instance(
                &handles[0],
                Owner::Fragment {
                    activation: 99.try_into().unwrap(),
                },
                Context::default(),
            )
            .unwrap();
        world.remove_instance(old).unwrap();
        let owners = owners();
        let context = Context::default();
        let values = [(42, Value::Number { bits: 7 })];
        let mut changed = handles[1].clone();
        changed.version_sha256 = "f".repeat(64);
        let missing = ReferenceId(999.try_into().unwrap());
        let bad_owner = Owner::Placed { reference: missing };
        let bad_context = Context {
            calling_reference: Some(missing),
            ..Context::default()
        };
        let bad_values = [(
            42,
            Value::Reference {
                value: crate::identity::ReferenceValue::Null,
            },
        )];
        let missing_values = [(999, Value::Number { bits: 7 })];
        let mut requests = requests(&handles, &owners, &context, &values);
        match refusal {
            0 => requests[2].definition = &changed,
            1 => requests[2].owner = &owners[0],
            2 => requests[2].owner = &bad_owner,
            3 => requests[2].context = &bad_context,
            4 => requests[2].assignments = &bad_values,
            _ => requests[2].assignments = &missing_values,
        }
        let before = unchanged(&world);
        assert!(
            world
                .stage_instance_initialization_group(&requests, Limits::default())
                .is_err()
        );
        assert_eq!(unchanged(&world), before);
        assert!(!world.definitions.contains_key(&handles[1].key));
    }
}
#[test]
fn all_commit_bindings_final_rows_and_changed_capacities_precede_first_publication() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    for refusal in 0..14 {
        let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
        let owners = owners();
        let context = Context::default();
        let values = [(42, Value::Number { bits: 7 })];
        let mut stage = world
            .stage_instance_initialization_group(
                &requests(&handles, &owners, &context, &values),
                Limits::default(),
            )
            .unwrap();
        match refusal {
            0 => stage.epoch += 1,
            1 => stage.campaign = CampaignId::from_bytes([77; 16]).unwrap(),
            2 => stage.cohort = "f".repeat(64),
            3 => stage.revision += 1,
            4 => stage.next_instance += 1,
            5 => stage.rows[2].definition.version_sha256 = "f".repeat(64),
            6 => stage.rows[2].owner = stage.rows[0].owner.clone(),
            7 => {
                stage.rows[2].context.calling_reference = Some(ReferenceId(999.try_into().unwrap()))
            }
            8 => {
                stage.rows[2].locals.insert(
                    42,
                    Value::Reference {
                        value: crate::identity::ReferenceValue::Null,
                    },
                );
            }
            9 => {
                stage.rows[2].locals.insert(999, Value::Uninitialized);
            }
            10 => world.limits.max_instances = 2,
            11 => world.limits.max_locals = 3,
            12 => world.limits.max_event_blocks = 1,
            _ => {
                world.next_instance = u64::MAX - 1;
                stage.next_instance = world.next_instance;
            }
        }
        let before = unchanged(&world);
        assert!(world.commit_instance_initialization_group(stage).is_err());
        assert_eq!(unchanged(&world), before);
    }
}
#[test]
fn reused_warm_schema_fits_exact_cache_budget_and_reuses_existing_free_slot_generation() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    let mut world = World::new(
        &catalogue,
        WorldLimits {
            max_instances: 3,
            max_locals: 4,
            max_event_blocks: 2,
            ..Default::default()
        },
    )
    .unwrap();
    let old = world
        .create_instance(
            &handles[0],
            Owner::Fragment {
                activation: 99.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let warm = Arc::clone(&world.definitions[&handles[0].key]);
    world.remove_instance(old).unwrap();
    let owners = owners();
    let context = Context::default();
    let values = [(42, Value::Uninitialized)];
    let stage = world
        .stage_instance_initialization_group(
            &requests(&handles, &owners, &context, &values),
            Limits::default(),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&stage.rows[0].schema, &warm));
    assert!(Arc::ptr_eq(&stage.rows[1].schema, &warm));
    let (receipt, current) = world.commit_instance_initialization_group(stage).unwrap();
    assert_eq!(receipt.after_revision, 3);
    assert_eq!(receipt.next_instance_after, 5);
    assert_eq!(current[0].slot, old.slot);
    assert_eq!(current[0].generation, old.generation + 1);
    assert!(world.instance(old).is_err());
    assert_eq!(world.block_count, 2);
    assert_eq!(world.local_count, 4);
    assert!(Arc::ptr_eq(&world.definitions[&handles[0].key], &warm));
    let before = unchanged(&world);
    assert!(
        world
            .stage_instance_initialization_group(
                &requests(&handles, &owners, &context, &values),
                Limits::default()
            )
            .is_err()
    );
    assert_eq!(unchanged(&world), before);
}
#[test]
fn legacy_cache_warming_without_revision_is_rechecked_and_failed_group_drops_private_schema() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    let mut world = World::new(
        &catalogue,
        WorldLimits {
            max_locals: 1,
            max_event_blocks: 1,
            ..Default::default()
        },
    )
    .unwrap();
    let owners = owners();
    let context = Context::default();
    let request = [Request {
        definition: &handles[0],
        owner: &owners[0],
        context: &context,
        assignments: &[],
    }];
    let stage = world
        .stage_instance_initialization_group(&request, Limits::default())
        .unwrap();
    let private = Arc::downgrade(&stage.rows[0].schema);
    assert!(matches!(
        world.create_instance(&handles[1], owners[1].clone(), context),
        Err(Error::Capacity("local variables"))
    ));
    assert_eq!(world.revision, 0);
    assert_eq!(world.block_count, 1);
    let before = unchanged(&world);
    assert!(matches!(
        world.commit_instance_initialization_group(stage),
        Err(Error::Capacity("compiled event blocks"))
    ));
    assert_eq!(unchanged(&world), before);
    assert!(private.upgrade().is_none());
}
#[test]
fn identity_revision_overflow_and_empty_noop_are_checked_without_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, handles) = fixture(temp.path());
    let owners = owners();
    let context = Context::default();
    for exhausted in 0..2 {
        let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
        if exhausted == 0 {
            world.next_instance = u64::MAX;
        } else {
            world.revision = u64::MAX;
        }
        let before = unchanged(&world);
        let request = [Request {
            definition: &handles[0],
            owner: &owners[0],
            context: &context,
            assignments: &[],
        }];
        assert!(
            world
                .stage_instance_initialization_group(&request, Limits::default())
                .is_err()
        );
        let stage = world
            .stage_instance_initialization_group(&[], Limits::default())
            .unwrap();
        let (receipt, returned) = world.commit_instance_initialization_group(stage).unwrap();
        assert!(returned.is_empty());
        assert_eq!(receipt.before_revision, receipt.after_revision);
        assert_eq!(unchanged(&world), before);
    }
    let mut total = usize::MAX;
    assert!(bounded_add(&mut total, 1, usize::MAX, "test").is_err());
    assert_eq!(total, usize::MAX);
    let mut bytes = 17;
    assert!(add_charge(&mut bytes, usize::MAX, 2).is_err());
    assert_eq!(bytes, 17);
    let mut world = World::new(&catalogue, WorldLimits::default()).unwrap();
    let stage = world
        .stage_instance_initialization_group(&[], Limits::default())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            ..Default::default()
        })
        .unwrap();
    let before = unchanged(&world);
    assert!(world.commit_instance_initialization_group(stage).is_err());
    assert_eq!(unchanged(&world), before);
}
