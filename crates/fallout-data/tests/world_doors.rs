//! Actual plugin indexing and sealed destination-cell preparation consumers.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{dependencies::Limits, doors::DoorDestination, residency::CellResidency},
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
fn group(label: u32, kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn members(cell: u32, records: &[u8]) -> Vec<u8> {
    group(cell, 6, &group(cell, 9, records))
}
fn cell(id: u32) -> Vec<u8> {
    record(b"CELL", id, 0, &field(b"DATA", &[1]))
}
fn teleport(target: u32, pose: [u32; 6]) -> Vec<u8> {
    field(
        b"XTEL",
        &[
            target.to_le_bytes().as_slice(),
            &pose
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            &0xdead_beef_u32.to_le_bytes(),
        ]
        .concat(),
    )
}
const SOURCE_POSE: [u32; 6] = [
    0x8000_0000,
    0x4128_0000,
    0x42c8_0000,
    0x4049_0fdb,
    0x8000_0000,
    0x3fc0_0000,
];
fn reference(id: u32, base: u32, flags: u32, extra: &[u8]) -> Vec<u8> {
    // Target-door DATA deliberately differs from source XTEL, including rotation.
    let pose = [99.0_f32, 88.0, 77.0, -7.0, -8.0, -9.0]
        .into_iter()
        .flat_map(|value| value.to_bits().to_le_bytes())
        .collect::<Vec<_>>();
    record(
        b"REFR",
        id,
        flags,
        &[
            extra,
            &field(b"DATA", &pose),
            &field(b"NAME", &base.to_le_bytes()),
        ]
        .concat(),
    )
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
    names: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        let bytes = [
            header(false),
            record(b"DOOR", 0x400, 0, &[]),
            record(b"STAT", 0x401, 0, &[]),
            cell(0x200),
            members(
                0x200,
                &reference(0x300, 0x400, 0, &teleport(0x301, SOURCE_POSE)),
            ),
            cell(0x201),
            members(0x201, &reference(0x301, 0x400, 0, &teleport(0x300, [0; 6]))),
        ]
        .concat();
        fs::write(root.path().join("Data/Base.esm"), bytes).unwrap();
        Self {
            root,
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
    fn open(&self) -> RecordStore {
        RecordStore::open_nv_headers(
            &self.root.path().join("Data"),
            &self.names,
            plugin::Limits::default(),
        )
        .unwrap()
    }
}
fn load(store: &mut RecordStore) -> DoorDestination {
    DoorDestination::load(store, &key(0x200), &key(0x300), Limits::default()).unwrap()
}

#[test]
fn exact_source_xtel_prepares_its_destination_cell_without_applying_pose() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let request = load(&mut store);
    let metadata = request.metadata();
    assert!(metadata.source_destination_resolved);
    assert!(!metadata.runtime_ready);
    let destination = metadata.destination.as_ref().unwrap();
    assert_eq!(destination.cell, key(0x201));
    assert_eq!(destination.door, key(0x301));
    let pose = destination
        .authored_transform
        .position
        .into_iter()
        .chain(destination.authored_transform.rotation)
        .map(f32::to_bits)
        .collect::<Vec<_>>();
    assert_eq!(pose, SOURCE_POSE);
    assert_eq!(destination.authored_transform_words, SOURCE_POSE);
    let serialized = serde_json::to_value(metadata).unwrap();
    assert_eq!(
        serialized["destination"]["authored_transform_words"],
        serde_json::json!(SOURCE_POSE)
    );
    assert_eq!(destination.raw_flags, 0xdead_beef);
    let graph = request.graph();
    let node = &graph.nodes[metadata.destination_cell_node.unwrap()];
    assert!(node.fields.is_none()); // deferred CELL body, never claimed validated.
    assert_eq!(node.key, key(0x201));
    let edge = &graph.edges[metadata.teleport_edge.unwrap()];
    assert_eq!(edge.raw_form_id, 0x301);
    assert_eq!(edge.site.field.unwrap().bytes, 32);
    // A normal pair is cyclic source data; preparation follows one XTEL only.
    assert_eq!(
        metadata.source_cycle_component,
        metadata.destination_cycle_component
    );
    assert!(metadata.source_cycle_component.is_some());
    let plan = request
        .prepare_cell(&mut store, &MountIndex::default(), Default::default())
        .unwrap();
    assert_eq!(plan.receipt().root, key(0x201));
    assert_eq!(plan.graph().root_members, 1);
    assert_eq!(
        plan.receipt().source_cohort_sha256,
        metadata.source_cohort_sha256
    );
    // Existing zero-asset source request is valid; its missing model coverage
    // keeps real residency unsupported rather than becoming a simulated teleport.
    let mut residency = CellResidency::new(fixture.root.path(), None, Default::default()).unwrap();
    residency.request(plan).unwrap();
    let snapshot = residency.poll().unwrap();
    assert_eq!(snapshot.root, Some(key(0x201)));
    assert!(!snapshot.complete_model_coverage);
    assert!(!snapshot.simulation_ready);
}

#[test]
fn winning_moved_target_uses_its_own_source_parent_and_master_table() {
    let mut fixture = Fixture::new();
    let moved_cell = 0x0100_0202;
    fixture.patch(
        &[
            cell(moved_cell),
            members(
                moved_cell,
                &reference(0x301, 0x400, 0, &teleport(0x300, [0; 6])),
            ),
        ]
        .concat(),
    );
    let mut store = fixture.open();
    let request = load(&mut store);
    let destination = request.metadata().destination.as_ref().unwrap();
    assert_eq!(destination.cell.origin_plugin, "patch.esp");
    assert_eq!(destination.cell.local_id, 0x202);
    let node = &request.graph().nodes[request.metadata().destination_node.unwrap()];
    assert_eq!(node.source_plugin, "Patch.esp");
    let plan = request
        .prepare_cell(&mut store, &MountIndex::default(), Default::default())
        .unwrap();
    assert_eq!(&plan.receipt().root, &destination.cell);
    assert_eq!(plan.graph().root_members, 1);
}

#[test]
fn unresolved_tombstoned_wrong_base_and_nonmember_requests_cannot_prepare() {
    for patch in [
        members(
            0x200,
            &reference(0x300, 0x400, 0, &teleport(0x999, SOURCE_POSE)),
        ),
        members(0x201, &reference(0x301, 0x400, plugin::DELETED, &[])),
        members(0x201, &reference(0x301, 0x401, 0, &[])),
        members(
            0x200,
            &reference(0x300, 0x401, 0, &teleport(0x301, SOURCE_POSE)),
        ),
        members(
            0x201,
            &reference(0x300, 0x400, 0, &teleport(0x301, SOURCE_POSE)),
        ),
        members(0x200, &reference(0x300, 0x400, 0, &[])),
        reference(0x301, 0x400, 0, &[]), // winning destination has no parent CELL.
        members(0x999, &reference(0x301, 0x400, 0, &[])),
        record(b"STAT", 0x201, 0, &[]), // winning parent identity is no longer CELL.
        record(b"CELL", 0x201, plugin::DELETED, &field(b"DATA", &[1])),
        members(0x200, &reference(0x300, 0x400, plugin::DELETED, &[])),
        members(
            0x200,
            &reference(0x300, 0x400, 0, &teleport(0, SOURCE_POSE)),
        ),
    ] {
        let mut fixture = Fixture::new();
        fixture.patch(&patch);
        let mut store = fixture.open();
        let request = load(&mut store);
        assert!(!request.metadata().source_destination_resolved);
        assert!(!request.metadata().issues.is_empty());
        assert!(request.metadata().destination.is_none());
        assert!(
            request
                .prepare_cell(&mut store, &MountIndex::default(), Default::default())
                .is_err()
        );
        assert!(!request.metadata().runtime_ready);
    }
}

#[test]
fn changed_or_reordered_source_cohort_cannot_reuse_a_destination_request() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let request = load(&mut store);
    drop(store);
    let path = fixture.root.path().join("Data/Base.esm");
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend(record(b"DOOR", 0x402, 0, &[]));
    fs::write(path, bytes).unwrap();
    let mut changed = fixture.open();
    assert!(
        request
            .prepare_cell(&mut changed, &MountIndex::default(), Default::default())
            .is_err()
    );
    drop(changed);
    let mut fixture = Fixture::new();
    fixture.patch(&[]);
    fs::write(fixture.root.path().join("Data/Other.esm"), header(false)).unwrap();
    fixture.names.push("Other.esm".into());
    let mut store = fixture.open();
    let request = load(&mut store);
    drop(store);
    fixture.names.swap(1, 2); // Both orders still put Base before its dependent.
    let mut reordered = fixture.open();
    assert!(
        request
            .prepare_cell(&mut reordered, &MountIndex::default(), Default::default())
            .is_err()
    );
}

