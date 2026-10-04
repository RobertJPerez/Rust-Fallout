use super::*;
use crate::{
    Limits,
    events::{Clocks, Context, Trigger},
    identity::Value,
    reference_state::{Pose, State},
    save::{Captured, Recovery, Repository, SaveWorker, format},
    snapshot::{ReferenceState, Snapshot},
};
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore, world};
use std::{fs, num::NonZeroU64, path::Path, process::Command, sync::Arc};

// Author private fixture bytes; all decoding below uses existing production
// source readers. No parser or alternate source identity authority lives here.
fn form(local_id: u32) -> fallout_data::identity::FormKey {
    fallout_data::identity::FormKey {
        profile: fallout_data::identity::ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id,
    }
}
fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
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
    assert!(masters.is_empty());
    record(
        b"TES4",
        0,
        0,
        &field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    )
}
fn unit(variables: &[(u32, u8)], references: &[(&[u8; 4], u32)]) -> Vec<u8> {
    assert!(references.is_empty());
    let compiled = [0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0];
    let mut schema = [0; 20];
    schema[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    schema[12..16].copy_from_slice(&(variables.len() as u32).to_le_bytes());
    let mut body = field(b"SCHR", &schema);
    body.extend(field(b"SCDA", &compiled));
    for (index, kind) in variables {
        let mut declaration = [0; 24];
        declaration[..4].copy_from_slice(&index.to_le_bytes());
        declaration[16] = *kind;
        body.extend(field(b"SLSD", &declaration));
        body.extend(field(b"SCVR", format!("local_{index}\0").as_bytes()));
    }
    body
}
fn definition(catalogue: &Catalogue) -> Handle {
    catalogue.iter().next().unwrap().1.handle().clone()
}

fn reference(n: u64) -> ReferenceId {
    ReferenceId(NonZeroU64::new(n).unwrap())
}
fn instance(n: u64) -> InstanceId {
    InstanceId(NonZeroU64::new(n).unwrap())
}
fn fragment(n: u64) -> Owner {
    Owner::Fragment {
        activation: NonZeroU64::new(n).unwrap(),
    }
}

fn fixture(root: &Path) {
    fs::create_dir_all(root).unwrap();
    let transform: Vec<_> = [
        0x3f800000_u32,
        0x80000000,
        1,
        0x3f000000,
        0xbf800000,
        0x40000000,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let placed = [
        field(b"NAME", &0x100_u32.to_le_bytes()),
        field(b"DATA", &transform),
    ]
    .concat();
    fs::write(
        root.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &unit(&[(42, 0)], &[])),
            record(b"SCPT", 0x301, 0, &unit(&[(42, 0)], &[])),
            record(b"ACTI", 0x100, 0, &[]),
            record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
            record(b"CELL", 0x402, 0, &field(b"DATA", &[1])),
            record(b"REFR", 0x500, 0, &placed),
            record(b"REFR", 0x501, 0, &placed),
        ]
        .concat(),
    )
    .unwrap();
}
fn source(root: &Path, names: &[&str]) -> (Catalogue, [State; 2]) {
    let mut store = RecordStore::open_nv_headers(
        root,
        &names.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    // Membership and enable are explicit caller choices; only source transforms
    // and source CELL/REFR bodies are decoded here by existing production code.
    let states = [(0x400, 0x500, false), (0x402, 0x501, true)].map(|(cell, placed, enabled)| {
        let cell_record = store.winner(&form(cell)).unwrap();
        world::decode_cell(&store.read(cell_record).unwrap(), "FalloutNV.esm").unwrap();
        let placed_record = store.winner(&form(placed)).unwrap();
        let placed =
            world::decode_placement(&store.read(placed_record).unwrap(), "FalloutNV.esm").unwrap();
        State::new(
            form(cell),
            Pose::from_source(&placed.transform.value, placed.scale.map(|s| s.value)).unwrap(),
            enabled,
        )
        .unwrap()
    });
    (catalogue, states)
}
fn host(catalogue: &Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x19; 16]).unwrap(),
    )
    .unwrap();
    world.register_reference(Some(form(0x500))).unwrap();
    world.register_reference(Some(form(0x501))).unwrap();
    let stage = world
        .stage_instance_initialization(
            &definition(catalogue),
            &Owner::Placed {
                reference: reference(1),
            },
            &Context::default(),
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
            super::super::initialization::Limits::default(),
        )
        .unwrap();
    let (_, handle) = world.commit_instance_initialization(stage).unwrap();
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
fn requirements(world: &World<'_>) -> HostRequirements {
    HostRequirements {
        campaign: world.campaign(),
        catalogue_sha256: world.catalogue_fingerprint().into(),
        reference_states: vec![reference(2), reference(1)],
        inventory_owners: vec![reference(1), reference(2)],
        instances: vec![InstanceRequirement {
            owner: Owner::Placed {
                reference: reference(1),
            },
            definition: definition(world.catalogue()),
            instance: instance(1),
        }],
        expected_journal_head: Some(JournalHead::Event {
            sequence: 1,
            instance: instance(1),
        }),
    }
}
fn identity_only(world: &World<'_>) -> HostRequirements {
    let mut request = requirements(world);
    request.reference_states.clear();
    request.inventory_owners.clear();
    request.instances.clear();
    request.expected_journal_head = None;
    request
}
fn fulfill(world: &mut World<'_>, states: &[State; 2]) {
    for (n, state) in states.iter().enumerate() {
        let id = reference(n as u64 + 1);
        let stage = world
            .stage_reference_state(&world.reference_view(id).unwrap(), state.clone())
            .unwrap();
        world.commit_reference_state(stage).unwrap();
        world.initialize_inventory(id).unwrap();
    }
}
fn missing() -> Vec<HostUnavailable> {
    vec![
        HostUnavailable::ReferenceState {
            index: 0,
            reference: reference(2),
            reason: ComponentUnavailable::Uninitialized,
        },
        HostUnavailable::ReferenceState {
            index: 1,
            reference: reference(1),
            reason: ComponentUnavailable::Uninitialized,
        },
        HostUnavailable::Inventory {
            index: 0,
            owner: reference(1),
            reason: ComponentUnavailable::Uninitialized,
        },
        HostUnavailable::Inventory {
            index: 1,
            owner: reference(2),
            reason: ComponentUnavailable::Uninitialized,
        },
    ]
}

