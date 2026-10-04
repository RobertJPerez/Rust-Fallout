mod common;
use common::*;
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore, world};
use fallout_runtime::{
    Error, Limits, World,
    events::{Clocks, Context, Trigger},
    identity::{CampaignId, Owner, ReferenceId, Value},
    inventory::Facts,
    reference_state::{BatchLimits, BatchReceipt, Pose, State, View},
    save::{Captured, Recovery, Repository, SaveStatus, SaveWorker, format},
    snapshot::{ReferenceState, Snapshot},
};
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn fixture(root: &Path) {
    fs::create_dir_all(root).unwrap();
    let mut bytes = [
        header(&[]),
        record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
        record(b"MISC", 0x100, 0, &[]),
        record(b"SCPT", 0x300, 0, &unit(&[(42, 0)], &[])),
    ]
    .concat();
    for index in 0..4 {
        let data = [
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
        body.extend(field(b"DATA", &data));
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
    let catalogue = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let cell = store.winner(&form(0x400)).unwrap();
    world::decode_cell(&store.read(cell).unwrap(), "FalloutNV.esm").unwrap();
    let poses = (0x500..0x504)
        .map(|key| {
            let placed = store.winner(&form(key)).unwrap();
            let placed =
                world::decode_placement(&store.read(placed).unwrap(), "FalloutNV.esm").unwrap();
            Pose::from_source(&placed.transform.value, placed.scale.map(|s| s.value)).unwrap()
        })
        .collect();
    (catalogue, poses)
}
fn seed(catalogue: &Catalogue) -> (World<'_>, Vec<ReferenceId>) {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x16; 16]).unwrap(),
    )
    .unwrap();
    let ids = (0x500..0x504)
        .map(|key| world.register_reference(Some(form(key))).unwrap())
        .collect::<Vec<_>>();
    world.initialize_inventory(ids[0]).unwrap();
    world
        .add_item(ids[0], Facts::unknown(form(0x100)), 3.try_into().unwrap())
        .unwrap();
    let handle = world
        .create_instance(
            &definition(catalogue),
            Owner::Placed { reference: ids[0] },
            Context {
                calling_reference: Some(ids[1]),
                ..Context::default()
            },
        )
        .unwrap();
    world
        .assign(
            handle,
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
        )
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
    (world, ids)
}
fn changes(world: &World<'_>, ids: &[ReferenceId], poses: &[Pose]) -> Vec<(View, State)> {
    // Explicit host group, source cell and enable choices; no inferred authored
    // group or default initial state. Intentionally preserve a non-ID order.
    [2, 0, 1]
        .into_iter()
        .map(|index| {
            (
                world.reference_view(ids[index]).unwrap(),
                State::new(form(0x400), poses[index].clone(), index % 2 == 0).unwrap(),
            )
        })
        .collect()
}
fn expected_after(before: &Snapshot, changes: &[(View, State)]) -> Snapshot {
    let mut expected = before.clone();
    expected.state_revision += 1;
    for (view, state) in changes {
        expected
            .reference_states
            .retain(|row| row.id != view.reference());
        expected.reference_states.push(ReferenceState {
            id: view.reference(),
            state: state.clone(),
        });
    }
    expected.reference_states.sort_by_key(|row| row.id);
    expected
}

// Actual source-selected host operation used for both initialization and group
// changes. Every observation is taken at the same revision before one commit.
fn apply_group(world: &mut World<'_>, ids: &[ReferenceId], poses: &[Pose]) -> BatchReceipt {
    let rows = changes(world, ids, poses);
    apply_request(world, &rows)
}
fn apply_request(world: &mut World<'_>, rows: &[(View, State)]) -> BatchReceipt {
    let stage = world
        .stage_reference_batch(rows, BatchLimits::default())
        .unwrap();
    world.commit_reference_batch(stage).unwrap()
}

#[test]
fn source_selected_group_initializes_atomically_once_with_exact_receipt_order_and_drop_semantics() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (mut world, ids) = seed(&catalogue);
    let before = world.snapshot();
    let rows = changes(&world, &ids, &poses);
    let stage = world
        .stage_reference_batch(&rows, BatchLimits::default())
        .unwrap();
    assert_eq!(stage.rows().len(), 3);
    assert_eq!(stage.rows()[0].base().reference(), ids[2]);
    assert_eq!(stage.rows()[0].state(), &rows[0].1);
    assert_eq!(world.snapshot(), before);
    drop(stage);
    assert_eq!(world.snapshot(), before);
    let receipt = apply_group(&mut world, &ids, &poses);
    assert_eq!(receipt.campaign(), world.campaign());
    assert_eq!(
        receipt.catalogue_fingerprint(),
        world.catalogue_fingerprint()
    );
    assert_eq!(receipt.before_revision(), before.state_revision);
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    assert_eq!(
        receipt
            .changes()
            .iter()
            .map(|r| r.reference())
            .collect::<Vec<_>>(),
        vec![ids[2], ids[0], ids[1]]
    );
    for (change, (view, state)) in receipt.changes().iter().zip(&rows) {
        assert_eq!(change.authored(), view.authored());
        assert_eq!(change.state(), state);
    }
    assert_eq!(world.snapshot(), expected_after(&before, &rows));
    assert!(world.reference_view(ids[3]).unwrap().state().is_none());
    // Repeating explicit identical writes is still one revision, consistent with
    // the single-reference API; there is no successful empty/no-effect command.
    let before_noop = world.snapshot();
    let identical = changes(&world, &ids, &poses);
    let receipt = apply_group(&mut world, &ids, &poses);
    assert_eq!(receipt.after_revision(), before_noop.state_revision + 1);
    assert_eq!(world.snapshot(), expected_after(&before_noop, &identical));
    let exact = world.snapshot();
    assert!(
        matches!(world.stage_reference_batch(&[], BatchLimits::default()), Err(Error::Invalid(reason)) if reason == "empty reference batch")
    );
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn invalid_last_state_duplicate_identity_and_stale_last_view_refuse_the_whole_group() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (mut world, ids) = seed(&catalogue);
    let before = world.snapshot();
    let rows = changes(&world, &ids, &poses);
    let mut invalid = rows.clone();
    let mut forged = serde_json::to_value(&invalid[2].1).unwrap();
    forged["schema_version"] = json!(2);
    invalid[2].1 = serde_json::from_value(forged).unwrap();
    assert!(
        world
            .stage_reference_batch(&invalid, BatchLimits::default())
            .is_err()
    );
    assert_eq!(world.snapshot(), before);
    let mut duplicate = rows.clone();
    duplicate[2] = duplicate[0].clone();
    assert!(
        matches!(world.stage_reference_batch(&duplicate, BatchLimits::default()), Err(Error::Invalid(reason)) if reason == "duplicate reference batch identity")
    );
    assert_eq!(world.snapshot(), before);
    world
        .add_item(ids[0], Facts::unknown(form(0x100)), 2.try_into().unwrap())
        .unwrap();
    let exact = world.snapshot();
    let mut stale_last = changes(&world, &ids, &poses);
    stale_last[2].0 = rows[2].0.clone();
    assert!(
        world
            .stage_reference_batch(&stale_last, BatchLimits::default())
            .is_err()
    );
    assert_eq!(world.snapshot(), exact);
    let (other, _) = seed(&catalogue);
    let mut foreign_last = changes(&world, &ids, &poses);
    foreign_last[2].0 = other.reference_view(ids[1]).unwrap();
    assert!(matches!(
        world.stage_reference_batch(&foreign_last, BatchLimits::default()),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), exact);
}

