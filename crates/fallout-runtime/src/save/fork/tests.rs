use super::{ForkLimits, ForkRequest, ForkSelection, ForkStage};
use crate::{
    Limits, World,
    identity::CampaignId,
    save::{self, Captured, Recovery, Repository, Slot, SlotRejectionCode, test_source as common},
};
use fallout_data::loaded_scripts::Catalogue;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

fn seeded_repository(root: &Path) -> (Arc<Catalogue>, Repository, [crate::snapshot::Snapshot; 2]) {
    common::write_fixture(root, false);
    let catalogue = Arc::new(common::load(root, &["FalloutNV.esm"]));
    let campaign = CampaignId::from_bytes([0x43; 16]).unwrap();
    let mut world =
        World::with_campaign(Arc::clone(&catalogue), Limits::default(), campaign).unwrap();
    world.register_reference(None).unwrap();
    let first = world.snapshot();
    let repository = Repository::create(&root.join("source-repository"), &[], campaign).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    world.register_reference(None).unwrap();
    let second = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    (catalogue, repository, [first, second])
}

fn repository_bytes(repository: &Repository) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(repository.path())
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn selection_from(receipt: &save::LoadReceipt) -> ForkSelection {
    ForkSelection::new(
        receipt.metadata.campaign,
        &receipt.metadata.catalogue_sha256,
        &receipt.metadata.snapshot_sha256,
    )
    .unwrap()
}

fn add_stat_whitespace(bytes: &[u8]) -> Vec<u8> {
    const META_BODY: usize = 64;
    const META_SHA: usize = 32;
    const SNAPSHOT_SIZE: usize = 120;
    const STAT_LENGTH: usize = 160;
    const STAT_SHA: usize = 168;
    const STAT_BODY: usize = 200;

    let mut result = bytes.to_vec();
    assert_eq!(result[STAT_BODY], b'{');
    let snapshot_bytes =
        u64::from_le_bytes(result[SNAPSHOT_SIZE..SNAPSHOT_SIZE + 8].try_into().unwrap());
    result[SNAPSHOT_SIZE..SNAPSHOT_SIZE + 8].copy_from_slice(&(snapshot_bytes + 1).to_le_bytes());
    let stat_bytes = u64::from_le_bytes(result[STAT_LENGTH..STAT_LENGTH + 8].try_into().unwrap());
    result[STAT_LENGTH..STAT_LENGTH + 8].copy_from_slice(&(stat_bytes + 1).to_le_bytes());
    result.insert(STAT_BODY, b' ');

    let meta_hash = Sha256::digest(&result[META_BODY..152]);
    result[META_SHA..META_SHA + 32].copy_from_slice(&meta_hash);
    let stat_end = STAT_BODY + usize::try_from(stat_bytes + 1).unwrap();
    assert_eq!(result.len() - 32, stat_end);
    let stat_hash = Sha256::digest(&result[STAT_BODY..stat_end]);
    result[STAT_SHA..STAT_SHA + 32].copy_from_slice(&stat_hash);
    let container_hash = Sha256::digest(&result[..stat_end]);
    result[stat_end..].copy_from_slice(&container_hash);
    result
}

#[test]
fn selection_requires_canonical_sha256_identities() {
    let campaign = CampaignId::from_bytes([0x43; 16]).unwrap();
    assert!(ForkSelection::new(campaign, &"a".repeat(63), &"b".repeat(64)).is_err());
    assert!(ForkSelection::new(campaign, &"A".repeat(64), &"b".repeat(64)).is_err());
    assert!(ForkSelection::new(campaign, &"a".repeat(64), &"g".repeat(64)).is_err());
}

#[test]
fn fork_uses_raw_stat_hash_from_load_receipt_and_cold_restores_that_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, source, [_, expected]) = seeded_repository(temp.path());
    let current_path = source.path().join("current.frsv");
    let original = fs::read(&current_path).unwrap();
    let raw_source = add_stat_whitespace(&original);
    fs::write(&current_path, &raw_source).unwrap();
    let before = repository_bytes(&source);

    let (loaded, source_receipt) = source
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(loaded.snapshot(), expected);
    assert_eq!(source_receipt.slot, Slot::Current);
    assert!(!source_receipt.current_repaired);
    let canonical_bytes = loaded
        .snapshot()
        .encode(Limits::default().max_snapshot_bytes)
        .unwrap();
    let canonical_hash = format!("{:x}", Sha256::digest(&canonical_bytes));
    let raw_hash = format!(
        "{:x}",
        Sha256::digest(&raw_source[200..raw_source.len() - 32])
    );
    assert_eq!(source_receipt.metadata.snapshot_sha256, raw_hash);
    assert_ne!(source_receipt.metadata.snapshot_sha256, canonical_hash);
    let selection = selection_from(&source_receipt);
    drop(loaded);

    let destination = temp.path().join("current-fork");
    let receipt = source
        .fork_boundary(
            catalogue.as_ref(),
            ForkRequest {
                destination: &destination,
                protected: &[],
                recovery: Recovery::Strict,
                expected: &selection,
                world_limits: Limits::default(),
                limits: ForkLimits::default(),
            },
        )
        .unwrap();
    assert_eq!(receipt.source().slot, Slot::Current);
    assert_eq!(receipt.source().metadata.snapshot_sha256, raw_hash);
    assert_eq!(
        receipt.destination().metadata.snapshot_sha256,
        canonical_hash
    );
    assert_eq!(receipt.destination().metadata.generation, 1);
    assert!(!receipt.source().current_repaired);
    assert!(!receipt.current_failure_truncated());

    let forked_repository = Repository::open(&destination, &[]).unwrap();
    let (forked_world, cold_receipt) = forked_repository
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(forked_world.snapshot(), expected);
    assert_eq!(cold_receipt.slot, Slot::Current);
    assert_eq!(cold_receipt.metadata.snapshot_sha256, canonical_hash);
    assert_eq!(repository_bytes(&source), before);
}

