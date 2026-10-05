mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::{
        local_copy::{Intent, Unsupported},
        pending_batch::{
            self, CommittedAdapter, Job, Outcome, OwnerRequest, Request, Status, WorkCounts,
        },
    },
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use std::{fs, path::Path, sync::Arc};

fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn assignment(destination: u8, expression: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x15,
        &[
            &[b's', destination, 0][..],
            &(expression.len() as u16).to_le_bytes(),
            expression,
        ]
        .concat(),
    );
    out
}
fn event(destination: u8, source: u8) -> Vec<u8> {
    event_expression(destination, &[b'f', source, 0])
}
fn event_expression(destination: u8, expression: &[u8]) -> Vec<u8> {
    let body = assignment(destination, expression);
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
fn write_source(path: &Path, unsupported: bool) {
    let bytes = [
        event(2, 1),
        if unsupported {
            event_expression(3, b"123")
        } else {
            event(3, 2)
        },
        event(4, 3),
    ]
    .concat();
    let original = unit(
        &[(1, 0), (2, 1), (3, 0), (4, 0), (90, 0)],
        &[(b"SCRO", 0x100), (b"SCRV", 90)],
    );
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", &bytes));
    source.extend(&original[46..]);
    let placement: Vec<_> = [0x3f800000_u32, 0x80000000, 1, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &source),
            record(b"MISC", 0x100, 0, &[]),
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
fn fixture(unsupported: bool) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    write_source(directory.path(), unsupported);
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
        .map(|(i, text)| Operator {
            code: i as u32,
            precedence: i as u8,
            spelling: text.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Signatures::new(),
        Default::default(),
    )
    .unwrap()
}
fn saved(catalogue: Arc<Catalogue>, bits: Option<u64>) -> World<'static> {
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        Default::default(),
        CampaignId::from_bytes([0x73; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x500))).unwrap();
    let view = world.reference_view(reference).unwrap();
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
            fallout_runtime::reference_state::State::new(form(0x400), pose, false).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(proposal).unwrap();
    world.initialize_inventory(reference).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x100));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc12345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"TEST",
            bytes: vec![0, 255, 1],
        });
    world
        .add_item(reference, facts, 19.try_into().unwrap())
        .unwrap();
    let context = Context {
        arguments: vec![
            ReferenceValue::Live { id: reference },
            ReferenceValue::Content { key: form(0x100) },
        ],
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
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x100) },
                },
            )],
        )
        .unwrap();
    if let Some(bits) = bits {
        world
            .assign(handle, &[(1, Value::Number { bits })])
            .unwrap();
    }
    for begin_byte_offset in [0, 26, 52] {
        world
            .enqueue(
                handle,
                Trigger::Block {
                    event_id: 0,
                    begin_byte_offset,
                },
                context.clone(),
            )
            .unwrap();
    }
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
        .assign(other, &[(1, Value::Number { bits: 999 })])
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
    world
}
fn requests(count: usize) -> Vec<Request> {
    (1..=count)
        .map(|sequence| Request {
            sequence: (sequence as u64).try_into().unwrap(),
            activation: 1.try_into().unwrap(),
        })
        .collect()
}
fn append_owned_events(world: &mut World<'_>) -> (Owner, Owner) {
    let initial = world.snapshot();
    let fragment = initial
        .instances
        .iter()
        .find(|instance| {
            matches!(
                &instance.owner,
                Owner::Fragment { activation } if activation.get() == 1
            )
        })
        .unwrap();
    let fragment_handle = world.handle(fragment.id).unwrap();
    let definition = world
        .instance(fragment_handle)
        .unwrap()
        .definition()
        .clone();
    let placed_reference = initial.references[0].id;
    let quest_owner = Owner::Quest { key: form(0x400) };
    let quest = world
        .create_instance(&definition, quest_owner.clone(), Context::default())
        .unwrap();
    world
        .assign(
            quest,
            &[(
                1,
                Value::Number {
                    bits: 11.0_f64.to_bits(),
                },
            )],
        )
        .unwrap();
    world
        .enqueue(
            quest,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let placed_owner = Owner::Placed {
        reference: placed_reference,
    };
    let placed = world
        .create_instance(&definition, placed_owner.clone(), Context::default())
        .unwrap();
    world
        .assign(
            placed,
            &[(
                1,
                Value::Number {
                    bits: 22.0_f64.to_bits(),
                },
            )],
        )
        .unwrap();
    world
        .enqueue(
            placed,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (quest_owner, placed_owner)
}
fn owner_requests(world: &World<'_>) -> Vec<OwnerRequest> {
    world
        .pending_events()
        .map(|pending| OwnerRequest {
            sequence: pending.sequence.try_into().unwrap(),
            expected_owner: world
                .instance(world.handle(pending.instance).unwrap())
                .unwrap()
                .owner()
                .clone(),
        })
        .collect()
}
fn restored(catalogue: Arc<Catalogue>, snapshot: &Snapshot) -> World<'static> {
    World::restore(catalogue, snapshot.clone(), Default::default()).unwrap()
}
fn complete(outcome: Outcome) -> Box<pending_batch::Completed> {
    match outcome {
        Outcome::EngineeringCommitted { result } => result,
        other => panic!("{other:?}"),
    }
}
fn expected(mut before: Snapshot, count: usize, bits: u64) -> Snapshot {
    for index in 2..=count as u32 + 1 {
        before.instances[0]
            .locals
            .iter_mut()
            .find(|local| local.index == index)
            .unwrap()
            .value = Value::Number { bits };
    }
    before.state_revision += count as u64;
    before.pending_events.drain(..count);
    before
}

#[test]
fn ordered_owner_batch_routes_mixed_activations_and_commits_the_exact_prefix_once() {
    let (_directory, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue);
    let mut world = saved(Arc::clone(&catalogue), Some(0x4009_21fb_5444_2d18));
    let (quest_owner, placed_owner) = append_owned_events(&mut world);
    let requests = owner_requests(&world);
    assert_eq!(requests.len(), 6);
    let before = world.snapshot();
    let mut job = Job::new_ordered(
        world,
        &prepared,
        &content,
        &requests,
        Intent::Engineering,
        pending_batch::Limits {
            maximum_source_instructions: 18,
            ..Default::default()
        },
    )
    .unwrap();

    let first = job.advance(2).unwrap();
    assert_eq!(first.status, Status::Pending);
    assert_eq!(first.counts.events, 2);
    let second = job.advance(2).unwrap();
    assert_eq!(second.status, Status::Pending);
    assert_eq!(second.counts.events, 4);
    let last = job.advance(2).unwrap();
    assert_eq!(last.status, Status::Ready);
    assert_eq!(last.counts.events, 6);
    assert_eq!(last.counts.source_instructions, 18);

    let result = complete(job.finish().unwrap());
    assert_eq!(result.counts.events, 6);
    assert_eq!(result.counts.source_instructions, 18);
    assert_eq!(result.committed.len(), 4);
    assert_eq!(result.committed_multi.len(), 2);
    assert_eq!(result.ordered_events.len(), 6);
    assert!(
        result.ordered_events[..4]
            .iter()
            .all(|event| matches!(&event.adapter, CommittedAdapter::FragmentCopy))
    );
    assert!(
        result.ordered_events[4..]
            .iter()
            .all(|event| matches!(&event.adapter, CommittedAdapter::OwnedMultiCopy))
    );
    assert_eq!(result.ordered_events[4].expected_owner, quest_owner);
    assert_eq!(result.ordered_events[5].expected_owner, placed_owner);
    assert_eq!(result.snapshot.state_revision, before.state_revision + 6);
    assert!(result.snapshot.pending_events.is_empty());

    let fragment_one = Owner::Fragment {
        activation: 1.try_into().unwrap(),
    };
    let fragment_two = Owner::Fragment {
        activation: 2.try_into().unwrap(),
    };
    for (owner, index, expected_bits) in [
        (&fragment_one, 2, 0x4009_21fb_5444_2d18),
        (&fragment_one, 3, 0x4009_21fb_5444_2d18),
        (&fragment_one, 4, 0x4009_21fb_5444_2d18),
        (&fragment_two, 2, 999),
        (&quest_owner, 2, 11.0_f64.to_bits()),
        (&placed_owner, 2, 22.0_f64.to_bits()),
    ] {
        let instance = result
            .snapshot
            .instances
            .iter()
            .find(|instance| &instance.owner == owner)
            .unwrap();
        let local = instance
            .locals
            .iter()
            .find(|local| local.index == index)
            .unwrap();
        assert_eq!(
            local.value,
            Value::Number {
                bits: expected_bits
            }
        );
    }

    let mut under_world = saved(Arc::clone(&catalogue), Some(0x4009_21fb_5444_2d18));
    append_owned_events(&mut under_world);
    let under_requests = owner_requests(&under_world);
    let mut under = Job::new_ordered(
        under_world,
        &prepared,
        &content,
        &under_requests,
        Intent::Engineering,
        pending_batch::Limits {
            maximum_source_instructions: 17,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(under.advance(usize::MAX).is_err());
    assert!(under.finish().is_err());
}

#[test]
fn ordered_owner_batch_refuses_a_later_unsupported_quest_operand_without_a_candidate() {
    let (_directory, catalogue, content) = fixture(true);
    let prepared = sources(&catalogue);
    let definition = definition(&catalogue);
    let owner = Owner::Quest { key: form(0x400) };
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x74; 16]).unwrap(),
    )
    .unwrap();
    let handle = world
        .create_instance(&definition, owner.clone(), Context::default())
        .unwrap();
    world
        .assign(
            handle,
            &[(
                1,
                Value::Number {
                    bits: 7.0_f64.to_bits(),
                },
            )],
        )
        .unwrap();
    let first = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let second = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 26,
            },
            Context::default(),
        )
        .unwrap();
    let requests = [first, second].map(|sequence| OwnerRequest {
        sequence: sequence.try_into().unwrap(),
        expected_owner: owner.clone(),
    });
    let mut job = Job::new_ordered(
        world,
        &prepared,
        &content,
        &requests,
        Intent::Engineering,
        Default::default(),
    )
    .unwrap();
    let progress = job.advance(2).unwrap();
    assert_eq!(progress.status, Status::Unsupported);
    assert_eq!(progress.counts.events, 1);
    assert_eq!(progress.work.source_frame_attempts, 2);
    assert!(matches!(
        job.finish().unwrap(),
        Outcome::Unsupported {
            event_index: Some(1),
            reason: Unsupported::ExpressionShape,
            ..
        }
    ));
}

