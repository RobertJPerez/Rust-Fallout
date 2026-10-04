mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, factions},
    inventory, loaded_scripts, plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::factions::{Error, Limits, Requests},
    foreign::Content,
    snapshot::Snapshot,
};
use std::{fs, path::Path};

fn disk(kind: &[u8; 4], id: u32, flags: u32, version: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = record(kind, id, flags, body);
    bytes[20..22].copy_from_slice(&version.to_le_bytes());
    bytes
}
fn relation(raw: u32, modifier: i32, reaction: u32) -> Vec<u8> {
    field(
        b"XNAM",
        &[
            raw.to_le_bytes().as_slice(),
            &modifier.to_le_bytes(),
            &reaction.to_le_bytes(),
        ]
        .concat(),
    )
}
fn fixture(path: &Path, configurations: usize, mask: u16) {
    let mut actor = Vec::new();
    for _ in 0..configurations {
        let mut bytes = [0; 24];
        bytes[22..24].copy_from_slice(&mask.to_le_bytes());
        bytes[20..22].copy_from_slice(&i16::MIN.to_le_bytes());
        actor.extend(field(b"ACBS", &bytes));
    }
    actor.extend(field(b"DATA", &[0; 11]));
    for (raw, rank) in [
        (0x200_u32, -128_i8),
        (0x201, 127),
        (0x200, -1),
        (0, 0),
        (0x777, 2),
        (0x202, 3),
        (0x300, 4),
    ] {
        actor.extend(field(
            b"SNAM",
            &[
                raw.to_le_bytes().as_slice(),
                &[rank as u8, 0xaa, 0xbb, 0xcc],
            ]
            .concat(),
        ));
    }
    let legacy = [
        field(b"DATA", &[255]),
        field(b"CNAM", &0x7fc0_1234_u32.to_le_bytes()),
        field(b"RNAM", &i32::MIN.to_le_bytes()),
        field(b"MNAM", b"raw rank label\0"),
        relation(0x201, i32::MIN, u32::MAX),
        relation(0x400, i32::MAX, 0),
        relation(0, -1, 1),
        relation(0x777, 2, 2),
        relation(0x202, 3, 3),
        relation(0x300, 4, 4),
        relation(0x201, 5, 2),
    ]
    .concat();
    let modern = [
        field(b"DATA", &[1, 128, 0x55, 0xaa]),
        field(b"RNAM", &i32::MAX.to_le_bytes()),
        relation(0x200, -3, 1),
    ]
    .concat();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, 0, 15, &actor),
            disk(b"FACT", 0x200, 0, 1, &legacy),
            disk(b"FACT", 0x201, 0, 15, &modern),
            disk(
                b"FACT",
                0x202,
                plugin::DELETED | plugin::COMPRESSED,
                16,
                b"unread tombstone",
            ),
            disk(b"MISC", 0x300, 0, 15, &[]),
            disk(b"RACE", 0x400, 0, 15, &[]),
        ]
        .concat(),
    )
    .unwrap();
}
fn with_sources(
    path: &Path,
    callback: impl FnOnce(
        &World<'_>,
        &Content,
        &actors::Catalogue<'_>,
        &associations::Catalogue<'_>,
        &factions::Catalogue,
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
    let links = associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let factions = factions::Catalogue::load(&mut store, Default::default()).unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    let mut world = World::new(&scripts, WorldLimits::default()).unwrap();
    // Deliberately a different authored origin: reference existence is not base ownership.
    let subject = world.register_reference(Some(form(0x300))).unwrap();
    callback(&world, &content, &actors, &links, &factions, subject);
}

fn retain(
    path: &Path,
    world: &World<'_>,
    observation: &fallout_runtime::actor_rules::factions::Observation<'_>,
) {
    let Some(root) = std::env::var_os("FALLOUT_ACTOR_FACTION_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&root);
    assert!(root.is_absolute() && root.is_dir());
    let case = root.join("authored-factions");
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
fn physical_signed_ranks_and_ordered_relationships_are_read_only_source_requests() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, factions, subject| {
            let before = world.snapshot();
            let requests = Requests::prepare(
                world,
                actors,
                links,
                factions,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            let observation = requests
                .observe(world, content, Some(subject), Limits::default())
                .unwrap();
            assert_eq!(observation.factions.len(), 7);
            assert_eq!(observation.relationship_requests, 15);
            assert_eq!(observation.faction_fields, 25);
            assert_eq!(observation.explicit_subject_origin, Some(&form(0x300)));
            assert!(
                !observation.membership_initialization_supported
                    && !observation.relationship_evaluation_supported
                    && !observation.condition_truth_supported
            );
            assert!(observation.issues.is_empty());
            assert!(matches!(
                observation.authored_configuration.unwrap().1.value,
                actors::fields::Value::Configuration {
                    disposition_base: i16::MIN,
                    ..
                }
            ));
            let ranks: Vec<_> = observation
                .factions
                .iter()
                .map(|row| row.association.faction_rank.unwrap())
                .collect();
            assert_eq!(ranks, [-128, 127, -1, 0, 2, 3, 4]);
            assert!(
                observation
                    .factions
                    .iter()
                    .all(|row| row.live_membership.is_none()
                        && row.effective_rank.is_none()
                        && row.association.faction_unused == Some([0xaa, 0xbb, 0xcc]))
            );
            assert_eq!(
                observation
                    .factions
                    .iter()
                    .map(|row| row.association.binding.status)
                    .collect::<Vec<_>>(),
                [
                    inventory::Status::Defined,
                    inventory::Status::Defined,
                    inventory::Status::Defined,
                    inventory::Status::Null,
                    inventory::Status::Missing,
                    inventory::Status::Deleted,
                    inventory::Status::Defined
                ]
            );
            assert!(
                observation.factions[3..]
                    .iter()
                    .all(|row| row.faction.is_none())
            );
            let first = observation.factions[0].faction.as_ref().unwrap();
            assert!(matches!(
                first.definition.fields[0].value,
                factions::Value::Flags {
                    flags_1: 255,
                    flags_2: None,
                    unused: None
                }
            ));
            assert!(matches!(
                first.definition.fields[1].value,
                factions::Value::UnusedFloat { bits: 0x7fc0_1234 }
            ));
            assert!(matches!(
                first.definition.fields[2].value,
                factions::Value::RankNumber { rank: i32::MIN }
            ));
            let values: Vec<_> = first
                .relations
                .iter()
                .map(|row| {
                    assert!(row.evaluated_modifier.is_none() && row.evaluated_reaction.is_none());
                    assert!(std::ptr::eq(
                        row.field,
                        &first.definition.fields[row.faction_field_index]
                    ));
                    let factions::Value::Relation {
                        modifier,
                        group_combat_reaction,
                        faction,
                        schema_kind_allowed,
                    } = &row.field.value
                    else {
                        panic!("relation")
                    };
                    (
                        *modifier,
                        *group_combat_reaction,
                        faction.status,
                        *schema_kind_allowed,
                    )
                })
                .collect();
            assert_eq!(
                values,
                [
                    (i32::MIN, u32::MAX, inventory::Status::Defined, Some(true)),
                    (i32::MAX, 0, inventory::Status::Defined, Some(true)),
                    (-1, 1, inventory::Status::Null, None),
                    (2, 2, inventory::Status::Missing, None),
                    (3, 3, inventory::Status::Deleted, Some(true)),
                    (4, 4, inventory::Status::Defined, Some(false)),
                    (5, 2, inventory::Status::Defined, Some(true))
                ]
            );
            assert_eq!(first.relations[1].field.kind, *b"XNAM");
            let factions::Value::Relation { faction, .. } = &first.relations[1].field.value else {
                unreachable!()
            };
            assert_eq!(faction.target.as_ref().unwrap().kind, *b"RACE");
            assert!(std::ptr::eq(
                first.definition,
                observation.factions[2].faction.as_ref().unwrap().definition
            ));
            assert_eq!(before, world.snapshot());
            retain(directory.path(), world, &observation);
        },
    );
}

#[test]
fn explicit_context_canonical_restore_and_campaign_rejection_do_not_infer_membership() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, factions, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                factions,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            let bytes = world
                .snapshot()
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap();
            let restored = World::restore(
                world.catalogue(),
                Snapshot::decode(&bytes, WorldLimits::default()).unwrap(),
                WorldLimits::default(),
            )
            .unwrap();
            assert_eq!(
                serde_json::to_value(
                    requests
                        .observe(world, content, Some(subject), Limits::default())
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(
                    requests
                        .observe(&restored, content, Some(subject), Limits::default())
                        .unwrap()
                )
                .unwrap()
            );
            let absent = requests
                .observe(world, content, None, Limits::default())
                .unwrap();
            assert!(
                absent.explicit_subject.is_none() && absent.factions[0].live_membership.is_none()
            );
            let other = World::new(world.catalogue(), WorldLimits::default()).unwrap();
            assert!(matches!(
                requests.observe(&other, content, None, Limits::default()),
                Err(Error::ContextChanged)
            ));
            let missing = fallout_runtime::identity::ReferenceId(999.try_into().unwrap());
            assert!(matches!(
                requests.observe(world, content, Some(missing), Limits::default()),
                Err(Error::State(fallout_runtime::Error::MissingReference))
            ));
        },
    );
}

