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
    programs::{self, LookupError, PreparedSources},
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
        out.extend(field(b"SCDA", body));
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
    instruction(&mut nested, 0x16, &[0, 0, 1, 0, b'1']);
    instruction(&mut nested, 0x19, &[]);
    let extra = record(
        b"SCPT",
        0x301,
        0,
        &script(Some(&event(&nested)), None, false),
    );
    let (_directory, catalogue) = fixture(&first, &extra);
    let mut limits = programs::Limits::default();
    limits.source.control.maximum_depth = 0;
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
