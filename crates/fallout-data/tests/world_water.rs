//! Literal source bytes and a tiny compressed v104 member exercise the existing
//! importer/job/cache path without claiming a rendered water plane.
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    resource_jobs::{self, Generation, ResourceJobs},
    store::RecordStore,
    vfs::{AssetSource, MountIndex},
    world::water::{CellWaterSources, Limits},
};
use sha2::{Digest, Sha256};
use std::{fs, io::Write};
fn field(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(kind: &[u8; 4], form: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header(masters: &[&[u8]]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[*master, &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
fn key(raw: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: raw,
    }
}
fn cell(word: u32, path: &[u8]) -> Vec<u8> {
    [
        field(b"DATA", &[2]),
        field(b"XCLW", &word.to_le_bytes()),
        field(b"XCWT", &0x200_u32.to_le_bytes()),
        field(b"XNAM", path),
    ]
    .concat()
}
fn watr() -> Vec<u8> {
    record(
        b"WATR",
        0x200,
        0,
        &[field(b"EDID", b"w\0"), field(b"DATA", &[3, 0])].concat(),
    )
}
const NOISE: &[u8] = &[0x11, 0x22, 0x33, 0x44, 0x55];
fn archive() -> Vec<u8> {
    let mut bytes = vec![0; 88];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104_u32),
        (8, 36),
        (12, 7),
        (16, 1),
        (20, 1),
        (24, 9),
        (28, 10),
        (44, 1),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[52] = 9;
    bytes[53..62].copy_from_slice(b"textures\0");
    bytes[62..70].copy_from_slice(&1_u64.to_le_bytes());
    bytes[78..88].copy_from_slice(b"noise.dds\0");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(NOISE).unwrap();
    let stored = [
        (NOISE.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat();
    bytes[70..74].copy_from_slice(&(stored.len() as u32).to_le_bytes());
    bytes[74..78].copy_from_slice(&88_u32.to_le_bytes());
    bytes.extend(stored);
    bytes
}
struct Fixture {
    root: tempfile::TempDir,
    cache: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(cell: &[u8], watr: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        fs::write(
            root.path().join("Data/Base.esm"),
            [header(&[]), record(b"CELL", 0x100, 0, cell), watr.to_vec()].concat(),
        )
        .unwrap();
        fs::write(root.path().join("Data/noise.bsa"), archive()).unwrap();
        Self {
            root,
            cache,
            names: vec!["Base.esm".into()],
        }
    }
    fn try_store(&self, forensic: bool) -> fallout_data::Result<RecordStore> {
        let open = if forensic {
            RecordStore::open_nv
        } else {
            RecordStore::open_nv_headers
        };
        open(
            &self.root.path().join("Data"),
            &self.names,
            plugin::Limits {
                inspect_checksum_mismatches: forensic,
                ..Default::default()
            },
        )
    }
    fn store(&self) -> RecordStore {
        self.try_store(false).unwrap()
    }
    fn mounts(&self) -> MountIndex {
        let mut mounts = MountIndex::default();
        NvArchive::open(&self.root.path().join("Data/noise.bsa"))
            .unwrap()
            .census(&mut mounts)
            .unwrap();
        mounts
    }
    fn load(
        &self,
        store: &mut RecordStore,
        limits: Limits,
    ) -> fallout_data::Result<CellWaterSources> {
        CellWaterSources::load(store, &key(0x100), &self.mounts(), limits)
    }
    fn patch(&mut self, masters: &[&[u8]], body: &[u8]) {
        fs::write(
            self.root.path().join("Data/Patch.esp"),
            [header(masters), body.to_vec()].concat(),
        )
        .unwrap();
        self.names.push("Patch.esp".into());
    }
}
#[test]
fn every_literal_height_word_and_exact_source_span_survives_without_float_or_defaults() {
    for word in [0x80000000, 1, 0x7f7fffff, 0x7fc12345] {
        let fixture = Fixture::new(&cell(word, b"noise.dds\0"), &watr());
        let mut store = fixture.store();
        let sources = fixture.load(&mut store, Default::default()).unwrap();
        let r = sources.receipt();
        let xclw = r.xclw.as_ref().unwrap();
        assert_eq!(xclw.value, word);
        assert_eq!(
            xclw.framing,
            [b"XCLW\x04\0".as_slice(), &word.to_le_bytes()].concat()
        );
        assert_eq!(xclw.site.decoded_header_offset, 7);
        assert_eq!(xclw.site.span.decoded_offset, 13);
        assert_eq!(xclw.site.span.bytes, 4);
        assert_eq!(xclw.physical_framing_offset, Some(73));
        assert_eq!(r.xcwt.as_ref().unwrap().site.decoded_header_offset, 17);
        assert_eq!(r.xcwt.as_ref().unwrap().physical_framing_offset, Some(83));
        let xnam = r.xnam.as_ref().unwrap();
        assert_eq!(xnam.value, b"noise.dds");
        assert_eq!(xnam.framing, b"XNAM\x0a\0noise.dds\0");
        assert_eq!(xnam.site.decoded_header_offset, 27);
        assert_eq!(xnam.site.span.decoded_offset, 33);
        assert_eq!(xnam.physical_framing_offset, Some(93));
        assert_eq!(xnam.site.span.bytes, 10);
        assert_eq!(r.cell.header.offset, 42);
        assert_eq!(r.cell_flags.value, 2);
        let ty = r.water_type.as_ref().unwrap();
        assert_eq!(ty.target.key, Some(key(0x200)));
        assert_eq!(ty.target.status, "resolved");
        assert_eq!(ty.source.as_ref().unwrap().header.offset, 109);
        assert_eq!(
            ty.source.as_ref().unwrap().decoded_sha256.as_deref(),
            Some(
                format!(
                    "{:x}",
                    Sha256::digest([field(b"EDID", b"w\0"), field(b"DATA", &[3, 0])].concat())
                )
                .as_str()
            )
        );
        assert!(!ty.typed_parameters_included);
        assert!(!r.runtime_ready);
        assert_eq!(
            r.noise
                .as_ref()
                .unwrap()
                .request
                .as_ref()
                .unwrap()
                .decoded_bytes,
            5
        );
        assert_eq!(
            r.noise.as_ref().unwrap().path.as_ref().unwrap().bytes(),
            b"textures/noise.dds"
        );
        let serialized = serde_json::to_value(&sources).unwrap();
        assert_eq!(serialized["xclw"]["value"], word);
    }
}
#[test]
fn absent_and_empty_fields_and_unavailable_watr_and_noise_remain_explicit() {
    for (raw, kind, flags, status) in [
        (0_u32, b"WATR", 0, "null"),
        (0x999, b"WATR", 0, "missing"),
        (0x200, b"WATR", plugin::DELETED, "deleted"),
        (0x200, b"STAT", 0, "wrong-record-kind"),
    ] {
        let body = [
            field(b"DATA", &[2]),
            field(b"XCWT", &raw.to_le_bytes()),
            field(b"XNAM", b"\0"),
        ]
        .concat();
        let fixture = Fixture::new(&body, &record(kind, 0x200, flags, &[]));
        let mut store = fixture.store();
        let sources = fixture.load(&mut store, Default::default()).unwrap();
        let r = sources.receipt();
        assert!(r.xclw.is_none());
        assert_eq!(r.water_type.as_ref().unwrap().target.status, status);
        if ["deleted", "wrong-record-kind"].contains(&status) {
            assert!(
                r.water_type
                    .as_ref()
                    .unwrap()
                    .source
                    .as_ref()
                    .unwrap()
                    .decoded_sha256
                    .is_none()
            );
        }
        assert_eq!(r.noise.as_ref().unwrap().status, "empty-declaration");
        assert_eq!(r.usage.jobs, 0);
        assert!(r.noise.as_ref().unwrap().request.is_none());
    }
    let fixture = Fixture::new(&field(b"DATA", &[2]), &[]);
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let r = sources.receipt();
    assert!(
        r.xclw.is_none()
            && r.xcwt.is_none()
            && r.xnam.is_none()
            && r.water_type.is_none()
            && r.noise.is_none()
    );
    let fixture = Fixture::new(&cell(0, b"missing.dds\0"), &watr());
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let noise = sources.receipt().noise.as_ref().unwrap();
    assert_eq!(noise.status, "missing");
    assert!(noise.archive.is_none() && noise.request.is_none());
}
#[test]
fn winning_cell_two_master_remap_and_watr_override_use_only_their_own_sources() {
    let mut fixture = Fixture::new(&cell(0, b"noise.dds\0"), &watr());
    fs::write(fixture.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    fixture.names.push("Other.esm".into());
    let mut override_body = cell(0x80000000, b"noise.dds\0");
    override_body[23..27].copy_from_slice(&0x01000200_u32.to_le_bytes());
    fixture.patch(
        &[b"Other.esm", b"Base.esm"],
        &[
            record(b"CELL", 0x01000100, 0, &override_body),
            record(b"WATR", 0x01000200, 0, &field(b"DATA", &[99, 0])),
        ]
        .concat(),
    );
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let r = sources.receipt();
    assert_eq!(r.cell.source_ordinal, 2);
    assert_eq!(r.cell.header.offset, 101);
    assert_eq!(r.xcwt.as_ref().unwrap().value, 0x01000200);
    assert_eq!(r.water_type.as_ref().unwrap().target.key, Some(key(0x200)));
    let source = r.water_type.as_ref().unwrap().source.as_ref().unwrap();
    assert_eq!(source.source_ordinal, 2);
    assert_eq!(source.header.offset, 168);
    assert_eq!(
        source.decoded_sha256.as_deref(),
        Some(format!("{:x}", Sha256::digest(field(b"DATA", &[99, 0]))).as_str())
    );
    assert_eq!(r.xclw.as_ref().unwrap().value, 0x80000000);
}
#[test]
fn unsafe_ambiguous_duplicate_bad_width_and_unterminated_declarations_refuse_whole_request() {
    for path in [
        b"/noise.dds\0".as_slice(),
        b"..\\noise.dds\0",
        b"C:\\noise.dds\0",
        b"textures//noise.dds\0",
        b"noise.dds",
        b"noise\0tail\0",
    ] {
        let fixture = Fixture::new(&cell(0, path), &watr());
        let mut store = fixture.store();
        assert!(fixture.load(&mut store, Default::default()).is_err());
    }
    for kind in [b"XCLW", b"XCWT", b"XNAM"] {
        let fixture = Fixture::new(
            &[
                cell(0, b"noise.dds\0"),
                field(kind, if kind == b"XNAM" { b"\0" } else { &[0; 4] }),
            ]
            .concat(),
            &watr(),
        );
        assert!(
            fixture
                .load(&mut fixture.store(), Default::default())
                .is_err()
        );
    }
    for kind in [b"XCLW", b"XCWT"] {
        for len in [0, 1, 2, 3, 5, 8] {
            let fixture = Fixture::new(
                &[field(b"DATA", &[2]), field(kind, &vec![0; len])].concat(),
                &[],
            );
            assert!(
                fixture
                    .load(&mut fixture.store(), Default::default())
                    .is_err()
            );
        }
    }
    let fixture = Fixture::new(&cell(0, b"noise.dds\0"), &watr());
    let mut mounts = fixture.mounts();
    let selected = mounts.candidates(b"textures/noise.dds").unwrap()[0].clone();
    mounts
        .insert(AssetSource {
            container: "other-unopened.bsa".into(),
            ..selected
        })
        .unwrap();
    assert!(
        CellWaterSources::load(
            &mut fixture.store(),
            &key(0x100),
            &mounts,
            Default::default()
        )
        .is_err()
    );
    let fixture = Fixture::new(
        &[field(b"DATA", &[2]), b"XCLW\x04\0\0".to_vec()].concat(),
        &[],
    );
    assert!(
        fixture
            .try_store(false)
            .and_then(|mut store| fixture.load(&mut store, Default::default()))
            .is_err()
    );
}
#[test]
fn all_thirteen_allowances_accept_exact_and_one_under_refuses_without_retained_source_handles() {
    let fixture = Fixture::new(&cell(0, b"noise.dds\0"), &watr());
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let u = &sources.receipt().usage;
    let exact = Limits {
        sources: u.sources,
        records: u.records,
        fields: u.fields,
        record_bytes: 43,
        read_bytes: u.read_bytes,
        string_bytes: u.string_bytes,
        raw_bytes: u.raw_bytes,
        metadata_bytes: u.metadata_bytes,
        dependencies: u.dependencies,
        archives: u.archives,
        mapped_bytes: u.mapped_bytes,
        jobs: u.jobs,
        noise_bytes: u.noise_bytes,
    };
    assert_eq!(
        (
            u.sources,
            u.records,
            u.fields,
            u.read_bytes,
            u.string_bytes,
            u.raw_bytes,
            u.dependencies,
            u.archives,
            u.jobs,
            u.noise_bytes
        ),
        (1, 2, 6, 59, 10, 36, 1, 1, 1, 5)
    );
    let identity = sources.identity().to_owned();
    drop(sources);
    assert_eq!(
        fixture.load(&mut store, exact).unwrap().identity(),
        identity
    );
    let mut limits = Vec::new();
    macro_rules! under {
        ($field:ident) => {
            limits.push(Limits {
                $field: exact.$field - 1,
                ..exact
            });
        };
    }
    under!(sources);
    under!(records);
    under!(fields);
    under!(record_bytes);
    under!(read_bytes);
    under!(string_bytes);
    under!(raw_bytes);
    under!(metadata_bytes);
    under!(dependencies);
    under!(archives);
    under!(mapped_bytes);
    under!(jobs);
    under!(noise_bytes);
    for limit in limits {
        assert!(fixture.load(&mut store, limit).is_err());
        #[cfg(windows)]
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .open(fixture.root.path().join("Data/noise.bsa"))
                .is_ok()
        );
    }
    assert_eq!(
        fixture.load(&mut store, exact).unwrap().identity(),
        identity
    );
    assert!(
        fixture
            .load(
                &mut store,
                Limits {
                    jobs: 2,
                    ..Default::default()
                }
            )
            .is_err()
    );
}
#[test]
fn protected_noise_jobs_reuse_cache_and_pin_their_source_until_last_artifact_drop() {
    let fixture = Fixture::new(&cell(0, b"noise.dds\0"), &watr());
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let owner = Generation::new(sources.identity().to_owned()).unwrap();
    let jobs = ResourceJobs::new(
        resource_jobs::Limits {
            workers: 1,
            outstanding: 1,
            decoded_bytes: 5,
        },
        owner.clone(),
    )
    .unwrap();
    let cache = Some((fixture.cache.path().into(), fixture.root.path().into()));
    let handle = sources
        .submit_noise(&mut store, &jobs, owner.token().unwrap(), cache.clone())
        .unwrap()
        .unwrap();
    let mut artifact = handle.wait().unwrap();
    assert_eq!(artifact.bytes(), NOISE);
    assert_eq!(jobs.usage().outstanding, 1);
    assert_eq!(jobs.usage().decoded_bytes, 5);
    let receipt = artifact.take_cache_receipt().unwrap();
    assert!(!receipt.reused);
    assert_eq!(receipt.manifest.identity.path_bytes, b"textures/noise.dds");
    assert_eq!(receipt.manifest.bytes, 5);
    assert_eq!(
        receipt.manifest.sha256,
        format!("{:x}", Sha256::digest(NOISE))
    );
    assert!(
        sources
            .submit_noise(&mut store, &jobs, owner.token().unwrap(), cache.clone())
            .is_err()
    );
    drop(artifact);
    assert_eq!(jobs.usage().outstanding, 0);
    let handle = sources
        .submit_noise(&mut store, &jobs, owner.token().unwrap(), cache)
        .unwrap()
        .unwrap();
    let mut retained = handle.wait().unwrap();
    assert!(retained.take_cache_receipt().unwrap().reused);
    drop(sources);
    #[cfg(windows)]
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.root.path().join("Data/noise.bsa"))
            .is_err()
    );
    assert_eq!(retained.bytes(), NOISE);
    assert_eq!(jobs.usage().decoded_bytes, 5);
    drop(retained);
    assert_eq!(jobs.usage().decoded_bytes, 0);
    #[cfg(windows)]
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.root.path().join("Data/noise.bsa"))
            .is_ok()
    );
}
#[test]
fn changed_sources_and_wrong_or_stale_job_generation_cannot_submit_noise() {
    let mut fixture = Fixture::new(&cell(0, b"noise.dds\0"), &watr());
    fixture.patch(&[b"Base.esm"], &[]);
    let mut store = fixture.store();
    let sources = fixture.load(&mut store, Default::default()).unwrap();
    let owner = Generation::new(sources.identity().to_owned()).unwrap();
    let jobs = ResourceJobs::new(
        resource_jobs::Limits {
            workers: 1,
            outstanding: 1,
            decoded_bytes: 5,
        },
        owner.clone(),
    )
    .unwrap();
    let foreign = Generation::new(sources.identity().to_owned()).unwrap();
    assert!(
        sources
            .submit_noise(&mut store, &jobs, foreign.token().unwrap(), None)
            .is_err()
    );
    let old = owner.token().unwrap();
    owner.advance(sources.identity().to_owned()).unwrap();
    assert!(sources.submit_noise(&mut store, &jobs, old, None).is_err());
    owner.advance("00".repeat(32)).unwrap();
    assert!(
        sources
            .submit_noise(&mut store, &jobs, owner.token().unwrap(), None)
            .is_err()
    );
    drop(store);
    fixture.names.pop();
    assert!(sources.validate_sources(&mut fixture.store()).is_err());
    fixture.names.push("Patch.esp".into());
    fs::write(
        fixture.root.path().join("Data/Patch.esp"),
        [
            header(&[b"Base.esm"]),
            record(b"STAT", 0x01000900, 0, &[1, 2, 3]),
        ]
        .concat(),
    )
    .unwrap();
    let mut changed = fixture.store();
    owner.advance(sources.identity().to_owned()).unwrap();
    assert!(
        sources
            .submit_noise(&mut changed, &jobs, owner.token().unwrap(), None)
            .is_err()
    );
    assert_eq!(jobs.usage(), Default::default());
}
#[test]
fn compressed_and_xxxx_fields_preserve_decoded_extents_and_tainted_cell_or_watr_never_prepare() {
    let mut extended = field(b"DATA", &[2]);
    extended.extend(field(b"XXXX", &4_u32.to_le_bytes()));
    extended.extend(b"XCLW\0\0");
    extended.extend([0, 0, 0, 128]);
    let fixture = Fixture::new(&extended, &[]);
    let sources = fixture
        .load(&mut fixture.store(), Default::default())
        .unwrap();
    let raw = sources.receipt().xclw.as_ref().unwrap();
    assert_eq!(raw.value, 0x80000000);
    assert_eq!(raw.site.decoded_header_offset, 17);
    assert_eq!(raw.site.span.decoded_offset, 23);
    assert_eq!(raw.decoded_framing_offset, 7);
    assert_eq!(raw.framing.len(), 20);
    assert_eq!(raw.physical_framing_offset, Some(73));
    for (target, tainted) in [(false, false), (false, true), (true, true)] {
        let body = if target {
            field(b"DATA", &[1, 0])
        } else {
            cell(0x80000000, b"noise.dds\0")
        };
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&body).unwrap();
        let mut stream = encoder.finish().unwrap();
        if tainted {
            *stream.last_mut().unwrap() ^= 1;
        }
        let compressed = [(body.len() as u32).to_le_bytes().as_slice(), &stream].concat();
        let mut fixture = Fixture::new(&cell(0, b"noise.dds\0"), &watr());
        fixture.patch(
            &[b"Base.esm"],
            &record(
                if target { b"WATR" } else { b"CELL" },
                if target { 0x200 } else { 0x100 },
                plugin::COMPRESSED,
                &compressed,
            ),
        );
        if tainted {
            assert!(
                fixture
                    .try_store(false)
                    .and_then(|mut store| fixture.load(&mut store, Default::default()))
                    .is_err()
            );
            let mut forensic = fixture.try_store(true).unwrap();
            assert!(fixture.load(&mut forensic, Default::default()).is_err());
        } else {
            let sources = fixture
                .load(&mut fixture.store(), Default::default())
                .unwrap();
            let raw = sources.receipt().xclw.as_ref().unwrap();
            assert_eq!(raw.value, 0x80000000);
            assert!(raw.physical_framing_offset.is_none());
            assert_eq!(raw.stored_body_offset, 95);
            assert_eq!(raw.stored_body_bytes, compressed.len() as u32);
        }
    }
}