#[test]
fn authored_inputs_remain_unavailable_until_explicit_mutations_and_empty_banks_are_available() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, states) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue);
    let request = requirements(&world);
    let before = world.snapshot();
    let schema = Arc::downgrade(world.definitions.values().next().unwrap());
    let retained = schema.strong_count();
    let report = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert_eq!(report.unavailable(), missing());
    assert_eq!(report.first_unavailable(), missing().first());
    assert_eq!(report.requested_requirements(), 6);
    assert_eq!(report.campaign(), before.campaign);
    assert_eq!(report.catalogue_fingerprint(), before.catalogue_sha256);
    assert_eq!(report.revision(), 4);
    assert!(!report.canonical_data_available());
    assert_eq!(world.snapshot(), before);
    assert_eq!(schema.strong_count(), retained);
    assert_eq!(world.block_count, 1);
    world.initialize_inventory(reference(1)).unwrap();
    assert!(
        world
            .inventory_items(reference(1))
            .unwrap()
            .next()
            .is_none()
    );
    assert!(world.inventory_items(reference(2)).is_err());
    let partial = world.snapshot();
    let checked = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    let mut expected_missing = missing();
    expected_missing.remove(2);
    assert_eq!(checked.unavailable(), expected_missing);
    assert_eq!(checked.unavailable().len(), 3);
    assert_eq!(world.snapshot(), partial);
    let stage = world
        .stage_reference_state(
            &world.reference_view(reference(1)).unwrap(),
            states[0].clone(),
        )
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    let stage = world
        .stage_reference_state(
            &world.reference_view(reference(2)).unwrap(),
            states[1].clone(),
        )
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    world.initialize_inventory(reference(2)).unwrap();
    let mut expected = before.clone();
    expected.state_revision += 4;
    expected.inventory_banks = vec![
        crate::inventory::Bank {
            owner: reference(1),
            items: vec![],
        },
        crate::inventory::Bank {
            owner: reference(2),
            items: vec![],
        },
    ];
    expected.reference_states = vec![
        ReferenceState {
            id: reference(1),
            state: states[0].clone(),
        },
        ReferenceState {
            id: reference(2),
            state: states[1].clone(),
        },
    ];
    assert_eq!(world.snapshot(), expected);
    let ready = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert!(ready.canonical_data_available());
    assert!(ready.first_unavailable().is_none());
    assert_eq!(ready.revision(), 8);
    assert_eq!(world.snapshot(), expected);
    let restored = World::restore(&catalogue, expected.clone(), Limits::default()).unwrap();
    assert_eq!(
        restored
            .check_host_requirements(&request, HostLimits::default())
            .unwrap(),
        ready
    );
    assert_eq!(restored.snapshot(), expected);
}

