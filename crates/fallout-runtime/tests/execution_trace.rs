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
    execution::{copy_probe, fixture as authored, local_copy, native, trace::*},
    foreign::Content,
    identity::{CampaignId, Owner, Value},
    programs::PreparedSources,
};
use sha2::{Digest, Sha256};
use std::{fs, sync::Arc};

fn instruction(opcode: u16, payload: &[u8]) -> Vec<u8> {
    [
        opcode.to_le_bytes().as_slice(),
        &(payload.len() as u16).to_le_bytes(),
        payload,
    ]
    .concat()
}
fn copy() -> Vec<u8> {
    instruction(0x15, &[b's', 2, 0, 3, 0, b'f', 1, 0])
}
fn event(middle: &[u8]) -> Vec<u8> {
    [
        instruction(
            0x10,
            &[
                0_u16.to_le_bytes().as_slice(),
                &((middle.len() + 4) as u32).to_le_bytes(),
            ]
            .concat(),
        ),
        middle.to_vec(),
        instruction(0x11, &[]),
    ]
    .concat()
}
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    let original = unit(&[(1, 0), (2, 1)], &[(b"SCRO", 0x100)]);
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
    let mut store = fallout_data::store::RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
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
        .map(|(index, spelling)| Operator {
            code: index as u32,
            precedence: index as u8,
            spelling: spelling.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap();
    let signatures: Signatures = [(
        0x102f,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![Parameter {
                type_id: 50,
                optional_word: 0,
            }],
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
fn manifest(
    plan: &fallout_data::obscript::definition_plan::Plan<'_>,
    cohort: &str,
    operation: Operation,
    offset: u32,
) -> Manifest {
    Manifest {
        schema_version: 1,
        identity: Identity {
            executable_sha256: "a".repeat(64),
            profile_receipt_sha256: "b".repeat(64),
            source_cohort_sha256: cohort.into(),
            winning_content_sha256: plan.source_cohort_sha256().into(),
            definition: plan.handle().clone(),
            compiled_sha256: format!("{:x}", Sha256::digest(plan.control().bytes())),
            compiled_bytes: plan.control().bytes().len(),
        },
        purpose: operation,
        steps: vec![StepInput {
            event_ordinal: 0,
            event_id: 0,
            begin_scda_offset: 0,
            scda_offset: offset,
            operation,
            caller: Caller {
                calling_reference: Some(form(0x14)),
                containing_reference: None,
                target: None,
                activation: 1,
            },
            operands: vec![Word::binary64(0x8000000000000000)],
            item: (operation == Operation::GetItemCount).then(|| form(0x100)),
        }],
    }
}
fn capture(manifest: &Manifest, producer: Producer, output: StepOutput) -> Capture {
    Capture {
        schema_version: 1,
        identity: manifest.identity.clone(),
        producer,
        producer_executable_sha256: if producer == Producer::Original {
            "a"
        } else {
            "c"
        }
        .repeat(64),
        transport_receipt_sha256: "d".repeat(64),
        instrumentation: "Synthetic comparison regression; never an original-game capture".into(),
        finish: Finish::Completed,
        steps: manifest
            .steps
            .iter()
            .cloned()
            .map(|input| Step {
                input,
                output: output.clone(),
            })
            .collect(),
    }
}
fn output() -> StepOutput {
    StepOutput {
        return_value: None,
        successor_scda_offset: None,
        writes: vec![LocalWrite {
            index: 2,
            value: Word::binary64(0x8000000000000000),
        }],
        error: None,
    }
}

#[test]
fn all_four_probe_purposes_use_prepared_positions_and_altered_outputs_fail() {
    // These bytes reuse the existing authored framing fixtures. This test proves
    // comparison mechanics, not original numeric, branch or command semantics.
    let body = event(
        &[
            copy(),
            instruction(0x16, &[0, 0, 3, 0, b'f', 1, 0]),
            instruction(0x19, &[]),
            instruction(0x102f, &[1, 0, b'r', 1, 0]),
        ]
        .concat(),
    );
    let (_directory, catalogue, _content) = fixture(&body);
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    for (operation, offset) in [
        (Operation::Assignment, 10),
        (Operation::Conversion, 10),
        (Operation::Branch, 22),
        (Operation::GetItemCount, 37),
    ] {
        let manifest = manifest(plan, sources.source_cohort_sha256(), operation, offset);
        let mut observed = output();
        if operation == Operation::Branch {
            observed.successor_scda_offset = Some(33);
            observed.writes.clear();
        }
        if operation == Operation::GetItemCount {
            observed.return_value = Some(Word::binary64(0x4008000000000000));
            observed.writes.clear();
        }
        let original = capture(&manifest, Producer::Original, observed.clone());
        let mut replacement = capture(&manifest, Producer::Replacement, observed);
        let matched = compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Default::default(),
        )
        .unwrap();
        assert_eq!(matched.status, Status::Matched);
        assert_eq!(matched.compared_steps, 1);
        assert!(!matched.gameplay_accepted);
        match operation {
            Operation::Assignment | Operation::Conversion => {
                replacement.steps[0].output.writes[0].value = Word::binary64(0)
            }
            Operation::Branch => replacement.steps[0].output.successor_scda_offset = Some(37),
            Operation::GetItemCount => {
                replacement.steps[0].output.return_value = Some(Word::binary64(0x4010000000000000))
            }
        }
        let mismatch = compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Default::default(),
        )
        .unwrap();
        assert_eq!(mismatch.status, Status::Mismatched);
        assert_eq!(mismatch.first_difference.unwrap().step, Some(0));
    }
}

#[test]
fn missing_interrupted_empty_and_unobserved_captures_never_match() {
    let (_directory, catalogue, _) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    let original = capture(&manifest, Producer::Original, output());
    let replacement = capture(&manifest, Producer::Replacement, output());
    let blocked = compare(
        &sources,
        &manifest,
        None,
        Some(&replacement),
        Default::default(),
    )
    .unwrap();
    assert_eq!(blocked.status, Status::Blocked);
    assert_eq!(blocked.first_difference.unwrap().field, "missing_capture");
    for finish in [Finish::Interrupted, Finish::Unsupported] {
        let mut capture = original.clone();
        capture.finish = finish;
        assert_eq!(
            compare(
                &sources,
                &manifest,
                Some(&capture),
                Some(&replacement),
                Default::default()
            )
            .unwrap()
            .status,
            Status::Blocked
        );
    }
    let mut empty = replacement.clone();
    empty.steps.clear();
    assert_eq!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&empty),
            Default::default()
        )
        .unwrap()
        .status,
        Status::Mismatched
    );
    let mut empty = replacement;
    empty.steps[0].output.writes.clear();
    assert_eq!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&empty),
            Default::default()
        )
        .unwrap()
        .status,
        Status::Blocked
    );
}

