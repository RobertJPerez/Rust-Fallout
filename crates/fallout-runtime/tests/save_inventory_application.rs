mod common;
use common::*;
use fallout_data::{
    loaded_scripts::{Catalogue, Limits as CatalogueLimits},
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits, World,
    application::{self, Host, HostLimits},
    foreign::Content,
    identity::{CampaignId, ReferenceId},
    inventory::{Condition, Facts, Ownership, TransferLimits},
    save::{Recovery, Repository, SaveWorker},
    source_items::{Policy, Role},
};
use std::{fs, num::NonZeroU64, path::Path, sync::Arc};

fn id(value: u64) -> NonZeroU64 {
    value.try_into().unwrap()
}

fn write_inventory_fixture(root: &Path) {
    write_fixture(root, false);
    let mut bytes = fs::read(root.join("FalloutNV.esm")).unwrap();
    bytes.extend(record(b"CONT", 0x600, 0, &[]));
    bytes.extend(record(b"NPC_", 0x700, 0, &[]));
    bytes.extend(record(
        b"REFR",
        0x500,
        0,
        &field(b"NAME", &0x600_u32.to_le_bytes()),
    ));
    bytes.extend(record(
        b"ACHR",
        0x501,
        0,
        &field(b"NAME", &0x700_u32.to_le_bytes()),
    ));
    fs::write(root.join("FalloutNV.esm"), bytes).unwrap();
}