#[test]
fn missing_registry_and_each_owner_definition_identity_mismatch_are_distinct_and_read_only() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, _) = source(root.path(), &["FalloutNV.esm"]);
    let world = host(&catalogue);
    let before = world.snapshot();
    let mut request = identity_only(&world);
    request.reference_states = vec![reference(999), reference(1)];
    request.inventory_owners = vec![reference(999), reference(2)];
    let report = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert_eq!(
        report.unavailable(),
        vec![
            HostUnavailable::ReferenceState {
                index: 0,
                reference: reference(999),
                reason: ComponentUnavailable::MissingReference
            },
            HostUnavailable::ReferenceState {
                index: 1,
                reference: reference(1),
                reason: ComponentUnavailable::Uninitialized
            },
            HostUnavailable::Inventory {
                index: 0,
                owner: reference(999),
                reason: ComponentUnavailable::MissingReference
            },
            HostUnavailable::Inventory {
                index: 1,
                owner: reference(2),
                reason: ComponentUnavailable::Uninitialized
            },
        ]
    );
    let actual = requirements(&world).instances.remove(0);
    let mut wrong_id = actual.clone();
    wrong_id.instance = instance(99);
    let mut absent_owner = actual.clone();
    absent_owner.owner = fragment(3);
    let mut absent_reference = actual.clone();
    absent_reference.owner = Owner::Placed {
        reference: reference(999),
    };
    let mut absent_definition = absent_reference.clone();
    absent_definition.definition.version_sha256 = "f".repeat(64);
    let mut other_definition = actual.clone();
    other_definition.definition = catalogue
        .iter()
        .find(|(key, _)| key.record.local_id == 0x301)
        .unwrap()
        .1
        .handle()
        .clone();
    for (expected, reason) in [
        (
            wrong_id,
            InstanceUnavailable::InstanceMismatch {
                actual: instance(1),
            },
        ),
        (absent_owner, InstanceUnavailable::MissingOwner),
        (absent_reference, InstanceUnavailable::OwnerReferenceMissing),
        (
            absent_definition,
            InstanceUnavailable::DefinitionUnavailable,
        ),
        (
            other_definition,
            InstanceUnavailable::DefinitionMismatch {
                actual: actual.definition.clone(),
            },
        ),
    ] {
        let mut request = identity_only(&world);
        request.instances.push(expected.clone());
        assert_eq!(
            world
                .check_host_requirements(&request, HostLimits::default())
                .unwrap()
                .unavailable(),
            vec![HostUnavailable::Instance {
                index: 0,
                expected,
                reason
            }]
        );
        assert_eq!(world.snapshot(), before);
    }
    let mut request = identity_only(&world);
    request.campaign = CampaignId::from_bytes([0x29; 16]).unwrap();
    assert!(
        matches!(world.check_host_requirements(&request, HostLimits::default()), Err(Error::Invalid(reason)) if reason == "host requirements campaign changed")
    );
    request.campaign = world.campaign();
    request.catalogue_sha256 = "0".repeat(64);
    assert!(matches!(
        world.check_host_requirements(&request, HostLimits::default()),
        Err(Error::DefinitionChanged)
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn request_issue_and_copied_budgets_have_exact_boundaries_before_owned_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, _) = source(root.path(), &["FalloutNV.esm"]);
    let world = host(&catalogue);
    let before = world.snapshot();
    let request = requirements(&world);
    let report = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    let exact = HostLimits {
        max_requirements: 6,
        max_unavailable: 4,
        max_copied_bytes: report.charged_bytes(),
    };
    assert_eq!(
        world.check_host_requirements(&request, exact).unwrap(),
        report
    );
    for (limits, message) in [
        (
            HostLimits {
                max_requirements: 5,
                ..exact
            },
            "host requirements",
        ),
        (
            HostLimits {
                max_unavailable: 3,
                ..exact
            },
            "host unavailable inputs",
        ),
        (
            HostLimits {
                max_copied_bytes: exact.max_copied_bytes - 1,
                ..exact
            },
            "host readiness copied bytes",
        ),
    ] {
        assert!(
            matches!(world.check_host_requirements(&request, limits), Err(Error::Capacity(reason)) if reason == message)
        );
        assert_eq!(world.snapshot(), before);
    }
    let empty = identity_only(&world);
    let observed = world
        .check_host_requirements(
            &empty,
            HostLimits {
                max_requirements: 0,
                max_unavailable: 0,
                ..HostLimits::default()
            },
        )
        .unwrap();
    assert!(observed.canonical_data_available());
    assert_eq!(observed.requested_requirements(), 0);
    let mut huge = empty;
    let mut required = requirements(&world).instances.remove(0);
    required.owner = Owner::Quest {
        key: fallout_data::identity::FormKey {
            origin_plugin: "x".repeat(1024 * 1024),
            ..form(0x600)
        },
    };
    huge.instances.push(required);
    assert!(matches!(
        world.check_host_requirements(
            &huge,
            HostLimits {
                max_copied_bytes: 512,
                ..HostLimits::default()
            }
        ),
        Err(Error::Capacity("host readiness copied bytes"))
    ));
    assert_eq!(world.snapshot(), before);
}

