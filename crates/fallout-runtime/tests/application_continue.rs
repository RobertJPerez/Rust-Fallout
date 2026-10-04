mod common;
use common::*;
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore};
use fallout_runtime::application::{
    self, ContinueBoundary, ContinueRequest, Failure, Host, HostLimits, ScenePublisher,
};
use fallout_runtime::{
    Limits, World,
    events::{Context, Trigger},
    foreign::Content,
    identity::{Owner, ReferenceId, ReferenceValue, Value},
    inventory::{Facts, ItemId},
    save::{
        Captured, Recovery, Repository, RestorePoll, RestoreTask, RestoredCandidate, SaveWorker,
    },
    snapshot::Snapshot,
    source_items::{Policy, Role},
    state::initialization,
};
use std::{num::NonZeroU64, path::Path, sync::Arc};

fn id(n: u64) -> NonZeroU64 {
    n.try_into().unwrap()
}
fn policy() -> Policy {
    Policy::new(&[(Role::Base, &[*b"MISC"])]).unwrap()
}
fn sources(root: &Path) -> (Arc<Catalogue>, Arc<Content>) {
    let catalogue = Arc::new(load(root, &["FalloutNV.esm"]));
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
    (catalogue, content)
}
struct Fixture {
    root: tempfile::TempDir,
    catalogue: Arc<Catalogue>,
    repository: Repository,
    host: Host<'static>,
    a: ReferenceId,
    b: ReferenceId,
    lot: ItemId,
    saved: Snapshot,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    write_fixture(root.path(), false);
    let source = root.path().join("FalloutNV.esm");
    let mut bytes = std::fs::read(&source).unwrap();
    for (kind, key) in [
        (b"MISC", 0x110),
        (b"REFR", 0x500),
        (b"ACHR", 0x501),
        (b"QUST", 0x800),
    ] {
        bytes.extend(record(kind, key, 0, &[]));
    }
    std::fs::write(source, bytes).unwrap();
    let (catalogue, content) = sources(root.path());
    let mut world = World::new(Arc::clone(&catalogue), Limits::default()).unwrap();
    let a = world.register_reference(Some(form(0x500))).unwrap();
    let b = world.register_reference(Some(form(0x501))).unwrap();
    world.initialize_inventory(a).unwrap();
    world.initialize_inventory(b).unwrap();
    let lot = world
        .add_source_item(
            &content,
            &policy(),
            a,
            Facts::unknown(form(0x110)),
            8.try_into().unwrap(),
        )
        .unwrap()
        .0;
    let context = Context {
        calling_reference: Some(a),
        containing_reference: Some(a),
        target: Some(ReferenceValue::Live { id: b }),
        arguments: vec![ReferenceValue::Null],
    };
    let stage = world
        .stage_instance_initialization(
            &definition(&catalogue),
            &Owner::Quest { key: form(0x800) },
            &context,
            &[
                (
                    2,
                    Value::Number {
                        bits: 1337_f64.to_bits(),
                    },
                ),
                (
                    42,
                    Value::Number {
                        bits: 0x7ff8_1234_5678_9abc,
                    },
                ),
                (
                    90,
                    Value::Reference {
                        value: ReferenceValue::Live { id: b },
                    },
                ),
            ],
            initialization::Limits::default(),
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
            context,
        )
        .unwrap();
    let saved = world.snapshot();
    let repository =
        Repository::create(&root.path().join("native"), &[], world.campaign()).unwrap();
    save(&repository, &world);
    let host = Host::new(world, content, policy(), id(7), HostLimits::default()).unwrap();
    Fixture {
        root,
        catalogue,
        repository,
        host,
        a,
        b,
        lot,
        saved,
    }
}
fn save(repository: &Repository, world: &World<'_>) {
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let ticket = worker.try_submit(Captured::at_boundary(world)).unwrap();
    worker.finish().unwrap();
    assert_eq!(
        ticket.wait().unwrap().metadata.state_revision,
        world.revision()
    );
}
fn candidate(f: &Fixture, request: &ContinueRequest) -> (RestoreTask, RestoredCandidate) {
    let mut task = RestoreTask::start(
        f.repository.clone(),
        Arc::clone(&f.catalogue),
        Limits::default(),
        Recovery::Strict,
        request.identity().clone(),
    )
    .unwrap();
    task.finish().unwrap();
    let RestorePoll::Ready(candidate) = task.try_poll() else {
        panic!("expected native candidate")
    };
    (task, *candidate)
}

