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

fn owned_fixture() -> (tempfile::TempDir, Arc<Catalogue>, Vec<u8>, usize) {
    let mut middle = Vec::new();
    instruction(&mut middle, 0x15, &[b's', 2, 0, 3, 0, b's', 42, 0]);
    let mut one = Vec::new();
    instruction(
        &mut one,
        0x10,
        &[
            0_u16.to_le_bytes().as_slice(),
            &((middle.len() + 4) as u32).to_le_bytes(),
        ]
        .concat(),
    );
    one.extend(middle);
    instruction(&mut one, 0x11, &[]);
    let body = [one.clone(), one.clone()].concat();
    let original = unit(&[(2, 1), (42, 0), (90, 0), (99, 7)], &[(b"SCRV", 90)]);
    let mut payload = original[..26].to_vec();
    payload[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
    payload.extend(field(b"SCDA", &body));
    payload.extend(&original[46..]);
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(&[]), record(b"SCPT", 0x300, 0, &payload)].concat(),
    )
    .unwrap();
    let catalogue = Arc::new(load(directory.path(), &["FalloutNV.esm"]));
    (directory, catalogue, body, one.len())
}

#[test]
fn owned_frame_is_historical_after_mutation_acknowledgement_restore_and_world_drop() {
    let (_directory, catalogue, body, length) = owned_fixture();
    let mut world = World::new(Arc::clone(&catalogue), WorldLimits::default()).unwrap();
    let context = Context {
        target: Some(ReferenceValue::Content { key: form(0x100) }),
        arguments: vec![
            ReferenceValue::Content { key: form(0x101) },
            ReferenceValue::Null,
        ],
        ..Default::default()
    };
    let handle = world
        .create_instance(
            &definition(&catalogue),
            Owner::Quest { key: form(0x100) },
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            handle,
            &[
                (
                    2,
                    Value::Number {
                        bits: 0x8000000000000000,
                    },
                ),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8123456789abc,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Content { key: form(0x102) },
                    },
                ),
            ],
        )
        .unwrap();
    let first = world
        .enqueue(handle, trigger(0, 0), Context::default())
        .unwrap();
    let sequence = world
        .enqueue(handle, trigger(length as u32, 0), context)
        .unwrap();
    let before = world.snapshot();
    let observation = preparation::EventObservation::capture(
        &prepare(&world, sequence, Default::default()).unwrap(),
        &[90, 42, 2, 42, 99],
        Default::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), before);
    assert_eq!(observation.source_bytes, body[length..]);
    assert_eq!(observation.begin_scda_offset, length);
    assert_eq!(observation.end_scda_offset, body.len());
    assert_eq!(observation.pending.sequence, sequence);
    assert_eq!(observation.definition, definition(&catalogue));
    assert_eq!(observation.state_revision, world.revision());
    assert_eq!(observation.source_operands.len(), 2);
    assert!(
        observation
            .source_operands
            .iter()
            .all(|binding| binding.scda_offset >= length)
    );
    assert_eq!(
        observation.locals[0].value,
        Value::Reference {
            value: ReferenceValue::Content { key: form(0x102) }
        }
    );
    assert_eq!(
        observation.locals[1].value,
        Value::Number {
            bits: 0x7ff8123456789abc
        }
    );
    assert_eq!(
        observation.locals[2].value,
        Value::Number {
            bits: 0x8000000000000000
        }
    );
    assert_eq!(observation.locals[3].value, observation.locals[1].value);
    assert_eq!(observation.locals[4].value, Value::Uninitialized);
    let expected = serde_json::to_value(&observation).unwrap();
    // The thread cannot read until the canonical instance is changed, the
    // observed journal entry is acknowledged, and both Worlds are dropped.
    let (release, wait) = std::sync::mpsc::channel();
    let consumer = std::thread::spawn(move || {
        wait.recv().unwrap();
        serde_json::to_value(observation).unwrap()
    });
    world
        .assign(handle, &[(42, Value::Number { bits: 123 })])
        .unwrap();
    world.acknowledge(first).unwrap();
    world.acknowledge(sequence).unwrap();
    assert!(world.revision() > before.state_revision);
    let restored: World<'static> = World::restore(
        Arc::clone(&catalogue),
        world.snapshot(),
        WorldLimits::default(),
    )
    .unwrap();
    assert_eq!(
        restored
            .instance(restored.handle(before.instances[0].id).unwrap())
            .unwrap()
            .local(42)
            .unwrap(),
        &Value::Number { bits: 123 }
    );
    drop(restored);
    drop(world);
    drop(catalogue);
    release.send(()).unwrap();
    assert_eq!(consumer.join().unwrap(), expected);
    assert_eq!(expected["historical"], true);
}

