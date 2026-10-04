mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::{CommandSignature, Signatures},
        arguments::{Convention, Parameter},
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::native::{self, Inputs, Intent, Location, Outcome, Unsupported},
    foreign::Content,
    identity::{Owner, ReferenceId, ReferenceValue, Value},
    inventory::Facts,
    programs::PreparedSources,
    query::GET_ITEM_COUNT_COMMAND,
};
use std::{fs, sync::Arc};

fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn call(prefix: Option<u16>, command: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(index) = prefix {
        out.extend([0x1c, 0]);
        out.extend(index.to_le_bytes());
    }
    instruction(&mut out, command, payload);
    out
}
fn item(index: u16) -> Vec<u8> {
    [vec![1, 0, b'r'], index.to_le_bytes().to_vec()].concat()
}
fn get(prefix: Option<u16>, index: u16) -> Vec<u8> {
    call(prefix, GET_ITEM_COUNT_COMMAND, &item(index))
}
fn expression(prefix: Option<u16>, index: u16) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(index) = prefix {
        out.push(b'r');
        out.extend(index.to_le_bytes());
    }
    out.push(b'X');
    out.extend(GET_ITEM_COUNT_COMMAND.to_le_bytes());
    out.extend(5_u16.to_le_bytes());
    out.extend(item(index));
    out
}
fn assignment(prefix: Option<u16>, tokens: &[u8]) -> Vec<u8> {
    let data = [
        vec![b's', 2, 0],
        (tokens.len() as u16).to_le_bytes().to_vec(),
        tokens.to_vec(),
    ]
    .concat();
    call(prefix, 0x15, &data)
}
fn event(middle: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x10,
        &[
            0_u16.to_le_bytes().as_slice(),
            &((middle.len() + 4) as u32).to_le_bytes(),
        ]
        .concat(),
    );
    out.extend(middle);
    instruction(&mut out, 0x11, &[]);
    out
}
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let d = tempfile::tempdir().unwrap();
    let original = unit(
        &[(2, 1), (90, 0)],
        &[
            (b"SCRO", 0x100),
            (b"SCRV", 90),
            (b"SCRO", 0x400),
            (b"SCRO", 0),
        ],
    );
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", body));
    source.extend(&original[46..]);
    // No SCTX: production preparation and dispatch consume compiled source.
    fs::write(
        d.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &source),
            record(b"MISC", 0x100, 0, &[]),
            record(b"FLST", 0x400, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let (catalogue, content) = load_content(d.path());
    (d, catalogue, content)
}
fn load_content(path: &std::path::Path) -> (Arc<Catalogue>, Content) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (Arc::new(catalogue), content)
}
fn operators() -> Operators {
    Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(index, text)| Operator {
            code: index as u32,
            precedence: index as u8,
            spelling: text.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap()
}
fn signatures() -> Signatures {
    [
        (
            GET_ITEM_COUNT_COMMAND,
            vec![Parameter {
                type_id: 50,
                optional_word: 0,
            }],
        ),
        (0x1001, vec![]),
    ]
    .into_iter()
    .map(|(id, parameters)| {
        (
            id,
            CommandSignature {
                convention: Convention::Default,
                parameters,
            },
        )
    })
    .collect()
}
fn prepared_sources<'a>(catalogue: &'a Catalogue) -> PreparedSources<'a> {
    let operators = operators();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &signatures(),
        Default::default(),
    )
    .unwrap()
}

fn owned_plan(
    result: fallout_runtime::execution::native_plan::Preparation,
) -> Box<fallout_runtime::execution::native_plan::Plan> {
    match result {
        fallout_runtime::execution::native_plan::Preparation::Ready(plan) => plan,
        fallout_runtime::execution::native_plan::Preparation::Unsupported { reason, detail } => {
            panic!("{reason:?}: {detail}")
        }
    }
}

#[test]
fn owned_native_plan_survives_world_drop_and_counts_current_cold_inventory_not_old_values() {
    use fallout_runtime::execution::native_plan::{self, Selection};
    let (_directory, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (mut world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let inputs = Inputs {
        supplied_subject: Some(subject),
        player: None,
    };
    let plan = owned_plan(
        native_plan::prepare(
            &world,
            &sources,
            &content,
            Selection {
                sequence,
                occurrence: 0,
                inputs,
                intent: Intent::EngineeringObservation,
            },
            Default::default(),
        )
        .unwrap(),
    );
    assert_eq!(plan.call().scda_bytes, 10..19);
    assert_eq!(plan.call().argument_scda_bytes, 14..19);
    assert_eq!(plan.subject(), subject);
    assert_eq!(plan.item(), &form(0x100));
    assert_eq!(plan.source().source_bytes, event(&get(None, 1)));
    let old = plan.observe(&world, &sources, &content, 2).unwrap();
    let Outcome::EngineeringObservation { trace } = old else {
        panic!("old query unavailable")
    };
    assert_eq!(trace.query.result, 8589934590);
    let item = before.inventory_banks[0].items[0].id();
    world
        .remove_item_quantity(item, 1.try_into().unwrap())
        .unwrap();
    let changed = world.snapshot();
    drop(world);
    let plan = std::thread::spawn(move || {
        assert!(plan.source().historical);
        plan
    })
    .join()
    .unwrap();
    for (snapshot, total) in [(&before, 8589934590), (&changed, 8589934589)] {
        let current =
            World::restore(Arc::clone(&catalogue), snapshot.clone(), Default::default()).unwrap();
        let captured = current.snapshot();
        let actual = plan.observe(&current, &sources, &content, 2).unwrap();
        let Outcome::EngineeringObservation { trace } = actual else {
            panic!("current query unavailable")
        };
        assert_eq!(trace.query.result, total);
        assert_eq!(trace.query.contributions.len(), 2);
        let current_calls = current
            .prepare_native_calls_with_sources(sequence, &sources, Default::default())
            .unwrap();
        let legacy = current_calls
            .observe(0, &content, inputs, Intent::EngineeringObservation, 2)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&legacy.outcome).unwrap(),
            serde_json::to_value(Outcome::EngineeringObservation { trace }).unwrap()
        );
        assert_eq!(current.snapshot(), captured);
        assert!(matches!(
            plan.observe(&current, &sources, &content, 1),
            Err(native_plan::Error::Capacity("query contributions"))
        ));
        assert_eq!(current.snapshot(), captured);
    }
}

