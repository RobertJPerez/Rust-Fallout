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
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::local_copy::{self, Intent, Preparation, StagedCopy, Unsupported},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use std::{fs, path::Path, process::Command, sync::Arc};

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
fn copy() -> Vec<u8> {
    assignment(&[b's', 2, 0], &[b'f', 1, 0])
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
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    let original = unit(
        &[(1, 0), (2, 1), (90, 0)],
        &[(b"SCRO", 0x100), (b"SCRV", 90)],
    );
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", body));
    source.extend(&original[46..]);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &source),
            record(b"MISC", 0x100, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let (catalogue, content) = load_content(directory.path());
    (directory, catalogue, content)
}
fn load_content(path: &Path) -> (Arc<Catalogue>, Content) {
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
fn prepared_sources(catalogue: &Catalogue) -> PreparedSources<'_> {
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
    let signatures: Signatures = [
        (
            0x102f,
            vec![Parameter {
                type_id: 50,
                optional_word: 0,
            }],
        ),
        (0x1001, vec![]),
    ]
    .into_iter()
    .map(|(id, parameters)| {
        (
            id,
            CommandSignature {
                convention: Convention::Default,
                parameters,
            },
        )
    })
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
    bits: Option<u64>,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        Default::default(),
        CampaignId::from_bytes([0x56; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(None).unwrap();
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: vec![ReferenceValue::Null],
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
    if let Some(bits) = bits {
        world
            .assign(handle, &[(1, Value::Number { bits })])
            .unwrap();
    }
    let sequence = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    (world, handle, sequence)
}
fn staged(result: Preparation) -> Box<StagedCopy> {
    match result {
        Preparation::Staged(stage) => stage,
        other => panic!("{other:?}"),
    }
}
fn reason(result: Preparation, expected: Unsupported) {
    assert!(
        matches!(result, Preparation::Unsupported { reason, .. } if reason == expected),
        "{result:?}"
    );
}

#[test]
fn source_less_copy_retains_fixed_source_extents_and_commits_only_one_assignment_and_head() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    for bits in [
        0,
        1,
        0x8000000000000000,
        0x7ff8123456789abc,
        0xfff0123456789abc,
        0x4340000000000001,
        u64::MAX,
    ] {
        let (mut world, handle, sequence) = seed(Arc::clone(&catalogue), Some(bits));
        let before = world.snapshot();
        let stage = staged(
            world
                .stage_source_local_copy_with_sources(
                    sequence,
                    &sources,
                    &content,
                    Intent::Engineering,
                    Default::default(),
                )
                .unwrap(),
        );
        assert_eq!(world.snapshot(), before);
        assert_eq!(stage.trace().instruction_index, 1);
        assert_eq!(stage.trace().statement_scda_bytes, 10..22);
        assert_eq!(stage.trace().source_token_scda_bytes, 19..22);
        assert_eq!(
            stage.trace().statement_bytes,
            [0x15, 0, 8, 0, b's', 2, 0, 3, 0, b'f', 1, 0]
        );
        assert_eq!(stage.trace().source_index, 1);
        assert_eq!(stage.trace().destination_index, 2);
        assert_eq!(stage.trace().destination_before, Value::Uninitialized);
        assert!(!stage.trace().original_behavior_verified);
        assert_eq!(stage.trace().operands.pending, before.pending_events[0]);
        assert_eq!(stage.changes().assignments(), [(2, Value::Number { bits })]);
        assert!(stage.changes().acknowledges());
        let committed = (*stage).commit(&mut world).unwrap();
        assert_eq!(committed.receipt.before_revision, before.state_revision);
        assert_eq!(committed.receipt.after_revision, before.state_revision + 1);
        assert_eq!(committed.receipt.assignments, 1);
        assert_eq!(
            committed.receipt.acknowledged.as_ref(),
            Some(&before.pending_events[0])
        );
        let mut expected = before;
        expected.instances[0]
            .locals
            .iter_mut()
            .find(|local| local.index == 2)
            .unwrap()
            .value = Value::Number { bits };
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        assert_eq!(world.snapshot(), expected);
        assert_eq!(
            world.instance(handle).unwrap().local(1).unwrap(),
            &Value::Number { bits }
        );
        assert!(
            world
                .stage_source_local_copy_with_sources(
                    sequence,
                    &sources,
                    &content,
                    Intent::Engineering,
                    Default::default()
                )
                .is_err()
        );
    }
}

#[test]
fn faithful_assignment_never_reads_unset_storage_or_returns_a_successful_noop() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence) = seed(Arc::clone(&catalogue), None);
    let before = world.snapshot();
    reason(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Faithful,
                Default::default(),
            )
            .unwrap(),
        Unsupported::UnverifiedRetailSemantics,
    );
    reason(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
        Unsupported::LiveOperandUnavailable,
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn operators_literals_commands_foreign_and_reference_slots_are_rejected_before_effects() {
    let cases = [
        (
            assignment(&[b's', 2, 0], b"1"),
            Unsupported::ExpressionShape,
        ),
        (
            assignment(&[b's', 2, 0], &[b'f', 1, 0, b'f', 1, 0, b'+']),
            Unsupported::ExpressionShape,
        ),
        (
            assignment(&[b's', 2, 0], &[b'X', 1, 0x10, 2, 0, 0, 0]),
            Unsupported::ExpressionShape,
        ),
        (
            assignment(&[b's', 2, 0], &[b'r', 2, 0, b'f', 1, 0]),
            Unsupported::ExpressionShape,
        ),
        (
            assignment(&[b'r', 2, 0, b's', 2, 0], &[b'f', 1, 0]),
            Unsupported::DestinationShape,
        ),
        (
            assignment(&[b's', 2, 0], &[b'f', 90, 0]),
            Unsupported::NonNumericLocal,
        ),
        (
            assignment(&[b'f', 90, 0], &[b'f', 1, 0]),
            Unsupported::NonNumericLocal,
        ),
    ];
    for (body, expected) in cases {
        let (_directory, catalogue, content) = fixture(&event(&body));
        let sources = prepared_sources(&catalogue);
        let (mut world, handle, sequence) = seed(Arc::clone(&catalogue), Some(0x7ff8123456789abc));
        let reference = world
            .instance(handle)
            .unwrap()
            .context()
            .calling_reference
            .unwrap();
        world
            .assign(
                handle,
                &[(
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: reference },
                    },
                )],
            )
            .unwrap();
        let before = world.snapshot();
        reason(
            world
                .stage_source_local_copy_with_sources(
                    sequence,
                    &sources,
                    &content,
                    Intent::Engineering,
                    Default::default(),
                )
                .unwrap(),
            expected,
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn more_instructions_and_branch_containing_events_never_select_an_execution_path() {
    let mut branch = Vec::new();
    instruction(&mut branch, 0x16, &[1, 0, 1, 0, b'0']);
    branch.extend(copy());
    instruction(&mut branch, 0x19, &[]);
    for body in [[copy(), copy()].concat(), branch, Vec::new()] {
        let (_directory, catalogue, content) = fixture(&event(&body));
        let sources = prepared_sources(&catalogue);
        let (world, _, sequence) = seed(Arc::clone(&catalogue), Some(8));
        let before = world.snapshot();
        reason(
            world
                .stage_source_local_copy_with_sources(
                    sequence,
                    &sources,
                    &content,
                    Intent::Engineering,
                    Default::default(),
                )
                .unwrap(),
            Unsupported::EventShape,
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn exact_budgets_allow_staging_and_one_less_errors_leave_every_bank_and_event_unchanged() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (world, _, sequence) = seed(Arc::clone(&catalogue), Some(8));
    let before = world.snapshot();
    let exact = local_copy::Limits {
        maximum_event_instructions: 3,
        maximum_operand_uses: 2,
        maximum_statement_bytes: 12,
    };
    let stage = staged(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                exact,
            )
            .unwrap(),
    );
    drop(stage);
    for (limits, intended) in [
        (
            local_copy::Limits {
                maximum_event_instructions: 2,
                ..exact
            },
            "prepared event instruction budget exceeded",
        ),
        (
            local_copy::Limits {
                maximum_operand_uses: 1,
                ..exact
            },
            "event operand probe budget exceeded",
        ),
        (
            local_copy::Limits {
                maximum_statement_bytes: 11,
                ..exact
            },
            "local-copy statement-byte budget exceeded",
        ),
    ] {
        let error = world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                limits,
            )
            .unwrap_err();
        assert_eq!(error.to_string(), intended);
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn intervening_revision_restore_and_other_campaign_reject_old_stages_without_effects() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (mut world, handle, sequence) = seed(Arc::clone(&catalogue), Some(8));
    let stage = staged(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
    );
    world
        .assign(handle, &[(1, Value::Number { bits: 9 })])
        .unwrap();
    let changed = world.snapshot();
    assert_eq!(
        (*stage).commit(&mut world).unwrap_err().to_string(),
        "runtime state is invalid: staged event revision changed"
    );
    assert_eq!(world.snapshot(), changed);
    let stage = staged(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
    );
    let mut restored =
        World::restore(Arc::clone(&catalogue), changed.clone(), Default::default()).unwrap();
    assert!((*stage).commit(&mut restored).is_err());
    assert_eq!(restored.snapshot(), changed);
    let stage = staged(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
    );
    let mut other_snapshot = changed.clone();
    other_snapshot.campaign = CampaignId::from_bytes([0x57; 16]).unwrap();
    let mut other = World::restore(
        Arc::clone(&catalogue),
        other_snapshot.clone(),
        Default::default(),
    )
    .unwrap();
    assert!((*stage).commit(&mut other).is_err());
    assert_eq!(other.snapshot(), other_snapshot);
}

#[test]
fn nonhead_events_and_changed_whole_source_cohorts_never_get_commit_authority() {
    let (directory, catalogue, content) = fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (mut world, handle, sequence) = seed(Arc::clone(&catalogue), Some(8));
    let second = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    assert!(
        world
            .stage_source_local_copy_with_sources(
                second,
                &sources,
                &content,
                Intent::Engineering,
                Default::default()
            )
            .unwrap_err()
            .to_string()
            .contains("staging must name the first pending event")
    );
    let mut changed = fs::read(directory.path().join("FalloutNV.esm")).unwrap();
    changed.extend(record(b"MISC", 0x401, 0, &[]));
    fs::write(directory.path().join("FalloutNV.esm"), changed).unwrap();
    let (other_catalogue, other_content) = load_content(directory.path());
    let other_sources = prepared_sources(&other_catalogue);
    assert!(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &other_sources,
                &content,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
    );
    assert!(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &other_content,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn committed_source_copy_survives_a_fresh_process_with_fixed_numeric_and_pending_expectations() {
    let (directory, catalogue, content) = fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue), Some(0x7ff8123456789abc));
    let stage = staged(
        world
            .stage_source_local_copy_with_sources(
                sequence,
                &sources,
                &content,
                Intent::Engineering,
                Default::default(),
            )
            .unwrap(),
    );
    (*stage).commit(&mut world).unwrap();
    fs::write(
        directory.path().join("after.snapshot.json"),
        world.snapshot().encode(64 * 1024 * 1024).unwrap(),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_restore_helper", "--ignored", "--nocapture"])
        .env("FALLOUT_VM_COPY_COLD_DIRECTORY", directory.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("VM_COPY_COLD_FIXED_EXPECTATIONS_PASSED")
    );
}

#[test]
#[ignore = "launched by parent with private authored source and snapshot"]
fn cold_restore_helper() {
    let directory = std::env::var_os("FALLOUT_VM_COPY_COLD_DIRECTORY").unwrap();
    let directory = Path::new(&directory);
    let (catalogue, _) = load_content(directory);
    let limits = fallout_runtime::Limits::default();
    let snapshot = Snapshot::decode(
        &fs::read(directory.join("after.snapshot.json")).unwrap(),
        limits,
    )
    .unwrap();
    let world = World::restore(catalogue, snapshot, limits).unwrap();
    assert_eq!(
        world.campaign(),
        CampaignId::from_bytes([0x56; 16]).unwrap()
    );
    assert_eq!(world.instance_count(), 1);
    assert_eq!(world.pending_events().len(), 0);
    assert_eq!(world.revision(), 5);
    let instance = world.snapshot().instances[0].id;
    let live = world.instance(world.handle(instance).unwrap()).unwrap();
    for index in [1, 2] {
        assert_eq!(
            live.local(index).unwrap(),
            &Value::Number {
                bits: 0x7ff8123456789abc
            }
        );
    }
    assert_eq!(live.locals()[&90], Value::Uninitialized);
    println!("VM_COPY_COLD_FIXED_EXPECTATIONS_PASSED");
}

fn saved_fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let (directory, _, _) = fixture(body);
    let path = directory.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    let placement: Vec<_> = [0x3f800000_u32, 0x80000000, 1, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    bytes.extend(record(b"CELL", 0x400, 0, &field(b"DATA", &[1])));
    bytes.extend(record(
        b"REFR",
        0x500,
        0,
        &[
            field(b"NAME", &0x100_u32.to_le_bytes()),
            field(b"DATA", &placement),
        ]
        .concat(),
    ));
    fs::write(path, bytes).unwrap();
    let (catalogue, content) = load_content(directory.path());
    (directory, catalogue, content)
}

fn saved_world(catalogue: Arc<Catalogue>) -> (World<'static>, u64) {
    let (mut world, handle, sequence) = seed(catalogue, Some(0x7ff8123456789abc));
    world
        .assign(
            handle,
            &[(
                2,
                Value::Number {
                    bits: 0x8000000000000000,
                },
            )],
        )
        .unwrap();
    let unrelated = world.register_reference(Some(form(0x500))).unwrap();
    let view = world.reference_view(unrelated).unwrap();
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
    world.initialize_inventory(unrelated).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x100));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc12345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"TEST",
            bytes: vec![0, 255, 1],
        });
    world
        .add_item(unrelated, facts, 19.try_into().unwrap())
        .unwrap();
    world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, sequence)
}

