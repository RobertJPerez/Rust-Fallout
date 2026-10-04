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
    execution::{local_copy, native, trace::*},
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
