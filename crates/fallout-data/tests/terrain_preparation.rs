//! Authored physical plugin/BSA sources, with no retail material assumptions.
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::{self, JobError},
    store::RecordStore,
    terrain::{
        self, Fields,
        preparation::{Limits, TexturePreparation, TextureSourcePlan},
    },
    vfs::MountIndex,
};
use flate2::{Compression, write::ZlibEncoder};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

fn sub(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(tag: &[u8; 4], id: u32, flags: u32, bytes: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
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
fn group(label: u32, kind: i32, bytes: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(bytes.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}
fn layer(tag: &[u8; 4], raw: u32, quadrant: u8) -> Vec<u8> {
    sub(
        tag,
        &[
            raw.to_le_bytes().as_slice(),
            &[quadrant, 77],
            &(-1i16).to_le_bytes(),
        ]
        .concat(),
    )
}
fn source(
    texture: u32,
    tnam: Option<u32>,
    txst_flags: u32,
    paths: &[(&[u8; 4], &[u8])],
    extra_land: bool,
    revision: u8,
) -> Vec<u8> {
    let mut out = record(
        b"TES4",
        0,
        0,
        &sub(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    out.extend(record(b"WRLD", 0x100, 0, &sub(b"DATA", &[0])));
    let cell = record(
        b"CELL",
        0x200,
        0,
        &[
            sub(b"EDID", b"PreparedExterior\0"),
            sub(b"DATA", &[0]),
            sub(b"XCLC", &[0; 12]),
        ]
        .concat(),
    );
    let body = [
        layer(b"BTXT", texture, 0),
        layer(b"ATXT", texture, 2),
        sub(
            b"VTXT",
            &[
                17u16.to_le_bytes().as_slice(),
                &[13, 29],
                &0.5f32.to_bits().to_le_bytes(),
            ]
            .concat(),
        ),
    ]
    .concat();
    let mut lands = record(b"LAND", 0x300, 0, &body);
    if extra_land {
        lands.extend(record(b"LAND", 0x301, 0, &body));
    }
    out.extend(group(
        0x100,
        1,
        &[cell, group(0x200, 6, &group(0x200, 9, &lands))].concat(),
    ));
    let mut ltex = sub(b"HNAM", &[18, 255, 128]);
    if let Some(raw) = tnam {
        ltex.extend(sub(b"TNAM", &raw.to_le_bytes()));
    }
    out.extend(record(b"LTEX", 0x400, 0, &ltex));
    let mut txst = sub(b"ZZZZ", &[revision]);
    for (tag, path) in paths {
        txst.extend(sub(tag, &[*path, &[0]].concat()));
    }
    out.extend(record(b"TXST", 0x500, txst_flags, &txst));
    out
}
fn default_source(revision: u8) -> Vec<u8> {
    source(
        0x400,
        Some(0x500),
        0,
        &[
            (b"TX00", b"land/stone.dds"),
            (b"TX01", b"textures\\LAND\\STONE.dds"),
            (b"TX04", b""),
            (b"TX05", b"land/sand.dds"),
        ],
        false,
        revision,
    )
}
fn archive(payloads: &[(&[u8], &[u8])]) -> Vec<u8> {
    let folder = b"textures\\land\0";
    let names: Vec<u8> = payloads
        .iter()
        .flat_map(|(name, _)| [*name, &[0]].concat())
        .collect();
    let entries = 53 + folder.len();
    let data_offset = entries + 16 * payloads.len() + names.len();
    let mut bytes = vec![0; data_offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104),
        (8, 36),
        (12, 7),
        (16, 1),
        (20, payloads.len() as u32),
        (24, folder.len() as u32),
        (28, names.len() as u32),
        (44, payloads.len() as u32),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[52] = folder.len() as u8;
    bytes[53..entries].copy_from_slice(folder);
    bytes[entries + 16 * payloads.len()..data_offset].copy_from_slice(&names);
    for (index, (_, payload)) in payloads.iter().enumerate() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        let stored = [
            &(payload.len() as u32).to_le_bytes(),
            encoder.finish().unwrap().as_slice(),
        ]
        .concat();
        let at = entries + index * 16;
        bytes[at + 8..at + 12].copy_from_slice(&(stored.len() as u32).to_le_bytes());
        let start = bytes.len() as u32;
        bytes[at + 12..at + 16].copy_from_slice(&start.to_le_bytes());
        bytes.extend(stored);
    }
    bytes
}
fn key() -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id: 0x200,
    }
}
struct Fixture {
    source: tempfile::TempDir,
    cache: tempfile::TempDir,
}
impl Fixture {
    fn new(plugin: &[u8]) -> Self {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir(source.path().join("Data")).unwrap();
        fs::write(source.path().join("Data/FalloutNV.esm"), plugin).unwrap();
        fs::write(
            source.path().join("Data/authored.bsa"),
            archive(&[
                (b"stone.dds", &[1, 2, 3, 4]),
                (b"sand.dds", &[5, 6, 7, 8, 9]),
            ]),
        )
        .unwrap();
        Self {
            source,
            cache: tempfile::tempdir().unwrap(),
        }
    }
    fn store(&self) -> RecordStore {
        RecordStore::open_nv_headers(
            &self.source.path().join("Data"),
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap()
    }
    fn mounts(&self) -> MountIndex {
        let mut mounts = MountIndex::default();
        NvArchive::open(&self.source.path().join("Data/authored.bsa"))
            .unwrap()
            .census(&mut mounts)
            .unwrap();
        mounts
    }
    fn plan(&self, store: &mut RecordStore) -> TextureSourcePlan {
        TextureSourcePlan::load(store, &key(), &self.mounts(), Default::default()).unwrap()
    }
    fn prepare(&self, plan: TextureSourcePlan) -> TexturePreparation {
        TexturePreparation::new(
            plan,
            self.source.path(),
            Some(self.cache.path()),
            Default::default(),
        )
        .unwrap()
    }
}
fn drained(preparation: &TexturePreparation) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while preparation.usage().outstanding != 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(preparation.usage().decoded_bytes, 0);
}

#[test]
fn cold_warm_exact_layers_sites_members_and_changed_source_cohort() {
    let fixture = Fixture::new(&default_source(0));
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    assert_eq!(plan.receipt().texture_sources.bindings.len(), 2);
    assert_eq!(plan.receipt().texture_sources.path_usages.len(), 4);
    assert_eq!(plan.receipt().requests.len(), 2);
    assert_eq!(plan.receipt().usage.texture_bytes, 9);
    let Some(Fields::Land(land)) = &plan.terrain().landscapes[0].fields else {
        panic!()
    };
    assert_eq!(land.layers[0].unused, 77);
    assert_eq!(
        land.layers[1].alpha.as_ref().unwrap().value[0].unused,
        [13, 29]
    );
    let mut preparation = fixture.prepare(plan.clone());
    let cold = preparation
        .wait()
        .unwrap()
        .publish_for(plan.terrain())
        .unwrap();
    assert!(
        cold.all_requested_texture_sources_ready
            && cold.all_authored_texture_sources_resolved
            && !cold.runtime_ready
    );
    assert!(
        cold.textures
            .iter()
            .all(|t| !t.cache.as_ref().unwrap().reused)
    );
    assert_eq!(
        cold.textures[0].sha256,
        format!("{:x}", Sha256::digest([5, 6, 7, 8, 9]))
    );
    drained(&preparation);
    preparation.retry().unwrap();
    let warm = preparation
        .wait()
        .unwrap()
        .publish_for(plan.terrain())
        .unwrap();
    assert_eq!(warm.generation, 2);
    assert!(
        warm.textures
            .iter()
            .all(|t| t.cache.as_ref().unwrap().reused)
    );
    drop(preparation);
    drop(warm);
    drop(cold);
    drop(store);
    fs::write(
        fixture.source.path().join("Data/FalloutNV.esm"),
        default_source(1),
    )
    .unwrap();
    let mut store = fixture.store();
    let changed = fixture.plan(&mut store);
    assert_ne!(changed.identity(), plan.identity());
    assert_ne!(
        changed.receipt().source_cohort_sha256,
        plan.receipt().source_cohort_sha256
    );
    let mut preparation = fixture.prepare(changed.clone());
    let ready = preparation
        .wait()
        .unwrap()
        .publish_for(changed.terrain())
        .unwrap();
    assert!(
        ready
            .textures
            .iter()
            .all(|t| t.cache.as_ref().unwrap().reused)
    );
}

#[test]
fn ready_is_revoked_by_identical_replacement_cancel_and_owner_drop() {
    let fixture = Fixture::new(&default_source(0));
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let before = serde_json::to_vec(plan.terrain()).unwrap();
    let mut preparation = fixture.prepare(plan.clone());
    let ready = preparation.wait().unwrap();
    preparation.replace(plan.clone()).unwrap();
    assert!(matches!(
        ready.publish_for(plan.terrain()),
        Err(JobError::Stale)
    ));
    let ready = preparation.wait().unwrap();
    preparation.cancel();
    assert!(matches!(
        ready.publish_for(plan.terrain()),
        Err(JobError::Cancelled)
    ));
    preparation.retry().unwrap();
    let ready = preparation.wait().unwrap();
    drop(preparation);
    assert!(matches!(
        ready.publish_for(plan.terrain()),
        Err(JobError::Closed)
    ));
    assert_eq!(serde_json::to_vec(plan.terrain()).unwrap(), before);
}

#[test]
fn partial_cache_failure_exposes_no_ready_then_exact_retry_succeeds() {
    let fixture = Fixture::new(&default_source(0));
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let mut preparation = fixture.prepare(plan.clone());
    let receipt = preparation
        .wait()
        .unwrap()
        .publish_for(plan.terrain())
        .unwrap();
    let damaged = receipt.textures[1].cache.as_ref().unwrap();
    fs::write(
        fixture.cache.path().join(format!("{}.blob", damaged.key)),
        [1, 2],
    )
    .unwrap();
    let mut manifest = damaged.manifest.clone();
    manifest.bytes = 2;
    manifest.sha256 = format!("{:x}", Sha256::digest([1, 2]));
    fs::write(
        fixture.cache.path().join(format!("{}.json", damaged.key)),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    preparation.retry().unwrap();
    assert!(preparation.wait().is_err());
    assert!(preparation.take_ready().is_err());
    drained(&preparation);
    fs::remove_file(fixture.cache.path().join(format!("{}.json", damaged.key))).unwrap();
    fs::remove_file(fixture.cache.path().join(format!("{}.blob", damaged.key))).unwrap();
    preparation.retry().unwrap();
    let repaired = preparation
        .wait()
        .unwrap()
        .publish_for(plan.terrain())
        .unwrap();
    assert_eq!(repaired.generation, 3);
    assert_eq!(repaired.textures.len(), 2);
    assert!(repaired.textures[0].cache.as_ref().unwrap().reused);
    assert!(!repaired.textures[1].cache.as_ref().unwrap().reused);
    drained(&preparation);
}

#[test]
fn changed_physical_layer_or_root_sink_cannot_publish() {
    let fixture = Fixture::new(&default_source(0));
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let mut preparation = fixture.prepare(plan.clone());
    for layer_edit in [false, true] {
        let mut report = terrain::inspect_cell_key(&mut store, &key(), None).unwrap();
        if layer_edit {
            let Some(Fields::Land(land)) = &mut report.landscapes[0].fields else {
                panic!()
            };
            land.layers[1].decoded_offset += 6;
        } else {
            report.cell.key.local_id += 1;
        }
        let before = serde_json::to_vec(&report).unwrap();
        assert!(preparation.wait().unwrap().publish_for(&report).is_err());
        assert_eq!(serde_json::to_vec(&report).unwrap(), before);
        preparation.retry().unwrap();
    }
}

#[test]
fn unresolved_defaults_missing_deleted_wrong_kind_absent_and_collisions_stay_explicit() {
    for (texture, tnam, flags, paths, status) in [
        (
            0,
            Some(0x500),
            0,
            vec![(b"TX00", b"land/stone.dds".as_slice())],
            "null-default-unapplied",
        ),
        (
            0x999,
            Some(0x500),
            0,
            vec![(b"TX00", b"land/stone.dds".as_slice())],
            "unresolved-LTEX",
        ),
        (
            0x400,
            None,
            0,
            vec![(b"TX00", b"land/stone.dds".as_slice())],
            "absent-TNAM",
        ),
        (
            0x400,
            Some(0x400),
            0,
            vec![(b"TX00", b"land/stone.dds".as_slice())],
            "unresolved-TXST",
        ),
        (
            0x400,
            Some(0x500),
            plugin::DELETED,
            vec![(b"TX00", b"land/stone.dds".as_slice())],
            "unresolved-TXST",
        ),
        (
            0x400,
            Some(0x500),
            0,
            vec![(b"TX00", b"".as_slice())],
            "missing-diffuse-path",
        ),
    ] {
        let fixture = Fixture::new(&source(texture, tnam, flags, &paths, false, 0));
        let mut store = fixture.store();
        let plan = fixture.plan(&mut store);
        assert_eq!(plan.receipt().texture_sources.bindings[0].status, status);
        let mut preparation = fixture.prepare(plan.clone());
        let receipt = preparation
            .wait()
            .unwrap()
            .publish_for(plan.terrain())
            .unwrap();
        assert!(!receipt.all_authored_texture_sources_resolved && receipt.textures.is_empty());
    }
    let fixture = Fixture::new(&source(
        0x400,
        Some(0x500),
        0,
        &[(b"TX00", b"land/missing.dds")],
        false,
        0,
    ));
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    assert_eq!(plan.receipt().texture_sources.assets[0].candidates.len(), 0);
    assert!(plan.receipt().texture_sources.assets[0].error.is_some());
    let fixture = Fixture::new(&default_source(0));
    fs::write(
        fixture.source.path().join("Data/duplicate.bsa"),
        archive(&[(b"stone.dds", &[1, 2, 3, 4])]),
    )
    .unwrap();
    let mut mounts = fixture.mounts();
    NvArchive::open(&fixture.source.path().join("Data/duplicate.bsa"))
        .unwrap()
        .census(&mut mounts)
        .unwrap();
    let mut store = fixture.store();
    let plan = TextureSourcePlan::load(&mut store, &key(), &mounts, Default::default()).unwrap();
    assert_eq!(plan.receipt().requests.len(), 1);
    assert_eq!(plan.receipt().texture_sources.assets[1].candidates.len(), 2);
    let mut preparation = fixture.prepare(plan.clone());
    let receipt = preparation
        .wait()
        .unwrap()
        .publish_for(plan.terrain())
        .unwrap();
    assert!(!receipt.all_authored_texture_sources_resolved && receipt.textures.len() == 1);
}

#[test]
fn all_source_ceilings_and_single_land_refusal_are_retryable() {
    let fixture = Fixture::new(&default_source(0));
    let mut store = fixture.store();
    let mounts = fixture.mounts();
    let mut variants = Vec::new();
    macro_rules! low {
        ($field:ident,$value:expr) => {
            variants.push(Limits {
                $field: $value,
                ..Default::default()
            });
        };
    }
    low!(winners, 0);
    low!(sources, 0);
    low!(worlds, 0);
    low!(records, 0);
    low!(layers, 0);
    low!(assets, 0);
    low!(candidates, 0);
    low!(archives, 0);
    low!(record_bytes, 1);
    low!(plugin_bytes, 1);
    low!(field_sites, 0);
    low!(path_bytes, 1);
    low!(texture_bytes, 8);
    low!(metadata_bytes, 1);
    low!(layers, 4097);
    for limits in variants {
        assert!(TextureSourcePlan::load(&mut store, &key(), &mounts, limits).is_err());
    }
    let plan = fixture.plan(&mut store);
    let limits = resource_jobs::Limits {
        workers: 1,
        outstanding: 1,
        decoded_bytes: 5,
    };
    let mut preparation =
        TexturePreparation::new(plan.clone(), fixture.source.path(), None, limits).unwrap();
    assert_eq!(
        preparation
            .wait()
            .unwrap()
            .publish_for(plan.terrain())
            .unwrap()
            .textures
            .len(),
        2
    );
    drained(&preparation);
    assert!(matches!(
        TexturePreparation::new(
            plan,
            fixture.source.path(),
            None,
            resource_jobs::Limits {
                decoded_bytes: 4,
                ..limits
            }
        ),
        Err(JobError::ByteBudget)
    ));
    let fixture = Fixture::new(&source(
        0x400,
        Some(0x500),
        0,
        &[(b"TX00", b"land/stone.dds")],
        true,
        0,
    ));
    let mut store = fixture.store();
    let error = TextureSourcePlan::load(&mut store, &key(), &fixture.mounts(), Default::default())
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("exactly one winning present LAND")
    );
}

#[cfg(windows)]
#[test]
fn archive_pins_release_only_after_plan_and_ready_owners_drop() {
    let fixture = Fixture::new(&default_source(0));
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let path = fixture.source.path().join("Data/authored.bsa");
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    let mut preparation = fixture.prepare(plan.clone());
    let ready = preparation.wait().unwrap();
    drop(plan);
    drop(preparation);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    drop(ready);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_ok());
}

#[test]
fn forensic_texture_body_cannot_construct_a_ready_plan_or_cache() {
    let original = default_source(0);
    let start = original
        .windows(4)
        .rposition(|word| word == b"TXST")
        .unwrap();
    let body = &original[start + 24..];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    let mut encoded = encoder.finish().unwrap();
    *encoded.last_mut().unwrap() ^= 1;
    let compressed = record(
        b"TXST",
        0x500,
        plugin::COMPRESSED,
        &[&(body.len() as u32).to_le_bytes(), encoded.as_slice()].concat(),
    );
    let fixture = Fixture::new(&[&original[..start], compressed.as_slice()].concat());
    let mut store = RecordStore::open_nv(
        &fixture.source.path().join("Data"),
        &["FalloutNV.esm".into()],
        plugin::Limits {
            inspect_checksum_mismatches: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(store.integrity_failures(), 1);
    let error = TextureSourcePlan::load(&mut store, &key(), &fixture.mounts(), Default::default())
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("refuses tainted source cohort"),
        "{error}"
    );
    assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 0);
}
