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
