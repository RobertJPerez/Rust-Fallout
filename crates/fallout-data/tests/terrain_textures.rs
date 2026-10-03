//! Original plugins exercise winning identities, dependency diagnostics and limits.
//! In-memory archive fixtures supply bytes; no retail assets are distributed.
use fallout_data::{
    Error, Result,
    plugin::{self, Record, RecordHeader},
    store::RecordStore,
    terrain::{
        self, Fields,
        textures::{self, AssetRead, AssetReader},
    },
    vfs::{AssetPath, AssetSource, MountIndex},
};
use flate2::{Compression, write::ZlibEncoder};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn sub(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], id: u32, flags: u32, bytes: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        bytes,
    ]
    .concat()
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
fn base(ltex: &[u8], txst: &[u8]) -> Vec<u8> {
    let mut out = header(&[]);
    out.extend(record(b"WRLD", 0x100, 0, &sub(b"DATA", &[0])));
    let cell = record(
        b"CELL",
        0x200,
        0,
        &[
            sub(b"EDID", b"Outside\0"),
            sub(b"DATA", &[0]),
            sub(b"XCLC", &[0; 12]),
        ]
        .concat(),
    );
    let layer = sub(
        b"BTXT",
        &[0x400u32.to_le_bytes().as_slice(), &[0, 77, 0xff, 0xff]].concat(),
    );
    let land = record(b"LAND", 0x300, 0, &[layer.clone(), layer].concat());
    out.extend(group(
        0x100,
        1,
        &[cell, group(0x200, 6, &group(0x200, 9, &land))].concat(),
    ));
    out.extend(record(b"LTEX", 0x400, 0, ltex));
    out.extend(record(b"TXST", 0x500, 0, txst));
    out
}
fn ltex(raw: u32) -> Vec<u8> {
    [
        sub(b"TNAM", &raw.to_le_bytes()),
        sub(b"HNAM", &[18, 255, 128]),
        sub(b"SNAM", &[255]),
    ]
    .concat()
}
fn txst() -> Vec<u8> {
    [
        sub(b"TX00", b"Land\\stone.dds\0"),
        sub(b"TX01", b"textures/land/STONE.dds\0"),
        sub(b"TX04", b"\0"),
        sub(b"DNAM", &0x8001u16.to_le_bytes()),
        sub(b"ZZZZ", &[9, 8, 7]),
    ]
    .concat()
}
fn setup(base: &[u8], patch: Option<&[u8]>) -> (tempfile::TempDir, RecordStore) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Data")).unwrap();
    fs::write(dir.path().join("Data/Base.esm"), base).unwrap();
    let mut order = vec!["Base.esm".into()];
    if let Some(patch) = patch {
        fs::write(dir.path().join("Data/Patch.esp"), patch).unwrap();
        order.push("Patch.esp".into());
    }
    let store =
        RecordStore::open_nv_headers(&dir.path().join("Data"), &order, plugin::Limits::default())
            .unwrap();
    (dir, store)
}
struct Assets {
    root: PathBuf,
    mounts: MountIndex,
    bytes: BTreeMap<AssetPath, Vec<u8>>,
    reads: usize,
}
impl Assets {
    fn new(root: &Path) -> Self {
        Self {
            root: root.into(),
            mounts: MountIndex::default(),
            bytes: BTreeMap::new(),
            reads: 0,
        }
    }
    fn add(&mut self, path: &[u8], container: &str, bytes: Vec<u8>) {
        self.mounts
            .insert(AssetSource {
                container: container.into(),
                entry_index: 0,
                original_path: path.into(),
            })
            .unwrap();
        self.bytes.insert(AssetPath::new(path).unwrap(), bytes);
    }
    fn stone(root: &Path) -> Self {
        let mut a = Self::new(root);
        a.add(b"textures/land/stone.dds", "fixture.bsa", vec![1, 2, 3, 4]);
        a
    }
}
impl AssetReader for Assets {
    fn source_tree(&self) -> &Path {
        &self.root
    }
    fn candidates(&self, p: &AssetPath) -> Result<&[AssetSource]> {
        self.mounts.candidates(p.bytes())
    }
    fn read(&mut self, p: &AssetPath, maximum: usize) -> Result<AssetRead> {
        self.reads += 1;
        let source = self.mounts.unique(p.bytes())?.unwrap().clone();
        let bytes = &self.bytes[p];
        if bytes.len() > maximum {
            return Err(Error::Unsupported("fixture byte budget exceeded".into()));
        }
        Ok(AssetRead {
            source,
            archive_sha256: "ab".repeat(32),
            bytes: bytes.clone(),
        })
    }
}
fn inspect(
    store: &mut RecordStore,
    assets: &mut Assets,
    limits: textures::Limits,
) -> Result<textures::Report> {
    let terrain = terrain::inspect_cell(store, b"Outside", None)?;
    textures::inspect(store, &terrain, assets, None, None, limits)
}