#[test]
fn caller_operand_error_and_order_differences_are_observable() {
    let (_directory, catalogue, _) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    let mut next = manifest.steps[0].clone();
    next.event_ordinal = 1;
    next.operands = vec![Word::binary64(0x7ff8123456789abc)];
    manifest.steps.push(next);
    let original = capture(&manifest, Producer::Original, output());
    let replacement = capture(&manifest, Producer::Replacement, output());
    let mut variants = Vec::new();
    let mut altered = replacement.clone();
    altered.steps[0].input.caller.target = Some(form(0x200));
    variants.push(altered);
    let mut altered = replacement.clone();
    altered.steps[0].input.operands[0] = Word::binary64(0);
    variants.push(altered);
    let mut altered = replacement.clone();
    altered.steps.swap(0, 1);
    variants.push(altered);
    let mut altered = replacement.clone();
    altered.steps[0].output.error = Some("observed_native_error".into());
    variants.push(altered);
    let mut altered = replacement.clone();
    altered.identity.profile_receipt_sha256 = "e".repeat(64);
    variants.push(altered);
    let mut altered = replacement;
    altered.producer_executable_sha256 = "a".repeat(64);
    variants.push(altered);
    for altered in variants {
        assert_eq!(
            compare(
                &sources,
                &manifest,
                Some(&original),
                Some(&altered),
                Default::default()
            )
            .unwrap()
            .status,
            Status::Mismatched
        );
    }
}

#[test]
fn malformed_source_words_schemas_and_exact_limits_have_precise_refusals() {
    let (_directory, catalogue, _) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    let original = capture(&manifest, Producer::Original, output());
    let replacement = capture(&manifest, Producer::Replacement, output());
    let limits = Limits {
        maximum_steps: 1,
        maximum_words: 1,
        maximum_writes: 1,
    };
    assert_eq!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            limits
        )
        .unwrap()
        .status,
        Status::Matched
    );
    assert!(matches!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Limits {
                maximum_steps: 0,
                ..limits
            }
        ),
        Err(Error::Capacity("manifest steps"))
    ));
    assert!(matches!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Limits {
                maximum_words: 0,
                ..limits
            }
        ),
        Err(Error::Capacity("operand words"))
    ));
    assert!(matches!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Limits {
                maximum_writes: 0,
                ..limits
            }
        ),
        Err(Error::Capacity("local writes"))
    ));
    let mut bad = manifest.clone();
    bad.steps[0].scda_offset = 11;
    assert!(matches!(
        validate_manifest(&sources, &bad, limits),
        Err(Error::Source("operation/SCDA position"))
    ));
    let mut bad = manifest.clone();
    bad.steps[0].event_id = 1;
    assert!(matches!(
        validate_manifest(&sources, &bad, limits),
        Err(Error::Source("event block"))
    ));
    let mut bad = manifest.clone();
    bad.steps[0].event_ordinal = 1;
    assert!(matches!(
        validate_manifest(&sources, &bad, limits),
        Err(Error::Invalid(_))
    ));
    let mut bad = manifest.clone();
    bad.identity.compiled_sha256 = "e".repeat(64);
    assert!(matches!(
        validate_manifest(&sources, &bad, limits),
        Err(Error::Source("compiled body"))
    ));
    let mut bad = manifest.clone();
    bad.identity.source_cohort_sha256 = "e".repeat(64);
    assert!(matches!(
        validate_manifest(&sources, &bad, limits),
        Err(Error::Source("source receipt cohort"))
    ));
    let mut bad = replacement.clone();
    bad.steps[0].output.successor_scda_offset = Some(11);
    assert!(matches!(
        compare(&sources, &manifest, Some(&original), Some(&bad), limits),
        Err(Error::Source(
            "observed successor is not an instruction header"
        ))
    ));
    let mut bad = replacement.clone();
    bad.steps[0].input.operands[0].bits = "7FF8123456789ABC".into();
    assert!(matches!(
        compare(&sources, &manifest, None, Some(&bad), limits),
        Err(Error::Invalid(_))
    ));
    let mut value = serde_json::to_value(replacement).unwrap();
    value["unrecognized"] = true.into();
    assert!(serde_json::from_value::<Capture>(value).is_err());
}

#[test]
fn encoded_source_caller_and_assignment_destination_cannot_be_relabelled() {
    let prefixed = [
        vec![0x1c, 0, 1, 0],
        instruction(0x102f, &[1, 0, b'r', 1, 0]),
    ]
    .concat();
    let (_directory, catalogue, _) = fixture(&event(&prefixed));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::GetItemCount,
        10,
    );
    manifest.steps[0].caller.calling_reference = Some(form(0x100));
    let original = capture(
        &manifest,
        Producer::Original,
        StepOutput {
            return_value: Some(Word::binary64(0)),
            successor_scda_offset: None,
            writes: vec![],
            error: None,
        },
    );
    let replacement = capture(
        &manifest,
        Producer::Replacement,
        original.steps[0].output.clone(),
    );
    assert_eq!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Default::default()
        )
        .unwrap()
        .status,
        Status::Matched
    );
    manifest.steps[0].caller.calling_reference = Some(form(0x14));
    let wrong_original = capture(
        &manifest,
        Producer::Original,
        original.steps[0].output.clone(),
    );
    let wrong_replacement = capture(
        &manifest,
        Producer::Replacement,
        original.steps[0].output.clone(),
    );
    assert!(
        compare(
            &sources,
            &manifest,
            Some(&wrong_original),
            Some(&wrong_replacement),
            Default::default()
        )
        .is_err(),
        "An encoded static caller cannot be relabelled as the player"
    );
    let (_directory, catalogue, _) = fixture(&event(&copy()));
    let sources = crate::sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let manifest = crate::manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    let mut wrong = output();
    wrong.writes[0].index = 1;
    let original = capture(&manifest, Producer::Original, wrong.clone());
    let replacement = capture(&manifest, Producer::Replacement, wrong);
    assert!(
        compare(
            &sources,
            &manifest,
            Some(&original),
            Some(&replacement),
            Default::default()
        )
        .is_err(),
        "Both captures naming the wrong local cannot change the encoded destination"
    );
}

