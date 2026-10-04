mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, package_dependencies, packages},
    condition_operands::{Parameter, Signature, Signatures},
    inventory, loaded_scripts, plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::packages::{
        CapabilityLimits, Error, Limits, Operation, Requests, UnsupportedDependency,
    },
    execution::condition::{Intent, Outcome, Unsupported},
    foreign::Content,
    inventory::Facts,
    snapshot::Snapshot,
};
use std::{fs, path::Path};

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = record(kind, id, flags, body);
    bytes[20..22].copy_from_slice(&15_u16.to_le_bytes());
    bytes
}
fn ctda(function: u16, run_on: u32) -> Vec<u8> {
    let mut bytes = [0; 24];
    bytes[8..10].copy_from_slice(&function.to_le_bytes());
    bytes[12..16].copy_from_slice(&0x300_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&run_on.to_le_bytes());
    field(b"CTDA", &bytes)
}
fn fixture(path: &Path, configuration: usize, template_flags: u16) {
    let mut npc = Vec::new();
    for _ in 0..configuration {
        let mut bytes = [0; 24];
        bytes[22..24].copy_from_slice(&template_flags.to_le_bytes());
        npc.extend(field(b"ACBS", &bytes));
    }
    npc.extend(field(b"DATA", &[0; 11]));
    for package in [0x200_u32, 0x201, 0x200, 0, 0x777, 0x202, 0x300] {
        npc.extend(field(b"PKID", &package.to_le_bytes()));
    }
    let a = [
        field(b"PKDT", &[0; 12]),
        field(b"PSDT", &[0xff; 8]),
        ctda(47, 0),
        ctda(0x777, 0),
    ]
    .concat();
    let b = [field(b"PKDT", &[0; 8]), ctda(47, 1)].concat();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, 0, &npc),
            disk(b"PACK", 0x200, 0, &a),
            disk(b"PACK", 0x201, 0, &b),
            disk(
                b"PACK",
                0x202,
                plugin::DELETED | plugin::COMPRESSED,
                b"unread tombstone",
            ),
            disk(b"MISC", 0x300, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
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
fn with_sources(
    path: &Path,
    callback: impl FnOnce(
        &World<'_>,
        &Content,
        &actors::Catalogue<'_>,
        &associations::Catalogue<'_>,
        &package_dependencies::Catalogue<'_>,
        fallout_runtime::identity::ReferenceId,
    ),
) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let packages = packages::Catalogue::load(&mut store, Default::default()).unwrap();
    let dependencies = package_dependencies::Catalogue::load(
        &mut store,
        &packages,
        &scripts,
        &signatures(),
        Default::default(),
    )
    .unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    let mut world = World::new(&scripts, WorldLimits::default()).unwrap();
    let subject = world.register_reference(None).unwrap();
    world.initialize_inventory(subject).unwrap();
    for count in [2_u32, 3] {
        world
            .add_item(
                subject,
                Facts::unknown(form(0x300)),
                count.try_into().unwrap(),
            )
            .unwrap();
    }
    callback(
        &world,
        &content,
        &actors,
        &associations,
        &dependencies,
        subject,
    );
}