#[test]
fn shared_layers_and_case_variants_read_one_asset_and_keep_all_authored_usages() {
    let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), None);
    let mut assets = Assets::stone(dir.path());
    let report = inspect(&mut store, &mut assets, textures::Limits::default()).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.records.len(), 2);
    assert_eq!(report.bindings.len(), 2);
    assert_eq!(assets.reads, 1);
    assert_eq!(report.assets.len(), 1);
    assert_eq!(report.assets[0].usages, [0, 1]);
    assert_eq!(report.decoded_asset_bytes, 4);
    assert_eq!(report.path_usages.len(), 3);
    assert!(report.path_usages[2].raw_path.is_empty() && report.path_usages[2].path.is_none());
    let set = report
        .records
        .iter()
        .find_map(|r| {
            if let Some(Fields::TextureSet(s)) = &r.fields {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert!(set.paths[2].is_none());
    assert_eq!(set.flags.as_ref().unwrap().value, 0x8001);
    assert_eq!(set.unhandled[0].bytes, [9, 8, 7]);
    assert!(!report.runtime_ready);
}

#[test]
fn grass_links_and_unknown_texture_slots_remain_explicit() {
    let texture = [
        ltex(0x500),
        sub(b"GNAM", &0x600u32.to_le_bytes()),
        sub(b"GNAM", &[0; 4]),
    ]
    .concat();
    let set = [txst(), sub(b"TX06", b"future.dds\0")].concat();
    for kind in [b"GRAS", b"STAT"] {
        let mut source = base(&texture, &set);
        source.extend(record(kind, 0x600, 0, &[]));
        let (dir, mut store) = setup(&source, None);
        let mut assets = Assets::stone(dir.path());
        let report = inspect(&mut store, &mut assets, Default::default()).unwrap();
        assert_eq!(report.failures, usize::from(kind == b"STAT"));
        let texture = report
            .records
            .iter()
            .find(|r| r.header.kind == *b"LTEX")
            .unwrap();
        assert_eq!(texture.links["grass[1]"].status, "null");
        assert_eq!(
            texture.links["grass[0]"].status,
            if kind == b"GRAS" {
                "resolved"
            } else {
                "wrong-record-kind"
            }
        );
        let set = report
            .records
            .iter()
            .find_map(|r| {
                if let Some(Fields::TextureSet(s)) = &r.fields {
                    Some(s)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(set.unhandled.last().unwrap().kind, "TX06");
        assert_eq!(report.assets.len(), 1);
    }
}

#[test]
fn null_land_layers_keep_unapplied_defaults_without_inventing_an_asset() {
    let mut source = base(&ltex(0x500), &txst());
    let pattern = [0, 4, 0, 0, 0, 77, 255, 255];
    while let Some(at) = source.windows(8).position(|v| v == pattern) {
        source[at..at + 4].fill(0);
    }
    let (dir, mut store) = setup(&source, None);
    let mut assets = Assets::stone(dir.path());
    let report = inspect(&mut store, &mut assets, Default::default()).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.unapplied_default_layers, 2);
    assert!(report.records.is_empty() && report.assets.is_empty() && !report.runtime_ready);
    assert!(
        report
            .bindings
            .iter()
            .all(|b| b.status == "null-default-unapplied" && b.land_texture.key.is_none())
    );
    assert_eq!(assets.reads, 0);
}

#[test]
fn winning_texture_override_rebinds_in_its_own_master_table_without_field_merge() {
    let mut patch = header(&["Base.esm"]);
    patch.extend(record(b"TXST", 0x500, 0, &sub(b"TX00", b"patch.dds\0")));
    let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), Some(&patch));
    let mut assets = Assets::new(dir.path());
    assets.add(b"textures/patch.dds", "patch.bsa", vec![42]);
    let report = inspect(&mut store, &mut assets, textures::Limits::default()).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.path_usages.len(), 1);
    assert_eq!(report.path_usages[0].texture_set.origin_plugin, "base.esm");
    let winner = report
        .records
        .iter()
        .find(|r| r.header.kind == *b"TXST")
        .unwrap();
    assert_eq!(winner.source_plugin, "Patch.esp");
    let Some(Fields::TextureSet(set)) = &winner.fields else {
        panic!()
    };
    assert!(set.paths[1].is_none() && set.flags.is_none());
    // An LTEX introduced by the patch resolves its self-selector to Patch.esp.
    patch.extend(record(b"LTEX", 0x400, 0, &ltex(0x01000700)));
    patch.extend(record(
        b"TXST",
        0x01000700,
        0,
        &sub(b"TX00", b"patch.dds\0"),
    ));
    let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), Some(&patch));
    let mut assets = Assets::new(dir.path());
    assets.add(b"textures/patch.dds", "patch.bsa", vec![42]);
    let report = inspect(&mut store, &mut assets, textures::Limits::default()).unwrap();
    assert_eq!(report.failures, 0);
    assert_eq!(report.path_usages[0].texture_set.origin_plugin, "patch.esp");
}

