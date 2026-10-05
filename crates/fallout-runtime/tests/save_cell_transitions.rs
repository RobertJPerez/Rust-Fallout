mod common;
use common::*;
use fallout_data::world::Transform;
use fallout_runtime::{
    Error, Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue},
    reference_state::{Pose, State},
    save::{self, Captured, CompletionError, Recovery, Repository, SaveWorker},
};
use std::fs;

fn fixture(path: &std::path::Path) {
    write_fixture(path, false);
    let source_path = path.join("FalloutNV.esm");
    let mut bytes = fs::read(&source_path).unwrap();
    for cell in [0x400, 0x402, 0x404] {
        bytes.extend(record(b"CELL", cell, 0, &field(b"DATA", &[1])));
    }
    let placement = [1.0_f32, 2.0, 3.0, 0.0, 0.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    bytes.extend(record(
        b"REFR",
        0x500,
        0,
        &[
            field(b"NAME", &0x100_u32.to_le_bytes()),
            field(b"DATA", &placement),
        ]
        .concat(),
    ));
    fs::write(source_path, bytes).unwrap();
}

fn state(cell: u32, x: f32) -> State {
    State::new(
        form(cell),
        Pose::from_source(
            &Transform {
                position: [x, -0.0, 3.5],
                rotation: [0.25, -0.5, 1.0],
            },
            Some(1.0),
        )
        .unwrap(),
        true,
    )
    .unwrap()
}

#[test]
fn stale_reference_stage_and_save_capture_cannot_replace_cold_saved_destination() {
    let directory = tempfile::tempdir().unwrap();
    fixture(directory.path());
    let catalogue = load(directory.path(), &["FalloutNV.esm"]);
    let mut world = World::with_campaign(
        &catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x63; 16]).unwrap(),
    )
    .unwrap();

    let reference = world.register_reference(Some(form(0x500))).unwrap();
    let initial_view = world.reference_view(reference).unwrap();
    let initial = world
        .stage_reference_state(&initial_view, state(0x400, 1.0))
        .unwrap();
    world.commit_reference_state(initial).unwrap();

    let owner = Owner::Placed { reference };
    let context = Context {
        calling_reference: Some(reference),
        containing_reference: Some(reference),
        target: Some(ReferenceValue::Live { id: reference }),
        arguments: Vec::new(),
    };
    let instance = world
        .create_instance(&definition(&catalogue), owner.clone(), context.clone())
        .unwrap();
    let instance_id = world.instance(instance).unwrap().id();
    world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context.clone(),
        )
        .unwrap();

    let stale_capture = Captured::at_boundary(&world);
    world
        .enqueue(
            instance,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            context,
        )
        .unwrap();
    let before_transition = world.snapshot();
    assert_eq!(before_transition.pending_events.len(), 2);
    assert_eq!(before_transition.instances.len(), 1);
    let view = world.reference_view(reference).unwrap();
    let stale_stage = world
        .stage_reference_state(&view, state(0x402, 20.0))
        .unwrap();
    let accepted_stage = world
        .stage_reference_state(&view, state(0x404, 40.0))
        .unwrap();
    let transition = world.commit_reference_state(accepted_stage).unwrap();
    let after_transition = world.snapshot();

    assert_eq!(transition.before_revision, before_transition.state_revision);
    assert_eq!(
        transition.after_revision,
        before_transition.state_revision + 1
    );
    assert_eq!(transition.state.cell(), &form(0x404));
    assert_eq!(after_transition.state_revision, transition.after_revision);
    assert_eq!(after_transition.references, before_transition.references);
    assert_eq!(after_transition.instances, before_transition.instances);
    assert_eq!(
        after_transition.pending_events,
        before_transition.pending_events
    );
    assert_eq!(after_transition.reference_states.len(), 1);
    assert_eq!(
        after_transition.reference_states[0].state.cell(),
        &form(0x404)
    );

    assert!(matches!(
        world.commit_reference_state(stale_stage),
        Err(Error::Invalid(message)) if message == "reference view revision changed"
    ));
    assert_eq!(world.snapshot(), after_transition);

    let repository =
        Repository::create(&directory.path().join("native"), &[], world.campaign()).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 2).unwrap();
    let current = worker
        .try_submit(Captured::at_boundary(&world))
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(
        current.metadata.state_revision,
        after_transition.state_revision
    );
    assert_eq!(current.metadata.generation, 1);
    let bytes_before_stale = fs::read(repository.path().join("current.frsv")).unwrap();

    let stale = worker
        .try_submit(stale_capture)
        .unwrap()
        .wait()
        .unwrap_err();
    assert!(matches!(
        &stale,
        CompletionError::Save(save::Error::Format(reason))
            if reason == "captured request would replace newer canonical state"
    ));
    assert_eq!(
        fs::read(repository.path().join("current.frsv")).unwrap(),
        bytes_before_stale
    );
    worker.finish().unwrap();

    let (restored, receipt) = repository
        .load(&catalogue, Limits::default(), Recovery::Strict)
        .unwrap();
    let cold = restored.snapshot();
    assert_eq!(receipt.metadata.generation, 1);
    assert_eq!(cold, after_transition);
    assert_eq!(cold.reference_states[0].state.cell(), &form(0x404));
    assert_eq!(cold.pending_events, before_transition.pending_events);
    assert_eq!(cold.instances[0].id, instance_id);
    assert_eq!(cold.instances[0].owner, owner);
    assert_eq!(restored.owner_instance(&owner), Some(instance_id));
}