#[test]
fn self_links_are_bounded_source_cycles_and_nonfinite_destinations_refuse() {
    let mut fixture = Fixture::new();
    fixture.patch(&members(
        0x200,
        &reference(0x300, 0x400, 0, &teleport(0x300, SOURCE_POSE)),
    ));
    let mut store = fixture.open();
    let request = load(&mut store);
    let metadata = request.metadata();
    assert_eq!(metadata.source_node, metadata.destination_node);
    assert!(metadata.source_cycle_component.is_some());
    assert!(metadata.source_destination_resolved);
    assert_eq!(metadata.destination.as_ref().unwrap().cell, key(0x200));
    assert!(!metadata.runtime_ready);
    let plan = request
        .prepare_cell(&mut store, &MountIndex::default(), Default::default())
        .unwrap();
    assert_eq!(plan.receipt().root, key(0x200));
    for bits in [f32::INFINITY.to_bits(), f32::NAN.to_bits()] {
        let mut fixture = Fixture::new();
        let mut pose = SOURCE_POSE;
        pose[0] = bits;
        fixture.patch(&members(
            0x200,
            &reference(0x300, 0x400, 0, &teleport(0x301, pose)),
        ));
        let mut store = fixture.open();
        assert!(
            DoorDestination::load(&mut store, &key(0x200), &key(0x300), Limits::default()).is_err()
        );
    }
}

#[test]
fn source_graph_and_added_request_metadata_share_the_existing_bound() {
    let fixture = Fixture::new();
    let mut store = fixture.open();
    let request = load(&mut store);
    let exact = request.graph().usage.metadata_bytes;
    let limits = Limits {
        max_metadata_bytes: exact,
        ..Default::default()
    };
    assert!(DoorDestination::load(&mut store, &key(0x200), &key(0x300), limits).is_ok());
    assert!(
        DoorDestination::load(
            &mut store,
            &key(0x200),
            &key(0x300),
            Limits {
                max_metadata_bytes: exact - 1,
                ..limits
            }
        )
        .is_err()
    );
    assert!(
        DoorDestination::load(
            &mut store,
            &key(0x200),
            &key(0x300),
            Limits {
                max_nodes: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
}
