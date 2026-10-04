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
    execution::{local_copy, reference_literal as literal},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    preparation::ObservationLimits,
    programs::PreparedSources,
    snapshot::Snapshot,
};
use sha2::{Digest, Sha256};
use std::{fs, sync::Arc};

const LIVE_A: u64 = 9_007_199_254_740_993;
const LIVE_B: u64 = 9_007_199_254_741_007;
fn id(n: u64) -> ReferenceId {
    ReferenceId(n.try_into().unwrap())
}
fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn local(index: u16) -> Vec<u8> {
    [vec![b'f'], index.to_le_bytes().to_vec()].concat()
}
fn token(index: u16) -> Vec<u8> {
    [vec![b'Z'], index.to_le_bytes().to_vec()].concat()
}
fn assignment(target: &[u8], expression: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x15,
        &[target, &(expression.len() as u16).to_le_bytes(), expression].concat(),
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
            &((body.len() + 4) as u32).to_le_bytes()[..],
        ]
        .concat(),
    );
    out.extend(body);
    instruction(&mut out, 0x11, &[]);
    out
}
fn code(index: u16) -> Vec<u8> {
    event(&assignment(&local(90), &token(index)))
}
fn raw_script(compiled: &[u8]) -> Vec<u8> {
    let old = unit(
        &[(42, 0), (90, 1), (91, 0)],
        &[
            (b"SCRO", 0x777),
            (b"SCRV", 42),
            (b"SCRO", 0),
            (b"SCRO", 0x14),
            (b"SCRO", 0x778),
            (b"SCRO", 0x999),
            (b"SCRV", 999),
            (b"SCRV", 0),
            (b"SCRV", 90),
        ],
    );
    let mut raw = old[..26].to_vec();
    raw[14..18].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    raw.extend(field(b"SCDA", compiled));
    raw.extend(&old[46..]);
    raw
}
fn fixture(compiled: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &raw_script(compiled)),
            record(b"MISC", 0x777, 0, &[]),
            record(b"MISC", 0x778, plugin::DELETED, &[]),
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
        .map(|(i, s)| Operator {
            code: i as u32,
            precedence: i as u8,
            spelling: s.as_bytes().to_vec(),
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
fn seed(catalogue: Arc<Catalogue>) -> (World<'static>, u64) {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([79; 16]).unwrap(),
    )
    .unwrap();
    let mut initial = world.snapshot();
    initial.next_reference = LIVE_A;
    world = World::restore(Arc::clone(&catalogue), initial, Default::default()).unwrap();
    assert_eq!(world.register_reference(None).unwrap(), id(LIVE_A));
    let mut initial = world.snapshot();
    initial.next_reference = LIVE_B;
    world = World::restore(Arc::clone(&catalogue), initial, Default::default()).unwrap();
    assert_eq!(world.register_reference(None).unwrap(), id(LIVE_B));
    let view = world.reference_view(id(LIVE_A)).unwrap();
    let pose = fallout_runtime::reference_state::Pose::from_source(
        &fallout_data::world::Transform {
            position: [1.0, -0.0, f32::from_bits(1)],
            rotation: [0.0; 3],
        },
        None,
    )
    .unwrap();
    let staged = world
        .stage_reference_state(
            &view,
            fallout_runtime::reference_state::State::new(form(0x779), pose, false).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(staged).unwrap();
    world.initialize_inventory(id(LIVE_A)).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x777));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc1_2345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"BLOB",
            bytes: vec![0, 255, 1],
        });
    world
        .add_item(id(LIVE_A), facts, 7.try_into().unwrap())
        .unwrap();
    let context = Context {
        calling_reference: Some(id(LIVE_A)),
        containing_reference: Some(id(LIVE_B)),
        target: Some(ReferenceValue::Live { id: id(LIVE_A) }),
        arguments: vec![
            ReferenceValue::Content { key: form(0x777) },
            ReferenceValue::Null,
        ],
    };
    let own = world
        .create_instance(
            &definition(&catalogue),
            Owner::Fragment {
                activation: 7.try_into().unwrap(),
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
                        value: ReferenceValue::Live { id: id(LIVE_A) },
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
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9012,
                    },
                ),
            ],
        )
        .unwrap();
    world
        .create_instance(
            &definition(&catalogue),
            Owner::Fragment {
                activation: 8.try_into().unwrap(),
            },
            Context::default(),
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
            context.clone(),
        )
        .unwrap();
    world
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    (world, sequence)
}
fn select(sequence: u64, explicit_player: Option<ReferenceId>) -> literal::Selection {
    literal::Selection {
        sequence,
        explicit_player,
        intent: literal::Intent::EngineeringIdentityAssignment,
    }
}
fn staged(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    selection: literal::Selection,
    limits: literal::Limits,
) -> Box<literal::StagedReferenceLiteral> {
    match literal::stage(world, sources, content, selection, limits).unwrap() {
        literal::Preparation::Staged(stage) => stage,
        literal::Preparation::Unsupported { reason, detail } => panic!("{reason:?}: {detail}"),
    }
}
fn expected(before: &Snapshot, value: ReferenceValue) -> Snapshot {
    let mut after = before.clone();
    after.state_revision += 1;
    after.pending_events.remove(0);
    after
        .instances
        .iter_mut()
        .find(|i| {
            i.owner
                == Owner::Fragment {
                    activation: 7.try_into().unwrap(),
                }
        })
        .unwrap()
        .locals
        .iter_mut()
        .find(|l| l.index == 90)
        .unwrap()
        .value = Value::Reference { value };
    after
}
fn set_source(before: &Snapshot, value: Value) -> Snapshot {
    let mut input = before.clone();
    input.instances[0]
        .locals
        .iter_mut()
        .find(|l| l.index == 42)
        .unwrap()
        .value = value;
    input
}