#[test]
fn owned_payload_admission_covers_every_clone_with_exact_and_one_under_limits() {
    let (_directory, catalogue, _, _) = owned_fixture();
    let (mut world, handle) = seeded(catalogue);
    world
        .assign(
            handle,
            &[(
                90,
                Value::Reference {
                    value: ReferenceValue::Content { key: form(0x101) },
                },
            )],
        )
        .unwrap();
    let sequence = world
        .enqueue(
            handle,
            trigger(0, 0),
            Context {
                target: Some(ReferenceValue::Content { key: form(0x100) }),
                arguments: vec![ReferenceValue::Content { key: form(0x102) }],
                ..Default::default()
            },
        )
        .unwrap();
    let before = world.snapshot();
    let frame = prepare(&world, sequence, Default::default()).unwrap();
    let baseline =
        preparation::EventObservation::capture(&frame, &[90, 2, 90], Default::default()).unwrap();
    let counts = baseline.counts;
    assert_eq!(counts.rows, 2 + 3 + 1);
    assert_eq!(counts.binding_uses, 4);
    // Three 64-byte hash strings plus definition, pending target/argument and
    // two repeated local content-reference plugin strings; no omitted fields.
    assert_eq!(counts.variable_bytes, 3 * 64 + 5 * "falloutnv.esm".len());
    let exact = preparation::ObservationLimits {
        maximum_source_bytes: counts.source_bytes,
        maximum_rows: counts.rows,
        maximum_variable_bytes: counts.variable_bytes,
        maximum_binding_uses: counts.binding_uses,
    };
    assert_eq!(
        serde_json::to_value(
            preparation::EventObservation::capture(&frame, &[90, 2, 90], exact).unwrap()
        )
        .unwrap(),
        serde_json::to_value(baseline).unwrap()
    );
    for (kind, limits) in [
        (
            "source bytes",
            preparation::ObservationLimits {
                maximum_source_bytes: counts.source_bytes - 1,
                ..exact
            },
        ),
        (
            "rows",
            preparation::ObservationLimits {
                maximum_rows: counts.rows - 1,
                ..exact
            },
        ),
        (
            "variable bytes",
            preparation::ObservationLimits {
                maximum_variable_bytes: counts.variable_bytes - 1,
                ..exact
            },
        ),
        (
            "binding uses",
            preparation::ObservationLimits {
                maximum_binding_uses: counts.binding_uses - 1,
                ..exact
            },
        ),
    ] {
        assert!(
            matches!(preparation::EventObservation::capture(&frame, &[90, 2, 90], limits), Err(preparation::ObservationError::Capacity(actual)) if kind == actual)
        );
    }
    assert!(matches!(
        preparation::EventObservation::capture(&frame, &[2, 999], Default::default()),
        Err(preparation::ObservationError::State(
            fallout_runtime::Error::MissingLocal(999)
        ))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn owned_cached_and_fresh_frames_are_identical_and_empty_local_selection_stays_explicit() {
    let (_directory, catalogue, body, length) = owned_fixture();
    let (mut world, handle) = seeded(Arc::clone(&catalogue));
    let sequence = world
        .enqueue(handle, trigger(length as u32, 0), Context::default())
        .unwrap();
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = fallout_runtime::programs::PreparedSources::load(
        &catalogue,
        &model,
        &Signatures::new(),
        Default::default(),
    )
    .unwrap();
    let fresh = preparation::EventObservation::capture(
        &prepare(&world, sequence, Default::default()).unwrap(),
        &[],
        Default::default(),
    )
    .unwrap();
    let cached = preparation::EventObservation::capture(
        &world
            .prepare_event_with_sources(sequence, &sources, 3)
            .unwrap(),
        &[],
        Default::default(),
    )
    .unwrap();
    assert!(cached.locals.is_empty());
    assert_eq!(cached.source_bytes, body[length..]);
    assert_eq!(
        serde_json::to_value(cached).unwrap(),
        serde_json::to_value(fresh).unwrap()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "built CLI and authored executable metadata; owned historical observations only, no original launch"]
fn cli_owned_frame_helper() {
    use serde_json::{Value as Json, json};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_OWNED_FRAME_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let (temporary, catalogue, _, length) = owned_fixture();
    let (mut world, handle) = seeded(Arc::clone(&catalogue));
    world
        .assign(
            handle,
            &[
                (
                    2,
                    Value::Number {
                        bits: 0x8000000000000000,
                    },
                ),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8123456789abc,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Content { key: form(0x101) },
                    },
                ),
            ],
        )
        .unwrap();
    world
        .enqueue(handle, trigger(0, 0), Context::default())
        .unwrap();
    let sequence = world
        .enqueue(
            handle,
            trigger(length as u32, 0),
            Context {
                target: Some(ReferenceValue::Content { key: form(0x101) }),
                arguments: vec![
                    ReferenceValue::Null,
                    ReferenceValue::Content { key: form(0x102) },
                ],
                ..Default::default()
            },
        )
        .unwrap();
    let before = world.snapshot();
    let snapshot_bytes = before
        .encode(WorldLimits::default().max_snapshot_bytes)
        .unwrap();
    let snapshot = evidence.join("snapshot.json");
    fs::write(&snapshot, &snapshot_bytes).unwrap();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        temporary.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        metadata.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let selected = [90, 42, 2, 42, 99];
    let expected = preparation::EventObservation::capture(
        &prepare(&world, sequence, Default::default()).unwrap(),
        &selected,
        Default::default(),
    )
    .unwrap();
    let counts = expected.counts;
    let request = json!({"schema_version":1,"sequence":sequence,"selected_locals":selected,
        "maximum_source_bytes":counts.source_bytes,"maximum_rows":counts.rows,
        "maximum_variable_bytes":counts.variable_bytes,"maximum_binding_uses":counts.binding_uses});
    let run_on = |name: &str,
                  install: &std::path::Path,
                  snapshot: &std::path::Path,
                  request: &Json,
                  extra: &[&str]| {
        let original_snapshot_bytes = fs::read(snapshot).unwrap();
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let input = directory.join("request.json");
        fs::write(&input, serde_json::to_vec(request).unwrap()).unwrap();
        let report = directory.join("report.json");
        let output = Command::new(&cli)
            .args(["event-frames", "--install"])
            .arg(install)
            .arg("--load-order")
            .arg(&order)
            .arg("--owned-observation")
            .arg(input)
            .arg("--snapshot-input")
            .arg(snapshot)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        assert_eq!(
            fs::read(snapshot).unwrap(),
            original_snapshot_bytes,
            "{name}: source snapshot changed"
        );
        (output, report)
    };
    let run = |name: &str, request: &Json| run_on(name, &install, &snapshot, request, &[]);
    let (output, report) = run("exact", &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 2);
    assert_eq!(
        report["observation"],
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(report["consumer"]["thread"], true);
    assert_eq!(report["consumer"]["world_dropped_before_read"], true);
    assert_eq!(report["canonical_state_unchanged"], true);
    let mut empty = request.clone();
    empty["selected_locals"] = json!([]);
    let (output, report) = run("empty-locals", &empty);
    assert!(output.status.success());
    let empty_report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(empty_report["observation"]["locals"], json!([]));
    for (name, field, value, reason) in [
        (
            "source-short",
            "maximum_source_bytes",
            json!(counts.source_bytes - 1),
            "source bytes",
        ),
        ("rows-short", "maximum_rows", json!(counts.rows - 1), "rows"),
        (
            "variable-short",
            "maximum_variable_bytes",
            json!(counts.variable_bytes - 1),
            "variable bytes",
        ),
        (
            "binding-short",
            "maximum_binding_uses",
            json!(counts.binding_uses - 1),
            "binding uses",
        ),
        (
            "missing-local",
            "selected_locals",
            json!([2, 999]),
            "not declared",
        ),
        ("missing-pending", "sequence", json!(99), "pending event 99"),
        (
            "zero-sequence",
            "sequence",
            json!(0),
            "invalid owned event request",
        ),
        (
            "request-schema",
            "schema_version",
            json!(2),
            "invalid owned event request",
        ),
        (
            "source-ceiling",
            "maximum_source_bytes",
            json!(1024 * 1024 + 1),
            "invalid owned event request",
        ),
        (
            "rows-ceiling",
            "maximum_rows",
            json!(65_537),
            "invalid owned event request",
        ),
        (
            "variable-ceiling",
            "maximum_variable_bytes",
            json!(1024 * 1024 + 1),
            "invalid owned event request",
        ),
        (
            "binding-ceiling",
            "maximum_binding_uses",
            json!(262_145),
            "invalid owned event request",
        ),
        (
            "selection-ceiling",
            "selected_locals",
            json!(vec![0; 4097]),
            "invalid owned event request",
        ),
        ("unknown-field", "initialize", json!(true), "unknown field"),
        (
            "request-byte-limit",
            "extra",
            json!("x".repeat(16 * 1024)),
            "input byte budget",
        ),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, report) = run(name, &invalid);
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists(), "{name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (name, field, value) in [
        ("legacy", "schema_version", json!(3)),
        ("stale", "catalogue_sha256", json!("0".repeat(64))),
    ] {
        let mut invalid = serde_json::to_value(&before).unwrap();
        invalid[field] = value;
        let file = evidence.join(format!("{name}.snapshot.json"));
        fs::write(&file, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let (output, report) = run_on(name, &install, &file, &request, &[]);
        assert!(!output.status.success());
        assert!(!report.exists());
    }
    let bundle = evidence.join("forbidden.bundle");
    let (output, report) = run_on(
        "bundle-conflict",
        &install,
        &snapshot,
        &request,
        &["--comparison-bundle", bundle.to_str().unwrap()],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!bundle.exists());
    for (name, flag, input) in [
        (
            "missing-snapshot",
            "--owned-observation",
            evidence.join("exact/request.json"),
        ),
        ("orphan-snapshot", "--snapshot-input", snapshot.clone()),
    ] {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let report = directory.join("report.json");
        let output = Command::new(&cli)
            .args(["event-frames", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg(flag)
            .arg(input)
            .arg("--output")
            .arg(&report)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        assert!(!output.status.success());
        assert!(!report.exists());
    }
    let changed = evidence.join("changed-source-copy");
    fs::create_dir(&changed).unwrap();
    fs::create_dir(changed.join("Data")).unwrap();
    fs::copy(install.join("FalloutNV.exe"), changed.join("FalloutNV.exe")).unwrap();
    let bytes = [
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        record(b"GLOB", 0x701, 0, &[]),
    ]
    .concat();
    fs::write(changed.join("Data/FalloutNV.esm"), bytes).unwrap();
    let (output, report) = run_on("changed-source", &changed, &snapshot, &request, &[]);
    assert!(!output.status.success());
    assert!(!report.exists());
    // Preserve and exercise the original source-only inspector and bundle.
    let directory = evidence.join("legacy-mode");
    fs::create_dir(&directory).unwrap();
    let report = directory.join("report.json");
    let bundle = directory.join("frames.bundle");
    let output = Command::new(&cli)
        .args(["event-frames", "--install"])
        .arg(&install)
        .arg("--load-order")
        .arg(&order)
        .arg("--comparison-bundle")
        .arg(&bundle)
        .arg("--output")
        .arg(&report)
        .output()
        .unwrap();
    fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let legacy: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(legacy["schema_version"], 1);
    assert_eq!(legacy["prepared_frames"], 1);
    assert!(legacy.get("observation").is_none());
    assert_eq!(&fs::read(bundle).unwrap()[..8], b"FROBS001");
    assert_eq!(fs::read(&snapshot).unwrap(), snapshot_bytes);
    assert_eq!(world.snapshot(), before);
    fs::write(evidence.join("scope.json"), serde_json::to_vec_pretty(&json!({"scope":"owned historical source frame only", "actual_cli_calls":24,"original_executed":false,"canonical_state_unchanged":true,"counts":counts,"retail_parity_accepted":false})).unwrap()).unwrap();
}
