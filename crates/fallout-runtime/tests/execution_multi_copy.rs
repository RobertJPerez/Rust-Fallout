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

const CONNECTED_ACTIVATION: u64 = 9_007_199_254_740_993;
const CONNECTED_A: u64 = 0x7ff8123456789abc;
fn connected_source(path: &Path, unsupported: bool) -> Vec<u8> {
    let body = if unsupported {
        [
            copy(2, 1),
            copy(3, 2),
            copy(2, 4),
            assignment(&[b's', 1, 0], b"123"),
        ]
        .concat()
    } else {
        chain()
    };
    let bytes = [
        event(&body),
        event(&copy(2, 1)),
        event(&copy(3, 2)),
        event(&copy(4, 3)),
    ]
    .concat();
    assert_eq!(bytes.len(), 140);
    write_source(path, &bytes);
    let placement: Vec<_> = [0x3f800000_u32, 0x80000000, 1, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    let mut plugin = fs::read(path.join("FalloutNV.esm")).unwrap();
    plugin.extend(
        [
            record(b"QUST", 0x200, 0, &field(b"SCRI", &0x300_u32.to_le_bytes())),
            record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
            record(
                b"REFR",
                0x500,
                0,
                &[
                    field(b"NAME", &0x100_u32.to_le_bytes()),
                    field(b"DATA", &placement),
                ]
                .concat(),
            ),
        ]
        .concat(),
    );
    fs::write(path.join("FalloutNV.esm"), plugin).unwrap();
    bytes
}
fn connected_fixture(unsupported: bool) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    connected_source(directory.path(), unsupported);
    let (catalogue, content) = loaded(directory.path());
    (directory, catalogue, content)
}
fn connected_world(catalogue: Arc<Catalogue>, placed: bool) -> (World<'static>, Owner) {
    use fallout_runtime::{events::Clocks, inventory, reference_state};
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        Default::default(),
        CampaignId::from_bytes([0x75; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x500))).unwrap();
    let view = world.reference_view(reference).unwrap();
    let pose = reference_state::Pose::from_source(
        &fallout_data::world::Transform {
            position: [1.0, -0.0, f32::from_bits(1)],
            rotation: [0.0; 3],
        },
        None,
    )
    .unwrap();
    let edit = world
        .stage_reference_state(
            &view,
            reference_state::State::new(form(0x400), pose, false).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(edit).unwrap();
    world.initialize_inventory(reference).unwrap();
    let mut facts = inventory::Facts::unknown(form(0x100));
    facts.condition = Some(inventory::Condition::Float32 { bits: 0x7fc12345 });
    facts.extra_fields.push(inventory::OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 255, 1],
    });
    world
        .add_item(reference, facts, 19.try_into().unwrap())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        })
        .unwrap();
    let quest_owner = Owner::Quest { key: form(0x200) };
    let placed_owner = Owner::Placed { reference };
    let mut handles = Vec::new();
    let mut contexts = Vec::new();
    for (is_placed, owner, a, d) in [
        (false, quest_owner.clone(), CONNECTED_A, 0x8000000000000000),
        (
            true,
            placed_owner.clone(),
            0xfff0123456789abc,
            0x8000000000000001,
        ),
    ] {
        let context = Context {
            calling_reference: Some(reference),
            arguments: vec![
                if is_placed {
                    ReferenceValue::Content { key: form(0x100) }
                } else {
                    ReferenceValue::Null
                },
                ReferenceValue::Live { id: reference },
            ],
            ..Default::default()
        };
        let handle = world
            .create_instance(&definition, owner, context.clone())
            .unwrap();
        world
            .assign(
                handle,
                &[
                    (1, Value::Number { bits: a }),
                    (
                        3,
                        Value::Number {
                            bits: 0x3ff0000000000000,
                        },
                    ),
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
        handles.push(handle);
        contexts.push(context);
    }
    let selected = usize::from(placed);
    world
        .enqueue(
            handles[selected],
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            contexts[selected].clone(),
        )
        .unwrap();
    let fragment_context = Context {
        arguments: vec![
            ReferenceValue::Null,
            ReferenceValue::Content { key: form(0x100) },
        ],
        ..Default::default()
    };
    let fragment = world
        .create_instance(
            &definition,
            Owner::Fragment {
                activation: CONNECTED_ACTIVATION.try_into().unwrap(),
            },
            fragment_context.clone(),
        )
        .unwrap();
    world
        .assign(
            fragment,
            &[
                (1, Value::Number { bits: CONNECTED_A }),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Content { key: form(0x100) },
                    },
                ),
            ],
        )
        .unwrap();
    for begin_byte_offset in [62, 88, 114] {
        world
            .enqueue(
                fragment,
                Trigger::Block {
                    event_id: 0,
                    begin_byte_offset,
                },
                fragment_context.clone(),
            )
            .unwrap();
    }
    world
        .enqueue(
            handles[1 - selected],
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            contexts[1 - selected].clone(),
        )
        .unwrap();
    let tail = world
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
            tail,
            &[
                (1, Value::Number { bits: u64::MAX }),
                (4, Value::Number { bits: 1 }),
            ],
        )
        .unwrap();
    world
        .enqueue(
            tail,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 19,
            game_nanoseconds: 124,
            menu_nanoseconds: 9,
            real_nanoseconds: 1000,
        })
        .unwrap();
    (world, if placed { placed_owner } else { quest_owner })
}
fn connected_expected(
    mut before: fallout_runtime::snapshot::Snapshot,
    placed: bool,
    drained: usize,
) -> fallout_runtime::snapshot::Snapshot {
    let (a, d) = if placed {
        (0xfff0123456789abc, 0x8000000000000001)
    } else {
        (CONNECTED_A, 0x8000000000000000)
    };
    let instance = before
        .instances
        .iter_mut()
        .find(|instance| {
            if placed {
                matches!(instance.owner, Owner::Placed { .. })
            } else {
                matches!(instance.owner, Owner::Quest { .. })
            }
        })
        .unwrap();
    for (index, bits) in [(1, d), (2, d), (3, a)] {
        instance
            .locals
            .iter_mut()
            .find(|local| local.index == index)
            .unwrap()
            .value = Value::Number { bits };
    }
    let fragment = before
        .instances
        .iter_mut()
        .find(|instance| {
            instance.owner
                == Owner::Fragment {
                    activation: CONNECTED_ACTIVATION.try_into().unwrap(),
                }
        })
        .unwrap();
    for index in 2..2 + drained as u32 {
        fragment
            .locals
            .iter_mut()
            .find(|local| local.index == index)
            .unwrap()
            .value = Value::Number { bits: CONNECTED_A };
    }
    before.state_revision += 1 + drained as u64;
    before.pending_events.drain(..1 + drained);
    before
}

