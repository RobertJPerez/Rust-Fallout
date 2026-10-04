mod common;
use common::*;
use fallout_data::{
    identity::FormKey, loaded_scripts::Catalogue, plugin, store::RecordStore, world,
};
use fallout_runtime::{
    Error, Limits, World,
    identity::{CampaignId, ReferenceId},
    reference_state::{PageLimits, PageRequest, Pose, State, View},
    save::{Captured, Recovery, Repository, SaveStatus, SaveWorker},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn fixture(root: &Path) {
    fs::create_dir_all(root).unwrap();
    let mut bytes = header(&[]);
    for cell in 0x400..=0x402 {
        bytes.extend(record(b"CELL", cell, 0, &field(b"DATA", &[1])));
    }
    bytes.extend(record(b"ACTI", 0x100, 0, &[]));
    bytes.extend(record(b"SCPT", 0x300, 0, &unit(&[(42, 0)], &[])));
    for index in 0..9 {
        let transform = [
            (index as f32 + 1.0).to_bits(),
            0x80000000,
            1,
            0x3f000000,
            0xbf800000,
            0x40000000,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
        let mut body = field(b"NAME", &0x100_u32.to_le_bytes());
        body.extend(field(b"DATA", &transform));
        if index % 2 == 1 {
            body.extend(field(b"XSCL", &0.75_f32.to_le_bytes()));
        }
        bytes.extend(record(b"REFR", 0x500 + index, 0, &body));
    }
    fs::write(root.join("FalloutNV.esm"), bytes).unwrap();
}

fn source(root: &Path) -> (Catalogue, Vec<Pose>) {
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue = Catalogue::load(
        &mut store,
        fallout_data::loaded_scripts::Limits::default(),
        |_, _| Ok(()),
    )
    .unwrap();
    for cell in 0x400..=0x402 {
        let handle = store.winner(&form(cell)).unwrap();
        world::decode_cell(&store.read(handle).unwrap(), "FalloutNV.esm").unwrap();
    }
    let poses = (0x500..0x509)
        .map(|id| {
            let handle = store.winner(&form(id)).unwrap();
            let placement =
                world::decode_placement(&store.read(handle).unwrap(), "FalloutNV.esm").unwrap();
            Pose::from_source(&placement.transform.value, placement.scale.map(|s| s.value)).unwrap()
        })
        .collect();
    (catalogue, poses)
}

fn seeded<'a>(catalogue: &'a Catalogue, poses: &[Pose]) -> (World<'a>, Vec<ReferenceId>) {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x15; 16]).unwrap(),
    )
    .unwrap();
    let mut ids = Vec::new();
    for (index, pose) in poses.iter().enumerate() {
        let id = world
            .register_reference(Some(form(0x500 + index as u32)))
            .unwrap();
        ids.push(id);
        if index < 8 {
            // Membership and enable are explicit harness inputs. The runtime
            // never infers them from record order, source groups or missing state.
            let state =
                State::new(form(0x400 + index as u32 % 2), pose.clone(), index % 3 != 0).unwrap();
            let stage = world
                .stage_reference_state(&world.reference_view(id).unwrap(), state)
                .unwrap();
            world.commit_reference_state(stage).unwrap();
        }
    }
    ids.push(world.register_reference(None).unwrap());
    (world, ids)
}

fn small() -> PageLimits {
    PageLimits {
        max_visited: 3,
        max_rows: 2,
        ..PageLimits::default()
    }
}

