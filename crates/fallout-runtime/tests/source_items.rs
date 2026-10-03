mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits, World,
    foreign::Content,
    inventory::{Ammo, Facts, Ownership},
    source_items::{self, Failure, Policy, Role},
};
fn load_fixture() -> (tempfile::TempDir, Catalogue, Content) {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let mut bytes = std::fs::read(dir.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"AMMO", 0x110, 0, &[]));
    bytes.extend(record(b"FACT", 0x111, 0, &[]));
    bytes.extend(record(b"NPC_", 0x112, 0, &[]));
    bytes.extend(record(b"IMOD", 0x113, 0, &[]));
    std::fs::write(dir.path().join("FalloutNV.esm"), bytes).unwrap();
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let c = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &c, 100).unwrap();
    (dir, c, content)
}
fn policy() -> Policy {
    Policy::new(&[
        (Role::Base, &[*b"ACTI"]),
        (Role::ActorOwner, &[*b"NPC_"]),
        (Role::FactionOwner, &[*b"FACT"]),
        (Role::Ammo, &[*b"AMMO"]),
        (Role::Modification, &[*b"IMOD"]),
    ])
    .unwrap()
}
#[test]
fn source_checks_every_supplied_role_and_keeps_duplicate_links_in_order() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let mut facts = Facts::unknown(form(0x100));
    facts.ownership = Some(Ownership::Faction {
        key: form(0x111),
        rank: -3,
    });
    facts.ammo = Some(Ammo {
        base: form(0x110),
        count: 0,
    });
    facts.modifications = Some(vec![form(0x113), form(0x113)]);
    let before = w.revision();
    let (id, proof) = w
        .add_source_item(
            &content,
            &policy(),
            owner,
            facts.clone(),
            7.try_into().unwrap(),
        )
        .unwrap();
    assert_eq!(proof.state_revision, before);
    assert_eq!(w.revision(), before + 1);
    assert_eq!(w.item(id).unwrap().facts(), &facts);
    assert_eq!(
        proof.forms.iter().map(|f| f.role).collect::<Vec<_>>(),
        [
            Role::Base,
            Role::FactionOwner,
            Role::Ammo,
            Role::Modification,
            Role::Modification
        ]
    );
    assert_eq!(proof.forms[1].source.kind, *b"FACT");
    assert_eq!(proof.forms[3].key, proof.forms[4].key);
    facts.ownership = Some(Ownership::Actor { key: form(0x112) });
    w.replace_source_item_facts(&content, &policy(), id, facts)
        .unwrap();
    assert_eq!(w.inventory_count(owner, &form(0x100)).unwrap(), 7);
}
#[test]
fn missing_deleted_and_wrong_kind_reject_without_canonical_mutation() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let before = w.snapshot();
    for key in [form(0x777), form(0x200), form(0x110)] {
        assert!(
            w.add_source_item(
                &content,
                &policy(),
                owner,
                Facts::unknown(key),
                1.try_into().unwrap()
            )
            .is_err()
        );
        assert_eq!(w.snapshot(), before);
    }
    let (id, _) = w
        .add_source_item(
            &content,
            &policy(),
            owner,
            Facts::unknown(form(0x100)),
            1.try_into().unwrap(),
        )
        .unwrap();
    let before = w.snapshot();
    let mut bad = Facts::unknown(form(0x100));
    bad.modifications = Some(vec![form(0x113), form(0x777)]);
    assert!(
        w.replace_source_item_facts(&content, &policy(), id, bad)
            .is_err()
    );
    assert_eq!(w.snapshot(), before);
}
#[test]
fn absent_policy_is_not_a_permissive_rule_and_unknown_facts_stay_unknown() {
    let (_dir, c, content) = load_fixture();
    let w = World::new(&c, Limits::default()).unwrap();
    let p = Policy::new(&[(Role::Base, &[*b"ACTI"])]).unwrap();
    let f = Facts::unknown(form(0x100));
    let proof = source_items::validate(&w, &content, &p, &f).unwrap();
    assert_eq!(proof.forms.len(), 1);
    let mut f = f;
    f.ammo = Some(Ammo {
        base: form(0x110),
        count: 0,
    });
    assert!(matches!(
        source_items::validate(&w, &content, &p, &f),
        Err(Failure::MissingRule(Role::Ammo))
    ));
}
#[test]
fn source_index_is_bound_to_world_and_restored_state() {
    let (dir, c, content) = load_fixture();
    let w = World::new(&c, Limits::default()).unwrap();
    let proof =
        source_items::validate(&w, &content, &policy(), &Facts::unknown(form(0x100))).unwrap();
    let restored = World::restore(&c, w.snapshot(), Limits::default()).unwrap();
    let again =
        source_items::validate(&restored, &content, &policy(), &Facts::unknown(form(0x100)))
            .unwrap();
    assert_eq!(
        serde_json::to_value(proof).unwrap(),
        serde_json::to_value(again).unwrap()
    );
    let mut bytes = std::fs::read(dir.path().join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"MISC", 0x114, 0, &[]));
    std::fs::write(dir.path().join("FalloutNV.esm"), bytes).unwrap();
    let changed = load(dir.path(), &["FalloutNV.esm"]);
    let other = World::new(&changed, Limits::default()).unwrap();
    assert!(matches!(
        source_items::validate(&other, &content, &policy(), &Facts::unknown(form(0x100))),
        Err(Failure::Source(
            fallout_runtime::foreign::Failure::ContentChanged
        ))
    ));
}
#[test]
fn explicit_policy_is_bounded_unique_and_order_independent() {
    assert!(Policy::new(&[]).is_err());
    assert!(Policy::new(&[(Role::Ammo, &[*b"AMMO"])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[*b"ACTI", *b"ACTI"])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[*b"ACTI"]), (Role::Base, &[*b"MISC"])]).is_err());
    assert!(Policy::new(&[(Role::Base, &[[0; 4]])]).is_err());
    let kinds = (0_u32..65)
        .map(|n| format!("{n:04}").as_bytes().try_into().unwrap())
        .collect::<Vec<[u8; 4]>>();
    assert!(Policy::new(&[(Role::Base, &kinds)]).is_err());
    let a = Policy::new(&[
        (Role::Base, &[*b"MISC", *b"ACTI"]),
        (Role::Ammo, &[*b"AMMO"]),
    ])
    .unwrap();
    let b = Policy::new(&[
        (Role::Ammo, &[*b"AMMO"]),
        (Role::Base, &[*b"ACTI", *b"MISC"]),
    ])
    .unwrap();
    assert_eq!(a.sha256(), b.sha256());
}
#[test]
fn canonical_budgets_and_missing_banks_still_apply_to_source_validated_mutations() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(
        &c,
        Limits {
            max_item_links: 0,
            ..Limits::default()
        },
    )
    .unwrap();
    let owner = w.register_reference(None).unwrap();
    let before = w.snapshot();
    assert!(
        w.add_source_item(
            &content,
            &policy(),
            owner,
            Facts::unknown(form(0x100)),
            1.try_into().unwrap()
        )
        .is_err()
    );
    assert_eq!(w.snapshot(), before);
    w.initialize_inventory(owner).unwrap();
    let before = w.snapshot();
    let mut f = Facts::unknown(form(0x100));
    f.modifications = Some(vec![form(0x113)]);
    assert!(
        w.add_source_item(&content, &policy(), owner, f, 1.try_into().unwrap())
            .is_err()
    );
    assert_eq!(w.snapshot(), before);
}

