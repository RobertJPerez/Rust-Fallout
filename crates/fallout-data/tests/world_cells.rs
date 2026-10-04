//! Authored source directories exercise actual winners, parent masters and CELL plans.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{
        cells::{CellGridSources, Limits, Role},
        residency::CellResidency,
    },
};
use std::fs;

fn field(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(body.len() as u16).to_le_bytes(), body].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header(master: bool) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    if master {
        body.extend(field(b"MAST", b"Base.esm\0"));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn group(world: u32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &world.to_le_bytes(),
        &1_i32.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn cell(id: u32, flags: u32, data: u8, grid: Option<[i32; 2]>) -> Vec<u8> {
    let mut body = field(b"DATA", &[data]);
    if let Some(grid) = grid {
        body.extend(field(
            b"XCLC",
            &grid
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
        ));
    }
    record(b"CELL", id, flags, &body)
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
struct Fixture {
    root: tempfile::TempDir,
    cache: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        fs::write(
            root.path().join("Data/Base.esm"),
            [
                header(false),
                record(b"WRLD", 0x100, 0, &field(b"WNAM", &0x101_u32.to_le_bytes())),
                group(
                    0x100,
                    &[
                        cell(0x200, 0, 0, Some([-18, 0])),
                        cell(0x201, 0, 0, Some([0, 0])),
                        cell(0x202, plugin::PERSISTENT, 0, None),
                        cell(0x203, 0, 1, Some([-18, 0])),
                        cell(0x204, 0, 0, None),
                        cell(0x205, plugin::DELETED, 0, Some([-18, 0])),
                        cell(0x206, 0, 0, Some([i32::MIN, i32::MAX])),
                    ]
                    .concat(),
                ),
                record(b"WRLD", 0x101, 0, &[]),
                group(0x101, &cell(0x210, 0, 0, Some([-18, 0]))),
            ]
            .concat(),
        )
        .unwrap();
        Self {
            root,
            cache,
            names: vec!["Base.esm".into()],
        }
    }
    fn patch(&mut self, body: &[u8]) {
        fs::write(
            self.root.path().join("Data/Patch.esp"),
            [header(true), body.to_vec()].concat(),
        )
        .unwrap();
        self.names.push("Patch.esp".into());
    }
    fn open(&self, cached: bool) -> RecordStore {
        let data = self.root.path().join("Data");
        if cached {
            RecordStore::open_nv_headers_cached(
                &data,
                &self.names,
                plugin::Limits::default(),
                self.cache.path(),
            )
            .unwrap()
        } else {
            RecordStore::open_nv_headers(&data, &self.names, plugin::Limits::default()).unwrap()
        }
    }
}
fn load(store: &mut RecordStore) -> CellGridSources {
    CellGridSources::load(store, &key(0x100), Limits::default()).unwrap()
}

#[test]
fn explicit_grid_selects_source_cell_and_reaches_existing_residency() {
    let fixture = Fixture::new();
    let mut store = fixture.open(false);
    let sources = load(&mut store);
    let request = sources.request([-18, 0]).unwrap();
    assert_eq!(request.world(), &key(0x100));
    assert_eq!(request.cell(), &key(0x200));
    assert_eq!(request.grid(), [-18, 0]);
    let metadata = sources.metadata();
    assert_eq!(metadata.entries.len(), 7);
    let role = |id| {
        metadata
            .entries
            .iter()
            .find(|entry| entry.key == key(id))
            .unwrap()
            .role
    };
    assert_eq!(role(0x202), Role::PersistentGroup);
    assert_eq!(role(0x203), Role::Interior);
    assert_eq!(role(0x204), Role::NoGrid);
    assert_eq!(role(0x205), Role::Deleted);
    assert!(
        metadata
            .entries
            .iter()
            .find(|entry| entry.key == key(0x205))
            .unwrap()
            .fields
            .is_none()
    );
    assert!(!metadata.runtime_ready);
    assert!(sources.request([77, 88]).is_err());
    assert_eq!(
        sources.request([i32::MIN, i32::MAX]).unwrap().cell(),
        &key(0x206)
    );
    // WNAM is not applied as inherited cell membership; the other world remains separate.
    let other = CellGridSources::load(&mut store, &key(0x101), Limits::default()).unwrap();
    assert_eq!(other.request([-18, 0]).unwrap().cell(), &key(0x210));
    assert!(
        other
            .prepare_cell(
                &mut store,
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    let plan = sources
        .prepare_cell(
            &mut store,
            &request,
            &MountIndex::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(plan.receipt().root, key(0x200));
    assert_eq!(
        plan.receipt().source_cohort_sha256,
        metadata.source_cohort_sha256
    );
    let mut owner = CellResidency::new(fixture.root.path(), None, Default::default()).unwrap();
    owner.request(plan).unwrap();
    let snapshot = owner.poll().unwrap();
    assert_eq!(snapshot.root, Some(key(0x200)));
    assert!(!snapshot.simulation_ready);
}

#[test]
fn winning_grid_and_world_moves_use_the_defining_master_table_without_old_fallback() {
    let mut fixture = Fixture::new();
    fixture.patch(
        &[
            group(0x100, &cell(0x200, 0, 0, Some([5, -6]))),
            group(0x101, &cell(0x201, 0, 0, Some([7, 8]))),
        ]
        .concat(),
    );
    let mut store = fixture.open(false);
    let sources = load(&mut store);
    assert!(sources.request([-18, 0]).is_err());
    assert!(sources.request([0, 0]).is_err());
    let request = sources.request([5, -6]).unwrap();
    assert_eq!(request.cell(), &key(0x200));
    let entry = sources
        .metadata()
        .entries
        .iter()
        .find(|entry| entry.key == key(0x200))
        .unwrap();
    assert_eq!(
        sources.metadata().sources[entry.source_ordinal].source_name,
        "Patch.esp"
    );
    assert_eq!(entry.parent_world_raw, 0x100);
    let other = CellGridSources::load(&mut store, &key(0x101), Limits::default()).unwrap();
    assert_eq!(other.request([7, 8]).unwrap().cell(), &key(0x201));
}

#[test]
fn duplicates_and_deleted_winners_do_not_manufacture_a_grid_winner() {
    let mut fixture = Fixture::new();
    fixture.patch(&group(0x100, &cell(0x0100_0220, 0, 0, Some([-18, 0]))));
    let mut store = fixture.open(false);
    assert!(
        load(&mut store)
            .request([-18, 0])
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    drop(store);
    let mut fixture = Fixture::new();
    fixture.patch(&group(
        0x100,
        &cell(0x200, plugin::DELETED, 0, Some([-18, 0])),
    ));
    let mut store = fixture.open(false);
    assert!(load(&mut store).request([-18, 0]).is_err());
}

#[test]
fn every_source_admission_bound_accepts_exact_and_refuses_one_below() {
    let fixture = Fixture::new();
    let mut store = fixture.open(false);
    let sources = load(&mut store);
    let usage = &sources.metadata().usage;
    let record_bytes = sources
        .metadata()
        .entries
        .iter()
        .filter(|entry| entry.role != Role::Deleted)
        .map(|entry| entry.header.stored_size as usize)
        .max()
        .unwrap();
    let exact = Limits {
        sources: usage.sources,
        winners_scanned: usage.winners_scanned,
        cells: usage.cells,
        record_bytes,
        read_bytes: usage.read_bytes,
        metadata_bytes: usage.metadata_bytes,
    };
    assert!(CellGridSources::load(&mut store, &key(0x100), exact).is_ok());
    for lower in [
        Limits {
            sources: exact.sources - 1,
            ..exact
        },
        Limits {
            winners_scanned: exact.winners_scanned - 1,
            ..exact
        },
        Limits {
            cells: exact.cells - 1,
            ..exact
        },
        Limits {
            record_bytes: exact.record_bytes - 1,
            ..exact
        },
        Limits {
            read_bytes: exact.read_bytes - 1,
            ..exact
        },
        Limits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
    ] {
        assert!(CellGridSources::load(&mut store, &key(0x100), lower).is_err());
    }
    assert!(
        CellGridSources::load(
            &mut store,
            &key(0x100),
            Limits {
                cells: 65_537,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn unchanged_cold_and_warm_metadata_match_and_changed_or_reordered_seals_refuse() {
    let mut fixture = Fixture::new();
    fixture.patch(&[]);
    fs::write(fixture.root.path().join("Data/Other.esm"), header(false)).unwrap();
    fixture.names.push("Other.esm".into());
    let mut store = fixture.open(false);
    let sources = load(&mut store);
    let request = sources.request([-18, 0]).unwrap();
    let expected = serde_json::to_value(sources.metadata()).unwrap();
    drop(store);
    for _ in 0..2 {
        let mut cached = fixture.open(true);
        assert_eq!(
            serde_json::to_value(load(&mut cached).metadata()).unwrap(),
            expected
        );
        assert!(
            sources
                .prepare_cell(
                    &mut cached,
                    &request,
                    &MountIndex::default(),
                    Default::default()
                )
                .is_ok()
        );
    }
    fixture.names.swap(1, 2);
    let mut reordered = fixture.open(false);
    assert!(
        sources
            .prepare_cell(
                &mut reordered,
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    drop(reordered);
    let path = fixture.root.path().join("Data/Base.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"STAT", 0x500, 0, &[]));
    fs::write(path, bytes).unwrap();
    fixture.names.swap(1, 2);
    let mut changed = fixture.open(false);
    assert!(
        sources
            .prepare_cell(
                &mut changed,
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    fixture.names.pop();
    let mut fewer = fixture.open(false);
    assert!(
        sources
            .prepare_cell(
                &mut fewer,
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
}

#[test]
fn wrong_or_deleted_world_and_malformed_requested_cell_refuse_construction() {
    let mut fixture = Fixture::new();
    let mut store = fixture.open(false);
    for world in [
        key(0x999),
        key(0x200),
        FormKey {
            profile: ProfileId::Fo3Original,
            ..key(0x100)
        },
    ] {
        assert!(CellGridSources::load(&mut store, &world, Limits::default()).is_err());
    }
    drop(store);
    fixture.patch(&record(b"WRLD", 0x100, plugin::DELETED, &[]));
    assert!(
        CellGridSources::load(&mut fixture.open(false), &key(0x100), Limits::default()).is_err()
    );
    let mut fixture = Fixture::new();
    fixture.patch(&group(
        0x100,
        &record(b"CELL", 0x200, 0, &field(b"DATA", &[0, 0])),
    ));
    assert!(
        CellGridSources::load(&mut fixture.open(false), &key(0x100), Limits::default()).is_err()
    );
}