// Actual headless scene reconciliation: only pages and immutable observations
// cross this boundary. It never copies a World snapshot to discover references.
fn reconcile(world: &World<'_>, cell: Option<&FormKey>, limits: PageLimits) -> Vec<View> {
    let mut cursor = None;
    let mut result: Vec<View> = Vec::new();
    for _ in 0..=world.reference_count() {
        let page = world
            .reference_state_page(
                PageRequest {
                    cell,
                    after: cursor.as_ref(),
                },
                limits,
            )
            .unwrap();
        assert_eq!(page.campaign(), world.campaign());
        assert_eq!(page.catalogue_fingerprint(), world.catalogue_fingerprint());
        assert_eq!(page.cell(), cell);
        let usage = page.usage();
        assert!(usage.visited <= limits.max_visited);
        assert!(usage.returned <= limits.max_rows);
        assert!(usage.charged_bytes <= limits.max_copied_bytes);
        assert_eq!(usage.returned, page.rows().len());
        for row in page.rows() {
            assert_eq!(row.revision(), page.revision());
            assert_eq!(row.campaign(), page.campaign());
            assert_eq!(row.catalogue_fingerprint(), page.catalogue_fingerprint());
            if let Some(previous) = result.last() {
                assert!(previous.reference() < row.reference());
            }
            result.push(row.clone());
        }
        let (_, next) = page.into_parts();
        if next.is_none() {
            return result;
        }
        assert!(
            usage.visited > 0,
            "a continuation must make bounded progress"
        );
        cursor = next;
    }
    panic!("page cursor did not finish the stable registry");
}

fn observations(world: &World<'_>) -> Value {
    json!({
        "all": reconcile(world, None, small()),
        "cell_a": reconcile(world, Some(&form(0x400)), small()),
        "cell_b": reconcile(world, Some(&form(0x401)), small()),
        "empty_cell": reconcile(world, Some(&form(0x402)), small()),
    })
}

