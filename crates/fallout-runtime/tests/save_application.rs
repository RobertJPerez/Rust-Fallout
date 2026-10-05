mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
    world::{self, Transform},
};
use fallout_runtime::{
    Limits, World,
    application::{self, ContinueBoundary, HostIdentity, ScenePublisher},
    events::{Context, Trigger},
    foreign::Content,
    identity::{Owner, Value},
    inventory::Facts,
    reference_state::{Pose, State},
    save::{
        self, Captured, Recovery, Repository, RestoreError, RestorePoll, RestoreTask, SaveWorker,
        Slot, Stage, format,
    },
    snapshot::Snapshot,
    source_items::{Policy, Role},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    num::NonZeroU64,
    path::Path,
    process::{Command, ExitStatus},
    sync::Arc,
};

const INTERRUPTED_EXIT: i32 = 73;

fn id(value: u64) -> NonZeroU64 {
    value.try_into().unwrap()
}

fn policy() -> Policy {
    Policy::new(&[(Role::Base, &[*b"ACTI"])]).unwrap()
}

fn write_authored_sources(root: &Path) {
    write_fixture(root, false);
    let mut bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"CELL", 0x400, 0, &field(b"DATA", &[1])));
    for (reference, position) in [
        (0x500, [1.0_f32, 2.0, 3.0, 0.25, -0.5, 1.0]),
        (0x501, [4.0_f32, 5.0, 6.0, 0.0, 0.0, 0.0]),
    ] {
        let placement = position
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend(record(
            b"REFR",
            reference,
            0,
            &[
                field(b"NAME", &0x100_u32.to_le_bytes()),
                field(b"DATA", &placement),
            ]
            .concat(),
        ));
    }
    fs::write(root.join("FalloutNV.esm"), bytes).unwrap();
}

fn authored_sources(root: &Path) -> (Arc<Catalogue>, Arc<Content>, Pose) {
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap());
    let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
    let placed = store.winner(&form(0x500)).unwrap();
    let placement = world::decode_placement(&store.read(placed).unwrap(), "FalloutNV.esm").unwrap();
    let pose = Pose::from_source(
        &placement.transform.value,
        placement.scale.map(|field| field.value),
    )
    .unwrap();
    (catalogue, content, pose)
}

fn changed_reference_state() -> State {
    State::new(
        form(0x400),
        Pose::from_source(
            &Transform {
                position: [8192.25, -0.0, -30.5],
                rotation: [0.125, -0.75, 1.5],
            },
            Some(0.75),
        )
        .unwrap(),
        false,
    )
    .unwrap()
}