#[test]
fn encoded_expression_callers_and_foreign_local_scope_are_not_assumed_from_host_roles() {
    let expression = [
        vec![b'r', 1, 0, b'X', 0x2f, 0x10, 5, 0],
        vec![1, 0, b'r', 1, 0],
    ]
    .concat();
    let payload = [
        vec![b'f', 2, 0],
        (expression.len() as u16).to_le_bytes().to_vec(),
        expression,
    ]
    .concat();
    let (_directory, catalogue, _) = fixture(&event(&instruction(0x15, &payload)));
    let prepared = sources(&catalogue);
    let plan = prepared.get(&definition(&catalogue)).unwrap().plan();
    let mut case = manifest(
        plan,
        prepared.source_cohort_sha256(),
        Operation::GetItemCount,
        22,
    );
    case.steps[0].caller.calling_reference = Some(form(0x100));
    assert!(validate_manifest(&prepared, &case, Default::default()).is_ok());
    case.steps[0].caller.calling_reference = None;
    assert!(validate_manifest(&prepared, &case, Default::default()).is_err());
    // This source's foreign read needs an independently identified external bank;
    // schema1 own-local writes cannot pretend that host caller roles provide it.
    let (_directory, catalogue, _) = fixture(&event(&instruction(
        0x15,
        &[b'f', 2, 0, 6, 0, b'r', 1, 0, b'f', 1, 0],
    )));
    let prepared = sources(&catalogue);
    let plan = prepared.get(&definition(&catalogue)).unwrap().plan();
    let case = manifest(
        plan,
        prepared.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    assert!(validate_manifest(&prepared, &case, Default::default()).is_err());
    let branch = [
        instruction(0x16, &[0, 0, 6, 0, b'r', 1, 0, b'f', 1, 0]),
        instruction(0x19, &[]),
    ]
    .concat();
    let (_directory, catalogue, _) = fixture(&event(&branch));
    let prepared = sources(&catalogue);
    let plan = prepared.get(&definition(&catalogue)).unwrap().plan();
    let case = manifest(plan, prepared.source_cohort_sha256(), Operation::Branch, 10);
    assert!(validate_manifest(&prepared, &case, Default::default()).is_err());
}

#[test]
fn actual_engineering_copy_supplies_replacement_bits_via_canonical_commit() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x56; 16]).unwrap(),
    )
    .unwrap();
    let context = Context::default();
    let handle = world
        .create_instance(
            plan.handle(),
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
                1,
                Value::Number {
                    bits: 0x7ff8123456789abc,
                },
            )],
        )
        .unwrap();
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
    let before = world.snapshot();
    let stage = match world
        .stage_source_local_copy_with_sources(
            sequence,
            &sources,
            &content,
            local_copy::Intent::Engineering,
            Default::default(),
        )
        .unwrap()
    {
        local_copy::Preparation::Staged(stage) => stage,
        other => panic!("{other:?}"),
    };
    assert_eq!(world.snapshot(), before);
    let mut manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    manifest.steps[0].operands = vec![Word::binary64(0x7ff8123456789abc)];
    manifest.steps[0].caller = Caller {
        calling_reference: None,
        containing_reference: None,
        target: None,
        activation: 1,
    };
    assert_eq!(stage.trace().operands.pending.context, Context::default());
    let Value::Number { bits } = stage.trace().copied_value else {
        panic!("not numeric")
    };
    let actual = StepOutput {
        writes: vec![LocalWrite {
            index: stage.trace().destination_index,
            value: Word::binary64(bits),
        }],
        ..output()
    };
    let replacement = capture(&manifest, Producer::Replacement, actual);
    // No original capture is available: a real replacement result remains blocked.
    assert_eq!(
        compare(
            &sources,
            &manifest,
            None,
            Some(&replacement),
            Default::default()
        )
        .unwrap()
        .status,
        Status::Blocked
    );
    let committed = stage.commit(&mut world).unwrap();
    assert_eq!(committed.receipt.assignments, 1);
    assert_eq!(
        world.instance(handle).unwrap().local(2).unwrap(),
        &Value::Number {
            bits: 0x7ff8123456789abc
        }
    );
    assert_eq!(world.pending_events().count(), 0);
}

#[test]
fn engineering_count_without_original_numeric_return_is_not_a_completed_native_observation() {
    let (_directory, catalogue, content) =
        fixture(&event(&instruction(0x102f, &[1, 0, b'r', 1, 0])));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x57; 16]).unwrap(),
    )
    .unwrap();
    let subject = world.register_reference(None).unwrap();
    world.initialize_inventory(subject).unwrap();
    let context = Context::default();
    let handle = world
        .create_instance(
            plan.handle(),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            context.clone(),
        )
        .unwrap();
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
    let before = world.snapshot();
    let calls = world
        .prepare_native_calls_with_sources(sequence, &sources, Default::default())
        .unwrap();
    let observation = calls
        .observe(
            0,
            &content,
            native::Inputs {
                supplied_subject: Some(subject),
                player: None,
            },
            native::Intent::EngineeringObservation,
            10,
        )
        .unwrap();
    let native::Outcome::EngineeringObservation { trace } = observation.outcome else {
        panic!("missing host observation")
    };
    assert_eq!(trace.query.result, 0);
    assert!(trace.original_numeric_return.is_none());
    assert_eq!(world.snapshot(), before);
    let manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::GetItemCount,
        10,
    );
    let original = capture(
        &manifest,
        Producer::Original,
        StepOutput {
            return_value: Some(Word::binary64(0)),
            writes: vec![],
            ..output()
        },
    );
    let replacement = capture(
        &manifest,
        Producer::Replacement,
        StepOutput {
            writes: vec![],
            ..output()
        },
    );
    let result = compare(
        &sources,
        &manifest,
        Some(&original),
        Some(&replacement),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.status, Status::Blocked);
    assert_eq!(
        result.first_difference.unwrap().field,
        "missing_observation"
    );
}

