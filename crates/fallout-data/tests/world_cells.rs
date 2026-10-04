//! Authored source directories exercise actual winners, parent masters and CELL plans.
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{
        cells::{CellGridSources, Limits, Role},
        preparation::{Limits as ModelLimits, ModelSetLimits},
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

fn child_group(cell: u32, kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &cell.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn model_pair(fixture: &Fixture) -> MountIndex {
    let a = record(
        b"REFR",
        0x300,
        0,
        &[
            field(b"NAME", &0x400_u32.to_le_bytes()),
            field(b"DATA", &[0; 24]),
        ]
        .concat(),
    );
    let b = [
        record(
            b"REFR",
            0x301,
            0,
            &[
                field(b"NAME", &0x400_u32.to_le_bytes()),
                field(b"DATA", &[0; 24]),
            ]
            .concat(),
        ),
        record(
            b"REFR",
            0x302,
            0,
            &[
                field(b"NAME", &0x401_u32.to_le_bytes()),
                field(b"DATA", &[0; 24]),
            ]
            .concat(),
        ),
    ]
    .concat();
    let extra = [
        group(
            0x100,
            &[
                child_group(0x200, 6, &child_group(0x200, 9, &a)),
                child_group(0x201, 6, &child_group(0x201, 9, &b)),
            ]
            .concat(),
        ),
        record(b"STAT", 0x400, 0, &field(b"MODL", b"a.nif\0")),
        record(b"STAT", 0x401, 0, &field(b"MODL", b"b.nif\0")),
    ]
    .concat();
    let path = fixture.root.path().join("Data/Base.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(extra);
    fs::write(&path, bytes).unwrap();
    // Literal two-member uncompressed BSA104. Entry extents are [104,107) and
    // [107,112), independent of the production importer/planning algorithm.
    let mut bytes = vec![0; 112];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, word) in [
        (4, 104_u32),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 2),
        (24, 7),
        (28, 12),
        (44, 2),
        (48, 52),
        (68, 3),
        (72, 104),
        (84, 5),
        (88, 107),
    ] {
        bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes[52] = 7;
    bytes[53..60].copy_from_slice(b"meshes\0");
    bytes[92..104].copy_from_slice(b"a.nif\0b.nif\0");
    bytes[104..107].copy_from_slice(&[1, 2, 3]);
    bytes[107..112].copy_from_slice(&[5, 6, 7, 8, 9]);
    let path = fixture.root.path().join("Data/models.bsa");
    fs::write(&path, bytes).unwrap();
    let mut mounts = MountIndex::default();
    NvArchive::open(&path).unwrap().census(&mut mounts).unwrap();
    mounts
}

#[test]
fn unequal_ordered_cell_set_keeps_override_spans_and_reuses_exact_shared_archive() {
    let mut fixture = Fixture::new();
    let mounts = model_pair(&fixture);
    fixture.patch(
        &[
            group(0x100, &cell(0x200, 0, 0, Some([5, -6]))),
            record(b"STAT", 0x400, 0, &field(b"MODL", b"a.nif\0")),
        ]
        .concat(),
    );
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_set(&[[0, 0], [5, -6]]).unwrap();
    let set = directory
        .prepare_cells(&mut store, &request, &mounts, Default::default())
        .unwrap();
    assert_eq!(set.requests()[0].cell(), &key(0x201));
    assert_eq!(set.requests()[1].cell(), &key(0x200));
    assert_eq!(set.usage().grids, 2);
    assert_eq!(set.usage().models, 3);
    assert_eq!(set.usage().model_bytes, 11);
    assert_eq!(set.usage().archives, 1);
    assert_eq!(set.usage().mapped_bytes, 112);
    assert_eq!(set.plan(0).unwrap().receipt().requests.len(), 2);
    assert_eq!(set.plan(1).unwrap().receipt().requests.len(), 1);
    for i in 0..2 {
        let plan = set.plan(i).unwrap();
        let source = request.requests()[i].cell();
        assert_eq!(plan.root(), source);
        assert_eq!(
            plan.receipt().source_cohort_sha256,
            set.source_cohort_sha256()
        );
        let single = directory
            .prepare_cell(
                &mut store,
                &request.requests()[i],
                &mounts,
                Default::default(),
            )
            .unwrap();
        assert_eq!(single.identity(), plan.identity());
        let root = plan
            .graph()
            .nodes
            .iter()
            .find(|node| &node.key == source)
            .unwrap();
        assert_eq!(root.header.offset, if i == 0 { 145 } else { 95 });
        let coverage = &plan.receipt().coverage[0];
        assert_eq!(coverage.source_plugin, "Patch.esp");
        assert_eq!(coverage.header.offset, 140);
        assert_eq!(coverage.model_field.as_ref().unwrap().decoded_offset, 0);
        assert_eq!(coverage.model_field.as_ref().unwrap().value, b"a.nif");
    }
    let held = set.plan(0).unwrap().clone();
    let bytes = fs::read(fixture.root.path().join("Data/models.bsa")).unwrap();
    drop(set);
    #[cfg(windows)]
    assert!(fs::write(fixture.root.path().join("Data/models.bsa"), &bytes).is_err());
    drop(held);
    fs::write(fixture.root.path().join("Data/models.bsa"), bytes).unwrap();
}

#[test]
fn cell_set_exact_aggregate_bounds_accept_and_one_under_refuses_without_partial_pins() {
    let fixture = Fixture::new();
    let mounts = model_pair(&fixture);
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_set(&[[-18, 0], [0, 0]]).unwrap();
    let set = directory
        .prepare_cells(&mut store, &request, &mounts, Default::default())
        .unwrap();
    let u = set.usage();
    let exact = ModelSetLimits {
        grids: u.grids,
        sources: u.sources,
        winners_scanned: u.winners_scanned,
        nodes: u.nodes,
        edges: u.edges,
        bases: u.bases,
        candidates: u.candidates,
        models: u.models,
        field_sites: u.field_sites,
        read_bytes: u.read_bytes,
        decoded_bytes: u.decoded_bytes,
        model_bytes: u.model_bytes,
        probe_metadata_bytes: u.probe_metadata_bytes,
        metadata_bytes: u.metadata_bytes,
        archives: u.archives,
        mapped_bytes: u.mapped_bytes,
        ..Default::default()
    };
    let identity = set.identity().to_owned();
    drop(set);
    assert_eq!(
        directory
            .prepare_cells(&mut store, &request, &mounts, exact)
            .unwrap()
            .identity(),
        identity
    );
    let variants = [
        ModelSetLimits {
            grids: exact.grids - 1,
            ..exact
        },
        ModelSetLimits {
            sources: exact.sources - 1,
            ..exact
        },
        ModelSetLimits {
            winners_scanned: exact.winners_scanned - 1,
            ..exact
        },
        ModelSetLimits {
            nodes: exact.nodes - 1,
            ..exact
        },
        ModelSetLimits {
            edges: exact.edges - 1,
            ..exact
        },
        ModelSetLimits {
            bases: exact.bases - 1,
            ..exact
        },
        ModelSetLimits {
            candidates: exact.candidates - 1,
            ..exact
        },
        ModelSetLimits {
            models: exact.models - 1,
            ..exact
        },
        ModelSetLimits {
            field_sites: exact.field_sites - 1,
            ..exact
        },
        ModelSetLimits {
            read_bytes: exact.read_bytes - 1,
            ..exact
        },
        ModelSetLimits {
            decoded_bytes: exact.decoded_bytes - 1,
            ..exact
        },
        ModelSetLimits {
            model_bytes: exact.model_bytes - 1,
            ..exact
        },
        ModelSetLimits {
            probe_metadata_bytes: exact.probe_metadata_bytes - 1,
            ..exact
        },
        ModelSetLimits {
            metadata_bytes: exact.metadata_bytes - 1,
            ..exact
        },
        ModelSetLimits {
            archives: 0,
            ..exact
        },
        ModelSetLimits {
            mapped_bytes: exact.mapped_bytes - 1,
            ..exact
        },
    ];
    let path = fixture.root.path().join("Data/models.bsa");
    let bytes = fs::read(&path).unwrap();
    for (i, limits) in variants.into_iter().enumerate() {
        assert!(
            directory
                .prepare_cells(&mut store, &request, &mounts, limits)
                .is_err(),
            "bound {i}"
        );
        // A refused late plan releases earlier plans and the pooled mapping.
        fs::write(&path, &bytes).unwrap();
    }
    assert!(
        directory
            .prepare_cells(
                &mut store,
                &request,
                &mounts,
                ModelSetLimits {
                    grids: 9,
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn cell_set_selection_never_infers_missing_ambiguous_deleted_or_duplicate_grids() {
    let mut fixture = Fixture::new();
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    for grids in [
        vec![],
        vec![[-18, 0], [-18, 0]],
        vec![[-18, 0], [1, 2]],
        vec![[0, 0]; 9],
    ] {
        assert!(directory.request_set(&grids).is_err());
    }
    let extremes = directory
        .request_set(&[[i32::MIN, i32::MAX], [0, 0]])
        .unwrap();
    assert_eq!(extremes.requests()[0].grid(), [i32::MIN, i32::MAX]);
    drop(store);
    fixture.patch(&group(
        0x100,
        &cell(0x201, plugin::DELETED, 0, Some([0, 0])),
    ));
    let mut store = fixture.open(false);
    assert!(load(&mut store).request_set(&[[-18, 0], [0, 0]]).is_err());
    drop(store);
    for (flags, data, grid) in [
        (plugin::PERSISTENT, 0, Some([0, 0])),
        (0, 1, Some([0, 0])),
        (0, 0, None),
    ] {
        let mut fixture = Fixture::new();
        fixture.patch(&group(0x100, &cell(0x201, flags, data, grid)));
        assert!(
            load(&mut fixture.open(false))
                .request_set(&[[-18, 0], [0, 0]])
                .is_err()
        );
    }
    let mut fixture = Fixture::new();
    fixture.patch(&group(0x100, &cell(0x0100_0220, 0, 0, Some([0, 0]))));
    assert!(
        load(&mut fixture.open(false))
            .request_set(&[[-18, 0], [0, 0]])
            .is_err()
    );
}

#[test]
fn cell_set_rejects_changed_order_count_bytes_and_another_world_directory() {
    let mut fixture = Fixture::new();
    fixture.patch(&[]);
    fs::write(fixture.root.path().join("Data/Other.esm"), header(false)).unwrap();
    fixture.names.push("Other.esm".into());
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_set(&[[-18, 0], [0, 0]]).unwrap();
    let other = CellGridSources::load(&mut store, &key(0x101), Limits::default()).unwrap();
    assert!(
        other
            .prepare_cells(
                &mut store,
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    drop(store);
    fixture.names.swap(1, 2);
    assert!(
        directory
            .prepare_cells(
                &mut fixture.open(false),
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    fixture.names.swap(1, 2);
    fixture.names.pop();
    assert!(
        directory
            .prepare_cells(
                &mut fixture.open(false),
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    fixture.names.push("Other.esm".into());
    let path = fixture.root.path().join("Data/Base.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"STAT", 0x900, 0, &[]));
    fs::write(path, bytes).unwrap();
    assert!(
        directory
            .prepare_cells(
                &mut fixture.open(false),
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
}

#[test]
fn empty_model_cell_sets_still_charge_metadata_and_allow_exact_zero_byte_records() {
    let fixture = Fixture::new();
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_set(&[[-18, 0], [0, 0]]).unwrap();
    let set = directory
        .prepare_cells(
            &mut store,
            &request,
            &MountIndex::default(),
            Default::default(),
        )
        .unwrap();
    let limits = ModelSetLimits {
        read_bytes: set.usage().read_bytes,
        decoded_bytes: set.usage().decoded_bytes,
        metadata_bytes: set.usage().metadata_bytes,
        models: 0,
        model_bytes: 0,
        archives: 0,
        mapped_bytes: 0,
        ..Default::default()
    };
    assert!(set.usage().metadata_bytes > 0);
    assert_eq!(set.usage().models, 0);
    drop(set);
    let set = directory
        .prepare_cells(&mut store, &request, &MountIndex::default(), limits)
        .unwrap();
    assert_eq!(set.usage().grids, 2);
}

#[test]
fn persistent_request_keeps_misleading_grid_and_remapped_override_as_separate_authority() {
    let mut fixture = Fixture::new();
    let mounts = model_pair(&fixture);
    let base_path = fixture.root.path().join("Data/Base.esm");
    let mut base = fs::read(&base_path).unwrap();
    base.extend(group(
        0x100,
        &child_group(
            0x202,
            6,
            &child_group(
                0x202,
                9,
                &record(
                    b"REFR",
                    0x350,
                    0,
                    &[
                        field(b"NAME", &0x400_u32.to_le_bytes()),
                        field(b"DATA", &[0; 24]),
                    ]
                    .concat(),
                ),
            ),
        ),
    ));
    fs::write(&base_path, base).unwrap();
    fs::write(fixture.root.path().join("Data/Other.esm"), header(false)).unwrap();
    // Two authored master slots: the winning CELL/world labels use Base at slot1.
    let patch = [
        record(
            b"TES4",
            0,
            0,
            &[
                field(
                    b"HEDR",
                    &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
                ),
                field(b"MAST", b"Other.esm\0"),
                field(b"DATA", &[0; 8]),
                field(b"MAST", b"Base.esm\0"),
                field(b"DATA", &[0; 8]),
            ]
            .concat(),
        ),
        group(
            0x0100_0100,
            &cell(0x0100_0202, plugin::PERSISTENT, 0, Some([-18, 0])),
        ),
    ]
    .concat();
    assert_eq!(&patch[109..113], &0x0100_0100_u32.to_le_bytes());
    assert_eq!(&patch[125..129], b"CELL");
    assert_eq!(&patch[133..137], &0x400_u32.to_le_bytes());
    assert_eq!(&patch[137..141], &0x0100_0202_u32.to_le_bytes());
    assert_eq!(&patch[162..170], &[0xee, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
    fs::write(fixture.root.path().join("Data/Patch.esp"), patch).unwrap();
    fixture.names = vec!["Base.esm".into(), "Other.esm".into(), "Patch.esp".into()];
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_persistent().unwrap();
    assert_eq!(request.world(), &key(0x100));
    assert_eq!(request.cell(), &key(0x202));
    assert!(
        serde_json::to_value(&request)
            .unwrap()
            .get("grid")
            .is_none()
    );
    assert_eq!(directory.request([-18, 0]).unwrap().cell(), &key(0x200));
    let entry = directory
        .metadata()
        .entries
        .iter()
        .find(|entry| &entry.key == request.cell())
        .unwrap();
    assert_eq!(entry.role, Role::PersistentGroup);
    assert_eq!(entry.source_ordinal, 2);
    assert_eq!(entry.header.offset, 125);
    assert_eq!(entry.header.form_id, 0x0100_0202);
    assert_eq!(entry.parent_world_raw, 0x0100_0100);
    assert_eq!(
        entry
            .fields
            .as_ref()
            .unwrap()
            .grid
            .as_ref()
            .unwrap()
            .decoded_offset,
        7
    );
    let plan = directory
        .prepare_persistent(&mut store, &request, &mounts, Default::default())
        .unwrap();
    assert_eq!(plan.root(), &key(0x202));
    assert_eq!(plan.graph().root_members, 1);
    assert_eq!(plan.receipt().requests.len(), 1);
    assert_eq!(plan.receipt().requests[0].decoded_bytes, 3);
    assert_eq!(
        plan.receipt().source_cohort_sha256,
        directory.metadata().source_cohort_sha256
    );
    assert!(!plan.receipt().runtime_ready);
    assert!(
        directory
            .prepare_persistent(
                &mut store,
                &request,
                &mounts,
                ModelLimits {
                    max_requests: 0,
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn persistent_selection_refuses_missing_deleted_multiple_and_wrong_directory() {
    for mode in ["missing", "deleted", "multiple"] {
        let mut fixture = Fixture::new();
        let (raw, flags) = match mode {
            "missing" => (0x202, 0),
            "deleted" => (0x202, plugin::PERSISTENT | plugin::DELETED),
            _ => (0x0100_0220, plugin::PERSISTENT),
        };
        fixture.patch(&group(0x100, &cell(raw, flags, 0, Some([-18, 0]))));
        let mut store = fixture.open(false);
        assert!(load(&mut store).request_persistent().is_err());
    }
    let fixture = Fixture::new();
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_persistent().unwrap();
    let other = CellGridSources::load(&mut store, &key(0x101), Limits::default()).unwrap();
    assert!(
        other
            .prepare_persistent(
                &mut store,
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    assert!(
        directory
            .prepare_persistent(
                &mut store,
                &request,
                &MountIndex::default(),
                ModelLimits {
                    dependencies: fallout_data::world::dependencies::Limits {
                        max_nodes: 0,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn persistent_plan_rejects_changed_source_order_count_and_bytes() {
    let mut fixture = Fixture::new();
    fixture.patch(&[]);
    fs::write(fixture.root.path().join("Data/Other.esm"), header(false)).unwrap();
    fixture.names.push("Other.esm".into());
    let mut store = fixture.open(false);
    let directory = load(&mut store);
    let request = directory.request_persistent().unwrap();
    drop(store);
    fixture.names.swap(1, 2);
    assert!(
        directory
            .prepare_persistent(
                &mut fixture.open(false),
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    fixture.names.swap(1, 2);
    fixture.names.pop();
    assert!(
        directory
            .prepare_persistent(
                &mut fixture.open(false),
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
    fixture.names.push("Other.esm".into());
    let path = fixture.root.path().join("Data/Base.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"STAT", 0x901, 0, &[]));
    fs::write(path, bytes).unwrap();
    assert!(
        directory
            .prepare_persistent(
                &mut fixture.open(false),
                &request,
                &MountIndex::default(),
                Default::default()
            )
            .is_err()
    );
}
