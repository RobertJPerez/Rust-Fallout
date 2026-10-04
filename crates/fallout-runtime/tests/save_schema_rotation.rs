mod common;
use common::*;
use fallout_runtime::{
    Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    save::{self, Captured, Recovery, Repository, Slot, format},
};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, process::Command};

fn seed(catalogue: &fallout_data::loaded_scripts::Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x31; 16]).unwrap(),
    )
    .unwrap();
    let handle = world
        .create_instance(
            &definition(catalogue),
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 1 })])
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
    world
}

// Rewrite the wire independently of Captured's private snapshot. Update all
// extents and checksums, so rejection must come from source-bound state checks.
fn rewrite(bytes: &[u8], edit: impl FnOnce(&mut fallout_runtime::snapshot::Snapshot)) -> Vec<u8> {
    let mut snapshot = format::decode(bytes, Limits::default()).unwrap().snapshot;
    edit(&mut snapshot);
    let body = snapshot
        .encode(Limits::default().max_snapshot_bytes)
        .unwrap();
    let mut out = bytes[..200].to_vec();
    out[120..128].copy_from_slice(&(body.len() as u64).to_le_bytes());
    out[160..168].copy_from_slice(&(body.len() as u64).to_le_bytes());
    let meta_digest = Sha256::digest(&out[64..152]);
    out[32..64].copy_from_slice(&meta_digest);
    out[168..200].copy_from_slice(&Sha256::digest(&body));
    out.extend(body);
    let digest = Sha256::digest(&out);
    out.extend(digest);
    format::decode(&out, Limits::default()).unwrap();
    out
}

#[test]
fn checksum_valid_wrong_local_kind_cannot_rotate_over_valid_previous() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let expected = world.snapshot();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let handle = world.handle(expected.instances[0].id).unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 2 })])
        .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    let forged = rewrite(&current, |snapshot| {
        snapshot.instances[0]
            .locals
            .iter_mut()
            .find(|local| local.index == 42)
            .unwrap()
            .value = Value::Reference {
            value: ReferenceValue::Null,
        };
    });
    fs::write(repository.path().join("current.frsv"), &forged).unwrap();
    assert!(matches!(
        repository.load(&catalogue, Limits::default(), Recovery::Strict),
        Err(save::Error::State(
            fallout_runtime::Error::IncompatibleLocal(42)
        ))
    ));
    world
        .assign(handle, &[(42, Value::Number { bits: 3 })])
        .unwrap();
    let mut stages = Vec::new();
    let result =
        repository.commit_observing(&Captured::at_boundary(&world), |stage| stages.push(stage));
    assert!(
        matches!(
            result,
            Err(save::Error::State(
                fallout_runtime::Error::IncompatibleLocal(42)
            ))
        ),
        "publication result: {result:?}; previous was replaced by schema-invalid current: {}",
        fs::read(repository.path().join("previous.frsv")).unwrap() == forged
    );
    assert!(stages.is_empty());
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        forged
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 4);

    // A fresh process observes the exact previous state and explicit fallback;
    // recovery alone cannot conceal or repair the failed current file.
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "cold_schema_recovery",
            "--nocapture",
        ])
        .env("FALLOUT_SCHEMA_RECOVERY_ROOT", directory.path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    assert!(command.status().unwrap().success());
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        forged
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    let (restored, receipt) = repository
        .recover_previous(&catalogue, Limits::default())
        .unwrap();
    assert!(receipt.current_repaired);
    assert_eq!(restored.snapshot(), expected);
    let receipt = repository.commit(&Captured::at_boundary(&world)).unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        previous
    );
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        world.snapshot()
    );
}

#[test]
fn declaration_banks_versions_and_compiled_event_sites_share_restore_refusals() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let handle = world.handle(world.snapshot().instances[0].id).unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 2 })])
        .unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let previous = fs::read(repository.path().join("previous.frsv")).unwrap();
    world
        .assign(handle, &[(42, Value::Number { bits: 3 })])
        .unwrap();
    let capture = Captured::at_boundary(&world);
    for case in 0..6 {
        let forged = rewrite(&current, |snapshot| match case {
            0 => {
                snapshot.instances[0]
                    .locals
                    .iter_mut()
                    .find(|local| local.index == 90)
                    .unwrap()
                    .value = Value::Number { bits: 0 }
            }
            1 => {
                snapshot.instances[0].locals.pop();
            }
            2 => {
                snapshot.instances[0]
                    .locals
                    .iter_mut()
                    .find(|local| local.index == 42)
                    .unwrap()
                    .index = 999
            }
            3 => {
                snapshot.pending_events[0].trigger = Trigger::Block {
                    event_id: 3,
                    begin_byte_offset: 0,
                }
            }
            4 => {
                snapshot.pending_events[0].trigger = Trigger::Block {
                    event_id: 0,
                    begin_byte_offset: 12,
                }
            }
            5 => snapshot.instances[0].definition.version_sha256 = "ff".repeat(32),
            _ => unreachable!(),
        });
        fs::write(repository.path().join("current.frsv"), &forged).unwrap();
        let restore_error = match repository.load(&catalogue, Limits::default(), Recovery::Strict) {
            Err(error) => error,
            Ok(_) => panic!("invalid case {case} restored"),
        };
        let mut stages = Vec::new();
        let publication_error = repository
            .commit_observing(&capture, |stage| stages.push(stage))
            .unwrap_err();
        assert_eq!(
            publication_error.to_string(),
            restore_error.to_string(),
            "case {case}"
        );
        assert!(stages.is_empty());
        assert_eq!(
            fs::read(repository.path().join("current.frsv")).unwrap(),
            forged
        );
        assert_eq!(
            fs::read(repository.path().join("previous.frsv")).unwrap(),
            previous
        );
        assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 4);
    }
}

