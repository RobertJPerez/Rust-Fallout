use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Limits},
    plugin,
    store::RecordStore,
};
use flate2::{Compression, write::ZlibEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
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
fn header() -> Vec<u8> {
    record(
        b"TES4",
        0,
        0,
        &field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    )
}
fn compressed(body: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).unwrap();
    [
        (body.len() as u32).to_le_bytes().as_slice(),
        &encoder.finish().unwrap(),
    ]
    .concat()
}
fn unit() -> Vec<u8> {
    let mut schr = [0; 20];
    schr[8..12].copy_from_slice(&4_u32.to_le_bytes());
    [field(b"SCHR", &schr), field(b"SCDA", &[0x1d, 0, 0, 0])].concat()
}
fn load_error(store: &mut RecordStore, limits: Limits, observed: &mut usize) -> String {
    match Catalogue::load(store, limits, |_, _| {
        *observed += 1;
        Ok(())
    }) {
        Ok(_) => panic!("candidate admission should reject"),
        Err(error) => error.to_string(),
    }
}
fn open(directory: &Path) -> RecordStore {
    RecordStore::open_nv_headers(
        directory,
        &["FalloutNV.esm".into()],
        plugin::Limits::default(),
    )
    .unwrap()
}
fn retain(name: &str, directory: &Path, observation: Value) {
    let Some(root) = std::env::var_os("FALLOUT_SCRIPT_CANDIDATE_EVIDENCE_DIR") else {
        return;
    };
    let root = Path::new(&root);
    assert!(root.is_absolute() && root.is_dir());
    let case = root.join(name);
    fs::create_dir(&case).unwrap();
    fs::copy(directory.join("FalloutNV.esm"), case.join("FalloutNV.esm")).unwrap();
    fs::write(
        case.join("observation.json"),
        serde_json::to_vec_pretty(&observation).unwrap(),
    )
    .unwrap();
}

#[test]
fn empty_candidate_read_work_is_separate_from_retained_script_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let empty = field(b"EDID", b"NoScript\0");
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"QUST", 0x100, 0, &empty),
            record(b"INFO", 0x101, 0, &empty),
        ]
        .concat(),
    )
    .unwrap();
    let mut observed = 0;
    let catalogue = Catalogue::load(
        &mut open(directory.path()),
        Limits {
            max_retained_bytes: 0,
            ..Limits::default()
        },
        |_, _| {
            observed += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(catalogue.counts.candidate_records_read, 2);
    assert_eq!(
        catalogue.counts.payload_bytes_scanned,
        (2 * empty.len()) as u64
    );
    assert_eq!(catalogue.counts.records_retained, 0);
    assert_eq!(catalogue.counts.scripts, 0);
    assert_eq!(catalogue.counts.payload_bytes_retained, 0);
    assert_eq!(observed, 0);
    retain(
        "empty-candidates",
        directory.path(),
        json!({"counts":catalogue.counts,"observer_calls":observed,"retained_limit":0}),
    );
}

#[test]
fn bounded_reader_rejects_declared_compressed_extent_before_zlib() {
    let directory = tempfile::tempdir().unwrap();
    let declared = 2 * 1024 * 1024_u32;
    let stored = [declared.to_le_bytes().as_slice(), b"not-zlib"].concat();
    let source = [
        header(),
        record(b"SCPT", 0x100, plugin::COMPRESSED, &stored),
    ]
    .concat();
    fs::write(directory.path().join("FalloutNV.esm"), &source).unwrap();
    let mut store = open(directory.path());
    let location = store
        .winner(&FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x100,
        })
        .unwrap();
    let unbounded = store.read(location).unwrap_err().to_string();
    assert!(unbounded.contains("zlib:") && unbounded.contains("strict integrity"));
    let bounded = store.read_bounded(location, 1024).unwrap_err().to_string();
    assert!(bounded.contains("decompression budget exceeded"));
    let mut observed = 0;
    let catalogue = match Catalogue::load(&mut store, Limits::default(), |_, _| {
        observed += 1;
        Ok(())
    }) {
        Ok(_) => panic!("invalid zlib admitted"),
        Err(error) => error.to_string(),
    };
    assert_eq!(catalogue, unbounded);
    assert_eq!(observed, 0);
    retain(
        "compressed-declared-extent",
        directory.path(),
        json!({"source_sha256":format!("{:x}",Sha256::digest(&source)),"declared_decoded_bytes":declared,
               "stored_bytes":stored.len(),"read_bound":1024,"default_reader_error":unbounded,
               "bounded_reader_error":bounded,"default_catalogue_error":catalogue,"observer_calls":observed}),
    );
}

