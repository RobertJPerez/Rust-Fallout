//! Observe actual schema ownership through the existing queue, rather than
//! inferring its lifetime from serialized-byte admission counters.
use super::*;
use crate::{
    Limits, World,
    events::Context,
    identity::{CampaignId, Owner, Value},
    save::{Recovery, SaveState, SaveStatus, Stage, test_source as common},
    state::DefinitionSchema,
};
use fallout_data::loaded_scripts::Catalogue;
use std::{fs, path::PathBuf, sync::Weak, time::Duration};

struct Directory {
    path: PathBuf,
    _temporary: Option<tempfile::TempDir>,
}
impl Directory {
    fn new(name: &str) -> Self {
        if let Some(root) = std::env::var_os("FALLOUT_SOURCE_CONTEXT_EVIDENCE") {
            let path = PathBuf::from(root).join(name);
            fs::create_dir(&path).unwrap();
            Self {
                path,
                _temporary: None,
            }
        } else {
            let temporary = tempfile::tempdir().unwrap();
            Self {
                path: temporary.path().to_owned(),
                _temporary: Some(temporary),
            }
        }
    }
    fn write(&self, name: &str, value: &impl serde::Serialize) {
        fs::write(
            self.path.join(name),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    }
}

fn source_world(directory: &Directory, limits: Limits) -> (World<'static>, Weak<Catalogue>) {
    let body = common::unit(&[(2, 1), (42, 0)], &[]);
    fs::write(
        directory.path.join("FalloutNV.esm"),
        [
            common::header(&[]),
            common::record(b"SCPT", 0x300, 0, &body),
            common::record(b"SCPT", 0x301, 0, &body),
        ]
        .concat(),
    )
    .unwrap();
    let catalogue = Arc::new(common::load(&directory.path, &["FalloutNV.esm"]));
    let weak = Arc::downgrade(&catalogue);
    let world = World::with_campaign(
        catalogue,
        limits,
        CampaignId::from_bytes([0x74; 16]).unwrap(),
    )
    .unwrap();
    (world, weak)
}

// The second capture has no instance but still needs the old source schema to
// validate the repository's current generation before rotating it.
fn removed_captures(
    directory: &Directory,
) -> (Captured, Captured, Weak<DefinitionSchema>, Weak<Catalogue>) {
    let (mut world, catalogue) = source_world(directory, Limits::default());
    let definition = common::definition(&world.catalogue);
    let instance = world
        .create_instance(
            &definition,
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    world
        .assign(
            instance,
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
        )
        .unwrap();
    let schema = Arc::downgrade(world.definitions.get(&definition.key).unwrap());
    let first = Captured::at_boundary(&world);
    world.remove_instance(instance).unwrap();
    let removed = Captured::at_boundary(&world);
    directory.write("before-removal.json", first.snapshot());
    directory.write("after-removal.json", removed.snapshot());
    drop(world);
    assert!(
        catalogue.upgrade().is_none(),
        "captures must not retain the catalogue"
    );
    assert_eq!(
        schema.strong_count(),
        2,
        "only the two captures retain schemas"
    );
    (first, removed, schema, catalogue)
}

fn repository(directory: &Directory, capture: &Captured) -> Repository {
    Repository::create(
        &directory.path.join("native"),
        &[],
        capture.snapshot().campaign,
    )
    .unwrap()
}
fn gated(
    repository: Repository,
    maximum: usize,
    before_commit: bool,
    panic_at_gate: bool,
) -> (SaveWorker, Receiver<()>, SyncSender<()>) {
    let (entered, entries) = mpsc::sync_channel(1);
    let (release, releases) = mpsc::sync_channel(1);
    let worker = SaveWorker::spawn_with(
        maximum,
        SaveWorker::DEFAULT_MAX_RESERVED_SNAPSHOT_BYTES,
        move |capture| {
            let gate = || {
                entered.send(()).unwrap();
                releases.recv_timeout(Duration::from_secs(10)).unwrap();
                assert!(!panic_at_gate, "injected source-context writer panic");
            };
            if before_commit {
                gate();
            }
            repository.commit_observing(capture, |stage| {
                if !before_commit && stage == Stage::CurrentTempWritten {
                    gate();
                }
            })
        },
    )
    .unwrap();
    (worker, entries, release)
}
fn published(status: &mut SaveStatus, generation: u64) {
    assert!(matches!(status.poll(), SaveState::Published(receipt)
        if receipt.metadata.generation == generation));
}

#[test]
fn source_context_outlives_world_and_removed_instance_but_not_retained_success_status() {
    let directory = Directory::new("success");
    let (first, removed, schema, catalogue) = removed_captures(&directory);
    let before = first.snapshot().clone();
    let after = removed.snapshot().clone();
    let repository = repository(&directory, &first);
    let (mut worker, entered, release) = gated(repository.clone(), 2, false, false);
    let admission = Arc::clone(&worker.admission);
    let mut first_status = SaveStatus::new(worker.try_submit(first).unwrap());
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut removed_status = SaveStatus::new(worker.try_submit(removed).unwrap());
    assert_eq!(schema.strong_count(), 2);
    assert!(catalogue.upgrade().is_none());
    assert!(matches!(first_status.poll(), SaveState::Pending));
    assert!(matches!(removed_status.poll(), SaveState::Pending));
    release.send(()).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    published(&mut first_status, 1);
    assert_eq!(
        schema.strong_count(),
        1,
        "queued capture alone retains schema"
    );
    assert_eq!(admission.usage(), (1, Limits::default().max_snapshot_bytes));
    release.send(()).unwrap();
    worker.finish().unwrap();
    assert!(schema.upgrade().is_none());
    assert_eq!(admission.usage(), (0, 0));
    published(&mut first_status, 1);
    published(&mut removed_status, 2);
    published(&mut removed_status, 2);
    let cold_catalogue = common::load(&directory.path, &["FalloutNV.esm"]);
    assert_eq!(
        repository
            .load(&cold_catalogue, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        after
    );
    assert_eq!(
        super::super::format::decode(
            &fs::read(repository.path().join("previous.frsv")).unwrap(),
            Limits::default()
        )
        .unwrap()
        .snapshot,
        before
    );
    directory.write("lifetime.json", &serde_json::json!({"schema_owners": [2, 1, 0], "catalogue_retained": false,
        "generations": [first_status.wait().unwrap().metadata.generation, removed_status.wait().unwrap().metadata.generation],
        "terminal_reservations": admission.usage()}));
}

#[test]
fn source_context_releases_before_sticky_actual_repository_failure_is_retained() {
    let directory = Directory::new("busy");
    let (first, removed, schema, _) = removed_captures(&directory);
    let repository = repository(&directory, &first);
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(repository.path().join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let (mut worker, entered, release) = gated(repository.clone(), 2, true, false);
    let admission = Arc::clone(&worker.admission);
    let mut first_status = SaveStatus::new(worker.try_submit(first).unwrap());
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut removed_status = SaveStatus::new(worker.try_submit(removed).unwrap());
    assert_eq!(schema.strong_count(), 2);
    release.send(()).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(matches!(
        first_status.poll(),
        SaveState::Failed(CompletionError::Save(super::super::Error::Busy))
    ));
    assert_eq!(schema.strong_count(), 1);
    release.send(()).unwrap();
    worker.finish().unwrap();
    assert!(schema.upgrade().is_none());
    assert_eq!(admission.usage(), (0, 0));
    for status in [&mut first_status, &mut removed_status] {
        assert!(matches!(
            status.poll(),
            SaveState::Failed(CompletionError::Save(super::super::Error::Busy))
        ));
        assert!(matches!(
            status.poll(),
            SaveState::Failed(CompletionError::Save(super::super::Error::Busy))
        ));
    }
    assert!(!repository.path().join("current.frsv").exists());
    directory.write("lifetime.json", &serde_json::json!({"schema_owners": [2, 1, 0], "failure": "repository_busy", "terminal_reservations": admission.usage()}));
    drop(lock);
}

#[test]
fn writer_panic_releases_active_and_queued_source_contexts_even_with_live_statuses() {
    let directory = Directory::new("panic");
    let (first, removed, schema, _) = removed_captures(&directory);
    let repository = repository(&directory, &first);
    let (mut worker, entered, release) = gated(repository.clone(), 2, false, true);
    let admission = Arc::clone(&worker.admission);
    let mut first_status = SaveStatus::new(worker.try_submit(first).unwrap());
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut removed_status = SaveStatus::new(worker.try_submit(removed).unwrap());
    assert_eq!(schema.strong_count(), 2);
    release.send(()).unwrap();
    assert!(matches!(worker.finish(), Err(WorkerError::Panicked)));
    assert!(schema.upgrade().is_none());
    assert_eq!(admission.usage(), (0, 0));
    for status in [&mut first_status, &mut removed_status] {
        assert!(matches!(
            status.poll(),
            SaveState::Failed(CompletionError::WorkerStopped)
        ));
        assert!(matches!(
            status.poll(),
            SaveState::Failed(CompletionError::WorkerStopped)
        ));
    }
    assert!(!repository.path().join("current.frsv").exists());
    assert!(!repository.path().join("previous.frsv").exists());
    directory.write("lifetime.json", &serde_json::json!({"schema_owners": [2, 0], "failure": "worker_stopped", "terminal_reservations": admission.usage()}));
}

#[test]
fn rejected_capture_owns_its_source_context_until_host_releases_it() {
    let directory = Directory::new("rejection");
    let (first, removed, schema, _) = removed_captures(&directory);
    let expected = removed.snapshot().clone();
    let repository = repository(&directory, &first);
    let expected_published = first.snapshot().clone();
    let (mut worker, entered, release) = gated(repository.clone(), 1, false, false);
    let admission = Arc::clone(&worker.admission);
    let ticket = worker.try_submit(first).unwrap();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let rejection = worker.try_submit(removed).unwrap_err();
    assert_eq!(rejection.reason, Rejection::Capacity);
    assert_eq!(rejection.capture.snapshot(), &expected);
    assert_eq!(schema.strong_count(), 2);
    assert_eq!(admission.usage(), (1, Limits::default().max_snapshot_bytes));
    drop(rejection);
    assert_eq!(schema.strong_count(), 1);
    drop(ticket); // Consumer disconnection cannot cancel the accepted publication.
    release.send(()).unwrap();
    worker.finish().unwrap();
    assert!(schema.upgrade().is_none());
    assert_eq!(admission.usage(), (0, 0));
    let source = common::load(&directory.path, &["FalloutNV.esm"]);
    assert_eq!(
        repository
            .load(&source, Limits::default(), Recovery::Strict)
            .unwrap()
            .0
            .snapshot(),
        expected_published
    );
    directory.write("lifetime.json", &serde_json::json!({"schema_owners": [2, 1, 0], "terminal_reservations": admission.usage()}));
}

#[test]
fn over_limit_removed_definition_cache_retains_no_schemas_and_cannot_rotate_current() {
    let directory = Directory::new("bounded-refusal");
    let limits = Limits {
        max_instances: 1,
        ..Limits::default()
    };
    let (mut world, catalogue) = source_world(&directory, limits);
    let definitions: Vec<_> = world
        .catalogue
        .iter()
        .map(|(_, script)| script.handle().clone())
        .collect();
    let first = world
        .create_instance(
            &definitions[0],
            Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let schema1 = Arc::downgrade(world.definitions.get(&definitions[0].key).unwrap());
    let capture = Captured::at_boundary(&world);
    directory.write("admitted-snapshot.json", capture.snapshot());
    let repository = repository(&directory, &capture);
    repository.commit(&capture).unwrap();
    drop(capture);
    let current = fs::read(repository.path().join("current.frsv")).unwrap();
    world.remove_instance(first).unwrap();
    let second = world
        .create_instance(
            &definitions[1],
            Owner::Fragment {
                activation: 2.try_into().unwrap(),
            },
            Context::default(),
        )
        .unwrap();
    let schema2 = Arc::downgrade(world.definitions.get(&definitions[1].key).unwrap());
    world.remove_instance(second).unwrap();
    assert_eq!(world.definitions.len(), 2);
    assert!(world.snapshot().instances.is_empty());
    let refused = Captured::at_boundary(&world);
    directory.write("over-limit-snapshot.json", refused.snapshot());
    assert_eq!(
        schema1.strong_count(),
        1,
        "refused capture must not clone even the first schema"
    );
    assert_eq!(schema2.strong_count(), 1);
    drop(world);
    assert!(schema1.upgrade().is_none());
    assert!(schema2.upgrade().is_none());
    assert!(catalogue.upgrade().is_none());
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let admission = Arc::clone(&worker.admission);
    let mut status = SaveStatus::new(worker.try_submit(refused).unwrap());
    worker.finish().unwrap();
    assert!(matches!(
        status.poll(),
        SaveState::Failed(CompletionError::Save(super::super::Error::State(
            crate::Error::Capacity("publication source definitions")
        )))
    ));
    assert_eq!(admission.usage(), (0, 0));
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        current
    );
    assert!(!repository.path().join("previous.frsv").exists());
    assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 3);
    directory.write("lifetime.json", &serde_json::json!({"cached_definitions": 2, "definition_limit": 1,
        "retained_schemas_at_capture": 0, "failure": "publication source definitions", "terminal_reservations": admission.usage()}));
}