#[test]
fn owned_native_plan_revalidates_dynamic_caller_item_head_owner_and_full_source() {
    use fallout_runtime::execution::native_plan::{self, Selection};
    let (directory, catalogue, content) = fixture(&event(&get(Some(2), 1)));
    let sources = prepared_sources(&catalogue);
    let (mut world, handle, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: subject },
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    let inputs = Inputs {
        supplied_subject: None,
        player: Some(subject),
    };
    let plan = owned_plan(
        native_plan::prepare(
            &world,
            &sources,
            &content,
            Selection {
                sequence,
                occurrence: 0,
                inputs,
                intent: Intent::EngineeringObservation,
            },
            Default::default(),
        )
        .unwrap(),
    );
    assert_eq!(plan.call().calling_reference_index, Some(2));
    assert_eq!(plan.explicit_player(), Some(subject));
    for variant in 0..5 {
        let mut changed = before.clone();
        match variant {
            0 => {
                changed.pending_events.remove(0);
            }
            1 => changed.pending_events[0].context.target = None,
            2 => changed.instances[0].context.target = None,
            3 => {
                changed.instances[0].owner = Owner::Fragment {
                    activation: 2.try_into().unwrap(),
                }
            }
            _ => {
                changed.instances[0]
                    .locals
                    .iter_mut()
                    .find(|local| local.index == 90)
                    .unwrap()
                    .value = Value::Reference {
                    value: ReferenceValue::Null,
                }
            }
        };
        let current =
            World::restore(Arc::clone(&catalogue), changed.clone(), Default::default()).unwrap();
        let result = plan.observe(&current, &sources, &content, 2);
        assert!(result.is_err() || matches!(result, Ok(Outcome::Unsupported { .. })));
        assert_eq!(current.snapshot(), changed);
    }
    let mut current =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    let other = current.register_reference(None).unwrap();
    current.initialize_inventory(other).unwrap();
    let live = current.handle(before.instances[0].id).unwrap();
    current
        .assign(
            live,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: other },
                },
            )],
        )
        .unwrap();
    let captured = current.snapshot();
    assert!(matches!(
        plan.observe(&current, &sources, &content, 2),
        Err(native_plan::Error::ContextChanged(
            "fresh resolved caller or item"
        ))
    ));
    assert_eq!(current.snapshot(), captured);
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let changed = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    assert_eq!(definition(&changed), definition(&catalogue));
    let mut changed_snapshot = before.clone();
    changed_snapshot.catalogue_sha256 = World::new(Arc::clone(&changed), Default::default())
        .unwrap()
        .catalogue_fingerprint()
        .into();
    let changed_world = World::restore(changed, changed_snapshot, Default::default()).unwrap();
    assert_eq!(changed_world.campaign(), world.campaign());
    assert!(matches!(
        plan.observe(&changed_world, &sources, &content, 2),
        Err(native_plan::Error::ContextChanged(_))
    ));
    let (_item_dir, item_catalogue, item_content) = fixture(&event(&get(None, 2)));
    let item_sources = prepared_sources(&item_catalogue);
    let (mut item_world, item_handle, item_sequence, item_subject) =
        seed(Arc::clone(&item_catalogue), 0);
    item_world
        .assign(
            item_handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x100) },
                },
            )],
        )
        .unwrap();
    let item_plan = owned_plan(
        native_plan::prepare(
            &item_world,
            &item_sources,
            &item_content,
            Selection {
                sequence: item_sequence,
                occurrence: 0,
                inputs: Inputs {
                    supplied_subject: Some(item_subject),
                    player: None,
                },
                intent: Intent::EngineeringObservation,
            },
            Default::default(),
        )
        .unwrap(),
    );
    item_world
        .assign(
            item_handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x400) },
                },
            )],
        )
        .unwrap();
    let captured = item_world.snapshot();
    assert!(matches!(
        item_plan.observe(&item_world, &item_sources, &item_content, 2),
        Err(native_plan::Error::ContextChanged(
            "fresh resolved caller or item"
        ))
    ));
    assert_eq!(item_world.snapshot(), captured);
    for value in [
        Value::Uninitialized,
        Value::Reference {
            value: ReferenceValue::Null,
        },
    ] {
        item_world.assign(item_handle, &[(90, value)]).unwrap();
        let before = item_world.snapshot();
        assert!(matches!(
            item_plan
                .observe(&item_world, &item_sources, &item_content, 2)
                .unwrap(),
            Outcome::Unsupported {
                reason: Unsupported::ArgumentResolution
                    | Unsupported::ArgumentNeedsContentReference,
                ..
            }
        ));
        assert_eq!(item_world.snapshot(), before);
    }
}