// This is an independently authored application fixture, not a GPU/Native Host
// proof. Its immutable input keys model the complete-set gate in the old host.
struct Scene {
    host_identity: application::HostIdentity,
    generation: NonZeroU64,
    displayed: Snapshot,
    active_keys: Vec<fallout_data::identity::FormKey>,
    prepared_keys: Vec<fallout_data::identity::FormKey>,
    publishes: usize,
}
impl Scene {
    fn current(host: &Host<'_>) -> Self {
        Self {
            host_identity: host.identity(),
            generation: host.scene_generation(),
            displayed: host.world().snapshot(),
            active_keys: vec![form(0x500), form(0x501)],
            prepared_keys: vec![form(0x500), form(0x501)],
            publishes: 0,
        }
    }
}
impl ScenePublisher for Scene {
    type Stage = Snapshot;
    fn prepare(
        &self,
        world: &World<'_>,
        boundary: &ContinueBoundary,
    ) -> application::Result<Snapshot> {
        if self.host_identity != boundary.prior_host_identity()
            || self.generation != boundary.scene_generation()
            || self.displayed.state_revision != boundary.prior_revision()
        {
            return Err(Failure::Refused("scene boundary changed"));
        }
        if !self
            .active_keys
            .iter()
            .all(|key| self.prepared_keys.contains(key) && world.authored_reference(key).is_some())
        {
            return Err(Failure::Refused(
                "Continue does not bind every active source view",
            ));
        }
        Ok(world.snapshot())
    }
    fn publish(&mut self, stage: Snapshot, boundary: &ContinueBoundary) {
        self.displayed = stage;
        self.host_identity = boundary.candidate_host_identity();
        self.publishes += 1;
    }
}

#[test]
fn late_membership_refusal_preserves_complete_canonical_and_displayed_state() {
    let mut f = fixture();
    let command = f
        .host
        .select_transfer(f.a, f.lot, f.b, 8)
        .unwrap()
        .command(id(1));
    f.host.transfer(command.clone()).unwrap();
    let active = f.host.world().snapshot();
    assert_ne!(active, f.saved);
    let mut scene = Scene::current(&f.host);
    scene.prepared_keys.pop();
    let request = f.host.begin_continue(id(1)).unwrap();
    let (_task, candidate) = candidate(&f, &request);
    let prepared = f.host.prepare_continue(request, candidate).unwrap();
    assert_eq!(prepared.world().snapshot(), f.saved);
    assert_eq!(f.host.world().snapshot(), active);
    assert!(matches!(
        f.host.publish_continue(prepared, &mut scene),
        Err(Failure::Refused(
            "Continue does not bind every active source view"
        ))
    ));
    assert_eq!(f.host.world().snapshot(), active);
    assert_eq!(scene.displayed, active);
    assert_eq!(scene.publishes, 0);
    assert!(f.host.transfer(command).unwrap().replayed);
}

#[test]
fn admitted_continue_publishes_same_state_once_and_expires_old_transfer_selections() {
    let mut f = fixture();
    let command = f
        .host
        .select_transfer(f.a, f.lot, f.b, 8)
        .unwrap()
        .command(id(1));
    f.host.transfer(command.clone()).unwrap();
    let active_revision = f.host.world().revision();
    let prior_host = f.host.identity();
    let mut scene = Scene::current(&f.host);
    let request = f.host.begin_continue(id(1)).unwrap();
    let (_task, candidate) = candidate(&f, &request);
    let prepared = f.host.prepare_continue(request, candidate).unwrap();
    assert_eq!(
        prepared.native_receipt().metadata.state_revision,
        f.saved.state_revision
    );
    let receipt = f.host.publish_continue(prepared, &mut scene).unwrap();
    assert_eq!(receipt.boundary.prior_revision(), active_revision);
    assert_eq!(
        receipt.boundary.candidate_revision(),
        f.saved.state_revision
    );
    assert_eq!(
        receipt.native.metadata.state_revision,
        f.saved.state_revision
    );
    assert_eq!(f.host.world().snapshot(), f.saved);
    assert_eq!(scene.displayed, f.saved);
    assert_eq!(scene.publishes, 1);
    assert_ne!(f.host.identity(), prior_host);
    assert_eq!(scene.host_identity, f.host.identity());
    assert!(matches!(
        f.host.transfer(command),
        Err(Failure::ExpiredSelection)
    ));
    assert!(f.host.begin_continue(id(1)).is_err());
}

