mod common;
use common::*;
use fallout_data::{
    identity::FormKey,
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    plugin,
    quest_scripts::{Attachments, Status},
    store::RecordStore,
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::{
        attachment_boot::{self, Error, Limits, Request},
        copy_probe::Initializer,
    },
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::{Local, ScriptInstance, Snapshot},
};
use std::{fs, path::Path, sync::Arc};

fn quest(id: u32) -> FormKey {
    FormKey {
        origin_plugin: "quest.esp".into(),
        ..form(id)
    }
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
struct Fixture {
    directory: tempfile::TempDir,
    catalogue: Arc<Catalogue>,
    attachments: Attachments,
    content: Content,
}
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let valid = unit(&[(2, 1), (42, 0), (90, 0), (99, 7)], &[(b"SCRV", 90)]);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &valid),
            record(b"SCPT", 0x301, 0, &[valid.clone(), valid.clone()].concat()),
            record(b"SCPT", 0x302, plugin::DELETED, &[]),
            record(b"ACTI", 0x400, 0, &[]),
            record(b"SCPT", 0x304, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let mut records = header(&["FalloutNV.esm"]);
    for (id, target) in [
        (0x100, 0x300_u32),
        (0x102, 0),
        (0x105, 0x999),
        (0x106, 0x302),
        (0x107, 0x400),
        (0x108, 0x301),
        (0x109, 0x304),
    ] {
        records.extend(record(
            b"QUST",
            0x01000000 | id,
            0,
            &field(b"SCRI", &target.to_le_bytes()),
        ));
    }
    records.extend(record(b"QUST", 0x01000101, 0, &[]));
    records.extend(record(
        b"QUST",
        0x01000103,
        0,
        &[
            field(b"SCRI", &0x300_u32.to_le_bytes()),
            field(b"SCRI", &0x300_u32.to_le_bytes()),
        ]
        .concat(),
    ));
    records.extend(record(b"QUST", 0x01000104, plugin::DELETED, &[]));
    records.extend(record(
        b"QUST",
        0x01000110,
        0,
        &[
            field(b"SCRI", &0x300_u32.to_le_bytes()),
            field(b"DATA", &[5, 0]),
        ]
        .concat(),
    ));
    records.extend(record(b"ACTI", 0x01000200, 0, &[]));
    fs::write(directory.path().join("Quest.esp"), records).unwrap();
    fs::write(directory.path().join("Other.esm"), header(&[])).unwrap();
    let (catalogue, attachments, content) =
        load_all(directory.path(), &["FalloutNV.esm", "Quest.esp"]);
    Fixture {
        directory,
        catalogue,
        attachments,
        content,
    }
}
fn load_all(directory: &Path, names: &[&str]) -> (Arc<Catalogue>, Attachments, Content) {
    let mut store = RecordStore::open_nv_headers(
        directory,
        &names.iter().map(|name| (*name).into()).collect::<Vec<_>>(),
        Default::default(),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    let attachments = Attachments::load(&mut store, &catalogue, 100, |_, _| Ok(())).unwrap();
    (Arc::new(catalogue), attachments, content)
}
fn request(campaign: CampaignId) -> Request {
    Request {
        campaign,
        context: Context {
            target: Some(ReferenceValue::Live {
                id: 1
                    .try_into()
                    .map(fallout_runtime::identity::ReferenceId)
                    .unwrap(),
            }),
            arguments: vec![ReferenceValue::Content { key: form(0x400) }],
            ..Default::default()
        },
        initializers: vec![
            Initializer {
                index: 2,
                value: Value::Number {
                    bits: 0x8000000000000000,
                },
            },
            Initializer {
                index: 42,
                value: Value::Number {
                    bits: 0x7ff8123456789abc,
                },
            },
            Initializer {
                index: 90,
                value: Value::Reference {
                    value: ReferenceValue::Content { key: form(0x400) },
                },
            },
        ],
    }
}
fn base(catalogue: Arc<Catalogue>) -> World<'static> {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x79; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(None).unwrap();
    world.initialize_inventory(reference).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x400));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc12345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"BLOB",
            bytes: vec![0, 128, 255],
        });
    world
        .add_item(reference, facts, 19.try_into().unwrap())
        .unwrap();
    let definition = catalogue
        .record_scripts(&form(0x300))
        .next()
        .unwrap()
        .handle();
    let handle = world
        .create_instance(
            definition,
            Owner::Fragment {
                activation: 7.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 123 })])
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
    world
}
fn sources<'a>(catalogue: &'a Catalogue, model: &Model<'_>) -> PreparedSources<'a> {
    PreparedSources::load_selected(
        catalogue,
        model,
        &Signatures::new(),
        &[catalogue
            .record_scripts(&form(0x300))
            .next()
            .unwrap()
            .handle()
            .clone()],
        Default::default(),
    )
    .unwrap()
}
fn expected(
    before: &Snapshot,
    request: &Request,
    definition: &fallout_data::loaded_scripts::Handle,
) -> Snapshot {
    let mut after = before.clone();
    let instance = fallout_runtime::identity::InstanceId(after.next_instance.try_into().unwrap());
    after.next_instance += 1;
    after.state_revision += 1 + u64::from(!request.initializers.is_empty());
    after.instances.push(ScriptInstance {
        id: instance,
        definition: definition.clone(),
        owner: Owner::Quest { key: quest(0x100) },
        context: request.context.clone(),
        locals: [2, 42, 90, 99]
            .map(|index| Local {
                index,
                value: request
                    .initializers
                    .iter()
                    .find(|entry| entry.index == index)
                    .map_or(Value::Uninitialized, |entry| entry.value.clone()),
            })
            .into(),
    });
    after
}

