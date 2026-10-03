mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Handle},
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
    event_operands::{self, Access, Outcome, Resolution},
    events::{Context, Trigger},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceValue, Value},
};
use std::{fs, sync::Arc};

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
fn instruction(out: &mut Vec<u8>, opcode: u16, data: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((data.len() as u16).to_le_bytes());
    out.extend(data);
}
fn assignment(target: &[u8], expression: &[u8]) -> Vec<u8> {
    let mut data = target.to_vec();
    data.extend((expression.len() as u16).to_le_bytes());
    data.extend(expression);
    let mut out = Vec::new();
    instruction(&mut out, 0x15, &data);
    out
}
fn local(index: u16) -> Vec<u8> {
    [vec![b's'], index.to_le_bytes().to_vec()].concat()
}
fn reference(index: u16) -> Vec<u8> {
    [vec![b'Z'], index.to_le_bytes().to_vec()].concat()
}
fn foreign(context: u16, index: u16) -> Vec<u8> {
    [vec![b'r'], context.to_le_bytes().to_vec(), local(index)].concat()
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
fn script(compiled: &[u8], variables: &[(u32, u8)], references: &[(&[u8; 4], u32)]) -> Vec<u8> {
    let original = unit(variables, references);
    // The common fixture's SCHR/SCDA occupy 26/20 bytes. Retain the actual
    // declaration/reference fields, replacing only the compiled body and size.
    let mut out = original[..26].to_vec();
    out[14..18].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    out.extend(field(b"SCDA", compiled));
    out.extend(&original[46..]);
    out
}
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    let source = script(
        body,
        &[(0, 0), (2, 1), (42, 0), (90, 0), (99, 7)],
        &[
            (b"SCRO", 0x100),
            (b"SCRO", 0x101),
            (b"SCRO", 0),
            (b"SCRO", 0x14),
            (b"SCRV", 90),
            (b"SCRO", 0x110),
            (b"SCRV", 0),
        ],
    );
    let data = [
        header(&[]),
        record(b"SCPT", 0x300, 0, &source),
        record(b"SCPT", 0x301, 0, &unit(&[(42, 0)], &[])),
        record(b"SCPT", 0x302, 0, &unit(&[(42, 1)], &[])),
        record(b"QUST", 0x100, 0, &field(b"SCRI", &0x301_u32.to_le_bytes())),
        record(b"REFR", 0x101, 0, &field(b"NAME", &0x102_u32.to_le_bytes())),
        record(b"ACTI", 0x102, 0, &field(b"SCRI", &0x301_u32.to_le_bytes())),
        record(b"GLOB", 0x110, 0, &[]),
    ]
    .concat();
    fs::write(directory.path().join("FalloutNV.esm"), data).unwrap();
    let (catalogue, content) = load_content(directory.path());
    (directory, catalogue, content)
}
fn load_content(path: &std::path::Path) -> (Arc<Catalogue>, Content) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(
        &mut store,
        fallout_data::loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (Arc::new(catalogue), content)
}
fn definition(catalogue: &Catalogue, id: u32) -> Handle {
    catalogue
        .record_scripts(&form(id))
        .next()
        .unwrap()
        .handle()
        .clone()
}
fn seed(
    catalogue: Arc<Catalogue>,
    offset: u32,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let handle = definition(&catalogue, 0x300);
    let mut world = World::with_campaign(
        catalogue,
        fallout_runtime::Limits::default(),
        CampaignId::from_bytes([0x44; 16]).unwrap(),
    )
    .unwrap();
    let handle = world
        .create_instance(
            &handle,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let sequence = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: offset,
            },
            Context::default(),
        )
        .unwrap();
    (world, handle, sequence)
}
fn probe(
    world: &World<'_>,
    sequence: u64,
    content: &Content,
    player: Option<fallout_runtime::identity::ReferenceId>,
    limits: event_operands::Limits,
) -> Result<event_operands::Probe, event_operands::Error> {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    world.probe_event_operands(
        sequence,
        &model,
        &Signatures::new(),
        content,
        player,
        limits,
    )
}
fn unresolved(outcome: &Outcome, code: &str) -> bool {
    matches!(outcome,Outcome::Unresolved{code:actual,..} if actual==code)
}