fn copy_request() -> copy_probe::Request {
    copy_probe::Request {
        schema_version: 1,
        campaign: CampaignId::from_bytes([0x31; 16]).unwrap(),
        activation: 1.try_into().unwrap(),
        initializers: vec![
            copy_probe::Initializer {
                index: 1,
                value: Value::Number {
                    bits: 0x8000000000000000,
                },
            },
            copy_probe::Initializer {
                index: 2,
                value: Value::Number {
                    bits: 0xc010000000000000,
                },
            },
        ],
    }
}
fn authored_request(purpose: Operation) -> authored::Request {
    authored::Request {
        schema_version: 1,
        purpose,
        campaign: CampaignId::from_bytes([0x31; 16]).unwrap(),
        activation: 1.try_into().unwrap(),
        input: Value::Number {
            bits: 0x8000000000000000,
        },
        destination_before: Value::Number {
            bits: 0xc010000000000000,
        },
    }
}
#[test]
fn authored_fixtures_round_trip_through_existing_store_plans_and_canonical_copy() {
    for purpose in [Operation::Assignment, Operation::Conversion] {
        let request = authored_request(purpose);
        let artifact = authored::generate(&request, 4096).unwrap();
        assert_eq!(artifact.compiled.len(), 30);
        assert_eq!(&artifact.compiled[..4], &[0x1d, 0, 0, 0]);
        assert_eq!(
            artifact.compiled[18],
            if purpose == Operation::Conversion {
                b's'
            } else {
                b'f'
            }
        );
        assert_eq!(
            artifact.plugin_sha256,
            format!("{:x}", Sha256::digest(&artifact.plugin))
        );
        assert!(!artifact.shape.original_expected_output_generated);
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join(authored::PLUGIN_NAME),
            &artifact.plugin,
        )
        .unwrap();
        let mut store = fallout_data::store::RecordStore::open_nv_headers(
            directory.path(),
            &[authored::PLUGIN_NAME.into()],
            Default::default(),
        )
        .unwrap();
        let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
        assert_eq!(catalogue.iter().count(), 1);
        let source = catalogue.iter().next().unwrap().1;
        assert_eq!(source.handle().key.record.local_id, authored::SCRIPT_ID);
        assert!(source.issues().is_empty() && source.references().is_empty());
        assert_eq!(source.declarations().len(), 2);
        let content = Content::load(&mut store, &catalogue, 10).unwrap();
        let sources = sources(&catalogue);
        let plan = sources.get(source.handle()).unwrap().plan();
        let mut case = manifest(plan, sources.source_cohort_sha256(), purpose, 14);
        case.steps[0].begin_scda_offset = 4;
        case.steps[0].caller = artifact.shape.caller;
        case.steps[0].operands = artifact.shape.operand_bits;
        validate_manifest(&sources, &case, Default::default()).unwrap();
        let observed = copy_probe::observe(
            &sources,
            &content,
            &case,
            &artifact.copy_request,
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        )
        .unwrap();
        if purpose == Operation::Assignment {
            assert_eq!(observed.capture.finish, Finish::Completed);
            assert_eq!(observed.committed.len(), 1);
            assert_eq!(
                observed.capture.steps[0].output.successor_scda_offset,
                Some(26)
            );
            assert_eq!(
                observed.capture.steps[0].output.writes[0].value,
                Word::binary64(0x8000000000000000)
            );
        } else {
            assert_eq!(observed.capture.finish, Finish::Unsupported);
            assert!(observed.committed.is_empty());
            assert_eq!(observed.initial_snapshot, observed.final_snapshot);
        }
    }
}
#[test]
fn authored_fixture_limits_inputs_and_truncation_never_generate_original_expectations() {
    let mut request = authored_request(Operation::Assignment);
    let artifact = authored::generate(&request, 4096).unwrap();
    assert!(authored::generate(&request, artifact.plugin.len()).is_ok());
    assert!(matches!(
        authored::generate(&request, artifact.plugin.len() - 1),
        Err(authored::Error::Capacity)
    ));
    for cut in 0..artifact.plugin.len() {
        let mut scripts = 0;
        let result = fallout_data::plugin::visit(
            &mut std::io::Cursor::new(&artifact.plugin[..cut]),
            cut as u64,
            authored::PLUGIN_NAME,
            Default::default(),
            |event| {
                if let fallout_data::plugin::Event::Record(record) = event
                    && record.header.kind == *b"SCPT"
                {
                    scripts += fallout_data::script_units::decode(
                        record,
                        authored::PLUGIN_NAME,
                        Default::default(),
                    )?
                    .len();
                }
                Ok(())
            },
        );
        assert!(
            result.is_err() || scripts == 0,
            "truncated at {cut} appears complete"
        );
    }
    for purpose in [Operation::Branch, Operation::GetItemCount] {
        request.purpose = purpose;
        assert!(matches!(
            authored::generate(&request, 4096),
            Err(authored::Error::Invalid(_))
        ));
    }
    request.purpose = Operation::Assignment;
    for bits in [f64::INFINITY.to_bits(), f64::NAN.to_bits()] {
        request.input = Value::Number { bits };
        assert!(matches!(
            authored::generate(&request, 4096),
            Err(authored::Error::Invalid(_))
        ));
    }
    assert!(
        serde_json::from_value::<authored::Request>(serde_json::json!({
        "schema_version":1,"purpose":"assignment","campaign":([49_u8;16]),"activation":1,
        "input":{"kind":"number","bits":0},"destination_before":{"kind":"number","bits":0},
        "expected_original_output":0}))
        .is_err()
    );
}
fn own_case(plan: &fallout_data::obscript::definition_plan::Plan<'_>, cohort: &str) -> Manifest {
    let mut case = manifest(plan, cohort, Operation::Assignment, 10);
    case.steps[0].caller = Caller {
        calling_reference: None,
        containing_reference: None,
        target: None,
        activation: 1,
    };
    case.steps[0].operands = vec![Word::binary64(0x8000000000000000)];
    case
}