#[test]
fn aggregate_read_limit_charges_empty_candidates_and_retry_restarts_admission() {
    let directory = tempfile::tempdir().unwrap();
    let empty = field(b"EDID", b"Empty\0");
    let stored = compressed(&empty);
    let charge = empty.len() + stored.len().max(empty.len());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"QUST", 0x100, 0, &empty),
            record(b"INFO", 0x101, plugin::COMPRESSED, &stored),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = open(directory.path());
    let exact = Limits {
        max_candidate_read_bytes: charge,
        max_retained_bytes: 0,
        ..Limits::default()
    };
    let mut observed = 0;
    let error = load_error(
        &mut store,
        Limits {
            max_candidate_read_bytes: charge - 1,
            ..exact
        },
        &mut observed,
    );
    assert!(error.contains("record size budget exceeded"), "{error}");
    assert_eq!(observed, 0);
    let retry = Catalogue::load(&mut store, exact, |_, _| Ok(())).unwrap();
    assert_eq!(retry.counts.candidate_records_read, 2);
    assert_eq!(retry.counts.payload_bytes_scanned, (2 * empty.len()) as u64);
    assert_eq!(retry.counts.payload_bytes_retained, 0);
    retain(
        "aggregate-empty-retry",
        directory.path(),
        json!({"exact_charge":charge,"one_less_error":error,"retry_counts":retry.counts,"observer_calls":observed}),
    );
}

#[test]
fn record_read_bound_rejects_declared_extent_before_invalid_compressed_stream() {
    let directory = tempfile::tempdir().unwrap();
    let stored = [(2 * 1024 * 1024_u32).to_le_bytes().as_slice(), b"not-zlib"].concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"SCPT", 0x100, plugin::COMPRESSED, &stored),
        ]
        .concat(),
    )
    .unwrap();
    let mut observed = 0;
    let error = load_error(
        &mut open(directory.path()),
        Limits {
            max_candidate_record_bytes: 1024,
            ..Limits::default()
        },
        &mut observed,
    );
    assert!(error.contains("decompression budget exceeded"), "{error}");
    assert_eq!(observed, 0);
    retain(
        "record-predecode",
        directory.path(),
        json!({"read_bound":1024,"declared_decoded_bytes":2*1024*1024,"error":error,"observer_calls":observed}),
    );
}

#[test]
fn aggregate_remaining_bound_rejects_after_earlier_success_without_observing_failed_record() {
    let directory = tempfile::tempdir().unwrap();
    let first = unit();
    let second = [(2 * 1024 * 1024_u32).to_le_bytes().as_slice(), b"not-zlib"].concat();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"SCPT", 0x100, 0, &first),
            record(b"SCPT", 0x101, plugin::COMPRESSED, &second),
        ]
        .concat(),
    )
    .unwrap();
    let mut observed = 0;
    let error = load_error(
        &mut open(directory.path()),
        Limits {
            max_candidate_read_bytes: first.len() + 1024,
            ..Limits::default()
        },
        &mut observed,
    );
    assert!(error.contains("decompression budget exceeded"), "{error}");
    assert_eq!(observed, 1);
    retain(
        "aggregate-predecode",
        directory.path(),
        json!({"first_charge":first.len(),"remaining_bound":1024,"error":error,"observer_calls":observed}),
    );
}

#[test]
fn compressed_stored_overhead_is_part_of_the_exact_record_read_bound() {
    let directory = tempfile::tempdir().unwrap();
    let body = field(b"EDID", b"X\0");
    let stored = compressed(&body);
    assert!(stored.len() > body.len());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"INFO", 0x100, plugin::COMPRESSED, &stored),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = open(directory.path());
    let exact = Limits {
        max_candidate_record_bytes: stored.len(),
        max_candidate_read_bytes: stored.len(),
        ..Limits::default()
    };
    let mut observed = 0;
    let error = load_error(
        &mut store,
        Limits {
            max_candidate_record_bytes: stored.len() - 1,
            ..exact
        },
        &mut observed,
    );
    assert!(error.contains("record size budget exceeded"), "{error}");
    let retry = Catalogue::load(&mut store, exact, |_, _| Ok(())).unwrap();
    assert_eq!(retry.counts.payload_bytes_scanned, body.len() as u64);
    assert_eq!(retry.counts.records_retained, 0);
    retain(
        "stored-overhead",
        directory.path(),
        json!({"stored_bytes":stored.len(),"decoded_bytes":body.len(),"one_less_error":error,"exact_counts":retry.counts}),
    );
}

