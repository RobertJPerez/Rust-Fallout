#![allow(dead_code)]
#[path = "common/mod.rs"]
mod common;
pub use common::form;
use common::{field, header, record, unit};
use fallout_data::{
    actors::{self, associations, factions, placements},
    inventory,
    loaded_scripts::Catalogue,
    plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    events::{Clocks, Context, Trigger},
    foreign::Content,
    identity::{CampaignId, Owner, ReferenceId},
    inventory::Facts,
    reference_state::{Pose, State},
};
use std::{
    fs,
    num::{NonZeroU32, NonZeroU64},
    path::Path,
};

pub fn reference(id: u64) -> ReferenceId {
    ReferenceId(NonZeroU64::new(id).unwrap())
}
fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15u16.to_le_bytes());
    raw
}
fn actor(creature: bool, marker: u8, mask: u16, memberships: &[(u32, i8)]) -> Vec<u8> {
    let mut config = [0; 24];
    config[22..24].copy_from_slice(&mask.to_le_bytes());
    let mut body = [
        field(b"ACBS", &config),
        field(b"DATA", &vec![marker; if creature { 17 } else { 11 }]),
    ]
    .concat();
    for &(key, rank) in memberships {
        body.extend(field(
            b"SNAM",
            &[
                key.to_le_bytes().as_slice(),
                &[rank as u8, 0xaa, 0xbb, 0xcc],
            ]
            .concat(),
        ));
    }
    body
}
fn relation(key: u32, modifier: i32, reaction: u32) -> Vec<u8> {
    field(
        b"XNAM",
        &[
            key.to_le_bytes().as_slice(),
            &modifier.to_le_bytes(),
            &reaction.to_le_bytes(),
        ]
        .concat(),
    )
}
fn placed(base: u32, id: u32) -> Vec<u8> {
    let words = [
        (id as f32).to_bits(),
        0x80000000,
        1,
        0x3f000000,
        0xbf800000,
        0x40000000,
    ];
    [
        field(b"EDID", b"DecisionActor\0"),
        field(b"NAME", &base.to_le_bytes()),
        field(
            b"DATA",
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        field(b"XSCL", &1.25f32.to_le_bytes()),
    ]
    .concat()
}
pub fn fixture(path: &Path, marker: u8, repeated: usize, overrides: bool) {
    fs::create_dir_all(path.join("Data")).unwrap();
    let b_members = vec![(0x302, 3), (0x302, -7)];
    let b_members = if repeated == 0 {
        b_members
    } else {
        vec![(0x302, -7); repeated]
    };
    let mut raw = [
        header(&[]),
        disk(b"CELL", 0x400, 0, &field(b"DATA", &[1])),
        disk(
            b"NPC_",
            0x100,
            0,
            &actor(false, marker, 0, &[(0x300, -2), (0x301, 7), (0x300, -1)]),
        ),
        disk(b"NPC_", 0x101, 0, &actor(false, 8, 0, &b_members)),
        disk(
            b"CREA",
            0x200,
            0,
            &actor(true, 4, 4, &[(0x302, -128), (0x999, 1)]),
        ),
        disk(
            b"FACT",
            0x300,
            0,
            &[
                field(b"DATA", &[1, 128, 0, 0]),
                relation(0x302, -13, u32::MAX),
                relation(0x302, 17, 0x80000000),
            ]
            .concat(),
        ),
        disk(
            b"FACT",
            0x301,
            0,
            &[field(b"DATA", &[0, 0, 0, 0]), relation(0x302, 123, 5)].concat(),
        ),
        disk(
            b"FACT",
            0x302,
            0,
            &[
                field(b"DATA", &[0, 0, 0, 0]),
                relation(0x300, -71, 0x7fff0001),
                relation(0x301, 9, 4),
                relation(0x999, -1, 3),
            ]
            .concat(),
        ),
        disk(b"SCPT", 0x600, 0, &unit(&[(2, 1)], &[])),
        disk(b"MISC", 0x700, 0, &[]),
    ]
    .concat();
    for (kind, id, base, flags) in [
        (b"ACHR", 0x500, 0x100, 0x800),
        (b"ACHR", 0x501, 0x101, 0),
        (b"ACRE", 0x502, 0x200, 0),
        (b"REFR", 0x503, 0x100, 0),
        (b"ACHR", 0x504, 0x100, plugin::DELETED),
        (b"ACHR", 0x505, 0x999, 0),
        (b"ACHR", 0x506, 0x200, 0),
    ] {
        raw.extend(disk(kind, id, flags, &placed(base, id)));
    }
    for id in 0x510..0x550 {
        raw.extend(disk(b"ACHR", id, 0, &placed(0x100, id)));
    }
    fs::write(path.join("Data/FalloutNV.esm"), raw).unwrap();
    let names = if overrides {
        fs::write(
            path.join("Data/ActorPatch.esp"),
            [
                header(&["FalloutNV.esm"]),
                disk(b"ACHR", 0x500, 0, &placed(0x101, 0x500)),
                disk(
                    b"NPC_",
                    0x100,
                    0,
                    &actor(false, 12, 0, &[(0x300, -2), (0x301, 7), (0x300, -1)]),
                ),
            ]
            .concat(),
        )
        .unwrap();
        vec!["FalloutNV.esm", "ActorPatch.esp"]
    } else {
        vec!["FalloutNV.esm"]
    };
    fs::write(path.join("order.json"), serde_json::to_vec(&names).unwrap()).unwrap();
    fs::write(path.join("order.txt"), names.join("\n")).unwrap();
}
pub struct Sources<'a> {
    pub placements: &'a placements::Catalogue,
    pub actors: &'a actors::Catalogue<'a>,
    pub associations: &'a associations::Catalogue<'a>,
    pub factions: &'a factions::Catalogue,
}
pub fn with_sources(path: &Path, f: impl FnOnce(&Catalogue, &Content, Sources<'_>)) {
    let names: Vec<String> =
        serde_json::from_slice(&fs::read(path.join("order.json")).unwrap()).unwrap();
    let mut store =
        RecordStore::open_nv_headers(&path.join("Data"), &names, Default::default()).unwrap();
    let scripts = Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 1000).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let factions = factions::Catalogue::load(&mut store, Default::default()).unwrap();
    f(
        &scripts,
        &content,
        Sources {
            placements: &placements,
            actors: &actors,
            associations: &associations,
            factions: &factions,
        },
    );
}
pub fn world(scripts: &Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        scripts,
        WorldLimits::default(),
        CampaignId::from_bytes([0x34; 16]).unwrap(),
    )
    .unwrap();
    for id in [0x500, 0x501, 0x502, 0x503, 0x504, 0x505, 0x506] {
        world.register_reference(Some(form(id))).unwrap();
    }
    for id in 0x510..0x550 {
        world.register_reference(Some(form(id))).unwrap();
    }
    world.register_reference(None).unwrap();
    let script = scripts.iter().next().unwrap().1.handle().clone();
    let handle = world
        .create_instance(
            &script,
            Owner::Placed {
                reference: reference(1),
            },
            Context {
                calling_reference: Some(reference(1)),
                ..Default::default()
            },
        )
        .unwrap();
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
                calling_reference: Some(reference(1)),
                ..Default::default()
            },
        )
        .unwrap();
    world.initialize_inventory(reference(1)).unwrap();
    world
        .add_item(
            reference(1),
            Facts::unknown(form(0x700)),
            NonZeroU32::new(9).unwrap(),
        )
        .unwrap();
    for (id, enabled) in [(1, false), (8, true), (9, false)] {
        let pose:Pose=serde_json::from_value(serde_json::json!({"position_bits":[1065353216u32,2147483648u32,1],"rotation_bits":[0,0,0],"scale_bits":null})).unwrap();
        let state = State::new(form(0x400), pose, enabled).unwrap();
        let view = world.reference_view(reference(id)).unwrap();
        let stage = world.stage_reference_state(&view, state).unwrap();
        world.commit_reference_state(stage).unwrap();
    }
    world
}
