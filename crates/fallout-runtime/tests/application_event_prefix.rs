mod common;
use common::*;
use fallout_data::{
    loaded_scripts::Catalogue,
    obscript::{
        argument_census::Signatures,
        expression::{Operator, Operators},
        expression_plan::Model,
    },
};
use fallout_runtime::{
    World,
    application::{EventPrefixError, EventPrefixPreparation, Host, HostLimits},
    events::{Context, Trigger},
    execution::{
        local_copy::{Intent, Unsupported},
        pending_batch::{self, OwnerRequest},
    },
    foreign::Content,
    identity::{CampaignId, Owner, Value},
    programs::PreparedSources,
};
use std::{fs, sync::Arc};

fn instruction(out: &mut Vec<u8>, opcode: u16, payload: &[u8]) {
    out.extend(opcode.to_le_bytes());
    out.extend((payload.len() as u16).to_le_bytes());
    out.extend(payload);
}
fn event(destination: u8, expression: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    instruction(
        &mut body,
        0x15,
        &[
            &[b's', destination, 0][..],
            &(expression.len() as u16).to_le_bytes(),
            expression,
        ]
        .concat(),
    );
    let mut out = Vec::new();
    instruction(
        &mut out,
        0x10,
        &[
            &0_u16.to_le_bytes()[..],
            &((body.len() + 4) as u32).to_le_bytes(),
        ]
        .concat(),
    );
    out.extend(body);
    instruction(&mut out, 0x11, &[]);
    out
}
fn fixture(unsupported: bool) -> (tempfile::TempDir, Arc<Catalogue>, Arc<Content>) {
    let root = tempfile::tempdir().unwrap();
    let compiled = [
        event(2, &[b'f', 1, 0]),
        event(3, if unsupported { b"123" } else { &[b'f', 2, 0] }),
    ]
    .concat();
    let original = unit(&[(1, 0), (2, 1), (3, 0)], &[]);
    let mut script = original[..26].to_vec();
    script[14..18].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    script.extend(field(b"SCDA", &compiled));
    script.extend(&original[46..]);
    fs::write(
        root.path().join("FalloutNV.esm"),
        [header(&[]), record(b"SCPT", 0x300, 0, &script)].concat(),
    )
    .unwrap();
    let mut store = fallout_data::store::RecordStore::open_nv_headers(
        root.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap());
    let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
    (root, catalogue, content)
}
fn sources(catalogue: &Catalogue, changed_decoder: bool) -> PreparedSources<'_> {
    let operators = Operators::new(
        [
            "(", ")", "&&", "||", "<=", "<", ">=", ">", "==", "!=", "-", "+", "*", "/", "%", "~",
        ]
        .iter()
        .enumerate()
        .map(|(i, text)| Operator {
            code: i as u32,
            precedence: i as u8 + u8::from(changed_decoder),
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
fn host(catalogue: Arc<Catalogue>, content: Arc<Content>) -> Host<'static> {
    host_with_limits(catalogue, content, Default::default())
}
fn host_with_limits(
    catalogue: Arc<Catalogue>,
    content: Arc<Content>,
    limits: fallout_runtime::Limits,
) -> Host<'static> {
    let definition = definition(&catalogue);
    let mut world = World::with_campaign(
        catalogue,
        limits,
        CampaignId::from_bytes([0x76; 16]).unwrap(),
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
        .assign(
            handle,
            &[(
                1,
                Value::Number {
                    bits: 123.0_f64.to_bits(),
                },
            )],
        )
        .unwrap();
    for offset in [0, 26] {
        world
            .enqueue(
                handle,
                Trigger::Block {
                    event_id: 0,
                    begin_byte_offset: offset,
                },
                Context::default(),
            )
            .unwrap();
    }
    Host::new(
        world,
        content,
        Default::default(),
        1.try_into().unwrap(),
        HostLimits::default(),
    )
    .unwrap()
}
fn requests() -> Vec<OwnerRequest> {
    (1..=2)
        .map(|sequence| OwnerRequest {
            sequence: sequence.try_into().unwrap(),
            expected_owner: Owner::Fragment {
                activation: 1.try_into().unwrap(),
            },
        })
        .collect()
}

#[test]
fn private_prefix_keeps_active_state_and_binds_host_scene_and_decoder() {
    let (_root, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue, false);
    let changed = sources(&catalogue, true);
    let mut live = host(Arc::clone(&catalogue), Arc::clone(&content));
    let before = live.world().snapshot();
    let EventPrefixPreparation::EngineeringPrepared(candidate) = live
        .prepare_event_prefix(
            &prepared,
            &requests(),
            Intent::Engineering,
            Default::default(),
        )
        .unwrap()
    else {
        panic!("engineering candidate required")
    };
    assert_eq!(live.world().snapshot(), before);
    assert!(candidate.matches_current_boundary(&live, &prepared));
    assert!(!candidate.matches_current_boundary(&live, &changed));
    assert_eq!(candidate.input_revision(), before.state_revision);
    assert_eq!(
        candidate.observation().snapshot.state_revision,
        before.state_revision + 2
    );
    assert!(candidate.observation().snapshot.pending_events.is_empty());
    assert_eq!(candidate.observation().ordered_events.len(), 2);
    assert!(
        candidate.observation().snapshot.instances[0]
            .locals
            .iter()
            .all(|local| local.value
                == Value::Number {
                    bits: 123.0_f64.to_bits()
                })
    );
    let EventPrefixPreparation::EngineeringPrepared(retry) = live
        .prepare_event_prefix(
            &prepared,
            &requests(),
            Intent::Engineering,
            Default::default(),
        )
        .unwrap()
    else {
        panic!("retry candidate required")
    };
    assert_eq!(
        retry.observation().snapshot,
        candidate.observation().snapshot
    );
    let cold = World::restore(Arc::clone(&catalogue), before.clone(), Default::default()).unwrap();
    let other = Host::new(
        cold,
        content,
        Default::default(),
        1.try_into().unwrap(),
        HostLimits::default(),
    )
    .unwrap();
    assert_eq!(other.world().snapshot(), before);
    assert!(!candidate.matches_current_boundary(&other, &prepared));
    live.advance_scene(2.try_into().unwrap()).unwrap();
    assert!(!candidate.matches_current_boundary(&live, &prepared));
    assert_eq!(live.world().snapshot(), before);
}

#[test]
fn late_unsupported_and_faithful_refusal_expose_no_private_snapshot() {
    let (_root, catalogue, content) = fixture(true);
    let prepared = sources(&catalogue, false);
    let live = host(Arc::clone(&catalogue), content);
    let before = live.world().snapshot();
    let outcome = live
        .prepare_event_prefix(
            &prepared,
            &requests(),
            Intent::Engineering,
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        outcome,
        EventPrefixPreparation::Unsupported {
            event_index: Some(1),
            ..
        }
    ));
    assert_eq!(live.world().snapshot(), before);
    let faithful = live
        .prepare_event_prefix(&prepared, &requests(), Intent::Faithful, Default::default())
        .unwrap();
    assert!(matches!(
        faithful,
        EventPrefixPreparation::Unsupported {
            event_index: None,
            reason: Unsupported::UnverifiedRetailSemantics,
            ..
        }
    ));
    assert_eq!(live.world().snapshot(), before);
}

#[test]
fn foreign_sources_wrong_late_owner_and_prefix_capacity_preserve_input() {
    let (_root, catalogue, content) = fixture(false);
    let (_other_root, other_catalogue, _) = fixture(true);
    let prepared = sources(&catalogue, false);
    let foreign = sources(&other_catalogue, false);
    let live = host(Arc::clone(&catalogue), content);
    let before = live.world().snapshot();
    assert!(matches!(
        live.prepare_event_prefix(
            &foreign,
            &requests(),
            Intent::Engineering,
            Default::default()
        ),
        Err(EventPrefixError::Sources(_))
    ));
    let mut wrong = requests();
    wrong[1].expected_owner = Owner::Fragment {
        activation: 2.try_into().unwrap(),
    };
    assert!(
        live.prepare_event_prefix(&prepared, &wrong, Intent::Engineering, Default::default())
            .is_err()
    );
    assert!(matches!(
        live.prepare_event_prefix(
            &prepared,
            &requests(),
            Intent::Engineering,
            pending_batch::Limits {
                maximum_events: 1,
                ..Default::default()
            }
        ),
        Err(EventPrefixError::Batch(pending_batch::Error::Capacity(
            "events"
        )))
    ));
    assert_eq!(live.world().snapshot(), before);
}

#[test]
fn input_snapshot_cap_and_late_instruction_exhaustion_preserve_active_state() {
    let (_root, catalogue, content) = fixture(false);
    let prepared = sources(&catalogue, false);
    let small = host_with_limits(
        Arc::clone(&catalogue),
        Arc::clone(&content),
        fallout_runtime::Limits {
            max_snapshot_bytes: 1,
            ..Default::default()
        },
    );
    let before_small = small.world().snapshot();
    assert!(matches!(
        small.prepare_event_prefix(
            &prepared,
            &requests(),
            Intent::Engineering,
            Default::default()
        ),
        Err(EventPrefixError::State(_))
    ));
    assert_eq!(small.world().snapshot(), before_small);
    let live = host(Arc::clone(&catalogue), content);
    let before = live.world().snapshot();
    assert!(
        live.prepare_event_prefix(
            &prepared,
            &requests(),
            Intent::Engineering,
            pending_batch::Limits {
                maximum_source_instructions: 3,
                ..Default::default()
            },
        )
        .is_err()
    );
    assert_eq!(live.world().snapshot(), before);
}
