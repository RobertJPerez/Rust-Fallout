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
    events::{Clocks, Context, Trigger},
    execution::{native::Inputs, native_assignment as assign},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    inventory::{Facts, ItemId},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use std::{fs, sync::Arc};
fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn command(prefix: Option<u16>, argument: u16, opcode: u16) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(index) = prefix {
        out.push(b'r');
        out.extend(index.to_le_bytes());
    }
    out.push(b'X');
    out.extend(opcode.to_le_bytes());
    out.extend(5_u16.to_le_bytes());
    out.extend([1, 0, b'r']);
    out.extend(argument.to_le_bytes());
    out
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
            0_u16.to_le_bytes().as_slice(),
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
    let old = unit(
        &[(42, 0), (90, 1), (91, 0)],
        &[
            (b"SCRV", 42),
            (b"SCRO", 0x400),
            (b"SCRO", 0x14),
            (b"SCRO", 0x500),
        ],
    );
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
            record(b"MISC", 0x401, 0, &[]),
            record(b"FLST", 0x500, 0, &[]),
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
fn sources(catalogue: &Catalogue) -> PreparedSources<'_> {
    let operators = Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(i, s)| Operator {
            code: i as u32,
            precedence: i as u8,
            spelling: s.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap();
    let signatures: Signatures = [
        (
            0x102f,
            CommandSignature {
                convention: Convention::Default,
                parameters: vec![Parameter {
                    type_id: 50,
                    optional_word: 0,
                }],
            },
        ),
        (
            0x1001,
            CommandSignature {
                convention: Convention::Default,
                parameters: vec![Parameter {
                    type_id: 50,
                    optional_word: 0,
                }],
            },
        ),
    ]
    .into_iter()
    .collect();
    PreparedSources::load_selected(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &signatures,
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
        CampaignId::from_bytes([0x31; 16]).unwrap(),
    )
    .unwrap();
    for _ in 0..3 {
        world.register_reference(None).unwrap();
    }
    world.initialize_inventory(reference(1)).unwrap();
    world.initialize_inventory(reference(2)).unwrap();
    let mut first = Facts::unknown(form(0x400));
    first.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc12345 });
    first
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"BLOB",
            bytes: vec![0, 128, 255],
        });
    let one = world
        .add_item(reference(1), first, 7.try_into().unwrap())
        .unwrap();
    let mut second = Facts::unknown(form(0x400));
    second.condition = Some(fallout_runtime::inventory::Condition::Float64 {
        bits: 0x8000000000000000,
    });
    world
        .add_item(reference(1), second, 13.try_into().unwrap())
        .unwrap();
    world.split_item(one, 3.try_into().unwrap()).unwrap();
    world
        .add_item(
            reference(1),
            Facts::unknown(form(0x401)),
            99.try_into().unwrap(),
        )
        .unwrap();
    world
        .add_item(
            reference(2),
            Facts::unknown(form(0x400)),
            5.try_into().unwrap(),
        )
        .unwrap();
    let context = Context {
        calling_reference: Some(reference(2)),
        containing_reference: Some(reference(1)),
        target: Some(ReferenceValue::Null),
        arguments: vec![ReferenceValue::Content { key: form(0x401) }],
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
                        value: ReferenceValue::Live { id: reference(1) },
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
fn selection(sequence: u64, inputs: Inputs) -> assign::Selection {
    assign::Selection {
        sequence,
        inputs,
        intent: assign::Intent::EngineeringExactCountToNumber,
    }
}
fn staged(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    selection: assign::Selection,
    limits: assign::Limits,
) -> Box<assign::StagedAssignment> {
    match assign::stage(world, sources, content, selection, limits).unwrap() {
        assign::Preparation::Staged(p) => p,
        assign::Preparation::Unsupported { reason, detail } => panic!("{reason:?}: {detail}"),
    }
}
fn expected_after(mut input: Snapshot, bits: u64) -> Snapshot {
    let own = input.pending_events.remove(0).instance;
    input
        .instances
        .iter_mut()
        .find(|i| i.id == own)
        .unwrap()
        .locals
        .iter_mut()
        .find(|l| l.index == 90)
        .unwrap()
        .value = Value::Number { bits };
    input.state_revision += 1;
    input
}
#[test]
fn native_source_count_uses_literal_contribution_ids_and_one_atomic_own_effect_after_cold_restore()
{
    let (_dir, catalogue, content) = fixture(&event(&assignment(90, &command(Some(1), 2, 0x102f))));
    let sources = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let proposal = staged(
        &world,
        &sources,
        &content,
        selection(
            sequence,
            Inputs {
                supplied_subject: Some(reference(2)),
                player: Some(reference(2)),
            },
        ),
        Default::default(),
    );
    let trace = proposal.trace();
    assert_eq!(trace.query.query.result, 20);
    assert_eq!(trace.query.query.subject, reference(1));
    assert_eq!(trace.query.query.item, form(0x400));
    assert_eq!(
        trace.query.query.contributions,
        vec![
            (ItemId(1.try_into().unwrap()), 4),
            (ItemId(2.try_into().unwrap()), 13),
            (ItemId(3.try_into().unwrap()), 3)
        ]
    );
    assert_eq!(trace.assigned_bits, 0x4034000000000000);
    assert_eq!(trace.inventory_visits, 8);
    assert_eq!(trace.call.scda_bytes, 22..32);
    assert_eq!(trace.call.argument_scda_bytes, 27..32);
    assert_eq!(trace.statement_scda_bytes, 10..32);
    assert_eq!(trace.call.calling_reference_index, Some(1));
    assert!(trace.query.original_numeric_return.is_none());
    assert!(!trace.query.original_behavior_verified);
    assert_eq!(world.snapshot(), before);
    let receipt = proposal.commit(&mut world).unwrap().receipt;
    assert_eq!(receipt.assignments, 1);
    assert_eq!(receipt.acknowledged, Some(before.pending_events[0].clone()));
    let target = expected_after(before, 0x4034000000000000);
    assert_eq!(world.snapshot(), target);
    let bytes = target.encode(1024 * 1024).unwrap();
    let cold = World::restore(
        Arc::clone(&catalogue),
        Snapshot::decode(&bytes, Default::default()).unwrap(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(cold.snapshot(), target);
    assert!(
        assign::stage(
            &cold,
            &sources,
            &content,
            selection(sequence, Inputs::default()),
            Default::default()
        )
        .is_err()
    );
}
#[test]
fn explicit_subject_player_source_prefix_unavailable_query_and_source_shape_refuse_without_effects()
{
    for (n, prefix, argument, index, extra) in [
        (0, Some(1), 2, 90, false),
        (1, None, 2, 90, false),
        (2, Some(3), 2, 90, false),
        (3, Some(2), 2, 90, false),
        (4, Some(1), 4, 90, false),
        (5, Some(1), 2, 42, false),
        (6, Some(1), 2, 90, true),
    ] {
        let mut token = command(prefix, argument, 0x102f);
        if extra {
            token.extend(b"1 +");
        }
        let (_dir, catalogue, content) = fixture(&event(&assignment(index, &token)));
        let sources = sources(&catalogue);
        let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        if matches!(n, 0..=2) {
            let inputs = match n {
                0 => Inputs {
                    supplied_subject: Some(reference(2)),
                    player: Some(reference(2)),
                },
                1 => Inputs {
                    supplied_subject: Some(reference(2)),
                    player: Some(reference(1)),
                },
                _ => Inputs {
                    supplied_subject: Some(reference(1)),
                    player: Some(reference(2)),
                },
            };
            let p = staged(
                &world,
                &sources,
                &content,
                selection(sequence, inputs),
                Default::default(),
            );
            let (count, bits) = if n == 0 {
                (20, 0x4034000000000000)
            } else {
                (5, 0x4014000000000000)
            };
            assert_eq!(p.trace().query.query.result, count);
            p.commit(&mut world).unwrap();
            assert_eq!(world.snapshot(), expected_after(before.clone(), bits));
        } else {
            assert!(matches!(
                assign::stage(
                    &world,
                    &sources,
                    &content,
                    selection(sequence, Inputs::default()),
                    Default::default()
                )
                .unwrap(),
                assign::Preparation::Unsupported { .. }
            ));
            assert_eq!(world.snapshot(), before);
        }
        if n == 1 {
            let world =
                World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
            assert!(matches!(
                assign::stage(
                    &world,
                    &sources,
                    &content,
                    selection(
                        sequence,
                        Inputs {
                            player: Some(reference(1)),
                            supplied_subject: None
                        }
                    ),
                    Default::default()
                )
                .unwrap(),
                assign::Preparation::Unsupported { .. }
            ));
            assert_eq!(world.snapshot(), before);
        }
    }
    for body in [
        assignment(90, &command(Some(1), 2, 0x1001)),
        [
            assignment(90, &command(Some(1), 2, 0x102f)),
            assignment(90, b"1"),
        ]
        .concat(),
    ] {
        let (_dir, catalogue, content) = fixture(&event(&body));
        let sources = sources(&catalogue);
        let (world, _, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        assert!(matches!(
            assign::stage(
                &world,
                &sources,
                &content,
                selection(sequence, Inputs::default()),
                Default::default()
            )
            .unwrap(),
            assign::Preparation::Unsupported { .. }
        ));
        assert_eq!(world.snapshot(), before);
    }
    let (_dir, catalogue, content) = fixture(&event(&assignment(90, &command(None, 2, 0x102f))));
    let sources = sources(&catalogue);
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    assert!(matches!(
        assign::stage(
            &world,
            &sources,
            &content,
            selection(
                sequence,
                Inputs {
                    supplied_subject: Some(reference(3)),
                    player: None
                }
            ),
            Default::default()
        )
        .unwrap(),
        assign::Preparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
}
#[test]
fn all_native_assignment_creation_caps_and_stale_stage_identity_fail_atomically() {
    let (_dir, catalogue, content) = fixture(&event(&assignment(90, &command(Some(1), 2, 0x102f))));
    let sources = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let select = selection(sequence, Inputs::default());
    let sample = staged(&world, &sources, &content, select, Default::default());
    let count = sample.trace().frame.counts;
    let exact = assign::Limits {
        native: fallout_runtime::execution::native::Limits {
            maximum_event_instructions: 3,
            maximum_calls: 1,
            maximum_argument_bytes: 5,
        },
        maximum_operand_uses: 3,
        maximum_statement_bytes: 22,
        observation: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: count.source_bytes,
            maximum_rows: count.rows,
            maximum_variable_bytes: count.variable_bytes,
            maximum_binding_uses: count.binding_uses,
        },
        maximum_query_variable_bytes: 13,
        maximum_stage_variable_bytes: sample.trace().stage_variable_reservation,
        maximum_inventory_visits: 8,
        maximum_contributions: 3,
        maximum_trace_bytes: serde_json::to_vec(sample.trace()).unwrap().len(),
    };
    assert_eq!(
        serde_json::to_vec(staged(&world, &sources, &content, select, exact).trace()).unwrap(),
        serde_json::to_vec(sample.trace()).unwrap()
    );
    for n in 0..14 {
        let mut low = exact;
        match n {
            0 => low.native.maximum_event_instructions -= 1,
            1 => low.native.maximum_calls -= 1,
            2 => low.native.maximum_argument_bytes -= 1,
            3 => low.maximum_operand_uses -= 1,
            4 => low.maximum_statement_bytes -= 1,
            5 => low.observation.maximum_source_bytes -= 1,
            6 => low.observation.maximum_rows -= 1,
            7 => low.observation.maximum_variable_bytes -= 1,
            8 => low.observation.maximum_binding_uses -= 1,
            9 => low.maximum_query_variable_bytes -= 1,
            10 => low.maximum_stage_variable_bytes -= 1,
            11 => low.maximum_inventory_visits -= 1,
            12 => low.maximum_contributions -= 1,
            _ => low.maximum_trace_bytes -= 1,
        };
        assert!(
            assign::stage(&world, &sources, &content, select, low).is_err(),
            "{n}"
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(matches!(
        assign::stage(
            &world,
            &sources,
            &content,
            assign::Selection {
                intent: assign::Intent::Faithful,
                ..select
            },
            assign::Limits {
                maximum_stage_variable_bytes: 0,
                ..exact
            }
        )
        .unwrap(),
        assign::Preparation::Unsupported { .. }
    ));
    assert!(
        assign::stage(
            &world,
            &sources,
            &content,
            assign::Selection {
                sequence: sequence + 1,
                ..select
            },
            exact
        )
        .is_err()
    );
    let mut cold =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    assert!(sample.commit(&mut cold).is_err());
    assert_eq!(cold.snapshot(), before);
    let sample = staged(&world, &sources, &content, select, exact);
    world
        .add_item(
            reference(1),
            Facts::unknown(form(0x401)),
            1.try_into().unwrap(),
        )
        .unwrap();
    let changed = world.snapshot();
    assert!(sample.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), changed);
    let mut exhausted = before;
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
        select,
        Default::default(),
    );
    assert!(sample.commit(&mut exhausted_world).is_err());
    assert_eq!(exhausted_world.snapshot(), exhausted);
}

#[test]
fn changed_source_content_and_campaign_cannot_authorize_native_assignment_effects() {
    let (_dir, catalogue, content) = fixture(&event(&assignment(90, &command(Some(1), 2, 0x102f))));
    let current = sources(&catalogue);
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let (_other_dir, other_catalogue, other_content) =
        fixture(&event(&assignment(90, &command(Some(1), 3, 0x102f))));
    let changed = sources(&other_catalogue);
    assert_ne!(
        current.source_cohort_sha256(),
        changed.source_cohort_sha256()
    );
    for (selected_sources, selected_content) in [
        (&changed, &content),
        (&current, &other_content),
        (&changed, &other_content),
    ] {
        assert!(
            assign::stage(
                &world,
                selected_sources,
                selected_content,
                selection(sequence, Inputs::default()),
                Default::default()
            )
            .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let proposal = staged(
        &world,
        &current,
        &content,
        selection(sequence, Inputs::default()),
        Default::default(),
    );
    let mut another_campaign = before;
    another_campaign.campaign = CampaignId::from_bytes([0x32; 16]).unwrap();
    let mut target = World::restore(
        Arc::clone(&catalogue),
        another_campaign.clone(),
        Default::default(),
    )
    .unwrap();
    assert!(proposal.commit(&mut target).is_err());
    assert_eq!(target.snapshot(), another_campaign);
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
        .arg("--snapshot-native-assignment-request")
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
fn cli_saved_native_assignment_helper() {
    use serde_json::{Value as Json, json};
    use std::{
        cell::Cell,
        path::{Path, PathBuf},
    };
    let cli = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_CLI").unwrap());
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").unwrap());
    let evidence = PathBuf::from(std::env::var_os("RF_SCRIPT_NATIVE_ASSIGNMENT_EVIDENCE").unwrap());
    assert!(cli.is_file());
    assert!(!evidence.exists());
    fs::create_dir(&evidence).unwrap();
    let (install, catalogue) = cli_install(
        &evidence,
        &metadata,
        "install-main",
        &event(&assignment(90, &command(Some(1), 2, 0x102f))),
    );
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let request = json!({"schema_version":1,"sequence":sequence,"owner":before.instances[0].owner,"intent":"engineering_exact_count_to_number","supplied_subject":2,"explicit_player":2,"maximum_calls":1,"maximum_argument_bytes":1024,"maximum_query_variable_bytes":1024,"maximum_inventory_visits":65536,"maximum_contributions":4096,"maximum_source_instructions":4096,"maximum_operand_uses":3,"maximum_statement_bytes":65539,"maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,"maximum_stage_variable_bytes":3145728,"maximum_trace_bytes":2097152,"maximum_prepared_instructions":2000000,"maximum_prepared_operand_uses":1000000,"maximum_prepared_tokens":2000000,"maximum_prepared_record_bytes":536870912,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
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
    let expected = expected_after(before.clone(), 0x4034000000000000);
    let actual = Snapshot::decode(&result_bytes, Default::default()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        World::restore(Arc::clone(&catalogue), actual, Default::default())
            .unwrap()
            .snapshot(),
        expected
    );
    let trace = &report["snapshot_native_assignment"]["committed"]["trace"];
    assert_eq!(trace["assigned_bits"], json!(0x4034000000000000_u64));
    assert_eq!(trace["call"]["scda_bytes"], json!({"start":22,"end":32}));
    assert_eq!(
        trace["call"]["argument_scda_bytes"],
        json!({"start":27,"end":32})
    );
    assert_eq!(trace["statement_scda_bytes"], json!({"start":10,"end":32}));
    assert_eq!(trace["call"]["calling_reference_index"], 1);
    assert_eq!(trace["query"]["query"]["subject"], 1);
    assert_eq!(trace["query"]["query"]["item"], json!(form(0x400)));
    assert_eq!(trace["query"]["query"]["result"], 20);
    assert_eq!(
        trace["query"]["query"]["contributions"],
        json!([[1, 4], [2, 13], [3, 3]])
    );
    assert_eq!(trace["inventory_visits"], 8);
    assert_eq!(trace["supplied_subject"], 2);
    assert_eq!(trace["explicit_player"], 2);
    assert!(trace["query"]["original_numeric_return"].is_null());
    assert_eq!(
        report["snapshot_native_assignment"]["committed"]["receipt"]["assignments"],
        1
    );
    assert_eq!(
        report["snapshot_native_assignment"]["committed"]["receipt"]["acknowledged"],
        json!(before.pending_events[0])
    );
    assert_eq!(report["faithful_execution_admitted"], false);
    assert_eq!(report["retail_parity_accepted"], false);
    refused("replay-old", &expected, &request);
    let boundary_fields = [
        ("maximum_source_instructions", 3),
        ("maximum_calls", 1),
        ("maximum_argument_bytes", 5),
        ("maximum_operand_uses", 3),
        ("maximum_statement_bytes", 22),
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
        ("maximum_query_variable_bytes", 13),
        ("maximum_inventory_visits", 8),
        ("maximum_contributions", 3),
        (
            "maximum_stage_variable_bytes",
            trace["stage_variable_reservation"].as_u64().unwrap(),
        ),
        (
            "maximum_trace_bytes",
            serde_json::to_vec(trace).unwrap().len() as u64,
        ),
        ("maximum_prepared_instructions", 3),
        ("maximum_prepared_operand_uses", 3),
        ("maximum_prepared_tokens", 2),
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
        diagnostic["snapshot_native_assignment"]["reason"]["reason"],
        "unverified_retail_semantics"
    );
    for (name, key, value) in [
        ("wrong-owner", "owner", json!(before.instances[1].owner)),
        ("later-head", "sequence", json!(sequence + 1)),
        ("zero-head", "sequence", json!(0)),
        ("generic-engineering", "intent", json!("engineering")),
        (
            "bad-policy",
            "intent",
            json!({"kind":"engineering_exact_count_to_number","extra":1}),
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
        expected_after(big, 0x4034000000000000)
    );
    for (n, prefix, argument, inputs, bits, count, contributions) in [
        (
            0,
            None,
            2,
            json!({"supplied_subject":2,"explicit_player":1}),
            0x4014000000000000,
            5,
            json!([[5, 5]]),
        ),
        (
            1,
            Some(3),
            2,
            json!({"supplied_subject":1,"explicit_player":2}),
            0x4014000000000000,
            5,
            json!([[5, 5]]),
        ),
        (
            2,
            Some(1),
            2,
            json!({"supplied_subject":null,"explicit_player":null}),
            0x4034000000000000,
            20,
            json!([[1, 4], [2, 13], [3, 3]]),
        ),
        (
            3,
            Some(1),
            2,
            json!({"supplied_subject":3,"explicit_player":3}),
            0x4034000000000000,
            20,
            json!([[1, 4], [2, 13], [3, 3]]),
        ),
    ] {
        let (variant, cat) = cli_install(
            &evidence,
            &metadata,
            &format!("install-positive-{n}"),
            &event(&assignment(90, &command(prefix, argument, 0x102f))),
        );
        let (world, _, _) = seed(Arc::clone(&cat));
        let input = world.snapshot();
        let mut variant_request = request.clone();
        variant_request["supplied_subject"] = inputs["supplied_subject"].clone();
        variant_request["explicit_player"] = inputs["explicit_player"].clone();
        let (output, report, result) = run_on(
            &format!("positive-{n}"),
            &variant,
            &input,
            &serde_json::to_vec(&variant_request).unwrap(),
            (None, None),
            &[],
        );
        assert!(
            output.status.success(),
            "{}",
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
        let report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
        assert_eq!(
            report["snapshot_native_assignment"]["committed"]["trace"]["query"]["query"]["result"],
            count
        );
        assert_eq!(
            report["snapshot_native_assignment"]["committed"]["trace"]["query"]["query"]["contributions"],
            contributions
        );
    }
    for (n, body, subject, player) in [
        (
            0,
            assignment(90, &command(Some(2), 2, 0x102f)),
            json!(2),
            json!(2),
        ),
        (
            1,
            assignment(90, &command(Some(1), 4, 0x102f)),
            json!(2),
            json!(2),
        ),
        (
            2,
            assignment(42, &command(Some(1), 2, 0x102f)),
            json!(2),
            json!(2),
        ),
        (
            3,
            assignment(90, &[command(Some(1), 2, 0x102f), b"1 +".to_vec()].concat()),
            json!(2),
            json!(2),
        ),
        (
            4,
            assignment(90, &command(Some(1), 2, 0x1001)),
            json!(2),
            json!(2),
        ),
        (
            5,
            [
                assignment(90, &command(Some(1), 2, 0x102f)),
                assignment(90, b"1"),
            ]
            .concat(),
            json!(2),
            json!(2),
        ),
        (
            6,
            assignment(90, &command(None, 2, 0x102f)),
            json!(3),
            Json::Null,
        ),
        (
            7,
            assignment(90, &command(None, 2, 0x102f)),
            Json::Null,
            json!(1),
        ),
        (
            8,
            assignment(90, &command(Some(3), 2, 0x102f)),
            json!(1),
            Json::Null,
        ),
        (
            9,
            assignment(90, &command(Some(1), 9, 0x102f)),
            json!(2),
            json!(2),
        ),
        (10, assignment(90, b"1"), json!(2), json!(2)),
    ] {
        let (variant, cat) = cli_install(
            &evidence,
            &metadata,
            &format!("install-negative-{n}"),
            &event(&body),
        );
        let (world, _, _) = seed(cat);
        let input = world.snapshot();
        let mut variant_request = request.clone();
        variant_request["supplied_subject"] = subject;
        variant_request["explicit_player"] = player;
        let (output, _, result) = run_on(
            &format!("negative-{n}"),
            &variant,
            &input,
            &serde_json::to_vec(&variant_request).unwrap(),
            (None, None),
            &[],
        );
        assert!(!output.status.success(), "{n}");
        assert!(!result.exists(), "{n}");
    }
    // A zero count is a real query result and must produce an exact zero word.
    let mut zero = before.clone();
    let local = zero.instances[0]
        .locals
        .iter_mut()
        .find(|l| l.index == 42)
        .unwrap();
    local.value = Value::Reference {
        value: ReferenceValue::Live { id: reference(2) },
    };
    let mut zero_world = World::restore(Arc::clone(&catalogue), zero, Default::default()).unwrap();
    zero_world
        .remove_item_quantity(ItemId(5.try_into().unwrap()), 5.try_into().unwrap())
        .unwrap();
    let zero = zero_world.snapshot();
    let (output, report, result) = run("zero-count", &zero, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let target = expected_after(zero, 0);
    let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
    assert_eq!(actual, target);
    assert_eq!(
        World::restore(Arc::clone(&catalogue), actual, Default::default())
            .unwrap()
            .snapshot(),
        target
    );
    let report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(
        report["snapshot_native_assignment"]["committed"]["trace"]["query"]["query"]["result"],
        0
    );
    assert_eq!(
        report["snapshot_native_assignment"]["committed"]["trace"]["query"]["query"]["contributions"],
        json!([])
    );
    for (n, bad) in [
        json!({"faithful":null}),
        json!({"engineering_exact_count_to_number":null}),
        json!(1),
        Json::Null,
    ]
    .into_iter()
    .enumerate()
    {
        let mut changed = request.clone();
        changed["intent"] = bad;
        refused(&format!("intent-shape-{n}"), &before, &changed);
    }
    for key in ["supplied_subject", "explicit_player"] {
        for (n, bad) in [json!(0), json!(-1), json!(1.5), json!("1"), json!({"id":1})]
            .into_iter()
            .enumerate()
        {
            let mut changed = request.clone();
            changed[key] = bad;
            refused(&format!("role-shape-{key}-{n}"), &before, &changed);
        }
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
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":calls.get(),"conversion_policy":"engineering_exact_count_to_number","assigned_bits":0x4034000000000000_u64,"call_scda_start":22,"call_scda_end":32,"count":20,"contributions":[[1,4],[2,13],[3,3]],"cumulative_inventory_visits":8,"large_sequence":large,"whole_expected_and_cold_equal":true,"unrelated_inventory_pose_locals_context_pending_preserved":true,"input_source_metadata_unchanged":true,"original_launched":false,"retail_parity_accepted":false})).unwrap()).unwrap();
}
