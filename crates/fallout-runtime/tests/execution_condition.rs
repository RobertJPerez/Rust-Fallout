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

fn batch_fixture(count: usize) -> (tempfile::TempDir, Catalogue, Content, PreparedRecord) {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let mut body = field(b"EDID", b"condition_batch\0");
    for index in 0..count {
        if index % 17 == 0 {
            body.extend(field(b"FULL", b"physical marker\0"));
        }
        let bytes = raw(28, 47, if index % 2 == 0 { 0x100 } else { 0x102 }, 0);
        if index % 7 == 0 {
            body.extend(field(b"XXXX", &28_u32.to_le_bytes()));
        }
        body.extend(field(b"CTDA", &bytes));
    }
    let path = directory.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"QUST", 0x500, 0, &body));
    bytes.extend(record(b"MISC", 0x102, 0, &[]));
    fs::write(path, bytes).unwrap();
    let (catalogue, content, record) = load_sources(directory.path(), &signatures());
    (directory, catalogue, content, record)
}

fn cross_fixture() -> (tempfile::TempDir, Catalogue, Content, [PreparedRecord; 2]) {
    let (directory, _, _, _) = batch_fixture(4);
    let path = directory.path().join("FalloutNV.esm");
    let mut bytes = fs::read(&path).unwrap();
    let body = [
        field(b"EDID", b"second\0"),
        field(b"CTDA", &raw(28, 47, 0x100, 0)),
        field(b"CTDA", &raw(28, 47, 0x102, 0)),
    ]
    .concat();
    bytes.extend(record(b"QUST", 0x501, 0, &body));
    fs::write(&path, bytes).unwrap();
    let (catalogue, content, first) = load_sources(directory.path(), &signatures());
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let location = store.winner(&form(0x501)).unwrap();
    let second =
        condition_operands::prepare_record(&mut store, location, &signatures(), Default::default())
            .unwrap();
    (directory, catalogue, content, [first, second])
}

#[test]
fn cross_record_selection_keeps_each_subject_source_and_order_with_one_cohort_validation() {
    use fallout_runtime::execution::condition::{CrossRequests, RecordSelection};
    let (_directory, catalogue, content, records) = cross_fixture();
    let (mut world, subject) = populated(&catalogue, &[(0x100, 7), (0x100, 11), (0x102, 23)]);
    let other = world.register_reference(None).unwrap();
    world.initialize_inventory(other).unwrap();
    world
        .add_item(other, Facts::unknown(form(0x100)), 31.try_into().unwrap())
        .unwrap();
    world
        .add_item(other, Facts::unknown(form(0x102)), 41.try_into().unwrap())
        .unwrap();
    let before = world.snapshot();
    assert_eq!(records[1].sites()[0].field_decoded_offset(), 13);
    assert_eq!(records[1].sites()[1].field_decoded_offset(), 47);
    let selections = [
        (1, 1, other),
        (0, 3, subject),
        (1, 0, subject),
        (0, 0, other),
        (1, 1, other),
    ]
    .map(|(record_index, site, subject)| RecordSelection {
        record_index,
        field_decoded_offset: records[record_index].sites()[site].field_decoded_offset(),
        explicit_subject: Some(subject),
    });
    let batch = CrossRequests::prepare(
        &world,
        &[&records[0], &records[1]],
        &selections,
        Default::default(),
    )
    .unwrap();
    assert_eq!(batch.counts().cohort_validations, 1);
    assert_eq!(batch.counts().records, 2);
    assert_eq!(batch.counts().requests, 5);
    assert_eq!(batch.counts().receipt_comparisons, 2);
    let observed = batch
        .observe(
            &world,
            &content,
            Intent::EngineeringObservation,
            Default::default(),
        )
        .unwrap();
    for ((selection, observation), literal) in selections
        .iter()
        .zip(&observed)
        .zip([41_u64, 23, 18, 31, 41])
    {
        let record = &records[selection.record_index];
        let single = Request::prepare(&world, record, selection.field_decoded_offset).unwrap();
        let expected = single
            .observe(
                &world,
                &content,
                selection.explicit_subject,
                Intent::EngineeringObservation,
                2,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(observation).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            observation.source.key,
            form(if selection.record_index == 0 {
                0x500
            } else {
                0x501
            })
        );
        let Outcome::EngineeringObservation { trace } = &observation.outcome else {
            panic!("{observation:?}")
        };
        assert_eq!(trace.query.result, literal);
        assert_eq!(trace.query.subject, selection.explicit_subject.unwrap());
        assert!(observation.condition_truth.is_none());
        assert!(!observation.condition_evaluation_ready && !observation.original_behavior_verified);
    }
    assert_eq!(world.snapshot(), before);
    let compact = serde_json::to_vec(&observed).unwrap();
    let exact = fallout_runtime::execution::condition::CrossObservationLimits {
        maximum_contributions: 6,
        maximum_observation_bytes: compact.len(),
    };
    assert_eq!(
        serde_json::to_vec(
            &batch
                .observe(&world, &content, Intent::EngineeringObservation, exact)
                .unwrap()
        )
        .unwrap(),
        compact
    );
    assert!(matches!(
        batch.observe(
            &world,
            &content,
            Intent::EngineeringObservation,
            fallout_runtime::execution::condition::CrossObservationLimits {
                maximum_contributions: 5,
                ..exact
            }
        ),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        batch.observe(
            &world,
            &content,
            Intent::EngineeringObservation,
            fallout_runtime::execution::condition::CrossObservationLimits {
                maximum_observation_bytes: compact.len() - 1,
                ..exact
            }
        ),
        Err(Error::BatchCapacity("observation bytes"))
    ));
    assert_eq!(world.snapshot(), before);
    let restored = World::restore(&catalogue, before.clone(), Default::default()).unwrap();
    assert_eq!(
        serde_json::to_vec(
            &batch
                .observe(&restored, &content, Intent::EngineeringObservation, exact)
                .unwrap()
        )
        .unwrap(),
        compact
    );
    let current_item = world
        .inventory_items(subject)
        .unwrap()
        .find(|item| item.facts().base == form(0x100))
        .unwrap()
        .id();
    world
        .remove_item_quantity(current_item, 1.try_into().unwrap())
        .unwrap();
    let changed = batch
        .observe(
            &world,
            &content,
            Intent::EngineeringObservation,
            Default::default(),
        )
        .unwrap();
    let Outcome::EngineeringObservation { trace } = &changed[2].outcome else {
        panic!("missing current query")
    };
    assert_eq!(trace.query.result, 17);
}