#[test]
fn missing_null_deleted_and_wrong_kind_sets_stay_diagnostic() {
    for (raw, patch, status) in [
        (0x777, None, "missing"),
        (0, None, "null"),
        (
            0x500,
            Some(record(b"TXST", 0x500, plugin::DELETED, &[])),
            "deleted",
        ),
        (
            0x500,
            Some(record(b"STAT", 0x500, 0, &[])),
            "wrong-record-kind",
        ),
    ] {
        let patch = patch.map(|r| [header(&["Base.esm"]), r].concat());
        let (dir, mut store) = setup(&base(&ltex(raw), &txst()), patch.as_deref());
        let mut assets = Assets::stone(dir.path());
        let report = inspect(&mut store, &mut assets, textures::Limits::default()).unwrap();
        assert!(report.failures > 0);
        assert_eq!(
            report.bindings[0].texture_set.as_ref().unwrap().status,
            status
        );
        assert_eq!(assets.reads, 0);
    }
}

#[test]
fn absent_links_and_diffuse_paths_never_acquire_defaults() {
    for (texture, set, status) in [
        (vec![], txst(), "absent-TNAM"),
        (ltex(0x500), vec![], "missing-diffuse-path"),
        (ltex(0x500), sub(b"TX00", b"\0"), "missing-diffuse-path"),
    ] {
        let (dir, mut store) = setup(&base(&texture, &set), None);
        let mut assets = Assets::stone(dir.path());
        let report = inspect(&mut store, &mut assets, textures::Limits::default()).unwrap();
        assert!(report.failures > 0);
        assert_eq!(report.bindings[0].status, status);
        assert_eq!(assets.reads, 0);
    }
}

#[test]
fn ambiguous_missing_and_unsafe_paths_do_not_select_or_read_a_candidate() {
    for path in [
        b"../bad.dds\0".as_slice(),
        b"C:\\bad.dds\0",
        b"missing.dds\0",
        b"land/stone.dds\0",
    ] {
        let (dir, mut store) = setup(&base(&ltex(0x500), &sub(b"TX00", path)), None);
        let mut assets = Assets::stone(dir.path());
        assets.add(
            b"textures/land/stone.dds",
            "duplicate.bsa",
            vec![1, 2, 3, 4],
        );
        let report = inspect(&mut store, &mut assets, textures::Limits::default()).unwrap();
        assert!(report.failures > 0);
        assert_eq!(assets.reads, 0);
        assert!(report.assets.iter().all(|a| a.sha256.is_none()));
    }
}