#[test]
fn duplicate_and_malformed_requirements_refuse_without_query_side_effects() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, _) = source(root.path(), &["FalloutNV.esm"]);
    let world = host(&catalogue);
    let before = world.snapshot();
    let original = requirements(&world);
    let mut duplicate_ref = original.clone();
    duplicate_ref.reference_states.push(reference(1));
    let mut duplicate_bank = original.clone();
    duplicate_bank.inventory_owners.push(reference(1));
    let mut duplicate_owner = original.clone();
    let mut same_owner = original.instances[0].clone();
    same_owner.instance = instance(99);
    duplicate_owner.instances.push(same_owner);
    let mut duplicate_id = original.clone();
    let mut same_id = original.instances[0].clone();
    same_id.owner = fragment(99);
    duplicate_id.instances.push(same_id);
    let mut malformed_definition = original.clone();
    malformed_definition.instances[0].definition.version_sha256 = "X".repeat(64);
    let mut malformed_owner = original.clone();
    malformed_owner.instances[0].owner = Owner::Quest {
        key: fallout_data::identity::FormKey {
            origin_plugin: "FALLOUTNV.ESM".into(),
            ..form(0x600)
        },
    };
    let mut zero_head = original.clone();
    zero_head.expected_journal_head = Some(JournalHead::Event {
        sequence: 0,
        instance: instance(1),
    });
    for (request, message) in [
        (duplicate_ref, "duplicate reference-state requirement"),
        (duplicate_bank, "duplicate inventory requirement"),
        (duplicate_owner, "duplicate instance owner requirement"),
        (duplicate_id, "duplicate instance identity requirement"),
        (
            malformed_definition,
            "noncanonical required definition digest",
        ),
        (malformed_owner, "noncanonical NV form identity"),
        (zero_head, "zero required journal sequence"),
    ] {
        assert!(
            matches!(world.check_host_requirements(&request, HostLimits::default()), Err(Error::Invalid(reason)) if reason == message)
        );
        assert_eq!(world.snapshot(), before);
    }
    let mut zero_campaign = serde_json::to_value(&original).unwrap();
    zero_campaign["campaign"] = serde_json::json!(vec![0_u8; 16]);
    let request: HostRequirements = serde_json::from_value(zero_campaign).unwrap();
    assert!(
        matches!(world.check_host_requirements(&request, HostLimits::default()), Err(Error::Invalid(reason)) if reason == "zero campaign identity")
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn optional_journal_identity_never_acknowledges_and_observations_follow_new_revisions() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, _) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue);
    let mut request = identity_only(&world);
    request.expected_journal_head = Some(JournalHead::Event {
        sequence: 1,
        instance: instance(1),
    });
    let observation = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert!(observation.canonical_data_available());
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 1,
            menu_nanoseconds: 0,
            real_nanoseconds: 1,
        })
        .unwrap();
    world
        .enqueue(
            world.handle(instance(1)).unwrap(),
            Trigger::ObjectEvent { mask: 0x80000000 },
            Context::default(),
        )
        .unwrap();
    let before = world.snapshot();
    let checked = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert!(checked.canonical_data_available());
    assert_eq!(observation.revision(), 4);
    assert_eq!(checked.revision(), 6);
    assert_eq!(world.snapshot(), before);
    world.acknowledge(1).unwrap();
    let before = world.snapshot();
    let expected = request.expected_journal_head.clone().unwrap();
    assert_eq!(
        world
            .check_host_requirements(&request, HostLimits::default())
            .unwrap()
            .unavailable(),
        vec![HostUnavailable::JournalHead {
            expected,
            actual: JournalHead::Event {
                sequence: 2,
                instance: instance(1)
            }
        }]
    );
    request.expected_journal_head = None;
    assert!(
        world
            .check_host_requirements(&request, HostLimits::default())
            .unwrap()
            .canonical_data_available()
    );
    request.expected_journal_head = Some(JournalHead::Empty);
    assert!(
        !world
            .check_host_requirements(&request, HostLimits::default())
            .unwrap()
            .canonical_data_available()
    );
    assert_eq!(world.snapshot(), before);
    world.acknowledge(2).unwrap();
    let before = world.snapshot();
    assert!(
        world
            .check_host_requirements(&request, HostLimits::default())
            .unwrap()
            .canonical_data_available()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn equal_restore_has_same_observation_without_mutation_authority_even_at_revision_exhaustion() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, states) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue);
    fulfill(&mut world, &states);
    let request = requirements(&world);
    let report = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    let stage = world
        .stage_reference_state(
            &world.reference_view(reference(1)).unwrap(),
            states[0].clone(),
        )
        .unwrap();
    let mut restored = World::restore(&catalogue, world.snapshot(), Limits::default()).unwrap();
    assert_eq!(
        restored
            .check_host_requirements(&request, HostLimits::default())
            .unwrap(),
        report
    );
    assert!(matches!(
        restored.commit_reference_state(stage),
        Err(Error::StaleHandle)
    ));
    let mut snapshot = restored.snapshot();
    snapshot.state_revision = u64::MAX;
    let exhausted = World::restore(&catalogue, snapshot.clone(), Limits::default()).unwrap();
    let report = exhausted
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert!(report.canonical_data_available());
    assert_eq!(report.revision(), u64::MAX);
    assert_eq!(exhausted.snapshot(), snapshot);
}