#[test]
fn standalone_copy_producer_commits_real_ordered_bits_and_restores_the_canonical_snapshot() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut case = own_case(plan, sources.source_cohort_sha256());
    let mut repeated = case.steps[0].clone();
    repeated.event_ordinal = 1;
    case.steps.push(repeated);
    let observed = copy_probe::observe(
        &sources,
        &content,
        &case,
        &copy_request(),
        &"c".repeat(64),
        &"d".repeat(64),
        copy_probe::Limits {
            maximum_steps: 2,
            maximum_initializers: 2,
        },
    )
    .unwrap();
    assert!(observed.unsupported.is_none());
    assert_eq!(observed.capture.finish, Finish::Completed);
    assert_eq!(observed.capture.producer, Producer::Replacement);
    assert_eq!(observed.committed.len(), 2);
    assert_eq!(observed.capture.steps.len(), 2);
    assert!(observed.canonical_restore_verified);
    assert!(!observed.faithful_execution_admitted);
    for (ordinal, step) in observed.capture.steps.iter().enumerate() {
        assert_eq!(step.input.event_ordinal, ordinal as u32);
        assert_eq!(
            step.output.writes,
            vec![LocalWrite {
                index: 2,
                value: Word::binary64(0x8000000000000000)
            }]
        );
        assert_eq!(step.output.successor_scda_offset, Some(22));
        assert!(!observed.committed[ordinal].trace.original_behavior_verified);
    }
    assert!(observed.final_snapshot.pending_events.is_empty());
    assert_eq!(observed.final_snapshot.instances.len(), 1);
    assert_eq!(
        observed.final_snapshot.instances[0]
            .locals
            .iter()
            .find(|local| local.index == 2)
            .unwrap()
            .value,
        Value::Number {
            bits: 0x8000000000000000
        }
    );
    assert_ne!(observed.initial_snapshot, observed.final_snapshot);
    assert_eq!(
        compare(
            &sources,
            &case,
            None,
            Some(&observed.capture),
            Default::default()
        )
        .unwrap()
        .status,
        Status::Blocked
    );
}

#[test]
fn standalone_copy_refuses_duplicate_skipped_and_reversed_event_ordinals_atomically() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    for ordinals in [[0, 0], [0, 2], [1, 0], [1, 2]] {
        let mut case = own_case(plan, sources.source_cohort_sha256());
        case.steps.push(case.steps[0].clone());
        for (input, ordinal) in case.steps.iter_mut().zip(ordinals) {
            input.event_ordinal = ordinal;
        }
        if ordinals == [0, 0] {
            assert_eq!(
                compare(
                    &sources,
                    &case,
                    Some(&capture(&case, Producer::Original, output())),
                    Some(&capture(&case, Producer::Replacement, output())),
                    Default::default()
                )
                .unwrap()
                .status,
                Status::Matched
            );
        }
        let result = copy_probe::observe(
            &sources,
            &content,
            &case,
            &copy_request(),
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        );
        // The general trace schema already refuses skipped/reversed events.
        // A grouped same-event trace is valid to import, but cannot be replayed
        // as multiple complete events by this narrower producer.
        if ordinals != [0, 0] {
            assert!(matches!(result, Err(copy_probe::Error::Trace(_))));
            continue;
        }
        let observed = result.unwrap();
        assert_eq!(observed.capture.finish, Finish::Unsupported, "{ordinals:?}");
        let unsupported = observed.unsupported.unwrap();
        assert_eq!(unsupported.step, Some(if ordinals[0] == 0 { 1 } else { 0 }));
        assert!(
            unsupported
                .detail
                .contains("consecutive distinct event ordinals")
        );
        assert!(observed.capture.steps.is_empty() && observed.committed.is_empty());
        assert_eq!(observed.initial_snapshot, observed.final_snapshot);
        assert!(observed.final_snapshot.pending_events.is_empty());
    }
}

#[test]
fn standalone_copy_producer_refuses_unknown_scopes_conversion_branches_and_wrong_operand_bits() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let case = own_case(plan, sources.source_cohort_sha256());
    let mut cases = Vec::new();
    let mut wrong = case.clone();
    wrong.steps[0].caller.calling_reference = Some(form(0x14));
    cases.push(wrong);
    let mut wrong = case.clone();
    wrong.steps[0].caller.activation = 2;
    cases.push(wrong);
    let mut wrong = case.clone();
    wrong.steps[0].operands[0] = Word::binary64(0);
    cases.push(wrong);
    let mut wrong = case;
    wrong.purpose = Operation::Conversion;
    wrong.steps[0].operation = Operation::Conversion;
    cases.push(wrong);
    for case in cases {
        let result = copy_probe::observe(
            &sources,
            &content,
            &case,
            &copy_request(),
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default(),
        )
        .unwrap();
        assert!(result.unsupported.is_some());
        assert_eq!(result.capture.finish, Finish::Unsupported);
        assert!(result.capture.steps.is_empty() && result.committed.is_empty());
        assert_eq!(result.initial_snapshot, result.final_snapshot);
    }
    let branch = [
        instruction(0x16, &[0, 0, 3, 0, b'f', 1, 0]),
        instruction(0x19, &[]),
    ]
    .concat();
    let (_directory, catalogue, content) = fixture(&event(&branch));
    let sources = crate::sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut case = manifest(plan, sources.source_cohort_sha256(), Operation::Branch, 10);
    case.steps[0].caller = Caller {
        calling_reference: None,
        containing_reference: None,
        target: None,
        activation: 1,
    };
    let result = copy_probe::observe(
        &sources,
        &content,
        &case,
        &copy_request(),
        &"c".repeat(64),
        &"d".repeat(64),
        Default::default(),
    )
    .unwrap();
    assert_eq!(result.capture.finish, Finish::Unsupported);
    assert_eq!(result.initial_snapshot, result.final_snapshot);
}

