mod common;
use common::*;
use fallout_data::{
    condition_operands::{self, Parameter, PreparedRecord, RecordLimits, Signature, Signatures},
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    execution::condition::{Error, Intent, Observation, Outcome, Request, Unsupported},
    foreign::Content,
    identity::{Owner, ReferenceId},
    inventory::Facts,
    snapshot::Snapshot,
};
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn raw(length: usize, function: u16, item: u32, run_on: u32) -> Vec<u8> {
    let mut result = vec![0; length];
    result[8..10].copy_from_slice(&function.to_le_bytes());
    result[12..16].copy_from_slice(&item.to_le_bytes());
    if length >= 24 {
        result[20..24].copy_from_slice(&run_on.to_le_bytes());
    }
    result
}
fn signatures() -> Signatures {
    [(
        47,
        Signature {
            parameters: vec![Parameter {
                type_id: 50,
                optional_word: 0,
            }],
        },
    )]
    .into()
}
fn load_sources(path: &Path, signatures: &Signatures) -> (Catalogue, Content, PreparedRecord) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    let location = store.winner(&form(0x500)).unwrap();
    let record = condition_operands::prepare_record(
        &mut store,
        location,
        signatures,
        RecordLimits::default(),
    )
    .unwrap();
    (catalogue, content, record)
}
fn fixture(
    bytes: &[u8],
    signatures: &Signatures,
) -> (tempfile::TempDir, Catalogue, Content, PreparedRecord) {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let path = directory.path().join("FalloutNV.esm");
    let mut plugin = fs::read(&path).unwrap();
    plugin.extend(record(b"QUST", 0x500, 0, &field(b"CTDA", bytes)));
    plugin.extend(record(b"FLST", 0x400, 0, &[]));
    plugin.extend(record(b"MISC", 0x102, 0, &[]));
    fs::write(path, plugin).unwrap();
    let (catalogue, content, record) = load_sources(directory.path(), signatures);
    (directory, catalogue, content, record)
}
fn populated<'a>(catalogue: &'a Catalogue, lots: &[(u32, u32)]) -> (World<'a>, ReferenceId) {
    let mut world = World::new(catalogue, Limits::default()).unwrap();
    let subject = world.register_reference(None).unwrap();
    world.initialize_inventory(subject).unwrap();
    for &(item, quantity) in lots {
        world
            .add_item(
                subject,
                Facts::unknown(form(item)),
                quantity.try_into().unwrap(),
            )
            .unwrap();
    }
    let instance = world
        .create_instance(
            &definition(catalogue),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, subject)
}
fn reason(observation: &Observation<'_>, expected: Unsupported) {
    assert!(
        matches!(&observation.outcome, Outcome::Unsupported { reason, .. } if *reason == expected),
        "{observation:?}"
    );
    assert!(observation.condition_truth.is_none());
    assert!(!observation.condition_evaluation_ready && !observation.original_behavior_verified);
}
fn retain(
    name: &str,
    directory: &Path,
    world: &World<'_>,
    subject: ReferenceId,
    observation: &Observation<'_>,
) {
    let Some(root) = std::env::var_os("FALLOUT_CONDITION_QUERY_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&root);
    assert!(root.is_absolute() && root.is_dir());
    let case = root.join(name);
    fs::create_dir(&case).unwrap();
    let data = case.join("Data");
    fs::create_dir(&data).unwrap();
    fs::copy(directory.join("FalloutNV.esm"), data.join("FalloutNV.esm")).unwrap();
    fs::write(case.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        case.join("snapshot.json"),
        world
            .snapshot()
            .encode(Limits::default().max_snapshot_bytes)
            .unwrap(),
    )
    .unwrap();
    fs::write(case.join("request.json"), serde_json::to_vec_pretty(&json!({"record":form(0x500),"field_decoded_offset":0,"explicit_subject":subject,"snapshot":"snapshot.json"})).unwrap()).unwrap();
    fs::write(
        case.join("expected.json"),
        serde_json::to_vec_pretty(observation).unwrap(),
    )
    .unwrap();
}

#[test]
fn exact_host_counts_keep_raw_sources_and_never_change_state_or_truth() {
    let (directory, catalogue, content, record) = fixture(&raw(28, 47, 0x100, 0), &signatures());
    for (name, lots, expected) in [
        ("empty", vec![], 0_u64),
        ("small", vec![(0x100, 1), (0x100, 2)], 3),
        (
            "wide",
            vec![(0x100, u32::MAX), (0x100, u32::MAX)],
            8_589_934_590,
        ),
        ("other-item", vec![(0x100, 17), (0x102, 23)], 17),
    ] {
        let (world, subject) = populated(&catalogue, &lots);
        let before = world.snapshot();
        let request = Request::prepare(&world, &record, 0).unwrap();
        let observation = request
            .observe(
                &world,
                &content,
                Some(subject),
                Intent::EngineeringObservation,
                2,
            )
            .unwrap();
        let Outcome::EngineeringObservation { trace } = &observation.outcome else {
            panic!("{observation:?}")
        };
        assert_eq!(trace.query.result, expected);
        assert!(trace.original_numeric_return.is_none() && !trace.original_behavior_verified);
        assert_eq!(observation.site.raw_bytes(), raw(28, 47, 0x100, 0));
        assert!(observation.condition_truth.is_none() && !observation.condition_evaluation_ready);
        assert_eq!(world.snapshot(), before);
        assert_eq!(world.pending_events().len(), 1);
        retain(name, directory.path(), &world, subject, &observation);
    }
}

#[test]
fn faithful_and_unimplemented_functions_return_explicit_unsupported() {
    for (function, expected) in [
        (47, Unsupported::UnverifiedRetailSemantics),
        (0x7FFF, Unsupported::MissingImplementation),
    ] {
        let (_d, catalogue, content, record) = fixture(&raw(28, function, 0x100, 0), &signatures());
        let (world, subject) = populated(&catalogue, &[]);
        let before = world.snapshot();
        let request = Request::prepare(&world, &record, 0).unwrap();
        reason(
            &request
                .observe(&world, &content, Some(subject), Intent::Faithful, 0)
                .unwrap(),
            expected,
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn descriptor_and_unused_word_mismatches_do_not_route_a_query() {
    let raw = raw(28, 47, 0x100, 0);
    let mut tail = raw.clone();
    tail[16..20].copy_from_slice(&1_u32.to_le_bytes());
    let mut optional = signatures();
    optional.get_mut(&47).unwrap().parameters[0].optional_word = 1;
    let mut wrong = signatures();
    wrong.get_mut(&47).unwrap().parameters[0].type_id = 4;
    for (bytes, signatures) in [
        (raw.clone(), Signatures::new()),
        (raw.clone(), optional),
        (raw.clone(), wrong),
        (tail, signatures()),
    ] {
        let (_d, catalogue, content, record) = fixture(&bytes, &signatures);
        let (world, subject) = populated(&catalogue, &[]);
        let before = world.snapshot();
        let request = Request::prepare(&world, &record, 0).unwrap();
        reason(
            &request
                .observe(
                    &world,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    0,
                )
                .unwrap(),
            Unsupported::Signature,
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn missing_deleted_null_and_runtime_arguments_stay_unresolved() {
    for item in [0x777, 0x200, 0, 0x14] {
        let (_d, catalogue, content, record) = fixture(&raw(28, 47, item, 0), &signatures());
        let (world, subject) = populated(&catalogue, &[]);
        let before = world.snapshot();
        let request = Request::prepare(&world, &record, 0).unwrap();
        reason(
            &request
                .observe(
                    &world,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    0,
                )
                .unwrap(),
            Unsupported::Argument,
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn absent_target_and_reference_subjects_are_not_invented() {
    for bytes in [
        raw(20, 47, 0x100, 0),
        raw(28, 47, 0x100, 1),
        raw(28, 47, 0x100, 2),
        raw(28, 47, 0x100, 3),
        raw(28, 47, 0x100, 4),
    ] {
        let (_d, catalogue, content, record) = fixture(&bytes, &signatures());
        let (world, subject) = populated(&catalogue, &[]);
        let before = world.snapshot();
        let request = Request::prepare(&world, &record, 0).unwrap();
        reason(
            &request
                .observe(
                    &world,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    0,
                )
                .unwrap(),
            Unsupported::SubjectSelection,
        );
        assert_eq!(world.snapshot(), before);
    }
}

#[test]
fn source_findings_and_missing_subjects_cannot_be_successful_zeroes() {
    let mut padding = raw(28, 47, 0x100, 0);
    padding[1] = 1;
    let mut nonfinite = raw(28, 47, 0x100, 0);
    nonfinite[4..8].copy_from_slice(&0x7FC00001_u32.to_le_bytes());
    for bytes in [padding, nonfinite, raw(28, 47, 0x100, 99)] {
        let (_d, catalogue, content, record) = fixture(&bytes, &signatures());
        let (world, subject) = populated(&catalogue, &[]);
        let request = Request::prepare(&world, &record, 0).unwrap();
        reason(
            &request
                .observe(
                    &world,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    0,
                )
                .unwrap(),
            Unsupported::SourceFinding,
        );
    }
    let (_d, catalogue, content, record) = fixture(&raw(24, 47, 0x100, 0), &signatures());
    let (world, _) = populated(&catalogue, &[]);
    let request = Request::prepare(&world, &record, 0).unwrap();
    reason(
        &request
            .observe(&world, &content, None, Intent::EngineeringObservation, 0)
            .unwrap(),
        Unsupported::MissingSubject,
    );
    reason(
        &request
            .observe(
                &world,
                &content,
                Some(ReferenceId(999.try_into().unwrap())),
                Intent::EngineeringObservation,
                0,
            )
            .unwrap(),
        Unsupported::HostQueryUnavailable,
    );
}

#[test]
fn form_lists_and_contribution_capacity_are_explicit_refusals() {
    let (_d, catalogue, content, record) = fixture(&raw(28, 47, 0x400, 0), &signatures());
    let (world, subject) = populated(&catalogue, &[]);
    let request = Request::prepare(&world, &record, 0).unwrap();
    reason(
        &request
            .observe(
                &world,
                &content,
                Some(subject),
                Intent::EngineeringObservation,
                0,
            )
            .unwrap(),
        Unsupported::UnverifiedFormList,
    );
    let (_d, catalogue, content, record) = fixture(&raw(28, 47, 0x100, 0), &signatures());
    let (world, subject) = populated(&catalogue, &[(0x100, 1), (0x100, 2)]);
    let before = world.snapshot();
    let request = Request::prepare(&world, &record, 0).unwrap();
    for limit in [0, 1] {
        assert!(matches!(
            request.observe(
                &world,
                &content,
                Some(subject),
                Intent::EngineeringObservation,
                limit
            ),
            Err(Error::Capacity)
        ));
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
fn complete_source_campaign_content_and_site_identity_are_checked() {
    let (_d, catalogue, content, record) = fixture(&raw(28, 47, 0x100, 0), &signatures());
    let (world, subject) = populated(&catalogue, &[]);
    assert!(matches!(
        Request::prepare(&world, &record, 1),
        Err(Error::MissingSite(1))
    ));
    let request = Request::prepare(&world, &record, 0).unwrap();
    let (other, _) = populated(&catalogue, &[]);
    assert!(matches!(
        request.observe(
            &other,
            &content,
            Some(subject),
            Intent::EngineeringObservation,
            0
        ),
        Err(Error::ContextChanged)
    ));
    let (_d, changed, changed_content, changed_record) =
        fixture(&raw(28, 47, 0x102, 0), &signatures());
    assert!(matches!(
        Request::prepare(&world, &changed_record, 0),
        Err(Error::ContextChanged)
    ));
    assert!(matches!(
        request.observe(
            &world,
            &changed_content,
            Some(subject),
            Intent::EngineeringObservation,
            0
        ),
        Err(Error::Content(_))
    ));
    let changed_world = World::restore(&changed, world.snapshot(), Limits::default());
    assert!(changed_world.is_err());
}

#[test]
fn restored_requests_read_current_canonical_revision_and_exact_snapshot() {
    let (directory, catalogue, content, record) = fixture(&raw(28, 47, 0x100, 0), &signatures());
    let (mut world, subject) = populated(&catalogue, &[(0x100, 17)]);
    let request = Request::prepare(&world, &record, 0).unwrap();
    let item = world.inventory_items(subject).unwrap().next().unwrap().id();
    world
        .remove_item_quantity(item, 2.try_into().unwrap())
        .unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 2,
            menu_nanoseconds: 3,
            real_nanoseconds: 4,
        })
        .unwrap();
    let before = world.snapshot();
    let restored = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    let a = request
        .observe(
            &world,
            &content,
            Some(subject),
            Intent::EngineeringObservation,
            1,
        )
        .unwrap();
    let b = request
        .observe(
            &restored,
            &content,
            Some(subject),
            Intent::EngineeringObservation,
            1,
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
    let Outcome::EngineeringObservation { trace } = &a.outcome else {
        panic!("{a:?}")
    };
    assert_eq!(trace.query.result, 15);
    assert_eq!(trace.query.boundary, world.clocks());
    assert_eq!(trace.query.state_revision, world.revision());
    assert_eq!(world.snapshot(), before);
    fs::write(
        directory.path().join("snapshot.json"),
        before.encode(Limits::default().max_snapshot_bytes).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("expected.json"),
        serde_json::to_vec(&a).unwrap(),
    )
    .unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_snapshot_helper", "--nocapture"])
        .env("FALLOUT_CONDITION_COLD_DIR", directory.path())
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}

#[test]
fn cold_snapshot_helper() {
    let Some(directory) = std::env::var_os("FALLOUT_CONDITION_COLD_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    let (catalogue, content, record) = load_sources(directory, &signatures());
    let snapshot = Snapshot::decode(
        &fs::read(directory.join("snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let world = World::restore(&catalogue, snapshot.clone(), Limits::default()).unwrap();
    let request = Request::prepare(&world, &record, 0).unwrap();
    let observation = request
        .observe(
            &world,
            &content,
            Some(ReferenceId(1.try_into().unwrap())),
            Intent::EngineeringObservation,
            1,
        )
        .unwrap();
    let expected: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("expected.json")).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(observation).unwrap(), expected);
    assert_eq!(world.snapshot(), snapshot);
}