#[test]
fn fork_recovers_the_selected_previous_ancestor_without_repairing_source() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, source, [expected_previous, _]) = seeded_repository(temp.path());
    let current_path = source.path().join("current.frsv");
    fs::write(&current_path, b"truncated current boundary").unwrap();
    let before = repository_bytes(&source);

    let (selected_world, selected_receipt) = source
        .load(
            catalogue.as_ref(),
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(selected_world.snapshot(), expected_previous);
    assert_eq!(selected_receipt.slot, Slot::Previous);
    assert!(!selected_receipt.current_repaired);
    let selection = selection_from(&selected_receipt);
    drop(selected_world);

    let destination = temp.path().join("previous-fork");
    let fork = source
        .fork_boundary(
            catalogue.as_ref(),
            ForkRequest {
                destination: &destination,
                protected: &[],
                recovery: Recovery::PreviousIfCurrentInvalid,
                expected: &selection,
                world_limits: Limits::default(),
                limits: ForkLimits::default(),
            },
        )
        .unwrap();
    assert_eq!(fork.source().slot, Slot::Previous);
    assert_eq!(
        fork.source().metadata.snapshot_sha256,
        selected_receipt.metadata.snapshot_sha256
    );
    assert_eq!(
        fork.source().current_failure,
        selected_receipt.current_failure
    );
    assert!(!fork.source().current_repaired);

    let forked_repository = Repository::open(&destination, &[]).unwrap();
    let (forked_world, cold_receipt) = forked_repository
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(forked_world.snapshot(), expected_previous);
    assert_eq!(cold_receipt.metadata.generation, 1);
    assert_eq!(repository_bytes(&source), before);
}

#[test]
fn selection_mismatch_refuses_before_destination_creation_and_preserves_source() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, source, _) = seeded_repository(temp.path());
    let (_, receipt) = source
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    let mut wrong_hash = receipt.metadata.snapshot_sha256.clone();
    wrong_hash.replace_range(
        0..1,
        if wrong_hash.starts_with('f') {
            "e"
        } else {
            "f"
        },
    );
    let wrong = ForkSelection::new(
        receipt.metadata.campaign,
        &receipt.metadata.catalogue_sha256,
        &wrong_hash,
    )
    .unwrap();
    let destination = temp.path().join("wrong-selection");
    let before = repository_bytes(&source);
    let failure = source
        .fork_boundary(
            catalogue.as_ref(),
            ForkRequest {
                destination: &destination,
                protected: &[],
                recovery: Recovery::Strict,
                expected: &wrong,
                world_limits: Limits::default(),
                limits: ForkLimits::default(),
            },
        )
        .unwrap_err();
    assert_eq!(failure.stage(), ForkStage::Selection);
    assert_eq!(failure.reason().code(), SlotRejectionCode::NativeFormat);
    assert!(!destination.exists());
    assert_eq!(repository_bytes(&source), before);
}

#[test]
fn failed_destination_publication_keeps_source_slots_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, source, _) = seeded_repository(temp.path());
    let (_, source_receipt) = source
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    let selection = selection_from(&source_receipt);
    let source_before = repository_bytes(&source);
    let destination = temp.path().join("publication-failure-fork");
    let mut obstruction_created = false;
    let mut obstruction_error = None;
    let failure = source
        .fork_boundary_observing(
            catalogue.as_ref(),
            ForkRequest {
                destination: &destination,
                protected: &[],
                recovery: Recovery::Strict,
                expected: &selection,
                world_limits: Limits::default(),
                limits: ForkLimits::default(),
            },
            |stage| {
                if stage == super::super::Stage::CurrentTempWritten {
                    match fs::create_dir(destination.join("current.frsv")) {
                        Ok(()) => obstruction_created = true,
                        Err(error) => obstruction_error = Some(error.to_string()),
                    }
                }
            },
        )
        .unwrap_err();
    assert!(obstruction_error.is_none(), "{obstruction_error:?}");
    assert!(obstruction_created);
    assert_eq!(failure.stage(), ForkStage::Publication);
    assert_eq!(failure.reason().code(), SlotRejectionCode::NativeFormat);
    assert_eq!(repository_bytes(&source), source_before);

    assert!(destination.join(".rust-fallout-saves").is_file());
    assert!(destination.join("writer.lock").is_file());
    assert!(destination.join("current.frsv").is_dir());
    assert!(fs::read_dir(&destination).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".pending-")
    }));
}

#[test]
fn captured_clones_share_one_payload_until_a_test_mutation_detaches() {
    let temp = tempfile::tempdir().unwrap();
    let (catalogue, _, _) = seeded_repository(temp.path());
    let world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x43; 16]).unwrap(),
    )
    .unwrap();
    let capture = Captured::at_boundary(&world);
    let weak = Arc::downgrade(&capture.payload);
    let shared = capture.clone();
    assert!(Arc::ptr_eq(&capture.payload, &shared.payload));
    assert!(std::ptr::eq(capture.snapshot(), shared.snapshot()));
    assert_eq!(Arc::strong_count(&capture.payload), 2);

    let mut detached = shared;
    detached.snapshot_mut().clocks.tick = 19;
    assert!(!Arc::ptr_eq(&capture.payload, &detached.payload));
    assert_eq!(capture.snapshot().clocks.tick, 0);
    assert_eq!(detached.snapshot().clocks.tick, 19);
    assert_eq!(Arc::strong_count(&capture.payload), 1);
    drop(detached);
    drop(capture);
    assert!(weak.upgrade().is_none());
}