#[test]
fn record_layer_asset_and_byte_limits_block_incomplete_closure() {
    for limits in [
        textures::Limits {
            records: 1,
            ..Default::default()
        },
        textures::Limits {
            layers: 1,
            ..Default::default()
        },
        textures::Limits {
            assets: 0,
            ..Default::default()
        },
    ] {
        let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), None);
        let mut assets = Assets::stone(dir.path());
        assert!(inspect(&mut store, &mut assets, limits).is_err());
        assert_eq!(assets.reads, 0);
    }
    let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), None);
    let mut assets = Assets::stone(dir.path());
    let report = inspect(
        &mut store,
        &mut assets,
        textures::Limits {
            decoded_asset_bytes: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.failures, 1);
    assert_eq!(report.decoded_asset_bytes, 0);
    assert!(report.assets[0].sha256.is_none());
}

#[test]
fn body_and_asset_caches_keep_tagged_provenance_and_refuse_source_destinations() {
    let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), None);
    let bodies = tempfile::tempdir().unwrap();
    let images = tempfile::tempdir().unwrap();
    let mut assets = Assets::stone(dir.path());
    let terrain = terrain::inspect_cell(&mut store, b"Outside", None).unwrap();
    let report = textures::inspect(
        &mut store,
        &terrain,
        &mut assets,
        Some((bodies.path(), dir.path())),
        Some(images.path()),
        Default::default(),
    )
    .unwrap();
    assert_eq!(report.failures, 0);
    assert!(report.records.iter().all(|r| r.body_cache.is_some()));
    assert!(report.assets[0].cache.is_some());
    assert!(
        textures::inspect(
            &mut store,
            &terrain,
            &mut assets,
            None,
            Some(dir.path()),
            Default::default()
        )
        .is_err()
    );
    assert!(
        textures::inspect(
            &mut store,
            &terrain,
            &mut assets,
            Some((dir.path(), dir.path())),
            None,
            Default::default()
        )
        .is_err()
    );
    assert_eq!(assets.reads, 1);
}

#[test]
fn strict_texture_payload_reads_reject_checksum_damage_and_oversized_inflation() {
    let (dir, mut store) = setup(&base(&ltex(0x500), &txst()), None);
    let terrain = terrain::inspect_cell(&mut store, b"Outside", None).unwrap();
    drop(store);
    for size in [50usize, 65 * 1024] {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&vec![0; size]).unwrap();
        let mut compressed = encoder.finish().unwrap();
        if size == 50 {
            *compressed.last_mut().unwrap() ^= 1;
        }
        let mut patch = header(&["Base.esm"]);
        patch.extend(record(
            b"TXST",
            0x500,
            plugin::COMPRESSED,
            &[(size as u32).to_le_bytes().as_slice(), &compressed].concat(),
        ));
        fs::write(dir.path().join("Data/Patch.esp"), patch).unwrap();
        let mut store = RecordStore::open_nv_headers(
            &dir.path().join("Data"),
            &["Base.esm".into(), "Patch.esp".into()],
            Default::default(),
        )
        .unwrap();
        let mut assets = Assets::stone(dir.path());
        assert!(
            textures::inspect(
                &mut store,
                &terrain,
                &mut assets,
                None,
                None,
                Default::default()
            )
            .is_err()
        );
        assert_eq!(assets.reads, 0);
    }
}

#[test]
fn texture_schema_rejects_duplicate_short_nonterminated_and_oversized_fields() {
    for (kind, body) in [
        (
            b"TXST",
            [sub(b"TX00", b"a\0"), sub(b"TX00", b"b\0")].concat(),
        ),
        (b"TXST", sub(b"TX00", b"a\0b\0")),
        (b"TXST", sub(b"TX01", b"missing")),
        (b"TXST", sub(b"TX00", &vec![1; 4098])),
        (b"TXST", sub(b"DNAM", &[0])),
        (b"LTEX", sub(b"HNAM", &[0; 4])),
        (b"LTEX", sub(b"TNAM", &[0; 3])),
        (b"LTEX", [sub(b"SNAM", &[1]), sub(b"SNAM", &[2])].concat()),
    ] {
        let r = Record {
            header: RecordHeader {
                kind: *kind,
                offset: 24,
                stored_size: body.len() as u32,
                flags: 0,
                form_id: 1,
                revision: [0; 4],
                version: 15,
                trailing_bytes: [0; 2],
            },
            payload: body,
            integrity_issue: None,
        };
        assert!(terrain::decode(&r, "fixture").is_err());
    }
}
