mod common;
use common::*;
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore, world};
use fallout_runtime::{
    Error, Limits, World,
    foreign::{Content, Failure as ContentFailure},
    identity::CampaignId,
    reference_state::{
        Pose, SourceReferenceFailure as Failure, SourceReferenceLimits as AdmissionLimits,
        SourceReferenceOutcome as Outcome, SourceReferenceReceipt,
        SourceReferenceRequest as Request, State,
    },
    save::{Captured, Recovery, Repository, SaveStatus, SaveWorker, format},
    snapshot::Snapshot,
};
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn fixture(root: &Path) {
    fs::create_dir_all(root).unwrap();
    let data = [
        0x3f800000_u32,
        0x80000000,
        1,
        0x3f000000,
        0xbf800000,
        0x40000000,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect::<Vec<_>>();
    let placed = [
        field(b"NAME", &0x100_u32.to_le_bytes()),
        field(b"DATA", &data),
    ]
    .concat();
    fs::write(
        root.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &unit(&[(42, 0)], &[])),
            record(b"ACTI", 0x100, 0, &[]),
            record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
            record(b"CELL", 0x401, plugin::DELETED, &field(b"DATA", &[1])),
            record(b"CELL", 0x402, 0, &field(b"DATA", &[1])),
            record(b"REFR", 0x500, 0, &placed),
            record(b"ACHR", 0x501, 0, &placed),
            record(b"ACRE", 0x502, 0, &placed),
            record(b"REFR", 0x503, plugin::DELETED, &placed),
        ]
        .concat(),
    )
    .unwrap();
}
fn source(root: &Path, order: &[&str]) -> (Catalogue, Content, Pose) {
    let mut store = RecordStore::open_nv_headers(
        root,
        &order.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap();
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    for cell in [0x400, 0x402] {
        let cell = store.winner(&form(cell)).unwrap();
        world::decode_cell(&store.read(cell).unwrap(), "FalloutNV.esm").unwrap();
    }
    let placed = store.winner(&form(0x500)).unwrap();
    let placed = world::decode_placement(&store.read(placed).unwrap(), "FalloutNV.esm").unwrap();
    let pose = Pose::from_source(&placed.transform.value, placed.scale.map(|s| s.value)).unwrap();
    (catalogue, content, pose)
}
fn host(catalogue: &Catalogue, limits: Limits) -> World<'_> {
    World::with_campaign(
        catalogue,
        limits,
        CampaignId::from_bytes([0x17; 16]).unwrap(),
    )
    .unwrap()
}

// Actual source-selected headless residency consumer. Source pose is decoded by
// the shared world decoder above; membership/enable are explicit host choices.
fn select(world: &mut World<'_>, content: &Content, pose: &Pose) -> SourceReferenceReceipt {
    let stage = world
        .stage_source_reference(
            Request {
                authored: &form(0x500),
                allowed_kinds: &[*b"REFR"],
                cell: &form(0x400),
                pose,
                enabled: true,
            },
            content,
            AdmissionLimits::default(),
        )
        .unwrap();
    world.commit_source_reference(stage).unwrap()
}
fn changed() -> State {
    State::new(
        form(0x402),
        Pose::from_source(
            &world::Transform {
                position: [8192.25, -0.0, -30.5],
                rotation: [0.125, -0.75, 1.5],
            },
            None,
        )
        .unwrap(),
        false,
    )
    .unwrap()
}

