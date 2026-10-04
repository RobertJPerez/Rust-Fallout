mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    store::RecordStore,
};
use fallout_runtime::{
    World,
    events::{Clocks, Context, Pending, Trigger},
    execution::event_request::{self as enqueue, Error, Intent, Limits, Preparation, Selection},
    foreign::Content,
    identity::{CampaignId, InstanceId, Owner, ReferenceId, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use std::{fs, sync::Arc};
fn reference(n: u64) -> ReferenceId {
    ReferenceId(n.try_into().unwrap())
}
fn instance(n: u64) -> InstanceId {
    InstanceId(n.try_into().unwrap())
}
fn owner(n: u64) -> Owner {
    Owner::Placed {
        reference: reference(n),
    }
}
fn operators() -> Operators {
    Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(n, text)| Operator {
            code: n as u32,
            precedence: n as u8,
            spelling: text.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap()
}
fn fixture() -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let dir = tempfile::tempdir().unwrap();
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&28u32.to_le_bytes());
    schr[12..16].copy_from_slice(&1u32.to_le_bytes());
    let compiled = [
        0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0, 0x10, 0, 6, 0, 5, 0, 4, 0, 0, 0, 0x11, 0,
        0, 0,
    ];
    let mut slsd = [0; 24];
    slsd[..4].copy_from_slice(&42u32.to_le_bytes());
    let body = [
        field(b"SCHR", &schr),
        field(b"SCDA", &compiled),
        field(b"SLSD", &slsd),
        field(b"SCVR", b"local_42\0"),
    ]
    .concat();
    fs::write(
        dir.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &body),
            record(b"ACTI", 0x400, 0, &[]),
            record(b"CELL", 0x600, 0, &field(b"DATA", &[1])),
            record(b"QUST", 0x700, 0, &field(b"SCRI", &0x300u32.to_le_bytes())),
        ]
        .concat(),
    )
    .unwrap();
    let mut store =
        RecordStore::open_nv_headers(dir.path(), &["FalloutNV.esm".into()], Default::default())
            .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (dir, catalogue, content)
}
fn world(catalogue: Arc<Catalogue>) -> World<'static> {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x29; 16]).unwrap(),
    )
    .unwrap();
    for n in 1..=3 {
        assert_eq!(world.register_reference(None).unwrap(), reference(n));
    }
    world.initialize_inventory(reference(1)).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x400));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc12345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"BLOB",
            bytes: vec![0, 128, 255],
        });
    world
        .add_item(reference(1), facts, 19.try_into().unwrap())
        .unwrap();
    let pose = fallout_runtime::reference_state::Pose::from_source(
        &fallout_data::world::Transform {
            position: [f32::from_bits(0x80000000), f32::from_bits(1), 19.0],
            rotation: [0.0, 1.0, 3.0],
        },
        None,
    )
    .unwrap();
    let state = fallout_runtime::reference_state::State::new(form(0x600), pose, false).unwrap();
    let stage = world
        .stage_reference_state(&world.reference_view(reference(3)).unwrap(), state)
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    for n in 1..=2 {
        let handle = world
            .create_instance(
                catalogue
                    .record_scripts(&form(0x300))
                    .next()
                    .unwrap()
                    .handle(),
                owner(n),
                Context::default(),
            )
            .unwrap();
        world
            .assign(
                handle,
                &[(
                    42,
                    Value::Number {
                        bits: if n == 1 {
                            0x7ff8123456789abc
                        } else {
                            0x8000000000000000
                        },
                    },
                )],
            )
            .unwrap();
    }
    world
        .advance_clocks(Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        })
        .unwrap();
    world
        .enqueue(
            world.handle(instance(1)).unwrap(),
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    world
}
fn context() -> Context {
    Context {
        calling_reference: Some(reference(1)),
        containing_reference: Some(reference(2)),
        target: Some(ReferenceValue::Live { id: reference(3) }),
        arguments: vec![
            ReferenceValue::Null,
            ReferenceValue::Content { key: form(0x400) },
        ],
    }
}
fn selection<'a>(
    owner: &'a Owner,
    definition: &'a fallout_data::loaded_scripts::Handle,
) -> Selection<'a> {
    Selection {
        instance: instance(2),
        expected_owner: owner,
        definition,
        begin_scda_offset: 14,
        event_id: 5,
        intent: Intent::Engineering,
    }
}
fn ready<'p, 's>(
    world: &World<'_>,
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    selection: Selection<'p>,
    context: &'p Context,
    limits: Limits,
) -> Box<enqueue::Request<'p, 's>> {
    match enqueue::prepare(world, sources, content, selection, context, limits).unwrap() {
        Preparation::Ready(request) => request,
        _ => panic!("engineering refused"),
    }
}
#[test]
fn exact_second_instance_second_block_appends_one_literal_pending_at_unchanged_current_clocks() {
    let (_dir, catalogue, content) = fixture();
    let world = world(Arc::clone(&catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources =
        PreparedSources::load(&catalogue, &model, &Signatures::new(), Default::default()).unwrap();
    let definition = definition(&catalogue);
    let owner = owner(2);
    let context = context();
    let request = ready(
        &world,
        &sources,
        &content,
        selection(&owner, &definition),
        &context,
        Default::default(),
    );
    assert_eq!(request.trace().event_scda_bytes, 14..28);
    assert_eq!(request.trace().counts.instructions, 2);
    assert_eq!(request.trace().counts.source_bytes, 28);
    let result = request.apply(before.clone(), Default::default()).unwrap();
    assert_eq!(result.sequence, 2);
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected.next_event_sequence = 3;
    expected.pending_events.push(Pending {
        sequence: 2,
        instance: instance(2),
        trigger: Trigger::Block {
            event_id: 5,
            begin_byte_offset: 14,
        },
        context: context.clone(),
        arrived: Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        },
    });
    assert_eq!(result.snapshot, expected);
    assert_eq!(world.snapshot(), before);
    let bytes = result.snapshot.encode(1024 * 1024).unwrap();
    let cold = World::restore(
        Arc::clone(&catalogue),
        Snapshot::decode(&bytes, Default::default()).unwrap(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(cold.snapshot(), expected);
    assert!(matches!(
        request.apply(result.snapshot, Default::default()),
        Err(Error::Input(_))
    ));
}
#[test]
fn wrong_owner_definition_block_context_and_private_capacity_fail_before_result() {
    let (_dir, catalogue, content) = fixture();
    let world = world(Arc::clone(&catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources =
        PreparedSources::load(&catalogue, &model, &Signatures::new(), Default::default()).unwrap();
    let definition = definition(&catalogue);
    let owner = owner(2);
    let context = context();
    let mut invalid_definition = definition.clone();
    invalid_definition.version_sha256 = "0".repeat(64);
    let wrong_owner = crate::owner(1);
    for case in 0..6 {
        let mut selected = selection(&owner, &definition);
        match case {
            0 => selected.expected_owner = &wrong_owner,
            1 => selected.definition = &invalid_definition,
            2 => selected.begin_scda_offset = 0,
            3 => selected.event_id = 0,
            4 => selected.begin_scda_offset = 15,
            5 => selected.instance = instance(999),
            _ => unreachable!(),
        }
        assert!(
            enqueue::prepare(
                &world,
                &sources,
                &content,
                selected,
                &context,
                Default::default()
            )
            .is_err(),
            "case {case}"
        );
    }
    for case in 0..4 {
        let mut bad = context.clone();
        match case {
            0 => bad.calling_reference = Some(reference(999)),
            1 => bad.containing_reference = Some(reference(999)),
            2 => bad.target = Some(ReferenceValue::Live { id: reference(999) }),
            3 => bad
                .arguments
                .push(ReferenceValue::Live { id: reference(999) }),
            _ => unreachable!(),
        }
        assert!(
            enqueue::prepare(
                &world,
                &sources,
                &content,
                selection(&owner, &definition),
                &bad,
                Default::default()
            )
            .is_err()
        );
    }
    let request = ready(
        &world,
        &sources,
        &content,
        selection(&owner, &definition),
        &context,
        Default::default(),
    );
    let limit = fallout_runtime::Limits {
        max_pending_events: 1,
        ..Default::default()
    };
    assert!(matches!(
        request.apply(before.clone(), limit),
        Err(Error::State(fallout_runtime::Error::Capacity(
            "pending events"
        )))
    ));
    for case in 0..6 {
        let mut input = before.clone();
        match case {
            0 => input.campaign = CampaignId::from_bytes([1; 16]).unwrap(),
            1 => input.state_revision += 1,
            2 => input.clocks.tick += 1,
            3 => input.catalogue_sha256 = "0".repeat(64),
            4 => input.instances[1].definition = invalid_definition.clone(),
            5 => input.references.retain(|r| r.id != reference(3)),
            _ => unreachable!(),
        }
        assert!(
            request.apply(input, Default::default()).is_err(),
            "private case {case}"
        );
    }
    for case in 0..2 {
        let mut input = before.clone();
        if case == 0 {
            input.next_event_sequence = u64::MAX;
        } else {
            input.state_revision = u64::MAX;
        }
        let supplied =
            World::restore(Arc::clone(&catalogue), input.clone(), Default::default()).unwrap();
        let request = ready(
            &supplied,
            &sources,
            &content,
            selection(&owner, &definition),
            &context,
            Default::default(),
        );
        assert!(matches!(
            request.apply(input, Default::default()),
            Err(Error::State(fallout_runtime::Error::Capacity(_)))
        ));
    }
    let mut faithful = selection(&wrong_owner, &invalid_definition);
    faithful.intent = Intent::Faithful;
    assert!(matches!(
        enqueue::prepare(
            &world,
            &sources,
            &content,
            faithful,
            &context,
            Limits {
                maximum_trace_bytes: 0,
                ..Default::default()
            }
        )
        .unwrap(),
        Preparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
}
#[test]
fn all_six_creation_caps_admit_exact_and_refuse_one_under() {
    let (_dir, catalogue, content) = fixture();
    let world = world(Arc::clone(&catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources =
        PreparedSources::load(&catalogue, &model, &Signatures::new(), Default::default()).unwrap();
    let definition = definition(&catalogue);
    let owner = owner(2);
    let context = context();
    let selected = selection(&owner, &definition);
    let request = ready(
        &world,
        &sources,
        &content,
        selected,
        &context,
        Default::default(),
    );
    let counts = request.trace().counts;
    let trace_bytes = serde_json::to_vec(request.trace()).unwrap().len();
    for field in 0..6 {
        let mut limits = Limits::default();
        match field {
            0 => limits.maximum_source_bytes = counts.source_bytes,
            1 => limits.maximum_source_instructions = counts.instructions,
            2 => limits.maximum_context_arguments = counts.context_arguments,
            3 => limits.maximum_variable_bytes = counts.variable_bytes,
            4 => limits.maximum_source_receipts = counts.source_receipts,
            5 => limits.maximum_trace_bytes = trace_bytes,
            _ => unreachable!(),
        }
        ready(&world, &sources, &content, selected, &context, limits)
            .apply(before.clone(), Default::default())
            .unwrap();
        match field {
            0 => limits.maximum_source_bytes -= 1,
            1 => limits.maximum_source_instructions -= 1,
            2 => limits.maximum_context_arguments -= 1,
            3 => limits.maximum_variable_bytes -= 1,
            4 => limits.maximum_source_receipts -= 1,
            5 => limits.maximum_trace_bytes -= 1,
            _ => unreachable!(),
        }
        assert!(
            matches!(
                enqueue::prepare(&world, &sources, &content, selected, &context, limits),
                Err(Error::Capacity(_))
            ),
            "cap {field}"
        );
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "frozen replacement CLI and authored executable metadata; explicit saved enqueue only, no original launch"]
fn cli_saved_event_request_helper() {
    use serde_json::{Value as Json, json};
    use std::{
        cell::Cell,
        path::{Path, PathBuf},
        process::Command,
    };
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_EVENT_REQUEST_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let (dir, catalogue, content) = fixture();
    let world = world(Arc::clone(&catalogue));
    let before = world.snapshot();
    let context = context();
    let definition = definition(&catalogue);
    let owner = owner(2);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources =
        PreparedSources::load(&catalogue, &model, &Signatures::new(), Default::default()).unwrap();
    let operation = ready(
        &world,
        &sources,
        &content,
        selection(&owner, &definition),
        &context,
        Default::default(),
    );
    let counts = operation.trace().counts;
    let compact_trace = serde_json::to_vec(operation.trace()).unwrap().len();
    let defaults = Limits::default();
    let prepared = fallout_runtime::programs::Limits::default();
    let request = json!({"schema_version":1,"instance":2,"owner":owner,"definition":definition,"begin_scda_offset":14,"event_id":5,"intent":"engineering","context":context,
        "maximum_source_bytes":defaults.maximum_source_bytes,"maximum_source_instructions":defaults.maximum_source_instructions,"maximum_context_arguments":defaults.maximum_context_arguments,"maximum_variable_bytes":defaults.maximum_variable_bytes,"maximum_source_receipts":defaults.maximum_source_receipts,"maximum_trace_bytes":defaults.maximum_trace_bytes,
        "maximum_prepared_instructions":prepared.maximum_instructions,"maximum_prepared_operand_uses":prepared.maximum_uses,"maximum_prepared_tokens":prepared.maximum_tokens,"maximum_prepared_record_bytes":prepared.maximum_attempted_record_bytes,
        "maximum_result_snapshot_bytes":64*1024*1024,"maximum_report_bytes":8*1024*1024});
    let mut expected = before.clone();
    expected.state_revision += 1;
    expected.next_event_sequence = 3;
    expected.pending_events.push(Pending {
        sequence: 2,
        instance: instance(2),
        trigger: Trigger::Block {
            event_id: 5,
            begin_byte_offset: 14,
        },
        context: context.clone(),
        arrived: Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        },
    });
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        dir.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        metadata.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let source_before = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let exe_before = fs::read(install.join("FalloutNV.exe")).unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let calls = Cell::new(0usize);
    let run_on = |name: &str,
                  input: &[u8],
                  raw: &[u8],
                  result_override: Option<&Path>,
                  report_override: Option<&Path>,
                  extra: &[&str]| {
        let work = evidence.join(name);
        fs::create_dir(&work).unwrap();
        let input_path = work.join("input.snapshot.json");
        let request_path = work.join("request.json");
        fs::write(&input_path, input).unwrap();
        fs::write(&request_path, raw).unwrap();
        let result =
            result_override.map_or_else(|| work.join("result.snapshot.json"), Path::to_path_buf);
        let report = report_override.map_or_else(|| work.join("report.json"), Path::to_path_buf);
        let order_bytes = fs::read(&order).unwrap();
        calls.set(calls.get() + 1);
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--snapshot-event-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(&input_path)
            .arg("--snapshot-output")
            .arg(&result)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(work.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(work.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(work.join("result.json"),serde_json::to_vec_pretty(&json!({"success":output.status.success(),"exit_code":output.status.code(),"snapshot_exists":result.exists(),"report_exists":report.exists()})).unwrap()).unwrap();
        assert_eq!(
            fs::read(&input_path).unwrap(),
            input,
            "{name}: input changed"
        );
        assert_eq!(
            fs::read(&request_path).unwrap(),
            raw,
            "{name}: request changed"
        );
        assert_eq!(
            fs::read(&order).unwrap(),
            order_bytes,
            "{name}: order changed"
        );
        (output, report, result)
    };
    let run = |name: &str, input: &Snapshot, request: &Json| {
        run_on(
            name,
            &input.encode(64 * 1024 * 1024).unwrap(),
            &serde_json::to_vec(request).unwrap(),
            None,
            None,
            &[],
        )
    };
    let refused = |name: &str, input: &Snapshot, request: &Json| {
        let (output, report, result) = run(name, input, request);
        assert!(!output.status.success(), "{name}: succeeded");
        assert!(!report.exists(), "{name}: report");
        assert!(!result.exists(), "{name}: result");
    };
    let (output, report_path, result_path) = run("copy-0", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(&report_path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    let actual = Snapshot::decode(&fs::read(&result_path).unwrap(), Default::default()).unwrap();
    assert_eq!(actual, expected);
    let cold = World::restore(Arc::clone(&catalogue), actual.clone(), Default::default()).unwrap();
    assert_eq!(cold.snapshot(), expected);
    assert_eq!(report["snapshot_event_request"]["sequence"], 2);
    assert_eq!(
        report["snapshot_event_request"]["queued_event"],
        serde_json::to_value(expected.pending_events.last().unwrap()).unwrap()
    );
    assert_eq!(
        report["snapshot_event_request"]["trace"]["event_scda_bytes"],
        json!({"start":14,"end":28})
    );
    assert_eq!(
        report["snapshot_event_request"]["trace"]["counts"]["instructions"],
        2
    );
    assert_eq!(report["prepared_sources"]["counts"]["instructions"], 4);
    for field in [
        "event_executed",
        "event_acknowledged",
        "clocks_advanced",
        "original_dispatch_verified",
        "faithful_execution_admitted",
        "retail_parity_accepted",
    ] {
        assert_eq!(report[field], false, "{field}");
    }
    for (name, selected_owner) in [
        ("quest-owner", Owner::Quest { key: form(0x700) }),
        (
            "fragment-owner",
            Owner::Fragment {
                activation: 9_007_199_254_741_099u64.try_into().unwrap(),
            },
        ),
    ] {
        let mut input = before.clone();
        input.instances[1].owner = selected_owner.clone();
        let mut changed = request.clone();
        changed["owner"] = json!(selected_owner);
        let (output, _, result) = run(name, &input, &changed);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut target = expected.clone();
        target.instances[1].owner = selected_owner;
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            target
        );
    }
    // Neither persistent instance IDs nor journal sequences pass through f64.
    let large = 9_007_199_254_741_099u64;
    let mut large_input = before.clone();
    large_input.instances[1].id = instance(large);
    large_input.next_instance = large + 1;
    let mut large_request = request.clone();
    large_request["instance"] = json!(large);
    let (output, path, result) = run("large-instance", &large_input, &large_request);
    assert!(output.status.success());
    let mut target = expected.clone();
    target.instances[1].id = instance(large);
    target.next_instance = large + 1;
    target.pending_events.last_mut().unwrap().instance = instance(large);
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        target
    );
    let r: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        r["snapshot_event_request"]["trace"]["selection"]["instance"],
        large
    );
    let mut large_input = before.clone();
    large_input.next_event_sequence = large;
    let (output, path, result) = run("large-sequence", &large_input, &request);
    assert!(output.status.success());
    let mut target = expected.clone();
    target.next_event_sequence = large + 1;
    target.pending_events.last_mut().unwrap().sequence = large;
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        target
    );
    let r: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(r["snapshot_event_request"]["sequence"], large);
    let mut explicit_null = request.clone();
    explicit_null["context"] =
        json!({"calling_reference":null,"containing_reference":null,"target":null,"arguments":[]});
    let (output, _, result) = run("explicit-null-context", &before, &explicit_null);
    assert!(output.status.success());
    let mut target = expected.clone();
    target.pending_events.last_mut().unwrap().context = Context::default();
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        target
    );
    let mut null_value = request.clone();
    null_value["context"]["target"] = json!({"kind":"null"});
    let (output, _, result) = run("explicit-stored-null-target", &before, &null_value);
    assert!(output.status.success());
    let mut target = expected.clone();
    target.pending_events.last_mut().unwrap().context.target = Some(ReferenceValue::Null);
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        target
    );
    let mut first = request.clone();
    first["instance"] = json!(1);
    first["owner"] = json!(crate::owner(1));
    first["begin_scda_offset"] = json!(0);
    first["event_id"] = json!(0);
    let (output, _, result) = run("explicit-first-block", &before, &first);
    assert!(output.status.success());
    let mut target = expected.clone();
    target.pending_events.last_mut().unwrap().instance = instance(1);
    target.pending_events.last_mut().unwrap().trigger = Trigger::Block {
        event_id: 0,
        begin_byte_offset: 0,
    };
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        target
    );
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    let (output, path, result) = run("faithful", &before, &faithful);
    assert!(!output.status.success());
    assert!(!result.exists());
    let r: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(r["snapshot_event_request"]["status"], "unsupported");
    assert_eq!(
        r["snapshot_event_request"]["reason"],
        "unverified_retail_semantics"
    );
    for (name, pointer, value) in [
        ("zero-instance", "/instance", json!(0)),
        ("missing-instance", "/instance", json!(999)),
        ("other-instance-owner", "/instance", json!(1)),
        ("wrong-owner", "/owner", json!(crate::owner(1))),
        (
            "wrong-version",
            "/definition/version_sha256",
            json!("0".repeat(64)),
        ),
        (
            "wrong-definition",
            "/definition/key/record/local_id",
            json!(0x301),
        ),
        (
            "wrong-header-offset",
            "/definition/key/header_decoded_offset",
            json!(6),
        ),
        ("wrong-event", "/event_id", json!(0)),
        ("wrong-begin", "/begin_scda_offset", json!(0)),
        ("interior-offset", "/begin_scda_offset", json!(15)),
        ("end-offset", "/begin_scda_offset", json!(24)),
        ("descriptor-overflow", "/event_id", json!(65536)),
        ("missing-caller", "/context/calling_reference", json!(999)),
        (
            "missing-container",
            "/context/containing_reference",
            json!(999),
        ),
        ("missing-target", "/context/target/id", json!(999)),
        (
            "missing-argument",
            "/context/arguments/0",
            json!({"kind":"live","id":999}),
        ),
        (
            "invalid-content-reference",
            "/context/arguments/1/key/origin_plugin",
            json!("FalloutNV.esm"),
        ),
        ("bad-intent", "/intent", json!("automatic")),
        ("bad-schema", "/schema_version", json!(2)),
    ] {
        let mut invalid = request.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        refused(name, &before, &invalid);
    }
    for field in [
        "calling_reference",
        "containing_reference",
        "target",
        "arguments",
    ] {
        let mut invalid = request.clone();
        invalid["context"].as_object_mut().unwrap().remove(field);
        refused(&format!("missing-context-field-{field}"), &before, &invalid);
    }
    for field in [
        "instance",
        "owner",
        "definition",
        "event_id",
        "begin_scda_offset",
        "intent",
        "context",
        "maximum_source_bytes",
        "maximum_trace_bytes",
        "maximum_prepared_record_bytes",
        "maximum_result_snapshot_bytes",
        "maximum_report_bytes",
    ] {
        let mut invalid = request.clone();
        invalid.as_object_mut().unwrap().remove(field);
        refused(&format!("missing-request-field-{field}"), &before, &invalid);
    }
    for (name, path, field) in [
        ("unknown-field", "", "unknown"),
        ("unknown-context", "/context", "unknown"),
        ("unknown-owner", "/owner", "unknown"),
        ("object-mask-unavailable", "", "mask"),
        ("initializers-unavailable", "", "initializers"),
    ] {
        let mut invalid = request.clone();
        invalid
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!(1));
        refused(name, &before, &invalid);
    }
    for (name, mut input) in [
        ("old-schema", before.clone()),
        ("wrong-cohort", before.clone()),
        ("revision-max", before.clone()),
        ("sequence-max", before.clone()),
        ("stale-instance-version", before.clone()),
        ("missing-live-context-in-input", before.clone()),
    ] {
        match name {
            "old-schema" => input.schema_version = 3,
            "wrong-cohort" => input.catalogue_sha256 = "0".repeat(64),
            "revision-max" => input.state_revision = u64::MAX,
            "sequence-max" => input.next_event_sequence = u64::MAX,
            "stale-instance-version" => {
                input.instances[1].definition.version_sha256 = "0".repeat(64)
            }
            "missing-live-context-in-input" => {
                input.references.retain(|r| r.id != reference(3));
                input.reference_states.clear();
            }
            _ => unreachable!(),
        }
        if name == "old-schema" {
            let (output, report, result) = run_on(
                name,
                &serde_json::to_vec(&input).unwrap(),
                &serde_json::to_vec(&request).unwrap(),
                None,
                None,
                &[],
            );
            assert!(!output.status.success());
            assert!(!report.exists());
            assert!(!result.exists());
        } else {
            refused(name, &input, &request);
        }
    }
    let mut full = before.clone();
    full.pending_events = (1..=65_536)
        .map(|sequence| Pending {
            sequence,
            instance: instance(1),
            trigger: Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context: Context::default(),
            arrived: before.clocks,
        })
        .collect();
    full.next_event_sequence = 65_537;
    refused("pending-capacity", &full, &request);
    for (field, value) in [
        ("maximum_source_bytes", counts.source_bytes),
        ("maximum_source_instructions", counts.instructions),
        ("maximum_context_arguments", counts.context_arguments),
        ("maximum_variable_bytes", counts.variable_bytes),
        ("maximum_source_receipts", counts.source_receipts),
        ("maximum_trace_bytes", compact_trace),
    ] {
        let mut exact = request.clone();
        exact[field] = json!(value);
        let (output, _, result) = run(&format!("exact-{field}"), &before, &exact);
        assert!(
            output.status.success(),
            "{field}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            expected
        );
        exact[field] = json!(value - 1);
        refused(&format!("under-{field}"), &before, &exact);
    }
    for (field, value) in [
        ("maximum_prepared_instructions", 4),
        ("maximum_prepared_record_bytes", 105),
    ] {
        let mut exact = request.clone();
        exact[field] = json!(value);
        let (output, _, result) = run(&format!("exact-{field}"), &before, &exact);
        assert!(output.status.success());
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            expected
        );
        exact[field] = json!(value - 1);
        refused(&format!("under-{field}"), &before, &exact);
    }
    for field in ["maximum_prepared_operand_uses", "maximum_prepared_tokens"] {
        let mut exact = request.clone();
        exact[field] = json!(0);
        let (output, _, result) = run(&format!("zero-{field}"), &before, &exact);
        assert!(output.status.success());
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            expected
        );
    }
    for (field, value) in [
        (
            "maximum_result_snapshot_bytes",
            fs::metadata(&result_path).unwrap().len(),
        ),
        ("maximum_report_bytes", report_bytes.len() as u64),
    ] {
        let mut exact = request.clone();
        exact[field] = json!(value);
        let name = if field == "maximum_report_bytes" {
            "exact0".to_string()
        } else {
            format!("exact-{field}")
        };
        let (output, _, result) = run(&name, &before, &exact);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            expected
        );
        exact[field] = json!(value - 1);
        refused(&format!("under-{field}"), &before, &exact);
    }
    for (field, value) in [
        ("maximum_source_bytes", defaults.maximum_source_bytes),
        (
            "maximum_source_instructions",
            defaults.maximum_source_instructions,
        ),
        (
            "maximum_context_arguments",
            defaults.maximum_context_arguments,
        ),
        ("maximum_variable_bytes", defaults.maximum_variable_bytes),
        ("maximum_source_receipts", defaults.maximum_source_receipts),
        ("maximum_trace_bytes", defaults.maximum_trace_bytes),
        (
            "maximum_prepared_instructions",
            prepared.maximum_instructions,
        ),
        ("maximum_prepared_operand_uses", prepared.maximum_uses),
        ("maximum_prepared_tokens", prepared.maximum_tokens),
        (
            "maximum_prepared_record_bytes",
            prepared.maximum_attempted_record_bytes,
        ),
        ("maximum_result_snapshot_bytes", 64 * 1024 * 1024),
        ("maximum_report_bytes", 8 * 1024 * 1024),
    ] {
        let mut invalid = request.clone();
        invalid[field] = json!(value + 1);
        refused(&format!("ceiling-{field}"), &before, &invalid);
    }
    let mut dup = serde_json::to_vec(&request).unwrap();
    dup.pop();
    dup.extend_from_slice(b",\"instance\":2}");
    for (name, bytes) in [
        ("duplicate-field", dup),
        ("malformed-request", b"{".to_vec()),
        ("oversized-request", vec![b' '; 16 * 1024 + 1]),
    ] {
        let (output, report, result) = run_on(
            name,
            &before.encode(64 * 1024 * 1024).unwrap(),
            &bytes,
            None,
            None,
            &[],
        );
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, extra) in [
        ("conflicting-prepared", vec!["--prepared-sources"]),
        ("conflicting-player", vec!["--player-id", "1"]),
        ("conflicting-native", vec!["--native-capabilities"]),
        (
            "conflicting-copy",
            vec!["--snapshot-copy-request", "missing"],
        ),
        (
            "conflicting-reference-boot",
            vec!["--reference-boot-request", "missing"],
        ),
        (
            "conflicting-quest-output",
            vec!["--quest-boot-output", "missing"],
        ),
    ] {
        let (output, report, result) = run_on(
            name,
            &before.encode(64 * 1024 * 1024).unwrap(),
            &serde_json::to_vec(&request).unwrap(),
            None,
            None,
            &extra,
        );
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    let blocked = evidence.join("preserved.json");
    fs::write(&blocked, b"preserved").unwrap();
    for (name, result_override, report_override) in [
        ("existing-result", Some(blocked.as_path()), None),
        ("existing-report", None, Some(blocked.as_path())),
    ] {
        let (output, _, _) = run_on(
            name,
            &before.encode(64 * 1024 * 1024).unwrap(),
            &serde_json::to_vec(&request).unwrap(),
            result_override,
            report_override,
            &[],
        );
        assert!(!output.status.success());
        assert_eq!(fs::read(&blocked).unwrap(), b"preserved");
    }
    for (name, result_override, report_override) in [
        (
            "protected-result",
            Some(install.join("Data/blocked-result.json")),
            None,
        ),
        (
            "protected-report",
            None,
            Some(install.join("Data/blocked-report.json")),
        ),
    ] {
        let (output, report, result) = run_on(
            name,
            &before.encode(64 * 1024 * 1024).unwrap(),
            &serde_json::to_vec(&request).unwrap(),
            result_override.as_deref(),
            report_override.as_deref(),
            &[],
        );
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    let alias = evidence.join("alias.json");
    let (output, report, result) = run_on(
        "identical-output-paths",
        &before.encode(64 * 1024 * 1024).unwrap(),
        &serde_json::to_vec(&request).unwrap(),
        Some(&alias),
        Some(&alias),
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!result.exists());
    if cfg!(windows) {
        let upper = evidence.join("ALIAS.json");
        let (output, report, result) = run_on(
            "case-output-alias",
            &before.encode(64 * 1024 * 1024).unwrap(),
            &serde_json::to_vec(&request).unwrap(),
            Some(&alias),
            Some(&upper),
            &[],
        );
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source_before
    );
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), exe_before);
    assert_eq!(world.snapshot(), before);
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":calls.get(),"literal_instance":2,"literal_begin_offset":14,"literal_event_id":5,"literal_sequence":2,"large_instance_and_sequence":large,"selected_source_span_instructions":2,"whole_prepared_instructions":4,"original_launched":false,"original_dispatch_verified":false,"retail_parity_accepted":false,"whole_snapshot_equal":true,"cold_restore_verified":true,"unrelated_inventory_pose_locals_pending_context_preserved":true,"original_source_bytes_unchanged":true})).unwrap()).unwrap();
}