fn retain(
    path: &Path,
    world: &World<'_>,
    observation: &fallout_runtime::actor_rules::packages::Observation<'_>,
) {
    let Some(root) = std::env::var_os("FALLOUT_ACTOR_PACKAGE_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&root);
    assert!(root.is_absolute() && root.is_dir());
    let case = root.join("authored-packages");
    fs::create_dir(&case).unwrap();
    fs::create_dir(case.join("Data")).unwrap();
    fs::copy(path.join("FalloutNV.esm"), case.join("Data/FalloutNV.esm")).unwrap();
    fs::write(case.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(
        case.join("snapshot.json"),
        world
            .snapshot()
            .encode(WorldLimits::default().max_snapshot_bytes)
            .unwrap(),
    )
    .unwrap();
    fs::write(
        case.join("expected.json"),
        serde_json::to_vec_pretty(observation).unwrap(),
    )
    .unwrap();
}

#[test]
fn authored_order_repeats_and_unavailable_links_reach_shared_canonical_observations() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, packages, subject| {
            let before = world.snapshot();
            let requests = Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            let observation = requests
                .observe(
                    world,
                    content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    Limits::default(),
                )
                .unwrap();
            assert_eq!(
                observation
                    .packages
                    .iter()
                    .map(|package| package.association.binding.raw_form)
                    .collect::<Vec<_>>(),
                [0x200, 0x201, 0x200, 0, 0x777, 0x202, 0x300]
            );
            assert_eq!(observation.condition_requests, 5);
            assert_eq!(observation.query_contributions, 4);
            assert!(observation.issues.is_empty());
            assert_eq!(observation.configuration.unwrap().1.kind, *b"ACBS");
            assert_eq!(observation.explicit_subject, Some(subject));
            assert!(observation.explicit_subject_origin.is_none());
            for index in [0, 2] {
                let package = &observation.packages[index];
                assert_eq!(package.field.kind, *b"PKID");
                let source = package.package.as_ref().unwrap();
                assert!(source.source.decoded_record_sha256.is_some());
                let record = packages
                    .get(&form(0x200))
                    .unwrap()
                    .conditions
                    .as_ref()
                    .unwrap();
                assert!(std::ptr::eq(package.conditions[0].site, &record.sites()[0]));
                match &package.conditions[0].outcome {
                    Outcome::EngineeringObservation { trace } => assert_eq!(trace.query.result, 5),
                    result => panic!("actual canonical item count required: {result:?}"),
                }
                assert!(
                    package
                        .conditions
                        .iter()
                        .all(|condition| condition.condition_truth.is_none()
                            && !condition.condition_evaluation_ready)
                );
            }
            assert!(matches!(
                observation.packages[1].conditions[0].outcome,
                Outcome::Unsupported {
                    reason: Unsupported::SubjectSelection,
                    ..
                }
            ));
            assert!(
                observation.packages[3..]
                    .iter()
                    .all(|package| package.package.is_none() && package.conditions.is_empty())
            );
            assert!(
                observation
                    .packages
                    .iter()
                    .all(|package| package.eligible.is_none())
            );
            assert!(
                !observation.effective_package_selection_supported
                    && !observation.scheduling_supported
                    && !observation.original_behavior_verified
            );
            assert_eq!(world.snapshot(), before);
            retain(directory.path(), world, &observation);
        },
    );
}

#[test]
fn canonical_restore_preserves_requests_and_other_campaign_or_missing_reference_fails() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, packages, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            let snapshot = world.snapshot();
            let bytes = snapshot.encode(1_000_000).unwrap();
            let snapshot = Snapshot::decode(&bytes, WorldLimits::default()).unwrap();
            let restored =
                World::restore(world.catalogue(), snapshot, WorldLimits::default()).unwrap();
            let before = requests
                .observe(
                    world,
                    content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    Limits::default(),
                )
                .unwrap();
            let after = requests
                .observe(
                    &restored,
                    content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    Limits::default(),
                )
                .unwrap();
            assert_eq!(
                serde_json::to_value(before).unwrap(),
                serde_json::to_value(after).unwrap()
            );
            let other = World::new(world.catalogue(), WorldLimits::default()).unwrap();
            assert!(matches!(
                requests.observe(
                    &other,
                    content,
                    None,
                    Intent::EngineeringObservation,
                    Limits::default()
                ),
                Err(Error::ContextChanged)
            ));
            let missing = fallout_runtime::identity::ReferenceId(999.try_into().unwrap());
            assert!(matches!(
                requests.observe(
                    world,
                    content,
                    Some(missing),
                    Intent::EngineeringObservation,
                    Limits::default()
                ),
                Err(Error::State(fallout_runtime::Error::MissingReference))
            ));
        },
    );
}