#[test]
fn aggregate_staging_admission_is_exact_and_revision_exhaustion_has_no_partial_commit() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (mut world, ids) = seed(&catalogue);
    let before = world.snapshot();
    let rows = changes(&world, &ids, &poses);
    let stage = world
        .stage_reference_batch(&rows, BatchLimits::default())
        .unwrap();
    let exact = stage.charged_bytes();
    drop(stage);
    assert!(matches!(
        world.stage_reference_batch(
            &rows,
            BatchLimits {
                max_rows: 2,
                ..BatchLimits::default()
            }
        ),
        Err(Error::Capacity("reference batch rows"))
    ));
    assert!(matches!(
        world.stage_reference_batch(
            &rows,
            BatchLimits {
                max_rows: 0,
                ..BatchLimits::default()
            }
        ),
        Err(Error::Capacity("reference batch rows"))
    ));
    assert!(matches!(
        world.stage_reference_batch(
            &rows,
            BatchLimits {
                max_copied_bytes: exact - 1,
                ..BatchLimits::default()
            }
        ),
        Err(Error::Capacity("reference batch copied bytes"))
    ));
    let admitted = world
        .stage_reference_batch(
            &rows,
            BatchLimits {
                max_copied_bytes: exact,
                max_rows: usize::MAX,
            },
        )
        .unwrap();
    assert_eq!(admitted.charged_bytes(), exact);
    drop(admitted);
    assert_eq!(world.snapshot(), before);
    let mut exhausted = before;
    exhausted.state_revision = u64::MAX;
    world.replace_from_snapshot(exhausted.clone()).unwrap();
    let rows = changes(&world, &ids, &poses);
    let stage = world
        .stage_reference_batch(&rows, BatchLimits::default())
        .unwrap();
    assert!(matches!(
        world.commit_reference_batch(stage),
        Err(Error::Capacity("state revisions"))
    ));
    assert_eq!(world.snapshot(), exhausted);
}