#[test]
fn actual_source_consumer_initializes_once_reuses_changed_state_and_never_reserves_on_stage() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content, pose) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue, Limits::default());
    let before = world.snapshot();
    let stage = world
        .stage_source_reference(
            Request {
                authored: &form(0x500),
                allowed_kinds: &[*b"REFR"],
                cell: &form(0x400),
                pose: &pose,
                enabled: true,
            },
            &content,
            AdmissionLimits::default(),
        )
        .unwrap();
    assert_eq!(stage.authored(), &form(0x500));
    assert!(stage.existing().is_none());
    assert_eq!(stage.requested_state().pose(), &pose);
    assert_eq!(world.snapshot(), before);
    drop(stage);
    assert_eq!(world.snapshot(), before);
    let receipt = select(&mut world, &content, &pose);
    let id = receipt.view().reference();
    assert_eq!(receipt.outcome(), Outcome::Created);
    assert_eq!(receipt.before_revision(), before.state_revision);
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    assert_eq!(id.0.get(), before.next_reference);
    assert_eq!(receipt.view().authored(), Some(&form(0x500)));
    assert_eq!(receipt.placed_source().kind, *b"REFR");
    assert_eq!(receipt.cell_source().kind, *b"CELL");
    assert_eq!(receipt.requested_cell(), &form(0x400));
    assert_eq!(receipt.view().state().unwrap().pose(), &pose);
    assert_eq!(
        pose.source_transform().position.map(f32::to_bits),
        [0x3f800000, 0x80000000, 1]
    );
    assert!(pose.source_scale().is_none());
    let created = world.snapshot();
    assert_eq!(created.next_reference, before.next_reference + 1);
    assert_eq!(created.references.len(), 1);
    assert_eq!(created.reference_states.len(), 1);
    let stage = world
        .stage_reference_state(&world.reference_view(id).unwrap(), changed())
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    let exact = world.snapshot();
    let reused = select(&mut world, &content, &pose);
    assert_eq!(reused.outcome(), Outcome::Reused);
    assert_eq!(reused.view().reference(), id);
    assert_eq!(reused.before_revision(), exact.state_revision);
    assert_eq!(reused.after_revision(), exact.state_revision);
    assert_eq!(reused.view().state(), Some(&changed()));
    assert_eq!(reused.requested_cell(), &form(0x400));
    assert_eq!(reused.view().state().unwrap().cell(), &form(0x402));
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn registered_unavailable_component_is_reported_without_inferred_initialization() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content, pose) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue, Limits::default());
    let id = world.register_reference(Some(form(0x500))).unwrap();
    let exact = world.snapshot();
    let receipt = select(&mut world, &content, &pose);
    assert_eq!(receipt.outcome(), Outcome::Reused);
    assert_eq!(receipt.view().reference(), id);
    assert!(receipt.view().state().is_none());
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn missing_deleted_wrong_kind_cell_and_invalid_pose_refuse_before_identity_or_revision() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content, pose) = source(root.path(), &["FalloutNV.esm"]);
    let world = host(&catalogue, Limits::default());
    let exact = world.snapshot();
    for (key, code) in [(0x999, "missing_form"), (0x503, "deleted_form")] {
        let error = world
            .stage_source_reference(
                Request {
                    authored: &form(key),
                    allowed_kinds: &[*b"REFR"],
                    cell: &form(0x400),
                    pose: &pose,
                    enabled: false,
                },
                &content,
                AdmissionLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(error, Failure::Content(ref failure) if failure.code() == code));
        assert_eq!(world.snapshot(), exact);
    }
    for key in [0x100, 0x501, 0x502] {
        assert!(matches!(
            world.stage_source_reference(
                Request {
                    authored: &form(key),
                    allowed_kinds: &[*b"REFR"],
                    cell: &form(0x400),
                    pose: &pose,
                    enabled: false
                },
                &content,
                AdmissionLimits::default()
            ),
            Err(Failure::ReferenceKind(_))
        ));
        assert_eq!(world.snapshot(), exact);
    }
    for (key, code) in [(0x999, "missing_form"), (0x401, "deleted_form")] {
        let error = world
            .stage_source_reference(
                Request {
                    authored: &form(0x500),
                    allowed_kinds: &[*b"REFR"],
                    cell: &form(key),
                    pose: &pose,
                    enabled: false,
                },
                &content,
                AdmissionLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(error, Failure::Content(ref failure) if failure.code() == code));
        assert_eq!(world.snapshot(), exact);
    }
    assert!(
        matches!(world.stage_source_reference(Request { authored: &form(0x500), allowed_kinds: &[*b"REFR"], cell: &form(0x100), pose: &pose, enabled: true }, &content, AdmissionLimits::default()), Err(Failure::CellKind(kind)) if kind == *b"ACTI")
    );
    let mut forged = serde_json::to_value(&pose).unwrap();
    forged["position_bits"][0] = json!(0x7f800000_u32);
    let invalid: Pose = serde_json::from_value(forged).unwrap();
    assert!(matches!(
        world.stage_source_reference(
            Request {
                authored: &form(0x500),
                allowed_kinds: &[*b"REFR"],
                cell: &form(0x400),
                pose: &invalid,
                enabled: true
            },
            &content,
            AdmissionLimits::default()
        ),
        Err(Failure::State(Error::Invalid(_)))
    ));
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn explicit_kind_set_and_owned_copy_limits_admit_exactly_before_clone() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content, pose) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue, Limits::default());
    let before = world.snapshot();
    for allowed in [&[][..], &[*b"REFR", *b"REFR"][..]] {
        assert!(
            world
                .stage_source_reference(
                    Request {
                        authored: &form(0x500),
                        allowed_kinds: allowed,
                        cell: &form(0x400),
                        pose: &pose,
                        enabled: true
                    },
                    &content,
                    AdmissionLimits::default()
                )
                .is_err()
        );
    }
    assert!(
        matches!(world.stage_source_reference(Request { authored: &form(0x500), allowed_kinds: &[*b"CELL"], cell: &form(0x400), pose: &pose, enabled: true }, &content, AdmissionLimits::default()), Err(Failure::InvalidAllowedKind(kind)) if kind == *b"CELL")
    );
    assert!(matches!(
        world.stage_source_reference(
            Request {
                authored: &form(0x500),
                allowed_kinds: &[*b"REFR"],
                cell: &form(0x400),
                pose: &pose,
                enabled: true
            },
            &content,
            AdmissionLimits {
                max_allowed_kinds: 0,
                ..AdmissionLimits::default()
            }
        ),
        Err(Failure::State(Error::Capacity("source reference kinds")))
    ));
    let request = Request {
        authored: &form(0x500),
        allowed_kinds: &[*b"REFR"],
        cell: &form(0x400),
        pose: &pose,
        enabled: true,
    };
    let exact = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap()
        .charged_bytes();
    assert!(matches!(
        world.stage_source_reference(
            request,
            &content,
            AdmissionLimits {
                max_copied_bytes: exact - 1,
                ..AdmissionLimits::default()
            }
        ),
        Err(Failure::State(Error::Capacity(
            "source reference copied bytes"
        )))
    ));
    let stage = world
        .stage_source_reference(
            request,
            &content,
            AdmissionLimits {
                max_copied_bytes: exact,
                ..AdmissionLimits::default()
            },
        )
        .unwrap();
    assert_eq!(stage.charged_bytes(), exact);
    drop(stage);
    assert_eq!(world.snapshot(), before);
    for (key, kind) in [(0x500, *b"REFR"), (0x501, *b"ACHR"), (0x502, *b"ACRE")] {
        let stage = world
            .stage_source_reference(
                Request {
                    authored: &form(key),
                    allowed_kinds: &[kind],
                    cell: &form(0x400),
                    pose: &pose,
                    enabled: false,
                },
                &content,
                AdmissionLimits::default(),
            )
            .unwrap();
        let receipt = world.commit_source_reference(stage).unwrap();
        assert_eq!(receipt.placed_source().kind, kind);
    }
    assert_eq!(world.reference_count(), 3);
}