#[test]
fn capture_keeps_removed_instance_schema_after_world_and_catalogue_are_dropped() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let handle = world.handle(world.snapshot().instances[0].id).unwrap();
    world.acknowledge(1).unwrap();
    world.remove_instance(handle).unwrap();
    let expected = world.snapshot();
    let capture = Captured::at_boundary(&world);
    drop(world);
    drop(catalogue);
    let worker_repository = repository.clone();
    let receipt = std::thread::spawn(move || worker_repository.commit(&capture))
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(receipt.metadata.generation, 2);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    assert_eq!(
        repository
            .load(&catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected
    );
}

fn two_definitions(path: &std::path::Path) -> fallout_data::loaded_scripts::Catalogue {
    write_fixture(path, false);
    let plugin_path = path.join("FalloutNV.esm");
    let mut bytes = fs::read(&plugin_path).unwrap();
    bytes.extend(record(b"SCPT", 0x301, 0, &unit(&[(7, 0)], &[])));
    fs::write(plugin_path, bytes).unwrap();
    load(path, &["FalloutNV.esm"])
}

#[test]
fn cached_source_context_is_bounded_before_publication_without_partial_rotation() {
    for (limits, expected_error) in [
        (
            Limits {
                max_instances: 1,
                ..Limits::default()
            },
            "publication source definitions",
        ),
        (
            Limits {
                max_locals: 3,
                ..Limits::default()
            },
            "publication source locals",
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let catalogue = two_definitions(directory.path());
        let mut world = World::new(&catalogue, limits).unwrap();
        let first = world
            .create_instance(
                &definition(&catalogue),
                Owner::Fragment {
                    activation: 1.try_into().unwrap(),
                },
                Context::default(),
            )
            .unwrap();
        let repository =
            Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
        repository.commit(&Captured::at_boundary(&world)).unwrap();
        let current = fs::read(repository.path().join("current.frsv")).unwrap();
        world.remove_instance(first).unwrap();
        let second = catalogue
            .record_scripts(&form(0x301))
            .next()
            .unwrap()
            .handle();
        world
            .create_instance(
                second,
                Owner::Fragment {
                    activation: 2.try_into().unwrap(),
                },
                Context::default(),
            )
            .unwrap();
        assert!(
            matches!(repository.commit(&Captured::at_boundary(&world)), Err(save::Error::State(fallout_runtime::Error::Capacity(reason))) if reason == expected_error)
        );
        assert_eq!(
            fs::read(repository.path().join("current.frsv")).unwrap(),
            current
        );
        assert!(!repository.path().join("previous.frsv").exists());
        assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 3);
    }
}

#[test]
fn unavailable_source_context_refuses_rotation_and_explicit_load_prepares_it() {
    let directory = tempfile::tempdir().unwrap();
    write_fixture(directory.path(), false);
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let world = seed(&catalogue);
    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    repository.commit(&Captured::at_boundary(&world)).unwrap();
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    let mut without_instances = world.snapshot();
    without_instances.instances.clear();
    without_instances.pending_events.clear();
    without_instances.state_revision += 2;
    let empty = World::restore(&catalogue, without_instances.clone(), Limits::default()).unwrap();
    assert!(matches!(
        repository.commit(&Captured::at_boundary(&empty)),
        Err(save::Error::State(
            fallout_runtime::Error::DefinitionChanged
        ))
    ));
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        current
    );
    assert!(!repository.path().join("previous.frsv").exists());
    let (mut loaded, _) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let handle = loaded.handle(loaded.snapshot().instances[0].id).unwrap();
    loaded.acknowledge(1).unwrap();
    loaded.remove_instance(handle).unwrap();
    assert_eq!(loaded.snapshot(), without_instances);
    repository.commit(&Captured::at_boundary(&loaded)).unwrap();
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        current
    );
}

#[test]
#[ignore = "cold helper launched by source-schema rotation regression"]
fn cold_schema_recovery() {
    let root = PathBuf::from(std::env::var_os("FALLOUT_SCHEMA_RECOVERY_ROOT").unwrap());
    let catalogue = load(&root, &["FalloutNV.esm"]);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    assert!(matches!(
        repository.load(&catalogue, Limits::default(), Recovery::Strict),
        Err(save::Error::State(
            fallout_runtime::Error::IncompatibleLocal(42)
        ))
    ));
    let (world, receipt) = repository
        .load(
            &catalogue,
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(receipt.slot, Slot::Previous);
    assert_eq!(receipt.metadata.generation, 1);
    assert!(
        receipt
            .current_failure
            .unwrap()
            .contains("different storage kind")
    );
    assert!(!receipt.current_repaired);
    assert_eq!(world.snapshot(), seed(&catalogue).snapshot());
}