#[test]
fn configuration_and_template_uncertainty_preserves_only_authored_requests() {
    for (count, flags, issue) in [
        (0, 0, "missing_actor_configuration"),
        (2, 0, "ambiguous_actor_configuration"),
        (1, 0x20, "ai_package_template_selection_unsupported"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        fixture(directory.path(), count, flags);
        with_sources(
            directory.path(),
            |world, content, actors, links, packages, subject| {
                let requests = Requests::prepare(
                    world,
                    actors,
                    links,
                    packages,
                    &form(0x100),
                    Limits::default(),
                )
                .unwrap();
                let observation = requests
                    .observe(
                        world,
                        content,
                        Some(subject),
                        Intent::Faithful,
                        Limits::default(),
                    )
                    .unwrap();
                assert_eq!(observation.issues[0].code, issue);
                assert_eq!(observation.packages.len(), 7);
                assert_eq!(observation.condition_requests, 5);
                assert!(
                    observation
                        .packages
                        .iter()
                        .flat_map(|package| &package.conditions)
                        .all(|condition| matches!(condition.outcome, Outcome::Unsupported { .. }))
                );
                assert!(matches!(
                    observation.packages[0].conditions[0].outcome,
                    Outcome::Unsupported {
                        reason: Unsupported::UnverifiedRetailSemantics,
                        ..
                    }
                ));
            },
        );
    }
}

#[test]
fn preparation_and_observation_work_and_projection_have_exact_aggregate_limits() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, packages, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            let observation = requests
                .observe(
                    world,
                    content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    Limits::default(),
                )
                .unwrap();
            let bytes = serde_json::to_vec(&observation).unwrap().len();
            let exact = Limits {
                max_occurrences: 7,
                max_conditions: 5,
                max_visits: observation.preparation_visits,
                max_query_contributions: 4,
                max_projection_bytes: bytes,
            };
            let bounded =
                Requests::prepare(world, actors, links, packages, &form(0x100), exact).unwrap();
            bounded
                .observe(
                    world,
                    content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    exact,
                )
                .unwrap();
            for limits in [
                Limits {
                    max_occurrences: 6,
                    ..exact
                },
                Limits {
                    max_conditions: 4,
                    ..exact
                },
                Limits {
                    max_visits: exact.max_visits - 1,
                    ..exact
                },
            ] {
                assert!(matches!(
                    Requests::prepare(world, actors, links, packages, &form(0x100), limits),
                    Err(Error::Capacity(_))
                ));
                assert!(matches!(
                    requests.observe(
                        world,
                        content,
                        Some(subject),
                        Intent::EngineeringObservation,
                        limits
                    ),
                    Err(Error::Capacity(_))
                ));
            }
            assert!(
                requests
                    .observe(
                        world,
                        content,
                        Some(subject),
                        Intent::EngineeringObservation,
                        Limits {
                            max_projection_bytes: bytes - 1,
                            ..exact
                        }
                    )
                    .is_err()
            );
            assert!(matches!(
                requests.observe(
                    world,
                    content,
                    Some(subject),
                    Intent::EngineeringObservation,
                    Limits {
                        max_query_contributions: 3,
                        ..Limits::default()
                    }
                ),
                Err(Error::Condition(
                    fallout_runtime::execution::condition::Error::Capacity
                ))
            ));
            assert!(
                Requests::prepare(
                    world,
                    actors,
                    links,
                    packages,
                    &form(0x888),
                    Limits::default()
                )
                .is_err()
            );
        },
    );
}

#[test]
fn changed_content_cohort_is_rejected_before_host_requests_are_prepared() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(directory.path(), |world, _, actors, links, packages, _| {
        let other_directory = tempfile::tempdir().unwrap();
        fixture(other_directory.path(), 1, 0x20);
        let other = load(other_directory.path(), &["FalloutNV.esm"]);
        let other_world = World::new(&other, WorldLimits::default()).unwrap();
        assert!(matches!(
            Requests::prepare(
                &other_world,
                actors,
                links,
                packages,
                &form(0x100),
                Limits::default()
            ),
            Err(Error::ContextChanged)
        ));
        assert!(
            Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Limits::default()
            )
            .is_ok()
        );
    });
}

