mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, classes, initialization_inputs, races},
    inventory, loaded_scripts,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::initialization_inputs::{Error, Limits, Requests},
    foreign::Content,
    snapshot::Snapshot,
};
use std::{fs, path::Path};
fn disk(kind: &[u8; 4], id: u32, raw: &[u8]) -> Vec<u8> {
    let mut r = record(kind, id, 0, raw);
    r[20..22].copy_from_slice(&15_u16.to_le_bytes());
    r
}
fn fixture(path: &Path, variant: u8) {
    let race = [
        field(b"DATA", &[0; 36]),
        field(b"PNAM", &[0; 4]),
        field(b"UNAM", &[0; 4]),
        field(b"UNKN", &[variant]),
    ]
    .concat();
    let class = [field(b"DATA", &[0; 28]), field(b"ATTR", &[0; 7])].concat();
    let actor = [
        field(b"ACBS", &[0; 24]),
        field(b"DATA", &[0; 11]),
        field(b"RNAM", &0x300_u32.to_le_bytes()),
        field(b"CNAM", &0x400_u32.to_le_bytes()),
    ]
    .concat();
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            disk(b"NPC_", 0x100, &actor),
            disk(b"RACE", 0x300, &race),
            disk(b"CLAS", 0x400, &class),
        ]
        .concat(),
    )
    .unwrap();
}
#[test]
fn strict_cold_observation_preserves_state_and_refuses_another_campaign() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 0);
    let mut store =
        RecordStore::open_nv_headers(temp.path(), &["FalloutNV.esm".into()], Default::default())
            .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 100).unwrap();
    let inv = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inv, Default::default()).unwrap();
    let assoc = associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let races = races::Catalogue::load(&mut store, Default::default()).unwrap();
    let classes = classes::Catalogue::load(&mut store, Default::default()).unwrap();
    let manifest = initialization_inputs::request(
        &mut store,
        &actors,
        &assoc,
        &races,
        &classes,
        &form(0x100),
        Default::default(),
    )
    .unwrap();
    let mut world = World::new(&scripts, WorldLimits::default()).unwrap();
    world.register_reference(None).unwrap();
    let before = world.snapshot();
    let requests = Requests::prepare(&world, &content, manifest, Default::default()).unwrap();
    let observed = requests
        .observe(&world, &content, Default::default())
        .unwrap();
    assert!(!observed.initialization_supported && !observed.actor_reference_bound);
    assert!(observed.current_actor_values.is_none());
    let bytes = before
        .encode(WorldLimits::default().max_snapshot_bytes)
        .unwrap();
    let snapshot = Snapshot::decode(&bytes, WorldLimits::default()).unwrap();
    let restored = World::restore(&scripts, snapshot, WorldLimits::default()).unwrap();
    assert_eq!(
        serde_json::to_value(&observed).unwrap(),
        serde_json::to_value(
            requests
                .observe(&restored, &content, Default::default())
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(before, world.snapshot());
    assert_eq!(before, restored.snapshot());
    if let Some(destination) = std::env::var_os("FALLOUT_ACTOR_INIT_EVIDENCE_DIR") {
        let destination = Path::new(&destination);
        assert!(destination.is_absolute() && destination.is_dir());
        let case = destination.join("canonical-case");
        fs::create_dir(&case).unwrap();
        fs::create_dir(case.join("Data")).unwrap();
        fs::copy(
            temp.path().join("FalloutNV.esm"),
            case.join("Data/FalloutNV.esm"),
        )
        .unwrap();
        fs::write(case.join("order.json"), b"[\"FalloutNV.esm\"]").unwrap();
        fs::write(case.join("snapshot.json"), &bytes).unwrap();
        fs::write(
            case.join("cold-snapshot.json"),
            restored
                .snapshot()
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        fs::write(
            case.join("host-observation.json"),
            serde_json::to_vec_pretty(&observed).unwrap(),
        )
        .unwrap();
    }
    let different = World::new(&scripts, WorldLimits::default()).unwrap();
    assert!(matches!(
        requests.observe(&different, &content, Default::default()),
        Err(Error::ContextChanged)
    ));
    let size = serde_json::to_vec(&observed).unwrap().len();
    requests
        .observe(
            &world,
            &content,
            Limits {
                max_sources: 1,
                max_links: 2,
                max_projection_bytes: size,
            },
        )
        .unwrap();
    for limit in [
        Limits {
            max_sources: 0,
            ..Default::default()
        },
        Limits {
            max_links: 1,
            ..Default::default()
        },
        Limits {
            max_projection_bytes: size - 1,
            ..Default::default()
        },
    ] {
        assert!(requests.observe(&world, &content, limit).is_err());
    }
}
#[test]
fn changed_body_cohort_refuses_before_preparing_canonical_observation() {
    let old = tempfile::tempdir().unwrap();
    let changed = tempfile::tempdir().unwrap();
    fixture(old.path(), 0);
    fixture(changed.path(), 1);
    let mut s =
        RecordStore::open_nv_headers(old.path(), &["FalloutNV.esm".into()], Default::default())
            .unwrap();
    let inv = inventory::Catalogue::load(&mut s, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inv, Default::default()).unwrap();
    let assoc = associations::Catalogue::load(&mut s, &actors, Default::default()).unwrap();
    let races = races::Catalogue::load(&mut s, Default::default()).unwrap();
    let classes = classes::Catalogue::load(&mut s, Default::default()).unwrap();
    let manifest = initialization_inputs::request(
        &mut s,
        &actors,
        &assoc,
        &races,
        &classes,
        &form(0x100),
        Default::default(),
    )
    .unwrap();
    let mut fresh = RecordStore::open_nv_headers(
        changed.path(),
        &["FalloutNV.esm".into()],
        Default::default(),
    )
    .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut fresh, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut fresh, &scripts, 100).unwrap();
    let world = World::new(&scripts, WorldLimits::default()).unwrap();
    assert!(matches!(
        Requests::prepare(&world, &content, manifest, Default::default()),
        Err(Error::ContextChanged)
    ));
}