#[test]
fn unavailable_configuration_and_template_flags_never_choose_effective_factions() {
    for (count, mask, issue) in [
        (0, 0, "missing_actor_configuration"),
        (2, 0, "ambiguous_actor_configuration"),
        (1, 4, "faction_template_selection_unsupported"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        fixture(directory.path(), count, mask);
        with_sources(
            directory.path(),
            |world, content, actors, links, factions, _| {
                let requests = Requests::prepare(
                    world,
                    actors,
                    links,
                    factions,
                    &form(0x100),
                    Limits::default(),
                )
                .unwrap();
                let observation = requests
                    .observe(world, content, None, Limits::default())
                    .unwrap();
                assert_eq!(observation.issues[0].code, issue);
                assert_eq!(observation.factions.len(), 7);
                assert!(
                    observation
                        .factions
                        .iter()
                        .all(|row| row.live_membership.is_none())
                );
                assert_eq!(observation.configuration.is_some(), count == 1);
                assert_eq!(observation.authored_configuration.is_some(), count == 1);
            },
        );
    }
}

#[test]
fn repeated_source_fields_relations_visits_and_projection_have_exact_aggregate_limits() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, factions, subject| {
            let requests = Requests::prepare(
                world,
                actors,
                links,
                factions,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            let observed = requests
                .observe(world, content, Some(subject), Limits::default())
                .unwrap();
            let bytes = serde_json::to_vec(&observed).unwrap().len();
            let exact = Limits {
                max_occurrences: 7,
                max_faction_fields: 25,
                max_relations: 15,
                max_visits: observed.preparation_visits,
                max_projection_bytes: bytes,
            };
            Requests::prepare(world, actors, links, factions, &form(0x100), exact)
                .unwrap()
                .observe(world, content, Some(subject), exact)
                .unwrap();
            for limits in [
                Limits {
                    max_occurrences: 6,
                    ..exact
                },
                Limits {
                    max_faction_fields: 24,
                    ..exact
                },
                Limits {
                    max_relations: 14,
                    ..exact
                },
                Limits {
                    max_visits: exact.max_visits - 1,
                    ..exact
                },
            ] {
                assert!(matches!(
                    Requests::prepare(world, actors, links, factions, &form(0x100), limits),
                    Err(Error::Capacity(_))
                ));
                assert!(matches!(
                    requests.observe(world, content, Some(subject), limits),
                    Err(Error::Capacity(_))
                ));
            }
            assert!(
                requests
                    .observe(
                        world,
                        content,
                        Some(subject),
                        Limits {
                            max_projection_bytes: bytes - 1,
                            ..exact
                        }
                    )
                    .is_err()
            );
            assert!(matches!(
                Requests::prepare(world, actors, links, factions, &form(0x888), exact),
                Err(Error::Source(_))
            ));
        },
    );
}