fn capability_fixture(path: &Path, declared: bool, rich: bool) {
    let mut npc = [field(b"ACBS", &[0; 24]), field(b"DATA", &[0; 11])].concat();
    if declared {
        for _ in 0..2 {
            npc.extend(field(b"PKID", &0x200_u32.to_le_bytes()));
        }
    }
    let mut package = [field(b"PKDT", &[0xa5; 12]), field(b"PSDT", &[0xff; 8])].concat();
    if rich {
        package.extend(ctda(47, 0));
        package.extend(field(b"POBA", &[]));
        package.extend(field(b"INAM", &0x400_u32.to_le_bytes()));
        package.extend(unit(&[(1, 1)], &[(b"SCRO", 0x300)]));
        package.extend(field(b"POCA", &[]));
        package.extend(field(b"TNAM", &0x300_u32.to_le_bytes()));
        package.extend(field(b"SCHR", &[0; 20]));
    }
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, 0, &npc),
            disk(b"PACK", 0x200, 0, &package),
            disk(b"MISC", 0x300, 0, &[]),
            disk(b"IDLE", 0x400, 0, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn retain_capability(
    name: &str,
    path: &Path,
    world: &World<'_>,
    capability: &fallout_runtime::actor_rules::packages::Capability<'_>,
) {
    if let Some(root) = std::env::var_os("FALLOUT_ACTOR_PACKAGE_CAPABILITY_EVIDENCE_DIR") {
        let case = Path::new(&root).join(name);
        fs::create_dir_all(case.join("Data")).unwrap();
        fs::copy(path.join("FalloutNV.esm"), case.join("Data/FalloutNV.esm")).unwrap();
        fs::write(case.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
        fs::write(
            case.join("snapshot.json"),
            world
                .snapshot()
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        fs::write(
            case.join("expected-capability.json"),
            serde_json::to_vec(capability).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn source_complete_zero_conditions_and_empty_lists_refuse_every_operation_without_noop_ai() {
    let directory = tempfile::tempdir().unwrap();
    for (name, declared) in [("empty", false), ("zero-conditions", true)] {
        capability_fixture(directory.path(), declared, false);
        with_sources(
            directory.path(),
            |world, content, actors, links, packages, subject| {
                let before = world.snapshot();
                let requests = Requests::prepare(
                    world,
                    actors,
                    links,
                    packages,
                    &form(0x100),
                    Default::default(),
                )
                .unwrap();
                for operation in [
                    Operation::Eligibility,
                    Operation::Selection,
                    Operation::Scheduling,
                ] {
                    let result = requests
                        .capability(world, content, Some(subject), operation, Default::default())
                        .unwrap();
                    assert!(result.physical_source_complete);
                    assert!(!result.execution_admitted && !result.state_changed);
                    assert_eq!(result.observation.condition_requests, 0);
                    assert_eq!(result.observation.intent, Intent::Faithful);
                    assert_eq!(result.package_inputs.len(), if declared { 2 } else { 0 });
                    let refusal = result.require_execution().unwrap_err();
                    assert_eq!(refusal.operation, operation);
                    assert_eq!(
                        refusal.dependencies.len(),
                        match operation {
                            Operation::Eligibility => 2,
                            Operation::Selection => 3,
                            Operation::Scheduling => 5,
                        }
                    );
                    assert!(
                        refusal
                            .dependencies
                            .contains(&UnsupportedDependency::OriginalConditionTruth)
                    );
                    if declared {
                        assert!(std::ptr::eq(
                            result.package_inputs[0].source.unwrap(),
                            packages.get(&form(0x200)).unwrap().source_definition()
                        ));
                        assert!(std::ptr::eq(
                            result.package_inputs[0].source.unwrap(),
                            result.package_inputs[1].source.unwrap()
                        ));
                        assert!(matches!(
                            result.package_inputs[0].source.unwrap().fields[0].value,
                            fallout_data::actors::packages::Value::General {
                                package_type: 165,
                                ..
                            }
                        ));
                    }
                    assert_eq!(world.snapshot(), before);
                    if operation == Operation::Scheduling {
                        retain_capability(name, directory.path(), world, &result);
                    }
                }
            },
        );
    }
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, packages, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Default::default(),
            )
            .unwrap();
            let result = requests
                .capability(
                    world,
                    content,
                    Some(subject),
                    Operation::Eligibility,
                    Default::default(),
                )
                .unwrap();
            assert!(!result.physical_source_complete);
            assert_eq!(
                result
                    .package_inputs
                    .iter()
                    .map(|input| input.physical_source_complete)
                    .collect::<Vec<_>>(),
                vec![true, true, true, false, false, false, false]
            );
            assert!(
                result.package_inputs[3..]
                    .iter()
                    .all(|input| input.source.is_none())
            );
            assert!(
                result
                    .observation
                    .packages
                    .iter()
                    .flat_map(|package| &package.conditions)
                    .all(|site| matches!(site.outcome, Outcome::Unsupported { .. }))
            );
            assert_eq!(result.observation.query_contributions, 0);
            assert!(result.require_execution().is_err());
            retain_capability("partial", directory.path(), world, &result);
        },
    );
}

#[test]
fn capability_borrows_existing_events_compiled_units_findings_and_enforces_every_bound() {
    let directory = tempfile::tempdir().unwrap();
    capability_fixture(directory.path(), true, true);
    with_sources(
        directory.path(),
        |world, content, actors, links, packages, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Default::default(),
            )
            .unwrap();
            let result = requests
                .capability(
                    world,
                    content,
                    Some(subject),
                    Operation::Scheduling,
                    Default::default(),
                )
                .unwrap();
            let original = packages.get(&form(0x200)).unwrap();
            assert!(result.physical_source_complete);
            assert!(std::ptr::eq(
                result.package_inputs[0].event_fields,
                original.event_fields.as_slice()
            ));
            assert!(std::ptr::eq(
                result.package_inputs[0].scripts,
                original.scripts.as_slice()
            ));
            assert_eq!(result.package_inputs[0].scripts.len(), 2);
            assert_eq!(
                result.package_inputs[0].scripts[0].version.compiled_bytes,
                Some(14)
            );
            assert_eq!(
                result.package_inputs[0].scripts[1].version.compiled_bytes,
                None
            );
            assert_eq!(
                result.package_inputs[0].dependency_findings[0].code,
                "package_event_link_schema_kind_mismatch"
            );
            assert_eq!(result.observation.query_contributions, 0);
            assert!(
                result
                    .observation
                    .packages
                    .iter()
                    .flat_map(|package| &package.conditions)
                    .all(|site| matches!(
                        site.outcome,
                        Outcome::Unsupported {
                            reason: Unsupported::UnverifiedRetailSemantics,
                            ..
                        }
                    ))
            );
            assert_eq!(result.counts.scripts, 4);
            assert!(result.counts.event_fields > 0 && result.counts.script_entries > 0);
            let exact = CapabilityLimits {
                max_fields: result.counts.fields,
                max_event_fields: result.counts.event_fields,
                max_scripts: result.counts.scripts,
                max_script_entries: result.counts.script_entries,
                max_visits: result.counts.visits,
                max_projection_bytes: serde_json::to_vec(&result).unwrap().len(),
                ..Default::default()
            };
            requests
                .capability(world, content, Some(subject), Operation::Scheduling, exact)
                .unwrap();
            for limits in [
                CapabilityLimits {
                    max_fields: exact.max_fields - 1,
                    ..exact
                },
                CapabilityLimits {
                    max_event_fields: exact.max_event_fields - 1,
                    ..exact
                },
                CapabilityLimits {
                    max_scripts: exact.max_scripts - 1,
                    ..exact
                },
                CapabilityLimits {
                    max_script_entries: exact.max_script_entries - 1,
                    ..exact
                },
                CapabilityLimits {
                    max_visits: exact.max_visits - 1,
                    ..exact
                },
                CapabilityLimits {
                    max_projection_bytes: exact.max_projection_bytes - 1,
                    ..exact
                },
                CapabilityLimits {
                    requests: Limits {
                        max_conditions: 1,
                        ..Default::default()
                    },
                    ..exact
                },
            ] {
                assert!(
                    requests
                        .capability(world, content, Some(subject), Operation::Scheduling, limits)
                        .is_err()
                );
            }
            retain_capability("rich", directory.path(), world, &result);
        },
    );
}

#[test]
fn capability_uses_fresh_canonical_restore_and_rejects_campaign_content_and_reference_mismatch() {
    let directory = tempfile::tempdir().unwrap();
    capability_fixture(directory.path(), true, true);
    with_sources(
        directory.path(),
        |world, content, actors, links, packages, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                packages,
                &form(0x100),
                Default::default(),
            )
            .unwrap();
            let snapshot = world.snapshot();
            let restored =
                World::restore(world.catalogue(), snapshot.clone(), WorldLimits::default())
                    .unwrap();
            let a = requests
                .capability(
                    world,
                    content,
                    Some(subject),
                    Operation::Selection,
                    Default::default(),
                )
                .unwrap();
            let b = requests
                .capability(
                    &restored,
                    content,
                    Some(subject),
                    Operation::Selection,
                    Default::default(),
                )
                .unwrap();
            assert_eq!(
                serde_json::to_value(a).unwrap(),
                serde_json::to_value(b).unwrap()
            );
            assert_eq!(restored.snapshot(), snapshot);
            let other = World::new(world.catalogue(), WorldLimits::default()).unwrap();
            assert!(matches!(
                requests.capability(
                    &other,
                    content,
                    None,
                    Operation::Eligibility,
                    Default::default()
                ),
                Err(Error::ContextChanged)
            ));
            let missing = fallout_runtime::identity::ReferenceId(999.try_into().unwrap());
            assert!(matches!(
                requests.capability(
                    world,
                    content,
                    Some(missing),
                    Operation::Eligibility,
                    Default::default()
                ),
                Err(Error::State(fallout_runtime::Error::MissingReference))
            ));
            let other_dir = tempfile::tempdir().unwrap();
            fixture(other_dir.path(), 1, 0x20);
            let mut other_store = RecordStore::open_nv_headers(
                other_dir.path(),
                &["FalloutNV.esm".into()],
                Default::default(),
            )
            .unwrap();
            let scripts = loaded_scripts::Catalogue::load(
                &mut other_store,
                Default::default(),
                |_, _| Ok(()),
            )
            .unwrap();
            let other_content = Content::load(&mut other_store, &scripts, 100).unwrap();
            assert!(matches!(
                requests.capability(
                    world,
                    &other_content,
                    Some(subject),
                    Operation::Eligibility,
                    Default::default()
                ),
                Err(Error::Content(_))
            ));
        },
    );
}
