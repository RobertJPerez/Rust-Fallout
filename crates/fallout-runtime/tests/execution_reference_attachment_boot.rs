mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    plugin,
    script_reference_attachment::{self as source, Error as SourceError, Limits as SourceLimits},
    store::RecordStore,
};
use fallout_runtime::{
    World,
    events::{Context, Trigger},
    execution::{
        attachment_boot::Request,
        copy_probe::Initializer,
        reference_attachment_boot::{self as boot, Error, Intent, Preparation, Selection},
    },
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::{Local, ScriptInstance, Snapshot},
};
use std::{fs, path::Path, sync::Arc};

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
fn id(n: u64) -> ReferenceId {
    ReferenceId(n.try_into().unwrap())
}
fn script() -> Vec<u8> {
    [
        field(b"EDID", b"attached\0"),
        unit(&[(2, 1), (42, 0), (90, 0)], &[(b"SCRV", 90)]),
    ]
    .concat()
}
fn placement(base: u32) -> Vec<u8> {
    [
        field(b"EDID", b"placed\0"),
        field(b"NAME", &base.to_le_bytes()),
        field(b"DATA", &[0; 24]),
    ]
    .concat()
}
fn container(script: u32) -> Vec<u8> {
    [
        field(b"EDID", b"box\0"),
        field(b"SCRI", &script.to_le_bytes()),
        field(b"DATA", &[&[0], 1f32.to_le_bytes().as_slice()].concat()),
    ]
    .concat()
}
fn write(
    directory: &Path,
    base: Vec<u8>,
    placed: Vec<u8>,
    script_payload: Vec<u8>,
    script_flags: u32,
    base_kind: &[u8; 4],
    script_kind: &[u8; 4],
) {
    let mut script_record = record(script_kind, 0x300, script_flags, &script_payload);
    script_record[20..22].copy_from_slice(&65535u16.to_le_bytes());
    fs::write(
        directory.join("FalloutNV.esm"),
        [
            header(&[]),
            script_record,
            record(base_kind, 0x400, 0, &base),
            record(b"REFR", 0x500, 0, &placed),
            record(b"REFR", 0x501, 0, &placement(0x400)),
            record(b"CELL", 0x600, 0, &field(b"DATA", &[1])),
        ]
        .concat(),
    )
    .unwrap();
}
fn store(directory: &Path) -> RecordStore {
    RecordStore::open_nv_headers(directory, &["FalloutNV.esm".into()], Default::default()).unwrap()
}
struct Fixture {
    directory: tempfile::TempDir,
    catalogue: Arc<Catalogue>,
    content: Content,
}
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path(),
        container(0x300),
        placement(0x400),
        script(),
        0,
        b"CONT",
        b"SCPT",
    );
    let mut store = store(directory.path());
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    Fixture {
        directory,
        catalogue,
        content,
    }
}
fn world(catalogue: Arc<Catalogue>) -> World<'static> {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([0x28; 16]).unwrap(),
    )
    .unwrap();
    assert_eq!(world.register_reference(Some(form(0x500))).unwrap(), id(1));
    assert_eq!(world.register_reference(Some(form(0x501))).unwrap(), id(2));
    assert_eq!(world.register_reference(None).unwrap(), id(3));
    let pose = fallout_runtime::reference_state::Pose::from_source(
        &fallout_data::world::Transform {
            position: [f32::from_bits(0x80000000), f32::from_bits(1), 19.0],
            rotation: [0.0, 1.0, 3.0],
        },
        None,
    )
    .unwrap();
    let state = fallout_runtime::reference_state::State::new(form(0x600), pose, false).unwrap();
    let stage = world
        .stage_reference_state(&world.reference_view(id(2)).unwrap(), state)
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    world.initialize_inventory(id(2)).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x400));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc12345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"BLOB",
            bytes: vec![0, 128, 255],
        });
    world
        .add_item(id(2), facts, 19.try_into().unwrap())
        .unwrap();
    let handle = world
        .create_instance(
            catalogue
                .record_scripts(&form(0x300))
                .next()
                .unwrap()
                .handle(),
            Owner::Placed { reference: id(2) },
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
fn request(campaign: CampaignId) -> Request {
    Request {
        campaign,
        context: Context {
            calling_reference: Some(id(3)),
            containing_reference: Some(id(2)),
            target: Some(ReferenceValue::Live { id: id(1) }),
            arguments: vec![
                ReferenceValue::Null,
                ReferenceValue::Content { key: form(0x400) },
            ],
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
                    value: ReferenceValue::Live { id: id(3) },
                },
            },
        ],
    }
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
fn ready<'p, 's>(
    sources: &'p PreparedSources<'s>,
    attachment: &'p source::Request<'s>,
    content: &'p Content,
    key: &'p fallout_data::identity::FormKey,
    request: &'p Request,
    limits: boot::Limits,
) -> boot::BootPlan<'p, 's> {
    match boot::prepare(
        sources,
        attachment,
        content,
        Selection {
            reference: id(1),
            expected_authored_key: key,
            intent: Intent::Engineering,
        },
        request,
        limits,
    )
    .unwrap()
    {
        Preparation::Ready(plan) => plan,
        _ => panic!("engineering plan refused"),
    }
}
fn expected(
    before: &Snapshot,
    request: &Request,
    definition: &fallout_data::loaded_scripts::Handle,
) -> Snapshot {
    let mut result = before.clone();
    let instance = fallout_runtime::identity::InstanceId(result.next_instance.try_into().unwrap());
    result.next_instance += 1;
    result.state_revision += 2;
    result.instances.push(ScriptInstance {
        id: instance,
        definition: definition.clone(),
        owner: Owner::Placed { reference: id(1) },
        context: request.context.clone(),
        locals: vec![
            Local {
                index: 2,
                value: Value::Number {
                    bits: 0x8000000000000000,
                },
            },
            Local {
                index: 42,
                value: Value::Number {
                    bits: 0x7ff8123456789abc,
                },
            },
            Local {
                index: 90,
                value: Value::Reference {
                    value: ReferenceValue::Live { id: id(3) },
                },
            },
        ],
    });
    result
}
#[test]
fn two_existing_references_share_base_but_only_exact_selected_owner_boots_and_cold_restores() {
    let f = fixture();
    let mut store = store(f.directory.path());
    let before_source = fs::read(f.directory.path().join("FalloutNV.esm")).unwrap();
    let attachment =
        source::request(&mut store, &f.catalogue, &form(0x500), Default::default()).unwrap();
    assert_eq!(attachment.proof().placement.key, form(0x500));
    assert_eq!(attachment.proof().name.decoded_offset, 13);
    assert_eq!(attachment.proof().name.raw_form, 0x400);
    assert_eq!(attachment.proof().name.key, form(0x400));
    assert_eq!(attachment.proof().scri.decoded_offset, 10);
    assert_eq!(attachment.proof().scri.raw_form, 0x300);
    assert_eq!(attachment.proof().scri.key, form(0x300));
    assert_eq!(attachment.proof().script.header.offset, 42);
    assert_eq!(attachment.proof().script.header.version, 65535); // raw header version; SCRI has no version branch
    assert_eq!(attachment.definition().key.header_decoded_offset, 15);
    assert_eq!(attachment.script_version().source_plugin, "FalloutNV.esm");
    assert_eq!(attachment.script_version().record_file_offset, 42);
    let world = world(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let request = request(world.campaign());
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    let key = form(0x500);
    let plan = ready(
        &sources,
        &attachment,
        &f.content,
        &key,
        &request,
        Default::default(),
    );
    let result = plan.apply(before.clone(), Default::default()).unwrap();
    assert_eq!(
        result.snapshot,
        expected(&before, &request, attachment.definition())
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
        cold.owner_instance(&Owner::Placed { reference: id(1) }),
        Some(result.instance)
    );
    assert_eq!(
        cold.instance(cold.handle(result.instance).unwrap())
            .unwrap()
            .definition(),
        attachment.definition()
    );
    assert_eq!(cold.authored_reference(&form(0x501)), Some(id(2)));
    assert_eq!(
        fs::read(f.directory.path().join("FalloutNV.esm")).unwrap(),
        before_source
    );
    assert!(matches!(
        plan.apply(result.snapshot, Default::default()),
        Err(Error::Input(
            "selected placed owner already has a script instance"
        ))
    ));
}
#[test]
fn origin_owner_campaign_late_initializers_context_and_capacity_never_expose_partial_world() {
    let f = fixture();
    let attachment = source::request(
        &mut store(f.directory.path()),
        &f.catalogue,
        &form(0x500),
        Default::default(),
    )
    .unwrap();
    let world = world(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    let key = form(0x500);
    for reference in [id(2), id(3), id(99)] {
        let request = request(world.campaign());
        let Preparation::Ready(plan) = boot::prepare(
            &sources,
            &attachment,
            &f.content,
            Selection {
                reference,
                expected_authored_key: &key,
                intent: Intent::Engineering,
            },
            &request,
            Default::default(),
        )
        .unwrap() else {
            panic!()
        };
        assert!(plan.apply(before.clone(), Default::default()).is_err());
    }
    let wrong_key = form(0x501);
    let request = request(world.campaign());
    assert!(matches!(
        boot::prepare(
            &sources,
            &attachment,
            &f.content,
            Selection {
                reference: id(1),
                expected_authored_key: &wrong_key,
                intent: Intent::Engineering
            },
            &request,
            Default::default()
        ),
        Err(Error::Input(_))
    ));
    for case in 0..8 {
        let mut request = crate::request(world.campaign());
        let mut input = before.clone();
        let mut limits = fallout_runtime::Limits::default();
        match case {
            0 => request.initializers.push(Initializer {
                index: 999,
                value: Value::Number { bits: 1 },
            }),
            1 => request.initializers[2].value = Value::Number { bits: 1 },
            2 => request.context.calling_reference = Some(id(999)),
            3 => {
                request.initializers[2].value = Value::Reference {
                    value: ReferenceValue::Live { id: id(999) },
                }
            }
            4 => request.campaign = CampaignId::from_bytes([1; 16]).unwrap(),
            5 => input.state_revision = u64::MAX - 1, // create succeeds; assignment revision fails
            6 => limits.max_instances = before.instances.len(),
            7 => limits.max_locals = 3,
            _ => unreachable!(),
        }
        let plan = ready(
            &sources,
            &attachment,
            &f.content,
            &key,
            &request,
            Default::default(),
        );
        assert!(plan.apply(input, limits).is_err(), "case {case}");
        assert_eq!(world.snapshot(), before);
    }
    assert!(matches!(
        boot::prepare(
            &sources,
            &attachment,
            &f.content,
            Selection {
                reference: id(99),
                expected_authored_key: &wrong_key,
                intent: Intent::Faithful
            },
            &request,
            Default::default()
        )
        .unwrap(),
        Preparation::Unsupported { .. }
    ));
}
#[test]
fn every_initializer_and_source_cap_accepts_exact_and_refuses_one_under() {
    let f = fixture();
    let mut store = store(f.directory.path());
    let attachment =
        source::request(&mut store, &f.catalogue, &form(0x500), Default::default()).unwrap();
    let c = attachment.counts();
    for field in 0..8 {
        let mut cap = SourceLimits::default();
        match field {
            0 => cap.maximum_sources = c.sources,
            1 => cap.maximum_source_bytes = c.source_bytes,
            2 => cap.maximum_header_visits = c.header_visits,
            3 => cap.maximum_catalogue_scripts = c.catalogue_scripts,
            4 => cap.maximum_variable_bytes = c.variable_bytes,
            5 => cap.maximum_record_bytes = 205,
            6 => cap.maximum_read_bytes = c.read_bytes,
            7 => cap.maximum_field_visits = c.field_visits,
            _ => unreachable!(),
        }
        source::request(&mut store, &f.catalogue, &form(0x500), cap).unwrap();
        match field {
            0 => cap.maximum_sources -= 1,
            1 => cap.maximum_source_bytes -= 1,
            2 => cap.maximum_header_visits -= 1,
            3 => cap.maximum_catalogue_scripts -= 1,
            4 => cap.maximum_variable_bytes -= 1,
            5 => cap.maximum_record_bytes -= 1,
            6 => cap.maximum_read_bytes -= 1,
            7 => cap.maximum_field_visits -= 1,
            _ => unreachable!(),
        }
        assert!(
            source::request(&mut store, &f.catalogue, &form(0x500), cap).is_err(),
            "source cap {field}"
        );
    }
    let world = world(Arc::clone(&f.catalogue));
    let request = request(world.campaign());
    let operators = operators();
    let model = Model::vanilla(&operators).unwrap();
    let sources = sources(&f.catalogue, &model);
    let key = form(0x500);
    let c = ready(
        &sources,
        &attachment,
        &f.content,
        &key,
        &request,
        Default::default(),
    )
    .counts();
    for field in 0..5 {
        let mut cap = boot::Limits::default();
        match field {
            0 => cap.maximum_initializers = c.initializers,
            1 => cap.maximum_context_arguments = c.context_arguments,
            2 => cap.maximum_variable_bytes = c.variable_bytes,
            3 => cap.maximum_source_receipt_bytes = c.source_receipt_bytes,
            4 => cap.maximum_declarations = c.declarations,
            _ => unreachable!(),
        }
        ready(&sources, &attachment, &f.content, &key, &request, cap);
        match field {
            0 => cap.maximum_initializers -= 1,
            1 => cap.maximum_context_arguments -= 1,
            2 => cap.maximum_variable_bytes -= 1,
            3 => cap.maximum_source_receipt_bytes -= 1,
            4 => cap.maximum_declarations -= 1,
            _ => unreachable!(),
        }
        assert!(
            boot::prepare(
                &sources,
                &attachment,
                &f.content,
                Selection {
                    reference: id(1),
                    expected_authored_key: &key,
                    intent: Intent::Engineering
                },
                &request,
                cap
            )
            .is_err(),
            "initializer cap {field}"
        );
    }
}
#[test]
fn missing_null_duplicate_deleted_wrong_kind_and_multiple_units_cannot_seal_a_chain() {
    for case in 0..14 {
        let directory = tempfile::tempdir().unwrap();
        let mut base = container(0x300);
        let mut placed = placement(0x400);
        let mut scpt = script();
        let mut flags = 0;
        let mut base_kind = b"CONT";
        let mut script_kind = b"SCPT";
        match case {
            0 => base = field(b"EDID", b"box\0"),
            1 => base = container(0),
            2 => base.extend(field(b"SCRI", &0x300u32.to_le_bytes())),
            3 => base = container(0x999),
            4 => flags = plugin::DELETED,
            5 => script_kind = b"ACTI",
            6 => scpt.extend(script()),
            7 => scpt = field(b"EDID", b"missing\0"),
            8 => base_kind = b"ACTI",
            9 => base = field(b"SCRI", &[1, 2, 3]),
            10 => placed = placement(0),
            11 => placed = placement(0x999),
            12 => placed.extend(field(b"NAME", &0x400u32.to_le_bytes())),
            13 => placed = field(b"DATA", &[0; 24]),
            _ => unreachable!(),
        }
        write(
            directory.path(),
            base,
            placed,
            scpt,
            flags,
            base_kind,
            script_kind,
        );
        let mut store = store(directory.path());
        let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
        assert!(
            source::request(&mut store, &catalogue, &form(0x500), Default::default()).is_err(),
            "source case {case}"
        );
    }
}
#[test]
fn changed_source_bodies_even_with_relabelled_receipts_and_changed_winners_are_rejected() {
    let f = fixture();
    let mut catalogue = Arc::try_unwrap(f.catalogue).ok().unwrap();
    // Protected retained handles close before writing only these authored files.
    fs::write(
        f.directory.path().join("Patch.esp"),
        [
            header(&["FalloutNV.esm"]),
            record(b"REFR", 0x500, 0, &placement(0x401)),
            record(b"CONT", 0x401, 0, &container(0x300)),
        ]
        .concat(),
    )
    .unwrap();
    let mut changed = RecordStore::open_nv_headers(
        f.directory.path(),
        &["FalloutNV.esm".into(), "Patch.esp".into()],
        Default::default(),
    )
    .unwrap();
    catalogue.sources = changed.source_receipts().unwrap();
    assert!(matches!(
        source::request(&mut changed, &catalogue, &form(0x500), Default::default()),
        Err(SourceError::Unavailable("winning header cohort differs"))
    ));
    drop(changed);
    let mut body = script();
    body.extend(field(b"ZZZZ", &[1]));
    write(
        f.directory.path(),
        container(0x300),
        placement(0x400),
        body,
        0,
        b"CONT",
        b"SCPT",
    );
    let mut changed = store(f.directory.path());
    catalogue.sources = changed.source_receipts().unwrap();
    assert!(matches!(
        source::request(&mut changed, &catalogue, &form(0x500), Default::default()),
        Err(SourceError::Unavailable(
            "retained script source digest differs"
        ))
    ));
}

#[test]
#[ignore = "frozen CLI and authored executable metadata; existing reference boot only, no original launch"]
fn cli_reference_attachment_boot_helper() {
    use serde_json::{Value as Json, json};
    use std::{cell::Cell, path::PathBuf, process::Command};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_REFERENCE_BOOT_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let f = fixture();
    let world = world(Arc::clone(&f.catalogue));
    let before = world.snapshot();
    let initialization = request(world.campaign());
    let mut source_store = store(f.directory.path());
    let attachment = source::request(
        &mut source_store,
        &f.catalogue,
        &form(0x500),
        Default::default(),
    )
    .unwrap();
    let expected_snapshot = expected(&before, &initialization, attachment.definition());
    let source_defaults = SourceLimits::default();
    let init_defaults = boot::Limits::default();
    let p = fallout_runtime::programs::Limits::default();
    let request = json!({"schema_version":1,"reference":1,"expected_authored_key":form(0x500),"intent":"engineering","initialization":initialization,
        "source_limits":{"maximum_sources":source_defaults.maximum_sources,"maximum_source_bytes":source_defaults.maximum_source_bytes,"maximum_header_visits":source_defaults.maximum_header_visits,"maximum_catalogue_scripts":source_defaults.maximum_catalogue_scripts,"maximum_variable_bytes":source_defaults.maximum_variable_bytes,"maximum_record_bytes":source_defaults.maximum_record_bytes,"maximum_read_bytes":source_defaults.maximum_read_bytes,"maximum_field_visits":source_defaults.maximum_field_visits},
        "initialization_limits":{"maximum_initializers":init_defaults.maximum_initializers,"maximum_context_arguments":init_defaults.maximum_context_arguments,"maximum_variable_bytes":init_defaults.maximum_variable_bytes,"maximum_source_receipt_bytes":init_defaults.maximum_source_receipt_bytes,"maximum_declarations":init_defaults.maximum_declarations},
        "maximum_source_instructions":p.maximum_instructions,"maximum_source_operand_uses":p.maximum_uses,"maximum_source_tokens":p.maximum_tokens,"maximum_trace_bytes":2*1024*1024,"maximum_result_snapshot_bytes":64*1024*1024,"maximum_report_bytes":8*1024*1024});
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        f.directory.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        metadata.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let source_before = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let exe_before = fs::read(install.join("FalloutNV.exe")).unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let calls = Cell::new(0usize);
    let run_on = |name: &str,
                  selected_install: &Path,
                  input: &[u8],
                  raw_request: &[u8],
                  result_override: Option<&Path>,
                  report_override: Option<&Path>,
                  extra: &[&str]| {
        let dir = evidence.join(name);
        fs::create_dir(&dir).unwrap();
        let input_path = dir.join("input.snapshot.json");
        fs::write(&input_path, input).unwrap();
        let request_path = dir.join("request.json");
        fs::write(&request_path, raw_request).unwrap();
        let result =
            result_override.map_or_else(|| dir.join("result.snapshot.json"), Path::to_path_buf);
        let report = report_override.map_or_else(|| dir.join("report.json"), Path::to_path_buf);
        let order_before = fs::read(&order).unwrap();
        calls.set(calls.get() + 1);
        let output = Command::new(&cli)
            .args(["event-operands", "--install"])
            .arg(selected_install)
            .arg("--load-order")
            .arg(&order)
            .arg("--reference-boot-request")
            .arg(&request_path)
            .arg("--snapshot-input")
            .arg(&input_path)
            .arg("--snapshot-output")
            .arg(&result)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(dir.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(dir.join("stderr.txt"), &output.stderr).unwrap();
        fs::write(dir.join("result.json"),serde_json::to_vec_pretty(&json!({"success":output.status.success(),"exit_code":output.status.code(),"snapshot_exists":result.exists(),"report_exists":report.exists()})).unwrap()).unwrap();
        assert_eq!(
            fs::read(&input_path).unwrap(),
            input,
            "{name}: input changed"
        );
        assert_eq!(
            fs::read(&request_path).unwrap(),
            raw_request,
            "{name}: request changed"
        );
        assert_eq!(
            fs::read(&order).unwrap(),
            order_before,
            "{name}: order changed"
        );
        (output, report, result)
    };
    let input = before.encode(64 * 1024 * 1024).unwrap();
    let run = |name: &str, snapshot: &Snapshot, request: &Json| {
        run_on(
            name,
            &install,
            &snapshot.encode(64 * 1024 * 1024).unwrap(),
            &serde_json::to_vec(request).unwrap(),
            None,
            None,
            &[],
        )
    };
    let refused = |name: &str, snapshot: &Snapshot, request: &Json| {
        let (output, report, result) = run(name, snapshot, request);
        assert!(!output.status.success(), "{name}: succeeded");
        assert!(!report.exists(), "{name}: report");
        assert!(!result.exists(), "{name}: snapshot");
    };
    let (output, report_path, result_path) = run("copy-0", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(&report_path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    let actual = Snapshot::decode(&fs::read(&result_path).unwrap(), Default::default()).unwrap();
    assert_eq!(actual, expected_snapshot);
    assert_eq!(report["reference_boot"]["instance"], 2);
    assert_eq!(report["trace"]["reference"], 1);
    assert_eq!(report["trace"]["attachment"]["name"]["decoded_offset"], 13);
    assert_eq!(report["trace"]["attachment"]["name"]["raw_form"], 0x400);
    assert_eq!(report["trace"]["attachment"]["scri"]["decoded_offset"], 10);
    assert_eq!(report["trace"]["attachment"]["scri"]["raw_form"], 0x300);
    assert_eq!(
        report["trace"]["attachment"]["script"]["header"]["version"],
        65535
    );
    assert_eq!(
        report["trace"]["definition"],
        serde_json::to_value(attachment.definition()).unwrap()
    );
    assert_eq!(report["event_enqueued"], false);
    assert_eq!(report["original_activation_verified"], false);
    assert_eq!(report["result_snapshot"]["decode_restore_equal"], true);
    let cold =
        World::restore(Arc::clone(&f.catalogue), actual.clone(), Default::default()).unwrap();
    assert_eq!(cold.snapshot(), actual);
    // Literal empty initialization leaves every slot uninitialized and advances
    // the revision only for instance creation. Context defaults are explicit.
    let mut empty = request.clone();
    empty["initialization"]["initializers"] = json!([]);
    empty["initialization"]["context"] =
        json!({"calling_reference":null,"containing_reference":null,"target":null,"arguments":[]});
    let (output, _, path) = run("empty-initialization", &before, &empty);
    assert!(output.status.success());
    let mut empty_expected = expected_snapshot.clone();
    empty_expected.state_revision -= 1;
    let created = empty_expected.instances.last_mut().unwrap();
    created.context = Context::default();
    for local in &mut created.locals {
        local.value = Value::Uninitialized;
    }
    assert_eq!(
        Snapshot::decode(&fs::read(path).unwrap(), Default::default()).unwrap(),
        empty_expected
    );
    let mut null = request.clone();
    null["initialization"]["initializers"][2]["value"] =
        json!({"kind":"reference","value":{"kind":"null"}});
    let (output, _, path) = run("null-reference-local", &before, &null);
    assert!(output.status.success());
    let mut null_expected = expected_snapshot.clone();
    null_expected.instances.last_mut().unwrap().locals[2].value = Value::Reference {
        value: ReferenceValue::Null,
    };
    assert_eq!(
        Snapshot::decode(&fs::read(path).unwrap(), Default::default()).unwrap(),
        null_expected
    );
    // Selection stays a typed integer past binary64's exact integer range.
    let large = 9_007_199_254_741_099u64;
    let mut large_before = before.clone();
    large_before.references[0].id = id(large);
    large_before.references.sort_by_key(|row| row.id);
    large_before.next_reference = large + 1;
    let mut large_request = request.clone();
    large_request["reference"] = json!(large);
    large_request["initialization"]["context"]["target"]["id"] = json!(large);
    let (output, large_report, path) = run("large-reference-id", &large_before, &large_request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut large_expected = expected_snapshot.clone();
    large_expected.references = large_before.references.clone();
    large_expected.next_reference = large + 1;
    let created = large_expected.instances.last_mut().unwrap();
    created.owner = Owner::Placed {
        reference: id(large),
    };
    created.context.target = Some(ReferenceValue::Live { id: id(large) });
    assert_eq!(
        Snapshot::decode(&fs::read(path).unwrap(), Default::default()).unwrap(),
        large_expected
    );
    let large_report: Json = serde_json::from_slice(&fs::read(large_report).unwrap()).unwrap();
    assert_eq!(large_report["trace"]["reference"], large);
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    let (output, path, result) = run("faithful", &before, &faithful);
    assert!(!output.status.success());
    assert!(!result.exists());
    let r: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(r["reference_boot"]["status"], "unsupported");
    assert_eq!(r["reference_boot"]["reason"], "unverified_retail_semantics");
    for (name, pointer, value) in [
        ("missing-reference", "/reference", json!(999)),
        ("zero-reference", "/reference", json!(0)),
        ("dynamic-reference", "/reference", json!(3)),
        ("swapped-reference", "/reference", json!(2)),
        (
            "duplicate-owner",
            "/expected_authored_key",
            json!(form(0x501)),
        ),
        ("wrong-form", "/expected_authored_key", json!(form(0x400))),
        (
            "wrong-profile",
            "/expected_authored_key/profile",
            json!("fo3-original"),
        ),
        (
            "noncanonical-key",
            "/expected_authored_key/origin_plugin",
            json!("FalloutNV.esm"),
        ),
        (
            "missing-local",
            "/initialization/initializers/2/index",
            json!(999),
        ),
        (
            "incompatible-local",
            "/initialization/initializers/2/value",
            json!({"kind":"number","bits":1}),
        ),
        (
            "missing-local-reference",
            "/initialization/initializers/2/value/value/id",
            json!(999),
        ),
        (
            "wrong-campaign",
            "/initialization/campaign",
            json!(vec![1; 16]),
        ),
        (
            "missing-calling",
            "/initialization/context/calling_reference",
            json!(999),
        ),
        (
            "missing-containing",
            "/initialization/context/containing_reference",
            json!(999),
        ),
        (
            "missing-target",
            "/initialization/context/target/id",
            json!(999),
        ),
        (
            "missing-argument",
            "/initialization/context/arguments/0",
            json!({"kind":"live","id":999}),
        ),
        ("unknown-intent", "/intent", json!("automatic")),
        ("wrong-schema", "/schema_version", json!(2)),
    ] {
        let mut invalid = request.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        if name == "duplicate-owner" {
            invalid["reference"] = json!(2);
        }
        refused(name, &before, &invalid);
    }
    let mut duplicate = request.clone();
    let entry = duplicate["initialization"]["initializers"][0].clone();
    duplicate["initialization"]["initializers"]
        .as_array_mut()
        .unwrap()
        .push(entry);
    refused("duplicate-initializer", &before, &duplicate);
    for (name, pointer, field) in [
        (
            "absent-calling",
            "/initialization/context",
            "calling_reference",
        ),
        (
            "absent-containing",
            "/initialization/context",
            "containing_reference",
        ),
        ("absent-target", "/initialization/context", "target"),
        ("absent-arguments", "/initialization/context", "arguments"),
        ("absent-source-cap", "/source_limits", "maximum_read_bytes"),
        (
            "absent-init-cap",
            "/initialization_limits",
            "maximum_variable_bytes",
        ),
        ("absent-owner", "", "expected_authored_key"),
        ("absent-intent", "", "intent"),
        ("absent-trace-cap", "", "maximum_trace_bytes"),
    ] {
        let mut invalid = request.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        refused(name, &before, &invalid);
    }
    for (name, pointer) in [
        ("unknown-root", ""),
        ("unknown-context", "/initialization/context"),
        ("unknown-source-limit", "/source_limits"),
        ("unknown-init-limit", "/initialization_limits"),
    ] {
        let mut invalid = request.clone();
        invalid
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(1));
        refused(name, &before, &invalid);
    }
    for (name, mut invalid) in [
        ("old-snapshot", before.clone()),
        ("changed-snapshot-cohort", before.clone()),
        ("revision-exhausted", before.clone()),
        ("late-revision-exhausted", before.clone()),
        ("instance-counter-exhausted", before.clone()),
    ] {
        match name {
            "old-snapshot" => invalid.schema_version = 3,
            "changed-snapshot-cohort" => invalid.catalogue_sha256 = "0".repeat(64),
            "revision-exhausted" => invalid.state_revision = u64::MAX,
            "late-revision-exhausted" => invalid.state_revision = u64::MAX - 1,
            "instance-counter-exhausted" => invalid.next_instance = u64::MAX,
            _ => unreachable!(),
        }
        if name == "old-snapshot" {
            let (output, report, result) = run_on(
                name,
                &install,
                &serde_json::to_vec(&invalid).unwrap(),
                &serde_json::to_vec(&request).unwrap(),
                None,
                None,
                &[],
            );
            assert!(!output.status.success());
            assert!(!report.exists());
            assert!(!result.exists());
        } else {
            refused(name, &invalid, &request);
        }
    }
    let source_counts = attachment.counts();
    let init_counts = report["trace"]["initialization_counts"].clone();
    for (field, value) in [
        ("maximum_sources", source_counts.sources as u64),
        ("maximum_source_bytes", source_counts.source_bytes),
        ("maximum_header_visits", source_counts.header_visits as u64),
        (
            "maximum_catalogue_scripts",
            source_counts.catalogue_scripts as u64,
        ),
        (
            "maximum_variable_bytes",
            source_counts.variable_bytes as u64,
        ),
        ("maximum_record_bytes", 205),
        // Existing catalogue candidates include both REFR records: literal
        // SCPT205 + REFR53 + REFR53 =311. The fresh chain separately reads289.
        ("maximum_read_bytes", 311),
        ("maximum_field_visits", source_counts.field_visits as u64),
    ] {
        let mut exact = request.clone();
        exact["source_limits"][field] = json!(value);
        let name = format!("source-exact-{field}");
        let (output, _, path) = run(&name, &before, &exact);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            Snapshot::decode(&fs::read(path).unwrap(), Default::default()).unwrap(),
            expected_snapshot
        );
        exact["source_limits"][field] = json!(value - 1);
        refused(&format!("source-under-{field}"), &before, &exact);
    }
    for (field, count) in [
        ("maximum_initializers", "initializers"),
        ("maximum_context_arguments", "context_arguments"),
        ("maximum_variable_bytes", "variable_bytes"),
        ("maximum_source_receipt_bytes", "source_receipt_bytes"),
        ("maximum_declarations", "declarations"),
    ] {
        let value = init_counts[count].as_u64().unwrap();
        let mut exact = request.clone();
        exact["initialization_limits"][field] = json!(value);
        let (output, _, path) = run(&format!("init-exact-{field}"), &before, &exact);
        assert!(output.status.success());
        assert_eq!(
            Snapshot::decode(&fs::read(path).unwrap(), Default::default()).unwrap(),
            expected_snapshot
        );
        exact["initialization_limits"][field] = json!(value - 1);
        refused(&format!("init-under-{field}"), &before, &exact);
    }
    let instructions = report["prepared_sources"]["counts"]["instructions"]
        .as_u64()
        .unwrap();
    let mut exact = request.clone();
    exact["maximum_source_instructions"] = json!(instructions);
    let (output, _, _) = run("exact-instructions", &before, &exact);
    assert!(output.status.success());
    exact["maximum_source_instructions"] = json!(instructions - 1);
    refused("under-instructions", &before, &exact);
    for field in ["maximum_source_operand_uses", "maximum_source_tokens"] {
        let mut zero = request.clone();
        zero[field] = json!(0);
        let (output, _, _) = run(&format!("zero-{field}"), &before, &zero);
        assert!(output.status.success());
    }
    for (field, value) in [
        (
            "maximum_trace_bytes",
            report["trace_bytes"].as_u64().unwrap(),
        ),
        (
            "maximum_result_snapshot_bytes",
            fs::metadata(&result_path).unwrap().len(),
        ),
        ("maximum_report_bytes", report_bytes.len() as u64),
    ] {
        let mut exact = request.clone();
        exact[field] = json!(value);
        let name = if field == "maximum_report_bytes" {
            "exact0".to_string()
        } else {
            format!("exact-{field}")
        };
        let (output, _, _) = run(&name, &before, &exact);
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        exact[field] = json!(value - 1);
        refused(&format!("under-{field}"), &before, &exact);
    }
    for (group, field, value) in [
        (
            "source_limits",
            "maximum_sources",
            source_defaults.maximum_sources as u64,
        ),
        (
            "source_limits",
            "maximum_source_bytes",
            source_defaults.maximum_source_bytes,
        ),
        (
            "source_limits",
            "maximum_header_visits",
            source_defaults.maximum_header_visits as u64,
        ),
        (
            "source_limits",
            "maximum_catalogue_scripts",
            source_defaults.maximum_catalogue_scripts as u64,
        ),
        (
            "source_limits",
            "maximum_variable_bytes",
            source_defaults.maximum_variable_bytes as u64,
        ),
        (
            "source_limits",
            "maximum_record_bytes",
            source_defaults.maximum_record_bytes as u64,
        ),
        (
            "source_limits",
            "maximum_read_bytes",
            source_defaults.maximum_read_bytes as u64,
        ),
        (
            "source_limits",
            "maximum_field_visits",
            source_defaults.maximum_field_visits as u64,
        ),
        (
            "initialization_limits",
            "maximum_initializers",
            init_defaults.maximum_initializers as u64,
        ),
        (
            "initialization_limits",
            "maximum_context_arguments",
            init_defaults.maximum_context_arguments as u64,
        ),
        (
            "initialization_limits",
            "maximum_variable_bytes",
            init_defaults.maximum_variable_bytes as u64,
        ),
        (
            "initialization_limits",
            "maximum_source_receipt_bytes",
            init_defaults.maximum_source_receipt_bytes as u64,
        ),
        (
            "initialization_limits",
            "maximum_declarations",
            init_defaults.maximum_declarations as u64,
        ),
        (
            "",
            "maximum_source_instructions",
            p.maximum_instructions as u64,
        ),
        ("", "maximum_source_operand_uses", p.maximum_uses as u64),
        ("", "maximum_source_tokens", p.maximum_tokens as u64),
        ("", "maximum_trace_bytes", 2 * 1024 * 1024),
        ("", "maximum_result_snapshot_bytes", 64 * 1024 * 1024),
        ("", "maximum_report_bytes", 8 * 1024 * 1024),
    ] {
        let mut invalid = request.clone();
        *invalid
            .pointer_mut(&format!("/{}/{}", group, field).replace("//", "/"))
            .unwrap() = json!(value + 1);
        refused(&format!("ceiling-{group}-{field}"), &before, &invalid);
    }
    for (name, raw) in [
        (
            "duplicate-request-field",
            serde_json::to_vec(&request)
                .unwrap()
                .iter()
                .copied()
                .take(serde_json::to_vec(&request).unwrap().len() - 1)
                .chain(b",\"reference\":1}".iter().copied())
                .collect::<Vec<_>>(),
        ),
        ("request-size", vec![b' '; 16 * 1024 + 1]),
        ("malformed-request", b"{".to_vec()),
    ] {
        let (output, report, result) = run_on(name, &install, &input, &raw, None, None, &[]);
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for (name, extra) in [
        ("conflicting-prepared", vec!["--prepared-sources"]),
        ("conflicting-player", vec!["--player-id", "1"]),
        (
            "conflicting-quest-output",
            vec!["--quest-boot-output", "missing"],
        ),
        (
            "conflicting-copy",
            vec!["--snapshot-copy-request", "missing"],
        ),
        ("conflicting-native", vec!["--native-capabilities"]),
    ] {
        let (output, report, result) = run_on(
            name,
            &install,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            None,
            None,
            &extra,
        );
        assert!(!output.status.success());
        assert!(!report.exists());
        assert!(!result.exists());
    }
    for case in 0..14 {
        let alt = evidence.join(format!("bad-source-install-{case}"));
        fs::create_dir(&alt).unwrap();
        fs::create_dir(alt.join("Data")).unwrap();
        fs::copy(install.join("FalloutNV.exe"), alt.join("FalloutNV.exe")).unwrap();
        let mut base = container(0x300);
        let mut placed = placement(0x400);
        let mut scpt = script();
        let mut flags = 0;
        let mut base_kind = b"CONT";
        let mut script_kind = b"SCPT";
        match case {
            0 => base = field(b"EDID", b"box\0"),
            1 => base = container(0),
            2 => base.extend(field(b"SCRI", &0x300u32.to_le_bytes())),
            3 => base = container(0x999),
            4 => flags = plugin::DELETED,
            5 => script_kind = b"ACTI",
            6 => scpt.extend(script()),
            7 => scpt = field(b"EDID", b"missing\0"),
            8 => base_kind = b"ACTI",
            9 => base = field(b"SCRI", &[1, 2, 3]),
            10 => placed = placement(0),
            11 => placed = placement(0x999),
            12 => placed.extend(field(b"NAME", &0x400u32.to_le_bytes())),
            13 => placed = field(b"DATA", &[0; 24]),
            _ => unreachable!(),
        }
        write(
            &alt.join("Data"),
            base,
            placed,
            scpt,
            flags,
            base_kind,
            script_kind,
        );
        let source_before = fs::read(alt.join("Data/FalloutNV.esm")).unwrap();
        let (output, report, result) = run_on(
            &format!("bad-source-{case}"),
            &alt,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            None,
            None,
            &[],
        );
        assert!(!output.status.success(), "bad source {case}");
        assert!(!report.exists());
        assert!(!result.exists());
        assert_eq!(
            fs::read(alt.join("Data/FalloutNV.esm")).unwrap(),
            source_before
        );
    }
    let protected = install.join("Data/blocked.snapshot.json");
    let (output, report, result) = run_on(
        "protected-result",
        &install,
        &input,
        &serde_json::to_vec(&request).unwrap(),
        Some(&protected),
        None,
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!result.exists());
    let protected = install.join("Data/blocked.report.json");
    let (output, report, result) = run_on(
        "protected-report",
        &install,
        &input,
        &serde_json::to_vec(&request).unwrap(),
        None,
        Some(&protected),
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert!(!result.exists());
    let existing = evidence.join("existing.json");
    fs::write(&existing, b"preserved").unwrap();
    let (output, report, _) = run_on(
        "existing-result",
        &install,
        &input,
        &serde_json::to_vec(&request).unwrap(),
        Some(&existing),
        None,
        &[],
    );
    assert!(!output.status.success());
    assert!(!report.exists());
    assert_eq!(fs::read(&existing).unwrap(), b"preserved");
    let (output, _, result) = run_on(
        "existing-report",
        &install,
        &input,
        &serde_json::to_vec(&request).unwrap(),
        None,
        Some(&existing),
        &[],
    );
    assert!(!output.status.success());
    assert!(!result.exists());
    assert_eq!(fs::read(&existing).unwrap(), b"preserved");
    let alias = evidence.join("alias.json");
    let (output, _, result) = run_on(
        "same-output-path",
        &install,
        &input,
        &serde_json::to_vec(&request).unwrap(),
        Some(&alias),
        Some(&alias),
        &[],
    );
    assert!(!output.status.success());
    assert!(!result.exists());
    if cfg!(windows) {
        let upper = evidence.join("ALIAS.json");
        let (output, _, result) = run_on(
            "case-output-alias",
            &install,
            &input,
            &serde_json::to_vec(&request).unwrap(),
            Some(&alias),
            Some(&upper),
            &[],
        );
        assert!(!output.status.success());
        assert!(!result.exists());
    }
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source_before
    );
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), exe_before);
    fs::write(evidence.join("cli-reference-boot-summary.json"),serde_json::to_vec_pretty(&json!({"cases":calls.get(),"literal_name_offset":13,"literal_scri_offset":10,"literal_script_header_offset":15,"literal_owner_reference":1,"large_reference_id":large,"whole_snapshot_equal":true,"cold_restore_verified":true,"unrelated_inventory_pose_pending_owner_preserved":true,"source_files_unchanged":true,"original_launched":false,"retail_parity_accepted":false})).unwrap()).unwrap();
}
