//! Authored source graphs exercise real indexing, winning overrides and reads.
use fallout_data::{
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    vfs::MountIndex,
    world::{
        self,
        dependencies::{self, Limits, Report},
    },
};
use flate2::{Compression, write::ZlibEncoder};
use sha2::{Digest, Sha256};
use std::{fs, io::Write};

fn sub(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}

fn record(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
    ]
    .concat()
}

fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = sub(
        b"HEDR",
        &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(sub(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(sub(b"DATA", &[0; 8]));
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

fn cell(id: u32, name: &[u8]) -> Vec<u8> {
    record(
        b"CELL",
        id,
        0,
        &[
            sub(b"EDID", &[name, &[0]].concat()),
            sub(b"DATA", &[0]),
            sub(
                b"XCLC",
                &[(-4i32).to_le_bytes(), 7i32.to_le_bytes()].concat(),
            ),
        ]
        .concat(),
    )
}

fn parent(raw: u32) -> Vec<u8> {
    sub(
        b"XESP",
        &[raw.to_le_bytes().as_slice(), &[0x85, 7, 8, 9]].concat(),
    )
}

fn extended_parent(raw: u32) -> Vec<u8> {
    [
        sub(b"XXXX", &8u32.to_le_bytes()),
        b"XESP".to_vec(),
        0u16.to_le_bytes().to_vec(),
        raw.to_le_bytes().to_vec(),
        vec![0x85, 7, 8, 9],
    ]
    .concat()
}

fn teleport(raw: u32) -> Vec<u8> {
    sub(
        b"XTEL",
        &[
            raw.to_le_bytes().as_slice(),
            &[0; 24],
            &0xdeadbeefu32.to_le_bytes(),
        ]
        .concat(),
    )
}

fn placement(kind: &[u8; 4], id: u32, base: u32, extra: &[u8]) -> Vec<u8> {
    // XESP/XTEL deliberately precede NAME. Source bits include negative zero.
    let transform = [0x80000000u32, 0x3fc00000, 0, 0, 0, 0]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    record(
        kind,
        id,
        0,
        &[
            extra,
            &sub(b"NAME", &base.to_le_bytes()),
            &sub(b"DATA", &transform),
            &sub(b"ZZZZ", &[13, 14]),
        ]
        .concat(),
    )
}

fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: id,
    }
}

struct Fixture {
    source: tempfile::TempDir,
    cache: tempfile::TempDir,
    names: Vec<String>,
}

impl Fixture {
    fn open(&self, cached: bool) -> RecordStore {
        if cached {
            RecordStore::open_nv_headers_cached(
                &self.source.path().join("Data"),
                &self.names,
                plugin::Limits::default(),
                self.cache.path(),
            )
            .unwrap()
        } else {
            RecordStore::open_nv_headers(
                &self.source.path().join("Data"),
                &self.names,
                plugin::Limits::default(),
            )
            .unwrap()
        }
    }
}

fn fixture() -> Fixture {
    let source = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let data = source.path().join("Data");
    fs::create_dir(&data).unwrap();
    fs::write(
        data.join("Aux.esm"),
        [header(&[]), record(b"SCPT", 0x800, 0, b"deferred")].concat(),
    )
    .unwrap();
    let mut base = header(&[]);
    base.extend(record(
        b"WRLD",
        0x100,
        0,
        &[
            sub(b"ZNAM", &0x504u32.to_le_bytes()),
            sub(b"WNAM", &0x101u32.to_le_bytes()),
            sub(b"CNAM", &0x500u32.to_le_bytes()),
            sub(b"NAM2", &0x501u32.to_le_bytes()),
            sub(b"NAM3", &0u32.to_le_bytes()),
            sub(b"INAM", &0x502u32.to_le_bytes()),
            sub(b"XEZN", &0x503u32.to_le_bytes()),
            sub(b"ZZZZ", &[99]),
        ]
        .concat(),
    ));
    base.extend(record(
        b"WRLD",
        0x101,
        0,
        &sub(b"WNAM", &0x100u32.to_le_bytes()),
    ));
    for (kind, id, flags, body) in [
        (b"STAT", 0x400, 0, sub(b"ZZZZ", &[17])),
        (b"SCPT", 0x500, 0, b"not decoded".to_vec()),
        (b"WATR", 0x501, plugin::DELETED, b"deleted bytes".to_vec()),
        (b"ECZN", 0x503, 0, b"terminal bytes".to_vec()),
        (b"MUSC", 0x504, 0, b"terminal bytes".to_vec()),
        (b"NPC_", 0x600, 0, sub(b"ZZZZ", &[18])),
        (b"CREA", 0x601, 0, sub(b"ZZZZ", &[19])),
    ] {
        base.extend(record(kind, id, flags, &body));
    }
    let children = [
        placement(b"REFR", 0x3f0, 0x400, &extended_parent(0x310)),
        placement(
            b"REFR",
            0x310,
            0x400,
            &[parent(0x3f0), teleport(0x320)].concat(),
        ),
        placement(b"REFR", 0x330, 0x400, &[]),
        placement(b"REFR", 0x340, 0x400, &[]),
        placement(b"ACHR", 0x360, 0x600, &parent(0x14)),
        placement(b"ACRE", 0x370, 0x601, &[]),
        placement(
            b"REFR",
            0x380,
            0x500,
            &[parent(0x330), teleport(0x500)].concat(),
        ),
        placement(
            b"REFR",
            0x390,
            0x999,
            &[parent(0), teleport(0x330)].concat(),
        ),
        placement(b"REFR", 0x3a0, 0x400, &parent(0x602)),
        record(b"PGRE", 0x602, 0, b"deferred projectile"),
        record(b"NAVM", 0x350, 0, b"deferred navmesh"),
    ]
    .concat();
    base.extend(group(
        0x100,
        1,
        &[
            cell(0x200, b"RootClinic"),
            group(0x200, 6, &group(0x200, 9, &children)),
            cell(0x201, b"OtherCell"),
            group(
                0x201,
                6,
                &group(
                    0x201,
                    9,
                    &placement(b"REFR", 0x320, 0x400, &teleport(0x310)),
                ),
            ),
        ]
        .concat(),
    ));
    fs::write(data.join("FalloutNV.esm"), base).unwrap();
    let mut patch = header(&["Aux.esm", "FalloutNV.esm"]);
    patch.extend(group(
        0x01000100,
        1,
        &[
            cell(0x01000200, b"RootClinic"),
            group(
                0x01000200,
                6,
                &group(
                    0x01000200,
                    9,
                    &[
                        record(
                            b"REFR",
                            0x01000330,
                            plugin::DELETED,
                            b"ignored invalid bytes",
                        ),
                        // Noncanonical self selector resolves to Patch.esp, not load-order slot.
                        placement(b"REFR", 0xfe000700, 0x01000400, &parent(0x00000800)),
                    ]
                    .concat(),
                ),
            ),
            group(
                0x01000201,
                6,
                &group(
                    0x01000201,
                    9,
                    &placement(b"REFR", 0x01000340, 0x01000400, &[]),
                ),
            ),
        ]
        .concat(),
    ));
    fs::write(data.join("Patch.esp"), patch).unwrap();
    Fixture {
        source,
        cache,
        names: vec!["Aux.esm".into(), "FalloutNV.esm".into(), "Patch.esp".into()],
    }
}

fn inspect(store: &mut RecordStore) -> Report {
    dependencies::inspect_cell_key(store, &key(0x200), Limits::default()).unwrap()
}

#[test]
fn exact_winners_membership_and_source_physical_order_do_not_resurrect_predecessors() {
    let fixture = fixture();
    let mut store = fixture.open(false);
    let report = inspect(&mut store);
    assert_eq!(report.root_members, 11);
    let members: Vec<_> = report
        .edges
        .iter()
        .filter(|edge| edge.role == "member")
        .map(|edge| edge.target.key.as_ref().unwrap().local_id)
        .collect();
    assert_eq!(
        members,
        [
            0x3f0, 0x310, 0x360, 0x370, 0x380, 0x390, 0x3a0, 0x602, 0x350, 0x330, 0x700
        ]
    );
    assert!(!report.nodes.iter().any(|node| node.key == key(0x340)));
    assert!(
        report
            .nodes
            .windows(2)
            .all(|pair| (pair[0].source_ordinal, pair[0].header.offset)
                < (pair[1].source_ordinal, pair[1].header.offset))
    );
    let root = report
        .nodes
        .iter()
        .find(|node| node.key == key(0x200))
        .unwrap();
    assert_eq!(root.source_plugin, "Patch.esp");
    assert_eq!(root.parent.world, Some(0x01000100));
    let deleted = report
        .nodes
        .iter()
        .find(|node| node.key == key(0x330))
        .unwrap();
    assert_eq!(deleted.source_plugin, "Patch.esp");
    assert!(deleted.decoded_body.is_none() && deleted.fields.is_none());
    for id in [
        0x350, 0x400, 0x500, 0x501, 0x503, 0x504, 0x600, 0x601, 0x602, 0x201,
    ] {
        let node = report
            .nodes
            .iter()
            .find(|node| node.key == key(id))
            .unwrap();
        assert!(node.decoded_body.is_none(), "terminal {id:x} was decoded");
    }
    assert!(
        report
            .nodes
            .iter()
            .find(|node| node.key == key(0x320))
            .unwrap()
            .decoded_body
            .is_some()
    );
    let own = report
        .nodes
        .iter()
        .find(|node| node.key.origin_plugin == "patch.esp" && node.key.local_id == 0x700)
        .unwrap();
    assert_eq!(own.header.form_id, 0xfe000700);
    assert_eq!(
        report
            .sources
            .iter()
            .map(|source| source.source_name.as_str())
            .collect::<Vec<_>>(),
        ["Aux.esm", "FalloutNV.esm", "Patch.esp"]
    );
    for source in &report.sources {
        let raw = fs::read(fixture.source.path().join("Data").join(&source.source_name)).unwrap();
        assert_eq!(source.source_bytes, raw.len() as u64);
        assert_eq!(source.source_sha256, format!("{:x}", Sha256::digest(raw)));
    }
    assert!(!report.runtime_ready);
}

#[test]
fn typed_statuses_and_raw_field_sites_preserve_unknowns_and_cycle_evidence() {
    let fixture = fixture();
    let mut store = fixture.open(false);
    let report = inspect(&mut store);
    let status = |owner, role| {
        report
            .edges
            .iter()
            .find(|edge| edge.owner == key(owner) && edge.role == role)
            .unwrap()
            .target
            .status
    };
    for (owner, role, expected) in [
        (0x100, "CNAM", "wrong-record-kind"),
        (0x100, "NAM2", "deleted"),
        (0x100, "NAM3", "null"),
        (0x100, "INAM", "missing"),
        (0x360, "XESP", "runtime-player-binding-unimplemented"),
        (0x380, "NAME", "wrong-record-kind"),
        (0x380, "XESP", "deleted"),
        (0x380, "XTEL", "wrong-record-kind"),
        (0x390, "NAME", "missing"),
        (0x390, "XESP", "null"),
        (0x390, "XTEL", "deleted"),
        (0x3a0, "XESP", "resolved"),
    ] {
        assert_eq!(status(owner, role), expected, "{owner:x} {role}");
    }
    let cycles: Vec<Vec<_>> = report
        .cyclic_link_components
        .iter()
        .map(|component| {
            let mut ids: Vec<_> = component
                .iter()
                .map(|index| report.nodes[*index].key.local_id)
                .collect();
            ids.sort_unstable();
            ids
        })
        .collect();
    assert_eq!(cycles, [vec![0x100, 0x101], vec![0x310, 0x320, 0x3f0]]);
    let roles: Vec<_> = report
        .edges
        .iter()
        .filter(|edge| edge.owner == key(0x100) && edge.site.field.is_some())
        .map(|edge| edge.role)
        .collect();
    assert_eq!(
        roles,
        ["ZNAM", "WNAM", "CNAM", "NAM2", "NAM3", "INAM", "XEZN"]
    );
    for node in &report.nodes {
        if let Some(body) = &node.decoded_body {
            assert_eq!(
                node.decoded_sha256.as_ref().unwrap(),
                &format!("{:x}", Sha256::digest(body))
            );
            assert!(
                node.field_sites
                    .windows(2)
                    .all(|pair| pair[0].span.decoded_offset < pair[1].span.decoded_offset)
            );
            for site in &node.field_sites {
                assert_eq!(site.span.decoded_offset, site.decoded_header_offset + 6);
                assert_eq!(
                    &body[site.decoded_header_offset..site.decoded_header_offset + 4],
                    &site.kind
                );
                assert!(site.span.decoded_offset + site.span.bytes <= body.len());
            }
            for edge in report
                .edges
                .iter()
                .filter(|edge| edge.owner == node.key && edge.site.field.is_some())
            {
                let span = edge.site.field.unwrap();
                assert_eq!(
                    u32::from_le_bytes(
                        body[span.decoded_offset..span.decoded_offset + 4]
                            .try_into()
                            .unwrap()
                    ),
                    edge.raw_form_id
                );
            }
        }
    }
    let placed = report
        .nodes
        .iter()
        .find(|node| node.key == key(0x3f0))
        .unwrap();
    let body = placed.decoded_body.as_ref().unwrap();
    let data = placed
        .field_sites
        .iter()
        .find(|site| site.kind == *b"DATA")
        .unwrap();
    assert_eq!(
        &body[data.span.decoded_offset..data.span.decoded_offset + 4],
        &0x80000000u32.to_le_bytes()
    );
    assert!(placed.field_sites.iter().any(|site| site.kind == *b"ZZZZ"));
}

#[test]
fn every_budget_is_enforced_and_failed_requests_can_retry_without_partial_state() {
    let fixture = fixture();
    let mut store = fixture.open(false);
    let baseline = inspect(&mut store);
    let usage = &baseline.usage;
    let maximum_body = baseline
        .nodes
        .iter()
        .filter_map(|node| {
            node.decoded_body
                .as_ref()
                .map(|body| body.len().max(node.header.stored_size as usize))
        })
        .max()
        .unwrap();
    let cases = [
        (
            Limits {
                max_winners_scanned: usage.winners_scanned - 1,
                ..Default::default()
            },
            "winning scan budget",
        ),
        (
            Limits {
                max_sources: usage.sources - 1,
                ..Default::default()
            },
            "sources budget",
        ),
        (
            Limits {
                max_nodes: usage.nodes - 1,
                ..Default::default()
            },
            "nodes budget",
        ),
        (
            Limits {
                max_edges: usage.edges - 1,
                ..Default::default()
            },
            "edges budget",
        ),
        (
            Limits {
                max_record_bytes: maximum_body - 1,
                ..Default::default()
            },
            "record size budget",
        ),
        (
            Limits {
                max_decoded_bytes: usage.decoded_bytes - 1,
                ..Default::default()
            },
            "record size budget",
        ),
        (
            Limits {
                max_field_sites: usage.field_sites - 1,
                ..Default::default()
            },
            "field sites budget",
        ),
        (
            Limits {
                max_metadata_bytes: usage.metadata_bytes - 1,
                ..Default::default()
            },
            "metadata bytes budget",
        ),
        (
            Limits {
                max_sources: 257,
                ..Default::default()
            },
            "ceiling exceeded",
        ),
    ];
    for (limits, reason) in cases {
        let error = dependencies::inspect_cell_key(&mut store, &key(0x200), limits)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(reason),
            "wrong rejection: {error}; expected {reason}"
        );
        assert_eq!(
            serde_json::to_value(inspect(&mut store)).unwrap(),
            serde_json::to_value(&baseline).unwrap()
        );
    }
    let exact = Limits {
        max_winners_scanned: usage.winners_scanned,
        max_sources: usage.sources,
        max_nodes: usage.nodes,
        max_edges: usage.edges,
        max_record_bytes: maximum_body,
        max_decoded_bytes: usage.decoded_bytes,
        max_field_sites: usage.field_sites,
        max_metadata_bytes: usage.metadata_bytes,
    };
    let report = dependencies::inspect_cell_key(&mut store, &key(0x200), exact).unwrap();
    assert_eq!(report.usage.metadata_bytes, usage.metadata_bytes);
    // Returned evidence owns bytes only; retained source locks belong to the store.
    let path = fixture.source.path().join("Data/Patch.esp");
    #[cfg(windows)]
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    drop(store);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_ok());
    assert!(report.nodes.iter().any(|node| node.decoded_body.is_some()));
}

