use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Handle, ScriptKey},
    obscript::{
        argument_census::Signatures,
        definition_plan,
        expression::{Operator, Operators},
        expression_plan,
    },
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}
fn header(masters: &[&str]) -> Vec<u8> {
    let mut payload = field(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        payload.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        payload.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &payload)
}
fn instruction(bytes: &mut Vec<u8>, opcode: u16, operands: &[u8]) {
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&(operands.len() as u16).to_le_bytes());
    bytes.extend_from_slice(operands);
}
fn unit(compiled: Option<&[u8]>, variables: &[u32]) -> Vec<u8> {
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&(compiled.map_or(0, <[u8]>::len) as u32).to_le_bytes());
    let mut payload = field(b"SCHR", &schr);
    if let Some(compiled) = compiled {
        payload.extend(field(b"SCDA", compiled));
    }
    for index in variables {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        payload.extend(field(b"SLSD", &declaration));
        payload.extend(field(b"SCVR", b"counter\0"));
    }
    // Source text is deliberately unrelated. Preparation uses SCDA exclusively.
    payload.extend(field(b"SCTX", b"This is not the compiled script."));
    payload
}
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
fn load(path: &Path, names: &[&str]) -> Catalogue {
    let mut store = RecordStore::open_nv_headers(
        path,
        &names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap();
    Catalogue::load(
        &mut store,
        fallout_data::loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap()
}
fn key(id: u32) -> ScriptKey {
    ScriptKey {
        record: FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "base.esm".into(),
            local_id: id,
        },
        header_decoded_offset: 0,
    }
}
fn fixture(path: &Path, compiled: Option<&[u8]>, variables: &[u32]) -> Catalogue {
    fs::write(
        path.join("Base.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x100, 0, &unit(compiled, variables)),
        ]
        .concat(),
    )
    .unwrap();
    load(path, &["Base.esm"])
}
fn body() -> Vec<u8> {
    let mut bytes = Vec::new();
    instruction(&mut bytes, 0x1d, &[]);
    instruction(&mut bytes, 0x10, &[3, 0, 31, 0, 0, 0]);
    instruction(&mut bytes, 0x16, &[1, 0, 1, 0, b'1']);
    instruction(
        &mut bytes,
        0x15,
        &[b's', 7, 0, 5, 0, b'1', b' ', b'2', b' ', b'+'],
    );
    instruction(&mut bytes, 0x19, &[]);
    instruction(&mut bytes, 0x11, &[]);
    bytes
}

#[test]
fn plans_bind_exact_winning_handles_bytes_tables_and_statement_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let bytes = body();
    let catalogue = fixture(directory.path(), Some(&bytes), &[7]);
    let handle = catalogue.get(&key(0x100)).unwrap().handle();
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let plan = definition_plan::prepare(
        &catalogue,
        handle,
        &model,
        &Signatures::new(),
        definition_plan::Limits::default(),
    )
    .unwrap();
    assert_eq!(plan.handle(), handle);
    assert_eq!(
        plan.source_cohort_sha256(),
        catalogue.winning_content_sha256()
    );
    assert_eq!(plan.control().bytes(), bytes);
    assert_eq!(plan.source().compiled(), Some(bytes.as_slice()));
    assert_eq!(plan.statements().len(), 2);
    assert_eq!(plan.tokens(), 4);
    assert_eq!(plan.nodes(), 4);
    assert_eq!(plan.statement(3).unwrap().plan().source_bytes(), b"1 2 +");
    assert_eq!(plan.statement(3).unwrap().expression_scda_offset(), 32);
    assert!(plan.statement(0).is_none());
    assert!(plan.statement(99).is_none());
    assert_eq!(plan.bindings().counts.uses, 1);
    assert_eq!(plan.bindings().uses[0].target_value, Some(7));
    assert_eq!(plan.event_at_scda_offset(4).unwrap().event_id, 3);
    for offset in [0, 5, 14, bytes.len(), usize::MAX] {
        assert!(plan.event_at_scda_offset(offset).is_none());
    }
}