fn expected_saved_copy(mut before: Snapshot) -> Snapshot {
    before.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 2)
        .unwrap()
        .value = Value::Number {
        bits: 0x7ff8123456789abc,
    };
    before.state_revision += 1;
    before.pending_events.remove(0);
    before
}

#[test]
fn saved_pending_consumer_commits_existing_head_and_keeps_every_unrelated_bank_on_cold_restore() {
    use fallout_runtime::execution::copy_probe::{self, PendingOutcome};
    let (_directory, catalogue, content) = saved_fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (world, sequence) = saved_world(Arc::clone(&catalogue));
    let before = world.snapshot();
    let mut restored =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    let result = copy_probe::commit_pending(
        &mut restored,
        &sources,
        &content,
        sequence,
        1.try_into().unwrap(),
        Intent::Engineering,
        Default::default(),
    )
    .unwrap();
    let PendingOutcome::EngineeringCommitted { committed } = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        committed.trace.destination_before,
        Value::Number {
            bits: 0x8000000000000000
        }
    );
    assert_eq!(committed.receipt.assignments, 1);
    assert_eq!(
        committed.receipt.acknowledged.as_ref(),
        Some(&before.pending_events[0])
    );
    let expected = expected_saved_copy(before);
    assert_eq!(restored.snapshot(), expected);
    let mut cold =
        World::restore(Arc::clone(&catalogue), expected.clone(), Default::default()).unwrap();
    assert!(
        copy_probe::commit_pending(
            &mut cold,
            &sources,
            &content,
            sequence,
            1.try_into().unwrap(),
            Intent::Engineering,
            Default::default()
        )
        .is_err()
    );
    assert_eq!(cold.snapshot(), expected);
}

