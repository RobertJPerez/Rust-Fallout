mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        definition_plan,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    events::{Context, Trigger},
    identity::{Owner, ReferenceValue, Value},
    preparation::{self, PreparedEvent},
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
fn instruction(body: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    body.extend(opcode.to_le_bytes());
    body.extend((payload.len() as u16).to_le_bytes());
    body.extend(payload);
}
fn block(body: &mut Vec<u8>, id: u16) {
    let operands = [id.to_le_bytes().as_slice(), &4_u32.to_le_bytes()].concat();
    instruction(body, 0x10, &operands);
    instruction(body, 0x11, &[]);
}
fn fixture(body: &[u8], bad_table_count: bool) -> (tempfile::TempDir, Arc<Catalogue>) {
    let directory = tempfile::tempdir().unwrap();
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&u32::from(bad_table_count).to_le_bytes());
    schr[8..12].copy_from_slice(&(body.len() as u32).to_le_bytes());
    schr[12..16].copy_from_slice(&1_u32.to_le_bytes());
    let mut declaration = [0; 24];
    declaration[..4].copy_from_slice(&2_u32.to_le_bytes());
    declaration[16] = 1;
    let payload = [
        field(b"SCHR", &schr),
        field(b"SCDA", body),
        field(b"SLSD", &declaration),
        field(b"SCVR", b"counter\0"),
        field(b"SCTX", b"Unrelated diagnostic text"),
    ]
    .concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), record(b"SCPT", 0x300, 0, &payload)].concat(),
    )
    .unwrap();
    let catalogue = Arc::new(load(directory.path(), &["FalloutNV.esm"]));
    (directory, catalogue)
}
fn seeded(catalogue: Arc<Catalogue>) -> (World<'static>, fallout_runtime::state::InstanceHandle) {
    let source = definition(&catalogue);
    let mut world = World::new(catalogue, WorldLimits::default()).unwrap();
    let handle = world
        .create_instance(
            &source,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    (world, handle)
}
fn prepare<'a>(
    world: &'a World<'_>,
    sequence: u64,
    limits: preparation::Limits,
) -> Result<PreparedEvent<'a>, preparation::Error> {
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    world.prepare_event(sequence, &model, &Signatures::new(), limits)
}
fn trigger(offset: u32, id: u16) -> Trigger {
    Trigger::Block {
        event_id: id,
        begin_byte_offset: offset,
    }
}