#[test]
fn source_selected_host_missing_and_completed_inputs_survive_native_worker_and_fresh_restore() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_HOST_REQUIREMENTS_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fixture(root);
    let original_source = fs::read(root.join("FalloutNV.esm")).unwrap();
    let (catalogue, states) = source(root, &["FalloutNV.esm"]);
    let mut world = host(&catalogue);
    let request = requirements(&world);
    let before = world.snapshot();
    let unavailable = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert_eq!(unavailable.unavailable(), missing());
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    fulfill(&mut world, &states);
    let after = world.snapshot();
    let mut expected = before.clone();
    expected.state_revision += 4;
    expected.inventory_banks = vec![
        crate::inventory::Bank {
            owner: reference(1),
            items: vec![],
        },
        crate::inventory::Bank {
            owner: reference(2),
            items: vec![],
        },
    ];
    expected.reference_states = vec![
        ReferenceState {
            id: reference(1),
            state: states[0].clone(),
        },
        ReferenceState {
            id: reference(2),
            state: states[1].clone(),
        },
    ];
    assert_eq!(after, expected);
    let ready = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert!(ready.canonical_data_available());
    assert_eq!(world.snapshot(), after);
    assert_eq!(first.wait().unwrap().metadata.generation, 1);
    let second = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    world
        .advance_clocks(Clocks {
            tick: 1,
            game_nanoseconds: 1,
            menu_nanoseconds: 0,
            real_nanoseconds: 1,
        })
        .unwrap();
    worker.finish().unwrap();
    assert_eq!(second.wait().unwrap().metadata.generation, 2);
    assert_eq!(
        format::decode(
            &fs::read(root.join("saved/previous.frsv")).unwrap(),
            Limits::default()
        )
        .unwrap()
        .snapshot,
        before
    );
    assert_eq!(
        format::decode(
            &fs::read(root.join("saved/current.frsv")).unwrap(),
            Limits::default()
        )
        .unwrap()
        .snapshot,
        after
    );
    fs::write(
        root.join("before.snapshot.json"),
        before.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("after.snapshot.json"),
        after.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("requirements.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("before.readiness.json"),
        serde_json::to_vec_pretty(&unavailable).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("after.readiness.json"),
        serde_json::to_vec_pretty(&ready).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::readiness::tests::cold_host_requirements_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_HOST_REQUIREMENTS_COLD_ROOT", root)
        .output()
        .unwrap();
    fs::write(root.join("cold.stdout.txt"), &child.stdout).unwrap();
    fs::write(root.join("cold.stderr.txt"), &child.stderr).unwrap();
    assert!(
        child.status.success(),
        "{}{}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
    assert_eq!(
        fs::read(root.join("FalloutNV.esm")).unwrap(),
        original_source
    );
}

#[test]
#[ignore = "fresh source-bound host consumer invoked by its parent"]
fn cold_host_requirements_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_HOST_REQUIREMENTS_COLD_ROOT").unwrap());
    let (catalogue, _) = source(&root, &["FalloutNV.esm"]);
    let request: HostRequirements =
        serde_json::from_slice(&fs::read(root.join("requirements.json")).unwrap()).unwrap();
    let before = Snapshot::decode(
        &fs::read(root.join("before.snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let expected = Snapshot::decode(
        &fs::read(root.join("after.snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let unavailable = World::restore(&catalogue, before.clone(), Limits::default()).unwrap();
    assert_eq!(
        unavailable
            .check_host_requirements(&request, HostLimits::default())
            .unwrap()
            .unavailable(),
        missing()
    );
    assert_eq!(unavailable.snapshot(), before);
    let repository = Repository::open(&root.join("saved"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    assert_eq!(world.snapshot(), expected);
    let ready = world
        .check_host_requirements(&request, HostLimits::default())
        .unwrap();
    assert!(ready.canonical_data_available());
    assert_eq!(world.snapshot(), expected);
    fs::write(
        root.join("cold.snapshot.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("cold.readiness.json"),
        serde_json::to_vec_pretty(&ready).unwrap(),
    )
    .unwrap();
    fs::write(root.join("Other.esm"), header(&[])).unwrap();
    let (changed, _) = source(&root, &["FalloutNV.esm", "Other.esm"]);
    let changed_world =
        World::with_campaign(&changed, Limits::default(), request.campaign).unwrap();
    assert!(matches!(
        changed_world.check_host_requirements(&request, HostLimits::default()),
        Err(Error::DefinitionChanged)
    ));
    assert!(
        repository
            .load(&changed, Limits::default(), Recovery::Strict)
            .is_err()
    );
    println!(
        "Native cold host reports exact missing versus completed canonical inputs; source mismatch refuses without executing or acknowledging"
    );
}
