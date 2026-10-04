mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
    store::RecordStore,
    world::Transform,
};
use fallout_runtime::{
    Limits, World,
    events::{Clocks, Context, Trigger},
    execution::local_copy::{Intent, Preparation, StagedCopy},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId, Value},
    inventory::Facts,
    programs::PreparedSources,
    reference_state::{Pose, State},
    save::{Captured, Recovery, Repository},
};
use std::{fs, path::Path, sync::Arc};

fn fixture(path: &Path) -> (Arc<Catalogue>, Content) {
    // One independently authored compiled assignment: own short2 <- float42.
    // It exercises the existing engineering bit-copy consumer, not retail cast.
    let compiled = [
        0x10, 0, 6, 0, 0, 0, 16, 0, 0, 0, 0x15, 0, 8, 0, b's', 2, 0, 3, 0, b'f', 42, 0, 0x11, 0, 0,
        0,
    ];
    let original = unit(&[(42, 0), (2, 1)], &[]);
    let mut source = original[..26].to_vec();
    source[14..18].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    source.extend(field(b"SCDA", &compiled));
    source.extend(&original[46..]);
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &source),
            record(b"MISC", 0x100, 0, &[]),
            record(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
            record(
                b"REFR",
                0x500,
                0,
                &[
                    field(b"NAME", &0x100_u32.to_le_bytes()),
                    field(b"DATA", &[0; 24]),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
    .unwrap();
    load_content(path)
}
fn load_content(path: &Path) -> (Arc<Catalogue>, Content) {
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], Default::default()).unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Content::load(&mut store, &catalogue, 100).unwrap();
    (catalogue, content)
}
fn prepared(catalogue: &Catalogue) -> PreparedSources<'_> {
    let operators = Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(index, text)| Operator {
            code: index as u32,
            precedence: index as u8,
            spelling: text.as_bytes().to_vec(),
        })
        .collect(),
    )
    .unwrap();
    PreparedSources::load(
        catalogue,
        &Model::vanilla(&operators).unwrap(),
        &Signatures::new(),
        Default::default(),
    )
    .unwrap()
}
fn pose(enabled: bool) -> State {
    State::new(
        form(0x400),
        Pose::from_source(
            &Transform {
                position: [12.25, -0.0, 3.0],
                rotation: [0.125, 0.5, -1.0],
            },
            Some(0.75),
        )
        .unwrap(),
        enabled,
    )
    .unwrap()
}
fn seed(catalogue: Arc<Catalogue>) -> (World<'static>, ReferenceId, u64) {
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x68; 16]).unwrap(),
    )
    .unwrap();
    let id = world.register_reference(Some(form(0x500))).unwrap();
    world.initialize_inventory(id).unwrap();
    world
        .add_item(id, Facts::unknown(form(0x100)), 3.try_into().unwrap())
        .unwrap();
    world
        .commit_reference_state(
            world
                .stage_reference_state(&world.reference_view(id).unwrap(), pose(true))
                .unwrap(),
        )
        .unwrap();
    let handle = world
        .create_instance(
            &definition,
            Owner::Placed { reference: id },
            Context::default(),
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
    let sequence = world
        .enqueue(
            handle,
            Trigger::Block {
                event_id: 0,
                begin_byte_offset: 0,
            },
            Context::default(),
        )
        .unwrap();
    (world, id, sequence)
}
fn copy_stage(
    world: &World<'_>,
    sequence: u64,
    sources: &PreparedSources<'_>,
    content: &Content,
) -> Box<StagedCopy> {
    match world
        .stage_source_local_copy_with_sources(
            sequence,
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

#[test]
fn reference_and_real_compiled_copy_at_same_revision_have_exactly_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, content) = fixture(dir.path());
    let sources = prepared(&catalogue);
    for copy_wins in [false, true] {
        let (mut world, id, sequence) = seed(Arc::clone(&catalogue));
        let before = world.snapshot();
        let copy = copy_stage(&world, sequence, &sources, &content);
        assert_eq!(
            copy.trace().statement_bytes,
            [0x15, 0, 8, 0, b's', 2, 0, 3, 0, b'f', 42, 0]
        );
        assert_eq!(copy.trace().source_index, 42);
        assert_eq!(copy.trace().destination_index, 2);
        assert!(!copy.trace().original_behavior_verified);
        let reference = world
            .stage_reference_state(&world.reference_view(id).unwrap(), pose(false))
            .unwrap();
        assert_eq!(world.snapshot(), before);
        if copy_wins {
            let receipt = (*copy).commit(&mut world).unwrap().receipt;
            assert_eq!(receipt.after_revision, before.state_revision + 1);
            let exact = world.snapshot();
            assert!(world.commit_reference_state(reference).is_err());
            assert_eq!(world.snapshot(), exact);
            assert_eq!(exact.reference_states, before.reference_states);
            assert_eq!(exact.inventory_banks, before.inventory_banks);
            assert_eq!(exact.clocks, before.clocks);
            assert!(exact.pending_events.is_empty());
        } else {
            let receipt = world.commit_reference_state(reference).unwrap();
            assert_eq!(receipt.after_revision, before.state_revision + 1);
            let exact = world.snapshot();
            assert!((*copy).commit(&mut world).is_err());
            assert_eq!(world.snapshot(), exact);
            assert_eq!(exact.instances, before.instances);
            assert_eq!(exact.pending_events, before.pending_events);
            assert_eq!(exact.inventory_banks, before.inventory_banks);
            assert_eq!(exact.clocks, before.clocks);
        }
        let exact = world.snapshot();
        let repo = Repository::create(
            &dir.path().join(format!("winner-{copy_wins}")),
            &[],
            world.campaign(),
        )
        .unwrap();
        repo.commit(&Captured::at_boundary(&world)).unwrap();
        assert_eq!(
            repo.load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
                .unwrap()
                .0
                .snapshot(),
            exact
        );
    }
}

