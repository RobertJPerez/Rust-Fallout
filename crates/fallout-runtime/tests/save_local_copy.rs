mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        expression::{Operator, Operators},
        expression_plan::Model,
    },
};
use fallout_runtime::{
    Limits, World,
    events::{Context, Trigger},
    execution::local_copy::{Intent, Preparation, StagedCopy, Unsupported},
    foreign::Content,
    identity::{CampaignId, Owner, Value},
    programs::PreparedSources,
    save::{
        self, Captured, CompletionError, Recovery, Rejection, Repository, SaveWorker, Slot, format,
    },
    snapshot::Snapshot,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

const BITS: u64 = 0x7ff8_1234_5678_9abc;
const BUDGET: usize = 4096;
fn limits() -> Limits {
    Limits {
        max_snapshot_bytes: BUDGET,
        ..Default::default()
    }
}
fn fixture(name: &str) -> (Option<tempfile::TempDir>, PathBuf, Arc<Catalogue>, Content) {
    let (temporary, root) = match std::env::var_os("FALLOUT_SAVE_COPY_EVIDENCE") {
        Some(root) => {
            let root = PathBuf::from(root).join(name);
            // Evidence runs must have fresh directories, never replace a proof.
            fs::create_dir(&root).unwrap();
            (None, root)
        }
        None => {
            let temporary = tempfile::tempdir().unwrap();
            let root = temporary.path().to_path_buf();
            (Some(temporary), root)
        }
    };
    // Authored source-less Begin / set own short2 to float1 / End, fixed bytes.
    let compiled = [
        0x10, 0, 6, 0, 0, 0, 16, 0, 0, 0, 0x15, 0, 8, 0, b's', 2, 0, 3, 0, b'f', 1, 0, 0x11, 0, 0,
        0,
    ];
    let original = unit(&[(1, 0), (2, 1)], &[]);
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", &compiled));
    source.extend(&original[46..]);
    fs::write(
        root.join("FalloutNV.esm"),
        [header(&[]), record(b"SCPT", 0x300, 0, &source)].concat(),
    )
    .unwrap();
    let (catalogue, content) = load_content(&root);
    (temporary, root, catalogue, content)
}
fn load_content(root: &Path) -> (Arc<Catalogue>, Content) {
    let mut store = fallout_data::store::RecordStore::open_nv_headers(
        root,
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn prepared_sources(catalogue: &Catalogue) -> PreparedSources<'_> {
    let operators = Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(code, text)| Operator {
            code: code as u32,
            precedence: code as u8,
            spelling: text.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Default::default(),
        Default::default(),
    )
    .unwrap()
}
fn seed(catalogue: Arc<Catalogue>) -> World<'static> {
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        limits(),
        CampaignId::from_bytes([0x4b; 16]).unwrap(),
    )
    .unwrap();
    let handle = world
        .create_instance(
            &definition,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(handle, &[(1, Value::Number { bits: BITS })])
        .unwrap();
    for _ in 0..2 {
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
    }
    world
}
fn stage(world: &World<'_>, sources: &PreparedSources<'_>, content: &Content) -> Box<StagedCopy> {
    let head = world.pending_events().next().unwrap().sequence;
    match world
        .stage_source_local_copy_with_sources(
            head,
            sources,
            content,
            Intent::Engineering,
            Default::default(),
        )
        .unwrap()
    {
        Preparation::Staged(stage) => stage,
        other => panic!("{other:?}"),
    }
}
fn commit(
    world: &mut World<'_>,
    sources: &PreparedSources<'_>,
    content: &Content,
) -> serde_json::Value {
    let before = world.snapshot();
    let stage = stage(world, sources, content);
    assert_eq!(stage.trace().statement_scda_bytes, 10..22);
    assert_eq!(stage.trace().source_token_scda_bytes, 19..22);
    assert_eq!(
        stage.changes().assignments(),
        [(2, Value::Number { bits: BITS })]
    );
    assert_eq!(world.snapshot(), before);
    let committed = (*stage).commit(world).unwrap();
    assert_eq!(
        committed.receipt.acknowledged.as_ref(),
        Some(&before.pending_events[0])
    );
    assert_eq!(committed.receipt.after_revision, before.state_revision + 1);
    let mut expected = before;
    expected.instances[0]
        .locals
        .iter_mut()
        .find(|local| local.index == 2)
        .unwrap()
        .value = Value::Number { bits: BITS };
    expected.state_revision += 1;
    expected.pending_events.remove(0);
    assert_eq!(world.snapshot(), expected);
    serde_json::to_value(committed).unwrap()
}
fn slot_files(repository: &Repository) -> BTreeMap<String, String> {
    ["current.frsv", "previous.frsv"]
        .into_iter()
        .filter_map(|slot| {
            let path = repository.path().join(slot);
            path.exists().then(|| {
                (
                    slot.into(),
                    format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                )
            })
        })
        .collect()
}
fn cold(
    root: &Path,
    repository: &str,
    expected: &str,
    recovery: &str,
    generation: u64,
) -> serde_json::Value {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_native_copy_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_SAVE_COPY_COLD_ROOT", root)
        .env("FALLOUT_SAVE_COPY_COLD_REPOSITORY", repository)
        .env("FALLOUT_SAVE_COPY_COLD_EXPECTED", expected)
        .env("FALLOUT_SAVE_COPY_COLD_RECOVERY", recovery)
        .env("FALLOUT_SAVE_COPY_COLD_GENERATION", generation.to_string())
        .output()
        .unwrap();
    let name = format!("cold-{repository}-{recovery}-{expected}");
    fs::write(root.join(format!("{name}.stdout.txt")), &output.stdout).unwrap();
    fs::write(root.join(format!("{name}.stderr.txt")), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find_map(|line| line.strip_prefix("SAVE_COPY_COLD_RECEIPT "))
        .unwrap();
    serde_json::from_str(line).unwrap()
}
fn expected(root: &Path, name: &str, snapshot: &Snapshot) {
    fs::write(root.join(name), snapshot.encode(BUDGET).unwrap()).unwrap();
}

#[test]
fn source_copy_worker_boundaries_cold_recovery_and_fresh_replay_are_exact() {
    let (_temporary, root, catalogue, content) = fixture("boundaries");
    let sources = prepared_sources(&catalogue);
    let mut world = seed(Arc::clone(&catalogue));
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let before = world.snapshot();
    expected(&root, "before.json", &before);
    let mut worker = SaveWorker::start_with_budget(repository.clone(), 8, 2 * BUDGET).unwrap();
    let first = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let committed = commit(&mut world, &sources, &content);
    let after = world.snapshot();
    expected(&root, "after.json", &after);
    let second = worker.try_submit(Captured::at_boundary(&world)).unwrap();
    let old_handle = world.handle(after.instances[0].id).unwrap();
    world
        .assign(old_handle, &[(1, Value::Number { bits: 17 })])
        .unwrap();
    drop(world);
    drop(sources);
    drop(content);
    drop(catalogue);
    worker.finish().unwrap();
    let first_receipt = first.wait().unwrap();
    let second_receipt = second.wait().unwrap();
    assert_eq!(first_receipt.metadata.generation, 1);
    assert_eq!(second_receipt.metadata.generation, 2);
    for (slot, snapshot) in [("previous", &before), ("current", &after)] {
        let bytes = fs::read(repository.path().join(format!("{slot}.frsv"))).unwrap();
        assert_eq!(
            format::decode(&bytes, limits()).unwrap().snapshot,
            *snapshot
        );
        fs::write(root.join(format!("golden-{slot}.frsv")), bytes).unwrap();
    }
    let current = cold(&root, "native", "after.json", "strict", 2);
    let recovery = Repository::create(&root.join("recovery"), &[], before.campaign).unwrap();
    fs::write(
        recovery.path().join("current.frsv"),
        b"truncated copy current",
    )
    .unwrap();
    fs::copy(
        root.join("golden-previous.frsv"),
        recovery.path().join("previous.frsv"),
    )
    .unwrap();
    let previous = cold(&root, "recovery", "before.json", "previous", 1);
    let (catalogue, content) = load_content(&root);
    let sources = prepared_sources(&catalogue);
    let (mut replay, repair) = recovery
        .recover_previous(catalogue.as_ref(), limits())
        .unwrap();
    assert!(repair.current_repaired);
    assert!(replay.instance(old_handle).is_err());
    assert_eq!(replay.snapshot(), before);
    let replay_trace = commit(&mut replay, &sources, &content);
    assert_eq!(replay.snapshot(), after);
    let mut restarted = SaveWorker::start_with_budget(recovery.clone(), 1, BUDGET).unwrap();
    let ticket = restarted
        .try_submit(Captured::at_boundary(&replay))
        .unwrap();
    restarted.finish().unwrap();
    assert_eq!(ticket.wait().unwrap().metadata, second_receipt.metadata);
    assert_eq!(slot_files(&recovery), slot_files(&repository));
    let replay_cold = cold(&root, "recovery", "after.json", "strict", 2);
    fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&json!({"first":first_receipt,"second":second_receipt,"committed":committed,"cold_current":current,"cold_previous":previous,"repair":repair,"replay":replay_trace,"cold_replay":replay_cold,"slots":slot_files(&repository),"engineering_only":true})).unwrap()).unwrap();
}

