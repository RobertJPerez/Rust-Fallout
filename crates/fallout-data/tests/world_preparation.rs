//! Authored physical source records and compressed BSA members exercise the
//! aggregate cell gate, independently of retail model-selection assumptions.
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::{self, JobError},
    store::RecordStore,
    vfs::MountIndex,
    world::{
        self, CellReport,
        preparation::{CellModelPlan, CellPreparation, Limits},
    },
};
use flate2::{Compression, write::ZlibEncoder};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    time::{Duration, Instant},
};

fn sub(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(tag: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        body,
    ]
    .concat()
}
fn group(kind: i32, body: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(body.len() as u32 + 24).to_le_bytes(),
        &0x200u32.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn source(models: &[Option<&[u8]>], revision: u8) -> Vec<u8> {
    let mut bytes = record(
        b"TES4",
        0,
        &sub(
            b"HEDR",
            &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    );
    let mut members = Vec::new();
    for (index, model) in models.iter().enumerate() {
        let base = 0x400 + index as u32;
        let mut body = sub(b"ZZZZ", &[revision]);
        if let Some(model) = model {
            body.extend(sub(b"MODL", &[*model, &[0]].concat()));
        }
        bytes.extend(record(b"STAT", base, &body));
        members.extend(record(
            b"REFR",
            0x300 + index as u32,
            &[sub(b"NAME", &base.to_le_bytes()), sub(b"DATA", &[0; 24])].concat(),
        ));
    }
    bytes.extend(record(
        b"CELL",
        0x200,
        &[sub(b"EDID", b"PreparedClinic\0"), sub(b"DATA", &[1])].concat(),
    ));
    bytes.extend(group(6, &group(9, &members)));
    bytes
}
fn nif() -> Vec<u8> {
    let mut bytes = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    bytes.extend(0x14020007u32.to_le_bytes());
    bytes.push(1);
    for value in [11u32, 0, 34] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 0, 0]);
    bytes.extend(0u16.to_le_bytes());
    for _ in 0..4 {
        bytes.extend(0u32.to_le_bytes());
    }
    bytes
}
fn archive(payloads: &[(&[u8], &[u8])]) -> Vec<u8> {
    let names: Vec<u8> = payloads
        .iter()
        .flat_map(|(name, _)| [*name, &[0]].concat())
        .collect();
    let data_offset = 60 + 16 * payloads.len() + names.len();
    let mut bytes = vec![0; data_offset];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104),
        (8, 36),
        (12, 7),
        (16, 1),
        (20, payloads.len() as u32),
        (24, 7),
        (28, names.len() as u32),
        (44, payloads.len() as u32),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[52] = 7;
    bytes[53..60].copy_from_slice(b"meshes\0");
    bytes[60 + 16 * payloads.len()..data_offset].copy_from_slice(&names);
    for (index, (_, payload)) in payloads.iter().enumerate() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        let stored = [
            &(payload.len() as u32).to_le_bytes(),
            encoder.finish().unwrap().as_slice(),
        ]
        .concat();
        let at = 60 + index * 16;
        bytes[at..at + 8].copy_from_slice(&(index as u64 + 1).to_le_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&(stored.len() as u32).to_le_bytes());
        let offset = bytes.len() as u32;
        bytes[at + 12..at + 16].copy_from_slice(&offset.to_le_bytes());
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
    fn new(models: &[Option<&[u8]>], payloads: &[(&[u8], &[u8])]) -> Self {
        let source_dir = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        fs::create_dir(source_dir.path().join("Data")).unwrap();
        fs::write(
            source_dir.path().join("Data/FalloutNV.esm"),
            source(models, 1),
        )
        .unwrap();
        fs::write(
            source_dir.path().join("Data/authored.bsa"),
            archive(payloads),
        )
        .unwrap();
        Self {
            source: source_dir,
            cache,
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
    fn plan(&self, store: &mut RecordStore) -> CellModelPlan {
        CellModelPlan::load(store, &key(), &self.mounts(), Limits::default()).unwrap()
    }
    fn report(&self, store: &mut RecordStore) -> CellReport {
        world::inspect_cell(store, b"PreparedClinic", &self.mounts()).unwrap()
    }
    fn prepare(&self, plan: CellModelPlan) -> CellPreparation {
        CellPreparation::new(
            plan,
            self.source.path(),
            Some(self.cache.path()),
            resource_jobs::Limits::default(),
        )
        .unwrap()
    }
}
fn report_bytes(report: &CellReport) -> Vec<u8> {
    serde_json::to_vec(report).unwrap()
}
fn drained(preparation: &CellPreparation) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while preparation.usage().outstanding != 0 {
        assert!(Instant::now() < deadline, "reservations did not drain");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(preparation.usage().decoded_bytes, 0);
}

#[test]
fn exact_source_cold_warm_and_changed_plugin_cohort_reuse_only_archive_bytes() {
    let payload = nif();
    let fixture = Fixture::new(&[Some(b"a.nif")], &[(b"a.nif", &payload)]);
    let mut store = fixture.store();
    let first = fixture.plan(&mut store);
    let identity = first.identity().to_owned();
    for reused in [false, true] {
        let mut report = fixture.report(&mut store);
        let mut preparation = fixture.prepare(first.clone());
        let receipt = preparation
            .wait()
            .unwrap()
            .publish_into(&mut report)
            .unwrap();
        assert_eq!(receipt.plan().identity, identity);
        assert_eq!(receipt.generation, 1);
        assert!(receipt.all_requested_model_sources_ready);
        assert!(!receipt.runtime_ready && !report.runtime_ready);
        let probe = &report.model_probes[0];
        assert_eq!(
            probe.sha256.as_deref(),
            Some(format!("{:x}", Sha256::digest(&payload)).as_str())
        );
        assert_eq!(probe.decoded_bytes, Some(payload.len()));
        assert_eq!(probe.cache.as_ref().unwrap().reused, reused);
        drained(&preparation);
    }
    drop(store);
    fs::write(
        fixture.source.path().join("Data/FalloutNV.esm"),
        source(&[Some(b"a.nif")], 2),
    )
    .unwrap();
    let mut store = fixture.store();
    let changed = fixture.plan(&mut store);
    assert_ne!(changed.identity(), identity);
    assert_ne!(
        first.receipt().source_cohort_sha256,
        changed.receipt().source_cohort_sha256
    );
    let mut report = fixture.report(&mut store);
    let mut preparation = fixture.prepare(changed);
    preparation
        .wait()
        .unwrap()
        .publish_into(&mut report)
        .unwrap();
    assert!(report.model_probes[0].cache.as_ref().unwrap().reused);
}

#[test]
fn replacement_cancel_and_owner_drop_revoke_unconsumed_ready_without_sink_mutation() {
    let payload = nif();
    let fixture = Fixture::new(&[Some(b"a.nif")], &[(b"a.nif", &payload)]);
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let mut report = fixture.report(&mut store);
    let original = report_bytes(&report);
    let mut preparation = fixture.prepare(plan.clone());
    let stale = preparation.wait().unwrap();
    preparation.replace(plan).unwrap();
    assert_eq!(preparation.generation(), 2);
    assert!(matches!(
        stale.publish_into(&mut report),
        Err(JobError::Stale)
    ));
    assert_eq!(report_bytes(&report), original);
    let cancelled = preparation.wait().unwrap();
    preparation.cancel();
    assert!(matches!(
        cancelled.publish_into(&mut report),
        Err(JobError::Cancelled)
    ));
    assert_eq!(report_bytes(&report), original);
    preparation.retry().unwrap();
    let closed = preparation.wait().unwrap();
    drop(preparation);
    assert!(matches!(
        closed.publish_into(&mut report),
        Err(JobError::Closed)
    ));
    assert_eq!(report_bytes(&report), original);
}

#[test]
fn aggregate_failure_rejects_partial_nif_and_partial_cache_then_retry_is_fresh() {
    let payload = nif();
    let fixture = Fixture::new(
        &[Some(b"a.nif"), Some(b"b.nif")],
        &[(b"a.nif", &payload), (b"b.nif", &payload)],
    );
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let mut report = fixture.report(&mut store);
    let mut preparation = fixture.prepare(plan.clone());
    preparation
        .wait()
        .unwrap()
        .publish_into(&mut report)
        .unwrap();
    let original = report_bytes(&report);
    let cache = report.model_probes[1].cache.as_ref().unwrap();
    let cache_key = cache.key.clone();
    let mut manifest = cache.manifest.clone();
    // This has the source-declared size and a matching cache checksum, but its
    // container is not a valid NIF. The other requested model is still valid.
    let bad = vec![0x80; payload.len()];
    manifest.sha256 = format!("{:x}", Sha256::digest(&bad));
    fs::write(fixture.cache.path().join(format!("{cache_key}.blob")), &bad).unwrap();
    fs::write(
        fixture.cache.path().join(format!("{cache_key}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    preparation.retry().unwrap();
    let error = preparation
        .wait()
        .err()
        .expect("bad NIF must fail aggregate");
    assert!(error.to_string().contains("NIF"), "{error}");
    assert!(preparation.take_ready().is_err());
    drained(&preparation);
    assert_eq!(report_bytes(&report), original);
    // A self-consistent shortened blob/marker must still fail against source.
    let partial = &payload[..8];
    manifest.bytes = partial.len() as u64;
    manifest.sha256 = format!("{:x}", Sha256::digest(partial));
    fs::write(
        fixture.cache.path().join(format!("{cache_key}.blob")),
        partial,
    )
    .unwrap();
    fs::write(
        fixture.cache.path().join(format!("{cache_key}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    preparation.retry().unwrap();
    let error = preparation.wait().err().expect("partial cache must fail");
    assert!(
        error
            .to_string()
            .contains("decoded length differs from source"),
        "{error}"
    );
    drained(&preparation);
    assert_eq!(report_bytes(&report), original);
    manifest.bytes = payload.len() as u64;
    manifest.sha256 = format!("{:x}", Sha256::digest(&payload));
    fs::write(
        fixture.cache.path().join(format!("{cache_key}.blob")),
        &payload,
    )
    .unwrap();
    fs::write(
        fixture.cache.path().join(format!("{cache_key}.json")),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    preparation.retry().unwrap();
    assert_eq!(preparation.generation(), 4);
    preparation
        .wait()
        .unwrap()
        .publish_into(&mut report)
        .unwrap();
    assert_eq!(report.model_probes.len(), 2);
    assert!(
        report
            .model_probes
            .iter()
            .all(|probe| probe.error.is_none())
    );
    drained(&preparation);
}

#[test]
fn wrong_root_or_physical_modl_binding_rejects_publication() {
    let payload = nif();
    let fixture = Fixture::new(&[Some(b"a.nif")], &[(b"a.nif", &payload)]);
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let mut preparation = fixture.prepare(plan);
    let mut report = fixture.report(&mut store);
    report.key.local_id += 1;
    let original = report_bytes(&report);
    assert!(
        preparation
            .wait()
            .unwrap()
            .publish_into(&mut report)
            .err()
            .unwrap()
            .to_string()
            .contains("root/source binding")
    );
    assert_eq!(report_bytes(&report), original);
    preparation.retry().unwrap();
    let mut report = fixture.report(&mut store);
    report.models[0]
        .model_field
        .as_mut()
        .unwrap()
        .decoded_offset += 6;
    let original = report_bytes(&report);
    assert!(
        preparation
            .wait()
            .unwrap()
            .publish_into(&mut report)
            .err()
            .unwrap()
            .to_string()
            .contains("MODL/archive binding")
    );
    assert_eq!(report_bytes(&report), original);
}

#[test]
fn deferred_sources_stay_coverage_and_queue_byte_limits_do_not_stall() {
    let payload = nif();
    let fixture = Fixture::new(
        &[Some(b"a.nif"), Some(b"b.nif"), Some(b"missing.nif"), None],
        &[(b"a.nif", &payload), (b"b.nif", &payload)],
    );
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    assert_eq!(plan.receipt().coverage.len(), 4);
    assert_eq!(plan.receipt().requests.len(), 2);
    assert!(plan.receipt().coverage[2].status.contains("missing"));
    assert!(plan.receipt().coverage[3].status.contains("no-modl"));
    let limits = resource_jobs::Limits {
        workers: 1,
        outstanding: 1,
        decoded_bytes: payload.len(),
    };
    let mut preparation =
        CellPreparation::new(plan.clone(), fixture.source.path(), None, limits).unwrap();
    let mut report = fixture.report(&mut store);
    preparation
        .wait()
        .unwrap()
        .publish_into(&mut report)
        .unwrap();
    assert_eq!(report.model_probes.len(), 2);
    assert!(!report.runtime_ready);
    drained(&preparation);
    assert!(matches!(
        CellPreparation::new(
            plan,
            fixture.source.path(),
            None,
            resource_jobs::Limits {
                decoded_bytes: payload.len() - 1,
                ..limits
            }
        ),
        Err(JobError::ByteBudget)
    ));
    // A second physical source is an ambiguity, never an inferred precedence.
    fs::write(
        fixture.source.path().join("Data/duplicate.bsa"),
        archive(&[(b"a.nif", &payload)]),
    )
    .unwrap();
    let mut mounts = fixture.mounts();
    NvArchive::open(&fixture.source.path().join("Data/duplicate.bsa"))
        .unwrap()
        .census(&mut mounts)
        .unwrap();
    let ambiguous = CellModelPlan::load(&mut store, &key(), &mounts, Limits::default()).unwrap();
    assert_eq!(ambiguous.receipt().requests.len(), 1);
    assert!(ambiguous.receipt().coverage[0].status.contains("ambiguous"));
}

#[test]
fn source_and_metadata_limits_fail_before_batch_and_allow_exact_retry() {
    let payload = nif();
    let fixture = Fixture::new(&[Some(b"a.nif")], &[(b"a.nif", &payload)]);
    let mut store = fixture.store();
    let mounts = fixture.mounts();
    for (limits, reason) in [
        (
            Limits {
                max_bases: 0,
                ..Limits::default()
            },
            "bases budget",
        ),
        (
            Limits {
                max_requests: 0,
                ..Limits::default()
            },
            "requests budget",
        ),
        (
            Limits {
                max_candidates: 0,
                ..Limits::default()
            },
            "candidates budget",
        ),
        (
            Limits {
                max_archives: 0,
                ..Limits::default()
            },
            "archives budget",
        ),
        (
            Limits {
                max_record_bytes: 1,
                ..Limits::default()
            },
            "budget",
        ),
        (
            Limits {
                max_record_decoded_bytes: 1,
                ..Limits::default()
            },
            "budget",
        ),
        (
            Limits {
                max_field_sites: 0,
                ..Limits::default()
            },
            "field sites",
        ),
        (
            Limits {
                max_path_bytes: 3,
                ..Limits::default()
            },
            "path bytes budget",
        ),
        (
            Limits {
                max_model_decoded_bytes: payload.len() - 1,
                ..Limits::default()
            },
            "model decoded bytes budget",
        ),
        (
            Limits {
                max_metadata_bytes: 1,
                ..Limits::default()
            },
            "metadata",
        ),
        (
            Limits {
                max_probe_metadata_bytes: 1,
                ..Limits::default()
            },
            "probe metadata bytes budget",
        ),
        (
            Limits {
                max_bases: 1025,
                ..Limits::default()
            },
            "ceiling exceeded",
        ),
        (
            Limits {
                dependencies: world::dependencies::Limits {
                    max_record_bytes: 4 * 1024 * 1024 + 1,
                    ..Default::default()
                },
                ..Limits::default()
            },
            "ceiling exceeded",
        ),
    ] {
        let error = CellModelPlan::load(&mut store, &key(), &mounts, limits)
            .err()
            .expect("admission must fail");
        assert!(
            error.to_string().contains(reason),
            "{error}; expected {reason}"
        );
    }
    let plan = fixture.plan(&mut store);
    let mut preparation = fixture.prepare(plan);
    preparation.poll().unwrap();
    preparation.cancel();
    assert!(matches!(preparation.poll(), Err(JobError::Cancelled)));
    drained(&preparation);
    preparation.retry().unwrap();
    let mut report = fixture.report(&mut store);
    preparation
        .wait()
        .unwrap()
        .publish_into(&mut report)
        .unwrap();
    drained(&preparation);
}

#[cfg(windows)]
#[test]
fn sealed_plan_and_ready_pin_archive_then_release_on_owner_drop() {
    let payload = nif();
    let fixture = Fixture::new(&[Some(b"a.nif")], &[(b"a.nif", &payload)]);
    let mut store = fixture.store();
    let plan = fixture.plan(&mut store);
    let path = fixture.source.path().join("Data/authored.bsa");
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    let mut preparation = fixture.prepare(plan);
    let ready = preparation.wait().unwrap();
    drop(preparation);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    drop(ready);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_ok());
}

#[test]
fn forensic_recovered_base_is_inspectable_but_cannot_construct_a_ready_plan() {
    let payload = nif();
    let fixture = Fixture::new(&[Some(b"a.nif")], &[(b"a.nif", &payload)]);
    let original = source(&[Some(b"a.nif")], 1);
    let base = 24 + u32::from_le_bytes(original[4..8].try_into().unwrap()) as usize;
    let end =
        base + 24 + u32::from_le_bytes(original[base + 4..base + 8].try_into().unwrap()) as usize;
    let body = &original[base + 24..end];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    let mut encoded = encoder.finish().unwrap();
    *encoded.last_mut().unwrap() ^= 1; // Adler trailer only: body is recoverable.
    let mut compressed = record(
        b"STAT",
        0x400,
        &[&(body.len() as u32).to_le_bytes(), encoded.as_slice()].concat(),
    );
    compressed[8..12].copy_from_slice(&plugin::COMPRESSED.to_le_bytes());
    fs::write(
        fixture.source.path().join("Data/FalloutNV.esm"),
        [&original[..base], compressed.as_slice(), &original[end..]].concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv(
        &fixture.source.path().join("Data"),
        &["FalloutNV.esm".into()],
        plugin::Limits {
            inspect_checksum_mismatches: true,
            ..Default::default()
        },
    )
    .unwrap();
    let report = fixture.report(&mut store);
    assert_eq!(report.integrity_failures, 1);
    assert_eq!(
        report.models[0].model_field.as_ref().unwrap().value,
        b"a.nif"
    );
    assert!(!report.runtime_ready);
    let error = CellModelPlan::load(&mut store, &key(), &fixture.mounts(), Limits::default())
        .err()
        .expect("recovered body must not prepare");
    assert!(
        error.to_string().contains("refuses tainted base body"),
        "{error}"
    );
    assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 0);
}