#[test]
fn named_complete_event_separates_owners_and_preserves_literal_whole_cold_world() {
    let (_directory, catalogue, content) = connected_fixture(false);
    let prepared = sources(&catalogue);
    for placed in [false, true] {
        let (mut world, owner) = connected_world(Arc::clone(&catalogue), placed);
        let before = world.snapshot();
        let input = before.encode(64 * 1024 * 1024).unwrap();
        let sample = staged(
            copy_probe::stage_pending_multi_owned(
                &world,
                &prepared,
                &content,
                1,
                &owner,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
        );
        assert_eq!(world.snapshot(), before);
        let (a, d) = if placed {
            (0xfff0123456789abc, 0x8000000000000001)
        } else {
            (CONNECTED_A, 0x8000000000000000)
        };
        assert_eq!(
            sample
                .trace()
                .statements
                .iter()
                .map(|s| s.copied_bits)
                .collect::<Vec<_>>(),
            [a, a, d, d]
        );
        assert_eq!(
            sample
                .trace()
                .statements
                .iter()
                .map(|s| s.source_from_overlay)
                .collect::<Vec<_>>(),
            [false, true, false, true]
        );
        for (index, statement) in sample.trace().statements.iter().enumerate() {
            assert_eq!(
                statement.statement_scda_bytes,
                (10 + index * 12)..(22 + index * 12)
            );
            assert_eq!(
                statement.source_token_scda_bytes,
                (19 + index * 12)..(22 + index * 12)
            );
        }
        assert_eq!(sample.trace().frame.counts.binding_uses, 14);
        assert_eq!(sample.trace().frame.counts.source_bytes, 62);
        drop(sample);
        let committed = match copy_probe::commit_pending_multi_owned(
            &mut world,
            &prepared,
            &content,
            1,
            &owner,
            Intent::Engineering,
            Default::default(),
        )
        .unwrap()
        {
            copy_probe::PendingMultiOutcome::EngineeringCommitted { committed } => committed,
            other => panic!("{other:?}"),
        };
        assert_eq!(committed.receipt.assignments, 3);
        assert_eq!(committed.receipt.before_revision, before.state_revision);
        assert_eq!(committed.receipt.after_revision, before.state_revision + 1);
        assert_eq!(
            committed.receipt.acknowledged.as_ref(),
            Some(&before.pending_events[0])
        );
        let expected = connected_expected(before.clone(), placed, 0);
        assert_eq!(world.snapshot(), expected);
        let mut cold =
            World::restore(Arc::clone(&catalogue), expected.clone(), Default::default()).unwrap();
        assert_eq!(cold.snapshot(), expected);
        assert!(
            copy_probe::commit_pending_multi_owned(
                &mut cold,
                &prepared,
                &content,
                1,
                &owner,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
        );
        assert_eq!(cold.snapshot(), expected);
        assert_eq!(before.encode(64 * 1024 * 1024).unwrap(), input);
    }
}

#[test]
fn named_event_then_cooperative_fragment_prefix_preserves_independent_whole_expected() {
    use fallout_runtime::execution::pending_batch::{self, Job, Outcome, Request, Status};
    let (_directory, catalogue, content) = connected_fixture(false);
    let prepared = sources(&catalogue);
    let requests: Vec<_> = (2..=4_u64)
        .map(|sequence| Request {
            sequence: sequence.try_into().unwrap(),
            activation: CONNECTED_ACTIVATION.try_into().unwrap(),
        })
        .collect();
    for placed in [false, true] {
        let (mut world, owner) = connected_world(Arc::clone(&catalogue), placed);
        let before = world.snapshot();
        copy_probe::commit_pending_multi_owned(
            &mut world,
            &prepared,
            &content,
            1,
            &owner,
            Intent::Engineering,
            Default::default(),
        )
        .unwrap();
        let after_multi = world.snapshot();
        for slice in [1, 2, 3, usize::MAX] {
            let private = World::restore(
                Arc::clone(&catalogue),
                after_multi.clone(),
                Default::default(),
            )
            .unwrap();
            let mut job = Job::new(
                private,
                &prepared,
                &content,
                &requests,
                Intent::Engineering,
                pending_batch::Limits::default(),
            )
            .unwrap();
            while job.progress().status == Status::Pending {
                job.advance(slice).unwrap();
            }
            assert_eq!(job.progress().work.source_frame_attempts, 3);
            assert_eq!(job.progress().work.copy_adapter_attempts, 3);
            let result = match job.finish().unwrap() {
                Outcome::EngineeringCommitted { result } => result,
                other => panic!("{other:?}"),
            };
            assert_eq!(result.counts.source_instructions, 9);
            assert_eq!(result.counts.statement_bytes, 36);
            assert_eq!(result.counts.trace_projection.source_bytes, 78);
            assert_eq!(result.counts.trace_projection.rows, 24);
            assert_eq!(result.counts.trace_projection.binding_uses, 42);
            let expected = connected_expected(before.clone(), placed, 3);
            assert_eq!(result.snapshot, expected);
            assert_eq!(
                World::restore(Arc::clone(&catalogue), result.snapshot, Default::default())
                    .unwrap()
                    .snapshot(),
                expected
            );
        }
    }
}

#[test]
fn named_identity_and_all_exact_one_under_limits_refuse_before_effects() {
    let (_directory, catalogue, content) = connected_fixture(false);
    let prepared = sources(&catalogue);
    for placed in [false, true] {
        let (world, owner) = connected_world(Arc::clone(&catalogue), placed);
        let before = world.snapshot();
        let sample = staged(
            copy_probe::stage_pending_multi_owned(
                &world,
                &prepared,
                &content,
                1,
                &owner,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
        );
        let c = sample.trace().frame.counts;
        let owner_bytes = if placed { 0 } else { "falloutnv.esm".len() };
        let exact = local_copy::MultiLimits {
            maximum_event_instructions: 6,
            maximum_statements: 4,
            maximum_operand_uses: 8,
            maximum_statement_bytes: 48,
            observation: fallout_runtime::preparation::ObservationLimits {
                maximum_source_bytes: 62,
                maximum_rows: c.rows,
                maximum_variable_bytes: c.variable_bytes + owner_bytes,
                maximum_binding_uses: 14,
            },
        };
        assert!(matches!(
            copy_probe::stage_pending_multi_owned(
                &world,
                &prepared,
                &content,
                1,
                &owner,
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
            }
            assert!(
                copy_probe::stage_pending_multi_owned(
                    &world,
                    &prepared,
                    &content,
                    1,
                    &owner,
                    Intent::Engineering,
                    limit
                )
                .is_err(),
                "field{field}"
            );
            assert_eq!(world.snapshot(), before);
        }
        for wrong in [
            Owner::Quest { key: form(0x201) },
            Owner::Placed {
                reference: 2_u64
                    .try_into()
                    .map(fallout_runtime::identity::ReferenceId)
                    .unwrap(),
            },
            Owner::Fragment {
                activation: CONNECTED_ACTIVATION.try_into().unwrap(),
            },
        ] {
            assert!(
                copy_probe::stage_pending_multi_owned(
                    &world,
                    &prepared,
                    &content,
                    1,
                    &wrong,
                    Intent::Engineering,
                    Default::default()
                )
                .is_err()
            );
        }
        assert!(
            copy_probe::stage_pending_multi_owned(
                &world,
                &prepared,
                &content,
                2,
                &owner,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
        );
        let oversized = Owner::Quest {
            key: fallout_data::identity::FormKey {
                origin_plugin: "x".repeat(1024),
                ..form(0x200)
            },
        };
        let mut limit = exact;
        limit.observation.maximum_variable_bytes = 1023;
        assert!(matches!(
            copy_probe::stage_pending_multi_owned(
                &world,
                &prepared,
                &content,
                1,
                &oversized,
                Intent::Engineering,
                limit
            ),
            Err(copy_probe::Error::Capacity("explicit owner variable bytes"))
        ));
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn named_late_unsupported_and_stale_opaque_stages_return_no_effects() {
    let (_directory, catalogue, content) = connected_fixture(true);
    let prepared = sources(&catalogue);
    for placed in [false, true] {
        let (mut world, owner) = connected_world(Arc::clone(&catalogue), placed);
        let before = world.snapshot();
        assert!(matches!(
            copy_probe::commit_pending_multi_owned(
                &mut world,
                &prepared,
                &content,
                1,
                &owner,
                Intent::Engineering,
                Default::default()
            )
            .unwrap(),
            copy_probe::PendingMultiOutcome::Unsupported {
                reason: Unsupported::ExpressionShape,
                ..
            }
        ));
        assert_eq!(world.snapshot(), before);
        assert!(matches!(
            copy_probe::commit_pending_multi_owned(
                &mut world,
                &prepared,
                &content,
                1,
                &owner,
                Intent::Faithful,
                Default::default()
            )
            .unwrap(),
            copy_probe::PendingMultiOutcome::Unsupported {
                reason: Unsupported::UnverifiedRetailSemantics,
                ..
            }
        ));
        assert_eq!(world.snapshot(), before);
    }
    let (_directory, catalogue, content) = connected_fixture(false);
    let prepared = sources(&catalogue);
    for variant in 0..4 {
        let (mut world, owner) = connected_world(Arc::clone(&catalogue), false);
        let before = world.snapshot();
        let proposal = staged(
            copy_probe::stage_pending_multi_owned(
                &world,
                &prepared,
                &content,
                1,
                &owner,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
        );
        match variant {
            0 => {
                let handle = world.handle(before.instances[0].id).unwrap();
                world
                    .assign(handle, &[(4, Value::Number { bits: 21 })])
                    .unwrap();
            }
            1 => {
                world = World::restore(Arc::clone(&catalogue), before, Default::default()).unwrap()
            }
            2 => {
                let mut other = before;
                other.campaign = CampaignId::from_bytes([0x76; 16]).unwrap();
                world = World::restore(Arc::clone(&catalogue), other, Default::default()).unwrap();
            }
            _ => {
                let changes = world.stage_event_changes(1, &[], true).unwrap();
                world.commit_event_changes(changes).unwrap();
            }
        }
        let after_external = world.snapshot();
        assert!(proposal.commit(&mut world).is_err());
        assert_eq!(world.snapshot(), after_external);
    }
}

#[test]
#[ignore = "fresh frozen CLI and authored metadata; named complete event then cooperative saved prefix"]
fn cli_saved_named_multi_copy_helper() {
    use fallout_runtime::snapshot::Snapshot;
    use serde_json::{Value as Json, json};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_NAMED_MULTI_COPY_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    let scda = connected_source(&install.join("Data"), false);
    fs::write(evidence.join("source.scda"), &scda).unwrap();
    fs::copy(
        input.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let executable = fs::read(install.join("FalloutNV.exe")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&executable)),
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let source = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (catalogue, _) = loaded(&install.join("Data"));
    let cases = std::cell::Cell::new(0_usize);
    let run = |name: &str,
               source_install: &Path,
               snapshot: &Path,
               request: &Json,
               flag: &str,
               extra: &[&str],
               paths: (Option<&Path>, Option<&Path>)| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        let request_bytes = serde_json::to_vec_pretty(request).unwrap();
        fs::write(&request_path, &request_bytes).unwrap();
        let result = paths
            .0
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory.join("result.snapshot.json"));
        let report = paths
            .1
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory.join("report.json"));
        let input_bytes = fs::read(snapshot).unwrap();
        let source_bytes = fs::read(source_install.join("Data/FalloutNV.esm")).unwrap();
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(source_install)
            .arg("--load-order")
            .arg(&order)
            .arg(flag)
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(snapshot)
            .arg("--snapshot-output")
            .arg(&result)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        cases.set(cases.get() + 1);
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(directory.join("process.json"), serde_json::to_vec_pretty(&json!({"exit_code":output.status.code(),"original_launched":false,"input_snapshot_sha256":format!("{:x}",Sha256::digest(&input_bytes)),"source_sha256":format!("{:x}",Sha256::digest(&source_bytes)),"request_sha256":format!("{:x}",Sha256::digest(&request_bytes))})).unwrap()).unwrap();
        assert_eq!(fs::read(snapshot).unwrap(), input_bytes);
        assert_eq!(fs::read(&request_path).unwrap(), request_bytes);
        assert_eq!(
            fs::read(source_install.join("Data/FalloutNV.esm")).unwrap(),
            source_bytes
        );
        let value = report
            .exists()
            .then(|| serde_json::from_slice::<Json>(&fs::read(&report).unwrap()).ok())
            .flatten();
        (output, value, result, report)
    };
    let base = json!({"schema_version":1,"sequence":1,"owner":{"kind":"quest","key":form(0x200)},"intent":"engineering",
        "maximum_source_instructions":262144,"maximum_statements":64,"maximum_operand_uses":128,"maximum_statement_bytes":65539,
        "maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,
        "maximum_trace_bytes":2097152,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let multi_flag = "--snapshot-multi-copy-request";
    let batch = json!({"schema_version":1,"intent":"engineering","events":[{"sequence":2,"activation":CONNECTED_ACTIVATION},{"sequence":3,"activation":CONNECTED_ACTIVATION},{"sequence":4,"activation":CONNECTED_ACTIVATION}],"maximum_source_instructions":192,"maximum_statement_bytes":65539,"maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    for placed in [false, true] {
        let kind = if placed { "placed" } else { "quest" };
        let (world, owner) = connected_world(Arc::clone(&catalogue), placed);
        let before = world.snapshot();
        let initial = evidence.join(format!("{kind}-initial.snapshot.json"));
        fs::write(&initial, before.encode(64 * 1024 * 1024).unwrap()).unwrap();
        fs::write(
            evidence.join(format!("{kind}-expected-multi.snapshot.json")),
            connected_expected(before.clone(), placed, 0)
                .encode(64 * 1024 * 1024)
                .unwrap(),
        )
        .unwrap();
        fs::write(
            evidence.join(format!("{kind}-expected-connected.snapshot.json")),
            connected_expected(before.clone(), placed, 3)
                .encode(64 * 1024 * 1024)
                .unwrap(),
        )
        .unwrap();
        let mut request = base.clone();
        request["owner"] = serde_json::to_value(&owner).unwrap();
        let (output, report, multi_result, _) = run(
            &format!("{kind}-complete"),
            &install,
            &initial,
            &request,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = report.unwrap();
        assert_eq!(
            report["snapshot_multi_copy"]["status"],
            "engineering_committed"
        );
        let committed = &report["snapshot_multi_copy"]["committed"];
        let (a, d) = if placed {
            (0xfff0123456789abc, 0x8000000000000001)
        } else {
            (CONNECTED_A, 0x8000000000000000)
        };
        for (index, bits) in [a, a, d, d].into_iter().enumerate() {
            let statement = &committed["trace"]["statements"][index];
            assert_eq!(statement["copied_bits"], bits);
            assert_eq!(statement["source_from_overlay"], index == 1 || index == 3);
            assert_eq!(
                statement["statement_scda_bytes"],
                json!({"start":10+index*12,"end":22+index*12})
            );
            assert_eq!(
                statement["source_token_scda_bytes"],
                json!({"start":19+index*12,"end":22+index*12})
            );
        }
        assert_eq!(committed["receipt"]["assignments"], 3);
        assert_eq!(
            committed["receipt"]["before_revision"],
            before.state_revision
        );
        assert_eq!(
            committed["receipt"]["after_revision"],
            before.state_revision + 1
        );
        assert_eq!(
            committed["receipt"]["acknowledged"],
            serde_json::to_value(&before.pending_events[0]).unwrap()
        );
        assert_eq!(
            committed["trace"]["frame"]["source_bytes"],
            serde_json::to_value(&scda[..62]).unwrap()
        );
        assert_eq!(committed["trace"]["frame"]["counts"]["binding_uses"], 14);
        assert_eq!(report["faithful_execution_admitted"], false);
        let expected_multi = connected_expected(before.clone(), placed, 0);
        let actual =
            Snapshot::decode(&fs::read(&multi_result).unwrap(), Default::default()).unwrap();
        assert_eq!(actual, expected_multi);
        assert_eq!(
            World::restore(Arc::clone(&catalogue), actual, Default::default())
                .unwrap()
                .snapshot(),
            expected_multi
        );
        let (output, report_value, result, _) = run(
            &format!("{kind}-cold-replay"),
            &install,
            &multi_result,
            &request,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(!output.status.success() && report_value.is_none() && !result.exists());
        for slice in [1, 2] {
            let slice_text = slice.to_string();
            let (output, report_value, result, _) = run(
                &format!("{kind}-connected-slice-{slice}"),
                &install,
                &multi_result,
                &batch,
                "--snapshot-copy-batch-request",
                &["--snapshot-copy-batch-slice-events", &slice_text],
                (None, None),
            );
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report_value = report_value.unwrap();
            assert_eq!(
                report_value["snapshot_batch"]["counts"]["source_instructions"],
                9
            );
            assert_eq!(
                report_value["snapshot_batch"]["counts"]["trace_projection"]["binding_uses"],
                42
            );
            let progress = report_value["cooperative_execution"]["progress"]
                .as_array()
                .unwrap()
                .last()
                .unwrap();
            assert_eq!(progress["work"]["source_frame_attempts"], 3);
            assert_eq!(progress["work"]["copy_adapter_attempts"], 3);
            let expected = connected_expected(before.clone(), placed, 3);
            let actual = Snapshot::decode(&fs::read(&result).unwrap(), Default::default()).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                World::restore(Arc::clone(&catalogue), actual, Default::default())
                    .unwrap()
                    .snapshot(),
                expected
            );
            if slice == 1 {
                let other_owner = before
                    .instances
                    .iter()
                    .find(|instance| {
                        if placed {
                            matches!(instance.owner, Owner::Quest { .. })
                        } else {
                            matches!(instance.owner, Owner::Placed { .. })
                        }
                    })
                    .unwrap()
                    .owner
                    .clone();
                let mut next = request.clone();
                next["sequence"] = json!(5);
                next["owner"] = serde_json::to_value(other_owner).unwrap();
                let (output, _, next_result, _) = run(
                    &format!("{kind}-cold-other-owner"),
                    &install,
                    &result,
                    &next,
                    multi_flag,
                    &[],
                    (None, None),
                );
                assert!(output.status.success());
                assert_eq!(
                    Snapshot::decode(&fs::read(next_result).unwrap(), Default::default()).unwrap(),
                    connected_expected(expected, !placed, 0)
                );
            }
        }
        let mut late = batch.clone();
        late["maximum_source_instructions"] = json!(8);
        let (output, report_value, result, _) = run(
            &format!("{kind}-connected-late-cap"),
            &install,
            &multi_result,
            &late,
            "--snapshot-copy-batch-request",
            &["--snapshot-copy-batch-slice-events", "1"],
            (None, None),
        );
        assert!(!output.status.success() && report_value.is_none() && !result.exists());
        let counts = &committed["trace"]["frame"]["counts"];
        let mut exact = request.clone();
        exact["maximum_source_instructions"] = json!(6);
        exact["maximum_statements"] = json!(4);
        exact["maximum_operand_uses"] = json!(8);
        exact["maximum_statement_bytes"] = json!(48);
        exact["maximum_trace_source_bytes"] = json!(62);
        exact["maximum_trace_rows"] = counts["rows"].clone();
        exact["maximum_trace_variable_bytes"] = json!(
            counts["variable_bytes"].as_u64().unwrap()
                + report["explicit_owner_variable_bytes"].as_u64().unwrap()
        );
        exact["maximum_trace_binding_uses"] = json!(14);
        exact["maximum_trace_bytes"] = report["trace_bytes"].clone();
        exact["maximum_result_snapshot_bytes"] = json!(fs::read(&multi_result).unwrap().len());
        let (output, _, _, _) = run(
            &format!("{kind}-exact-limits"),
            &install,
            &initial,
            &exact,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for field in [
            "maximum_source_instructions",
            "maximum_statements",
            "maximum_operand_uses",
            "maximum_statement_bytes",
            "maximum_trace_source_bytes",
            "maximum_trace_rows",
            "maximum_trace_variable_bytes",
            "maximum_trace_binding_uses",
            "maximum_trace_bytes",
            "maximum_result_snapshot_bytes",
        ] {
            let mut low = exact.clone();
            low[field] = json!(low[field].as_u64().unwrap() - 1);
            let (output, report_value, result, _) = run(
                &format!("{kind}-one-under-{field}"),
                &install,
                &initial,
                &low,
                multi_flag,
                &[],
                (None, None),
            );
            assert!(
                !output.status.success() && report_value.is_none() && !result.exists(),
                "{kind}/{field}"
            );
        }
        let (output, _, _, path) = run(
            &format!("{kind}-report-bound-a"),
            &install,
            &initial,
            &request,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(output.status.success());
        let length = fs::read(path).unwrap().len();
        let mut capped = request.clone();
        capped["maximum_report_bytes"] = json!(length);
        let (output, _, _, _) = run(
            &format!("{kind}-report-bound-b"),
            &install,
            &initial,
            &capped,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(output.status.success());
        capped["maximum_report_bytes"] = json!(length - 1);
        let (output, report_value, result, _) = run(
            &format!("{kind}-report-bound-c"),
            &install,
            &initial,
            &capped,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(!output.status.success() && report_value.is_none() && !result.exists());
        for (name, variant) in [
            ("wrong-owner", 0),
            ("wrong-kind", 1),
            ("wrong-head", 2),
            ("fragment", 3),
            ("intent-map", 4),
            ("intent-null", 5),
            ("unknown-field", 6),
            ("bad-schema", 7),
            ("zero-sequence", 8),
            ("missing-owner", 9),
            ("source-ceiling", 10),
            ("owner-precharge", 11),
        ] {
            let mut invalid = request.clone();
            match variant {
                0 => {
                    invalid["owner"] = if placed {
                        json!({"kind":"placed","reference":2})
                    } else {
                        json!({"kind":"quest","key":form(0x201)})
                    }
                }
                1 => {
                    invalid["owner"] = if placed {
                        json!({"kind":"quest","key":form(0x200)})
                    } else {
                        json!({"kind":"placed","reference":1})
                    }
                }
                2 => invalid["sequence"] = json!(2),
                3 => {
                    invalid["owner"] = json!({"kind":"fragment","activation":CONNECTED_ACTIVATION})
                }
                4 => invalid["intent"] = json!({"engineering":null}),
                5 => invalid["intent"] = Json::Null,
                6 => invalid["unknown"] = json!(true),
                7 => invalid["schema_version"] = json!(2),
                8 => invalid["sequence"] = json!(0),
                9 => {
                    invalid.as_object_mut().unwrap().remove("owner");
                }
                10 => invalid["maximum_source_instructions"] = json!(262145),
                _ => invalid["maximum_trace_variable_bytes"] = json!(0),
            }
            let (output, report_value, result, _) = run(
                &format!("{kind}-{name}"),
                &install,
                &initial,
                &invalid,
                multi_flag,
                &[],
                (None, None),
            );
            assert!(
                !output.status.success() && report_value.is_none() && !result.exists(),
                "{kind}/{name}"
            );
        }
        let mut faithful = request.clone();
        faithful["intent"] = json!("faithful");
        let (output, report_value, result, _) = run(
            &format!("{kind}-faithful"),
            &install,
            &initial,
            &faithful,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(!output.status.success() && !result.exists());
        let report_value = report_value.unwrap();
        assert_eq!(
            report_value["snapshot_multi_copy"]["reason"],
            "unverified_retail_semantics"
        );
        assert_eq!(
            report_value["before_revision"],
            report_value["after_revision"]
        );
        let unset = evidence.join(format!("{kind}-unset.snapshot.json"));
        let mut snapshot = before.clone();
        snapshot
            .instances
            .iter_mut()
            .find(|instance| instance.owner == owner)
            .unwrap()
            .locals
            .iter_mut()
            .find(|local| local.index == 1)
            .unwrap()
            .value = Value::Uninitialized;
        fs::write(&unset, snapshot.encode(64 * 1024 * 1024).unwrap()).unwrap();
        let (output, report_value, result, _) = run(
            &format!("{kind}-unset"),
            &install,
            &unset,
            &request,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(!output.status.success() && !result.exists());
        let report_value = report_value.unwrap();
        assert_eq!(
            report_value["snapshot_multi_copy"]["reason"],
            "live_operand_unavailable"
        );
        assert_eq!(report_value["result_snapshot"], Json::Null);
    }
    let initial = evidence.join("quest-initial.snapshot.json");
    for (name, extra) in [
        (
            "single-conflict",
            vec!["--snapshot-copy-request", "missing.json"],
        ),
        (
            "batch-conflict",
            vec!["--snapshot-copy-batch-request", "missing.json"],
        ),
        (
            "native-conflict",
            vec!["--snapshot-native-request", "missing.json"],
        ),
        (
            "slice-requires-batch",
            vec!["--snapshot-copy-batch-slice-events", "1"],
        ),
        (
            "seed-conflict",
            vec!["--engineering-local-copy", "missing.json"],
        ),
        ("prepared-conflict", vec!["--prepared-sources"]),
    ] {
        let (output, report_value, result, _) = run(
            name,
            &install,
            &initial,
            &base,
            multi_flag,
            &extra,
            (None, None),
        );
        assert!(!output.status.success() && report_value.is_none() && !result.exists());
    }
    let mixed = evidence.join("unsupported-source-copy");
    fs::create_dir(&mixed).unwrap();
    fs::create_dir(mixed.join("Data")).unwrap();
    connected_source(&mixed.join("Data"), true);
    fs::copy(install.join("FalloutNV.exe"), mixed.join("FalloutNV.exe")).unwrap();
    let (mixed_catalogue, _) = loaded(&mixed.join("Data"));
    let (mixed_world, _) = connected_world(mixed_catalogue, false);
    let mixed_input = evidence.join("unsupported.snapshot.json");
    fs::write(
        &mixed_input,
        mixed_world.snapshot().encode(64 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    let (output, report_value, result, _) = run(
        "unsupported-last-assignment",
        &mixed,
        &mixed_input,
        &base,
        multi_flag,
        &[],
        (None, None),
    );
    assert!(!output.status.success() && !result.exists());
    let report_value = report_value.unwrap();
    assert_eq!(
        report_value["snapshot_multi_copy"]["reason"],
        "expression_shape"
    );
    assert!(
        report_value["snapshot_multi_copy"]
            .get("committed")
            .is_none()
    );
    assert_eq!(
        report_value["before_revision"],
        report_value["after_revision"]
    );
    let (output, report_value, result, _) = run(
        "stale-whole-cohort",
        &mixed,
        &initial,
        &base,
        multi_flag,
        &[],
        (None, None),
    );
    assert!(!output.status.success() && report_value.is_none() && !result.exists());
    for variant in 0..3 {
        let mut invalid: Json = serde_json::from_slice(
            &fs::read(evidence.join("placed-initial.snapshot.json")).unwrap(),
        )
        .unwrap();
        match variant {
            0 => invalid["schema_version"] = json!(3),
            1 => invalid["references"] = json!([]),
            _ => invalid["pending_events"][0]["trigger"]["begin_byte_offset"] = json!(1),
        }
        let input_path = evidence.join(format!("invalid-{variant}.snapshot.json"));
        fs::write(&input_path, serde_json::to_vec_pretty(&invalid).unwrap()).unwrap();
        let mut placed_request = base.clone();
        placed_request["owner"] = json!({"kind":"placed","reference":1});
        let (output, report_value, result, _) = run(
            &format!("invalid-saved-{variant}"),
            &install,
            &input_path,
            &placed_request,
            multi_flag,
            &[],
            (None, None),
        );
        assert!(!output.status.success() && report_value.is_none() && !result.exists());
    }
    let sentinel = evidence.join("existing-output.json");
    fs::write(&sentinel, b"Retain existing output").unwrap();
    let protected = install.join("protected-output.json");
    for (name, result, report) in [
        ("result-input", Some(initial.as_path()), None),
        ("report-input", None, Some(initial.as_path())),
        ("result-existing", Some(sentinel.as_path()), None),
        ("report-existing", None, Some(sentinel.as_path())),
        ("result-protected", Some(protected.as_path()), None),
        ("report-protected", None, Some(protected.as_path())),
    ] {
        let (output, _, actual_result, actual_report) = run(
            name,
            &install,
            &initial,
            &base,
            multi_flag,
            &[],
            (result, report),
        );
        assert!(!output.status.success());
        if result.is_none() {
            assert!(!actual_result.exists());
        }
        if report.is_none() {
            assert!(!actual_report.exists());
        }
    }
    let alias = evidence.join("same-output.json");
    let (output, _, _, _) = run(
        "same-outputs",
        &install,
        &initial,
        &base,
        multi_flag,
        &[],
        (Some(&alias), Some(&alias)),
    );
    assert!(!output.status.success() && !alias.exists());
    let hardlink = evidence.join("input-hardlink.json");
    fs::hard_link(&initial, &hardlink).unwrap();
    let (output, _, result, _) = run(
        "report-hardlink",
        &install,
        &initial,
        &base,
        multi_flag,
        &[],
        (None, Some(&hardlink)),
    );
    assert!(!output.status.success() && !result.exists());
    let mut oversized = base.clone();
    oversized["padding"] = json!("x".repeat(16 * 1024));
    let (output, report_value, result, _) = run(
        "oversized-request",
        &install,
        &initial,
        &oversized,
        multi_flag,
        &[],
        (None, None),
    );
    assert!(!output.status.success() && report_value.is_none() && !result.exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"Retain existing output");
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), executable);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source
    );
    assert_eq!(fs::read(&order).unwrap(), b"[\"FalloutNV.esm\"]");
    fs::write(evidence.join("acceptance.json"),serde_json::to_vec_pretty(&json!({"schema_version":1,"cases":cases.get(),"scope":"exact saved Quest/Placed complete copy event and connected cooperative Fragment prefix","source_scda_sha256":format!("{:x}",Sha256::digest(&scda)),"source_scda_bytes":scda.len(),"original_launches":0,"faithful_execution_admitted":false,"gameplay_accepted":false})).unwrap()).unwrap();
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
