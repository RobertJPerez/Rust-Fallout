mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Handle},
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::{foreign_copy, local_copy},
    foreign::Content,
    identity::{Owner, ReferenceValue, Value},
    programs::PreparedSources,
};
use std::{fs, sync::Arc};

fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn event(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x10,
        &[
            0_u16.to_le_bytes().as_slice(),
            &((body.len() + 4) as u32).to_le_bytes(),
        ]
        .concat(),
    );
    out.extend(body);
    instruction(&mut out, 0x11, &[]);
    out
}
fn local(index: u16) -> Vec<u8> {
    [vec![b'f'], index.to_le_bytes().to_vec()].concat()
}
fn foreign(context: u16, index: u16) -> Vec<u8> {
    [vec![b'r'], context.to_le_bytes().to_vec(), local(index)].concat()
}
fn assignment(target: &[u8], expression: &[u8]) -> Vec<u8> {
    let data = [
        target.to_vec(),
        (expression.len() as u16).to_le_bytes().to_vec(),
        expression.to_vec(),
    ]
    .concat();
    let mut out = Vec::new();
    instruction(&mut out, 0x15, &data);
    out
}
fn script(body: &[u8], declarations: &[(u32, u8)], references: &[(&[u8; 4], u32)]) -> Vec<u8> {
    let original = unit(declarations, references);
    let mut out = original[..26].to_vec();
    out[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(field(b"SCDA", body));
    out.extend(&original[46..]);
    out
}
fn fixture(body: &[u8]) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(
                b"SCPT",
                0x300,
                0,
                &script(
                    body,
                    &[(42, 0), (90, 0)],
                    &[
                        (b"SCRO", 0x100),
                        (b"SCRO", 0x101),
                        (b"SCRV", 90),
                        (b"SCRO", 0),
                        (b"SCRO", 0x14),
                        (b"SCRO", 0x110),
                    ],
                ),
            ),
            record(b"SCPT", 0x301, 0, &script(&event(&[]), &[(42, 1)], &[])),
            record(
                b"SCPT",
                0x302,
                0,
                &script(&event(&[]), &[(42, 0)], &[(b"SCRV", 42)]),
            ),
            record(b"QUST", 0x100, 0, &[]),
            record(b"REFR", 0x101, 0, &field(b"NAME", &0x110_u32.to_le_bytes())),
            record(b"MISC", 0x110, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (directory, catalogue, content)
}
fn definition(catalogue: &Catalogue, id: u32) -> Handle {
    catalogue
        .record_scripts(&form(id))
        .next()
        .unwrap()
        .handle()
        .clone()
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
    PreparedSources::load_selected(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Signatures::new(),
        &[definition(catalogue, 0x300)],
        Default::default(),
    )
    .unwrap()
}
fn seed(
    catalogue: Arc<Catalogue>,
) -> (World<'static>, fallout_runtime::state::InstanceHandle, u64) {
    let mut world = World::new(Arc::clone(&catalogue), Default::default()).unwrap();
    let handle = world
        .create_instance(
            &definition(&catalogue, 0x300),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 199 })])
        .unwrap();
    let sequence = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, handle, sequence)
}
fn stage(
    world: &World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
    sequence: u64,
    limits: foreign_copy::Limits,
) -> Box<foreign_copy::StagedForeignCopy> {
    match foreign_copy::stage(
        world,
        sources,
        content,
        foreign_copy::Selection {
            sequence,
            explicit_player: None,
            intent: local_copy::Intent::Engineering,
        },
        limits,
    )
    .unwrap()
    {
        foreign_copy::Preparation::Staged(stage) => stage,
        foreign_copy::Preparation::Unsupported { reason, detail } => panic!("{reason:?}: {detail}"),
    }
}

