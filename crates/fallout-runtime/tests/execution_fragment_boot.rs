mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Handle, OwnerKind},
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
    events::{Clocks, Context, Trigger},
    execution::{attachment_boot, copy_probe::Initializer, fragment_boot as boot},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::{Local, ScriptInstance, Snapshot},
};
use std::{fs, sync::Arc};

const ACTIVATION: u64 = 9_007_199_254_740_993;
fn live(n: u64) -> ReferenceId {
    ReferenceId(n.try_into().unwrap())
}
fn fixture() -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let first = unit(&[(42, 0), (90, 1), (92, 0)], &[(b"SCRV", 90)]);
    assert_eq!(first.len(), 191);
    fixture_with_first(first)
}
fn fixture_with_first(first: Vec<u8>) -> (tempfile::TempDir, Arc<Catalogue>, Content) {
    let directory = tempfile::tempdir().unwrap();
    let second = unit(&[(43, 0), (90, 1), (93, 0)], &[(b"SCRV", 90)]);
    assert_eq!(second.len(), 191);
    let info = [first.clone(), field(b"NEXT", &[]), second].concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &unit(&[(1, 0)], &[])),
            record(b"INFO", 0x600, 0, &info),
            record(b"PACK", 0x620, 0, &first),
            record(b"QUST", 0x510, 0, &[]),
            record(b"REFR", 0x500, 0, &field(b"NAME", &0x777_u32.to_le_bytes())),
            record(b"MISC", 0x777, 0, &[]),
            record(b"CELL", 0x779, 0, &[]),
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
fn handle(catalogue: &Catalogue, record_id: u32, offset: u32) -> Handle {
    catalogue
        .record_scripts(&form(record_id))
        .find(|s| s.handle().key.header_decoded_offset == offset)
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
        .map(|(i, s)| Operator {
            code: i as u32,
            precedence: i as u8,
            spelling: s.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Signatures::new(),
        Default::default(),
    )
    .unwrap()
}
fn seed(catalogue: Arc<Catalogue>) -> World<'static> {
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Default::default(),
        CampaignId::from_bytes([83; 16]).unwrap(),
    )
    .unwrap();
    let a = world.register_reference(Some(form(0x500))).unwrap();
    assert_eq!(a, live(1));
    let b = world.register_reference(None).unwrap();
    assert_eq!(b, live(2));
    world.initialize_inventory(a).unwrap();
    let mut facts = fallout_runtime::inventory::Facts::unknown(form(0x777));
    facts.condition = Some(fallout_runtime::inventory::Condition::Float32 { bits: 0x7fc1_2345 });
    facts
        .extra_fields
        .push(fallout_runtime::inventory::OpaqueExtra {
            tag: *b"TEST",
            bytes: vec![0, 255, 1],
        });
    world.add_item(a, facts, 7.try_into().unwrap()).unwrap();
    let view = world.reference_view(a).unwrap();
    let pose = fallout_runtime::reference_state::Pose::from_source(
        &fallout_data::world::Transform {
            position: [-0.0, 1.0, f32::from_bits(1)],
            rotation: [0.0; 3],
        },
        None,
    )
    .unwrap();
    let stage = world
        .stage_reference_state(
            &view,
            fallout_runtime::reference_state::State::new(form(0x779), pose, false).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    let standalone = handle(&catalogue, 0x300, 0);
    let quest = world
        .create_instance(
            &standalone,
            Owner::Quest { key: form(0x510) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            quest,
            &[(
                1,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9012,
                },
            )],
        )
        .unwrap();
    let placed = world
        .create_instance(
            &standalone,
            Owner::Placed { reference: a },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            placed,
            &[(
                1,
                Value::Number {
                    bits: 0x8000_0000_0000_0000,
                },
            )],
        )
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 17,
            game_nanoseconds: 123,
            menu_nanoseconds: 7,
            real_nanoseconds: 999,
        })
        .unwrap();
    world
        .enqueue(
            quest,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    world
        .enqueue(
            placed,
            Trigger::ObjectEvent { mask: 0x8000_0001 },
            Context::default(),
        )
        .unwrap();
    world
}
fn request(world: &World<'_>, source_index: u32) -> boot::Request {
    boot::Request {
        campaign: world.campaign(),
        context: Context {
            calling_reference: Some(live(1)),
            containing_reference: Some(live(2)),
            target: Some(ReferenceValue::Content { key: form(0x777) }),
            arguments: vec![ReferenceValue::Live { id: live(1) }, ReferenceValue::Null],
        },
        initializers: vec![
            Initializer {
                index: source_index,
                value: Value::Number {
                    bits: 0x7ff8_1234_5678_9012,
                },
            },
            Initializer {
                index: 90,
                value: Value::Reference {
                    value: ReferenceValue::Live { id: live(2) },
                },
            },
        ],
    }
}
fn selection(definition: &Handle) -> boot::Selection<'_> {
    boot::Selection {
        definition,
        activation: ACTIVATION.try_into().unwrap(),
        intent: boot::Intent::Engineering,
    }
}
fn ready<'p, 's>(
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    definition: &'p Handle,
    request: &'p boot::Request,
    limits: boot::Limits,
) -> boot::BootPlan<'p, 's> {
    match boot::prepare(sources, content, selection(definition), request, limits).unwrap() {
        boot::Preparation::Ready(plan) => plan,
        boot::Preparation::Unsupported { reason, detail } => panic!("{reason:?}: {detail}"),
    }
}
fn expected(
    before: &Snapshot,
    definition: &Handle,
    request: &boot::Request,
    unused: u32,
) -> Snapshot {
    let mut after = before.clone();
    after.state_revision += 2;
    after.next_instance += 1;
    let mut locals = request
        .initializers
        .iter()
        .map(|init| Local {
            index: init.index,
            value: init.value.clone(),
        })
        .collect::<Vec<_>>();
    locals.push(Local {
        index: unused,
        value: Value::Uninitialized,
    });
    locals.sort_by_key(|local| local.index);
    after.instances.push(ScriptInstance {
        id: fallout_runtime::identity::InstanceId(before.next_instance.try_into().unwrap()),
        definition: definition.clone(),
        owner: Owner::Fragment {
            activation: ACTIVATION.try_into().unwrap(),
        },
        context: request.context.clone(),
        locals,
    });
    after
}