fn wrong_profile_container(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(&bytes[..8], format::MAGIC);
    let body_len = u64::from_le_bytes(bytes[160..168].try_into().unwrap()) as usize;
    let mut body = String::from_utf8(bytes[200..200 + body_len].to_vec()).unwrap();
    let original = r#""profile":"nv-original""#;
    let start = body.find(original).expect("snapshot profile field");
    body.replace_range(start..start + original.len(), r#""profile":"fo3-original""#);
    let body = body.into_bytes();

    // META and STAT declare this body length and carry separate chunk hashes.
    let mut forged = bytes[..200].to_vec();
    let extent = (body.len() as u64).to_le_bytes();
    forged[120..128].copy_from_slice(&extent);
    forged[160..168].copy_from_slice(&extent);
    let metadata_hash = Sha256::digest(&forged[64..152]);
    forged[32..64].copy_from_slice(&metadata_hash);
    forged[168..200].copy_from_slice(&Sha256::digest(&body));
    forged.extend_from_slice(&body);
    let container_hash = Sha256::digest(&forged);
    forged.extend_from_slice(&container_hash);
    forged
}

fn launch_helper(name: &str, root: &Path, expected: Option<&Path>) -> ExitStatus {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", name, "--nocapture"])
        .env("FALLOUT_GAMEPLAY_SAVE_ROOT", root);
    if let Some(expected) = expected {
        command.env("FALLOUT_GAMEPLAY_SAVE_EXPECTED", expected);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.status().unwrap()
}

struct TestScene {
    host: HostIdentity,
    generation: NonZeroU64,
    displayed: Snapshot,
    content: Arc<Content>,
    publishes: usize,
}

impl TestScene {
    fn for_host(host: &application::Host<'_>, content: Arc<Content>) -> Self {
        Self {
            host: host.identity(),
            generation: host.scene_generation(),
            displayed: host.world().snapshot(),
            content,
            publishes: 0,
        }
    }
}

impl ScenePublisher for TestScene {
    type Stage = Snapshot;

    fn prepare(
        &self,
        candidate: &World<'_>,
        boundary: &ContinueBoundary,
    ) -> application::Result<Snapshot> {
        if self.host != boundary.prior_host_identity()
            || self.generation != boundary.scene_generation()
            || self.displayed.state_revision != boundary.prior_revision()
        {
            return Err(application::Failure::Refused("scene boundary changed"));
        }
        self.content.validate_world(candidate)?;
        Ok(candidate.snapshot())
    }

    fn publish(&mut self, stage: Snapshot, boundary: &ContinueBoundary) {
        self.displayed = stage;
        self.host = boundary.candidate_host_identity();
        self.publishes += 1;
    }
}

#[test]
fn application_save_cold_continue_wrong_profile_and_interruption_recovery() {
    let directory = tempfile::tempdir().unwrap();
    write_authored_sources(directory.path());
    let (catalogue, content, source_pose) = authored_sources(directory.path());

    let mut world = World::new(Arc::clone(&catalogue), Limits::default()).unwrap();
    let source = world.register_reference(Some(form(0x500))).unwrap();
    let target = world.register_reference(Some(form(0x501))).unwrap();
    world.initialize_inventory(source).unwrap();
    world.initialize_inventory(target).unwrap();
    let proposal = world
        .stage_reference_state(
            &world.reference_view(source).unwrap(),
            State::new(form(0x400), source_pose, true).unwrap(),
        )
        .unwrap();
    world.commit_reference_state(proposal).unwrap();
    let (item, _) = world
        .add_source_item(
            &content,
            &policy(),
            source,
            Facts::unknown(form(0x100)),
            8.try_into().unwrap(),
        )
        .unwrap();
    let instance = world
        .create_instance(
            &definition(&catalogue),
            Owner::Fragment { activation: id(11) },
            Context::default(),
        )
        .unwrap();
    world
        .assign(instance, &[(42, Value::Number { bits: 0x1234 })])
        .unwrap();
    world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();

    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let mut host = application::Host::new(
        world,
        Arc::clone(&content),
        policy(),
        id(1),
        application::HostLimits::default(),
    )
    .unwrap();
    let transfer = host
        .select_transfer(source, item, target, 8)
        .unwrap()
        .command(id(1));
    let accepted = host.transfer(transfer).unwrap();
    assert!(!accepted.replayed);
    let expected = host.world().snapshot();
    assert_eq!(expected.inventory_banks.len(), 2);
    assert!(
        expected
            .inventory_banks
            .iter()
            .find(|bank| bank.owner == source)
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(
        expected
            .inventory_banks
            .iter()
            .find(|bank| bank.owner == target)
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(expected.references.len(), 2);
    assert_eq!(expected.reference_states.len(), 1);
    assert_eq!(expected.reference_states[0].id, source);
    assert_eq!(expected.instances.len(), 1);
    assert_eq!(expected.pending_events.len(), 1);

    for request in [1, 2] {
        let save_request = host.select_save(id(request)).unwrap();
        let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
        let submission = host.submit_save(save_request, &mut worker).unwrap();
        assert!(submission.matches_current_boundary(&host));
        worker.finish().unwrap();
        let receipt = submission.wait().unwrap();
        assert_eq!(receipt.metadata.generation, request);
    }
    let valid_generation_two = fs::read(repository.path().join("current.frsv")).unwrap();
    assert_eq!(
        format::decode(&valid_generation_two, Limits::default())
            .unwrap()
            .snapshot,
        expected
    );

    // A checksum-valid native container with a different typed profile reaches
    // the application's real restore boundary and is refused before publish.
    let forged = wrong_profile_container(&valid_generation_two);
    assert!(matches!(
        format::decode(&forged, Limits::default()),
        Err(save::Error::Format(_))
    ));
    fs::write(repository.path().join("current.frsv"), &forged).unwrap();
    let prior_host = host.identity();
    let scene = TestScene::for_host(&host, Arc::clone(&content));
    let old_scene = scene.displayed.clone();
    let request = host.begin_continue(id(1)).unwrap();
    let mut wrong_profile = RestoreTask::start(
        repository.clone(),
        Arc::clone(&catalogue),
        Limits::default(),
        Recovery::Strict,
        request.identity().clone(),
    )
    .unwrap();
    wrong_profile.finish().unwrap();
    match wrong_profile.try_poll() {
        RestorePoll::Failed(error) => assert!(matches!(
            error.as_ref(),
            RestoreError::Save(save::Error::Format(_))
        )),
        other => panic!("wrong-profile Continue result: {other:?}"),
    }
    assert_eq!(host.world().snapshot(), expected);
    assert_eq!(host.identity(), prior_host);
    assert_eq!(scene.displayed, old_scene);
    assert_eq!(scene.publishes, 0);
    assert!(host.cancel_continue(&request));

    let (fallback, fallback_receipt) = repository
        .load(
            catalogue.as_ref(),
            Limits::default(),
            Recovery::PreviousIfCurrentInvalid,
        )
        .unwrap();
    assert_eq!(fallback_receipt.slot, Slot::Previous);
    assert!(fallback_receipt.current_failure.is_some());
    assert_eq!(fallback.snapshot(), expected);
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        forged
    );
    let (repaired, repair_receipt) = repository
        .recover_previous(catalogue.as_ref(), Limits::default())
        .unwrap();
    assert!(repair_receipt.current_repaired);
    assert_eq!(repaired.snapshot(), expected);
    let pre_interrupt_current = fs::read(repository.path().join("current.frsv")).unwrap();

    // Exit a separate writer process immediately after it publishes previous.
    // The old current remains loadable and the same state change can be retried.
    let interrupted = launch_helper("gameplay_save_interrupt_child", directory.path(), None);
    assert_eq!(interrupted.code(), Some(INTERRUPTED_EXIT));
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        pre_interrupt_current
    );
    assert_eq!(
        fs::read(repository.path().join("previous.frsv")).unwrap(),
        pre_interrupt_current
    );
    let (old_current, old_receipt) = repository
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(old_receipt.metadata.generation, 1);
    assert_eq!(old_current.snapshot(), expected);

    let (mut retry_world, _) = repository
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    let source = retry_world.authored_reference(&form(0x500)).unwrap();
    let changed = retry_world
        .stage_reference_state(
            &retry_world.reference_view(source).unwrap(),
            changed_reference_state(),
        )
        .unwrap();
    retry_world.commit_reference_state(changed).unwrap();
    let final_expected = retry_world.snapshot();
    let retry_receipt = repository
        .commit(&Captured::at_boundary(&retry_world))
        .unwrap();
    assert_eq!(retry_receipt.metadata.generation, 2);
    assert_eq!(
        format::decode(
            &fs::read(repository.path().join("current.frsv")).unwrap(),
            Limits::default()
        )
        .unwrap()
        .snapshot,
        final_expected
    );

    let expected_path = directory.path().join("expected-final.json");
    fs::write(&expected_path, serde_json::to_vec(&final_expected).unwrap()).unwrap();
    let cold = launch_helper(
        "gameplay_save_cold_continue_child",
        directory.path(),
        Some(&expected_path),
    );
    assert!(cold.success(), "fresh Continue process exited with {cold}");
}

#[test]
#[ignore = "launched by the parent after a native publication boundary"]
fn gameplay_save_interrupt_child() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_GAMEPLAY_SAVE_ROOT").expect("child root"),
    );
    let (catalogue, _, _) = authored_sources(&root);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let (mut world, _) = repository
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    let source = world.authored_reference(&form(0x500)).unwrap();
    let staged = world
        .stage_reference_state(
            &world.reference_view(source).unwrap(),
            changed_reference_state(),
        )
        .unwrap();
    world.commit_reference_state(staged).unwrap();
    repository
        .commit_observing(&Captured::at_boundary(&world), |stage| {
            if stage == Stage::PreviousPublished {
                std::process::exit(INTERRUPTED_EXIT);
            }
        })
        .unwrap();
    panic!("interruption boundary was not reached");
}