#[test]
fn standalone_copy_inputs_have_exact_limits_and_finite_explicit_numeric_slots() {
    let (_directory, catalogue, content) = fixture(&event(&copy()));
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let case = own_case(plan, sources.source_cohort_sha256());
    for limits in [
        copy_probe::Limits {
            maximum_steps: 0,
            maximum_initializers: 2,
        },
        copy_probe::Limits {
            maximum_steps: 1,
            maximum_initializers: 1,
        },
    ] {
        assert!(matches!(
            copy_probe::observe(
                &sources,
                &content,
                &case,
                &copy_request(),
                &"c".repeat(64),
                &"d".repeat(64),
                limits
            ),
            Err(copy_probe::Error::Capacity(_))
        ));
    }
    for bits in [0x7ff8000000000001, 0x7ff0000000000000, 0xfff0000000000000] {
        let mut request = copy_request();
        request.initializers[0].value = Value::Number { bits };
        assert!(matches!(
            copy_probe::observe(
                &sources,
                &content,
                &case,
                &request,
                &"c".repeat(64),
                &"d".repeat(64),
                Default::default()
            ),
            Err(copy_probe::Error::Input("initializer must be finite"))
        ));
    }
    let mut request = copy_request();
    request.initializers[1].index = 1;
    assert!(matches!(
        copy_probe::observe(
            &sources,
            &content,
            &case,
            &request,
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default()
        ),
        Err(copy_probe::Error::Input("duplicate local initializer"))
    ));
    let mut request = copy_request();
    request.initializers[0].value = Value::Uninitialized;
    assert!(matches!(
        copy_probe::observe(
            &sources,
            &content,
            &case,
            &request,
            &"c".repeat(64),
            &"d".repeat(64),
            Default::default()
        ),
        Err(copy_probe::Error::Input(_))
    ));
    assert!(matches!(
        copy_probe::observe(
            &sources,
            &content,
            &case,
            &copy_request(),
            &"a".repeat(64),
            &"d".repeat(64),
            Default::default()
        ),
        Err(copy_probe::Error::Input(
            "replacement producer cannot be the original executable"
        ))
    ));
    assert!(matches!(
        copy_probe::observe(
            &sources,
            &content,
            &case,
            &copy_request(),
            &"C".repeat(64),
            &"d".repeat(64),
            Default::default()
        ),
        Err(copy_probe::Error::Trace(_))
    ));
    assert!(matches!(
        copy_probe::observe(
            &sources,
            &content,
            &case,
            &copy_request(),
            &"c".repeat(64),
            "missing",
            Default::default()
        ),
        Err(copy_probe::Error::Trace(_))
    ));
}

#[test]
fn standalone_copy_discards_an_earlier_preview_effect_when_a_later_source_operation_is_unmeasured()
{
    let body = [
        event(&copy()),
        event(&instruction(0x15, &[b's', 2, 0, 1, 0, b'1'])),
    ]
    .concat();
    let (_directory, catalogue, content) = fixture(&body);
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut case = own_case(plan, sources.source_cohort_sha256());
    let mut later = case.steps[0].clone();
    later.event_ordinal = 1;
    later.begin_scda_offset = 26;
    later.scda_offset = 36;
    later.operands = vec![Word::binary64(0x3ff0000000000000)];
    case.steps.push(later);
    let result = copy_probe::observe(
        &sources,
        &content,
        &case,
        &copy_request(),
        &"c".repeat(64),
        &"d".repeat(64),
        Default::default(),
    )
    .unwrap();
    let unsupported = result.unsupported.unwrap();
    assert_eq!(unsupported.step, Some(1));
    assert!(unsupported.detail.contains("ExpressionShape"));
    assert_eq!(result.capture.finish, Finish::Unsupported);
    assert!(result.capture.steps.is_empty() && result.committed.is_empty());
    assert_eq!(result.initial_snapshot, result.final_snapshot);
    assert!(result.final_snapshot.pending_events.is_empty());
    assert_eq!(
        result.final_snapshot.instances[0]
            .locals
            .iter()
            .find(|local| local.index == 2)
            .unwrap()
            .value,
        Value::Number {
            bits: 0xc010000000000000
        }
    );
}

#[test]
#[ignore = "requires built CLI and read-only retail metadata; authored source only"]
fn cli_fixture_generation_helper() {
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let executable = std::path::PathBuf::from(
        std::env::var_os("RF_SCRIPT_TRACE_RETAIL_EXE").expect("retail metadata"),
    );
    let input =
        std::path::PathBuf::from(std::env::var_os("RF_SCRIPT_COPY_INPUT").expect("profile input"));
    let evidence =
        std::path::PathBuf::from(std::env::var_os("RF_SCRIPT_FIXTURE_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    for purpose in ["assignment", "conversion"] {
        let request = serde_json::json!({"schema_version":1,"purpose":purpose,
            "campaign":([49_u8;16]),"activation":1,
            "input":{"kind":"number","bits":0x8000000000000000_u64},
            "destination_before":{"kind":"number","bits":0xc010000000000000_u64}});
        let request_path = evidence.join(format!("{purpose}-request.json"));
        fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
        let destination = evidence.join(purpose);
        let invoke = || {
            std::process::Command::new(&cli)
                .args(["script-fixture", "--install"])
                .arg(executable.parent().unwrap())
                .arg("--request")
                .arg(&request_path)
                .arg("--profile-receipt")
                .arg(input.join("profile-receipt.txt"))
                .arg("--destination")
                .arg(&destination)
                .output()
                .unwrap()
        };
        let generated = invoke();
        fs::write(
            evidence.join(format!("{purpose}-generate-stdout.txt")),
            &generated.stdout,
        )
        .unwrap();
        fs::write(
            evidence.join(format!("{purpose}-generate-stderr.txt")),
            &generated.stderr,
        )
        .unwrap();
        assert!(
            generated.status.success(),
            "{}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let receipt_path = destination.join("receipt.json");
        let receipt_bytes = fs::read(&receipt_path).unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
        assert_eq!(receipt["original_expected_output_generated"], false);
        assert_eq!(receipt["retail_loading_verified"], false);
        assert_eq!(receipt["faithful_execution_admitted"], false);
        assert_eq!(receipt["identity"]["compiled_bytes"], 30);
        assert_eq!(
            receipt["plugin_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    fs::read(
                        destination
                            .join("authored-source-copy/Data")
                            .join(authored::PLUGIN_NAME)
                    )
                    .unwrap()
                )
            )
        );
        let repeated = invoke();
        assert!(!repeated.status.success());
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt_bytes);
        fs::write(
            evidence.join(format!("{purpose}-existing-destination-stderr.txt")),
            &repeated.stderr,
        )
        .unwrap();
        let output_path = evidence.join(format!("{purpose}-copy-report.json"));
        let output = std::process::Command::new(&cli)
            .args(["script-trace", "--install"])
            .arg(destination.join("authored-source-copy"))
            .arg("--load-order")
            .arg(destination.join("order.json"))
            .arg("--manifest")
            .arg(destination.join("manifest.json"))
            .arg("--profile-receipt")
            .arg(destination.join("profile-receipt.json"))
            .arg("--replacement-copy")
            .arg(destination.join("copy-request.json"))
            .arg("--output")
            .arg(&output_path)
            .output()
            .unwrap();
        fs::write(
            evidence.join(format!("{purpose}-copy-stderr.txt")),
            &output.stderr,
        )
        .unwrap();
        fs::write(
            evidence.join(format!("{purpose}-copy-stdout.txt")),
            &output.stdout,
        )
        .unwrap();
        assert!(!output.status.success()); // No independent original capture.
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(output_path).unwrap()).unwrap();
        assert_eq!(report["comparison"]["status"], "blocked");
        let observation = &report["replacement_observation"];
        if purpose == "assignment" {
            assert_eq!(observation["capture"]["finish"], "completed");
            assert_eq!(observation["committed"].as_array().unwrap().len(), 1);
            assert_eq!(
                observation["capture"]["steps"][0]["input"]["scda_offset"],
                14
            );
            assert_eq!(
                observation["capture"]["steps"][0]["output"]["writes"][0]["value"]["bits"],
                "8000000000000000"
            );
        } else {
            assert_eq!(observation["capture"]["finish"], "unsupported");
            assert!(observation["committed"].as_array().unwrap().is_empty());
            assert_eq!(
                observation["initial_snapshot"],
                observation["final_snapshot"]
            );
        }
    }
}