#[test]
fn exact_physical_scro_scrv_null_player_and_large_live_identity_preserve_complete_cold_state() {
    for (index, value, player, reservation, rows) in [
        (
            1,
            ReferenceValue::Content { key: form(0x777) },
            None,
            2136,
            7,
        ),
        (2, ReferenceValue::Live { id: id(LIVE_A) }, None, 2194, 8),
        (3, ReferenceValue::Null, None, 1938, 7),
        (
            4,
            ReferenceValue::Live { id: id(LIVE_B) },
            Some(id(LIVE_B)),
            2068,
            7,
        ),
        (9, ReferenceValue::Null, None, 2194, 7),
    ] {
        let compiled = code(index);
        // Independently authored raw words: Begin jump16, own f90, Z one-based index, End.
        let mut raw = vec![
            0x10, 0, 6, 0, 0, 0, 16, 0, 0, 0, 0x15, 0, 8, 0, b'f', 90, 0, 3, 0, b'Z',
        ];
        raw.extend(index.to_le_bytes());
        raw.extend([0x11, 0, 0, 0]);
        assert_eq!(compiled, raw);
        let script = raw_script(&compiled);
        assert_eq!(script.len(), 283);
        assert_eq!(&script[103..109], b"SLSD\x18\0");
        let offset = 193 + (usize::from(index) - 1) * 10;
        let table = match index {
            2 | 9 => b"SCRV",
            _ => b"SCRO",
        };
        assert_eq!(&script[offset..offset + 4], table);
        let (directory, catalogue, content) = fixture(&compiled);
        let prepared = sources(&catalogue);
        assert_eq!(prepared.counts().instructions, 3);
        assert_eq!(prepared.counts().uses, 2);
        assert_eq!(prepared.counts().attempted_record_bytes, 283);
        let (mut world, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        let source_bytes = fs::read(directory.path().join("FalloutNV.esm")).unwrap();
        let proposal = staged(
            &world,
            &prepared,
            &content,
            select(sequence, player),
            Default::default(),
        );
        let trace = proposal.trace();
        assert_eq!(trace.assigned_reference, value);
        assert!(!trace.original_behavior_verified);
        assert_eq!(trace.statement_scda_bytes, 10..22);
        assert_eq!(trace.literal_scda_bytes, 19..22);
        assert_eq!(
            trace.source_reference_field_decoded_bytes,
            offset..offset + 10
        );
        assert_eq!(trace.source_reference.index, u32::from(index));
        assert_eq!(trace.destination_index, 90);
        assert_eq!(
            trace.script_source.compiled_sha256,
            Some(format!("{:x}", Sha256::digest(&compiled)))
        );
        assert_eq!(
            trace.script_source.decoded_record_sha256,
            format!("{:x}", Sha256::digest(&script))
        );
        assert_eq!(
            trace.script_source.source_sha256,
            format!("{:x}", Sha256::digest(&source_bytes))
        );
        assert_eq!(trace.frame.source_bytes, compiled);
        assert_eq!(trace.frame.counts.source_bytes, 26);
        assert_eq!(trace.frame.counts.rows, rows);
        assert_eq!(trace.frame.counts.variable_bytes, 231);
        assert_eq!(trace.stage_variable_reservation, reservation);
        assert_eq!(trace.frame.source_operands[0].scda_offset, 15);
        assert_eq!(
            trace.frame.source_operands[0].local_declaration_decoded_offset,
            Some(103)
        );
        assert_eq!(trace.frame.source_operands[1].scda_offset, 20);
        assert_eq!(
            trace.frame.source_operands[1].reference_field_decoded_offset,
            Some(offset)
        );
        if index == 2 {
            assert_eq!(
                trace
                    .source_local_declaration
                    .as_ref()
                    .unwrap()
                    .decoded_offset,
                58
            );
        }
        assert_eq!(world.snapshot(), before);
        let committed = proposal.commit(&mut world).unwrap();
        assert_eq!(committed.receipt.assignments, 1);
        let after = expected(&before, value);
        assert_eq!(world.snapshot(), after);
        let cold = World::restore(
            Arc::clone(&catalogue),
            Snapshot::decode(&after.encode(64 * 1024 * 1024).unwrap(), Default::default()).unwrap(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(cold.snapshot(), after);
        assert!(
            literal::stage(
                &cold,
                &prepared,
                &content,
                select(sequence, player),
                Default::default()
            )
            .is_err()
        );
        assert_eq!(
            fs::read(directory.path().join("FalloutNV.esm")).unwrap(),
            source_bytes
        );
    }
}

#[test]
fn all_ten_logical_caps_admit_exact_values_and_refuse_one_under_without_changes() {
    let (_, catalogue, content) = fixture(&code(1));
    let prepared = sources(&catalogue);
    let (world, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let selection = select(sequence, None);
    let sample = staged(&world, &prepared, &content, selection, Default::default());
    let exact = literal::Limits {
        maximum_event_instructions: 3,
        maximum_operand_uses: 2,
        maximum_statement_bytes: 12,
        observation: ObservationLimits {
            maximum_source_bytes: 26,
            maximum_rows: 7,
            maximum_variable_bytes: 231,
            maximum_binding_uses: 2,
        },
        maximum_reference_variable_bytes: 13,
        maximum_stage_variable_bytes: 2136,
        maximum_trace_bytes: serde_json::to_vec(sample.trace()).unwrap().len(),
    };
    assert_eq!(
        serde_json::to_vec(staged(&world, &prepared, &content, selection, exact).trace()).unwrap(),
        serde_json::to_vec(sample.trace()).unwrap()
    );
    for n in 0..10 {
        let mut low = exact;
        match n {
            0 => low.maximum_event_instructions -= 1,
            1 => low.maximum_operand_uses -= 1,
            2 => low.maximum_statement_bytes -= 1,
            3 => low.observation.maximum_source_bytes -= 1,
            4 => low.observation.maximum_rows -= 1,
            5 => low.observation.maximum_variable_bytes -= 1,
            6 => low.observation.maximum_binding_uses -= 1,
            7 => low.maximum_reference_variable_bytes -= 1,
            8 => low.maximum_stage_variable_bytes -= 1,
            _ => low.maximum_trace_bytes -= 1,
        };
        assert!(
            literal::stage(&world, &prepared, &content, selection, low).is_err(),
            "cap{n}"
        );
        assert_eq!(world.snapshot(), before);
    }
    drop(sample);
    assert_eq!(world.snapshot(), before);
    let faithful = literal::stage(
        &world,
        &prepared,
        &content,
        literal::Selection {
            sequence: u64::MAX,
            explicit_player: None,
            intent: literal::Intent::Faithful,
        },
        literal::Limits {
            maximum_stage_variable_bytes: 0,
            ..exact
        },
    )
    .unwrap();
    assert!(matches!(
        faithful,
        literal::Preparation::Unsupported {
            reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn unavailable_source_targets_and_dynamic_storage_never_fall_back_to_null() {
    for index in [0, 4, 5, 6, 7, 8, 10, u16::MAX] {
        let (_, catalogue, content) = fixture(&code(index));
        let prepared = sources(&catalogue);
        let (world, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        assert!(
            matches!(
                literal::stage(
                    &world,
                    &prepared,
                    &content,
                    select(sequence, None),
                    Default::default()
                ),
                Ok(literal::Preparation::Unsupported { .. }) | Err(literal::Error::Preparation(_))
            ),
            "index{index}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let (_, catalogue, content) = fixture(&code(4));
    let prepared = sources(&catalogue);
    let (world, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    assert!(matches!(
        literal::stage(
            &world,
            &prepared,
            &content,
            select(sequence, Some(id(99))),
            Default::default()
        )
        .unwrap(),
        literal::Preparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
    let (_, catalogue, content) = fixture(&code(2));
    let prepared = sources(&catalogue);
    let (world, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    for value in [
        Value::Uninitialized,
        Value::Number { bits: 0 },
        Value::Reference {
            value: ReferenceValue::Content { key: form(0x778) },
        },
        Value::Reference {
            value: ReferenceValue::Content { key: form(0x999) },
        },
    ] {
        let input = set_source(&before, value);
        if matches!(
            input.instances[0]
                .locals
                .iter()
                .find(|local| local.index == 42)
                .unwrap()
                .value,
            Value::Number { .. }
        ) {
            assert!(World::restore(Arc::clone(&catalogue), input, Default::default()).is_err());
            assert_eq!(world.snapshot(), before);
            continue;
        }
        let world =
            World::restore(Arc::clone(&catalogue), input.clone(), Default::default()).unwrap();
        assert!(matches!(
            literal::stage(
                &world,
                &prepared,
                &content,
                select(sequence, None),
                Default::default()
            )
            .unwrap(),
            literal::Preparation::Unsupported { .. }
        ));
        assert_eq!(world.snapshot(), input);
    }
    for value in [
        ReferenceValue::Null,
        ReferenceValue::Content { key: form(0x777) },
        ReferenceValue::Live { id: id(LIVE_B) },
    ] {
        let input = set_source(
            &before,
            Value::Reference {
                value: value.clone(),
            },
        );
        let mut world =
            World::restore(Arc::clone(&catalogue), input.clone(), Default::default()).unwrap();
        staged(
            &world,
            &prepared,
            &content,
            select(sequence, None),
            Default::default(),
        )
        .commit(&mut world)
        .unwrap();
        assert_eq!(world.snapshot(), expected(&input, value));
    }
    let giant = ReferenceValue::Content {
        key: fallout_data::identity::FormKey {
            origin_plugin: "x".repeat(32 * 1024),
            ..form(0x777)
        },
    };
    let input = set_source(&before, Value::Reference { value: giant });
    let world = World::restore(Arc::clone(&catalogue), input.clone(), Default::default()).unwrap();
    assert!(matches!(
        literal::stage(
            &world,
            &prepared,
            &content,
            select(sequence, None),
            literal::Limits {
                maximum_reference_variable_bytes: 32767,
                ..Default::default()
            }
        ),
        Err(literal::Error::Capacity("reference variable bytes"))
    ));
    assert_eq!(world.snapshot(), input);
}

#[test]
fn own_shape_and_cached_source_guards_refuse_other_expressions_and_foreign_writes() {
    for body in [
        assignment(&local(91), &token(1)),
        assignment(&local(0), &token(1)),
        assignment(&local(999), &token(1)),
        assignment(&local(90), b"1"),
        assignment(&local(90), &local(42)),
        assignment(&local(90), &[token(1), token(1), vec![b'+']].concat()),
        assignment(&[vec![b'r', 1, 0], local(90)].concat(), &token(1)),
        [
            assignment(&local(90), &token(1)),
            assignment(&local(90), &token(1)),
        ]
        .concat(),
    ] {
        let (_, catalogue, content) = fixture(&event(&body));
        let prepared = sources(&catalogue);
        let (world, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        assert!(!matches!(
            literal::stage(
                &world,
                &prepared,
                &content,
                select(sequence, None),
                Default::default()
            ),
            Ok(literal::Preparation::Staged(_))
        ));
        assert_eq!(world.snapshot(), before);
    }
    let (_, catalogue, content) = fixture(&code(1));
    let prepared = sources(&catalogue);
    let (world, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let (_, other, other_content) = fixture(&code(3));
    let other_prepared = sources(&other);
    assert!(
        literal::stage(
            &world,
            &other_prepared,
            &content,
            select(sequence, None),
            Default::default()
        )
        .is_err()
    );
    assert!(
        literal::stage(
            &world,
            &prepared,
            &other_content,
            select(sequence, None),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn opaque_stages_refuse_changed_revision_head_cold_epoch_and_exhausted_revision() {
    let (_, catalogue, content) = fixture(&code(1));
    let prepared = sources(&catalogue);
    let (mut world, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let proposal = staged(
        &world,
        &prepared,
        &content,
        select(sequence, None),
        Default::default(),
    );
    let mut cold =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    assert!(proposal.commit(&mut cold).is_err());
    assert_eq!(cold.snapshot(), before);
    let proposal = staged(
        &world,
        &prepared,
        &content,
        select(sequence, None),
        Default::default(),
    );
    let own = world.handle(before.instances[0].id).unwrap();
    world
        .assign(
            own,
            &[(
                91,
                Value::Number {
                    bits: 0x8000_0000_0000_0000,
                },
            )],
        )
        .unwrap();
    let changed = world.snapshot();
    assert!(proposal.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), changed);
    let stale = staged(
        &world,
        &prepared,
        &content,
        select(sequence, None),
        Default::default(),
    );
    staged(
        &world,
        &prepared,
        &content,
        select(sequence, None),
        Default::default(),
    )
    .commit(&mut world)
    .unwrap();
    let changed = world.snapshot();
    assert!(stale.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), changed);
    assert!(
        literal::stage(
            &world,
            &prepared,
            &content,
            select(changed.pending_events[0].sequence + 1, None),
            Default::default()
        )
        .is_err()
    );
    let mut exhausted = before.clone();
    exhausted.state_revision = u64::MAX;
    let mut exhausted_world = World::restore(
        Arc::clone(&catalogue),
        exhausted.clone(),
        Default::default(),
    )
    .unwrap();
    assert!(
        staged(
            &exhausted_world,
            &prepared,
            &content,
            select(sequence, None),
            Default::default()
        )
        .commit(&mut exhausted_world)
        .is_err()
    );
    assert_eq!(exhausted_world.snapshot(), exhausted);
}

fn cli_run(
    cli: &std::path::Path,
    install: &std::path::Path,
    order: &std::path::Path,
    work: &std::path::Path,
    inputs: (&[u8], &[u8]),
    mode: &str,
    extra: &[&str],
) -> (std::process::Output, std::path::PathBuf, std::path::PathBuf) {
    fs::create_dir(work).unwrap();
    let input = work.join("input.snapshot.json");
    let request = work.join("request.json");
    fs::write(&input, inputs.0).unwrap();
    fs::write(&request, inputs.1).unwrap();
    let result = match mode {
        "result-input" => input.clone(),
        "result-protected" => install.join("protected-result.json"),
        _ => work.join("result.snapshot.json"),
    };
    let report = match mode {
        "input" => input.clone(),
        "request" => request.clone(),
        "order" => order.to_path_buf(),
        "result" => result.clone(),
        "result-case" => work.join("RESULT.SNAPSHOT.JSON"),
        "protected" => install.join("protected-report.json"),
        _ => work.join("report.json"),
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
    let order_before = fs::read(order).unwrap();
    let source_before = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let exe_before = fs::read(install.join("FalloutNV.exe")).unwrap();
    let mut command = std::process::Command::new(cli);
    command
        .args(["event-operands", "--install"])
        .arg(install)
        .arg("--load-order")
        .arg(order)
        .arg("--snapshot-reference-literal-request")
        .arg(&request)
        .arg("--snapshot-input")
        .arg(&input)
        .arg("--snapshot-output")
        .arg(&result)
        .args(extra);
    if mode != "stdout" {
        command.arg("--output").arg(&report);
    }
    let output = command.output().unwrap();
    fs::write(work.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(work.join("stderr.txt"), &output.stderr).unwrap();
    fs::write(work.join("process.json"),serde_json::to_vec_pretty(&serde_json::json!({"success":output.status.success(),"exit_code":output.status.code(),"snapshot_exists":result.exists(),"report_exists":report.exists(),"original_launched":false})).unwrap()).unwrap();
    assert_eq!(fs::read(input).unwrap(), inputs.0);
    assert_eq!(fs::read(request).unwrap(), inputs.1);
    assert_eq!(fs::read(order).unwrap(), order_before);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source_before
    );
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), exe_before);
    if mode == "existing" {
        assert_eq!(fs::read(&report).unwrap(), b"Existing report");
    }
    if mode == "result-existing" {
        assert_eq!(fs::read(&result).unwrap(), b"Existing result");
    }
    (output, report, result)
}

#[test]
#[ignore = "fresh frozen CLI against authored sources; Original never launched"]
fn cli_saved_reference_literal_helper() {
    use serde_json::{Value as Json, json};
    use std::path::{Path, PathBuf};
    let cli = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI"));
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_REFERENCE_LITERAL_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let executable = metadata.join("authored-source-copy/FalloutNV.exe");
    let metadata_bytes = fs::read(&executable).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&metadata_bytes)),
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let producer_bytes = fs::read(&cli).unwrap();
    let install_from = |name: &str, compiled: &[u8]| {
        let (dir, catalogue, _) = fixture(compiled);
        let install = evidence.join(name);
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        fs::copy(
            dir.path().join("FalloutNV.esm"),
            install.join("Data/FalloutNV.esm"),
        )
        .unwrap();
        fs::copy(&executable, install.join("FalloutNV.exe")).unwrap();
        (install, catalogue)
    };
    let (install, catalogue) = install_from("authored-source-copy", &code(1));
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (world, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let request = json!({"schema_version":1,"sequence":sequence,"owner":{"kind":"fragment","activation":7},"intent":"engineering_identity_assignment","explicit_player":null,
        "maximum_source_instructions":4096,"maximum_operand_uses":2,"maximum_statement_bytes":65539,"maximum_reference_variable_bytes":65536,
        "maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,
        "maximum_stage_variable_bytes":3145728,"maximum_trace_bytes":2097152,"maximum_prepared_instructions":2000000,"maximum_prepared_operand_uses":1000000,
        "maximum_prepared_tokens":2000000,"maximum_prepared_record_bytes":536870912,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let calls = std::cell::Cell::new(0usize);
    let run_on =
        |name: &str, source: &Path, input: &Snapshot, raw: &[u8], mode: &str, extra: &[&str]| {
            calls.set(calls.get() + 1);
            cli_run(
                &cli,
                source,
                &order,
                &evidence.join(name),
                (&input.encode(64 * 1024 * 1024).unwrap(), raw),
                mode,
                extra,
            )
        };
    let run = |name: &str, input: &Snapshot, req: &Json| {
        run_on(
            name,
            &install,
            input,
            &serde_json::to_vec(req).unwrap(),
            "new",
            &[],
        )
    };
    let refused = |name: &str, input: &Snapshot, req: &Json| {
        let (output, report, result) = run(name, input, req);
        assert!(!output.status.success(), "{name}: succeeded");
        assert!(!result.exists(), "{name}: result exists");
        assert!(!report.exists(), "{name}: success report exists");
    };
    let (output, report_path, result_path) = run("base-01", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(&report_path).unwrap();
    let result_bytes = fs::read(&result_path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    let trace = &report["snapshot_reference_literal"]["committed"]["trace"];
    let after = expected(&before, ReferenceValue::Content { key: form(0x777) });
    assert_eq!(
        Snapshot::decode(&result_bytes, Default::default()).unwrap(),
        after
    );
    assert_eq!(
        World::restore(
            Arc::clone(&catalogue),
            Snapshot::decode(&result_bytes, Default::default()).unwrap(),
            Default::default()
        )
        .unwrap()
        .snapshot(),
        after
    );
    assert_eq!(
        trace["assigned_reference"],
        json!({"kind":"content","key":form(0x777)})
    );
    assert_eq!(trace["literal_scda_bytes"], json!({"start":19,"end":22}));
    assert_eq!(
        trace["source_reference_field_decoded_bytes"],
        json!({"start":193,"end":203})
    );
    assert_eq!(
        trace["frame"]["source_operands"][0]["local_declaration_decoded_offset"],
        103
    );
    assert_eq!(
        report["snapshot_reference_literal"]["committed"]["receipt"]["acknowledged"],
        json!(before.pending_events[0])
    );
    assert_eq!(report["faithful_execution_admitted"], false);
    assert_eq!(report["retail_parity_accepted"], false);
    refused("cold-replay", &after, &request);
    let boundaries = [
        ("maximum_source_instructions", 3u64),
        ("maximum_operand_uses", 2),
        ("maximum_statement_bytes", 12),
        ("maximum_reference_variable_bytes", 13),
        ("maximum_trace_source_bytes", 26),
        ("maximum_trace_rows", 7),
        ("maximum_trace_variable_bytes", 231),
        ("maximum_trace_binding_uses", 2),
        ("maximum_stage_variable_bytes", 2136),
        (
            "maximum_trace_bytes",
            serde_json::to_vec(trace).unwrap().len() as u64,
        ),
        ("maximum_prepared_instructions", 3),
        ("maximum_prepared_operand_uses", 2),
        ("maximum_prepared_tokens", 1),
        ("maximum_prepared_record_bytes", 283),
        ("maximum_result_snapshot_bytes", result_bytes.len() as u64),
        ("maximum_report_bytes", report_bytes.len() as u64),
    ];
    let mut exact = request.clone();
    for (key, value) in boundaries {
        exact[key] = json!(value);
    }
    let (output, _, result) = run("exact01", &before, &exact);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        after
    );
    for (n, (key, value)) in boundaries.into_iter().enumerate() {
        let mut under = exact.clone();
        under[key] = json!(value - 1);
        under["maximum_report_bytes"] = request["maximum_report_bytes"].clone();
        let under_name = if key == "maximum_report_bytes" {
            under[key] = json!(value - 1);
            "under15".to_owned()
        } else {
            format!("cap-under-{n}")
        };
        refused(&under_name, &before, &under);
        let mut over = request.clone();
        over[key] = json!(request[key].as_u64().unwrap() + 1);
        refused(&format!("cap-ceiling-{n}"), &before, &over);
    }
    for key in request.as_object().unwrap().keys() {
        let mut missing = request.clone();
        missing.as_object_mut().unwrap().remove(key);
        refused(&format!("missing-{key}"), &before, &missing);
    }
    for (name, key, value) in [
        ("owner", "owner", json!({"kind":"fragment","activation":8})),
        ("later-head", "sequence", json!(sequence + 1)),
        ("zero-head", "sequence", json!(0)),
        ("zero-player", "explicit_player", json!(0)),
        ("generic-policy", "intent", json!("engineering")),
        (
            "object-null-policy",
            "intent",
            json!({"engineering_identity_assignment":null}),
        ),
        ("object-null-faithful", "intent", json!({"faithful":null})),
        ("null-policy", "intent", Json::Null),
        ("old-request", "schema_version", json!(0)),
        ("unknown", "invented", json!(true)),
    ] {
        let mut changed = request.clone();
        changed[key] = value;
        refused(name, &before, &changed);
    }
    let mut nested = request.clone();
    nested["owner"]["extra"] = json!(true);
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
        let (output, report, result) = run_on(name, &install, &before, &raw, "new", &[]);
        assert!(!output.status.success() && !report.exists() && !result.exists());
    }
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    faithful["maximum_stage_variable_bytes"] = json!(0);
    let (output, path, result) = run("faithful", &before, &faithful);
    assert!(!output.status.success() && !result.exists());
    let diagnostic: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        diagnostic["snapshot_reference_literal"]["reason"],
        "unverified_retail_semantics"
    );
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
        let (output, _, result) = run_on(
            &format!("path-{mode}"),
            &install,
            &before,
            &serde_json::to_vec(&request).unwrap(),
            mode,
            &[],
        );
        assert!(!output.status.success(), "{mode}");
        if !matches!(mode, "result-input" | "result-existing") {
            assert!(!result.exists(), "{mode}");
        }
    }
    for (name, extra) in [
        ("conflict-prepared", vec!["--prepared-sources"]),
        ("conflict-native", vec!["--native-capabilities"]),
        ("conflict-player", vec!["--player-id", "1"]),
        (
            "conflict-copy",
            vec!["--snapshot-copy-request", "missing.json"],
        ),
        (
            "conflict-reference-copy",
            vec!["--snapshot-reference-copy-request", "missing.json"],
        ),
        (
            "conflict-native-assignment",
            vec!["--snapshot-native-assignment-request", "missing.json"],
        ),
        (
            "conflict-literal-assignment",
            vec!["--snapshot-literal-assignment-request", "missing.json"],
        ),
    ] {
        let (output, report, result) = run_on(
            name,
            &install,
            &before,
            &serde_json::to_vec(&request).unwrap(),
            "new",
            &extra,
        );
        assert!(!output.status.success() && !report.exists() && !result.exists());
    }
    let (output, _, result) = run_on(
        "stdout",
        &install,
        &before,
        &serde_json::to_vec(&request).unwrap(),
        "stdout",
        &[],
    );
    assert!(output.status.success());
    let stdout: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        stdout["snapshot_reference_literal"]["status"],
        "engineering_committed"
    );
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        after
    );
    for (n, mut input) in [
        before.clone(),
        before.clone(),
        before.clone(),
        before.clone(),
        before.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        match n {
            0 => input.state_revision = u64::MAX,
            1 => input.schema_version = 3,
            2 => input.catalogue_sha256 = "0".repeat(64),
            3 => input.instances[0].definition.version_sha256 = "0".repeat(64),
            _ => input.references.retain(|r| r.id != id(LIVE_A)),
        };
        refused(&format!("invalid-snapshot-{n}"), &input, &request);
    }
    let mut big = before.clone();
    big.pending_events[0].sequence = 9_007_199_254_741_099;
    big.pending_events[1].sequence = 9_007_199_254_741_100;
    big.next_event_sequence = 9_007_199_254_741_101;
    let mut selected = request.clone();
    selected["sequence"] = json!(9_007_199_254_741_099u64);
    let (output, _, result) = run("large-sequence", &big, &selected);
    assert!(output.status.success());
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        expected(&big, ReferenceValue::Content { key: form(0x777) })
    );
    for (index, value, player) in [
        (2, ReferenceValue::Live { id: id(LIVE_A) }, None),
        (3, ReferenceValue::Null, None),
        (4, ReferenceValue::Live { id: id(LIVE_B) }, Some(id(LIVE_B))),
        (9, ReferenceValue::Null, None),
    ] {
        let (variant, cat) = install_from(&format!("install-positive-{index}"), &code(index));
        let (world, _) = seed(Arc::clone(&cat));
        let input = world.snapshot();
        let mut req = request.clone();
        req["explicit_player"] = json!(player);
        let (output, path, result) = run_on(
            &format!("positive-{index}"),
            &variant,
            &input,
            &serde_json::to_vec(&req).unwrap(),
            "new",
            &[],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        let target = expected(&input, value.clone());
        assert_eq!(actual, target);
        assert_eq!(
            World::restore(Arc::clone(&cat), actual, Default::default())
                .unwrap()
                .snapshot(),
            target
        );
        let trace: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            trace["snapshot_reference_literal"]["committed"]["trace"]["assigned_reference"],
            json!(value)
        );
        if index == 4 {
            let (output, _, result) = run_on(
                "missing-explicit-player",
                &variant,
                &input,
                &serde_json::to_vec(&request).unwrap(),
                "new",
                &[],
            );
            assert!(!output.status.success() && !result.exists());
        }
        let (output, report, result) = run_on(
            &format!("cohort-positive-{index}"),
            &variant,
            &before,
            &serde_json::to_vec(&request).unwrap(),
            "new",
            &[],
        );
        assert!(!output.status.success() && !report.exists() && !result.exists());
    }
    let (dynamic, dynamic_cat) = install_from("install-dynamic", &code(2));
    let (dynamic_world, _) = seed(Arc::clone(&dynamic_cat));
    let dynamic_before = dynamic_world.snapshot();
    for (n, value) in [
        ReferenceValue::Null,
        ReferenceValue::Content { key: form(0x777) },
        ReferenceValue::Live { id: id(LIVE_B) },
    ]
    .into_iter()
    .enumerate()
    {
        let input = set_source(
            &dynamic_before,
            Value::Reference {
                value: value.clone(),
            },
        );
        let (output, _, result) = run_on(
            &format!("dynamic-value-{n}"),
            &dynamic,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            "new",
            &[],
        );
        assert!(output.status.success());
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        let target = expected(&input, value);
        assert_eq!(actual, target);
        assert_eq!(
            World::restore(Arc::clone(&dynamic_cat), actual, Default::default())
                .unwrap()
                .snapshot(),
            target
        );
    }
    for (n, value) in [
        Value::Uninitialized,
        Value::Number { bits: 0 },
        Value::Reference {
            value: ReferenceValue::Content { key: form(0x778) },
        },
        Value::Reference {
            value: ReferenceValue::Content { key: form(0x999) },
        },
        Value::Reference {
            value: ReferenceValue::Live { id: id(99) },
        },
    ]
    .into_iter()
    .enumerate()
    {
        let input = set_source(&dynamic_before, value);
        let (output, _, result) = run_on(
            &format!("dynamic-refused-{n}"),
            &dynamic,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            "new",
            &[],
        );
        assert!(!output.status.success() && !result.exists());
    }
    let giant = set_source(
        &dynamic_before,
        Value::Reference {
            value: ReferenceValue::Content {
                key: fallout_data::identity::FormKey {
                    origin_plugin: "x".repeat(32768),
                    ..form(0x777)
                },
            },
        },
    );
    for (n, key) in [
        "maximum_reference_variable_bytes",
        "maximum_trace_variable_bytes",
        "maximum_stage_variable_bytes",
    ]
    .into_iter()
    .enumerate()
    {
        let mut low = request.clone();
        low[key] = json!(4096);
        let (output, report, result) = run_on(
            &format!("giant-{n}"),
            &dynamic,
            &giant,
            &serde_json::to_vec(&low).unwrap(),
            "new",
            &[],
        );
        assert!(!output.status.success() && !report.exists() && !result.exists());
    }
    for (n, compiled) in [
        code(0),
        code(5),
        code(6),
        code(7),
        code(8),
        code(10),
        event(&assignment(&local(91), &token(1))),
        event(&assignment(&local(90), b"1")),
        event(&assignment(&local(90), &local(42))),
        event(&assignment(&[b'r', 1, 0, b'f', 90, 0], &token(1))),
        event(&assignment(
            &local(90),
            &[token(1), token(1), vec![b'+']].concat(),
        )),
        event(
            &[
                assignment(&local(90), &token(1)),
                assignment(&local(90), &token(1)),
            ]
            .concat(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (variant, cat) = install_from(&format!("install-negative-{n}"), &compiled);
        let (world, _) = seed(cat);
        let (output, _, result) = run_on(
            &format!("negative-{n}"),
            &variant,
            &world.snapshot(),
            &serde_json::to_vec(&request).unwrap(),
            "new",
            &[],
        );
        assert!(!output.status.success() && !result.exists(), "negative{n}");
    }
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":calls.get(),"passed":true,"source_reference_table_start":193,"destination_declaration_offset":103,
        "live_ids":[LIVE_A,LIVE_B],"large_sequence":9007199254741099u64,"source_identity_policy":"engineering_identity_assignment","whole_expected_and_cold_equal":true,
        "unrelated_inventory_pose_locals_context_pending_preserved":true,"input_source_metadata_unchanged":true,"producer_sha256":format!("{:x}",Sha256::digest(&producer_bytes)),"original_launched":false,"retail_parity_accepted":false})).unwrap()).unwrap();
    assert_eq!(fs::read(executable).unwrap(), metadata_bytes);
    assert_eq!(fs::read(cli).unwrap(), producer_bytes);
}
