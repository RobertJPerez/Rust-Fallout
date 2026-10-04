mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    World,
    events::{Clocks, Context, Trigger},
    execution::{literal_assignment as literal, local_copy},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use std::{fs, sync::Arc};
fn instruction(out: &mut Vec<u8>, opcode: u16, bytes: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((bytes.len() as u16).to_le_bytes());
    out.extend(bytes);
}
fn assignment(index: u16, expression: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x15,
        &[
            b"s".as_slice(),
            &index.to_le_bytes(),
            &(expression.len() as u16).to_le_bytes(),
            expression,
        ]
        .concat(),
    );
    out
}
fn event(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x10,
        &[
            &0_u16.to_le_bytes()[..],
            &((body.len() + 4) as u32).to_le_bytes(),
        ]
        .concat(),
    );
    out.extend(body);
    instruction(&mut out, 0x11, &[]);
    out
}
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let dir = tempfile::tempdir().unwrap();
    let old = unit(&[(42, 0), (90, 1), (91, 0)], &[(b"SCRV", 42)]);
    let mut script = old[..26].to_vec();
    script[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
    script.extend(field(b"SCDA", body));
    script.extend(&old[46..]);
    fs::write(
        dir.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &script),
            record(b"MISC", 0x400, 0, &[]),
            record(b"CELL", 0x600, 0, &field(b"DATA", &1_u16.to_le_bytes())),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (dir, catalogue, content)
}
fn operators() -> Operators {
    Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(n, s)| Operator {
            code: n as u32,
            precedence: n as u8,
            spelling: s.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap()
}
fn sources(catalogue: &Catalogue) -> PreparedSources<'_> {
    PreparedSources::load_selected(
        catalogue,
        &Model::vanilla(&operators()).unwrap(),
        &Signatures::new(),
        &[definition(catalogue)],
        Default::default(),
    )
    .unwrap()
}
fn reference(n: u64) -> ReferenceId {
    ReferenceId(n.try_into().unwrap())
}
fn seed(
    catalogue: Arc<Catalogue>,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x30; 16]).unwrap(),
    )
    .unwrap();
    for _ in 0..3 {
        world.register_reference(None).unwrap();
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
    let stage = world
        .stage_reference_state(
            &world.reference_view(reference(3)).unwrap(),
            fallout_runtime::reference_state::State::new(form(0x600), pose, false).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    let context = Context {
        calling_reference: Some(reference(1)),
        containing_reference: Some(reference(2)),
        target: Some(ReferenceValue::Live { id: reference(3) }),
        arguments: vec![
            ReferenceValue::Null,
            ReferenceValue::Content { key: form(0x400) },
        ],
    };
    let own = world
        .create_instance(
            &definition(&catalogue),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            own,
            &[
                (
                    42,
                    Value::Reference {
                        value: ReferenceValue::Null,
                    },
                ),
                (
                    91,
                    Value::Number {
                        bits: 0x7ff8123456789abc,
                    },
                ),
            ],
        )
        .unwrap();
    let other = world
        .create_instance(
            &definition(&catalogue),
            Owner::Placed {
                reference: reference(2),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            other,
            &[(
                90,
                Value::Number {
                    bits: 0x8000000000000000,
                },
            )],
        )
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        })
        .unwrap();
    let sequence = world
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    world
        .enqueue(
            other,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, own, sequence)
}
fn selection(sequence: u64) -> literal::Selection {
    literal::Selection {
        sequence,
        intent: literal::Intent::EngineeringExactIntegralDecimal,
    }
}
fn staged(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    sequence: u64,
    limits: literal::Limits,
) -> Box<literal::StagedLiteral> {
    match literal::stage(world, sources, content, selection(sequence), limits).unwrap() {
        literal::Preparation::Staged(p) => p,
        literal::Preparation::Unsupported { reason, detail } => panic!("{reason:?}: {detail}"),
    }
}
fn expected_after(mut snapshot: Snapshot, bits: u64) -> Snapshot {
    let own = snapshot.pending_events[0].instance;
    snapshot
        .instances
        .iter_mut()
        .find(|i| i.id == own)
        .unwrap()
        .locals
        .iter_mut()
        .find(|l| l.index == 90)
        .unwrap()
        .value = Value::Number { bits };
    snapshot.pending_events.remove(0);
    snapshot.state_revision += 1;
    snapshot
}
#[test]
fn integral_literal_bits_match_independent_integer_boundary_constants_and_complete_cold_state() {
    // Expected exponent/significand words are independently specified; no
    // float parser or production arithmetic supplies these expected values.
    for (text, bits) in [
        ("0", 0),
        ("00000", 0),
        ("1", 0x3ff0000000000000),
        ("10", 0x4024000000000000),
        ("9007199254740991", 0x433fffffffffffff),
        ("9007199254740992", 0x4340000000000000),
        ("9007199254740994", 0x4340000000000001),
        ("18446744073709549568", 0x43efffffffffffff),
        ("18446744073709551616", 0x43f0000000000000),
        (
            "170141183460469231731687303715884105728",
            0x47e0000000000000,
        ),
    ] {
        let (_dir, catalogue, content) = fixture(&event(&assignment(90, text.as_bytes())));
        let sources = sources(&catalogue);
        let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        let proposal = staged(&world, &sources, &content, sequence, Default::default());
        assert_eq!(proposal.trace().assigned_bits, bits, "{text}");
        assert_eq!(proposal.trace().literal_bytes, text.as_bytes());
        assert_eq!(proposal.trace().literal_scda_bytes, 19..19 + text.len());
        assert_eq!(proposal.trace().statement_scda_bytes, 10..19 + text.len());
        assert_eq!(
            proposal.changes().assignments(),
            &[(90, Value::Number { bits })]
        );
        assert!(proposal.changes().acknowledges());
        assert_eq!(world.snapshot(), before);
        let committed = proposal.commit(&mut world).unwrap();
        assert_eq!(committed.receipt.assignments, 1);
        assert_eq!(
            committed.receipt.acknowledged.unwrap(),
            before.pending_events[0]
        );
        let expected = expected_after(before, bits);
        assert_eq!(world.snapshot(), expected);
        let encoded = expected.encode(1024 * 1024).unwrap();
        let cold = World::restore(
            Arc::clone(&catalogue),
            Snapshot::decode(&encoded, Default::default()).unwrap(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(cold.snapshot(), expected);
        assert!(
            literal::stage(
                &cold,
                &sources,
                &content,
                selection(sequence),
                Default::default()
            )
            .is_err()
        );
    }
}
#[test]
fn nonintegral_inexact_overflow_extra_operator_and_nonnumeric_source_shapes_refuse_atomically() {
    for text in [
        "1.0",
        "1.",
        ".0",
        "1e0",
        "1E+0",
        "9007199254740993",
        "18446744073709551615",
        "340282366920938463463374607431768211455",
        "340282366920938463463374607431768211456",
        "1 2 +",
        "1 ~",
    ] {
        let (_dir, catalogue, content) = fixture(&event(&assignment(90, text.as_bytes())));
        let sources = sources(&catalogue);
        let (world, _, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        assert!(
            matches!(
                literal::stage(
                    &world,
                    &sources,
                    &content,
                    selection(sequence),
                    Default::default()
                )
                .unwrap(),
                literal::Preparation::Unsupported { .. }
            ),
            "{text}"
        );
        assert_eq!(world.snapshot(), before);
    }
    for body in [
        assignment(42, b"1"),
        [assignment(90, b"1"), assignment(90, b"2")].concat(),
        assignment(90, b"f[\0"),
    ] {
        let (_dir, catalogue, content) = fixture(&event(&body));
        let sources = sources(&catalogue);
        let (world, _, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        assert!(matches!(
            literal::stage(
                &world,
                &sources,
                &content,
                selection(sequence),
                Default::default()
            )
            .unwrap(),
            literal::Preparation::Unsupported { .. }
        ));
        assert_eq!(world.snapshot(), before);
    }
    // Malformed spelling/sign/operator grammar never creates PreparedSources.
    for text in [b"".as_slice(), b"-0", b"+1", b"NaN", b"1e", b"1x", b"\xff"] {
        let (_dir, catalogue, _) = fixture(&event(&assignment(90, text)));
        let rejected = PreparedSources::load_selected(
            &catalogue,
            &Model::vanilla(&operators()).unwrap(),
            &Signatures::new(),
            &[definition(&catalogue)],
            Default::default(),
        )
        .unwrap();
        assert!(rejected.get(&definition(&catalogue)).is_err(), "{text:?}");
    }
    let zeros = vec![b'0'; 129];
    let (_dir, catalogue, content) = fixture(&event(&assignment(90, &zeros)));
    let sources = sources(&catalogue);
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    assert!(matches!(
        literal::stage(
            &world,
            &sources,
            &content,
            selection(sequence),
            Default::default()
        ),
        Err(literal::Error::Capacity("literal bytes"))
    ));
    assert_eq!(world.snapshot(), before);
}
#[test]
fn every_creation_boundary_and_stale_epoch_revision_head_and_counter_fail_without_effects() {
    let (_dir, catalogue, content) = fixture(&event(&assignment(90, b"9007199254740994")));
    let sources = sources(&catalogue);
    let (mut world, own, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let sample = staged(&world, &sources, &content, sequence, Default::default());
    let counts = sample.trace().frame.counts;
    let exact = literal::Limits {
        maximum_event_instructions: 3,
        maximum_operand_uses: 1,
        maximum_statement_bytes: 25,
        maximum_literal_bytes: 16,
        observation: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: counts.source_bytes,
            maximum_rows: counts.rows,
            maximum_variable_bytes: counts.variable_bytes,
            maximum_binding_uses: counts.binding_uses,
        },
        maximum_stage_variable_bytes: sample.trace().stage_variable_reservation,
        maximum_trace_bytes: serde_json::to_vec(sample.trace()).unwrap().len(),
    };
    assert_eq!(
        serde_json::to_vec(staged(&world, &sources, &content, sequence, exact).trace()).unwrap(),
        serde_json::to_vec(sample.trace()).unwrap()
    );
    for index in 0..10 {
        let mut low = exact;
        match index {
            0 => low.maximum_event_instructions -= 1,
            1 => low.maximum_operand_uses -= 1,
            2 => low.maximum_statement_bytes -= 1,
            3 => low.maximum_literal_bytes -= 1,
            4 => low.observation.maximum_source_bytes -= 1,
            5 => low.observation.maximum_rows -= 1,
            6 => low.observation.maximum_variable_bytes -= 1,
            7 => low.observation.maximum_binding_uses -= 1,
            8 => low.maximum_stage_variable_bytes -= 1,
            _ => low.maximum_trace_bytes -= 1,
        };
        assert!(
            literal::stage(&world, &sources, &content, selection(sequence), low).is_err(),
            "{index}"
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(literal::stage(&world, &sources, &content, selection(sequence + 1), exact).is_err());
    assert!(matches!(
        literal::stage(
            &world,
            &sources,
            &content,
            literal::Selection {
                sequence,
                intent: literal::Intent::Faithful
            },
            literal::Limits {
                maximum_literal_bytes: 0,
                ..exact
            }
        )
        .unwrap(),
        literal::Preparation::Unsupported {
            reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    let mut cold =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    assert!(sample.commit(&mut cold).is_err());
    assert_eq!(cold.snapshot(), before);
    let sample = staged(&world, &sources, &content, sequence, exact);
    world
        .assign(
            own,
            &[(
                91,
                Value::Number {
                    bits: 0x8000000000000000,
                },
            )],
        )
        .unwrap();
    let changed = world.snapshot();
    assert!(sample.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), changed);
    let mut exhausted = before.clone();
    exhausted.state_revision = u64::MAX;
    let mut exhausted_world = World::restore(
        Arc::clone(&catalogue),
        exhausted.clone(),
        Default::default(),
    )
    .unwrap();
    let sample = staged(
        &exhausted_world,
        &sources,
        &content,
        sequence,
        Default::default(),
    );
    assert!(sample.commit(&mut exhausted_world).is_err());
    assert_eq!(exhausted_world.snapshot(), exhausted);
    let (_other_dir, other, _) = fixture(&event(&assignment(90, b"1")));
    let wrong = sources_for(&other);
    assert!(
        literal::stage(
            &world,
            &wrong,
            &content,
            selection(sequence),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(world.snapshot(), changed);
}
fn sources_for(catalogue: &Catalogue) -> PreparedSources<'_> {
    sources(catalogue)
}

fn cli_run(
    cli: &std::path::Path,
    install: &std::path::Path,
    order: &std::path::Path,
    work: &std::path::Path,
    inputs: (&[u8], &[u8]),
    outputs: (Option<&std::path::Path>, Option<&std::path::Path>),
    extra: &[&str],
) -> (std::process::Output, std::path::PathBuf, std::path::PathBuf) {
    fs::create_dir(work).unwrap();
    let input = work.join("input.snapshot.json");
    let request = work.join("request.json");
    fs::write(&input, inputs.0).unwrap();
    fs::write(&request, inputs.1).unwrap();
    let result = outputs.0.map_or_else(
        || work.join("result.snapshot.json"),
        std::path::Path::to_path_buf,
    );
    let report = outputs
        .1
        .map_or_else(|| work.join("report.json"), std::path::Path::to_path_buf);
    let order_before = fs::read(order).unwrap();
    let plugin_before = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let exe_before = fs::read(install.join("FalloutNV.exe")).unwrap();
    let output = std::process::Command::new(cli)
        .args(["event-operands", "--install"])
        .arg(install)
        .arg("--load-order")
        .arg(order)
        .arg("--snapshot-literal-assignment-request")
        .arg(&request)
        .arg("--snapshot-input")
        .arg(&input)
        .arg("--snapshot-output")
        .arg(&result)
        .arg("--output")
        .arg(&report)
        .args(extra)
        .output()
        .unwrap();
    fs::write(work.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(work.join("stderr.txt"), &output.stderr).unwrap();
    fs::write(work.join("process.json"),serde_json::to_vec_pretty(&serde_json::json!({"success":output.status.success(),"exit_code":output.status.code(),"snapshot_exists":result.exists(),"report_exists":report.exists()})).unwrap()).unwrap();
    assert_eq!(fs::read(input).unwrap(), inputs.0);
    assert_eq!(fs::read(request).unwrap(), inputs.1);
    assert_eq!(fs::read(order).unwrap(), order_before);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        plugin_before
    );
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), exe_before);
    (output, report, result)
}
fn cli_install(
    evidence: &std::path::Path,
    metadata: &std::path::Path,
    name: &str,
    body: &[u8],
) -> (std::path::PathBuf, Arc<Catalogue>) {
    let (dir, catalogue, _) = fixture(body);
    let install = evidence.join(name);
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
    (install, catalogue)
}
#[test]
#[ignore = "requires an explicitly frozen replacement producer and fresh evidence scope"]
fn cli_saved_literal_assignment_helper() {
    use serde_json::{Value as Json, json};
    use std::{
        cell::Cell,
        path::{Path, PathBuf},
    };
    let cli = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_CLI").unwrap());
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").unwrap());
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_LITERAL_ASSIGNMENT_EVIDENCE").unwrap());
    assert!(cli.is_file());
    assert!(!evidence.exists());
    fs::create_dir(&evidence).unwrap();
    let (install, catalogue) = cli_install(
        &evidence,
        &metadata,
        "install-main",
        &event(&assignment(90, b"9007199254740994")),
    );
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let request = json!({"schema_version":1,"sequence":sequence,"owner":before.instances[0].owner,"intent":"engineering_exact_integral_decimal","maximum_source_instructions":4096,"maximum_operand_uses":1,"maximum_statement_bytes":65539,"maximum_literal_bytes":128,"maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,"maximum_stage_variable_bytes":3145728,"maximum_trace_bytes":2097152,"maximum_prepared_instructions":2000000,"maximum_prepared_operand_uses":1000000,"maximum_prepared_tokens":2000000,"maximum_prepared_record_bytes":536870912,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let calls = Cell::new(0_usize);
    let run_on = |name: &str,
                  install: &Path,
                  input: &Snapshot,
                  raw: &[u8],
                  outputs: (Option<&Path>, Option<&Path>),
                  extra: &[&str]| {
        calls.set(calls.get() + 1);
        cli_run(
            &cli,
            install,
            &order,
            &evidence.join(name),
            (&input.encode(64 * 1024 * 1024).unwrap(), raw),
            outputs,
            extra,
        )
    };
    let run = |name: &str, input: &Snapshot, request: &Json| {
        run_on(
            name,
            &install,
            input,
            &serde_json::to_vec(request).unwrap(),
            (None, None),
            &[],
        )
    };
    let refused = |name: &str, input: &Snapshot, request: &Json| {
        let (output, _, result) = run(name, input, request);
        assert!(!output.status.success(), "{name}: succeeded");
        assert!(!result.exists(), "{name}: result published");
    };
    let (output, report_path, result_path) = run("base-0", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(report_path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    let result_bytes = fs::read(result_path).unwrap();
    let expected = expected_after(before.clone(), 0x4340000000000001);
    let actual = Snapshot::decode(&result_bytes, Default::default()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        World::restore(Arc::clone(&catalogue), actual, Default::default())
            .unwrap()
            .snapshot(),
        expected
    );
    let trace = &report["snapshot_literal_assignment"]["committed"]["trace"];
    assert_eq!(trace["assigned_bits"], json!(0x4340000000000001_u64));
    assert_eq!(trace["literal_scda_bytes"], json!({"start":19,"end":35}));
    assert_eq!(trace["statement_scda_bytes"], json!({"start":10,"end":35}));
    assert_eq!(trace["literal_bytes"], json!(b"9007199254740994".to_vec()));
    assert_eq!(
        report["snapshot_literal_assignment"]["committed"]["receipt"]["assignments"],
        1
    );
    assert_eq!(
        report["snapshot_literal_assignment"]["committed"]["receipt"]["acknowledged"],
        json!(before.pending_events[0])
    );
    assert_eq!(report["faithful_execution_admitted"], false);
    assert_eq!(report["retail_parity_accepted"], false);
    refused("replay-old", &expected, &request);
    let boundary_fields = [
        ("maximum_source_instructions", 3),
        ("maximum_operand_uses", 1),
        ("maximum_statement_bytes", 25),
        ("maximum_literal_bytes", 16),
        (
            "maximum_trace_source_bytes",
            trace["frame"]["counts"]["source_bytes"].as_u64().unwrap(),
        ),
        (
            "maximum_trace_rows",
            trace["frame"]["counts"]["rows"].as_u64().unwrap(),
        ),
        (
            "maximum_trace_variable_bytes",
            trace["frame"]["counts"]["variable_bytes"].as_u64().unwrap(),
        ),
        (
            "maximum_trace_binding_uses",
            trace["frame"]["counts"]["binding_uses"].as_u64().unwrap(),
        ),
        (
            "maximum_stage_variable_bytes",
            trace["stage_variable_reservation"].as_u64().unwrap(),
        ),
        (
            "maximum_trace_bytes",
            serde_json::to_vec(trace).unwrap().len() as u64,
        ),
        ("maximum_prepared_instructions", 3),
        ("maximum_prepared_operand_uses", 1),
        ("maximum_prepared_tokens", 1),
        (
            "maximum_prepared_record_bytes",
            report["prepared_sources"]["counts"]["attempted_record_bytes"]
                .as_u64()
                .unwrap(),
        ),
        ("maximum_result_snapshot_bytes", result_bytes.len() as u64),
        ("maximum_report_bytes", report_bytes.len() as u64),
    ];
    let mut exact = request.clone();
    for (key, value) in boundary_fields {
        exact[key] = json!(value);
    }
    let (output, _, result) = run("exact0", &before, &exact);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        expected
    );
    for (n, (key, value)) in boundary_fields.into_iter().enumerate() {
        let mut under = exact.clone();
        under[key] = json!(value - 1);
        refused(&format!("limit-under-{n}"), &before, &under);
        let mut over = request.clone();
        over[key] = json!(request[key].as_u64().unwrap() + 1);
        refused(&format!("limit-ceiling-{n}"), &before, &over);
    }
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    let (output, path, result) = run("faithful", &before, &faithful);
    assert!(!output.status.success());
    assert!(!result.exists());
    let diagnostic: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        diagnostic["snapshot_literal_assignment"]["reason"],
        "unverified_retail_semantics"
    );
    for (name, key, value) in [
        ("wrong-owner", "owner", json!(before.instances[1].owner)),
        ("later-head", "sequence", json!(sequence + 1)),
        ("zero-head", "sequence", json!(0)),
        ("generic-engineering", "intent", json!("engineering")),
        (
            "object-null-engineering",
            "intent",
            json!({"engineering_exact_integral_decimal":null}),
        ),
        ("object-null-faithful", "intent", json!({"faithful":null})),
        (
            "bad-policy",
            "intent",
            json!({"kind":"engineering_exact_integral_decimal","extra":1}),
        ),
        ("old-request", "schema_version", json!(0)),
    ] {
        let mut changed = request.clone();
        changed[key] = value;
        refused(name, &before, &changed);
    }
    for key in request.as_object().unwrap().keys() {
        let mut missing = request.clone();
        missing.as_object_mut().unwrap().remove(key);
        refused(&format!("missing-{key}"), &before, &missing);
    }
    let mut unknown = request.clone();
    unknown["initializers"] = json!([]);
    refused("unknown-field", &before, &unknown);
    let mut nested = request.clone();
    nested["owner"]["extra"] = json!(1);
    refused("unknown-owner", &before, &nested);
    for (name, raw) in [
        ("raw-malformed", b"{".to_vec()),
        (
            "raw-duplicate",
            [
                b"{\"schema_version\":1,".as_slice(),
                &serde_json::to_vec(&request).unwrap()[1..],
            ]
            .concat(),
        ),
        ("raw-oversize", vec![b' '; 16 * 1024 + 1]),
    ] {
        let (output, _, result) = run_on(name, &install, &before, &raw, (None, None), &[]);
        assert!(!output.status.success());
        assert!(!result.exists());
    }
    for (n, mut changed) in [
        before.clone(),
        before.clone(),
        before.clone(),
        before.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        match n {
            0 => changed.state_revision = u64::MAX,
            1 => changed.schema_version = 3,
            2 => changed.catalogue_sha256 = "0".repeat(64),
            _ => changed.instances[0].definition.version_sha256 = "0".repeat(64),
        };
        refused(&format!("invalid-snapshot-{n}"), &changed, &request);
    }
    let large = 9_007_199_254_741_099_u64;
    let mut big = before.clone();
    big.pending_events[0].sequence = large;
    big.pending_events[1].sequence = large + 1;
    big.next_event_sequence = large + 2;
    let mut big_request = request.clone();
    big_request["sequence"] = json!(large);
    let (output, _, result) = run("large-sequence", &big, &big_request);
    assert!(output.status.success());
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        expected_after(big, 0x4340000000000001)
    );
    for (n, (text, bits)) in [
        ("0", 0),
        ("00000", 0),
        ("1", 0x3ff0000000000000),
        ("9007199254740991", 0x433fffffffffffff),
        ("9007199254740992", 0x4340000000000000),
        ("18446744073709551616", 0x43f0000000000000),
        (
            "170141183460469231731687303715884105728",
            0x47e0000000000000,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (variant, cat) = cli_install(
            &evidence,
            &metadata,
            &format!("install-positive-{n}"),
            &event(&assignment(90, text.as_bytes())),
        );
        let (world, _, _) = seed(Arc::clone(&cat));
        let input = world.snapshot();
        let (output, _, result) = run_on(
            &format!("positive-{n}"),
            &variant,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            (None, None),
            &[],
        );
        assert!(
            output.status.success(),
            "{text}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        let target = expected_after(input, bits);
        assert_eq!(actual, target);
        assert_eq!(
            World::restore(cat, actual, Default::default())
                .unwrap()
                .snapshot(),
            target
        );
    }
    for (n, text) in [
        b"1.0".as_slice(),
        b"1.",
        b".0",
        b"1e0",
        b"1E+0",
        b"9007199254740993",
        b"18446744073709551615",
        b"340282366920938463463374607431768211455",
        b"340282366920938463463374607431768211456",
        b"1 2 +",
        b"1 ~",
        b"-0",
        b"+1",
        b"1e",
        b"",
        b"NaN",
        b"\xff",
    ]
    .into_iter()
    .enumerate()
    {
        let (variant, cat) = cli_install(
            &evidence,
            &metadata,
            &format!("install-negative-{n}"),
            &event(&assignment(90, text)),
        );
        let (world, _, _) = seed(cat);
        let input = world.snapshot();
        let (output, _, result) = run_on(
            &format!("negative-{n}"),
            &variant,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            (None, None),
            &[],
        );
        assert!(!output.status.success(), "{text:?}");
        assert!(!result.exists());
    }
    for (n, body) in [
        assignment(42, b"1"),
        [assignment(90, b"1"), assignment(90, b"2")].concat(),
        assignment(90, &[b'0'; 129]),
    ]
    .into_iter()
    .enumerate()
    {
        let (variant, cat) = cli_install(
            &evidence,
            &metadata,
            &format!("install-shape-{n}"),
            &event(&body),
        );
        let (world, _, _) = seed(cat);
        let input = world.snapshot();
        let (output, _, result) = run_on(
            &format!("shape-{n}"),
            &variant,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            (None, None),
            &[],
        );
        assert!(!output.status.success());
        assert!(!result.exists());
    }
    let protected = install.join("blocked.snapshot.json");
    let raw = serde_json::to_vec(&request).unwrap();
    let (output, _, _) = run_on(
        "protected-result",
        &install,
        &before,
        &raw,
        (Some(&protected), None),
        &[],
    );
    assert!(!output.status.success());
    assert!(!protected.exists());
    let existing = evidence.join("existing.json");
    fs::write(&existing, b"keep existing").unwrap();
    let (output, _, _) = run_on(
        "existing-result",
        &install,
        &before,
        &raw,
        (Some(&existing), None),
        &[],
    );
    assert!(!output.status.success());
    assert_eq!(fs::read(&existing).unwrap(), b"keep existing");
    let same = evidence.join("same.json");
    let (output, _, _) = run_on(
        "same-outputs",
        &install,
        &before,
        &raw,
        (Some(&same), Some(&same)),
        &[],
    );
    assert!(!output.status.success());
    assert!(!same.exists());
    let upper = evidence.join("ALIAS.json");
    let lower = evidence.join("alias.json");
    let (output, _, _) = run_on(
        "alias-outputs",
        &install,
        &before,
        &raw,
        (Some(&upper), Some(&lower)),
        &[],
    );
    assert!(!output.status.success());
    assert!(!upper.exists());
    assert!(!lower.exists());
    let (output, _, result) = run_on(
        "flag-conflict",
        &install,
        &before,
        &raw,
        (None, None),
        &["--snapshot-event-request", "unopened.json"],
    );
    assert!(!output.status.success());
    assert!(!result.exists());
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":calls.get(),"integral_policy":"engineering_exact_integral_decimal","literal_bits":0x4340000000000001_u64,"literal_scda_start":19,"literal_scda_end":35,"large_sequence":large,"whole_expected_and_cold_equal":true,"unrelated_inventory_pose_locals_context_pending_preserved":true,"input_source_metadata_unchanged":true,"original_launched":false,"retail_parity_accepted":false})).unwrap()).unwrap();
}