#[test]
fn exact_qualified_foreign_bits_change_only_own_slot_and_one_head_across_cold_restore() {
    let (_directory, catalogue, content) =
        fixture(&event(&assignment(&local(42), &foreign(1, 42))));
    let prepared = sources(&catalogue);
    for bits in [0_u64, 1 << 63, 0x7ff8_0000_0000_0203, u64::MAX] {
        let (mut world, source, sequence) = seed(Arc::clone(&catalogue));
        let target = world
            .create_instance(
                &definition(&catalogue, 0x301),
                Owner::Quest { key: form(0x100) },
                Context::default(),
            )
            .unwrap();
        world
            .assign(target, &[(42, Value::Number { bits })])
            .unwrap();
        let before = world.snapshot();
        let proposal = stage(&world, &prepared, &content, sequence, Default::default());
        assert_eq!(world.snapshot(), before);
        assert_eq!(proposal.trace().copied_bits, bits);
        assert_eq!(proposal.trace().source_index, 42);
        assert_eq!(proposal.trace().destination_index, 42);
        assert_eq!(
            proposal.trace().foreign_read.target.target_instance,
            world.instance(target).unwrap().id()
        );
        assert_eq!(
            proposal.trace().foreign_read.target.target_definition,
            definition(&catalogue, 0x301)
        );
        assert_eq!(proposal.trace().context_token_scda_bytes, 19..22);
        assert_eq!(proposal.trace().source_token_scda_bytes, 22..25);
        assert_eq!(proposal.trace().statement_scda_bytes, 10..25);
        assert!(!proposal.trace().original_behavior_verified);
        let committed = proposal.commit(&mut world).unwrap();
        assert_eq!(committed.receipt.assignments, 1);
        assert_eq!(committed.receipt.after_revision, before.state_revision + 1);
        let mut expected = before.clone();
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        expected
            .instances
            .iter_mut()
            .find(|instance| instance.id == world.instance(source).unwrap().id())
            .unwrap()
            .locals
            .iter_mut()
            .find(|local| local.index == 42)
            .unwrap()
            .value = Value::Number { bits };
        assert_eq!(world.snapshot(), expected);
        let cold =
            World::restore(Arc::clone(&catalogue), expected.clone(), Default::default()).unwrap();
        assert_eq!(cold.snapshot(), expected);
        assert!(matches!(
            foreign_copy::stage(
                &cold,
                &prepared,
                &content,
                foreign_copy::Selection {
                    sequence,
                    explicit_player: None,
                    intent: local_copy::Intent::Engineering
                },
                Default::default()
            ),
            Err(foreign_copy::Error::HeadChanged)
        ));
    }
}

