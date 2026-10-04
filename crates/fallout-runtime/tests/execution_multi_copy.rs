mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::{CommandSignature, Signatures},
        arguments::Convention,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::{
        copy_probe,
        local_copy::{self, Intent, MultiPreparation, StagedMultiCopy, Unsupported},
        trace,
    },
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceValue, Value},
    programs::PreparedSources,
};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::Arc};

fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn assignment(destination: &[u8], expression: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x15,
        &[
            destination,
            &(expression.len() as u16).to_le_bytes(),
            expression,
        ]
        .concat(),
    );
    out
}
fn copy(destination: u8, source: u8) -> Vec<u8> {
    assignment(&[b's', destination, 0], &[b'f', source, 0])
}
fn chain() -> Vec<u8> {
    [copy(2, 1), copy(3, 2), copy(2, 4), copy(1, 2)].concat()
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
fn write_source(path: &Path, bytes: &[u8]) {
    let original = unit(
        &[(1, 0), (2, 1), (3, 0), (4, 0), (90, 0), (99, 7)],
        &[(b"SCRO", 0x100), (b"SCRV", 90)],
    );
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", bytes));
    source.extend(&original[46..]);
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &source),
            record(b"MISC", 0x100, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn loaded(path: &Path) -> (Arc<Catalogue>, Content) {
    let mut store = fallout_data::store::RecordStore::open_nv_headers(
        path,
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn fixture(bytes: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    write_source(directory.path(), bytes);
    let (catalogue, content) = loaded(directory.path());
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
    let signatures: Signatures = [(
        0x1001,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![],
        },
    )]
    .into_iter()
    .collect();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &signatures,
        Default::default(),
    )
    .unwrap()
}
fn seed(
    catalogue: Arc<Catalogue>,
    a: Option<u64>,
    d: u64,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        Default::default(),
        CampaignId::from_bytes([0x62; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(None).unwrap();
    let context = Context {
        arguments: vec![ReferenceValue::Null, ReferenceValue::Live { id: reference }],
        ..Default::default()
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
    world
        .assign(
            handle,
            &[
                (4, Value::Number { bits: d }),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    },
                ),
            ],
        )
        .unwrap();
    if let Some(bits) = a {
        world
            .assign(handle, &[(1, Value::Number { bits })])
            .unwrap();
    }
    let seq = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    let other = world
        .create_instance(
            &definition,
            Owner::Fragment {
                activation: 2.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            other,
            &[
                (1, Value::Number { bits: 123 }),
                (4, Value::Number { bits: 321 }),
            ],
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
    (world, handle, seq)
}
fn staged(result: MultiPreparation) -> Box<StagedMultiCopy> {
    match result {
        MultiPreparation::Staged(stage) => stage,
        other => panic!("{other:?}"),
    }
}
fn stage(
    world: &World<'_>,
    seq: u64,
    prepared: &PreparedSources<'_>,
    content: &Content,
) -> Box<StagedMultiCopy> {
    staged(
        world
            .stage_source_multi_copy_with_sources(
                seq,
                prepared,
                content,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
    )
}

#[test]
fn physical_overlay_reads_and_repeated_destinations_have_one_atomic_revision_and_ack() {
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    for a in [0, 1, 0x8000000000000000, 0x7ff8123456789abc, u64::MAX] {
        let d = 0xfff0123456789abc;
        let (mut world, handle, seq) = seed(Arc::clone(&catalogue), Some(a), d);
        let before = world.snapshot();
        let instance_id = world.instance(handle).unwrap().id();
        let staged = stage(&world, seq, &prepared, &content);
        assert_eq!(world.snapshot(), before);
        let trace = staged.trace();
        assert_eq!(trace.statements.len(), 4);
        assert_eq!(
            trace
                .statements
                .iter()
                .map(|s| s.copied_bits)
                .collect::<Vec<_>>(),
            [a, a, d, d]
        );
        assert_eq!(
            trace
                .statements
                .iter()
                .map(|s| s.source_from_overlay)
                .collect::<Vec<_>>(),
            [false, true, false, true]
        );
        assert_eq!(
            trace.statements[1].canonical_source_before,
            Value::Uninitialized
        );
        assert_eq!(
            trace.statements[2].destination_before,
            Value::Number { bits: a }
        );
        assert_eq!(
            trace.statements[3].destination_before,
            Value::Number { bits: a }
        );
        assert_eq!(trace.frame.source_operands.len(), 8);
        assert_eq!(trace.frame.source_bytes, event(&chain()));
        assert_eq!(trace.statements[0].statement_scda_bytes, 10..22);
        assert_eq!(trace.statements[3].source_token_scda_bytes, 55..58);
        assert_eq!(
            staged.changes().assignments(),
            [
                (1, Value::Number { bits: d }),
                (2, Value::Number { bits: d }),
                (3, Value::Number { bits: a })
            ]
        );
        let committed = (*staged).commit(&mut world).unwrap();
        assert_eq!(committed.receipt.assignments, 3);
        assert_eq!(committed.receipt.before_revision, before.state_revision);
        assert_eq!(committed.receipt.after_revision, before.state_revision + 1);
        assert_eq!(
            committed.receipt.acknowledged.as_ref(),
            Some(&before.pending_events[0])
        );
        let mut expected = before;
        for (index, bits) in [(1, d), (2, d), (3, a)] {
            expected
                .instances
                .iter_mut()
                .find(|i| i.id == instance_id)
                .unwrap()
                .locals
                .iter_mut()
                .find(|l| l.index == index)
                .unwrap()
                .value = Value::Number { bits };
        }
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        assert_eq!(world.snapshot(), expected);
        let restored =
            World::restore(Arc::clone(&catalogue), expected.clone(), Default::default()).unwrap();
        assert_eq!(restored.snapshot(), expected);
        assert!(
            world
                .stage_source_multi_copy_with_sources(
                    seq,
                    &prepared,
                    &content,
                    Intent::Engineering,
                    Default::default()
                )
                .is_err()
        );
        assert!(!committed.trace.original_behavior_verified);
    }
}

#[test]
fn later_unsupported_statement_or_uninitialized_read_never_produces_a_stage() {
    let mut branch = Vec::new();
    instruction(&mut branch, 0x16, &[1, 0, 1, 0, b'0']);
    branch.extend(copy(3, 1));
    instruction(&mut branch, 0x19, &[]);
    for later in [
        assignment(&[b's', 3, 0], b"1"),
        assignment(&[b's', 3, 0], &[b'f', 1, 0, b'f', 1, 0, b'+']),
        assignment(&[b's', 3, 0], &[b'X', 1, 0x10, 2, 0, 0, 0]),
        assignment(&[b's', 3, 0], &[b'r', 1, 0, b'f', 1, 0]),
        copy(3, 90),
        copy(99, 1),
        branch,
    ] {
        let (_directory, catalogue, content) = fixture(&event(&[copy(2, 1), later].concat()));
        let prepared = sources(&catalogue);
        let (world, _, seq) = seed(Arc::clone(&catalogue), Some(42), 0);
        let before = world.snapshot();
        let result = world.stage_source_multi_copy_with_sources(
            seq,
            &prepared,
            &content,
            Intent::Engineering,
            Default::default(),
        );
        assert!(
            matches!(result, Ok(MultiPreparation::Unsupported { .. })),
            "{result:?}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    let (world, _, seq) = seed(Arc::clone(&catalogue), None, 0);
    let before = world.snapshot();
    assert!(matches!(
        world
            .stage_source_multi_copy_with_sources(
                seq,
                &prepared,
                &content,
                Intent::Faithful,
                Default::default()
            )
            .unwrap(),
        MultiPreparation::Unsupported {
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert!(matches!(
        world
            .stage_source_multi_copy_with_sources(
                seq,
                &prepared,
                &content,
                Intent::Engineering,
                Default::default()
            )
            .unwrap(),
        MultiPreparation::Unsupported {
            reason: Unsupported::LiveOperandUnavailable,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn exact_limits_and_each_one_under_refuse_before_mutation() {
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    let (world, _, seq) = seed(Arc::clone(&catalogue), Some(17), 19);
    let before = world.snapshot();
    let sample = stage(&world, seq, &prepared, &content);
    let counts = &sample.trace().frame.counts;
    let exact = local_copy::MultiLimits {
        maximum_event_instructions: 6,
        maximum_statements: 4,
        maximum_operand_uses: 8,
        maximum_statement_bytes: 48,
        observation: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: counts.source_bytes,
            maximum_rows: counts.rows,
            maximum_variable_bytes: counts.variable_bytes,
            maximum_binding_uses: counts.binding_uses,
        },
    };
    assert!(matches!(
        world
            .stage_source_multi_copy_with_sources(
                seq,
                &prepared,
                &content,
                Intent::Engineering,
                exact
            )
            .unwrap(),
        MultiPreparation::Staged(_)
    ));
    for field in 0..8 {
        let mut limit = exact;
        match field {
            0 => limit.maximum_event_instructions -= 1,
            1 => limit.maximum_statements -= 1,
            2 => limit.maximum_operand_uses -= 1,
            3 => limit.maximum_statement_bytes -= 1,
            4 => limit.observation.maximum_source_bytes -= 1,
            5 => limit.observation.maximum_rows -= 1,
            6 => limit.observation.maximum_variable_bytes -= 1,
            _ => limit.observation.maximum_binding_uses -= 1,
        };
        assert!(
            world
                .stage_source_multi_copy_with_sources(
                    seq,
                    &prepared,
                    &content,
                    Intent::Engineering,
                    limit
                )
                .is_err(),
            "field {field}"
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn full_binding_work_and_full_cohort_guards_include_unselected_source() {
    let bytes = [event(&chain()), event(&chain())].concat();
    let (directory, catalogue, content) = fixture(&bytes);
    let prepared = sources(&catalogue);
    let (world, _, seq) = seed(Arc::clone(&catalogue), Some(17), 19);
    let before = world.snapshot();
    let sample = stage(&world, seq, &prepared, &content);
    assert_eq!(sample.trace().frame.counts.binding_uses, 16);
    assert_eq!(sample.trace().frame.source_operands.len(), 8);
    let mut limits = local_copy::MultiLimits::default();
    limits.observation.maximum_binding_uses = 15;
    assert!(
        world
            .stage_source_multi_copy_with_sources(
                seq,
                &prepared,
                &content,
                Intent::Engineering,
                limits
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    // Preserve the same winning script bytes/handle but change another source.
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let changed = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    assert_eq!(definition(&changed), definition(&catalogue));
    let (changed_world, _, changed_seq) = seed(changed, Some(17), 19);
    let changed_before = changed_world.snapshot();
    assert!(
        changed_world
            .stage_source_multi_copy_with_sources(
                changed_seq,
                &prepared,
                &content,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
    );
    assert_eq!(changed_world.snapshot(), changed_before);
    let (_empty, empty_catalogue, empty_content) = fixture(&event(&[]));
    let empty_sources = sources(&empty_catalogue);
    let (empty_world, _, empty_seq) = seed(Arc::clone(&empty_catalogue), Some(17), 19);
    assert!(matches!(
        empty_world
            .stage_source_multi_copy_with_sources(
                empty_seq,
                &empty_sources,
                &empty_content,
                Intent::Engineering,
                Default::default()
            )
            .unwrap(),
        MultiPreparation::Unsupported {
            reason: Unsupported::EventShape,
            ..
        }
    ));
}

#[test]
fn old_single_copy_shape_is_unchanged_and_non_head_or_stale_proposals_refuse_atomically() {
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    for failure in 0..4 {
        let (mut world, handle, seq) = seed(Arc::clone(&catalogue), Some(17), 19);
        assert!(matches!(
            world
                .stage_source_local_copy_with_sources(
                    seq,
                    &prepared,
                    &content,
                    Intent::Engineering,
                    Default::default()
                )
                .unwrap(),
            local_copy::Preparation::Unsupported {
                reason: Unsupported::EventShape,
                ..
            }
        ));
        let before = world.snapshot();
        let staged = stage(&world, seq, &prepared, &content);
        match failure {
            0 => world
                .assign(handle, &[(4, Value::Number { bits: 21 })])
                .unwrap(),
            1 => {
                world = World::restore(Arc::clone(&catalogue), before, Default::default()).unwrap();
            }
            2 => {
                let mut changed = before;
                changed.campaign = CampaignId::from_bytes([0x63; 16]).unwrap();
                world =
                    World::restore(Arc::clone(&catalogue), changed, Default::default()).unwrap();
            }
            _ => {
                let changes = world.stage_event_changes(seq, &[], true).unwrap();
                world.commit_event_changes(changes).unwrap();
            }
        }
        let after_external = world.snapshot();
        assert!((*staged).commit(&mut world).is_err());
        assert_eq!(world.snapshot(), after_external);
    }
    let (world, _, _) = seed(Arc::clone(&catalogue), Some(17), 19);
    let before = world.snapshot();
    let tail = before.pending_events[1].sequence;
    assert!(
        world
            .stage_source_multi_copy_with_sources(
                tail,
                &prepared,
                &content,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

fn request() -> copy_probe::Request {
    copy_probe::Request {
        schema_version: 1,
        campaign: CampaignId::from_bytes([0x62; 16]).unwrap(),
        activation: 1.try_into().unwrap(),
        initializers: vec![
            copy_probe::Initializer {
                index: 1,
                value: Value::Number {
                    bits: 0x8000000000000000,
                },
            },
            copy_probe::Initializer {
                index: 4,
                value: Value::Number { bits: 19 },
            },
        ],
    }
}
fn manifest(prepared: &PreparedSources<'_>, events: usize) -> trace::Manifest {
    let handle = definition(prepared.catalogue());
    let plan = prepared.get(&handle).unwrap().plan();
    let mut steps = Vec::new();
    for ordinal in 0..events {
        for (index, bits) in [0x8000000000000000, 0x8000000000000000, 19, 19]
            .into_iter()
            .enumerate()
        {
            steps.push(trace::StepInput {
                event_ordinal: ordinal as u32,
                event_id: 0,
                begin_scda_offset: 0,
                scda_offset: (10 + index * 12) as u32,
                operation: trace::Operation::Assignment,
                caller: trace::Caller {
                    calling_reference: None,
                    containing_reference: None,
                    target: None,
                    activation: 1,
                },
                operands: vec![trace::Word::binary64(if ordinal == 0 { bits } else { 19 })],
                item: None,
            });
        }
    }
    trace::Manifest {
        schema_version: 1,
        identity: trace::Identity {
            executable_sha256: "a".repeat(64),
            profile_receipt_sha256: "b".repeat(64),
            source_cohort_sha256: prepared.source_cohort_sha256().into(),
            winning_content_sha256: plan.source_cohort_sha256().into(),
            definition: handle,
            compiled_sha256: format!("{:x}", Sha256::digest(plan.control().bytes())),
            compiled_bytes: plan.control().bytes().len(),
        },
        purpose: trace::Operation::Assignment,
        steps,
    }
}
#[test]
fn executable_probe_groups_multiple_observations_per_event_without_inventing_original_capture() {
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    for events in [1, 2] {
        let case = manifest(&prepared, events);
        let observed = copy_probe::observe_multi_copy(
            &prepared,
            &content,
            &case,
            &request(),
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        )
        .unwrap();
        assert_eq!(observed.capture.finish, trace::Finish::Completed);
        assert_eq!(observed.capture.steps.len(), 4 * events);
        assert_eq!(observed.committed.len(), events);
        assert!(observed.canonical_restore_verified);
        assert!(!observed.faithful_execution_admitted);
        for result in &observed.committed {
            assert_eq!(result.receipt.assignments, 3);
            assert_eq!(
                result.receipt.after_revision,
                result.receipt.before_revision + 1
            );
            assert!(result.receipt.acknowledged.is_some());
        }
        assert!(observed.final_snapshot.pending_events.is_empty());
        assert_eq!(
            trace::compare(
                &prepared,
                &case,
                None,
                Some(&observed.capture),
                Default::default()
            )
            .unwrap()
            .status,
            trace::Status::Blocked
        );
        let old = copy_probe::observe(
            &prepared,
            &content,
            &case,
            &request(),
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        )
        .unwrap();
        assert_eq!(old.capture.finish, trace::Finish::Unsupported);
        assert_eq!(old.initial_snapshot, old.final_snapshot);
    }
}
#[test]
fn incomplete_reordered_or_altered_later_manifest_discards_whole_preview() {
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    for variant in 0..5 {
        let mut case = manifest(&prepared, 2);
        match variant {
            0 => {
                case.steps.pop();
            }
            1 => case.steps[7].operands[0] = trace::Word::binary64(20),
            2 => case.steps.swap(5, 6),
            3 => case.steps[6].event_ordinal = 2,
            _ => case.steps[7].caller.activation = 2,
        };
        let result = copy_probe::observe_multi_copy(
            &prepared,
            &content,
            &case,
            &request(),
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        );
        if variant == 3 {
            assert!(result.is_err());
        } else {
            let observed = result.unwrap();
            assert_eq!(observed.capture.finish, trace::Finish::Unsupported);
            assert!(observed.capture.steps.is_empty() && observed.committed.is_empty());
            assert_eq!(observed.initial_snapshot, observed.final_snapshot);
        }
    }
}
#[test]
fn aggregate_probe_copy_and_binding_work_is_bounded_across_events() {
    let (_directory, catalogue, content) = fixture(&event(&chain()));
    let prepared = sources(&catalogue);
    let case = manifest(&prepared, 2);
    let observed = copy_probe::observe_multi_copy(
        &prepared,
        &content,
        &case,
        &request(),
        &"c".repeat(64),
        &"d".repeat(64),
        Default::default(),
    )
    .unwrap();
    let first = &observed.committed[0].trace.frame.counts;
    let exact = copy_probe::MultiProbeLimits {
        probe: copy_probe::Limits {
            maximum_steps: 8,
            maximum_initializers: 6,
        },
        event: local_copy::MultiLimits::default(),
        observation: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: first.source_bytes * 2,
            maximum_rows: first.rows * 2,
            maximum_variable_bytes: first.variable_bytes * 2,
            maximum_binding_uses: first.binding_uses * 2,
        },
        maximum_statement_bytes: 96,
    };
    assert!(
        copy_probe::observe_multi_copy(
            &prepared,
            &content,
            &case,
            &request(),
            &"c".repeat(64),
            &"d".repeat(64),
            exact
        )
        .is_ok()
    );
    for field in 0..7 {
        let mut limit = exact;
        match field {
            0 => limit.probe.maximum_steps -= 1,
            1 => limit.probe.maximum_initializers = 1,
            2 => limit.observation.maximum_source_bytes -= 1,
            3 => limit.observation.maximum_rows -= 1,
            4 => limit.observation.maximum_variable_bytes -= 1,
            5 => limit.observation.maximum_binding_uses -= 1,
            _ => limit.maximum_statement_bytes -= 1,
        };
        assert!(
            copy_probe::observe_multi_copy(
                &prepared,
                &content,
                &case,
                &request(),
                &"c".repeat(64),
                &"d".repeat(64),
                limit
            )
            .is_err(),
            "field {field}"
        );
    }
}

#[test]
#[ignore = "built frozen CLI and authored metadata copy; engineering only, no original launch"]
fn cli_multi_copy_helper() {
    use serde_json::{Value as Json, json};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_MULTI_COPY_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    write_source(&install.join("Data"), &event(&chain()));
    fs::copy(
        input.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let executable_bytes = fs::read(install.join("FalloutNV.exe")).unwrap();
    let executable_sha = format!("{:x}", Sha256::digest(&executable_bytes));
    assert_eq!(
        executable_sha,
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let source_bytes = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let profile = evidence.join("profile-receipt.txt");
    let profile_bytes=b"Authored VM22 metadata regression only. Original executable is read as metadata, never launched. No original capture or behavioral expectation.\n";
    fs::write(&profile, profile_bytes).unwrap();
    let (catalogue, content) = loaded(&install.join("Data"));
    let prepared = sources(&catalogue);
    let mut base = manifest(&prepared, 1);
    base.identity.executable_sha256 = executable_sha.clone();
    base.identity.profile_receipt_sha256 = format!("{:x}", Sha256::digest(profile_bytes));
    let run_on = |name: &str,
                  source_install: &Path,
                  case: &trace::Manifest,
                  request: &Json,
                  extra: &[&str],
                  mode: &str| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let case_path = directory.join("manifest.json");
        let request_path = directory.join("request.json");
        let report_path = directory.join("report.json");
        fs::write(&case_path, serde_json::to_vec_pretty(case).unwrap()).unwrap();
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["script-trace", "--install"])
            .arg(source_install)
            .arg("--load-order")
            .arg(&order)
            .arg("--manifest")
            .arg(&case_path)
            .arg("--profile-receipt")
            .arg(&profile)
            .arg(mode)
            .arg(&request_path)
            .args(extra)
            .arg("--output")
            .arg(&report_path)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(
            directory.join("exit-code.json"),
            serde_json::to_vec(
                &json!({"exit_code":output.status.code(),"original_launched":false}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(!output.status.success(), "missing original must never pass");
        let report = report_path
            .exists()
            .then(|| serde_json::from_slice::<Json>(&fs::read(report_path).unwrap()).unwrap());
        (output, report)
    };
    let run = |name: &str, case: &trace::Manifest, request: &Json, extra: &[&str]| {
        run_on(
            name,
            &install,
            case,
            request,
            extra,
            "--replacement-multi-copy",
        )
    };
    let request_json = serde_json::to_value(request()).unwrap();
    for (name, expression) in [
        ("unsupported-later-literal", b"1".to_vec()),
        (
            "unsupported-later-operator",
            vec![b'f', 1, 0, b'f', 1, 0, b'+'],
        ),
    ] {
        let mixed_install = evidence.join(format!("{name}-source"));
        fs::create_dir(&mixed_install).unwrap();
        fs::create_dir(mixed_install.join("Data")).unwrap();
        let first_event = event(&chain());
        let later_event = event(&assignment(&[b's', 3, 0], &expression));
        write_source(
            &mixed_install.join("Data"),
            &[first_event.clone(), later_event].concat(),
        );
        fs::copy(
            install.join("FalloutNV.exe"),
            mixed_install.join("FalloutNV.exe"),
        )
        .unwrap();
        let (mixed_catalogue, _) = loaded(&mixed_install.join("Data"));
        let mixed_sources = sources(&mixed_catalogue);
        let mut mixed_case = manifest(&mixed_sources, 1);
        mixed_case.identity.executable_sha256 = base.identity.executable_sha256.clone();
        mixed_case.identity.profile_receipt_sha256 = base.identity.profile_receipt_sha256.clone();
        let mut later = mixed_case.steps[0].clone();
        later.event_ordinal = 1;
        later.begin_scda_offset = first_event.len() as u32;
        later.scda_offset = first_event.len() as u32 + 10;
        later.operands = vec![trace::Word::binary64(19)];
        mixed_case.steps.push(later);
        let (output, report) = run_on(
            name,
            &mixed_install,
            &mixed_case,
            &request_json,
            &[],
            "--replacement-multi-copy",
        );
        let report =
            report.unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(&output.stderr)));
        let observation = &report["replacement_observation"];
        assert_eq!(observation["capture"]["finish"], "unsupported");
        assert_eq!(observation["committed"], json!([]));
        assert_eq!(
            observation["initial_snapshot"],
            observation["final_snapshot"]
        );
    }
    let single_install = evidence.join("old-single-source");
    fs::create_dir(&single_install).unwrap();
    fs::create_dir(single_install.join("Data")).unwrap();
    write_source(&single_install.join("Data"), &event(&copy(2, 1)));
    fs::copy(
        install.join("FalloutNV.exe"),
        single_install.join("FalloutNV.exe"),
    )
    .unwrap();
    let (single_catalogue, _) = loaded(&single_install.join("Data"));
    let single_sources = sources(&single_catalogue);
    let mut single_case = manifest(&single_sources, 1);
    single_case.steps.truncate(1);
    single_case.identity.executable_sha256 = base.identity.executable_sha256.clone();
    single_case.identity.profile_receipt_sha256 = base.identity.profile_receipt_sha256.clone();
    let (output, report) = run_on(
        "old-single-mode",
        &single_install,
        &single_case,
        &request_json,
        &[],
        "--replacement-copy",
    );
    let report = report.unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    assert_eq!(report["schema_version"], 1);
    assert!(report.get("multi_copy_request_sha256").is_none());
    assert_eq!(report["comparison"]["status"], "blocked");
    assert_eq!(
        report["replacement_observation"]["capture"]["finish"],
        "completed"
    );
    assert_eq!(
        report["replacement_observation"]["committed"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for events in [1, 2] {
        let mut case = manifest(&prepared, events);
        case.identity = base.identity.clone();
        let (output, report) = run(
            &format!("complete-{events}-events"),
            &case,
            &request_json,
            &[],
        );
        let report =
            report.unwrap_or_else(|| panic!("{}", String::from_utf8_lossy(&output.stderr)));
        assert_eq!(report["schema_version"], 2);
        assert_eq!(report["comparison"]["status"], "blocked");
        assert_eq!(report["original_capture_sha256"], Json::Null);
        let observation = &report["replacement_observation"];
        assert_eq!(observation["capture"]["finish"], "completed");
        assert_eq!(
            observation["capture"]["steps"].as_array().unwrap().len(),
            events * 4
        );
        assert_eq!(observation["committed"].as_array().unwrap().len(), events);
        assert_eq!(observation["canonical_restore_verified"], true);
        assert_eq!(report["faithful_execution_admitted"], false);
        assert_eq!(report["retail_execution_performed"], false);
        let expected_observation = copy_probe::observe_multi_copy(
            &prepared,
            &content,
            &case,
            &request(),
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        )
        .unwrap();
        let final_snapshot: fallout_runtime::snapshot::Snapshot =
            serde_json::from_value(observation["final_snapshot"].clone()).unwrap();
        assert_eq!(final_snapshot, expected_observation.final_snapshot);
        // Independent whole-state expectation: exactly the three final slots,
        // one enqueued sequence and one commit per group; all other fields stay.
        let mut expected = expected_observation.initial_snapshot;
        for (index, bits) in [
            (1, 19),
            (2, 19),
            (3, if events == 1 { 0x8000000000000000 } else { 19 }),
        ] {
            expected.instances[0]
                .locals
                .iter_mut()
                .find(|l| l.index == index)
                .unwrap()
                .value = Value::Number { bits };
        }
        expected.next_event_sequence += events as u64;
        expected.state_revision += (events * 2) as u64;
        assert_eq!(final_snapshot, expected);
        let restored = World::restore(
            Arc::clone(&catalogue),
            final_snapshot.clone(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(restored.snapshot(), final_snapshot);
        assert!(report["multi_copy_request_sha256"].as_str().is_some());
    }
    for (name, change) in [
        ("missing-later", 0),
        ("changed-later-word", 1),
        ("reordered-later", 2),
        ("wrong-caller", 3),
    ] {
        let mut case = manifest(&prepared, 2);
        case.identity = base.identity.clone();
        match change {
            0 => {
                case.steps.pop();
            }
            1 => case.steps[7].operands[0] = trace::Word::binary64(20),
            2 => case.steps.swap(5, 6),
            _ => case.steps[7].caller.activation = 2,
        };
        let (_, report) = run(name, &case, &request_json, &[]);
        let report = report.unwrap();
        let observation = &report["replacement_observation"];
        assert_eq!(observation["capture"]["finish"], "unsupported");
        assert_eq!(observation["capture"]["steps"], json!([]));
        assert_eq!(observation["committed"], json!([]));
        assert_eq!(
            observation["initial_snapshot"],
            observation["final_snapshot"]
        );
    }
    for (name, change) in [
        ("wrong-profile", 0),
        ("wrong-executable", 1),
        ("changed-compiled-source", 2),
        ("bad-ordinal", 3),
    ] {
        let mut case = base.clone();
        match change {
            0 => case.identity.profile_receipt_sha256 = "0".repeat(64),
            1 => case.identity.executable_sha256 = "0".repeat(64),
            2 => case.identity.compiled_sha256 = "0".repeat(64),
            _ => case.steps[3].event_ordinal = 3,
        };
        let (_, report) = run(name, &case, &request_json, &[]);
        assert!(report.is_none());
    }
    for (name, change) in [
        ("unknown-request-field", 0),
        ("zero-activation", 1),
        ("duplicate-initializer", 2),
        ("nonfinite-initializer", 3),
        ("unsupported-request-schema", 4),
    ] {
        let mut request = request_json.clone();
        match change {
            0 => request["unknown"] = json!(true),
            1 => request["activation"] = json!(0),
            2 => {
                let entry = request["initializers"][0].clone();
                request["initializers"].as_array_mut().unwrap().push(entry);
            }
            3 => request["initializers"][0]["value"]["bits"] = json!(0x7ff8123456789abc_u64),
            _ => request["schema_version"] = json!(2),
        };
        let (_, report) = run(name, &base, &request, &[]);
        assert!(report.is_none());
    }
    for (name, extra) in [
        (
            "single-mode-conflict",
            vec!["--replacement-copy", "missing.json"],
        ),
        (
            "import-mode-conflict",
            vec!["--replacement-trace", "missing.json"],
        ),
    ] {
        let (output, report) = run(name, &base, &request_json, &extra);
        assert!(report.is_none());
        assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with"));
    }
    let directory = evidence.join("oversized-request");
    fs::create_dir(&directory).unwrap();
    let case_path = directory.join("manifest.json");
    fs::write(&case_path, serde_json::to_vec(&base).unwrap()).unwrap();
    let request_path = directory.join("request.json");
    fs::write(&request_path, vec![b' '; 4 * 1024 * 1024 + 1]).unwrap();
    let report = directory.join("report.json");
    let output = Command::new(&cli)
        .args(["script-trace", "--install"])
        .arg(&install)
        .arg("--load-order")
        .arg(&order)
        .arg("--manifest")
        .arg(case_path)
        .arg("--profile-receipt")
        .arg(&profile)
        .arg("--replacement-multi-copy")
        .arg(request_path)
        .arg("--output")
        .arg(&report)
        .output()
        .unwrap();
    fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
    assert!(!output.status.success() && !report.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("input byte budget"));
    assert_eq!(
        fs::read(install.join("FalloutNV.exe")).unwrap(),
        executable_bytes
    );
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source_bytes
    );
    fs::write(evidence.join("acceptance.json"),serde_json::to_vec_pretty(&json!({"schema_version":1,"cases":21,"scope":"authored_sequential_engineering_copies","original_launches":0,"completed_original_captures":0,"faithful_execution_admitted":false,"gameplay_accepted":false})).unwrap()).unwrap();
}