fn native_probe(
    world: &World<'_>,
    sequence: u64,
    content: &Content,
) -> Result<event_operands::Probe, event_operands::Error> {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = [(0x1001, 4), (0x1006, 1)]
        .into_iter()
        .map(|(opcode, type_id)| {
            (
                opcode,
                CommandSignature {
                    convention: Convention::Default,
                    parameters: vec![Parameter {
                        type_id,
                        optional_word: 0,
                    }],
                },
            )
        })
        .collect();
    world.probe_event_operands(
        sequence,
        &model,
        &signatures,
        content,
        None,
        Default::default(),
    )
}

#[test]
fn native_operands_distinguish_direct_form_storage_from_reference_table_entries() {
    let mut calls = Vec::new();
    instruction(&mut calls, 0x1001, &[1, 0, b'f', 90, 0]);
    // The reference prefix is a separate caller header, not a local index.
    calls.extend([0x1c, 0, 5, 0]);
    instruction(&mut calls, 0x1001, &[1, 0, b'r', 5, 0]);
    instruction(&mut calls, 0x1001, &[1, 0, b'f', 42, 0]);
    instruction(&mut calls, 0x1006, &[1, 0, b'f', 42, 0]);
    instruction(&mut calls, 0x1006, &[1, 0, b'r', 1, 0, b's', 42, 0]);
    let (_directory, catalogue, content) = fixture(&event(&calls));
    let (mut world, handle, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    let report = native_probe(&world, sequence, &content).unwrap();
    assert_eq!(
        report
            .operands
            .iter()
            .map(|operand| operand.binding.role)
            .collect::<Vec<_>>(),
        [11, 1, 10, 11, 8, 12, 8]
    );
    for operand in &report.operands[..5] {
        assert!(unresolved(&operand.outcome, "uninitialized_local"));
    }
    assert!(unresolved(
        &report.operands[6].outcome,
        "missing_live_event_list"
    ));
    assert_eq!(world.snapshot(), before);
    world
        .assign(
            handle,
            &[
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Null,
                    },
                ),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9abc,
                    },
                ),
            ],
        )
        .unwrap();
    let before = world.snapshot();
    let report = native_probe(&world, sequence, &content).unwrap();
    assert!(matches!(
        report.operands[0].outcome,
        Outcome::Resolved {
            resolution: Resolution::Local {
                value: Some(Value::Reference {
                    value: ReferenceValue::Null
                }),
                ..
            },
            ..
        }
    ));
    for index in [1, 2] {
        assert!(matches!(
            report.operands[index].outcome,
            Outcome::Resolved {
                resolution: Resolution::Reference {
                    value: ReferenceValue::Null
                },
                ..
            }
        ));
    }
    assert!(unresolved(
        &report.operands[3].outcome,
        "incompatible_local"
    ));
    assert!(matches!(
        report.operands[4].outcome,
        Outcome::Resolved {
            resolution: Resolution::Local {
                value: Some(Value::Number {
                    bits: 0x7ff8_1234_5678_9abc
                }),
                ..
            },
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn a_reference_variable_with_an_unverified_zero_index_reports_unsupported_storage() {
    let (_directory, catalogue, content) = fixture(&event(&assignment(&local(2), &reference(7))));
    let (world, _, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(unresolved(&report.operands[1].outcome, "unsupported_local"));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn a_foreign_scrv_context_checks_storage_before_reading_its_unset_value() {
    let body = event(
        &[
            assignment(&foreign(7, 42), b"1"),
            assignment(&local(42), &foreign(7, 42)),
        ]
        .concat(),
    );
    let (_directory, catalogue, content) = fixture(&body);
    let (world, handle, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    assert!(matches!(
        world.resolve_script_reference(handle, 7, None),
        Err(fallout_runtime::Error::UnsupportedLocal(0))
    ));
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    let context_uses: Vec<_> = report
        .operands
        .iter()
        .filter(|operand| {
            operand.binding.context_reference == Some(7)
                || (operand.binding.index == 7 && operand.binding.status == 3)
        })
        .collect();
    assert_eq!(context_uses.len(), 4);
    assert!(
        context_uses
            .iter()
            .all(|operand| unresolved(&operand.outcome, "unsupported_local"))
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn unknown_native_signatures_fail_preparation_without_consuming_the_event() {
    let mut call = Vec::new();
    instruction(&mut call, 0x1001, &[1, 0, b'f', 90, 0]);
    let (_directory, catalogue, content) = fixture(&event(&call));
    let (world, _, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    assert!(matches!(
        probe(&world, sequence, &content, None, Default::default()),
        Err(event_operands::Error::Preparation(_))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn destinations_do_not_read_uninitialized_values_and_reads_preserve_numeric_bits() {
    let body = event(&assignment(&local(2), &local(2)));
    let (_directory, catalogue, content) = fixture(&body);
    let (mut world, handle, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert_eq!(report.operands.len(), 2);
    assert!(matches!(
        report.operands[0].outcome,
        Outcome::Resolved {
            access: Access::Destination,
            resolution: Resolution::Local { value: None, .. }
        }
    ));
    assert!(unresolved(
        &report.operands[1].outcome,
        "uninitialized_local"
    ));
    assert_eq!(world.snapshot(), before);
    let bits = 0x7ff8_1234_5678_9abc;
    world
        .assign(handle, &[(2, Value::Number { bits })])
        .unwrap();
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(
        matches!(report.operands[1].outcome,Outcome::Resolved{resolution:Resolution::Local{value:Some(Value::Number{bits:actual}),..},..} if actual==bits)
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn null_dynamic_and_explicit_player_references_remain_distinct() {
    let middle = [
        assignment(&local(42), &reference(3)),
        assignment(&local(42), &reference(4)),
        assignment(&local(42), &reference(5)),
    ]
    .concat();
    let (_directory, catalogue, content) = fixture(&event(&middle));
    let (mut world, handle, sequence) = seed(catalogue, 0);
    let id = world.register_reference(None).unwrap();
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Live { id },
                },
            )],
        )
        .unwrap();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(matches!(
        report.operands[1].outcome,
        Outcome::Resolved {
            resolution: Resolution::Reference {
                value: ReferenceValue::Null
            },
            ..
        }
    ));
    assert!(unresolved(
        &report.operands[3].outcome,
        "unresolved_context_reference"
    ));
    assert!(
        matches!(report.operands[5].outcome,Outcome::Resolved{resolution:Resolution::Reference{value:ReferenceValue::Live{id:actual}},..} if actual==id)
    );
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, Some(id), Default::default()).unwrap();
    assert!(
        matches!(report.operands[3].outcome,Outcome::Resolved{resolution:Resolution::Reference{value:ReferenceValue::Live{id:actual}},..} if actual==id)
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn foreign_destinations_and_reads_use_the_current_live_definition_not_an_attachment() {
    let middle = [
        assignment(&foreign(1, 42), b"1"),
        assignment(&local(42), &foreign(1, 42)),
    ]
    .concat();
    let (_directory, catalogue, content) = fixture(&event(&middle));
    let (mut world, source, sequence) = seed(Arc::clone(&catalogue), 0);
    world
        .assign(source, &[(42, Value::Number { bits: 999 })])
        .unwrap();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(
        report
            .operands
            .iter()
            .filter(|row| row.binding.context_reference.is_some())
            .all(|row| unresolved(&row.outcome, "missing_live_event_list"))
    );
    let target = world
        .create_instance(
            &definition(&catalogue, 0x302),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    let destination = report
        .operands
        .iter()
        .find(|row| row.binding.role == 2 && row.binding.context_reference.is_some())
        .unwrap();
    assert!(
        matches!(&destination.outcome,Outcome::Resolved{access:Access::Destination,resolution:Resolution::Foreign{target,value:None}} if target.declaration.kind==fallout_runtime::schema::Kind::Integer && target.target_definition==definition(&catalogue,0x302))
    );
    let read = report
        .operands
        .iter()
        .find(|row| row.binding.role == 4 && row.binding.context_reference.is_some())
        .unwrap();
    assert!(unresolved(&read.outcome, "uninitialized_local"));
    world
        .assign(target, &[(42, Value::Number { bits: 123 })])
        .unwrap();
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    let read = report
        .operands
        .iter()
        .find(|row| row.binding.role == 4 && row.binding.context_reference.is_some())
        .unwrap();
    assert!(matches!(
        read.outcome,
        Outcome::Resolved {
            resolution: Resolution::Foreign {
                value: Some(Value::Number { bits: 123 }),
                ..
            },
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn authored_placed_context_requires_registration_and_an_explicit_live_list() {
    let (_directory, catalogue, content) =
        fixture(&event(&assignment(&local(42), &foreign(2, 42))));
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue), 0);
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(unresolved(
        &report.operands.last().unwrap().outcome,
        "reference_not_registered"
    ));
    let reference = world.register_reference(Some(form(0x101))).unwrap();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(unresolved(
        &report.operands.last().unwrap().outcome,
        "missing_live_event_list"
    ));
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Placed { reference },
            Context::default(),
        )
        .unwrap();
    world
        .assign(target, &[(42, Value::Number { bits: 456 })])
        .unwrap();
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(matches!(
        report.operands.last().unwrap().outcome,
        Outcome::Resolved {
            resolution: Resolution::Foreign {
                value: Some(Value::Number { bits: 456 }),
                ..
            },
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn unknown_globals_and_unsupported_local_schemas_do_not_become_values() {
    let middle = [
        assignment(&local(99), b"1"),
        assignment(&local(42), &[b'G', 6, 0]),
        assignment(&[b'G', 6, 0], b"1"),
    ]
    .concat();
    let (_directory, catalogue, content) = fixture(&event(&middle));
    let (world, _, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    let report = probe(&world, sequence, &content, None, Default::default()).unwrap();
    assert!(unresolved(&report.operands[0].outcome, "unsupported_local"));
    assert_eq!(
        report
            .operands
            .iter()
            .filter(|row| unresolved(&row.outcome, "unverified_global_value"))
            .count(),
        2
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn exact_budgets_and_whole_cohort_validation_fail_without_state_changes() {
    let (directory, catalogue, content) = fixture(&event(&assignment(&local(2), &local(2))));
    let (world, _, sequence) = seed(catalogue, 0);
    let before = world.snapshot();
    let limits = event_operands::Limits {
        maximum_uses: 2,
        ..Default::default()
    };
    assert_eq!(
        probe(&world, sequence, &content, None, limits)
            .unwrap()
            .operands
            .len(),
        2
    );
    assert!(matches!(
        probe(
            &world,
            sequence,
            &content,
            None,
            event_operands::Limits {
                maximum_uses: 1,
                ..limits
            }
        ),
        Err(event_operands::Error::Capacity)
    ));
    let mut changed = fs::read(directory.path().join("FalloutNV.esm")).unwrap();
    changed.extend(record(b"GLOB", 0x111, 0, &[]));
    fs::write(directory.path().join("FalloutNV.esm"), changed).unwrap();
    let (_, other) = load_content(directory.path());
    assert!(matches!(
        probe(&world, sequence, &other, None, limits),
        Err(event_operands::Error::Content(
            fallout_runtime::foreign::Failure::ContentChanged
        ))
    ));
    assert!(matches!(
        probe(&world, sequence + 1, &content, None, limits),
        Err(event_operands::Error::Preparation(
            fallout_runtime::preparation::Error::MissingPending(_)
        ))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn owned_restoration_reproduces_observations_without_accepting_old_handles() {
    let (_directory, catalogue, content) = fixture(&event(&assignment(&local(2), &local(2))));
    let (mut world, handle, sequence) = seed(Arc::clone(&catalogue), 0);
    world
        .assign(handle, &[(2, Value::Number { bits: u64::MAX })])
        .unwrap();
    let before = world.snapshot();
    let expected = probe(&world, sequence, &content, None, Default::default()).unwrap();
    let restored = World::restore(
        catalogue,
        before.clone(),
        fallout_runtime::Limits::default(),
    )
    .unwrap();
    let actual = probe(&restored, sequence, &content, None, Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(expected).unwrap(),
        serde_json::to_value(actual).unwrap()
    );
    assert!(matches!(
        restored.instance(handle),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(restored.snapshot(), before);
}

#[test]
fn probing_a_second_event_does_not_include_other_blocks_or_consume_the_queue() {
    let first = event(&assignment(&local(2), &local(2)));
    let second = event(&assignment(&local(42), &reference(3)));
    let offset = first.len() as u32;
    let (_directory, catalogue, content) = fixture(&[first, second].concat());
    let (mut world, handle, first_sequence) = seed(catalogue, 0);
    let second_sequence = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: offset,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let report = probe(
        &world,
        second_sequence,
        &content,
        None,
        event_operands::Limits {
            maximum_uses: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.pending.sequence, second_sequence);
    assert_eq!(report.begin_scda_offset, offset as usize);
    assert_eq!(report.operands.len(), 2);
    assert!(
        report
            .operands
            .iter()
            .all(|row| row.binding.scda_offset >= offset as usize)
    );
    assert_eq!(
        world.pending_events().next().unwrap().sequence,
        first_sequence
    );
    assert_eq!(world.snapshot(), before);
}