#[test]
fn saved_prefix_has_literal_whole_state_expected_locals_revisions_receipts_and_untouched_tail() {
    let (_directory, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue);
    for bits in [0x8000000000000000, 0x7ff8123456789abc, u64::MAX] {
        let before = saved(Arc::clone(&catalogue), Some(bits)).snapshot();
        let bytes = before.encode(64 * 1024 * 1024).unwrap();
        for count in [1, 2, 3] {
            let result = complete(
                pending_batch::consume(
                    restored(Arc::clone(&catalogue), &before),
                    &prepared,
                    &content,
                    &requests(count),
                    Intent::Engineering,
                    Default::default(),
                )
                .unwrap(),
            );
            let expected = expected(before.clone(), count, bits);
            assert_eq!(result.snapshot, expected);
            assert_eq!(result.counts.events, count);
            assert_eq!(result.counts.source_instructions, count * 3);
            assert_eq!(result.counts.statement_bytes, count * 12);
            assert_eq!(result.counts.trace_projection.source_bytes, count * 26);
            for (index, copy) in result.committed.iter().enumerate() {
                assert_eq!(copy.receipt.assignments, 1);
                assert_eq!(
                    copy.receipt.before_revision,
                    before.state_revision + index as u64
                );
                assert_eq!(
                    copy.receipt.after_revision,
                    before.state_revision + index as u64 + 1
                );
                assert_eq!(
                    copy.receipt.acknowledged.as_ref(),
                    Some(&before.pending_events[index])
                );
            }
            let cold = restored(Arc::clone(&catalogue), &result.snapshot);
            assert_eq!(cold.snapshot(), expected);
            assert_eq!(
                cold.pending_events().next(),
                expected.pending_events.first()
            );
            assert!(
                pending_batch::consume(
                    cold,
                    &prepared,
                    &content,
                    &requests(count),
                    Intent::Engineering,
                    Default::default()
                )
                .is_err()
            );
            assert_eq!(before.encode(64 * 1024 * 1024).unwrap(), bytes);
        }
    }
}
#[test]
fn later_unsupported_and_every_prefix_identity_error_expose_no_partial_result() {
    let (_directory, catalogue, content) = fixture(true);
    let prepared = sources(&catalogue);
    let before = saved(Arc::clone(&catalogue), Some(17)).snapshot();
    let result = pending_batch::consume(
        restored(Arc::clone(&catalogue), &before),
        &prepared,
        &content,
        &requests(2),
        Intent::Engineering,
        Default::default(),
    )
    .unwrap();
    assert!(matches!(
        result,
        Outcome::Unsupported {
            event_index: Some(1),
            reason: Unsupported::ExpressionShape,
            ..
        }
    ));
    let json = serde_json::to_value(result).unwrap();
    assert!(
        json.get("snapshot").is_none()
            && json.get("committed").is_none()
            && json.get("result").is_none()
    );
    for variant in 0..6 {
        let mut request = requests(2);
        match variant {
            0 => request[0].sequence = 2.try_into().unwrap(),
            1 => request[1].sequence = 3.try_into().unwrap(),
            2 => request[1].sequence = 1.try_into().unwrap(),
            3 => request[1].activation = 2.try_into().unwrap(),
            4 => request.clear(),
            _ => request = requests(5),
        };
        assert!(
            pending_batch::consume(
                restored(Arc::clone(&catalogue), &before),
                &prepared,
                &content,
                &request,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
        );
    }
    let faithful_limits = pending_batch::Limits {
        maximum_source_instructions: 0,
        trace_projection: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(matches!(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &requests(2),
            Intent::Faithful,
            faithful_limits
        )
        .unwrap(),
        Outcome::Unsupported {
            event_index: None,
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    let unset = saved(Arc::clone(&catalogue), None).snapshot();
    assert!(matches!(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &unset),
            &prepared,
            &content,
            &requests(2),
            Intent::Engineering,
            Default::default()
        )
        .unwrap(),
        Outcome::Unsupported {
            event_index: Some(0),
            reason: Unsupported::LiveOperandUnavailable,
            ..
        }
    ));
}
#[test]
fn aggregate_projection_and_source_work_exact_and_one_under_limits_bound_later_retention() {
    let (_directory, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue);
    let before = saved(Arc::clone(&catalogue), Some(17)).snapshot();
    let result = complete(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &requests(3),
            Intent::Engineering,
            Default::default(),
        )
        .unwrap(),
    );
    let c = &result.counts;
    let exact = pending_batch::Limits {
        maximum_events: c.events,
        maximum_source_instructions: c.source_instructions,
        maximum_statement_bytes: c.statement_bytes,
        trace_projection: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: c.trace_projection.source_bytes,
            maximum_rows: c.trace_projection.rows,
            maximum_variable_bytes: c.trace_projection.variable_bytes,
            maximum_binding_uses: c.trace_projection.binding_uses,
        },
    };
    assert!(matches!(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &requests(3),
            Intent::Engineering,
            exact
        )
        .unwrap(),
        Outcome::EngineeringCommitted { .. }
    ));
    for field in 0..7 {
        let mut limit = exact;
        match field {
            0 => limit.maximum_events -= 1,
            1 => limit.maximum_source_instructions -= 1,
            2 => limit.maximum_statement_bytes -= 1,
            3 => limit.trace_projection.maximum_source_bytes -= 1,
            4 => limit.trace_projection.maximum_rows -= 1,
            5 => limit.trace_projection.maximum_variable_bytes -= 1,
            _ => limit.trace_projection.maximum_binding_uses -= 1,
        };
        assert!(
            pending_batch::consume(
                restored(Arc::clone(&catalogue), &before),
                &prepared,
                &content,
                &requests(3),
                Intent::Engineering,
                limit
            )
            .is_err(),
            "field{field}"
        );
    }
    // Projection covers both contexts and two selected canonical local values.
    assert_eq!(c.trace_projection.rows, 24);
    assert_eq!(c.trace_projection.binding_uses, 18);
}
#[test]
fn changed_whole_cohort_unselected_source_and_nonfragment_owner_never_return_result() {
    let (directory, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue);
    let before = saved(Arc::clone(&catalogue), Some(17)).snapshot();
    let selected = PreparedSources::load_selected(
        &catalogue,
        &Model::vanilla(
            &Operators::new(
                [
                    "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/",
                    "%", "~",
                ]
                .iter()
                .enumerate()
                .map(|(i, text)| Operator {
                    code: i as u32,
                    precedence: i as u8,
                    spelling: text.as_bytes().to_vec(),
                })
                .collect(),
            )
            .unwrap(),
        )
        .unwrap(),
        &Signatures::new(),
        &[],
        Default::default(),
    )
    .unwrap();
    assert!(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &selected,
            &content,
            &requests(2),
            Intent::Engineering,
            Default::default()
        )
        .is_err()
    );
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let changed = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    assert_eq!(definition(&changed), definition(&catalogue));
    let changed_before = saved(Arc::clone(&changed), Some(17)).snapshot();
    assert!(
        pending_batch::consume(
            restored(changed, &changed_before),
            &prepared,
            &content,
            &requests(2),
            Intent::Engineering,
            Default::default()
        )
        .is_err()
    );
    let mut wrong_owner = before;
    wrong_owner.instances[0].owner = Owner::Quest { key: form(0x400) };
    assert!(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &wrong_owner),
            &prepared,
            &content,
            &requests(2),
            Intent::Engineering,
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn cooperative_slices_preserve_whole_state_and_never_repeat_complete_events() {
    let (_directory, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue);
    for bits in [0x8000000000000000, 0x7ff8123456789abc, u64::MAX] {
        let before = saved(Arc::clone(&catalogue), Some(bits)).snapshot();
        let input = before.encode(64 * 1024 * 1024).unwrap();
        let request = requests(3);
        let synchronous = pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &request,
            Intent::Engineering,
            Default::default(),
        )
        .unwrap();
        for slice in [1, 2, 3, usize::MAX] {
            let mut job = Job::new(
                restored(Arc::clone(&catalogue), &before),
                &prepared,
                &content,
                &request,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap();
            assert_eq!(job.progress().status, Status::Pending);
            assert_eq!(
                job.progress().work,
                WorkCounts {
                    source_frame_attempts: 0,
                    copy_adapter_attempts: 0,
                }
            );
            while job.progress().status == Status::Pending {
                let previous = job.progress();
                let progress = job.advance(slice).unwrap();
                let completed = (3 - previous.counts.events).min(slice);
                assert_eq!(progress.counts.events, previous.counts.events + completed);
                assert_eq!(progress.work.source_frame_attempts, progress.counts.events);
                assert_eq!(progress.work.copy_adapter_attempts, progress.counts.events);
                assert_eq!(
                    progress.counts.source_instructions,
                    progress.counts.events * 3
                );
                assert_eq!(progress.counts.statement_bytes, progress.counts.events * 12);
                assert_eq!(
                    progress.counts.trace_projection.source_bytes,
                    progress.counts.events * 26
                );
                assert_eq!(
                    progress.counts.trace_projection.rows,
                    progress.counts.events * 8
                );
                assert_eq!(
                    progress.counts.trace_projection.binding_uses,
                    progress.counts.events * 6
                );
                let counters = serde_json::to_value(progress).unwrap();
                assert_eq!(counters.as_object().unwrap().len(), 3);
                assert!(counters.get("snapshot").is_none() && counters.get("committed").is_none());
            }
            let ready = job.progress();
            assert_eq!(ready.status, Status::Ready);
            assert_eq!(job.advance(1).unwrap(), ready);
            assert_eq!(job.advance(usize::MAX).unwrap(), ready);
            let outcome = job.finish().unwrap();
            assert_eq!(
                serde_json::to_value(&outcome).unwrap(),
                serde_json::to_value(&synchronous).unwrap()
            );
            let result = complete(outcome);
            let expected = expected(before.clone(), 3, bits);
            assert_eq!(result.snapshot, expected);
            assert_eq!(
                restored(Arc::clone(&catalogue), &result.snapshot).snapshot(),
                expected
            );
            for (index, copy) in result.committed.iter().enumerate() {
                assert_eq!(copy.receipt.assignments, 1);
                assert_eq!(
                    copy.receipt.before_revision,
                    before.state_revision + index as u64
                );
                assert_eq!(
                    copy.receipt.after_revision,
                    before.state_revision + index as u64 + 1
                );
                assert_eq!(
                    copy.receipt.acknowledged.as_ref(),
                    Some(&before.pending_events[index])
                );
            }
            assert_eq!(before.encode(64 * 1024 * 1024).unwrap(), input);
        }
    }
}