#[test]
fn cross_record_creation_limits_late_sites_and_mixed_sources_refuse_without_partial_authority() {
    use fallout_runtime::execution::condition::{CrossLimits, CrossRequests, RecordSelection};
    let (_directory, catalogue, _content, records) = cross_fixture();
    let (world, subject) = populated(&catalogue, &[(0x100, 1)]);
    let before = world.snapshot();
    let selections = [
        RecordSelection {
            record_index: 1,
            field_decoded_offset: 47,
            explicit_subject: Some(subject),
        },
        RecordSelection {
            record_index: 0,
            field_decoded_offset: records[0].sites()[0].field_decoded_offset(),
            explicit_subject: Some(subject),
        },
    ];
    let counts = CrossRequests::prepare(
        &world,
        &[&records[0], &records[1]],
        &selections,
        Default::default(),
    )
    .unwrap()
    .counts();
    let exact = CrossLimits {
        maximum_records: counts.records,
        maximum_source_bytes: counts.source_bytes,
        maximum_record_fields: counts.record_fields,
        maximum_record_sites: counts.record_sites,
        maximum_retained_source_bytes: counts.retained_source_bytes,
        maximum_requests: counts.requests,
        maximum_source_receipt_bytes: counts.source_receipt_bytes,
        maximum_receipt_comparisons: counts.receipt_comparisons,
        maximum_site_comparisons: counts.site_comparisons,
        maximum_query_variable_bytes: counts.query_variable_bytes,
    };
    assert_eq!(
        CrossRequests::prepare(&world, &[&records[0], &records[1]], &selections, exact)
            .unwrap()
            .counts(),
        counts
    );
    for (limits, reason) in [
        (
            CrossLimits {
                maximum_records: exact.maximum_records - 1,
                ..exact
            },
            "records",
        ),
        (
            CrossLimits {
                maximum_source_bytes: exact.maximum_source_bytes - 1,
                ..exact
            },
            "source bytes",
        ),
        (
            CrossLimits {
                maximum_record_fields: exact.maximum_record_fields - 1,
                ..exact
            },
            "record fields",
        ),
        (
            CrossLimits {
                maximum_record_sites: exact.maximum_record_sites - 1,
                ..exact
            },
            "record sites",
        ),
        (
            CrossLimits {
                maximum_retained_source_bytes: exact.maximum_retained_source_bytes - 1,
                ..exact
            },
            "retained source bytes",
        ),
        (
            CrossLimits {
                maximum_requests: exact.maximum_requests - 1,
                ..exact
            },
            "requests",
        ),
        (
            CrossLimits {
                maximum_source_receipt_bytes: exact.maximum_source_receipt_bytes - 1,
                ..exact
            },
            "source receipt bytes",
        ),
        (
            CrossLimits {
                maximum_receipt_comparisons: exact.maximum_receipt_comparisons - 1,
                ..exact
            },
            "receipt comparisons",
        ),
        (
            CrossLimits {
                maximum_site_comparisons: exact.maximum_site_comparisons - 1,
                ..exact
            },
            "site comparisons",
        ),
        (
            CrossLimits {
                maximum_query_variable_bytes: exact.maximum_query_variable_bytes - 1,
                ..exact
            },
            "query variable bytes",
        ),
    ] {
        assert!(
            matches!(CrossRequests::prepare(&world,&[&records[0],&records[1]],&selections,limits),
            Err(Error::BatchCapacity(value)) if value==reason),
            "{reason}"
        );
        assert_eq!(world.snapshot(), before);
    }
    let mut late = selections;
    late[1].field_decoded_offset += 1;
    assert!(matches!(
        CrossRequests::prepare(&world, &[&records[0], &records[1]], &late, exact),
        Err(Error::MissingSite(_))
    ));
    late = selections;
    late[1].record_index = 2;
    assert!(matches!(
        CrossRequests::prepare(&world, &[&records[0], &records[1]], &late, exact),
        Err(Error::MissingRecord(2))
    ));
    assert!(matches!(
        CrossRequests::prepare(&world, &[&records[0], &records[0]], &selections, exact),
        Err(Error::DuplicateRecord)
    ));
    let (_other, _, _, foreign) = batch_fixture(4);
    assert!(matches!(
        CrossRequests::prepare(
            &world,
            &[&foreign, &records[1]],
            &selections,
            Default::default()
        ),
        Err(Error::ContextChanged)
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn indexed_batch_matches_individual_sites_in_requested_order_with_one_receipt_validation() {
    use fallout_runtime::execution::condition::{BatchLimits, Requests};
    let (_dir, catalogue, content, record) = batch_fixture(257);
    let (world, subject) = populated(
        &catalogue,
        &[(0x100, u32::MAX), (0x100, u32::MAX), (0x102, 23)],
    );
    let before = world.snapshot();
    let offsets: Vec<_> = record
        .sites()
        .iter()
        .rev()
        .chain(record.sites()[..4].iter())
        .map(|site| site.field_decoded_offset())
        .collect();
    assert!(
        record
            .sites()
            .windows(2)
            .all(|sites| sites[0].field_decoded_offset() < sites[1].field_decoded_offset())
    );
    let batch = Requests::prepare(&world, &record, &offsets, BatchLimits::default()).unwrap();
    assert_eq!(batch.len(), 261);
    assert!(!batch.is_empty());
    let counts = batch.counts();
    assert_eq!(counts.cohort_validations, 1);
    assert_eq!(
        counts.source_receipt_bytes,
        serde_json::to_vec(&world.catalogue().sources)
            .unwrap()
            .len()
    );
    assert_eq!(counts.requests, offsets.len());
    assert_eq!(counts.record_sites, 257);
    assert!(counts.site_comparisons <= offsets.len() * 9);
    let old_search: usize = (1..=257).sum::<usize>() + 10;
    assert!(counts.site_comparisons * 10 < old_search);
    let observations = batch
        .observe(
            &world,
            &content,
            Some(subject),
            Intent::EngineeringObservation,
            1024,
        )
        .unwrap();
    let mut total = 0;
    for (&offset, observed) in offsets.iter().zip(&observations) {
        assert_eq!(observed.site.field_decoded_offset(), offset);
        let individual = Request::prepare(&world, &record, offset).unwrap();
        let expected = individual
            .observe(
                &world,
                &content,
                Some(subject),
                Intent::EngineeringObservation,
                2,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(observed).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        let Outcome::EngineeringObservation { trace } = &observed.outcome else {
            panic!("{observed:?}")
        };
        total += trace.query.contributions.len();
        assert!(observed.condition_truth.is_none() && !observed.condition_evaluation_ready);
    }
    assert_eq!(
        batch
            .observe(
                &world,
                &content,
                Some(subject),
                Intent::EngineeringObservation,
                total
            )
            .unwrap()
            .len(),
        offsets.len()
    );
    assert!(matches!(
        batch.observe(
            &world,
            &content,
            Some(subject),
            Intent::EngineeringObservation,
            total - 1
        ),
        Err(Error::Capacity)
    ));
    for observed in batch
        .observe(&world, &content, Some(subject), Intent::Faithful, 0)
        .unwrap()
    {
        reason(&observed, Unsupported::UnverifiedRetailSemantics);
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
fn batch_exact_admission_and_search_limits_refuse_without_a_partial_selection() {
    use fallout_runtime::execution::condition::{BatchLimits, Requests};
    let (_dir, catalogue, content, record) = batch_fixture(257);
    let (world, subject) = populated(&catalogue, &[(0x100, 1)]);
    let before = world.snapshot();
    let offsets = [
        record.sites()[256].field_decoded_offset(),
        record.sites()[0].field_decoded_offset(),
        record.sites()[256].field_decoded_offset(),
        record.sites()[128].field_decoded_offset(),
    ];
    let counts = Requests::prepare(&world, &record, &offsets, Default::default())
        .unwrap()
        .counts();
    let exact = BatchLimits {
        maximum_requests: 4,
        maximum_source_receipt_bytes: counts.source_receipt_bytes,
        maximum_site_comparisons: counts.site_comparisons,
    };
    assert_eq!(
        Requests::prepare(&world, &record, &offsets, exact)
            .unwrap()
            .counts(),
        counts
    );
    for (limits, expected) in [
        (
            BatchLimits {
                maximum_requests: 3,
                ..exact
            },
            "requests",
        ),
        (
            BatchLimits {
                maximum_source_receipt_bytes: exact.maximum_source_receipt_bytes - 1,
                ..exact
            },
            "source receipt bytes",
        ),
        (
            BatchLimits {
                maximum_site_comparisons: exact.maximum_site_comparisons - 1,
                ..exact
            },
            "site comparisons",
        ),
    ] {
        assert!(
            matches!(Requests::prepare(&world,&record,&offsets,limits),Err(Error::BatchCapacity(reason)) if reason==expected)
        );
        assert_eq!(world.snapshot(), before);
    }
    assert!(
        matches!(Requests::prepare(&world,&record,&[offsets[0],offsets[0]+1],Default::default()),Err(Error::MissingSite(offset)) if offset==offsets[0]+1)
    );
    let empty = Requests::prepare(
        &world,
        &record,
        &[],
        BatchLimits {
            maximum_requests: 0,
            maximum_site_comparisons: 0,
            ..exact
        },
    )
    .unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.counts().cohort_validations, 1);
    assert!(
        empty
            .observe(
                &world,
                &content,
                Some(subject),
                Intent::EngineeringObservation,
                0
            )
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn batch_context_guards_cover_empty_selection_foreign_content_and_cold_campaign_changes() {
    use fallout_runtime::execution::condition::Requests;
    let (dir, catalogue, content, record) = batch_fixture(2);
    let (world, subject) = populated(&catalogue, &[(0x100, 19)]);
    let before = world.snapshot();
    let selected = Requests::prepare(
        &world,
        &record,
        &[record.sites()[0].field_decoded_offset()],
        Default::default(),
    )
    .unwrap();
    let empty = Requests::prepare(&world, &record, &[], Default::default()).unwrap();
    let restored = World::restore(&catalogue, before.clone(), Default::default()).unwrap();
    assert_eq!(
        serde_json::to_value(
            selected
                .observe(
                    &world,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    1
                )
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(
            selected
                .observe(
                    &restored,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    1
                )
                .unwrap()
        )
        .unwrap()
    );
    let mut changed = before.clone();
    changed.campaign = fallout_runtime::identity::CampaignId::from_bytes([0x59; 16]).unwrap();
    let other_campaign = World::restore(&catalogue, changed.clone(), Default::default()).unwrap();
    for batch in [&selected, &empty] {
        assert!(matches!(
            batch.observe(
                &other_campaign,
                &content,
                Some(subject),
                Intent::Faithful,
                0
            ),
            Err(Error::ContextChanged)
        ));
    }
    assert_eq!(other_campaign.snapshot(), changed);
    let foreign_catalogue = load(dir.path(), &["FalloutNV.esm", "Other.esm"]);
    let mut foreign_store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into(), "Other.esm".into()],
        Default::default(),
    )
    .unwrap();
    let foreign_content = Content::load(&mut foreign_store, &foreign_catalogue, 100).unwrap();
    for batch in [&selected, &empty] {
        assert!(matches!(
            batch.observe(&world, &foreign_content, Some(subject), Intent::Faithful, 0),
            Err(Error::Content(_))
        ));
    }
    let foreign_world = World::new(&foreign_catalogue, Default::default()).unwrap();
    for offsets in [vec![], vec![record.sites()[0].field_decoded_offset()]] {
        assert!(matches!(
            Requests::prepare(&foreign_world, &record, &offsets, Default::default()),
            Err(Error::ContextChanged)
        ));
    }
    assert_eq!(world.snapshot(), before);
}

#[test]
#[ignore = "built CLI and authored metadata; condition source batching only, no original launch or truth"]
fn cli_condition_batch_helper() {
    use fallout_runtime::execution::condition::Requests;
    use serde_json::Value as Json;
    use std::path::PathBuf;
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let metadata =
        PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata input"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_CONDITION_BATCH_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let (temporary, catalogue, content, record) = batch_fixture(64);
    let (world, subject) = populated(
        &catalogue,
        &[(0x100, u32::MAX), (0x100, u32::MAX), (0x102, 23)],
    );
    let before = world.snapshot();
    let input_bytes = before.encode(Limits::default().max_snapshot_bytes).unwrap();
    let snapshot = evidence.join("snapshot.json");
    fs::write(&snapshot, &input_bytes).unwrap();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        temporary.path().join("FalloutNV.esm"),
        install.join("Data/FalloutNV.esm"),
    )
    .unwrap();
    fs::copy(
        metadata.join("authored-source-copy/FalloutNV.exe"),
        install.join("FalloutNV.exe"),
    )
    .unwrap();
    let order = evidence.join("order.json");
    fs::write(&order, b"[\"FalloutNV.esm\"]").unwrap();
    let offsets: Vec<_> = [63, 0, 63, 31]
        .map(|index| record.sites()[index].field_decoded_offset())
        .into();
    let batch = Requests::prepare(&world, &record, &offsets, Default::default()).unwrap();
    let counts = batch.counts();
    let request = json!({"schema_version":1,"record":form(0x500),"field_decoded_offsets":offsets,
        "explicit_subject":subject,"snapshot":snapshot,"maximum_source_receipt_bytes":counts.source_receipt_bytes,
        "maximum_site_comparisons":counts.site_comparisons,"maximum_contributions":5});
    let run_on = |name: &str, install: &Path, flag: &str, request: &Json, extra: &[&str]| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        let report_path = directory.join("report.json");
        // Compact input isolates the request-count cap from the separate 16 KiB
        // transport cap, including 4097 selected zero offsets.
        fs::write(&request_path, serde_json::to_vec(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["condition-dependencies", "--install"])
            .arg(install)
            .arg("--load-order")
            .arg(&order)
            .arg(flag)
            .arg(&request_path)
            .arg("--output")
            .arg(&report_path)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        assert_eq!(
            fs::read(&snapshot).unwrap(),
            input_bytes,
            "{name}: input changed"
        );
        (output, report_path)
    };
    let run = |name: &str, flag: &str, request: &Json| run_on(name, &install, flag, request, &[]);
    let (output, path) = run("batch", "--engineering-query-batch", &request);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(report["schema_version"], 5);
    let result = &report["engineering_batch"];
    assert_eq!(result["preparation"], serde_json::to_value(counts).unwrap());
    assert_eq!(result["canonical_state_unchanged"], true);
    assert_eq!(
        result["engineering"],
        serde_json::to_value(
            batch
                .observe(
                    &world,
                    &content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    5
                )
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        result["faithful"],
        serde_json::to_value(
            batch
                .observe(&world, &content, Some(subject), Intent::Faithful, 0)
                .unwrap()
        )
        .unwrap()
    );
    let expected_counts = [23_u64, 8_589_934_590, 23, 23];
    for (index, &offset) in offsets.iter().enumerate() {
        let single = json!({"record":form(0x500),"field_decoded_offset":offset,"explicit_subject":subject,"snapshot":snapshot});
        let (output, path) = run(
            &format!("single-{index}"),
            "--engineering-query-input",
            &single,
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let single: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(single["schema_version"], 4);
        assert_eq!(single["records"], report["records"]);
        assert_eq!(
            single["engineering_query"]["engineering"],
            result["engineering"][index]
        );
        assert_eq!(
            single["engineering_query"]["faithful"],
            result["faithful"][index]
        );
        assert_eq!(
            result["engineering"][index]["outcome"]["trace"]["query"]["result"],
            expected_counts[index]
        );
        assert_eq!(result["engineering"][index]["condition_truth"], Json::Null);
        assert_eq!(
            result["engineering"][index]["condition_evaluation_ready"],
            false
        );
    }
    for (name, field, value, reason) in [
        (
            "source-byte-short",
            "maximum_source_receipt_bytes",
            json!(counts.source_receipt_bytes - 1),
            "source receipt bytes",
        ),
        (
            "search-short",
            "maximum_site_comparisons",
            json!(counts.site_comparisons - 1),
            "site comparisons",
        ),
        (
            "contribution-short",
            "maximum_contributions",
            json!(4),
            "contribution budget",
        ),
        (
            "missing-site",
            "field_decoded_offsets",
            json!([offsets[0], offsets[0] + 1]),
            "is absent",
        ),
        (
            "wrong-record",
            "record",
            json!(form(0x777)),
            "no nondeleted admitted candidate",
        ),
        (
            "request-schema",
            "schema_version",
            json!(2),
            "invalid condition batch request",
        ),
        (
            "empty-selection",
            "field_decoded_offsets",
            json!([]),
            "invalid condition batch request",
        ),
        (
            "selection-limit",
            "field_decoded_offsets",
            json!(vec![0; 4097]),
            "invalid condition batch request",
        ),
        (
            "source-ceiling",
            "maximum_source_receipt_bytes",
            json!(1024 * 1024 + 1),
            "invalid condition batch request",
        ),
        (
            "search-ceiling",
            "maximum_site_comparisons",
            json!(1_048_577),
            "invalid condition batch request",
        ),
        (
            "contribution-ceiling",
            "maximum_contributions",
            json!(65_537),
            "invalid condition batch request",
        ),
        ("zero-subject", "explicit_subject", json!(0), "non-zero"),
        ("unknown-field", "initialize", json!(true), "unknown field"),
        (
            "request-byte-limit",
            "extra",
            json!("x".repeat(16 * 1024)),
            "input byte budget",
        ),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, path) = run(name, "--engineering-query-batch", &invalid);
        assert!(!output.status.success(), "{name}");
        assert!(!path.exists(), "{name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(reason),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (name, field, value) in [
        ("legacy", "schema_version", json!(3)),
        ("stale", "catalogue_sha256", json!("0".repeat(64))),
    ] {
        let mut invalid = serde_json::to_value(&before).unwrap();
        invalid[field] = value;
        let invalid_snapshot = evidence.join(format!("{name}.snapshot.json"));
        fs::write(&invalid_snapshot, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let mut input = request.clone();
        input["snapshot"] = json!(invalid_snapshot);
        let (output, path) = run(name, "--engineering-query-batch", &input);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    let mut unavailable = request.clone();
    unavailable["explicit_subject"] = json!(999);
    let (output, path) = run("unknown-subject", "--engineering-query-batch", &unavailable);
    assert!(!output.status.success());
    let unsupported: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        unsupported["engineering_batch"]["engineering"][0]["outcome"]["reason"],
        "host_query_unavailable"
    );
    assert_eq!(
        unsupported["engineering_batch"]["canonical_state_unchanged"],
        true
    );
    let (output, path) = run_on(
        "flags-conflict",
        &install,
        "--engineering-query-batch",
        &request,
        &["--engineering-query-input", "unused.json"],
    );
    assert!(!output.status.success());
    assert!(!path.exists());
    let partial = evidence.join("unsupported-source-copy");
    fs::create_dir(&partial).unwrap();
    fs::create_dir(partial.join("Data")).unwrap();
    fs::copy(install.join("FalloutNV.exe"), partial.join("FalloutNV.exe")).unwrap();
    let mut bytes = fs::read(temporary.path().join("FalloutNV.esm")).unwrap();
    // Mutate only a separately authored source's chosen CTDA Run On word.
    let position = record.identity().record_file_offset as usize + 24 + offsets[0] + 6 + 20;
    bytes[position..position + 4].copy_from_slice(&1_u32.to_le_bytes());
    fs::write(partial.join("Data/FalloutNV.esm"), bytes).unwrap();
    let (changed_catalogue, _, changed_record) = load_sources(&partial.join("Data"), &signatures());
    let (changed_world, changed_subject) =
        populated(&changed_catalogue, &[(0x100, 19), (0x102, 23)]);
    let changed_snapshot = evidence.join("unsupported.snapshot.json");
    fs::write(
        &changed_snapshot,
        changed_world
            .snapshot()
            .encode(Limits::default().max_snapshot_bytes)
            .unwrap(),
    )
    .unwrap();
    let mut input = request.clone();
    input["snapshot"] = json!(changed_snapshot);
    input["explicit_subject"] = json!(changed_subject);
    input["field_decoded_offsets"] =
        json!([changed_record.sites()[0].field_decoded_offset(), offsets[0]]);
    input["maximum_source_receipt_bytes"] = json!(1024 * 1024);
    input["maximum_site_comparisons"] = json!(1024);
    let (output, path) = run_on(
        "later-unsupported",
        &partial,
        "--engineering-query-batch",
        &input,
        &[],
    );
    assert!(!output.status.success());
    let report: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        report["engineering_batch"]["engineering"][0]["outcome"]["status"],
        "engineering_observation"
    );
    assert_eq!(
        report["engineering_batch"]["engineering"][1]["outcome"]["reason"],
        "subject_selection"
    );
    assert_eq!(
        report["engineering_batch"]["canonical_state_unchanged"],
        true
    );
    assert_eq!(world.snapshot(), before);
    fs::write(evidence.join("scope.json"),serde_json::to_vec_pretty(&json!({
        "scope":"source-bound condition batching only","original_executed":false,"actual_cli_calls":24,
        "preparation":counts,"individual_search_comparisons":64+1+64+32,
        "individual_source_receipt_validations":4,"requested_offsets":offsets,
        "canonical_snapshot":before,"condition_truth_verified":false,"retail_parity_accepted":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "built CLI and authored metadata; cross-record current condition queries, no original truth"]
fn cli_condition_records_helper() {
    use fallout_runtime::execution::condition::{CrossRequests, RecordSelection};
    use serde_json::Value as Json;
    use std::{cell::Cell, path::PathBuf};
    let cli = std::env::var_os("RF_SCRIPT_TRACE_CLI").expect("CLI");
    let input = PathBuf::from(std::env::var_os("RF_SCRIPT_TRACE_INPUT").expect("metadata"));
    let evidence =
        PathBuf::from(std::env::var_os("RF_SCRIPT_CONDITION_RECORDS_EVIDENCE").expect("evidence"));
    fs::create_dir(&evidence).unwrap();
    let (temporary, catalogue, content, records) = cross_fixture();
    let install = evidence.join("authored-source-copy");
    fs::create_dir(&install).unwrap();
    fs::create_dir(install.join("Data")).unwrap();
    fs::copy(
        temporary.path().join("FalloutNV.esm"),
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
    let (mut world, subject) = populated(&catalogue, &[(0x100, 7), (0x100, 11), (0x102, 23)]);
    let other = world.register_reference(None).unwrap();
    world.initialize_inventory(other).unwrap();
    world
        .add_item(other, Facts::unknown(form(0x100)), 31.try_into().unwrap())
        .unwrap();
    world
        .add_item(other, Facts::unknown(form(0x102)), 41.try_into().unwrap())
        .unwrap();
    let before = world.snapshot();
    let selections = [
        (1, 1, other),
        (0, 3, subject),
        (1, 0, subject),
        (0, 0, other),
        (1, 1, other),
    ]
    .map(|(record_index, site, subject)| RecordSelection {
        record_index,
        field_decoded_offset: records[record_index].sites()[site].field_decoded_offset(),
        explicit_subject: Some(subject),
    });
    let expected: Vec<_> = selections
        .iter()
        .map(|selection| {
            let request = Request::prepare(
                &world,
                &records[selection.record_index],
                selection.field_decoded_offset,
            )
            .unwrap();
            serde_json::to_value(
                request
                    .observe(
                        &world,
                        &content,
                        selection.explicit_subject,
                        Intent::EngineeringObservation,
                        2,
                    )
                    .unwrap(),
            )
            .unwrap()
        })
        .collect();
    let batch = CrossRequests::prepare(
        &world,
        &[&records[0], &records[1]],
        &selections,
        Default::default(),
    )
    .unwrap();
    let counts = batch.counts();
    let rows:Vec<_>=selections.iter().map(|selection|json!({"record":records[selection.record_index].identity().key,
        "field_decoded_offset":selection.field_decoded_offset,"explicit_subject":selection.explicit_subject})).collect();
    let request = json!({"schema_version":1,"intent":"engineering_observation","selections":rows,"snapshot":"snapshot.json",
        "maximum_records":64,"maximum_source_bytes":8388608,"maximum_record_fields":262144,
        "maximum_record_sites":65536,"maximum_retained_source_bytes":16777216,"maximum_requests":4096,
        "maximum_source_receipt_bytes":1048576,"maximum_receipt_comparisons":262144,"maximum_site_comparisons":1048576,
        "maximum_query_variable_bytes":1048576,"maximum_contributions":6,"maximum_observation_bytes":8388608,"maximum_report_bytes":8388608});
    let calls = Cell::new(0);
    let run = |name: &str,
               request: &Json,
               snapshot: &Snapshot,
               extra: &[&str],
               report_target: Option<&Path>| {
        let directory = evidence.join(name);
        fs::create_dir(&directory).unwrap();
        let request_path = directory.join("request.json");
        let snapshot_path = directory.join("snapshot.json");
        let report = report_target
            .map(Path::to_path_buf)
            .unwrap_or_else(|| directory.join("report.json"));
        let bytes = serde_json::to_vec(snapshot).unwrap();
        fs::write(&snapshot_path, &bytes).unwrap();
        fs::write(&request_path, serde_json::to_vec_pretty(request).unwrap()).unwrap();
        let output = Command::new(&cli)
            .args(["condition-dependencies", "--install"])
            .arg(&install)
            .arg("--load-order")
            .arg(&order)
            .arg("--engineering-query-records")
            .arg(&request_path)
            .arg("--output")
            .arg(&report)
            .args(extra)
            .output()
            .unwrap();
        fs::write(directory.join("stdout.txt"), &output.stdout).unwrap();
        fs::write(directory.join("stderr.txt"), &output.stderr).unwrap();
        assert_eq!(
            fs::read(&snapshot_path).unwrap(),
            bytes,
            "{name}: input changed"
        );
        calls.set(calls.get() + 1);
        (output, report)
    };
    let (output, path) = run("selected-records", &request, &before, &[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report_bytes = fs::read(path).unwrap();
    let report: Json = serde_json::from_slice(&report_bytes).unwrap();
    assert_eq!(report["preparation"], serde_json::to_value(counts).unwrap());
    assert_eq!(report["observations"], json!(expected));
    assert_eq!(report["canonical_state_unchanged"], true);
    assert_eq!(report["event_acknowledged"], false);
    assert_eq!(report["group_evaluation_verified"], false);
    assert_eq!(report["condition_evaluation_ready"], false);
    for (index, literal) in [41_u64, 23, 18, 31, 41].into_iter().enumerate() {
        assert_eq!(
            report["observations"][index]["outcome"]["trace"]["query"]["result"],
            literal
        );
        assert_eq!(report["observations"][index]["condition_truth"], Json::Null);
    }
    assert_eq!(
        report["observations"][0]["site"]["field_decoded_offset"],
        47
    );
    assert_eq!(
        report["observations"][0]["source"]["key"],
        json!(form(0x501))
    );
    let mut exact = request.clone();
    for (field, value) in [
        ("maximum_records", counts.records),
        ("maximum_source_bytes", counts.source_bytes),
        ("maximum_record_fields", counts.record_fields),
        ("maximum_record_sites", counts.record_sites),
        (
            "maximum_retained_source_bytes",
            counts.retained_source_bytes,
        ),
        ("maximum_requests", counts.requests),
        ("maximum_source_receipt_bytes", counts.source_receipt_bytes),
        ("maximum_receipt_comparisons", counts.receipt_comparisons),
        ("maximum_site_comparisons", counts.site_comparisons),
        ("maximum_query_variable_bytes", counts.query_variable_bytes),
        (
            "maximum_observation_bytes",
            serde_json::to_vec(&expected).unwrap().len(),
        ),
        ("maximum_report_bytes", report_bytes.len()),
    ] {
        exact[field] = json!(value);
    }
    let (output, path) = run("all-exact", &exact, &before, &[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(path).unwrap(), report_bytes);
    for field in [
        "maximum_records",
        "maximum_source_bytes",
        "maximum_record_fields",
        "maximum_record_sites",
        "maximum_retained_source_bytes",
        "maximum_requests",
        "maximum_source_receipt_bytes",
        "maximum_receipt_comparisons",
        "maximum_site_comparisons",
        "maximum_query_variable_bytes",
        "maximum_contributions",
        "maximum_observation_bytes",
        "maximum_report_bytes",
    ] {
        let mut short = exact.clone();
        short[field] = json!(short[field].as_u64().unwrap() - 1);
        let (output, path) = run(&format!("short-{field}"), &short, &before, &[], None);
        assert!(!output.status.success(), "{field}");
        assert!(!path.exists(), "{field}");
    }
    for (name, change, reason) in [
        ("faithful", 0, "unverified_retail_semantics"),
        ("late-null-subject", 1, "missing_subject"),
        ("late-unavailable-subject", 2, "host_query_unavailable"),
    ] {
        let mut refused = request.clone();
        match change {
            0 => refused["intent"] = json!("faithful"),
            1 => refused["selections"][4]["explicit_subject"] = Json::Null,
            _ => refused["selections"][4]["explicit_subject"] = json!(999),
        }
        let (output, path) = run(name, &refused, &before, &[], None);
        assert!(!output.status.success());
        let refused: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(refused["observations"][4]["outcome"]["reason"], reason);
        assert_eq!(refused["canonical_state_unchanged"], true);
        if change != 0 {
            assert_eq!(
                refused["observations"][0]["outcome"]["trace"]["query"]["result"],
                41
            );
        }
    }
    let mut late = request.clone();
    late["selections"][4]["field_decoded_offset"] = json!(48);
    let (output, path) = run("late-wrong-site", &late, &before, &[], None);
    assert!(!output.status.success());
    assert!(!path.exists());
    for (name, key) in [
        ("wrong-record", form(0x999)),
        ("wrong-record-kind", form(0x100)),
    ] {
        let mut invalid = request.clone();
        invalid["selections"][4]["record"] = json!(key);
        let (output, path) = run(name, &invalid, &before, &[], None);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    for (name, field, value) in [
        ("wrong-schema", "schema_version", json!(2)),
        ("no-selection", "selections", json!([])),
        ("unknown-field", "initialize", json!(true)),
        ("zero-report", "maximum_report_bytes", json!(0)),
        ("report-ceiling", "maximum_report_bytes", json!(8388609)),
        ("record-ceiling", "maximum_records", json!(65)),
        ("request-ceiling", "maximum_requests", json!(4097)),
        (
            "observation-ceiling",
            "maximum_observation_bytes",
            json!(8388609),
        ),
        ("request-bytes", "extra", json!("x".repeat(16 * 1024))),
    ] {
        let mut invalid = request.clone();
        invalid[field] = value;
        let (output, path) = run(name, &invalid, &before, &[], None);
        assert!(!output.status.success(), "{name}");
        assert!(!path.exists());
    }
    for (name, field) in [
        ("omitted-subject", "explicit_subject"),
        ("omitted-offset", "field_decoded_offset"),
    ] {
        let mut invalid = request.clone();
        invalid["selections"][0]
            .as_object_mut()
            .unwrap()
            .remove(field);
        let (output, path) = run(name, &invalid, &before, &[], None);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    let mut stale = before.clone();
    stale.catalogue_sha256 = "0".repeat(64);
    let (output, path) = run("stale-source", &request, &stale, &[], None);
    assert!(!output.status.success());
    assert!(!path.exists());
    stale = before.clone();
    stale.schema_version = 3;
    let (output, path) = run("old-snapshot-schema", &request, &stale, &[], None);
    assert!(!output.status.success());
    assert!(!path.exists());
    let current_item = world
        .inventory_items(subject)
        .unwrap()
        .find(|item| item.facts().base == form(0x100))
        .unwrap()
        .id();
    world
        .remove_item_quantity(current_item, 1.try_into().unwrap())
        .unwrap();
    let changed = world.snapshot();
    let (output, path) = run("changed-current-inventory", &request, &changed, &[], None);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current: Json = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        current["observations"][2]["outcome"]["trace"]["query"]["result"],
        17
    );
    assert_eq!(
        current["observations"][0]["outcome"]["trace"]["query"]["result"],
        41
    );
    for (name, extra) in [
        (
            "conflict-batch",
            vec!["--engineering-query-batch", "unused.json"],
        ),
        (
            "conflict-single",
            vec!["--engineering-query-input", "unused.json"],
        ),
        ("conflict-owners", vec!["--include-source-owners"]),
        ("conflict-runs", vec!["--include-source-runs"]),
    ] {
        let (output, path) = run(name, &request, &before, &extra, None);
        assert!(!output.status.success());
        assert!(!path.exists());
    }
    let protected = install.join("protected.json");
    let (output, path) = run("protected-report", &request, &before, &[], Some(&protected));
    assert!(!output.status.success());
    assert!(!path.exists());
    let existing = evidence.join("existing.json");
    fs::write(&existing, b"preserve").unwrap();
    let (output, _) = run("existing-report", &request, &before, &[], Some(&existing));
    assert!(!output.status.success());
    let linked = evidence.join("linked.json");
    fs::hard_link(&existing, &linked).unwrap();
    let (output, _) = run("hardlink-report", &request, &before, &[], Some(&linked));
    assert!(!output.status.success());
    assert_eq!(fs::read(existing).unwrap(), b"preserve");
    assert_eq!(world.snapshot(), changed);
    fs::write(evidence.join("scope.json"),serde_json::to_vec_pretty(&json!({
        "scope":"ordered_cross_record_condition_queries_over_strict_current_snapshot",
        "actual_cli_calls":calls.get(),"preparation":counts,"initial_snapshot":before,"changed_snapshot":changed,
        "literal_initial_counts":[41,23,18,31,41],"literal_changed_count":17,"exact_report_bytes":report_bytes.len(),
        "condition_truth_verified":false,"original_executed":false,"retail_parity_accepted":false
    })).unwrap()).unwrap();
}