#[test]
fn winning_kind_tombstones_and_master_namespaces_control_source_checks() {
    let (dir, _, _) = load_fixture();
    let mut other = header(&["FalloutNV.esm"]);
    other.extend(record(b"AMMO", 0x100, 0, &[]));
    other.extend(record(b"WEAP", 0x0100_0100, 0, &[]));
    other.extend(record(b"ACTI", 0x110, plugin::DELETED, &[]));
    std::fs::write(dir.path().join("Other.esm"), other).unwrap();
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into(), "Other.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let c = Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &c, 100).unwrap();
    let w = World::new(&c, Limits::default()).unwrap();
    assert_eq!(
        content.source_form(&w, &form(0x100)).unwrap().kind,
        *b"AMMO"
    );
    assert!(source_items::validate(&w, &content, &policy(), &Facts::unknown(form(0x100))).is_err());
    let p = Policy::new(&[(Role::Base, &[*b"AMMO", *b"WEAP"])]).unwrap();
    source_items::validate(&w, &content, &p, &Facts::unknown(form(0x100))).unwrap();
    let mut own = form(0x100);
    own.origin_plugin = "other.esm".into();
    assert_eq!(
        source_items::validate(&w, &content, &p, &Facts::unknown(own))
            .unwrap()
            .forms[0]
            .source
            .kind,
        *b"WEAP"
    );
    assert!(matches!(
        source_items::validate(&w, &content, &p, &Facts::unknown(form(0x110))),
        Err(Failure::Source(
            fallout_runtime::foreign::Failure::DeletedForm(_)
        ))
    ));
}

#[test]
fn every_optional_content_role_rejects_wrong_kinds_atomically() {
    let (_dir, c, content) = load_fixture();
    let mut w = World::new(&c, Limits::default()).unwrap();
    let owner = w.register_reference(None).unwrap();
    w.initialize_inventory(owner).unwrap();
    let (id, _) = w
        .add_source_item(
            &content,
            &policy(),
            owner,
            Facts::unknown(form(0x100)),
            1.try_into().unwrap(),
        )
        .unwrap();
    let before = w.snapshot();
    let mut actor = Facts::unknown(form(0x100));
    actor.ownership = Some(Ownership::Actor { key: form(0x111) });
    let mut faction = Facts::unknown(form(0x100));
    faction.ownership = Some(Ownership::Faction {
        key: form(0x112),
        rank: 0,
    });
    let mut ammo = Facts::unknown(form(0x100));
    ammo.ammo = Some(Ammo {
        base: form(0x113),
        count: 0,
    });
    let mut modification = Facts::unknown(form(0x100));
    modification.modifications = Some(vec![form(0x110)]);
    for facts in [actor, faction, ammo, modification] {
        assert!(matches!(
            w.replace_source_item_facts(&content, &policy(), id, facts),
            Err(Failure::Kind { .. })
        ));
        assert_eq!(w.snapshot(), before);
    }
}