#[test]
fn competing_batches_and_unrelated_mutations_have_one_winner_without_partial_effects() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    for winner_order in [false, true] {
        let (mut world, ids) = seed(&catalogue);
        let before = world.snapshot();
        let first = changes(&world, &ids, &poses);
        let mut second = first.clone();
        for (_, state) in &mut second {
            *state =
                State::new(state.cell().clone(), state.pose().clone(), !state.enabled()).unwrap();
        }
        let a = world
            .stage_reference_batch(&first, BatchLimits::default())
            .unwrap();
        let b = world
            .stage_reference_batch(&second, BatchLimits::default())
            .unwrap();
        let (winner, loser, expected) = if winner_order {
            (a, b, &first)
        } else {
            (b, a, &second)
        };
        let receipt = world.commit_reference_batch(winner).unwrap();
        let exact = expected_after(&before, expected);
        assert_eq!(world.snapshot(), exact);
        assert_eq!(receipt.after_revision(), exact.state_revision);
        assert!(world.commit_reference_batch(loser).is_err());
        assert_eq!(world.snapshot(), exact);
    }
    for mutation in 0..4 {
        let (mut world, ids) = seed(&catalogue);
        let stage = world
            .stage_reference_batch(&changes(&world, &ids, &poses), BatchLimits::default())
            .unwrap();
        match mutation {
            0 => {
                world
                    .add_item(ids[0], Facts::unknown(form(0x100)), 1.try_into().unwrap())
                    .unwrap();
            }
            1 => {
                world
                    .advance_clocks(Clocks {
                        tick: 1,
                        real_nanoseconds: 17,
                        ..Clocks::default()
                    })
                    .unwrap();
            }
            2 => {
                let instance = world.snapshot().instances[0].id;
                world
                    .assign(
                        world.handle(instance).unwrap(),
                        &[(
                            42,
                            Value::Number {
                                bits: 9_f64.to_bits(),
                            },
                        )],
                    )
                    .unwrap();
            }
            _ => {
                let view = world.reference_view(ids[3]).unwrap();
                let single = world
                    .stage_reference_state(
                        &view,
                        State::new(form(0x400), poses[3].clone(), false).unwrap(),
                    )
                    .unwrap();
                world.commit_reference_state(single).unwrap();
            }
        }
        let exact = world.snapshot();
        assert!(world.commit_reference_batch(stage).is_err());
        assert_eq!(world.snapshot(), exact);
    }
}