fn changed() -> State {
    State::new(
        form(0x401),
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
fn authored_multi_cell_reconciliation_preserves_identity_unknowns_and_exact_source_bits() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (world, ids) = seeded(&catalogue, &poses);
    let before = world.snapshot();
    let all = reconcile(&world, None, small());
    assert_eq!(all.iter().map(View::reference).collect::<Vec<_>>(), ids);
    for (index, row) in all.iter().take(8).enumerate() {
        assert_eq!(row.authored(), Some(&form(0x500 + index as u32)));
        let state = row.state().unwrap();
        assert_eq!(state.pose(), &poses[index]);
        assert_eq!(
            state.pose().source_transform().position[1].to_bits(),
            0x80000000
        );
        assert_eq!(state.pose().source_transform().position[2].to_bits(), 1);
        assert_eq!(state.enabled(), index % 3 != 0);
        assert_eq!(
            state.pose().source_scale(),
            (index % 2 == 1).then_some(0.75)
        );
    }
    assert!(all[8].authored().is_some());
    assert!(all[8].state().is_none());
    assert!(all[9].authored().is_none());
    assert!(all[9].state().is_none());
    for (cell, expected) in [
        (0x400, vec![ids[0], ids[2], ids[4], ids[6]]),
        (0x401, vec![ids[1], ids[3], ids[5], ids[7]]),
    ] {
        assert_eq!(
            reconcile(&world, Some(&form(cell)), small())
                .iter()
                .map(View::reference)
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert_eq!(world.snapshot(), before);
    // The returned immutable observations keep their data after residency ends.
    drop(world);
    assert_eq!(all[0].state().unwrap().pose(), &poses[0]);
}

#[test]
fn zero_match_pages_advance_by_visited_limit_without_fabricating_missing_components() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (world, _) = seeded(&catalogue, &poses);
    let cell = form(0x402);
    let limits = PageLimits {
        max_visited: 3,
        max_rows: 1,
        ..PageLimits::default()
    };
    let mut cursor = None;
    for visited in [3, 3, 3, 1] {
        let page = world
            .reference_state_page(
                PageRequest {
                    cell: Some(&cell),
                    after: cursor.as_ref(),
                },
                limits,
            )
            .unwrap();
        assert!(page.rows().is_empty());
        assert_eq!(page.usage().visited, visited);
        let (_, next) = page.into_parts();
        assert_eq!(next.is_some(), visited == 3);
        cursor = next;
    }
    assert!(cursor.is_none());
}

#[test]
fn byte_admission_does_not_skip_the_unconsumed_matching_candidate() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (world, ids) = seeded(&catalogue, &poses);
    let first = world
        .reference_state_page(
            PageRequest::default(),
            PageLimits {
                max_rows: 1,
                ..PageLimits::default()
            },
        )
        .unwrap();
    let exact = first.usage().charged_bytes;
    let too_small = PageLimits {
        max_copied_bytes: exact - 1,
        ..PageLimits::default()
    };
    assert!(matches!(
        world.reference_state_page(PageRequest::default(), too_small),
        Err(Error::Capacity("reference page copied bytes"))
    ));
    let bounded = PageLimits {
        max_copied_bytes: exact,
        max_rows: usize::MAX,
        max_visited: usize::MAX,
    };
    let page = world
        .reference_state_page(PageRequest::default(), bounded)
        .unwrap();
    assert_eq!(page.rows().len(), 1);
    assert_eq!(page.rows()[0].reference(), ids[0]);
    assert_eq!(page.usage().visited, 2); // second inspected, deliberately not consumed
    assert_eq!(page.usage().charged_bytes, exact);
    let all = reconcile(&world, None, bounded);
    assert_eq!(all.iter().map(View::reference).collect::<Vec<_>>(), ids);
    // A filter may consume a nonmatch before discovering a row that cannot fit.
    let cell = form(0x401);
    let one_visit = world
        .reference_state_page(
            PageRequest {
                cell: Some(&cell),
                after: None,
            },
            PageLimits {
                max_visited: 1,
                ..PageLimits::default()
            },
        )
        .unwrap();
    let metadata_only = PageLimits {
        max_copied_bytes: one_visit.usage().charged_bytes,
        ..PageLimits::default()
    };
    let zero = world
        .reference_state_page(
            PageRequest {
                cell: Some(&cell),
                after: None,
            },
            metadata_only,
        )
        .unwrap();
    assert_eq!(zero.usage().visited, 2);
    assert!(zero.rows().is_empty());
    assert!(matches!(
        world.reference_state_page(
            PageRequest {
                cell: Some(&cell),
                after: zero.next_cursor()
            },
            metadata_only
        ),
        Err(Error::Capacity("reference page copied bytes"))
    ));
    let resumed = world
        .reference_state_page(
            PageRequest {
                cell: Some(&cell),
                after: zero.next_cursor(),
            },
            small(),
        )
        .unwrap();
    assert_eq!(resumed.rows()[0].reference(), ids[1]);
}

#[test]
fn mutation_restore_campaign_source_and_filter_invalidate_cursor_before_copy_admission() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (mut world, ids) = seeded(&catalogue, &poses);
    let first = world
        .reference_state_page(PageRequest::default(), small())
        .unwrap();
    let cursor = first.next_cursor().unwrap().clone();
    let no_copy = PageLimits {
        max_copied_bytes: 0,
        ..small()
    };
    let before = world.snapshot();
    assert!(
        matches!(world.reference_state_page(PageRequest { cell: Some(&form(0x400)), after: Some(&cursor) }, no_copy), Err(Error::Invalid(reason)) if reason == "reference page filter changed")
    );
    let other = World::with_campaign(
        &catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x16; 16]).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        other.reference_state_page(
            PageRequest {
                cell: None,
                after: Some(&cursor)
            },
            no_copy
        ),
        Err(Error::StaleHandle)
    ));
    fs::write(root.path().join("Other.esm"), header(&[])).unwrap();
    let changed_source = load(root.path(), &["FalloutNV.esm", "Other.esm"]);
    let other_source = World::new(&changed_source, Limits::default()).unwrap();
    assert!(matches!(
        other_source.reference_state_page(
            PageRequest {
                cell: None,
                after: Some(&cursor)
            },
            no_copy
        ),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), before);
    world.initialize_inventory(ids[0]).unwrap();
    let mutated = world.snapshot();
    assert!(
        matches!(world.reference_state_page(PageRequest { cell: None, after: Some(&cursor) }, no_copy), Err(Error::Invalid(reason)) if reason == "reference page revision changed")
    );
    assert_eq!(world.snapshot(), mutated);
    let current = world
        .reference_state_page(PageRequest::default(), small())
        .unwrap();
    world.replace_from_snapshot(mutated.clone()).unwrap();
    assert!(matches!(
        world.reference_state_page(
            PageRequest {
                cell: None,
                after: current.next_cursor()
            },
            no_copy
        ),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), mutated);
}

