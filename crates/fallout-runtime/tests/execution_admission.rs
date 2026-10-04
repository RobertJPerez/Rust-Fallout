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
    quest_scripts::Attachments,
    store::RecordStore,
};
use fallout_runtime::{
    execution::admission::*,
    programs::{LookupError, PreparedSources},
};
use std::fs;

fn instruction(opcode: u16, payload: &[u8]) -> Vec<u8> {
    [
        opcode.to_le_bytes().as_slice(),
        &(payload.len() as u16).to_le_bytes(),
        payload,
    ]
    .concat()
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
fn native() -> Vec<u8> {
    instruction(0x102f, &[1, 0, b'r', 1, 0])
}
fn foreign() -> Vec<u8> {
    instruction(0x15, &[b'f', 1, 0, 6, 0, b'r', 1, 0, b'f', 1, 0])
}
// Authored postfix shape only: no arithmetic or evaluation-order claim.
fn nested_local_expression(reads: usize) -> Vec<u8> {
    assert!((1..=10_000).contains(&reads));
    let mut expression = vec![b'f', 1, 0];
    for _ in 1..reads {
        expression.extend([b' ', b'f', 1, 0, b' ', b'+']);
    }
    let payload = [
        &[b'f', 1, 0][..],
        &(expression.len() as u16).to_le_bytes(),
        &expression,
    ]
    .concat();
    instruction(0x15, &payload)
}
type Unit = (u32, Vec<u8>, Vec<u32>);
fn fixture(units: &[Unit], other: Vec<Vec<u8>>) -> (tempfile::TempDir, Catalogue, Attachments) {
    let directory = tempfile::tempdir().unwrap();
    let mut records = vec![header(&[])];
    for (id, body, refs) in units {
        let refs: Vec<_> = refs.iter().map(|value| (b"SCRO", *value)).collect();
        let original = unit(&[(1, 0)], &refs);
        let mut source = original[..26].to_vec();
        source[14..18].copy_from_slice(&(body.len() as u32).to_le_bytes());
        if body.len() > u16::MAX as usize {
            source.extend(field(b"XXXX", &(body.len() as u32).to_le_bytes()));
            source.extend([b"SCDA".as_slice(), &0_u16.to_le_bytes(), body].concat());
        } else {
            source.extend(field(b"SCDA", body));
        }
        source.extend(&original[46..]);
        records.push(record(b"SCPT", *id, 0, &source));
    }
    records.extend(other);
    fs::write(directory.path().join("FalloutNV.esm"), records.concat()).unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
    (directory, catalogue, attachments)
}
fn prepared(catalogue: &Catalogue) -> PreparedSources<'_> {
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
fn root_handles(catalogue: &Catalogue) -> Vec<Handle> {
    catalogue.iter().map(|(_, s)| s.handle().clone()).collect()
}

#[test]
fn physical_dependencies_and_cycles_have_exact_offsets_and_deterministic_priority() {
    let (_directory, catalogue, attachments) = fixture(
        &[
            (0x300, event(&native()), vec![0x301]),
            (0x301, event(&native()), vec![0x300]),
        ],
        vec![],
    );
    let sources = prepared(&catalogue);
    let handles = root_handles(&catalogue);
    let report = check(&sources, &attachments, &handles[..1], Limits::default()).unwrap();
    assert_eq!(report.definitions, handles);
    assert_eq!(report.first_unsupported.definition, handles[0]);
    assert_eq!(
        report.first_unsupported.code,
        Code::UnverifiedNativeSemantics
    );
    assert_eq!(report.first_unsupported.source_scda_offset, Some(10));
    assert_eq!(
        report.first_unsupported.instruction_scda_bytes,
        Some(10..19)
    );
    assert_eq!(report.first_unsupported.operand_scda_bytes, Some(14..19));
    assert_eq!(report.dependencies.len(), 2);
    assert!(
        report
            .dependencies
            .iter()
            .all(|d| d.operand_scda_offset == 17 && d.reference_index == 1)
    );
    assert_eq!(report.cycle_back_edges.len(), 1);
    assert_eq!(report.cycle_back_edges[0].from, handles[1].key);
    assert_eq!(report.cycle_back_edges[0].to, handles[0].key);
    assert!(!report.faithful_execution_admitted && !report.retail_lifecycle_verified);
    assert_eq!(report.source_cohort_sha256, sources.source_cohort_sha256());
    let reversed = check(
        &sources,
        &attachments,
        &[handles[1].clone(), handles[0].clone(), handles[0].clone()],
        Limits::default(),
    )
    .unwrap();
    assert_eq!(reversed.roots, handles);
    assert_eq!(
        serde_json::to_value(report.first_unsupported).unwrap(),
        serde_json::to_value(reversed.first_unsupported).unwrap()
    );
}

#[test]
fn static_foreign_quest_declarations_reuse_the_existing_join_and_dynamic_context_stays_unsupported()
{
    let quest = |id, script: u32| record(b"QUST", id, 0, &field(b"SCRI", &script.to_le_bytes()));
    let (_directory, catalogue, attachments) = fixture(
        &[
            (0x300, event(&foreign()), vec![0x200]),
            (0x301, event(&foreign()), vec![0x201]),
        ],
        vec![quest(0x200, 0x301), quest(0x201, 0x300)],
    );
    let sources = prepared(&catalogue);
    let handles = root_handles(&catalogue);
    let report = check(&sources, &attachments, &handles[..1], Limits::default()).unwrap();
    assert_eq!(report.definitions, handles);
    assert_eq!(report.dependencies.len(), 2);
    assert_eq!(report.dependencies[0].operand_scda_offset, 23);
    assert!(matches!(
        report.dependencies[0].kind,
        DependencyKind::ForeignQuestDeclaration
    ));
    assert!(report.dependency_findings.is_empty());
    assert_eq!(report.first_unsupported.code, Code::UnverifiedAssignment);
    assert_eq!(report.cycle_back_edges.len(), 1);
    let (_directory2, missing, attachments2) = fixture(
        &[(0x300, event(&foreign()), vec![0x200])],
        vec![record(b"REFR", 0x200, 0, &[])],
    );
    let sources2 = prepared(&missing);
    let report = check(
        &sources2,
        &attachments2,
        &root_handles(&missing),
        Limits::default(),
    )
    .unwrap();
    assert!(report.dependencies.is_empty());
    assert_eq!(report.dependency_findings.len(), 1);
    assert_eq!(
        report.dependency_findings[0].code,
        Code::ForeignContextUnavailable
    );
    assert_eq!(report.dependency_findings[0].source_scda_offset, Some(23));
    assert!(
        report.dependency_findings[0]
            .detail
            .contains("PlacedReferenceNeedsEventList")
    );
}

#[test]
fn exact_resource_limits_succeed_and_one_less_never_returns_a_partial_graph() {
    let (_directory, catalogue, attachments) = fixture(
        &[
            (0x300, event(&native()), vec![0x301]),
            (0x301, event(&native()), vec![0x300]),
        ],
        vec![],
    );
    let sources = prepared(&catalogue);
    let roots = root_handles(&catalogue);
    let limits = Limits {
        maximum_roots: 1,
        maximum_definitions: 2,
        maximum_dependencies: 2,
        maximum_instructions: 6,
        maximum_operand_uses: 2,
    };
    assert_eq!(
        check(&sources, &attachments, &roots[..1], limits)
            .unwrap()
            .definitions
            .len(),
        2
    );
    for (limits, label) in [
        (
            Limits {
                maximum_roots: 0,
                ..limits
            },
            "roots",
        ),
        (
            Limits {
                maximum_definitions: 1,
                ..limits
            },
            "definitions",
        ),
        (
            Limits {
                maximum_dependencies: 1,
                ..limits
            },
            "dependencies",
        ),
        (
            Limits {
                maximum_instructions: 5,
                ..limits
            },
            "instructions",
        ),
    ] {
        assert!(
            matches!(check(&sources,&attachments,&roots[..1],limits),Err(Error::Capacity(name)) if name==label)
        );
    }
}

#[test]
fn own_local_nested_expressions_debit_a_global_admission_work_budget_with_exact_sites() {
    let (_directory, catalogue, attachments) = fixture(
        &[
            (0x300, event(&nested_local_expression(8)), vec![]),
            (0x301, event(&nested_local_expression(8)), vec![]),
        ],
        vec![],
    );
    let sources = prepared(&catalogue);
    let roots = root_handles(&catalogue);
    for root in &roots {
        let plan = sources.get(root).unwrap().plan();
        assert_eq!(plan.bindings().uses.len(), 9);
        assert_eq!(plan.nodes(), 15);
        assert_eq!(&plan.control().bytes()[59..62], &[b'f', 1, 0]);
    }
    let limits = Limits {
        maximum_roots: 3,
        maximum_definitions: 2,
        maximum_dependencies: 0,
        maximum_instructions: 6,
        maximum_operand_uses: 17,
    };
    let result = check(&sources, &attachments, &roots, limits);
    let Err(Error::OperandUseBudget {
        definition,
        source_scda_offset,
    }) = result
    else {
        panic!("own-local uses bypassed total admission work ceiling");
    };
    assert_eq!(*definition, roots[1]);
    assert_eq!(source_scda_offset, 60); // eighth RHS local index in authored bytes
    let result = check(
        &sources,
        &attachments,
        &[roots[1].clone(), roots[0].clone(), roots[0].clone()],
        limits,
    );
    assert!(
        matches!(result,Err(Error::OperandUseBudget{definition,source_scda_offset:60}) if *definition==roots[1])
    );
    let report = check(
        &sources,
        &attachments,
        &roots,
        Limits {
            maximum_operand_uses: 18,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(report.definitions, roots);
    assert!(report.dependencies.is_empty() && report.dependency_findings.is_empty());
    assert_eq!(report.first_unsupported.source_scda_offset, Some(10));
    assert!(!report.faithful_execution_admitted);
    let result = check(
        &sources,
        &attachments,
        &roots[..1],
        Limits {
            maximum_operand_uses: 0,
            ..limits
        },
    );
    assert!(
        matches!(result,Err(Error::OperandUseBudget{definition,source_scda_offset:15}) if *definition==roots[0])
    );
}

#[test]
fn stale_versions_conflicts_missing_roots_and_changed_full_cohorts_refuse() {
    let (_directory, catalogue, attachments) = fixture(&[(0x300, event(&[]), vec![])], vec![]);
    let sources = prepared(&catalogue);
    let handles = root_handles(&catalogue);
    assert!(matches!(
        check(&sources, &attachments, &[], Limits::default()),
        Err(Error::EmptyRoots)
    ));
    let mut stale = handles[0].clone();
    stale.version_sha256 = "f".repeat(64);
    assert!(matches!(
        check(&sources, &attachments, &[stale.clone()], Limits::default()),
        Err(Error::Source(LookupError::DefinitionChanged))
    ));
    assert!(matches!(
        check(
            &sources,
            &attachments,
            &[handles[0].clone(), stale],
            Limits::default()
        ),
        Err(Error::ConflictingVersion)
    ));
    let mut request = Request {
        schema_version: 1,
        source_cohort_sha256: sources.source_cohort_sha256().into(),
        roots: handles,
    };
    assert_eq!(
        request
            .check(&sources, &attachments, Limits::default())
            .unwrap()
            .first_unsupported
            .code,
        Code::UnverifiedEventLifecycle
    );
    request.source_cohort_sha256 = "f".repeat(64);
    assert!(matches!(
        request.check(&sources, &attachments, Limits::default()),
        Err(Error::CohortChanged)
    ));
    request.schema_version = 2;
    assert!(matches!(
        request.check(&sources, &attachments, Limits::default()),
        Err(Error::SchemaVersion(2))
    ));
    assert!(
        serde_json::from_str::<Request>(
            r#"{"schema_version":1,"source_cohort_sha256":"x","roots":[],"unknown":true}"#
        )
        .is_err()
    );
}

#[test]
fn cached_source_rejections_retain_the_first_exact_failure_without_reparsing() {
    let (_directory, catalogue, attachments) =
        fixture(&[(0x300, event(&instruction(0x19, &[])), vec![])], vec![]);
    let sources = prepared(&catalogue);
    let roots = root_handles(&catalogue);
    let attempts = sources.counts().preparation_attempts;
    for _ in 0..2 {
        let report = check(&sources, &attachments, &roots, Limits::default()).unwrap();
        assert_eq!(report.first_unsupported.code, Code::SourcePlanUnavailable);
        assert_eq!(report.first_unsupported.source_scda_offset, Some(10));
        assert!(!report.faithful_execution_admitted);
        assert_eq!(sources.counts().preparation_attempts, attempts);
    }
}

fn shared_graph() -> (tempfile::TempDir, Catalogue, Attachments) {
    let call = |index: u8| instruction(0x102f, &[1, 0, b'r', index, 0]);
    fixture(
        &[
            (
                0x300,
                event(&[call(1), call(2), call(1)].concat()),
                vec![0x301, 0x302],
            ),
            (0x301, event(&call(1)), vec![0x303]),
            (0x302, event(&call(1)), vec![0x303]),
            (0x303, event(&call(1)), vec![0x300]),
        ],
        vec![],
    )
}
fn shared_expected(sources: &PreparedSources<'_>, handles: &[Handle]) -> serde_json::Value {
    use serde_json::json;
    let dependency = |from: usize, to: usize, offset: usize, index: u16| {
        json!({"from":handles[from],"to":handles[to],"kind":"declared_script_reference",
            "operand_scda_offset":offset,"reference_index":index})
    };
    json!({
        "source_cohort_sha256":sources.source_cohort_sha256(),"roots":[handles[0]],
        "definitions":handles,
        "dependencies":[dependency(0,1,17,1),dependency(0,2,26,2),dependency(0,1,35,1),
            dependency(1,3,17,1),dependency(2,3,17,1),dependency(3,0,17,1)],
        "dependency_findings":[],"cycle_back_edges":[{"from":handles[3].key,"to":handles[0].key}],
        "first_unsupported":{"definition":handles[0],"source_scda_offset":10,
            "instruction_scda_bytes":{"start":10,"end":19},"operand_scda_bytes":{"start":14,"end":19},
            "opcode":0x102f,"calling_reference_index":null,"code":"unverified_native_semantics",
            "detail":"Engineering host reads do not implement the original native return"},
        "faithful_execution_admitted":false,"retail_lifecycle_verified":false
    })
}
#[test]
fn cooperative_schedules_preserve_independent_complete_graph_and_never_revisit_or_reparse() {
    let (_directory, catalogue, attachments) = shared_graph();
    let sources = prepared(&catalogue);
    let handles = root_handles(&catalogue);
    let expected = shared_expected(&sources, &handles);
    assert_eq!(
        serde_json::to_value(
            check(&sources, &attachments, &handles[..1], Default::default()).unwrap()
        )
        .unwrap(),
        expected
    );
    let attempts = sources.counts().preparation_attempts;
    for (definitions, operands) in [(usize::MAX, usize::MAX), (1, 1), (2, 2), (1, 3), (3, 1)] {
        let mut job = AdmissionJob::new(
            &sources,
            &attachments,
            &handles[..1],
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let mut expansions = 0;
        let mut visits = 0;
        loop {
            let p = job.advance(StepBudget {
                maximum_definition_expansions: definitions,
                maximum_operand_visits: operands,
            });
            assert!(
                p.step_definition_expansions <= definitions && p.step_operand_visits <= operands
            );
            expansions += p.step_definition_expansions;
            visits += p.step_operand_visits;
            assert_eq!(p.definition_expansions, expansions);
            assert_eq!(p.operand_visits, visits);
            assert_eq!(sources.counts().preparation_attempts, attempts);
            if p.status == AdmissionStatus::Complete {
                assert_eq!(expansions, 4);
                assert_eq!(visits, 6);
                assert_eq!(p.charged_instructions, 14);
                assert_eq!(p.charged_operand_uses, 6);
                assert_eq!(p.dependencies, 6);
                let terminal = job.advance(StepBudget {
                    maximum_definition_expansions: 0,
                    maximum_operand_visits: 0,
                });
                assert_eq!(terminal, job.progress());
                assert_eq!(
                    serde_json::to_value(job.finish().unwrap()).unwrap(),
                    expected
                );
                break;
            }
            assert_eq!(p.status, AdmissionStatus::Pending);
            assert!(p.step_definition_expansions + p.step_operand_visits > 0);
        }
    }
    let mut job = AdmissionJob::new(
        &sources,
        &attachments,
        &handles[..1],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let initial = job.progress();
    assert_eq!(
        job.advance(StepBudget {
            maximum_definition_expansions: 0,
            maximum_operand_visits: 0
        }),
        initial
    );
    assert!(matches!(job.finish(), Err(Error::Incomplete)));
    let mut cancelled = AdmissionJob::new(
        &sources,
        &attachments,
        &handles[..1],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        cancelled
            .advance(StepBudget {
                maximum_definition_expansions: 1,
                maximum_operand_visits: 1
            })
            .status,
        AdmissionStatus::Pending
    );
    drop(cancelled);
    assert_eq!(sources.counts().preparation_attempts, attempts);
    let reordered = [
        handles[2].clone(),
        handles[0].clone(),
        handles[1].clone(),
        handles[0].clone(),
    ];
    let mut job = AdmissionJob::new(
        &sources,
        &attachments,
        &reordered,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    while job
        .advance(StepBudget {
            maximum_definition_expansions: 1,
            maximum_operand_visits: 1,
        })
        .status
        == AdmissionStatus::Pending
    {}
    let mut all_roots = expected.clone();
    all_roots["roots"] = serde_json::json!([handles[0], handles[1], handles[2]]);
    assert_eq!(
        serde_json::to_value(job.finish().unwrap()).unwrap(),
        all_roots
    );
    let mut stale = handles[0].clone();
    stale.version_sha256 = "f".repeat(64);
    let conflict = [handles[0].clone(), stale.clone()];
    assert!(matches!(
        AdmissionJob::new(
            &sources,
            &attachments,
            &conflict,
            Default::default(),
            Default::default()
        ),
        Err(Error::ConflictingVersion)
    ));
    let stale_root = [stale];
    let mut job = AdmissionJob::new(
        &sources,
        &attachments,
        &stale_root,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        job.advance(StepBudget {
            maximum_definition_expansions: 1,
            maximum_operand_visits: 1
        })
        .status,
        AdmissionStatus::Failed
    );
    assert!(matches!(
        job.finish(),
        Err(Error::Source(LookupError::DefinitionChanged))
    ));
}
#[test]
fn cooperative_global_and_copy_limits_never_reset_or_publish_a_partial_report() {
    let (_directory, catalogue, attachments) = shared_graph();
    let sources = prepared(&catalogue);
    let roots = root_handles(&catalogue);
    let limits = Limits {
        maximum_roots: 1,
        maximum_definitions: 4,
        maximum_dependencies: 6,
        maximum_instructions: 14,
        maximum_operand_uses: 6,
    };
    let mut sample = AdmissionJob::new(
        &sources,
        &attachments,
        &roots[..1],
        limits,
        Default::default(),
    )
    .unwrap();
    let complete = sample.advance(StepBudget {
        maximum_definition_expansions: usize::MAX,
        maximum_operand_visits: usize::MAX,
    });
    assert_eq!(complete.status, AdmissionStatus::Complete);
    let jobs = JobLimits {
        maximum_frontier: 2,
        maximum_variable_bytes: complete.variable_bytes,
        maximum_lookup_source_visits: 0,
        maximum_indivisible_instructions: 5,
    };
    for n in 0..7 {
        let mut bound = limits;
        let mut job_bound = jobs;
        match n {
            0 => bound.maximum_definitions -= 1,
            1 => bound.maximum_dependencies -= 1,
            2 => bound.maximum_instructions -= 1,
            3 => bound.maximum_operand_uses -= 1,
            4 => job_bound.maximum_frontier -= 1,
            5 => job_bound.maximum_variable_bytes -= 1,
            _ => job_bound.maximum_indivisible_instructions -= 1,
        }
        let mut job =
            AdmissionJob::new(&sources, &attachments, &roots[..1], bound, job_bound).unwrap();
        loop {
            let p = job.advance(StepBudget {
                maximum_definition_expansions: 1,
                maximum_operand_visits: 1,
            });
            if p.status == AdmissionStatus::Failed {
                let failed = job.progress();
                assert_eq!(
                    job.advance(StepBudget {
                        maximum_definition_expansions: usize::MAX,
                        maximum_operand_visits: usize::MAX
                    }),
                    failed
                );
                assert!(job.finish().is_err());
                break;
            }
            assert_ne!(
                p.status,
                AdmissionStatus::Complete,
                "{n} bypassed global cap"
            );
        }
    }
    let mut exact = AdmissionJob::new(&sources, &attachments, &roots[..1], limits, jobs).unwrap();
    while exact
        .advance(StepBudget {
            maximum_definition_expansions: 1,
            maximum_operand_visits: 1,
        })
        .status
        == AdmissionStatus::Pending
    {}
    assert_eq!(
        serde_json::to_value(exact.finish().unwrap()).unwrap(),
        shared_expected(&sources, &roots)
    );
    assert!(matches!(
        AdmissionJob::new(
            &sources,
            &attachments,
            &roots[..1],
            Limits {
                maximum_roots: 0,
                ..limits
            },
            jobs
        ),
        Err(Error::Capacity("roots"))
    ));
    assert!(matches!(
        AdmissionJob::new(
            &sources,
            &attachments,
            &roots[..1],
            limits,
            JobLimits {
                maximum_frontier: 0,
                ..jobs
            }
        ),
        Err(Error::Capacity("frontier"))
    ));
    assert!(matches!(
        AdmissionJob::new(
            &sources,
            &attachments,
            &roots[..1],
            limits,
            JobLimits {
                maximum_variable_bytes: 0,
                ..jobs
            }
        ),
        Err(Error::Capacity("variable bytes"))
    ));
}
#[test]
fn cooperative_foreign_lookup_visits_and_cached_rejections_keep_exact_diagnostics() {
    let quest = |id, script: u32| record(b"QUST", id, 0, &field(b"SCRI", &script.to_le_bytes()));
    let (_directory, catalogue, attachments) = fixture(
        &[
            (0x300, event(&foreign()), vec![0x200]),
            (0x301, event(&foreign()), vec![0x201]),
        ],
        vec![quest(0x200, 0x301), quest(0x201, 0x300)],
    );
    let sources = prepared(&catalogue);
    let roots = root_handles(&catalogue);
    for maximum in [1, 2] {
        let mut job = AdmissionJob::new(
            &sources,
            &attachments,
            &roots[..1],
            Default::default(),
            JobLimits {
                maximum_lookup_source_visits: maximum,
                ..Default::default()
            },
        )
        .unwrap();
        loop {
            let p = job.advance(StepBudget {
                maximum_definition_expansions: 1,
                maximum_operand_visits: 1,
            });
            if p.status != AdmissionStatus::Pending {
                if maximum == 1 {
                    assert_eq!(p.status, AdmissionStatus::Failed);
                    assert!(matches!(
                        job.finish(),
                        Err(Error::Capacity("lookup source visits"))
                    ));
                } else {
                    assert_eq!(p.lookup_source_visits, 2);
                    // Each source has an own write, caller prefix and foreign
                    // read. Prefix visits count even though they add no edge.
                    assert_eq!(p.operand_visits, 6);
                    let expected =
                        check(&sources, &attachments, &roots[..1], Default::default()).unwrap();
                    assert_eq!(
                        serde_json::to_value(job.finish().unwrap()).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                }
                break;
            }
        }
    }
    let (_directory, bad, attachments) =
        fixture(&[(0x300, event(&instruction(0x19, &[])), vec![])], vec![]);
    let cached = prepared(&bad);
    let roots = root_handles(&bad);
    let attempts = cached.counts().preparation_attempts;
    let mut job = AdmissionJob::new(
        &cached,
        &attachments,
        &roots,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let p = job.advance(StepBudget {
        maximum_definition_expansions: 1,
        maximum_operand_visits: 0,
    });
    assert_eq!(p.status, AdmissionStatus::Complete);
    assert_eq!(p.operand_visits, 0);
    let report = job.finish().unwrap();
    assert_eq!(report.first_unsupported.code, Code::SourcePlanUnavailable);
    assert_eq!(report.first_unsupported.source_scda_offset, Some(10));
    assert_eq!(cached.counts().preparation_attempts, attempts);
    for maximum in [p.variable_bytes, p.variable_bytes - 1] {
        let mut limited = AdmissionJob::new(
            &cached,
            &attachments,
            &roots,
            Default::default(),
            JobLimits {
                maximum_variable_bytes: maximum,
                ..Default::default()
            },
        )
        .unwrap();
        let progress = limited.advance(StepBudget {
            maximum_definition_expansions: 1,
            maximum_operand_visits: 0,
        });
        if maximum == p.variable_bytes {
            assert_eq!(progress.status, AdmissionStatus::Complete);
            assert_eq!(
                serde_json::to_value(limited.finish().unwrap()).unwrap(),
                serde_json::to_value(&report).unwrap()
            );
        } else {
            assert_eq!(progress.status, AdmissionStatus::Failed);
            assert!(matches!(
                limited.finish(),
                Err(Error::Capacity("variable bytes"))
            ));
        }
        assert_eq!(cached.counts().preparation_attempts, attempts);
    }
}

#[test]
#[ignore = "built CLI and authored metadata inputs; source work bounds, no original launch"]
fn cli_operand_use_budget_helper() {
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = std::path::PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("input"));
    let evidence = std::path::PathBuf::from(
        std::env::var_os("RF_SCRIPT_OPERAND_BUDGET_EVIDENCE").expect("evidence"),
    );
    fs::create_dir(&evidence).unwrap();
    for (name, last_reads, total) in [("exact", 5529, 65536), ("one-over", 5530, 65537)] {
        let mut middle = Vec::new();
        for _ in 0..6 {
            middle.extend(nested_local_expression(10_000));
        }
        middle.extend(nested_local_expression(last_reads));
        let (_temporary, catalogue, _attachments) =
            fixture(&[(0x300, event(&middle), vec![])], vec![]);
        let sources = prepared(&catalogue);
        let roots = root_handles(&catalogue);
        let plan = sources.get(&roots[0]).unwrap().plan();
        assert_eq!(plan.bindings().uses.len(), total);
        let expected_cutoff = plan.bindings().uses.get(65536).map(|use_| use_.scda_offset);
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let install = directory.join("authored-source-copy");
        fs::create_dir(&install).unwrap();
        fs::create_dir(install.join("Data")).unwrap();
        fs::copy(
            _temporary.path().join("FalloutNV.esm"),
            install.join("Data/FalloutNV.esm"),
        )
        .unwrap();
        fs::copy(
            input.join("authored-source-copy/FalloutNV.exe"),
            install.join("FalloutNV.exe"),
        )
        .unwrap();
        let order = directory.join("order.json");
        fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
        let request = serde_json::json!({"schema_version":1,"source_cohort_sha256":sources.source_cohort_sha256(),"roots":roots});
        let request_path = directory.join("request.json");
        fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
        let report_path = directory.join("report.json");
        let output = std::process::Command::new(&cli)
            .args(["source-plans", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--execution-admission")
            .arg(&request_path)
            .arg("--output")
            .arg(&report_path)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(
            directory.join("source-shape.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"authored_structural_work_budget_only","operand_uses":total,
            "instructions":plan.control().instructions().len(),"nodes":plan.nodes(),
            "first_excluded_scda_offset":expected_cutoff,"original_executed":false}))
            .unwrap(),
        )
        .unwrap();
        assert!(!output.status.success()); // Faithful arithmetic is unmeasured.
        if name == "exact" {
            let report: serde_json::Value =
                serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
            assert_eq!(
                report["execution_admission"]["first_unsupported"]["code"],
                "unverified_assignment"
            );
            assert_eq!(
                report["execution_admission"]["first_unsupported"]["source_scda_offset"],
                10
            );
            assert_eq!(
                report["execution_admission"]["faithful_execution_admitted"],
                false
            );
            assert!(
                report["execution_admission"]["dependencies"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        } else {
            assert!(!report_path.exists());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("operand-use budget exceeded"), "{stderr}");
            assert!(stderr.contains(&format!("SCDA operand 0x{:X}", expected_cutoff.unwrap())));
        }
    }
}

#[test]
#[ignore = "requires a built CLI and prior synthetic import evidence; no retail engine launch"]
fn cli_admission_helper() {
    use std::process::Command;
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = std::path::PathBuf::from(
        std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("input evidence"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("RF_SCRIPT_ADMISSION_EVIDENCE").expect("output evidence"),
    );
    fs::create_dir(&output).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(input.join("manifest.json")).unwrap()).unwrap();
    let request = serde_json::json!({"schema_version":1,"source_cohort_sha256":manifest["identity"]["source_cohort_sha256"],"roots":[manifest["identity"]["definition"]]});
    let path = output.join("request.json");
    fs::write(&path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
    let report = output.join("report.json");
    let result = Command::new(&cli)
        .args(["source-plans", "--install"])
        .arg(input.join("authored-source-copy"))
        .arg("--load-order")
        .arg(input.join("order.json"))
        .arg("--execution-admission")
        .arg(&path)
        .arg("--output")
        .arg(&report)
        .output()
        .unwrap();
    fs::write(output.join("stdout.txt"), result.stdout).unwrap();
    fs::write(output.join("stderr.txt"), result.stderr).unwrap();
    assert!(!result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(
        value["execution_admission"]["first_unsupported"]["code"],
        "unverified_assignment"
    );
    assert_eq!(
        value["execution_admission"]["first_unsupported"]["source_scda_offset"],
        10
    );
    assert_eq!(
        value["execution_admission"]["faithful_execution_admitted"],
        false
    );
    assert_eq!(value["retail_parity_accepted"], false);
    let mut altered = request;
    altered["source_cohort_sha256"] = serde_json::json!("0".repeat(64));
    let altered_path = output.join("altered-cohort.json");
    fs::write(&altered_path, serde_json::to_vec_pretty(&altered).unwrap()).unwrap();
    let rejected = output.join("altered-report.json");
    let result = Command::new(cli)
        .args(["source-plans", "--install"])
        .arg(input.join("authored-source-copy"))
        .arg("--load-order")
        .arg(input.join("order.json"))
        .arg("--execution-admission")
        .arg(altered_path)
        .arg("--output")
        .arg(&rejected)
        .output()
        .unwrap();
    fs::write(output.join("altered-stderr.txt"), result.stderr).unwrap();
    assert!(!result.status.success());
    assert!(!rejected.exists());
}

fn cooperative_cli_run(
    cli: &std::path::Path,
    install: &std::path::Path,
    order: &std::path::Path,
    work: &std::path::Path,
    raw: &[u8],
    extra: &[&str],
) -> (std::process::Output, std::path::PathBuf) {
    fs::create_dir(work).unwrap();
    let request = work.join("request.json");
    let report = work.join("report.json");
    fs::write(&request, raw).unwrap();
    let input = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let metadata = fs::read(install.join("FalloutNV.exe")).unwrap();
    let order_bytes = fs::read(order).unwrap();
    let output = std::process::Command::new(cli)
        .args(["source-plans", "--install"])
        .arg(install)
        .arg("--load-order")
        .arg(order)
        .arg("--cooperative-admission")
        .arg(&request)
        .arg("--output")
        .arg(&report)
        .args(extra)
        .output()
        .unwrap();
    fs::write(work.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(work.join("stderr.txt"), &output.stderr).unwrap();
    fs::write(work.join("process.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "success":output.status.success(),"exit_code":output.status.code(),"report_exists":report.exists()
    })).unwrap()).unwrap();
    assert_eq!(fs::read(request).unwrap(), raw);
    assert_eq!(fs::read(install.join("Data/FalloutNV.esm")).unwrap(), input);
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), metadata);
    assert_eq!(fs::read(order).unwrap(), order_bytes);
    (output, report)
}
#[test]
#[ignore = "requires explicitly frozen CLI and fresh authored evidence; never Original execution"]
fn cli_cooperative_admission_helper() {
    use serde_json::{Value as Json, json};
    use std::{cell::Cell, path::PathBuf};
    let cli = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_CLI").unwrap());
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").unwrap());
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_COOPERATIVE_ADMISSION_EVIDENCE").unwrap());
    assert!(cli.is_file());
    assert!(!evidence.exists());
    fs::create_dir(&evidence).unwrap();
    let (directory, catalogue, _attachments) = shared_graph();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        directory.path().join("FalloutNV.esm"),
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
    let sources = prepared(&catalogue);
    let roots = root_handles(&catalogue);
    let expected = shared_expected(&sources, &roots);
    let request = json!({"schema_version":1,"source_cohort_sha256":sources.source_cohort_sha256(),"roots":[roots[0]],
        "maximum_roots":32,"maximum_definitions":256,"maximum_dependencies":4096,"maximum_instructions":65536,
        "maximum_operand_uses":65536,"maximum_frontier":256,"maximum_variable_bytes":2097152,
        "maximum_lookup_source_visits":16777216,"maximum_indivisible_instructions":65536,
        "maximum_definition_expansions_per_advance":1,"maximum_operand_visits_per_advance":1,"maximum_advances":64,
        "cancel_after_advances":null,"maximum_progress_bytes":2097152,"maximum_report_bytes":8388608});
    let cases = Cell::new(0_usize);
    let raw_run = |name: &str, raw: &[u8], extra: &[&str]| {
        cases.set(cases.get() + 1);
        cooperative_cli_run(&cli, &install, &order, &evidence.join(name), raw, extra)
    };
    let run =
        |name: &str, request: &Json| raw_run(name, &serde_json::to_vec(request).unwrap(), &[]);
    let no_report = |name: &str, request: &Json| {
        let (output, path) = run(name, request);
        assert!(!output.status.success(), "{name}");
        assert!(!path.exists(), "{name}: partial report published");
    };
    let (output, path) = run("base-0", &request);
    assert!(!output.status.success()); // structural completion never admits faithful execution
    let baseline_bytes = fs::read(path).unwrap();
    let baseline: Json = serde_json::from_slice(&baseline_bytes).unwrap();
    assert_eq!(baseline["execution_admission"], expected);
    assert_eq!(baseline["cooperative_admission"]["outcome"], "complete");
    assert_eq!(baseline["cooperative_admission"]["report_published"], true);
    assert_eq!(baseline["cooperative_admission"]["hard_time_slice"], false);
    let steps = baseline["cooperative_admission"]["advances"]
        .as_array()
        .unwrap();
    assert_eq!(steps.len(), 6);
    assert_eq!(steps.last().unwrap()["definition_expansions"], 4);
    assert_eq!(steps.last().unwrap()["operand_visits"], 6);
    assert_eq!(steps.last().unwrap()["charged_instructions"], 14);
    assert_eq!(steps.last().unwrap()["charged_operand_uses"], 6);
    assert_eq!(baseline["prepared_counts"]["preparation_attempts"], 4);
    for (n, definitions, operands) in [(0, 256, 65536), (1, 2, 2), (2, 1, 3), (3, 3, 1)] {
        let mut changed = request.clone();
        changed["maximum_definition_expansions_per_advance"] = json!(definitions);
        changed["maximum_operand_visits_per_advance"] = json!(operands);
        let (output, path) = run(&format!("schedule-{n}"), &changed);
        assert!(!output.status.success());
        let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(report["execution_admission"], expected);
        assert_eq!(report["prepared_counts"], baseline["prepared_counts"]);
    }
    let mut changed = request.clone();
    changed["roots"] = json!([roots[2], roots[0], roots[1], roots[0]]);
    let (_, path) = run("root-order-duplicates", &changed);
    let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let mut all_roots = expected.clone();
    all_roots["roots"] = json!([roots[0], roots[1], roots[2]]);
    assert_eq!(report["execution_admission"], all_roots);
    for (n, kind, field, value) in [
        (0, "cancelled", "cancel_after_advances", json!(0)),
        (1, "cancelled", "cancel_after_advances", json!(1)),
        (2, "advance_budget", "maximum_advances", json!(1)),
        (
            3,
            "step_budget",
            "maximum_definition_expansions_per_advance",
            json!(0),
        ),
        (
            4,
            "step_budget",
            "maximum_operand_visits_per_advance",
            json!(0),
        ),
    ] {
        let mut changed = request.clone();
        changed[field] = value;
        let (output, path) = run(&format!("unfinished-{n}"), &changed);
        assert!(!output.status.success());
        let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(report["cooperative_admission"]["outcome"], kind);
        assert_eq!(report["cooperative_admission"]["report_published"], false);
        assert!(report["execution_admission"].is_null());
    }
    let last = steps.last().unwrap();
    let exact_fields = [
        ("maximum_roots", 1),
        ("maximum_definitions", 4),
        ("maximum_dependencies", 6),
        ("maximum_instructions", 14),
        ("maximum_operand_uses", 6),
        ("maximum_frontier", 2),
        (
            "maximum_variable_bytes",
            last["variable_bytes"].as_u64().unwrap(),
        ),
        ("maximum_indivisible_instructions", 5),
        ("maximum_advances", 6),
        (
            "maximum_progress_bytes",
            baseline["cooperative_admission"]["progress_bytes"]
                .as_u64()
                .unwrap(),
        ),
        ("maximum_report_bytes", baseline_bytes.len() as u64),
    ];
    let mut exact = request.clone();
    exact["maximum_lookup_source_visits"] = json!(0);
    for (key, value) in exact_fields {
        exact[key] = json!(value);
    }
    let (_, path) = run("exact0", &exact);
    let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(report["execution_admission"], expected);
    for (n, (key, value)) in exact_fields.into_iter().enumerate() {
        let mut low = exact.clone();
        low[key] = json!(value - 1);
        let (output, path) = run(&format!("one-under-{n}"), &low);
        assert!(!output.status.success());
        if path.exists() {
            let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert_eq!(report["cooperative_admission"]["report_published"], false);
            assert!(report["execution_admission"].is_null());
        }
    }
    for key in [
        "maximum_roots",
        "maximum_definitions",
        "maximum_dependencies",
        "maximum_instructions",
        "maximum_operand_uses",
        "maximum_frontier",
        "maximum_variable_bytes",
        "maximum_lookup_source_visits",
        "maximum_indivisible_instructions",
        "maximum_definition_expansions_per_advance",
        "maximum_operand_visits_per_advance",
        "maximum_progress_bytes",
        "maximum_report_bytes",
    ] {
        let mut over = request.clone();
        over[key] = json!(match key {
            "maximum_definition_expansions_per_advance" => 257,
            "maximum_operand_visits_per_advance" => 65537,
            _ => request[key].as_u64().unwrap() + 1,
        });
        no_report(&format!("ceiling-{key}"), &over);
    }
    for key in request.as_object().unwrap().keys() {
        let mut missing = request.clone();
        missing.as_object_mut().unwrap().remove(key);
        no_report(&format!("missing-{key}"), &missing);
    }
    for (name, field, value) in [
        ("schema", "schema_version", json!(0)),
        ("cohort", "source_cohort_sha256", json!("0".repeat(64))),
        ("empty-roots", "roots", json!([])),
        ("zero-advances", "maximum_advances", json!(0)),
        ("advance-ceiling", "maximum_advances", json!(4097)),
        ("cancel-late", "cancel_after_advances", json!(65)),
        ("cancel-type", "cancel_after_advances", json!({"count":0})),
        ("unknown", "extra", json!(true)),
    ] {
        let mut changed = request.clone();
        changed[field] = value;
        no_report(name, &changed);
    }
    let mut wrong = request.clone();
    wrong["roots"][0]["version_sha256"] = json!("f".repeat(64));
    let (_, path) = run("stale-root", &wrong);
    let stale: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(stale["cooperative_admission"]["outcome"], "failed");
    assert!(stale["execution_admission"].is_null());
    wrong["roots"] = json!([roots[0], wrong["roots"][0]]);
    no_report("conflicting-version", &wrong);
    for (n, path) in [
        vec!["extra"],
        vec!["key", "extra"],
        vec!["key", "record", "extra"],
    ]
    .into_iter()
    .enumerate()
    {
        let mut bad = request.clone();
        let mut field = &mut bad["roots"][0];
        for key in &path[..path.len() - 1] {
            field = &mut field[*key];
        }
        field[path[path.len() - 1]] = json!(1);
        no_report(&format!("nested-unknown-{n}"), &bad);
    }
    for (n, raw) in [
        b"{".to_vec(),
        [
            b"{\"schema_version\":1,".as_slice(),
            &serde_json::to_vec(&request).unwrap()[1..],
        ]
        .concat(),
        vec![b' '; 65537],
    ]
    .into_iter()
    .enumerate()
    {
        let (output, path) = raw_run(&format!("raw-{n}"), &raw, &[]);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    let raw = serde_json::to_vec(&request).unwrap();
    for (n, flag) in [
        "--execution-admission",
        "--cooperative-preparation",
        "--selected-source",
        "--comparison-bundle",
    ]
    .into_iter()
    .enumerate()
    {
        let (output, path) = raw_run(&format!("conflict-{n}"), &raw, &[flag, "unopened.json"]);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({
        "cases":cases.get(),"complete_expected_graph":true,"definitions":4,"operand_visits":6,
        "dependencies":6,"instructions":14,"default_advances":6,"one_canonical_cycle_back_edge":true,
        "shared_and_repeated_edges_preserved":true,"partial_report_never_published":true,
        "source_and_metadata_unchanged":true,"original_launched":false,"retail_parity_accepted":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "read-only installed source selection and built CLI; never launches retail"]
fn installed_route_admission_helper() {
    use fallout_data::plugin;
    use sha2::{Digest, Sha256};
    use std::{path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let install = PathBuf::from(std::env::var_os("RF_SCRIPT_ADMISSION_INSTALL").expect("install"));
    let output = PathBuf::from(std::env::var_os("RF_SCRIPT_ROUTE_EVIDENCE").expect("evidence"));
    fs::create_dir(&output).unwrap();
    let names = vec!["FalloutNV.esm".to_owned()];
    fs::create_dir(output.join("private-index-cache")).unwrap();
    let mut store = RecordStore::open_nv_headers_cached(
        &install.join("Data"),
        &names,
        Default::default(),
        &output.join("private-index-cache"),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let candidates: Vec<_> = store
        .winning_definitions()
        .filter(|(_, location)| store.definition(*location).header.kind == *b"SCPT")
        .map(|(key, location)| (key.clone(), location))
        .collect();
    assert!(candidates.len() <= 16_384);
    let mut roots = Vec::new();
    let mut receipts = Vec::new();
    for (key, location) in candidates {
        let record = store.read(location).unwrap();
        let mut editor_id = None;
        plugin::visit_subrecords(&record, store.source_name(location), |field| {
            if field.kind == *b"EDID" {
                assert!(editor_id.is_none());
                editor_id = Some(field.data.to_vec());
            }
            Ok(())
        })
        .unwrap();
        let Some(editor_id) = editor_id else { continue };
        if !editor_id
            .windows(b"mitchell".len())
            .any(|window| window.eq_ignore_ascii_case(b"mitchell"))
        {
            continue;
        }
        for script in catalogue.record_scripts(&key) {
            assert!(roots.len() < 32);
            roots.push(script.handle().clone());
            receipts.push(serde_json::json!({"editor_id_bytes":editor_id,"handle":script.handle(),"version":script.version(),
                "record_file_offset":record.header.offset,"decoded_record_sha256":format!("{:x}",Sha256::digest(&record.payload)),
                "compiled_sha256":script.compiled().map(|bytes|format!("{:x}",Sha256::digest(bytes)))}));
        }
    }
    assert!(
        !roots.is_empty(),
        "No winning Mitchell-named standalone scripts found"
    );
    let cohort = fallout_runtime::snapshot::cohort(&catalogue).unwrap();
    let request =
        serde_json::json!({"schema_version":1,"source_cohort_sha256":cohort,"roots":roots});
    let request_path = output.join("request.json");
    fs::write(&request_path, serde_json::to_vec_pretty(&request).unwrap()).unwrap();
    fs::write(output.join("selection-receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version":1,"selection":"Winning standalone SCPT editor IDs containing Mitchell, inspected as declared source; activation is not inferred",
        "source_cohort_sha256":cohort,"explicit_load_order":names,"source_receipts":catalogue.sources,
        "selected":receipts,"gameplay_accepted":false,"retail_execution_performed":false
    })).unwrap()).unwrap();
    let order = output.join("order.json");
    fs::write(&order, serde_json::to_vec(&names).unwrap()).unwrap();
    let report_path = output.join("report.json");
    let result = Command::new(cli)
        .args(["source-plans", "--install"])
        .arg(&install)
        .arg("--load-order")
        .arg(order)
        .arg("--index-cache")
        .arg(output.join("private-index-cache"))
        .arg("--execution-admission")
        .arg(request_path)
        .arg("--output")
        .arg(&report_path)
        .output()
        .unwrap();
    fs::write(output.join("stdout.txt"), result.stdout).unwrap();
    fs::write(output.join("stderr.txt"), result.stderr).unwrap();
    assert!(!result.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(value["execution_admission"]["roots"], request["roots"]);
    assert_eq!(
        value["execution_admission"]["source_cohort_sha256"],
        request["source_cohort_sha256"]
    );
    assert_eq!(
        value["execution_admission"]["faithful_execution_admitted"],
        false
    );
    assert!(value["execution_admission"]["first_unsupported"]["source_scda_offset"].is_number());
    assert_eq!(value["retail_parity_accepted"], false);
}