fn authored_sources(root: &Path) -> (Arc<Catalogue>, Arc<Content>) {
    let mut store =
        RecordStore::open_nv_headers(root, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let catalogue =
        Arc::new(Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap());
    let content = Arc::new(Content::load(&mut store, &catalogue, 100).unwrap());
    (catalogue, content)
}

fn inventory_policy() -> Policy {
    Policy::new(&[(Role::Base, &[*b"ACTI"]), (Role::ActorOwner, &[*b"NPC_"])]).unwrap()
}

fn equipped_facts(slots: Vec<u16>, condition_bits: u32) -> Facts {
    let mut facts = Facts::unknown(form(0x100));
    facts.condition = Some(Condition::Float32 {
        bits: condition_bits,
    });
    facts.ownership = Some(Ownership::Actor { key: form(0x700) });
    facts.equipped_slots = Some(slots);
    facts
}

fn populated_host(
    catalogue: &Arc<Catalogue>,
    content: &Arc<Content>,
    mut limits: HostLimits,
) -> (
    Host<'static>,
    ReferenceId,
    ReferenceId,
    fallout_runtime::inventory::ItemId,
    fallout_runtime::inventory::ItemId,
) {
    let mut world = World::with_campaign(
        Arc::clone(catalogue),
        Limits::default(),
        CampaignId::from_bytes([0x49; 16]).unwrap(),
    )
    .unwrap();
    let container = world.register_reference(Some(form(0x500))).unwrap();
    let actor = world.register_reference(Some(form(0x501))).unwrap();
    world.initialize_inventory(container).unwrap();
    world.initialize_inventory(actor).unwrap();
    let first = world
        .add_source_item(
            content,
            &inventory_policy(),
            container,
            equipped_facts(vec![7, 1], 0x3f00_0000),
            4.try_into().unwrap(),
        )
        .unwrap()
        .0;
    let second = world
        .add_source_item(
            content,
            &inventory_policy(),
            container,
            equipped_facts(vec![2], 0x3e80_0000),
            3.try_into().unwrap(),
        )
        .unwrap()
        .0;
    limits.max_accepted_requests = 1;
    let host = Host::new(
        world,
        Arc::clone(content),
        inventory_policy(),
        id(5),
        limits,
    )
    .unwrap();
    (host, container, actor, first, second)
}

#[test]
fn container_to_actor_transfer_keeps_equipment_facts_and_refusals_are_atomic() {
    let directory = tempfile::tempdir().unwrap();
    write_inventory_fixture(directory.path());
    let (catalogue, content) = authored_sources(directory.path());
    let (mut host, container, actor, first, second) =
        populated_host(&catalogue, &content, HostLimits::default());
    let original = host.world().snapshot();
    let original_facts = host.world().item(first).unwrap().facts().clone();
    assert_eq!(
        host.world()
            .inventory_count(container, &form(0x100))
            .unwrap(),
        7
    );
    assert_eq!(
        host.world().inventory_count(actor, &form(0x100)).unwrap(),
        0
    );

    // A partial quantity is outside the existing complete-lot command.
    assert!(matches!(
        host.select_transfer(container, first, actor, 3),
        Err(application::Failure::Refused(_))
    ));
    assert_eq!(host.world().snapshot(), original);

    // The actor is a valid placed owner, but cannot spend a lot held by the container.
    assert!(matches!(
        host.select_transfer(actor, second, container, 3),
        Err(application::Failure::Refused(_))
    ));
    assert_eq!(host.world().snapshot(), original);

    // This second command was selected at the same boundary as the first.
    let stale = host
        .select_transfer(container, second, actor, 3)
        .unwrap()
        .command(id(2));
    let accepted = host
        .transfer(
            host.select_transfer(container, first, actor, 4)
                .unwrap()
                .command(id(1)),
        )
        .unwrap();
    assert_eq!(accepted.receipt.before_revision(), original.state_revision);
    assert_eq!(
        accepted.receipt.after_revision(),
        original.state_revision + 1
    );
    let after_transfer = host.world().snapshot();
    assert_eq!(after_transfer.state_revision, original.state_revision + 1);
    assert_eq!(after_transfer.references, original.references);
    assert_eq!(after_transfer.instances, original.instances);
    assert_eq!(after_transfer.pending_events, original.pending_events);
    assert_eq!(after_transfer.reference_states, original.reference_states);
    assert_eq!(host.world().item(first).unwrap().owner(), actor);
    assert_eq!(host.world().item(first).unwrap().facts(), &original_facts);
    assert_eq!(
        host.world().item(first).unwrap().facts().equipped_slots,
        Some(vec![7, 1])
    );
    assert_eq!(
        host.world()
            .inventory_items(container)
            .unwrap()
            .map(|item| item.id())
            .collect::<Vec<_>>(),
        [second]
    );
    assert_eq!(
        host.world()
            .inventory_items(actor)
            .unwrap()
            .map(|item| item.id())
            .collect::<Vec<_>>(),
        [first]
    );
    assert_eq!(
        host.world()
            .inventory_count(container, &form(0x100))
            .unwrap(),
        3
    );
    assert_eq!(
        host.world().inventory_count(actor, &form(0x100)).unwrap(),
        4
    );

    assert!(matches!(
        host.transfer(stale),
        Err(application::Failure::RevisionChanged)
    ));
    assert_eq!(host.world().snapshot(), after_transfer);

    // A full accepted-request ledger refuses before changing either owner bank.
    assert!(matches!(
        host.transfer(
            host.select_transfer(container, second, actor, 3)
                .unwrap()
                .command(id(2))
        ),
        Err(application::Failure::Capacity("accepted requests"))
    ));
    assert_eq!(host.world().snapshot(), after_transfer);

    // The accepted canonical transaction is the exact state sent through the
    // existing native writer and cold repository restore.
    let repository = Repository::create(
        &directory.path().join("native"),
        &[],
        host.world().campaign(),
    )
    .unwrap();
    let request = host.select_save(id(1)).unwrap();
    let mut worker = SaveWorker::start(repository.clone(), 1).unwrap();
    let submission = host.submit_save(request, &mut worker).unwrap();
    assert!(submission.matches_current_boundary(&host));
    worker.finish().unwrap();
    assert_eq!(submission.wait().unwrap().metadata.generation, 1);
    let (cold, _) = repository
        .load(catalogue.as_ref(), Limits::default(), Recovery::Strict)
        .unwrap();
    assert_eq!(cold.snapshot(), after_transfer);
    assert_eq!(cold.item(first).unwrap().facts(), &original_facts);
    assert_eq!(cold.item(first).unwrap().owner(), actor);
}

#[test]
fn transfer_copy_capacity_refuses_before_either_inventory_changes() {
    let directory = tempfile::tempdir().unwrap();
    write_inventory_fixture(directory.path());
    let (catalogue, content) = authored_sources(directory.path());
    let limited_transfer = TransferLimits {
        max_copied_bytes: 1,
        ..TransferLimits::default()
    };
    let limits = HostLimits {
        transfer: limited_transfer,
        ..HostLimits::default()
    };
    let (mut host, container, actor, first, _) = populated_host(&catalogue, &content, limits);
    let before = host.world().snapshot();
    let result = host.transfer(
        host.select_transfer(container, first, actor, 4)
            .unwrap()
            .command(id(1)),
    );
    assert!(matches!(
        result,
        Err(application::Failure::State(
            fallout_runtime::Error::Capacity("inventory transfer copied bytes")
        ))
    ));
    assert_eq!(host.world().snapshot(), before);
}
