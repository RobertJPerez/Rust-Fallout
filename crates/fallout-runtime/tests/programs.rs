mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::{CommandSignature, Signatures},
        arguments::{Convention, Parameter},
        definition_plan,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
};
use fallout_runtime::{
    World, event_operands,
    events::{Context, Trigger},
    foreign::Content,
    identity::{Owner, Value},
    preparation,
    programs::{self, LookupError, PreparationJob, PreparationStatus, PreparedSources, StepBudget},
};
use std::{fs, sync::Arc};

fn operators() -> Operators {
    Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(code, spelling)| Operator {
            code: code as u32,
            precedence: code as u8,
            spelling: spelling.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap()
}
fn instruction(body: &mut Vec<u8>, opcode: u16, operands: &[u8]) {
    body.extend(opcode.to_le_bytes());
    body.extend((operands.len() as u16).to_le_bytes());
    body.extend(operands);
}
fn event(middle: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    instruction(
        &mut body,
        0x10,
        &[
            0_u16.to_le_bytes().as_slice(),
            &((middle.len() + 4) as u32).to_le_bytes(),
        ]
        .concat(),
    );
    body.extend(middle);
    instruction(&mut body, 0x11, &[]);
    body
}
fn assignment(expression: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    instruction(
        &mut body,
        0x15,
        &[
            &[b's', 2, 0],
            (expression.len() as u16).to_le_bytes().as_slice(),
            expression,
        ]
        .concat(),
    );
    body
}
fn script(body: Option<&[u8]>, target: Option<u32>, bad_count: bool) -> Vec<u8> {
    let mut header = [0; 20];
    header[4..8]
        .copy_from_slice(&(u32::from(target.is_some()) + u32::from(bad_count)).to_le_bytes());
    header[8..12].copy_from_slice(&(body.map_or(0, <[u8]>::len) as u32).to_le_bytes());
    header[12..16].copy_from_slice(&1_u32.to_le_bytes());
    let mut declaration = [0; 24];
    declaration[..4].copy_from_slice(&2_u32.to_le_bytes());
    declaration[16] = 1;
    let mut out = field(b"SCHR", &header);
    if let Some(body) = body {
        if body.len() > u16::MAX as usize {
            out.extend(field(b"XXXX", &(body.len() as u32).to_le_bytes()));
            out.extend([b"SCDA".as_slice(), &0_u16.to_le_bytes(), body].concat());
        } else {
            out.extend(field(b"SCDA", body));
        }
    }
    out.extend(field(b"SLSD", &declaration));
    out.extend(field(b"SCVR", b"counter\0"));
    if let Some(target) = target {
        out.extend(field(b"SCRO", &target.to_le_bytes()));
    }
    out
}
fn fixture(body: &[u8], extras: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &script(Some(body), None, false)),
            extras.to_vec(),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = Arc::new(load(directory.path(), &["FalloutNV.esm"]));
    (directory, catalogue)
}
fn cache(
    catalogue: &Catalogue,
    limits: programs::Limits,
) -> Result<PreparedSources<'_>, programs::Error> {
    let operators = operators();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Signatures::new(),
        limits,
    )
}
fn seed(
    catalogue: Arc<Catalogue>,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let source = definition(&catalogue);
    let mut world = World::new(catalogue, Default::default()).unwrap();
    let instance = world
        .create_instance(
            &source,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let sequence = world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, instance, sequence)
}

#[test]
fn repeated_frames_share_the_prepared_plan_and_preserve_the_fresh_projection() {
    let body = event(&assignment(&[b's', 2, 0]));
    let (_directory, catalogue) = fixture(&body, &[]);
    let sources = cache(&catalogue, Default::default()).unwrap();
    let counts = sources.counts().clone();
    let handle = definition(&catalogue);
    let prepared = sources.get(&handle).unwrap();
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let fresh = world
        .prepare_event(sequence, &model, &Signatures::new(), Default::default())
        .unwrap();
    for _ in 0..100 {
        let frame = world
            .prepare_event_with_sources(sequence, &sources, 3)
            .unwrap();
        assert!(std::ptr::eq(frame.source(), prepared.plan()));
        assert_eq!(frame.pending(), fresh.pending());
        assert_eq!(frame.selected(), fresh.selected());
        assert_eq!(frame.instructions(), fresh.instructions());
        assert_eq!(frame.binding_sha256(), fresh.binding_sha256());
    }
    assert_eq!(counts.preparation_attempts, 1);
    assert_eq!(sources.counts(), &counts);
    assert_eq!(world.snapshot(), before);
    assert!(matches!(
        world.prepare_event_with_sources(sequence, &sources, 2),
        Err(preparation::Error::Capacity)
    ));
    assert!(matches!(
        world.prepare_event_with_sources(sequence + 1, &sources, 3),
        Err(preparation::Error::MissingPending(_))
    ));
    let mut forged = handle;
    forged.version_sha256 = "0".repeat(64);
    assert!(matches!(
        sources.get(&forged),
        Err(LookupError::DefinitionChanged)
    ));
}