#[test]
fn exact_two_embedded_units_and_unknown_owner_keep_physical_identity_and_full_cold_state() {
    let (directory, catalogue, content) = fixture();
    let prepared = sources(&catalogue);
    let world = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let raw = fs::read(directory.path().join("FalloutNV.esm")).unwrap();
    for (record, offset, index, unused, owner, verified) in [
        (0x600, 0, 42, 92, OwnerKind::DialogueBegin, true),
        (0x600, 197, 43, 93, OwnerKind::DialogueEnd, true),
        (0x620, 0, 42, 92, OwnerKind::UnverifiedEmbedded, false),
    ] {
        let definition = handle(&catalogue, record, offset);
        let request = request(&world, index);
        let plan = ready(
            &prepared,
            &content,
            &definition,
            &request,
            Default::default(),
        );
        assert_eq!(plan.definition(), &definition);
        assert_eq!(plan.definition().key.header_decoded_offset, offset);
        assert_eq!(plan.script_owner().kind, owner);
        assert_eq!(plan.script_owner().schema_ownership_verified, verified);
        assert_eq!(plan.counts().input.initializers, 2);
        assert_eq!(plan.counts().input.context_arguments, 2);
        assert_eq!(plan.counts().input.variable_bytes, 103);
        assert_eq!(plan.counts().input.source_receipt_bytes, 85);
        assert_eq!(plan.counts().input.declarations, 3);
        assert_eq!(plan.counts().retained_variable_bytes, 2694);
        if record == 0x600 {
            assert_eq!(plan.script_version().record_file_offset, 156);
        }
        if offset == 197 {
            // The EndScript section marker is its SCHR header at197.
            // The preceding empty NEXT subrecord occupies191..197.
            assert_eq!(plan.script_owner().section_marker, Some(197));
        }
        let result = plan.apply(before.clone(), Default::default()).unwrap();
        let target = expected(&before, &definition, &request, unused);
        assert_eq!(
            result.instance,
            fallout_runtime::identity::InstanceId(before.next_instance.try_into().unwrap())
        );
        assert_eq!(result.snapshot, target);
        assert_eq!(
            World::restore(
                Arc::clone(&catalogue),
                Snapshot::decode(
                    &target.encode(64 * 1024 * 1024).unwrap(),
                    Default::default()
                )
                .unwrap(),
                Default::default()
            )
            .unwrap()
            .snapshot(),
            target
        );
        assert_eq!(world.snapshot(), before);
        assert_eq!(
            fs::read(directory.path().join("FalloutNV.esm")).unwrap(),
            raw
        );
    }
}