#[test]
fn canonical_mutation_between_request_and_candidate_refuses_restore_without_losing_effect() {
    let mut f = fixture();
    let request = f.host.begin_continue(id(1)).unwrap();
    let (_task, candidate) = candidate(&f, &request);
    let command = f
        .host
        .select_transfer(f.a, f.lot, f.b, 8)
        .unwrap()
        .command(id(1));
    f.host.transfer(command).unwrap();
    let active = f.host.world().snapshot();
    assert!(matches!(
        f.host.prepare_continue(request, candidate),
        Err(Failure::RevisionChanged)
    ));
    assert_eq!(f.host.world().snapshot(), active);
}

#[test]
fn cancelled_superseded_and_old_scene_candidates_never_publish() {
    let mut f = fixture();
    let active = f.host.world().snapshot();
    let request = f.host.begin_continue(id(1)).unwrap();
    let (_task, first) = candidate(&f, &request);
    let newer = f.host.begin_continue(id(2)).unwrap();
    assert!(f.host.prepare_continue(request, first).is_err());
    let (_task, second) = candidate(&f, &newer);
    assert!(f.host.cancel_continue(&newer));
    assert!(!f.host.cancel_continue(&newer));
    assert!(f.host.prepare_continue(newer, second).is_err());
    let request = f.host.begin_continue(id(3)).unwrap();
    let (_task, third) = candidate(&f, &request);
    f.host.advance_scene(id(8)).unwrap();
    assert!(matches!(
        f.host.prepare_continue(request, third),
        Err(Failure::ExpiredSelection)
    ));
    let request = f.host.begin_continue(id(4)).unwrap();
    let (mut task, fourth) = candidate(&f, &request);
    assert!(task.cancel());
    assert!(matches!(
        f.host.prepare_continue(request, fourth),
        Err(Failure::Restore(_))
    ));
    assert_eq!(f.host.world().snapshot(), active);
}

#[test]
fn scene_generation_or_display_revision_change_after_preparation_preserves_both_authorities() {
    for generation_change in [false, true] {
        let mut f = fixture();
        let active = f.host.world().snapshot();
        let mut scene = Scene::current(&f.host);
        let request = f.host.begin_continue(id(1)).unwrap();
        let (_task, candidate) = candidate(&f, &request);
        let prepared = f.host.prepare_continue(request, candidate).unwrap();
        if generation_change {
            scene.generation = id(8);
        } else {
            scene.displayed.state_revision += 1;
        }
        let displayed = scene.displayed.clone();
        assert!(f.host.publish_continue(prepared, &mut scene).is_err());
        assert_eq!(f.host.world().snapshot(), active);
        assert_eq!(scene.displayed, displayed);
        assert_eq!(scene.publishes, 0);
    }
}

#[test]
fn cancellation_supersession_or_canonical_change_after_preparation_cannot_publish() {
    for change in 0..4 {
        let mut f = fixture();
        let mut scene = Scene::current(&f.host);
        let displayed = scene.displayed.clone();
        let request = f.host.begin_continue(id(1)).unwrap();
        let (_task, candidate) = candidate(&f, &request);
        let prepared = f.host.prepare_continue(request.clone(), candidate).unwrap();
        match change {
            0 => assert!(f.host.cancel_continue(&request)),
            1 => {
                f.host.begin_continue(id(2)).unwrap();
            }
            2 => {
                let command = f
                    .host
                    .select_transfer(f.a, f.lot, f.b, 8)
                    .unwrap()
                    .command(id(1));
                f.host.transfer(command).unwrap();
            }
            _ => f.host.advance_scene(id(8)).unwrap(),
        }
        let active = f.host.world().snapshot();
        assert!(f.host.publish_continue(prepared, &mut scene).is_err());
        assert_eq!(f.host.world().snapshot(), active);
        assert_eq!(scene.displayed, displayed);
        assert_eq!(scene.publishes, 0);
    }
}

