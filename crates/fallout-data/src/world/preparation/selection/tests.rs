use super::*;
use crate::{
    archive::NvArchive,
    world::{
        residency::test_sources::{Fixture, field, key, record, reference, until},
        residency::{CellResidency, Readiness, Stage, TexturePlan, TextureState},
    },
};
use std::fs;

fn selection(f: &Fixture) -> CellModelSelection {
    CellModelSelection::load(
        &mut f.store(),
        &key(0x200),
        &[key(0x300), key(0x312), key(0x313)],
        Default::default(),
        Default::default(),
    )
    .unwrap()
}
#[test]
fn three_source_instances_share_two_jobs_and_unsafe_omissions_never_establish_full_readiness() {
    let f = Fixture::selected_scene();
    assert!(
        CellModelPlan::load(&mut f.store(), &key(0x200), &f.mounts, Default::default()).is_err()
    );
    let selected = selection(&f);
    let receipt = selected.receipt();
    assert_eq!(receipt.requested, [key(0x300), key(0x312), key(0x313)]);
    assert!(!receipt.full_cell_coverage);
    assert_eq!(receipt.references.len(), 5);
    let states: Vec<_> = receipt
        .references
        .iter()
        .map(|r| (r.key.local_id, r.state))
        .collect();
    assert_eq!(
        states,
        [
            (0x300, SelectionState::Selected),
            (0x311, SelectionState::NotSelected),
            (0x312, SelectionState::Selected),
            (0x313, SelectionState::Selected),
            (0x314, SelectionState::NotSelected)
        ]
    );
    let bases: Vec<_> = receipt
        .references
        .iter()
        .filter_map(|r| r.base.as_ref())
        .collect();
    assert_eq!(bases, [&key(0x400), &key(0x400), &key(0x401)]);
    for reference in receipt
        .references
        .iter()
        .filter(|r| r.state == SelectionState::NotSelected)
    {
        assert!(reference.base.is_none());
        assert!(reference.base_edge.is_none());
    }
    let identity = receipt.identity.clone();
    let plan = selected.prepare(&mut f.store(), &f.mounts).unwrap();
    assert_eq!(plan.selection().unwrap().identity, identity);
    assert_eq!(plan.receipt().usage.bases, 2);
    assert_eq!(plan.receipt().requests.len(), 2);
    assert_eq!(
        plan.receipt()
            .coverage
            .iter()
            .map(|c| c.base_key.local_id)
            .collect::<Vec<_>>(),
        [0x400, 0x401]
    );
    assert_eq!(
        plan.receipt()
            .requests
            .iter()
            .map(|r| r.decoded_bytes)
            .sum::<usize>(),
        f.models[0].len() + f.models[1].len()
    );
    let mut owner =
        CellResidency::new(f.root.path(), Some(f.cache.path()), Default::default()).unwrap();
    let ticket = owner.request(plan).unwrap();
    until(|| owner.poll().unwrap().stage == Stage::Decoded);
    let sources = owner.sources(&ticket).unwrap();
    assert_eq!(sources.model(0).unwrap(), f.models[0]);
    assert_eq!(sources.model(1).unwrap(), f.models[1]);
    assert_eq!(
        sources
            .plan()
            .unwrap()
            .selection()
            .unwrap()
            .references
            .len(),
        5
    );
    assert_eq!(owner.snapshot().outstanding, 2);
    assert!(!owner.snapshot().complete_model_coverage);
    let textures = TexturePlan::load(sources.clone(), &f.mounts, Default::default()).unwrap();
    owner.request_textures(&ticket, textures).unwrap();
    until(|| owner.poll().unwrap().texture_state == TextureState::Decoded);
    let error = owner
        .report_dependencies(&ticket, Readiness::Ready)
        .unwrap_err();
    assert!(error.to_string().contains("selected reference sources"));
    assert_eq!(owner.snapshot().dependencies, Readiness::Pending);
    owner.unload().unwrap();
    assert!(sources.model(0).is_err());
    assert!(sources.plan().is_err());
    assert_eq!(owner.poll().unwrap().outstanding, 2);
    drop(sources);
    until(|| owner.poll().unwrap().stage == Stage::Unrequested);
    assert_eq!(owner.snapshot().plan_metadata_bytes, 0);
    assert_eq!(owner.snapshot().mapped_source_bytes, 0);
}
#[test]
fn last_bad_key_deleted_wrong_kind_null_base_and_selected_unsafe_path_refuse_all() {
    let f = Fixture::selected_scene();
    for keys in [
        vec![],
        vec![key(0x300), key(0x300)],
        vec![key(0x300), key(0xdead)],
        vec![key(0x300), key(0x301)],
        vec![key(0x300), key(0x400)],
    ] {
        assert!(
            CellModelSelection::load(
                &mut f.store(),
                &key(0x200),
                &keys,
                Default::default(),
                Default::default()
            )
            .is_err()
        );
    }
    let mut foreign = key(0x313);
    foreign.profile = crate::identity::ProfileId::Fo4Original;
    assert!(
        CellModelSelection::load(
            &mut f.store(),
            &key(0x200),
            &[key(0x300), foreign],
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    let unsafe_selection = CellModelSelection::load(
        &mut f.store(),
        &key(0x200),
        &[key(0x300), key(0x314)],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(unsafe_selection.prepare(&mut f.store(), &f.mounts).is_err());
    for mode in 0..5 {
        let mut f = Fixture::selected_scene();
        let patch = match mode {
            0 => reference(0x313, 0x401, plugin::DELETED, &[]),
            1 => record(b"STAT", 0x313, 0, &field(b"MODL", b"w.nif\0")),
            2 => reference(0x313, 0, 0, &[]),
            3 => record(b"STAT", 0x401, plugin::DELETED, &field(b"MODL", b"w.nif\0")),
            _ => record(b"STAT", 0x401, 0, &[]),
        };
        f.patch(&patch);
        let result = CellModelSelection::load(
            &mut f.store(),
            &key(0x200),
            &[key(0x300), key(0x313)],
            Default::default(),
            Default::default(),
        );
        match result {
            Err(_) => {}
            Ok(s) => assert!(s.prepare(&mut f.store(), &f.mounts).is_err()),
        }
    }
}
#[test]
fn stale_order_and_bytes_ambiguous_last_member_and_exact_aggregate_caps_refuse_without_a_plan() {
    let mut f = Fixture::selected_scene();
    let selected = selection(&f);
    f.patch(&record(b"STAT", 0x599, 0, &[]));
    assert!(selected.prepare(&mut f.store(), &f.mounts).is_err());
    let f = Fixture::selected_scene();
    let selected = selection(&f);
    let source = f.root.path().join("Data/Base.esm");
    let mut bytes = fs::read(&source).unwrap();
    bytes.extend(record(b"STAT", 0x599, 0, &[]));
    fs::write(&source, bytes).unwrap();
    assert!(selected.prepare(&mut f.store(), &f.mounts).is_err());
    let f = Fixture::selected_scene();
    let selected = selection(&f);
    let other = f.root.path().join("Data/other.bsa");
    fs::copy(f.root.path().join("Data/models.bsa"), &other).unwrap();
    let mut mounts = MountIndex::default();
    for name in ["models.bsa", "other.bsa"] {
        NvArchive::open(&f.root.path().join("Data").join(name))
            .unwrap()
            .census(&mut mounts)
            .unwrap();
    }
    assert!(selected.prepare(&mut f.store(), &mounts).is_err());
    let f = Fixture::selected_scene();
    let selected = selection(&f);
    let metadata = selected.receipt().metadata_bytes;
    let exact = SelectionLimits {
        references: 3,
        metadata_bytes: metadata,
    };
    assert!(
        CellModelSelection::load(
            &mut f.store(),
            &key(0x200),
            &[key(0x300), key(0x312), key(0x313)],
            Default::default(),
            exact
        )
        .is_ok()
    );
    for limits in [
        SelectionLimits {
            references: 2,
            ..exact
        },
        SelectionLimits {
            metadata_bytes: metadata - 1,
            ..exact
        },
    ] {
        assert!(
            CellModelSelection::load(
                &mut f.store(),
                &key(0x200),
                &[key(0x300), key(0x312), key(0x313)],
                Default::default(),
                limits
            )
            .is_err()
        );
    }
    let mut model_limits = ModelLimits {
        max_requests: 1,
        ..Default::default()
    };
    let selected = CellModelSelection::load(
        &mut f.store(),
        &key(0x200),
        &[key(0x300), key(0x313)],
        model_limits,
        Default::default(),
    )
    .unwrap();
    assert!(selected.prepare(&mut f.store(), &f.mounts).is_err());
    let plan = selection(&f).prepare(&mut f.store(), &f.mounts).unwrap();
    model_limits = ModelLimits {
        max_metadata_bytes: plan.receipt().usage.metadata_bytes - 1,
        ..Default::default()
    };
    let selected = CellModelSelection::load(
        &mut f.store(),
        &key(0x200),
        &[key(0x300), key(0x312), key(0x313)],
        model_limits,
        Default::default(),
    )
    .unwrap();
    assert!(selected.prepare(&mut f.store(), &f.mounts).is_err());
}
#[test]
fn ordinary_full_cell_receipt_identity_and_raw_requests_remain_identical() {
    let f = Fixture::scene();
    let before =
        CellModelPlan::load(&mut f.store(), &key(0x200), &f.mounts, Default::default()).unwrap();
    let selected = CellModelSelection::load(
        &mut f.store(),
        &key(0x200),
        &[key(0x300)],
        Default::default(),
        Default::default(),
    )
    .unwrap()
    .prepare(&mut f.store(), &f.mounts)
    .unwrap();
    let after =
        CellModelPlan::load(&mut f.store(), &key(0x200), &f.mounts, Default::default()).unwrap();
    assert!(before.selection().is_none());
    assert!(after.selection().is_none());
    assert_eq!(
        serde_json::to_vec(before.receipt()).unwrap(),
        serde_json::to_vec(after.receipt()).unwrap()
    );
    assert_eq!(before.identity(), after.identity());
    assert_ne!(before.identity(), selected.identity());
    assert_eq!(before.receipt().requests.len(), 1);
    assert_eq!(selected.receipt().requests.len(), 1);
    assert_eq!(
        before.receipt().requests[0].decoded_bytes,
        selected.receipt().requests[0].decoded_bytes
    );
}