#[test]
fn deleted_unrelated_and_nonwinning_bodies_do_not_consume_candidate_read_bytes() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"SCPT", 0x100, 0, b"bad"),
            record(b"SCPT", 0x101, plugin::DELETED | plugin::COMPRESSED, b"bad"),
            record(b"WEAP", 0x102, 0, &vec![0; 4096]),
        ]
        .concat(),
    )
    .unwrap();
    let master = [
        field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
        field(b"MAST", b"FalloutNV.esm\0"),
        field(b"DATA", &[0; 8]),
    ]
    .concat();
    let body = unit();
    fs::write(
        directory.path().join("Patch.esp"),
        [
            record(b"TES4", 0, 0, &master),
            record(b"SCPT", 0x100, 0, &body),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into(), "Patch.esp".into()],
        plugin::Limits::default(),
    )
    .unwrap();
    let mut observed = 0;
    let catalogue = Catalogue::load(
        &mut store,
        Limits {
            max_candidate_read_bytes: body.len(),
            max_candidate_record_bytes: body.len(),
            ..Limits::default()
        },
        |_, _| {
            observed += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(catalogue.counts.candidate_records_read, 1);
    assert_eq!(catalogue.counts.deleted_candidates_skipped, 1);
    assert_eq!(catalogue.counts.payload_bytes_scanned, body.len() as u64);
    assert_eq!(observed, 1);
}

#[test]
fn stricter_store_bounds_remain_in_force_and_candidate_limits_do_not_relax_them() {
    let directory = tempfile::tempdir().unwrap();
    let body = unit();
    let stored = compressed(&body);
    assert!(stored.len() < body.len());
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [
            header(),
            record(b"SCPT", 0x100, plugin::COMPRESSED, &stored),
        ]
        .concat(),
    )
    .unwrap();
    let mut store = RecordStore::open_nv_headers(
        directory.path(),
        &["FalloutNV.esm".into()],
        plugin::Limits {
            max_record_bytes: body.len() - 1,
            ..plugin::Limits::default()
        },
    )
    .unwrap();
    let mut observed = 0;
    let error = load_error(
        &mut store,
        Limits {
            max_candidate_record_bytes: body.len(),
            max_candidate_read_bytes: body.len(),
            ..Limits::default()
        },
        &mut observed,
    );
    assert!(error.contains("decompression budget exceeded"), "{error}");
    assert_eq!(observed, 0);
}

#[test]
fn retained_script_bytes_still_have_their_distinct_limit() {
    let directory = tempfile::tempdir().unwrap();
    let body = unit();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(), record(b"SCPT", 0x100, 0, &body)].concat(),
    )
    .unwrap();
    let mut observed = 0;
    let error = load_error(
        &mut open(directory.path()),
        Limits {
            max_candidate_read_bytes: body.len(),
            max_candidate_record_bytes: body.len(),
            max_retained_bytes: body.len() - 1,
            ..Limits::default()
        },
        &mut observed,
    );
    assert!(
        error.contains("loaded script retained byte budget exceeded"),
        "{error}"
    );
    assert_eq!(observed, 0);
}

#[test]
fn malformed_script_metadata_is_still_rejected_inside_the_read_allowance() {
    let directory = tempfile::tempdir().unwrap();
    let body = field(b"SCHR", &[0; 19]);
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(), record(b"SCPT", 0x100, 0, &body)].concat(),
    )
    .unwrap();
    let mut observed = 0;
    let error = load_error(
        &mut open(directory.path()),
        Limits {
            max_candidate_read_bytes: body.len(),
            max_candidate_record_bytes: body.len(),
            ..Limits::default()
        },
        &mut observed,
    );
    assert!(error.contains("SCHR must contain 20 bytes"), "{error}");
    assert_eq!(observed, 0);
}