#[test]
fn missing_changed_deleted_and_forged_handles_never_produce_plans() {
    let directory = tempfile::tempdir().unwrap();
    let catalogue = fixture(directory.path(), Some(&body()), &[7]);
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let original = catalogue.get(&key(0x100)).unwrap().handle().clone();
    for handle in [
        Handle {
            version_sha256: "00".repeat(32),
            ..original.clone()
        },
        Handle {
            key: key(0x101),
            ..original.clone()
        },
    ] {
        assert!(matches!(
            definition_plan::prepare(
                &catalogue,
                &handle,
                &model,
                &Signatures::new(),
                definition_plan::Limits::default()
            ),
            Err(definition_plan::Error::DefinitionChanged)
        ));
    }
    fs::write(
        directory.path().join("Patch.esp"),
        [
            header(&["Base.esm"]),
            record(b"SCPT", 0x100, 0, &unit(Some(&[0x1d, 0, 0, 0]), &[])),
        ]
        .concat(),
    )
    .unwrap();
    let patched = load(directory.path(), &["Base.esm", "Patch.esp"]);
    assert!(matches!(
        definition_plan::prepare(
            &patched,
            &original,
            &model,
            &Signatures::new(),
            definition_plan::Limits::default()
        ),
        Err(definition_plan::Error::DefinitionChanged)
    ));
    fs::write(
        directory.path().join("Patch.esp"),
        [header(&["Base.esm"]), record(b"SCPT", 0x100, 0x20, &[])].concat(),
    )
    .unwrap();
    let deleted = load(directory.path(), &["Base.esm", "Patch.esp"]);
    assert!(matches!(
        definition_plan::prepare(
            &deleted,
            &original,
            &model,
            &Signatures::new(),
            definition_plan::Limits::default()
        ),
        Err(definition_plan::Error::DefinitionChanged)
    ));
}

#[test]
fn missing_bodies_and_source_metadata_findings_are_explicit_failures() {
    let directory = tempfile::tempdir().unwrap();
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let catalogue = fixture(directory.path(), None, &[]);
    let handle = catalogue.get(&key(0x100)).unwrap().handle();
    assert!(matches!(
        definition_plan::prepare(
            &catalogue,
            handle,
            &model,
            &Signatures::new(),
            definition_plan::Limits::default()
        ),
        Err(definition_plan::Error::MissingBody)
    ));
    let mut payload = unit(Some(&body()), &[7]);
    payload[14..18].copy_from_slice(&999u32.to_le_bytes());
    fs::write(
        directory.path().join("Base.esm"),
        [header(&[]), record(b"SCPT", 0x100, 0, &payload)].concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["Base.esm"]);
    let handle = catalogue.get(&key(0x100)).unwrap().handle();
    assert!(matches!(
        definition_plan::prepare(
            &catalogue,
            handle,
            &model,
            &Signatures::new(),
            definition_plan::Limits::default()
        ),
        Err(definition_plan::Error::SourceMetadata(_))
    ));
}

#[test]
fn unresolved_expression_and_control_shapes_remain_errors() {
    let directory = tempfile::tempdir().unwrap();
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let mut bytes = Vec::new();
    instruction(&mut bytes, 0x16, &[0, 0, 3, 0, b'1', b' ', b'2']);
    instruction(&mut bytes, 0x19, &[]);
    let catalogue = fixture(directory.path(), Some(&bytes), &[]);
    let handle = catalogue.get(&key(0x100)).unwrap().handle();
    assert!(matches!(
        definition_plan::prepare(
            &catalogue,
            handle,
            &model,
            &Signatures::new(),
            definition_plan::Limits::default()
        ),
        Err(definition_plan::Error::ExpressionPlan { .. })
    ));
    let catalogue = fixture(directory.path(), Some(&[0x19, 0, 0, 0]), &[]);
    let handle = catalogue.get(&key(0x100)).unwrap().handle();
    assert!(matches!(
        definition_plan::prepare(
            &catalogue,
            handle,
            &model,
            &Signatures::new(),
            definition_plan::Limits::default()
        ),
        Err(definition_plan::Error::Control(_))
    ));
}

#[test]
fn missing_local_or_command_signature_cannot_be_prepared_as_success() {
    let directory = tempfile::tempdir().unwrap();
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    for (compiled, variables) in [(body(), Vec::new()), (vec![0, 0x10, 0, 0], vec![7])] {
        let catalogue = fixture(directory.path(), Some(&compiled), &variables);
        let handle = catalogue.get(&key(0x100)).unwrap().handle();
        assert!(matches!(
            definition_plan::prepare(
                &catalogue,
                handle,
                &model,
                &Signatures::new(),
                definition_plan::Limits::default()
            ),
            Err(definition_plan::Error::OperandFindings)
        ));
    }
}