#[test]
fn cached_probes_resolve_new_live_values_and_restoration_does_not_rebuild_sources() {
    let body = event(&assignment(&[b's', 2, 0]));
    let (directory, catalogue) = fixture(&body, &[]);
    let sources = cache(&catalogue, Default::default()).unwrap();
    let mut store = fallout_data::store::RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    let (mut world, instance, sequence) = seed(Arc::clone(&catalogue));
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    for bits in [0, u64::MAX, 0x7ff8_1234_5678_9abc] {
        world
            .assign(instance, &[(2, Value::Number { bits })])
            .unwrap();
        let before = world.snapshot();
        let cached = world
            .probe_event_operands_with_sources(
                sequence,
                &sources,
                &content,
                None,
                Default::default(),
            )
            .unwrap();
        let fresh = world
            .probe_event_operands(
                sequence,
                &model,
                &Signatures::new(),
                &content,
                None,
                Default::default(),
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&cached).unwrap(),
            serde_json::to_value(fresh).unwrap()
        );
        assert!(
            matches!(cached.operands[1].outcome, event_operands::Outcome::Resolved {
            resolution: event_operands::Resolution::Local { value: Some(Value::Number { bits: actual }), .. }, ..
        } if actual == bits)
        );
        assert_eq!(world.snapshot(), before);
    }
    let restored =
        World::restore(Arc::clone(&catalogue), world.snapshot(), Default::default()).unwrap();
    assert!(matches!(
        restored.instance(instance),
        Err(fallout_runtime::Error::StaleHandle)
    ));
    let cached = restored
        .probe_event_operands_with_sources(sequence, &sources, &content, None, Default::default())
        .unwrap();
    assert_eq!(cached.state_revision, world.revision());
    assert_eq!(sources.counts().preparation_attempts, 1);
}

#[test]
fn failed_admission_and_absent_fields_are_retained_without_repreparation() {
    let body = event(&[]);
    let missing = record(b"SCPT", 0x301, 0, &script(None, None, false));
    let bad = record(b"SCPT", 0x302, 0, &script(Some(&body), None, true));
    let (_directory, catalogue) = fixture(&body, &[missing, bad].concat());
    let sources = cache(&catalogue, Default::default()).unwrap();
    assert_eq!(sources.counts().definitions, 3);
    assert_eq!(sources.counts().absent_compiled_fields, 1);
    assert_eq!(sources.counts().preparation_attempts, 2);
    assert_eq!(sources.counts().prepared, 1);
    assert_eq!(sources.counts().rejected, 1);
    assert_eq!(sources.counts().attempted_source_bytes, body.len() * 2);
    let handles: Vec<_> = catalogue.iter().map(|(_, s)| s.handle().clone()).collect();
    let get_error = |handle| match sources.get(handle) {
        Err(LookupError::Source(error)) => error,
        _ => panic!("source rejection required"),
    };
    let missing_a = get_error(&handles[1]);
    let missing_b = get_error(&handles[1]);
    assert!(Arc::ptr_eq(&missing_a, &missing_b));
    assert!(matches!(&*missing_a, definition_plan::Error::MissingBody));
    let bad_a = get_error(&handles[2]);
    let bad_b = get_error(&handles[2]);
    assert!(Arc::ptr_eq(&bad_a, &bad_b));
    assert!(matches!(&*bad_a, definition_plan::Error::SourceMetadata(_)));
    assert_eq!(sources.counts().preparation_attempts, 2);
}

#[test]
fn authored_empty_bodies_and_metadata_without_a_body_have_distinct_admissions() {
    let body = event(&[]);
    let extras = [
        record(b"SCPT", 0x301, 0, &script(Some(&[]), None, false)),
        record(b"SCPT", 0x302, 0, &script(None, None, true)),
    ]
    .concat();
    let (_directory, catalogue) = fixture(&body, &extras);
    let sources = cache(&catalogue, Default::default()).unwrap();
    let handles: Vec<_> = catalogue.iter().map(|(_, s)| s.handle()).collect();
    let empty = sources.get(handles[1]).unwrap();
    assert!(empty.plan().control().bytes().is_empty());
    assert!(empty.plan().control().instructions().is_empty());
    assert!(matches!(
        sources.get(handles[2]),
        Err(LookupError::Source(error)) if matches!(&*error, definition_plan::Error::SourceMetadata(_))
    ));
    assert_eq!(sources.counts().preparation_attempts, 3);
    assert_eq!(sources.counts().prepared, 2);
    assert_eq!(sources.counts().rejected, 1);
    assert_eq!(sources.counts().absent_compiled_fields, 0);
}