#[test]
fn admission_is_stale_after_any_mutation_restore_other_campaign_or_changed_source() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content, pose) = source(root.path(), &["FalloutNV.esm"]);
    let mut world = host(&catalogue, Limits::default());
    let request = Request {
        authored: &form(0x500),
        allowed_kinds: &[*b"REFR"],
        cell: &form(0x400),
        pose: &pose,
        enabled: true,
    };
    let stale = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    world.register_reference(None).unwrap();
    let exact = world.snapshot();
    assert!(world.commit_source_reference(stale).is_err());
    assert_eq!(world.snapshot(), exact);
    let a = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    let b = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    world.commit_source_reference(a).unwrap();
    let exact = world.snapshot();
    assert!(world.commit_source_reference(b).is_err());
    assert_eq!(world.snapshot(), exact);
    let stage = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    world.replace_from_snapshot(exact.clone()).unwrap();
    assert!(matches!(
        world.commit_source_reference(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), exact);
    let mut other_campaign = exact.clone();
    other_campaign.campaign = CampaignId::from_bytes([0x18; 16]).unwrap();
    let mut other = World::restore(&catalogue, other_campaign.clone(), Limits::default()).unwrap();
    let stage = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    assert!(matches!(
        other.commit_source_reference(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), other_campaign);
    fs::write(root.path().join("Other.esm"), header(&[])).unwrap();
    let (changed_catalogue, changed_content, _) =
        source(root.path(), &["FalloutNV.esm", "Other.esm"]);
    let mut changed_world = host(&changed_catalogue, Limits::default());
    let before = changed_world.snapshot();
    assert!(matches!(
        changed_world.stage_source_reference(request, &content, AdmissionLimits::default()),
        Err(Failure::Content(ContentFailure::ContentChanged))
    ));
    assert!(matches!(
        world.stage_source_reference(request, &changed_content, AdmissionLimits::default()),
        Err(Failure::Content(ContentFailure::ContentChanged))
    ));
    let stage = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    assert!(matches!(
        changed_world.commit_source_reference(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(changed_world.snapshot(), before);
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn exhausted_id_revision_or_registry_cannot_leave_a_partial_reference() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, content, pose) = source(root.path(), &["FalloutNV.esm"]);
    let request = Request {
        authored: &form(0x500),
        allowed_kinds: &[*b"REFR"],
        cell: &form(0x400),
        pose: &pose,
        enabled: true,
    };
    for exhaust_id in [false, true] {
        let mut world = host(&catalogue, Limits::default());
        let mut exact = world.snapshot();
        if exhaust_id {
            exact.next_reference = u64::MAX;
        } else {
            exact.state_revision = u64::MAX;
        }
        world.replace_from_snapshot(exact.clone()).unwrap();
        let stage = world
            .stage_source_reference(request, &content, AdmissionLimits::default())
            .unwrap();
        let error = world.commit_source_reference(stage).unwrap_err();
        assert!(
            matches!(error, Error::Capacity(reason) if reason == if exhaust_id { "reference identities" } else { "state revisions" })
        );
        assert_eq!(world.snapshot(), exact);
    }
    let mut world = host(
        &catalogue,
        Limits {
            max_references: 1,
            ..Limits::default()
        },
    );
    world.register_reference(None).unwrap();
    let exact = world.snapshot();
    let stage = world
        .stage_source_reference(request, &content, AdmissionLimits::default())
        .unwrap();
    assert!(matches!(
        world.commit_source_reference(stage),
        Err(Error::Capacity("live references"))
    ));
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn source_admitted_native_capture_cold_reload_retains_changed_state_and_refuses_other_source() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_SOURCE_REFERENCE_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fixture(root);
    let source_bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    let (catalogue, content, pose) = source(root, &["FalloutNV.esm"]);
    let mut world = host(&catalogue, Limits::default());
    let created = select(&mut world, &content, &pose);
    let id = created.view().reference();
    let first = world.snapshot();
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let pending = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    world
        .commit_reference_state(
            world
                .stage_reference_state(&world.reference_view(id).unwrap(), changed())
                .unwrap(),
        )
        .unwrap();
    let exact = world.snapshot();
    let reused = select(&mut world, &content, &pose);
    assert_eq!(reused.outcome(), Outcome::Reused);
    assert_eq!(world.snapshot(), exact);
    assert_eq!(pending.wait().unwrap().metadata.generation, 1);
    let pending = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    worker.finish().unwrap();
    assert_eq!(pending.wait().unwrap().metadata.generation, 2);
    assert_eq!(
        format::decode(
            &fs::read(root.join("saved/previous.frsv")).unwrap(),
            Limits::default()
        )
        .unwrap()
        .snapshot,
        first
    );
    fs::write(
        root.join("expected.snapshot.json"),
        exact.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("created.receipt.json"),
        serde_json::to_vec_pretty(&created).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    let result = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_source_reference_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_SOURCE_REFERENCE_COLD_ROOT", root)
        .output()
        .unwrap();
    fs::write(root.join("cold.stdout.txt"), &result.stdout).unwrap();
    fs::write(root.join("cold.stderr.txt"), &result.stderr).unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(root.join("FalloutNV.esm")).unwrap(), source_bytes);
}

#[test]
#[ignore = "fresh child source consumer selected explicitly by the parent"]
fn cold_source_reference_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_SOURCE_REFERENCE_COLD_ROOT").unwrap());
    let (catalogue, content, pose) = source(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join("saved"), &[]).unwrap();
    let (mut world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    let expected = Snapshot::decode(
        &fs::read(root.join("expected.snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    let reused = select(&mut world, &content, &pose);
    assert_eq!(reused.outcome(), Outcome::Reused);
    assert_eq!(reused.view().state(), Some(&changed()));
    assert_eq!(world.snapshot(), expected);
    fs::write(
        root.join("cold.snapshot.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(root.join("Other.esm"), header(&[])).unwrap();
    let (changed_catalogue, _, _) = source(&root, &["FalloutNV.esm", "Other.esm"]);
    assert!(
        repository
            .load(&changed_catalogue, Limits::default(), Recovery::Strict)
            .is_err()
    );
    println!(
        "source-kind-admitted atomic identity/component survives native cold restore; source consumer reuses exact changed pose/enable without reset; changed whole source cohort refuses"
    );
}