#[test]
fn owned_native_plan_refuses_removed_explicit_caller_and_changed_decoder() {
    use fallout_runtime::execution::native_plan::{self, Selection};
    let (_directory, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let mut before = world.snapshot();
    before.instances[0].context = Context::default();
    before.pending_events[0].context = Context::default();
    let world = World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    let plan = owned_plan(
        native_plan::prepare(
            &world,
            &sources,
            &content,
            Selection {
                sequence,
                occurrence: 0,
                inputs: Inputs {
                    supplied_subject: Some(subject),
                    player: None,
                },
                intent: Intent::EngineeringObservation,
            },
            Default::default(),
        )
        .unwrap(),
    );
    let mut removed = before.clone();
    removed
        .references
        .retain(|reference| reference.id != subject);
    removed.inventory_banks.retain(|bank| bank.owner != subject);
    let current =
        World::restore(Arc::clone(&catalogue), removed.clone(), Default::default()).unwrap();
    assert!(matches!(
        plan.observe(&current, &sources, &content, 2).unwrap(),
        Outcome::Unsupported {
            reason: Unsupported::HostQueryUnavailable,
            ..
        }
    ));
    assert_eq!(current.snapshot(), removed);
    let mut other_signatures = signatures();
    other_signatures.insert(
        0x1050,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![],
        },
    );
    let operators = operators();
    let changed_sources = PreparedSources::load(
        &catalogue,
        &Model::vanilla(&operators).unwrap(),
        &other_signatures,
        Default::default(),
    )
    .unwrap();
    assert_ne!(sources.decoder_sha256(), changed_sources.decoder_sha256());
    assert!(matches!(
        plan.observe(&world, &changed_sources, &content, 2),
        Err(native_plan::Error::ContextChanged(
            "campaign, full source or decoder"
        ))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn owned_native_plan_exact_creation_limits_and_unsupported_admission_preserve_world() {
    use fallout_runtime::execution::native_plan::{self, Selection};
    let (_directory, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let selection = Selection {
        sequence,
        occurrence: 0,
        inputs: Inputs {
            supplied_subject: Some(subject),
            player: None,
        },
        intent: Intent::EngineeringObservation,
    };
    let sample = owned_plan(
        native_plan::prepare(&world, &sources, &content, selection, Default::default()).unwrap(),
    );
    let c = sample.counts();
    let exact = native_plan::Limits {
        native: native::Limits {
            maximum_event_instructions: 3,
            maximum_calls: 1,
            maximum_argument_bytes: 5,
        },
        source_projection: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: c.source.source_bytes,
            maximum_rows: c.source.rows,
            maximum_variable_bytes: c.source.variable_bytes,
            maximum_binding_uses: c.source.binding_uses,
        },
        maximum_query_variable_bytes: c.query_variable_bytes,
    };
    assert!(matches!(
        native_plan::prepare(&world, &sources, &content, selection, exact).unwrap(),
        native_plan::Preparation::Ready(_)
    ));
    for field in 0..8 {
        let mut limits = exact;
        match field {
            0 => limits.native.maximum_event_instructions -= 1,
            1 => limits.native.maximum_calls -= 1,
            2 => limits.native.maximum_argument_bytes -= 1,
            3 => limits.source_projection.maximum_source_bytes -= 1,
            4 => limits.source_projection.maximum_rows -= 1,
            5 => limits.source_projection.maximum_variable_bytes -= 1,
            6 => limits.source_projection.maximum_binding_uses -= 1,
            _ => limits.maximum_query_variable_bytes -= 1,
        };
        assert!(
            native_plan::prepare(&world, &sources, &content, selection, limits).is_err(),
            "field {field}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let mut faithful = selection;
    faithful.intent = Intent::Faithful;
    assert!(matches!(
        native_plan::prepare(
            &world,
            &sources,
            &content,
            faithful,
            native_plan::Limits {
                maximum_query_variable_bytes: 0,
                ..exact
            }
        )
        .unwrap(),
        native_plan::Preparation::Unsupported {
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    let missing = Selection {
        inputs: Inputs::default(),
        ..selection
    };
    assert!(matches!(
        native_plan::prepare(&world, &sources, &content, missing, exact).unwrap(),
        native_plan::Preparation::Unsupported {
            reason: Unsupported::MissingSubject,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn owned_native_plan_bounds_large_canonical_dynamic_reference_before_shared_resolution() {
    use fallout_runtime::execution::native_plan::{self, Selection};
    let (_directory, catalogue, content) = fixture(&event(&get(None, 2)));
    let sources = prepared_sources(&catalogue);
    let (mut world, handle, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x100) },
                },
            )],
        )
        .unwrap();
    let selection = Selection {
        sequence,
        occurrence: 0,
        inputs: Inputs {
            supplied_subject: Some(subject),
            player: None,
        },
        intent: Intent::EngineeringObservation,
    };
    let plan = owned_plan(
        native_plan::prepare(&world, &sources, &content, selection, Default::default()).unwrap(),
    );
    let mut oversized = form(0x100);
    oversized.origin_plugin = "x".repeat(32 * 1024);
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: oversized },
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    assert!(matches!(
        native_plan::prepare(&world, &sources, &content, selection, Default::default()),
        Err(native_plan::Error::Native(native::Error::Capacity(
            "reference variable bytes"
        )))
    ));
    assert!(matches!(
        plan.observe(&world, &sources, &content, 2),
        Err(native_plan::Error::Native(native::Error::Capacity(
            "reference variable bytes"
        )))
    ));
    let calls = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    // Legacy admission has its original bounds and semantic refusal; the new
    // plan's tighter variable budget does not alter existing one-shot behavior.
    let legacy = calls
        .observe(
            0,
            &content,
            selection.inputs,
            Intent::EngineeringObservation,
            2,
        )
        .unwrap();
    reason(&legacy.outcome, Unsupported::HostQueryUnavailable);
    assert!(matches!(
        native_plan::prepare(
            &world,
            &sources,
            &content,
            Selection {
                intent: Intent::Faithful,
                ..selection
            },
            native_plan::Limits {
                maximum_query_variable_bytes: 0,
                ..Default::default()
            }
        )
        .unwrap(),
        native_plan::Preparation::Unsupported {
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
    let (_caller_directory, caller_catalogue, caller_content) = fixture(&event(&get(Some(2), 1)));
    let caller_sources = prepared_sources(&caller_catalogue);
    let (mut caller_world, caller_handle, caller_sequence, _) =
        seed(Arc::clone(&caller_catalogue), 0);
    let mut caller_key = form(0x100);
    caller_key.origin_plugin = "q".repeat(32 * 1024);
    caller_world
        .assign(
            caller_handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: caller_key },
                },
            )],
        )
        .unwrap();
    let caller_before = caller_world.snapshot();
    let caller_selection = Selection {
        sequence: caller_sequence,
        occurrence: 0,
        inputs: Inputs::default(),
        intent: Intent::EngineeringObservation,
    };
    assert!(matches!(
        native_plan::prepare(
            &caller_world,
            &caller_sources,
            &caller_content,
            caller_selection,
            Default::default()
        ),
        Err(native_plan::Error::Native(native::Error::Capacity(
            "reference variable bytes"
        )))
    ));
    let caller_calls = caller_world
        .prepare_native_calls_with_sources(caller_sequence, &caller_sources, Default::default())
        .unwrap();
    reason(
        &caller_calls
            .observe(
                0,
                &caller_content,
                Inputs::default(),
                Intent::EngineeringObservation,
                2,
            )
            .unwrap()
            .outcome,
        Unsupported::CallerNeedsLiveReference,
    );
    assert_eq!(caller_world.snapshot(), caller_before);
}

fn seed(
    catalogue: Arc<Catalogue>,
    offset: u32,
) -> (
    World<'static>,
    fallout_runtime::state::InstanceHandle,
    u64,
    ReferenceId,
) {
    let definition = definition(&catalogue);
    let mut world = World::new(catalogue, Default::default()).unwrap();
    let subject = world.register_reference(None).unwrap();
    world.initialize_inventory(subject).unwrap();
    for _ in 0..2 {
        world
            .add_item(
                subject,
                Facts::unknown(form(0x100)),
                u32::MAX.try_into().unwrap(),
            )
            .unwrap();
    }
    let context = Context {
        calling_reference: Some(subject),
        containing_reference: Some(subject),
        target: Some(ReferenceValue::Live { id: subject }),
        arguments: vec![ReferenceValue::Null],
    };
    let handle = world
        .create_instance(
            &definition,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            context.clone(),
        )
        .unwrap();
    let sequence = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: offset,
            },
            context,
        )
        .unwrap();
    (world, handle, sequence, subject)
}
fn reason(outcome: &Outcome, expected: Unsupported) {
    assert!(
        matches!(outcome, Outcome::Unsupported { reason, .. } if *reason == expected),
        "{outcome:?}"
    );
}

#[test]
fn source_less_physical_occurrences_keep_exact_offsets_order_and_duplicates() {
    let tokens = [expression(Some(2), 1), expression(None, 1), vec![b'+']].concat();
    let body = event(
        &[
            get(None, 1),
            assignment(None, &tokens),
            call(None, 0x1001, &[0, 0]),
        ]
        .concat(),
    );
    let (_d, catalogue, _) = fixture(&body);
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, _) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    let calls = prepared.calls();
    assert_eq!(calls.len(), 4);
    assert_eq!(
        calls
            .iter()
            .map(|c| c.scda_bytes.clone())
            .collect::<Vec<_>>(),
        [10..19, 31..41, 41..51, 52..58]
    );
    assert_eq!(
        calls
            .iter()
            .map(|c| c.argument_scda_offset)
            .collect::<Vec<_>>(),
        [14, 36, 46, 56]
    );
    assert_eq!(
        calls
            .iter()
            .map(|c| c.instruction_index)
            .collect::<Vec<_>>(),
        [1, 2, 2, 3]
    );
    assert_eq!(calls[1].location, Location::Expression { token_index: 1 });
    assert_eq!(calls[1].calling_reference_index, Some(2));
    assert_eq!(calls[1].instruction_scda_bytes, 19..52);
    assert_eq!(calls[2].raw_arguments, item(1));
    assert!(
        calls
            .iter()
            .all(|c| c.capability.decoded_source && !c.capability.retail_executable)
    );
    assert_eq!(world.snapshot(), before);
    assert!(!native::capability(GET_ITEM_COUNT_COMMAND).decoded_source);
    assert!(!native::capability(47).engineering_host_read);
}

#[test]
fn capability_inventory_keeps_both_conditional_arms_without_evaluating_truth() {
    let mut middle = Vec::new();
    instruction(&mut middle, 0x16, &[1, 0, 1, 0, b'0']);
    middle.extend(get(None, 1));
    instruction(&mut middle, 0x17, &[1, 0]);
    middle.extend(call(None, 0x1001, &[0, 0]));
    instruction(&mut middle, 0x19, &[]);
    let (_d, catalogue, content) = fixture(&event(&middle));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, _) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    assert_eq!(
        prepared
            .calls()
            .iter()
            .map(|c| c.scda_bytes.clone())
            .collect::<Vec<_>>(),
        [19..28, 34..40]
    );
    for index in 0..2 {
        assert!(matches!(
            prepared
                .observe(index, &content, Inputs::default(), Intent::Faithful, 0)
                .unwrap()
                .outcome,
            Outcome::Unsupported { .. }
        ));
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
fn faithful_calls_and_unknown_commands_never_return_success_or_zero() {
    let (_d, catalogue, content) = fixture(&event(
        &[get(None, 1), call(None, 0x1001, &[0, 0])].concat(),
    ));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    let inputs = Inputs {
        supplied_subject: Some(subject),
        player: Some(subject),
    };
    reason(
        &prepared
            .observe(0, &content, inputs, Intent::Faithful, 0)
            .unwrap()
            .outcome,
        Unsupported::UnverifiedRetailSemantics,
    );
    for intent in [Intent::Faithful, Intent::EngineeringObservation] {
        reason(
            &prepared
                .observe(1, &content, inputs, intent, 0)
                .unwrap()
                .outcome,
            Unsupported::MissingImplementation,
        );
    }
    assert!(matches!(
        prepared.observe(2, &content, inputs, Intent::Faithful, 0),
        Err(native::Error::MissingCall(2))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn compiled_call_reaches_existing_host_query_without_numeric_conversion_or_ack() {
    let (_d, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    let observed = prepared
        .observe(
            0,
            &content,
            Inputs {
                supplied_subject: Some(subject),
                player: None,
            },
            Intent::EngineeringObservation,
            2,
        )
        .unwrap();
    let Outcome::EngineeringObservation { trace } = observed.outcome else {
        panic!("Expected engineering host read")
    };
    assert_eq!(trace.query.result, 8_589_934_590);
    assert_eq!(trace.query.contributions.len(), 2);
    assert!(trace.original_numeric_return.is_none());
    assert!(!trace.original_behavior_verified);
    assert_eq!(observed.state_revision, world.revision());
    assert_eq!(observed.pending.sequence, sequence);
    assert_eq!(world.snapshot(), before);
}

#[test]
fn host_subject_is_explicit_despite_pending_caller_target_containing_owner_and_player() {
    let (_d, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    let observed = prepared
        .observe(
            0,
            &content,
            Inputs {
                supplied_subject: None,
                player: Some(subject),
            },
            Intent::EngineeringObservation,
            2,
        )
        .unwrap();
    assert_eq!(observed.event_context.calling_reference, Some(subject));
    assert_eq!(observed.event_context.containing_reference, Some(subject));
    reason(&observed.outcome, Unsupported::MissingSubject);
}

#[test]
fn source_caller_resolves_live_storage_and_never_falls_back_to_host_subject() {
    let (_d, catalogue, content) = fixture(&event(&get(Some(2), 1)));
    let sources = prepared_sources(&catalogue);
    let (mut world, instance, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let inputs = Inputs {
        supplied_subject: Some(subject),
        player: None,
    };
    {
        let prepared = world
            .prepare_native_calls_with_sources(sequence, &sources, Default::default())
            .unwrap();
        reason(
            &prepared
                .observe(0, &content, inputs, Intent::EngineeringObservation, 2)
                .unwrap()
                .outcome,
            Unsupported::CallerResolution,
        );
    }
    let other = world.register_reference(None).unwrap();
    world.initialize_inventory(other).unwrap();
    world
        .assign(
            instance,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: other },
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    let Outcome::EngineeringObservation { trace } = prepared
        .observe(0, &content, inputs, Intent::EngineeringObservation, 0)
        .unwrap()
        .outcome
    else {
        panic!("Live caller expected")
    };
    assert_eq!(trace.query.result, 0); // explicitly initialized empty other bank
    assert_eq!(world.snapshot(), before);
}

#[test]
fn null_and_content_callers_do_not_become_the_player_or_registered_subject() {
    let (_d, catalogue, content) = fixture(&event(&[get(Some(1), 1), get(Some(4), 1)].concat()));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    for index in 0..2 {
        reason(
            &prepared
                .observe(
                    index,
                    &content,
                    Inputs {
                        supplied_subject: Some(subject),
                        player: Some(subject),
                    },
                    Intent::EngineeringObservation,
                    2,
                )
                .unwrap()
                .outcome,
            Unsupported::CallerNeedsLiveReference,
        );
    }
}

#[test]
fn form_lists_null_arguments_and_form_variables_remain_typed_unsupported() {
    let body = event(
        &[
            get(None, 3),
            get(None, 4),
            call(None, GET_ITEM_COUNT_COMMAND, &[1, 0, b'f', 90, 0]),
        ]
        .concat(),
    );
    let (_d, catalogue, content) = fixture(&body);
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    for (index, expected) in [
        Unsupported::UnverifiedFormList,
        Unsupported::ArgumentNeedsContentReference,
        Unsupported::ArgumentSemantics,
    ]
    .into_iter()
    .enumerate()
    {
        reason(
            &prepared
                .observe(
                    index,
                    &content,
                    Inputs {
                        supplied_subject: Some(subject),
                        player: None,
                    },
                    Intent::EngineeringObservation,
                    2,
                )
                .unwrap()
                .outcome,
            expected,
        );
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
fn a_prefixed_enclosing_statement_is_rejected_by_source_admission() {
    let (_d, catalogue, _) = fixture(&event(&assignment(Some(2), &expression(None, 1))));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, _) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let error = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .err()
        .unwrap();
    assert!(error.to_string().contains("statement_reference_prefix"));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn exact_and_one_less_event_call_argument_and_contribution_budgets_are_atomic() {
    let (_d, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let before = world.snapshot();
    let exact = native::Limits {
        maximum_event_instructions: 3,
        maximum_calls: 1,
        maximum_argument_bytes: 5,
    };
    for limits in [
        native::Limits {
            maximum_event_instructions: 2,
            ..exact
        },
        native::Limits {
            maximum_calls: 0,
            ..exact
        },
        native::Limits {
            maximum_argument_bytes: 4,
            ..exact
        },
    ] {
        assert!(
            world
                .prepare_native_calls_with_sources(sequence, &sources, limits)
                .is_err()
        );
    }
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, exact)
        .unwrap();
    assert!(matches!(
        prepared.observe(
            0,
            &content,
            Inputs {
                supplied_subject: Some(subject),
                player: None
            },
            Intent::EngineeringObservation,
            1
        ),
        Err(native::Error::Capacity("query contributions"))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn second_event_isolated_and_missing_or_acknowledged_entries_cannot_dispatch() {
    let first = event(&get(None, 1));
    let body = [first.clone(), event(&call(None, 0x1001, &[0, 0]))].concat();
    let (_d, catalogue, _) = fixture(&body);
    let sources = prepared_sources(&catalogue);
    let (mut world, _, sequence, _) = seed(Arc::clone(&catalogue), first.len() as u32);
    let before = world.snapshot();
    {
        let prepared = world
            .prepare_native_calls_with_sources(sequence, &sources, Default::default())
            .unwrap();
        assert_eq!(prepared.calls().len(), 1);
        assert_eq!(prepared.calls()[0].command_id, 0x1001);
        assert_eq!(prepared.calls()[0].scda_bytes, 33..39);
    }
    assert_eq!(world.snapshot(), before);
    world.acknowledge(sequence).unwrap();
    assert!(matches!(
        world.prepare_native_calls_with_sources(sequence, &sources, Default::default()),
        Err(native::Error::Preparation(_))
    ));
}

#[test]
fn current_state_and_restored_world_are_read_fresh_without_reusing_old_handles() {
    let (_d, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (mut world, handle, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let observe = |world: &World<'_>| {
        let prepared = world
            .prepare_native_calls_with_sources(sequence, &sources, Default::default())
            .unwrap();
        serde_json::to_value(
            prepared
                .observe(
                    0,
                    &content,
                    Inputs {
                        supplied_subject: Some(subject),
                        player: None,
                    },
                    Intent::EngineeringObservation,
                    2,
                )
                .unwrap(),
        )
        .unwrap()
    };
    let first = observe(&world);
    let item = world.inventory_items(subject).unwrap().next().unwrap().id();
    world
        .remove_item_quantity(item, 1.try_into().unwrap())
        .unwrap();
    let second = observe(&world);
    assert_eq!(
        second["outcome"]["trace"]["query"]["result"],
        8_589_934_589_u64
    );
    assert_ne!(first["state_revision"], second["state_revision"]);
    let saved = world.snapshot();
    let restored =
        World::restore(Arc::clone(&catalogue), saved.clone(), Default::default()).unwrap();
    assert_eq!(observe(&restored), second);
    assert!(restored.instance(handle).is_err());
    assert_eq!(restored.snapshot(), saved);
}

#[test]
fn changed_cohorts_and_unadmitted_source_never_yield_partial_dispatch() {
    let (d, catalogue, content) = fixture(&event(&get(None, 1)));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let mut changed = fs::read(d.path().join("FalloutNV.esm")).unwrap();
    changed.extend(record(b"MISC", 0x401, 0, &[]));
    fs::write(d.path().join("FalloutNV.esm"), changed).unwrap();
    let (other_catalogue, other_content) = load_content(d.path());
    let other_sources = prepared_sources(&other_catalogue);
    assert!(
        world
            .prepare_native_calls_with_sources(sequence, &other_sources, Default::default())
            .is_err()
    );
    let prepared = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    assert!(matches!(
        prepared.observe(
            0,
            &other_content,
            Inputs {
                supplied_subject: Some(subject),
                player: None
            },
            Intent::Faithful,
            0
        ),
        Err(native::Error::Content(_))
    ));
    reason(
        &prepared
            .observe(0, &content, Inputs::default(), Intent::Faithful, 0)
            .unwrap()
            .outcome,
        Unsupported::UnverifiedRetailSemantics,
    );
    let (_bad_d, bad_catalogue, _) =
        fixture(&event(&[get(None, 1), call(None, 0x1030, &[])].concat()));
    let bad_sources = prepared_sources(&bad_catalogue);
    let (bad_world, _, bad_sequence, _) = seed(Arc::clone(&bad_catalogue), 0);
    assert!(
        bad_world
            .prepare_native_calls_with_sources(bad_sequence, &bad_sources, Default::default())
            .is_err()
    );
}

#[test]
#[ignore = "built CLI and authored executable metadata; saved read-only native queries, no original launch"]
fn cli_saved_native_helper() {
    use fallout_runtime::snapshot::Snapshot;
    use serde_json::{Value as Json, json};
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_SAVED_NATIVE_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let body = event(
        &[
            get(None, 1),
            get(Some(2), 1),
            call(None, 0x1001, &item(2)),
            get(None, 1),
        ]
        .concat(),
    );
    let (temporary, catalogue, content) = fixture(&body);
    // The actual pinned descriptor is GetDistance(ObjectReferenceID), unlike
    // the intentionally minimal signatures used by older pure dispatch tests.
    let operators = operators();
    let mut native_signatures = signatures();
    native_signatures.insert(
        0x1001,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![Parameter {
                type_id: 4,
                optional_word: 0,
            }],
        },
    );
    let sources = PreparedSources::load(
        &catalogue,
        &Model::vanilla(&operators).unwrap(),
        &native_signatures,
        Default::default(),
    )
    .unwrap();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        temporary.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        input.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (mut world, handle, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    let other = world.register_reference(None).unwrap();
    world.initialize_inventory(other).unwrap();
    world
        .add_item(other, Facts::unknown(form(0x100)), 23.try_into().unwrap())
        .unwrap();
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: other },
                },
            )],
        )
        .unwrap();
    let second = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let input_bytes = before.encode(64 * 1024 * 1024).unwrap();
    let snapshot_path = evidence.join("input.snapshot.json");
    fs::write(&snapshot_path, &input_bytes).unwrap();
    let calls = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    assert_eq!(calls.calls().len(), 4);
    let selections = [
        (3, Some(subject)),
        (1, None),
        (0, Some(subject)),
        (3, Some(subject)),
    ];
    let mut expected = Vec::new();
    for (occurrence, supplied_subject) in selections {
        let observed = calls
            .observe(
                occurrence,
                &content,
                Inputs {
                    supplied_subject,
                    player: Some(subject),
                },
                Intent::EngineeringObservation,
                2,
            )
            .unwrap();
        let Outcome::EngineeringObservation { trace } = &observed.outcome else {
            panic!("{observed:?}")
        };
        assert!(trace.original_numeric_return.is_none());
        assert!(!trace.original_behavior_verified);
        if occurrence == 1 {
            assert_eq!(trace.query.subject, other);
            assert_eq!(trace.query.result, 23);
            assert_eq!(trace.query.contributions.len(), 1);
        } else {
            assert_eq!(trace.query.subject, subject);
            assert_eq!(trace.query.result, 8_589_934_590);
            assert_eq!(trace.query.contributions.len(), 2);
        }
        expected.push(json!({"occurrence":occurrence,"observation":observed}));
    }
    let selected: Vec<_> = selections
        .into_iter()
        .map(|(occurrence, supplied_subject)| {
            json!({
                "occurrence":occurrence,"intent":"engineering_observation",
                "supplied_subject":supplied_subject,"explicit_player":subject
            })
        })
        .collect();
    let request = json!({"schema_version":1,"sequence":sequence,"calls":selected,
        "maximum_contributions":7,"maximum_report_bytes":1024*1024});
    let run_on = |name: &str,
                  install: &Path,
                  order: &Path,
                  snapshot: &Path,
                  request: &Json,
                  extra: &[&str]| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        let report = directory.join("report.json");
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(install)
            .arg("--load-order")
            .arg(order)
            .arg("--snapshot-native-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(snapshot)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        assert_eq!(
            fs::read(&snapshot_path).unwrap(),
            input_bytes,
            "{name}: input changed"
        );
        (output, report)
    };
    let run = |name: &str, snapshot: &Path, request: &Json| {
        run_on(name, &install, &order, snapshot, request, &[])
    };
    let (output, report_path) = run("selected", &snapshot_path, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(&report_path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    assert_eq!(report["observations"], json!(expected));
    assert_eq!(report["canonical_state_unchanged"], true);
    assert_eq!(report["event_acknowledged"], false);
    assert_eq!(report["faithful_execution_admitted"], false);
    assert_eq!(
        report["prepared_sources"]["counts"]["preparation_attempts"],
        1
    );
    assert_eq!(report["physical_call_count"], 4);
    assert_eq!(report["state_revision"], before.state_revision);
    assert_eq!(world.snapshot(), before);
    for (name, intent) in [
        (
            "intent-object-engineering-null",
            json!({"engineering_observation":null}),
        ),
        ("intent-object-faithful-null", json!({"faithful":null})),
    ] {
        let mut invalid = request.clone();
        invalid["calls"][0]["intent"] = intent;
        let (output, path) = run(name, &snapshot_path, &invalid);
        assert!(!output.status.success() && !path.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("expected a string"));
    }
    for (name, maximum, success) in [
        ("exact-report", report_bytes.len(), true),
        ("short-report", report_bytes.len() - 1, false),
    ] {
        let mut bounded = request.clone();
        bounded["maximum_report_bytes"] = json!(maximum);
        let (output, path) = run(name, &snapshot_path, &bounded);
        assert_eq!(
            output.status.success(),
            success,
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        if success {
            assert_eq!(fs::read(path).unwrap(), report_bytes);
        } else {
            assert!(!path.exists());
            assert!(String::from_utf8_lossy(&output.stderr).contains("report-byte budget"));
        }
    }
    let mut insufficient = request.clone();
    insufficient["maximum_contributions"] = json!(6);
    let (output, path) = run("short-contributions", &snapshot_path, &insufficient);
    assert!(!output.status.success());
    assert!(!path.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("query contributions"));
    let first = json!({"occurrence":0,"intent":"engineering_observation","supplied_subject":subject,"explicit_player":null});
    for (name, selection, expected_reason) in [
        (
            "missing-subject",
            json!({"occurrence":0,"intent":"engineering_observation","supplied_subject":null,"explicit_player":subject}),
            "missing_subject",
        ),
        (
            "faithful",
            json!({"occurrence":0,"intent":"faithful","supplied_subject":null,"explicit_player":subject}),
            "unverified_retail_semantics",
        ),
        (
            "unsupported",
            json!({"occurrence":2,"intent":"engineering_observation","supplied_subject":subject,"explicit_player":null}),
            "missing_implementation",
        ),
        (
            "unknown-subject",
            json!({"occurrence":0,"intent":"engineering_observation","supplied_subject":999,"explicit_player":subject}),
            "host_query_unavailable",
        ),
    ] {
        let mut refused = request.clone();
        refused["calls"] = json!([selection]);
        let (output, path) = run(name, &snapshot_path, &refused);
        assert!(!output.status.success(), "{name}");
        let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            report["observations"][0]["observation"]["outcome"]["reason"], expected_reason,
            "{name}"
        );
        assert_eq!(report["canonical_state_unchanged"], true);
        assert_eq!(report["event_acknowledged"], false);
    }
    let mut nonhead = request.clone();
    nonhead["sequence"] = json!(second);
    nonhead["calls"] = json!([first]);
    nonhead["maximum_contributions"] = json!(2);
    let (output, path) = run("read-nonhead", &snapshot_path, &nonhead);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        report["observations"][0]["observation"]["pending"]["sequence"],
        second
    );
    assert_eq!(report["event_acknowledged"], false);
    for (name, field, value, reason) in [
        (
            "request-schema",
            "schema_version",
            json!(2),
            "invalid saved native request",
        ),
        (
            "empty-selection",
            "calls",
            json!([]),
            "invalid saved native request",
        ),
        (
            "selection-limit",
            "calls",
            json!(vec![first.clone(); 129]),
            "invalid saved native request",
        ),
        (
            "contribution-ceiling",
            "maximum_contributions",
            json!(65537),
            "invalid saved native request",
        ),
        (
            "report-ceiling",
            "maximum_report_bytes",
            json!(8 * 1024 * 1024 + 1),
            "invalid saved native request",
        ),
        (
            "zero-report",
            "maximum_report_bytes",
            json!(0),
            "invalid saved native request",
        ),
        ("zero-sequence", "sequence", json!(0), "nonzero"),
        (
            "missing-sequence",
            "sequence",
            json!(999),
            "not present in the pending journal",
        ),
        ("unknown-field", "initialize", json!(true), "unknown field"),
        (
            "request-byte-limit",
            "extra",
            json!("x".repeat(64 * 1024)),
            "request byte budget",
        ),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, path) = run(name, &snapshot_path, &invalid);
        assert!(!output.status.success(), "{name}");
        assert!(!path.exists(), "{name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (name, field, value, reason) in [
        (
            "bad-occurrence",
            "occurrence",
            json!(4),
            "outside this prepared event",
        ),
        (
            "copy-intent",
            "intent",
            json!("engineering"),
            "unknown variant",
        ),
        ("zero-subject", "supplied_subject", json!(0), "nonzero"),
        (
            "omitted-player",
            "explicit_player",
            Json::Null,
            "missing field",
        ),
        (
            "omitted-subject",
            "supplied_subject",
            Json::Null,
            "missing field",
        ),
    ] {
        let mut invalid = request.clone();
        invalid["calls"] = json!([first]);
        if name.starts_with("omitted") {
            invalid["calls"][0].as_object_mut().unwrap().remove(field);
        } else {
            invalid["calls"][0][field] = value;
        }
        let (output, path) = run(name, &snapshot_path, &invalid);
        assert!(!output.status.success(), "{name}");
        assert!(!path.exists(), "{name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (name, field, value) in [
        ("legacy-schema", "schema_version", json!(3)),
        ("stale-cohort", "catalogue_sha256", json!("0".repeat(64))),
        ("stale-definition", "version_sha256", json!("0".repeat(64))),
    ] {
        let mut snapshot = serde_json::to_value(&before).unwrap();
        if name == "stale-definition" {
            snapshot["instances"][0]["definition"][field] = value;
        } else {
            snapshot[field] = value;
        }
        let path = evidence.join(format!("{name}.snapshot.json"));
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let (output, report) = run(name, &path, &request);
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists(), "{name}");
    }
    let foreign = evidence.join("foreign-source-copy");
    fs::create_dir(&foreign).unwrap();
    fs::create_dir(foreign.join("Data")).unwrap();
    fs::copy(
        install.join("Data/FalloutNV.esm"),
        foreign.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::write(
        foreign.join("Data/Other.esm"),
        [header(&[]), record(b"MISC", 0x499, 0, &[])].concat(),
    )
    .unwrap();
    fs::copy(install.join("FalloutNV.exe"), foreign.join("FalloutNV.exe")).unwrap();
    let foreign_order = evidence.join("foreign-order.json");
    fs::write(&foreign_order, b"[\"FalloutNV.esm\",\"Other.esm\"]").unwrap();
    let mut foreign_store = RecordStore::open_nv_headers(
        &foreign.join("Data"),
        &["FalloutNV.esm".into(), "Other.esm".into()],
        Default::default(),
    )
    .unwrap();
    let foreign_catalogue =
        Catalogue::load(&mut foreign_store, Default::default(), |_, _| Ok(())).unwrap();
    assert_eq!(definition(&foreign_catalogue), definition(&catalogue));
    let (output, path) = run_on(
        "foreign-cohort",
        &foreign,
        &foreign_order,
        &snapshot_path,
        &request,
        &[],
    );
    assert!(!output.status.success());
    assert!(!path.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("has changed"));
    for (name, extra) in [
        (
            "copy-output-conflict",
            vec!["--snapshot-output", "unused.snapshot.json"],
        ),
        (
            "copy-request-conflict",
            vec!["--snapshot-copy-request", "unused.json"],
        ),
    ] {
        let (output, path) = run_on(name, &install, &order, &snapshot_path, &request, &extra);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    assert_eq!(
        Snapshot::decode(&fs::read(&snapshot_path).unwrap(), Default::default()).unwrap(),
        before
    );
    assert_eq!(world.snapshot(), before);
    fs::write(evidence.join("scope.json"),serde_json::to_vec_pretty(&json!({
        "scope":"strict_current_snapshot_read_only_native_observations","original_executed":false,
        "actual_cli_calls":32,"input_schema":before.schema_version,"input_snapshot":before,
        "requested_occurrences":[3,1,0,3],"aggregate_contributions":7,"exact_report_bytes":report_bytes.len(),
        "original_numeric_return":null,"retail_parity_accepted":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "built CLI and authored executable metadata; owned cold native plan, no original launch"]
fn cli_owned_native_plan_helper() {
    use fallout_runtime::snapshot::Snapshot;
    use serde_json::{Value as Json, json};
    use std::{
        cell::RefCell,
        path::{Path, PathBuf},
        process::Command,
    };
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_NATIVE_PLAN_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let body = event(
        &[
            get(None, 1),
            get(Some(2), 1),
            call(None, 0x1001, &item(2)),
            get(None, 2),
        ]
        .concat(),
    );
    let (temporary, catalogue, _content) = fixture(&body);
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        temporary.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        input.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (mut world, handle, sequence, subject) = seed(Arc::clone(&catalogue), 0);
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: subject },
                },
            )],
        )
        .unwrap();
    let second = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let first_item = before.inventory_banks[0].items[0].id();
    world
        .remove_item_quantity(first_item, 1.try_into().unwrap())
        .unwrap();
    let changed = world.snapshot();
    let request = json!({"schema_version":1,"sequence":sequence,"occurrence":0,
        "intent":"engineering_observation","supplied_subject":subject,"explicit_player":null,
        "maximum_source_instructions":4096,"maximum_calls":128,"maximum_argument_bytes":65539,
        "maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,
        "maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,
        "maximum_query_variable_bytes":1024,"maximum_contributions":2,"maximum_report_bytes":1048576});
    let receipts = RefCell::new(Vec::new());
    let run = |name: &str,
               initial: &Snapshot,
               current: &Snapshot,
               request: &Json,
               extra: &[&str],
               report_target: Option<&Path>| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let initial_path = directory.join("initial.snapshot.json");
        let current_path = directory.join("current.snapshot.json");
        let request_path = directory.join("request.json");
        let report = report_target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory.join("report.json"));
        let initial_bytes = initial.encode(64 * 1024 * 1024).unwrap();
        let current_bytes = current.encode(64 * 1024 * 1024).unwrap();
        fs::write(&initial_path, &initial_bytes).unwrap();
        fs::write(&current_path, &current_bytes).unwrap();
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--snapshot-native-plan-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(&initial_path)
            .arg("--snapshot-native-current")
            .arg(&current_path)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        assert_eq!(
            fs::read(&initial_path).unwrap(),
            initial_bytes,
            "{name} changed initial"
        );
        assert_eq!(
            fs::read(&current_path).unwrap(),
            current_bytes,
            "{name} changed current"
        );
        receipts
            .borrow_mut()
            .push(json!({"name":name,"exit_code":output.status.code(),
            "initial_unchanged":true,"current_unchanged":true}));
        (output, report)
    };
    let mut first_report = None;
    for (name, current, total, first_count) in [
        ("cold-initial", &before, 8_589_934_590_u64, u32::MAX),
        ("cold-changed", &changed, 8_589_934_589, u32::MAX - 1),
    ] {
        let (output, path) = run(name, &before, current, &request, &[], None);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytes = fs::read(path).unwrap();
        let report: Json = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(report["outcome"]["trace"]["query"]["result"], total);
        assert_eq!(
            report["outcome"]["trace"]["query"]["contributions"],
            json!([
                [first_item, first_count],
                [before.inventory_banks[0].items[1].id(), u32::MAX]
            ])
        );
        assert_eq!(
            report["plan"]["call"]["scda_bytes"],
            json!({"start":10,"end":19})
        );
        assert_eq!(
            report["plan"]["call"]["argument_scda_bytes"],
            json!({"start":14,"end":19})
        );
        assert_eq!(report["plan"]["source"]["source_bytes"], json!(body));
        assert_eq!(report["plan"]["supplied_subject"], json!(subject));
        assert_eq!(report["plan"]["explicit_player"], Json::Null);
        assert_eq!(report["initial_world_dropped"], true);
        assert_eq!(report["strict_current_restore"], true);
        assert_eq!(report["canonical_state_unchanged"], true);
        assert_eq!(report["event_acknowledged"], false);
        assert_eq!(
            report["outcome"]["trace"]["original_numeric_return"],
            Json::Null
        );
        assert_eq!(
            report["outcome"]["trace"]["original_behavior_verified"],
            false
        );
        assert_eq!(report["state_revision"], current.state_revision);
        if name == "cold-initial" {
            first_report = Some((report, bytes));
        }
    }
    let (report, report_bytes) = first_report.unwrap();
    let mut prefixed = request.clone();
    prefixed["occurrence"] = json!(1);
    prefixed["supplied_subject"] = Json::Null;
    prefixed["explicit_player"] = json!(subject);
    let (output, path) = run("prefix", &before, &changed, &prefixed, &[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let prefix: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        prefix["outcome"]["trace"]["query"]["result"],
        8_589_934_589_u64
    );
    assert_eq!(prefix["plan"]["call"]["calling_reference_index"], 2);
    assert_eq!(
        prefix["plan"]["call"]["scda_bytes"],
        json!({"start":19,"end":32})
    );
    assert_eq!(
        prefix["plan"]["call"]["argument_scda_bytes"],
        json!({"start":27,"end":32})
    );
    assert_eq!(prefix["plan"]["explicit_player"], json!(subject));
    assert_eq!(prefix["plan"]["supplied_subject"], Json::Null);
    let mut empty_initial = before.clone();
    empty_initial.inventory_banks.clear();
    let (output, path) = run(
        "late-inventory",
        &empty_initial,
        &changed,
        &request,
        &[],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let late: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        late["outcome"]["trace"]["query"]["result"],
        8_589_934_589_u64
    );
    let source_counts = &report["plan"]["creation_counts"]["source"];
    let mut exact = request.clone();
    for (field, value) in [
        ("maximum_source_instructions", json!(6)),
        ("maximum_calls", json!(4)),
        ("maximum_argument_bytes", json!(20)),
        (
            "maximum_trace_source_bytes",
            source_counts["source_bytes"].clone(),
        ),
        ("maximum_trace_rows", source_counts["rows"].clone()),
        (
            "maximum_trace_variable_bytes",
            source_counts["variable_bytes"].clone(),
        ),
        (
            "maximum_trace_binding_uses",
            source_counts["binding_uses"].clone(),
        ),
        (
            "maximum_query_variable_bytes",
            report["plan"]["creation_counts"]["query_variable_bytes"].clone(),
        ),
        ("maximum_report_bytes", json!(report_bytes.len())),
    ] {
        exact[field] = value;
    }
    let (output, path) = run("all-exact", &before, &before, &exact, &[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(path).unwrap(), report_bytes);
    for field in [
        "maximum_source_instructions",
        "maximum_calls",
        "maximum_argument_bytes",
        "maximum_trace_source_bytes",
        "maximum_trace_rows",
        "maximum_trace_variable_bytes",
        "maximum_trace_binding_uses",
        "maximum_query_variable_bytes",
        "maximum_contributions",
        "maximum_report_bytes",
    ] {
        let mut short = exact.clone();
        short[field] = json!(short[field].as_u64().unwrap() - 1);
        let (output, path) = run(
            &format!("short-{field}"),
            &before,
            &before,
            &short,
            &[],
            None,
        );
        assert!(!output.status.success(), "{field}");
        assert!(!path.exists(), "{field}");
    }
    for (name, field, value, reason) in [
        (
            "faithful",
            "intent",
            json!("faithful"),
            "unverified_retail_semantics",
        ),
        (
            "missing-subject",
            "supplied_subject",
            Json::Null,
            "missing_subject",
        ),
        (
            "unknown-subject",
            "supplied_subject",
            json!(999),
            "host_query_unavailable",
        ),
        (
            "unknown-command",
            "occurrence",
            json!(2),
            "missing_implementation",
        ),
    ] {
        let mut refused = request.clone();
        refused[field] = value;
        let (output, path) = run(name, &before, &changed, &refused, &[], None);
        assert!(!output.status.success(), "{name}");
        let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(report["outcome"]["reason"], reason, "{name}");
        assert_eq!(report["plan"], Json::Null);
    }
    let (output, path) = run(
        "missing-current-bank",
        &before,
        &empty_initial,
        &request,
        &[],
        None,
    );
    assert!(!output.status.success());
    let missing: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(missing["outcome"]["reason"], "host_query_unavailable");
    assert!(missing["plan"].is_object());
    for (name, intent) in [
        (
            "intent-object-engineering-null",
            json!({"engineering_observation":null}),
        ),
        ("intent-object-faithful-null", json!({"faithful":null})),
    ] {
        let mut invalid = request.clone();
        invalid["intent"] = intent;
        let (output, path) = run(name, &before, &changed, &invalid, &[], None);
        assert!(!output.status.success() && !path.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("expected a string"));
    }
    let mut detached = before.clone();
    detached.instances[0].context = Context::default();
    for pending in &mut detached.pending_events {
        pending.context = Context::default();
    }
    detached.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 90)
        .unwrap()
        .value = Value::Uninitialized;
    let mut removed = detached.clone();
    removed
        .references
        .retain(|reference| reference.id != subject);
    removed.inventory_banks.retain(|bank| bank.owner != subject);
    let (output, path) = run("removed-caller", &detached, &removed, &request, &[], None);
    assert!(!output.status.success());
    let missing: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(missing["outcome"]["reason"], "host_query_unavailable");
    for variant in 0..7 {
        let mut current = before.clone();
        match variant {
            0 => {
                current.pending_events.remove(0);
            }
            1 => current.pending_events[0].context.target = None,
            2 => current.instances[0].context.target = None,
            3 => {
                current.instances[0].owner = Owner::Fragment {
                    activation: 2.try_into().unwrap(),
                }
            }
            4 => current.catalogue_sha256 = "0".repeat(64),
            5 => current.campaign = fallout_runtime::identity::CampaignId::generate().unwrap(),
            _ => current.instances[0].definition.version_sha256 = "0".repeat(64),
        }
        let (output, path) = run(
            &format!("changed-context-{variant}"),
            &before,
            &current,
            &request,
            &[],
            None,
        );
        assert!(!output.status.success(), "variant{variant}");
        assert!(!path.exists(), "variant{variant}");
    }
    let mut rebound =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    let other = rebound.register_reference(None).unwrap();
    rebound.initialize_inventory(other).unwrap();
    let rebound_handle = rebound.handle(before.instances[0].id).unwrap();
    rebound
        .assign(
            rebound_handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id: other },
                },
            )],
        )
        .unwrap();
    let (output, path) = run(
        "changed-resolved-caller",
        &before,
        &rebound.snapshot(),
        &prefixed,
        &[],
        None,
    );
    assert!(!output.status.success());
    assert!(!path.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fresh resolved caller or item"));
    let mut dynamic_world =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    let dynamic_handle = dynamic_world.handle(before.instances[0].id).unwrap();
    dynamic_world
        .assign(
            dynamic_handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x100) },
                },
            )],
        )
        .unwrap();
    let dynamic_initial = dynamic_world.snapshot();
    let mut dynamic_request = request.clone();
    dynamic_request["occurrence"] = json!(3);
    let (output, path) = run(
        "dynamic-item",
        &dynamic_initial,
        &dynamic_initial,
        &dynamic_request,
        &[],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dynamic: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        dynamic["outcome"]["trace"]["query"]["result"],
        8_589_934_590_u64
    );
    assert_eq!(
        dynamic["plan"]["call"]["argument_scda_bytes"],
        json!({"start":45,"end":50})
    );
    let mut oversized = form(0x100);
    oversized.origin_plugin = "x".repeat(32 * 1024);
    let mut large_current = dynamic_initial.clone();
    large_current.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 90)
        .unwrap()
        .value = Value::Reference {
        value: ReferenceValue::Content { key: oversized },
    };
    for (name, initial, current, selected) in [
        (
            "large-current-item",
            &dynamic_initial,
            &large_current,
            &dynamic_request,
        ),
        (
            "large-initial-item",
            &large_current,
            &large_current,
            &dynamic_request,
        ),
        (
            "large-initial-caller",
            &large_current,
            &large_current,
            &prefixed,
        ),
    ] {
        let (output, path) = run(name, initial, current, selected, &[], None);
        assert!(!output.status.success());
        assert!(!path.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("reference variable bytes"));
    }
    for (name, value, reason) in [
        (
            "uninitialized-item",
            Value::Uninitialized,
            Some("argument_resolution"),
        ),
        (
            "null-item",
            Value::Reference {
                value: ReferenceValue::Null,
            },
            Some("argument_needs_content_reference"),
        ),
        (
            "changed-item",
            Value::Reference {
                value: ReferenceValue::Content { key: form(0x400) },
            },
            None,
        ),
    ] {
        dynamic_world
            .assign(dynamic_handle, &[(90, value)])
            .unwrap();
        let (output, path) = run(
            name,
            &dynamic_initial,
            &dynamic_world.snapshot(),
            &dynamic_request,
            &[],
            None,
        );
        assert!(!output.status.success(), "{name}");
        if let Some(reason) = reason {
            let refused: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert_eq!(refused["outcome"]["reason"], reason);
            assert!(refused["plan"].is_object());
        } else {
            assert!(!path.exists());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("fresh resolved caller or item")
            );
        }
    }
    for (name, field, value) in [
        ("wrong-schema", "schema_version", json!(2)),
        ("non-head", "sequence", json!(second)),
        ("missing-sequence", "sequence", json!(999)),
        ("zero-sequence", "sequence", json!(0)),
        ("out-of-range", "occurrence", json!(4)),
        ("unknown-field", "enqueue", json!(true)),
        ("query-ceiling", "maximum_query_variable_bytes", json!(1025)),
        (
            "report-ceiling",
            "maximum_report_bytes",
            json!(8 * 1024 * 1024 + 1),
        ),
        ("zero-report", "maximum_report_bytes", json!(0)),
        (
            "contribution-ceiling",
            "maximum_contributions",
            json!(65537),
        ),
        ("request-byte-limit", "extra", json!("x".repeat(16 * 1024))),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, path) = run(name, &before, &before, &invalid, &[], None);
        assert!(!output.status.success(), "{name}");
        assert!(!path.exists(), "{name}");
    }
    for field in ["supplied_subject", "explicit_player", "maximum_calls"] {
        let mut invalid = request.clone();
        invalid.as_object_mut().unwrap().remove(field);
        let (output, path) = run(
            &format!("omitted-{field}"),
            &before,
            &before,
            &invalid,
            &[],
            None,
        );
        assert!(!output.status.success());
        assert!(!path.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("missing field"));
    }
    for (name, extra) in [
        ("conflict-native", vec!["--native-capabilities"]),
        ("conflict-player", vec!["--player-id", "1"]),
        (
            "conflict-copy",
            vec!["--snapshot-copy-request", "unused.json"],
        ),
    ] {
        let (output, path) = run(name, &before, &before, &request, &extra, None);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    let protected = install.join("protected-report.json");
    let (output, path) = run(
        "protected-report",
        &before,
        &before,
        &request,
        &[],
        Some(&protected),
    );
    assert!(!output.status.success());
    assert!(!path.exists());
    let existing = evidence.join("existing-report.json");
    fs::write(&existing, b"preserve").unwrap();
    let (output, _) = run(
        "existing-report",
        &before,
        &before,
        &request,
        &[],
        Some(&existing),
    );
    assert!(!output.status.success());
    assert_eq!(fs::read(&existing).unwrap(), b"preserve");
    let alias = evidence.join("hardlink-report.json");
    fs::hard_link(&existing, &alias).unwrap();
    let (output, _) = run(
        "hardlink-report",
        &before,
        &before,
        &request,
        &[],
        Some(&alias),
    );
    assert!(!output.status.success());
    assert_eq!(fs::read(&existing).unwrap(), b"preserve");
    assert_eq!(world.snapshot(), changed);
    let cases = receipts.into_inner();
    fs::write(evidence.join("scope.json"),serde_json::to_vec_pretty(&json!({
        "scope":"owned_source_native_plan_after_initial_world_drop_and_strict_current_restore",
        "original_executed":false,"retail_parity_accepted":false,"actual_cli_calls":cases.len(),
        "cases":cases,"initial_snapshot":before,"changed_snapshot":changed,
        "exact_report_bytes":report_bytes.len(),"initial_count":8_589_934_590_u64,"changed_count":8_589_934_589_u64
    })).unwrap()).unwrap();
}