#[test]
fn foreign_copy_refusals_and_exact_creation_limits_leave_complete_state_unchanged() {
    let (_directory, catalogue, content) =
        fixture(&event(&assignment(&local(42), &foreign(1, 42))));
    let prepared = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
    let selection = foreign_copy::Selection {
        sequence,
        explicit_player: None,
        intent: local_copy::Intent::Engineering,
    };
    let before = world.snapshot();
    assert!(matches!(
        foreign_copy::stage(&world, &prepared, &content, selection, Default::default()).unwrap(),
        foreign_copy::Preparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    assert!(matches!(
        foreign_copy::stage(&world, &prepared, &content, selection, Default::default()).unwrap(),
        foreign_copy::Preparation::Unsupported { .. }
    ));
    world
        .assign(
            target,
            &[(
                42,
                Value::Number {
                    bits: 0x8000_0000_0000_0000,
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    let sample = stage(&world, &prepared, &content, sequence, Default::default());
    let projection = sample.trace().frame.counts;
    let trace_bytes = serde_json::to_vec(sample.trace()).unwrap().len();
    let exact = foreign_copy::Limits {
        maximum_event_instructions: 3,
        maximum_operand_uses: 3,
        maximum_statement_bytes: 15,
        observation: fallout_runtime::preparation::ObservationLimits {
            maximum_source_bytes: projection.source_bytes,
            maximum_rows: projection.rows,
            maximum_variable_bytes: projection.variable_bytes,
            maximum_binding_uses: projection.binding_uses,
        },
        maximum_metadata_rows: sample.trace().counts.metadata_rows,
        maximum_probe_variable_bytes: sample.trace().counts.probe_variable_reservation,
        maximum_trace_bytes: trace_bytes,
    };
    assert_eq!(
        serde_json::to_vec(stage(&world, &prepared, &content, sequence, exact).trace()).unwrap(),
        serde_json::to_vec(sample.trace()).unwrap()
    );
    for field in 0..10 {
        let mut limits = exact;
        match field {
            0 => limits.maximum_event_instructions -= 1,
            1 => limits.maximum_operand_uses -= 1,
            2 => limits.maximum_statement_bytes -= 1,
            3 => limits.observation.maximum_source_bytes -= 1,
            4 => limits.observation.maximum_rows -= 1,
            5 => limits.observation.maximum_variable_bytes -= 1,
            6 => limits.observation.maximum_binding_uses -= 1,
            7 => limits.maximum_metadata_rows -= 1,
            8 => limits.maximum_probe_variable_bytes -= 1,
            _ => limits.maximum_trace_bytes -= 1,
        }
        assert!(
            foreign_copy::stage(&world, &prepared, &content, selection, limits).is_err(),
            "field{field}"
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(matches!(
        foreign_copy::stage(
            &world,
            &prepared,
            &content,
            foreign_copy::Selection {
                intent: local_copy::Intent::Faithful,
                ..selection
            },
            foreign_copy::Limits {
                maximum_probe_variable_bytes: 0,
                ..exact
            }
        )
        .unwrap(),
        foreign_copy::Preparation::Unsupported {
            reason: local_copy::Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    let mut cold =
        World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    assert!(sample.commit(&mut cold).is_err());
    assert_eq!(cold.snapshot(), before);
    let stale = stage(&world, &prepared, &content, sequence, exact);
    world
        .assign(target, &[(42, Value::Number { bits: 7 })])
        .unwrap();
    let changed = world.snapshot();
    assert!(stale.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), changed);
}

#[test]
fn source_context_selects_current_quest_placed_dynamic_and_explicit_player_without_fallback() {
    for (context, mode) in [
        (1, "quest"),
        (2, "placed"),
        (3, "content"),
        (3, "live"),
        (5, "player"),
    ] {
        let (_directory, catalogue, content) =
            fixture(&event(&assignment(&local(42), &foreign(context, 42))));
        let prepared = sources(&catalogue);
        let (mut world, own, sequence) = seed(Arc::clone(&catalogue));
        let reference = world.register_reference(Some(form(0x101))).unwrap();
        let owner = if mode == "quest" || mode == "content" {
            Owner::Quest { key: form(0x100) }
        } else {
            Owner::Placed { reference }
        };
        let foreign_instance = world
            .create_instance(
                &definition(&catalogue, 0x301),
                owner.clone(),
                Context::default(),
            )
            .unwrap();
        world
            .assign(
                foreign_instance,
                &[(
                    42,
                    Value::Number {
                        bits: 0x7ff8_0000_0000_0505,
                    },
                )],
            )
            .unwrap();
        if context == 3 {
            let value = if mode == "content" {
                ReferenceValue::Content { key: form(0x100) }
            } else {
                ReferenceValue::Live { id: reference }
            };
            world
                .assign(own, &[(90, Value::Reference { value })])
                .unwrap();
        }
        let selection = foreign_copy::Selection {
            sequence,
            explicit_player: (mode == "player").then_some(reference),
            intent: local_copy::Intent::Engineering,
        };
        let before = world.snapshot();
        let foreign_copy::Preparation::Staged(proposal) =
            foreign_copy::stage(&world, &prepared, &content, selection, Default::default())
                .unwrap()
        else {
            panic!("{mode}")
        };
        assert_eq!(proposal.trace().foreign_read.target.target_owner, owner);
        assert_eq!(
            proposal.trace().foreign_read.target.target_instance,
            world.instance(foreign_instance).unwrap().id()
        );
        assert_eq!(proposal.trace().copied_bits, 0x7ff8_0000_0000_0505);
        assert_eq!(world.snapshot(), before);
        if mode == "player" {
            // An unrelated existing owner/reference never supplies the role.
            assert!(matches!(
                foreign_copy::stage(
                    &world,
                    &prepared,
                    &content,
                    foreign_copy::Selection {
                        explicit_player: None,
                        ..selection
                    },
                    Default::default()
                )
                .unwrap(),
                foreign_copy::Preparation::Unsupported { .. }
            ));
            assert_eq!(world.snapshot(), before);
        }
        proposal.commit(&mut world).unwrap();
        assert_eq!(
            world.instance(foreign_instance).unwrap().local(42).unwrap(),
            &Value::Number {
                bits: 0x7ff8_0000_0000_0505
            }
        );
        assert_eq!(
            world.instance(own).unwrap().local(42).unwrap(),
            world.instance(foreign_instance).unwrap().local(42).unwrap()
        );
    }
}

#[test]
fn unsupported_shapes_and_current_foreign_declarations_refuse_before_stage() {
    let cases = [
        assignment(&local(42), &local(42)),
        assignment(&local(42), b"1"),
        assignment(
            &local(42),
            &[foreign(1, 42), local(42), vec![b'+']].concat(),
        ),
        assignment(&local(42), &[b'X', 1, 0x10, 2, 0, 0, 0]),
        assignment(&foreign(1, 42), &local(42)),
        assignment(&local(90), &foreign(1, 42)),
        assignment(&local(43), &foreign(1, 42)),
        assignment(&local(42), &foreign(1, 43)),
        assignment(&local(42), &foreign(4, 42)),
        assignment(&local(42), &foreign(6, 42)),
        [
            assignment(&local(42), &foreign(1, 42)),
            assignment(&local(42), &foreign(1, 42)),
        ]
        .concat(),
    ];
    for (index, body) in cases.iter().enumerate() {
        let (_directory, catalogue, content) = fixture(&event(body));
        let prepared = sources(&catalogue);
        let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
        let other = world
            .create_instance(
                &definition(&catalogue, 0x301),
                Owner::Quest { key: form(0x100) },
                Context::default(),
            )
            .unwrap();
        world
            .assign(other, &[(42, Value::Number { bits: 17 })])
            .unwrap();
        let before = world.snapshot();
        let result = foreign_copy::stage(
            &world,
            &prepared,
            &content,
            foreign_copy::Selection {
                sequence,
                explicit_player: None,
                intent: local_copy::Intent::Engineering,
            },
            Default::default(),
        );
        if matches!(index, 3 | 6) {
            assert!(
                matches!(result, Err(foreign_copy::Error::Preparation(_))),
                "unbound native call or missing declaration must fail source admission"
            );
        } else {
            assert!(
                matches!(result, Ok(foreign_copy::Preparation::Unsupported { .. })),
                "case {index} must refuse the source shape"
            );
        }
        assert_eq!(world.snapshot(), before);
    }
    let (_directory, catalogue, content) =
        fixture(&event(&assignment(&local(42), &foreign(1, 42))));
    let prepared = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
    let target = world
        .create_instance(
            &definition(&catalogue, 0x302),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    let mut oversized = form(0x777);
    oversized.origin_plugin = "x".repeat(32 * 1024);
    world
        .assign(
            target,
            &[(
                42,
                Value::Reference {
                    value: ReferenceValue::Content { key: oversized },
                },
            )],
        )
        .unwrap();
    let before = world.snapshot();
    assert!(matches!(
        foreign_copy::stage(
            &world,
            &prepared,
            &content,
            foreign_copy::Selection {
                sequence,
                explicit_player: None,
                intent: local_copy::Intent::Engineering
            },
            foreign_copy::Limits {
                maximum_probe_variable_bytes: 2048,
                ..Default::default()
            }
        )
        .unwrap(),
        foreign_copy::Preparation::Unsupported {
            reason: local_copy::Unsupported::NonNumericLocal,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
    // Canonical owner uniqueness prevents ambiguous lists at their creation.
    assert!(
        world
            .create_instance(
                &definition(&catalogue, 0x301),
                Owner::Quest { key: form(0x100) },
                Context::default()
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn unbounded_canonical_context_names_are_charged_before_resolver_payload_copies() {
    let (_directory, catalogue, content) =
        fixture(&event(&assignment(&local(42), &foreign(3, 42))));
    let prepared = sources(&catalogue);
    for mode in ["content", "live", "dynamic-player"] {
        let (mut world, own, sequence) = seed(Arc::clone(&catalogue));
        let mut giant = form(0x777);
        giant.origin_plugin = "x".repeat(32 * 1024);
        let reference = world.register_reference(Some(giant.clone())).unwrap();
        let value = match mode {
            "content" => ReferenceValue::Content { key: giant },
            "live" => ReferenceValue::Live { id: reference },
            _ => ReferenceValue::Content { key: form(0x14) },
        };
        world
            .assign(own, &[(90, Value::Reference { value })])
            .unwrap();
        let before = world.snapshot();
        assert!(matches!(
            foreign_copy::stage(
                &world,
                &prepared,
                &content,
                foreign_copy::Selection {
                    sequence,
                    explicit_player: (mode == "dynamic-player").then_some(reference),
                    intent: local_copy::Intent::Engineering
                },
                foreign_copy::Limits {
                    maximum_probe_variable_bytes: 4096,
                    ..Default::default()
                }
            ),
            Err(foreign_copy::Error::Capacity("probe variable bytes"))
        ));
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn source_cohort_replacement_foreign_recycling_and_revision_exhaustion_are_atomic() {
    let (_directory, catalogue, content) =
        fixture(&event(&assignment(&local(42), &foreign(1, 42))));
    let prepared = sources(&catalogue);
    let (mut world, _, sequence) = seed(Arc::clone(&catalogue));
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(target, &[(42, Value::Number { bits: 99 })])
        .unwrap();
    let before = world.snapshot();
    let proposal = stage(&world, &prepared, &content, sequence, Default::default());
    world.remove_instance(target).unwrap();
    let replacement = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(replacement, &[(42, Value::Number { bits: 100 })])
        .unwrap();
    let replaced = world.snapshot();
    assert!(proposal.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), replaced);
    let (_changed_directory, changed, changed_content) =
        fixture(&event(&assignment(&local(42), &foreign(3, 42))));
    let changed_world = seed(Arc::clone(&changed)).0;
    let changed_before = changed_world.snapshot();
    assert!(
        foreign_copy::stage(
            &changed_world,
            &prepared,
            &changed_content,
            foreign_copy::Selection {
                sequence,
                explicit_player: None,
                intent: local_copy::Intent::Engineering
            },
            Default::default()
        )
        .is_err()
    );
    assert_eq!(changed_world.snapshot(), changed_before);
    assert!(
        foreign_copy::stage(
            &world,
            &sources(&changed),
            &content,
            foreign_copy::Selection {
                sequence,
                explicit_player: None,
                intent: local_copy::Intent::Engineering
            },
            Default::default()
        )
        .is_err()
    );
    assert_eq!(world.snapshot(), replaced);
    let mut exhausted = before;
    exhausted.state_revision = u64::MAX;
    let mut world = World::restore(
        Arc::clone(&catalogue),
        exhausted.clone(),
        Default::default(),
    )
    .unwrap();
    let proposal = stage(&world, &prepared, &content, sequence, Default::default());
    assert!(proposal.commit(&mut world).is_err());
    assert_eq!(world.snapshot(), exhausted);
}

#[test]
#[ignore = "frozen CLI and authored metadata copy; no original execution"]
fn cli_saved_foreign_copy_helper() {
    use fallout_runtime::snapshot::Snapshot;
    use serde_json::{Value as Json, json};
    use sha2::{Digest, Sha256};
    use std::{
        path::{Path, PathBuf},
        process::Command,
    };
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_FOREIGN_COPY_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let executable = metadata.join("authored-source-copy/FalloutNV.exe");
    let executable_bytes = fs::read(&executable).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&executable_bytes)),
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let install_from = |name: &str, directory: &Path| {
        let install = evidence.join(name);
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        fs::copy(
            directory.join("FalloutNV.esm"),
            install.join("Data/FalloutNV.esm"),
        )
        .unwrap();
        fs::copy(&executable, install.join("FalloutNV.exe")).unwrap();
        install
    };
    let (directory, catalogue, _) = fixture(&event(&assignment(&local(42), &foreign(1, 42))));
    let install = install_from("authored-source-copy", directory.path());
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let (mut world, own, sequence) = seed(Arc::clone(&catalogue));
    let own_id = world.instance(own).unwrap().id();
    let target = world
        .create_instance(
            &definition(&catalogue, 0x301),
            Owner::Quest { key: form(0x100) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(target, &[(42, Value::Number { bits: 0 })])
        .unwrap();
    world.register_reference(Some(form(0x101))).unwrap();
    world
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let unrelated = world
        .create_instance(
            &definition(&catalogue, 0x300),
            Owner::Fragment {
                activation: 2.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(unrelated, &[(42, Value::Number { bits: 54321 })])
        .unwrap();
    world
        .enqueue(
            unrelated,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let request = json!({"schema_version":1,"sequence":sequence,"owner":{"kind":"fragment","activation":1},"intent":"engineering","explicit_player":null,
        "maximum_source_instructions":4096,"maximum_operand_uses":3,"maximum_statement_bytes":65539,"maximum_trace_source_bytes":1048576,"maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,
        "maximum_metadata_rows":4096,"maximum_probe_variable_bytes":1048576,"maximum_trace_bytes":2097152,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let mut cases = 0;
    let mut run = |name: &str,
                   source_install: &Path,
                   snapshot: &Snapshot,
                   request: &Json,
                   mode: &str,
                   extra: &[&str]| {
        cases += 1;
        let case = evidence.join(name);
        fs::create_dir(&case).unwrap();
        let input = case.join("input.snapshot.json");
        let request_path = case.join("request.json");
        let input_bytes = snapshot.encode(64 * 1024 * 1024).unwrap();
        let request_bytes = serde_json::to_vec_pretty(request).unwrap();
        fs::write(&input, &input_bytes).unwrap();
        fs::write(&request_path, &request_bytes).unwrap();
        let result = match mode {
            "result-input" => input.clone(),
            "result-protected" => source_install.join("foreign-result.json"),
            _ => case.join("result.snapshot.json"),
        };
        let report = match mode {
            "input" => input.clone(),
            "request" => request_path.clone(),
            "order" => order.clone(),
            "result" => result.clone(),
            "result-case" => case.join("RESULT.SNAPSHOT.JSON"),
            "protected" => source_install.join("foreign-report.json"),
            _ => case.join("report.json"),
        };
        if mode == "existing" {
            fs::write(&report, b"Existing report").unwrap();
        }
        if mode == "result-existing" {
            fs::write(&result, b"Existing result").unwrap();
        }
        if mode == "hardlink" {
            fs::hard_link(&input, &report).unwrap();
        }
        let mut command = Command::new(&cli);
        command
            .args(["event-operands", "--install"])
            .arg(source_install)
            .arg("--load-order")
            .arg(&order)
            .arg("--snapshot-foreign-copy-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(&input)
            .arg("--snapshot-output")
            .arg(&result)
            .args(extra);
        if mode != "stdout" {
            command.arg("--output").arg(&report);
        }
        let output = command.output().unwrap();
        fs::write(case.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(case.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(
            case.join("exit-code.json"),
            serde_json::to_vec(
                &json!({"exit_code":output.status.code(),"original_launched":false}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(fs::read(&input).unwrap(), input_bytes);
        assert_eq!(fs::read(&request_path).unwrap(), request_bytes);
        assert_eq!(fs::read(&order).unwrap(), b"[\"FalloutNV.esm\"]");
        let value = if mode == "stdout" {
            serde_json::from_slice::<Json>(&output.stdout).ok()
        } else if mode == "new" && report.exists() {
            Some(serde_json::from_slice::<Json>(&fs::read(&report).unwrap()).unwrap())
        } else {
            None
        };
        if mode == "existing" {
            assert_eq!(fs::read(&report).unwrap(), b"Existing report");
        }
        if mode == "result-existing" {
            assert_eq!(fs::read(&result).unwrap(), b"Existing result");
        }
        (output, value, result, report)
    };
    let mut baseline = None;
    for (index, bits) in [0, 1_u64 << 63, 0x7ff8_0000_0000_0203, u64::MAX]
        .into_iter()
        .enumerate()
    {
        let mut input = before.clone();
        input
            .instances
            .iter_mut()
            .find(|instance| instance.id == world.instance(target).unwrap().id())
            .unwrap()
            .locals[0]
            .value = Value::Number { bits };
        let (output, report, result, report_path) = run(
            &format!("bits-{index}"),
            &install,
            &input,
            &request,
            "new",
            &[],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = report.unwrap();
        let committed = &report["snapshot_foreign_copy"]["committed"];
        assert_eq!(committed["trace"]["copied_bits"], bits);
        assert_eq!(committed["receipt"]["assignments"], 1);
        assert_eq!(report["faithful_execution_admitted"], false);
        assert_eq!(report["retail_parity_accepted"], false);
        let mut expected = input.clone();
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        expected
            .instances
            .iter_mut()
            .find(|instance| instance.id == own_id)
            .unwrap()
            .locals
            .iter_mut()
            .find(|local| local.index == 42)
            .unwrap()
            .value = Value::Number { bits };
        let actual = Snapshot::decode(&fs::read(&result).unwrap(), Default::default()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            World::restore(Arc::clone(&catalogue), actual, Default::default())
                .unwrap()
                .snapshot(),
            expected
        );
        if index == 0 {
            baseline = Some((
                report,
                fs::metadata(&result).unwrap().len(),
                fs::metadata(&report_path).unwrap().len(),
                expected,
            ));
        }
    }
    let (report, result_bytes, report_bytes, after) = baseline.unwrap();
    let trace = &report["snapshot_foreign_copy"]["committed"]["trace"];
    let frame = &trace["frame"]["counts"];
    let mut exact = request.clone();
    for (name, value) in [
        ("maximum_source_instructions", json!(3)),
        ("maximum_operand_uses", json!(3)),
        ("maximum_statement_bytes", json!(15)),
        ("maximum_trace_source_bytes", frame["source_bytes"].clone()),
        ("maximum_trace_rows", frame["rows"].clone()),
        (
            "maximum_trace_variable_bytes",
            frame["variable_bytes"].clone(),
        ),
        ("maximum_trace_binding_uses", frame["binding_uses"].clone()),
        (
            "maximum_metadata_rows",
            trace["counts"]["metadata_rows"].clone(),
        ),
        (
            "maximum_probe_variable_bytes",
            trace["counts"]["probe_variable_reservation"].clone(),
        ),
        (
            "maximum_trace_bytes",
            json!(serde_json::to_vec(trace).unwrap().len()),
        ),
        ("maximum_result_snapshot_bytes", json!(result_bytes)),
        ("maximum_report_bytes", json!(report_bytes)),
    ] {
        exact[name] = value;
    }
    let (output, _, result, _) = run("exact0", &install, &before, &exact, "new", &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        after
    );
    for field in [
        "maximum_source_instructions",
        "maximum_operand_uses",
        "maximum_statement_bytes",
        "maximum_trace_source_bytes",
        "maximum_trace_rows",
        "maximum_trace_variable_bytes",
        "maximum_trace_binding_uses",
        "maximum_metadata_rows",
        "maximum_probe_variable_bytes",
        "maximum_trace_bytes",
        "maximum_result_snapshot_bytes",
    ] {
        let mut low = exact.clone();
        low[field] = json!(low[field].as_u64().unwrap() - 1);
        low["maximum_report_bytes"] = request["maximum_report_bytes"].clone();
        let (output, report, result, _) = run(field, &install, &before, &low, "new", &[]);
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{field}"
        );
    }
    for (name, field, value) in [
        ("schema", "schema_version", json!(2)),
        ("zero", "sequence", json!(0)),
        ("later-head", "sequence", json!(2)),
        ("owner", "owner", json!({"kind":"fragment","activation":2})),
        ("report-cap", "maximum_report_bytes", json!(1)),
        (
            "instruction-ceiling",
            "maximum_source_instructions",
            json!(4097),
        ),
        ("trace-ceiling", "maximum_trace_bytes", json!(2097153)),
        (
            "snapshot-ceiling",
            "maximum_result_snapshot_bytes",
            json!(67108865),
        ),
        ("unknown", "invented", json!(true)),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, report, result, _) = run(name, &install, &before, &invalid, "new", &[]);
        assert!(
            !output.status.success() && report.is_none() && !result.exists(),
            "{name}"
        );
    }
    for field in ["explicit_player", "maximum_metadata_rows", "owner"] {
        let mut missing = request.clone();
        missing.as_object_mut().unwrap().remove(field);
        let (output, report, result, _) = run(
            &format!("missing-{field}"),
            &install,
            &before,
            &missing,
            "new",
            &[],
        );
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    for mode in [
        "input",
        "request",
        "order",
        "result",
        "result-case",
        "protected",
        "existing",
        "result-input",
        "result-protected",
        "result-existing",
        "hardlink",
    ] {
        let (output, report, result, _) = run(
            &format!("path-{mode}"),
            &install,
            &before,
            &request,
            mode,
            &[],
        );
        assert!(!output.status.success() && report.is_none(), "{mode}");
        if mode != "result-input" && mode != "result-existing" {
            assert!(!result.exists(), "{mode}");
        }
    }
    for (name, args) in [
        ("conflict-prepared", vec!["--prepared-sources"]),
        ("conflict-native", vec!["--native-capabilities"]),
        ("conflict-player", vec!["--player-id", "1"]),
        (
            "conflict-old-copy",
            vec!["--snapshot-copy-request", "missing.json"],
        ),
    ] {
        let (output, report, result, _) = run(name, &install, &before, &request, "new", &args);
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    let (output, report, result, _) = run("stdout", &install, &before, &request, "stdout", &[]);
    assert!(output.status.success() && report.is_some() && result.exists());
    let (output, report, result, _) = run("cold-old-head", &install, &after, &request, "new", &[]);
    assert!(!output.status.success() && report.is_none() && !result.exists());
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    let (output, report, result, _) = run("faithful", &install, &before, &faithful, "new", &[]);
    assert!(!output.status.success() && !result.exists());
    assert_eq!(
        report.unwrap()["snapshot_foreign_copy"]["reason"],
        "unverified_retail_semantics"
    );
    for mode in ["uninitialized", "missing", "nonnumeric"] {
        let mut input = before.clone();
        let foreign = input
            .instances
            .iter_mut()
            .find(|instance| instance.id == world.instance(target).unwrap().id())
            .unwrap();
        match mode {
            "missing" => {
                input
                    .instances
                    .retain(|instance| instance.id != world.instance(target).unwrap().id());
            }
            "nonnumeric" => {
                foreign.definition = definition(&catalogue, 0x302);
                foreign.locals[0].value = Value::Reference {
                    value: ReferenceValue::Null,
                };
            }
            _ => foreign.locals[0].value = Value::Uninitialized,
        }
        let (output, report, result, _) = run(mode, &install, &input, &request, "new", &[]);
        assert!(!output.status.success() && !result.exists());
        assert_eq!(
            report.unwrap()["snapshot_foreign_copy"]["status"],
            "unsupported"
        );
    }
    for (index, body) in [
        assignment(&local(42), b"1"),
        assignment(&foreign(1, 42), &local(42)),
        assignment(&local(42), &foreign(5, 42)),
        assignment(&local(42), &foreign(3, 42)),
    ]
    .into_iter()
    .enumerate()
    {
        let (directory, other_catalogue, _) = fixture(&event(&body));
        let other_install = install_from(&format!("other-source-{index}"), directory.path());
        let (mut other, own, _) = seed(Arc::clone(&other_catalogue));
        let mut other_request = request.clone();
        if index == 3 {
            let mut huge = form(0x777);
            huge.origin_plugin = "x".repeat(32 * 1024);
            other
                .assign(
                    own,
                    &[(
                        90,
                        Value::Reference {
                            value: ReferenceValue::Content { key: huge },
                        },
                    )],
                )
                .unwrap();
            other_request["maximum_probe_variable_bytes"] = json!(4096);
        }
        let (output, report, result, _) = run(
            &format!("other-case-{index}"),
            &other_install,
            &other.snapshot(),
            &other_request,
            "new",
            &[],
        );
        assert!(!output.status.success() && !result.exists());
        if index == 3 {
            assert!(report.is_none());
        } else {
            assert_eq!(
                report.unwrap()["snapshot_foreign_copy"]["status"],
                "unsupported"
            );
        }
        let (output, report, result, _) = run(
            &format!("cohort-{index}"),
            &other_install,
            &before,
            &request,
            "new",
            &[],
        );
        assert!(!output.status.success() && report.is_none() && !result.exists());
    }
    for context in [2, 3, 5] {
        let (directory, catalogue, _) =
            fixture(&event(&assignment(&local(42), &foreign(context, 42))));
        let install = install_from(&format!("success-source-{context}"), directory.path());
        let (mut other, own, _) = seed(Arc::clone(&catalogue));
        let own_id = other.instance(own).unwrap().id();
        let reference = other.register_reference(Some(form(0x101))).unwrap();
        let target = other
            .create_instance(
                &definition(&catalogue, 0x301),
                Owner::Placed { reference },
                Context::default(),
            )
            .unwrap();
        other
            .assign(
                target,
                &[(
                    42,
                    Value::Number {
                        bits: 0x8000_0000_0000_0000,
                    },
                )],
            )
            .unwrap();
        if context == 3 {
            other
                .assign(
                    own,
                    &[(
                        90,
                        Value::Reference {
                            value: ReferenceValue::Live { id: reference },
                        },
                    )],
                )
                .unwrap();
        }
        let placed_owner = (context == 3).then(|| other.register_reference(None).unwrap());
        let mut input = other.snapshot();
        let owner = match context {
            2 => Owner::Quest { key: form(0x100) },
            3 => Owner::Placed {
                reference: placed_owner.unwrap(),
            },
            _ => input
                .instances
                .iter()
                .find(|i| i.id == own_id)
                .unwrap()
                .owner
                .clone(),
        };
        input
            .instances
            .iter_mut()
            .find(|i| i.id == own_id)
            .unwrap()
            .owner = owner.clone();
        assert_eq!(
            World::restore(Arc::clone(&catalogue), input.clone(), Default::default())
                .unwrap()
                .snapshot(),
            input
        );
        let mut selected = request.clone();
        selected["owner"] = serde_json::to_value(owner).unwrap();
        if context == 5 {
            selected["explicit_player"] = serde_json::to_value(reference).unwrap();
        }
        let (output, report, result, _) = run(
            &format!("success-context-{context}"),
            &install,
            &input,
            &selected,
            "new",
            &[],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            report.unwrap()["snapshot_foreign_copy"]["committed"]["trace"]["copied_bits"],
            0x8000_0000_0000_0000_u64
        );
        let mut expected = input;
        expected.state_revision += 1;
        expected.pending_events.remove(0);
        expected
            .instances
            .iter_mut()
            .find(|i| i.id == own_id)
            .unwrap()
            .locals
            .iter_mut()
            .find(|l| l.index == 42)
            .unwrap()
            .value = Value::Number {
            bits: 0x8000_0000_0000_0000,
        };
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            World::restore(Arc::clone(&catalogue), actual, Default::default())
                .unwrap()
                .snapshot(),
            expected
        );
    }
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":cases,"passed":true,"whole_snapshot_checked":true,"original_launched":false,"original_behavior_verified":false})).unwrap()).unwrap();
    assert_eq!(fs::read(&executable).unwrap(), executable_bytes);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        fs::read(directory.path().join("FalloutNV.esm")).unwrap()
    );
}
