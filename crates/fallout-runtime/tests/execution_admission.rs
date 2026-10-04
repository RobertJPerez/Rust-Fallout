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