#[test]
fn changed_source_cohort_and_cross_source_content_fail_before_observation() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path(), 1, 0);
    with_sources(
        directory.path(),
        |world, content, actors, links, factions, _| {
            let other_directory = tempfile::tempdir().unwrap();
            fixture(other_directory.path(), 1, 4);
            let mut other_store = RecordStore::open_nv_headers(
                other_directory.path(),
                &["FalloutNV.esm".into()],
                plugin::Limits::default(),
            )
            .unwrap();
            let other_scripts = loaded_scripts::Catalogue::load(
                &mut other_store,
                Default::default(),
                |_, _| Ok(()),
            )
            .unwrap();
            let other_world = World::new(&other_scripts, WorldLimits::default()).unwrap();
            let other_content = Content::load(&mut other_store, &other_scripts, 100).unwrap();
            let other_factions =
                factions::Catalogue::load(&mut other_store, Default::default()).unwrap();
            assert!(matches!(
                Requests::prepare(
                    &other_world,
                    actors,
                    links,
                    factions,
                    &form(0x100),
                    Limits::default()
                ),
                Err(Error::ContextChanged)
            ));
            assert!(matches!(
                Requests::prepare(
                    world,
                    actors,
                    links,
                    &other_factions,
                    &form(0x100),
                    Limits::default()
                ),
                Err(Error::ContextChanged)
            ));
            let requests = Requests::prepare(
                world,
                actors,
                links,
                factions,
                &form(0x100),
                Limits::default(),
            )
            .unwrap();
            assert!(matches!(
                requests.observe(world, &other_content, None, Limits::default()),
                Err(Error::Content(_))
            ));
            assert!(
                requests
                    .observe(world, content, None, Limits::default())
                    .is_ok()
            );
        },
    );
}
