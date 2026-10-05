#![allow(dead_code)]
#[path = "common/mod.rs"]
mod common;
pub use common::form;
use common::{field, header, record, unit};
use fallout_data::{
    actors::{self, placements},
    inventory,
    loaded_scripts::Catalogue,
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::reference_intent::{self, Claim, Intent, Limits, Sources},
    events::{Clocks, Context, Trigger},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId},
    inventory::{Ammo, Condition, Facts, ItemId, OpaqueExtra, Ownership},
    reference_state::State,
    snapshot::Snapshot,
};
use std::{
    fs,
    num::{NonZeroU32, NonZeroU64},
    path::Path,
};

pub fn reference(id: u64) -> ReferenceId {
    ReferenceId(NonZeroU64::new(id).unwrap())
}
pub fn item(id: u64) -> ItemId {
    ItemId(NonZeroU64::new(id).unwrap())
}
fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15u16.to_le_bytes());
    raw
}
pub fn fixture(path: &Path, marker: u8) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let actor = |creature: bool| {
        [
            field(b"ACBS", &[0; 24]),
            field(b"DATA", &vec![marker; if creature { 17 } else { 11 }]),
        ]
        .concat()
    };
    let placed = |base: u32| {
        [
            field(b"NAME", &base.to_le_bytes()),
            field(b"DATA", &[0; 24]),
            field(b"XSCL", &2f32.to_le_bytes()),
        ]
        .concat()
    };
    let mut raw = [
        header(&[]),
        disk(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
        disk(b"CELL", 0x401, 0, &field(b"DATA", &[0])),
        disk(
            b"CELL",
            0x402,
            plugin::DELETED,
            &field(b"UNKN", b"opaque tombstone"),
        ),
        disk(b"NPC_", 0x100, 0, &actor(false)),
        disk(b"CREA", 0x200, 0, &actor(true)),
        disk(b"ARMO", 0x300, 0, &field(b"MODL", b"explicit.nif\0")),
        disk(
            b"ARMO",
            0x301,
            plugin::DELETED,
            &field(b"UNKN", b"opaque tombstone"),
        ),
        disk(b"IMOD", 0x310, 0, &[]),
        disk(b"AMMO", 0x311, 0, &[]),
        disk(b"SCPT", 0x600, 0, &unit(&[(2, 1)], &[])),
    ]
    .concat();
    for (kind, id, base, flags) in [
        (b"ACHR", 0x500, 0x100, 0x800),
        (b"ACRE", 0x501, 0x200, 0),
        (b"ACHR", 0x502, 0x100, 0),
        (b"REFR", 0x503, 0x100, 0),
        (b"ACHR", 0x504, 0x999, 0),
        (b"ACHR", 0x505, 0x200, 0),
        (b"ACHR", 0x506, 0x100, plugin::DELETED),
    ] {
        raw.extend(disk(kind, id, flags, &placed(base)));
    }
    fs::write(path.join("Data/FalloutNV.esm"), raw).unwrap();
    fs::write(path.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
    fs::write(path.join("order.txt"), b"FalloutNV.esm").unwrap();
}
pub fn with_sources(path: &Path, f: impl FnOnce(&Catalogue, &Content, Sources<'_, '_>)) {
    let mut store = RecordStore::open_nv_headers(
        &path.join("Data"),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 1000).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    f(
        &scripts,
        &content,
        Sources {
            placements: &placements,
            actors: &actors,
        },
    );
}
pub fn world(scripts: &Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x30; 16]).unwrap(),
    )
    .unwrap();
    for origin in [
        Some(0x500),
        Some(0x501),
        Some(0x502),
        None,
        Some(0x503),
        Some(0x504),
        Some(0x505),
        Some(0x506),
    ] {
        world.register_reference(origin.map(form)).unwrap();
    }
    let script = scripts.iter().next().unwrap().1.handle().clone();
    let handle = world
        .create_instance(
            &script,
            Owner::Placed {
                reference: reference(3),
            },
            Context {
                calling_reference: Some(reference(3)),
                ..Default::default()
            },
        )
        .unwrap();
    let script_id = world.instance(handle).unwrap().id();
    world
        .advance_clocks(Clocks {
            tick: 19,
            game_nanoseconds: 1001,
            menu_nanoseconds: 71,
            real_nanoseconds: 2026,
        })
        .unwrap();
    world
        .enqueue(
            handle,
            Trigger::ObjectEvent { mask: 0x80000011 },
            Context {
                calling_reference: Some(reference(3)),
                ..Default::default()
            },
        )
        .unwrap();
    for owner in [reference(1), reference(2), reference(3)] {
        world.initialize_inventory(owner).unwrap();
    }
    for i in 0..3 {
        let mut facts = Facts::unknown(form(0x300));
        facts.condition = Some(if i == 0 {
            Condition::Float32 { bits: 0x80000000 }
        } else {
            Condition::Float64 {
                bits: 0x7ff8000000000031 + i,
            }
        });
        facts.equipped_slots = match i {
            0 => None,
            1 => Some(vec![]),
            _ => Some(vec![7, 0, u16::MAX]),
        };
        facts.ownership = Some(Ownership::Live {
            reference: reference(3),
        });
        facts.ammo = Some(Ammo {
            base: form(0x311),
            count: 0x80000001,
        });
        facts.modifications = Some(vec![form(0x310), form(0x310)]);
        facts.quest_item = Some(false);
        facts.script_instance = Some(script_id);
        facts.extra_fields = vec![OpaqueExtra {
            tag: *b"ZZZZ",
            bytes: vec![0, 255, i as u8],
        }];
        world
            .add_item(reference(1), facts, NonZeroU32::new(i as u32 + 2).unwrap())
            .unwrap();
    }
    world
        .add_item(
            reference(3),
            Facts::unknown(form(0x300)),
            NonZeroU32::new(11).unwrap(),
        )
        .unwrap();
    let state: State=serde_json::from_value(serde_json::json!({"schema_version":1,"cell":form(0x401),"pose":{
        "position_bits":[1065353216u32,2147483648u32,0u32],"rotation_bits":[0,0,0],"scale_bits":null},"enabled":true})).unwrap();
    let view = world.reference_view(reference(3)).unwrap();
    let staged = world.stage_reference_state(&view, state).unwrap();
    world.commit_reference_state(staged).unwrap();
    world
}
pub fn claim(snapshot: &Snapshot, id: u64, base: u32) -> Claim {
    Claim {
        expected_snapshot_sha256: reference_intent::snapshot_sha256(
            snapshot,
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap(),
        reference: reference(id),
        actor: form(base),
        intent: Intent::Engineering {},
    }
}
pub fn choice(snapshot: &Snapshot, id: u64, base: u32) -> reference_intent::Choice {
    reference_intent::Choice {
        claim: claim(snapshot, id, base),
        cell: form(0x400),
        position_bits: [8192.25f32.to_bits(), 0x80000000, (-30.5f32).to_bits()],
        rotation_bits: [0.125f32.to_bits(), (-0.75f32).to_bits(), 1.5f32.to_bits()],
        scale_bits: None,
        enabled: false,
    }
}
pub fn assert_cold(scripts: &Catalogue, snapshot: &Snapshot) {
    let bytes = snapshot
        .encode(WorldLimits::default().max_snapshot_bytes)
        .unwrap();
    let decoded = Snapshot::decode(&bytes, WorldLimits::default()).unwrap();
    let world = World::restore(scripts, decoded, WorldLimits::default()).unwrap();
    assert_eq!(&world.snapshot(), snapshot);
}
pub fn conserve(
    before: &Snapshot,
    after: &Snapshot,
    state_changed: bool,
    inventory_changed: bool,
    revision_delta: u64,
) {
    let mut expected = before.clone();
    expected.state_revision += revision_delta;
    if state_changed {
        expected.reference_states = after.reference_states.clone();
    }
    if inventory_changed {
        expected.inventory_banks = after.inventory_banks.clone();
    }
    assert_eq!(&expected, after);
    assert!(!before.instances.is_empty() && !before.pending_events.is_empty());
}