#[test]
fn repeated_event_ids_keep_the_exact_cached_window_and_event_budget() {
    let first = event(&[]);
    let second = event(&assignment(b"1"));
    let body = [first.clone(), second].concat();
    let (_directory, catalogue) = fixture(&body, &[]);
    let sources = cache(&catalogue, Default::default()).unwrap();
    let (mut world, instance, sequence) = seed(Arc::clone(&catalogue));
    let next = world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: first.len() as u32,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let a = world
        .prepare_event_with_sources(sequence, &sources, 2)
        .unwrap();
    let b = world.prepare_event_with_sources(next, &sources, 3).unwrap();
    assert!(std::ptr::eq(a.source(), b.source()));
    assert_eq!(a.selected().begin_instruction, 0);
    assert_eq!(b.selected().begin_instruction, 2);
    assert_eq!(b.instructions()[0].bytes.start, first.len());
    assert_eq!(b.instructions().len(), 3);
    assert!(matches!(
        world.prepare_event_with_sources(next, &sources, 2),
        Err(preparation::Error::Capacity)
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn exact_candidate_source_and_aggregate_budgets_abort_instead_of_returning_partial_caches() {
    let expression = assignment(b"1 2 +");
    let body = event(&[expression.clone(), expression].concat());
    let (_directory, catalogue) = fixture(&body, &[]);
    let initial = cache(&catalogue, Default::default()).unwrap();
    let counts = initial.counts();
    let exact = programs::Limits {
        maximum_definitions: counts.definitions,
        maximum_source_receipts: counts.source_receipts,
        maximum_attempted_bytes: counts.attempted_source_bytes,
        maximum_attempted_record_bytes: counts.attempted_record_bytes,
        maximum_instructions: counts.instructions,
        maximum_expressions: counts.expressions,
        maximum_tokens: counts.tokens,
        maximum_nodes: counts.nodes,
        maximum_uses: counts.uses,
        ..Default::default()
    };
    assert!(cache(&catalogue, exact).is_ok());
    for limits in [
        programs::Limits {
            maximum_definitions: 0,
            ..exact
        },
        programs::Limits {
            maximum_source_receipts: 0,
            ..exact
        },
        programs::Limits {
            maximum_attempted_bytes: exact.maximum_attempted_bytes - 1,
            ..exact
        },
        programs::Limits {
            maximum_attempted_record_bytes: exact.maximum_attempted_record_bytes - 1,
            ..exact
        },
        programs::Limits {
            maximum_instructions: exact.maximum_instructions - 1,
            ..exact
        },
        programs::Limits {
            maximum_expressions: exact.maximum_expressions - 1,
            ..exact
        },
        programs::Limits {
            maximum_tokens: exact.maximum_tokens - 1,
            ..exact
        },
        programs::Limits {
            maximum_nodes: exact.maximum_nodes - 1,
            ..exact
        },
        programs::Limits {
            maximum_uses: exact.maximum_uses - 1,
            ..exact
        },
    ] {
        assert!(matches!(
            cache(&catalogue, limits),
            Err(programs::Error::Capacity(_))
        ));
    }
}

#[test]
fn depth_and_inner_expression_exhaustion_are_resource_errors_after_earlier_successes() {
    let first = event(&[]);
    let mut nested = Vec::new();
    instruction(&mut nested, 0x16, &[2, 0, 1, 0, b'1']);
    instruction(&mut nested, 0x16, &[0, 0, 1, 0, b'1']);
    instruction(&mut nested, 0x19, &[]);
    instruction(&mut nested, 0x19, &[]);
    let extra = record(
        b"SCPT",
        0x301,
        0,
        &script(Some(&event(&nested)), None, false),
    );
    let (_directory, catalogue) = fixture(&first, &extra);
    let mut limits = programs::Limits::default();
    limits.source.control.maximum_depth = 2;
    let sources = cache(&catalogue, limits).expect("the configured depth limit is inclusive");
    let handle = catalogue
        .record_scripts(&form(0x301))
        .next()
        .unwrap()
        .handle()
        .clone();
    let prepared = sources.get(&handle).unwrap();
    assert_eq!(prepared.plan().control().maximum_depth(), 2);

    limits.source.control.maximum_depth = 1;
    assert!(matches!(
        cache(&catalogue, limits),
        Err(programs::Error::Capacity(_))
    ));
    let expression = assignment(b"1 2 +");
    let body = event(&[expression.clone(), expression].concat());
    let (_directory, catalogue) = fixture(&body, &[]);
    for (tokens, nodes) in [(5, 6), (6, 5)] {
        let limits = programs::Limits {
            maximum_tokens: tokens,
            maximum_nodes: nodes,
            ..Default::default()
        };
        assert!(matches!(
            cache(&catalogue, limits),
            Err(programs::Error::Capacity(_))
        ));
    }
}

#[test]
fn full_cohort_changes_fail_even_when_the_cached_script_handle_is_identical() {
    let body = event(&[]);
    let (directory, catalogue) = fixture(&body, &[]);
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let a = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    let b = Arc::new(load(directory.path(), &["Other.esm", "FalloutNV.esm"]));
    let sources = cache(&a, Default::default()).unwrap();
    let (world, _, sequence) = seed(b);
    assert!(
        world
            .prepare_event_with_sources(sequence, &sources, 2)
            .is_ok()
    );
    let original = definition(&catalogue);
    let mut bytes = fs::read(directory.path().join("Other.esm")).unwrap();
    bytes.extend(record(b"ACTI", 0x400, 0, &[]));
    fs::write(directory.path().join("Other.esm"), bytes).unwrap();
    let changed = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    assert_eq!(changed.get_handle(&original).unwrap().handle(), &original);
    let (world, _, sequence) = seed(changed);
    let before = world.snapshot();
    assert!(matches!(
        world.prepare_event_with_sources(sequence, &sources, 2),
        Err(preparation::Error::CachedSource(
            LookupError::ContentChanged
        ))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn identical_embedded_bodies_never_share_different_owning_tables() {
    let directory = tempfile::tempdir().unwrap();
    let body = event(&assignment(&[b'G', 1, 0]));
    let payload = [
        script(Some(&body), Some(0x400), false),
        script(Some(&body), Some(0x401), false),
    ]
    .concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"INFO", 0x300, 0, &payload),
            record(b"GLOB", 0x400, 0, &[]),
            record(b"GLOB", 0x401, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let sources = cache(&catalogue, Default::default()).unwrap();
    let definitions: Vec<_> = catalogue
        .iter()
        .map(|(_, s)| sources.get(s.handle()).unwrap())
        .collect();
    assert_eq!(definitions.len(), 2);
    assert_eq!(
        definitions[0].plan().control().bytes(),
        definitions[1].plan().control().bytes()
    );
    assert_ne!(
        definitions[0].binding_sha256(),
        definitions[1].binding_sha256()
    );
    assert_eq!(sources.counts().preparation_attempts, 2);
}

#[test]
fn decoder_identity_binds_descriptors_convention_and_ordered_parameter_words() {
    let (_directory, catalogue) = fixture(&event(&[]), &[]);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let first = cache(&catalogue, Default::default()).unwrap();
    let mut descriptors = operators.entries().to_vec();
    descriptors.reverse();
    let reordered = Operators::new(descriptors.clone()).unwrap();
    let same = PreparedSources::load(
        &catalogue,
        &Model::vanilla(&reordered).unwrap(),
        &Signatures::new(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(first.decoder_sha256(), same.decoder_sha256());
    descriptors[0].precedence += 1;
    let changed = Operators::new(descriptors).unwrap();
    let other = PreparedSources::load(
        &catalogue,
        &Model::vanilla(&changed).unwrap(),
        &Signatures::new(),
        Default::default(),
    )
    .unwrap();
    assert_ne!(first.decoder_sha256(), other.decoder_sha256());
    let signature = CommandSignature {
        convention: Convention::Default,
        parameters: vec![Parameter {
            type_id: 3,
            optional_word: 0,
        }],
    };
    let mut signatures = Signatures::from([(0x1001, signature)]);
    let a = PreparedSources::load(&catalogue, &model, &signatures, Default::default()).unwrap();
    assert_ne!(first.decoder_sha256(), a.decoder_sha256());
    signatures.get_mut(&0x1001).unwrap().parameters[0].optional_word = 1;
    let b = PreparedSources::load(&catalogue, &model, &signatures, Default::default()).unwrap();
    assert_ne!(a.decoder_sha256(), b.decoder_sha256());
    signatures.get_mut(&0x1001).unwrap().convention = Convention::Message;
    let c = PreparedSources::load(&catalogue, &model, &signatures, Default::default()).unwrap();
    assert_ne!(b.decoder_sha256(), c.decoder_sha256());
    assert!(matches!(
        PreparedSources::load(
            &catalogue,
            &model,
            &signatures,
            programs::Limits {
                maximum_parameters: 0,
                ..Default::default()
            }
        ),
        Err(programs::Error::Capacity(_))
    ));
}

#[test]
fn cooperative_steps_preserve_plans_findings_and_exact_total_accounting() {
    let body = event(&assignment(&[b's', 2, 0]));
    let extras = [
        record(b"SCPT", 0x301, 0, &script(Some(&body), None, false)),
        record(b"SCPT", 0x302, 0, &script(Some(&body), None, true)),
        record(b"SCPT", 0x303, 0, &script(None, None, false)),
        record(b"SCPT", 0x304, 0, &script(None, None, true)),
    ]
    .concat();
    let (_directory, catalogue) = fixture(&body, &extras);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::new();
    let eager = cache(&catalogue, Default::default()).unwrap();
    let mut job = PreparationJob::new(&catalogue, &model, &signatures, Default::default()).unwrap();
    let mut visited = 0;
    let mut bytes = 0;
    let mut calls = 0;
    loop {
        let progress = job.advance(StepBudget {
            maximum_definitions: 2,
            maximum_source_bytes: body.len(),
        });
        calls += 1;
        assert!(progress.step_definitions <= 2);
        assert!(progress.step_source_bytes <= body.len());
        visited += progress.step_definitions;
        bytes += progress.step_source_bytes;
        assert_eq!(progress.processed_definitions, visited);
        assert_eq!(progress.counts.attempted_source_bytes, bytes);
        assert_ne!(progress.status, PreparationStatus::Failed);
        if progress.status == PreparationStatus::Complete {
            break;
        }
        assert!(progress.step_definitions > 0);
        let unchanged = job.advance(StepBudget {
            maximum_definitions: 0,
            maximum_source_bytes: usize::MAX,
        });
        assert_eq!(unchanged.counts, progress.counts);
        assert_eq!(unchanged.processed_definitions, visited);
        assert_eq!(unchanged.step_definitions, 0);
    }
    assert!(calls >= 3);
    let completed = job.advance(StepBudget {
        maximum_definitions: usize::MAX,
        maximum_source_bytes: usize::MAX,
    });
    assert_eq!(completed.status, PreparationStatus::Complete);
    assert_eq!(completed.step_definitions, 0);
    let sources = job.finish().unwrap();
    assert_eq!(sources.counts(), eager.counts());
    assert_eq!(sources.counts().definitions, 5);
    assert_eq!(sources.counts().prepared, 2);
    assert_eq!(sources.counts().rejected, 2);
    assert_eq!(sources.counts().absent_compiled_fields, 1);
    assert_eq!(sources.counts().preparation_attempts, 4);
    assert_eq!(sources.source_cohort_sha256(), eager.source_cohort_sha256());
    assert_eq!(sources.decoder_sha256(), eager.decoder_sha256());
    for (_, source) in catalogue.iter() {
        let fresh = definition_plan::prepare(
            &catalogue,
            source.handle(),
            &model,
            &signatures,
            Default::default(),
        );
        match (fresh, sources.get(source.handle())) {
            (Ok(fresh), Ok(cached)) => {
                assert_eq!(fresh.control().bytes(), cached.plan().control().bytes());
                assert_eq!(
                    fresh.control().instructions(),
                    cached.plan().control().instructions()
                );
                assert_eq!(fresh.nodes(), cached.plan().nodes());
                assert_eq!(
                    fallout_data::obscript::operand_binding::digest(&fresh.bindings().uses),
                    cached.binding_sha256()
                );
            }
            (Err(fresh), Err(LookupError::Source(cached))) => {
                assert_eq!(fresh.to_string(), cached.to_string());
                let Err(LookupError::Source(again)) = sources.get(source.handle()) else {
                    panic!("lost cached finding");
                };
                assert!(Arc::ptr_eq(&cached, &again));
            }
            _ => panic!("cooperative plan/finding differs from the source preparer"),
        }
    }
}

#[test]
fn indivisible_step_can_yield_without_work_and_incomplete_finish_refuses() {
    let body = event(&[]);
    let (_directory, catalogue) = fixture(&body, &[]);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::new();
    let mut job = PreparationJob::new(&catalogue, &model, &signatures, Default::default()).unwrap();
    for _ in 0..3 {
        let progress = job.advance(StepBudget {
            maximum_definitions: 1,
            maximum_source_bytes: body.len() - 1,
        });
        assert_eq!(progress.status, PreparationStatus::Pending);
        assert_eq!(progress.next_definition_bytes, Some(body.len()));
        assert_eq!(progress.processed_definitions, 0);
        assert_eq!(progress.counts.preparation_attempts, 0);
    }
    assert!(matches!(
        job.finish(),
        Err(programs::Error::Incomplete {
            remaining_definitions: 1
        })
    ));
    let mut cancelled =
        PreparationJob::new(&catalogue, &model, &signatures, Default::default()).unwrap();
    cancelled.advance(StepBudget {
        maximum_definitions: 0,
        maximum_source_bytes: 0,
    });
    drop(cancelled);
    let fresh = cache(&catalogue, Default::default()).unwrap();
    assert_eq!(fresh.counts().prepared, 1);
    assert_eq!(fresh.counts().preparation_attempts, 1);
    assert!(matches!(
        PreparationJob::new(
            &catalogue,
            &model,
            &signatures,
            programs::Limits {
                maximum_definitions: 0,
                ..Default::default()
            }
        ),
        Err(programs::Error::Capacity("definitions"))
    ));
}

#[test]
fn zero_body_and_absent_definitions_still_consume_definition_allowance() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &script(None, None, false)),
            record(b"SCPT", 0x301, 0, &script(None, None, true)),
            record(b"SCPT", 0x302, 0, &script(Some(&[]), None, false)),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::new();
    let mut job = PreparationJob::new(&catalogue, &model, &signatures, Default::default()).unwrap();
    for processed in 1..=3 {
        let progress = job.advance(StepBudget {
            maximum_definitions: 1,
            maximum_source_bytes: 0,
        });
        assert_eq!(progress.processed_definitions, processed);
        assert_eq!(progress.step_definitions, 1);
        assert_eq!(progress.step_source_bytes, 0);
        assert_eq!(
            progress.status,
            if processed == 3 {
                PreparationStatus::Complete
            } else {
                PreparationStatus::Pending
            }
        );
    }
    let sources = job.finish().unwrap();
    assert_eq!(sources.counts().absent_compiled_fields, 1);
    assert_eq!(sources.counts().rejected, 1);
    assert_eq!(sources.counts().prepared, 1);
    assert_eq!(sources.counts().preparation_attempts, 2);
}

#[test]
fn failed_job_keeps_the_first_total_limit_error_and_never_resumes_preparation() {
    let body = event(&[]);
    let extra = record(b"SCPT", 0x301, 0, &script(Some(&body), None, false));
    let (_directory, catalogue) = fixture(&body, &extra);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::new();
    for (limits, expected) in [
        (
            programs::Limits {
                maximum_attempted_bytes: body.len(),
                ..Default::default()
            },
            "attempted source bytes",
        ),
        (
            programs::Limits {
                maximum_instructions: 2,
                ..Default::default()
            },
            "source preparation",
        ),
    ] {
        let mut job = PreparationJob::new(&catalogue, &model, &signatures, limits).unwrap();
        let step = StepBudget {
            maximum_definitions: 1,
            maximum_source_bytes: body.len(),
        };
        let first = job.advance(step);
        assert_eq!(first.status, PreparationStatus::Pending);
        assert_eq!(first.counts.prepared, 1);
        let failure = job.advance(step);
        assert_eq!(failure.status, PreparationStatus::Failed);
        assert_eq!(failure.next_definition_bytes, None);
        assert_eq!(failure.counts.prepared, 1);
        let repeated = job.advance(step);
        assert_eq!(repeated.counts, failure.counts);
        assert_eq!(
            repeated.processed_definitions,
            failure.processed_definitions
        );
        assert_eq!(repeated.step_definitions, 0);
        assert!(
            matches!(job.finish(), Err(programs::Error::Capacity(reason)) if reason == expected)
        );
        assert!(
            matches!(cache(&catalogue, limits), Err(programs::Error::Capacity(reason)) if reason == expected)
        );
    }
}

#[test]
#[ignore = "built CLI and authored executable metadata copy; cooperative source work, no original launch"]
fn cli_cooperative_preparation_helper() {
    use serde_json::{Value as Json, json};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_PREPARATION_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let body = event(&assignment(&[b's', 2, 0]));
    let extras: Vec<_> = (0x301..0x325)
        .flat_map(|id| record(b"SCPT", id, 0, &script(Some(&body), None, false)))
        .collect();
    let (temporary, catalogue) = fixture(&body, &extras);
    assert_eq!(catalogue.iter().count(), 37);
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
    let run = |name: &str, request: &Json| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        let report_path = directory.join("report.json");
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["source-plans", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--cooperative-preparation")
            .arg(&request_path)
            .arg("--output")
            .arg(&report_path)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        (output, report_path)
    };
    let mut expected_cache = None;
    for (name, definitions, bytes, advances, cancel, outcome, processed, calls) in [
        ("all", 100, 100 * body.len(), 100, None, "complete", 37, 1),
        (
            "one-definition",
            1,
            100 * body.len(),
            100,
            None,
            "complete",
            37,
            37,
        ),
        (
            "three-bodies",
            20,
            3 * body.len(),
            100,
            None,
            "complete",
            37,
            13,
        ),
        (
            "cancel-two",
            20,
            3 * body.len(),
            100,
            Some(2),
            "cancelled",
            6,
            2,
        ),
        (
            "cancel-zero",
            20,
            3 * body.len(),
            100,
            Some(0),
            "cancelled",
            0,
            0,
        ),
        (
            "advance-cap",
            20,
            3 * body.len(),
            1,
            None,
            "advance_budget",
            3,
            1,
        ),
        (
            "indivisible",
            20,
            body.len() - 1,
            100,
            None,
            "step_budget",
            0,
            1,
        ),
        (
            "after-cancellation",
            100,
            100 * body.len(),
            100,
            None,
            "complete",
            37,
            1,
        ),
    ] {
        let request = json!({"schema_version":1,"maximum_definitions_per_advance":definitions,
            "maximum_source_bytes_per_advance":bytes,"maximum_advances":advances,"cancel_after_advances":cancel});
        let (output, path) = run(name, &request);
        assert_eq!(
            output.status.success(),
            outcome == "complete",
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        let result = &report["cooperative_preparation"];
        assert_eq!(result["outcome"], outcome, "{name}");
        let steps = result["advances"].as_array().unwrap();
        assert_eq!(steps.len(), calls);
        assert_eq!(
            steps
                .last()
                .map_or(0, |p| p["processed_definitions"].as_u64().unwrap()),
            processed
        );
        let mut attempted = 0;
        for step in steps {
            assert!(step["step_definitions"].as_u64().unwrap() <= definitions as u64);
            assert!(step["step_source_bytes"].as_u64().unwrap() <= bytes as u64);
            attempted += step["step_definitions"].as_u64().unwrap();
            assert_eq!(step["counts"]["preparation_attempts"], attempted);
            assert_eq!(
                step["counts"]["attempted_source_bytes"],
                attempted * body.len() as u64
            );
        }
        if outcome == "complete" {
            assert_eq!(result["cache"]["counts"]["prepared"], 37);
            assert_eq!(result["cache"]["counts"]["rejected"], 0);
            assert_eq!(result["prepared_sources_published"], true);
            match &expected_cache {
                None => expected_cache = Some(result["cache"].clone()),
                Some(expected) => assert_eq!(&result["cache"], expected),
            }
        } else {
            assert!(result["cache"].is_null());
            assert_eq!(result["prepared_sources_published"], false);
        }
        assert_eq!(result["hard_time_slice"], false);
        assert_eq!(report["execution_ready"], false);
        assert_eq!(report["retail_parity_accepted"], false);
    }
    let mut invalid = json!({"schema_version":1,"maximum_definitions_per_advance":1,
        "maximum_source_bytes_per_advance":100,"maximum_advances":100,"cancel_after_advances":null});
    for (name, field, value) in [
        ("unknown-field", "execute", json!(true)),
        ("wrong-version", "schema_version", json!(2)),
        (
            "zero-allowance",
            "maximum_definitions_per_advance",
            json!(0),
        ),
        ("too-many-advances", "maximum_advances", json!(4097)),
        ("invalid-cancellation", "cancel_after_advances", json!(101)),
    ] {
        let previous = invalid[field].clone();
        invalid[field] = value;
        let (output, path) = run(name, &invalid);
        assert!(!output.status.success());
        assert!(!path.exists());
        if previous.is_null() && field == "execute" {
            invalid.as_object_mut().unwrap().remove(field);
        } else {
            invalid[field] = previous;
        }
    }
    fs::write(
        evidence.join("source-shape.json"),
        serde_json::to_vec_pretty(&json!({
            "scope":"authored_cooperative_source_preparation_only", "definitions":37,
            "compiled_bytes_per_definition":body.len(), "original_executed":false,
            "equivalent_completed_cache":expected_cache, "normal_cases":8, "refusals":5,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn large_unrelated_script() -> Vec<u8> {
    let mut middle = Vec::new();
    for _ in 0..20_000 {
        instruction(&mut middle, 0x7fff, &[]); // deliberately unavailable native signature
    }
    event(&middle)
}

#[test]
fn selected_source_saves_work_without_disguising_absent_or_rejected_unselected_definitions() {
    let body = event(&assignment(&[b's', 2, 0]));
    let unrelated = large_unrelated_script();
    let extra = [
        record(b"SCPT", 0x301, 0, &script(Some(&unrelated), None, false)),
        record(b"SCPT", 0x302, 0, &script(None, None, false)),
    ]
    .concat();
    let (_directory, catalogue) = fixture(&body, &extra);
    let handles: Vec<_> = catalogue
        .iter()
        .map(|(_, source)| source.handle().clone())
        .collect();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::new();
    let eager = cache(&catalogue, Default::default()).unwrap();
    assert_eq!(eager.counts().preparation_attempts, 2);
    assert_eq!(eager.counts().rejected, 1);
    let exact = programs::Limits {
        maximum_attempted_bytes: body.len(),
        maximum_attempted_record_bytes: catalogue
            .get_handle(&handles[0])
            .unwrap()
            .decoded_record_bytes(),
        maximum_instructions: 3,
        ..Default::default()
    };
    let selected =
        PreparedSources::load_selected(&catalogue, &model, &signatures, &handles[..1], exact)
            .unwrap();
    assert_eq!(selected.counts().definitions, 3); // full cohort admission unchanged
    assert_eq!(selected.counts().preparation_attempts, 1);
    assert_eq!(selected.counts().prepared, 1);
    assert_eq!(selected.counts().rejected, 0);
    assert_eq!(selected.counts().absent_compiled_fields, 0);
    assert_eq!(selected.counts().attempted_source_bytes, body.len());
    assert_eq!(
        selected.source_cohort_sha256(),
        eager.source_cohort_sha256()
    );
    assert_eq!(selected.decoder_sha256(), eager.decoder_sha256());
    assert_eq!(
        selected.get(&handles[0]).unwrap().binding_sha256(),
        eager.get(&handles[0]).unwrap().binding_sha256()
    );
    assert_eq!(
        selected
            .get(&handles[0])
            .unwrap()
            .plan()
            .control()
            .instructions(),
        eager
            .get(&handles[0])
            .unwrap()
            .plan()
            .control()
            .instructions()
    );
    for handle in &handles[1..] {
        assert!(matches!(
            selected.get(handle),
            Err(LookupError::NotSelected)
        ));
    }
    assert!(matches!(
        cache(&catalogue, exact),
        Err(programs::Error::Capacity("attempted source bytes"))
    ));
    assert!(matches!(
        PreparedSources::load_selected(&catalogue, &model, &signatures, &handles[..2], exact),
        Err(programs::Error::Capacity("attempted source bytes"))
    ));
    let with_absent = PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &[handles[2].clone(), handles[0].clone()],
        exact,
    )
    .unwrap();
    assert!(
        matches!(with_absent.get(&handles[2]), Err(LookupError::Source(error)) if matches!(*error, definition_plan::Error::MissingBody))
    );
    assert_eq!(with_absent.counts().absent_compiled_fields, 1);
    let rejected = PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        &handles[1..2],
        Default::default(),
    )
    .unwrap();
    assert_eq!(rejected.counts().rejected, 1);
    assert!(matches!(
        rejected.get(&handles[1]),
        Err(LookupError::Source(_))
    ));
    assert!(matches!(
        rejected.get(&handles[0]),
        Err(LookupError::NotSelected)
    ));
}

#[test]
fn exact_selection_validates_duplicates_and_stale_handles_before_any_source_preparation() {
    let body = event(&[]);
    let (_directory, catalogue) = fixture(&body, &[]);
    let handle = definition(&catalogue);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::new();
    let no_bytes = programs::Limits {
        maximum_attempted_bytes: 0,
        ..Default::default()
    };
    assert!(matches!(
        PreparedSources::load_selected(
            &catalogue,
            &model,
            &signatures,
            &[handle.clone(), handle.clone()],
            no_bytes
        ),
        Err(programs::Error::DuplicateSelection)
    ));
    let mut stale = handle.clone();
    stale.version_sha256 = "0".repeat(64);
    assert!(matches!(
        PreparedSources::load_selected(
            &catalogue,
            &model,
            &signatures,
            &[handle.clone(), stale.clone()],
            no_bytes
        ),
        Err(programs::Error::Lookup(LookupError::DefinitionChanged))
    ));
    let empty =
        PreparedSources::load_selected(&catalogue, &model, &signatures, &[], no_bytes).unwrap();
    assert_eq!(empty.counts().definitions, 1);
    assert_eq!(empty.counts().preparation_attempts, 0);
    assert_eq!(empty.counts().attempted_source_bytes, 0);
    assert!(matches!(empty.get(&handle), Err(LookupError::NotSelected)));
    assert!(matches!(
        empty.get(&stale),
        Err(LookupError::DefinitionChanged)
    ));
    let mut missing = handle;
    missing.key.record.local_id += 1;
    assert!(matches!(
        empty.get(&missing),
        Err(LookupError::DefinitionChanged)
    ));
    assert!(matches!(
        PreparedSources::load_selected(
            &catalogue,
            &model,
            &signatures,
            &[],
            programs::Limits {
                maximum_definitions: 0,
                ..no_bytes
            }
        ),
        Err(programs::Error::Capacity("definitions"))
    ));
}

#[test]
fn a_selected_cache_keeps_the_complete_world_cohort_guard() {
    let body = event(&[]);
    let (directory, catalogue) = fixture(&body, &[]);
    let original = definition(&catalogue);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = PreparedSources::load_selected(
        &catalogue,
        &model,
        &Signatures::new(),
        std::slice::from_ref(&original),
        Default::default(),
    )
    .unwrap();
    let (world, _, sequence) = seed(Arc::clone(&catalogue));
    assert!(
        world
            .prepare_event_with_sources(sequence, &sources, 2)
            .is_ok()
    );
    fs::write(
        directory.path().join("Other.esm"),
        [header(&[]), record(b"ACTI", 0x400, 0, &[])].concat(),
    )
    .unwrap();
    let changed = Arc::new(load(directory.path(), &["FalloutNV.esm", "Other.esm"]));
    assert_eq!(changed.get_handle(&original).unwrap().handle(), &original);
    let (world, _, sequence) = seed(changed);
    let before = world.snapshot();
    assert!(matches!(
        world.prepare_event_with_sources(sequence, &sources, 2),
        Err(preparation::Error::CachedSource(
            LookupError::ContentChanged
        ))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn an_existing_dependency_consumer_refuses_unselected_source_without_inferring_closure() {
    use fallout_data::{quest_scripts::Attachments, store::RecordStore};
    let mut native = Vec::new();
    instruction(&mut native, 0x102f, &[1, 0, b'r', 1, 0]);
    let extra = record(b"SCPT", 0x301, 0, &script(Some(&event(&[])), None, false));
    let directory = tempfile::tempdir().unwrap();
    let mut bytes = header(&[]);
    bytes.extend(record(
        b"SCPT",
        0x300,
        0,
        &script(Some(&event(&native)), Some(0x301), false),
    ));
    bytes.extend(extra);
    fs::write(directory.path().join("FalloutNV.esm"), bytes).unwrap();
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let handle = definition(&catalogue);
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let signatures = Signatures::from([(
        0x102f,
        CommandSignature {
            convention: Convention::Default,
            parameters: vec![Parameter {
                type_id: 50,
                optional_word: 0,
            }],
        },
    )]);
    let sources = PreparedSources::load_selected(
        &catalogue,
        &model,
        &signatures,
        std::slice::from_ref(&handle),
        Default::default(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
    assert!(matches!(
        fallout_runtime::execution::admission::check(
            &sources,
            &attachments,
            std::slice::from_ref(&handle),
            Default::default()
        ),
        Err(fallout_runtime::execution::admission::Error::Source(
            LookupError::NotSelected
        ))
    ));
    let full = PreparedSources::load(&catalogue, &model, &signatures, Default::default()).unwrap();
    let complete = fallout_runtime::execution::admission::check(
        &full,
        &attachments,
        std::slice::from_ref(&handle),
        Default::default(),
    )
    .unwrap();
    assert_eq!(complete.dependencies.len(), 1);
    assert_eq!(complete.definitions.len(), 2);
    assert!(!complete.faithful_execution_admitted);
}

#[test]
#[ignore = "built CLI and authored executable metadata; explicit source selection, no original launch"]
fn cli_selected_preparation_helper() {
    use serde_json::{Value as Json, json};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_SELECTION_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let body = event(&assignment(&[b's', 2, 0]));
    let unrelated = large_unrelated_script();
    let extra = [
        record(b"SCPT", 0x301, 0, &script(Some(&unrelated), None, false)),
        record(b"SCPT", 0x302, 0, &script(None, None, false)),
    ]
    .concat();
    let (temporary, catalogue) = fixture(&body, &extra);
    let full = cache(&catalogue, Default::default()).unwrap();
    let handles: Vec<_> = catalogue
        .iter()
        .map(|(_, source)| source.handle().clone())
        .collect();
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
    let run = |name: &str, flag: &str, request: &Json| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        let report_path = directory.join("report.json");
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["source-plans", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg(flag)
            .arg(&request_path)
            .arg("--output")
            .arg(&report_path)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        (output, report_path)
    };
    let whole_request = json!({"schema_version":1,"maximum_definitions_per_advance":100,
        "maximum_source_bytes_per_advance":1024*1024,"maximum_advances":1,"cancel_after_advances":null});
    let (output, path) = run("whole", "--cooperative-preparation", &whole_request);
    assert!(!output.status.success()); // explicit unavailable unrelated native signature
    let whole: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let whole_cache = &whole["cooperative_preparation"]["cache"];
    assert_eq!(whole_cache["counts"]["preparation_attempts"], 2);
    assert_eq!(whole_cache["counts"]["rejected"], 1);
    assert_eq!(
        whole_cache["counts"]["attempted_source_bytes"],
        body.len() + unrelated.len()
    );
    let selected_request = json!({"schema_version":1,"source_cohort_sha256":full.source_cohort_sha256(),"definitions":[handles[0]]});
    let (output, path) = run("selected", "--selected-source", &selected_request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let selected: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let result = &selected["selected_source"];
    let selected_cache = &result["cache"];
    assert_eq!(selected_cache["counts"]["definitions"], 3);
    assert_eq!(selected_cache["counts"]["preparation_attempts"], 1);
    assert_eq!(
        selected_cache["counts"]["attempted_source_bytes"],
        body.len()
    );
    assert_eq!(selected_cache["counts"]["rejected"], 0);
    assert_eq!(selected_cache["counts"]["absent_compiled_fields"], 0);
    assert_eq!(
        selected_cache["source_cohort_sha256"],
        whole_cache["source_cohort_sha256"]
    );
    assert_eq!(
        selected_cache["decoder_sha256"],
        whole_cache["decoder_sha256"]
    );
    assert_eq!(
        result["definitions"][0]["handle"],
        selected_request["definitions"][0]
    );
    assert_eq!(
        result["definitions"][0]["binding_sha256"],
        full.get(&handles[0]).unwrap().binding_sha256()
    );
    assert_eq!(result["inferred_dependency_closure"], false);
    assert_eq!(selected["execution_ready"], false);
    assert_eq!(selected["retail_parity_accepted"], false);
    for (name, index, status) in [
        ("selected-rejected", 1, "source_finding"),
        ("selected-absent", 2, "source_finding"),
    ] {
        let request = json!({"schema_version":1,"source_cohort_sha256":full.source_cohort_sha256(),"definitions":[handles[index]]});
        let (output, path) = run(name, "--selected-source", &request);
        assert!(!output.status.success());
        let result: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            result["selected_source"]["definitions"][0]["status"],
            status
        );
        assert_eq!(result["execution_ready"], false);
    }
    let mut stale = handles[0].clone();
    stale.version_sha256 = "0".repeat(64);
    for (name, definitions, cohort, reason) in [
        (
            "duplicate",
            vec![handles[0].clone(); 2],
            full.source_cohort_sha256().to_owned(),
            "duplicate",
        ),
        (
            "stale",
            vec![handles[0].clone(), stale],
            full.source_cohort_sha256().to_owned(),
            "missing or has changed",
        ),
        (
            "wrong-cohort",
            vec![handles[0].clone()],
            "0".repeat(64),
            "different source receipt cohort",
        ),
    ] {
        let request =
            json!({"schema_version":1,"source_cohort_sha256":cohort,"definitions":definitions});
        let (output, path) = run(name, "--selected-source", &request);
        assert!(!output.status.success());
        assert!(!path.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains(reason));
    }
    for (name, field, value, reason) in [
        (
            "wrong-version",
            "schema_version",
            json!(2),
            "invalid selected source request",
        ),
        (
            "too-many-handles",
            "definitions",
            json!(vec![handles[0].clone(); 129]),
            "invalid selected source request",
        ),
        ("unknown-field", "execute", json!(true), "unknown field"),
        (
            "request-byte-limit",
            "notes",
            json!("x".repeat(65_536)),
            "request byte budget exceeded",
        ),
    ] {
        let mut request = selected_request.clone();
        request[field] = value;
        let (output, path) = run(name, "--selected-source", &request);
        assert!(!output.status.success());
        assert!(!path.exists());
        assert!(String::from_utf8_lossy(&output.stderr).contains(reason));
    }
    fs::write(evidence.join("source-shape.json"), serde_json::to_vec_pretty(&json!({
        "scope":"explicit_authored_source_selection_only","catalogue_definitions":3,
        "selected_compiled_bytes":body.len(),"unrelated_compiled_bytes":unrelated.len(),
        "whole_attempted_bytes":body.len()+unrelated.len(),"selected_attempted_bytes":body.len(),
        "source_cohort_sha256":full.source_cohort_sha256(),"original_executed":false,
        "normal_source_requests":4,"invalid_source_requests":7,
    })).unwrap()).unwrap();
}