#[test]
#[ignore = "requires a built standalone CLI and prior authored source evidence, no retail launch"]
fn cli_copy_producer_helper() {
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = std::path::PathBuf::from(std::env::var_os("RF_SCRIPT_COPY_INPUT").expect("input"));
    let evidence =
        std::path::PathBuf::from(std::env::var_os("RF_SCRIPT_COPY_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let mut case: Manifest =
        serde_json::from_slice(&fs::read(input.join("manifest.json")).unwrap()).unwrap();
    case.steps[0].caller = Caller {
        calling_reference: None,
        containing_reference: None,
        target: None,
        activation: 1,
    };
    case.steps[0].operands = vec![Word::binary64(0x8000000000000000)];
    let case_path = evidence.join("manifest.json");
    fs::write(&case_path, serde_json::to_vec_pretty(&case).unwrap()).unwrap();
    let request = serde_json::json!({"schema_version":1,"campaign":([49_u8;16]),"activation":1,"initializers":[
        {"index":1,"value":{"kind":"number","bits":0x8000000000000000_u64}},
        {"index":2,"value":{"kind":"number","bits":0xc010000000000000_u64}}]});
    let request_path = evidence.join("copy-request.json");
    fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
    let report_path = evidence.join("report.json");
    let output = std::process::Command::new(&cli)
        .args(["script-trace", "--install"])
        .arg(input.join("authored-source-copy"))
        .arg("--load-order")
        .arg(input.join("order.json"))
        .arg("--manifest")
        .arg(&case_path)
        .arg("--profile-receipt")
        .arg(input.join("profile-receipt.txt"))
        .arg("--replacement-copy")
        .arg(&request_path)
        .arg("--output")
        .arg(&report_path)
        .output()
        .unwrap();
    fs::write(evidence.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(evidence.join("stderr.txt"), &output.stderr).unwrap();
    assert!(!output.status.success()); // Original is absent even though the copy ran.
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
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
    assert_eq!(
        report["replacement_observation"]["capture"]["steps"][0]["output"]["writes"][0]["value"]["bits"],
        "8000000000000000"
    );
    assert_eq!(
        report["replacement_observation"]["canonical_restore_verified"],
        true
    );
    assert_eq!(report["retail_execution_performed"], false);
    assert_eq!(report["faithful_execution_admitted"], false);
    let producer: &str = report["replacement_observation"]["capture"]["producer_executable_sha256"]
        .as_str()
        .unwrap();
    assert_eq!(
        producer,
        format!("{:x}", Sha256::digest(fs::read(&cli).unwrap()))
    );
    fs::write(
        evidence.join("replacement-capture.json"),
        serde_json::to_vec_pretty(&report["replacement_observation"]["capture"]).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("canonical-snapshot.json"),
        serde_json::to_vec_pretty(&report["replacement_observation"]["final_snapshot"]).unwrap(),
    )
    .unwrap();
    for (name, ordinals) in [
        ("duplicate", [0, 0]),
        ("skipped", [0, 2]),
        ("reversed", [1, 0]),
        ("ordered", [0, 1]),
    ] {
        let mut two_events = case.clone();
        two_events.steps.push(two_events.steps[0].clone());
        for (input, ordinal) in two_events.steps.iter_mut().zip(ordinals) {
            input.event_ordinal = ordinal;
        }
        let manifest_path = evidence.join(format!("{name}-manifest.json"));
        let report_path = evidence.join(format!("{name}-report.json"));
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&two_events).unwrap(),
        )
        .unwrap();
        let output = std::process::Command::new(&cli)
            .args(["script-trace", "--install"])
            .arg(input.join("authored-source-copy"))
            .arg("--load-order")
            .arg(input.join("order.json"))
            .arg("--manifest")
            .arg(&manifest_path)
            .arg("--profile-receipt")
            .arg(input.join("profile-receipt.txt"))
            .arg("--replacement-copy")
            .arg(&request_path)
            .arg("--output")
            .arg(&report_path)
            .output()
            .unwrap();
        fs::write(evidence.join(format!("{name}-stdout.txt")), &output.stdout).unwrap();
        fs::write(evidence.join(format!("{name}-stderr.txt")), &output.stderr).unwrap();
        assert!(!output.status.success()); // Original capture is always absent.
        if name == "skipped" || name == "reversed" {
            assert!(!report_path.exists()); // Invalid manifest, no execution.
            assert!(String::from_utf8_lossy(&output.stderr).contains("event ordinals"));
            continue;
        }
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
        let observation = &report["replacement_observation"];
        if name == "duplicate" {
            assert_eq!(observation["capture"]["finish"], "unsupported");
            assert_eq!(observation["unsupported"]["step"], 1);
            assert!(observation["committed"].as_array().unwrap().is_empty());
            assert!(
                observation["capture"]["steps"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                observation["initial_snapshot"],
                observation["final_snapshot"]
            );
        } else {
            assert_eq!(observation["capture"]["finish"], "completed");
            assert_eq!(observation["committed"].as_array().unwrap().len(), 2);
            for (index, step) in observation["capture"]["steps"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                assert_eq!(step["input"]["event_ordinal"], index);
            }
        }
    }
}

#[test]
#[ignore = "requires an explicitly supplied built CLI and read-only retail metadata executable"]
fn cli_import_helper() {
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("RF_SCRIPT_TRACE_CLI");
    let executable =
        std::env::var_os("RF_SCRIPT_TRACE_RETAIL_EXE").expect("RF_SCRIPT_TRACE_RETAIL_EXE");
    let evidence = std::path::PathBuf::from(
        std::env::var_os("RF_SCRIPT_TRACE_EVIDENCE").expect("RF_SCRIPT_TRACE_EVIDENCE"),
    );
    fs::create_dir(&evidence).unwrap();
    let executable_bytes = fs::read(&executable).unwrap();
    let executable_sha = format!("{:x}", Sha256::digest(&executable_bytes));
    assert_eq!(
        executable_sha,
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::write(install.join("FalloutNV.exe"), executable_bytes).unwrap();
    let (directory, catalogue, _) = fixture(&event(&copy()));
    fs::copy(
        directory.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    let sources = sources(&catalogue);
    let plan = sources.get(&definition(&catalogue)).unwrap().plan();
    let mut manifest = manifest(
        plan,
        sources.source_cohort_sha256(),
        Operation::Assignment,
        10,
    );
    manifest.identity.executable_sha256 = executable_sha.clone();
    let profile_receipt =
        b"Synthetic read-only CLI import fixture. No original execution or isolation proof.\n";
    manifest.identity.profile_receipt_sha256 = format!("{:x}", Sha256::digest(profile_receipt));
    let mut original = capture(&manifest, Producer::Original, output());
    original.producer_executable_sha256 = executable_sha;
    let replacement = capture(&manifest, Producer::Replacement, output());
    fs::write(evidence.join("profile-receipt.txt"), profile_receipt).unwrap();
    fs::write(evidence.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        evidence.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("original-synthetic.json"),
        serde_json::to_vec_pretty(&original).unwrap(),
    )
    .unwrap();
    fs::write(
        evidence.join("replacement.json"),
        serde_json::to_vec_pretty(&replacement).unwrap(),
    )
    .unwrap();
    let invoke = |name: &str, original: bool, replacement: &str| {
        let mut command = std::process::Command::new(&cli);
        command
            .args(["script-trace", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(evidence.join("order.json"))
            .arg("--manifest")
            .arg(evidence.join("manifest.json"))
            .arg("--profile-receipt")
            .arg(evidence.join("profile-receipt.txt"))
            .arg("--replacement-trace")
            .arg(evidence.join(replacement))
            .arg("--output")
            .arg(evidence.join(format!("{name}.json")));
        if original {
            command
                .arg("--original-trace")
                .arg(evidence.join("original-synthetic.json"));
        }
        let output = command.output().unwrap();
        fs::write(evidence.join(format!("{name}.stdout")), &output.stdout).unwrap();
        fs::write(evidence.join(format!("{name}.stderr")), &output.stderr).unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(evidence.join(format!("{name}.json"))).unwrap())
                .unwrap();
        assert_eq!(report["retail_execution_performed"], false);
        assert_eq!(report["faithful_execution_admitted"], false);
        assert_eq!(report["comparison"]["gameplay_accepted"], false);
        (output.status.success(), report)
    };
    let (success, report) = invoke("missing-original", false, "replacement.json");
    assert!(!success);
    assert_eq!(report["comparison"]["status"], "blocked");
    let (success, report) = invoke("matching-synthetic", true, "replacement.json");
    assert!(success);
    assert_eq!(report["comparison"]["status"], "matched");
    let mut altered = replacement.clone();
    altered.steps[0].output.writes[0].value = Word::binary64(0);
    fs::write(
        evidence.join("altered-replacement.json"),
        serde_json::to_vec_pretty(&altered).unwrap(),
    )
    .unwrap();
    let (success, report) = invoke("altered-write", true, "altered-replacement.json");
    assert!(!success);
    assert_eq!(report["comparison"]["status"], "mismatched");
    assert_eq!(
        report["comparison"]["first_difference"]["field"],
        "local_writes"
    );
    let mut wrong_scope = replacement;
    wrong_scope.steps[0].output.writes[0].index = 1;
    let wrong_path = evidence.join("wrong-source-destination.json");
    fs::write(
        &wrong_path,
        serde_json::to_vec_pretty(&wrong_scope).unwrap(),
    )
    .unwrap();
    let report_path = evidence.join("wrong-source-destination-report.json");
    let output = std::process::Command::new(cli)
        .args(["script-trace", "--install"])
        .arg(&install)
        .arg("--load-order")
        .arg(evidence.join("order.json"))
        .arg("--manifest")
        .arg(evidence.join("manifest.json"))
        .arg("--profile-receipt")
        .arg(evidence.join("profile-receipt.txt"))
        .arg("--original-trace")
        .arg(evidence.join("original-synthetic.json"))
        .arg("--replacement-trace")
        .arg(wrong_path)
        .arg("--output")
        .arg(&report_path)
        .output()
        .unwrap();
    fs::write(
        evidence.join("wrong-source-destination.stderr"),
        &output.stderr,
    )
    .unwrap();
    assert!(!output.status.success());
    assert!(!report_path.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source destination"));
}