#[test]
fn saved_pending_consumer_requires_matching_activation_head_and_explicit_engineering() {
    use fallout_runtime::execution::copy_probe::{self, PendingOutcome};
    let (_directory, catalogue, content) = saved_fixture(&event(&copy()));
    let sources = prepared_sources(&catalogue);
    let (mut world, sequence) = saved_world(Arc::clone(&catalogue));
    let before = world.snapshot();
    for (requested, activation) in [(sequence, 2), (sequence + 1, 1), (0, 1)] {
        assert!(
            copy_probe::commit_pending(
                &mut world,
                &sources,
                &content,
                requested,
                activation.try_into().unwrap(),
                Intent::Engineering,
                Default::default()
            )
            .is_err()
        );
        assert_eq!(world.snapshot(), before);
    }
    let result = copy_probe::commit_pending(
        &mut world,
        &sources,
        &content,
        sequence,
        1.try_into().unwrap(),
        Intent::Faithful,
        Default::default(),
    )
    .unwrap();
    assert!(matches!(
        result,
        PendingOutcome::Unsupported {
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "built CLI and authored metadata copy; saved engineering state only, no original launch"]
fn cli_saved_pending_copy_helper() {
    use serde_json::{Value as Json, json};
    use std::path::PathBuf;
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_SAVED_COPY_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let (temporary, catalogue, _) = saved_fixture(&event(&copy()));
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        temporary.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        input.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (world, sequence) = saved_world(Arc::clone(&catalogue));
    let before = world.snapshot();
    let before_bytes = before.encode(64 * 1024 * 1024).unwrap();
    let initial = evidence.join("initial.snapshot.json");
    fs::write(&initial, &before_bytes).unwrap();
    let request =
        json!({"schema_version":1,"sequence":sequence,"activation":1,"intent":"engineering"});
    let run_on =
        |name: &str, install: &Path, snapshot: &Path, request: &Json, result: Option<&Path>| {
            let directory = evidence.join(name);
            fs::create_dir(&directory).unwrap();
            let request_path = directory.join("request.json");
            let report = directory.join("report.json");
            let result = result
                .map(Path::to_path_buf)
                .unwrap_or_else(|| directory.join("result.snapshot.json"));
            fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
            let output = Command::new(&cli)
                .args(["event-operands", "--install"])
                .arg(install)
                .arg("--load-order")
                .arg(&order)
                .arg("--snapshot-copy-request")
                .arg(&request_path)
                .arg("--snapshot-input")
                .arg(snapshot)
                .arg("--snapshot-output")
                .arg(&result)
                .arg("--output")
                .arg(&report)
                .output()
                .unwrap();
            fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
            fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
            (output, report, result)
        };
    let run = |name: &str, snapshot: &Path, request: &Json, result: Option<&Path>| {
        run_on(name, &install, snapshot, request, result)
    };
    let (output, report_path, result_path) = run("commit", &initial, &request, None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["snapshot_copy"]["status"], "engineering_committed");
    assert_eq!(
        report["snapshot_copy"]["committed"]["trace"]["copied_value"]["bits"],
        0x7ff8123456789abc_u64
    );
    assert_eq!(report["faithful_execution_admitted"], false);
    assert_eq!(report["event_acknowledged"], true);
    assert_eq!(
        report["prepared_sources"]["counts"]["preparation_attempts"],
        1
    );
    let after = Snapshot::decode(&fs::read(&result_path).unwrap(), Default::default()).unwrap();
    assert_eq!(after, expected_saved_copy(before.clone()));
    assert_eq!(fs::read(&initial).unwrap(), before_bytes);
    let (output, report, result) = run("cold-replay", &result_path, &request, None);
    assert!(!output.status.success());
    assert!(!result.exists());
    assert!(!report.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("journal head"));
    for (name, field, value, reason) in [
        ("nonhead", "sequence", json!(sequence + 1), "journal head"),
        ("activation", "activation", json!(2), "activation differs"),
        ("zero-activation", "activation", json!(0), "nonzero"),
        ("missing-intent", "intent", Json::Null, "missing field"),
        ("faithful", "intent", json!("faithful"), "unknown variant"),
        (
            "intent-object-null",
            "intent",
            json!({"engineering":null}),
            "expected a string",
        ),
        (
            "intent-object-map",
            "intent",
            json!({"engineering":{}}),
            "expected a string",
        ),
        ("initializer", "initial_numbers", json!([]), "unknown field"),
        (
            "request-schema",
            "schema_version",
            json!(2),
            "request schema",
        ),
        (
            "request-byte-limit",
            "extra",
            json!("x".repeat(16 * 1024)),
            "byte budget",
        ),
    ] {
        let mut invalid = request.clone();
        if name == "missing-intent" {
            invalid.as_object_mut().unwrap().remove(field);
        } else {
            invalid[field] = value;
        }
        let (output, report, result) = run(name, &initial, &invalid, None);
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists(), "{name}");
        assert!(!result.exists(), "{name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let (output, report, _) = run("existing-output", &initial, &request, Some(&result_path));
    assert!(!output.status.success());
    assert!(!report.exists());
    assert_eq!(
        Snapshot::decode(&fs::read(&result_path).unwrap(), Default::default()).unwrap(),
        after
    );
    let protected = install.join("result.snapshot.json");
    let (output, report, _) = run("protected-output", &initial, &request, Some(&protected));
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!protected.exists());
    for (name, mutate) in [
        ("legacy-schema", 0),
        ("wrong-cohort", 1),
        ("unset-input", 2),
    ] {
        let mut snapshot = serde_json::to_value(&before).unwrap();
        match mutate {
            0 => snapshot["schema_version"] = json!(3),
            1 => snapshot["catalogue_sha256"] = json!("0".repeat(64)),
            _ => {
                let locals = snapshot["instances"][0]["locals"].as_array_mut().unwrap();
                locals.iter_mut().find(|local| local["index"] == 1).unwrap()["value"] =
                    json!({"kind":"uninitialized"});
            }
        }
        let path = evidence.join(format!("{name}.snapshot.json"));
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let (output, report, result) = run(name, &path, &request, None);
        assert!(!output.status.success());
        assert!(!result.exists());
        if mutate == 2 {
            let report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
            assert_eq!(report["snapshot_copy"]["status"], "unsupported");
            assert_eq!(report["canonical_state_unchanged"], true);
            assert_eq!(report["result_snapshot"], Json::Null);
            assert_eq!(report["event_acknowledged"], false);
        } else {
            assert!(!report.exists());
        }
    }
    // A different valid source event is bound to its own restored snapshot;
    // unmeasured literal semantics must refuse without publishing replacement.
    let (unsupported_source, unsupported_catalogue, _) =
        saved_fixture(&event(&assignment(&[b's', 2, 0], b"1")));
    let literal_install = evidence.join("literal-source-copy");
    fs::create_dir(&literal_install).unwrap();
    fs::create_dir(literal_install.join("Data")).unwrap();
    fs::copy(
        unsupported_source.path().join("FalloutNV.esm"),
        literal_install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        install.join("FalloutNV.exe"),
        literal_install.join("FalloutNV.exe"),
    )
    .unwrap();
    let (unsupported_world, _) = saved_world(unsupported_catalogue);
    let path = evidence.join("literal.snapshot.json");
    fs::write(
        &path,
        unsupported_world
            .snapshot()
            .encode(64 * 1024 * 1024)
            .unwrap(),
    )
    .unwrap();
    let (output, report, result) = run_on(
        "unsupported-literal",
        &literal_install,
        &path,
        &request,
        None,
    );
    assert!(!output.status.success());
    assert!(!result.exists());
    let report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(report["snapshot_copy"]["reason"], "expression_shape");
    assert_eq!(report["canonical_state_unchanged"], true);
    assert_eq!(report["event_acknowledged"], false);
    fs::write(evidence.join("scope.json"), serde_json::to_vec_pretty(&json!({
        "scope":"strict_current_snapshot_engineering_copy_only","original_executed":false,
        "input_schema":before.schema_version,"canonical_expected_snapshot":expected_saved_copy(before),
        "actual_cli_calls":18,"retail_parity_accepted":false
    })).unwrap()).unwrap();
}
