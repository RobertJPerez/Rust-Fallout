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
    events::{Context, Trigger},
    execution::{local_copy, reference_copy},
    foreign::Content,
    identity::{Owner, ReferenceId, ReferenceValue, Value},
    programs::PreparedSources,
};
use std::{fs, sync::Arc};
fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
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
fn local(index: u16) -> Vec<u8> {
    [vec![b'f'], index.to_le_bytes().to_vec()].concat()
}
fn assignment(target: &[u8], expression: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x15,
        &[
            target,
            (expression.len() as u16).to_le_bytes().as_slice(),
            expression,
        ]
        .concat(),
    );
    out
}
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    let old = unit(
        &[(42, 0), (90, 1), (42, 1), (91, 0)],
        &[(b"SCRV", 42), (b"SCRV", 90)],
    );
    let mut script = old[..26].to_vec();
    script[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
    script.extend(field(b"SCDA", body));
    script.extend(&old[46..]);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &script),
            record(b"MISC", 0x777, 0, &[]),
            record(b"QUST", 0x776, 0, &[]),
            record(b"REFR", 0x778, 0, &field(b"NAME", &0x777_u32.to_le_bytes())),
            record(b"CELL", 0x779, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (directory, catalogue, content)
}
fn sources(catalogue: &Catalogue) -> PreparedSources<'_> {
    let operators = Operators::new(
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
    .unwrap();
    PreparedSources::load_selected(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Signatures::new(),
        &[definition(catalogue)],
        Default::default(),
    )
    .unwrap()
}
fn seed(
    catalogue: Arc<Catalogue>,
    value: Option<ReferenceValue>,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let mut world = World::new(Arc::clone(&catalogue), Default::default()).unwrap();
    let own = world
        .create_instance(
            &definition(&catalogue),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            own,
            &[
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Content { key: form(0x777) },
                    },
                ),
                (
                    91,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9012,
                    },
                ),
            ],
        )
        .unwrap();
    if let Some(value) = value {
        world
            .assign(own, &[(42, Value::Reference { value })])
            .unwrap();
    }
    let sequence = world
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    world
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, own, sequence)
}
fn staged(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    sequence: u64,
    limits: reference_copy::Limits,
) -> Box<reference_copy::StagedReferenceCopy> {
    match reference_copy::stage(
        world,
        sources,
        content,
        reference_copy::Selection {
            sequence,
            intent: local_copy::Intent::Engineering,
        },
        limits,
    )
    .unwrap()
    {
        reference_copy::Preparation::Staged(p) => p,
        reference_copy::Preparation::Unsupported { reason, detail } => {
            panic!("{reason:?}: {detail}")
        }
    }
}
#[test]
fn exact_typed_null_content_and_large_live_id_preserve_complete_cold_state() {
    let (_directory, catalogue, content) = fixture(&event(&assignment(&local(90), &local(42))));
    let sources = sources(&catalogue);
    for value in [
        ReferenceValue::Null,
        ReferenceValue::Content { key: form(0x777) },
        ReferenceValue::Live {
            id: ReferenceId((9_007_199_254_741_099_u64).try_into().unwrap()),
        },
    ] {
        let (mut world, _, sequence) = seed(Arc::clone(&catalogue), None);
        if let ReferenceValue::Live { id } = value {
            let low = world.register_reference(None).unwrap();
            let mut input = world.snapshot();
            assert_eq!(input.references[0].id, low);
            input.references[0].id = id;
            input.next_reference = id.0.get() + 1;
            world = World::restore(Arc::clone(&catalogue), input, Default::default()).unwrap();
        }
        let own = world
            .handle(
                world
                    .snapshot()
                    .instances
                    .iter()
                    .find(|i| {
                        i.owner
                            == Owner::Fragment {
                                activation: 1.try_into().unwrap(),
                            }
                    })
                    .unwrap()
                    .id,
            )
            .unwrap();
        world
            .assign(
                own,
                &[(
                    42,
                    Value::Reference {
                        value: value.clone(),
                    },
                )],
            )
            .unwrap();
        let before = world.snapshot();
        let proposal = staged(&world, &sources, &content, sequence, Default::default());
        assert_eq!(proposal.trace().copied_reference(), Some(&value));
        assert_eq!(proposal.trace().source_index, 42);
        assert_eq!(proposal.trace().destination_index, 90);
        assert_eq!(proposal.trace().statement_scda_bytes, 10..22);
        assert_eq!(proposal.trace().source_token_scda_bytes, 19..22);
        assert_eq!(world.snapshot(), before);
        let committed = proposal.commit(&mut world).unwrap();
        assert_eq!(committed.receipt.assignments, 1);
        assert_eq!(committed.receipt.after_revision, before.state_revision + 1);
        let mut expected = before;
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        expected.instances[0]
            .locals
            .iter_mut()
            .find(|l| l.index == 90)
            .unwrap()
            .value = Value::Reference {
            value: value.clone(),
        };
        assert_eq!(world.snapshot(), expected);
        let bytes = expected.encode(64 * 1024 * 1024).unwrap();
        let cold = World::restore(
            Arc::clone(&catalogue),
            fallout_runtime::snapshot::Snapshot::decode(&bytes, Default::default()).unwrap(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(cold.snapshot(), expected);
        assert_eq!(
            cold.instance(cold.handle(expected.instances[0].id).unwrap())
                .unwrap()
                .local(90)
                .unwrap(),
            &Value::Reference { value }
        );
        assert!(
            reference_copy::stage(
                &cold,
                &sources,
                &content,
                reference_copy::Selection {
                    sequence,
                    intent: local_copy::Intent::Engineering
                },
                Default::default()
            )
            .is_err()
        );
    }
}
#[test]
fn exact_reference_creation_caps_unset_stale_and_missing_live_identity_refuse_atomically() {
    let (_directory, catalogue, content) = fixture(&event(&assignment(&local(90), &local(42))));
    let sources = sources(&catalogue);
    let (mut world, own, sequence) = seed(Arc::clone(&catalogue), None);
    let before = world.snapshot();
    assert!(matches!(
        reference_copy::stage(
            &world,
            &sources,
            &content,
            reference_copy::Selection {
                sequence,
                intent: local_copy::Intent::Engineering
            },
            Default::default()
        )
        .unwrap(),
        reference_copy::Preparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
    let mut giant = form(0x777);
    giant.origin_plugin = "x".repeat(32 * 1024);
    world
        .assign(
            own,
            &[(
                42,
                Value::Reference {
                    value: ReferenceValue::Content { key: giant },
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    let sample = staged(&world, &sources, &content, sequence, Default::default());
    let counts = sample.trace().frame.counts;
    let exact = reference_copy::Limits {
        maximum_event_instructions: 3,
        maximum_operand_uses: 2,
        maximum_statement_bytes: 12,
        observation: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: counts.source_bytes,
            maximum_rows: counts.rows,
            maximum_variable_bytes: counts.variable_bytes,
            maximum_binding_uses: counts.binding_uses,
        },
        maximum_probe_variable_bytes: sample.trace().probe_variable_reservation,
        maximum_trace_bytes: serde_json::to_vec(sample.trace()).unwrap().len(),
    };
    assert_eq!(
        serde_json::to_vec(staged(&world, &sources, &content, sequence, exact).trace()).unwrap(),
        serde_json::to_vec(sample.trace()).unwrap()
    );
    for index in 0..9 {
        let mut low = exact;
        match index {
            0 => low.maximum_event_instructions -= 1,
            1 => low.maximum_operand_uses -= 1,
            2 => low.maximum_statement_bytes -= 1,
            3 => low.observation.maximum_source_bytes -= 1,
            4 => low.observation.maximum_rows -= 1,
            5 => low.observation.maximum_variable_bytes -= 1,
            6 => low.observation.maximum_binding_uses -= 1,
            7 => low.maximum_probe_variable_bytes -= 1,
            _ => low.maximum_trace_bytes -= 1,
        };
        assert!(
            reference_copy::stage(
                &world,
                &sources,
                &content,
                reference_copy::Selection {
                    sequence,
                    intent: local_copy::Intent::Engineering
                },
                low
            )
            .is_err(),
            "{index}"
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(matches!(
        reference_copy::stage(
            &world,
            &sources,
            &content,
            reference_copy::Selection {
                sequence,
                intent: local_copy::Intent::Faithful
            },
            reference_copy::Limits {
                maximum_probe_variable_bytes: 0,
                ..exact
            }
        )
        .unwrap(),
        reference_copy::Preparation::Unsupported {
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
                42,
                Value::Reference {
                    value: ReferenceValue::Null,
                },
            )],
        )
        .unwrap();
    let changed = world.snapshot();
    assert!(sample.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), changed);
    let mut invalid = changed.clone();
    invalid.instances[0]
        .locals
        .iter_mut()
        .find(|l| l.index == 42)
        .unwrap()
        .value = Value::Reference {
        value: ReferenceValue::Live {
            id: ReferenceId(111.try_into().unwrap()),
        },
    };
    assert!(World::restore(Arc::clone(&catalogue), invalid, Default::default()).is_err());
    assert_eq!(world.snapshot(), changed);
}
#[test]
fn own_reference_shape_and_kind_guards_preserve_numeric_copy_and_alias_identity() {
    for (index, body) in [
        assignment(&local(90), &local(91)),
        assignment(&local(91), &local(42)),
        assignment(&local(90), b"1"),
        assignment(&local(90), &[local(42), local(42), vec![b'+']].concat()),
        assignment(&local(90), &[vec![b'r', 1, 0], local(42)].concat()),
        [
            assignment(&local(90), &local(42)),
            assignment(&local(90), &local(42)),
        ]
        .concat(),
    ]
    .into_iter()
    .enumerate()
    {
        let (_directory, catalogue, content) = fixture(&event(&body));
        let sources = sources(&catalogue);
        let (world, _, sequence) = seed(Arc::clone(&catalogue), Some(ReferenceValue::Null));
        let before = world.snapshot();
        let result = reference_copy::stage(
            &world,
            &sources,
            &content,
            reference_copy::Selection {
                sequence,
                intent: local_copy::Intent::Engineering,
            },
            Default::default(),
        );
        assert!(
            matches!(result, Ok(reference_copy::Preparation::Unsupported { .. })),
            "{index}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let (_directory, catalogue, content) = fixture(&event(&assignment(&local(42), &local(42))));
    let sources = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue), Some(ReferenceValue::Null));
    let before = world.snapshot();
    let proposal = staged(&world, &sources, &content, sequence, Default::default());
    assert_eq!(proposal.trace().frame.locals.len(), 1);
    proposal.commit(&mut world).unwrap();
    let mut expected = before;
    expected.state_revision += 1;
    expected.pending_events.remove(0);
    assert_eq!(world.snapshot(), expected);
}

#[test]
fn source_cohort_changes_refuse_and_existing_numeric_alias_copy_keeps_exact_bits() {
    let (_directory, catalogue, content) = fixture(&event(&assignment(&local(91), &local(91))));
    let prepared = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue), Some(ReferenceValue::Null));
    let before = world.snapshot();
    assert!(matches!(
        reference_copy::stage(
            &world,
            &prepared,
            &content,
            reference_copy::Selection {
                sequence,
                intent: local_copy::Intent::Engineering
            },
            Default::default()
        )
        .unwrap(),
        reference_copy::Preparation::Unsupported { .. }
    ));
    let local_copy::Preparation::Staged(proposal) = world
        .stage_source_local_copy_with_sources(
            sequence,
            &prepared,
            &content,
            local_copy::Intent::Engineering,
            Default::default(),
        )
        .unwrap()
    else {
        panic!("existing own numeric copy")
    };
    proposal.commit(&mut world).unwrap();
    let mut expected = before;
    expected.state_revision += 1;
    expected.pending_events.remove(0);
    assert_eq!(world.snapshot(), expected);
    let (_other_directory, changed, changed_content) =
        fixture(&event(&assignment(&local(90), &local(42))));
    let (other, _, sequence) = seed(Arc::clone(&changed), Some(ReferenceValue::Null));
    let other_before = other.snapshot();
    assert!(
        reference_copy::stage(
            &other,
            &prepared,
            &changed_content,
            reference_copy::Selection {
                sequence,
                intent: local_copy::Intent::Engineering
            },
            Default::default()
        )
        .is_err()
    );
    assert_eq!(other.snapshot(), other_before);
    assert!(
        reference_copy::stage(
            &other,
            &sources(&changed),
            &content,
            reference_copy::Selection {
                sequence,
                intent: local_copy::Intent::Engineering
            },
            Default::default()
        )
        .is_err()
    );
    assert_eq!(other.snapshot(), other_before);
}

#[test]
#[ignore = "frozen CLI and authored descriptor copy; no original execution"]
fn cli_saved_reference_copy_helper() {
    use fallout_runtime::snapshot::Snapshot;
    use serde_json::{Value as Json, json};
    use sha2::{Digest, Sha256};
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_REFERENCE_COPY_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let executable = metadata.join("authored-source-copy/FalloutNV.exe");
    let executable_bytes = fs::read(&executable).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&executable_bytes)),
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let install_from = |name: &str, directory: &Path| {
        let install = evidence.join(name);
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        fs::copy(
            directory.join("FalloutNV.esm"),
            install.join("Data/FalloutNV.esm"),
        )
        .unwrap();
        fs::copy(&executable, install.join("FalloutNV.exe")).unwrap();
        install
    };
    let (directory, catalogue, _) = fixture(&event(&assignment(&local(90), &local(42))));
    let install = install_from("authored-source-copy", directory.path());
    let source_bytes = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (mut world, own, sequence) = seed(Arc::clone(&catalogue), Some(ReferenceValue::Null));
    let own_id = world.instance(own).unwrap().id();
    let bank = world.register_reference(Some(form(0x778))).unwrap();
    let view = world.reference_view(bank).unwrap();
    let pose = fallout_runtime::reference_state::Pose::from_source(
        &fallout_data::world::Transform {
            position: [1.0, -0.0, f32::from_bits(1)],
            rotation: [0.0; 3],
        },
        None,
    )
    .unwrap();
    let proposal = world
        .stage_reference_state(
            &view,
            fallout_runtime::reference_state::State::new(form(0x779), pose, false).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(proposal).unwrap();
    world.initialize_inventory(bank).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x777));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc1_2345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"TEST",
            bytes: vec![0, 255, 1],
        });
    world.add_item(bank, facts, 19.try_into().unwrap()).unwrap();
    let source_live = world.register_reference(None).unwrap();
    let unrelated = world
        .create_instance(
            &definition(&catalogue),
            Owner::Fragment {
                activation: 2.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            unrelated,
            &[
                (
                    42,
                    Value::Reference {
                        value: ReferenceValue::Content { key: form(0x777) },
                    },
                ),
                (
                    91,
                    Value::Number {
                        bits: 0x8000_0000_0000_0000,
                    },
                ),
            ],
        )
        .unwrap();
    world
        .enqueue(
            unrelated,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let request = json!({"schema_version":1,"sequence":sequence,"owner":{"kind":"fragment","activation":1},"intent":"engineering","maximum_source_instructions":4096,"maximum_operand_uses":2,"maximum_statement_bytes":65539,"maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,"maximum_probe_variable_bytes":3145728,"maximum_trace_bytes":2097152,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let mut cases = 0;
    let mut run = |name: &str,
                   source_install: &Path,
                   snapshot: &Snapshot,
                   request: &Json,
                   mode: &str,
                   extra: &[&str]| {
        cases += 1;
        let case = evidence.join(name);
        fs::create_dir(&case).unwrap();
        let input = case.join("input.snapshot.json");
        let req = case.join("request.json");
        let input_bytes = snapshot.encode(64 * 1024 * 1024).unwrap();
        let req_bytes = serde_json::to_vec_pretty(request).unwrap();
        fs::write(&input, &input_bytes).unwrap();
        fs::write(&req, &req_bytes).unwrap();
        let result = match mode {
            "result-input" => input.clone(),
            "result-protected" => source_install.join("reference-result.json"),
            _ => case.join("result.snapshot.json"),
        };
        let report = match mode {
            "input" => input.clone(),
            "request" => req.clone(),
            "order" => order.clone(),
            "result" => result.clone(),
            "result-case" => case.join("RESULT.SNAPSHOT.JSON"),
            "protected" => source_install.join("reference-report.json"),
            _ => case.join("report.json"),
        };
        if mode == "existing" {
            fs::write(&report, b"Existing report").unwrap();
        }
        if mode == "result-existing" {
            fs::write(&result, b"Existing result").unwrap();
        }
        if mode == "hardlink" {
            fs::hard_link(&input, &report).unwrap();
        }
        let mut command = Command::new(&cli);
        command
            .args(["event-operands", "--install"])
            .arg(source_install)
            .arg("--load-order")
            .arg(&order)
            .arg("--snapshot-reference-copy-request")
            .arg(&req)
            .arg("--snapshot-input")
            .arg(&input)
            .arg("--snapshot-output")
            .arg(&result)
            .args(extra);
        if mode != "stdout" {
            command.arg("--output").arg(&report);
        }
        let output = command.output().unwrap();
        fs::write(case.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(case.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(
            case.join("exit-code.json"),
            serde_json::to_vec(
                &json!({"exit_code":output.status.code(),"original_launched":false}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(fs::read(&input).unwrap(), input_bytes);
        assert_eq!(fs::read(&req).unwrap(), req_bytes);
        assert_eq!(fs::read(&order).unwrap(), b"[\"FalloutNV.esm\"]");
        let value = if mode == "stdout" {
            serde_json::from_slice::<Json>(&output.stdout).ok()
        } else if mode == "new" && report.exists() {
            Some(serde_json::from_slice::<Json>(&fs::read(&report).unwrap()).unwrap())
        } else {
            None
        };
        if mode == "existing" {
            assert_eq!(fs::read(&report).unwrap(), b"Existing report");
        }
        if mode == "result-existing" {
            assert_eq!(fs::read(&result).unwrap(), b"Existing result");
        }
        (output, value, result, report)
    };
    let mut baseline = None;
    for (index, value) in [
        ReferenceValue::Null,
        ReferenceValue::Content { key: form(0x777) },
        ReferenceValue::Live {
            id: ReferenceId(9_007_199_254_741_099_u64.try_into().unwrap()),
        },
        ReferenceValue::Live {
            id: ReferenceId((u64::MAX - 1).try_into().unwrap()),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = before.clone();
        if let ReferenceValue::Live { id } = value {
            input
                .references
                .iter_mut()
                .find(|r| r.id == source_live)
                .unwrap()
                .id = id;
            input.next_reference = id.0.get() + 1;
        }
        input
            .instances
            .iter_mut()
            .find(|i| i.id == own_id)
            .unwrap()
            .locals
            .iter_mut()
            .find(|l| l.index == 42)
            .unwrap()
            .value = Value::Reference {
            value: value.clone(),
        };
        let (output, report, result, report_path) = run(
            &format!("copy-{index}"),
            &install,
            &input,
            &request,
            "new",
            &[],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = report.unwrap();
        let committed = &report["snapshot_reference_copy"]["committed"];
        assert_eq!(
            committed["trace"]["frame"]["locals"][0]["value"],
            serde_json::to_value(Value::Reference {
                value: value.clone()
            })
            .unwrap()
        );
        assert_eq!(committed["receipt"]["assignments"], 1);
        assert_eq!(report["faithful_execution_admitted"], false);
        assert_eq!(report["retail_parity_accepted"], false);
        let mut expected = input;
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        expected
            .instances
            .iter_mut()
            .find(|i| i.id == own_id)
            .unwrap()
            .locals
            .iter_mut()
            .find(|l| l.index == 90)
            .unwrap()
            .value = Value::Reference { value };
        let actual = Snapshot::decode(&fs::read(&result).unwrap(), Default::default()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            World::restore(Arc::clone(&catalogue), actual, Default::default())
                .unwrap()
                .snapshot(),
            expected
        );
        if index == 0 {
            baseline = Some((
                report,
                fs::metadata(&result).unwrap().len(),
                fs::metadata(&report_path).unwrap().len(),
                expected,
            ));
        }
    }
    let (report, result_bytes, report_bytes, after) = baseline.unwrap();
    let trace = &report["snapshot_reference_copy"]["committed"]["trace"];
    let frame = &trace["frame"]["counts"];
    let mut exact = request.clone();
    for (name, value) in [
        ("maximum_source_instructions", json!(3)),
        ("maximum_operand_uses", json!(2)),
        ("maximum_statement_bytes", json!(12)),
        ("maximum_trace_source_bytes", frame["source_bytes"].clone()),
        ("maximum_trace_rows", frame["rows"].clone()),
        (
            "maximum_trace_variable_bytes",
            frame["variable_bytes"].clone(),
        ),
        ("maximum_trace_binding_uses", frame["binding_uses"].clone()),
        (
            "maximum_probe_variable_bytes",
            trace["probe_variable_reservation"].clone(),
        ),
        (
            "maximum_trace_bytes",
            json!(serde_json::to_vec(trace).unwrap().len()),
        ),
        ("maximum_result_snapshot_bytes", json!(result_bytes)),
        ("maximum_report_bytes", json!(report_bytes)),
    ] {
        exact[name] = value;
    }
    let (output, _, result, _) = run("exact0", &install, &before, &exact, "new", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        after
    );
    for field in [
        "maximum_source_instructions",
        "maximum_operand_uses",
        "maximum_statement_bytes",
        "maximum_trace_source_bytes",
        "maximum_trace_rows",
        "maximum_trace_variable_bytes",
        "maximum_trace_binding_uses",
        "maximum_probe_variable_bytes",
        "maximum_trace_bytes",
        "maximum_result_snapshot_bytes",
    ] {
        let mut low = exact.clone();
        low[field] = json!(low[field].as_u64().unwrap() - 1);
        low["maximum_report_bytes"] = request["maximum_report_bytes"].clone();
        let (output, report, result, _) = run(field, &install, &before, &low, "new", &[]);
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{field}"
        );
    }
    for (name, field, value) in [
        ("schema", "schema_version", json!(2)),
        (
            "intent-object-engineering-null",
            "intent",
            json!({"engineering":null}),
        ),
        (
            "intent-object-faithful-null",
            "intent",
            json!({"faithful":null}),
        ),
        ("zero", "sequence", json!(0)),
        ("later-head", "sequence", json!(2)),
        ("owner", "owner", json!({"kind":"fragment","activation":2})),
        ("report-cap", "maximum_report_bytes", json!(1)),
        (
            "instruction-ceiling",
            "maximum_source_instructions",
            json!(4097),
        ),
        (
            "probe-ceiling",
            "maximum_probe_variable_bytes",
            json!(3145729),
        ),
        ("trace-ceiling", "maximum_trace_bytes", json!(2097153)),
        (
            "snapshot-ceiling",
            "maximum_result_snapshot_bytes",
            json!(67108865),
        ),
        ("unknown", "invented", json!(true)),
        ("player-rejected", "explicit_player", json!(1)),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, report, result, _) = run(name, &install, &before, &invalid, "new", &[]);
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{name}"
        );
    }
    for field in ["owner", "intent", "maximum_probe_variable_bytes"] {
        let mut invalid = request.clone();
        invalid.as_object_mut().unwrap().remove(field);
        let (output, report, result, _) = run(
            &format!("missing-{field}"),
            &install,
            &before,
            &invalid,
            "new",
            &[],
        );
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    for mode in [
        "input",
        "request",
        "order",
        "result",
        "result-case",
        "protected",
        "existing",
        "result-input",
        "result-protected",
        "result-existing",
        "hardlink",
    ] {
        let (output, report, result, _) = run(
            &format!("path-{mode}"),
            &install,
            &before,
            &request,
            mode,
            &[],
        );
        assert!(!output.status.success() && report.is_none(), "{mode}");
        if mode != "result-input" && mode != "result-existing" {
            assert!(!result.exists(), "{mode}");
        }
    }
    for (name, args) in [
        ("conflict-prepared", vec!["--prepared-sources"]),
        ("conflict-native", vec!["--native-capabilities"]),
        ("conflict-player", vec!["--player-id", "1"]),
        (
            "conflict-foreign",
            vec!["--snapshot-foreign-copy-request", "missing.json"],
        ),
        (
            "conflict-numeric",
            vec!["--snapshot-copy-request", "missing.json"],
        ),
    ] {
        let (output, report, result, _) = run(name, &install, &before, &request, "new", &args);
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    let (output, report, result, _) = run("stdout", &install, &before, &request, "stdout", &[]);
    assert!(output.status.success() && report.is_some() && result.exists());
    let (output, report, result, _) = run("cold-old-head", &install, &after, &request, "new", &[]);
    assert!(!output.status.success() && report.is_none() && !result.exists());
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    faithful["maximum_probe_variable_bytes"] = json!(0);
    let (output, report, result, _) = run("faithful", &install, &before, &faithful, "new", &[]);
    assert!(!output.status.success() && !result.exists());
    assert_eq!(
        report.unwrap()["snapshot_reference_copy"]["reason"],
        "unverified_retail_semantics"
    );
    for mode in [
        "uninitialized",
        "missing-live",
        "revision-max",
        "wrong-schema",
    ] {
        let mut input = before.clone();
        match mode {
            "uninitialized" => {
                input
                    .instances
                    .iter_mut()
                    .find(|i| i.id == own_id)
                    .unwrap()
                    .locals
                    .iter_mut()
                    .find(|l| l.index == 42)
                    .unwrap()
                    .value = Value::Uninitialized
            }
            "missing-live" => {
                input
                    .instances
                    .iter_mut()
                    .find(|i| i.id == own_id)
                    .unwrap()
                    .locals
                    .iter_mut()
                    .find(|l| l.index == 42)
                    .unwrap()
                    .value = Value::Reference {
                    value: ReferenceValue::Live {
                        id: ReferenceId(111.try_into().unwrap()),
                    },
                }
            }
            "revision-max" => input.state_revision = u64::MAX,
            _ => input.schema_version = 3,
        };
        let (output, report, result, _) = run(mode, &install, &input, &request, "new", &[]);
        assert!(!output.status.success() && !result.exists());
        if mode == "uninitialized" {
            assert_eq!(
                report.unwrap()["snapshot_reference_copy"]["status"],
                "unsupported"
            );
        } else {
            assert!(report.is_none());
        }
    }
    let mut giant = before.clone();
    let mut key = form(0x777);
    key.origin_plugin = "x".repeat(32 * 1024);
    giant
        .instances
        .iter_mut()
        .find(|i| i.id == own_id)
        .unwrap()
        .locals
        .iter_mut()
        .find(|l| l.index == 42)
        .unwrap()
        .value = Value::Reference {
        value: ReferenceValue::Content { key },
    };
    for (name, field) in [
        ("giant-projection", "maximum_trace_variable_bytes"),
        ("giant-probe", "maximum_probe_variable_bytes"),
    ] {
        let mut low = request.clone();
        low[field] = json!(4096);
        let (output, report, result, _) = run(name, &install, &giant, &low, "new", &[]);
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    for mode in [
        "missing-source-local",
        "incompatible-stored-value",
        "wrong-definition-version",
    ] {
        let mut input = before.clone();
        let instance = input.instances.iter_mut().find(|i| i.id == own_id).unwrap();
        match mode {
            "missing-source-local" => instance.locals.retain(|l| l.index != 42),
            "incompatible-stored-value" => {
                instance
                    .locals
                    .iter_mut()
                    .find(|l| l.index == 42)
                    .unwrap()
                    .value = Value::Number {
                    bits: 9007199254741099,
                }
            }
            _ => instance.definition.version_sha256 = "0".repeat(64),
        }
        let (output, report, result, _) = run(mode, &install, &input, &request, "new", &[]);
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    for (kind, owner) in [
        ("quest", Owner::Quest { key: form(0x776) }),
        ("placed", Owner::Placed { reference: bank }),
    ] {
        let mut input = before.clone();
        input
            .instances
            .iter_mut()
            .find(|i| i.id == own_id)
            .unwrap()
            .owner = owner.clone();
        let mut selected = request.clone();
        selected["owner"] = serde_json::to_value(owner).unwrap();
        let (output, report, result, _) = run(kind, &install, &input, &selected, "new", &[]);
        assert!(output.status.success() && report.is_some());
        input.state_revision += 1;
        input.pending_events.remove(0);
        input
            .instances
            .iter_mut()
            .find(|i| i.id == own_id)
            .unwrap()
            .locals
            .iter_mut()
            .find(|l| l.index == 90)
            .unwrap()
            .value = Value::Reference {
            value: ReferenceValue::Null,
        };
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            input
        );
    }
    for (index, body) in [
        assignment(&local(90), b"1"),
        assignment(&local(91), &local(42)),
        assignment(&local(90), &local(91)),
        assignment(&local(90), &[vec![b'r', 1, 0], local(42)].concat()),
        assignment(&local(90), &[local(42), local(42), vec![b'+']].concat()),
        assignment(&[b'r', 1, 0, b'f', 90, 0], &local(42)),
        assignment(&local(90), &[b'G', 1, 0]),
        assignment(&local(90), &[b'X', 1, 0x10, 2, 0, 0, 0]),
    ]
    .into_iter()
    .enumerate()
    {
        let (directory, other_catalogue, _) = fixture(&event(&body));
        let other_install = install_from(&format!("other-source-{index}"), directory.path());
        let (other, _, _) = seed(Arc::clone(&other_catalogue), Some(ReferenceValue::Null));
        let (output, report, result, _) = run(
            &format!("other-case-{index}"),
            &other_install,
            &other.snapshot(),
            &request,
            "new",
            &[],
        );
        assert!(!output.status.success() && !result.exists());
        if index == 7 {
            assert!(report.is_none());
        } else {
            assert_eq!(
                report.unwrap()["snapshot_reference_copy"]["status"],
                "unsupported"
            );
        }
        let (output, report, result, _) = run(
            &format!("cohort-{index}"),
            &other_install,
            &before,
            &request,
            "new",
            &[],
        );
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":cases,"passed":true,"whole_snapshot_checked":true,"original_launched":false,"original_behavior_verified":false})).unwrap()).unwrap();
    assert_eq!(fs::read(&executable).unwrap(), executable_bytes);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source_bytes
    );
}