#[test]
fn source_copy_failed_writer_intact_rejection_dropped_ticket_and_restart_preserve_authority() {
    let (_temporary, root, catalogue, content) = fixture("failures");
    let sources = prepared_sources(&catalogue);
    let mut world = seed(Arc::clone(&catalogue));
    let repository = Repository::create(&root.join("native"), &[], world.campaign()).unwrap();
    let before = world.snapshot();
    let old = Captured::at_boundary(&world);
    repository.commit(&old).unwrap();
    let files = slot_files(&repository);
    drop(stage(&world, &sources, &content));
    assert!(matches!(
        world
            .stage_source_local_copy_with_sources(
                1,
                &sources,
                &content,
                Intent::Faithful,
                Default::default()
            )
            .unwrap(),
        Preparation::Unsupported {
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert_eq!(world.snapshot(), before);
    assert_eq!(slot_files(&repository), files);
    let committed = commit(&mut world, &sources, &content);
    let after = world.snapshot();
    expected(&root, "after.json", &after);
    let mut too_small = SaveWorker::start_with_budget(repository.clone(), 8, BUDGET - 1).unwrap();
    let capture = Captured::at_boundary(&world);
    let pointer = capture.snapshot().catalogue_sha256.as_ptr();
    let rejected = too_small.try_submit(capture).unwrap_err();
    assert_eq!(rejected.reason, Rejection::Capacity);
    assert_eq!(rejected.capture.snapshot(), &after);
    assert_eq!(
        rejected.capture.snapshot().catalogue_sha256.as_ptr(),
        pointer
    );
    too_small.finish().unwrap();
    assert_eq!(slot_files(&repository), files);
    let mut worker = SaveWorker::start_with_budget(repository.clone(), 1, BUDGET).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let failed = worker.try_submit(rejected.capture).unwrap();
    assert!(matches!(
        failed.wait(),
        Err(CompletionError::Save(save::Error::Busy))
    ));
    assert_eq!(world.snapshot(), after);
    assert_eq!(slot_files(&repository), files);
    drop(lock);
    drop(worker.try_submit(Captured::at_boundary(&world)).unwrap());
    worker.finish().unwrap();
    let after_files = slot_files(&repository);
    let mut restarted = SaveWorker::start_with_budget(repository.clone(), 1, BUDGET).unwrap();
    let stale = restarted.try_submit(old).unwrap();
    let stale_error = stale.wait().unwrap_err().to_string();
    assert_eq!(
        stale_error,
        "native save format: captured request would replace newer canonical state"
    );
    assert_eq!(slot_files(&repository), after_files);
    let current = cold(&root, "native", "after.json", "strict", 2);
    let (mut restored, _) = repository
        .load(catalogue.as_ref(), limits(), Recovery::Strict)
        .unwrap();
    let tail = commit(&mut restored, &sources, &content);
    let completed = restored.snapshot();
    expected(&root, "completed.json", &completed);
    let ticket = restarted
        .try_submit(Captured::at_boundary(&restored))
        .unwrap();
    restarted.finish().unwrap();
    assert_eq!(ticket.wait().unwrap().metadata.generation, 3);
    let tail_cold = cold(&root, "native", "completed.json", "strict", 3);
    fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&json!({"committed":committed,"busy":"native save repository is busy","rejection":"capacity","rejected_capture_intact":true,"dropped_ticket_save_completed":true,"stale_error":stale_error,"slots_after_first_commit":files,"slots_after_dropped_ticket":after_files,"cold_after_dropped_ticket":current,"tail":tail,"cold_tail":tail_cold,"slots_final":slot_files(&repository),"engineering_only":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "parent launches fresh processes over private native repositories"]
fn cold_native_copy_helper() {
    let root = PathBuf::from(std::env::var_os("FALLOUT_SAVE_COPY_COLD_ROOT").unwrap());
    let (catalogue, _) = load_content(&root);
    let repository = Repository::open(
        &root.join(std::env::var("FALLOUT_SAVE_COPY_COLD_REPOSITORY").unwrap()),
        &[],
    )
    .unwrap();
    let files = slot_files(&repository);
    let policy = std::env::var("FALLOUT_SAVE_COPY_COLD_RECOVERY").unwrap();
    let recovery = if policy == "previous" {
        assert_eq!(
            repository
                .load(catalogue.as_ref(), limits(), Recovery::Strict)
                .err()
                .unwrap()
                .to_string(),
            "native save format: container byte budget/extent"
        );
        Recovery::PreviousIfCurrentInvalid
    } else {
        assert_eq!(policy, "strict");
        Recovery::Strict
    };
    let expected = Snapshot::decode(
        &fs::read(root.join(std::env::var("FALLOUT_SAVE_COPY_COLD_EXPECTED").unwrap())).unwrap(),
        limits(),
    )
    .unwrap();
    let (world, receipt) = repository
        .load(catalogue.as_ref(), limits(), recovery)
        .unwrap();
    assert_eq!(world.snapshot(), expected);
    assert_eq!(
        receipt.metadata.generation,
        std::env::var("FALLOUT_SAVE_COPY_COLD_GENERATION")
            .unwrap()
            .parse::<u64>()
            .unwrap()
    );
    assert_eq!(
        receipt.slot,
        if policy == "previous" {
            Slot::Previous
        } else {
            Slot::Current
        }
    );
    assert!(!receipt.current_repaired);
    assert_eq!(files, slot_files(&repository));
    let bytes = world.snapshot().encode(BUDGET).unwrap();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(hash, receipt.metadata.snapshot_sha256);
    println!(
        "SAVE_COPY_COLD_RECEIPT {}",
        json!({"receipt":receipt,"snapshot_sha256":hash,"pending":world.snapshot().pending_events,"snapshot_bytes":bytes.len(),"slots":files,"source_bound_restore":true,"cold_files_unchanged":true})
    );
}