#[test]
fn all_six_runtime_caps_admit_exact_values_and_refuse_under_before_result() {
    let (_, catalogue, content) = fixture();
    let prepared = sources(&catalogue);
    let world = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let definition = handle(&catalogue, 0x600, 0);
    let request = request(&world, 42);
    let exact = boot::Limits {
        input: attachment_boot::Limits {
            maximum_initializers: 2,
            maximum_context_arguments: 2,
            maximum_variable_bytes: 103,
            maximum_source_receipt_bytes: 85,
            maximum_declarations: 3,
        },
        maximum_retained_variable_bytes: 2694,
    };
    assert_eq!(
        ready(&prepared, &content, &definition, &request, exact)
            .apply(before.clone(), Default::default())
            .unwrap()
            .snapshot,
        expected(&before, &definition, &request, 92)
    );
    for n in 0..6 {
        let mut low = exact;
        match n {
            0 => low.input.maximum_initializers -= 1,
            1 => low.input.maximum_context_arguments -= 1,
            2 => low.input.maximum_variable_bytes -= 1,
            3 => low.input.maximum_source_receipt_bytes -= 1,
            4 => low.input.maximum_declarations -= 1,
            _ => low.maximum_retained_variable_bytes -= 1,
        };
        assert!(
            boot::prepare(&prepared, &content, selection(&definition), &request, low).is_err(),
            "cap{n}"
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(matches!(
        boot::prepare(
            &prepared,
            &content,
            boot::Selection {
                intent: boot::Intent::Faithful,
                ..selection(&definition)
            },
            &request,
            boot::Limits {
                maximum_retained_variable_bytes: 0,
                ..exact
            }
        )
        .unwrap(),
        boot::Preparation::Unsupported { .. }
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn standalone_wrong_header_version_source_and_duplicate_owner_never_create_subset() {
    let (_, catalogue, content) = fixture();
    let prepared = sources(&catalogue);
    let world = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let definition = handle(&catalogue, 0x600, 0);
    let request = request(&world, 42);
    let standalone = handle(&catalogue, 0x300, 0);
    assert!(
        boot::prepare(
            &prepared,
            &content,
            selection(&standalone),
            &request,
            Default::default()
        )
        .is_err()
    );
    for n in 0..3 {
        let mut wrong = definition.clone();
        match n {
            0 => wrong.key.header_decoded_offset = 1,
            1 => wrong.version_sha256 = "0".repeat(64),
            _ => wrong.key.record = form(0x999),
        };
        assert!(
            boot::prepare(
                &prepared,
                &content,
                selection(&wrong),
                &request,
                Default::default()
            )
            .is_err()
        );
    }
    let plan = ready(
        &prepared,
        &content,
        &definition,
        &request,
        Default::default(),
    );
    let result = plan.apply(before.clone(), Default::default()).unwrap();
    assert!(
        plan.apply(result.snapshot.clone(), Default::default())
            .is_err()
    );
    let mut campaign = before.clone();
    campaign.campaign = CampaignId::from_bytes([84; 16]).unwrap();
    assert!(plan.apply(campaign, Default::default()).is_err());
    let mut revision = before.clone();
    revision.state_revision = u64::MAX;
    assert!(plan.apply(revision, Default::default()).is_err());
    let mut allocator = before.clone();
    allocator.next_instance = u64::MAX;
    assert!(plan.apply(allocator, Default::default()).is_err());
    for limits in [
        fallout_runtime::Limits {
            max_instances: 2,
            ..Default::default()
        },
        fallout_runtime::Limits {
            max_locals: 4,
            ..Default::default()
        },
        fallout_runtime::Limits {
            max_event_blocks: 1,
            ..Default::default()
        },
    ] {
        assert!(plan.apply(before.clone(), limits).is_err());
    }
    let mut invalid = before.clone();
    invalid.catalogue_sha256 = "0".repeat(64);
    assert!(plan.apply(invalid, Default::default()).is_err());
    assert_eq!(world.snapshot(), before);
}

#[test]
fn late_initializer_and_context_validation_discards_created_private_world() {
    let (_, catalogue, content) = fixture();
    let prepared = sources(&catalogue);
    let world = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let definition = handle(&catalogue, 0x600, 0);
    for n in 0..5 {
        let mut request = request(&world, 42);
        match n {
            0 => request.initializers.push(Initializer {
                index: 999,
                value: Value::Number { bits: 0 },
            }),
            1 => request.initializers[1].value = Value::Number { bits: 0 },
            2 => request.context.calling_reference = Some(live(99)),
            3 => {
                request.initializers[1].value = Value::Reference {
                    value: ReferenceValue::Live { id: live(99) },
                }
            }
            _ => request.context.target = Some(ReferenceValue::Live { id: live(99) }),
        };
        let plan = ready(
            &prepared,
            &content,
            &definition,
            &request,
            Default::default(),
        );
        assert!(
            plan.apply(before.clone(), Default::default()).is_err(),
            "late{n}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let mut duplicate = request(&world, 42);
    duplicate.initializers.push(Initializer {
        index: 42,
        value: Value::Number { bits: 0 },
    });
    assert!(
        boot::prepare(
            &prepared,
            &content,
            selection(&definition),
            &duplicate,
            Default::default()
        )
        .is_err()
    );
    let wrong_second = handle(&catalogue, 0x600, 197);
    let mismatched = request(&world, 42);
    let plan = ready(
        &prepared,
        &content,
        &wrong_second,
        &mismatched,
        Default::default(),
    );
    assert!(plan.apply(before.clone(), Default::default()).is_err());
    assert_eq!(world.snapshot(), before);
}

fn cli_run(
    cli: &std::path::Path,
    install: &std::path::Path,
    order: &std::path::Path,
    work: &std::path::Path,
    inputs: (&[u8], &[u8]),
    mode: &str,
    extra: &[&str],
) -> (std::process::Output, std::path::PathBuf, std::path::PathBuf) {
    fs::create_dir(work).unwrap();
    let input = work.join("input.snapshot.json");
    let req = work.join("request.json");
    fs::write(&input, inputs.0).unwrap();
    fs::write(&req, inputs.1).unwrap();
    let result = match mode {
        "result-input" => input.clone(),
        "result-protected" => install.join("protected-result.json"),
        _ => work.join("result.snapshot.json"),
    };
    let report = match mode {
        "input" => input.clone(),
        "request" => req.clone(),
        "order" => order.to_path_buf(),
        "result" => result.clone(),
        "result-case" => work.join("RESULT.SNAPSHOT.JSON"),
        "protected" => install.join("protected-report.json"),
        _ => work.join("report.json"),
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
    let source_bytes = fs::read(install.join("Data/FalloutNV.esm")).unwrap();
    let exe_bytes = fs::read(install.join("FalloutNV.exe")).unwrap();
    let order_bytes = fs::read(order).unwrap();
    let mut command = std::process::Command::new(cli);
    command
        .args(["event-operands", "--install"])
        .arg(install)
        .arg("--load-order")
        .arg(order);
    command
        .arg(if mode == "reference-literal" {
            "--snapshot-reference-literal-request"
        } else {
            "--fragment-boot-request"
        })
        .arg(&req);
    if mode != "missing-input" {
        command.arg("--snapshot-input").arg(&input);
    }
    if mode != "missing-output" {
        command.arg("--snapshot-output").arg(&result);
    }
    if mode != "stdout" {
        command.arg("--output").arg(&report);
    }
    let output = command.args(extra).output().unwrap();
    fs::write(work.join("stdout.txt"), &output.stdout).unwrap();
    fs::write(work.join("stderr.txt"), &output.stderr).unwrap();
    fs::write(work.join("process.json"),serde_json::to_vec_pretty(&serde_json::json!({"success":output.status.success(),"exit_code":output.status.code(),"snapshot_exists":result.exists(),"report_exists":report.exists(),"original_launched":false})).unwrap()).unwrap();
    assert_eq!(fs::read(input).unwrap(), inputs.0);
    assert_eq!(fs::read(req).unwrap(), inputs.1);
    assert_eq!(fs::read(order).unwrap(), order_bytes);
    assert_eq!(
        fs::read(install.join("Data/FalloutNV.esm")).unwrap(),
        source_bytes
    );
    assert_eq!(fs::read(install.join("FalloutNV.exe")).unwrap(), exe_bytes);
    if mode == "existing" {
        assert_eq!(fs::read(&report).unwrap(), b"Existing report");
    }
    if mode == "result-existing" {
        assert_eq!(fs::read(&result).unwrap(), b"Existing result");
    }
    (output, report, result)
}

#[test]
#[ignore = "fresh frozen package producer and authored metadata; Original never launched"]
fn cli_saved_fragment_boot_helper() {
    use serde_json::{Value as Json, json};
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    let cli = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI"));
    let metadata = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_FRAGMENT_BOOT_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let executable = metadata.join("authored-source-copy/FalloutNV.exe");
    let exe_bytes = fs::read(&executable).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&exe_bytes)),
        "3a87f92f011e5dc9179ddf733cf08be2b39ea6e5b7a8a9e3a9a72dafcc1b104d"
    );
    let producer_bytes = fs::read(&cli).unwrap();
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
    let (directory, catalogue, _) = fixture();
    // Candidate reads include the ten-byte REFR NAME payload even though that
    // record has no embedded unit:90 SCPT +388 INFO +191 PACK +10 REFR.
    assert_eq!(catalogue.counts.payload_bytes_scanned, 679);
    let install = install_from("authored-source-copy", directory.path());
    assert_eq!(
        fs::metadata(install.join("Data/FalloutNV.esm"))
            .unwrap()
            .len(),
        889
    );
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let world = seed(Arc::clone(&catalogue));
    let before = world.snapshot();
    let definition = handle(&catalogue, 0x600, 0);
    let initialization = request(&world, 42);
    let request = json!({"schema_version":1,"definition":definition,"activation":ACTIVATION,"intent":"engineering_fragment_activation","initialization":initialization,
        "source_limits":{"maximum_sources":256,"maximum_source_bytes":17179869184u64,"maximum_header_visits":8000000,"maximum_catalogue_scripts":262144,"maximum_variable_bytes":1048576,"maximum_record_bytes":8388608,"maximum_read_bytes":25165824,"maximum_field_visits":65536},
        "initialization_limits":{"maximum_initializers":128,"maximum_context_arguments":64,"maximum_variable_bytes":65536,"maximum_source_receipt_bytes":1048576,"maximum_declarations":65536},
        "maximum_retained_variable_bytes":2097152,"maximum_prepared_instructions":2000000,"maximum_prepared_operand_uses":1000000,"maximum_prepared_tokens":2000000,"maximum_prepared_record_bytes":536870912,
        "maximum_trace_bytes":2097152,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let calls = std::cell::Cell::new(0usize);
    let run_on =
        |name: &str, source: &Path, input: &Snapshot, raw: &[u8], mode: &str, extra: &[&str]| {
            calls.set(calls.get() + 1);
            cli_run(
                &cli,
                source,
                &order,
                &evidence.join(name),
                (&input.encode(64 * 1024 * 1024).unwrap(), raw),
                mode,
                extra,
            )
        };
    let run = |name: &str, input: &Snapshot, req: &Json| {
        run_on(
            name,
            &install,
            input,
            &serde_json::to_vec(req).unwrap(),
            "new",
            &[],
        )
    };
    let refused = |name: &str, input: &Snapshot, req: &Json| {
        let (output, report, result) = run(name, input, req);
        assert!(!output.status.success(), "{name}: succeeded");
        assert!(
            !report.exists() && !result.exists(),
            "{name}: output exists"
        );
    };
    let (output, path, result) = run("base-01", &before, &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result_bytes = fs::read(&result).unwrap();
    let report_bytes = fs::read(&path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    let after = expected(&before, &definition, &initialization, 92);
    assert_eq!(
        Snapshot::decode(&result_bytes, Default::default()).unwrap(),
        after
    );
    assert_eq!(
        World::restore(
            Arc::clone(&catalogue),
            Snapshot::decode(&result_bytes, Default::default()).unwrap(),
            Default::default()
        )
        .unwrap()
        .snapshot(),
        after
    );
    assert_eq!(report["fragment_boot"]["status"], "engineering_booted");
    // TES4 is the plugin header, outside the seven indexed/winning forms.
    assert_eq!(
        report["trace"]["source_preflight_counts"]["header_visits"],
        14
    );
    assert_eq!(report["trace"]["definition"], json!(definition));
    assert_eq!(
        report["trace"]["physical_source_owner"]["kind"],
        "dialogue_begin"
    );
    assert_eq!(
        report["trace"]["initialization_counts"]["retained_variable_bytes"],
        2694
    );
    assert_eq!(report["event_enqueued"], false);
    assert_eq!(report["reference_created"], false);
    assert_eq!(report["faithful_execution_admitted"], false);
    assert_eq!(report["retail_parity_accepted"], false);
    refused("duplicate-activation", &after, &request);
    let caps = [
        ("/source_limits/maximum_sources", 1u64),
        ("/source_limits/maximum_source_bytes", 889),
        ("/source_limits/maximum_header_visits", 14),
        ("/source_limits/maximum_catalogue_scripts", 4),
        ("/source_limits/maximum_variable_bytes", 90),
        ("/source_limits/maximum_record_bytes", 388),
        ("/source_limits/maximum_read_bytes", 679),
        ("/source_limits/maximum_field_visits", 10),
        ("/initialization_limits/maximum_initializers", 2),
        ("/initialization_limits/maximum_context_arguments", 2),
        ("/initialization_limits/maximum_variable_bytes", 103),
        ("/initialization_limits/maximum_source_receipt_bytes", 85),
        ("/initialization_limits/maximum_declarations", 3),
        ("/maximum_retained_variable_bytes", 2694),
        ("/maximum_prepared_instructions", 2),
        ("/maximum_prepared_operand_uses", 0),
        ("/maximum_prepared_tokens", 0),
        ("/maximum_prepared_record_bytes", 388),
        (
            "/maximum_trace_bytes",
            serde_json::to_vec(&report["trace"]).unwrap().len() as u64,
        ),
        ("/maximum_result_snapshot_bytes", result_bytes.len() as u64),
        ("/maximum_report_bytes", report_bytes.len() as u64),
    ];
    let mut exact = request.clone();
    for (pointer, value) in caps {
        *exact.pointer_mut(pointer).unwrap() = json!(value);
    }
    let (output, _, result) = run("exact01", &before, &exact);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        after
    );
    for (n, (pointer, value)) in caps.into_iter().enumerate() {
        if value > 0 {
            let mut low = exact.clone();
            *low.pointer_mut(pointer).unwrap() = json!(value - 1);
            low["maximum_report_bytes"] = request["maximum_report_bytes"].clone();
            let name = if pointer == "/maximum_report_bytes" {
                low["maximum_report_bytes"] = json!(value - 1);
                "under20".to_owned()
            } else {
                format!("cap-under-{n}")
            };
            refused(&name, &before, &low);
        }
        let mut high = request.clone();
        *high.pointer_mut(pointer).unwrap() =
            json!(request.pointer(pointer).unwrap().as_u64().unwrap() + 1);
        refused(&format!("cap-ceiling-{n}"), &before, &high);
    }
    for key in request.as_object().unwrap().keys() {
        let mut missing = request.clone();
        missing.as_object_mut().unwrap().remove(key);
        refused(&format!("missing-{key}"), &before, &missing);
    }
    for parent in [
        "source_limits",
        "initialization_limits",
        "initialization",
        "definition",
    ] {
        for key in request[parent].as_object().unwrap().keys() {
            let mut missing = request.clone();
            missing[parent].as_object_mut().unwrap().remove(key);
            refused(&format!("missing-{parent}-{key}"), &before, &missing);
        }
    }
    for key in [
        "calling_reference",
        "containing_reference",
        "target",
        "arguments",
    ] {
        let mut missing = request.clone();
        missing["initialization"]["context"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        refused(&format!("missing-context-{key}"), &before, &missing);
    }
    for (name, pointer, value) in [
        ("zero-activation", "/activation", json!(0)),
        ("float-activation", "/activation", json!(1.0)),
        (
            "string-activation",
            "/activation",
            json!(ACTIVATION.to_string()),
        ),
        ("null-activation", "/activation", Json::Null),
        ("old-request", "/schema_version", json!(0)),
        ("generic-intent", "/intent", json!("engineering")),
        (
            "object-null-intent",
            "/intent",
            json!({"engineering_fragment_activation":null}),
        ),
        ("object-null-faithful", "/intent", json!({"faithful":null})),
        (
            "wrong-header",
            "/definition/key/header_decoded_offset",
            json!(1),
        ),
        (
            "wrong-version",
            "/definition/version_sha256",
            json!("0".repeat(64)),
        ),
        (
            "campaign",
            "/initialization/campaign",
            json!(vec![84_u8; 16]),
        ),
        (
            "late-context",
            "/initialization/context/calling_reference",
            json!(99),
        ),
        (
            "late-reference",
            "/initialization/initializers/1/value",
            json!({"kind":"reference","value":{"kind":"live","id":99}}),
        ),
        (
            "late-kind",
            "/initialization/initializers/1/value",
            json!({"kind":"number","bits":0}),
        ),
    ] {
        let mut changed = request.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        refused(name, &before, &changed);
    }
    for (n, parent) in [
        "",
        "/source_limits",
        "/initialization_limits",
        "/initialization",
        "/initialization/context",
        "/definition",
        "/definition/key",
        "/definition/key/record",
        "/initialization/initializers/0",
        "/initialization/initializers/0/value",
    ]
    .into_iter()
    .enumerate()
    {
        let mut changed = request.clone();
        changed
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        refused(&format!("unknown-{n}"), &before, &changed);
    }
    let mut late = request.clone();
    late["initialization"]["initializers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"index":999,"value":{"kind":"number","bits":0}}));
    refused("late-initializer", &before, &late);
    let mut duplicate = request.clone();
    duplicate["initialization"]["initializers"]
        .as_array_mut()
        .unwrap()
        .push(request["initialization"]["initializers"][0].clone());
    refused("duplicate-initializer", &before, &duplicate);
    let mut standalone = request.clone();
    standalone["definition"] = json!(handle(&catalogue, 0x300, 0));
    refused("standalone", &before, &standalone);
    for (name, raw) in [
        ("raw-malformed", b"{".to_vec()),
        (
            "raw-duplicate",
            [
                b"{\"schema_version\":1,".as_slice(),
                &serde_json::to_vec(&request).unwrap()[1..],
            ]
            .concat(),
        ),
        ("raw-oversize", vec![b' '; 512 * 1024 + 1]),
    ] {
        let (output, path, result) = run_on(name, &install, &before, &raw, "new", &[]);
        assert!(!output.status.success() && !path.exists() && !result.exists());
    }
    for (n, mut input) in [
        before.clone(),
        before.clone(),
        before.clone(),
        before.clone(),
        before.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        match n {
            0 => input.schema_version = 3,
            1 => input.catalogue_sha256 = "0".repeat(64),
            2 => input.state_revision = u64::MAX,
            3 => input.next_instance = u64::MAX,
            _ => input.references.retain(|r| r.id != live(1)),
        };
        refused(&format!("invalid-snapshot-{n}"), &input, &request);
    }
    let mut faithful = request.clone();
    faithful["intent"] = json!("faithful");
    faithful["maximum_retained_variable_bytes"] = json!(0);
    let (output, path, result) = run("faithful", &before, &faithful);
    assert!(!output.status.success() && !result.exists());
    let diagnostic: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        diagnostic["fragment_boot"]["reason"],
        "unverified_retail_semantics"
    );
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
        "missing-input",
        "missing-output",
    ] {
        let (output, _, result) = run_on(
            &format!("path-{mode}"),
            &install,
            &before,
            &serde_json::to_vec(&request).unwrap(),
            mode,
            &[],
        );
        assert!(!output.status.success(), "{mode}");
        if !matches!(mode, "result-input" | "result-existing") {
            assert!(!result.exists(), "{mode}");
        }
    }
    for (name, args) in [
        ("conflict-prepared", vec!["--prepared-sources"]),
        ("conflict-native", vec!["--native-capabilities"]),
        ("conflict-player", vec!["--player-id", "1"]),
        (
            "conflict-quest-set",
            vec!["--quest-boot-set-request", "missing.json"],
        ),
        (
            "conflict-reference-literal",
            vec!["--snapshot-reference-literal-request", "missing.json"],
        ),
        (
            "conflict-reference-boot",
            vec!["--reference-boot-request", "missing.json"],
        ),
    ] {
        let (output, path, result) = run_on(
            name,
            &install,
            &before,
            &serde_json::to_vec(&request).unwrap(),
            "new",
            &args,
        );
        assert!(!output.status.success() && !path.exists() && !result.exists());
    }
    let (output, _, result) = run_on(
        "stdout",
        &install,
        &before,
        &serde_json::to_vec(&request).unwrap(),
        "stdout",
        &[],
    );
    assert!(output.status.success());
    let stdout: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stdout["fragment_boot"]["status"], "engineering_booted");
    assert_eq!(
        Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap(),
        after
    );
    for (n, (record, offset, index, unused)) in [(0x600, 197, 43, 93), (0x620, 0, 42, 92)]
        .into_iter()
        .enumerate()
    {
        let def = handle(&catalogue, record, offset);
        let init = request_for(&world, index);
        let mut changed = request.clone();
        changed["definition"] = json!(def);
        changed["initialization"] = json!(init);
        let (output, path, result) = run(&format!("positive-unit-{n}"), &before, &changed);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        let target = expected(&before, &def, &init, unused);
        assert_eq!(actual, target);
        assert_eq!(
            World::restore(Arc::clone(&catalogue), actual, Default::default())
                .unwrap()
                .snapshot(),
            target
        );
        let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        if record == 0x620 {
            assert_eq!(
                report["trace"]["physical_source_owner"]["schema_ownership_verified"],
                false
            );
        }
    }
    for (n, activation) in [1u64, u64::MAX - 1, u64::MAX].into_iter().enumerate() {
        let mut changed = request.clone();
        changed["activation"] = json!(activation);
        let (output, _, result) = run(&format!("activation-{n}"), &before, &changed);
        assert!(output.status.success());
        let mut target = after.clone();
        target.instances.last_mut().unwrap().owner = Owner::Fragment {
            activation: activation.try_into().unwrap(),
        };
        let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
        assert_eq!(actual, target);
        assert_eq!(
            World::restore(Arc::clone(&catalogue), actual, Default::default())
                .unwrap()
                .snapshot(),
            target
        );
    }
    for (n, pointer) in [
        "/initialization/context/target",
        "/initialization/initializers/1/value/value",
    ]
    .into_iter()
    .enumerate()
    {
        let mut giant = request.clone();
        *giant.pointer_mut(pointer).unwrap() = json!({"kind":"content","key":{"profile":"nv-original","origin_plugin":"x".repeat(32768),"local_id":1911}});
        giant["initialization_limits"]["maximum_variable_bytes"] = json!(4096);
        refused(&format!("giant-input-{n}"), &before, &giant);
    }
    // Connected package consumer: boot an embedded unit containing f90 = Z1,
    // explicitly enqueue canonical events, then consume one saved source literal.
    let mut first = unit(
        &[(42, 0), (90, 1), (92, 0)],
        &[(b"SCRO", 0x777), (b"SCRV", 90)],
    );
    let compiled = [
        0x10, 0, 6, 0, 0, 0, 16, 0, 0, 0, 0x15, 0, 8, 0, b'f', 90, 0, 3, 0, b'Z', 1, 0, 0x11, 0, 0,
        0,
    ];
    let mut raw = first[..26].to_vec();
    raw[14..18].copy_from_slice(&26u32.to_le_bytes());
    raw.extend(field(b"SCDA", &compiled));
    raw.extend(&first[46..]);
    first = raw;
    assert_eq!(first.len(), 213);
    let (chain_dir, chain_cat, _) = fixture_with_first(first);
    let chain_install = install_from("connected-source-copy", chain_dir.path());
    let chain_world = seed(Arc::clone(&chain_cat));
    let mut chain_before = chain_world.snapshot();
    chain_before.pending_events.clear();
    let chain_def = handle(&chain_cat, 0x600, 0);
    let chain_init = request_for(&chain_world, 42);
    let mut chain_request = request.clone();
    chain_request["definition"] = json!(chain_def);
    chain_request["initialization"] = json!(chain_init);
    let (output, path, result) = run_on(
        "connected-boot",
        &chain_install,
        &chain_before,
        &serde_json::to_vec(&chain_request).unwrap(),
        "new",
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let booted = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
    assert_eq!(booted, expected(&chain_before, &chain_def, &chain_init, 92));
    let mut queued =
        World::restore(Arc::clone(&chain_cat), booted.clone(), Default::default()).unwrap();
    let own = queued.handle(booted.instances.last().unwrap().id).unwrap();
    let sequence = queued
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            chain_init.context.clone(),
        )
        .unwrap();
    queued
        .enqueue(
            own,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            chain_init.context.clone(),
        )
        .unwrap();
    let queued_before = queued.snapshot();
    let literal = json!({"schema_version":1,"sequence":sequence,"owner":{"kind":"fragment","activation":ACTIVATION},"intent":"engineering_identity_assignment","explicit_player":null,
        "maximum_source_instructions":4096,"maximum_operand_uses":2,"maximum_statement_bytes":65539,"maximum_reference_variable_bytes":65536,"maximum_trace_source_bytes":1048576,
        "maximum_trace_rows":65536,"maximum_trace_variable_bytes":1048576,"maximum_trace_binding_uses":262144,"maximum_stage_variable_bytes":3145728,"maximum_trace_bytes":2097152,
        "maximum_prepared_instructions":2000000,"maximum_prepared_operand_uses":1000000,"maximum_prepared_tokens":2000000,"maximum_prepared_record_bytes":536870912,"maximum_result_snapshot_bytes":67108864,"maximum_report_bytes":8388608});
    let (output, _, result) = run_on(
        "connected-reference-literal",
        &chain_install,
        &queued_before,
        &serde_json::to_vec(&literal).unwrap(),
        "reference-literal",
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut target = queued_before.clone();
    target.state_revision += 1;
    target.pending_events.remove(0);
    target
        .instances
        .last_mut()
        .unwrap()
        .locals
        .iter_mut()
        .find(|l| l.index == 90)
        .unwrap()
        .value = Value::Reference {
        value: ReferenceValue::Content { key: form(0x777) },
    };
    let actual = Snapshot::decode(&fs::read(result).unwrap(), Default::default()).unwrap();
    assert_eq!(actual, target);
    assert_eq!(
        World::restore(Arc::clone(&chain_cat), actual, Default::default())
            .unwrap()
            .snapshot(),
        target
    );
    let (output, _, result) = run_on(
        "connected-stale-head",
        &chain_install,
        &target,
        &serde_json::to_vec(&literal).unwrap(),
        "reference-literal",
        &[],
    );
    assert!(!output.status.success() && !result.exists());
    let boot_report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        boot_report["trace"]["definition"]["key"]["header_decoded_offset"],
        0
    );
    fs::write(evidence.join("assertions.json"),serde_json::to_vec_pretty(&json!({"cases":calls.get(),"passed":true,"baseline_source_bytes":889,"embedded_header_offsets":[0,197],"activation":ACTIVATION,
        "whole_expected_and_cold_equal":true,"late_refusal_no_success_subset":true,"unrelated_owners_inventory_pose_pending_context_clocks_preserved":true,
        "connected_boot_enqueue_literal_cold_equal":true,"connected_events_explicitly_enqueued_by_canonical_api":true,"process_transaction_group_atomicity_claimed":false,
        "producer_sha256":format!("{:x}",Sha256::digest(&producer_bytes)),"original_launched":false,"retail_parity_accepted":false})).unwrap()).unwrap();
    assert_eq!(fs::read(cli).unwrap(), producer_bytes);
    assert_eq!(fs::read(executable).unwrap(), exe_bytes);
}
fn request_for(world: &World<'_>, source_index: u32) -> boot::Request {
    request(world, source_index)
}