#[test]
fn inventory_and_clock_mutations_invalidate_both_reference_and_source_effect_stages() {
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, content) = fixture(dir.path());
    let sources = prepared(&catalogue);
    for clocks in [false, true] {
        let (mut world, id, sequence) = seed(Arc::clone(&catalogue));
        let copy = copy_stage(&world, sequence, &sources, &content);
        let reference = world
            .stage_reference_state(&world.reference_view(id).unwrap(), pose(false))
            .unwrap();
        if clocks {
            world
                .advance_clocks(Clocks {
                    tick: 1,
                    game_nanoseconds: 1,
                    menu_nanoseconds: 2,
                    real_nanoseconds: 3,
                })
                .unwrap();
        } else {
            world
                .add_item(id, Facts::unknown(form(0x100)), 4.try_into().unwrap())
                .unwrap();
        }
        let exact = world.snapshot();
        assert!(world.commit_reference_state(reference).is_err());
        assert_eq!(world.snapshot(), exact);
        assert!((*copy).commit(&mut world).is_err());
        assert_eq!(world.snapshot(), exact);
    }
}

#[test]
fn equal_persistent_revision_restore_and_other_campaigns_never_reuse_stage_authority() {
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, content) = fixture(dir.path());
    let sources = prepared(&catalogue);
    for other_campaign in [false, true] {
        let (world, id, sequence) = seed(Arc::clone(&catalogue));
        let copy = copy_stage(&world, sequence, &sources, &content);
        let reference = world
            .stage_reference_state(&world.reference_view(id).unwrap(), pose(false))
            .unwrap();
        let mut exact = world.snapshot();
        if other_campaign {
            exact.campaign = CampaignId::from_bytes([0x69; 16]).unwrap();
        }
        let mut restored =
            World::restore(Arc::clone(&catalogue), exact.clone(), Limits::default()).unwrap();
        assert!(restored.commit_reference_state(reference).is_err());
        assert_eq!(restored.snapshot(), exact);
        assert!((*copy).commit(&mut restored).is_err());
        assert_eq!(restored.snapshot(), exact);
    }
}

#[test]
fn changed_whole_source_cohort_cannot_accept_reference_or_compiled_effect_authority() {
    let dir = tempfile::tempdir().unwrap();
    let (catalogue, content) = fixture(dir.path());
    let sources = prepared(&catalogue);
    let (world, id, sequence) = seed(Arc::clone(&catalogue));
    let copy = copy_stage(&world, sequence, &sources, &content);
    let reference = world
        .stage_reference_state(&world.reference_view(id).unwrap(), pose(false))
        .unwrap();
    let mut bytes = fs::read(dir.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"MISC", 0x101, 0, &[]));
    fs::write(dir.path().join("FalloutNV.esm"), bytes).unwrap();
    let (other_catalogue, _) = load_content(dir.path());
    let (mut other, _, _) = seed(other_catalogue);
    let exact = other.snapshot();
    assert_ne!(world.catalogue_fingerprint(), other.catalogue_fingerprint());
    assert!(other.commit_reference_state(reference).is_err());
    assert_eq!(other.snapshot(), exact);
    assert!((*copy).commit(&mut other).is_err());
    assert_eq!(other.snapshot(), exact);
}
