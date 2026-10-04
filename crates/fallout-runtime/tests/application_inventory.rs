mod common;
use common::*;
// Bootstrap consumer until coordinator publishes the shared module export.
// These aliases compile the actual owned module against the existing runtime.
pub use fallout_runtime::{Error, World, foreign, identity, inventory, source_items};
#[allow(dead_code)]
#[path = "../src/application/mod.rs"]
mod application;
use application::{Failure, Host, HostLimits};
use fallout_data::{loaded_scripts::Catalogue, plugin, store::RecordStore};
use fallout_runtime::{
    Limits,
    foreign::Content,
    identity::ReferenceId,
    inventory::{Condition, Facts, ItemId, OpaqueExtra, Ownership},
    source_items::{Policy, Role},
};
use std::{num::NonZeroU64, sync::Arc};

fn id(n: u64) -> NonZeroU64 {
    n.try_into().unwrap()
}
fn policy() -> Policy {
    Policy::new(&[
        (Role::Base, &[*b"MISC"]),
        (Role::ActorOwner, &[*b"NPC_"]),
        (Role::Ammo, &[*b"AMMO"]),
        (Role::Modification, &[*b"IMOD"]),
    ])
    .unwrap()
}
fn facts() -> Facts {
    let mut f = Facts::unknown(form(0x110));
    f.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abc,
    });
    f.ownership = Some(Ownership::Actor { key: form(0x113) });
    f.ammo = Some(inventory::Ammo {
        base: form(0x111),
        count: 5,
    });
    f.modifications = Some(vec![form(0x112)]);
    f.extra_fields = vec![OpaqueExtra {
        tag: *b"TEST",
        bytes: vec![0, 255, 1],
    }];
    f
}
struct Fixture {
    _dir: tempfile::TempDir,
    catalogue: Arc<Catalogue>,
    content: Arc<Content>,
    world: World<'static>,
    a: ReferenceId,
    b: ReferenceId,
    uninitialized: ReferenceId,
    first: ItemId,
    second: ItemId,
}
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let path = dir.path().join("FalloutNV.esm");
    let mut bytes = std::fs::read(&path).unwrap();
    for (kind, key, flags) in [
        (b"MISC", 0x110, 0),
        (b"AMMO", 0x111, 0),
        (b"IMOD", 0x112, 0),
        (b"NPC_", 0x113, 0),
        (b"REFR", 0x500, 0),
        (b"ACHR", 0x501, 0),
        (b"REFR", 0x502, 0),
        (b"REFR", 0x503, plugin::DELETED),
    ] {
        bytes.extend(record(kind, key, flags, &[]));
    }
    std::fs::write(path, bytes).unwrap();
    let catalogue = Arc::new(load(dir.path(), &["FalloutNV.esm"]));
    let mut store = RecordStore::open_nv_headers(
        dir.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
    let mut world = World::new(Arc::clone(&catalogue), Limits::default()).unwrap();
    let a = world.register_reference(Some(form(0x500))).unwrap();
    let b = world.register_reference(Some(form(0x501))).unwrap();
    let uninitialized = world.register_reference(Some(form(0x502))).unwrap();
    world.initialize_inventory(a).unwrap();
    world.initialize_inventory(b).unwrap();
    let first = world
        .add_source_item(&content, &policy(), a, facts(), 7.try_into().unwrap())
        .unwrap()
        .0;
    let mut other = facts();
    other.condition = Some(Condition::Float64 {
        bits: 0x7ff8_1234_5678_9abd,
    });
    let second = world
        .add_source_item(&content, &policy(), a, other, 3.try_into().unwrap())
        .unwrap()
        .0;
    Fixture {
        _dir: dir,
        catalogue,
        content,
        world,
        a,
        b,
        uninitialized,
        first,
        second,
    }
}
fn host(f: Fixture, limits: HostLimits) -> Host<'static> {
    Host::new(f.world, f.content, policy(), id(1), limits).unwrap()
}

#[test]
fn source_qualified_input_commits_one_revision_preserves_facts_and_replays_one_receipt() {
    let f = fixture();
    let (a, b, lot) = (f.a, f.b, f.first);
    let mut host = host(f, HostLimits::default());
    let before = host.world().snapshot();
    let selection = host.select_transfer(a, lot, b, 7).unwrap();
    assert_eq!(selection.source_key(), &form(0x500));
    assert_eq!(selection.target_key(), &form(0x501));
    assert_eq!(selection.quantity(), 7);
    assert_eq!(selection.revision(), before.state_revision);
    assert_eq!(host.world().snapshot(), before);
    let command = selection.command(id(1));
    let result = host.transfer(command.clone()).unwrap();
    assert!(!result.replayed);
    assert_eq!(result.request, id(1));
    assert_eq!(result.receipt.before_revision(), before.state_revision);
    assert_eq!(result.receipt.after_revision(), before.state_revision + 1);
    assert_eq!(host.world().item(lot).unwrap().owner(), b);
    assert_eq!(host.world().item(lot).unwrap().count(), 7);
    assert_eq!(host.world().item(lot).unwrap().facts(), &facts());
    assert_eq!(host.world().inventory_count(a, &form(0x110)).unwrap(), 3);
    assert_eq!(host.world().inventory_count(b, &form(0x110)).unwrap(), 7);
    let after = host.world().snapshot();
    let retry = host.transfer(command).unwrap();
    assert!(retry.replayed);
    assert!(Arc::ptr_eq(&result.receipt, &retry.receipt));
    assert_eq!(host.world().snapshot(), after);
}