#[test]
fn explicit_attachment_boot_preserves_every_other_snapshot_field_and_cold_owner_identity() {
    let f = fixture();
    let world = base(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let request = request(world.campaign());
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    let plan = attachment_boot::prepare(
        &sources,
        &f.attachments,
        &f.content,
        &quest(0x100),
        &request,
        Default::default(),
    )
    .unwrap();
    assert_eq!(plan.attachment().source.plugin, "Quest.esp");
    assert_eq!(plan.script_version().source_plugin, "FalloutNV.esm");
    assert_eq!(plan.attachment().fields[0].key.as_ref(), Some(&form(0x300)));
    let result = plan.apply(before.clone(), Default::default()).unwrap();
    assert_eq!(
        result.snapshot,
        expected(&before, &request, plan.definition())
    );
    assert_eq!(world.snapshot(), before);
    let bytes = result.snapshot.encode(1024 * 1024).unwrap();
    let cold = World::restore(
        Arc::clone(&f.catalogue),
        Snapshot::decode(&bytes, Default::default()).unwrap(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(cold.snapshot(), result.snapshot);
    assert_eq!(
        cold.owner_instance(&Owner::Quest { key: quest(0x100) }),
        Some(result.instance)
    );
    assert_eq!(
        cold.instance(cold.handle(result.instance).unwrap())
            .unwrap()
            .definition(),
        plan.definition()
    );
    assert!(
        matches!(plan.apply(result.snapshot,Default::default()),Err(Error::State(fallout_runtime::Error::Invalid(reason))) if reason.contains("owner already"))
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn absent_deleted_wrong_kind_and_ambiguous_attachments_never_produce_a_result() {
    let f = fixture();
    let world = base(Arc::clone(&f.catalogue));
    let request = request(world.campaign());
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    for (id, status) in [
        (0x101, Status::NoScriptField),
        (0x102, Status::NullScript),
        (0x103, Status::MultipleScriptFields),
        (0x105, Status::MissingScript),
        (0x106, Status::DeletedScript),
        (0x107, Status::WrongScriptKind),
        (0x108, Status::MultipleStandaloneUnits),
        (0x109, Status::MissingLoadedDefinition),
    ] {
        assert!(
            matches!(attachment_boot::prepare(&sources,&f.attachments,&f.content,&quest(id),&request,Default::default()),Err(Error::Attachment(actual)) if actual==status),
            "{id:X}"
        );
    }
    for id in [0x104, 0x200, 0x777, 0x110] {
        assert!(
            attachment_boot::prepare(
                &sources,
                &f.attachments,
                &f.content,
                &quest(id),
                &request,
                Default::default()
            )
            .is_err(),
            "{id:X}"
        );
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
fn initializer_context_campaign_and_private_world_failure_drop_the_whole_result() {
    let f = fixture();
    let world = base(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    for (index, value) in [
        (999, Value::Number { bits: 1 }),
        (99, Value::Number { bits: 1 }),
        (90, Value::Number { bits: 1 }),
        (
            2,
            Value::Reference {
                value: ReferenceValue::Null,
            },
        ),
        (
            90,
            Value::Reference {
                value: ReferenceValue::Live {
                    id: 999
                        .try_into()
                        .map(fallout_runtime::identity::ReferenceId)
                        .unwrap(),
                },
            },
        ),
    ] {
        let mut request = request(world.campaign());
        request.initializers.retain(|entry| entry.index != index);
        request.initializers.push(Initializer { index, value });
        let plan = attachment_boot::prepare(
            &sources,
            &f.attachments,
            &f.content,
            &quest(0x100),
            &request,
            Default::default(),
        )
        .unwrap();
        assert!(plan.apply(before.clone(), Default::default()).is_err());
        assert_eq!(world.snapshot(), before);
    }
    let mut duplicate = request(world.campaign());
    duplicate.initializers.push(Initializer {
        index: 2,
        value: Value::Number { bits: 3 },
    });
    assert!(matches!(
        attachment_boot::prepare(
            &sources,
            &f.attachments,
            &f.content,
            &quest(0x100),
            &duplicate,
            Default::default()
        ),
        Err(Error::Input("duplicate local initializer"))
    ));
    let mut invalid_context = request(world.campaign());
    invalid_context.context.calling_reference = Some(fallout_runtime::identity::ReferenceId(
        999.try_into().unwrap(),
    ));
    let plan = attachment_boot::prepare(
        &sources,
        &f.attachments,
        &f.content,
        &quest(0x100),
        &invalid_context,
        Default::default(),
    )
    .unwrap();
    assert!(plan.apply(before.clone(), Default::default()).is_err());
    let mut wrong_campaign = request(world.campaign());
    wrong_campaign.campaign = CampaignId::from_bytes([0x78; 16]).unwrap();
    let plan = attachment_boot::prepare(
        &sources,
        &f.attachments,
        &f.content,
        &quest(0x100),
        &wrong_campaign,
        Default::default(),
    )
    .unwrap();
    assert!(matches!(
        plan.apply(before.clone(), Default::default()),
        Err(Error::Input(_))
    ));
    let empty = Request {
        initializers: vec![],
        context: Context::default(),
        ..request(world.campaign())
    };
    let plan = attachment_boot::prepare(
        &sources,
        &f.attachments,
        &f.content,
        &quest(0x100),
        &empty,
        Default::default(),
    )
    .unwrap();
    assert_eq!(
        plan.apply(before.clone(), Default::default())
            .unwrap()
            .snapshot,
        expected(&before, &empty, plan.definition())
    );
    assert!(
        plan.apply(
            before.clone(),
            fallout_runtime::Limits {
                max_instances: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn exact_limits_and_full_attachment_cohort_binding_precede_private_creation() {
    let f = fixture();
    let world = base(Arc::clone(&f.catalogue));
    let request = request(world.campaign());
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    let counts = attachment_boot::prepare(
        &sources,
        &f.attachments,
        &f.content,
        &quest(0x100),
        &request,
        Default::default(),
    )
    .unwrap()
    .counts();
    assert_eq!(counts.initializers, 3);
    assert_eq!(counts.context_arguments, 1);
    assert_eq!(
        counts.variable_bytes,
        "quest.esp".len() + 2 * "falloutnv.esm".len()
    );
    assert_eq!(counts.declarations, 4);
    let oversized_quest = FormKey {
        origin_plugin: "/".repeat(64 * 1024 + 1),
        ..quest(0x100)
    };
    assert!(matches!(
        attachment_boot::prepare(
            &sources,
            &f.attachments,
            &f.content,
            &oversized_quest,
            &request,
            Default::default()
        ),
        Err(Error::Capacity("variable bytes"))
    ));
    let exact = Limits {
        maximum_initializers: counts.initializers,
        maximum_context_arguments: counts.context_arguments,
        maximum_variable_bytes: counts.variable_bytes,
        maximum_source_receipt_bytes: counts.source_receipt_bytes,
        maximum_declarations: counts.declarations,
    };
    assert_eq!(
        attachment_boot::prepare(
            &sources,
            &f.attachments,
            &f.content,
            &quest(0x100),
            &request,
            exact
        )
        .unwrap()
        .counts(),
        counts
    );
    for limits in [
        Limits {
            maximum_initializers: 2,
            ..exact
        },
        Limits {
            maximum_context_arguments: 0,
            ..exact
        },
        Limits {
            maximum_variable_bytes: counts.variable_bytes - 1,
            ..exact
        },
        Limits {
            maximum_source_receipt_bytes: counts.source_receipt_bytes - 1,
            ..exact
        },
        Limits {
            maximum_declarations: 3,
            ..exact
        },
    ] {
        assert!(matches!(
            attachment_boot::prepare(
                &sources,
                &f.attachments,
                &f.content,
                &quest(0x100),
                &request,
                limits
            ),
            Err(Error::Capacity(_))
        ));
    }
    let (foreign, attachments, content) = load_all(
        f.directory.path(),
        &["FalloutNV.esm", "Quest.esp", "Other.esm"],
    );
    let exact_handle = sources
        .catalogue()
        .record_scripts(&form(0x300))
        .next()
        .unwrap()
        .handle();
    assert!(foreign.get_handle(exact_handle).is_some());
    assert!(matches!(
        attachment_boot::prepare(
            &sources,
            &attachments,
            &f.content,
            &quest(0x100),
            &request,
            Default::default()
        ),
        Err(Error::Input(_))
    ));
    assert!(matches!(
        attachment_boot::prepare(
            &sources,
            &f.attachments,
            &content,
            &quest(0x100),
            &request,
            Default::default()
        ),
        Err(Error::Content(_))
    ));
    let unselected = PreparedSources::load_selected(
        &f.catalogue,
        &model,
        &Signatures::new(),
        &[],
        Default::default(),
    )
    .unwrap();
    assert!(matches!(
        attachment_boot::prepare(
            &unselected,
            &f.attachments,
            &f.content,
            &quest(0x100),
            &request,
            Default::default()
        ),
        Err(Error::Source(_))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "built CLI and authored executable metadata; explicit engineering quest owner only, no original launch"]
fn cli_attachment_boot_helper() {
    use serde_json::{Value as Json, json};
    use std::{cell::Cell, path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_QUEST_BOOT_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let f = fixture();
    let world = base(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let initialization = request(world.campaign());
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    let plan = attachment_boot::prepare(
        &sources,
        &f.attachments,
        &f.content,
        &quest(0x100),
        &initialization,
        Default::default(),
    )
    .unwrap();
    let expected_snapshot = expected(&before, &initialization, plan.definition());
    let request = json!({"schema_version":1,"quest":quest(0x100),"initialization":initialization});
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    for name in ["FalloutNV.esm", "Quest.esp"] {
        fs::copy(
            f.directory.path().join(name),
            install.join("Data").join(name),
        )
        .unwrap();
    }
    fs::copy(
        metadata.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\",\"Quest.esp\"]").unwrap();
    let calls = Cell::new(0);
    let run_on = |name: &str,
                  install: &Path,
                  snapshot: &Snapshot,
                  request: &Json,
                  result_override: Option<&Path>,
                  extra: &[&str]| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let input = directory.join("input.snapshot.json");
        let original = snapshot
            .encode(fallout_runtime::Limits::default().max_snapshot_bytes)
            .unwrap();
        fs::write(&input, &original).unwrap();
        let request_path = directory.join("request.json");
        fs::write(&request_path, serde_json::to_vec(request).unwrap()).unwrap();
        let report = directory.join("report.json");
        let result = result_override
            .map_or_else(|| directory.join("result.snapshot.json"), Path::to_path_buf);
        calls.set(calls.get() + 1);
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(install)
            .arg("--load-order")
            .arg(&order)
            .arg("--quest-boot-request")
            .arg(request_path)
            .arg("--snapshot-input")
            .arg(&input)
            .arg("--quest-boot-output")
            .arg(&result)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(directory.join("result.json"),serde_json::to_vec_pretty(&json!({"success":output.status.success(),"exit_code":output.status.code(),"snapshot_artifact_exists":result.exists(),"report_exists":report.exists()})).unwrap()).unwrap();
        assert_eq!(
            fs::read(input).unwrap(),
            original,
            "{name}: input snapshot changed"
        );
        (output, report, result)
    };
    let run = |name: &str, snapshot: &Snapshot, request: &Json| {
        run_on(name, &install, snapshot, request, None, &[])
    };
    let (output, report, result) = run("valid", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    let actual = Snapshot::decode(&fs::read(&result).unwrap(), Default::default()).unwrap();
    assert_eq!(actual, expected_snapshot);
    assert_eq!(
        report["quest_attachment"],
        serde_json::to_value(plan.attachment()).unwrap()
    );
    assert_eq!(
        report["script_source"],
        serde_json::to_value(plan.script_version()).unwrap()
    );
    assert_eq!(
        report["source_receipts"],
        serde_json::to_value(f.attachments.source_receipts()).unwrap()
    );
    assert_eq!(
        report["initialization_counts"],
        serde_json::to_value(plan.counts()).unwrap()
    );
    assert_eq!(report["canonical_restore_verified"], true);
    assert_eq!(report["quest_activation_verified"], false);
    assert_eq!(report["event_enqueued"], false);
    let cold =
        World::restore(Arc::clone(&f.catalogue), actual.clone(), Default::default()).unwrap();
    assert_eq!(cold.snapshot(), actual);
    let mut empty = request.clone();
    empty["initialization"]["initializers"] = json!([]);
    empty["initialization"]["context"] = json!(Context::default());
    let (output, _, result) = run("empty-initializers", &before, &empty);
    assert!(output.status.success());
    let empty_request = Request {
        campaign: initialization.campaign,
        context: Context::default(),
        initializers: vec![],
    };
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        expected(&before, &empty_request, plan.definition())
    );
    for id in [
        0x101, 0x102, 0x103, 0x104, 0x105, 0x106, 0x107, 0x108, 0x109, 0x110, 0x200, 0x777,
    ] {
        let mut invalid = request.clone();
        invalid["quest"] = json!(quest(id));
        let (output, report, result) = run(&format!("attachment-{id:x}"), &before, &invalid);
        assert!(!output.status.success(), "{id:X}");
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, index, value) in [
        ("missing-local", 999, json!({"kind":"number","bits":1})),
        ("unsupported-local", 99, json!({"kind":"number","bits":1})),
        ("incompatible-local", 90, json!({"kind":"number","bits":1})),
        (
            "missing-live-reference",
            90,
            json!({"kind":"reference","value":{"kind":"live","id":999}}),
        ),
    ] {
        let mut invalid = request.clone();
        let entries = invalid["initialization"]["initializers"]
            .as_array_mut()
            .unwrap();
        entries.retain(|entry| entry["index"] != index);
        entries.push(json!({"index":index,"value":value}));
        let (output, report, result) = run(name, &before, &invalid);
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, pointer, value) in [
        (
            "duplicate-initializer",
            "/initialization/initializers",
            json!([{"index":2,"value":{"kind":"number","bits":1}},{"index":2,"value":{"kind":"number","bits":2}}]),
        ),
        (
            "initializer-cap",
            "/initialization/initializers",
            json!(
                (0..129)
                    .map(|index| json!({"index":index,"value":{"kind":"uninitialized"}}))
                    .collect::<Vec<_>>()
            ),
        ),
        (
            "context-cap",
            "/initialization/context/arguments",
            json!(vec![ReferenceValue::Null; 65]),
        ),
        (
            "invalid-context",
            "/initialization/context/calling_reference",
            json!(999),
        ),
        (
            "wrong-campaign",
            "/initialization/campaign",
            json!(vec![0x78; 16]),
        ),
        (
            "zero-campaign",
            "/initialization/campaign",
            json!(vec![0; 16]),
        ),
        ("request-schema", "/schema_version", json!(2)),
    ] {
        let mut invalid = request.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        let (output, report, result) = run(name, &before, &invalid);
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, pointer, field, value) in [
        (
            "unknown-initialization",
            "/initialization",
            "defaults",
            json!(true),
        ),
        ("unknown-request", "", "activate", json!(true)),
        (
            "request-byte-limit",
            "",
            "extra",
            json!("x".repeat(16 * 1024)),
        ),
    ] {
        let mut invalid = request.clone();
        invalid.pointer_mut(pointer).unwrap()[field] = value;
        let (output, report, result) = run(name, &before, &invalid);
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, mut invalid) in [
        ("duplicate-owner", actual),
        ("legacy-snapshot", before.clone()),
        ("stale-snapshot", before.clone()),
    ] {
        if name == "legacy-snapshot" {
            invalid.schema_version = 3;
        } else if name == "stale-snapshot" {
            invalid.catalogue_sha256 = "0".repeat(64);
        }
        let (output, report, result) = run(name, &invalid, &request);
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    let existing = evidence.join("existing.snapshot.json");
    fs::write(&existing, b"preserve-existing").unwrap();
    let (output, report, result) = run_on(
        "existing-output",
        &install,
        &before,
        &request,
        Some(&existing),
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert_eq!(fs::read(result).unwrap(), b"preserve-existing");
    let protected = install.join("Data/prohibited.snapshot.json");
    let (output, report, result) = run_on(
        "protected-output",
        &install,
        &before,
        &request,
        Some(&protected),
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!result.exists());
    let changed = evidence.join("changed-source-copy");
    fs::create_dir(&changed).unwrap();
    fs::create_dir(changed.join("Data")).unwrap();
    fs::copy(install.join("FalloutNV.exe"), changed.join("FalloutNV.exe")).unwrap();
    fs::copy(
        install.join("Data/Quest.esp"),
        changed.join("Data/Quest.esp"),
    )
    .unwrap();
    fs::write(
        changed.join("Data/FalloutNV.esm"),
        [
            fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
            record(b"GLOB", 0x501, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let (output, report, result) = run_on("changed-source", &changed, &before, &request, None, &[]);
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!result.exists());
    for (name, extra) in [
        ("copy-conflict", vec!["--snapshot-copy-request", "unused"]),
        (
            "native-conflict",
            vec!["--snapshot-native-request", "unused"],
        ),
        ("copy-output-conflict", vec!["--snapshot-output", "unused"]),
        ("seed-conflict", vec!["--engineering-local-copy", "unused"]),
        ("native-mode-conflict", vec!["--native-capabilities"]),
        ("player-conflict", vec!["--player-id", "1"]),
        ("cache-mode-conflict", vec!["--prepared-sources"]),
    ] {
        let (output, report, result) = run_on(name, &install, &before, &request, None, &extra);
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists());
        assert!(!result.exists());
    }
    assert_eq!(world.snapshot(), before);
    fs::write(evidence.join("scope.json"),serde_json::to_vec_pretty(&json!({"scope":"explicit source-attached engineering quest owner only","actual_cli_calls":calls.get(),"initialization_counts":plan.counts(),"original_executed":false,"quest_activation_verified":false,"retail_parity_accepted":false,"input_world_unchanged":true})).unwrap()).unwrap();
}

fn many_fixture() -> (Fixture, [u64; 3]) {
    let original = fixture();
    let script_path = original.directory.path().join("FalloutNV.esm");
    let mut scripts = fs::read(&script_path).unwrap();
    scripts.extend(record(
        b"SCPT",
        0x305,
        0,
        &unit(&[(2, 0), (42, 1), (90, 0)], &[(b"SCRV", 90)]),
    ));
    fs::write(script_path, scripts).unwrap();
    let quest_path = original.directory.path().join("Quest.esp");
    let mut quests = fs::read(&quest_path).unwrap();
    let mut offsets = [0; 3];
    for (index, (id, script)) in [(0x120, 0x300_u32), (0x121, 0x300), (0x122, 0x305)]
        .into_iter()
        .enumerate()
    {
        offsets[index] = quests.len() as u64;
        quests.extend(record(
            b"QUST",
            0x01000000 | id,
            0,
            &field(b"SCRI", &script.to_le_bytes()),
        ));
    }
    fs::write(quest_path, quests).unwrap();
    let (catalogue, attachments, content) =
        load_all(original.directory.path(), &["FalloutNV.esm", "Quest.esp"]);
    (
        Fixture {
            directory: original.directory,
            catalogue,
            attachments,
            content,
        },
        offsets,
    )
}
fn many_sources<'a>(catalogue: &'a Catalogue, model: &Model<'_>) -> PreparedSources<'a> {
    let handles = [0x300, 0x305].map(|id| {
        catalogue
            .record_scripts(&form(id))
            .next()
            .unwrap()
            .handle()
            .clone()
    });
    PreparedSources::load_selected(
        catalogue,
        model,
        &Signatures::new(),
        &handles,
        Default::default(),
    )
    .unwrap()
}
fn many_requests(campaign: CampaignId) -> [Request; 3] {
    let mut requests = [request(campaign), request(campaign), request(campaign)];
    requests[1].initializers[0].value = Value::Number {
        bits: 0x3ff0000000000000,
    };
    requests[1].initializers[1].value = Value::Number {
        bits: 0x4000000000000000,
    };
    requests[2].initializers[0].value = Value::Number {
        bits: 0x7ff8000000000001,
    };
    requests[2].initializers[1].value = Value::Number {
        bits: 0x8000000000000000,
    };
    requests
}
fn many_expected(
    before: &Snapshot,
    keys: &[FormKey],
    requests: &[Request],
    definitions: &[fallout_data::loaded_scripts::Handle],
) -> Snapshot {
    let mut after = before.clone();
    for ((key, request), definition) in keys.iter().zip(requests).zip(definitions) {
        let instance =
            fallout_runtime::identity::InstanceId(after.next_instance.try_into().unwrap());
        after.next_instance += 1;
        after.state_revision += 1 + u64::from(!request.initializers.is_empty());
        let indices: &[u32] = if definition.key.record.local_id == 0x300 {
            &[2, 42, 90, 99]
        } else {
            &[2, 42, 90]
        };
        after.instances.push(ScriptInstance {
            id: instance,
            definition: definition.clone(),
            owner: Owner::Quest { key: key.clone() },
            context: request.context.clone(),
            locals: indices
                .iter()
                .map(|&index| Local {
                    index,
                    value: request
                        .initializers
                        .iter()
                        .find(|e| e.index == index)
                        .map_or(Value::Uninitialized, |e| e.value.clone()),
                })
                .collect(),
        });
    }
    after
}
fn ready_many<'p, 's>(
    sources: &'p PreparedSources<'s>,
    f: &'p Fixture,
    selections: &[attachment_boot::Selection<'p>],
    limits: attachment_boot::ManyLimits,
) -> attachment_boot::ManyBootPlan<'p, 's> {
    match attachment_boot::prepare_many(
        sources,
        &f.attachments,
        &f.content,
        selections,
        fallout_runtime::execution::local_copy::Intent::Engineering,
        limits,
    )
    .unwrap()
    {
        attachment_boot::ManyPreparation::Ready(plan) => plan,
        _ => panic!("engineering plan expected"),
    }
}
#[test]
fn atomic_quest_set_preserves_shared_definition_owners_exact_receipts_and_complete_cold_state() {
    use sha2::{Digest, Sha256};
    let (f, offsets) = many_fixture();
    let mut world = base(Arc::clone(&f.catalogue));
    world
        .advance_clocks(fallout_runtime::events::Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        })
        .unwrap();
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = many_sources(&f.catalogue, &model);
    let keys = [quest(0x120), quest(0x121), quest(0x122)];
    let definitions = [form(0x300), form(0x300), form(0x305)].map(|key| {
        f.catalogue
            .record_scripts(&key)
            .next()
            .unwrap()
            .handle()
            .clone()
    });
    let requests = many_requests(world.campaign());
    let selections: Vec<_> = (0..3)
        .map(|i| attachment_boot::Selection {
            quest: &keys[i],
            expected_definition: &definitions[i],
            initialization: &requests[i],
        })
        .collect();
    let plan = ready_many(&sources, &f, &selections, Default::default());
    assert_eq!(
        plan.counts(),
        attachment_boot::ManyCounts {
            quests: 3,
            input: attachment_boot::Counts {
                initializers: 9,
                context_arguments: 3,
                variable_bytes: 336,
                source_receipt_bytes: 498,
                declarations: 11
            },
            retained_variable_bytes: 6936
        }
    );
    assert_eq!(sources.counts().preparation_attempts, 2);
    let quest_sha = format!(
        "{:x}",
        Sha256::digest(fs::read(f.directory.path().join("Quest.esp")).unwrap())
    );
    for (i, row) in plan.plans().iter().enumerate() {
        assert_eq!(row.attachment().quest, keys[i]);
        assert_eq!(row.definition(), &definitions[i]);
        assert_eq!(row.attachment().source.record_file_offset, offsets[i]);
        assert_eq!(row.attachment().source.plugin, "Quest.esp");
        assert_eq!(row.attachment().source.sha256, quest_sha);
        assert_eq!(row.attachment().fields.len(), 1);
        assert_eq!(row.attachment().fields[0].decoded_offset, 0);
        assert_eq!(
            row.attachment().fields[0].key.as_ref(),
            Some(&definitions[i].key.record)
        );
        assert_eq!(
            row.attachment().source.decoded_record_sha256.as_deref(),
            Some(
                format!(
                    "{:x}",
                    Sha256::digest(field(
                        b"SCRI",
                        &(if i < 2 { 0x300_u32 } else { 0x305 }).to_le_bytes()
                    ))
                )
                .as_str()
            )
        );
    }
    let result = plan.apply(before.clone(), Default::default()).unwrap();
    let expected = many_expected(&before, &keys, &requests, &definitions);
    assert_eq!(result.snapshot, expected);
    assert_eq!(
        result
            .instances
            .iter()
            .map(|id| id.0.get())
            .collect::<Vec<_>>(),
        vec![2, 3, 4]
    );
    let bytes = result.snapshot.encode(1024 * 1024).unwrap();
    let cold = World::restore(
        Arc::clone(&f.catalogue),
        Snapshot::decode(&bytes, Default::default()).unwrap(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(cold.snapshot(), expected);
    for (key, &instance) in keys.iter().zip(&result.instances) {
        assert_eq!(
            cold.owner_instance(&Owner::Quest { key: key.clone() }),
            Some(instance)
        );
    }
    let reversed: Vec<_> = selections.iter().rev().copied().collect();
    let reverse_plan = ready_many(&sources, &f, &reversed, Default::default());
    assert_eq!(
        reverse_plan
            .apply(before.clone(), Default::default())
            .unwrap()
            .snapshot,
        many_expected(
            &before,
            &keys.into_iter().rev().collect::<Vec<_>>(),
            &requests.into_iter().rev().collect::<Vec<_>>(),
            &definitions.into_iter().rev().collect::<Vec<_>>()
        )
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn quest_set_global_exact_limits_and_late_failures_discard_every_private_owner() {
    use fallout_runtime::execution::local_copy::Intent;
    let (f, _) = many_fixture();
    let world = base(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = many_sources(&f.catalogue, &model);
    let keys = [quest(0x120), quest(0x121), quest(0x122)];
    let definitions = [form(0x300), form(0x300), form(0x305)].map(|key| {
        f.catalogue
            .record_scripts(&key)
            .next()
            .unwrap()
            .handle()
            .clone()
    });
    let requests = many_requests(world.campaign());
    let selections: Vec<_> = (0..3)
        .map(|i| attachment_boot::Selection {
            quest: &keys[i],
            expected_definition: &definitions[i],
            initialization: &requests[i],
        })
        .collect();
    let counts = ready_many(&sources, &f, &selections, Default::default()).counts();
    let exact = attachment_boot::ManyLimits {
        maximum_quests: 3,
        input: Limits {
            maximum_initializers: 9,
            maximum_context_arguments: 3,
            maximum_variable_bytes: 336,
            maximum_source_receipt_bytes: 498,
            maximum_declarations: 11,
        },
        maximum_retained_variable_bytes: 6936,
    };
    assert_eq!(
        ready_many(&sources, &f, &selections, exact).counts(),
        counts
    );
    for (name, mut limits) in (0..7).map(|i| (i, exact)) {
        match name {
            0 => limits.maximum_quests -= 1,
            1 => limits.input.maximum_initializers -= 1,
            2 => limits.input.maximum_context_arguments -= 1,
            3 => limits.input.maximum_variable_bytes -= 1,
            4 => limits.input.maximum_source_receipt_bytes -= 1,
            5 => limits.input.maximum_declarations -= 1,
            _ => limits.maximum_retained_variable_bytes -= 1,
        }
        assert!(
            matches!(
                attachment_boot::prepare_many(
                    &sources,
                    &f.attachments,
                    &f.content,
                    &selections,
                    Intent::Engineering,
                    limits
                ),
                Err(Error::Capacity(_))
            ),
            "budget {name}"
        );
    }
    let mut duplicate = selections.clone();
    duplicate[2] = selections[0];
    assert!(matches!(
        attachment_boot::prepare_many(
            &sources,
            &f.attachments,
            &f.content,
            &duplicate,
            Intent::Engineering,
            Default::default()
        ),
        Err(Error::Input("duplicate selected quest"))
    ));
    let mut stale = definitions[2].clone();
    stale.version_sha256 = "0".repeat(64);
    let mut invalid = selections.clone();
    invalid[2].expected_definition = &stale;
    assert!(
        attachment_boot::prepare_many(
            &sources,
            &f.attachments,
            &f.content,
            &invalid,
            Intent::Engineering,
            Default::default()
        )
        .is_err()
    );
    for key in [quest(0x103), quest(0x104), quest(0x200), quest(0x777)] {
        let mut invalid = selections.clone();
        invalid[2].quest = &key;
        assert!(
            attachment_boot::prepare_many(
                &sources,
                &f.attachments,
                &f.content,
                &invalid,
                Intent::Engineering,
                Default::default()
            )
            .is_err()
        );
    }
    for failure in 0..5 {
        let mut late = request(world.campaign());
        match failure {
            0 => late.initializers.push(Initializer {
                index: 999,
                value: Value::Number { bits: 1 },
            }),
            1 => late.initializers[2].value = Value::Number { bits: 1 },
            2 => {
                late.context.calling_reference = Some(fallout_runtime::identity::ReferenceId(
                    999.try_into().unwrap(),
                ))
            }
            3 => late.context.arguments.push(ReferenceValue::Live {
                id: fallout_runtime::identity::ReferenceId(999.try_into().unwrap()),
            }),
            _ => {
                late.initializers[2].value = Value::Reference {
                    value: ReferenceValue::Live {
                        id: fallout_runtime::identity::ReferenceId(999.try_into().unwrap()),
                    },
                }
            }
        }
        let mut invalid = selections.clone();
        invalid[2].initialization = &late;
        let plan = ready_many(&sources, &f, &invalid, Default::default());
        assert!(
            plan.apply(before.clone(), Default::default()).is_err(),
            "late {failure}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let plan = ready_many(&sources, &f, &selections, Default::default());
    assert!(
        plan.apply(
            before.clone(),
            fallout_runtime::Limits {
                max_instances: 3,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        plan.apply(
            before.clone(),
            fallout_runtime::Limits {
                max_locals: 14,
                ..Default::default()
            }
        )
        .is_err()
    );
    let mut owned = world.snapshot();
    owned.instances[0].owner = Owner::Quest {
        key: keys[2].clone(),
    };
    assert!(plan.apply(owned, Default::default()).is_err());
    let mut wrong = before.clone();
    wrong.campaign = CampaignId::from_bytes([0x78; 16]).unwrap();
    assert!(plan.apply(wrong, Default::default()).is_err());
    assert!(matches!(
        attachment_boot::prepare_many(
            &sources,
            &f.attachments,
            &f.content,
            &[],
            Intent::Engineering,
            Default::default()
        ),
        Err(Error::Input("quest selection is empty"))
    ));
    assert!(matches!(
        attachment_boot::prepare_many(
            &sources,
            &f.attachments,
            &f.content,
            &selections,
            Intent::Faithful,
            exact
        )
        .unwrap(),
        attachment_boot::ManyPreparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "frozen built CLI and authored executable metadata; no Original launch"]
fn cli_atomic_quest_set_boot_helper() {
    use serde_json::{Value as Json, json};
    use sha2::{Digest, Sha256};
    use std::{cell::Cell, path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_QUEST_SET_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let (f, offsets) = many_fixture();
    let mut world = base(Arc::clone(&f.catalogue));
    world
        .advance_clocks(fallout_runtime::events::Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        })
        .unwrap();
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = many_sources(&f.catalogue, &model);
    let keys = [quest(0x120), quest(0x121), quest(0x122)];
    let definitions = [form(0x300), form(0x300), form(0x305)].map(|key| {
        f.catalogue
            .record_scripts(&key)
            .next()
            .unwrap()
            .handle()
            .clone()
    });
    let requests = many_requests(world.campaign());
    let expected_snapshot = many_expected(&before, &keys, &requests, &definitions);
    let request = json!({"schema_version":1,"intent":"engineering","source_cohort_sha256":sources.source_cohort_sha256(),
        "selections":(0..3).map(|i| json!({"quest":keys[i],"expected_definition":definitions[i],"initialization":requests[i]})).collect::<Vec<_>>(),
        "maximum_quests":32,"initialization_limits":{"maximum_initializers":128,"maximum_context_arguments":64,"maximum_variable_bytes":65536,"maximum_source_receipt_bytes":1048576,"maximum_declarations":65536},
        "maximum_retained_variable_bytes":2097152,"maximum_prepared_instructions":2000000,"maximum_prepared_operand_uses":1000000,"maximum_prepared_tokens":2000000,"maximum_prepared_record_bytes":536870912,
        "maximum_trace_bytes":2097152,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    for name in ["FalloutNV.esm", "Quest.esp"] {
        fs::copy(
            f.directory.path().join(name),
            install.join("Data").join(name),
        )
        .unwrap();
    }
    fs::copy(
        metadata.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let protected_hashes: Vec<_> = ["FalloutNV.exe", "Data/FalloutNV.esm", "Data/Quest.esp"]
        .map(|p| format!("{:x}", Sha256::digest(fs::read(install.join(p)).unwrap())))
        .into();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\",\"Quest.esp\"]").unwrap();
    fs::write(
        evidence.join("independent-expected.snapshot.json"),
        expected_snapshot.encode(1024 * 1024).unwrap(),
    )
    .unwrap();
    let calls = Cell::new(0);
    let run_on = |name: &str,
                  install: &Path,
                  snapshot: &Snapshot,
                  raw: &[u8],
                  result_override: Option<&Path>,
                  report_override: Option<&Path>,
                  extra: &[&str]| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let input = directory.join("input.snapshot.json");
        let original = snapshot.encode(64 * 1024 * 1024).unwrap();
        fs::write(&input, &original).unwrap();
        let request_path = directory.join("request.json");
        fs::write(&request_path, raw).unwrap();
        let report =
            report_override.map_or_else(|| directory.join("report.json"), Path::to_path_buf);
        let result = result_override
            .map_or_else(|| directory.join("result.snapshot.json"), Path::to_path_buf);
        calls.set(calls.get() + 1);
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(install)
            .arg("--load-order")
            .arg(&order)
            .arg("--quest-boot-set-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(&input)
            .arg("--snapshot-output")
            .arg(&result)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(directory.join("result.json"), serde_json::to_vec_pretty(&json!({"success":output.status.success(),"exit_code":output.status.code(),"result_exists":result.exists(),"report_exists":report.exists()})).unwrap()).unwrap();
        assert_eq!(fs::read(input).unwrap(), original, "{name}: input mutated");
        (output, report, result)
    };
    let run = |name: &str, snapshot: &Snapshot, request: &Json| {
        run_on(
            name,
            &install,
            snapshot,
            &serde_json::to_vec(request).unwrap(),
            None,
            None,
            &[],
        )
    };
    let failed = |name: &str, snapshot: &Snapshot, request: &Json| {
        let (output, report, result) = run(name, snapshot, request);
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists(), "{name}");
        assert!(!result.exists(), "{name}");
    };
    let (output, report_path, result) = run("valid", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
    assert_eq!(actual, expected_snapshot);
    assert_eq!(
        report["quest_set_boot"],
        json!({"status":"engineering_booted","instances":[2,3,4]})
    );
    assert_eq!(
        report["trace"]["initialization_counts"],
        json!({"quests":3,"input":{"initializers":9,"context_arguments":3,"variable_bytes":336,"source_receipt_bytes":498,"declarations":11},"retained_variable_bytes":6936})
    );
    assert_eq!(
        report["prepared_sources"]["counts"]["preparation_attempts"],
        2
    );
    assert_eq!(report["prepared_sources"]["counts"]["instructions"], 4);
    assert_eq!(
        report["prepared_sources"]["counts"]["attempted_record_bytes"],
        425
    );
    assert_eq!(
        report["trace"]["source_receipts"],
        serde_json::to_value(f.attachments.source_receipts()).unwrap()
    );
    for (i, offset) in offsets.into_iter().enumerate() {
        assert_eq!(
            report["trace"]["selections"][i]["quest_attachment"]["source"]["record_file_offset"],
            offset
        );
        assert_eq!(
            report["trace"]["selections"][i]["selection"],
            request["selections"][i]
        );
        assert_eq!(
            report["trace"]["selections"][i]["quest_attachment"],
            serde_json::to_value(f.attachments.get(&keys[i]).unwrap()).unwrap()
        );
        assert_eq!(
            report["trace"]["selections"][i]["script_source"],
            serde_json::to_value(f.catalogue.get_handle(&definitions[i]).unwrap().version())
                .unwrap()
        );
    }
    assert_eq!(
        World::restore(Arc::clone(&f.catalogue), actual.clone(), Default::default())
            .unwrap()
            .snapshot(),
        expected_snapshot
    );
    for flag in [
        "event_enqueued",
        "reference_created",
        "quest_activation_verified",
        "faithful_execution_admitted",
        "retail_parity_accepted",
    ] {
        assert_eq!(report[flag], false);
    }
    let mut reverse = request.clone();
    reverse["selections"].as_array_mut().unwrap().reverse();
    let (output, _, result) = run("reverse", &before, &reverse);
    assert!(output.status.success());
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        many_expected(
            &before,
            &keys.clone().into_iter().rev().collect::<Vec<_>>(),
            &many_requests(world.campaign())
                .into_iter()
                .rev()
                .collect::<Vec<_>>(),
            &definitions.clone().into_iter().rev().collect::<Vec<_>>()
        )
    );
    let mut empty = request.clone();
    for row in empty["selections"].as_array_mut().unwrap() {
        row["initialization"]["initializers"] = json!([]);
        row["initialization"]["context"] = json!(Context::default());
    }
    let (output, _, result) = run("empty-initializers", &before, &empty);
    assert!(output.status.success());
    let empty_requests = (0..3)
        .map(|_| Request {
            campaign: world.campaign(),
            context: Context::default(),
            initializers: vec![],
        })
        .collect::<Vec<_>>();
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        many_expected(&before, &keys, &empty_requests, &definitions)
    );
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    let (output, report_path, result) = run("faithful", &before, &faithful);
    assert!(!output.status.success());
    assert!(!result.exists());
    let unsupported: Json = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
    assert_eq!(unsupported["quest_set_boot"]["status"], "unsupported");
    assert!(unsupported["result_snapshot"].is_null());
    for (name, pointer, value) in [
        (
            "duplicate-quest",
            "/selections/2",
            request["selections"][0].clone(),
        ),
        (
            "late-stale-definition",
            "/selections/2/expected_definition/version_sha256",
            json!("0".repeat(64)),
        ),
        (
            "late-wrong-script",
            "/selections/2/expected_definition",
            json!(definitions[0]),
        ),
        (
            "late-attachment",
            "/selections/2/quest",
            json!(quest(0x103)),
        ),
        (
            "late-wrong-kind",
            "/selections/2/quest",
            json!(quest(0x200)),
        ),
        (
            "late-missing-local",
            "/selections/2/initialization/initializers/0/index",
            json!(999),
        ),
        (
            "late-wrong-local-kind",
            "/selections/2/initialization/initializers/2/value",
            json!({"kind":"number","bits":1}),
        ),
        (
            "late-live-local",
            "/selections/2/initialization/initializers/2/value",
            json!({"kind":"reference","value":{"kind":"live","id":999}}),
        ),
        (
            "late-context-caller",
            "/selections/2/initialization/context/calling_reference",
            json!(999),
        ),
        (
            "late-context-target",
            "/selections/2/initialization/context/target",
            json!({"kind":"live","id":999}),
        ),
        (
            "late-campaign",
            "/selections/2/initialization/campaign",
            json!(vec![0x78; 16]),
        ),
        (
            "zero-campaign",
            "/selections/2/initialization/campaign",
            json!(vec![0; 16]),
        ),
        (
            "duplicate-local",
            "/selections/2/initialization/initializers/1/index",
            json!(2),
        ),
        ("empty-set", "/selections", json!([])),
        (
            "wrong-cohort",
            "/source_cohort_sha256",
            json!("0".repeat(64)),
        ),
        (
            "cohort-uppercase",
            "/source_cohort_sha256",
            json!("A".repeat(64)),
        ),
        ("schema", "/schema_version", json!(2)),
        ("intent-map", "/intent", json!({"engineering":null})),
        ("intent-unverified", "/intent", json!("activate")),
    ] {
        let mut invalid = request.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        failed(name, &before, &invalid);
    }
    failed("duplicate-existing-owner", &actual, &request);
    let mut existing_late = before.clone();
    existing_late.instances[0].owner = Owner::Quest {
        key: keys[2].clone(),
    };
    failed("late-existing-owner", &existing_late, &request);
    let mut legacy = before.clone();
    legacy.schema_version = 3;
    failed("legacy", &legacy, &request);
    let mut stale = before.clone();
    stale.catalogue_sha256 = "0".repeat(64);
    failed("stale-snapshot", &stale, &request);
    let mut revision = before.clone();
    revision.state_revision = u64::MAX - 4;
    failed("late-revision-overflow", &revision, &request);
    let mut ids = before.clone();
    ids.next_instance = u64::MAX - 2;
    failed("late-instance-overflow", &ids, &request);
    let cap_values = [
        ("quests", "/maximum_quests", 3),
        (
            "initializers",
            "/initialization_limits/maximum_initializers",
            9,
        ),
        (
            "arguments",
            "/initialization_limits/maximum_context_arguments",
            3,
        ),
        (
            "variables",
            "/initialization_limits/maximum_variable_bytes",
            336,
        ),
        (
            "receipts",
            "/initialization_limits/maximum_source_receipt_bytes",
            498,
        ),
        (
            "declarations",
            "/initialization_limits/maximum_declarations",
            11,
        ),
        ("retention", "/maximum_retained_variable_bytes", 6936),
        ("instructions", "/maximum_prepared_instructions", 4),
        ("record", "/maximum_prepared_record_bytes", 425),
        (
            "trace",
            "/maximum_trace_bytes",
            report["trace_bytes"].as_u64().unwrap(),
        ),
        (
            "snapshot",
            "/maximum_result_snapshot_bytes",
            expected_snapshot.encode(1024 * 1024).unwrap().len() as u64,
        ),
    ];
    for (name, pointer, exact) in cap_values {
        let mut bounded = request.clone();
        *bounded.pointer_mut(pointer).unwrap() = json!(exact);
        let (output, _, result) = run(&format!("cap-{name}-exact"), &before, &bounded);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
            expected_snapshot
        );
        *bounded.pointer_mut(pointer).unwrap() = json!(exact - 1);
        failed(&format!("cap-{name}-under"), &before, &bounded);
    }
    for pointer in ["/maximum_prepared_operand_uses", "/maximum_prepared_tokens"] {
        let mut bounded = request.clone();
        *bounded.pointer_mut(pointer).unwrap() = json!(0);
        let (output, _, _) = run(
            if pointer.contains("operand") {
                "zero-uses"
            } else {
                "zero-tokens"
            },
            &before,
            &bounded,
        );
        assert!(output.status.success());
    }
    for name in ["exact", "under"] {
        let mut expected_report = report.clone();
        expected_report["result_snapshot"]["path"] = json!(
            evidence
                .join(format!("report-{name}"))
                .join("result.snapshot.json")
        );
        let exact = serde_json::to_vec_pretty(&expected_report).unwrap().len() + 1;
        let mut bounded = request.clone();
        bounded["maximum_report_bytes"] = json!(exact - usize::from(name == "under"));
        let (output, report_path, result) = run(&format!("report-{name}"), &before, &bounded);
        assert_eq!(
            output.status.success(),
            name == "exact",
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(report_path.exists(), name == "exact");
        assert_eq!(result.exists(), name == "exact");
        if name == "exact" {
            assert_eq!(fs::read(report_path).unwrap().len(), exact);
        }
    }
    for key in request.as_object().unwrap().keys() {
        let mut invalid = request.clone();
        invalid.as_object_mut().unwrap().remove(key);
        failed(&format!("missing-{key}"), &before, &invalid);
    }
    for key in request["initialization_limits"].as_object().unwrap().keys() {
        let mut invalid = request.clone();
        invalid["initialization_limits"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        failed(&format!("missing-limit-{key}"), &before, &invalid);
    }
    for (name, pointer) in [
        ("quest", "/selections/2/quest"),
        ("definition", "/selections/2/expected_definition"),
        ("initialization", "/selections/2/initialization"),
        ("context", "/selections/2/initialization/context"),
        ("target", "/selections/2/initialization/context/target"),
        (
            "caller",
            "/selections/2/initialization/context/calling_reference",
        ),
        (
            "container",
            "/selections/2/initialization/context/containing_reference",
        ),
        ("args", "/selections/2/initialization/context/arguments"),
        ("value", "/selections/2/initialization/initializers/0/value"),
    ] {
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        let mut invalid = request.clone();
        invalid
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        failed(&format!("missing-row-{name}"), &before, &invalid);
    }
    for (name, pointer) in [
        ("root", ""),
        ("limits", "/initialization_limits"),
        ("row", "/selections/2"),
        ("quest", "/selections/2/quest"),
        ("handle", "/selections/2/expected_definition"),
        ("key", "/selections/2/expected_definition/key"),
        ("record", "/selections/2/expected_definition/key/record"),
        ("init", "/selections/2/initialization"),
        ("context", "/selections/2/initialization/context"),
        ("value", "/selections/2/initialization/initializers/0/value"),
    ] {
        let mut invalid = request.clone();
        invalid.pointer_mut(pointer).unwrap()["unknown"] = json!(true);
        failed(&format!("unknown-{name}"), &before, &invalid);
    }
    for key in request
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k.starts_with("maximum_"))
    {
        let mut invalid = request.clone();
        invalid[key] = json!(request[key].as_u64().unwrap() + 1);
        failed(&format!("ceiling-{key}"), &before, &invalid);
    }
    for key in request["initialization_limits"].as_object().unwrap().keys() {
        let mut invalid = request.clone();
        invalid["initialization_limits"][key] =
            json!(request["initialization_limits"][key].as_u64().unwrap() + 1);
        failed(&format!("ceiling-limit-{key}"), &before, &invalid);
    }
    for (name, raw) in [
        ("malformed", b"{".to_vec()),
        (
            "duplicate-field",
            b"{\"schema_version\":1,\"schema_version\":1}".to_vec(),
        ),
        ("oversized", vec![b' '; 64 * 1024 + 1]),
    ] {
        let (output, report, result) = run_on(name, &install, &before, &raw, None, None, &[]);
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    let existing = evidence.join("existing.snapshot.json");
    fs::write(&existing, b"preserve-existing").unwrap();
    let raw = serde_json::to_vec(&request).unwrap();
    let (output, report, _) = run_on(
        "existing-output",
        &install,
        &before,
        &raw,
        Some(&existing),
        None,
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert_eq!(fs::read(&existing).unwrap(), b"preserve-existing");
    let (output, _, result) = run_on(
        "existing-report",
        &install,
        &before,
        &raw,
        None,
        Some(&existing),
        &[],
    );
    assert!(!output.status.success());
    assert!(!result.exists());
    assert_eq!(fs::read(&existing).unwrap(), b"preserve-existing");
    for (name, result_path, report_path) in [
        (
            "protected-result",
            install.join("Data/forbidden.json"),
            evidence.join("unused-report.json"),
        ),
        (
            "protected-report",
            evidence.join("unused-result.json"),
            install.join("Data/forbidden-report.json"),
        ),
        (
            "same-output",
            evidence.join("same.json"),
            evidence.join("same.json"),
        ),
    ] {
        let (output, report, result) = run_on(
            name,
            &install,
            &before,
            &raw,
            Some(&result_path),
            Some(&report_path),
            &[],
        );
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, flags) in [
        ("old-boot", vec!["--quest-boot-request", "unused"]),
        ("old-boot-output", vec!["--quest-boot-output", "unused"]),
        ("copy", vec!["--snapshot-copy-request", "unused"]),
        ("batch", vec!["--snapshot-copy-batch-request", "unused"]),
        ("foreign", vec!["--snapshot-foreign-copy-request", "unused"]),
        (
            "reference",
            vec!["--snapshot-reference-copy-request", "unused"],
        ),
        ("placed", vec!["--reference-boot-request", "unused"]),
        ("event", vec!["--snapshot-event-request", "unused"]),
        (
            "literal",
            vec!["--snapshot-literal-assignment-request", "unused"],
        ),
        (
            "native-assignment",
            vec!["--snapshot-native-assignment-request", "unused"],
        ),
        ("native", vec!["--snapshot-native-request", "unused"]),
        (
            "native-plan",
            vec!["--snapshot-native-plan-request", "unused"],
        ),
        (
            "native-current",
            vec!["--snapshot-native-current", "unused"],
        ),
        ("seed", vec!["--engineering-local-copy", "unused"]),
        ("capabilities", vec!["--native-capabilities"]),
        ("player", vec!["--player-id", "1"]),
        ("prepared", vec!["--prepared-sources"]),
    ] {
        let (output, report, result) = run_on(
            &format!("conflict-{name}"),
            &install,
            &before,
            &raw,
            None,
            None,
            &flags,
        );
        assert!(!output.status.success(), "{name}");
        assert!(!report.exists());
        assert!(!result.exists());
    }
    let changed = evidence.join("changed-source-copy");
    fs::create_dir(&changed).unwrap();
    fs::create_dir(changed.join("Data")).unwrap();
    fs::copy(install.join("FalloutNV.exe"), changed.join("FalloutNV.exe")).unwrap();
    fs::copy(
        install.join("Data/Quest.esp"),
        changed.join("Data/Quest.esp"),
    )
    .unwrap();
    fs::write(
        changed.join("Data/FalloutNV.esm"),
        [
            fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
            record(b"GLOB", 0x501, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
    let (output, report, result) =
        run_on("changed-source", &changed, &before, &raw, None, None, &[]);
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!result.exists());
    let after_hashes: Vec<_> = ["FalloutNV.exe", "Data/FalloutNV.esm", "Data/Quest.esp"]
        .map(|p| format!("{:x}", Sha256::digest(fs::read(install.join(p)).unwrap())))
        .into();
    assert_eq!(after_hashes, protected_hashes);
    assert_eq!(world.snapshot(), before);
    fs::write(evidence.join("scope.json"), serde_json::to_vec_pretty(&json!({"scope":"atomic explicit engineering quest owner set","actual_cli_calls":calls.get(),"independent_counts":{"quests":3,"initializers":9,"arguments":3,"variables":336,"receipt_comparison_bytes":498,"declarations":11,"retained_copy_reservations":6936,"unique_prepared_definitions":2,"prepared_record_bytes":425},"protected_source_hashes":protected_hashes,"original_executed":false,"quest_activation_verified":false,"retail_parity_accepted":false,"input_world_unchanged":true})).unwrap()).unwrap();
}