#[test]
fn empty_registry_and_invalid_bounds_are_read_only_and_do_not_allocate_to_request_caps() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, _) = source(root.path());
    let world = World::new(&catalogue, Limits::default()).unwrap();
    let before = world.snapshot();
    let limits = PageLimits {
        max_visited: usize::MAX,
        max_rows: usize::MAX,
        max_copied_bytes: usize::MAX,
    };
    let empty = world
        .reference_state_page(PageRequest::default(), limits)
        .unwrap();
    assert!(empty.rows().is_empty());
    assert!(empty.next_cursor().is_none());
    assert_eq!(empty.usage().visited, 0);
    assert!(
        world
            .reference_state_page(
                PageRequest::default(),
                PageLimits {
                    max_visited: 0,
                    ..limits
                }
            )
            .is_err()
    );
    assert!(
        world
            .reference_state_page(
                PageRequest::default(),
                PageLimits {
                    max_rows: 0,
                    ..limits
                }
            )
            .is_err()
    );
    assert!(
        world
            .reference_state_page(
                PageRequest::default(),
                PageLimits {
                    max_copied_bytes: 0,
                    ..limits
                }
            )
            .is_err()
    );
    assert!(
        world
            .reference_state_page(
                PageRequest {
                    cell: Some(&form(0x01000000)),
                    after: None
                },
                limits
            )
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
}

#[test]
fn actual_paged_scene_consumer_retains_changed_pose_enable_across_worker_and_cold_restart() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_REFERENCE_PAGING_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fixture(root);
    let source_bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    let (catalogue, poses) = source(root);
    let (mut world, ids) = seeded(&catalogue, &poses);
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let first = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    let stage = world
        .stage_reference_state(&world.reference_view(ids[2]).unwrap(), changed())
        .unwrap();
    world.commit_reference_state(stage).unwrap();
    let expected = observations(&world);
    assert_eq!(
        world.reference_view(ids[2]).unwrap().state(),
        Some(&changed())
    );
    assert_eq!(reconcile(&world, Some(&form(0x400)), small()).len(), 3);
    assert_eq!(reconcile(&world, Some(&form(0x401)), small()).len(), 5);
    assert_eq!(first.wait().unwrap().metadata.generation, 1);
    let second = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    worker.finish().unwrap();
    assert_eq!(second.wait().unwrap().metadata.generation, 2);
    fs::write(
        root.join("expected.observations.json"),
        serde_json::to_vec_pretty(&expected).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_paging_helper", "--ignored", "--nocapture"])
        .env("FALLOUT_REFERENCE_PAGING_COLD_ROOT", root)
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
#[ignore = "fresh child consumer selected explicitly by its parent"]
fn cold_paging_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_REFERENCE_PAGING_COLD_ROOT").unwrap());
    let (catalogue, _) = source(&root);
    let repository = Repository::open(&root.join("saved"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    let expected: Value =
        serde_json::from_slice(&fs::read(root.join("expected.observations.json")).unwrap())
            .unwrap();
    let actual = observations(&world);
    assert_eq!(actual, expected);
    let id = world.authored_reference(&form(0x502)).unwrap();
    let row = reconcile(&world, Some(&form(0x401)), small())
        .into_iter()
        .find(|row| row.reference() == id)
        .unwrap();
    assert_eq!(row.state(), Some(&changed()));
    assert!(row.state().unwrap().pose().source_scale().is_none());
    fs::write(
        root.join("cold.observations.json"),
        serde_json::to_vec_pretty(&actual).unwrap(),
    )
    .unwrap();
    println!(
        "paged scene reconciliation: exact stable identity, explicit cells, pose bits, enable and unavailable components after cold native restore; no whole-world snapshot per page"
    );
}