#[test]
fn equal_state_restore_other_campaign_and_changed_source_never_reuse_batch_authority() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path());
    let (catalogue, poses) = source(root.path());
    let (mut world, ids) = seed(&catalogue);
    let before = world.snapshot();
    let stage = world
        .stage_reference_batch(&changes(&world, &ids, &poses), BatchLimits::default())
        .unwrap();
    world.replace_from_snapshot(before.clone()).unwrap();
    assert!(matches!(
        world.commit_reference_batch(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(world.snapshot(), before);
    let mut campaign = before.clone();
    campaign.campaign = CampaignId::from_bytes([0x17; 16]).unwrap();
    let mut other = World::restore(&catalogue, campaign.clone(), Limits::default()).unwrap();
    let stage = world
        .stage_reference_batch(&changes(&world, &ids, &poses), BatchLimits::default())
        .unwrap();
    assert!(matches!(
        other.commit_reference_batch(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other.snapshot(), campaign);
    fs::write(root.path().join("Other.esm"), header(&[])).unwrap();
    let changed_source = load(root.path(), &["FalloutNV.esm", "Other.esm"]);
    let (mut other_source, _) = seed(&changed_source);
    let exact = other_source.snapshot();
    let stage = world
        .stage_reference_batch(&changes(&world, &ids, &poses), BatchLimits::default())
        .unwrap();
    assert!(matches!(
        other_source.commit_reference_batch(stage),
        Err(Error::StaleHandle)
    ));
    assert_eq!(other_source.snapshot(), exact);
    assert_eq!(world.snapshot(), before);
}

#[test]
fn real_source_group_and_explicit_changes_native_publish_and_cold_restore_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let retained =
        std::env::var_os("FALLOUT_REFERENCE_BATCH_EVIDENCE").map(std::path::PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    fixture(root);
    let source_bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    let (catalogue, poses) = source(root);
    let (mut world, ids) = seed(&catalogue);
    let receipt = apply_group(&mut world, &ids, &poses);
    let first = world.snapshot();
    assert_eq!(receipt.changes().len(), 3);
    let repository = Repository::create(&root.join("saved"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let pending = SaveStatus::new(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    let changed_poses = (0..4)
        .map(|index| {
            Pose::from_source(
                &world::Transform {
                    position: [8192.25 + index as f32, -0.0, -30.5],
                    rotation: [0.125, -0.75, 1.5],
                },
                (index % 2 == 1).then_some(1.25),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let before = world.snapshot();
    let mut rows = changes(&world, &ids, &changed_poses);
    for (_, state) in &mut rows {
        *state = State::new(state.cell().clone(), state.pose().clone(), !state.enabled()).unwrap();
    }
    let receipt = apply_request(&mut world, &rows);
    assert_eq!(receipt.after_revision(), before.state_revision + 1);
    let second = expected_after(&before, &rows);
    assert_eq!(world.snapshot(), second);
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
        second.encode(1 << 20).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("batch.receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    drop(world);
    drop(catalogue);
    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cold_batch_helper", "--ignored", "--nocapture"])
        .env("FALLOUT_REFERENCE_BATCH_COLD_ROOT", root)
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
#[ignore = "fresh child group consumer selected explicitly by the parent"]
fn cold_batch_helper() {
    let root =
        std::path::PathBuf::from(std::env::var_os("FALLOUT_REFERENCE_BATCH_COLD_ROOT").unwrap());
    let (catalogue, _) = source(&root);
    let repository = Repository::open(&root.join("saved"), &[]).unwrap();
    let (world, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    let expected = Snapshot::decode(
        &fs::read(root.join("expected.snapshot.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(world.snapshot(), expected);
    for row in &expected.reference_states {
        let view = world.reference_view(row.id).unwrap();
        assert_eq!(view.state(), Some(&row.state));
        assert_eq!(
            world.authored_reference(view.authored().unwrap()),
            Some(row.id)
        );
        assert_eq!(
            row.state.pose().source_transform().position[1].to_bits(),
            0x80000000
        );
    }
    assert_eq!(expected.reference_states.len(), 3);
    let unavailable = world.authored_reference(&form(0x503)).unwrap();
    assert!(world.reference_view(unavailable).unwrap().state().is_none());
    fs::write(
        root.join("cold.snapshot.json"),
        world.snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
    println!(
        "source-selected atomic group: exact references, changed pose/enable, unavailable peer, inventory, locals, contexts and pending events retained after native worker and cold restore"
    );
}