#[test]
fn equal_revision_restore_expires_prior_acknowledgement_and_allows_new_host_request() {
    let mut f = fixture();
    let command = f
        .host
        .select_transfer(f.a, f.lot, f.b, 8)
        .unwrap()
        .command(id(1));
    let old = f.host.transfer(command.clone()).unwrap();
    assert_eq!(old.host_identity(), f.host.identity());
    assert_eq!(old.scene_generation(), f.host.scene_generation());
    save(&f.repository, f.host.world());
    let active = f.host.world().snapshot();
    let mut scene = Scene::current(&f.host);
    let request = f.host.begin_continue(id(1)).unwrap();
    let (_task, candidate) = candidate(&f, &request);
    let prepared = f.host.prepare_continue(request, candidate).unwrap();
    let receipt = f.host.publish_continue(prepared, &mut scene).unwrap();
    assert_eq!(receipt.boundary.prior_revision(), active.state_revision);
    assert_eq!(receipt.boundary.candidate_revision(), active.state_revision);
    assert_eq!(f.host.world().snapshot(), active);
    assert_eq!(scene.displayed, active);
    // The numeric scene and revision are deliberately identical. A consumer
    // rejects the old acknowledgement by the identity of its canonical host.
    assert_eq!(old.scene_generation(), f.host.scene_generation());
    assert_ne!(old.host_identity(), f.host.identity());
    assert!(matches!(
        f.host.transfer(command),
        Err(Failure::ExpiredSelection)
    ));
    let command = f
        .host
        .select_transfer(f.b, f.lot, f.a, 8)
        .unwrap()
        .command(id(1));
    let current = f.host.transfer(command).unwrap();
    assert!(!current.replayed);
    assert_eq!(current.host_identity(), f.host.identity());
    assert_eq!(current.scene_generation(), f.host.scene_generation());
    assert_eq!(f.host.world().item(f.lot).unwrap().owner(), f.a);
    assert_eq!(f.host.world().revision(), active.state_revision + 1);
}

#[test]
fn native_save_and_fresh_continue_preserve_exact_inventory_quest_locals_and_pending_events() {
    let mut f = fixture();
    let command = f
        .host
        .select_transfer(f.a, f.lot, f.b, 8)
        .unwrap()
        .command(id(1));
    f.host.transfer(command).unwrap();
    save(&f.repository, f.host.world());
    let expected = f.host.world().snapshot();
    assert_eq!(
        expected.instances[0].owner,
        Owner::Quest { key: form(0x800) }
    );
    assert_eq!(expected.pending_events.len(), 1);
    let root = f.root.path();
    std::fs::write(
        root.join("expected.json"),
        expected.encode(1 << 20).unwrap(),
    )
    .unwrap();
    let native = std::fs::read(f.repository.path().join("current.frsv")).unwrap();
    let source = std::fs::read(root.join("FalloutNV.esm")).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cold_continue_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("FALLOUT_APPLICATION_COLD_ROOT", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = Snapshot::decode(
        &std::fs::read(root.join("cold-restored.json")).unwrap(),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        std::fs::read(f.repository.path().join("current.frsv")).unwrap(),
        native
    );
    assert_eq!(std::fs::read(root.join("FalloutNV.esm")).unwrap(), source);
}

#[test]
#[ignore = "fresh process application consumer invoked by parent"]
fn cold_continue_helper() {
    let root = std::path::PathBuf::from(std::env::var_os("FALLOUT_APPLICATION_COLD_ROOT").unwrap());
    let (catalogue, content) = sources(&root);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let empty = World::with_campaign(
        Arc::clone(&catalogue),
        Limits::default(),
        repository.campaign(),
    )
    .unwrap();
    let mut host = Host::new(empty, content, policy(), id(7), HostLimits::default()).unwrap();
    let mut scene = Scene::current(&host);
    let request = host.begin_continue(id(1)).unwrap();
    let mut task = RestoreTask::start(
        repository,
        catalogue,
        Limits::default(),
        Recovery::Strict,
        request.identity().clone(),
    )
    .unwrap();
    task.finish().unwrap();
    let RestorePoll::Ready(candidate) = task.try_poll() else {
        panic!("expected cold candidate")
    };
    let prepared = host.prepare_continue(request, *candidate).unwrap();
    host.publish_continue(prepared, &mut scene).unwrap();
    assert_eq!(host.world().snapshot(), scene.displayed);
    std::fs::write(
        root.join("cold-restored.json"),
        host.world().snapshot().encode(1 << 20).unwrap(),
    )
    .unwrap();
}
