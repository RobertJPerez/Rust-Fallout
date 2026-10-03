mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits, World,
    events::Clocks,
    foreign::Content,
    identity::{ReferenceId, ReferenceValue, Value},
    inventory::Facts,
    query::{Entry, Failure, GET_ITEM_COUNT_COMMAND, GET_ITEM_COUNT_CONDITION, Request},
};
fn fixture() -> (tempfile::TempDir, Catalogue, Content) {
    let d = tempfile::tempdir().unwrap();
    write_fixture(d.path(), false);
    let mut b = std::fs::read(d.path().join("FalloutNV.esm")).unwrap();
    b.extend(record(b"FLST", 0x400, 0, &[]));
    std::fs::write(d.path().join("FalloutNV.esm"), b).unwrap();
    let mut s = RecordStore::open_nv_headers(
        d.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let c = Catalogue::load(&mut s, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let i = Content::load(&mut s, &c, 100).unwrap();
    (d, c, i)
}
fn entry() -> Entry {
    Entry::Native {
        command_id: GET_ITEM_COUNT_COMMAND,
    }
}
fn argument(id: u32) -> Value {
    Value::Reference {
        value: ReferenceValue::Content { key: form(id) },
    }
}
fn populated(c: &Catalogue) -> (World<'_>, ReferenceId) {
    let mut w = World::new(c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    for _ in 0..2 {
        w.add_item(
            owner,
            Facts::unknown(form(0x100)),
            u32::MAX.try_into().unwrap(),
        )
        .unwrap();
    }
    (w, owner)
}
#[test]
fn both_source_entries_share_the_exact_host_query_without_numeric_coercion() {
    let (_d, c, i) = fixture();
    let (w, o) = populated(&c);
    let before = w.snapshot();
    let a = Request::prepare(&w, entry(), Some(o), &[argument(0x100)])
        .unwrap()
        .evaluate(&w, &i, 10)
        .unwrap();
    let b = Request::prepare(
        &w,
        Entry::Condition {
            function_id: GET_ITEM_COUNT_CONDITION,
        },
        Some(o),
        &[argument(0x100)],
    )
    .unwrap()
    .evaluate(&w, &i, 10)
    .unwrap();
    assert_eq!(
        serde_json::to_value(&a.query).unwrap(),
        serde_json::to_value(&b.query).unwrap()
    );
    assert_eq!(a.query.result, 8_589_934_590);
    assert!(a.original_numeric_return.is_none());
    assert!(!a.original_behavior_verified);
    assert_eq!(w.snapshot(), before);
}
#[test]
fn unknown_entries_subjects_and_argument_kinds_fail_explicitly() {
    let (_d, c, _i) = fixture();
    let (w, o) = populated(&c);
    let before = w.snapshot();
    for e in [
        Entry::Native { command_id: 1 },
        Entry::Condition { function_id: 4143 },
    ] {
        assert!(matches!(
            Request::prepare(&w, e, Some(o), &[argument(0x100)]),
            Err(Failure::UnsupportedEntry)
        ));
    }
    assert!(matches!(
        Request::prepare(&w, entry(), None, &[argument(0x100)]),
        Err(Failure::MissingSubject)
    ));
    for args in [
        vec![],
        vec![argument(0x100), argument(0x100)],
        vec![Value::Uninitialized],
        vec![Value::Number { bits: 0 }],
        vec![Value::Reference {
            value: ReferenceValue::Null,
        }],
        vec![Value::Reference {
            value: ReferenceValue::Live { id: o },
        }],
    ] {
        assert!(matches!(
            Request::prepare(&w, entry(), Some(o), &args),
            Err(Failure::Arguments)
        ));
    }
    let mut v = argument(0x100);
    if let Value::Reference {
        value: ReferenceValue::Content { key },
    } = &mut v
    {
        key.origin_plugin = "FalloutNV.esm".into();
    }
    assert!(Request::prepare(&w, entry(), Some(o), &[v]).is_err());
    assert_eq!(w.snapshot(), before);
}
#[test]
fn missing_and_uninitialized_inventory_subjects_are_not_successful_zeroes() {
    let (_d, c, i) = fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let o = w.register_reference(None).unwrap();
    let r = Request::prepare(&w, entry(), Some(o), &[argument(0x100)]).unwrap();
    let before = w.snapshot();
    assert!(r.evaluate(&w, &i, 10).is_err());
    assert!(
        Request::prepare(
            &w,
            entry(),
            Some(ReferenceId(999.try_into().unwrap())),
            &[argument(0x100)]
        )
        .is_err()
    );
    assert_eq!(w.snapshot(), before);
    w.initialize_inventory(o).unwrap();
    assert_eq!(r.evaluate(&w, &i, 0).unwrap().query.result, 0);
}
#[test]
fn missing_deleted_and_form_list_arguments_remain_unsupported() {
    let (_d, c, i) = fixture();
    let (w, o) = populated(&c);
    let before = w.snapshot();
    for id in [0x777, 0x200] {
        assert!(
            Request::prepare(&w, entry(), Some(o), &[argument(id)])
                .unwrap()
                .evaluate(&w, &i, 10)
                .is_err()
        );
    }
    assert!(matches!(
        Request::prepare(&w, entry(), Some(o), &[argument(0x400)])
            .unwrap()
            .evaluate(&w, &i, 10),
        Err(Failure::UnverifiedFormList)
    ));
    assert_eq!(w.snapshot(), before);
}
#[test]
fn prepared_requests_observe_current_revision_clocks_and_quantities() {
    let (_d, c, i) = fixture();
    let (mut w, o) = populated(&c);
    let r = Request::prepare(&w, entry(), Some(o), &[argument(0x100)]).unwrap();
    let first = r.evaluate(&w, &i, 10).unwrap();
    let id = w.inventory_items(o).unwrap().next().unwrap().id();
    w.remove_item_quantity(id, 1.try_into().unwrap()).unwrap();
    w.advance_clocks(Clocks {
        tick: 1,
        game_nanoseconds: 2,
        menu_nanoseconds: 3,
        real_nanoseconds: 4,
    })
    .unwrap();
    let next = r.evaluate(&w, &i, 10).unwrap();
    assert_eq!(next.query.result, first.query.result - 1);
    assert_eq!(next.query.state_revision, w.revision());
    assert_eq!(next.query.boundary, w.clocks());
}
#[test]
fn restore_keeps_persistent_request_identity_but_other_campaigns_fail() {
    let (_d, c, i) = fixture();
    let (w, o) = populated(&c);
    let r = Request::prepare(&w, entry(), Some(o), &[argument(0x100)]).unwrap();
    let before = r.evaluate(&w, &i, 10).unwrap();
    let restored = World::restore(&c, w.snapshot(), Limits::default()).unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(r.evaluate(&restored, &i, 10).unwrap()).unwrap()
    );
    let other = World::new(&c, Limits::default()).unwrap();
    assert!(matches!(
        r.evaluate(&other, &i, 10),
        Err(Failure::ContextChanged)
    ));
}
#[test]
fn contribution_budget_is_checked_without_mutating_state() {
    let (_d, c, i) = fixture();
    let (w, o) = populated(&c);
    let r = Request::prepare(&w, entry(), Some(o), &[argument(0x100)]).unwrap();
    let before = w.snapshot();
    for budget in [0, 1] {
        assert!(r.evaluate(&w, &i, budget).is_err());
    }
    assert_eq!(r.evaluate(&w, &i, 2).unwrap().query.contributions.len(), 2);
    assert_eq!(w.snapshot(), before);
}
#[test]
fn a_changed_source_cohort_invalidates_prepared_requests() {
    let (d, c, i) = fixture();
    let (w, o) = populated(&c);
    let r = Request::prepare(&w, entry(), Some(o), &[argument(0x100)]).unwrap();
    let mut b = std::fs::read(d.path().join("FalloutNV.esm")).unwrap();
    b.extend(record(b"MISC", 0x401, 0, &[]));
    std::fs::write(d.path().join("FalloutNV.esm"), b).unwrap();
    let changed = load(d.path(), &["FalloutNV.esm"]);
    let other = World::with_campaign(&changed, Limits::default(), w.campaign()).unwrap();
    assert!(matches!(
        r.evaluate(&other, &i, 10),
        Err(Failure::ContextChanged)
    ));
}