#[test]
fn aggregate_plan_budgets_fail_without_returning_partial_definitions() {
    let directory = tempfile::tempdir().unwrap();
    let catalogue = fixture(directory.path(), Some(&body()), &[7]);
    let handle = catalogue.get(&key(0x100)).unwrap().handle();
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let limits = definition_plan::Limits {
        maximum_expressions: 2,
        maximum_tokens: 4,
        maximum_nodes: 4,
        maximum_operand_uses: 1,
        ..definition_plan::Limits::default()
    };
    assert!(
        definition_plan::prepare(&catalogue, handle, &model, &Signatures::new(), limits).is_ok()
    );
    for limits in [
        definition_plan::Limits {
            maximum_expressions: 1,
            ..limits
        },
        definition_plan::Limits {
            maximum_tokens: 3,
            ..limits
        },
        definition_plan::Limits {
            maximum_nodes: 3,
            ..limits
        },
        definition_plan::Limits {
            maximum_operand_uses: 0,
            ..limits
        },
    ] {
        assert!(
            definition_plan::prepare(&catalogue, handle, &model, &Signatures::new(), limits)
                .is_err()
        );
    }
}

#[test]
fn unrelated_plugin_reordering_keeps_handle_and_plan_source_stable() {
    let directory = tempfile::tempdir().unwrap();
    let _ = fixture(directory.path(), Some(&body()), &[7]);
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let a = load(directory.path(), &["Other.esm", "Base.esm"]);
    let b = load(directory.path(), &["Base.esm", "Other.esm"]);
    let handle = a.get(&key(0x100)).unwrap().handle();
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let pa = definition_plan::prepare(
        &a,
        handle,
        &model,
        &Signatures::new(),
        definition_plan::Limits::default(),
    )
    .unwrap();
    let pb = definition_plan::prepare(
        &b,
        handle,
        &model,
        &Signatures::new(),
        definition_plan::Limits::default(),
    )
    .unwrap();
    assert_eq!(pa.handle(), pb.handle());
    assert_eq!(pa.control().bytes(), pb.control().bytes());
    assert_eq!(
        pa.statement(3).unwrap().plan().shape_sha256(),
        pb.statement(3).unwrap().plan().shape_sha256()
    );
}

#[test]
fn identical_compiled_bodies_keep_each_embedded_units_own_reference_table() {
    let directory = tempfile::tempdir().unwrap();
    let mut compiled = Vec::new();
    instruction(&mut compiled, 0x16, &[0, 0, 3, 0, b'G', 1, 0]);
    instruction(&mut compiled, 0x19, &[]);
    let mut payload = Vec::new();
    for target in [0x200u32, 0x201] {
        let mut authored = unit(Some(&compiled), &[]);
        authored[10..14].copy_from_slice(&1u32.to_le_bytes());
        authored.extend(field(b"SCRO", &target.to_le_bytes()));
        payload.extend(authored);
    }
    fs::write(
        directory.path().join("Base.esm"),
        [
            header(&[]),
            record(b"INFO", 0x100, 0, &payload),
            record(b"GLOB", 0x200, 0, &[]),
            record(b"GLOB", 0x201, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = load(directory.path(), &["Base.esm"]);
    let handles = catalogue
        .iter()
        .map(|(_, s)| s.handle().clone())
        .collect::<Vec<_>>();
    assert_eq!(handles.len(), 2);
    let operators = operators();
    let model = expression_plan::Model::vanilla(&operators).unwrap();
    let a = definition_plan::prepare(
        &catalogue,
        &handles[0],
        &model,
        &Signatures::new(),
        definition_plan::Limits::default(),
    )
    .unwrap();
    let b = definition_plan::prepare(
        &catalogue,
        &handles[1],
        &model,
        &Signatures::new(),
        definition_plan::Limits::default(),
    )
    .unwrap();
    assert_eq!(a.control().bytes(), b.control().bytes());
    assert_ne!(
        a.handle().key.header_decoded_offset,
        b.handle().key.header_decoded_offset
    );
    assert_eq!(a.bindings().uses[0].target_value, Some(0x200));
    assert_eq!(b.bindings().uses[0].target_value, Some(0x201));
    assert_ne!(
        fallout_data::obscript::operand_binding::digest(&a.bindings().uses),
        fallout_data::obscript::operand_binding::digest(&b.bindings().uses)
    );
}