#[test]
fn cooperative_abort_zero_slice_and_late_refusal_expose_no_partial_state() {
    let (_directory, catalogue, content) = fixture(true);
    let prepared = sources(&catalogue);
    let before = saved(Arc::clone(&catalogue), Some(17)).snapshot();
    let input = before.encode(64 * 1024 * 1024).unwrap();
    let request = requests(3);
    for advance_first in [false, true] {
        let mut job = Job::new(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &request,
            Intent::Engineering,
            Default::default(),
        )
        .unwrap();
        let unchanged = job.progress();
        assert!(job.advance(0).is_err());
        assert_eq!(job.progress(), unchanged);
        if advance_first {
            assert_eq!(job.advance(1).unwrap().counts.events, 1);
        }
        assert!(job.finish().is_err());
    }
    let mut dropped = Job::new(
        restored(Arc::clone(&catalogue), &before),
        &prepared,
        &content,
        &request,
        Intent::Engineering,
        Default::default(),
    )
    .unwrap();
    dropped.advance(1).unwrap();
    drop(dropped);
    let mut job = Job::new(
        restored(Arc::clone(&catalogue), &before),
        &prepared,
        &content,
        &request,
        Intent::Engineering,
        Default::default(),
    )
    .unwrap();
    assert_eq!(job.advance(1).unwrap().counts.events, 1);
    let refused = job.advance(1).unwrap();
    assert_eq!(refused.status, Status::Unsupported);
    assert_eq!(refused.counts.events, 1);
    assert_eq!(
        refused.work,
        WorkCounts {
            source_frame_attempts: 2,
            copy_adapter_attempts: 2
        }
    );
    assert_eq!(job.advance(usize::MAX).unwrap(), refused);
    let outcome = job.finish().unwrap();
    assert!(matches!(
        outcome,
        Outcome::Unsupported {
            event_index: Some(1),
            reason: Unsupported::ExpressionShape,
            ..
        }
    ));
    let json = serde_json::to_value(outcome).unwrap();
    assert!(
        json.get("snapshot").is_none()
            && json.get("committed").is_none()
            && json.get("result").is_none()
    );
    let mut faithful = Job::new(
        restored(Arc::clone(&catalogue), &before),
        &prepared,
        &content,
        &request,
        Intent::Faithful,
        pending_batch::Limits {
            maximum_source_instructions: 0,
            trace_projection: fallout_runtime::preparation::ObservationLimits {
                maximum_source_bytes: 0,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(faithful.progress().status, Status::Unsupported);
    assert_eq!(
        faithful.advance(1).unwrap().work,
        WorkCounts {
            source_frame_attempts: 0,
            copy_adapter_attempts: 0
        }
    );
    assert!(matches!(
        faithful.finish().unwrap(),
        Outcome::Unsupported {
            event_index: None,
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert_eq!(before.encode(64 * 1024 * 1024).unwrap(), input);
}

#[test]
fn cooperative_aggregate_caps_do_not_reset_between_advances_and_poison_failed_jobs() {
    let (_directory, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue);
    let before = saved(Arc::clone(&catalogue), Some(17)).snapshot();
    let request = requests(3);
    let sample = complete(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &request,
            Intent::Engineering,
            Default::default(),
        )
        .unwrap(),
    );
    let exact = pending_batch::Limits {
        maximum_events: 3,
        maximum_source_instructions: 9,
        maximum_statement_bytes: 36,
        trace_projection: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: 78,
            maximum_rows: 24,
            maximum_variable_bytes: sample.counts.trace_projection.variable_bytes,
            maximum_binding_uses: 18,
        },
    };
    for field in 0..6 {
        let mut limits = exact;
        match field {
            0 => limits.maximum_source_instructions -= 1,
            1 => limits.maximum_statement_bytes -= 1,
            2 => limits.trace_projection.maximum_source_bytes -= 1,
            3 => limits.trace_projection.maximum_rows -= 1,
            4 => limits.trace_projection.maximum_variable_bytes -= 1,
            _ => limits.trace_projection.maximum_binding_uses -= 1,
        }
        let mut job = Job::new(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &request,
            Intent::Engineering,
            limits,
        )
        .unwrap();
        assert_eq!(job.advance(1).unwrap().counts.events, 1);
        assert_eq!(job.advance(1).unwrap().counts.events, 2);
        assert!(job.advance(1).is_err(), "field{field}");
        let failed = job.progress();
        assert_eq!(failed.status, Status::Failed);
        assert_eq!(failed.counts.events, 2);
        assert_eq!(
            failed.work,
            WorkCounts {
                source_frame_attempts: 3,
                copy_adapter_attempts: 2
            }
        );
        assert!(job.advance(1).is_err());
        assert_eq!(job.progress(), failed);
        assert!(job.finish().is_err());
    }
    let mut job = Job::new(
        restored(Arc::clone(&catalogue), &before),
        &prepared,
        &content,
        &request,
        Intent::Engineering,
        exact,
    )
    .unwrap();
    for _ in 0..3 {
        job.advance(1).unwrap();
    }
    assert_eq!(
        complete(job.finish().unwrap()).snapshot,
        expected(before, 3, 17)
    );
}

#[test]
#[ignore = "built frozen CLI and authored metadata copy; strict saved engineering batch only"]
fn cli_saved_batch_helper() {
    use serde_json::{Value as Json, json};
    use sha2::{Digest, Sha256};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_SAVED_BATCH_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    write_source(&install.join("Data"), false);
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
    let (catalogue, content) = loaded(&install.join("Data"));
    let prepared = sources(&catalogue);
    let before = saved(Arc::clone(&catalogue), Some(0x7ff8123456789abc)).snapshot();
    let bytes = before.encode(64 * 1024 * 1024).unwrap();
    let initial = evidence.join("initial.snapshot.json");
    fs::write(&initial, &bytes).unwrap();
    let request = json!({"schema_version":1,"intent":"engineering","events":[{"sequence":1,"activation":1},{"sequence":2,"activation":1}],"maximum_source_instructions":192,"maximum_statement_bytes":65539,"maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let cases = std::cell::Cell::new(0_usize);
    let run_on = |name: &str,
                  source_install: &Path,
                  snapshot: &Path,
                  request: &Json,
                  result_override: Option<&Path>,
                  report_mode: &str,
                  extra: &[&str]| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let result = result_override
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory.join("result.snapshot.json"));
        let report = match report_mode {
            "input" => snapshot.to_path_buf(),
            "request" => request_path.clone(),
            "order" => order.clone(),
            "result" => result.clone(),
            "result-case" => directory.join("RESULT.SNAPSHOT.JSON"),
            "protected" => install.join("batch-report.json"),
            _ => directory.join("report.json"),
        };
        if report_mode == "existing" {
            fs::write(&report, b"Existing report must remain unchanged").unwrap();
        }
        if report_mode == "hardlink-input" {
            fs::hard_link(snapshot, &report).unwrap();
        }
        let mut command = Command::new(&cli);
        command
            .args(["event-operands", "--install"])
            .arg(source_install)
            .arg("--load-order")
            .arg(&order)
            .arg("--snapshot-copy-batch-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(snapshot)
            .arg("--snapshot-output")
            .arg(&result)
            .args(extra);
        if report_mode != "stdout" {
            command.arg("--output").arg(&report);
        }
        let output = command.output().unwrap();
        cases.set(cases.get() + 1);
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
        let value = if report_mode == "stdout" {
            serde_json::from_slice::<Json>(&output.stdout).ok()
        } else if report_mode == "new" {
            report
                .exists()
                .then(|| serde_json::from_slice::<Json>(&fs::read(&report).unwrap()).unwrap())
        } else {
            None
        };
        (output, value, result, report)
    };
    let run =
        |name: &str,
         snapshot: &Path,
         request: &Json,
         result: Option<&Path>,
         mode: &str,
         extra: &[&str]| run_on(name, &install, snapshot, request, result, mode, extra);
    let mut after_two = None;
    let mut after_three = None;
    for count in [2, 3] {
        let mut request = request.clone();
        request["events"] = serde_json::to_value(requests(count)).unwrap();
        let (output, report, result, _) = run(
            &format!("complete-{count}"),
            &initial,
            &request,
            None,
            "new",
            &[],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = report.unwrap();
        assert!(report.get("cooperative_execution").is_none());
        assert_eq!(report["snapshot_batch"]["status"], "engineering_committed");
        assert_eq!(
            report["snapshot_batch"]["committed"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
        assert_eq!(report["snapshot_batch"]["counts"]["events"], count);
        assert_eq!(
            report["snapshot_batch"]["counts"]["source_instructions"],
            count * 3
        );
        assert_eq!(
            report["result_snapshot"]["remaining_head"]["sequence"],
            count + 1
        );
        assert_eq!(report["faithful_execution_admitted"], false);
        assert_eq!(report["prepared_sources"]["counts"]["definitions"], 1);
        let actual = Snapshot::decode(&fs::read(&result).unwrap(), Default::default()).unwrap();
        let expected = expected(before.clone(), count, 0x7ff8123456789abc);
        assert_eq!(actual, expected);
        assert_eq!(
            restored(Arc::clone(&catalogue), &actual).snapshot(),
            expected
        );
        if count == 2 {
            after_two = Some(result);
        } else {
            after_three = Some(result);
        }
    }
    let mut continuation = request.clone();
    continuation["events"] = json!([{"sequence":3,"activation":1}]);
    let (output, _, result, _) = run(
        "cold-next-head",
        after_two.as_ref().unwrap(),
        &continuation,
        None,
        "new",
        &[],
    );
    assert!(output.status.success());
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        expected(before.clone(), 3, 0x7ff8123456789abc)
    );
    for slice in [1_u8, 2, 3, 64] {
        let mut sliced = request.clone();
        sliced["events"] = serde_json::to_value(requests(3)).unwrap();
        let slice_text = slice.to_string();
        let (output, report, result, _) = run(
            &format!("cooperative-slice-{slice}"),
            &initial,
            &sliced,
            None,
            "new",
            &["--snapshot-copy-batch-slice-events", &slice_text],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = report.unwrap();
        let progress = report["cooperative_execution"]["progress"]
            .as_array()
            .unwrap();
        assert_eq!(report["cooperative_execution"]["slice_events"], slice);
        assert_eq!(progress.len(), 1 + 3_usize.div_ceil(usize::from(slice)));
        for (index, row) in progress.iter().enumerate() {
            let completed = (index * usize::from(slice)).min(3);
            assert_eq!(row["counts"]["events"], completed);
            assert_eq!(row["work"]["source_frame_attempts"], completed);
            assert_eq!(row["work"]["copy_adapter_attempts"], completed);
            assert_eq!(row["counts"]["source_instructions"], completed * 3);
            assert_eq!(row.as_object().unwrap().len(), 3);
            assert_eq!(
                row["status"],
                if completed == 3 { "ready" } else { "pending" }
            );
        }
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        let expected = expected(before.clone(), 3, 0x7ff8123456789abc);
        assert_eq!(actual, expected);
        assert_eq!(
            restored(Arc::clone(&catalogue), &actual).snapshot(),
            expected
        );
        let default_report: Json =
            serde_json::from_slice(&fs::read(evidence.join("complete-3/report.json")).unwrap())
                .unwrap();
        assert_eq!(report["snapshot_batch"], default_report["snapshot_batch"]);
    }
    let (output, _, result, _) = run(
        "cooperative-cold-next-head",
        after_two.as_ref().unwrap(),
        &continuation,
        None,
        "new",
        &["--snapshot-copy-batch-slice-events", "1"],
    );
    assert!(output.status.success());
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        expected(before.clone(), 3, 0x7ff8123456789abc)
    );
    for value in ["0", "65", "-1", "text"] {
        let (output, report, result, _) = run(
            &format!("cooperative-invalid-{value}"),
            &initial,
            &request,
            None,
            "new",
            &["--snapshot-copy-batch-slice-events", value],
        );
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    let (output, report, result, _) = run(
        "cold-old-sequence",
        after_three.as_ref().unwrap(),
        &request,
        None,
        "new",
        &[],
    );
    assert!(!output.status.success() && report.is_none() && !result.exists());
    let sample = complete(
        pending_batch::consume(
            restored(Arc::clone(&catalogue), &before),
            &prepared,
            &content,
            &requests(2),
            Intent::Engineering,
            Default::default(),
        )
        .unwrap(),
    );
    let c = &sample.counts;
    let mut exact = request.clone();
    exact["maximum_source_instructions"] = json!(c.source_instructions);
    exact["maximum_statement_bytes"] = json!(c.statement_bytes);
    exact["maximum_trace_source_bytes"] = json!(c.trace_projection.source_bytes);
    exact["maximum_trace_rows"] = json!(c.trace_projection.rows);
    exact["maximum_trace_variable_bytes"] = json!(c.trace_projection.variable_bytes);
    exact["maximum_trace_binding_uses"] = json!(c.trace_projection.binding_uses);
    exact["maximum_result_snapshot_bytes"] =
        json!(sample.snapshot.encode(64 * 1024 * 1024).unwrap().len());
    let (output, _, _, _) = run(
        "exact-all-source-snapshot-limits",
        &initial,
        &exact,
        None,
        "new",
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for field in [
        "maximum_source_instructions",
        "maximum_statement_bytes",
        "maximum_trace_source_bytes",
        "maximum_trace_rows",
        "maximum_trace_variable_bytes",
        "maximum_trace_binding_uses",
        "maximum_result_snapshot_bytes",
    ] {
        let mut low = exact.clone();
        low[field] = json!(low[field].as_u64().unwrap() - 1);
        let (output, report, result, _) = run(
            &format!("one-under-{field}"),
            &initial,
            &low,
            None,
            "new",
            &[],
        );
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{field}"
        );
        let (output, report, result, _) = run(
            &format!("cooperative-one-under-{field}"),
            &initial,
            &low,
            None,
            "new",
            &["--snapshot-copy-batch-slice-events", "1"],
        );
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{field}"
        );
    }
    let (output, _, _, report_path) = run("report-bound-a", &initial, &request, None, "new", &[]);
    assert!(output.status.success());
    let report_bytes = fs::read(report_path).unwrap().len();
    let mut cap = request.clone();
    cap["maximum_report_bytes"] = json!(report_bytes);
    let (output, _, _, _) = run("report-bound-b", &initial, &cap, None, "new", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    cap["maximum_report_bytes"] = json!(report_bytes - 1);
    let (output, report, result, _) = run("report-bound-c", &initial, &cap, None, "new", &[]);
    assert!(!output.status.success() && report.is_none() && !result.exists());
    let mut cooperative_low_report = request.clone();
    cooperative_low_report["maximum_report_bytes"] = json!(1);
    let (output, report, result, _) = run(
        "cooperative-report-cap",
        &initial,
        &cooperative_low_report,
        None,
        "new",
        &["--snapshot-copy-batch-slice-events", "1"],
    );
    assert!(!output.status.success() && report.is_none() && !result.exists());
    let mut wrong_later_owner = request.clone();
    wrong_later_owner["events"][1]["activation"] = json!(2);
    let (output, report, result, _) = run(
        "cooperative-late-owner",
        &initial,
        &wrong_later_owner,
        None,
        "new",
        &["--snapshot-copy-batch-slice-events", "1"],
    );
    assert!(!output.status.success() && report.is_none() && !result.exists());
    for (name, intent) in [
        ("intent-object-null", json!({"engineering":null})),
        ("intent-object-map", json!({"engineering":{}})),
    ] {
        let mut invalid = request.clone();
        invalid["intent"] = intent;
        let (output, report, result, _) = run(name, &initial, &invalid, None, "new", &[]);
        assert!(!output.status.success() && report.is_none() && !result.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains("expected a string"));
    }
    for (name, variant) in [
        ("wrong-first-head", 0),
        ("skipped-second", 1),
        ("duplicate-sequence", 2),
        ("wrong-later-activation", 3),
        ("empty-prefix", 4),
        ("exceeds-existing-journal", 5),
        ("zero-sequence", 6),
        ("zero-activation", 7),
        ("unknown-event-field", 8),
        ("missing-intent", 9),
        ("faithful-intent", 10),
        ("bad-schema", 11),
        ("excessive-events", 12),
        ("excessive-instruction-ceiling", 13),
        ("unknown-request-field", 14),
    ] {
        let mut bad = request.clone();
        match variant {
            0 => bad["events"][0]["sequence"] = json!(2),
            1 => bad["events"][1]["sequence"] = json!(3),
            2 => bad["events"][1]["sequence"] = json!(1),
            3 => bad["events"][1]["activation"] = json!(2),
            4 => bad["events"] = json!([]),
            5 => bad["events"] = serde_json::to_value(requests(5)).unwrap(),
            6 => bad["events"][0]["sequence"] = json!(0),
            7 => bad["events"][0]["activation"] = json!(0),
            8 => bad["events"][0]["unknown"] = json!(true),
            9 => {
                bad.as_object_mut().unwrap().remove("intent");
            }
            10 => bad["intent"] = json!("faithful"),
            11 => bad["schema_version"] = json!(2),
            12 => bad["events"] = serde_json::to_value(requests(65)).unwrap(),
            13 => bad["maximum_source_instructions"] = json!(193),
            _ => bad["unknown"] = json!(true),
        };
        let (output, report, result, _) = run(name, &initial, &bad, None, "new", &[]);
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{name}"
        );
    }
    for (name, variant) in [
        ("legacy-schema", 0),
        ("stale-cohort", 1),
        ("missing-instance", 2),
        ("wrong-block-site", 3),
        ("unset-source", 4),
        ("nonfragment-owner", 5),
    ] {
        let directory = evidence.join(format!("{name}-input"));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("snapshot.json");
        let mut json = serde_json::to_value(&before).unwrap();
        match variant {
            0 => json["schema_version"] = json!(3),
            1 => json["catalogue_sha256"] = json!("0".repeat(64)),
            2 => {
                json["instances"].as_array_mut().unwrap().remove(0);
            }
            3 => json["pending_events"][0]["trigger"]["begin_byte_offset"] = json!(25),
            4 => json["instances"][0]["locals"][0]["value"] = json!({"kind":"uninitialized"}),
            _ => json["instances"][0]["owner"] = json!({"kind":"quest","key":form(0x400)}),
        };
        fs::write(&path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
        let input_bytes = fs::read(&path).unwrap();
        let (output, report, result, _) = run(name, &path, &request, None, "new", &[]);
        assert!(!output.status.success() && !result.exists());
        if variant == 4 {
            let report = report.unwrap();
            assert_eq!(report["snapshot_batch"]["status"], "unsupported");
            assert_eq!(report["result_snapshot"], Json::Null);
        } else {
            assert!(report.is_none());
        }
        assert_eq!(fs::read(path).unwrap(), input_bytes);
    }
    let mixed_install = evidence.join("unsupported-source-copy");
    fs::create_dir(&mixed_install).unwrap();
    fs::create_dir(mixed_install.join("Data")).unwrap();
    write_source(&mixed_install.join("Data"), true);
    fs::copy(
        install.join("FalloutNV.exe"),
        mixed_install.join("FalloutNV.exe"),
    )
    .unwrap();
    let (mixed_catalogue, _) = loaded(&mixed_install.join("Data"));
    let mixed_snapshot = saved(mixed_catalogue, Some(0x7ff8123456789abc)).snapshot();
    let mixed_input = evidence.join("unsupported.snapshot.json");
    let mixed_bytes = mixed_snapshot.encode(64 * 1024 * 1024).unwrap();
    fs::write(&mixed_input, &mixed_bytes).unwrap();
    let (output, report, result, _) = run_on(
        "unsupported-second",
        &mixed_install,
        &mixed_input,
        &request,
        None,
        "new",
        &[],
    );
    assert!(!output.status.success() && !result.exists());
    let report = report.unwrap();
    assert_eq!(report["snapshot_batch"]["event_index"], 1);
    assert!(report["snapshot_batch"].get("committed").is_none());
    assert_eq!(report["before_revision"], report["after_revision"]);
    assert_eq!(report["private_result_discarded"], true);
    assert_eq!(fs::read(&mixed_input).unwrap(), mixed_bytes);
    let (output, report, result, _) = run_on(
        "cooperative-unsupported-second",
        &mixed_install,
        &mixed_input,
        &request,
        None,
        "new",
        &["--snapshot-copy-batch-slice-events", "1"],
    );
    assert!(!output.status.success() && !result.exists());
    let report = report.unwrap();
    assert_eq!(report["snapshot_batch"]["event_index"], 1);
    assert!(report["snapshot_batch"].get("committed").is_none());
    assert_eq!(report["before_revision"], report["after_revision"]);
    assert_eq!(report["result_snapshot"], Json::Null);
    assert_eq!(report["private_result_discarded"], true);
    let stopped = report["cooperative_execution"]["progress"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(stopped["status"], "unsupported");
    assert_eq!(stopped["counts"]["events"], 1);
    assert_eq!(stopped["work"]["source_frame_attempts"], 2);
    assert_eq!(stopped["work"]["copy_adapter_attempts"], 2);
    assert_eq!(fs::read(&mixed_input).unwrap(), mixed_bytes);
    for mode in [
        "input",
        "request",
        "order",
        "result",
        "result-case",
        "existing",
        "protected",
        "hardlink-input",
    ] {
        let (output, _, result, report) = run(
            &format!("report-{mode}"),
            &initial,
            &request,
            None,
            mode,
            &[],
        );
        assert!(!output.status.success() && !result.exists());
        if mode == "existing" {
            assert_eq!(
                fs::read(&report).unwrap(),
                b"Existing report must remain unchanged"
            );
        }
        if mode == "protected" {
            assert!(!report.exists());
        }
    }
    let existing = evidence.join("existing.snapshot.json");
    fs::write(&existing, b"Existing result must remain unchanged").unwrap();
    let (output, report, _, _) = run(
        "existing-result",
        &initial,
        &request,
        Some(&existing),
        "new",
        &[],
    );
    assert!(!output.status.success() && report.is_none());
    assert_eq!(
        fs::read(existing).unwrap(),
        b"Existing result must remain unchanged"
    );
    let protected = install.join("result.snapshot.json");
    let (output, report, _, _) = run(
        "protected-result",
        &initial,
        &request,
        Some(&protected),
        "new",
        &[],
    );
    assert!(!output.status.success() && report.is_none() && !protected.exists());
    for (name, extra) in [
        (
            "single-mode-conflict",
            vec!["--snapshot-copy-request", "missing.json"],
        ),
        (
            "native-mode-conflict",
            vec!["--snapshot-native-request", "missing.json"],
        ),
        (
            "quest-mode-conflict",
            vec![
                "--quest-boot-request",
                "missing.json",
                "--quest-boot-output",
                "missing-output.json",
            ],
        ),
        (
            "seed-mode-conflict",
            vec!["--engineering-local-copy", "missing.json"],
        ),
        ("player-mode-conflict", vec!["--player-id", "1"]),
        ("prepared-mode-conflict", vec!["--prepared-sources"]),
        ("native-capability-conflict", vec!["--native-capabilities"]),
    ] {
        let (output, report, result, _) = run(name, &initial, &request, None, "new", &extra);
        assert!(!output.status.success() && report.is_none() && !result.exists());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("cannot be used with"),
            "{name}"
        );
    }
    let (output, report, result, _) = run("stdout-report", &initial, &request, None, "stdout", &[]);
    assert!(output.status.success() && result.exists());
    assert_eq!(
        report.unwrap()["snapshot_batch"]["status"],
        "engineering_committed"
    );
    let mut oversized = request.clone();
    oversized["padding"] = json!("x".repeat(16 * 1024));
    let (output, report, result, _) =
        run("oversized-request", &initial, &oversized, None, "new", &[]);
    assert!(!output.status.success() && report.is_none() && !result.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("request byte budget"));
    assert_eq!(fs::read(&initial).unwrap(), bytes);
    assert_eq!(fs::read(&order).unwrap(), b"[\"FalloutNV.esm\"]");
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), executable);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source
    );
    fs::write(evidence.join("acceptance.json"),serde_json::to_vec_pretty(&json!({"schema_version":1,"cases":cases.get(),"scope":"explicit strict saved engineering prefix and cooperative complete-event slices","original_launches":0,"faithful_execution_admitted":false,"gameplay_accepted":false})).unwrap()).unwrap();
}
