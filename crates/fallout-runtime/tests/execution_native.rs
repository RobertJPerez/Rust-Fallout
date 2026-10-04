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
        "actual_cli_calls":30,"input_schema":before.schema_version,"input_snapshot":before,
        "requested_occurrences":[3,1,0,3],"aggregate_contributions":7,"exact_report_bytes":report_bytes.len(),
        "original_numeric_return":null,"retail_parity_accepted":false
    })).unwrap()).unwrap();
}