#[test]
fn invalid_quantity_wrong_owner_same_destination_and_missing_lot_preserve_complete_state() {
    let f = fixture();
    let (a, b, lot) = (f.a, f.b, f.first);
    let host = host(f, HostLimits::default());
    let before = host.world().snapshot();
    for quantity in [0, 1, 6, 8, u32::MAX] {
        assert!(host.select_transfer(a, lot, b, quantity).is_err());
    }
    assert!(host.select_transfer(b, lot, a, 7).is_err());
    assert!(host.select_transfer(a, lot, a, 7).is_err());
    assert!(host.select_transfer(a, ItemId(id(999)), b, 7).is_err());
    assert_eq!(host.world().snapshot(), before);
}

#[test]
fn late_uninitialized_destination_refuses_transaction_and_does_not_consume_request_identity() {
    let f = fixture();
    let (a, b, uninitialized, lot) = (f.a, f.b, f.uninitialized, f.first);
    let mut host = host(f, HostLimits::default());
    let before = host.world().snapshot();
    let rejected = host
        .select_transfer(a, lot, uninitialized, 7)
        .unwrap()
        .command(id(1));
    assert!(host.transfer(rejected).is_err());
    assert_eq!(host.world().snapshot(), before);
    assert!(
        !host
            .transfer(host.select_transfer(a, lot, b, 7).unwrap().command(id(1)))
            .unwrap()
            .replayed
    );
}

#[test]
fn stale_revision_and_reused_request_with_different_selection_refuse_without_effects() {
    let f = fixture();
    let (a, b, first, second) = (f.a, f.b, f.first, f.second);
    let mut host = host(f, HostLimits::default());
    let first_command = host.select_transfer(a, first, b, 7).unwrap().command(id(1));
    let stale = host
        .select_transfer(a, second, b, 3)
        .unwrap()
        .command(id(2));
    host.transfer(first_command.clone()).unwrap();
    let before = host.world().snapshot();
    assert!(matches!(
        host.transfer(stale),
        Err(Failure::RevisionChanged)
    ));
    let conflict = host
        .select_transfer(a, second, b, 3)
        .unwrap()
        .command(id(1));
    assert!(matches!(
        host.transfer(conflict),
        Err(Failure::RequestConflict)
    ));
    assert_eq!(host.world().snapshot(), before);
    assert!(host.transfer(first_command).unwrap().replayed);
}

#[test]
fn scene_cancellation_and_new_host_after_restore_expire_selections_even_at_equal_revision() {
    let f = fixture();
    let (a, b, lot) = (f.a, f.b, f.first);
    let catalogue = Arc::clone(&f.catalogue);
    let content = Arc::clone(&f.content);
    let mut host = host(f, HostLimits::default());
    let command = host.select_transfer(a, lot, b, 7).unwrap().command(id(1));
    let before = host.world().snapshot();
    host.advance_scene(id(2)).unwrap();
    assert_eq!(host.scene_generation(), id(2));
    assert!(matches!(
        host.transfer(command.clone()),
        Err(Failure::ExpiredSelection)
    ));
    assert!(host.advance_scene(id(2)).is_err());
    assert_eq!(host.world().snapshot(), before);
    let restored = World::restore(catalogue, before.clone(), Limits::default()).unwrap();
    let mut fresh = Host::new(restored, content, policy(), id(1), HostLimits::default()).unwrap();
    assert!(matches!(
        fresh.transfer(command),
        Err(Failure::ExpiredSelection)
    ));
    assert_eq!(fresh.into_world().snapshot(), before);
}

#[test]
fn request_and_receipt_capacity_refusals_preserve_state_and_keep_existing_retry_available() {
    let f = fixture();
    let (a, b, first, second) = (f.a, f.b, f.first, f.second);
    let mut host = host(
        f,
        HostLimits {
            max_accepted_requests: 1,
            ..HostLimits::default()
        },
    );
    let accepted = host.select_transfer(a, first, b, 7).unwrap().command(id(1));
    host.transfer(accepted.clone()).unwrap();
    let before = host.world().snapshot();
    let next = host
        .select_transfer(a, second, b, 3)
        .unwrap()
        .command(id(2));
    assert!(matches!(
        host.transfer(next),
        Err(Failure::Capacity("accepted requests"))
    ));
    assert!(host.transfer(accepted).unwrap().replayed);
    assert_eq!(host.world().snapshot(), before);
    let f = fixture();
    let (a, b, lot) = (f.a, f.b, f.first);
    let mut host = Host::new(
        f.world,
        f.content,
        policy(),
        id(1),
        HostLimits {
            max_receipt_bytes: 512,
            ..HostLimits::default()
        },
    )
    .unwrap();
    let before = host.world().snapshot();
    let command = host.select_transfer(a, lot, b, 7).unwrap().command(id(1));
    assert!(matches!(
        host.transfer(command),
        Err(Failure::Capacity("receipt bytes"))
    ));
    assert_eq!(host.world().snapshot(), before);
}

#[test]
fn missing_deleted_wrong_kind_and_unqualified_owner_sources_remain_refusals() {
    for key in [
        Some(form(0x999)),
        Some(form(0x503)),
        Some(form(0x110)),
        None,
    ] {
        let mut f = fixture();
        let (a, first) = (f.a, f.first);
        let invalid = f.world.register_reference(key).unwrap();
        f.world.initialize_inventory(invalid).unwrap();
        let host = host(f, HostLimits::default());
        let before = host.world().snapshot();
        assert!(host.select_transfer(a, first, invalid, 7).is_err());
        assert_eq!(host.world().snapshot(), before);
    }
}
