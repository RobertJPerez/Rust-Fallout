mod common;

use common::{load, record, write_fixture};
use fallout_runtime::{
    Limits, World,
    events::Clocks,
    identity::CampaignId,
    save::{self, Captured, Recovery, Repository, Slot, Stage},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, fs, io::Write, path::Path, path::PathBuf, process::Command, sync::Arc,
};

const CAMPAIGN: [u8; 16] = [0x61; 16];

fn evidence_directory(fallback: &Path) -> PathBuf {
    let root = std::env::var_os("FALLOUT_ROUTE_ACCEPTANCE_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.to_path_buf());
    fs::create_dir_all(&root).unwrap();
    root
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_json(root: &Path, name: &str, value: serde_json::Value) {
    let mut bytes = serde_json::to_vec_pretty(&value).unwrap();
    bytes.push(b'\n');
    fs::write(root.join(name), bytes).unwrap();
}

fn catalogue(root: &Path) -> Arc<fallout_data::loaded_scripts::Catalogue> {
    write_fixture(root, false);
    Arc::new(load(root, &["FalloutNV.esm"]))
}

fn tick(world: &mut World<'_>, tick: u64) {
    world
        .advance_clocks(Clocks {
            tick,
            game_nanoseconds: tick * 100,
            menu_nanoseconds: tick * 2,
            real_nanoseconds: tick * 3,
        })
        .unwrap();
}

fn slots(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().to_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn interrupted_publication_preserves_the_last_valid_current_slot() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let catalogue = catalogue(root);
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes(CAMPAIGN).unwrap(),
    )
    .unwrap();
    tick(&mut world, 1);
    let expected = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let prior_current_bytes = fs::read(repository.path().join("current.frsv")).unwrap();
    drop(world);
    drop(catalogue);

    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "interrupt_publication_child",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_ROUTE_ACCEPTANCE_ROOT", root)
        .output()
        .unwrap();
    let evidence = evidence_directory(root);
    fs::write(evidence.join("interrupt.stdout.log"), &child.stdout).unwrap();
    fs::write(evidence.join("interrupt.stderr.log"), &child.stderr).unwrap();
    assert_eq!(child.status.code(), Some(73));
    let child_stdout = String::from_utf8_lossy(&child.stdout);
    let marker = "INTERRUPT_AT_PREVIOUS_PUBLISHED pid=";
    let child_pid = child_stdout
        .lines()
        .find_map(|line| line.strip_prefix(marker)?.parse::<u32>().ok())
        .expect("owned child reports its pid at the declared boundary");
    assert!(child.stderr.is_empty());

    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let current_bytes = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous_bytes = fs::read(repository.path().join("previous.frsv")).unwrap();
    assert_eq!(current_bytes, prior_current_bytes);
    assert_eq!(previous_bytes, prior_current_bytes);
    let current = save::format::decode(&current_bytes, Limits::default()).unwrap();
    let previous = save::format::decode(&previous_bytes, Limits::default()).unwrap();
    assert_eq!(current.metadata.generation, 1);
    assert_eq!(previous.metadata.generation, 1);
    assert_eq!(current.snapshot, expected);
    assert_eq!(previous.snapshot, expected);
    let (restored, receipt) = repository
        .load(
            Arc::new(load(root, &["FalloutNV.esm"])),
            Limits::default(),
            Recovery::Strict,
        )
        .unwrap();
    assert_eq!(restored.snapshot(), expected);
    assert_eq!(receipt.slot, Slot::Current);
    write_json(
        &evidence,
        "interruption-receipt.json",
        serde_json::json!({
            "classification": "synthetic_repository_interruption_acceptance",
            "retail_pass": false,
            "boundary": "previous_published",
            "child_pid": child_pid,
            "child_exit_code": child.status.code(),
            "child_executable_sha256": sha256(&fs::read(std::env::current_exe().unwrap()).unwrap()),
            "prior_current_sha256": sha256(&prior_current_bytes),
            "current_sha256": sha256(&current_bytes),
            "previous_sha256": sha256(&previous_bytes),
            "current_generation": current.metadata.generation,
            "previous_generation": previous.metadata.generation,
            "strict_current_load": "passed",
            "snapshots_match_prior_current": current.snapshot == expected && previous.snapshot == expected
        }),
    );
}

#[test]
#[ignore = "owned subprocess exits at the declared publication boundary"]
fn interrupt_publication_child() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_ROUTE_ACCEPTANCE_ROOT").expect("parent supplies fixture root"),
    );
    let catalogue = catalogue(&root);
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes(CAMPAIGN).unwrap(),
    )
    .unwrap();
    tick(&mut world, 1);
    tick(&mut world, 2);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    repository
        .commit_observing(&Captured::at_boundary(&world), |stage| {
            if stage == Stage::PreviousPublished {
                let mut stdout = std::io::stdout().lock();
                writeln!(
                    stdout,
                    "INTERRUPT_AT_PREVIOUS_PUBLISHED pid={}",
                    std::process::id()
                )
                .unwrap();
                stdout.flush().unwrap();
                std::process::exit(73);
            }
        })
        .unwrap();
    panic!("declared interruption boundary was not reached");
}