#[test]
fn cold_and_warm_indices_agree_and_legacy_cell_report_bytes_stay_identical() {
    let fixture = fixture();
    let mut uncached = fixture.open(false);
    let before = serde_json::to_vec(
        &world::inspect_cell(&mut uncached, b"RootClinic", &MountIndex::default()).unwrap(),
    )
    .unwrap();
    let expected = serde_json::to_value(inspect(&mut uncached)).unwrap();
    let after = serde_json::to_vec(
        &world::inspect_cell(&mut uncached, b"RootClinic", &MountIndex::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(before, after);
    for _ in 0..2 {
        let mut cached = fixture.open(true);
        assert_eq!(
            serde_json::to_value(inspect(&mut cached)).unwrap(),
            expected
        );
    }
}

#[test]
fn root_requires_exact_non_deleted_cell_identity() {
    let fixture = fixture();
    let mut store = fixture.open(false);
    for (root, reason) in [
        (key(0x999), "root CELL missing"),
        (key(0x400), "deleted or wrong-record-kind"),
        (key(0x330), "deleted or wrong-record-kind"),
        (
            FormKey {
                profile: ProfileId::Fo3Original,
                ..key(0x200)
            },
            "requires nv-original",
        ),
    ] {
        let error = dependencies::inspect_cell_key(&mut store, &root, Limits::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains(reason), "wrong root rejection: {error}");
    }
}

#[test]
fn deferred_compressed_body_fails_for_declared_budget_and_corruption_then_recovers() {
    let source = tempfile::tempdir().unwrap();
    let data = source.path().join("Data");
    fs::create_dir(&data).unwrap();
    let raw = [
        sub(b"NAME", &0x400u32.to_le_bytes()),
        sub(b"DATA", &[0; 24]),
        sub(b"ZZZZ", &[0; 2048]),
    ]
    .concat();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&raw).unwrap();
    let frame = [
        raw.len().to_le_bytes()[..4].to_vec(),
        encoder.finish().unwrap(),
    ]
    .concat();
    let make = |frame: &[u8]| {
        [
            header(&[]),
            record(b"STAT", 0x400, 0, &[]),
            cell(0x200, b"CompressedRoot"),
            group(
                0x200,
                6,
                &group(0x200, 9, &record(b"REFR", 0x300, plugin::COMPRESSED, frame)),
            ),
        ]
        .concat()
    };
    let path = data.join("FalloutNV.esm");
    fs::write(&path, make(&frame)).unwrap();
    let mut store =
        RecordStore::open_nv_headers(&data, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    for limits in [
        Limits {
            max_record_bytes: 512,
            ..Default::default()
        },
        Limits {
            max_decoded_bytes: 512,
            ..Default::default()
        },
    ] {
        let error = dependencies::inspect_cell_key(&mut store, &key(0x200), limits)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("decompression budget exceeded"),
            "wrong compressed rejection: {error}"
        );
    }
    assert_eq!(
        inspect(&mut store)
            .nodes
            .iter()
            .find(|node| node.key == key(0x300))
            .unwrap()
            .decoded_body
            .as_ref()
            .unwrap(),
        &raw
    );
    drop(store);
    let mut corrupt = frame.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    fs::write(&path, make(&corrupt)).unwrap();
    let mut bad =
        RecordStore::open_nv_headers(&data, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let error = dependencies::inspect_cell_key(&mut bad, &key(0x200), Limits::default())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("strict integrity check failed"),
        "wrong corruption rejection: {error}"
    );
    drop(bad);
    fs::write(&path, make(&frame)).unwrap();
    let mut repaired =
        RecordStore::open_nv_headers(&data, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    assert_eq!(inspect(&mut repaired).root_members, 1);
}