#[test]
fn exact_pending_context_instance_and_source_are_borrowed_without_state_effects() {
    let mut body = Vec::new();
    block(&mut body, 3);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(catalogue);
    let reference = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Null],
    };
    let sequence = world
        .enqueue(handle, trigger(0, 3), context.clone())
        .unwrap();
    let before = world.snapshot();
    {
        let prepared = prepare(&world, sequence, preparation::Limits::default()).unwrap();
        assert_eq!(prepared.pending().sequence, sequence);
        assert_eq!(prepared.pending().context, context);
        assert_eq!(
            prepared.instance().id(),
            world.instance(handle).unwrap().id()
        );
        assert_eq!(
            prepared.source().handle(),
            world.instance(handle).unwrap().definition()
        );
        assert_eq!(prepared.source().control().bytes(), body);
        assert_eq!(prepared.selected().event_id, 3);
        assert_eq!(prepared.instructions().len(), 2);
        assert!(matches!(
            prepared.instance().local(2),
            Err(fallout_runtime::Error::UninitializedLocal(2))
        ));
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
fn repeated_event_ids_select_the_exact_authored_begin_header() {
    let mut body = Vec::new();
    block(&mut body, 3);
    block(&mut body, 3);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(catalogue);
    let first = world
        .enqueue(handle, trigger(0, 3), Context::default())
        .unwrap();
    let second = world
        .enqueue(handle, trigger(14, 3), Context::default())
        .unwrap();
    let a = prepare(&world, first, preparation::Limits::default()).unwrap();
    let b = prepare(&world, second, preparation::Limits::default()).unwrap();
    assert_eq!(a.selected().begin_instruction, 0);
    assert_eq!(b.selected().begin_instruction, 2);
    assert_eq!(a.instructions()[0].bytes.start, 0);
    assert_eq!(b.instructions()[0].bytes.start, 14);
    assert_eq!(a.pending().sequence, first);
    assert_eq!(b.pending().sequence, second);
}

#[test]
fn missing_and_acknowledged_sequences_remain_errors() {
    let mut body = Vec::new();
    block(&mut body, 0);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(catalogue);
    assert!(matches!(
        prepare(&world, 0, preparation::Limits::default()),
        Err(preparation::Error::MissingPending(0))
    ));
    let sequence = world
        .enqueue(handle, trigger(0, 0), Context::default())
        .unwrap();
    world.acknowledge(sequence).unwrap();
    let before = world.snapshot();
    assert!(
        matches!(prepare(&world, sequence, preparation::Limits::default()), Err(preparation::Error::MissingPending(n)) if n == sequence)
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn object_event_mapping_is_explicitly_unverified_and_does_not_consume_the_entry() {
    let mut body = Vec::new();
    block(&mut body, 0);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(catalogue);
    let sequence = world
        .enqueue(
            handle,
            Trigger::ObjectEvent { mask: 0x80000001 },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    assert!(matches!(
        prepare(&world, sequence, preparation::Limits::default()),
        Err(preparation::Error::ObjectEventMapping(0x80000001))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn source_table_and_raw_distance_findings_fail_without_partial_preparation() {
    for bad_table_count in [false, true] {
        let mut body = Vec::new();
        block(&mut body, 0);
        if !bad_table_count {
            body[6..10].copy_from_slice(&1_u32.to_le_bytes());
        }
        let (_directory, catalogue) = fixture(&body, bad_table_count);
        let (mut world, handle) = seeded(catalogue);
        let sequence = world
            .enqueue(handle, trigger(0, 0), Context::default())
            .unwrap();
        let before = world.snapshot();
        let error = prepare(&world, sequence, preparation::Limits::default())
            .err()
            .unwrap();
        if bad_table_count {
            assert!(matches!(
                error,
                preparation::Error::Source(definition_plan::Error::SourceMetadata(_))
            ));
        } else {
            assert!(matches!(
                error,
                preparation::Error::Source(definition_plan::Error::Control(_))
            ));
        }
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn exact_event_and_source_budgets_reject_one_less_without_truncating() {
    let mut body = Vec::new();
    block(&mut body, 0);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(catalogue);
    let sequence = world
        .enqueue(handle, trigger(0, 0), Context::default())
        .unwrap();
    let limits = preparation::Limits {
        maximum_event_instructions: 2,
        ..Default::default()
    };
    assert_eq!(
        prepare(&world, sequence, limits)
            .unwrap()
            .instructions()
            .len(),
        2
    );
    let before = world.snapshot();
    assert!(matches!(
        prepare(
            &world,
            sequence,
            preparation::Limits {
                maximum_event_instructions: 1,
                ..limits
            }
        ),
        Err(preparation::Error::Capacity)
    ));
    let mut limits = limits;
    limits.source.control.decode.max_bytes = body.len() - 1;
    assert!(matches!(
        prepare(&world, sequence, limits),
        Err(preparation::Error::Source(_))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn owned_restore_prepares_the_same_frame_with_new_transient_handles() {
    let mut body = Vec::new();
    block(&mut body, 0);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(Arc::clone(&catalogue));
    world
        .assign(
            handle,
            &[(
                2,
                Value::Number {
                    bits: 0x7ff8123456789abc,
                },
            )],
        )
        .unwrap();
    let sequence = world
        .enqueue(handle, trigger(0, 0), Context::default())
        .unwrap();
    let saved = world.snapshot();
    let restored: World<'static> =
        World::restore(catalogue, saved.clone(), WorldLimits::default()).unwrap();
    let expected = prepare(&world, sequence, preparation::Limits::default()).unwrap();
    let actual = prepare(&restored, sequence, preparation::Limits::default()).unwrap();
    assert_eq!(expected.pending(), actual.pending());
    assert_eq!(expected.source().handle(), actual.source().handle());
    assert_eq!(expected.selected(), actual.selected());
    assert_eq!(expected.instance().locals(), actual.instance().locals());
    assert!(matches!(
        restored.instance(handle),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    assert_eq!(restored.snapshot(), saved);
}

#[test]
fn wrapped_queue_lookup_keeps_exact_sequence_selection() {
    let mut body = Vec::new();
    block(&mut body, 0);
    let (_directory, catalogue) = fixture(&body, false);
    let (mut world, handle) = seeded(catalogue);
    for _ in 0..32 {
        world
            .enqueue(handle, trigger(0, 0), Context::default())
            .unwrap();
    }
    for sequence in 1..=24 {
        world.acknowledge(sequence).unwrap();
    }
    for _ in 0..24 {
        world
            .enqueue(handle, trigger(0, 0), Context::default())
            .unwrap();
    }
    let before = world.snapshot();
    for sequence in 25..=56 {
        assert_eq!(
            prepare(&world, sequence, preparation::Limits::default())
                .unwrap()
                .pending()
                .sequence,
            sequence
        );
    }
    assert_eq!(world.snapshot(), before);
}