#[test]
#[ignore = "launched in a fresh process to exercise native Continue"]
fn gameplay_save_cold_continue_child() {
    let root = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_GAMEPLAY_SAVE_ROOT").expect("child root"),
    );
    let expected_path = std::path::PathBuf::from(
        std::env::var_os("FALLOUT_GAMEPLAY_SAVE_EXPECTED").expect("expected snapshot"),
    );
    let expected: Snapshot = serde_json::from_slice(&fs::read(expected_path).unwrap()).unwrap();
    let (catalogue, content, _) = authored_sources(&root);
    let repository = Repository::open(&root.join("native"), &[]).unwrap();
    let initial =
        World::with_campaign(Arc::clone(&catalogue), Limits::default(), expected.campaign).unwrap();
    let mut host = application::Host::new(
        initial,
        Arc::clone(&content),
        policy(),
        id(1),
        application::HostLimits::default(),
    )
    .unwrap();
    let request = host.begin_continue(id(1)).unwrap();
    let mut restore = RestoreTask::start(
        repository,
        Arc::clone(&catalogue),
        Limits::default(),
        Recovery::Strict,
        request.identity().clone(),
    )
    .unwrap();
    restore.finish().unwrap();
    let candidate = match restore.try_poll() {
        RestorePoll::Ready(candidate) => *candidate,
        other => panic!("fresh Continue restore result: {other:?}"),
    };
    let prepared = host.prepare_continue(request, candidate).unwrap();
    assert_eq!(prepared.world().snapshot(), expected);
    let mut scene = TestScene::for_host(&host, Arc::clone(&content));
    let receipt = host.publish_continue(prepared, &mut scene).unwrap();
    assert_eq!(
        receipt.boundary.candidate_revision(),
        expected.state_revision
    );
    assert_eq!(host.world().snapshot(), expected);
    assert_eq!(scene.displayed, expected);
    assert_eq!(scene.publishes, 1);
}
