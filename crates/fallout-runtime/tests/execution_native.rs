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