#[test]
fn damaged_profile_and_stale_source_receipts_are_rejected_without_slot_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let catalogue = catalogue(root);
    let mut world = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        CampaignId::from_bytes(CAMPAIGN).unwrap(),
    )
    .unwrap();
    tick(&mut world, 1);
    let first = world.snapshot();
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    tick(&mut world, 2);
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let current_path = repository.path().join("current.frsv");
    let previous_path = repository.path().join("previous.frsv");
    let original_current = fs::read(&current_path).unwrap();
    let original_previous = fs::read(&previous_path).unwrap();

    let mut damaged = original_current.clone();
    *damaged.last_mut().unwrap() ^= 0x80;
    fs::write(&current_path, &damaged).unwrap();
    let damaged_slots = slots(repository.path());
    assert!(matches!(
        repository.load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict),
        Err(save::Error::Format(_))
    ));
    let (fallback, fallback_receipt) = repository
        .load(
            Arc::clone(&catalogue),
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(fallback.snapshot(), first);
    assert_eq!(fallback_receipt.slot, Slot::Previous);
    assert!(!fallback_receipt.current_repaired);
    drop(fallback);
    assert_eq!(slots(repository.path()), damaged_slots);

    let (recovered, recovery_receipt) = repository
        .recover_previous(Arc::clone(&catalogue), Limits::default())
        .unwrap();
    assert_eq!(recovered.snapshot(), first);
    assert_eq!(recovery_receipt.slot, Slot::Previous);
    assert!(recovery_receipt.current_repaired);
    drop(recovered);
    let valid_slots = slots(repository.path());
    assert_eq!(fs::read(&current_path).unwrap(), original_previous);

    let mut changed_source = fs::read(root.join("FalloutNV.esm")).unwrap();
    changed_source.extend(record(b"ACTI", 0x500, 0, &[]));
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(root.join("FalloutNV.esm"))
        .unwrap()
        .write_all(&changed_source)
        .unwrap();
    let changed_catalogue = Arc::new(load(root, &["FalloutNV.esm"]));
    let stale_source = repository
        .load(
            Arc::clone(&changed_catalogue),
            Limits::default(),
            Recovery::Strict,
        )
        .err()
        .unwrap();
    assert!(matches!(
        stale_source,
        save::Error::State(fallout_runtime::Error::DefinitionChanged)
    ));
    assert_eq!(slots(repository.path()), valid_slots);

    let mut foreign_profile = fs::read(&current_path).unwrap();
    foreign_profile[64..68].copy_from_slice(&2_u32.to_le_bytes());
    let meta_hash = Sha256::digest(&foreign_profile[64..152]);
    foreign_profile[32..64].copy_from_slice(&meta_hash);
    let checksum_start = foreign_profile.len() - 32;
    let container_hash = Sha256::digest(&foreign_profile[..checksum_start]);
    foreign_profile[checksum_start..].copy_from_slice(&container_hash);
    assert!(save::format::decode(&foreign_profile, Limits::default()).is_err());
    fs::write(&current_path, &foreign_profile).unwrap();
    let foreign_slots = slots(repository.path());
    assert!(matches!(
        repository.load(Arc::clone(&catalogue), Limits::default(), Recovery::Strict),
        Err(save::Error::Format(_))
    ));
    assert_eq!(slots(repository.path()), foreign_slots);
    assert_eq!(foreign_slots.get("previous.frsv"), Some(&original_previous));
    let profile_decode_error = save::format::decode(&foreign_profile, Limits::default())
        .err()
        .unwrap()
        .to_string();
    write_json(
        &evidence_directory(root),
        "repository-rejection-receipt.json",
        serde_json::json!({
            "classification": "synthetic_repository_rejection_acceptance",
            "retail_pass": false,
            "damaged_current_sha256": sha256(&damaged),
            "damaged_strict_load": "rejected",
            "fallback_slot": "previous",
            "fallback_repaired_current": false,
            "explicit_recovery_repaired_current": true,
            "recovered_current_sha256": sha256(&original_previous),
            "changed_source_sha256": sha256(&changed_source),
            "stale_source": "definition_changed",
            "profile_container_sha256": sha256(&foreign_profile),
            "profile_decode_error": profile_decode_error,
            "all_rejections_left_slots_unchanged": true
        }),
    );
}
